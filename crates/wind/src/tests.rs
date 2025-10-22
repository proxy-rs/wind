//! SOCKS5 Proxy Testing Module
//!
//! This module re-exports testing utilities from the `wind-test` crate.
//! All test implementations have been moved to `wind-test` for better organization.

// Re-export all public test functions from wind-test
pub use wind_test::socks5::{
	test_direct_tcp, test_direct_udp, test_socks5_tcp, test_socks5_udp, test_socks5_udp_large_packet,
};
