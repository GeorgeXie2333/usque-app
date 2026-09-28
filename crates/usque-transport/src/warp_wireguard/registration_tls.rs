//! Cloudflare Android API TLS profile, referenced from wgcf
//! ace873cbaa618365beebde5790a7fb3481e5a211/cloudflare/api.go (MIT).
//! Reuses BoringSSL already linked for MASQUE. WebPKI still verifies trust,
//! certificate validity, server usage and the fixed API hostname.
use crate::internal_network::InternalHttpError;
use boring::ssl::{
    Ssl, SslAlert, SslContext, SslContextBuilder, SslMethod, SslOptions,
    SslSignatureAlgorithm as Sig, SslVerifyError, SslVerifyMode, SslVersion,
};
use rustls::client::danger::ServerCertVerifier;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{RootCertStore, client::WebPkiServerVerifier};
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncWrite};

pub(crate) const HOST: &str = "api.cloudflareclient.com";

pub(crate) async fn connect<S>(stream: S) -> Result<tokio_boring::SslStream<S>, InternalHttpError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let roots = RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let context = context(roots)?;
    connect_with_context(stream, &context).await
}

async fn connect_with_context<S>(
    stream: S,
    context: &SslContext,
) -> Result<tokio_boring::SslStream<S>, InternalHttpError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut ssl = Ssl::new(context).map_err(|_| InternalHttpError::Tls)?;
    ssl.set_hostname(HOST).map_err(|_| InternalHttpError::Tls)?;
    tokio_boring::SslStreamBuilder::new(ssl, stream)
        .connect()
        .await
        .map_err(|_| InternalHttpError::Tls)
}

fn context(roots: RootCertStore) -> Result<SslContext, InternalHttpError> {
    let verifier = WebPkiServerVerifier::builder_with_provider(
        Arc::new(roots),
        Arc::new(rustls::crypto::ring::default_provider()),
    )
    .build()
    .map_err(|_| InternalHttpError::Tls)?;
    let mut builder =
        SslContextBuilder::new(SslMethod::tls_client()).map_err(|_| InternalHttpError::Tls)?;
    builder.set_options(SslOptions::NO_COMPRESSION | SslOptions::NO_RENEGOTIATION);
    builder
        .set_min_proto_version(Some(SslVersion::TLS1_2))
        .map_err(|_| InternalHttpError::Tls)?;
    builder
        .set_max_proto_version(Some(SslVersion::TLS1_2))
        .map_err(|_| InternalHttpError::Tls)?;
    builder
        .set_cipher_list("ECDHE-ECDSA-AES256-GCM-SHA384:ECDHE-RSA-AES256-GCM-SHA384")
        .map_err(|_| InternalHttpError::Tls)?;
    builder
        .set_curves_list("X25519:P-256:P-384")
        .map_err(|_| InternalHttpError::Tls)?;
    builder
        .set_verify_algorithm_prefs(&[
            Sig::ECDSA_SECP256R1_SHA256,
            Sig::RSA_PSS_RSAE_SHA256,
            Sig::RSA_PKCS1_SHA256,
            Sig::ECDSA_SECP384R1_SHA384,
            Sig::RSA_PSS_RSAE_SHA384,
            Sig::RSA_PKCS1_SHA384,
            Sig::RSA_PSS_RSAE_SHA512,
            Sig::RSA_PKCS1_SHA512,
            Sig::RSA_PKCS1_SHA1,
        ])
        .map_err(|_| InternalHttpError::Tls)?;
    builder.set_grease_enabled(false);
    builder.set_permute_extensions(false);
    builder.enable_ocsp_stapling();
    builder
        .set_alpn_protos(b"\x08http/1.1")
        .map_err(|_| InternalHttpError::Tls)?;
    builder.set_custom_verify_callback(SslVerifyMode::PEER, move |ssl| {
        let verify = || -> Option<()> {
            let chain = ssl.peer_cert_chain()?;
            let certificates = chain
                .iter()
                .map(|cert| cert.to_der().map(CertificateDer::from))
                .collect::<Result<Vec<_>, _>>()
                .ok()?;
            let (leaf, intermediates) = certificates.split_first()?;
            let name = ServerName::try_from(HOST).ok()?;
            verifier
                .verify_server_cert(
                    leaf,
                    intermediates,
                    &name,
                    ssl.ocsp_status().unwrap_or_default(),
                    UnixTime::now(),
                )
                .ok()?;
            Some(())
        };
        verify().ok_or(SslVerifyError::Invalid(SslAlert::BAD_CERTIFICATE))
    });
    Ok(builder.build())
}

#[cfg(test)]
#[path = "registration_tls_tests.rs"]
mod tests;
