use std::{sync::Arc, time::Duration};

use bytes::Bytes;
use crossfire::AsyncRx;
use quinn::{RecvStream, SendStream};
use tokio_util::sync::CancellationToken;
use wind_core::{AppContext, info};

use crate::Error;

/// Size of the single-producer single-consumer buffer for QUIC streams
/// This controls how many elements can be buffered in the channel
/// before backpressure is applied
const SPSC_BUFFER_SIZE: usize = 16;

type IncomingRx = (
	AsyncRx<Bytes>,
	AsyncRx<(SendStream, RecvStream)>,
	AsyncRx<RecvStream>,
);

pub trait ClientTaskExt {
	async fn handle_incoming(
		&self,
		ctx: Arc<AppContext>,
		cancel_token: CancellationToken,
	) -> Result<IncomingRx, Error>;
}

impl ClientTaskExt for quinn::Connection {
	async fn handle_incoming(
		&self,
		ctx: Arc<AppContext>,
		cancel_token: CancellationToken,
	) -> Result<IncomingRx, Error> {
		// Create channels for datagrams
		let (datagram_tx, datagram_rx) = crossfire::spsc::bounded_async(SPSC_BUFFER_SIZE);
		let conn_datagram = self.clone();
		let cancel_datagram = cancel_token.clone();

		// Create channels for bidirectional streams
		let (bi_tx, bi_rx) = crossfire::spsc::bounded_async(SPSC_BUFFER_SIZE);
		let conn_bi = self.clone();
		let cancel_bi = cancel_token.clone();

		// Create channels for unidirectional streams
		let (uni_tx, uni_rx) = crossfire::spsc::bounded_async(SPSC_BUFFER_SIZE);
		let conn_uni = self.clone();
		let cancel_uni = cancel_token.clone();

		// Spawn task for handling datagrams
		ctx.tasks.spawn(async move {
			loop {
				tokio::select! {
					res = conn_datagram.read_datagram() => {
						match res {
							Ok(datagram) => {
								info!("Received datagram: {} bytes", datagram.len());
								if let Err(e) = datagram_tx.send_timeout(datagram, Duration::from_secs(1)).await {
									unimplemented!("unhandled error {e:?}");
								}
							},
							Err(e) => unimplemented!("unhandled error {e:?}")
						}
					}
					_ = cancel_datagram.cancelled() => {
						info!("Cancellation requested for datagram task");
						return;
					}
				}
			}
		});

		// Spawn task for handling bidirectional streams
		ctx.tasks.spawn(async move {
			loop {
				tokio::select! {
					res = conn_bi.accept_bi() => {
						match res {
							Ok(stream) => {
								info!("Accepted new bi-directional stream");
								if let Err(e) = bi_tx.send_timeout(stream, Duration::from_secs(1)).await {
									unimplemented!("unhandled error {e:?}");
								}
							},
							Err(e) => unimplemented!("unhandled error {e:?}"),
						}
					}
					_ = cancel_bi.cancelled() => {
						info!("Cancellation requested for bidirectional stream task");
						break;
					}
				}
			}
		});

		// Spawn task for handling unidirectional streams
		ctx.tasks.spawn(async move {
			loop {
				tokio::select! {
					res = conn_uni.accept_uni() => {
						match res {
							Ok(stream) => {
								info!("Accepted new uni-directional stream");
								if let Err(e) = uni_tx.send_timeout(stream, Duration::from_secs(1)).await {
									unimplemented!("unhandled error {e}");
								}
							},
							Err(e) => unimplemented!("unhandled error {e}")
						}
					}
					_ = cancel_uni.cancelled() => {
						info!("Cancellation requested for unidirectional stream task");
						break;
					}
				}
			}
		});

		// Return the tuple of receivers for datagrams, bidirectional, and
		// unidirectional streams
		Ok((datagram_rx, bi_rx, uni_rx))
	}
}
