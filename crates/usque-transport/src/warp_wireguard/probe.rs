use super::*;
use crate::{
    NetworkQualityTelemetry, NoopSocketProtector, internal_network::InternalRequest,
    masque_runtime::MasqueRuntime, vpngate::GateDriver,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use boringtun::x25519::{PublicKey, StaticSecret};
use bytes::Bytes;
use http::Method;
use p256::elliptic_curve::Generate;
use serde_json::{Value, json};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use tokio::time::{Instant as TokioInstant, timeout};
use uuid::Uuid;
use zeroize::Zeroizing;

const API: &str = "https://api.cloudflareclient.com/v0a4471/reg";
const TRACE: &str = "https://www.cloudflare.com/cdn-cgi/trace";
const META: &str = "https://speed.cloudflare.com/meta";

pub(super) async fn observe_exit(network: &InternalNetwork) -> ExitObservation {
    let cancel = CancellationToken::new();
    let (ipv4, ipv6) = tokio::join!(
        observe(network, false, &cancel),
        observe(network, true, &cancel)
    );
    ExitObservation {
        checked_at: now(),
        ipv4,
        ipv6,
    }
}

async fn http(
    network: &InternalNetwork,
    request: InternalRequest<'_>,
    cancel: &CancellationToken,
) -> Result<Vec<u8>, ImportError> {
    tokio::select! {
        biased;
        _=cancel.cancelled()=>Err(error("cancelled")),
        result=timeout(Duration::from_secs(5),network.request_https(request,cancel))=>result.map_err(|_| error("probe_timeout"))?.map_err(|_| error("request_failed")),
    }
}
pub(super) async fn register(
    network: &InternalNetwork,
    cancel: &CancellationToken,
) -> Result<ImportSecrets, ImportError> {
    let private = StaticSecret::from(<[u8; 32]>::generate());
    let public = STANDARD.encode(PublicKey::from(&private).as_bytes());
    let body=json!({"key":public,"key_type":"curve25519","tunnel_type":"wireguard","tos":now(),"install_id":"","fcm_token":"","model":"Usque","serial_number":Uuid::new_v4().to_string(),"os_version":"","locale":"en_US"}).to_string();
    let headers = || {
        vec![
            ("Content-Type", "application/json"),
            ("User-Agent", "okhttp/3.12.1"),
            ("CF-Client-Version", "a-6.35-4471"),
        ]
    };
    let response = Zeroizing::new(
        http(
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
        )
        .await?,
    );
    let response: Value =
        serde_json::from_slice(&response).map_err(|_| error("registration_invalid"))?;
    let id = response["id"]
        .as_str()
        .filter(|s| {
            !s.is_empty()
                && s.len() <= 128
                && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
        .ok_or_else(|| error("registration_invalid"))?;
    let token = response["token"]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 4096)
        .ok_or_else(|| error("registration_invalid"))?;
    let peer = response["config"]["peers"][0]["public_key"]
        .as_str()
        .ok_or_else(|| error("registration_invalid"))?;
    let addresses = &response["config"]["interface"]["addresses"];
    let v4 = addresses["v4"]
        .as_str()
        .and_then(|s| s.parse::<Ipv4Addr>().ok())
        .ok_or_else(|| error("registration_invalid"))?;
    let v6 = addresses["v6"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(|s| s.parse::<Ipv6Addr>())
        .transpose()
        .map_err(|_| error("registration_invalid"))?;
    let endpoint = response["config"]["peers"][0]["endpoint"]["v4"]
        .as_str()
        .and_then(|s| s.parse::<std::net::SocketAddr>().ok())
        .map_or_else(|| "162.159.192.1:2408".into(), |s| s.to_string());
    let auth = Zeroizing::new(format!("Bearer {token}"));
    let mut patch_headers = headers();
    patch_headers.push(("Authorization", &auth));
    http(
        network,
        InternalRequest {
            url: &format!("{API}/{id}"),
            method: Method::PATCH,
            headers: patch_headers,
            body: Bytes::from_static(br#"{"warp_enabled":true}"#),
            limit: 64 * 1024,
            ipv6: None,
        },
        cancel,
    )
    .await?;
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
    ValidatedProfile::parse(ChainSource::WarpWireguard, &secrets)?;
    Ok(secrets)
}

fn field<'a>(body: &'a str, name: &str) -> Option<&'a str> {
    body.lines()
        .filter_map(|s| s.split_once('='))
        .find_map(|(key, value)| (key == name).then_some(value.trim()))
}
fn code(value: Option<&str>, length: usize) -> Option<String> {
    value
        .filter(|s| s.len() == length && s.bytes().all(|c| c.is_ascii_alphabetic()))
        .map(str::to_ascii_uppercase)
}
fn trace_ip(body: &[u8], ipv6: Option<bool>) -> Option<IpAddr> {
    let text = std::str::from_utf8(body).ok()?;
    let ip: IpAddr = field(text, "ip")?.parse().ok()?;
    (!ip.is_unspecified()
        && !ip.is_loopback()
        && !ip.is_multicast()
        && ipv6.is_none_or(|v6| ip.is_ipv6() == v6))
    .then_some(ip)
}
async fn trace(
    network: &InternalNetwork,
    ipv6: Option<bool>,
    cancel: &CancellationToken,
) -> Result<Vec<u8>, ImportError> {
    http(
        network,
        InternalRequest {
            url: TRACE,
            method: Method::GET,
            headers: vec![],
            body: Bytes::new(),
            limit: 4096,
            ipv6,
        },
        cancel,
    )
    .await
}
pub(super) async fn outer_address(
    network: &InternalNetwork,
    cancel: &CancellationToken,
) -> Option<String> {
    trace_ip(&trace(network, None, cancel).await.ok()?, None).map(|ip| ip.to_string())
}
async fn observe(
    network: &InternalNetwork,
    ipv6: bool,
    cancel: &CancellationToken,
) -> Option<Observation> {
    let start = Instant::now();
    let trace = trace(network, Some(ipv6), cancel).await.ok()?;
    let exit_ip = trace_ip(&trace, Some(ipv6))?;
    let response_ms = start.elapsed().as_millis().min(u64::MAX as u128) as u64;
    // A valid trace proves in-tunnel data. Metadata availability is independent.
    let meta = http(
        network,
        InternalRequest {
            url: META,
            method: Method::GET,
            headers: vec![("Referer", "https://speed.cloudflare.com")],
            body: Bytes::new(),
            limit: 4096,
            ipv6: Some(ipv6),
        },
        cancel,
    )
    .await
    .ok()
    .and_then(|b| serde_json::from_slice::<Value>(&b).ok());
    Some(Observation {
        exit_ip: Some(exit_ip),
        country: code(meta.as_ref().and_then(|m| m["country"].as_str()), 2),
        colo: code(meta.as_ref().and_then(|m| m["colo"]["iata"].as_str()), 3),
        response_ms: Some(response_ms),
    })
}
pub(super) async fn endpoint(
    outer: &InternalNetwork,
    profile: &Profile,
    secrets: &ImportSecrets,
    endpoint: Endpoint,
    index: usize,
    cancel: &CancellationToken,
) -> ProbeResult {
    let mut row = ProbeResult {
        index,
        endpoint: endpoint.clone(),
        checked_at: now(),
        ipv4: None,
        ipv6: None,
        failure: None,
    };
    let result = probe(outer, profile, secrets, &endpoint, cancel).await;
    match result {
        Ok((v4, v6)) => {
            row.ipv4 = v4;
            row.ipv6 = v6;
        }
        Err(failure) => row.failure = Some(failure.reason),
    }
    row
}
async fn probe(
    outer: &InternalNetwork,
    profile: &Profile,
    secrets: &ImportSecrets,
    endpoint: &Endpoint,
    cancel: &CancellationToken,
) -> Result<(Option<Observation>, Option<Observation>), ImportError> {
    let mut parsed = ValidatedProfile::parse(ChainSource::WarpWireguard, secrets)?;
    let ValidatedProfile::WireGuard(wg) = &mut parsed else {
        return Err(error("invalid_configuration"));
    };
    wg.endpoint = endpoint.clone();
    let v4 = wg.addresses.iter().any(|n| n.addr().is_ipv4());
    let v6 = wg.addresses.iter().any(|n| n.addr().is_ipv6());
    let mut summary = parsed.summary("WARP probe", Uuid::new_v4(), Uuid::new_v4())?;
    summary.source = ChainSource::WarpWireguard;
    let mut effective = DataPlaneRuntime::headless_profile(profile);
    effective.chain_exit = Some(summary.selection());
    effective.mtu = summary.mtu.unwrap_or(1280);
    effective.dns_servers = summary.dns_servers.clone();
    let prepared = usque_core::vpngate::PreparedProfile::imported(summary, parsed, secrets.clone());
    let started = TokioInstant::now();
    let (mut driver, tunnel, network) = GateDriver::start(
        &prepared,
        outer.clone(),
        NetworkQualityTelemetry::default(),
        None,
        cancel,
        started + Duration::from_secs(3),
    )
    .await
    .map_err(|_| error("handshake_failed"))?;
    let frontend = MasqueRuntime::start_over_tunnel(
        &effective,
        tunnel,
        (
            network.ipv4.unwrap_or(Ipv4Addr::UNSPECIFIED),
            network.ipv6.unwrap_or(Ipv6Addr::UNSPECIFIED),
        ),
        Arc::new(NoopSocketProtector),
        Arc::new(GeoDirectPolicy::disabled()),
    )
    .await;
    let mut frontend = match frontend {
        Ok(frontend) => frontend,
        Err(_) => {
            driver.shutdown().await;
            return Err(error("probe_failed"));
        }
    };
    driver.admit();
    let private = frontend.internal_network();
    let work = async {
        while !matches!(private.health_snapshot(), RuntimeHealth::Connected { .. }) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let (a, b) = tokio::join!(
            async {
                if v4 {
                    observe(&private, false, cancel).await
                } else {
                    None
                }
            },
            async {
                if v6 {
                    observe(&private, true, cancel).await
                } else {
                    None
                }
            }
        );
        if a.is_none() && b.is_none() {
            Err(error("no_tunnel_data"))
        } else {
            Ok((a, b))
        }
    };
    let result = tokio::select! {
        biased;
        _=cancel.cancelled()=>Err(error("cancelled")),
        result=tokio::time::timeout_at(started+Duration::from_secs(20),work)=>result.unwrap_or_else(|_| Err(error("probe_timeout"))),
    };
    frontend.shutdown().await;
    if !driver.shutdown().await {
        return Err(error("cleanup_pending"));
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn never_substitute_a_node_or_wrong_family_for_an_exit() {
        assert_eq!(
            trace_ip(b"ip=1.2.3.4\nloc=US\n", Some(false)),
            Some("1.2.3.4".parse().unwrap())
        );
        assert_eq!(trace_ip(b"ip=1.2.3.4\n", Some(true)), None);
        assert_eq!(trace_ip(b"ip=127.0.0.1\n", None), None);
        assert_eq!(code(Some("SG"), 2), Some("SG".into()));
        assert_eq!(code(Some("FRA"), 2), None);
    }
}
