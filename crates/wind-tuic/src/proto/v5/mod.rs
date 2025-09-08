mod header;

use bytes::BytesMut;
pub use header::*;

mod cmd;
pub use cmd::*;

mod tests;

mod addr;
pub use addr::*;
use snafu::ResultExt;
use tokio_util::codec::Encoder;
use wind_core::{AbstractTcpStream, io::quinn::QuinnCompat, types::TargetAddr};

use crate::{Error, SendDatagramSnafu};

const VER: u8 = 5;

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
	fn send_udp(
		&self,
		assoc_id: u16,
		pkt_id: u16,
		addr: &TargetAddr,
		packet: bytes::Bytes,
		datagram: bool,
	) -> impl Future<Output = Result<(), Error>> + Send;
	fn drop_udp(&self, assoc_id: u16) -> impl Future<Output = Result<(), Error>> + Send;
}

impl TuicClientConnection for quinn::Connection {
	async fn send_auth(&self, uuid: &uuid::Uuid, secret: &[u8]) -> Result<(), Error> {
		let mut token = [0u8; 32];
		self.export_keying_material(&mut token, uuid.as_bytes(), secret)?;

		let auth_cmd = Command::Auth { uuid: *uuid, token };
		let mut send = self.open_uni().await?;
		let mut buf = BytesMut::with_capacity(50);
		HeaderCodec.encode(Header::new(CmdType::Auth), &mut buf)?;
		CmdCodec(CmdType::Auth).encode(auth_cmd, &mut buf)?;
		send.write_chunk(buf.into()).await?;
		Ok(())
	}

	async fn open_tcp(
		&self,
		addr: &TargetAddr,
		mut stream: impl AbstractTcpStream,
	) -> Result<(usize, usize), Error> {
		let (mut send, recv) = self.open_bi().await?;
		let mut buf = BytesMut::with_capacity(9);
		HeaderCodec.encode(Header::new(CmdType::Connect), &mut buf)?;
		CmdCodec(CmdType::Connect).encode(Command::Connect, &mut buf)?;
		AddressCodec.encode(addr.to_owned().into(), &mut buf)?;
		send.write_chunk(buf.into()).await?;
		let (a, b, err) =
			wind_core::io::copy_io(&mut stream, &mut QuinnCompat::new(send, recv)).await;
		if let Some(e) = err {
			return Err(e.into());
		}
		Ok((a, b))
	}

	async fn send_udp(
		&self,
		assoc_id: u16,
		pkt_id: u16,
		addr: &TargetAddr,
		payload: bytes::Bytes,
		datagram: bool,
	) -> Result<(), Error> {
		let mut send = self.open_uni().await?;
		let mut buf = BytesMut::with_capacity(12);
		HeaderCodec.encode(Header::new(CmdType::Packet), &mut buf)?;
		CmdCodec(CmdType::Packet).encode(
			Command::Packet {
				assoc_id,
				pkt_id,
				frag_total: 1,
				frag_id: 0,
				size: payload.len() as u16,
			},
			&mut buf,
		)?;
		AddressCodec.encode(addr.to_owned().into(), &mut buf)?;

		send.write_all_chunks(&mut [buf.into(), payload]).await?;
		Ok(())
	}

	async fn drop_udp(&self, assoc_id: u16) -> Result<(), Error> {
		let mut send = self.open_uni().await?;
		let mut buf = BytesMut::with_capacity(4);
		HeaderCodec.encode(Header::new(CmdType::Dissociate), &mut buf)?;
		CmdCodec(CmdType::Packet).encode(Command::Dissociate { assoc_id }, &mut buf)?;
		send.write_chunk(buf.into()).await?;
		Ok(())
	}

	async fn send_heartbeat(&self) -> Result<(), Error> {
		let mut buf = BytesMut::with_capacity(2);
		HeaderCodec.encode(Header::new(CmdType::Heartbeat), &mut buf)?;
		self.send_datagram(buf.into()).context(SendDatagramSnafu)?;
		Ok(())
	}
}
