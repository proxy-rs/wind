use tokio::io::{AsyncRead, AsyncWrite};

pub trait AbstractTcpStream: AsyncRead + AsyncWrite {}

impl<T> AbstractTcpStream for T where T: AsyncRead + AsyncWrite {}

pub trait AbstractOutbound {
   /// TCP traffic which needs handled by outbound
   fn handle_tcp(
      via: Option<impl AbstractOutbound + Sized + Send>,
   ) -> impl Future<Output = impl AbstractTcpStream> + Send;
}
