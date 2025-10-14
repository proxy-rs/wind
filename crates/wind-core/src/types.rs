use std::{
	fmt::Display,
	net::{Ipv4Addr, Ipv6Addr},
};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

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

impl Serialize for TargetAddr {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.to_string().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for TargetAddr {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        use serde::de::Error;
        
        let s = String::deserialize(deserializer)?;
        
        // Check if this is an IPv6 address with brackets [IPv6]:port
        if s.starts_with('[') {
            let end_bracket = match s.find(']') {
                Some(pos) => pos,
                None => return Err(Error::custom("Invalid IPv6 address format, missing closing bracket")),
            };
            
            // Ensure there's a colon after the closing bracket
            if end_bracket + 1 >= s.len() || !s[end_bracket+1..].starts_with(':') {
                return Err(Error::custom("Invalid IPv6 address format, expected [IPv6]:port"));
            }
            
            let ipv6_str = &s[1..end_bracket];
            let port_str = &s[end_bracket+2..];
            
            let ipv6_addr = match ipv6_str.parse::<Ipv6Addr>() {
                Ok(addr) => addr,
                Err(_) => return Err(Error::custom("Invalid IPv6 address")),
            };
            
            let port = match port_str.parse::<u16>() {
                Ok(p) => p,
                Err(_) => return Err(Error::custom("Invalid port number")),
            };
            
            return Ok(TargetAddr::IPv6(ipv6_addr, port));
        }
        
        // Split the string into host and port parts for IPv4 or domain
        let parts: Vec<&str> = s.split(':').collect();
        if parts.len() != 2 {
            return Err(Error::custom("Invalid address format, expected host:port"));
        }
        
        // Parse the port
        let port = match parts[1].parse::<u16>() {
            Ok(p) => p,
            Err(_) => return Err(Error::custom("Invalid port number")),
        };
        
        // Try to parse as IPv4 first
        if let Ok(ipv4) = parts[0].parse::<Ipv4Addr>() {
            Ok(TargetAddr::IPv4(ipv4, port))
        } else {
            // Otherwise treat as domain
            Ok(TargetAddr::Domain(parts[0].to_string(), port))
        }
    }
}
