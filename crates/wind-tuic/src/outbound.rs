use std::{
	io::IoSliceMut,
	net::{Ipv4Addr, SocketAddr},
	sync::{Arc, atomic::AtomicU16},
	time::Duration,
};

use eyre::ensure;
use moka::future::Cache;
use quinn::{TokioRuntime, udp::RecvMeta};
use snafu::ResultExt;
use tokio::net::UdpSocket;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use wind_core::{AbstractOutbound, AppContext, info, tcp::AbstractTcpStream, types::TargetAddr, udp::AbstractUdpSocket, warn};

use crate::{
	BindSocketSnafu, Error, QuicConnectSnafu,
	proto::{ClientProtoExt, UdpStream},
	task::ClientTaskExt,
};

pub struct TuicOutboundOpts {
	pub peer_addr:          SocketAddr,
	pub sni:                String,
	pub auth:               (Uuid, Arc<[u8]>),
	pub zero_rtt_handshake: bool,
	pub heartbeat:          Duration,
	pub gc_interval:        Duration,
	pub gc_lifetime:        Duration,
	pub skip_cert_verify:   bool,
	pub alpn:               Vec<String>,
}

pub struct TuicOutbound {
	pub ctx:               Arc<AppContext>,
	pub endpoint:          quinn::Endpoint,
	pub peer_addr:         SocketAddr,
	pub sni:               String,
	pub opts:              TuicOutboundOpts,
	pub connection:        quinn::Connection,
	pub udp_assoc_counter: AtomicU16,
	pub token:             CancellationToken,
	pub udp_session:       Cache<u16, Arc<UdpStream>>,
}

impl TuicOutbound {
	pub async fn new(ctx: Arc<AppContext>, opts: TuicOutboundOpts) -> Result<Self, Error> {
		let peer_addr = opts.peer_addr;
		let server_name = opts.sni.clone();

		// TODO move to top-level initialization
		{
			#[cfg(feature = "aws-lc-rs")]
			rustls::crypto::aws_lc_rs::default_provider().install_default().unwrap();
			#[cfg(feature = "ring")]
			rustls::crypto::ring::default_provider().install_default().unwrap();
		}
		info!(target: "[OUT]", "Creating a new outboud");
		let client_config = {
			let tls_config = super::tls::tls_config(&server_name, &opts)?;

			let mut client_config = quinn::ClientConfig::new(Arc::new(
				quinn::crypto::rustls::QuicClientConfig::try_from(tls_config).unwrap(),
			));
			let mut transport_config = quinn::TransportConfig::default();
			transport_config
				.congestion_controller_factory(Arc::new(quinn::congestion::BbrConfig::default()))
				.keep_alive_interval(None);

			client_config.transport_config(Arc::new(transport_config));
			client_config
		};
		let socket_addr = SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0));
		let socket = UdpSocket::bind(&socket_addr)
			.await
			.context(BindSocketSnafu { socket_addr })?
			.into_std()?;

		let mut endpoint = quinn::Endpoint::new(quinn::EndpointConfig::default(), None, socket, Arc::new(TokioRuntime))?;
		endpoint.set_default_client_config(client_config);
		let connection = endpoint
			.connect(peer_addr, &server_name)
			.context(QuicConnectSnafu {
				addr:        peer_addr,
				server_name: server_name.clone(),
			})?
			.await?;

		connection.send_auth(&opts.auth.0, &opts.auth.1).await?;

		Ok(Self {
			token: ctx.token.child_token(),
			ctx,
			endpoint,
			peer_addr,
			sni: server_name,
			opts,
			connection,
			udp_assoc_counter: AtomicU16::new(0),
			udp_session: Cache::new(u16::MAX.into()),
		})
	}

	pub async fn start_poll(&self) -> eyre::Result<()> {
		// Monitor cancellation token for shutdown
		let cancel_token = self.ctx.token.child_token();
		let connection = self.connection.clone();
		let udp_session = self.udp_session.clone();

		let mut hb_interval = tokio::time::interval(self.opts.heartbeat);
		const HEARTBEAT_MAX_FAILURES: usize = 3;

		let (datagram_rx, bi_rx, uni_rx) = self
			.connection
			.handle_incoming(self.ctx.clone(), cancel_token.clone())
			.await?;

		self.ctx.tasks.spawn(async move {
			let mut hb_failures = 0;
			hb_interval.tick().await;

			loop {
				tokio::select! {
					_ = cancel_token.cancelled() => {
						info!(target: "[OUT]", "Heartbeat poll cancelled");
						return Ok(());
					}
					_ = hb_interval.tick() => {
						if let Err(e) = connection.send_heartbeat().await {
							hb_failures += 1;
							info!(target: "[OUT]", "Heartbeat failed ({}/{}): {}", hb_failures, HEARTBEAT_MAX_FAILURES, e);

							if hb_failures >= HEARTBEAT_MAX_FAILURES {
								return Err(eyre::eyre!("Too many heartbeat failures ({}/{})", hb_failures, HEARTBEAT_MAX_FAILURES));
							}
						} else if hb_failures > 0 {
							info!(target: "[OUT]", "Heartbeat succeeded after {} failures", hb_failures);
							hb_failures = 0;
						}
					}
					Ok(_) = bi_rx.recv() => {
						warn!(target: "[OUT]", "Received bi-directional stream on Outbound");
					}
					Ok(bytes) = datagram_rx.recv() => {
						info!(target: "[OUT]", "Received datagram: {} bytes", bytes.len());
						// Process the received datagram
						use bytes::Buf;
						use tokio_util::codec::Decoder;

						let mut buf = bytes::BytesMut::from(bytes.as_ref());

						// Parse header
						let header = match crate::proto::HeaderCodec.decode(&mut buf) {
							Ok(Some(h)) => h,
							Ok(None) => {
								warn!(target: "[OUT]", "Incomplete header in datagram");
								continue;
							}
							Err(e) => {
								warn!(target: "[OUT]", "Failed to decode header: {}", e);
								continue;
							}
						};

						// Parse command based on header type
						let cmd = match crate::proto::CmdCodec(header.command).decode(&mut buf) {
							Ok(Some(c)) => c,
							Ok(None) => {
								warn!(target: "[OUT]", "Incomplete command in datagram");
								continue;
							}
							Err(e) => {
								warn!(target: "[OUT]", "Failed to decode command: {}", e);
								continue;
							}
						};

						// Process UDP packet
						if let crate::proto::Command::Packet {
							assoc_id,
							pkt_id,
							frag_total,
							frag_id,
							size,
						} = cmd {
							// Parse address
							let addr = match crate::proto::AddressCodec.decode(&mut buf) {
								Ok(Some(a)) => a,
								Ok(None) => {
									warn!(target: "[OUT]", "Incomplete address in UDP packet");
									continue;
								}
								Err(e) => {
									warn!(target: "[OUT]", "Failed to decode address: {}", e);
									continue;
								}
							};

							// Convert address to TargetAddr
							let target = match addr {
								crate::proto::Address::IPv4(ip, port) => TargetAddr::IPv4(ip, port),
								crate::proto::Address::IPv6(ip, port) => TargetAddr::IPv6(ip, port),
								crate::proto::Address::Domain(domain, port) => TargetAddr::Domain(domain, port),
								crate::proto::Address::None => {
									warn!(target: "[OUT]", "Received UDP packet with no address");
									continue;
								}
							};

							// Extract payload
							let payload = buf.copy_to_bytes(size as usize);

							info!(target: "[OUT]", "Received UDP packet: assoc={:#06x}, pkt={}, frag={}/{}, size={}, target={}",
								assoc_id, pkt_id, frag_id + 1, frag_total, size, target);

							// Find the corresponding UDP session
							if let Some(udp_stream) = udp_session.get(&assoc_id).await {
								// Use process_fragment to handle fragmented packets
								// This will return Some(packet) when all fragments are received and reassembled
								let complete_packet = if frag_total > 1 {
									// Fragmented packet - use process_fragment for reassembly
									udp_stream.process_fragment(assoc_id, pkt_id, frag_total, frag_id, payload, target).await
								} else {
									// Single packet (no fragmentation)
									Some(wind_core::udp::UdpPacket {
										target,
										payload,
									})
								};

								// If we have a complete packet, send it to the receive channel
								if let Some(packet) = complete_packet {
									if let Err(e) = udp_stream.receive_packet(packet).await {
										warn!(target: "[OUT]", "Failed to send packet to UDP session {:#06x}: {}", assoc_id, e);
									}
								}
							} else {
								warn!(target: "[OUT]", "Received UDP packet for unknown association {:#06x}", assoc_id);
							}
						} else {
							warn!(target: "[OUT]", "Received non-Packet command in datagram: {:?}", cmd);
						}
					}

					Ok(_recv) = uni_rx.recv() => {
						info!(target: "[OUT]", "Received uni-directional stream");
					}
				}
			}
		});

		Ok(())
	}
}

pub struct TuicTcpStream;

impl AbstractOutbound for TuicOutbound {
	async fn handle_tcp(
		&self,
		target_addr: TargetAddr,
		stream: impl AbstractTcpStream,
		_dialer: Option<impl AbstractOutbound>,
	) -> eyre::Result<()> {
		self.connection.open_tcp(&target_addr, stream).await?;
		Ok(())
	}

	async fn handle_udp(
		&self,
		socket: impl AbstractUdpSocket + 'static,
		_dialer: Option<impl AbstractOutbound>,
	) -> eyre::Result<()> {
		use std::sync::atomic::Ordering;
		// Create a cancel token for single udp session
		let cancel = self.token.child_token();
		// Generate a new UDP association ID
		let assoc_id = self.udp_assoc_counter.fetch_add(1, Ordering::SeqCst);
		info!(target: "[OUT]", "Creating new UDP association: {:#06x}", assoc_id);

		let socket = Arc::new(socket);
		let connection = self.connection.clone();
		let (send_tx, send_rx) = crossfire::mpmc::bounded_async(128);
		let (receive_tx, receive_rx) = crossfire::mpmc::bounded_async(128);
		let udp_stream = Arc::new(UdpStream::new(connection.clone(), assoc_id, receive_tx));
		self.udp_session.insert(assoc_id, udp_stream.clone()).await;
		let cancel_stream = cancel.clone();
		let socket_clone = socket.clone();

		self.ctx.tasks.spawn(async move {
			loop {
				tokio::select! {
					_ = cancel_stream.cancelled() => {
						info!(target: "[OUT]", "UDP stream sender for association {:#06x} cancelled", assoc_id);
						break;
					}

					result = receive_rx.recv() => {
						match result {
							Ok(packet) => {
								// Received packet from remote, send to local socket
								let target_addr = match &packet.target {
									TargetAddr::IPv4(ip, port) => SocketAddr::from((*ip, *port)),
									TargetAddr::IPv6(ip, port) => SocketAddr::from((*ip, *port)),
									TargetAddr::Domain(_, _) => {
										warn!(target: "[OUT]", "Cannot send UDP packet to domain name target for association {:#06x}", assoc_id);
										continue;
									}
								};

								if let Err(e) = socket_clone.send(&packet.payload, target_addr).await {
									warn!(target: "[OUT]", "Failed to send UDP packet to local socket (assoc {:#06x}): {}", assoc_id, e);
								} else {
									info!(target: "[OUT]", "Sent UDP packet to {} ({} bytes, assoc {:#06x})", target_addr, packet.payload.len(), assoc_id);
								}
							}
							Err(e) => {
								warn!(target: "[OUT]", "Error receiving packet from channel for association {:#06x}: {}", assoc_id, e);
								break;
							}
						}
					}

					packet = send_rx.recv() => {
						match packet {
							Ok(packet) => {
								// Send packet to remote via UDP stream
								if let Err(e) = udp_stream.send_packet(packet).await {
									warn!(target: "[OUT]", "Failed to send UDP packet to remote (assoc {:#06x}): {}", assoc_id, e);
								} else {
									info!(target: "[OUT]", "Sent UDP packet to remote ({} bytes, assoc {:#06x})", packet.payload.len(), assoc_id);
								}
							}
							Err(e) => {
								warn!(target: "[OUT]", "Error receiving packet from channel for association {:#06x}: {}", assoc_id, e);
								break;
							}
						}
					}
				}
			}
			eyre::Ok(())
		});

		// Spawn task to continuously read from local socket and send to remote
		self.ctx.tasks.spawn(async move {
			let mut buf = vec![0u8; u16::MAX as usize];

			loop {
				tokio::select! {
					_ = cancel.cancelled() => {
						info!(target: "[OUT]", "UDP session {:#06x} cancelled", assoc_id);
						break;
					}

					result = async {
						let mut meta = RecvMeta::default();
						let result = socket
							.recv(
								&mut [IoSliceMut::new(&mut buf)],
								std::slice::from_mut(&mut meta),
							)
							.await?;
						ensure!(result == 1, "Expected to receive 1 datagram, got {}", result);

						eyre::Ok(meta)
					} => {
						match result {
							Ok(meta) => {
								// In outbound context, meta.addr contains the target address (where to send)
								let target_addr = meta.addr;
								let total_len = meta.len;

								// Convert SocketAddr to TargetAddr
								let target = match target_addr {
									SocketAddr::V4(addr) => TargetAddr::IPv4(*addr.ip(), addr.port()),
									SocketAddr::V6(addr) => TargetAddr::IPv6(*addr.ip(), addr.port()),
								};

								// Handle GRO (Generic Receive Offload): stride indicates segment size
								// If stride > 0, the buffer contains multiple segments of that size
								let stride = meta.stride;

								if stride > 0 && total_len > stride {
									// Multiple segments received via GRO, send each separately
									let num_segments = (total_len + stride - 1) / stride;
									info!(target: "[OUT]", "Received {} GRO segments ({} bytes total, stride {}) for assoc {:#06x}",
										num_segments, total_len, stride, assoc_id);

									for segment_idx in 0..num_segments {
										let segment_start = segment_idx * stride;
										let segment_end = std::cmp::min(segment_start + stride, total_len);

										let payload = bytes::Bytes::copy_from_slice(&buf[segment_start..segment_end]);

										// Create UdpPacket and send via channel
										let packet = wind_core::udp::UdpPacket {
											target: target.clone(),
											payload,
										};

										if let Err(e) = send_tx.send(packet).await {
											warn!(target: "[OUT]", "Failed to send UDP segment {}/{} to channel for association {:#06x}: {}",
												segment_idx + 1, num_segments, assoc_id, e);
										}
									}
								} else {
									// Single packet (no GRO or single segment)
									let payload = bytes::Bytes::copy_from_slice(&buf[..total_len]);

									info!(target: "[OUT]", "Sending UDP packet to {}: {} bytes (assoc {:#06x})", target, total_len, assoc_id);

									// Create UdpPacket and send via channel
									let packet = wind_core::udp::UdpPacket {
										target,
										payload,
									};

									if let Err(e) = send_tx.send(packet).await {
										warn!(target: "[OUT]", "Failed to send UDP packet to channel for association {:#06x}: {}", assoc_id, e);
									}
								}
							}
							Err(e) => {
								warn!(target: "[OUT]", "Error receiving from UDP socket (assoc {:#06x}): {}", assoc_id, e);
								break;
							}
						}
					}
				}
			}
			eyre::Ok(())
		});

		let cancel_healthy = self.ctx.token.clone();
		loop {
			tokio::select! {
				_ = tokio::time::sleep(tokio::time::Duration::from_secs(30)) => {
					info!(target: "[OUT]", "UDP handler for association {:#06x} active", assoc_id);
				}

				_ = cancel_healthy.cancelled() => break,
			}
		}


		// Clean up the UDP association before exiting
		if let Err(err) = self.connection.drop_udp(assoc_id).await {
			info!(target: "[OUT]", "Error dropping UDP association {:#06x}: {}", assoc_id, err);
		}

		Ok(())
	}
}
