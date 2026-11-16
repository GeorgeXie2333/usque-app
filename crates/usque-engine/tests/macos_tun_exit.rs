//! Exercise the GUI's control protocol against a separate, installed Engine and root Agent.
#![cfg(target_os = "macos")]

use std::{
    fs,
    io::{Read, Write},
    net::{Ipv4Addr, TcpListener, TcpStream},
    os::unix::net::UnixStream,
    path::Path,
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
    let timeout = Some(Duration::from_secs(60));
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

fn seed_identity(id: Uuid, keychain: &str) {
    // Ephemeral login fixture. Only the pinned Engine is granted permission to
    // read these synthetic values. No remote registration or user tokens.
    let private = MasqueKeyPair::generate().private_sec1_der().unwrap();
    let pin = MasqueKeyPair::generate().public_spki_der().unwrap();
    let metadata = IdentityProvider::Consumer.to_metadata_json().unwrap();
    for (field, bytes) in [
        (SecretRecord::MasquePrivateKey, private.as_slice()),
        (SecretRecord::AccessToken, b"e2e-token"),
        (SecretRecord::EndpointPin, pin.as_slice()),
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
                keychain,
            ])
            .status()
            .unwrap();
        assert!(status.success(), "could not seed isolated {}", field.key());
    }
}

struct EngineProcess(Child);

impl Drop for EngineProcess {
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

#[test]
#[ignore = "remote disposable Mac only: installs real PF protection and dials a closed local endpoint"]
fn failed_endpoint_disconnect_and_exit() {
    for (key, value) in [
        ("USQUE_DISPOSABLE_MAC", "1"),
        ("GITHUB_ACTIONS", "true"),
        ("RUNNER_ENVIRONMENT", "github-hosted"),
    ] {
        assert_eq!(std::env::var(key).as_deref(), Ok(value));
    }
    let keychain = std::env::var("USQUE_TEST_KEYCHAIN").unwrap();
    let host: Ipv4Addr = std::env::var("USQUE_TEST_PHYSICAL_V4")
        .unwrap()
        .parse()
        .unwrap();
    let fixture = std::env::var("USQUE_TEST_DIRECTORY").unwrap();
    let dir = Path::new(&fixture).join("engine-state");
    fs::create_dir(&dir).unwrap();
    let config_path = dir.join("config.json");
    let socket = dir.join("control.sock");
    let closed_endpoint = TcpListener::bind((host, 0)).unwrap().local_addr().unwrap();
    assert_eq!(
        TcpStream::connect_timeout(&closed_endpoint, Duration::from_secs(1))
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::ConnectionRefused
    );
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
    config.network.endpoint.selection = EndpointSelection::Custom;
    config.network.endpoint.ipv4 = host;
    config.network.endpoint.port = closed_endpoint.port();
    config.network.ip_policy = IpPolicy::Ipv4Only;
    config.network.transport = TransportPolicy::Http2;
    config.network.auto_connect = false;
    config.network.geo_direct_countries.clear();
    config.network.routing.ads_enabled = false;
    ConfigStore::new(config_path.clone()).save(&config).unwrap();
    seed_identity(id, &keychain);

    let mut child = EngineProcess(
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
        assert_eq!(
            host_network(),
            baseline,
            "failed dial changed system DNS or routes"
        );

        // Same Disconnect RPC and clean-snapshot conditions used by disconnectForExit().
        let response = control(&socket, Payload::Disconnect(v1::DisconnectRequest {}));
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
            host_network(),
            baseline,
            "Disconnect changed system DNS or routes"
        );
    }

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
    println!(
        "Verified one-shot and continuous H2 dial failure, retained protection, explicit Disconnect, clean Agent and normal Engine exit."
    );
}
