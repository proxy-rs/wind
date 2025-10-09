use std::{ops::Deref, sync::Arc, time::Duration};

use clap::Parser as _;
use tokio_util::{sync::CancellationToken, task::TaskTracker};
use tracing::Level;
use uuid::Uuid;
use wind_core::{
	AbstractOutbound, AppContext, InboundCallback, inbound::AbstractInbound, info,
	tcp::AbstractTcpStream, types::TargetAddr, udp::AbstractUdpSocket,
};
use wind_socks::inbound::{AuthMode, SocksInbound, SocksInboundOpt};
use wind_tuic::outbound::{TuicOutbound, TuicOutboundOpts};

use crate::cli::Cli;

mod cli;
mod conf;
mod log;

struct Manager {
	inbound:  Arc<SocksInbound>,
	outbound: Arc<TuicOutbound>,
}

impl InboundCallback for Manager {
	async fn handle_tcpstream(
		&self,
		target_addr: TargetAddr,
		stream: impl AbstractTcpStream,
	) -> eyre::Result<()> {
		info!(target: "[TCP-IN] START","target address {target_addr}");
		self.outbound
			.handle_tcp(target_addr, stream, None::<Outbounds>)
			.await?;
		Ok(())
	}

	async fn handle_udpsocket(&self, socket: impl AbstractUdpSocket) -> eyre::Result<()> {
		info!(target: "[UDP-IN] START","UDP association started");
		self.outbound.handle_udp(socket, None::<Outbounds>).await?;
		Ok(())
	}
}

pub enum Outbounds {
	Tuic(TuicOutbound),
}
impl AbstractOutbound for Outbounds {
	fn handle_tcp(
		&self,
		target_addr: TargetAddr,
		stream: impl AbstractTcpStream,
		via: Option<impl AbstractOutbound + Sized + Send>,
	) -> impl Future<Output = eyre::Result<()>> + Send {
		match &self {
			Outbounds::Tuic(tuic_outbound) => tuic_outbound.handle_tcp(target_addr, stream, via),
		}
	}

	fn handle_udp(
		&self,
		socket: impl AbstractUdpSocket,
		via: Option<impl AbstractOutbound + Sized + Send>,
	) -> impl Future<Output = eyre::Result<()>> + Send {
		match &self {
			Outbounds::Tuic(tuic_outbound) => tuic_outbound.handle_udp(socket, via),
		}
	}
}
// curl --socks5 127.0.0.1:6666 https://www.bing.com
#[tokio::main]
async fn main() -> eyre::Result<()> {
	log::init_log(Level::TRACE)?;
	info!(target: "[MAIN]", "Wind starting");
	let cli = match Cli::try_parse() {
		Ok(v) => v,
		Err(err) => {
			println!("{:#}", err);
			return Ok(());
		}
	};

	if cli.version {
		const VER: &str = match option_env!("WIND_OVERRIVE_VERSION") {
			Some(v) => v,
			None => env!("CARGO_PKG_VERSION"),
		};
		println!("wind {VER}");
		return Ok(());
	}
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
		heartbeat:          Duration::from_secs(10),
		gc_interval:        Duration::from_secs(20),
		gc_lifetime:        Duration::from_secs(20),
		skip_cert_verify:   true,
		alpn:               vec![String::from("h3")],
	};
	let ctx = Arc::new(AppContext {
		tasks: TaskTracker::new(),
		token: CancellationToken::new(),
	});
	let outbound = TuicOutbound::new(
		ctx.clone(),
		"127.0.0.1:9443".parse()?,
		"localhost".to_string(),
		tuic_opts,
	)
	.await?;
	let inbound = Arc::new(SocksInbound::new(socks_opt).await);
	let outbound = Arc::new(outbound);
	let manager = Manager { inbound, outbound };
	let manager = Arc::new(manager);

	let manager_clone = manager.clone();
	ctx.tasks.spawn(async move {
		manager_clone.outbound.start_poll().await?;
		eyre::Ok(())
	});

	let manager_clone = manager.clone();
	ctx.tasks.spawn(async move {
		manager_clone.inbound.listen(manager.deref()).await?;
		eyre::Ok(())
	});
	tokio::signal::ctrl_c().await?;
	info!(target: "[MAIN]", "Ctrl-C received, shutting down");
	ctx.token.cancel();
	tokio::time::timeout(Duration::from_secs(10), ctx.tasks.wait()).await?;
	info!(target: "[MAIN]", "Shutdown complete");
	Ok(())
}
