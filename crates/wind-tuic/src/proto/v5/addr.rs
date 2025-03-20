use std::net::{Ipv4Addr, Ipv6Addr};

use bytes::{Buf, BufMut};
use num_enum::{FromPrimitive, IntoPrimitive};
use snafu::{ResultExt, ensure};
use tokio_util::codec::{Decoder, Encoder};

use crate::{
   BytesRemainingSnafu, DomainTooLongSnafu, FailParseDomainSnafu, UnknownAddressTypeSnafu,
};

#[derive(Debug, Clone, Copy)]
pub struct AddressCodec;

#[derive(Debug, Clone, PartialEq)]
pub enum Address {
   None,
   Domain(String, u16),
   IPv4(Ipv4Addr, u16),
   IPv6(Ipv6Addr, u16),
}

#[derive(IntoPrimitive, FromPrimitive, Copy, Clone, Debug, PartialEq)]
#[repr(u8)]
pub enum AddressType {
   None   = u8::MAX,
   Domain = 0,
   IPv4   = 1,
   IPv6   = 2,
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
      let addr_type = AddressType::from(src[0]);

      ensure!(!matches!(addr_type, AddressType::Other(..)), UnknownAddressTypeSnafu { value: u8::from(addr_type) });
      match addr_type {
         AddressType::None => {
            src.advance(1);
            Ok(Some(Address::None))
         }
         AddressType::IPv4 => {
            if src.len() < 1 + 4 + 2 {
               return Ok(None);
            }
            src.advance(1);
            let mut octets = [0; 4];
            src.copy_to_slice(&mut octets);
            let ip = Ipv4Addr::from(octets);
            let port = src.get_u16();
            Ok(Some(Address::IPv4(ip, port)))
         }
         AddressType::IPv6 => {
            if src.len() < 1 + 16 + 2 {
               return Ok(None);
            }
            src.advance(1);
            let mut octets = [0; 16];
            src.copy_to_slice(&mut octets);
            let ip = Ipv6Addr::from(octets);
            let port = src.get_u16();
            Ok(Some(Address::IPv6(ip, port)))
         }
         AddressType::Domain => {
            if src.len() < 1 + 1 {
               return Ok(None);
            }
            let domain_len = src[1] as usize;
            if src.len() < 1 + 1 + domain_len + 2 {
               return Ok(None);
            }
            src.advance(2);

            let domain = &src[..domain_len];
            let domain = str::from_utf8(domain)
               .context(FailParseDomainSnafu {
                  raw: hex::encode(domain),
               })?
               .to_string();
            src.advance(domain_len);
            let port = src.get_u16();
            Ok(Some(Address::Domain(domain, port)))
         }
         _ => unreachable!(),
      }
   }

   fn decode_eof(&mut self, buf: &mut bytes::BytesMut) -> Result<Option<Self::Item>, Self::Error> {
      match self.decode(buf) {
         Ok(None) => BytesRemainingSnafu.fail(),
         v => v,
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
            dst.put_u8(AddressType::IPv4.into());
            dst.put_slice(&ip.octets());
            dst.put_u16(port);
         }
         Address::IPv6(ip, port) => {
            dst.reserve(1 + 16 + 2);
            dst.put_u8(AddressType::IPv6.into());
            dst.put_slice(&ip.octets());
            dst.put_u16(port);
         }
         Address::Domain(domain, port) => {
            if domain.len() > u8::MAX as usize {
               return DomainTooLongSnafu { domain }.fail();
            }
            dst.reserve(1 + domain.len() + 2);
            dst.put_u8(AddressType::Domain.into());
            dst.put_u8(domain.len() as u8);
            dst.put_slice(domain.as_bytes());
            dst.put_u16(port);
         }
      }
      Ok(())
   }
}

#[cfg(test)]
mod test {
   use std::net::{Ipv4Addr, Ipv6Addr};

   use futures_util::SinkExt as _;
   use tokio_stream::StreamExt as _;
   use tokio_util::codec::{FramedRead, FramedWrite};

   use super::{Address, AddressCodec};
   use crate::Error;

   /// Usual test
   #[tokio::test]
   async fn test_addr_1() -> eyre::Result<()> {
      let buffer = Vec::with_capacity(128);
      let vars = vec![
         Address::None,
         Address::IPv4(Ipv4Addr::LOCALHOST, 80),
         Address::IPv6(Ipv6Addr::UNSPECIFIED, 12),
         Address::Domain(String::from("www.google.com"), 443),
      ];

      let mut writer = FramedWrite::new(buffer, AddressCodec);
      let mut expect_len = 0;
      for var in &vars {
         match var {
            Address::None => expect_len = expect_len + 1,
            Address::Domain(domain, _) => expect_len = expect_len + 1 + 1 + domain.len() + 2,
            Address::IPv4(..) => expect_len = expect_len + 1 + 4 + 2,
            Address::IPv6(..) => expect_len = expect_len + 1 + 16 + 2,
         }
         writer.send(var.clone()).await?;
         assert_eq!(writer.get_ref().len(), expect_len);
      }

      let buffer = writer.get_ref();
      let mut reader = FramedRead::new(buffer.as_slice(), AddressCodec);
      for var in vars {
         let frame = reader.next().await.unwrap()?;
         assert_eq!(var, frame);
      }
      Ok(())
   }
   /// Data not fully arrive
   #[tokio::test]
   async fn test_addr_2() -> eyre::Result<()> {
      let vars = vec![
         Address::IPv4(Ipv4Addr::LOCALHOST, 80),
         Address::IPv6(Ipv6Addr::UNSPECIFIED, 12),
         Address::Domain(String::from("www.google.com"), 443),
      ];
      for addr in vars {
         let buffer = Vec::with_capacity(128);
         let mut writer = FramedWrite::new(buffer, AddressCodec);
         writer.send(addr.clone()).await?;
         let mut buffer = writer.into_inner();
         let full_len = buffer.len();
         let mut half_b = buffer.split_off(full_len / 2 as usize);
         let mut half_a = buffer;
         {
            let mut reader = FramedRead::new(half_a.as_slice(), AddressCodec);
            assert!(matches!(
               reader.next().await.unwrap().unwrap_err(),
               Error::BytesRemaining
            ));
         }
         half_a.append(&mut half_b);
         let mut reader = FramedRead::new(half_a.as_slice(), AddressCodec);
         assert_eq!(reader.next().await.unwrap()?, addr);
      }

      Ok(())
   }
}
