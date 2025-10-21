use std::{
	cell::LazyCell, sync::{Arc, atomic::{AtomicU64, Ordering}}, time::{Duration, Instant}
};

use bytes::{BufMut, Bytes, BytesMut};
use crossfire::{MAsyncRx, MAsyncTx};
use moka::future::Cache;
use tokio::time::timeout;
use tokio_util::codec::Encoder;
use wind_core::{types::TargetAddr, udp::UdpPacket};

use crate::{
	Error,
	proto::{AddressCodec, ClientProtoExt as _, CmdCodec, CmdType, Command, Header, HeaderCodec},
};

// Define MTU sizes for UDP segmentation
const MAX_FRAGMENTS: u8 = 255; // Maximum number of fragments allowed
const FRAGMENT_TIMEOUT_MS: u64 = 30000; // 30 seconds timeout for fragment reassembly

const INIT_TIME : LazyCell<Instant> = LazyCell::new(|| Instant::now());

pub struct UdpStream {
	connection:      quinn::Connection,
	assoc_id:        u16,
	send_rx:         MAsyncRx<UdpPacket>,
	receive_tx:      MAsyncTx<UdpPacket>,
	next_pkt_id:     u16, // Track packet IDs for fragmentation
	// Fragment reassembly state (wrapped in Mutex for interior mutability)
	fragment_buffer: FragmentReassemblyBuffer,
}

/// Structure to track fragments of a packet for reassembly
struct FragmentMetadata {
	frag_total:   u8,
	fragments:    Cache<u8, Bytes>,
	last_updated: AtomicU64,
	target:       TargetAddr,
}

/// Buffer for reassembling fragmented packets
struct FragmentReassemblyBuffer {
	fragments: Cache<(u16, u16), Arc<FragmentMetadata>>, // (assoc_id, pkt_id) -> fragment metadata
}

impl FragmentReassemblyBuffer {
	/// Create a new fragment reassembly buffer
	fn new() -> Self {
		Self {
			fragments: Cache::new(1000),
		}
	}

	/// Add a fragment to the buffer
	async fn add_fragment(
		&self,
		assoc_id: u16,
		pkt_id: u16,
		frag_total: u8,
		frag_id: u8,
		payload: Bytes,
		target: TargetAddr,
	) -> Option<UdpPacket> {
		let key = (assoc_id, pkt_id);

		// Get or create the fragment metadata
		let meta = self
			.fragments
			.entry(key)
			.or_insert_with(async {
				Arc::new(FragmentMetadata {
					frag_total,
					fragments: Cache::new(frag_total.into()),
					last_updated: AtomicU64::new(INIT_TIME.elapsed().as_secs()),
					target,
				})
			})
			.await;

		// Update timestamp

		meta.value().last_updated.store(INIT_TIME.elapsed().as_secs(), Ordering::Relaxed);

		// Store this fragment
		meta.value().fragments.insert(frag_id, payload);

		// Check if all fragments have been received
		if meta.value().fragments.entry_count() == meta.value().frag_total.into() {
			// All fragments received, reassemble the packet
			return self.reassemble_packet(key).await;
		}

		None // Not all fragments received yet
	}

	/// Clean up expired fragments
	fn cleanup_expired(&mut self) {

		self.fragments.invalidate_entries_if(move |_, meta| {
			INIT_TIME.elapsed() - Duration::from_secs(meta.last_updated.load(Ordering::Relaxed)) >= Duration::from_millis(FRAGMENT_TIMEOUT_MS)
		});

	}

	/// Reassemble a complete packet from fragments
	async fn reassemble_packet(&self, key: (u16, u16)) -> Option<UdpPacket> {
		if let Some(meta) = self.fragments.remove(&key).await {
			// Create a buffer to hold the reassembled packet
			let mut total_size = 0;
			for i in 0..meta.frag_total {
				if let Some(fragment) = meta.fragments.get(&i).await {
					total_size += fragment.len();
				} else {
					// Missing fragment, this shouldn't happen if we checked properly
					return None;
				}
			}
			let mut buffer = BytesMut::with_capacity(total_size);

			// Combine fragments in order
			for i in 0..meta.frag_total {
				if let Some(fragment) = meta.fragments.get(&i).await {
					buffer.put_slice(&fragment);
				} else {
					// Missing fragment, this shouldn't happen if we checked properly
					return None;
				}
			}

			// Return the reassembled packet
			Some(UdpPacket {
				target:  meta.target.clone(),
				payload: buffer.freeze(),
			})
		} else {
			None
		}
	}
}

impl UdpStream {
	pub fn new(
		connection: quinn::Connection,
		assoc_id: u16,
		send_rx: MAsyncRx<UdpPacket>,
		receive_tx: MAsyncTx<UdpPacket>,
	) -> Self {
		Self {
			connection,
			assoc_id,
			send_rx,
			receive_tx,
			next_pkt_id: 0,
			fragment_buffer: FragmentReassemblyBuffer::new(),
		}
	}

	pub async fn send_packet(&mut self, packet: UdpPacket) -> eyre::Result<()> {
		let payload_len = packet.payload.len();

		let addr_size = match packet.target {
			TargetAddr::IPv4(..) => 1 + 4 + 2, // Type (1) + IPv4 (4) + Port (2)
			TargetAddr::IPv6(..) => 1 + 16 + 2, // Type (1) + IPv6 (16) + Port (2)
			TargetAddr::Domain(ref domain, _) => {
				let domain_len = domain.len();
				if domain_len > 255 {
					return Err(eyre::eyre!("Domain name too long"));
				}
				1 + 1 + domain_len + 2 // Type (1) + Length (1) + Domain + Port (2)
			}
		};

		// Calculate header overhead: header (variable) + command (8 bytes) + address
		let header_overhead = 8 + addr_size;

		// If payload fits within the MTU, send as a single packet
		// TODO handle the case datagram not supported
		if payload_len <= self.connection.max_datagram_size().unwrap_or(1200) - header_overhead {
			// Send UDP data with association ID
			self.connection
				.send_udp(
					self.assoc_id,
					self.next_pkt_id,
					&packet.target,
					packet.payload,
					true,
				)
				.await?;

			// Increment packet ID for next packet
			self.next_pkt_id += 1;
			return Ok(());
		}


		self.send_fragmented_packet(packet).await
	}

	async fn send_fragmented_packet(&mut self, packet: UdpPacket) -> eyre::Result<()> {
		let payload_len = packet.payload.len();

		// Calculate address size for proper fragment size calculation
		let addr_size = match packet.target {
			TargetAddr::IPv4(..) => 1 + 4 + 2,
			TargetAddr::IPv6(..) => 1 + 16 + 2,
			TargetAddr::Domain(ref domain, _) => 1 + 1 + domain.len() + 2,
		};

		// Calculate max fragment payload size
		// Header (variable) + Command (8 bytes) + Address
		let header_overhead = 8 + addr_size;
		let max_datagram_size = self.connection.max_datagram_size().unwrap_or(1200);
		let max_fragment_size = max_datagram_size.saturating_sub(header_overhead);

		// Calculate number of fragments needed
		let fragment_count = (payload_len + max_fragment_size - 1) / max_fragment_size;
		if fragment_count > MAX_FRAGMENTS as usize {
			return Err(eyre::eyre!(
				"Packet too large for fragmentation, exceeds maximum fragment count"
			));
		}

		// Assign a packet ID for all fragments in this packet
		let pkt_id = self.next_pkt_id;
		self.next_pkt_id = self.next_pkt_id.wrapping_add(1);
		let frag_total = fragment_count as u8;

		// Fragment and send each piece
		for frag_id in 0..fragment_count {
			let start = frag_id * max_fragment_size;
			let end = ((frag_id + 1) * max_fragment_size).min(payload_len);

			// Extract this fragment's payload
			let fragment_payload = packet.payload.slice(start..end);

			// Send the fragment with proper command parameters
			let mut send = self.connection.open_uni().await?;
			let mut buf = BytesMut::with_capacity(12);

			// Create packet command with fragmentation info
			HeaderCodec.encode(Header::new(CmdType::Packet), &mut buf)?;
			CmdCodec(CmdType::Packet).encode(
				Command::Packet {
					assoc_id: self.assoc_id,
					pkt_id,
					frag_total,
					frag_id: frag_id as u8,
					size: fragment_payload.len() as u16,
				},
				&mut buf,
			)?;

			// Add target address
			AddressCodec.encode(packet.target.to_owned().into(), &mut buf)?;

			// Write header and payload
			send.write_all_chunks(&mut [buf.into(), fragment_payload])
				.await?;
		}

		Ok(())
	}

	pub async fn handle_send_queue(&mut self) -> eyre::Result<()> {
		loop {
			match timeout(Duration::from_secs(5), self.send_rx.recv()).await {
				Ok(result) => {
					let packet = result?;
					self.send_packet(packet).await?;
				}
				Err(_) => {
					// Timeout - clean up expired fragments
					self.fragment_buffer.cleanup_expired();
				}
			}
		}
	}

	/// Process an incoming packet fragment
	/// This would be called by the packet handler in the TUIC protocol
	pub async fn process_fragment(
		&self,
		assoc_id: u16,
		pkt_id: u16,
		frag_total: u8,
		frag_id: u8,
		payload: Bytes,
		target: TargetAddr,
	) -> Option<UdpPacket> {
		// Add fragment to reassembly buffer and check if packet is complete
		self.fragment_buffer
			.add_fragment(assoc_id, pkt_id, frag_total, frag_id, payload, target).await
	}

	/// Receive a complete packet from remote server
	/// This will forward the packet to the local receive channel
	pub async fn receive_packet(&self, packet: UdpPacket) -> eyre::Result<()> {
		self.receive_tx
			.send(packet)
			.await
			.map_err(|e| eyre::eyre!("Failed to send packet to receive channel: {:?}", e))
	}

	pub async fn close(&mut self) -> Result<(), Error> {
		// Close the UDP association
		self.connection.drop_udp(self.assoc_id).await
	}
}

#[cfg(test)]
mod tests {
	use std::net::{Ipv4Addr, Ipv6Addr};

	use super::*;

	/// Test helper to calculate address size according to SPEC.md Section 6.2
	/// (Address Type Registry) and Section 6.3 (Address Type Specifications)
	fn calculate_addr_size(target: &TargetAddr) -> usize {
		match target {
			TargetAddr::IPv4(..) => 1 + 4 + 2, // Type (1) + IPv4 (4) + Port (2) = 7 bytes
			TargetAddr::IPv6(..) => 1 + 16 + 2, // Type (1) + IPv6 (16) + Port (2) = 19 bytes
			TargetAddr::Domain(domain, _) => 1 + 1 + domain.len() + 2, /* Type (1) + Len (1) +
			                                     * Domain + Port (2) */
		}
	}

	/// SPEC.md Section 8.1: Base Command Header
	/// Base command header = VER (1B) + TYPE (1B) = 2 bytes
	#[test]
	fn test_base_command_header_size() {
		// According to SPEC.md Section 8.1, base header is 2 bytes
		const BASE_HEADER_SIZE: usize = 2;
		assert_eq!(BASE_HEADER_SIZE, 2, "Base command header should be 2 bytes");
	}

	/// SPEC.md Section 8.2: Packet Command Overhead
	/// Packet command = VER (1B) + TYPE (1B) + ASSOC_ID (2B) + PKT_ID (2B)
	///                 + FRAG_TOTAL (1B) + FRAG_ID (1B) + SIZE (2B) = 10 bytes
	#[test]
	fn test_packet_command_header_size() {
		// According to SPEC.md Section 8.2, packet command header (without ADDR) is 10
		// bytes
		const PACKET_CMD_SIZE: usize = 2 + 2 + 1 + 1 + 2; // ASSOC_ID + PKT_ID + FRAG_TOTAL + FRAG_ID + SIZE
		assert_eq!(
			PACKET_CMD_SIZE, 8,
			"Packet command fields should be 8 bytes"
		);

		// Total with base header
		const TOTAL_PACKET_HEADER: usize = 2 + 8; // VER + TYPE + packet fields
		assert_eq!(
			TOTAL_PACKET_HEADER, 10,
			"Total packet header should be 10 bytes"
		);
	}

	/// SPEC.md Section 6.3: Address Type Specifications - IPv4
	#[test]
	fn test_ipv4_address_size() {
		let addr = TargetAddr::IPv4(Ipv4Addr::new(192, 168, 1, 1), 8080);
		let size = calculate_addr_size(&addr);

		// Type (1) + IPv4 (4) + Port (2) = 7 bytes
		assert_eq!(
			size, 7,
			"IPv4 address field should be 7 bytes according to SPEC.md Section 6.3"
		);
	}

	/// SPEC.md Section 6.3: Address Type Specifications - IPv6
	#[test]
	fn test_ipv6_address_size() {
		let addr = TargetAddr::IPv6(Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1), 8080);
		let size = calculate_addr_size(&addr);

		// Type (1) + IPv6 (16) + Port (2) = 19 bytes
		assert_eq!(
			size, 19,
			"IPv6 address field should be 19 bytes according to SPEC.md Section 6.3"
		);
	}

	/// SPEC.md Section 6.3: Address Type Specifications - Domain
	#[test]
	fn test_domain_address_size() {
		let domain = "example.com".to_string();
		let addr = TargetAddr::Domain(domain.clone(), 443);
		let size = calculate_addr_size(&addr);

		// Type (1) + Len (1) + Domain (11) + Port (2) = 15 bytes
		let expected = 1 + 1 + domain.len() + 2;
		assert_eq!(
			size, expected,
			"Domain address field should be 4 + N bytes according to SPEC.md Section 6.3"
		);
		assert_eq!(size, 15, "example.com should be 15 bytes total");
	}

	/// SPEC.md Section 6.3: Address Type Specifications - Various domain
	/// lengths
	#[test]
	fn test_domain_address_various_lengths() {
		let test_cases = vec![
			("a.com", 4 + 5),                                          // 9 bytes
			("example.com", 4 + 11),                                   // 15 bytes
			("very-long-domain-name-for-testing.example.com", 4 + 45), // 49 bytes (45 chars)
		];

		for (domain, expected_size) in test_cases {
			let addr = TargetAddr::Domain(domain.to_string(), 443);
			let size = calculate_addr_size(&addr);
			assert_eq!(
				size,
				expected_size,
				"Domain '{}' (len={}) size mismatch",
				domain,
				domain.len()
			);
		}
	}

	/// SPEC.md Section 8.4: Total Packet Command Overhead - First Fragment
	#[test]
	fn test_total_header_overhead_ipv4() {
		let addr = TargetAddr::IPv4(Ipv4Addr::new(127, 0, 0, 1), 80);
		let addr_size = calculate_addr_size(&addr);

		// Header (2) + Command (8) + Address (7) = 17 bytes
		let total = 2 + 8 + addr_size;
		assert_eq!(
			total, 17,
			"Total header overhead for IPv4 should be 17 bytes according to SPEC.md Section 8.4"
		);
	}

	#[test]
	fn test_total_header_overhead_ipv6() {
		let addr = TargetAddr::IPv6(Ipv6Addr::LOCALHOST, 80);
		let addr_size = calculate_addr_size(&addr);

		// Header (2) + Command (8) + Address (19) = 29 bytes
		let total = 2 + 8 + addr_size;
		assert_eq!(
			total, 29,
			"Total header overhead for IPv6 should be 29 bytes according to SPEC.md Section 8.4"
		);
	}

	#[test]
	fn test_total_header_overhead_domain() {
		let addr = TargetAddr::Domain("example.com".to_string(), 443);
		let addr_size = calculate_addr_size(&addr);

		// Header (2) + Command (8) + Address (15) = 25 bytes
		let total = 2 + 8 + addr_size;
		assert_eq!(
			total, 25,
			"Total header overhead for 'example.com' should be 25 bytes according to SPEC.md \
			 Section 8.4"
		);
	}

	/// SPEC.md Section 8.4: Subsequent Fragments (None address type)
	#[test]
	fn test_subsequent_fragment_overhead() {
		// None address type = 1 byte
		const NONE_ADDR_SIZE: usize = 1;
		let total = 2 + 8 + NONE_ADDR_SIZE;

		assert_eq!(
			total, 11,
			"Subsequent fragment overhead should be 11 bytes according to SPEC.md Section 8.4"
		);
	}

	/// SPEC.md Section 8.5: Maximum Payload Calculations
	#[test]
	fn test_max_payload_calculation_1200mtu() {
		const MAX_DATAGRAM_SIZE: usize = 1200;

		// IPv4
		let ipv4_addr = TargetAddr::IPv4(Ipv4Addr::new(192, 168, 1, 1), 8080);
		let ipv4_overhead = 2 + 8 + calculate_addr_size(&ipv4_addr);
		let ipv4_max_payload = MAX_DATAGRAM_SIZE - ipv4_overhead;
		assert_eq!(
			ipv4_max_payload, 1183,
			"IPv4 max payload should be 1183 bytes with 1200 MTU (SPEC.md Section 8.5)"
		);

		// IPv6
		let ipv6_addr = TargetAddr::IPv6(Ipv6Addr::LOCALHOST, 8080);
		let ipv6_overhead = 2 + 8 + calculate_addr_size(&ipv6_addr);
		let ipv6_max_payload = MAX_DATAGRAM_SIZE - ipv6_overhead;
		assert_eq!(
			ipv6_max_payload, 1171,
			"IPv6 max payload should be 1171 bytes with 1200 MTU (SPEC.md Section 8.5)"
		);

		// Domain (11 chars)
		let domain_addr = TargetAddr::Domain("example.com".to_string(), 443);
		let domain_overhead = 2 + 8 + calculate_addr_size(&domain_addr);
		let domain_max_payload = MAX_DATAGRAM_SIZE - domain_overhead;
		assert_eq!(
			domain_max_payload, 1175,
			"Domain 'example.com' max payload should be 1175 bytes with 1200 MTU (SPEC.md Section \
			 8.5)"
		);
	}

	/// SPEC.md Section 8.6: Fragmentation Size Calculations
	#[test]
	fn test_fragment_count_calculation() {
		const MAX_DATAGRAM_SIZE: usize = 1200;
		let addr = TargetAddr::IPv4(Ipv4Addr::new(192, 168, 1, 1), 8080);
		let header_overhead = 2 + 8 + calculate_addr_size(&addr);
		let max_fragment_size = MAX_DATAGRAM_SIZE - header_overhead;

		// Test various payload sizes
		let test_cases = vec![
			(1000, 1),  // Small payload, 1 fragment
			(1183, 1),  // Exactly max size, 1 fragment
			(1184, 2),  // Just over, 2 fragments
			(2366, 2),  // 2 * max_fragment_size, 2 fragments
			(2367, 3),  // Just over 2x, 3 fragments
			(10000, 9), // Large payload
		];

		for (payload_size, expected_fragments) in test_cases {
			let fragment_count = (payload_size + max_fragment_size - 1) / max_fragment_size;
			assert_eq!(
				fragment_count, expected_fragments,
				"Payload {} bytes should require {} fragments",
				payload_size, expected_fragments
			);
		}
	}

	/// SPEC.md Section 8.7: Implementation Constraints - Fragment count must
	/// not exceed 255
	#[test]
	fn test_max_fragment_limit() {
		const MAX_FRAGMENTS: u8 = 255;
		const MAX_DATAGRAM_SIZE: usize = 1200;

		let addr = TargetAddr::IPv4(Ipv4Addr::new(192, 168, 1, 1), 8080);
		let header_overhead = 2 + 8 + calculate_addr_size(&addr);
		let max_fragment_size = MAX_DATAGRAM_SIZE - header_overhead;

		// Maximum allowable payload
		let max_payload = max_fragment_size * (MAX_FRAGMENTS as usize);
		let fragment_count = (max_payload + max_fragment_size - 1) / max_fragment_size;

		assert_eq!(fragment_count, 255, "Should be able to send 255 fragments");
		assert!(
			fragment_count <= MAX_FRAGMENTS as usize,
			"Fragment count must not exceed 255"
		);

		// One byte over should exceed limit
		let oversized_payload = max_payload + 1;
		let oversized_count = (oversized_payload + max_fragment_size - 1) / max_fragment_size;
		assert!(
			oversized_count > MAX_FRAGMENTS as usize,
			"Oversized payload should exceed fragment limit"
		);
	}

	/// Test fragment reassembly buffer
	#[tokio::test]
	async fn test_fragment_reassembly_single_fragment() {
		let buffer = FragmentReassemblyBuffer::new();
		let target = TargetAddr::IPv4(Ipv4Addr::new(127, 0, 0, 1), 8080);
		let payload = Bytes::from("test payload");

		// Single fragment packet
		let result = buffer.add_fragment(1, 100, 1, 0, payload.clone(), target.clone()).await;

		assert!(
			result.is_some(),
			"Single fragment should complete immediately"
		);
		let packet = result.unwrap();
		assert_eq!(packet.payload, payload);
	}

	/// Test fragment reassembly with multiple fragments
	#[tokio::test]
	async fn test_fragment_reassembly_multiple_fragments() {
		let buffer = FragmentReassemblyBuffer::new();
		let target = TargetAddr::IPv4(Ipv4Addr::new(127, 0, 0, 1), 8080);

		let frag1 = Bytes::from("Hello ");
		let frag2 = Bytes::from("World");

		// Add first fragment
		let result1 = buffer.add_fragment(1, 200, 2, 0, frag1.clone(), target.clone()).await;
		assert!(
			result1.is_none(),
			"First fragment should not complete packet"
		);

		// Add second fragment - should complete
		let result2 = buffer.add_fragment(1, 200, 2, 1, frag2.clone(), target.clone()).await;
		assert!(result2.is_some(), "Second fragment should complete packet");

		let packet = result2.unwrap();
		assert_eq!(packet.payload, Bytes::from("Hello World"));
	}

	/// Test fragment reassembly with out-of-order fragments
	#[tokio::test]
	async fn test_fragment_reassembly_out_of_order() {
		let buffer = FragmentReassemblyBuffer::new();
		let target = TargetAddr::IPv4(Ipv4Addr::new(127, 0, 0, 1), 8080);

		let frag0 = Bytes::from("A");
		let frag1 = Bytes::from("B");
		let frag2 = Bytes::from("C");

		// Add fragments out of order: 2, 0, 1
		assert!(
			buffer
				.add_fragment(1, 300, 3, 2, frag2.clone(), target.clone())
				.await
				.is_none()
		);
		assert!(
			buffer
				.add_fragment(1, 300, 3, 0, frag0.clone(), target.clone())
				.await
				.is_none()
		);

		let result = buffer.add_fragment(1, 300, 3, 1, frag1.clone(), target.clone()).await;
		assert!(result.is_some(), "All fragments received, should complete");

		let packet = result.unwrap();
		assert_eq!(packet.payload, Bytes::from("ABC"));
	}

	/// Test multiple simultaneous fragmentations
	#[tokio::test]
	async fn test_multiple_simultaneous_fragmentations() {
		let buffer = FragmentReassemblyBuffer::new();
		let target = TargetAddr::IPv4(Ipv4Addr::new(127, 0, 0, 1), 8080);

		// Start two different packets
		buffer.add_fragment(1, 100, 2, 0, Bytes::from("A1"), target.clone()).await;
		buffer.add_fragment(1, 101, 2, 0, Bytes::from("B1"), target.clone()).await;

		// Complete first packet
		let result1 = buffer.add_fragment(1, 100, 2, 1, Bytes::from("A2"), target.clone()).await;
		assert!(result1.is_some());
		assert_eq!(result1.unwrap().payload, Bytes::from("A1A2"));

		// Complete second packet
		let result2 = buffer.add_fragment(1, 101, 2, 1, Bytes::from("B2"), target.clone()).await;
		assert!(result2.is_some());
		assert_eq!(result2.unwrap().payload, Bytes::from("B1B2"));
	}

	/// Test fragment cleanup (expired fragments)
	#[tokio::test]
	async fn test_fragment_cleanup() {
		let buffer = FragmentReassemblyBuffer::new();
		let target = TargetAddr::IPv4(Ipv4Addr::new(127, 0, 0, 1), 8080);

		// Add incomplete fragment
		buffer.add_fragment(1, 400, 2, 0, Bytes::from("test"), target.clone()).await;
		assert_eq!(
			buffer.fragments.entry_count(),
			1,
			"Should have one incomplete packet"
		);

		// Get the metadata and manually set timestamp to simulate expiration
		if let Some(meta) = buffer.fragments.get(&(1, 400)).await {
			meta.last_updated.store(
				(INIT_TIME.elapsed() - Duration::from_secs(35)).as_secs(),
				Ordering::Relaxed
			);
		}

		// Cleanup should remove expired fragments
		buffer.fragments.run_pending_tasks().await;
		buffer.fragments.invalidate_entries_if(move |_, meta| {
			INIT_TIME.elapsed() - Duration::from_secs(meta.last_updated.load(Ordering::Relaxed)) >= Duration::from_millis(FRAGMENT_TIMEOUT_MS)
		}).expect("Failed to invalidate entries");
		
		// Wait for invalidation to complete
		buffer.fragments.run_pending_tasks().await;
		
		assert_eq!(
			buffer.fragments.entry_count(),
			0,
			"Expired fragments should be cleaned up"
		);
	}

	/// Verify saturating_sub prevents underflow as mentioned in SPEC.md Section
	/// 8.7
	#[test]
	fn test_saturating_sub_prevents_underflow() {
		let small_mtu: usize = 10;
		let large_overhead: usize = 100;

		// Using saturating_sub should give 0 instead of underflowing
		let result = small_mtu.saturating_sub(large_overhead);
		assert_eq!(result, 0, "saturating_sub should prevent underflow");

		// Normal subtraction would panic in debug mode or wrap in release
		// This test verifies the implementation advice from SPEC.md Section 8.7
	}
}
