#![feature(error_generic_member_access)]

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
		#[snafu(provide)]
		source:    SocksServerError,
		backtrace: Backtrace,
	},
	SocksReply {
		#[snafu(provide)]
		source:    ReplyError,
		backtrace: Backtrace,
	},
	Callback {
		#[snafu(provide)]
		source:    eyre::Report,
		backtrace: Backtrace,
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
