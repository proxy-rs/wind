use uuid::Uuid;
use wind_socks::inbound::SocksInboundOpt;
use wind_tuic::outbound::TuicOutboundOpts;
use wind_core::types::TargetAddr;

pub struct Config {
	pub socks_opt: SocksInboundOpt,
	pub tuic_opt:  TuicOutboundOpts,
}
impl Config {
	pub fn from_persist(config: super::persistent::PersistentConfig) -> Self {
		Self {
			socks_opt: SocksInboundOpt {
				listen_addr: config.socks_opt.listen_addr,
				public_addr: config.socks_opt.public_addr,
				auth:        config.socks_opt.auth.into(),
				skip_auth:   config.socks_opt.skip_auth,
				allow_udp:   config.socks_opt.allow_udp,
			},
			tuic_opt:  TuicOutboundOpts {
				server:             (
					config.tuic_opt.sni.clone(),
					match config.tuic_opt.server_addr {
						TargetAddr::Domain(_, port) => port,
						TargetAddr::IPv4(_, port) => port,
						TargetAddr::IPv6(_, port) => port,
					},
				),
				peer_addr:          match config.tuic_opt.server_addr {
					TargetAddr::Domain(_, _) => None, // For domain names, we let outbound.rs perform DNS resolution
					TargetAddr::IPv4(ip, _) => Some(std::net::IpAddr::V4(ip)),
					TargetAddr::IPv6(ip, _) => Some(std::net::IpAddr::V6(ip)),
				},
				sni:                config.tuic_opt.sni.clone(),
				auth:               (
					Uuid::parse_str(&config.tuic_opt.uuid).expect("Invalid UUID"),
					config.tuic_opt.password.as_bytes().to_vec().into(),
				),
				zero_rtt_handshake: config.tuic_opt.zero_rtt_handshake,
				heartbeat:          config.tuic_opt.heartbeat,
				gc_interval:        config.tuic_opt.gc_interval,
				gc_lifetime:        config.tuic_opt.gc_lifetime,
				skip_cert_verify:   config.tuic_opt.skip_cert_verify,
				alpn:               config.tuic_opt.alpn.clone(),
			},
		}
	}
}
