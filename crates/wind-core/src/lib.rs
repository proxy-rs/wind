mod interface;
pub mod io;
mod outbound;
mod inbound;
pub mod types;
pub use interface::*;
pub use outbound::*;
pub use inbound::*;
use tokio::io::{AsyncRead, AsyncWrite};

pub trait AbstractTcpStream: AsyncRead + AsyncWrite + Send + Unpin {}

impl<T> AbstractTcpStream for T where T: AsyncRead + AsyncWrite + Send + Unpin {}