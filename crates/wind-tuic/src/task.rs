use std::time::Duration;

use tokio_util::sync::CancellationToken;
use wind_core::info;

use crate::Error;

const SPSC_BUFFER_SIZE: usize = 8;

pub trait ClientTaskExt {
	async fn handle_incoming(&self, cancel_token: CancellationToken) -> Result<(), Error>;
}

impl ClientTaskExt for quinn::Connection {
	async fn handle_incoming(&self, cancel_token: CancellationToken) -> Result<(), Error> {
		// Create channels for datagrams
		let (datagram_tx, _datagram_rx) = crossfire::spsc::bounded_async(SPSC_BUFFER_SIZE);
		let conn_datagram = self.clone();
		let cancel_datagram = cancel_token.clone();

		// Create channels for bidirectional streams
		let (bi_tx, _bi_rx) = crossfire::spsc::bounded_async(SPSC_BUFFER_SIZE);
		let conn_bi = self.clone();
		let cancel_bi = cancel_token.clone();

		// Create channels for unidirectional streams
		let (uni_tx, _uni_rx) = crossfire::spsc::bounded_async(SPSC_BUFFER_SIZE);
		let conn_uni = self.clone();
		let cancel_uni = cancel_token.clone();

		// Spawn task for handling datagrams
		tokio::spawn(async move {
			loop {
				tokio::select! {
					res = conn_datagram.read_datagram() => {
						match res {
							Ok(datagram) => {
								info!("Received datagram: {:?}", &datagram);
								if let Err(e) = datagram_tx.send_timeout(datagram, Duration::from_secs(1)).await {
									unimplemented!("unhandled error {e:?}");
								}
							},
							Err(e) => unimplemented!("unhandled error {e:?}")
						}
					}
					_ = cancel_datagram.cancelled() => {
						info!("Cancellation requested for datagram task");
						break;
					}
				}
			}
		});

		// Spawn task for handling bidirectional streams
		tokio::spawn(async move {
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
		tokio::spawn(async move {
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

		// The function now directly handles the streams and doesn't need to return the
		// receivers
		Ok(())
	}
}
