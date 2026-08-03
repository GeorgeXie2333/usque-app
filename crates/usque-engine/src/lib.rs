//! Versioned control plane for the unprivileged desktop engine.
//!
//! OS-specific Named Pipe and Unix Socket listeners are intentionally kept
//! outside this module. This service accepts already-authenticated protobuf
//! requests, serializes configuration mutations, and persists only non-secret
//! profile data through [`ConfigStore`].

use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Instant,
};

use ipnet::IpNet;
use thiserror::Error;
use tokio::sync::{Mutex, RwLock};
use usque_core::{
    AddressFamily, AppConfig, ConnectionError, ConnectionPhase, ConnectionSnapshot,
    ConnectionWarning, ConsumerRegistrationClient, DnsMode, EndpointPin, EndpointSettings,
    ErrorCode, IpPolicy, IpSbProbe, KillSwitchState, LockdownState, MasqueKeyPair, OperatingMode,
    Profile, ProxyDnsMode, ProxySettings, RegistrationOptions, StateMachine, Statistics, Transport,
    TransportPolicy, WarpIdentity, parse_manual_warp_secret,
    storage::{ConfigStore, StoreError},
};
use usque_ipc::v1::{
    self, ControlRequest, ControlResponse, StructuredError, control_request, control_response,
};
use usque_platform::{SecretRecord, SecretVault, VaultError};
use usque_transport::{
    EndpointPinRefresher, MasqueTlsIdentity, NoopSocketProtector, ProxyRuntime, RuntimeHealth,
    RuntimePath, TrafficSnapshot, TransportError, refresh_endpoint_pin_over_protected_socket,
};
use uuid::Uuid;
use zeroize::Zeroizing;

mod event_stream;
mod ipc_stream;
pub mod logging;
mod maintenance;

#[cfg(windows)]
mod windows_agent;

#[cfg(target_os = "macos")]
pub mod macos_ipc;

#[cfg(windows)]
pub mod windows_ipc;

pub struct ControlService {
    store: ConfigStore,
    config: RwLock<AppConfig>,
    state: Mutex<StateMachine>,
    mutation_lock: Mutex<()>,
    vault: Arc<dyn SecretVault>,
    data_plane: Mutex<Option<ActiveDataPlane>>,
    disconnect_cleanup: Mutex<Option<tokio::task::JoinHandle<Result<(), ControlServiceError>>>>,
    maintenance: maintenance::Maintenance,
    event_sequence: AtomicU64,
}

struct ActiveDataPlane {
    profile_id: Uuid,
    connected_at: Instant,
    last_sample_at: Instant,
    last_bytes_sent: u64,
    last_bytes_received: u64,
    runtime: ActiveRuntime,
    captive_pause_deadline: Option<Instant>,
}

enum ActiveRuntime {
    Proxy(ActiveProxyRuntime),
    #[cfg(windows)]
    Vpn(windows_agent::WindowsVpnRuntime),
}

struct ActiveProxyRuntime {
    runtime: ProxyRuntime,
    #[cfg(windows)]
    system_proxy: Option<windows_agent::WindowsSystemProxyGuard>,
}

struct VaultEndpointPinRefresher {
    profile_id: Uuid,
    vault: Arc<dyn SecretVault>,
    identity: Mutex<WarpIdentity>,
}

#[async_trait::async_trait]
impl EndpointPinRefresher for VaultEndpointPinRefresher {
    async fn refresh(
        &self,
        protector: Arc<dyn usque_transport::SocketProtector>,
    ) -> Result<MasqueTlsIdentity, TransportError> {
        let mut identity = self.identity.lock().await;
        let refresh =
            refresh_endpoint_pin_over_protected_socket(&identity, None, protector).await?;
        let previous_pin = identity.endpoint_pin.clone();
        let previous_ipv4 = identity.assigned_ipv4;
        let previous_ipv6 = identity.assigned_ipv6;
        let previous_portable = self
            .vault
            .get(self.profile_id, SecretRecord::WarpSecret)
            .await
            .map_err(|error| TransportError::EndpointPinRefresh(error.to_string()))?;

        identity.endpoint_pin = refresh.endpoint_pin;
        identity.assigned_ipv4 = refresh.assigned_ipv4;
        identity.assigned_ipv6 = refresh.assigned_ipv6;
        let refreshed_portable = if previous_portable.is_some() {
            Some(
                identity
                    .to_portable_secret_json()
                    .map_err(|error| TransportError::EndpointPinRefresh(error.to_string()))?,
            )
        } else {
            None
        };
        let updated_records = [
            (
                SecretRecord::EndpointPin,
                Zeroizing::new(identity.endpoint_pin.spki_der().to_vec()),
            ),
            (
                SecretRecord::AssignedIpv4,
                Zeroizing::new(identity.assigned_ipv4.to_string().into_bytes()),
            ),
            (
                SecretRecord::AssignedIpv6,
                Zeroizing::new(identity.assigned_ipv6.to_string().into_bytes()),
            ),
        ];
        let previous_records = [
            (
                SecretRecord::EndpointPin,
                Zeroizing::new(previous_pin.spki_der().to_vec()),
            ),
            (
                SecretRecord::AssignedIpv4,
                Zeroizing::new(previous_ipv4.to_string().into_bytes()),
            ),
            (
                SecretRecord::AssignedIpv6,
                Zeroizing::new(previous_ipv6.to_string().into_bytes()),
            ),
        ];

        let persist_result = async {
            for (record, value) in &updated_records {
                self.vault
                    .put(self.profile_id, *record, value)
                    .await
                    .map_err(|error| TransportError::EndpointPinRefresh(error.to_string()))?;
            }
            if let Some(portable) = refreshed_portable.as_ref() {
                self.vault
                    .put(
                        self.profile_id,
                        SecretRecord::WarpSecret,
                        portable.as_bytes(),
                    )
                    .await
                    .map_err(|error| TransportError::EndpointPinRefresh(error.to_string()))?;
            }
            Ok::<(), TransportError>(())
        }
        .await;

        if let Err(error) = persist_result {
            identity.endpoint_pin = previous_pin;
            identity.assigned_ipv4 = previous_ipv4;
            identity.assigned_ipv6 = previous_ipv6;
            let mut rollback_failed = false;
            for (record, value) in &previous_records {
                rollback_failed |= self
                    .vault
                    .put(self.profile_id, *record, value)
                    .await
                    .is_err();
            }
            if let Some(portable) = previous_portable.as_ref() {
                rollback_failed |= self
                    .vault
                    .put(self.profile_id, SecretRecord::WarpSecret, portable)
                    .await
                    .is_err();
            }
            return Err(if rollback_failed {
                TransportError::EndpointPinRefresh(
                    "secure storage rejected the refreshed enrollment and its rollback; the identity must be replaced"
                        .to_owned(),
                )
            } else {
                error
            });
        }

        MasqueTlsIdentity::from_warp_identity(&identity)
    }
}

impl ActiveRuntime {
    fn cancel_immediately(&mut self) {
        match self {
            Self::Proxy(runtime) => runtime.runtime.cancel_immediately(),
            #[cfg(windows)]
            Self::Vpn(runtime) => runtime.cancel_immediately(),
        }
    }

    fn path(&self) -> RuntimePath {
        match self {
            Self::Proxy(runtime) => runtime.runtime.path(),
            #[cfg(windows)]
            Self::Vpn(runtime) => runtime.path(),
        }
    }

    fn listeners(&self) -> &[SocketAddr] {
        match self {
            Self::Proxy(runtime) => runtime.runtime.listeners(),
            #[cfg(windows)]
            Self::Vpn(_) => &[],
        }
    }

    fn health(&self) -> RuntimeHealth {
        match self {
            Self::Proxy(runtime) => runtime.runtime.health(),
            #[cfg(windows)]
            Self::Vpn(runtime) => runtime.health(),
        }
    }

    fn statistics(&self) -> TrafficSnapshot {
        match self {
            Self::Proxy(runtime) => runtime.runtime.statistics(),
            #[cfg(windows)]
            Self::Vpn(runtime) => runtime.statistics(),
        }
    }

    fn failure(&self) -> Option<String> {
        match self {
            Self::Proxy(runtime) => runtime.runtime.failure(),
            #[cfg(windows)]
            Self::Vpn(runtime) => runtime.failure(),
        }
    }

    fn is_vpn(&self) -> bool {
        match self {
            Self::Proxy(_) => false,
            #[cfg(windows)]
            Self::Vpn(_) => true,
        }
    }

    #[cfg(windows)]
    fn requires_agent_reattach(&self) -> bool {
        matches!(self, Self::Vpn(runtime) if runtime.requires_agent_reattach())
    }

    #[cfg(windows)]
    async fn detach_for_agent_reattach(&mut self) -> Result<(), ControlServiceError> {
        match self {
            Self::Vpn(runtime) => runtime
                .detach_for_agent_reattach()
                .await
                .map_err(map_windows_vpn_error),
            Self::Proxy(_) => Err(ControlServiceError::InvalidRequest(
                "only an active Windows VPN can reattach to the Agent".to_owned(),
            )),
        }
    }

    async fn pause_for_captive_portal(&mut self, seconds: u32) -> Result<(), ControlServiceError> {
        match self {
            Self::Proxy(_) => Err(ControlServiceError::CaptivePortalPauseUnavailable),
            #[cfg(windows)]
            Self::Vpn(runtime) => runtime
                .pause_for_captive_portal(seconds)
                .await
                .map_err(map_windows_vpn_error),
        }
    }

    async fn shutdown(&mut self) -> Result<(), ControlServiceError> {
        match self {
            Self::Proxy(runtime) => {
                #[cfg(windows)]
                let system_proxy_result = match runtime.system_proxy.as_mut() {
                    Some(system_proxy) => {
                        system_proxy.shutdown().await.map_err(map_windows_vpn_error)
                    }
                    None => Ok(()),
                };
                runtime.runtime.shutdown().await;
                #[cfg(windows)]
                system_proxy_result?;
                Ok(())
            }
            #[cfg(windows)]
            Self::Vpn(runtime) => runtime.shutdown().await.map_err(map_windows_vpn_error),
        }
    }
}

impl ControlService {
    pub fn open(store: ConfigStore) -> Result<Self, ControlServiceError> {
        Self::open_with_vault(store, platform_vault())
    }

    pub fn open_with_vault(
        store: ConfigStore,
        vault: Arc<dyn SecretVault>,
    ) -> Result<Self, ControlServiceError> {
        let config = store.load_or_default()?;
        config
            .validate()
            .map_err(ControlServiceError::configuration)?;
        if !store.path().exists() {
            store.save(&config)?;
        }
        Ok(Self {
            maintenance: maintenance::Maintenance::new(store.path()),
            store,
            config: RwLock::new(config),
            state: Mutex::new(StateMachine::default()),
            mutation_lock: Mutex::new(()),
            vault,
            data_plane: Mutex::new(None),
            disconnect_cleanup: Mutex::new(None),
            event_sequence: AtomicU64::new(0),
        })
    }

    pub async fn config_snapshot(&self) -> AppConfig {
        self.config.read().await.clone()
    }

    /// Stops forwarding immediately, then waits for privileged platform state
    /// to be restored before the Engine process is allowed to exit.
    pub async fn shutdown(&self) -> Result<(), ControlServiceError> {
        let _mutation = self.mutation_lock.lock().await;
        self.disconnect_locked().await?;
        self.await_disconnect_cleanup().await
    }

    /// Retries secure-record deletion left pending by a previous crash or
    /// platform-vault failure. Non-secret profile deletion is committed first,
    /// so a removed profile can never be resurrected by this cleanup step.
    pub async fn reap_pending_identity_deletions(&self) -> Result<(), ControlServiceError> {
        let _mutation = self.mutation_lock.lock().await;
        self.reap_pending_identity_deletions_locked().await
    }

    /// Handles one authenticated v1 request. Application errors are returned
    /// as structured protobuf errors so the transport itself remains usable.
    pub async fn handle(&self, request: ControlRequest) -> ControlResponse {
        let request_id = request.request_id;
        let result = match request.payload {
            Some(payload) => self.handle_payload(payload).await,
            None => Err(ControlServiceError::InvalidRequest(
                "control request payload is missing".to_owned(),
            )),
        };

        match result {
            Ok(payload) => ControlResponse {
                request_id,
                error: None,
                payload: Some(payload),
            },
            Err(error) => ControlResponse {
                request_id,
                error: Some(error.as_structured_error()),
                payload: None,
            },
        }
    }

    async fn handle_payload(
        &self,
        payload: control_request::Payload,
    ) -> Result<control_response::Payload, ControlServiceError> {
        match payload {
            control_request::Payload::GetStatus(_) => {
                let snapshot = self.status_snapshot().await;
                Ok(control_response::Payload::Status(Box::new(
                    snapshot_to_proto(&snapshot),
                )))
            }
            control_request::Payload::ListProfiles(_) => Ok(
                control_response::Payload::ProfileList(self.profile_catalog().await),
            ),
            control_request::Payload::GetCapabilities(_) => Ok(
                control_response::Payload::Capabilities(current_capabilities()),
            ),
            control_request::Payload::ImportLegacyProfiles(request) => {
                self.import_legacy_profiles(request).await?;
                Ok(control_response::Payload::ProfileList(
                    self.profile_catalog().await,
                ))
            }
            control_request::Payload::UpsertProfile(request) => {
                let profile = request
                    .profile
                    .ok_or_else(|| {
                        ControlServiceError::InvalidRequest(
                            "upsert profile payload is missing".to_owned(),
                        )
                    })
                    .and_then(profile_from_proto)?;
                let stored = self.upsert_profile(profile).await?;
                Ok(control_response::Payload::Profile(Box::new(
                    profile_to_proto(&stored),
                )))
            }
            control_request::Payload::DeleteProfile(request) => {
                let id = parse_profile_id(&request.profile_id)?;
                self.delete_profile(id).await?;
                Ok(control_response::Payload::Empty(v1::Empty {}))
            }
            control_request::Payload::SetActiveProfile(request) => {
                let id = parse_profile_id(&request.profile_id)?;
                self.set_active_profile(id).await?;
                Ok(control_response::Payload::Empty(v1::Empty {}))
            }
            control_request::Payload::ResetProfile(request) => {
                let id = parse_profile_id(&request.profile_id)?;
                let profile = self.reset_profile(id).await?;
                Ok(control_response::Payload::Profile(Box::new(
                    profile_to_proto(&profile),
                )))
            }
            control_request::Payload::Disconnect(_) => {
                let snapshot = self.disconnect().await?;
                Ok(control_response::Payload::Status(Box::new(
                    snapshot_to_proto(&snapshot),
                )))
            }
            control_request::Payload::Connect(request) => {
                let id = parse_profile_id(&request.profile_id)?;
                let snapshot = self.connect(id).await?;
                Ok(control_response::Payload::Status(Box::new(
                    snapshot_to_proto(&snapshot),
                )))
            }
            control_request::Payload::ProvisionIdentity(request) => {
                self.provision_identity(request).await?;
                Ok(control_response::Payload::Empty(v1::Empty {}))
            }
            control_request::Payload::CreateProfileWithIdentity(request) => {
                let profile = request
                    .profile
                    .ok_or_else(|| {
                        ControlServiceError::InvalidRequest(
                            "create profile payload is missing".to_owned(),
                        )
                    })
                    .and_then(profile_from_proto)?;
                let identity = request.identity.ok_or_else(|| {
                    ControlServiceError::InvalidRequest(
                        "identity provisioning payload is missing".to_owned(),
                    )
                })?;
                self.create_profile_with_identity(profile, identity).await?;
                Ok(control_response::Payload::ProfileList(
                    self.profile_catalog().await,
                ))
            }
            control_request::Payload::Retry(_) => {
                let snapshot = self.retry().await?;
                Ok(control_response::Payload::Status(Box::new(
                    snapshot_to_proto(&snapshot),
                )))
            }
            control_request::Payload::ClearAllData(request) => {
                self.clear_all_data(request.confirmed).await?;
                Ok(control_response::Payload::Empty(v1::Empty {}))
            }
            control_request::Payload::PauseCaptivePortal(request) => {
                let snapshot = self.pause_captive_portal(request.seconds).await?;
                Ok(control_response::Payload::Status(Box::new(
                    snapshot_to_proto(&snapshot),
                )))
            }
            control_request::Payload::CheckUpdate(request) => {
                let enabled = self.config.read().await.preferences.update_check_enabled;
                let update = self
                    .maintenance
                    .check_update(request.manual, enabled)
                    .await?;
                Ok(control_response::Payload::Update(v1::UpdateInfo {
                    available: update.available,
                    version: update.version,
                    release_url: update.release_url,
                }))
            }
            control_request::Payload::ExportDiagnostics(request) => {
                let destination = request.destination.trim();
                if destination.is_empty() {
                    return Err(ControlServiceError::InvalidRequest(
                        "a diagnostic bundle destination is required".to_owned(),
                    ));
                }
                let config = self.config.read().await.clone();
                let snapshot = self.status_snapshot().await;
                self.maintenance
                    .export_diagnostics(destination.into(), config, snapshot)
                    .await?;
                Ok(control_response::Payload::Empty(v1::Empty {}))
            }
        }
    }

    async fn status_snapshot(&self) -> ConnectionSnapshot {
        self.resume_captive_portal_if_due().await;
        let mut data_plane = self.data_plane.lock().await;
        let mut state = self.state.lock().await;
        if let Some(active) = data_plane.as_mut() {
            if let Some(deadline) = active.captive_pause_deadline {
                let remaining = deadline
                    .checked_duration_since(Instant::now())
                    .map(|duration| {
                        u32::try_from(duration.as_secs().saturating_add(1)).unwrap_or(u32::MAX)
                    })
                    .unwrap_or(0);
                state.update_captive_portal_pause_remaining(remaining);
            }
            match active.runtime.health() {
                RuntimeHealth::Connected { path, .. }
                    if state.snapshot().phase == ConnectionPhase::Reconnecting =>
                {
                    if let Err(error) = state.mark_connected(
                        path.transport,
                        path.endpoint_family,
                        path.ipv4_available,
                        path.ipv6_available,
                    ) {
                        state.mark_error(ConnectionError {
                            code: ErrorCode::Internal,
                            message: error.to_string(),
                            retryable: false,
                        });
                    }
                }
                RuntimeHealth::Reconnecting { .. }
                    if matches!(
                        state.snapshot().phase,
                        ConnectionPhase::Connected | ConnectionPhase::Degraded
                    ) =>
                {
                    if let Err(error) = state.transition(ConnectionPhase::Reconnecting) {
                        state.mark_error(ConnectionError {
                            code: ErrorCode::Internal,
                            message: error.to_string(),
                            retryable: false,
                        });
                    }
                }
                RuntimeHealth::Failed { message, .. } => {
                    state.mark_error(ConnectionError {
                        code: ErrorCode::TransportUnavailable,
                        message,
                        retryable: false,
                    });
                }
                _ => {}
            }
            state.update_reconnect_count(active.runtime.health().reconnect_count());
            let traffic = active.runtime.statistics();
            let now = Instant::now();
            let sample_seconds = now.duration_since(active.last_sample_at).as_secs_f64();
            let upload_rate =
                rate_since(traffic.bytes_sent, active.last_bytes_sent, sample_seconds);
            let download_rate = rate_since(
                traffic.bytes_received,
                active.last_bytes_received,
                sample_seconds,
            );
            active.last_sample_at = now;
            active.last_bytes_sent = traffic.bytes_sent;
            active.last_bytes_received = traffic.bytes_received;
            state.update_statistics(Statistics {
                connected_seconds: active.connected_at.elapsed().as_secs(),
                bytes_sent: traffic.bytes_sent,
                bytes_received: traffic.bytes_received,
                current_upload_bytes_per_second: upload_rate,
                current_download_bytes_per_second: download_rate,
            });
            if let Some(message) = active.runtime.failure()
                && matches!(
                    state.snapshot().phase,
                    ConnectionPhase::Connected | ConnectionPhase::Degraded
                )
            {
                state.mark_error(ConnectionError {
                    code: ErrorCode::TransportUnavailable,
                    message,
                    retryable: true,
                });
            }
        }
        state.snapshot().clone()
    }

    async fn resume_captive_portal_if_due(&self) {
        let due = self
            .data_plane
            .lock()
            .await
            .as_ref()
            .and_then(|active| active.captive_pause_deadline)
            .is_some_and(|deadline| deadline <= Instant::now());
        if !due {
            return;
        }
        // A status/event tick must never deadlock a user mutation already in
        // progress. The next tick retries automatic resume if the lock is busy.
        let Ok(_mutation) = self.mutation_lock.try_lock() else {
            return;
        };
        let profile_id = {
            let data_plane = self.data_plane.lock().await;
            let Some(active) = data_plane.as_ref() else {
                return;
            };
            if active
                .captive_pause_deadline
                .is_none_or(|deadline| deadline > Instant::now())
            {
                return;
            }
            active.profile_id
        };
        if let Err(error) = self.disconnect_locked().await {
            tracing::warn!(%error, "failed to clear a completed captive-portal pause");
            return;
        }
        if let Err(error) = self.connect_locked(profile_id).await {
            tracing::warn!(%error, "automatic VPN resume after captive-portal pause failed");
        }
    }

    pub(crate) async fn event_snapshot(&self) -> v1::ConnectionSnapshot {
        snapshot_to_proto(&self.status_snapshot().await)
    }

    pub(crate) fn next_event_sequence(&self) -> u64 {
        self.event_sequence.fetch_add(1, Ordering::Relaxed) + 1
    }

    async fn connect(&self, profile_id: Uuid) -> Result<ConnectionSnapshot, ControlServiceError> {
        let _mutation = self.mutation_lock.lock().await;
        self.connect_locked(profile_id).await
    }

    async fn connect_locked(
        &self,
        profile_id: Uuid,
    ) -> Result<ConnectionSnapshot, ControlServiceError> {
        self.await_disconnect_cleanup().await?;
        {
            let data_plane = self.data_plane.lock().await;
            if let Some(active) = data_plane.as_ref() {
                if active.profile_id == profile_id {
                    drop(data_plane);
                    return Ok(self.state.lock().await.snapshot().clone());
                }
                return Err(ControlServiceError::AlreadyConnected(active.profile_id));
            }
        }

        let profile = self
            .config
            .read()
            .await
            .profiles
            .iter()
            .find(|profile| profile.id == profile_id)
            .cloned()
            .ok_or(ControlServiceError::ProfileNotFound(profile_id))?;
        if profile.mode == OperatingMode::Vpn && !cfg!(windows) {
            return Err(ControlServiceError::OperatingModeUnavailable(profile.mode));
        }

        {
            let mut state = self.state.lock().await;
            state.transition(ConnectionPhase::Preparing)?;
        }
        let warp_identity = match self.load_warp_identity(profile_id).await {
            Ok(identity) => identity,
            Err(error) => {
                self.mark_connection_error(&error).await;
                return Err(error);
            }
        };
        let identity = match MasqueTlsIdentity::from_warp_identity(&warp_identity) {
            Ok(identity) => identity,
            Err(error) => {
                let error = ControlServiceError::Transport(error);
                self.mark_connection_error(&error).await;
                return Err(error);
            }
        };
        let pin_refresher: Arc<dyn EndpointPinRefresher> = Arc::new(VaultEndpointPinRefresher {
            profile_id,
            vault: Arc::clone(&self.vault),
            identity: Mutex::new(warp_identity),
        });

        {
            let mut state = self.state.lock().await;
            match profile.transport {
                TransportPolicy::Auto => {
                    state.transition(ConnectionPhase::ConnectingHttp3)?;
                }
                TransportPolicy::Http3 => {
                    state.transition(ConnectionPhase::ConnectingHttp3)?;
                }
                TransportPolicy::Http2 => {
                    state.transition(ConnectionPhase::ConnectingHttp2)?;
                }
            }
        }

        let runtime = if profile.mode == OperatingMode::Vpn {
            #[cfg(windows)]
            {
                match windows_agent::WindowsVpnRuntime::start(
                    &profile,
                    identity,
                    Arc::clone(&pin_refresher),
                )
                .await
                {
                    Ok(runtime) => ActiveRuntime::Vpn(runtime),
                    Err(error) => {
                        let error = map_windows_vpn_error(error);
                        self.mark_connection_error(&error).await;
                        return Err(error);
                    }
                }
            }
            #[cfg(not(windows))]
            {
                unreachable!("non-Windows VPN mode was rejected before identity loading")
            }
        } else {
            match ProxyRuntime::start_with_refresh(
                &profile,
                identity,
                Arc::new(NoopSocketProtector),
                Some(pin_refresher),
            )
            .await
            {
                Ok(mut runtime) => {
                    #[cfg(windows)]
                    let system_proxy = if profile.mode == OperatingMode::HttpProxy
                        && profile.proxy.system_proxy
                    {
                        let listener = runtime
                            .listeners()
                            .iter()
                            .copied()
                            .find(|listener| listener.ip().is_loopback() && listener.ip().is_ipv4())
                            .or_else(|| {
                                runtime
                                    .listeners()
                                    .iter()
                                    .copied()
                                    .find(|listener| listener.ip().is_loopback())
                            });
                        let Some(listener) = listener else {
                            runtime.shutdown().await;
                            let error = ControlServiceError::PlatformVpn(
                                "system proxy requires a Loopback HTTP listener".to_owned(),
                            );
                            self.mark_connection_error(&error).await;
                            return Err(error);
                        };
                        match windows_agent::WindowsSystemProxyGuard::start(listener).await {
                            Ok(system_proxy) => Some(system_proxy),
                            Err(error) => {
                                runtime.shutdown().await;
                                let error = map_windows_vpn_error(error);
                                self.mark_connection_error(&error).await;
                                return Err(error);
                            }
                        }
                    } else {
                        None
                    };
                    ActiveRuntime::Proxy(ActiveProxyRuntime {
                        runtime,
                        #[cfg(windows)]
                        system_proxy,
                    })
                }
                Err(error) => {
                    let error = ControlServiceError::Transport(error);
                    self.mark_connection_error(&error).await;
                    return Err(error);
                }
            }
        };
        let path = runtime.path();
        let exit_listener = runtime
            .listeners()
            .iter()
            .copied()
            .find(|address| address.ip().is_loopback());
        let flag_cache = self
            .store
            .path()
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join("cache")
            .join("flag-icons-7.5.0");
        let exit_probe = match (profile.mode, exit_listener) {
            (OperatingMode::Socks5, Some(listener)) => IpSbProbe::through_socks(listener)
                .ok()
                .map(|probe| probe.with_flag_cache(&flag_cache)),
            (OperatingMode::HttpProxy, Some(listener)) => IpSbProbe::through_http(listener)
                .ok()
                .map(|probe| probe.with_flag_cache(&flag_cache)),
            (OperatingMode::Vpn, _) => IpSbProbe::new()
                .ok()
                .map(|probe| probe.with_flag_cache(&flag_cache)),
            _ => None,
        };
        let exit = match exit_probe {
            Some(probe) => probe.probe().await.ok(),
            None => None,
        };
        let snapshot = {
            let mut state = self.state.lock().await;
            if profile.transport == TransportPolicy::Auto && path.transport == Transport::Http2 {
                state.transition(ConnectionPhase::ConnectingHttp2)?;
            }
            state.mark_connected(
                path.transport,
                path.endpoint_family,
                path.ipv4_available,
                path.ipv6_available,
            )?;
            if let Some(exit) = exit {
                state.set_exit_info(exit);
            }
            let mut warnings = Vec::new();
            if profile.proxy.exposes_lan(profile.mode) {
                warnings.push(ConnectionWarning {
                    code: "LAN_EXPOSED".to_owned(),
                    message:
                        "The proxy accepts non-loopback clients without username/password authentication."
                            .to_owned(),
                });
            }
            if profile.proxy.dns_mode != ProxyDnsMode::Remote {
                warnings.push(ConnectionWarning {
                    code: "LOCAL_DNS_LEAK_RISK".to_owned(),
                    message:
                        "Proxy domain resolution is using local or system DNS outside the tunnel."
                            .to_owned(),
                });
            }
            if profile.mode == OperatingMode::Vpn && !profile.kill_switch {
                warnings.push(ConnectionWarning {
                    code: "KILL_SWITCH_DISABLED".to_owned(),
                    message:
                        "Traffic may leave the physical network if the VPN data path is unavailable."
                            .to_owned(),
                });
            }
            state.update_runtime_metadata(
                runtime.health().reconnect_count(),
                runtime
                    .listeners()
                    .iter()
                    .map(ToString::to_string)
                    .collect(),
                warnings,
            );
            state.update_safety_state(
                if profile.mode == OperatingMode::Vpn {
                    if profile.kill_switch {
                        KillSwitchState::Active
                    } else {
                        KillSwitchState::Inactive
                    }
                } else {
                    KillSwitchState::NotApplicable
                },
                LockdownState::NotSupported,
            );
            state.snapshot().clone()
        };
        *self.data_plane.lock().await = Some(ActiveDataPlane {
            profile_id,
            connected_at: Instant::now(),
            last_sample_at: Instant::now(),
            last_bytes_sent: 0,
            last_bytes_received: 0,
            runtime,
            captive_pause_deadline: None,
        });
        Ok(snapshot)
    }

    async fn disconnect(&self) -> Result<ConnectionSnapshot, ControlServiceError> {
        let _mutation = self.mutation_lock.lock().await;
        self.disconnect_locked().await
    }

    async fn disconnect_locked(&self) -> Result<ConnectionSnapshot, ControlServiceError> {
        let mut data_plane = self.data_plane.lock().await;
        let phase = self.state.lock().await.snapshot().phase;
        if phase == ConnectionPhase::Disconnected && data_plane.is_none() {
            return Ok(self.state.lock().await.snapshot().clone());
        }
        {
            let mut state = self.state.lock().await;
            if state.snapshot().phase != ConnectionPhase::Disconnecting {
                state.transition(ConnectionPhase::Disconnecting)?;
            }
        }
        if let Some(mut active) = data_plane.take() {
            // Stop accepting and forwarding traffic synchronously. Platform
            // rollback (routes, WFP, DNS and system proxy) can take seconds and
            // must not keep the Disconnect action or data plane alive.
            active.runtime.cancel_immediately();
            drop(data_plane);

            let cleanup = tokio::spawn(async move { active.runtime.shutdown().await });
            let mut pending = self.disconnect_cleanup.lock().await;
            debug_assert!(
                pending.is_none(),
                "a previous disconnect cleanup is still pending"
            );
            if pending.is_some() {
                tracing::error!(
                    "disconnect cleanup invariant violated; detaching the older cleanup task"
                );
            }
            *pending = Some(cleanup);
        } else {
            drop(data_plane);
        }
        let snapshot = self
            .state
            .lock()
            .await
            .transition(ConnectionPhase::Disconnected)?
            .clone();
        Ok(snapshot)
    }

    async fn await_disconnect_cleanup(&self) -> Result<(), ControlServiceError> {
        let cleanup = self.disconnect_cleanup.lock().await.take();
        let Some(cleanup) = cleanup else {
            return Ok(());
        };
        cleanup
            .await
            .map_err(|error| ControlServiceError::DisconnectCleanup(error.to_string()))?
    }

    async fn retry(&self) -> Result<ConnectionSnapshot, ControlServiceError> {
        let _mutation = self.mutation_lock.lock().await;
        let connected_profile = self
            .data_plane
            .lock()
            .await
            .as_ref()
            .map(|active| active.profile_id);
        let profile_id = match connected_profile {
            Some(profile_id) => Some(profile_id),
            None => self.config.read().await.active_profile_id,
        }
        .ok_or_else(|| {
            ControlServiceError::InvalidRequest("an active profile is required".to_owned())
        })?;

        #[cfg(windows)]
        {
            let mut data_plane = self.data_plane.lock().await;
            if data_plane
                .as_ref()
                .is_some_and(|active| active.runtime.requires_agent_reattach())
            {
                let mut active = data_plane.take().expect("checked active data plane");
                active.runtime.detach_for_agent_reattach().await?;
                drop(data_plane);
                // The Agent journal remains Active and WFP stays fail-closed.
                // `connect_locked` detects that transaction and recreates only
                // MASQUE plus the volatile packet session.
                return self.connect_locked(profile_id).await;
            }
        }

        self.disconnect_locked().await?;
        self.connect_locked(profile_id).await
    }

    async fn pause_captive_portal(
        &self,
        seconds: u32,
    ) -> Result<ConnectionSnapshot, ControlServiceError> {
        if !(1..=600).contains(&seconds) {
            return Err(ControlServiceError::InvalidRequest(
                "captive portal pause must be between 1 and 600 seconds".to_owned(),
            ));
        }
        let _mutation = self.mutation_lock.lock().await;
        let mut data_plane = self.data_plane.lock().await;
        let active = data_plane
            .as_mut()
            .ok_or(ControlServiceError::CaptivePortalPauseUnavailable)?;
        if !active.runtime.is_vpn() || !current_capabilities().vpn {
            return Err(ControlServiceError::CaptivePortalPauseUnavailable);
        }
        active.runtime.pause_for_captive_portal(seconds).await?;
        active.captive_pause_deadline =
            Some(Instant::now() + std::time::Duration::from_secs(u64::from(seconds)));
        let snapshot = {
            let mut state = self.state.lock().await;
            state.transition(ConnectionPhase::CaptivePortalPaused)?;
            state.update_captive_portal_pause_remaining(seconds);
            state.update_safety_state(KillSwitchState::Paused, LockdownState::NotSupported);
            state.snapshot().clone()
        };
        Ok(snapshot)
    }

    async fn clear_all_data(&self, confirmed: bool) -> Result<(), ControlServiceError> {
        if !confirmed {
            return Err(ControlServiceError::ConfirmationRequired);
        }
        let _mutation = self.mutation_lock.lock().await;
        self.disconnect_locked().await?;
        self.await_disconnect_cleanup().await?;
        let config = self.config.read().await;
        let profile_ids = config
            .profiles
            .iter()
            .map(|profile| profile.id)
            .chain(config.pending_identity_deletions.iter().copied())
            .collect::<std::collections::HashSet<_>>();
        drop(config);
        for profile_id in profile_ids {
            self.vault.delete_identity(profile_id).await?;
        }
        self.persist(AppConfig::default()).await?;
        self.maintenance.clear_local_state().await?;
        Ok(())
    }

    async fn load_warp_identity(
        &self,
        profile_id: Uuid,
    ) -> Result<WarpIdentity, ControlServiceError> {
        let private_key = self
            .required_secret(profile_id, SecretRecord::MasquePrivateKey)
            .await?;
        let endpoint_pin = self
            .required_secret(profile_id, SecretRecord::EndpointPin)
            .await?;
        let assigned_ipv4 = self
            .required_secret(profile_id, SecretRecord::AssignedIpv4)
            .await?;
        let assigned_ipv6 = self
            .required_secret(profile_id, SecretRecord::AssignedIpv6)
            .await?;
        let access_token = self
            .required_secret(profile_id, SecretRecord::AccessToken)
            .await?;
        let device_id = self
            .required_secret(profile_id, SecretRecord::DeviceId)
            .await?;
        let license = self.vault.get(profile_id, SecretRecord::License).await?;
        let key_pair = MasqueKeyPair::from_sec1_der(&private_key)?;
        let endpoint_pin = EndpointPin::from_spki_der(&endpoint_pin)?;
        let assigned_ipv4 = std::str::from_utf8(&assigned_ipv4)
            .map_err(|_| ControlServiceError::InvalidStoredIdentity)?
            .parse()
            .map_err(|_| ControlServiceError::InvalidStoredIdentity)?;
        let assigned_ipv6 = std::str::from_utf8(&assigned_ipv6)
            .map_err(|_| ControlServiceError::InvalidStoredIdentity)?
            .parse()
            .map_err(|_| ControlServiceError::InvalidStoredIdentity)?;
        let device_id = String::from_utf8(device_id.to_vec())
            .map_err(|_| ControlServiceError::InvalidStoredIdentity)?;
        let access_token = String::from_utf8(access_token.to_vec())
            .map_err(|_| ControlServiceError::InvalidStoredIdentity)?;
        let license = license
            .map(|value| {
                String::from_utf8(value.to_vec())
                    .map_err(|_| ControlServiceError::InvalidStoredIdentity)
            })
            .transpose()?;
        WarpIdentity::from_secure_records(
            key_pair,
            endpoint_pin,
            device_id,
            access_token,
            license,
            assigned_ipv4,
            assigned_ipv6,
        )
        .map_err(Into::into)
    }

    async fn required_secret(
        &self,
        profile_id: Uuid,
        record: SecretRecord,
    ) -> Result<Zeroizing<Vec<u8>>, ControlServiceError> {
        self.vault
            .get(profile_id, record)
            .await?
            .ok_or(ControlServiceError::MissingCredential(record.key()))
    }

    async fn mark_connection_error(&self, error: &ControlServiceError) {
        self.state
            .lock()
            .await
            .mark_error(connection_error_for(error));
    }

    async fn provision_identity(
        &self,
        request: v1::ProvisionIdentityRequest,
    ) -> Result<(), ControlServiceError> {
        if !request.terms_accepted {
            return Err(ControlServiceError::TermsNotAccepted);
        }
        let profile_id = parse_profile_id(&request.profile_id)?;
        if !self
            .config
            .read()
            .await
            .profiles
            .iter()
            .any(|profile| profile.id == profile_id)
        {
            return Err(ControlServiceError::ProfileNotFound(profile_id));
        }

        let _mutation = self.mutation_lock.lock().await;
        let manual_secret = Zeroizing::new(request.warp_secret);
        let identity = if manual_secret.is_empty() {
            let options = RegistrationOptions {
                terms_accepted: true,
                model: desktop_registration_model().to_owned(),
                device_name: nonempty(request.device_name),
                locale: if request.locale.trim().is_empty() {
                    "en_US".to_owned()
                } else {
                    request.locale
                },
            };
            ConsumerRegistrationClient::new()?
                .register(&options)
                .await?
        } else {
            let value = std::str::from_utf8(&manual_secret)
                .map_err(|_| ControlServiceError::InvalidManualSecretEncoding)?;
            parse_manual_warp_secret(value)?
        };

        self.persist_identity(
            profile_id,
            &identity,
            (!manual_secret.is_empty()).then_some(manual_secret.as_slice()),
        )
        .await
    }

    async fn create_profile_with_identity(
        &self,
        profile: Profile,
        provisioning: v1::IdentityProvisioning,
    ) -> Result<(), ControlServiceError> {
        let profile_id = profile.id;
        profile
            .validate()
            .map_err(ControlServiceError::configuration)?;
        if !provisioning.terms_accepted {
            return Err(ControlServiceError::TermsNotAccepted);
        }
        if self
            .config
            .read()
            .await
            .profiles
            .iter()
            .any(|existing| existing.id == profile.id)
        {
            return Err(ControlServiceError::InvalidRequest(format!(
                "profile already exists: {}",
                profile.id
            )));
        }

        let secret = Zeroizing::new(provisioning.warp_secret);
        let method = v1::IdentityProvisioningMethod::try_from(provisioning.method)
            .unwrap_or(v1::IdentityProvisioningMethod::Unspecified);
        let identity = match method {
            v1::IdentityProvisioningMethod::Register => {
                if !secret.is_empty() {
                    return Err(ControlServiceError::InvalidRequest(
                        "registration provisioning must not contain a WARP Secret".to_owned(),
                    ));
                }
                let options = RegistrationOptions {
                    terms_accepted: true,
                    model: desktop_registration_model().to_owned(),
                    device_name: nonempty(provisioning.device_name),
                    locale: if provisioning.locale.trim().is_empty() {
                        "en_US".to_owned()
                    } else {
                        provisioning.locale
                    },
                };
                ConsumerRegistrationClient::new()?
                    .register(&options)
                    .await?
            }
            v1::IdentityProvisioningMethod::ImportSecret => {
                if secret.is_empty() {
                    return Err(ControlServiceError::InvalidRequest(
                        "import provisioning requires a WARP Secret".to_owned(),
                    ));
                }
                let value = std::str::from_utf8(&secret)
                    .map_err(|_| ControlServiceError::InvalidManualSecretEncoding)?;
                parse_manual_warp_secret(value)?
            }
            v1::IdentityProvisioningMethod::Unspecified => {
                return Err(ControlServiceError::InvalidRequest(
                    "identity provisioning method is missing".to_owned(),
                ));
            }
        };

        let _mutation = self.mutation_lock.lock().await;
        let mut pending = self.config.read().await.clone();
        if pending
            .profiles
            .iter()
            .any(|existing| existing.id == profile.id)
            || pending.pending_identity_creations.contains(&profile.id)
        {
            return Err(ControlServiceError::InvalidRequest(format!(
                "profile already exists or is being created: {}",
                profile.id
            )));
        }
        pending.pending_identity_creations.push(profile.id);
        self.persist(pending).await?;

        if let Err(error) = self
            .persist_identity(
                profile.id,
                &identity,
                matches!(method, v1::IdentityProvisioningMethod::ImportSecret)
                    .then_some(secret.as_slice()),
            )
            .await
        {
            self.abort_pending_identity_creation(profile.id).await;
            return Err(error);
        }

        let mut committed = self.config.read().await.clone();
        committed
            .pending_identity_creations
            .retain(|profile_id| *profile_id != profile.id);
        committed.profiles.push(profile);
        if let Err(error) = self.persist(committed).await {
            let _ = self.vault.delete_identity(profile_id).await;
            self.abort_pending_identity_creation(profile_id).await;
            return Err(error);
        }
        Ok(())
    }

    async fn abort_pending_identity_creation(&self, profile_id: Uuid) {
        let _ = self.vault.delete_identity(profile_id).await;
        let mut next = self.config.read().await.clone();
        next.pending_identity_creations
            .retain(|pending| *pending != profile_id);
        let _ = self.persist(next).await;
    }

    async fn profile_catalog(&self) -> v1::ProfileList {
        let config = self.config.read().await.clone();
        let mut catalog = profile_list_to_proto(&config);
        for profile in &config.profiles {
            let state = match self.load_warp_identity(profile.id).await {
                Ok(_) => v1::ProfileIdentityState::Ready,
                Err(ControlServiceError::MissingCredential(_)) => v1::ProfileIdentityState::Missing,
                Err(_) => v1::ProfileIdentityState::Invalid,
            };
            catalog.identity_statuses.push(v1::ProfileIdentityStatus {
                profile_id: profile.id.to_string(),
                state: state as i32,
            });
        }
        catalog
    }

    async fn persist_identity(
        &self,
        profile_id: Uuid,
        identity: &WarpIdentity,
        manual_secret: Option<&[u8]>,
    ) -> Result<(), ControlServiceError> {
        let mut records = Vec::with_capacity(8);
        if let Some(secret) = manual_secret {
            records.push((SecretRecord::WarpSecret, Zeroizing::new(secret.to_vec())));
        }
        records.push((
            SecretRecord::MasquePrivateKey,
            identity.key_pair.private_sec1_der()?,
        ));
        records.push((
            SecretRecord::AccessToken,
            Zeroizing::new(identity.access_token().as_bytes().to_vec()),
        ));
        records.push((
            SecretRecord::DeviceId,
            Zeroizing::new(identity.device_id().as_bytes().to_vec()),
        ));
        if let Some(license) = identity.license() {
            records.push((
                SecretRecord::License,
                Zeroizing::new(license.as_bytes().to_vec()),
            ));
        }
        records.push((
            SecretRecord::EndpointPin,
            Zeroizing::new(identity.endpoint_pin.spki_der().to_vec()),
        ));
        records.push((
            SecretRecord::AssignedIpv4,
            Zeroizing::new(identity.assigned_ipv4.to_string().into_bytes()),
        ));
        records.push((
            SecretRecord::AssignedIpv6,
            Zeroizing::new(identity.assigned_ipv6.to_string().into_bytes()),
        ));

        for (record, value) in records {
            if let Err(error) = self.vault.put(profile_id, record, &value).await {
                let _ = self.vault.delete_identity(profile_id).await;
                return Err(error.into());
            }
        }
        if manual_secret.is_none() {
            self.vault
                .delete(profile_id, SecretRecord::WarpSecret)
                .await?;
        }
        if identity.license().is_none() {
            self.vault.delete(profile_id, SecretRecord::License).await?;
        }
        Ok(())
    }

    async fn upsert_profile(&self, profile: Profile) -> Result<Profile, ControlServiceError> {
        profile
            .validate()
            .map_err(ControlServiceError::configuration)?;
        let _mutation = self.mutation_lock.lock().await;
        let mut next = self.config.read().await.clone();
        match next
            .profiles
            .iter()
            .position(|existing| existing.id == profile.id)
        {
            Some(index) => next.profiles[index] = profile.clone(),
            None => next.profiles.push(profile.clone()),
        }
        if next.active_profile_id.is_none() {
            next.active_profile_id = Some(profile.id);
        }
        self.persist(next).await?;
        Ok(profile)
    }

    async fn import_legacy_profiles(
        &self,
        request: v1::ImportLegacyProfilesRequest,
    ) -> Result<v1::ProfileList, ControlServiceError> {
        let profiles = request
            .profiles
            .into_iter()
            .map(profile_from_proto)
            .collect::<Result<Vec<_>, _>>()?;
        let active_profile_id = if request.active_profile_id.trim().is_empty() {
            None
        } else {
            Some(parse_profile_id(&request.active_profile_id)?)
        };
        let _mutation = self.mutation_lock.lock().await;
        let mut next = self.config.read().await.clone();
        if !next.preferences.profiles_migrated_from_flutter {
            let mut incoming_ids = std::collections::HashSet::new();
            for profile in profiles {
                if !incoming_ids.insert(profile.id) {
                    return Err(ControlServiceError::InvalidRequest(
                        "legacy profile IDs must be unique".to_owned(),
                    ));
                }
                match next
                    .profiles
                    .iter()
                    .position(|existing| existing.id == profile.id)
                {
                    Some(index) => next.profiles[index] = profile,
                    None => next.profiles.push(profile),
                }
            }
            if let Some(active_profile_id) = active_profile_id {
                if !next
                    .profiles
                    .iter()
                    .any(|profile| profile.id == active_profile_id)
                {
                    return Err(ControlServiceError::InvalidRequest(
                        "legacy active profile does not exist".to_owned(),
                    ));
                }
                next.active_profile_id = Some(active_profile_id);
            }
            next.preferences.profiles_migrated_from_flutter = true;
            self.persist(next).await?;
        }
        let config = self.config.read().await;
        Ok(profile_list_to_proto(&config))
    }

    async fn delete_profile(&self, id: Uuid) -> Result<(), ControlServiceError> {
        let _mutation = self.mutation_lock.lock().await;
        let mut next = self.config.read().await.clone();
        let Some(index) = next.profiles.iter().position(|profile| profile.id == id) else {
            return Err(ControlServiceError::ProfileNotFound(id));
        };
        if next.profiles.len() == 1 {
            return Err(ControlServiceError::LastProfile);
        }
        next.profiles.remove(index);
        if next.active_profile_id == Some(id) {
            next.active_profile_id = next.profiles.first().map(|profile| profile.id);
        }
        if !next.pending_identity_deletions.contains(&id) {
            next.pending_identity_deletions.push(id);
        }
        self.persist(next).await?;
        self.reap_pending_identity_deletions_locked().await
    }

    async fn set_active_profile(&self, id: Uuid) -> Result<(), ControlServiceError> {
        let _mutation = self.mutation_lock.lock().await;
        let mut next = self.config.read().await.clone();
        if !next.profiles.iter().any(|profile| profile.id == id) {
            return Err(ControlServiceError::ProfileNotFound(id));
        }
        next.active_profile_id = Some(id);
        self.persist(next).await
    }

    async fn reset_profile(&self, id: Uuid) -> Result<Profile, ControlServiceError> {
        let _mutation = self.mutation_lock.lock().await;
        let mut next = self.config.read().await.clone();
        let profile = next
            .profiles
            .iter_mut()
            .find(|profile| profile.id == id)
            .ok_or(ControlServiceError::ProfileNotFound(id))?;
        profile.reset_network_defaults();
        let profile = profile.clone();
        self.persist(next).await?;
        Ok(profile)
    }

    async fn reap_pending_identity_deletions_locked(&self) -> Result<(), ControlServiceError> {
        let snapshot = self.config.read().await.clone();
        let pending = snapshot.pending_identity_deletions;
        let pending_creations = snapshot.pending_identity_creations;
        if pending.is_empty() && pending_creations.is_empty() {
            return Ok(());
        }

        let mut completed = std::collections::HashSet::new();
        let mut first_error = None;
        for profile_id in pending {
            match self.vault.delete_identity(profile_id).await {
                Ok(()) => {
                    completed.insert(profile_id);
                }
                Err(error) if first_error.is_none() => first_error = Some(error),
                Err(_) => {}
            }
        }
        let mut completed_creations = std::collections::HashSet::new();
        for profile_id in pending_creations {
            match self.vault.delete_identity(profile_id).await {
                Ok(()) => {
                    completed_creations.insert(profile_id);
                }
                Err(error) if first_error.is_none() => first_error = Some(error),
                Err(_) => {}
            }
        }
        if !completed.is_empty() || !completed_creations.is_empty() {
            let mut next = self.config.read().await.clone();
            next.pending_identity_deletions
                .retain(|profile_id| !completed.contains(profile_id));
            next.pending_identity_creations
                .retain(|profile_id| !completed_creations.contains(profile_id));
            self.persist(next).await?;
        }
        first_error.map_or(Ok(()), |error| Err(error.into()))
    }

    async fn persist(&self, next: AppConfig) -> Result<(), ControlServiceError> {
        next.validate()
            .map_err(ControlServiceError::configuration)?;
        let store = self.store.clone();
        let persisted = next.clone();
        tokio::task::spawn_blocking(move || store.save(&persisted))
            .await
            .map_err(|error| ControlServiceError::PersistenceWorker(error.to_string()))??;
        *self.config.write().await = next;
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum ControlServiceError {
    #[error("invalid request: {0}")]
    InvalidRequest(String),
    #[error("invalid profile configuration: {0}")]
    InvalidConfiguration(String),
    #[error("profile does not exist: {0}")]
    ProfileNotFound(Uuid),
    #[error("profile {0} is already connected")]
    AlreadyConnected(Uuid),
    #[error("at least one profile must remain")]
    LastProfile,
    #[error("{0:?} mode is not available in this build; select a proxy mode")]
    OperatingModeUnavailable(OperatingMode),
    #[error("the destructive operation requires explicit confirmation")]
    ConfirmationRequired,
    #[error("the secure identity record is missing: {0}")]
    MissingCredential(&'static str),
    #[error("the secure identity records are malformed")]
    InvalidStoredIdentity,
    #[error("the connection state machine rejected an operation: {0}")]
    State(#[from] usque_core::state::TransitionError),
    #[error("the MASQUE data plane failed: {0}")]
    Transport(#[from] TransportError),
    #[error("the Windows platform VPN failed: {0}")]
    PlatformVpn(String),
    #[error("captive portal pause is only available for an active platform VPN")]
    CaptivePortalPauseUnavailable,
    #[error("Cloudflare terms must be accepted before identity provisioning")]
    TermsNotAccepted,
    #[error("the manually entered WARP Secret is not UTF-8")]
    InvalidManualSecretEncoding,
    #[error("identity validation failed: {0}")]
    Identity(#[from] usque_core::IdentityError),
    #[error("Consumer WARP registration failed: {0}")]
    Registration(#[from] usque_core::RegistrationError),
    #[error("secure identity storage failed: {0}")]
    Vault(#[from] VaultError),
    #[error("configuration persistence failed: {0}")]
    Persistence(#[from] StoreError),
    #[error("configuration persistence worker failed: {0}")]
    PersistenceWorker(String),
    #[error("maintenance operation failed: {0}")]
    Maintenance(#[from] maintenance::MaintenanceError),
    #[error("the previous disconnect cleanup failed: {0}")]
    DisconnectCleanup(String),
}

impl ControlServiceError {
    fn configuration(error: impl std::fmt::Display) -> Self {
        Self::InvalidConfiguration(error.to_string())
    }

    fn as_structured_error(&self) -> StructuredError {
        let (code, retryable) = match self {
            Self::InvalidRequest(_) | Self::InvalidConfiguration(_) => ("INVALID_ARGUMENT", false),
            Self::ProfileNotFound(_) => ("PROFILE_NOT_FOUND", false),
            Self::AlreadyConnected(_) => ("ALREADY_CONNECTED", false),
            Self::LastProfile => ("LAST_PROFILE", false),
            Self::OperatingModeUnavailable(_) => ("OPERATING_MODE_UNAVAILABLE", false),
            Self::ConfirmationRequired => ("CONFIRMATION_REQUIRED", false),
            Self::MissingCredential(_) => ("MISSING_CREDENTIAL", false),
            Self::InvalidStoredIdentity => ("INVALID_STORED_IDENTITY", false),
            Self::State(_) => ("INVALID_CONNECTION_STATE", false),
            Self::Transport(TransportError::EndpointPinMismatch) => {
                ("ENDPOINT_PIN_MISMATCH", false)
            }
            Self::Transport(TransportError::Http3DatagramUnavailable) => {
                ("TRANSPORT_UNAVAILABLE", false)
            }
            Self::Transport(
                TransportError::EndpointTimeout(_)
                | TransportError::ConnectTimeout
                | TransportError::AllEndpointsFailed(_),
            ) => ("ENDPOINT_UNREACHABLE", true),
            Self::Transport(TransportError::Dns(_)) => ("DNS_UNAVAILABLE", true),
            Self::Transport(
                TransportError::SocksListener { .. } | TransportError::HttpProxyListener { .. },
            ) => ("PROXY_LISTENER_FAILED", false),
            Self::Transport(_) => ("DATA_PLANE_FAILED", true),
            Self::PlatformVpn(_) => ("PLATFORM_VPN_FAILED", true),
            Self::CaptivePortalPauseUnavailable => ("CAPTIVE_PORTAL_PAUSE_UNAVAILABLE", false),
            Self::TermsNotAccepted => ("TERMS_NOT_ACCEPTED", false),
            Self::InvalidManualSecretEncoding | Self::Identity(_) => ("INVALID_WARP_SECRET", false),
            Self::Registration(_) => ("REGISTRATION_FAILED", true),
            Self::Vault(_) => ("SECURE_STORAGE_FAILED", false),
            Self::Persistence(_) | Self::PersistenceWorker(_) => ("PERSISTENCE_FAILED", true),
            Self::Maintenance(maintenance::MaintenanceError::Update(_)) => {
                ("UPDATE_CHECK_FAILED", true)
            }
            Self::Maintenance(_) => ("DIAGNOSTICS_EXPORT_FAILED", false),
            Self::DisconnectCleanup(_) => ("DISCONNECT_CLEANUP_FAILED", true),
        };
        StructuredError {
            code: code.to_owned(),
            message: self.to_string(),
            retryable,
        }
    }
}

fn connection_error_for(error: &ControlServiceError) -> ConnectionError {
    let code = match error {
        ControlServiceError::MissingCredential(_) => ErrorCode::MissingCredential,
        ControlServiceError::Transport(TransportError::EndpointPinMismatch) => {
            ErrorCode::PinMismatch
        }
        ControlServiceError::Transport(
            TransportError::EndpointTimeout(_)
            | TransportError::ConnectTimeout
            | TransportError::AllEndpointsFailed(_),
        ) => ErrorCode::EndpointUnreachable,
        ControlServiceError::Transport(TransportError::Dns(_)) => ErrorCode::DnsUnavailable,
        ControlServiceError::Transport(TransportError::Http3DatagramUnavailable)
        | ControlServiceError::OperatingModeUnavailable(_) => ErrorCode::TransportUnavailable,
        ControlServiceError::PlatformVpn(_) => ErrorCode::PlatformSetupFailed,
        ControlServiceError::InvalidStoredIdentity
        | ControlServiceError::Transport(
            TransportError::InvalidIdentity
            | TransportError::InvalidPrivateKey
            | TransportError::InvalidEndpointPin,
        ) => ErrorCode::AuthenticationFailed,
        ControlServiceError::Vault(_) => ErrorCode::MissingCredential,
        _ => ErrorCode::Internal,
    };
    let structured = error.as_structured_error();
    ConnectionError {
        code,
        message: error.to_string(),
        retryable: structured.retryable,
    }
}

#[cfg(windows)]
fn map_windows_vpn_error(error: windows_agent::WindowsVpnError) -> ControlServiceError {
    match error {
        windows_agent::WindowsVpnError::Transport(error) => ControlServiceError::Transport(error),
        windows_agent::WindowsVpnError::Remote { code, .. }
            if code == "AGENT_ENDPOINT_UNREACHABLE" =>
        {
            ControlServiceError::PlatformVpn(
                "no physical network route to the configured WARP endpoint is available".to_owned(),
            )
        }
        windows_agent::WindowsVpnError::Remote { code, .. }
            if code == "AGENT_CONTROL_API_UNREACHABLE" =>
        {
            ControlServiceError::PlatformVpn(
                "no physical network route to the authenticated WARP control endpoint is available"
                    .to_owned(),
            )
        }
        error => ControlServiceError::PlatformVpn(error.to_string()),
    }
}

fn rate_since(current: u64, previous: u64, elapsed_seconds: f64) -> u64 {
    if elapsed_seconds <= f64::EPSILON {
        return 0;
    }
    ((current.saturating_sub(previous) as f64) / elapsed_seconds).clamp(0.0, u64::MAX as f64) as u64
}

fn nonempty(value: String) -> Option<String> {
    (!value.trim().is_empty()).then_some(value)
}

const fn desktop_registration_model() -> &'static str {
    if cfg!(target_os = "macos") {
        "Mac"
    } else {
        "PC"
    }
}

fn platform_vault() -> Arc<dyn SecretVault> {
    #[cfg(windows)]
    {
        Arc::new(usque_platform::WindowsCredentialVault)
    }
    #[cfg(target_os = "macos")]
    {
        Arc::new(usque_platform::MacOsKeychainVault)
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        Arc::new(UnavailableVault)
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
#[derive(Debug)]
struct UnavailableVault;

#[cfg(not(any(windows, target_os = "macos")))]
#[async_trait::async_trait]
impl SecretVault for UnavailableVault {
    async fn put(
        &self,
        _profile_id: Uuid,
        _record: SecretRecord,
        _value: &[u8],
    ) -> Result<(), VaultError> {
        Err(VaultError::Platform(
            "secure storage is unavailable on this platform".to_owned(),
        ))
    }

    async fn get(
        &self,
        _profile_id: Uuid,
        _record: SecretRecord,
    ) -> Result<Option<Zeroizing<Vec<u8>>>, VaultError> {
        Err(VaultError::Platform(
            "secure storage is unavailable on this platform".to_owned(),
        ))
    }

    async fn delete(&self, _profile_id: Uuid, _record: SecretRecord) -> Result<(), VaultError> {
        Err(VaultError::Platform(
            "secure storage is unavailable on this platform".to_owned(),
        ))
    }
}

fn parse_profile_id(value: &str) -> Result<Uuid, ControlServiceError> {
    Uuid::parse_str(value)
        .map_err(|_| ControlServiceError::InvalidRequest("profile ID must be a UUID".to_owned()))
}

fn profile_from_proto(source: v1::Profile) -> Result<Profile, ControlServiceError> {
    let defaults = Profile::default();
    let endpoint = source.endpoint.ok_or_else(|| {
        ControlServiceError::InvalidRequest("profile endpoint is missing".to_owned())
    })?;
    let proxy = source
        .proxy
        .unwrap_or_else(|| proxy_to_proto(&defaults.proxy));

    let profile = Profile {
        id: parse_profile_id(&source.id)?,
        name: source.name,
        mode: match source.mode {
            value if value == v1::OperatingMode::Unspecified as i32 => OperatingMode::Vpn,
            value if value == v1::OperatingMode::Vpn as i32 => OperatingMode::Vpn,
            value if value == v1::OperatingMode::Socks5 as i32 => OperatingMode::Socks5,
            value if value == v1::OperatingMode::HttpProxy as i32 => OperatingMode::HttpProxy,
            _ => {
                return Err(ControlServiceError::InvalidRequest(
                    "unknown operating mode".to_owned(),
                ));
            }
        },
        transport: match source.transport {
            value if value == v1::TransportPolicy::Unspecified as i32 => TransportPolicy::Auto,
            value if value == v1::TransportPolicy::Auto as i32 => TransportPolicy::Auto,
            value if value == v1::TransportPolicy::Http3 as i32 => TransportPolicy::Http3,
            value if value == v1::TransportPolicy::Http2 as i32 => TransportPolicy::Http2,
            _ => {
                return Err(ControlServiceError::InvalidRequest(
                    "unknown transport policy".to_owned(),
                ));
            }
        },
        endpoint: EndpointSettings {
            ipv4: endpoint
                .ipv4
                .parse::<Ipv4Addr>()
                .map_err(ControlServiceError::configuration)?,
            ipv6: endpoint
                .ipv6
                .parse::<Ipv6Addr>()
                .map_err(ControlServiceError::configuration)?,
            port: u16::try_from(endpoint.port).map_err(ControlServiceError::configuration)?,
            sni: endpoint.sni,
        },
        ip_policy: match source.ip_policy {
            value if value == v1::IpPolicy::Unspecified as i32 => IpPolicy::Auto,
            value if value == v1::IpPolicy::Auto as i32 => IpPolicy::Auto,
            value if value == v1::IpPolicy::PreferIpv4 as i32 => IpPolicy::PreferIpv4,
            value if value == v1::IpPolicy::PreferIpv6 as i32 => IpPolicy::PreferIpv6,
            value if value == v1::IpPolicy::Ipv4Only as i32 => IpPolicy::Ipv4Only,
            value if value == v1::IpPolicy::Ipv6Only as i32 => IpPolicy::Ipv6Only,
            _ => {
                return Err(ControlServiceError::InvalidRequest(
                    "unknown IP policy".to_owned(),
                ));
            }
        },
        mtu: u16::try_from(source.mtu).map_err(ControlServiceError::configuration)?,
        dns_mode: match source.dns_mode {
            value if value == v1::DnsMode::Unspecified as i32 => DnsMode::Tunnel,
            value if value == v1::DnsMode::Tunnel as i32 => DnsMode::Tunnel,
            value if value == v1::DnsMode::LocalConfigured as i32 => DnsMode::LocalConfigured,
            value if value == v1::DnsMode::System as i32 => DnsMode::System,
            _ => {
                return Err(ControlServiceError::InvalidRequest(
                    "unknown DNS mode".to_owned(),
                ));
            }
        },
        dns_servers: source
            .dns_servers
            .iter()
            .map(|value| {
                value
                    .parse::<IpAddr>()
                    .map_err(ControlServiceError::configuration)
            })
            .collect::<Result<Vec<_>, _>>()?,
        allow_lan: source.allow_lan,
        split_exclusions: source
            .split_exclusions
            .iter()
            .map(|value| {
                value
                    .parse::<IpNet>()
                    .map_err(ControlServiceError::configuration)
            })
            .collect::<Result<Vec<_>, _>>()?,
        kill_switch: source.kill_switch,
        auto_connect: source.auto_connect,
        proxy: ProxySettings {
            socks5_listeners: parse_listeners(&proxy.socks5_listeners)?,
            http_listeners: parse_listeners(&proxy.http_listeners)?,
            system_proxy: proxy.system_proxy,
            udp_idle_timeout_seconds: proxy.udp_idle_timeout_seconds,
            dns_mode: match proxy.dns_mode {
                value if value == v1::ProxyDnsMode::Unspecified as i32 => ProxyDnsMode::Remote,
                value if value == v1::ProxyDnsMode::Remote as i32 => ProxyDnsMode::Remote,
                value if value == v1::ProxyDnsMode::LocalConfigured as i32 => {
                    ProxyDnsMode::LocalConfigured
                }
                value if value == v1::ProxyDnsMode::System as i32 => ProxyDnsMode::System,
                _ => {
                    return Err(ControlServiceError::InvalidRequest(
                        "unknown proxy DNS mode".to_owned(),
                    ));
                }
            },
        },
    };
    profile
        .validate()
        .map_err(ControlServiceError::configuration)?;
    Ok(profile)
}

fn parse_listeners(values: &[String]) -> Result<Vec<SocketAddr>, ControlServiceError> {
    values
        .iter()
        .map(|value| {
            value
                .parse::<SocketAddr>()
                .map_err(ControlServiceError::configuration)
        })
        .collect()
}

fn profile_to_proto(profile: &Profile) -> v1::Profile {
    v1::Profile {
        id: profile.id.to_string(),
        name: profile.name.clone(),
        mode: match profile.mode {
            OperatingMode::Vpn => v1::OperatingMode::Vpn as i32,
            OperatingMode::Socks5 => v1::OperatingMode::Socks5 as i32,
            OperatingMode::HttpProxy => v1::OperatingMode::HttpProxy as i32,
        },
        transport: match profile.transport {
            TransportPolicy::Auto => v1::TransportPolicy::Auto as i32,
            TransportPolicy::Http3 => v1::TransportPolicy::Http3 as i32,
            TransportPolicy::Http2 => v1::TransportPolicy::Http2 as i32,
        },
        endpoint: Some(v1::EndpointSettings {
            ipv4: profile.endpoint.ipv4.to_string(),
            ipv6: profile.endpoint.ipv6.to_string(),
            port: u32::from(profile.endpoint.port),
            sni: profile.endpoint.sni.clone(),
        }),
        ip_policy: match profile.ip_policy {
            IpPolicy::Auto => v1::IpPolicy::Auto as i32,
            IpPolicy::PreferIpv4 => v1::IpPolicy::PreferIpv4 as i32,
            IpPolicy::PreferIpv6 => v1::IpPolicy::PreferIpv6 as i32,
            IpPolicy::Ipv4Only => v1::IpPolicy::Ipv4Only as i32,
            IpPolicy::Ipv6Only => v1::IpPolicy::Ipv6Only as i32,
        },
        mtu: u32::from(profile.mtu),
        dns_servers: profile
            .dns_servers
            .iter()
            .map(ToString::to_string)
            .collect(),
        allow_lan: profile.allow_lan,
        split_exclusions: profile
            .split_exclusions
            .iter()
            .map(ToString::to_string)
            .collect(),
        kill_switch: profile.kill_switch,
        auto_connect: profile.auto_connect,
        proxy: Some(proxy_to_proto(&profile.proxy)),
        dns_mode: match profile.dns_mode {
            DnsMode::Tunnel => v1::DnsMode::Tunnel as i32,
            DnsMode::LocalConfigured => v1::DnsMode::LocalConfigured as i32,
            DnsMode::System => v1::DnsMode::System as i32,
        },
    }
}

fn profile_list_to_proto(config: &AppConfig) -> v1::ProfileList {
    v1::ProfileList {
        profiles: config.profiles.iter().map(profile_to_proto).collect(),
        active_profile_id: config
            .active_profile_id
            .map(|id| id.to_string())
            .unwrap_or_default(),
        identity_statuses: Vec::new(),
    }
}

fn proxy_to_proto(proxy: &ProxySettings) -> v1::ProxySettings {
    v1::ProxySettings {
        socks5_listeners: proxy
            .socks5_listeners
            .iter()
            .map(ToString::to_string)
            .collect(),
        http_listeners: proxy
            .http_listeners
            .iter()
            .map(ToString::to_string)
            .collect(),
        system_proxy: proxy.system_proxy,
        udp_idle_timeout_seconds: proxy.udp_idle_timeout_seconds,
        dns_mode: match proxy.dns_mode {
            ProxyDnsMode::Remote => v1::ProxyDnsMode::Remote as i32,
            ProxyDnsMode::LocalConfigured => v1::ProxyDnsMode::LocalConfigured as i32,
            ProxyDnsMode::System => v1::ProxyDnsMode::System as i32,
        },
    }
}

fn current_capabilities() -> v1::Capabilities {
    v1::Capabilities {
        vpn: cfg!(windows),
        socks5: true,
        http_proxy: true,
        system_proxy: cfg!(windows),
        platform_lockdown: false,
        operating_system: std::env::consts::OS.to_owned(),
        architecture: std::env::consts::ARCH.to_owned(),
        transports: vec!["h3".to_owned(), "h2".to_owned()],
        secure_storage: cfg!(any(windows, target_os = "macos", target_os = "android")),
    }
}

fn snapshot_to_proto(snapshot: &ConnectionSnapshot) -> v1::ConnectionSnapshot {
    v1::ConnectionSnapshot {
        phase: match snapshot.phase {
            ConnectionPhase::Disconnected => v1::ConnectionPhase::Disconnected as i32,
            ConnectionPhase::Preparing => v1::ConnectionPhase::Preparing as i32,
            ConnectionPhase::ConnectingHttp3 => v1::ConnectionPhase::ConnectingHttp3 as i32,
            ConnectionPhase::ConnectingHttp2 => v1::ConnectionPhase::ConnectingHttp2 as i32,
            ConnectionPhase::Connected => v1::ConnectionPhase::Connected as i32,
            ConnectionPhase::Degraded => v1::ConnectionPhase::Degraded as i32,
            ConnectionPhase::Reconnecting => v1::ConnectionPhase::Reconnecting as i32,
            ConnectionPhase::Disconnecting => v1::ConnectionPhase::Disconnecting as i32,
            ConnectionPhase::CaptivePortalPaused => v1::ConnectionPhase::CaptivePortalPaused as i32,
            ConnectionPhase::Error => v1::ConnectionPhase::Error as i32,
        },
        transport: snapshot
            .transport
            .map(|transport| match transport {
                Transport::Http3 => "h3",
                Transport::Http2 => "h2",
            })
            .unwrap_or_default()
            .to_owned(),
        address_family: snapshot
            .address_family
            .map(|family| match family {
                AddressFamily::Ipv4 => "ipv4",
                AddressFamily::Ipv6 => "ipv6",
            })
            .unwrap_or_default()
            .to_owned(),
        ipv4_available: snapshot.ipv4_available,
        ipv6_available: snapshot.ipv6_available,
        statistics: Some(v1::Statistics {
            connected_seconds: snapshot.statistics.connected_seconds,
            bytes_sent: snapshot.statistics.bytes_sent,
            bytes_received: snapshot.statistics.bytes_received,
            upload_bytes_per_second: snapshot.statistics.current_upload_bytes_per_second,
            download_bytes_per_second: snapshot.statistics.current_download_bytes_per_second,
        }),
        exit: snapshot.exit.as_ref().map(exit_to_proto),
        error: snapshot.error.as_ref().map(|error| StructuredError {
            code: format!("{:?}", error.code).to_ascii_uppercase(),
            message: error.message.clone(),
            retryable: error.retryable,
        }),
        kill_switch_state: match snapshot.kill_switch_state {
            KillSwitchState::NotApplicable => v1::KillSwitchState::NotApplicable as i32,
            KillSwitchState::Inactive => v1::KillSwitchState::Inactive as i32,
            KillSwitchState::Active => v1::KillSwitchState::Active as i32,
            KillSwitchState::Paused => v1::KillSwitchState::Paused as i32,
            KillSwitchState::Error => v1::KillSwitchState::Error as i32,
        },
        lockdown_state: match snapshot.lockdown_state {
            LockdownState::NotSupported => v1::LockdownState::NotSupported as i32,
            LockdownState::Disabled => v1::LockdownState::Disabled as i32,
            LockdownState::Enabled => v1::LockdownState::Enabled as i32,
            LockdownState::Unknown => v1::LockdownState::Unknown as i32,
        },
        reconnect_count: snapshot.reconnect_count,
        active_listeners: snapshot.active_listeners.clone(),
        warnings: snapshot
            .warnings
            .iter()
            .map(|warning| v1::ConnectionWarning {
                code: warning.code.clone(),
                message: warning.message.clone(),
            })
            .collect(),
        captive_portal_pause_remaining_seconds: snapshot.captive_portal_pause_remaining_seconds,
    }
}

fn exit_to_proto(exit: &usque_core::ExitInfo) -> v1::ExitInfo {
    v1::ExitInfo {
        ipv4: exit.ipv4.map(|ip| ip.to_string()).unwrap_or_default(),
        ipv6: exit.ipv6.map(|ip| ip.to_string()).unwrap_or_default(),
        ipv4_location: exit.ipv4_location.as_ref().map(location_to_proto),
        ipv6_location: exit.ipv6_location.as_ref().map(location_to_proto),
        checked_at_unix_milliseconds: exit.checked_at.timestamp_millis(),
    }
}

fn location_to_proto(location: &usque_core::GeoLocation) -> v1::GeoLocation {
    v1::GeoLocation {
        ip: location.ip.to_string(),
        country_code: location.country_code.clone().unwrap_or_default(),
        country: location.country.clone().unwrap_or_default(),
        region: location.region.clone().unwrap_or_default(),
        city: location.city.clone().unwrap_or_default(),
        flag_url: location.flag_url().unwrap_or_default(),
        flag_svg: location.flag_svg.clone().unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use async_trait::async_trait;
    use base64::{Engine, engine::general_purpose::STANDARD as BASE64_STANDARD};
    use p256::{
        PublicKey,
        pkcs8::{DecodePublicKey, EncodePublicKey, LineEnding},
    };

    use super::*;

    #[derive(Default)]
    struct MemoryVault {
        records: Mutex<HashMap<(Uuid, SecretRecord), Vec<u8>>>,
    }

    #[async_trait]
    impl SecretVault for MemoryVault {
        async fn put(
            &self,
            profile_id: Uuid,
            record: SecretRecord,
            value: &[u8],
        ) -> Result<(), VaultError> {
            self.records
                .lock()
                .await
                .insert((profile_id, record), value.to_vec());
            Ok(())
        }

        async fn get(
            &self,
            profile_id: Uuid,
            record: SecretRecord,
        ) -> Result<Option<Zeroizing<Vec<u8>>>, VaultError> {
            Ok(self
                .records
                .lock()
                .await
                .get(&(profile_id, record))
                .cloned()
                .map(Zeroizing::new))
        }

        async fn delete(&self, profile_id: Uuid, record: SecretRecord) -> Result<(), VaultError> {
            self.records.lock().await.remove(&(profile_id, record));
            Ok(())
        }
    }

    #[tokio::test]
    async fn profile_crud_is_persisted_atomically() {
        let directory = tempfile::tempdir().expect("tempdir");
        let store = ConfigStore::new(directory.path().join("config.json"));
        let vault = Arc::new(MemoryVault::default());
        let service = ControlService::open_with_vault(
            store.clone(),
            Arc::clone(&vault) as Arc<dyn SecretVault>,
        )
        .expect("service");
        let original = service.config_snapshot().await.profiles[0].id;
        vault
            .put(original, SecretRecord::AccessToken, b"remove-me")
            .await
            .expect("seed identity");

        let added = Profile {
            id: Uuid::parse_str("887f91ff-3977-4ac8-8947-e02c1f7c8181").expect("uuid"),
            name: "Hotel Wi-Fi".to_owned(),
            endpoint: EndpointSettings {
                sni: "example.com".to_owned(),
                ..EndpointSettings::default()
            },
            ..Profile::default()
        };
        let added_id = added.id;
        let response = service
            .handle(request(
                "upsert",
                control_request::Payload::UpsertProfile(Box::new(v1::UpsertProfileRequest {
                    profile: Some(profile_to_proto(&added)),
                })),
            ))
            .await;
        assert!(response.error.is_none(), "{:?}", response.error);

        let response = service
            .handle(request(
                "activate",
                control_request::Payload::SetActiveProfile(v1::SetActiveProfileRequest {
                    profile_id: added.id.to_string(),
                }),
            ))
            .await;
        assert!(response.error.is_none(), "{:?}", response.error);

        let response = service
            .handle(request(
                "delete",
                control_request::Payload::DeleteProfile(v1::DeleteProfileRequest {
                    profile_id: original.to_string(),
                }),
            ))
            .await;
        assert!(response.error.is_none(), "{:?}", response.error);

        let reopened = ControlService::open(store).expect("reopen");
        let persisted = reopened.config_snapshot().await;
        assert_eq!(persisted.profiles, vec![added]);
        assert_eq!(persisted.active_profile_id, Some(added_id));
        assert!(persisted.pending_identity_deletions.is_empty());
        assert!(
            vault
                .get(original, SecretRecord::AccessToken)
                .await
                .expect("read identity")
                .is_none()
        );
    }

    #[tokio::test]
    async fn flutter_profiles_are_imported_exactly_once_and_returned_authoritatively() {
        let directory = tempfile::tempdir().expect("tempdir");
        let store = ConfigStore::new(directory.path().join("config.json"));
        let service = ControlService::open(store.clone()).expect("service");
        let imported = Profile {
            id: Uuid::parse_str("7b60ea7c-03a5-455d-9914-2cdf0e268ac2").expect("uuid"),
            name: "Imported".to_owned(),
            mode: OperatingMode::Socks5,
            ..Profile::default()
        };

        let first = service
            .handle(request(
                "import-first",
                control_request::Payload::ImportLegacyProfiles(v1::ImportLegacyProfilesRequest {
                    profiles: vec![profile_to_proto(&imported)],
                    active_profile_id: imported.id.to_string(),
                }),
            ))
            .await;
        assert!(first.error.is_none(), "{:?}", first.error);
        let Some(control_response::Payload::ProfileList(first_catalog)) = first.payload else {
            panic!("missing profile catalog");
        };
        assert_eq!(first_catalog.active_profile_id, imported.id.to_string());
        assert!(
            first_catalog
                .profiles
                .iter()
                .any(|profile| profile.name == "Imported")
        );

        let mut replacement = imported.clone();
        replacement.name = "Must not replace".to_owned();
        let second = service
            .handle(request(
                "import-second",
                control_request::Payload::ImportLegacyProfiles(v1::ImportLegacyProfilesRequest {
                    profiles: vec![profile_to_proto(&replacement)],
                    active_profile_id: replacement.id.to_string(),
                }),
            ))
            .await;
        assert!(second.error.is_none(), "{:?}", second.error);
        let Some(control_response::Payload::ProfileList(second_catalog)) = second.payload else {
            panic!("missing profile catalog");
        };
        assert!(
            second_catalog
                .profiles
                .iter()
                .any(|profile| profile.name == "Imported")
        );
        assert!(
            !second_catalog
                .profiles
                .iter()
                .any(|profile| profile.name == "Must not replace")
        );

        let reopened = ControlService::open(store).expect("reopen");
        assert!(
            reopened
                .config_snapshot()
                .await
                .preferences
                .profiles_migrated_from_flutter
        );
    }

    #[tokio::test]
    async fn capabilities_report_only_linked_release_slices() {
        let directory = tempfile::tempdir().unwrap();
        let service = ControlService::open_with_vault(
            ConfigStore::new(directory.path().join("config.json")),
            Arc::new(MemoryVault::default()),
        )
        .unwrap();
        let response = service
            .handle(request(
                "capabilities",
                control_request::Payload::GetCapabilities(v1::GetCapabilitiesRequest {}),
            ))
            .await;
        let Some(control_response::Payload::Capabilities(capabilities)) = response.payload else {
            panic!("missing capabilities response");
        };
        assert_eq!(capabilities.vpn, cfg!(windows));
        assert!(capabilities.socks5);
        assert!(capabilities.http_proxy);
        assert_eq!(capabilities.system_proxy, cfg!(windows));
        assert!(!capabilities.platform_lockdown);
        assert_eq!(capabilities.transports, ["h3", "h2"]);
        assert!(!capabilities.architecture.is_empty());
    }

    #[tokio::test]
    async fn retry_and_clear_all_data_are_real_control_operations() {
        let directory = tempfile::tempdir().unwrap();
        let config_path = directory.path().join("config.json");
        let vault = Arc::new(MemoryVault::default());
        let service =
            ControlService::open_with_vault(ConfigStore::new(&config_path), vault.clone()).unwrap();

        let retry = service
            .handle(request(
                "retry",
                control_request::Payload::Retry(v1::RetryRequest {}),
            ))
            .await;
        assert_eq!(
            retry.error.as_ref().map(|error| error.code.as_str()),
            Some(if cfg!(windows) {
                "MISSING_CREDENTIAL"
            } else {
                "OPERATING_MODE_UNAVAILABLE"
            })
        );

        let profile_id = service.config_snapshot().await.active_profile_id.unwrap();
        vault
            .put(profile_id, SecretRecord::AccessToken, b"sensitive")
            .await
            .unwrap();
        let rejected = service
            .handle(request(
                "clear-unconfirmed",
                control_request::Payload::ClearAllData(v1::ClearAllDataRequest {
                    confirmed: false,
                }),
            ))
            .await;
        assert_eq!(
            rejected.error.as_ref().map(|error| error.code.as_str()),
            Some("CONFIRMATION_REQUIRED")
        );
        assert!(
            vault
                .get(profile_id, SecretRecord::AccessToken)
                .await
                .unwrap()
                .is_some()
        );

        let cleared = service
            .handle(request(
                "clear-confirmed",
                control_request::Payload::ClearAllData(v1::ClearAllDataRequest { confirmed: true }),
            ))
            .await;
        assert!(cleared.error.is_none(), "{:?}", cleared.error);
        assert!(
            vault
                .get(profile_id, SecretRecord::AccessToken)
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(service.config_snapshot().await, AppConfig::default());
    }

    #[tokio::test]
    async fn reset_restores_network_defaults_without_removing_profile_identity() {
        let directory = tempfile::tempdir().expect("tempdir");
        let service = ControlService::open(ConfigStore::new(directory.path().join("config.json")))
            .expect("service");
        let mut profile = service.config_snapshot().await.profiles[0].clone();
        let id = profile.id;
        profile.name = "Keep this name".to_owned();
        profile.endpoint.sni = "example.com".to_owned();
        profile.mtu = 1400;
        service.upsert_profile(profile).await.expect("upsert");

        let response = service
            .handle(request(
                "reset",
                control_request::Payload::ResetProfile(v1::ResetProfileRequest {
                    profile_id: id.to_string(),
                }),
            ))
            .await;
        assert!(response.error.is_none(), "{:?}", response.error);

        let reset = service.config_snapshot().await.profiles[0].clone();
        assert_eq!(reset.id, id);
        assert_eq!(reset.name, "Keep this name");
        assert_eq!(reset.endpoint, EndpointSettings::default());
        assert_eq!(reset.mtu, 1280);
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn connect_fails_closed_for_unavailable_vpn_mode() {
        let directory = tempfile::tempdir().expect("tempdir");
        let service = ControlService::open(ConfigStore::new(directory.path().join("config.json")))
            .expect("service");
        let profile_id = service.config_snapshot().await.profiles[0].id;

        let response = service
            .handle(request(
                "connect",
                control_request::Payload::Connect(v1::ConnectRequest {
                    profile_id: profile_id.to_string(),
                }),
            ))
            .await;

        assert_eq!(
            response.error.as_ref().map(|error| error.code.as_str()),
            Some("OPERATING_MODE_UNAVAILABLE")
        );
        assert_eq!(
            service.state.lock().await.snapshot().phase,
            ConnectionPhase::Disconnected
        );
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn windows_vpn_fails_before_agent_mutation_when_identity_is_missing() {
        let directory = tempfile::tempdir().expect("tempdir");
        let service = ControlService::open_with_vault(
            ConfigStore::new(directory.path().join("config.json")),
            Arc::new(MemoryVault::default()),
        )
        .expect("service");
        let profile_id = service.config_snapshot().await.profiles[0].id;

        let response = service
            .handle(request(
                "connect",
                control_request::Payload::Connect(v1::ConnectRequest {
                    profile_id: profile_id.to_string(),
                }),
            ))
            .await;

        assert_eq!(
            response.error.as_ref().map(|error| error.code.as_str()),
            Some("MISSING_CREDENTIAL")
        );
        assert_eq!(
            service.state.lock().await.snapshot().phase,
            ConnectionPhase::Error
        );
    }

    #[tokio::test]
    async fn last_profile_and_malformed_profiles_are_rejected() {
        let directory = tempfile::tempdir().expect("tempdir");
        let service = ControlService::open(ConfigStore::new(directory.path().join("config.json")))
            .expect("service");
        let profile = service.config_snapshot().await.profiles[0].clone();

        let delete = service
            .handle(request(
                "delete",
                control_request::Payload::DeleteProfile(v1::DeleteProfileRequest {
                    profile_id: profile.id.to_string(),
                }),
            ))
            .await;
        assert_eq!(
            delete.error.as_ref().map(|error| error.code.as_str()),
            Some("LAST_PROFILE")
        );

        let mut malformed = profile_to_proto(&profile);
        malformed.mtu = 1;
        let upsert = service
            .handle(request(
                "upsert",
                control_request::Payload::UpsertProfile(Box::new(v1::UpsertProfileRequest {
                    profile: Some(malformed),
                })),
            ))
            .await;
        assert_eq!(
            upsert.error.as_ref().map(|error| error.code.as_str()),
            Some("INVALID_ARGUMENT")
        );
    }

    #[tokio::test]
    async fn identity_provisioning_requires_terms_and_valid_utf8() {
        let directory = tempfile::tempdir().expect("tempdir");
        let service = ControlService::open(ConfigStore::new(directory.path().join("config.json")))
            .expect("service");
        let profile_id = service.config_snapshot().await.profiles[0].id.to_string();

        let terms = service
            .handle(request(
                "terms",
                control_request::Payload::ProvisionIdentity(v1::ProvisionIdentityRequest {
                    profile_id: profile_id.clone(),
                    warp_secret: b"not-used".to_vec(),
                    terms_accepted: false,
                    locale: "en_US".to_owned(),
                    device_name: String::new(),
                }),
            ))
            .await;
        assert_eq!(
            terms.error.as_ref().map(|error| error.code.as_str()),
            Some("TERMS_NOT_ACCEPTED")
        );

        let encoding = service
            .handle(request(
                "encoding",
                control_request::Payload::ProvisionIdentity(v1::ProvisionIdentityRequest {
                    profile_id,
                    warp_secret: vec![0xff],
                    terms_accepted: true,
                    locale: "en_US".to_owned(),
                    device_name: String::new(),
                }),
            ))
            .await;
        assert_eq!(
            encoding.error.as_ref().map(|error| error.code.as_str()),
            Some("INVALID_WARP_SECRET")
        );
    }

    #[tokio::test]
    async fn create_profile_with_identity_is_committed_as_one_transaction() {
        let directory = tempfile::tempdir().expect("tempdir");
        let config_path = directory.path().join("config.json");
        let vault = Arc::new(MemoryVault::default());
        let service = ControlService::open_with_vault(
            ConfigStore::new(&config_path),
            Arc::clone(&vault) as Arc<dyn SecretVault>,
        )
        .expect("service");
        let profile = Profile {
            id: Uuid::parse_str("96c88d75-f9ce-412b-9835-5cd460b817c4").expect("uuid"),
            name: "Transactional profile".to_owned(),
            ..Profile::default()
        };

        let rejected = service
            .handle(request(
                "create-invalid",
                control_request::Payload::CreateProfileWithIdentity(Box::new(
                    v1::CreateProfileWithIdentityRequest {
                        profile: Some(profile_to_proto(&profile)),
                        identity: Some(v1::IdentityProvisioning {
                            method: v1::IdentityProvisioningMethod::ImportSecret as i32,
                            warp_secret: b"not-a-valid-secret".to_vec(),
                            terms_accepted: true,
                            locale: "en_US".to_owned(),
                            device_name: String::new(),
                        }),
                    },
                )),
            ))
            .await;
        assert!(rejected.error.is_some());
        let rejected_config = service.config_snapshot().await;
        assert!(
            !rejected_config
                .profiles
                .iter()
                .any(|item| item.id == profile.id)
        );
        assert!(rejected_config.pending_identity_creations.is_empty());
        assert!(
            vault
                .get(profile.id, SecretRecord::AccessToken)
                .await
                .expect("read rejected identity")
                .is_none()
        );

        let identity_key = usque_core::MasqueKeyPair::generate();
        let endpoint_key = usque_core::MasqueKeyPair::generate();
        let endpoint_public =
            PublicKey::from_public_key_der(&endpoint_key.public_spki_der().expect("endpoint DER"))
                .expect("endpoint key");
        let secret = serde_json::json!({
            "private_key": BASE64_STANDARD.encode(
                identity_key.private_sec1_der().expect("private DER").as_slice()
            ),
            "endpoint_pub_key": endpoint_public
                .to_public_key_pem(LineEnding::LF)
                .expect("endpoint PEM"),
            "id": "transaction-device",
            "access_token": "transaction-token",
            "license": "transaction-license",
            "ipv4": "172.16.0.2",
            "ipv6": "2606:4700:110:8f13::2"
        })
        .to_string();
        let committed = service
            .handle(request(
                "create-valid",
                control_request::Payload::CreateProfileWithIdentity(Box::new(
                    v1::CreateProfileWithIdentityRequest {
                        profile: Some(profile_to_proto(&profile)),
                        identity: Some(v1::IdentityProvisioning {
                            method: v1::IdentityProvisioningMethod::ImportSecret as i32,
                            warp_secret: secret.into_bytes(),
                            terms_accepted: true,
                            locale: "en_US".to_owned(),
                            device_name: String::new(),
                        }),
                    },
                )),
            ))
            .await;
        assert!(committed.error.is_none(), "{:?}", committed.error);
        let committed_config = service.config_snapshot().await;
        assert!(
            committed_config
                .profiles
                .iter()
                .any(|item| item.id == profile.id)
        );
        assert!(committed_config.pending_identity_creations.is_empty());
        assert!(
            vault
                .get(profile.id, SecretRecord::AccessToken)
                .await
                .expect("read committed identity")
                .is_some()
        );
        let plaintext_config = std::fs::read_to_string(config_path).expect("config");
        assert!(!plaintext_config.contains("transaction-token"));
        assert!(!plaintext_config.contains("transaction-license"));
    }

    /// Opt-in real-network smoke test. It deliberately exercises only the
    /// loopback SOCKS5 mode and never prepares a TUN or changes host routing.
    #[cfg(windows)]
    #[tokio::test]
    #[ignore = "requires enrolled Windows credentials and explicit USQUE_LIVE_CONFIG"]
    async fn live_socks5_connects_and_relays_http_without_tun() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let source_config_path =
            std::env::var_os("USQUE_LIVE_CONFIG").expect("USQUE_LIVE_CONFIG is required");
        let directory = tempfile::tempdir().expect("live config staging directory");
        let config_path = directory.path().join("config.json");
        std::fs::copy(source_config_path, &config_path)
            .expect("stage live config without mutation");
        let store = ConfigStore::new(&config_path);
        let mut config = store.load().expect("load staged live config");
        if let Ok(transport) = std::env::var("USQUE_LIVE_TRANSPORT") {
            let active_id = config.active_profile_id.expect("active profile ID");
            let active = config
                .profiles
                .iter_mut()
                .find(|profile| profile.id == active_id)
                .expect("active profile");
            active.transport = match transport.as_str() {
                "auto" => TransportPolicy::Auto,
                "h3" => TransportPolicy::Http3,
                "h2" => TransportPolicy::Http2,
                other => panic!("unsupported USQUE_LIVE_TRANSPORT value: {other}"),
            };
            store.save(&config).expect("save staged transport override");
        }
        let service = ControlService::open(store).expect("open live service");
        let profile = service
            .config_snapshot()
            .await
            .active_profile()
            .cloned()
            .expect("active profile");
        assert_eq!(profile.mode, OperatingMode::Socks5);

        let connected = service
            .handle(request(
                "live-connect",
                control_request::Payload::Connect(v1::ConnectRequest {
                    profile_id: profile.id.to_string(),
                }),
            ))
            .await;
        assert!(connected.error.is_none(), "{:?}", connected.error);
        let exit = match connected.payload.as_ref() {
            Some(control_response::Payload::Status(status)) => {
                eprintln!(
                    "live SOCKS5 path: transport={}, family={}",
                    status.transport, status.address_family
                );
                status.exit.as_ref()
            }
            other => panic!("unexpected connect response: {other:?}"),
        };
        if let Some(exit) = exit {
            if exit.ipv4.is_empty() && exit.ipv6.is_empty() {
                eprintln!("IP.SB returned no exit address; tunnel remains healthy by contract");
            }
            if exit.ipv4_location.is_none() && exit.ipv6_location.is_none() {
                eprintln!("IP.SB geo lookup was unavailable; tunnel remains healthy by contract");
            }
        } else {
            eprintln!("IP.SB exit lookup was unavailable; tunnel remains healthy by contract");
        }

        let mut proxy = tokio::net::TcpStream::connect("127.0.0.1:1080")
            .await
            .expect("connect loopback SOCKS5");
        proxy.write_all(&[5, 1, 0]).await.expect("greeting");
        let mut greeting = [0u8; 2];
        proxy.read_exact(&mut greeting).await.expect("auth reply");
        assert_eq!(greeting, [5, 0]);

        let host = b"example.com";
        let mut connect = vec![5, 1, 0, 3, host.len() as u8];
        connect.extend_from_slice(host);
        connect.extend_from_slice(&80u16.to_be_bytes());
        proxy.write_all(&connect).await.expect("CONNECT request");
        let mut reply_header = [0u8; 4];
        proxy
            .read_exact(&mut reply_header)
            .await
            .expect("CONNECT reply");
        assert_eq!(reply_header[1], 0, "SOCKS reply: {reply_header:?}");
        let address_length = match reply_header[3] {
            1 => 4,
            4 => 16,
            3 => usize::from(proxy.read_u8().await.expect("domain length")),
            other => panic!("unexpected SOCKS address type {other}"),
        };
        let mut bound_address_and_port = vec![0u8; address_length + 2];
        proxy
            .read_exact(&mut bound_address_and_port)
            .await
            .expect("bound address");
        proxy
            .write_all(b"GET / HTTP/1.1\r\nHost: example.com\r\nConnection: close\r\n\r\n")
            .await
            .expect("HTTP request");
        let mut response = [0u8; 16];
        let received = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            proxy.read(&mut response),
        )
        .await
        .expect("HTTP response timeout")
        .expect("HTTP response");
        assert!(received > 0);
        assert!(response[..received].starts_with(b"HTTP/"));
        drop(proxy);

        let mut udp_control = tokio::net::TcpStream::connect("127.0.0.1:1080")
            .await
            .expect("connect SOCKS5 UDP control");
        udp_control
            .write_all(&[5, 1, 0])
            .await
            .expect("UDP greeting");
        udp_control
            .read_exact(&mut greeting)
            .await
            .expect("UDP auth reply");
        assert_eq!(greeting, [5, 0]);
        udp_control
            .write_all(&[5, 3, 0, 1, 0, 0, 0, 0, 0, 0])
            .await
            .expect("UDP ASSOCIATE request");
        udp_control
            .read_exact(&mut reply_header)
            .await
            .expect("UDP ASSOCIATE reply");
        assert_eq!(reply_header[1], 0, "SOCKS UDP reply: {reply_header:?}");
        let relay_ip = match reply_header[3] {
            1 => {
                let mut octets = [0u8; 4];
                udp_control
                    .read_exact(&mut octets)
                    .await
                    .expect("UDP relay IPv4");
                IpAddr::V4(Ipv4Addr::from(octets))
            }
            4 => {
                let mut octets = [0u8; 16];
                udp_control
                    .read_exact(&mut octets)
                    .await
                    .expect("UDP relay IPv6");
                IpAddr::V6(Ipv6Addr::from(octets))
            }
            other => panic!("unexpected UDP relay address type {other}"),
        };
        let relay_port = udp_control.read_u16().await.expect("UDP relay port");
        let relay = SocketAddr::new(relay_ip, relay_port);
        let udp = tokio::net::UdpSocket::bind("127.0.0.1:0")
            .await
            .expect("bind SOCKS5 UDP client");
        let mut dns_query = vec![
            0x5a, 0x17, 0x01, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];
        dns_query.extend_from_slice(&[
            7, b'e', b'x', b'a', b'm', b'p', b'l', b'e', 3, b'c', b'o', b'm', 0, 0, 1, 0, 1,
        ]);
        let mut udp_request = vec![0, 0, 0, 1, 1, 1, 1, 1, 0, 53];
        udp_request.extend_from_slice(&dns_query);
        udp.send_to(&udp_request, relay)
            .await
            .expect("send DNS through SOCKS5 UDP");
        let mut udp_response = vec![0u8; 65_535];
        let (udp_length, _) = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            udp.recv_from(&mut udp_response),
        )
        .await
        .expect("SOCKS5 UDP response timeout")
        .expect("SOCKS5 UDP response");
        udp_response.truncate(udp_length);
        assert_eq!(&udp_response[..3], &[0, 0, 0]);
        let dns_offset = match udp_response[3] {
            1 => 10,
            4 => 22,
            3 => 7 + usize::from(udp_response[4]),
            other => panic!("unexpected SOCKS5 UDP response address type {other}"),
        };
        assert!(udp_response.len() >= dns_offset + 12);
        assert_eq!(&udp_response[dns_offset..dns_offset + 2], &[0x5a, 0x17]);
        assert_ne!(udp_response[dns_offset + 2] & 0x80, 0);
        drop(udp_control);

        let disconnected = service
            .handle(request(
                "live-disconnect",
                control_request::Payload::Disconnect(v1::DisconnectRequest {}),
            ))
            .await;
        assert!(disconnected.error.is_none(), "{:?}", disconnected.error);
    }

    /// Opt-in real-network smoke test for both HTTP proxy request forms. Like
    /// the SOCKS5 test, this binds loopback only and cannot create a TUN.
    #[cfg(windows)]
    #[tokio::test]
    #[ignore = "requires enrolled Windows credentials and explicit USQUE_LIVE_CONFIG"]
    async fn live_http_proxy_connect_and_forward_without_tun() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let source_config_path =
            std::env::var_os("USQUE_LIVE_CONFIG").expect("USQUE_LIVE_CONFIG is required");
        let directory = tempfile::tempdir().expect("live config staging directory");
        let config_path = directory.path().join("config.json");
        std::fs::copy(source_config_path, &config_path)
            .expect("stage live config without mutation");
        let reservation =
            std::net::TcpListener::bind("127.0.0.1:0").expect("reserve HTTP proxy port");
        let listener = reservation.local_addr().expect("reserved listener address");
        drop(reservation);

        let store = ConfigStore::new(&config_path);
        let mut config = store.load().expect("load staged live config");
        let active_id = config.active_profile_id.expect("active profile ID");
        let active = config
            .profiles
            .iter_mut()
            .find(|profile| profile.id == active_id)
            .expect("active profile");
        active.mode = OperatingMode::HttpProxy;
        active.proxy.http_listeners = vec![listener];
        if let Ok(transport) = std::env::var("USQUE_LIVE_TRANSPORT") {
            active.transport = match transport.as_str() {
                "auto" => TransportPolicy::Auto,
                "h3" => TransportPolicy::Http3,
                "h2" => TransportPolicy::Http2,
                other => panic!("unsupported USQUE_LIVE_TRANSPORT value: {other}"),
            };
        }
        store.save(&config).expect("save staged HTTP profile");

        let service = ControlService::open(store).expect("open live service");
        let profile = service
            .config_snapshot()
            .await
            .active_profile()
            .cloned()
            .expect("active profile");
        let connected = service
            .handle(request(
                "live-http-connect",
                control_request::Payload::Connect(v1::ConnectRequest {
                    profile_id: profile.id.to_string(),
                }),
            ))
            .await;
        assert!(connected.error.is_none(), "{:?}", connected.error);
        match connected.payload.as_ref() {
            Some(control_response::Payload::Status(status)) => {
                eprintln!(
                    "live HTTP proxy path: transport={}, family={}",
                    status.transport, status.address_family
                );
            }
            other => panic!("unexpected connect response: {other:?}"),
        }

        let mut forward = tokio::net::TcpStream::connect(listener)
            .await
            .expect("connect HTTP forward proxy");
        forward
            .write_all(
                b"GET http://example.com/ HTTP/1.1\r\nHost: wrong.invalid\r\nProxy-Connection: keep-alive\r\n\r\n",
            )
            .await
            .expect("ordinary proxy request");
        let mut response = [0u8; 16];
        let received = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            forward.read(&mut response),
        )
        .await
        .expect("ordinary proxy response timeout")
        .expect("ordinary proxy response");
        assert!(received > 0);
        assert!(response[..received].starts_with(b"HTTP/"));

        let mut connect = tokio::net::TcpStream::connect(listener)
            .await
            .expect("connect HTTP CONNECT proxy");
        connect
            .write_all(b"CONNECT example.com:80 HTTP/1.1\r\nHost: example.com:80\r\n\r\n")
            .await
            .expect("CONNECT request");
        let mut connect_head = Vec::new();
        while !connect_head.ends_with(b"\r\n\r\n") {
            assert!(connect_head.len() < 4096);
            connect_head.push(connect.read_u8().await.expect("CONNECT response"));
        }
        assert!(
            connect_head.starts_with(b"HTTP/1.1 200 "),
            "CONNECT response: {}",
            String::from_utf8_lossy(&connect_head)
        );
        connect
            .write_all(b"GET / HTTP/1.1\r\nHost: example.com\r\nConnection: close\r\n\r\n")
            .await
            .expect("request through CONNECT");
        let received = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            connect.read(&mut response),
        )
        .await
        .expect("CONNECT tunnel response timeout")
        .expect("CONNECT tunnel response");
        assert!(received > 0);
        assert!(response[..received].starts_with(b"HTTP/"));

        let disconnected = service
            .handle(request(
                "live-http-disconnect",
                control_request::Payload::Disconnect(v1::DisconnectRequest {}),
            ))
            .await;
        assert!(disconnected.error.is_none(), "{:?}", disconnected.error);
    }

    #[tokio::test]
    async fn valid_identity_is_split_into_vault_records_not_plaintext_config() {
        let directory = tempfile::tempdir().expect("tempdir");
        let config_path = directory.path().join("config.json");
        let vault = Arc::new(MemoryVault::default());
        let service = ControlService::open_with_vault(
            ConfigStore::new(&config_path),
            Arc::clone(&vault) as Arc<dyn SecretVault>,
        )
        .expect("service");
        let profile_id = service.config_snapshot().await.profiles[0].id;

        let identity_key = usque_core::MasqueKeyPair::generate();
        let endpoint_key = usque_core::MasqueKeyPair::generate();
        let endpoint_public =
            PublicKey::from_public_key_der(&endpoint_key.public_spki_der().expect("endpoint DER"))
                .expect("endpoint key");
        let secret = serde_json::json!({
            "private_key": BASE64_STANDARD.encode(
                identity_key.private_sec1_der().expect("private DER").as_slice()
            ),
            "endpoint_pub_key": endpoint_public
                .to_public_key_pem(LineEnding::LF)
                .expect("endpoint PEM"),
            "id": "device-test",
            "access_token": "access-token-test",
            "license": "license-test",
            "ipv4": "172.16.0.2",
            "ipv6": "2606:4700:110:8f13::2"
        })
        .to_string();

        let response = service
            .handle(request(
                "provision",
                control_request::Payload::ProvisionIdentity(v1::ProvisionIdentityRequest {
                    profile_id: profile_id.to_string(),
                    warp_secret: secret.as_bytes().to_vec(),
                    terms_accepted: true,
                    locale: "en_US".to_owned(),
                    device_name: String::new(),
                }),
            ))
            .await;
        assert!(response.error.is_none(), "{:?}", response.error);

        let records = vault.records.lock().await;
        for record in SecretRecord::ALL {
            assert!(
                records.contains_key(&(profile_id, record)),
                "missing {}",
                record.key()
            );
        }
        drop(records);
        let config = std::fs::read_to_string(config_path).expect("config");
        assert!(!config.contains("access-token-test"));
        assert!(!config.contains("license-test"));
        assert!(!config.contains("private_key"));
    }

    fn request(id: &str, payload: control_request::Payload) -> ControlRequest {
        ControlRequest {
            request_id: id.to_owned(),
            payload: Some(payload),
        }
    }
}
