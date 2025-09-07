use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use fast_socks5::{
	ReplyError, Socks5Command, server::Socks5ServerProtocol,
	util::target_addr::TargetAddr as SocksTargetAddr,
};
use snafu::ResultExt;
use tokio::net::{TcpListener, TcpStream};
use tracing::error;
use wind_core::{AbstractInbound, InboundCallback, types::TargetAddr};

use crate::{CallbackSnafu, Error, SocksSnafu};

pub struct SocksInboundOpt {
	/// Bind on address address. eg. `127.0.0.1:1080`
	pub listen_addr: SocketAddr,

	/// Our external IP address to be sent in reply packets (required for UDP)
	pub public_addr: Option<std::net::IpAddr>,

	/// Choose authentication type
	pub auth: AuthMode,

	/// Don't perform the auth handshake, send directly the command request
	pub skip_auth: bool,

	/// Allow UDP proxying, requires public-addr to be set
	pub allow_udp: bool,
}

pub enum AuthMode {
	NoAuth,
	Password { username: String, password: String },
}

pub struct SocksInbound {
	opts: SocksInboundOpt,
}

impl AbstractInbound for SocksInbound {
	async fn listen(&self, cb: &impl InboundCallback) -> eyre::Result<()> {
		let listener = TcpListener::bind(self.opts.listen_addr).await?;
		loop {
			match listener.accept().await {
				Err(err) => error!(name: "REACTOR", target:"[SOCKS-IN]", "{:?}", err),
				Ok((stream, client_addr)) => {
					match self.handle_income(stream, client_addr, cb).await {
						Ok(_) => {}
						Err(err) => error!(target: "[SOCKS-IN] HANDLER" , "{:?}", err),
					}
				}
			}
		}
	}
}

impl SocksInbound {
	pub async fn new(opts: SocksInboundOpt) -> Self {
		Self { opts }
	}

	async fn handle_income(
		&self,
		stream: TcpStream,
		_client_addr: SocketAddr,
		cb: &impl InboundCallback,
	) -> Result<(), Error> {
		let proto = match &self.opts.auth {
			AuthMode::NoAuth => Socks5ServerProtocol::accept_no_auth(stream)
				.await
				.context(SocksSnafu)?,
			AuthMode::Password { username, password } => {
				Socks5ServerProtocol::accept_password_auth(stream, |user, pass| {
					user == *username && pass == *password
				})
				.await
				.context(SocksSnafu)?
				.0
			}
		};
		let (proto, cmd, target_addr) = proto.read_command().await?;
		let target_addr = match target_addr {
			SocksTargetAddr::Ip(socket_addr) => match socket_addr {
				SocketAddr::V4(socket_addr) => {
					TargetAddr::IPv4(*socket_addr.ip(), socket_addr.port())
				}
				SocketAddr::V6(socket_addr) => {
					TargetAddr::IPv6(*socket_addr.ip(), socket_addr.port())
				}
			},
			SocksTargetAddr::Domain(domain, port) => TargetAddr::Domain(domain, port),
		};
		match cmd {
			Socks5Command::TCPConnect => {
				let inner = proto
					.reply_success(SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 0))
					.await?;
				cb.invoke(target_addr, inner).await.context(CallbackSnafu)?;
			}
			Socks5Command::UDPAssociate if self.opts.allow_udp => {
				// let reply_ip = opt.public_addr.context("invalid reply ip")?;
				// run_udp_proxy(proto, &target_addr, None, reply_ip, None).await?;
				proto.reply_error(&ReplyError::CommandNotSupported).await?;
				return Err(ReplyError::CommandNotSupported.into());
			}
			_ => {
				proto.reply_error(&ReplyError::CommandNotSupported).await?;
				return Err(ReplyError::CommandNotSupported.into());
			}
		};
		Ok(())
	}
}
