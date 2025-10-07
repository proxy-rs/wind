use std::sync::Arc;

use crossfire::AsyncRx;
use wind_core::udp::UdpPacket;

use crate::proto::ClientProtoExt as _;


pub struct UdpStream {
	connection: Arc<quinn::Connection>,
	assoc_id:   u16,
	rx:         AsyncRx<UdpPacket>,
}

impl UdpStream {
	pub fn new(connection: Arc<quinn::Connection>, assoc_id: u16, rx: AsyncRx<UdpPacket>) -> Self {
		Self {
			connection,
			assoc_id,
			rx,
		}
	}

	pub async fn send_packet(&mut self, packet: UdpPacket) -> eyre::Result<()> {
		// Send UDP data with association ID
		self.connection
			.send_udp(self.assoc_id, 0, &packet.target, packet.payload, false)
			.await?;
		Ok(())
	}

	pub async fn recv_packet(&mut self) -> eyre::Result<UdpPacket> {
		Ok(self.rx.recv().await?)
	}

	pub fn close(&mut self) -> eyre::Result<()> {
		todo!();
		// Close the UDP association
	}
}
