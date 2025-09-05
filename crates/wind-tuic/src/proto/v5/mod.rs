mod header;

use futures_util::SinkExt as _;
pub use header::*;

mod cmd;
pub use cmd::*;

mod tests;

mod addr;
pub use addr::*;
use snafu::ResultExt;
use tokio_util::codec::FramedWrite;
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
}

impl TuicClientConnection for quinn::Connection {
	async fn send_auth(&self, uuid: &uuid::Uuid, secret: &[u8]) -> Result<(), Error> {
		let mut token = [0u8; 32];
		self.export_keying_material(&mut token, uuid.as_bytes(), secret)?;

		let auth_cmd = Command::Auth { uuid: *uuid, token };
		let mut uni = self.open_uni().await?;
		FramedWrite::with_capacity(&mut uni, HeaderCodec, 2)
			.send(Header::new(CmdType::Auth))
			.await?;
		FramedWrite::with_capacity(&mut uni, CmdCodec(CmdType::Auth), 50)
			.send(auth_cmd)
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

	async fn open_tcp(
		&self,
		addr: &TargetAddr,
		mut stream: impl AbstractTcpStream,
	) -> Result<(usize, usize), Error> {
		let (mut send, recv) = self.open_bi().await?;
		FramedWrite::with_capacity(&mut send, HeaderCodec, 2)
			.send(Header::new(CmdType::Connect))
			.await?;
		FramedWrite::with_capacity(&mut send, CmdCodec(CmdType::Connect), 2)
			.send(Command::Connect)
			.await?;
		FramedWrite::new(&mut send, AddressCodec)
			.send(addr.to_owned().into())
			.await?;

		let (a, b, err) =
			wind_core::io::copy_io(&mut stream, &mut QuinnCompat::new(send, recv)).await;
		if let Some(e) = err {
			return Err(e.into());
		}
		Ok((a, b))
	}
}
