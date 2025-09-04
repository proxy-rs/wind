use std::{sync::Arc, time::Duration};

use uuid::Uuid;
use wind_socks::inbound::{AuthMode, SocksInbound, SocksInboundOpt};
use wind_tuic::outbound::{TuicOutbound, TuicOutboundOpts};

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
			Arc::from(String::from("test_passwd").into_bytes().into_boxed_slice()),
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
	let inbound = SocksInbound::new(socks_opt).await;
	let outbound = Arc::new(outbound);
	
	Ok(())
}
