use std::{backtrace::Backtrace, net::SocketAddr};

pub mod proto;

pub mod tls;

#[cfg(feature = "server")]
pub mod inbound;

#[cfg(feature = "client")]
pub mod outbound;

pub mod ext;

use proto::ProtoError;
use snafu::{IntoError, prelude::*};

#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub(crate) enum Error {
   Proto {
      #[snafu(backtrace)]
      source: ProtoError,
   },
   BindSocket {
      socket_addr: SocketAddr,
      source:      std::io::Error,
      backtrace:   Backtrace,
   },
   Io {
      source:    std::io::Error,
      backtrace: Backtrace,
   },
}

impl From<std::io::Error> for Error {
   #[inline(always)]
   fn from(value: std::io::Error) -> Self {
      IoSnafu.into_error(value)
   }
}

#[tokio::test]
async fn test_main() {}
