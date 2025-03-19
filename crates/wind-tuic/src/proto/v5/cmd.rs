use bytes::{Buf, BufMut as _};
use tokio_util::codec::{Decoder, Encoder};
use uuid::Uuid;

use super::CommandType;
use crate::UnknownCommandTypeSnafu;

#[derive(Debug, Clone, Copy)]
pub struct CommandCodec(CommandType);

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
#[cfg(feature = "server")]
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
                let assos_id = src.get_u16();
                let pkt_id = src.get_u16();
                let frag_total = src.get_u8();
                let frag_id = src.get_u8();
                let size = src.get_u16();
                Ok(Some(Command::Packet {
                    assos_id,
                    pkt_id,
                    frag_total,
                    frag_id,
                    size,
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
}

#[cfg(feature = "client")]
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
