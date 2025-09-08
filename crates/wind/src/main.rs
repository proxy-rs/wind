use std::{ops::Deref, sync::Arc, time::Duration};

use clap::Parser as _;
use tokio::task::JoinSet;
use tracing::Level;
use uuid::Uuid;
use wind_core::{
	AbstractOutbound, AbstractTcpStream, InboundCallback, inbound::AbstractInbound, info,
	types::TargetAddr,
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
	async fn invoke(
		&self,
		target_addr: TargetAddr,
		stream: impl wind_core::AbstractTcpStream,
	) -> eyre::Result<()> {
		info!(target: "[TCP-IN] START","target address {target_addr}");
		self.outbound
			.handle_tcp(target_addr, stream, None::<Outbounds>)
			.await?;
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
		target_addr: TargetAddr,
		packet: tokio_util::bytes::Bytes,
		via: Option<impl AbstractOutbound + Sized + Send>,
	) -> impl Future<Output = eyre::Result<()>> + Send {
		match &self {
			Outbounds::Tuic(tuic_outbound) => tuic_outbound.handle_udp(target_addr, packet, via),
		}
	}
}
// curl --socks5 127.0.0.1:6666 bing.com
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

	let mut set: JoinSet<eyre::Result<()>> = JoinSet::new();
	let manager_clone = manager.clone();
	set.spawn(async move {
		manager_clone.outbound.start_poll().await?;
		Ok(())
	});

	let manager_clone = manager.clone();
	set.spawn(async move {
		manager_clone.inbound.listen(manager.deref()).await?;
		Ok(())
	});
	while let Some(v) = set.join_next().await
		&& let Ok(Err(e)) = v
	{
		return Err(e);
	}

	Ok(())
}
