use crate::{tcp::AbstractTcpStream, types::TargetAddr, udp::AbstractUdpSocket};

pub trait FutResult<T> = Future<Output = eyre::Result<T>> + Send + Sync;

pub trait AbstractInbound {
	/// Should not return!
	fn listen(&self, cb: &impl InboundCallback) -> impl FutResult<()>;
}

pub trait InboundCallback: Send + Sync {
	fn handle_tcpstream(&self, target_addr: TargetAddr, stream: impl AbstractTcpStream) -> impl FutResult<()>;
	fn handle_udpsocket(&self, socket: impl AbstractUdpSocket + 'static) -> impl FutResult<()>;
}
