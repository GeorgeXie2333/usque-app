use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use bytes::{Bytes, BytesMut};
use http::{Method, Request, Version};
use rustls::pki_types::ServerName;
use rustls::{ClientConfig, RootCertStore, crypto::CryptoProvider};
use tokio::net::TcpSocket;
use tokio::time::timeout;
use usque_core::{
    EndpointPinRefresh, PreparedEndpointPinRefresh, REGISTRATION_API_HOST, REGISTRATION_API_PORT,
    WarpIdentity, parse_endpoint_pin_refresh_response, prepare_endpoint_pin_refresh,
};

use crate::h2::{MasqueTlsIdentity, TransportError};
use crate::socket::{SocketProtector, socket_handle};

const CONTROL_CONNECT_TIMEOUT: Duration = Duration::from_secs(8);
const CONTROL_REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_CONTROL_RESPONSE_BYTES: usize = 1024 * 1024;

/// Persists an authenticated enrollment refresh before returning the new
/// transport identity. Returning an identity that was not durably stored
/// violates this contract.
#[async_trait]
pub trait EndpointPinRefresher: Send + Sync {
    async fn refresh(
        &self,
        protector: Arc<dyn SocketProtector>,
    ) -> Result<MasqueTlsIdentity, TransportError>;
}

impl MasqueTlsIdentity {
    pub fn from_warp_identity(identity: &WarpIdentity) -> Result<Self, TransportError> {
        Self::new(
            identity
                .key_pair
                .private_sec1_der()
                .map_err(|_| TransportError::InvalidPrivateKey)?,
            identity.endpoint_pin.spki_der(),
            identity.assigned_ipv4,
            identity.assigned_ipv6,
        )
    }
}

/// Refreshes a Consumer WARP enrollment over a socket protected by the
/// platform. TLS uses the operating system trust roots and strict hostname
/// verification; the untrusted response is parsed only after HTTP 200.
pub async fn refresh_endpoint_pin_over_protected_socket(
    identity: &WarpIdentity,
    device_name: Option<&str>,
    protector: Arc<dyn SocketProtector>,
) -> Result<EndpointPinRefresh, TransportError> {
    let request = prepare_endpoint_pin_refresh(identity, device_name)
        .map_err(|error| TransportError::EndpointPinRefresh(error.to_string()))?;
    let resolver = Arc::clone(&protector);
    let addresses = tokio::task::spawn_blocking(move || {
        resolver.resolve(REGISTRATION_API_HOST, REGISTRATION_API_PORT)
    })
    .await
    .map_err(|error| TransportError::EndpointPinRefresh(error.to_string()))?
    .map_err(TransportError::EndpointPinRefresh)?;

    let mut failures = Vec::new();
    for endpoint in prefer_ipv6_then_ipv4(addresses) {
        match refresh_at_endpoint(endpoint, &request, protector.as_ref()).await {
            Ok(refresh) => return Ok(refresh),
            Err(error) => failures.push(format!("{endpoint}: {error}")),
        }
    }
    Err(TransportError::EndpointPinRefresh(if failures.is_empty() {
        "registration API resolution returned no usable address".to_owned()
    } else {
        failures.join("; ")
    }))
}

async fn refresh_at_endpoint(
    endpoint: SocketAddr,
    request: &PreparedEndpointPinRefresh,
    protector: &dyn SocketProtector,
) -> Result<EndpointPinRefresh, TransportError> {
    let socket = if endpoint.is_ipv4() {
        TcpSocket::new_v4()
    } else {
        TcpSocket::new_v6()
    }
    .map_err(refresh_error)?;
    protector
        .protect(socket_handle(&socket))
        .map_err(TransportError::SocketProtection)?;
    let tcp = timeout(CONTROL_CONNECT_TIMEOUT, socket.connect(endpoint))
        .await
        .map_err(|_| {
            TransportError::EndpointPinRefresh(format!(
                "registration API connection to {endpoint} timed out"
            ))
        })?
        .map_err(refresh_error)?;
    tcp.set_nodelay(true).map_err(refresh_error)?;

    let mut configuration = endpoint_pin_tls_config()?;
    configuration.alpn_protocols = vec![b"h2".to_vec()];
    let connector = tokio_rustls::TlsConnector::from(Arc::new(configuration));
    let server_name = ServerName::try_from(REGISTRATION_API_HOST.to_owned())
        .map_err(|error| TransportError::EndpointPinRefresh(error.to_string()))?;
    let tls = timeout(CONTROL_CONNECT_TIMEOUT, connector.connect(server_name, tcp))
        .await
        .map_err(|_| {
            TransportError::EndpointPinRefresh(
                "registration API TLS handshake timed out".to_owned(),
            )
        })?
        .map_err(|error| TransportError::EndpointPinRefresh(error.to_string()))?;
    if tls.get_ref().1.alpn_protocol() != Some(b"h2") {
        return Err(TransportError::EndpointPinRefresh(
            "registration API did not negotiate HTTP/2".to_owned(),
        ));
    }

    let (mut sender, connection) = h2::client::handshake(tls).await.map_err(refresh_error)?;
    let driver = tokio::spawn(connection);
    let result = timeout(CONTROL_REQUEST_TIMEOUT, async {
        sender = sender.ready().await.map_err(refresh_error)?;
        let uri = format!(
            "https://{REGISTRATION_API_HOST}{}",
            request.path_and_query()
        );
        let http_request = Request::builder()
            .method(Method::PATCH)
            .version(Version::HTTP_2)
            .uri(uri)
            .header("user-agent", request.user_agent())
            .header("cf-client-version", request.client_version())
            .header("content-type", "application/json; charset=UTF-8")
            .header(
                "authorization",
                format!("Bearer {}", request.bearer_token()),
            )
            .header("content-length", request.body().len())
            .body(())
            .map_err(refresh_error)?;
        let (response, mut body) = sender
            .send_request(http_request, false)
            .map_err(refresh_error)?;
        body.send_data(Bytes::copy_from_slice(request.body()), true)
            .map_err(refresh_error)?;
        let response = response.await.map_err(refresh_error)?;
        let status = response.status();
        let mut body = response.into_body();
        let mut response_bytes = BytesMut::new();
        while let Some(chunk) = body.data().await {
            let chunk = chunk.map_err(refresh_error)?;
            if response_bytes.len().saturating_add(chunk.len()) > MAX_CONTROL_RESPONSE_BYTES {
                return Err(TransportError::EndpointPinRefresh(
                    "registration API returned more than 1 MiB".to_owned(),
                ));
            }
            response_bytes.extend_from_slice(&chunk);
            body.flow_control()
                .release_capacity(chunk.len())
                .map_err(refresh_error)?;
        }
        parse_endpoint_pin_refresh_response(status.as_u16(), &response_bytes)
            .map_err(|error| TransportError::EndpointPinRefresh(error.to_string()))
    })
    .await
    .map_err(|_| {
        TransportError::EndpointPinRefresh("registration API request timed out".to_owned())
    });
    driver.abort();
    result?
}

fn endpoint_pin_tls_config() -> Result<ClientConfig, TransportError> {
    // Keep this path independent of Rustls' process-global provider inference:
    // dependency feature unification must never turn pin refresh into a panic.
    endpoint_pin_tls_config_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
}

fn endpoint_pin_tls_config_with_provider(
    provider: Arc<CryptoProvider>,
) -> Result<ClientConfig, TransportError> {
    let mut roots = RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    Ok(ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(refresh_error)?
        .with_root_certificates(roots)
        .with_no_client_auth())
}

fn prefer_ipv6_then_ipv4(mut addresses: Vec<SocketAddr>) -> Vec<SocketAddr> {
    addresses.sort_by_key(|address| if address.is_ipv6() { 0 } else { 1 });
    addresses.dedup();
    addresses
}

fn refresh_error(error: impl std::fmt::Display) -> TransportError {
    TransportError::EndpointPinRefresh(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_pin_tls_config_uses_the_explicit_crypto_provider() {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let configuration = endpoint_pin_tls_config_with_provider(Arc::clone(&provider))
            .expect("ring supports the safe default TLS versions");

        assert!(Arc::ptr_eq(configuration.crypto_provider(), &provider));
    }

    #[test]
    fn registration_candidates_are_deduplicated_and_prefer_ipv6() {
        let addresses = vec![
            "192.0.2.1:443".parse().unwrap(),
            "[2001:db8::1]:443".parse().unwrap(),
            "192.0.2.1:443".parse().unwrap(),
        ];
        assert_eq!(
            prefer_ipv6_then_ipv4(addresses),
            vec![
                "[2001:db8::1]:443".parse().unwrap(),
                "192.0.2.1:443".parse().unwrap(),
            ]
        );
    }
}
