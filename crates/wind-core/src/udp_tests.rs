#[cfg(test)]
mod tests {
	use core::slice;
	use std::{
		io::IoSliceMut,
		net::{IpAddr, Ipv4Addr, Ipv6Addr, UdpSocket},
	};
	use rand::RngCore;

	use quinn_udp::{RecvMeta, Transmit, UdpSocketState};
	use socket2::Socket;

	/// Generate a vector of random bytes with the specified length
	fn generate_random_data(len: usize) -> Vec<u8> {
		let mut data = vec![0u8; len];
		rand::rng().fill_bytes(&mut data);
		data
	}

	// Test with different data sizes
	fn test_with_data_size(data_len: usize) {
		let send = UdpSocket::bind((Ipv6Addr::LOCALHOST, 0))
			.or_else(|_| UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)))
			.unwrap();
		let recv = UdpSocket::bind((Ipv6Addr::LOCALHOST, 0))
			.or_else(|_| UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)))
			.unwrap();
		let dst_addr = recv.local_addr().unwrap();
		
		// Generate random data for testing
		let test_data = generate_random_data(data_len);
		
		test_send_recv(
			&send.into(),
			&recv.into(),
			Transmit {
				destination:  dst_addr,
				ecn:          None,
				contents:     &test_data,
				segment_size: None,
				src_ip:       None,
			},
		);
	}

	#[test]
	fn basic() {
		// Test with 1KB of data
		test_with_data_size(1024);
	}

	#[test]
	fn large_payload() {
		// Test with a larger payload (8KB)
		test_with_data_size(8 * 1024);
	}

	fn test_send_recv(send: &Socket, recv: &Socket, transmit: Transmit) {
		let send_state = UdpSocketState::new(send.into()).unwrap();
		let recv_state = UdpSocketState::new(recv.into()).unwrap();

		// Reverse non-blocking flag set by `UdpSocketState` to make the test non-racy
		recv.set_nonblocking(false).unwrap();

		send_state.try_send(send.into(), &transmit).unwrap();

		let mut buf = [0; u16::MAX as usize];
		let mut meta = RecvMeta::default();
		let segment_size = transmit.segment_size.unwrap_or(transmit.contents.len());
		let expected_datagrams = transmit.contents.len() / segment_size;
		let mut datagrams = 0;
		while datagrams < expected_datagrams {
			let n = recv_state
				.recv(
					recv.into(),
					&mut [IoSliceMut::new(&mut buf)],
					slice::from_mut(&mut meta),
				)
				.unwrap();
			assert_eq!(n, 1);
			let segments = meta.len / meta.stride;
			for i in 0..segments {
				assert_eq!(
					&buf[(i * meta.stride)..((i + 1) * meta.stride)],
					&transmit.contents
						[(datagrams + i) * segment_size..(datagrams + i + 1) * segment_size]
				);
			}
			datagrams += segments;

			assert_eq!(
				meta.addr.port(),
				send.local_addr().unwrap().as_socket().unwrap().port()
			);
			let send_v6 = send.local_addr().unwrap().as_socket().unwrap().is_ipv6();
			let recv_v6 = recv.local_addr().unwrap().as_socket().unwrap().is_ipv6();
			let mut addresses = vec![meta.addr.ip()];
			// Not populated on every OS. See `RecvMeta::dst_ip` for details.
			if let Some(addr) = meta.dst_ip {
				addresses.push(addr);
			}
			for addr in addresses {
				match (send_v6, recv_v6) {
					(_, false) => assert_eq!(addr, Ipv4Addr::LOCALHOST),
					// Windows gives us real IPv4 addrs, whereas *nix use IPv6-mapped IPv4
					// addrs. Canonicalize to IPv6-mapped for robustness.
					(false, true) => {
						assert_eq!(ip_to_v6_mapped(addr), Ipv4Addr::LOCALHOST.to_ipv6_mapped())
					}
					(true, true) => assert!(
						addr == Ipv6Addr::LOCALHOST || addr == Ipv4Addr::LOCALHOST.to_ipv6_mapped()
					),
				}
			}

			let ipv4_or_ipv4_mapped_ipv6 = match transmit.destination.ip() {
				IpAddr::V4(_) => true,
				IpAddr::V6(a) => a.to_ipv4_mapped().is_some(),
			};

			// On Android API level <= 25 the IPv4 `IP_TOS` control message is
			// not supported and thus ECN bits can not be received.
			if ipv4_or_ipv4_mapped_ipv6
				&& cfg!(target_os = "android")
				&& std::env::var("API_LEVEL")
					.ok()
					.and_then(|v| v.parse::<u32>().ok())
					.expect("API_LEVEL environment variable to be set on Android")
					<= 25
			{
				assert_eq!(meta.ecn, None);
			} else {
				assert_eq!(meta.ecn, transmit.ecn);
			}
		}
		assert_eq!(datagrams, expected_datagrams);
	}

	fn ip_to_v6_mapped(x: IpAddr) -> IpAddr {
		match x {
			IpAddr::V4(x) => IpAddr::V6(x.to_ipv6_mapped()),
			IpAddr::V6(_) => x,
		}
	}
}
