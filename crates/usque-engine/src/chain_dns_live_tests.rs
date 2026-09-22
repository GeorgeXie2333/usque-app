//! Explicitly opted-in loopback SOCKS tests. No Agent, TUN or host networking changes.
use super::*;
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;
use usque_core::chain_exit::{ChainSource, ImportSecrets, ValidatedProfile};

fn address(address: SocketAddr) -> Vec<u8> {
    let mut bytes = Vec::new();
    match address.ip() {
        IpAddr::V4(ip) => {
            bytes.push(1);
            bytes.extend_from_slice(&ip.octets());
        }
        IpAddr::V6(ip) => {
            bytes.push(4);
            bytes.extend_from_slice(&ip.octets());
        }
    }
    bytes.extend_from_slice(&address.port().to_be_bytes());
    bytes
}

async fn socks(
    proxy: SocketAddr,
    command: u8,
    target: &[u8],
) -> Result<(tokio::net::TcpStream, SocketAddr), String> {
    let mut stream = tokio::net::TcpStream::connect(proxy)
        .await
        .map_err(|_| "loopback connect")?;
    stream
        .write_all(&[5, 1, 0])
        .await
        .map_err(|_| "greeting write")?;
    let mut header = [0; 2];
    stream
        .read_exact(&mut header)
        .await
        .map_err(|_| "greeting read")?;
    if header != [5, 0] {
        return Err("SOCKS authentication rejected".into());
    }
    let mut request = vec![5, command, 0];
    request.extend_from_slice(target);
    stream
        .write_all(&request)
        .await
        .map_err(|_| "request write")?;
    let mut header = [0; 4];
    stream
        .read_exact(&mut header)
        .await
        .map_err(|_| "reply read")?;
    if header[1] != 0 {
        return Err(format!("SOCKS status {}", header[1]));
    }
    let ip = match header[3] {
        1 => {
            let mut b = [0; 4];
            stream.read_exact(&mut b).await.map_err(|_| "IPv4 reply")?;
            IpAddr::from(b)
        }
        4 => {
            let mut b = [0; 16];
            stream.read_exact(&mut b).await.map_err(|_| "IPv6 reply")?;
            IpAddr::from(b)
        }
        _ => return Err("unexpected SOCKS address type".into()),
    };
    let port = stream.read_u16().await.map_err(|_| "reply port")?;
    Ok((stream, SocketAddr::new(ip, port)))
}

fn query(id: u16, name: &str, kind: u16) -> Vec<u8> {
    let mut bytes = Vec::from(id.to_be_bytes());
    bytes.extend_from_slice(&[1, 0, 0, 1, 0, 0, 0, 0, 0, 0]);
    for label in name.split('.') {
        bytes.push(label.len() as u8);
        bytes.extend_from_slice(label.as_bytes());
    }
    bytes.push(0);
    bytes.extend_from_slice(&kind.to_be_bytes());
    bytes.extend_from_slice(&1u16.to_be_bytes());
    bytes
}

async fn sample_dns(proxy: SocketAddr, servers: &[IpAddr]) -> bool {
    let mut valid = 0;
    let mut domain_connected = 0;
    for (server_index, ip) in servers.iter().enumerate() {
        let server = SocketAddr::new(*ip, 53);
        for tcp in [false, true] {
            let setup_start = Instant::now();
            let socket = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
            let target = if tcp {
                server
            } else {
                socket.local_addr().unwrap()
            };
            let setup = tokio::time::timeout(
                Duration::from_secs(4),
                socks(proxy, if tcp { 1 } else { 3 }, &address(target)),
            )
            .await;
            let Ok(Ok((mut stream, relay))) = setup else {
                eprintln!(
                    "DNS_LIVE {{\"server\":{server_index},\"tcp\":{tcp},\"stage\":\"connect_failed\"}}"
                );
                continue;
            };
            let connect_us = setup_start.elapsed().as_micros();
            for round in 0..5 {
                let domain = [
                    "example.com",
                    "example.org",
                    "cloudflare.com",
                    "wikipedia.org",
                    "github.com",
                ][round];
                let request = query(
                    (round + 1) as u16,
                    domain,
                    if round % 2 == 0 { 1 } else { 28 },
                );
                let started = Instant::now();
                let result = tokio::time::timeout(Duration::from_secs(4), async {
                    let response = if tcp {
                        stream
                            .write_u16(request.len() as u16)
                            .await
                            .map_err(|_| "write")?;
                        stream.write_all(&request).await.map_err(|_| "write")?;
                        let length = stream.read_u16().await.map_err(|_| "read")?;
                        let mut response = vec![0; usize::from(length)];
                        stream.read_exact(&mut response).await.map_err(|_| "read")?;
                        response
                    } else {
                        let mut wrapped = vec![0, 0, 0];
                        wrapped.extend(address(server));
                        wrapped.extend(&request);
                        socket
                            .send_to(&wrapped, relay)
                            .await
                            .map_err(|_| "UDP send")?;
                        let mut response = vec![0; 65535];
                        let (length, source) = socket
                            .recv_from(&mut response)
                            .await
                            .map_err(|_| "UDP receive")?;
                        if source != relay || length < 10 {
                            return Err("UDP response source");
                        }
                        let header = match response[3] {
                            1 => 10,
                            4 => 22,
                            _ => return Err("UDP address"),
                        };
                        response.get(header..length).ok_or("UDP length")?.to_vec()
                    };
                    if response.len() < 12
                        || response[..2] != request[..2]
                        || response[2] & 0x80 == 0
                    {
                        return Err("DNS response");
                    }
                    Ok::<_, &str>(response[3] & 15)
                })
                .await;
                let rcode = result.as_ref().ok().and_then(|v| v.as_ref().ok()).copied();
                valid += usize::from(matches!(rcode, Some(0 | 3)));
                eprintln!(
                    "DNS_LIVE {}",
                    serde_json::json!({"server":server_index,"tcp":tcp,"round":round,
                    "connect_us":connect_us,"query_us":started.elapsed().as_micros(),"rcode":rcode,"ok":matches!(rcode, Some(0 | 3))})
                );
                // Do not reuse a stream after timeout/partial DNS framing.
                if tcp && rcode.is_none() {
                    break;
                }
            }
        }
    }
    for round in 0..5 {
        let host = b"example.com";
        let mut target = vec![3, host.len() as u8];
        target.extend_from_slice(host);
        target.extend_from_slice(&443u16.to_be_bytes());
        let started = Instant::now();
        let result = tokio::time::timeout(Duration::from_secs(8), socks(proxy, 1, &target)).await;
        domain_connected += usize::from(matches!(result, Ok(Ok(_))));
        eprintln!(
            "DNS_LIVE {}",
            serde_json::json!({"mode":"application_domain_connect","round":round,
            "elapsed_us":started.elapsed().as_micros(),"ok":matches!(result,Ok(Ok(_)))})
        );
    }
    valid > 0 && domain_connected > 0
}

#[tokio::test]
#[ignore = "explicit USQUE_LIVE_CONFIG and USQUE_LIVE_WIREGUARD; loopback SOCKS only, never TUN"]
async fn live_wireguard_dns_through_loopback_socks_without_tun() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("usque_transport::vpngate=info")
        .with_ansi(false)
        .with_writer(std::io::stderr)
        .try_init();
    use usque_core::chain_exit::store::{ChainProfileStore, WindowsProfileCipher};
    let source = std::env::var_os("USQUE_LIVE_CONFIG").expect("explicit enrolled config path");
    let wg_file =
        std::env::var_os("USQUE_LIVE_WIREGUARD").expect("explicit temporary WireGuard path");
    let directory = tempfile::tempdir().unwrap();
    let staged = directory.path().join("config.json");
    std::fs::copy(source, &staged).unwrap();
    let service = ControlService::open(ConfigStore::new(&staged)).unwrap();
    let mut profile = service.config_snapshot().await.active_profile().unwrap();
    if let Ok(transport) = std::env::var("USQUE_LIVE_TRANSPORT") {
        profile.transport = match transport.as_str() {
            "h3" => TransportPolicy::Http3,
            "h2" => TransportPolicy::Http2,
            _ => panic!("test transport must be h3 or h2"),
        };
    }
    let warp_only = std::env::var("USQUE_LIVE_WARP_ONLY").is_ok_and(|v| v == "1");
    let text = ImportSecrets::new(std::fs::read_to_string(wg_file).unwrap());
    let parsed = ValidatedProfile::parse(ChainSource::WireguardCustom, &text).unwrap();
    let ValidatedProfile::WireGuard(wg) = parsed else {
        unreachable!()
    };
    let servers = wg.dns_servers;
    let library = ChainProfileStore::new(directory.path(), &WindowsProfileCipher);
    let summary = library
        .import(ChainSource::WireguardCustom, "Temporary DNS test", text)
        .unwrap();
    profile.disable_chain();
    profile.chain_exit = Some(summary.selection());
    profile.data_plane = usque_core::DataPlaneMode::ConnectIp;
    profile.frontends = FrontendSettings {
        tunnel: false,
        socks5: true,
        http: false,
    };
    profile.proxy = ProxySettings::default();
    let reservation = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let proxy = reservation.local_addr().unwrap();
    assert!(proxy.ip().is_loopback() && proxy.port() > 1024);
    profile.proxy.socks5_listeners = vec![proxy];
    drop(reservation);
    profile.proxy.system_proxy = false;
    profile.kill_switch = false;
    profile.geo_direct_countries.clear();
    profile.split_exclusions.clear();
    profile.canonicalize_mode();
    assert!(!profile.frontends.tunnel && !profile.proxy.system_proxy && !profile.kill_switch);
    let selected = usque_core::chain_exit::prepare_selection(
        directory.path(),
        &profile,
        &WindowsProfileCipher,
    )
    .unwrap();
    let identity = service.load_warp_identity(profile.id).await.unwrap();
    let tls = MasqueTlsIdentity::from_warp_identity(&identity).unwrap();
    let refresher = Arc::new(VaultEndpointPinRefresher {
        profile_id: profile.id,
        vault: Arc::new(crate::tests::MemoryVault::default()),
        identity: Mutex::new(identity),
    });
    let cancel = CancellationToken::new();
    let (status, mut observed) = watch::channel(usque_core::vpngate::GateStatus::default());
    let _observer = AbortOnDropHandle::new(tokio::spawn(async move {
        while observed.changed().await.is_ok() {
            let s = observed.borrow_and_update();
            eprintln!(
                "DNS_LIVE {}",
                serde_json::json!({"phase":format!("{:?}",s.stage),
                "failure":s.failure.map(|f|format!("{f:?}")),"warp_phase":s.warp_stage})
            );
        }
    }));
    if warp_only {
        profile.disable_chain();
    }
    eprintln!(
        "DNS_LIVE {}",
        serde_json::json!({"warp_only":warp_only,"transport":format!("{:?}",profile.transport)})
    );
    let started = Instant::now();
    let result = usque_transport::DataPlaneRuntime::start_with_vpngate(
        &profile,
        tls,
        Arc::new(NoopSocketProtector),
        Some(refresher),
        Arc::new(GeoDirectPolicy::disabled()),
        usque_transport::VpnGateStart {
            selected,
            status: Some(status),
            cancellation: cancel.clone(),
            deadline: Some(tokio::time::Instant::now() + Duration::from_secs(120)),
        },
    )
    .await;
    let mut runtime = match result {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("DNS_LIVE startup_failed {error}");
            panic!("proxy-only startup failed");
        }
    };
    let activated = runtime.activate_final().await;
    eprintln!(
        "DNS_LIVE {}",
        serde_json::json!({"stage":"startup","ok":activated.is_ok(),
        "elapsed_us":started.elapsed().as_micros(),"loopback_port":proxy.port(),"tun":false})
    );
    let result = if activated.is_ok() {
        tokio::time::timeout(Duration::from_secs(150), sample_dns(proxy, &servers)).await
    } else {
        Ok(false)
    };
    cancel.cancel();
    runtime.shutdown().await;
    assert!(
        tokio::net::TcpStream::connect(proxy).await.is_err(),
        "loopback listener must close"
    );
    assert!(
        activated.is_ok() && matches!(result, Ok(true)),
        "proxy-only DNS test did not complete"
    );
}
