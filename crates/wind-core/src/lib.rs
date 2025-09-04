#![feature(impl_trait_in_fn_trait_return)]
#![feature(type_alias_impl_trait)]
#![feature(trait_alias)]

mod inbound;
mod interface;
pub mod io;
mod outbound;
pub mod types;
pub use inbound::*;
pub use interface::*;
pub use outbound::*;
use tokio::io::{AsyncRead, AsyncWrite};

pub trait AbstractTcpStream: AsyncRead + AsyncWrite + Send + Sync + Unpin {}

impl<T> AbstractTcpStream for T where T: AsyncRead + AsyncWrite + Send + Sync + Unpin {}
