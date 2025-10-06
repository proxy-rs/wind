use std::{
	fmt::Debug,
	io::IoSliceMut,
	net::SocketAddr,
	pin::Pin,
	sync::Arc,
	task::{Context, Poll, ready},
};

#[cfg(feature = "quic")]
pub use quinn::UdpPoller;
pub use quinn_udp::*;
use tokio::io::Interest;

pub trait AbstractUdpSocket: Send + Sync {
	fn create_io_poller(self: Arc<Self>) -> Pin<Box<dyn UdpPoller>>;

	fn try_send(&self, transmit: &Transmit) -> std::io::Result<()>;

	fn poll_recv(
		&self,
		cx: &mut Context,
		bufs: &mut [IoSliceMut<'_>],
		meta: &mut [RecvMeta],
	) -> Poll<std::io::Result<usize>>;

	fn local_addr(&self) -> std::io::Result<SocketAddr>;

	fn max_transmit_segments(&self) -> usize {
		1
	}

	fn max_receive_segments(&self) -> usize {
		1
	}

	fn may_fragment(&self) -> bool {
		true
	}
}

#[cfg(not(feature = "quic"))]
pub trait UdpPoller: Send + Sync + Debug + 'static {
	fn poll_writable(self: Pin<&mut Self>, cx: &mut Context) -> Poll<std::io::Result<()>>;
}

#[derive(Debug)]
pub struct TokioUdpSocket {
	io:    tokio::net::UdpSocket,
	inner: UdpSocketState,
}
impl TokioUdpSocket {
	pub fn new(sock: std::net::UdpSocket) -> std::io::Result<Self> {
		Ok(Self {
			inner: UdpSocketState::new((&sock).into())?,
			io:    tokio::net::UdpSocket::from_std(sock)?,
		})
	}
}
impl AbstractUdpSocket for TokioUdpSocket {
	fn create_io_poller(self: Arc<Self>) -> Pin<Box<dyn UdpPoller>> {
		Box::pin(UdpPollHelper::new(move || {
			let socket = self.clone();
			async move { socket.io.writable().await }
		}))
	}

	fn try_send(&self, transmit: &Transmit) -> std::io::Result<()> {
		self.io.try_io(Interest::WRITABLE, || {
			self.inner.send((&self.io).into(), transmit)
		})
	}

	fn poll_recv(
		&self,
		cx: &mut Context,
		bufs: &mut [std::io::IoSliceMut<'_>],
		meta: &mut [RecvMeta],
	) -> Poll<std::io::Result<usize>> {
		loop {
			ready!(self.io.poll_recv_ready(cx))?;
			if let Ok(res) = self.io.try_io(Interest::READABLE, || {
				self.inner.recv((&self.io).into(), bufs, meta)
			}) {
				return Poll::Ready(Ok(res));
			}
		}
	}

	fn local_addr(&self) -> std::io::Result<std::net::SocketAddr> {
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

pin_project_lite::pin_project! {
	struct UdpPollHelper<MakeFut, Fut> {
		make_fut: MakeFut,
		#[pin]
		fut: Option<Fut>,
	}
}

impl<MakeFut, Fut> UdpPollHelper<MakeFut, Fut> {
	fn new(make_fut: MakeFut) -> Self {
		Self {
			make_fut,
			fut: None,
		}
	}
}

impl<MakeFut, Fut> UdpPoller for UdpPollHelper<MakeFut, Fut>
where
	MakeFut: Fn() -> Fut + Send + Sync + 'static,
	Fut: Future<Output = std::io::Result<()>> + Send + Sync + 'static,
{
	fn poll_writable(self: Pin<&mut Self>, cx: &mut Context) -> Poll<std::io::Result<()>> {
		let mut this = self.project();
		if this.fut.is_none() {
			this.fut.set(Some((this.make_fut)()));
		}
		let result = this.fut.as_mut().as_pin_mut().unwrap().poll(cx);
		if result.is_ready() {
			this.fut.set(None);
		}
		result
	}
}

impl<MakeFut, Fut> Debug for UdpPollHelper<MakeFut, Fut> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("UdpPollHelper").finish_non_exhaustive()
	}
}
