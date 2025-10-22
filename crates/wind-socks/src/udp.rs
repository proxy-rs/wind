use std::{
	io::IoSliceMut,
	net::{Ipv4Addr, SocketAddr},
	pin::Pin,
	sync::Arc,
	task::{Context, Poll, ready},
};

use arc_swap::ArcSwap;
use fast_socks5::{new_udp_header, util::target_addr::TargetAddr as SocksTargetAddr};
use tokio::io::Interest;
use wind_core::{
	types::TargetAddr,
	udp::{AbstractUdpSocket, QuinnRecvMeta, RecvMeta, Transmit, UdpPollHelper, UdpPoller, UdpSocketState},
};

/// A virtual UDP socket that handles SOCKS5 UDP headers
/// It parses incoming SOCKS5 UDP packets and strips the headers,
/// and adds SOCKS5 headers to outgoing packets
#[derive(Debug)]
pub struct Socks5UdpSocket {
	io:          tokio::net::UdpSocket,
	inner:       UdpSocketState,
	source_addr: ArcSwap<SocketAddr>,
}

impl Socks5UdpSocket {
	pub fn new(sock: std::net::UdpSocket) -> std::io::Result<Self> {
		Ok(Self {
			inner:       UdpSocketState::new((&sock).into())?,
			io:          tokio::net::UdpSocket::from_std(sock)?,
			source_addr: ArcSwap::new(Arc::new(SocketAddr::new(Ipv4Addr::UNSPECIFIED.into(), 0))),
		})
	}

	/// Convert SOCKS target address to our TargetAddr
	fn convert_target_addr(socks_addr: &SocksTargetAddr) -> TargetAddr {
		match socks_addr {
			SocksTargetAddr::Ip(socket_addr) => match socket_addr {
				SocketAddr::V4(addr) => TargetAddr::IPv4(*addr.ip(), addr.port()),
				SocketAddr::V6(addr) => TargetAddr::IPv6(*addr.ip(), addr.port()),
			},
			SocksTargetAddr::Domain(domain, port) => TargetAddr::Domain(domain.clone(), *port),
		}
	}

	/// Get the currently stored source address
	pub fn source_addr(&self) -> SocketAddr {
		**self.source_addr.load()
	}

	/// Synchronously parse SOCKS5 UDP request header
	/// This is a simplified version that doesn't require async/await
	fn parse_udp_request_sync(data: &[u8]) -> Result<(u8, SocksTargetAddr, &[u8]), Box<dyn std::error::Error>> {
		if data.len() < 4 {
			return Err("Packet too short for SOCKS5 UDP header".into());
		}

		// Check reserved bytes (should be 0x00 0x00)
		if data[0] != 0x00 || data[1] != 0x00 {
			return Err("Invalid reserved bytes in SOCKS5 UDP header".into());
		}

		let frag = data[2];
		let atyp = data[3];

		let mut offset = 4;

		// Parse target address based on address type
		let target_addr = match atyp {
			0x01 => {
				// IPv4
				if data.len() < offset + 6 {
					return Err("Incomplete IPv4 address in SOCKS5 UDP header".into());
				}
				let ip = std::net::Ipv4Addr::new(data[offset], data[offset + 1], data[offset + 2], data[offset + 3]);
				let port = u16::from_be_bytes([data[offset + 4], data[offset + 5]]);
				offset += 6;
				SocksTargetAddr::Ip(SocketAddr::V4(std::net::SocketAddrV4::new(ip, port)))
			}
			0x03 => {
				// Domain name
				if data.len() < offset + 1 {
					return Err("Incomplete domain length in SOCKS5 UDP header".into());
				}
				let domain_len = data[offset] as usize;
				offset += 1;

				if data.len() < offset + domain_len + 2 {
					return Err("Incomplete domain name in SOCKS5 UDP header".into());
				}

				let domain = String::from_utf8_lossy(&data[offset..offset + domain_len]).to_string();
				offset += domain_len;
				let port = u16::from_be_bytes([data[offset], data[offset + 1]]);
				offset += 2;
				SocksTargetAddr::Domain(domain, port)
			}
			0x04 => {
				// IPv6
				if data.len() < offset + 18 {
					return Err("Incomplete IPv6 address in SOCKS5 UDP header".into());
				}
				let mut ip_bytes = [0u8; 16];
				ip_bytes.copy_from_slice(&data[offset..offset + 16]);
				let ip = std::net::Ipv6Addr::from(ip_bytes);
				let port = u16::from_be_bytes([data[offset + 16], data[offset + 17]]);
				offset += 18;
				SocksTargetAddr::Ip(SocketAddr::V6(std::net::SocketAddrV6::new(ip, port, 0, 0)))
			}
			_ => {
				return Err(format!("Unsupported address type: {}", atyp).into());
			}
		};

		let payload = &data[offset..];
		Ok((frag, target_addr, payload))
	}
}

impl AbstractUdpSocket for Socks5UdpSocket {
	fn create_io_poller(self: Arc<Self>) -> Pin<Box<dyn UdpPoller>> {
		Box::pin(UdpPollHelper::new(move || {
			let socket = self.clone();
			async move { socket.io.writable().await }
		}))
	}

	fn try_send(&self, transmit: &Transmit) -> std::io::Result<()> {
		// For outgoing packets in SOCKS5 UDP proxy, we need to add SOCKS5 headers
		// Create SOCKS5 target address from the destination
		let socks_target = self.source_addr();

		// Add SOCKS5 UDP header to the packet
		if let Ok(mut packet_with_header) = new_udp_header(socks_target) {
			packet_with_header.extend_from_slice(transmit.contents);

			// Create new transmit with the header-wrapped packet
			let new_transmit = Transmit {
				destination:  socks_target,
				contents:     &packet_with_header,
				ecn:          transmit.ecn,
				segment_size: transmit.segment_size,
				src_ip:       transmit.src_ip,
			};

			self.io
				.try_io(Interest::WRITABLE, || self.inner.send((&self.io).into(), &new_transmit))
		} else {
			// Fall back to direct send if header creation fails
			self.io
				.try_io(Interest::WRITABLE, || self.inner.send((&self.io).into(), transmit))
		}
	}

	fn poll_recv(&self, cx: &mut Context, bufs: &mut [IoSliceMut<'_>], meta: &mut [RecvMeta]) -> Poll<std::io::Result<usize>> {
		// For SOCKS5 UDP, we need to parse incoming packets and strip SOCKS5 headers
		loop {
			ready!(self.io.poll_recv_ready(cx))?;

			// First, receive data into a temporary buffer
			let mut temp_bufs: Vec<Vec<u8>> = Vec::new();

			// Create temporary buffers matching the input buffers
			for buf in bufs.iter() {
				let temp_buf = vec![0u8; buf.len()];
				temp_bufs.push(temp_buf);
			}

			// Convert temp_bufs to IoSliceMut
			let mut temp_io_bufs: Vec<IoSliceMut<'_>> = temp_bufs.iter_mut().map(|buf| IoSliceMut::new(buf)).collect();

			// Create temporary metadata using RecvMeta
			let mut temp_meta = vec![QuinnRecvMeta::default(); meta.len()];

			if let Ok(res) = self.io.try_io(Interest::READABLE, || {
				self.inner.recv((&self.io).into(), &mut temp_io_bufs, &mut temp_meta)
			}) {
				// Process each received packet
				let mut processed_count = 0;
				for i in 0..res {
					if temp_meta[i].len > 0 {
						let packet_data = &temp_bufs[i][..temp_meta[i].len];

						// Record the source address from the received packet
						self.source_addr.store(Arc::new(temp_meta[i].addr));

						// Try to parse SOCKS5 UDP header synchronously
						match Self::parse_udp_request_sync(packet_data) {
							Ok((_frag, target_addr, payload)) => {
								// Successfully parsed SOCKS5 header, copy payload to output buffer
								let payload_len = payload.len().min(bufs[processed_count].len());
								bufs[processed_count][..payload_len].copy_from_slice(&payload[..payload_len]);

								// Update metadata with SOCKS5 destination information
								meta[processed_count] = RecvMeta::from(temp_meta[i]);
								meta[processed_count].len = payload_len;
								meta[processed_count].destination = Some(Self::convert_target_addr(&target_addr));
								processed_count += 1;
							}
							Err(_) => {
								// Failed to parse SOCKS5 header, treat as raw UDP packet
								let data_len = temp_meta[i].len.min(bufs[processed_count].len());
								bufs[processed_count][..data_len].copy_from_slice(&packet_data[..data_len]);

								// Update metadata without destination info
								meta[processed_count] = RecvMeta::from(temp_meta[i]);
								meta[processed_count].len = data_len;
								processed_count += 1;
							}
						}
					}
				}
				return Poll::Ready(Ok(processed_count));
			}
		}
	}

	fn local_addr(&self) -> std::io::Result<SocketAddr> {
		self.io.local_addr()
	}

	fn may_fragment(&self) -> bool {
		self.inner.may_fragment()
	}

	fn max_transmit_segments(&self) -> usize {
		self.inner.max_gso_segments()
	}

	fn max_receive_segments(&self) -> usize {
		self.inner.gro_segments()
	}
}
