pub mod proto;

use std::{backtrace::Backtrace, str::Utf8Error};

use snafu::{IntoError as _, prelude::*};

#[derive(Debug, Snafu)]
pub enum Error {
    VersionDismatch {
        expect:    u8,
        current:   u8,
        backtrace: Backtrace,
    },
    UnknownCommandType {
        value:     u8,
        backtrace: Backtrace,
    },
    UnknownAddressType {
        value:     u8,
        backtrace: Backtrace,
    },
    FailParseDomain {
        // HEX
        raw:       String,
        source:    Utf8Error,
        backtrace: Backtrace,
    },
    DomainTooLong {
        domain:    String,
        backtrace: Backtrace,
    },
    Io {
        // #[snafu(backtrace)]
        source:    std::io::Error,
        backtrace: Backtrace,
    },
}

impl From<std::io::Error> for Error {
    fn from(source: std::io::Error) -> Self {
        IoSnafu.into_error(source)
    }
}
