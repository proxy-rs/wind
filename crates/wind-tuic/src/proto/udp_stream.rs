use std::{
	collections::HashMap,
	sync::Arc,
	time::{Duration, Instant},
};

use bytes::{BufMut, Bytes, BytesMut};
use crossfire::AsyncRx;
use tokio::time::timeout;
use tokio_util::codec::Encoder;
use wind_core::{types::TargetAddr, udp::UdpPacket};

use crate::proto::{
	AddressCodec, ClientProtoExt as _, CmdCodec, CmdType, Command, Header, HeaderCodec,
};

// Define MTU sizes for UDP segmentation
const MAX_UDP_PAYLOAD_SIZE: usize = 1420; // Conservative default MTU - header sizes
const MAX_FRAGMENTS: u8 = 255; // Maximum number of fragments allowed
const FRAGMENT_TIMEOUT_MS: u64 = 30000; // 30 seconds timeout for fragment reassembly

pub struct UdpStream {
	connection:      Arc<quinn::Connection>,
	assoc_id:        u16,
	rx:              AsyncRx<UdpPacket>,
	next_pkt_id:     u16, // Track packet IDs for fragmentation
	// Fragment reassembly state
	fragment_buffer: FragmentReassemblyBuffer,
}

/// Structure to track fragments of a packet for reassembly
struct FragmentMetadata {
	frag_total:   u8,
	fragments:    HashMap<u8, Bytes>,
	last_updated: Instant,
	target:       TargetAddr,
}

/// Buffer for reassembling fragmented packets
struct FragmentReassemblyBuffer {
	fragments: HashMap<(u16, u16), FragmentMetadata>, // (assoc_id, pkt_id) -> fragment metadata
}

impl FragmentReassemblyBuffer {
	/// Create a new fragment reassembly buffer
	fn new() -> Self {
		Self {
			fragments: HashMap::new(),
		}
	}

	/// Add a fragment to the buffer
	fn add_fragment(
		&mut self,
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
			.or_insert_with(|| FragmentMetadata {
				frag_total,
				fragments: HashMap::new(),
				last_updated: Instant::now(),
				target,
			});

		// Update timestamp
		meta.last_updated = Instant::now();

		// Store this fragment
		meta.fragments.insert(frag_id, payload);

		// Check if all fragments have been received
		if meta.fragments.len() == meta.frag_total as usize {
			// All fragments received, reassemble the packet
			return self.reassemble_packet(key);
		}

		None // Not all fragments received yet
	}

	/// Clean up expired fragments
	fn cleanup_expired(&mut self) {
		let now = Instant::now();
		self.fragments.retain(|_, meta| {
			now.duration_since(meta.last_updated) < Duration::from_millis(FRAGMENT_TIMEOUT_MS)
		});
	}

	/// Reassemble a complete packet from fragments
	fn reassemble_packet(&mut self, key: (u16, u16)) -> Option<UdpPacket> {
		if let Some(meta) = self.fragments.remove(&key) {
			// Create a buffer to hold the reassembled packet
			let total_size = meta
				.fragments
				.values()
				.fold(0, |acc, bytes| acc + bytes.len());
			let mut buffer = BytesMut::with_capacity(total_size);

			// Combine fragments in order
			for i in 0..meta.frag_total {
				if let Some(fragment) = meta.fragments.get(&i) {
					buffer.put_slice(fragment);
				} else {
					// Missing fragment, this shouldn't happen if we checked properly
					return None;
				}
			}

			// Return the reassembled packet
			Some(UdpPacket {
				target:  meta.target,
				payload: buffer.freeze(),
			})
		} else {
			None
		}
	}
}

impl UdpStream {
	pub fn new(connection: Arc<quinn::Connection>, assoc_id: u16, rx: AsyncRx<UdpPacket>) -> Self {
		Self {
			connection,
			assoc_id,
			rx,
			next_pkt_id: 0,
			fragment_buffer: FragmentReassemblyBuffer::new(),
		}
	}

	pub async fn send_packet(&mut self, packet: UdpPacket) -> eyre::Result<()> {
		let payload_len = packet.payload.len();

		let addr_size = match packet.target {
			TargetAddr::IPv4(_, _) => 1 + 4 + 2,    // Type (1) + IPv4 (4) + Port (2)
			TargetAddr::IPv6(_, _) => 1 + 16 + 2,   // Type (1) + IPv6 (16) + Port (2)
			TargetAddr::Domain(ref domain, _) => {
				let domain_len = domain.len();
				if domain_len > 255 {
					return Err(eyre::eyre!("Domain name too long"));
				}
				1 + 1 + domain_len + 2 // Type (1) + Length (1) + Domain + Port (2)
			}
		};

		// If payload fits within the MTU, send as a single packet
		// TODO handle the case datagram not supported
		if payload_len <= self.connection.max_datagram_size().unwrap_or(1200) - todo!() {
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

		// Calculate number of fragments needed
		let fragment_count = todo!();
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
			let start = frag_id * MAX_UDP_PAYLOAD_SIZE;
			let end = std::cmp::min(start + MAX_UDP_PAYLOAD_SIZE, payload_len);

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

	/// Receive a UDP packet
	/// Handles fragment reassembly if needed
	pub async fn recv_packet(&mut self) -> eyre::Result<UdpPacket> {
		// Periodically cleanup expired fragments while waiting for packets
		loop {
			// Try to receive a packet with timeout
			match timeout(Duration::from_secs(5), self.rx.recv()).await {
				Ok(result) => {
					let packet = result?;
					return Ok(packet);
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
	pub fn process_fragment(
		&mut self,
		assoc_id: u16,
		pkt_id: u16,
		frag_total: u8,
		frag_id: u8,
		payload: Bytes,
		target: TargetAddr,
	) -> Option<UdpPacket> {
		// Add fragment to reassembly buffer and check if packet is complete
		self.fragment_buffer
			.add_fragment(assoc_id, pkt_id, frag_total, frag_id, payload, target)
	}

	pub async fn close(&mut self) -> eyre::Result<()> {
		// Close the UDP association
		self.connection.drop_udp(self.assoc_id).await
	}
}
