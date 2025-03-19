use std::net::{Ipv4Addr, Ipv6Addr};

use bytes::{Buf, BufMut};
use num_enum::{FromPrimitive, IntoPrimitive};
use snafu::{ResultExt, ensure};
use tokio_util::codec::{Decoder, Encoder};

use crate::{DomainTooLongSnafu, FailParseDomainSnafu, UnknownAddressTypeSnafu};

#[derive(Debug, Clone, Copy)]
pub struct AddressCodec;

#[derive(Debug, Clone, PartialEq)]
pub enum Address {
    None,
    FQDN(String, u16),
    IPv4(Ipv4Addr, u16),
    IPv6(Ipv6Addr, u16),
}

#[derive(IntoPrimitive, FromPrimitive, Copy, Clone, Debug, PartialEq)]
#[repr(u8)]
pub enum AddressType {
    None = u8::MAX,
    FQDN = 0,
    IPv4 = 1,
    IPv6 = 2,
    #[num_enum(catch_all)]
    Other(u8),
}
// https://github.com/zephry-works/wind/blob/main/crates/wind-tuic/SPEC.md#5-address-encoding
#[cfg(feature = "server")]
impl Decoder for AddressCodec {
    type Error = crate::Error;
    type Item = Address;

    fn decode(&mut self, src: &mut bytes::BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if src.is_empty() {
            return Ok(None);
        }
        let addr_type = AddressType::from(src.get_u8());

        ensure!(!matches!(addr_type, AddressType::Other(..)), UnknownAddressTypeSnafu { value: u8::from(addr_type) });
        match addr_type {
            AddressType::None => Ok(Some(Address::None)),
            AddressType::IPv4 => {
                if src.len() < 4 + 2 {
                    return Ok(None);
                }
                let mut octets = [0; 4];
                src.copy_to_slice(&mut octets);
                let ip = Ipv4Addr::from(octets);
                let port = src.get_u16();
                Ok(Some(Address::IPv4(ip, port)))
            }
            AddressType::IPv6 => {
                if src.len() < 16 + 2 {
                    return Ok(None);
                }
                let mut octets = [0; 16];
                src.copy_to_slice(&mut octets);
                let ip = Ipv6Addr::from(octets);
                let port = src.get_u16();
                Ok(Some(Address::IPv6(ip, port)))
            }
            AddressType::FQDN => {
                if src.is_empty() {
                    return Ok(None);
                }
                let domain_len = src.get_u8() as usize;
                if src.len() < domain_len + 2 {
                    return Ok(None);
                }

                let domain = &src[..domain_len];
                let domain = str::from_utf8(domain)
                    .context(FailParseDomainSnafu {
                        raw: hex::encode(domain),
                    })?
                    .to_string();
                src.advance(domain_len);
                let port = src.get_u16();
                Ok(Some(Address::FQDN(domain, port)))
            }
            _ => unreachable!(),
        }
    }
}

#[cfg(feature = "client")]
impl Encoder<Address> for AddressCodec {
    type Error = crate::Error;

    fn encode(&mut self, item: Address, dst: &mut bytes::BytesMut) -> Result<(), Self::Error> {
        match item {
            Address::None => {
                dst.reserve(1);
                dst.put_u8(AddressType::None.into());
            }
            Address::IPv4(ip, port) => {
                dst.reserve(1 + 4 + 2);
                dst.put_slice(&ip.octets());
                dst.put_u16(port);
            }
            Address::IPv6(ip, port) => {
                dst.reserve(1 + 16 + 2);
                dst.put_slice(&ip.octets());
                dst.put_u16(port);
            }
            Address::FQDN(domain, port) => {
                if domain.len() > u8::MAX as usize {
                    return DomainTooLongSnafu { domain }.fail();
                }
                dst.reserve(1 + domain.len() + 2);
                dst.put_u8(domain.len() as u8);
                dst.put_slice(domain.as_bytes());
                dst.put_u16(port);
            }
        }
        Ok(())
    }
}
