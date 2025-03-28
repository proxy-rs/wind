use rustls::crypto::CryptoProvider;

pub fn tls_config() -> rustls::ClientConfig {
   use rustls::ClientConfig;
   use rustls_platform_verifier::BuilderVerifierExt;

   let arc_crypto_provider =
      CryptoProvider::get_default().expect("Unable to find default crypto provider");
   let config = ClientConfig::builder_with_provider(arc_crypto_provider.clone())
      .with_protocol_versions(&[&rustls::version::TLS13])
      .unwrap()
      .with_platform_verifier()
      .with_no_client_auth();
   config
}
