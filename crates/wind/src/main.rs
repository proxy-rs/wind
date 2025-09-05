use std::{ops::Deref, sync::Arc, time::Duration};

use tokio::task::JoinHandle;
use uuid::Uuid;
use wind_core::{AbstractOutbound, AbstractTcpStream, InboundCallback, inbound::AbstractInbound};
use wind_socks::inbound::{AuthMode, SocksInbound, SocksInboundOpt};
use wind_tuic::outbound::{TuicOutbound, TuicOutboundOpts};

struct Manager {
	inbound:  Arc<SocksInbound>,
	outbound: Arc<TuicOutbound>,
}

impl InboundCallback for Manager {
	async fn invoke(
		&self,
		target_addr: wind_core::types::TargetAddr,
		stream: impl wind_core::AbstractTcpStream,
	) -> eyre::Result<()> {
		self.outbound.handle_tcp(stream, None::<TuicOutbound>).await;
		todo!()
	}
}

pub enum Outbounds {
	Tuic(TuicOutbound),
}
impl AbstractOutbound for Outbounds {
	fn handle_tcp(
		&self,
		stream: impl AbstractTcpStream,
		via: Option<impl AbstractOutbound + Sized + Send>,
	) -> impl Future<Output = eyre::Result<impl AbstractTcpStream>> + Send {
		match &self {
			Outbounds::Tuic(tuic_outbound) => tuic_outbound.handle_tcp(stream, via),
		}
	}
}

#[tokio::main]
async fn main() -> eyre::Result<()> {
	let socks_opt = SocksInboundOpt {
		listen_addr: "127.0.0.1:6666".parse()?,
		public_addr: None,
		auth:        AuthMode::NoAuth,
		skip_auth:   false,
		allow_udp:   false,
	};
	let tuic_opts = TuicOutboundOpts {
		auth:               (
			Uuid::parse_str("c1e6dbe2-f417-4890-994c-9ee15b926597")?,
			Arc::from(String::from("test_passwd").into_bytes()),
		),
		zero_rtt_handshake: false,
		heartbeat:          Duration::from_secs(20),
		gc_interval:        Duration::from_secs(20),
		gc_lifetime:        Duration::from_secs(20),
		skip_cert_verify:   true,
		alpn:               vec![String::from("h3")],
	};
	let outbound = TuicOutbound::new(
		"127.0.0.1:9443".parse()?,
		"localhost".to_string(),
		tuic_opts,
	)
	.await?;
	let inbound = Arc::new(SocksInbound::new(socks_opt).await);
	let outbound = Arc::new(outbound);
	let manager = Manager { inbound, outbound };
	let manager = Arc::new(manager);
	let inbound_poller: JoinHandle<eyre::Result<()>> = tokio::spawn(async move {
		let manager = manager.clone();
		manager.inbound.listen(manager.deref()).await?;
		Ok(())
	});

	inbound_poller.await??;
	Ok(())
}
