use std::{
   net::{Ipv4Addr, SocketAddr},
   sync::Arc,
   time::Duration,
};

use quinn::TokioRuntime;
use snafu::ResultExt;
use tokio::{net::UdpSocket, task::JoinHandle};
use uuid::Uuid;
use wind_core::{AbstractOutbound, AbstractTcpStream};

use crate::{BindSocketSnafu, Error, QuicConnectSnafu, proto::TuicClientConnection as _};

pub struct TuicOutboundOpts {
   // TODO, it's not safe
   auth:                   (Uuid, Arc<[u8]>),
   pub zero_rtt_handshake: bool,
   pub heartbeat:          Duration,
   pub gc_interval:        Duration,
   pub gc_lifetime:        Duration,
   pub skip_cert_verify:   bool,
   pub alpn:               Vec<String>,
}

pub struct TuicOutbound {
   pub endpoint:    quinn::Endpoint,
   pub peer_addr:   SocketAddr,
   pub server_name: String,
   pub opts:        TuicOutboundOpts,
   pub connection:  quinn::Connection,
   handle:          Option<JoinHandle<Result<(), Error>>>,
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
         let tls_config = super::tls::tls_config(&server_name, &opts)?;

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
      let connection = endpoint
         .connect(peer_addr, &server_name)
         .context(QuicConnectSnafu {
            addr:        peer_addr,
            server_name: server_name.clone(),
         })?
         .await?;
      connection.send_auth(&opts.auth.0, &opts.auth.1).await?;

      Ok(Self {
         endpoint,
         peer_addr,
         server_name,
         opts,
         handle: None,
         connection,
      })
   }

   pub async fn start(&mut self, rt: tokio::runtime::Runtime) {
      let handle: JoinHandle<Result<(), Error>> = rt.spawn(async move { Ok(()) });
      self.handle = Some(handle)
   }
}

pub struct TuicTcpStream;

impl AbstractOutbound for TuicOutbound {
   async fn handle_tcp(
      self: &Self,
      stream: impl AbstractTcpStream,
      _dialer: Option<impl AbstractOutbound>,
   ) -> impl AbstractTcpStream {
      let _ = _dialer;
      TuicTcpStream
   }
}
