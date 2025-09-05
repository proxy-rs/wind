use std::net::{Ipv4Addr, Ipv6Addr};
use std::fmt::Display;

#[derive(Clone, Debug)]
pub enum TargetAddr {
	Domain(String, u16),
	IPv4(Ipv4Addr, u16),
	IPv6(Ipv6Addr, u16),
}

impl Display for TargetAddr {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			TargetAddr::Domain(domain, port) => write!(f, "{}:{}", domain, port),
			TargetAddr::IPv4(addr, port) => write!(f, "{}:{}", addr, port),
			TargetAddr::IPv6(addr, port) => write!(f, "[{}]:{}", addr, port),
		}
	}
}