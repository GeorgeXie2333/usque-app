use super::*;
use crate::{
    NetworkQualityTelemetry, NoopSocketProtector, internal_network::InternalRequest,
    masque_runtime::MasqueRuntime, vpngate::GateDriver,
};
use bytes::Bytes;
use http::Method;
use serde_json::Value;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use tokio::time::{Instant as TokioInstant, timeout};
use uuid::Uuid;

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
        result=timeout(Duration::from_secs(5),network.request_https(request,cancel))=>result.map_err(|_| error("probe_timeout"))?.map_err(|failure| error(&format!("probe_{}", failure.code()))),
    }
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
