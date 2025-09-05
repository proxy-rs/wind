use std::{backtrace::Backtrace, net::SocketAddr};

pub mod proto;

pub mod tls;

#[cfg(feature = "server")]
pub mod inbound;

#[cfg(feature = "client")]
pub mod outbound;

use proto::ProtoError;
use quinn::crypto::ExportKeyingMaterialError;
use snafu::{IntoError, prelude::*};

#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
pub enum Error {
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
	Tls {
		source:    rustls::Error,
		backtrace: Backtrace,
	},
	QuicConnect {
		addr:        SocketAddr,
		server_name: String,
		source:      quinn::ConnectError,
		backtrace:   Backtrace,
	},
	QuicConnection {
		source:    quinn::ConnectionError,
		backtrace: Backtrace,
	},
	ExportKeyingMaterial {},

	Write {
		source:    quinn::WriteError,
		backtrace: Backtrace,
	},
	SendDatagram {
		source:    quinn::SendDatagramError,
		backtrace: Backtrace,
	},
}

impl From<ProtoError> for Error {
	#[inline(always)]
	fn from(value: ProtoError) -> Self {
		ProtoSnafu.into_error(value)
	}
}

impl From<std::io::Error> for Error {
	#[inline(always)]
	fn from(value: std::io::Error) -> Self {
		IoSnafu.into_error(value)
	}
}

impl From<rustls::Error> for Error {
	#[inline(always)]
	fn from(value: rustls::Error) -> Self {
		TlsSnafu.into_error(value)
	}
}

impl From<quinn::ConnectionError> for Error {
	#[inline(always)]
	fn from(value: quinn::ConnectionError) -> Self {
		QuicConnectionSnafu.into_error(value)
	}
}

impl From<ExportKeyingMaterialError> for Error {
	#[inline(always)]
	fn from(_: ExportKeyingMaterialError) -> Self {
		ExportKeyingMaterialSnafu.build()
	}
}

impl From<quinn::WriteError> for Error {
	#[inline(always)]
	fn from(value: quinn::WriteError) -> Self {
		WriteSnafu.into_error(value)
	}
}
