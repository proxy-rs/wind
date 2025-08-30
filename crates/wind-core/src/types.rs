use std::net::{Ipv4Addr, Ipv6Addr};

#[derive(Clone, Debug)]
pub enum TargetAddr {
   Domain(String, u16),
   IPv4(Ipv4Addr, u16),
   IPv6(Ipv6Addr, u16),
}
