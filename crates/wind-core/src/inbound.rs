use crate::{AbstractTcpStream, types::TargetAddr};
use std::fmt::Debug;

pub trait FutResult<T> = Future<Output = eyre::Result<T>> + Send + Sync;

pub trait AbstractInbound {
	/// Should not return!
	fn listen(&self, cb: &impl InboundCallback) -> impl FutResult<()>;
}

pub trait InboundCallback: Send + Sync {
	fn handle_tcpstream(&self, target_addr: TargetAddr, stream: impl AbstractTcpStream)
	-> impl FutResult<()>;
	fn handle_udpsocket(&self, stream: impl AbstractUdpSocket)
	-> impl FutResult<()> {
		quinn::AsyncUdpSocket
		unimplemented!()
	}
}

pub trait AbstractUdpSocket: Send + Sync + Debug + 'static {

}