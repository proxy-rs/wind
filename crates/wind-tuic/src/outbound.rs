use std::{
   net::{Ipv4Addr, SocketAddr},
   sync::Arc,
   time::Duration,
};

use quinn::TokioRuntime;
use snafu::ResultExt;
use tokio::net::UdpSocket;
use uuid::Uuid;
use wind_core::{AbstractOutbound, AbstractTcpStream};

use crate::{BindSocketSnafu, Error};

pub struct TuicOutboundOpts {
   // TODO, it's not safe
   auth:                   (Uuid, Arc<[u8]>),
   pub zero_rtt_handshake: bool,
   pub heartbeat:          Duration,
   pub gc_interval:        Duration,
   pub gc_lifetime:        Duration,
}

pub struct TuicOutbound {
   pub endpoint:    quinn::Endpoint,
   pub peer_addr:   SocketAddr,
   pub server_name: String,
   pub opts:        TuicOutboundOpts,
}

impl TuicOutbound {
   pub async fn new(
      peer_addr: SocketAddr,
      server_name: String,
      opts: TuicOutboundOpts,
   ) -> Result<Self, Error> {
      // TODO
      {
         #[cfg(feature = "aws-lc-rs")]
         rustls::crypto::aws_lc_rs::default_provider()
            .install_default()
            .unwrap();
         #[cfg(feature = "ring")]
         rustls::crypto::ring::default_provider()
            .install_default()
            .unwrap();
      }
      let client_config = {
         let mut tls_config = super::tls::tls_config();

         tls_config.alpn_protocols = vec![String::from("h3")]
            .into_iter()
            .map(|alpn| alpn.into_bytes())
            .collect();
         let mut client_config = quinn::ClientConfig::new(Arc::new(
            quinn::crypto::rustls::QuicClientConfig::try_from(tls_config).unwrap(),
         ));
         let mut transport_config = quinn::TransportConfig::default();
         transport_config
            .congestion_controller_factory(Arc::new(quinn::congestion::BbrConfig::default()));
         client_config.transport_config(Arc::new(transport_config));
         client_config
      };
      let socket_addr = SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0));
      let socket = UdpSocket::bind(&socket_addr)
         .await
         .context(BindSocketSnafu { socket_addr })?
         .into_std()?;

      let mut endpoint = quinn::Endpoint::new(
         quinn::EndpointConfig::default(),
         None,
         socket,
         Arc::new(TokioRuntime),
      )?;
      endpoint.set_default_client_config(client_config);
      Ok(Self {
         endpoint,
         peer_addr,
         server_name,
         opts,
      })
   }
}

pub struct TuicTcpStream;

impl AbstractOutbound for TuicOutbound {
   async fn handle_tcp(_dialer: Option<impl AbstractOutbound>) -> impl AbstractTcpStream {
      TuicTcpStream
   }
}
