use tokio::io::{AsyncRead, AsyncWrite};

use crate::outbound::TuicTcpStream;

impl AsyncRead for TuicTcpStream {
   fn poll_read(
      self: std::pin::Pin<&mut Self>,
      cx: &mut std::task::Context<'_>,
      buf: &mut tokio::io::ReadBuf<'_>,
   ) -> std::task::Poll<std::io::Result<()>> {
      todo!()
   }
}
impl AsyncWrite for TuicTcpStream {
   fn poll_write(
      self: std::pin::Pin<&mut Self>,
      cx: &mut std::task::Context<'_>,
      buf: &[u8],
   ) -> std::task::Poll<Result<usize, std::io::Error>> {
      todo!()
   }

   fn poll_flush(
      self: std::pin::Pin<&mut Self>,
      cx: &mut std::task::Context<'_>,
   ) -> std::task::Poll<Result<(), std::io::Error>> {
      todo!()
   }

   fn poll_shutdown(
      self: std::pin::Pin<&mut Self>,
      cx: &mut std::task::Context<'_>,
   ) -> std::task::Poll<Result<(), std::io::Error>> {
      todo!()
   }
}
