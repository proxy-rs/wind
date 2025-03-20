use bytes::{Buf, BufMut as _};
use tokio_util::codec::{Decoder, Encoder};
use uuid::Uuid;

use super::CommandType;
use crate::{BytesRemainingSnafu, UnknownCommandTypeSnafu};

#[derive(Debug, Clone, Copy)]
pub struct CommandCodec(pub CommandType);

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
   Authenticate {
      uuid:  uuid::Uuid,
      token: [u8; 32],
   },
   Connect,
   Packet {
      assos_id:   u16,
      pkt_id:     u16,
      frag_total: u8,
      frag_id:    u8,
      size:       u16,
   },
   Dissociate {
      assos_id: u16,
   },
   Heartbeat,
}

// https://github.com/zephry-works/wind/blob/main/crates/wind-tuic/SPEC.md#3-command-specifications
#[cfg(feature = "decode")]
impl Decoder for CommandCodec {
   type Error = crate::Error;
   type Item = Command;

   fn decode(&mut self, src: &mut bytes::BytesMut) -> Result<Option<Self::Item>, Self::Error> {
      match self.0 {
         CommandType::Authenticate => {
            if src.len() < 16 + 32 {
               return Ok(None);
            }
            let mut uuid = [0; 16];
            src.copy_to_slice(&mut uuid);
            let uuid = Uuid::from_bytes(uuid);
            let mut token = [0; 32];
            src.copy_to_slice(&mut token);
            Ok(Some(Command::Authenticate { uuid, token }))
         }
         CommandType::Connect => Ok(Some(Command::Connect)),
         CommandType::Packet => {
            if src.len() < 8 {
               return Ok(None);
            }

            Ok(Some(Command::Packet {
               assos_id:   src.get_u16(),
               pkt_id:     src.get_u16(),
               frag_total: src.get_u8(),
               frag_id:    src.get_u8(),
               size:       src.get_u16(),
            }))
         }
         CommandType::Dissociate => {
            if src.len() < 2 {
               return Ok(None);
            }
            let assos_id = src.get_u16();
            Ok(Some(Command::Dissociate { assos_id }))
         }
         CommandType::Heartbeat => Ok(Some(Command::Heartbeat)),
         CommandType::Other(value) => UnknownCommandTypeSnafu { value }.fail(),
      }
   }

   fn decode_eof(&mut self, buf: &mut bytes::BytesMut) -> Result<Option<Self::Item>, Self::Error> {
      match self.decode(buf) {
         Ok(None) => BytesRemainingSnafu.fail(),
         v => v,
      }
   }
}

#[cfg(feature = "encode")]
impl Encoder<Command> for CommandCodec {
   type Error = crate::Error;

   fn encode(&mut self, item: Command, dst: &mut bytes::BytesMut) -> Result<(), Self::Error> {
      match item {
         Command::Authenticate { uuid, token } => {
            dst.reserve(16 + 32);
            dst.put_slice(uuid.as_bytes());
            dst.put_slice(&token);
         }
         Command::Connect => {}
         Command::Packet {
            assos_id,
            pkt_id,
            frag_total,
            frag_id,
            size,
         } => {
            dst.reserve(8);
            dst.put_u16(assos_id);
            dst.put_u16(pkt_id);
            dst.put_u8(frag_total);
            dst.put_u8(frag_id);
            dst.put_u16(size);
         }
         Command::Dissociate { assos_id } => {
            dst.reserve(2);
            dst.put_u16(assos_id);
         }
         Command::Heartbeat => {}
      }
      Ok(())
   }
}

#[cfg(test)]
mod test {
   use futures_util::SinkExt as _;
   use tokio_stream::StreamExt as _;
   use tokio_util::codec::{FramedRead, FramedWrite};
   use uuid::Uuid;

   use super::Command;
   use crate::{Error, proto::CommandCodec};

   /// Usual test
   #[tokio::test]
   async fn test_cmd_1() -> eyre::Result<()> {
      let vars = vec![
         Command::Authenticate {
            uuid:  Uuid::parse_str("02f09a3f-1624-3b1d-8409-44eff7708208")?,
            token: [1; 32],
         },
         Command::Connect,
         Command::Packet {
            assos_id:   123,
            pkt_id:     123,
            frag_total: 5,
            frag_id:    1,
            size:       8,
         },
         Command::Dissociate { assos_id: 23 },
         Command::Heartbeat,
      ];
      for cmd in vars {
         let buffer = Vec::with_capacity(128);
         let mut writer = FramedWrite::new(buffer, CommandCodec((&cmd).into()));
         let mut expect_len = 0;
         match cmd {
            Command::Authenticate { .. } => expect_len = expect_len + 16 + 32,
            Command::Connect => expect_len = expect_len + 0,
            Command::Packet { .. } => expect_len = expect_len + 8,
            Command::Dissociate { .. } => expect_len = expect_len + 2,
            Command::Heartbeat => expect_len = expect_len + 0,
         }
         writer.send(cmd.clone()).await?;
         assert_eq!(writer.get_ref().len(), expect_len);
         let buffer = writer.get_ref();
         let mut reader = FramedRead::new(buffer.as_slice(), CommandCodec((&cmd).into()));

         let frame = reader.next().await.unwrap()?;
         assert_eq!(cmd, frame);
      }

      Ok(())
   }
   /// Data not fully arrive
   #[tokio::test]
   async fn test_cmd_2() -> eyre::Result<()> {
      let vars = vec![
         Command::Authenticate {
            uuid:  Uuid::parse_str("02f09a3f-1624-3b1d-8409-44eff7708208")?,
            token: [1; 32],
         },
         Command::Packet {
            assos_id:   123,
            pkt_id:     123,
            frag_total: 5,
            frag_id:    1,
            size:       8,
         },
         Command::Dissociate { assos_id: 23 },
      ];
      for cmd in vars {
         let buffer = Vec::with_capacity(128);
         let mut writer = FramedWrite::new(buffer, CommandCodec((&cmd).into()));
         writer.send(cmd.clone()).await?;
         let mut buffer = writer.into_inner();
         let full_len = buffer.len();
         let mut half_b = buffer.split_off(full_len / 2 as usize);
         let mut half_a = buffer;
         {
            let mut reader = FramedRead::new(half_a.as_slice(), CommandCodec((&cmd).into()));
            assert!(matches!(
               reader.next().await.unwrap().unwrap_err(),
               Error::BytesRemaining
            ));
         }
         half_a.append(&mut half_b);
         let mut reader = FramedRead::new(half_a.as_slice(), CommandCodec((&cmd).into()));
         assert_eq!(reader.next().await.unwrap()?, cmd);
      }

      Ok(())
   }
}
