use std::{backtrace::Backtrace, net::SocketAddr};

use fast_socks5::{ReplyError, server::SocksServerError};
use snafu::{IntoError, Snafu};

pub mod inbound;

#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
pub enum Error {
	BindSocket {
		socket_addr: SocketAddr,
		source:      std::io::Error,
		backtrace:   Backtrace,
	},
	Io {
		source:    std::io::Error,
		backtrace: Backtrace,
	},
	Socks {
		source: SocksServerError,
	},
	SocksReply {
		source: ReplyError,
	},
	Callback {
		source: eyre::Report,
	},
}

impl From<SocksServerError> for Error {
	#[inline(always)]
	fn from(value: SocksServerError) -> Self {
		SocksSnafu.into_error(value)
	}
}

impl From<ReplyError> for Error {
	#[inline(always)]
	fn from(value: ReplyError) -> Self {
		SocksReplySnafu.into_error(value)
	}
}
