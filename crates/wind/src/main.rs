use std::{ops::Deref, sync::Arc, time::Duration};

use clap::Parser as _;
use tokio_util::{sync::CancellationToken, task::TaskTracker};
use tracing::Level;
use wind_core::{
	AbstractOutbound, AppContext, InboundCallback, inbound::AbstractInbound, info, tcp::AbstractTcpStream, types::TargetAddr,
	udp::AbstractUdpSocket,
};
use wind_socks::inbound::SocksInbound;
use wind_tuic::outbound::TuicOutbound;

// Testing module providing SOCKS5 proxy testing functionality
pub mod tests;


mod util;
use crate::{cli::Cli, conf::persistent::PersistentConfig};

mod cli;
mod conf;
mod log;

struct Manager {
	inbound:  Arc<SocksInbound>,
	outbound: Arc<TuicOutbound>,
}

impl InboundCallback for Manager {
	async fn handle_tcpstream(&self, target_addr: TargetAddr, stream: impl AbstractTcpStream) -> eyre::Result<()> {
		info!(target: "[TCP-IN] START","target address {target_addr}");
		self.outbound.handle_tcp(target_addr, stream, None::<Outbounds>).await?;
		Ok(())
	}

	async fn handle_udpsocket(&self, socket: impl AbstractUdpSocket + 'static) -> eyre::Result<()> {
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
		socket: impl AbstractUdpSocket + 'static,
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

	// Handle init subcommand
	match &cli.command {
		Some(crate::cli::Commands::Init { format }) => {
			// Create a default configuration
			let default_config = PersistentConfig::default();

			// Determine format and file name
			let format_str = match format {
				crate::cli::ConfigFormat::Yaml => "yaml",
				crate::cli::ConfigFormat::Toml => "toml",
			};

			// Determine the file path
			let file_name = format!("config.{}", format_str);
			let file_path = if let Some(config_dir) = &cli.config_dir {
				// Ensure the directory exists
				std::fs::create_dir_all(config_dir)?;
				config_dir.join(&file_name)
			} else {
				std::path::PathBuf::from(&file_name)
			};

			// Export the configuration
			default_config.export_to_file(&file_path, format_str)?;
			println!("Created default configuration at: {}", file_path.display());
			return Ok(());
		}
		None => {
			// 继续正常启动
		}
	}

	// Load configuration using the persistent config module
	let persistent_config = PersistentConfig::load(cli.config, cli.config_dir)?;
	info!(target: "[MAIN]", "Configuration loaded successfully");

	// Convert to runtime config
	let runtime_config = conf::runtime::Config::from_persist(persistent_config);

	let ctx = Arc::new(AppContext {
		tasks: TaskTracker::new(),
		token: CancellationToken::new(),
	});

	let outbound = TuicOutbound::new(ctx.clone(), runtime_config.tuic_opt).await?;
	let inbound = Arc::new(SocksInbound::new(runtime_config.socks_opt, ctx.token.child_token()).await);
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
	ctx.tasks.close();
	tokio::time::timeout(Duration::from_secs(10), ctx.tasks.wait()).await?;

	info!(target: "[MAIN]", "Shutdown complete");
	Ok(())
}
