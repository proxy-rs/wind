use std::{
	net::{Ipv4Addr, SocketAddr},
	sync::{Arc, atomic::AtomicU16},
	time::Duration,
};

use quinn::TokioRuntime;
use snafu::ResultExt;
use tokio::net::UdpSocket;
use uuid::Uuid;
use wind_core::{
	AbstractOutbound, info, tcp::AbstractTcpStream, types::TargetAddr, udp::AbstractUdpSocket,
};

use crate::{BindSocketSnafu, Error, QuicConnectSnafu, proto::ClientProtoExt};

pub struct TuicOutboundOpts {
	pub auth:               (Uuid, Arc<[u8]>),
	pub zero_rtt_handshake: bool,
	pub heartbeat:          Duration,
	pub gc_interval:        Duration,
	pub gc_lifetime:        Duration,
	pub skip_cert_verify:   bool,
	pub alpn:               Vec<String>,
}

pub struct TuicOutbound {
	pub endpoint:          quinn::Endpoint,
	pub peer_addr:         SocketAddr,
	pub server_name:       String,
	pub opts:              TuicOutboundOpts,
	pub connection:        quinn::Connection,
	pub udp_assoc_counter: AtomicU16,
}

impl TuicOutbound {
	pub async fn new(
		peer_addr: SocketAddr,
		server_name: String,
		opts: TuicOutboundOpts,
	) -> Result<Self, Error> {
		// TODO
		{
			#[cfg(feature = "aws-lc-rs")]
			rustls::crypto::aws_lc_rs::default_provider()
				.install_default()
				.unwrap();
			#[cfg(feature = "ring")]
			rustls::crypto::ring::default_provider()
				.install_default()
				.unwrap();
		}
		info!(target: "[OUT]", "Creating a new TUIC outboud");
		let client_config = {
			let tls_config = super::tls::tls_config(&server_name, &opts)?;

			let mut client_config = quinn::ClientConfig::new(Arc::new(
				quinn::crypto::rustls::QuicClientConfig::try_from(tls_config).unwrap(),
			));
			let mut transport_config = quinn::TransportConfig::default();
			transport_config
				.congestion_controller_factory(Arc::new(quinn::congestion::BbrConfig::default()))
				.keep_alive_interval(None);

			client_config.transport_config(Arc::new(transport_config));
			client_config
		};
		let socket_addr = SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0));
		let socket = UdpSocket::bind(&socket_addr)
			.await
			.context(BindSocketSnafu { socket_addr })?
			.into_std()?;

		let mut endpoint = quinn::Endpoint::new(
			quinn::EndpointConfig::default(),
			None,
			socket,
			Arc::new(TokioRuntime),
		)?;
		endpoint.set_default_client_config(client_config);
		let connection = endpoint
			.connect(peer_addr, &server_name)
			.context(QuicConnectSnafu {
				addr:        peer_addr,
				server_name: server_name.clone(),
			})?
			.await?;
		// Replace with correct authentication logic or import the extension trait
		connection.send_auth(&opts.auth.0, &opts.auth.1).await?;

		Ok(Self {
			endpoint,
			peer_addr,
			server_name,
			opts,
			connection,
			udp_assoc_counter: AtomicU16::new(0),
		})
	}

	pub async fn start_poll(&self) -> eyre::Result<()> {
		let mut interval = tokio::time::interval(self.opts.heartbeat);

		loop {
			interval.tick().await;
			self.connection.send_heartbeat().await?;
		}
	}
}

pub struct TuicTcpStream;

impl AbstractOutbound for TuicOutbound {
	async fn handle_tcp(
		&self,
		target_addr: TargetAddr,
		stream: impl AbstractTcpStream,
		_dialer: Option<impl AbstractOutbound>,
	) -> eyre::Result<()> {
		self.connection.open_tcp(&target_addr, stream).await?;
		Ok(())
	}

	async fn handle_udp(
		&self,
		_socket: impl AbstractUdpSocket,
		_dialer: Option<impl AbstractOutbound>,
	) -> eyre::Result<()> {
		use std::sync::{Arc, atomic::Ordering};

		// Generate a new UDP association ID
		let assoc_id = self.udp_assoc_counter.fetch_add(1, Ordering::SeqCst);
		info!(target: "[OUT]", "Creating new UDP association: {:#06x}", assoc_id);

		// Create a connection wrapper for UDP
		let connection = Arc::new(self.connection.clone());

		// Placeholder for UDP packet handling
		// In a real implementation, we would:
		// 1. Receive packets from local UDP socket
		// 2. Forward them through QUIC connection
		// 3. Receive packets from QUIC connection
		// 4. Forward them back to local UDP socket

		// For now, we'll just keep the UDP association alive
		// and log a message periodically
		loop {
			tokio::select! {
				_ = tokio::time::sleep(tokio::time::Duration::from_secs(30)) => {
					info!(target: "[OUT]", "UDP handler for association {:#06x} active", assoc_id);
				}

				// If we want to exit the loop, we can add more conditions here
				// such as a cancellation token, timeout, or error handling
				else => break,
			}
		}


		// Clean up the UDP association before exiting
		if let Err(err) = connection.drop_udp(assoc_id).await {
			info!(target: "[OUT]", "Error dropping UDP association {:#06x}: {}", assoc_id, err);
		}

		Ok(())
	}
}
