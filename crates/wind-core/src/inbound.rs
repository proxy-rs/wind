use crate::{AbstractTcpStream, types::TargetAddr};

pub trait FutResult<T> = Future<Output = eyre::Result<T>> + Send + Sync;

pub trait AbstractInbound {
   /// Should not return!
   fn listen(&self, cb: &impl InboundCallback) -> impl FutResult<()>;
}

pub trait InboundCallback: Send + Sync {
   fn invoke(&self, target_addr: TargetAddr, stream: impl AbstractTcpStream) -> impl FutResult<()>;
}
