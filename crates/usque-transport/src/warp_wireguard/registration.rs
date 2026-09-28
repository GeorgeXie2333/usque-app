//! WireGuard enrollment follows wgcf ace873cbaa618365beebde5790a7fb3481e5a211
//! cloudflare/api.go (MIT). No Go runtime or generated API client is included.
use super::*;
use crate::internal_network::{InternalHttpError, InternalRequest};
use base64::{Engine, engine::general_purpose::STANDARD};
use boringtun::x25519::{PublicKey, StaticSecret};
use bytes::Bytes;
use http::Method;
use p256::elliptic_curve::Generate;
use serde_json::{Value, json};
use std::net::{Ipv4Addr, Ipv6Addr};
use tokio::time::timeout;
use zeroize::Zeroizing;

const API: &str = "https://api.cloudflareclient.com/v0a5641/reg";
// Enrollment is not an endpoint probe. In particular, it needs its own budget
// after a cold MASQUE/DNS/TLS startup on mobile networks.
const REGISTRATION_TIMEOUT: Duration = Duration::from_secs(15);

#[cfg(test)]
#[path = "registration_tests.rs"]
mod tests;

#[async_trait::async_trait]
trait RegistrationHttp: Send + Sync {
    async fn request(
        &self,
        request: InternalRequest<'_>,
        cancel: &CancellationToken,
    ) -> Result<Vec<u8>, InternalHttpError>;
}
#[async_trait::async_trait]
impl RegistrationHttp for InternalNetwork {
    async fn request(
        &self,
        request: InternalRequest<'_>,
        cancel: &CancellationToken,
    ) -> Result<Vec<u8>, InternalHttpError> {
        self.request_warp_registration(request, cancel).await
    }
}
pub(super) async fn register(
    network: &InternalNetwork,
    cancel: &CancellationToken,
) -> Result<ImportSecrets, ImportError> {
    register_with(network, cancel).await
}
async fn api(
    network: &dyn RegistrationHttp,
    request: InternalRequest<'_>,
    cancel: &CancellationToken,
    stage: &str,
) -> Result<Vec<u8>, ImportError> {
    let result = tokio::select! {
        biased;
        _ = cancel.cancelled() => Err(InternalHttpError::Cancelled),
        result = timeout(REGISTRATION_TIMEOUT, network.request(request, cancel)) => result.unwrap_or(Err(InternalHttpError::Timeout)),
    };
    result.map_err(|failure| error(&format!("registration_{stage}_{}", failure.code())))
}
async fn register_with(
    network: &dyn RegistrationHttp,
    cancel: &CancellationToken,
) -> Result<ImportSecrets, ImportError> {
    let private = StaticSecret::from(<[u8; 32]>::generate());
    let public = STANDARD.encode(PublicKey::from(&private).as_bytes());
    let body = json!({
        "key": public, "key_type": "curve25519", "tunnel_type": "wireguard",
        "fcm_token": "", "install_id": "", "serial_number": "", "locale": "en_US",
        "model": "PC", "os_version": "16.0.0", "tos": now(),
    })
    .to_string();
    let headers = || {
        vec![
            ("Content-Type", "application/json; charset=UTF-8"),
            ("User-Agent", "1.1.1.1/6.38.9-5641 (Android 16.0.0)"),
            ("CF-Client-Version", "a-6.38.9-5641"),
        ]
    };
    let response = Zeroizing::new(
        api(
            network,
            InternalRequest {
                url: API,
                method: Method::POST,
                headers: headers(),
                body: Bytes::from(body),
                limit: 64 * 1024,
                ipv6: None,
            },
            cancel,
            "create",
        )
        .await?,
    );
    let response: Value =
        serde_json::from_slice(&response).map_err(|_| error("registration_response_invalid"))?;
    let id = response["id"]
        .as_str()
        .filter(|s| {
            !s.is_empty()
                && s.len() <= 128
                && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
        .ok_or_else(|| error("registration_response_invalid"))?;
    let token = response["token"]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 4096)
        .ok_or_else(|| error("registration_response_invalid"))?;
    let auth = Zeroizing::new(format!("Bearer {token}"));
    let mut device_headers = headers();
    device_headers.push(("Authorization", &auth));
    // Registration may omit config. Like wgcf generate, fetch the source device
    // instead of assuming POST always returns a complete WireGuard profile.
    let device = Zeroizing::new(
        api(
            network,
            InternalRequest {
                url: &format!("{API}/{id}"),
                method: Method::GET,
                headers: device_headers,
                body: Bytes::new(),
                limit: 64 * 1024,
                ipv6: None,
            },
            cancel,
            "device",
        )
        .await?,
    );
    let device: Value =
        serde_json::from_slice(&device).map_err(|_| error("registration_response_invalid"))?;
    if device["id"].as_str() != Some(id)
        || device["key"].as_str().is_some_and(|key| key != public)
        || device["tunnel_type"]
            .as_str()
            .is_some_and(|kind| kind != "wireguard")
    {
        return Err(error("registration_response_invalid"));
    }
    let peer = device["config"]["peers"][0]["public_key"]
        .as_str()
        .ok_or_else(|| error("registration_response_invalid"))?;
    let addresses = &device["config"]["interface"]["addresses"];
    let v4 = addresses["v4"]
        .as_str()
        .and_then(|s| s.parse::<Ipv4Addr>().ok())
        .ok_or_else(|| error("registration_response_invalid"))?;
    let v6 = addresses["v6"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(|s| s.parse::<Ipv6Addr>())
        .transpose()
        .map_err(|_| error("registration_response_invalid"))?;
    let endpoint = registered_endpoint(&device["config"]["peers"][0]["endpoint"])?;
    let endpoint = if endpoint.host.contains(':') {
        format!("[{}]:{}", endpoint.host, endpoint.port)
    } else {
        format!("{}:{}", endpoint.host, endpoint.port)
    };
    let mut address = format!("{v4}/32");
    let mut allowed = "0.0.0.0/0".to_owned();
    if let Some(v6) = v6 {
        address.push_str(&format!(", {v6}/128"));
        allowed.push_str(", ::/0");
    }
    let configuration = format!(
        "[Interface]\nPrivateKey = {}\nAddress = {address}\nDNS = 1.1.1.1, 1.0.0.1\nMTU = 1280\n\n[Peer]\nPublicKey = {peer}\nEndpoint = {endpoint}\nAllowedIPs = {allowed}\nPersistentKeepalive = 25\n",
        STANDARD.encode(private.to_bytes())
    );
    let secrets = ImportSecrets::new(configuration);
    ValidatedProfile::parse(ChainSource::WarpWireguard, &secrets)
        .map_err(|_| error("registration_response_invalid"))?;
    Ok(secrets)
}

fn registered_endpoint(value: &Value) -> Result<Endpoint, ImportError> {
    let invalid = || error("registration_response_invalid");
    let (field, host) = ["host", "v4", "v6"]
        .iter()
        .find_map(|key| {
            value[key]
                .as_str()
                .filter(|host| !host.is_empty())
                .map(|host| (*key, host))
        })
        .ok_or_else(invalid)?;
    let port = match value["ports"].as_array().and_then(|ports| ports.first()) {
        Some(port) => port
            .as_u64()
            .and_then(|p| u16::try_from(p).ok())
            .filter(|p| *p != 0)
            .ok_or_else(invalid)?,
        None => 2408,
    };
    if let Ok(address) = host.parse::<std::net::SocketAddr>() {
        // API v4/v6 entries may carry a placeholder :0. wgcf strips that
        // port and uses endpoint.ports; only endpoint.host is authoritative
        // when it includes an explicit port.
        let port = if field == "host" {
            address.port()
        } else {
            port
        };
        return Endpoint::parse(&address.ip().to_string(), &port.to_string(), 0)
            .map_err(|_| invalid());
    }
    if host.parse::<std::net::IpAddr>().is_ok() || !host.contains(':') {
        return Endpoint::parse(host, &port.to_string(), 0).map_err(|_| invalid());
    }
    let (host, explicit_port) = host.rsplit_once(':').ok_or_else(invalid)?;
    let port = if field == "host" {
        explicit_port.to_owned()
    } else {
        port.to_string()
    };
    Endpoint::parse(host, &port, 0).map_err(|_| invalid())
}
