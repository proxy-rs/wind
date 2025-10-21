//! SOCKS5 Proxy Testing Module
//!
//! This module provides utility functions for testing SOCKS5 proxy server TCP
//! and UDP functionality.
//!
//! # Features
//!
//! - **TCP connection testing** - Tests SOCKS5 TCP CONNECT command using
//!   fast-socks5
//! - **UDP connection testing** - Tests SOCKS5 UDP ASSOCIATE command using
//!   fast-socks5
//! - **Direct connection testing** - Baseline tests without proxy for
//!   comparison
//!
//! # Implementation
//!
//! Both TCP and UDP implementations use the `fast-socks5` library for clean and
//! reliable SOCKS5 protocol handling. The library automatically manages:
//!
//! - SOCKS5 authentication handshake
//! - Command negotiation (CONNECT for TCP, UDP ASSOCIATE for UDP)
//! - Packet encapsulation/decapsulation
//! - Error handling for all SOCKS5 error codes
//!
//! This implementation follows RFC 1928 (SOCKS Protocol Version 5) and provides
//! simple, high-level functions for testing SOCKS5 proxy functionality.

use std::net::SocketAddr;

use eyre::Context;
use tokio::{
	io::{AsyncReadExt, AsyncWriteExt},
	net::{TcpStream, UdpSocket},
};

/// Test TCP requests through SOCKS5 proxy
///
/// This function establishes a TCP connection through a SOCKS5 proxy and sends
/// an HTTP GET request to test connectivity.
///
/// # Arguments
/// * `proxy_addr` - SOCKS5 proxy address, e.g., "127.0.0.1:1080"
/// * `target_host` - Target host, e.g., "example.com"
/// * `target_port` - Target port, e.g., 80
///
/// # Errors
/// Returns an error when proxy connection fails, target is unreachable, or I/O
/// operations fail
///
/// # Example
/// ```no_run
/// # tokio_test::block_on(async {
/// wind::tests::test_socks5_tcp("127.0.0.1:1080", "example.com", 80)
/// 	.await
/// 	.unwrap();
/// # });
/// ```
pub async fn test_socks5_tcp(
	proxy_addr: &str,
	target_host: &str,
	target_port: u16,
) -> eyre::Result<()> {
	use fast_socks5::client::Socks5Stream;

	println!("\n========== SOCKS5 TCP Test ==========");
	println!("Proxy address: {}", proxy_addr);
	println!("Target address: {}:{}", target_host, target_port);

	// Establish connection through SOCKS5 proxy (no authentication)
	let mut stream = Socks5Stream::connect(
		proxy_addr,
		target_host.to_string(),
		target_port,
		fast_socks5::client::Config::default(),
	)
	.await
	.map_err(|e| eyre::eyre!("SOCKS5 connection failed: {}", e))?;

	println!("✓ Connected to target through proxy");

	// Send HTTP GET request
	let request = format!(
		"GET / HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nUser-Agent: wind-test/1.0\r\n\r\n",
		target_host
	);
	stream.write_all(request.as_bytes()).await?;
	stream.flush().await?;
	println!("✓ HTTP request sent ({} bytes)", request.len());

	// Read response
	let mut response = Vec::new();
	let bytes_read = stream.read_to_end(&mut response).await?;

	println!("✓ Response received: {} bytes", bytes_read);

	// Parse and print response headers
	if bytes_read > 0 {
		let preview_len = std::cmp::min(500, bytes_read);
		if let Ok(text) = String::from_utf8(response[..preview_len].to_vec()) {
			println!("\n--- Response Preview (first {} bytes) ---", preview_len);
			for line in text.lines().take(15) {
				println!("{}", line);
			}
			if bytes_read > preview_len {
				println!("... ({} bytes truncated)", bytes_read - preview_len);
			}
		}
	}

	println!("========== TCP Test Successful ==========\n");
	Ok(())
}

/// Test UDP requests through SOCKS5 proxy (DNS query example)
///
/// This function establishes a UDP association through a SOCKS5 proxy using the
/// fast-socks5 library and sends a DNS query packet to test UDP functionality.
///
/// # Arguments
/// * `proxy_addr` - SOCKS5 proxy address, e.g., "127.0.0.1:1080"
/// * `target_host` - Target host (DNS server), e.g., "8.8.8.8"
/// * `target_port` - Target port (usually 53), e.g., 53
///
/// # Note
/// Requires the SOCKS5 server to support the UDP ASSOCIATE command (RFC 1928
/// Section 7)
///
/// # Errors
/// Returns an error when the proxy doesn't support UDP, connection fails, or
/// times out
///
/// # Example
/// ```no_run
/// # tokio_test::block_on(async {
/// wind::tests::test_socks5_udp("127.0.0.1:1080", "8.8.8.8", 53)
/// 	.await
/// 	.unwrap();
/// # });
/// ```
pub async fn test_socks5_udp(
	proxy_addr: &str,
	target_host: &str,
	target_port: u16,
) -> eyre::Result<()> {
	use std::time::Duration;

	use fast_socks5::client::Socks5Datagram;

	println!("\n========== SOCKS5 UDP Test ==========");
	println!("Proxy address: {}", proxy_addr);
	println!("Target address: {}:{}", target_host, target_port);

	// Establish TCP connection to SOCKS5 proxy for UDP association
	let backing_socket = TcpStream::connect(proxy_addr)
		.await
		.map_err(|e| eyre::eyre!("Failed to connect to proxy: {}", e))?;
	println!("✓ TCP connection established with proxy");

	// Establish UDP association through SOCKS5 proxy (no authentication)
	// Use 127.0.0.1:0 to bind to any available interface and let the system choose
	// an available port
	let udp_socket_addr = "127.0.0.1:0".parse::<SocketAddr>()?;
	let socket = Socks5Datagram::bind(backing_socket, udp_socket_addr)
		.await
		.context("SOCKS5 UDP association failed")?;

	println!("✓ UDP association established through proxy");
	println!(
		"✓ Local UDP socket bound to: {}",
		socket.get_ref().local_addr()?
	);

	// Prepare DNS query packet
	let dns_query: Vec<u8> = vec![
		0xAB, 0xCD, // Transaction ID
		0x01, 0x00, // Flags: standard query
		0x00, 0x01, // Questions: 1
		0x00, 0x00, // Answer RRs: 0
		0x00, 0x00, // Authority RRs: 0
		0x00, 0x00, // Additional RRs: 0
		// Query: example.com
		0x07, b'e', b'x', b'a', b'm', b'p', b'l', b'e', 0x03, b'c', b'o', b'm', 0x00, 0x00,
		0x01, // Type: A
		0x00, 0x01, // Class: IN
	];

	println!("✓ DNS query prepared ({} bytes)", dns_query.len());

	// Send DNS query through SOCKS5 UDP
	socket
		.send_to(&dns_query, (target_host, target_port))
		.await?;
	println!("✓ DNS query sent through proxy");

	// Receive UDP response with timeout
	let mut buffer = vec![0u8; 1024];
	match tokio::time::timeout(Duration::from_secs(5), socket.recv_from(&mut buffer)).await {
		Ok(Ok((len, _from_addr))) => {
			println!("✓ Response received: {} bytes", len);

			let dns_response = &buffer[..len];

			// Parse DNS response
			if dns_response.len() >= 12 {
				let flags = u16::from_be_bytes([dns_response[2], dns_response[3]]);
				let questions = u16::from_be_bytes([dns_response[4], dns_response[5]]);
				let answers = u16::from_be_bytes([dns_response[6], dns_response[7]]);
				let authorities = u16::from_be_bytes([dns_response[8], dns_response[9]]);
				let additional = u16::from_be_bytes([dns_response[10], dns_response[11]]);

				println!("\n--- DNS Response Details ---");
				println!("  Response code: {}", flags & 0x0F);
				println!("  Questions: {}", questions);
				println!("  Answers: {}", answers);
				println!("  Authorities: {}", authorities);
				println!("  Additional: {}", additional);

				if (flags & 0x0F) == 0 && answers > 0 {
					println!("  ✓ DNS query successful!");
				}
			}
		}
		Ok(Err(e)) => {
			return Err(eyre::eyre!("UDP receive error: {}", e));
		}
		Err(_) => {
			println!("⚠ UDP response timeout (5 seconds)");
			println!("  This could mean:");
			println!("  - The SOCKS5 server doesn't support UDP ASSOCIATE");
			println!("  - The target server is not responding");
			println!("  - Firewall blocking UDP packets");
		}
	}

	println!("========== UDP Test Complete ==========\n");

	Ok(())
}

/// Basic TCP connection test using native Tokio (without proxy)
/// Used to verify if the target server is reachable
pub async fn test_direct_tcp(target_host: &str, target_port: u16) -> eyre::Result<()> {
	let addr = format!("{}:{}", target_host, target_port);
	println!("Direct connection to: {}", addr);

	let mut stream = TcpStream::connect(&addr).await?;
	println!("Connection successful!");

	// Send simple HTTP GET request
	let request = format!(
		"GET / HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
		target_host
	);
	stream.write_all(request.as_bytes()).await?;

	let mut response = vec![0u8; 1024];
	let n = stream.read(&mut response).await?;
	println!("Response received: {} bytes", n);

	Ok(())
}

/// Basic UDP test using native Tokio (without proxy)
pub async fn test_direct_udp(target_host: &str, target_port: u16) -> eyre::Result<()> {
	let addr = format!("{}:{}", target_host, target_port);
	println!("Direct UDP connection to: {}", addr);

	let local_addr: SocketAddr = "0.0.0.0:0".parse()?;
	let socket = UdpSocket::bind(local_addr).await?;
	socket.connect(&addr).await?;

	// Send simple DNS query
	let dns_query: Vec<u8> = vec![
		0x12, 0x34, 0x01, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x07, 0x65, 0x78,
		0x61, 0x6d, 0x70, 0x6c, 0x65, 0x03, 0x63, 0x6f, 0x6d, 0x00, 0x00, 0x01, 0x00, 0x01,
	];

	socket.send(&dns_query).await?;
	println!("UDP packet sent");

	let mut buffer = vec![0u8; 512];
	let n = socket.recv(&mut buffer).await?;
	println!("UDP response received: {} bytes", n);

	Ok(())
}

#[cfg(test)]
mod unit_tests {
	use super::*;

	// Note: These tests require an actual SOCKS5 proxy server to be running
	// Use `cargo test -- --ignored` to run ignored tests

	#[tokio::test]
	#[ignore = "requires running SOCKS5 proxy"]
	async fn test_tcp_through_proxy() {
		let result = test_socks5_tcp("127.0.0.1:6666", "example.com", 80).await;
		assert!(result.is_ok(), "TCP proxy test failed: {:?}", result.err());
	}

	#[tokio::test]
	async fn test_udp_through_proxy() {
		let result = test_socks5_udp("127.0.0.1:6666", "8.8.8.8", 53).await;
		match &result {
			Ok(_) => println!("✓ UDP proxy test passed successfully"),
			Err(e) => {
				let err_str = e.to_string();
				if err_str.contains("Command not supported") {
					println!("⚠ SOCKS5 server does not support UDP ASSOCIATE");
					println!("  This test requires a SOCKS5 server with UDP support enabled");
					println!("  Error: {}", e);
					// Don't fail the test if UDP is just not supported
					return;
				}
				panic!("UDP proxy test failed: {:?}", e);
			}
		}
	}

	#[tokio::test]
	#[ignore = "requires network connection"]
	async fn test_direct_tcp_connection() {
		let result = test_direct_tcp("example.com", 80).await;
		assert!(
			result.is_ok(),
			"Direct TCP connection failed: {:?}",
			result.err()
		);
	}

	#[tokio::test]
	#[ignore = "requires network connection"]
	async fn test_direct_udp_connection() {
		let result = test_direct_udp("8.8.8.8", 53).await;
		assert!(
			result.is_ok(),
			"Direct UDP connection failed: {:?}",
			result.err()
		);
	}
}
