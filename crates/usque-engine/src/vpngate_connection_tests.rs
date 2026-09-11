use super::*;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

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
        .connect_with_cancellation_locked(profile_id, stale)
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
