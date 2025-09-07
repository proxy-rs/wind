mod header;

use bytes::BytesMut;
use futures_util::SinkExt as _;
pub use header::*;

mod cmd;
pub use cmd::*;

mod tests;

mod addr;
pub use addr::*;
use snafu::ResultExt;
use tokio_util::codec::{Encoder, FramedWrite};
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
		send.write(&buf).await?;
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
		send.write(&buf).await?;
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
		FramedWrite::with_capacity(&mut send, HeaderCodec, 2)
			.send(Header::new(CmdType::Packet))
			.await?;
		FramedWrite::with_capacity(&mut send, CmdCodec(CmdType::Packet), 2)
			.send(Command::Packet {
				assoc_id,
				pkt_id,
				frag_total: 1,
				frag_id: 0,
				size: payload.len() as u16,
			})
			.await?;
		FramedWrite::new(&mut send, AddressCodec)
			.send(addr.to_owned().into())
			.await?;
		send.write(&payload).await?;
		Ok(())
	}

	async fn drop_udp(&self, assoc_id: u16) -> Result<(), Error> {
		let mut send = self.open_uni().await?;
		FramedWrite::with_capacity(&mut send, HeaderCodec, 2)
			.send(Header::new(CmdType::Dissociate))
			.await?;
		FramedWrite::with_capacity(&mut send, CmdCodec(CmdType::Packet), 2)
			.send(Command::Dissociate { assoc_id })
			.await?;
		Ok(())
	}

	async fn send_heartbeat(&self) -> Result<(), Error> {
		let mut buf = Vec::with_capacity(2);
		FramedWrite::with_capacity(&mut buf, HeaderCodec, 2)
			.send(Header::new(CmdType::Heartbeat))
			.await?;

		self.send_datagram(buf.into()).context(SendDatagramSnafu)?;
		Ok(())
	}
}
