use bytes::{Buf, BufMut};
use num_enum::{FromPrimitive, IntoPrimitive};
use snafu::ensure;
use tokio_util::codec::{Decoder, Encoder};

use super::VER;
use crate::{UnknownCommandTypeSnafu, VersionDismatchSnafu};

#[derive(Debug, Clone, Copy)]
pub struct HeaderCodec;

#[derive(Debug, Clone, PartialEq)]
pub struct Header {
    pub version: u8,
    pub command: CommandType,
}

#[derive(IntoPrimitive, FromPrimitive, Copy, Clone, Debug, PartialEq)]
#[repr(u8)]
pub enum CommandType {
    Authenticate = 0,
    Connect      = 1,
    Packet       = 2,
    Dissociate   = 3,
    Heartbeat    = 4,
    #[num_enum(catch_all)]
    Other(u8),
}

impl Header {
    pub fn new(command: CommandType) -> Self {
        Self {
            version: VER,
            command,
        }
    }
}

#[cfg(feature = "server")]
impl Decoder for HeaderCodec {
    type Error = crate::Error;
    type Item = Header;

    fn decode(&mut self, src: &mut bytes::BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if src.len() < 2 {
            return Ok(None);
        }
        let ver = src.get_u8();
        ensure!(ver == VER, VersionDismatchSnafu { expect: VER, current: ver });

        let cmd = CommandType::from(src.get_u8());

        ensure!(!matches!(cmd, CommandType::Other(..)), UnknownCommandTypeSnafu { value: u8::from(cmd) });

        Ok(Some(Header::new(cmd)))
    }
}

#[cfg(feature = "client")]
impl Encoder<Header> for HeaderCodec {
    type Error = crate::Error;

    fn encode(&mut self, item: Header, dst: &mut bytes::BytesMut) -> Result<(), Self::Error> {
        dst.reserve(2);
        dst.put_u8(item.version);
        dst.put_u8(item.command.into());
        Ok(())
    }
}
