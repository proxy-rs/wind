mod header;

use futures_util::SinkExt as _;
pub use header::*;

mod cmd;
pub use cmd::*;

mod addr;
pub use addr::*;
use snafu::ResultExt;
use tokio::io::AsyncWriteExt;
use tokio_util::codec::FramedWrite;
use wind_core::{AbstractTcpStream, io::quinn::QuinnCompat, types::TargetAddr};

use crate::{Error, SendDatagramSnafu};

const VER: u8 = 5;

#[test]
fn test() {
   use bytes::{Buf as _, BufMut as _, BytesMut};
   let mut bytes = BytesMut::with_capacity(64);
   bytes.put_slice(b"daadadwadwad");
   assert_eq!(bytes.len(), {
      bytes.advance(3);
      bytes.len() + 3
   })
}

pub trait TuicClientConnection {
   fn send_auth(
      &self,
      uuid: &uuid::Uuid,
      secret: &[u8],
   ) -> impl Future<Output = Result<(), Error>> + Send;
   fn send_heartbeat(&self) -> impl Future<Output = Result<(), Error>> + Send;
   fn open_tcp(
      &self,
      addr: &TargetAddr,
      stream: impl AbstractTcpStream,
   ) -> impl Future<Output = Result<(usize, usize), Error>> + Send;
}

impl TuicClientConnection for quinn::Connection {
   async fn send_auth(&self, uuid: &uuid::Uuid, secret: &[u8]) -> Result<(), Error> {
      let mut token = [0u8; 32];
      self.export_keying_material(&mut token, uuid.as_bytes(), secret)?;

      let auth_cmd = Command::Auth { uuid: *uuid, token };
      let mut uni = self.open_uni().await?;
      FramedWrite::with_capacity(&mut uni, HeaderCodec, 2).send(Header::new(CmdType::Auth)).await?;
      FramedWrite::with_capacity(&mut uni, CmdCodec(CmdType::Auth), 50).send(auth_cmd).await?;

      Ok(())
   }

   async fn send_heartbeat(&self) -> Result<(), Error> {
      let hb_cmd = Command::Heartbeat;

      let mut writer = FramedWrite::new(Vec::with_capacity(2), CmdCodec(CmdType::Auth));
      writer.send(hb_cmd).await?;
      self
         .send_datagram(writer.into_inner().into())
         .context(SendDatagramSnafu)?;
      Ok(())
   }

   async fn open_tcp(
      &self,
      addr: &TargetAddr,
      mut stream: impl AbstractTcpStream,
   ) -> Result<(usize, usize), Error> {
      let (mut send, recv) = self.open_bi().await?;
      FramedWrite::with_capacity(&mut send, CmdCodec(CmdType::Connect), 2)
         .feed(Command::Connect)
         .await?;
      FramedWrite::new(&mut send, AddressCodec)
         .feed(addr.to_owned().into())
         .await?;

      send.flush().await?;
      let (a, b, err) =
         wind_core::io::copy_io(&mut stream, &mut QuinnCompat::new(send, recv)).await;
      if let Some(e) = err {
         return Err(e.into());
      }
      Ok((a, b))
   }
}
