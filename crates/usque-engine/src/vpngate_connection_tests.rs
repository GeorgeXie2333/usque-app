use super::*;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use usque_core::vpngate::{
    Catalogue, CatalogueStore, GateFailure, GateStage, ListQuery, Selection,
};

fn gate_catalogue(service: &ControlService) -> Vec<Selection> {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use serde_json::json;
    use sha2::{Digest, Sha256};
    let hash = |data: &str| -> String {
        Sha256::digest(data)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    };
    let servers: Vec<_> = ["8.8.8.8", "1.1.1.1"].into_iter().map(|ip| {
        let host = format!("test-{ip}");
        // Only the directory envelope is exercised; no native TLS is started.
        let content = format!("client\ndev tun\nproto tcp\nremote {ip} 443\ncipher AES-128-CBC\nauth SHA1\n<ca>\nfixture\n</ca>\n");
        json!({
            "id": format!("v1:{}", hash(&format!("vpngate-node-v1\0{host}\0{ip}"))),
            "hostname": host, "ip": ip, "country_code": "JP", "country_name": "Japan",
            "score": 1, "ping_ms": null, "speed_bps": null, "num_vpn_sessions": 1,
            "openvpn_config_base64": STANDARD.encode(&content),
            "openvpn_config_sha256": hash(&content),
            "openvpn_config_bytes": content.len(),
        })
    }).collect();
    let bytes = serde_json::to_vec(&json!({
        "schema_version": 1, "source_csv_sha256": "0".repeat(64),
        "server_count": servers.len(), "servers": servers,
    }))
    .unwrap();
    let catalogue = Catalogue::parse(&bytes).unwrap();
    let store = CatalogueStore::new(&service.cache_dir);
    store
        .save(&catalogue, 1, usque_core::vpngate::RAW_URL)
        .unwrap();
    catalogue
        .list(&ListQuery::default())
        .servers
        .into_iter()
        .map(|server| Selection {
            server_id: server.id,
            config_sha256: server.config_sha256,
        })
        .collect()
}

async fn failed_gate_session() -> (tempfile::TempDir, ControlService, Profile, Vec<Selection>) {
    let directory = tempfile::tempdir().unwrap();
    let service = ControlService::open_with_vault(
        ConfigStore::new(directory.path().join("config.json")),
        Arc::new(crate::tests::MemoryVault::default()),
    )
    .unwrap();
    let selections = gate_catalogue(&service);
    let mut profile = service.config_snapshot().await.active_profile().unwrap();
    profile.vpn_gate.enabled = true;
    profile.vpn_gate.selection = Some(selections[0].clone());
    service
        .install_test_session(profile.clone(), true, 7)
        .await
        .unwrap();
    service
        .data_plane
        .lock()
        .await
        .as_mut()
        .unwrap()
        .runtime
        .fail_gate(GateFailure::Transport)
        .await;
    assert_eq!(
        service.status_snapshot().await.phase,
        ConnectionPhase::Error
    );
    (directory, service, profile, selections)
}

#[tokio::test]
async fn failed_gate_startup_cancels_warp_and_releases_the_runtime() {
    for vpn in [false, true] {
        for reason in [
            GateFailure::Transport,
            GateFailure::Authentication,
            GateFailure::Certificate,
        ] {
            let (_directory, service, profile, _) = failed_gate_session().await;
            // A startup result has not yet been installed as the active session.
            let mut active = service.data_plane.lock().await.take().unwrap();
            let ActiveRuntime::Harness(runtime) = &mut active.runtime else {
                panic!("memory runtime")
            };
            runtime.vpn = vpn;
            runtime.gate_status.failure = Some(reason);
            let stopped = runtime.stopped.clone();
            let stop_requested = runtime.stop_requested.clone();
            assert!(service.accept_gate_runtime(active.runtime).await.is_err());
            assert!(stop_requested.is_cancelled());
            assert!(service.data_plane.lock().await.is_none());
            service.await_disconnect_cleanup().await.unwrap();
            assert!(stopped.is_cancelled());
            let snapshot = service.status_snapshot().await;
            assert_eq!(snapshot.phase, ConnectionPhase::Error);
            assert!(
                snapshot
                    .frontends
                    .iter()
                    .all(|frontend| frontend.phase != FrontendPhase::Active)
            );
            assert_eq!(service.gate_status.borrow().failure, Some(reason));
            assert_eq!(
                service.gate_status.borrow().warp_stage.as_deref(),
                Some("disconnected")
            );
            assert!(service.session_profile.lock().await.is_none());
            assert_eq!(
                service.config_snapshot().await.active_profile_id,
                Some(profile.id)
            );
        }
    }
}

#[tokio::test]
async fn gate_supervisor_disconnects_the_failed_chain_without_an_in_place_retry() {
    let (_directory, service, _, _) = failed_gate_session().await;
    let stopped = {
        let active = service.data_plane.lock().await;
        let ActiveRuntime::Harness(runtime) = &active.as_ref().unwrap().runtime else {
            panic!("memory runtime")
        };
        runtime.stopped.clone()
    };
    service.ensure_gate_supervisor().await;
    tokio::time::timeout(Duration::from_secs(2), stopped.cancelled())
        .await
        .unwrap();
    let _mutation = service.mutation_lock.lock().await;
    assert!(service.data_plane.lock().await.is_none());
    service.await_disconnect_cleanup().await.unwrap();
    assert_eq!(
        service.status_snapshot().await.phase,
        ConnectionPhase::Error
    );
    assert_eq!(
        service.gate_status.borrow().warp_stage.as_deref(),
        Some("disconnected")
    );
}

#[tokio::test]
async fn a_fresh_warp_runtime_clears_the_previous_gate_failure_status() {
    let (_directory, service, mut profile, _) = failed_gate_session().await;
    service.stop_failed_gate_locked().await.unwrap();
    service.await_disconnect_cleanup().await.unwrap();
    profile.vpn_gate.enabled = false;
    let runtime = ActiveRuntime::Harness(Box::new(active_runtime::HarnessRuntime::from_profile(
        &profile, false, 0,
    )));
    let mut runtime = service.accept_gate_runtime(runtime).await.unwrap();
    assert_eq!(service.gate_status.borrow().stage, GateStage::Disabled);
    assert!(service.gate_status.borrow().failure.is_none());
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn failed_gate_connect_and_retry_start_a_new_warp_session_with_the_saved_target() {
    for retry in [false, true] {
        for disable in [false, true] {
            let (_directory, service, mut profile, selections) = failed_gate_session().await;
            profile.vpn_gate.enabled = !disable;
            profile.vpn_gate.selection = Some(selections[1].clone());
            service.upsert_profile(profile.clone()).await.unwrap();
            // No WARP identity is installed. A fresh connect must stop at the
            // vault instead of reviving the failed underlay or calling Agent.
            let result = if retry {
                service.retry().await
            } else {
                service.connect(profile.id).await
            };
            assert!(result.is_err());
            assert!(service.data_plane.lock().await.is_none());
            assert_eq!(
                service.config_snapshot().await.network.vpn_gate,
                profile.vpn_gate
            );
            service.await_disconnect_cleanup().await.unwrap();
        }
    }
}

#[tokio::test]
async fn failed_gate_switch_keeps_saved_target_and_disconnects_when_snapshot_is_missing() {
    let (_directory, service, mut profile, selections) = failed_gate_session().await;
    // Model a healthy old connection before selecting the missing new snapshot.
    let stopped = {
        let mut active = service.data_plane.lock().await;
        let ActiveRuntime::Harness(runtime) = &mut active.as_mut().unwrap().runtime else {
            panic!("memory runtime")
        };
        runtime.gate_status.stage = GateStage::Connected;
        runtime.gate_status.failure = None;
        runtime.warp_ready = true;
        runtime.stopped.clone()
    };
    profile.vpn_gate.selection = Some(selections[1].clone());
    service.upsert_profile(profile.clone()).await.unwrap();
    let selection = &selections[1];
    std::fs::remove_file(service.cache_dir.join("vpngate/selected").join(format!(
        "{}-{}.json",
        &selection.server_id[3..],
        selection.config_sha256,
    )))
    .unwrap();
    assert!(matches!(
        service.hot_replace_gate(&profile).await,
        Err(ControlServiceError::VpnGate(_))
    ));
    assert!(service.data_plane.lock().await.is_none());
    assert_eq!(
        service.config_snapshot().await.network.vpn_gate,
        profile.vpn_gate
    );
    assert_eq!(service.gate_status.borrow().stage, GateStage::Error);
    assert_eq!(
        service.gate_status.borrow().warp_stage.as_deref(),
        Some("disconnected")
    );
    service.await_disconnect_cleanup().await.unwrap();
    assert!(stopped.is_cancelled());
}

#[cfg(windows)]
#[tokio::test]
async fn connect_tracks_pending_disconnect_recovery_before_creating_another_tunnel() {
    let directory = tempfile::tempdir().unwrap();
    let service = ControlService::open_with_vault(
        ConfigStore::new(directory.path().join("config.json")),
        Arc::new(crate::tests::MemoryVault::default()),
    )
    .unwrap();
    let profile = service.config_snapshot().await.active_profile().unwrap();
    let operation_id = Uuid::new_v4().to_string();
    let pending_operation = operation_id.clone();
    *service.disconnect_cleanup.lock().await = Some(tokio::spawn(async move {
        Err(ControlServiceError::PlatformRecoveryPending {
            operation_id: pending_operation,
            journal_generation: 19,
        })
    }));
    let snapshot = service.connect(profile.id).await.unwrap();
    assert_eq!(snapshot.phase, ConnectionPhase::Reconnecting);
    assert!(service.data_plane.lock().await.is_none());
    let recovery = service.windows_recovery.lock().await;
    let watch = recovery.pending.as_ref().unwrap();
    assert_eq!(watch.operation_id, operation_id);
    assert_eq!(watch.journal_generation, 19);
    drop(recovery);
    service.disconnect().await.unwrap();
    assert!(service.windows_recovery.lock().await.pending.is_none());
}

#[tokio::test]
async fn disconnect_and_shutdown_cancel_gate_before_waiting_for_mutation() {
    for shutdown in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let service = ControlService::open_with_vault(
            ConfigStore::new(directory.path().join("config.json")),
            Arc::new(crate::tests::MemoryVault::default()),
        )
        .unwrap();
        service
            .state
            .lock()
            .await
            .transition(ConnectionPhase::Preparing)
            .unwrap();
        let cancel = service.gate_connection_request().await;
        let mutation = service.mutation_lock.clone().lock_owned().await;
        let owner = tokio::spawn(async move {
            cancel.cancelled().await;
            drop(mutation);
        });
        tokio::time::timeout(Duration::from_secs(2), async {
            if shutdown {
                service.shutdown().await.unwrap();
            } else {
                service.disconnect().await.unwrap();
            }
            owner.await.unwrap();
        })
        .await
        .expect("stop must wake the handshake owner before taking its lock");
        assert_eq!(
            service.status_snapshot().await.phase,
            ConnectionPhase::Disconnected
        );
        assert!(service.gate_startup_cancel.lock().await.is_cancelled());
    }
}

#[tokio::test]
async fn queued_connect_cannot_restart_after_disconnect() {
    let directory = tempfile::tempdir().unwrap();
    let service = ControlService::open_with_vault(
        ConfigStore::new(directory.path().join("config.json")),
        Arc::new(crate::tests::MemoryVault::default()),
    )
    .unwrap();
    let profile_id = service.config_snapshot().await.active_profile_id.unwrap();
    let stale = service.gate_connection_request().await;
    service.disconnect().await.unwrap();
    let fresh = service.gate_connection_request().await;
    assert!(!fresh.is_cancelled());
    let _mutation = service.mutation_lock.lock().await;
    let profile = service.config_snapshot().await.active_profile().unwrap();
    let retry = service
        .hot_replace_gate_with_cancellation(&profile, &stale)
        .await;
    assert!(matches!(
        retry,
        Err(ControlServiceError::Transport(
            usque_transport::TransportError::TunnelClosed
        ))
    ));
    let result = service
        .connect_with_cancellation_locked(profile_id, stale, true)
        .await
        .unwrap();
    assert_eq!(result.phase, ConnectionPhase::Disconnected);
    assert!(service.data_plane.lock().await.is_none());
}

/// Read enrolled credentials and the saved node only when explicitly opted in.
/// All settings changes live in a temporary copy; no Agent/TUN/system proxy API
/// is reachable. This checks the handshake and final-channel IP lookup only.
#[cfg(windows)]
#[tokio::test]
#[ignore = "requires enrolled Windows credentials and explicit USQUE_LIVE_CONFIG"]
async fn live_saved_vpngate_handshake_without_tun() {
    use usque_core::vpngate::{CatalogueStore, GateStage};
    use usque_transport::{DataPlaneRuntime, VpnGateStart};
    let source = PathBuf::from(std::env::var_os("USQUE_LIVE_CONFIG").expect("USQUE_LIVE_CONFIG"));
    let directory = tempfile::tempdir().unwrap();
    let staged = directory.path().join("config.json");
    std::fs::copy(&source, &staged).unwrap();
    let service = ControlService::open(ConfigStore::new(&staged)).unwrap();
    let mut profile = service.config_snapshot().await.active_profile().unwrap();
    if let Ok(transport) = std::env::var("USQUE_LIVE_TRANSPORT") {
        profile.transport = match transport.as_str() {
            "h2" => TransportPolicy::Http2,
            "h3" => TransportPolicy::Http3,
            "auto" => TransportPolicy::Auto,
            _ => panic!("USQUE_LIVE_TRANSPORT must be auto, h2 or h3"),
        };
    }
    let selected = CatalogueStore::new(source.parent().unwrap())
        .load_selection(
            profile
                .vpn_gate
                .selection
                .as_ref()
                .expect("saved VPN Gate node"),
        )
        .unwrap();
    profile.vpn_gate.enabled = true;
    profile.frontends = FrontendSettings {
        tunnel: false,
        socks5: true,
        http: false,
    };
    profile.proxy = ProxySettings::default();
    let reservation = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    profile.proxy.socks5_listeners = vec![reservation.local_addr().unwrap()];
    drop(reservation);
    profile.kill_switch = false;
    profile.geo_direct_countries.clear();
    profile.split_exclusions.clear();
    profile.canonicalize_mode();
    assert_eq!(profile.mode, OperatingMode::Socks5);
    assert!(!profile.frontends.tunnel && !profile.proxy.system_proxy);
    let identity = service.load_warp_identity(profile.id).await.unwrap();
    let transport_identity = MasqueTlsIdentity::from_warp_identity(&identity).unwrap();
    // Pin refresh uses the normal authenticated registration path, but its
    // vault writes go to this test-owned double, never the enrolled vault.
    let refresher = Arc::new(VaultEndpointPinRefresher {
        profile_id: profile.id,
        vault: Arc::new(crate::tests::MemoryVault::default()),
        identity: Mutex::new(identity),
    });
    let _ = tracing_subscriber::fmt()
        .with_env_filter("usque_transport::vpngate=info")
        .with_ansi(false)
        .with_writer(std::io::stderr)
        .try_init();
    let cancellation = CancellationToken::new();
    let cancel = cancellation.clone();
    let timer = AbortOnDropHandle::new(tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(45)).await;
        cancel.cancel();
    }));
    let mut runtime = DataPlaneRuntime::start_with_vpngate(
        &profile,
        transport_identity,
        Arc::new(NoopSocketProtector),
        Some(refresher),
        Arc::new(GeoDirectPolicy::disabled()),
        VpnGateStart {
            selected: Some(selected),
            status: None,
            cancellation,
        },
    )
    .await
    .unwrap();
    drop(timer);
    let result = runtime.activate_final().await;
    let status = runtime.gate_status();
    let exit = if result.is_ok() {
        runtime.internal_network().probe_exit().await.ok()
    } else {
        None
    };
    runtime.shutdown().await;
    assert!(
        result.is_ok(),
        "startup: {:?}, {:?}",
        status.stage,
        status.failure
    );
    assert_eq!(status.stage, GateStage::Connected);
    assert!(exit.is_some(), "no final-channel exit IP response");
}
