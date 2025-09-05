#[cfg(test)]
mod test {
	use std::net::Ipv4Addr;

	use futures_util::SinkExt as _;
	use tokio_util::codec::FramedWrite;

	use crate::proto::{Address, AddressCodec, CmdCodec, CmdType, Command, Header, HeaderCodec};

	#[tokio::test]
	async fn hex_check() -> eyre::Result<()> {
		let mut buffer = Vec::new();
		let addr = Address::IPv4(Ipv4Addr::LOCALHOST, 80);
		FramedWrite::with_capacity(&mut buffer, HeaderCodec, 2)
			.send(Header::new(CmdType::Connect))
			.await?;
		FramedWrite::with_capacity(&mut buffer, CmdCodec(CmdType::Connect), 2)
			.send(Command::Connect)
			.await?;
		FramedWrite::new(&mut buffer, AddressCodec)
			.send(addr.to_owned().into())
			.await?;

		assert_eq!("0501017f0000010050", hex::encode(buffer));
		Ok(())
	}
}
