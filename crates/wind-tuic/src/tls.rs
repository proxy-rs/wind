use rustls::crypto::CryptoProvider;

use crate::Error;

pub(crate) fn tls_config() -> Result<rustls::ClientConfig, Error> {
   use rustls::ClientConfig;
   use rustls_platform_verifier::BuilderVerifierExt;

   let arc_crypto_provider =
      CryptoProvider::get_default().expect("Unable to find default crypto provider");
   let config = ClientConfig::builder_with_provider(arc_crypto_provider.clone())
      .with_protocol_versions(&[&rustls::version::TLS13])?
      .with_platform_verifier()?
      .with_no_client_auth();
   Ok(config)
}
