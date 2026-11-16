//! Exercise the GUI's control protocol against a separate, installed Engine and root Agent.
#![cfg(target_os = "macos")]

use std::{
    fs,
    io::{Read, Write},
    net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpListener, TcpStream, UdpSocket},
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    process::{Child, Command},
    thread,
    time::{Duration, Instant},
};
use usque_core::{
    AppConfig, EndpointSelection, FrontendSettings, IdentityProvider, IpPolicy, MasqueKeyPair,
    TransportPolicy, storage::ConfigStore,
};
use usque_ipc::{
    MAX_FRAME_SIZE, decode_frame, encode_frame,
    v1::{self, control_request::Payload, control_response},
};
use usque_platform::SecretRecord;
use uuid::Uuid;

const ENGINE: &str = "/Library/PrivilegedHelperTools/com.usque.agent/usque-engine";
const SERVICE: &str = "io.github.georgexie2333.usque.identity";

fn control(socket: &Path, payload: Payload) -> v1::ControlResponse {
    let mut stream = UnixStream::connect(socket).unwrap();
    let timeout = Some(Duration::from_secs(
        if matches!(&payload, Payload::InitialIdentity(_)) {
            120
        } else {
            60
        },
    ));
    stream.set_read_timeout(timeout).unwrap();
    stream.set_write_timeout(timeout).unwrap();
    let request_id = Uuid::new_v4().to_string();
    let request = v1::ControlRequest {
        request_id: request_id.clone(),
        payload: Some(payload),
    };
    stream.write_all(&encode_frame(&request).unwrap()).unwrap();
    let mut header = [0; 4];
    stream.read_exact(&mut header).unwrap();
    let size = u32::from_be_bytes(header) as usize;
    assert!(size <= MAX_FRAME_SIZE);
    let mut frame = header.to_vec();
    frame.resize(size + 4, 0);
    stream.read_exact(&mut frame[4..]).unwrap();
    let response: v1::ControlResponse = decode_frame(frame.into()).unwrap();
    assert_eq!(response.request_id, request_id);
    response
}

fn agent_status() -> serde_json::Value {
    let output = Command::new(ENGINE)
        .arg("--macos-agent-status")
        .output()
        .unwrap();
    assert!(output.status.success());
    serde_json::from_slice(&output.stdout).unwrap()
}

fn observed_dial_failures(log: &str, continuous: bool) -> bool {
    let mut prepared = false;
    for entry in log
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
    {
        let fields = &entry["fields"];
        if fields["message"] == "macOS TUN Agent prepared the physical egress" {
            prepared = true;
        } else if prepared && fields["transport_failure"] == "H2_TCP_CONNECT_FAILED" {
            if continuous
                && fields["message"] == "MASQUE initial dial failed"
                && fields["attempt"]
                    .as_u64()
                    .is_some_and(|attempt| attempt >= 2)
                || !continuous && fields["stage"] == "runtime"
            {
                return true;
            }
        }
    }
    false
}

fn preparing_fence() -> serde_json::Value {
    // The ordinary Engine only uses unprivileged GUI RPC. Read its separate
    // Agent journal as an independent observer on the disposable runner.
    let output = Command::new("/usr/bin/sudo")
        .args([
            "-n",
            "/bin/cat",
            "/Library/Application Support/Usque/Agent/network.json",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let journal: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(journal["transaction"]["phase"], "preparing");
    let fence = &journal["transaction"]["fence"];
    assert!(
        fence["session"]
            .as_str()
            .is_some_and(|s| Uuid::parse_str(s).is_ok())
    );
    assert!(fence["configuration"].as_u64().is_some_and(|n| n > 0));
    assert!(fence["network"].as_u64().is_some_and(|n| n > 0));
    fence.clone()
}

fn retry_preparing(socket: &Path, log_path: &Path) {
    let before = preparing_fence();
    let response = control(
        socket,
        Payload::Retry(v1::RetryRequest { continuous: true }),
    );
    assert!(
        response.error.is_none(),
        "Retry error: {:?}",
        response.error
    );
    let Some(control_response::Payload::Status(snapshot)) = response.payload else {
        panic!("Retry did not acknowledge a snapshot")
    };
    // The Engine reports its H2 dial while the Agent journal remains Preparing.
    assert_eq!(snapshot.phase(), v1::ConnectionPhase::ConnectingHttp2);
    let acknowledged = fs::read(log_path).unwrap().len();
    wait_until(
        "subsequent real dial after GUI Retry",
        Duration::from_secs(15),
        || {
            let log = fs::read_to_string(log_path).unwrap();
            log.get(acknowledged..).is_some_and(|new_entries| {
                new_entries.lines().any(|line| {
                    serde_json::from_str::<serde_json::Value>(line).is_ok_and(|entry| {
                        entry["fields"]["message"] == "MASQUE initial dial failed"
                            && entry["fields"]["transport_failure"] == "H2_TCP_CONNECT_FAILED"
                    })
                })
            })
        },
    );
    assert_eq!(
        preparing_fence(),
        before,
        "Retry replaced the active fenced transaction"
    );
}

#[derive(Debug, PartialEq, Eq)]
struct HostNetwork {
    dns: Vec<String>,
    routes: Vec<Option<Vec<String>>>,
}
fn host_network() -> HostNetwork {
    let output = Command::new("/usr/sbin/scutil")
        .arg("--dns")
        .output()
        .unwrap();
    assert!(output.status.success());
    let dns = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("nameserver[") || line.starts_with("domain "))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    assert!(!dns.is_empty(), "expected physical resolver baseline");
    let routes = ["1.1.1.1", "2606:4700:4700::1111"]
        .map(|destination| {
            let family = if destination.contains(':') {
                "-inet6"
            } else {
                "-inet"
            };
            let output = Command::new("/sbin/route")
                .args(["-n", "get", family, destination])
                .env("LC_ALL", "C")
                .output()
                .unwrap();
            let text = String::from_utf8(output.stdout).unwrap();
            let error = String::from_utf8(output.stderr).unwrap();
            // Darwin can exit successfully after reporting ESRCH on stderr.
            if !text.contains("interface:")
                && matches!(
                    error.trim(),
                    "route: writing to routing socket: not in table"
                        | "route: message indicates error 3: No such process"
                )
            {
                return None;
            }
            assert!(
                output.status.success() && error.trim().is_empty() && text.contains("interface:"),
                "route to {destination}: {text} {error}"
            );
            Some(
                text.lines()
                    .map(str::trim)
                    .filter(|line| line.starts_with("gateway:") || line.starts_with("interface:"))
                    .map(str::to_owned)
                    .collect(),
            )
        })
        .to_vec();
    assert!(routes[0].is_some(), "expected a physical IPv4 route");
    HostNetwork { dns, routes }
}

fn seed_identity(id: Uuid, pin: &[u8]) {
    let keychain = std::env::var("USQUE_TEST_KEYCHAIN").unwrap();
    // Ephemeral login fixture. Only the pinned Engine is granted permission to
    // read these synthetic values. No remote registration or user tokens.
    let private = MasqueKeyPair::generate().private_sec1_der().unwrap();
    let metadata = IdentityProvider::Consumer.to_metadata_json().unwrap();
    for (field, bytes) in [
        (SecretRecord::MasquePrivateKey, private.as_slice()),
        (SecretRecord::AccessToken, b"e2e-token"),
        (SecretRecord::EndpointPin, pin),
        (SecretRecord::DeviceId, b"e2e-device"),
        (SecretRecord::AssignedIpv4, b"192.0.2.7"),
        (SecretRecord::AssignedIpv6, b"2001:db8::7"),
        (SecretRecord::IdentityMetadata, metadata.as_slice()),
    ] {
        let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
        let status = Command::new("/usr/bin/security")
            .args([
                "add-generic-password",
                "-a",
                &format!("{id}/{}", field.key()),
                "-s",
                SERVICE,
                "-X",
                &hex,
                "-T",
                ENGINE,
                &keychain,
            ])
            .status()
            .unwrap();
        assert!(status.success(), "could not seed isolated {}", field.key());
    }
}

struct TestProcess(Child);

impl Drop for TestProcess {
    fn drop(&mut self) {
        // Failure cleanup for the disposable runner. A forced termination can
        // never satisfy the normal-exit assertion below.
        if self.0.try_wait().is_ok_and(|exit| exit.is_none()) {
            let _ = self.0.kill();
        }
        let _ = self.0.wait();
    }
}

fn wait_until(description: &str, timeout: Duration, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + timeout;
    while !condition() {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {description}"
        );
        thread::sleep(Duration::from_millis(100));
    }
}

fn fixture(name: &str) -> (PathBuf, Ipv4Addr) {
    for (key, value) in [
        ("USQUE_DISPOSABLE_MAC", "1"),
        ("GITHUB_ACTIONS", "true"),
        ("RUNNER_ENVIRONMENT", "github-hosted"),
    ] {
        assert_eq!(std::env::var(key).as_deref(), Ok(value));
    }
    let host: Ipv4Addr = std::env::var("USQUE_TEST_PHYSICAL_V4")
        .unwrap()
        .parse()
        .unwrap();
    let fixture = std::env::var("USQUE_TEST_DIRECTORY").unwrap();
    let dir = Path::new(&fixture).join(name);
    fs::create_dir(&dir).unwrap();
    (dir, host)
}

fn start_engine(dir: &Path, endpoint: SocketAddrV4, pin: &[u8]) -> (TestProcess, Uuid, PathBuf) {
    launch_engine(dir, Some((endpoint, pin)))
}

fn launch_engine(dir: &Path, peer: Option<(SocketAddrV4, &[u8])>) -> (TestProcess, Uuid, PathBuf) {
    let config_path = dir.join("config.json");
    let socket = dir.join("control.sock");
    let mut config = AppConfig::default();
    let id = Uuid::new_v4();
    config.profiles[0].id = id;
    config.active_profile_id = Some(id);
    config.network.frontends = FrontendSettings {
        tunnel: true,
        socks5: false,
        http: false,
    };
    config.network.kill_switch = true;
    config.network.masque_connections = 1;
    if let Some((endpoint, pin)) = peer {
        config.network.endpoint.selection = EndpointSelection::Custom;
        config.network.endpoint.ipv4 = *endpoint.ip();
        config.network.endpoint.port = endpoint.port();
        config.network.ip_policy = IpPolicy::Ipv4Only;
        config.network.transport = TransportPolicy::Http2;
        seed_identity(id, pin);
    }
    config.network.auto_connect = false;
    config.network.geo_direct_countries.clear();
    config.network.routing.ads_enabled = false;
    ConfigStore::new(config_path.clone()).save(&config).unwrap();

    let mut child = TestProcess(
        Command::new(ENGINE)
            .arg("--config")
            .arg(&config_path)
            .arg("--socket")
            .arg(&socket)
            .arg("--parent-pid")
            .arg(std::process::id().to_string())
            .env("RUST_LOG", "info,usque_transport::netstack=debug")
            .spawn()
            .unwrap(),
    );
    wait_until("Engine control socket", Duration::from_secs(15), || {
        assert!(
            child.0.try_wait().unwrap().is_none(),
            "Engine exited before accepting IPC"
        );
        socket.exists()
    });
    let status = agent_status();
    assert_eq!(status["tunAvailable"], true, "Agent preflight: {status}");
    (child, id, socket)
}

fn disconnect(socket: &Path, baseline: &HostNetwork) {
    // Same Disconnect RPC and clean-snapshot conditions used by disconnectForExit().
    let response = control(socket, Payload::Disconnect(v1::DisconnectRequest {}));
    assert!(
        response.error.is_none(),
        "Disconnect error: {:?}",
        response.error
    );
    let Some(control_response::Payload::Status(snapshot)) = response.payload else {
        panic!("Disconnect did not acknowledge a snapshot");
    };
    assert_eq!(snapshot.phase, v1::ConnectionPhase::Disconnected as i32);
    assert!(snapshot.error.is_none());
    assert!(
        Command::new(ENGINE)
            .arg("--macos-agent-check-clean")
            .status()
            .unwrap()
            .success()
    );
    assert_eq!(
        &host_network(),
        baseline,
        "Disconnect changed system DNS or routes"
    );
}

fn normal_exit(mut child: TestProcess, socket: &Path) {
    // DesktopEngineTransport.dispose sends SIGTERM after confirming Disconnect.
    assert!(
        Command::new("/bin/kill")
            .args(["-TERM", &child.0.id().to_string()])
            .status()
            .unwrap()
            .success()
    );
    wait_until("ordinary Engine exit", Duration::from_secs(12), || {
        child.0.try_wait().unwrap().is_some_and(|status| {
            assert!(status.success());
            true
        })
    });
    assert!(!socket.exists(), "control socket survived clean exit");
}

#[test]
#[ignore = "remote disposable Mac only: installs real PF protection and dials a closed local endpoint"]
fn failed_endpoint_disconnect_and_exit() {
    let (dir, host) = fixture("engine-state");
    let closed = TcpListener::bind((host, 0)).unwrap().local_addr().unwrap();
    assert_eq!(
        TcpStream::connect_timeout(&closed, Duration::from_secs(1))
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::ConnectionRefused
    );
    let pin = MasqueKeyPair::generate().public_spki_der().unwrap();
    let (child, id, socket) = start_engine(&dir, SocketAddrV4::new(host, closed.port()), &pin);
    let baseline = host_network();
    let engine_log = dir.join("logs/engine.jsonl");
    // A one-shot request returns the dial error. The desktop GUI normally
    // acknowledges a continuous intent first, then dials on its worker.
    for continuous in [false, true] {
        let log_start = fs::read_to_string(&engine_log).unwrap_or_default().len();
        let response = control(
            &socket,
            Payload::Connect(v1::ConnectRequest {
                profile_id: id.to_string(),
                continuous,
                background: !continuous,
            }),
        );
        if continuous {
            assert!(
                response.error.is_none(),
                "Connect intent: {:?}",
                response.error
            );
            let Some(control_response::Payload::Status(snapshot)) = response.payload else {
                panic!("Connect intent did not acknowledge a snapshot");
            };
            assert_eq!(snapshot.phase, v1::ConnectionPhase::Preparing as i32);
        } else {
            let error = response
                .error
                .expect("closed endpoint unexpectedly connected");
            println!("Connect returned {}", error.code);
        }
        // A nonblocking connect can report refusal immediately in the Agent or
        // later via SO_ERROR in the Engine. Both reach this production failure
        // code. A Keychain/preflight failure or an absent Agent cannot qualify.
        wait_until(
            "prepared physical egress and H2 TCP connect failure",
            Duration::from_secs(25),
            || {
                let log = fs::read_to_string(&engine_log).unwrap_or_default();
                log.get(log_start..)
                    .is_some_and(|log| observed_dial_failures(log, continuous))
            },
        );
        wait_until("protected startup phase", Duration::from_secs(5), || {
            let status = agent_status();
            status["protectionRetained"] == true
                && if continuous {
                    status["phase"] == usque_ipc::agent_v1::AgentPhase::Preparing as i32
                } else {
                    status["recoveryPending"] == true
                }
        });
        if continuous {
            assert_eq!(
                phase(&socket),
                v1::ConnectionPhase::ConnectingHttp2,
                "GetStatus mislabeled a live dial as orphan recovery"
            );
            retry_preparing(&socket, &engine_log);
        }
        assert_eq!(
            host_network(),
            baseline,
            "failed dial changed system DNS or routes"
        );

        disconnect(&socket, &baseline);
    }
    normal_exit(child, &socket);
    println!(
        "Verified one-shot and continuous H2 dial failure, retained protection, explicit Disconnect, clean Agent and normal Engine exit."
    );
}

fn phase(socket: &Path) -> v1::ConnectionPhase {
    let response = control(socket, Payload::GetStatus(v1::GetStatusRequest {}));
    assert!(response.error.is_none());
    let Some(control_response::Payload::Status(status)) = response.payload else {
        panic!("missing connection status");
    };
    status.phase()
}

#[test]
#[ignore = "remote disposable Mac only: routes UDP through a real utun and a pinned H2 peer"]
fn connected_peer_loss_disconnect_and_exit() {
    let (dir, host) = fixture("peer-state");
    let nonce = Uuid::new_v4().to_string();
    let mut peer = TestProcess(
        Command::new(std::env::var("USQUE_TEST_PEER").unwrap())
            .arg(&dir)
            .arg(host.to_string())
            .arg(&nonce)
            .spawn()
            .unwrap(),
    );
    let ready = dir.join("peer-port");
    wait_until("pinned CONNECT-IP server", Duration::from_secs(5), || {
        assert!(peer.0.try_wait().unwrap().is_none());
        ready.is_file()
    });
    let port = fs::read_to_string(ready).unwrap().parse().unwrap();
    let (child, id, socket) = start_engine(
        &dir,
        SocketAddrV4::new(host, port),
        &fs::read(dir.join("pin.der")).unwrap(),
    );
    let baseline = host_network();
    let response = control(
        &socket,
        Payload::Connect(v1::ConnectRequest {
            profile_id: id.to_string(),
            continuous: true,
            background: true,
        }),
    );
    assert!(response.error.is_none(), "Connect: {:?}", response.error);
    wait_until(
        "connected H2 carrier and active Agent",
        Duration::from_secs(30),
        || {
            phase(&socket) == v1::ConnectionPhase::Connected
                && agent_status()["phase"] == usque_ipc::agent_v1::AgentPhase::Active as i32
        },
    );
    let expected_dns = usque_transport::SPLIT_DNS_IPV4.to_string();
    wait_until(
        "published tunnel resolver and route",
        Duration::from_secs(8),
        || {
            let active = host_network();
            let dns = active
                .dns
                .iter()
                .find(|line| line.starts_with("nameserver["))
                .and_then(|line| line.split_whitespace().last());
            dns == Some(expected_dns.as_str())
                && active.routes[0]
                    .as_ref()
                    .is_some_and(|fields| fields.iter().any(|s| s.starts_with("interface: utun")))
        },
    );
    // Only the remote end of the CONNECT-IP stream can return this nonce.
    let udp = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).unwrap();
    udp.set_read_timeout(Some(Duration::from_millis(500)))
        .unwrap();
    udp.connect(SocketAddr::from(([198, 51, 100, 42], 39017)))
        .unwrap();
    let mut packet = [0u8; 128];
    wait_until(
        "UDP echo through utun and CONNECT-IP",
        Duration::from_secs(10),
        || {
            udp.send(nonce.as_bytes()).unwrap();
            match udp.recv(&mut packet) {
                Ok(size) => {
                    assert_eq!(&packet[..size], nonce.as_bytes());
                    true
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    false
                }
                Err(error) => panic!("canary receive failed: {error}"),
            }
        },
    );
    println!(
        "Verified real Engine, Agent, configured DNS/route and bidirectional UDP over pinned H2."
    );
    peer.0.kill().unwrap();
    peer.0.wait().unwrap();
    wait_until(
        "lost carrier enters reconnect",
        Duration::from_secs(20),
        || phase(&socket) == v1::ConnectionPhase::Reconnecting,
    );
    assert_eq!(agent_status()["protectionRetained"], true);
    disconnect(&socket, &baseline);
    normal_exit(child, &socket);
    println!(
        "Verified peer-loss reconnect, explicit disconnect, restored DNS/routes and ordinary exit."
    );
}

// Trace includes IP and location. Keep only the WARP field, and bypass any runner proxy.
fn warp_trace() -> Result<&'static str, String> {
    let output = Command::new("/usr/bin/curl")
        .args([
            "--disable",
            "--silent",
            "--fail",
            "--noproxy",
            "*",
            "--max-time",
            "10",
            "https://www.cloudflare.com/cdn-cgi/trace",
        ])
        .output()
        .unwrap();
    if !output.status.success() {
        return Err(format!(
            "trace request failed (curl {:?})",
            output.status.code()
        ));
    }
    match std::str::from_utf8(&output.stdout)
        .ok()
        .and_then(|text| text.lines().find_map(|s| s.strip_prefix("warp=")))
    {
        Some("off") => Ok("off"),
        Some("on") => Ok("on"),
        Some("plus") => Ok("plus"),
        _ => Err("trace has no recognized warp field".into()),
    }
}

fn verify_live_tunnel(socket: &Path, id: Uuid) -> Result<String, String> {
    let response = control(
        socket,
        Payload::Connect(v1::ConnectRequest {
            profile_id: id.to_string(),
            continuous: true,
            background: false,
        }),
    );
    if let Some(error) = response.error {
        return Err(format!("Connect: {}", error.code));
    }
    let deadline = Instant::now() + Duration::from_secs(100);
    let mut previous = None;
    loop {
        let response = control(socket, Payload::GetStatus(v1::GetStatusRequest {}));
        let Some(control_response::Payload::Status(status)) = response.payload else {
            return Err("GetStatus returned no snapshot".into());
        };
        let phase = status.phase();
        if previous != Some(phase) {
            println!("Live connection phase: {phase:?}");
            previous = Some(phase);
        }
        if phase == v1::ConnectionPhase::Connected
            && agent_status()["phase"] == usque_ipc::agent_v1::AgentPhase::Active as i32
        {
            println!("Active Agent, negotiated transport: {}", status.transport);
            break;
        }
        if matches!(
            phase,
            v1::ConnectionPhase::Error | v1::ConnectionPhase::Disconnected
        ) || Instant::now() >= deadline
        {
            return Err(format!(
                "Connection ended or timed out: {phase:?}, error code {:?}, transport failure {:?}",
                status.error.map(|e| e.code),
                status.failure.map(|f| f.code)
            ));
        }
        thread::sleep(Duration::from_millis(500));
    }
    let deadline = Instant::now() + Duration::from_secs(8);
    let interface = loop {
        let network = host_network();
        let routed_utun = network.routes[0]
            .iter()
            .flatten()
            .find_map(|s| s.strip_prefix("interface: "))
            .filter(|s| s.starts_with("utun"));
        let resolver = network
            .dns
            .iter()
            .find(|s| s.starts_with("nameserver["))
            .and_then(|s| s.split_whitespace().last())
            .and_then(|s| s.parse::<std::net::IpAddr>().ok());
        if let Some(interface) = routed_utun
            && resolver.is_some_and(|ip| {
                ip == usque_transport::SPLIT_DNS_IPV4 || ip == usque_transport::SPLIT_DNS_IPV6
            })
        {
            break interface.to_owned();
        }
        if Instant::now() >= deadline {
            return Err("active system resolver or IPv4 route did not move onto the tunnel".into());
        }
        thread::sleep(Duration::from_millis(200));
    };
    let trace = warp_trace().or_else(|_| warp_trace())?;
    if !matches!(trace, "on" | "plus") {
        return Err(format!("active tunnel trace returned warp={trace}"));
    }
    println!("Cloudflare verified warp={trace} with active tunnel DNS and {interface} route.");
    Ok(interface)
}

#[test]
#[ignore = "explicit enrollment on disposable GitHub Macs only; uses real Cloudflare WARP"]
fn live_warp_connect_disconnect_and_exit() {
    assert_eq!(std::env::var("USQUE_TEST_LIVE_WARP").as_deref(), Ok("1"));
    let (dir, _) = fixture("live-warp");
    let baseline = host_network();
    assert_eq!(warp_trace().as_deref(), Ok("off"), "runner baseline trace");
    println!("Cloudflare baseline warp=off; registering one free device through the Engine.");
    let (child, id, socket) = launch_engine(&dir, None);
    let response = control(
        &socket,
        Payload::InitialIdentity(v1::InitialIdentityRequest {
            operation_id: Uuid::new_v4().to_string(),
            profile_id: id.to_string(),
            provisioning: Some(v1::IdentityProvisioning {
                method: v1::IdentityProvisioningMethod::Register as i32,
                terms_accepted: true,
                locale: "en_US".into(),
                device_name: format!("Usque CI {}", std::env::consts::ARCH),
                ..Default::default()
            }),
            resume_only: false,
        }),
    );
    if let Some(error) = response.error {
        let status = error
            .message
            .split_once("Cloudflare registration failed with ")
            .and_then(|(_, tail)| tail.split_whitespace().next())
            .and_then(|s| s.parse::<u16>().ok());
        panic!(
            "WARP registration failed: {}, HTTP status {status:?}",
            error.code
        );
    }
    let Some(control_response::Payload::InitialIdentityState(state)) = response.payload else {
        panic!("no initial identity receipt");
    };
    assert_eq!(state.phase(), v1::InitialIdentityPhase::Completed);
    assert!(!state.reused);
    println!(
        "Real WARP Consumer enrollment completed. Connecting with automatic endpoint and transport."
    );
    let connected = verify_live_tunnel(&socket, id);
    if let Err(reason) = &connected {
        eprintln!("Live traffic verification failed: {reason}");
        let log = fs::read_to_string(dir.join("logs/engine.jsonl")).unwrap_or_default();
        let mut codes = std::collections::BTreeSet::new();
        for line in log.lines() {
            if let Ok(event) = serde_json::from_str::<serde_json::Value>(line) {
                for field in ["transport_failure", "agent_failure", "error_code"] {
                    if let Some(code) = event["fields"][field].as_str()
                        && code.len() <= 80
                        && code
                            .bytes()
                            .all(|b| b.is_ascii_uppercase() || b == b'_' || b.is_ascii_digit())
                    {
                        codes.insert(code.to_owned());
                    }
                }
            }
        }
        eprintln!("Engine failure codes: {codes:?}");
    }
    // Exercise ordinary disconnect even after dial or traffic failure.
    disconnect(&socket, &baseline);
    normal_exit(child, &socket);
    assert_eq!(warp_trace().as_deref(), Ok("off"), "post-disconnect trace");
    println!(
        "Disconnect restored baseline DNS/routes, Agent is clean, Engine exited, Cloudflare warp=off."
    );
    let interface = connected.unwrap();
    let output = Command::new("/sbin/ifconfig").arg("-l").output().unwrap();
    assert!(output.status.success());
    assert!(
        !String::from_utf8(output.stdout)
            .unwrap()
            .split_whitespace()
            .any(|s| s == interface),
        "session utun survived disconnect"
    );
}
