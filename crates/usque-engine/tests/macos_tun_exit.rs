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
    let dir = tempfile::tempdir_in("/private/tmp").unwrap();
    let config_path = dir.path().join("config.json");
    let socket = dir.path().join("control.sock");
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
            .env("RUST_LOG", "info")
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
    let response = control(
        &socket,
        Payload::Connect(v1::ConnectRequest {
            profile_id: id.to_string(),
            continuous: false,
            background: true,
        }),
    );
    let error = response
        .error
        .expect("closed endpoint unexpectedly connected");
    println!("Connect returned {}", error.code);
    let engine_log = dir.path().join("logs/engine.jsonl");
    // A nonblocking connect can report refusal immediately in the Agent or
    // later via SO_ERROR in the Engine. Both reach this production failure
    // code. A Keychain/preflight failure or an absent Agent cannot qualify.
    wait_until(
        "prepared physical egress and H2 TCP connect failure",
        Duration::from_secs(5),
        || {
            fs::read_to_string(&engine_log).is_ok_and(|log| {
                log.contains("macOS TUN Agent prepared the physical egress")
                    && log.lines().any(|line| {
                        serde_json::from_str::<serde_json::Value>(line).is_ok_and(|entry| {
                            entry["fields"]["transport_failure"] == "H2_TCP_CONNECT_FAILED"
                        })
                    })
            })
        },
    );
    wait_until("retained Kill Switch", Duration::from_secs(5), || {
        agent_status()["protectionRetained"] == true
    });

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
        "Verified H2 dial failure, retained protection, explicit Disconnect, clean Agent and normal Engine exit."
    );
}
