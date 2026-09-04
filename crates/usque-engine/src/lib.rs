//! Versioned control plane for the unprivileged desktop engine.
//!
//! OS-specific Named Pipe and Unix Socket listeners are intentionally kept
//! outside this module. This service accepts already-authenticated protobuf
//! requests, serializes configuration mutations, and persists only non-secret
//! profile data through [`ConfigStore`].

use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Instant,
};

use ipnet::IpNet;
use thiserror::Error;
use tokio::sync::{Mutex, RwLock, watch};
use tokio_util::task::AbortOnDropHandle;
use usque_core::{
    AddressFamily, AppConfig, ConfigError, ConnectionError, ConnectionPhase, ConnectionSnapshot,
    ConnectionWarning, ConsumerEntitlement, ConsumerRegistrationClient,
    DirectDnsMode as ConfigDirectDnsMode, DirectDnsSettings, DnsMode, EndpointPin,
    EndpointSettings, ErrorCode, ExitInfo, FrontendKind, FrontendPhase, FrontendSettings,
    FrontendStatus, GeoProgress, IdentityMetadata, IdentityProvider, IpPolicy, IpSbProbe,
    KillSwitchState, LockdownState, ManagedEndpointIps, MasqueKeyPair, OperatingMode,
    PendingIdentityReplacement, Profile, ProxyAuthCredentials, ProxyDnsMode, ProxySettings,
    RegistrationError, RegistrationOptions, SHARED_NETWORK_SECRET_ID, SharedNetworkSettings,
    StateMachine, Statistics, Transport, TransportFailure, TransportPolicy, WarpIdentity,
    download_geo_rules, list_geo_rules, normalize_zero_trust_team,
    storage::{ConfigStore, StoreError},
    update_all_geo_rules, validate_proxy_password, validate_proxy_username,
};
use usque_geo::{CountryCode, GeoDownloader, ReqwestFetch, UpdateStatus};
use usque_ipc::v1::{
    self, ControlRequest, ControlResponse, StructuredError, control_request, control_response,
};
use usque_platform::{SecretRecord, SecretVault, VaultError};
use usque_transport::{
    EndpointPinRefresher, GeoDirectPolicy, MasqueTlsIdentity, NoopSocketProtector,
    ProxyPerformanceSnapshot, ProxyRuntime, RuntimeHealth, RuntimePath, TransportError,
    refresh_endpoint_pin_over_protected_socket,
};
use uuid::Uuid;
use zeroize::Zeroizing;

pub mod diagnostics;
#[cfg(any(windows, test))]
mod event_stream;
#[cfg(any(windows, target_os = "macos", test))]
mod ipc_stream;
pub mod logging;
mod maintenance;
mod network_quality;
mod sensitive_output;

mod active_runtime;
mod reconfigure;

use active_runtime::{ActiveDataPlane, ActiveProxyRuntime, ActiveRuntime};

#[cfg(windows)]
mod windows_agent;

#[cfg(target_os = "macos")]
pub mod macos_ipc;

#[cfg(windows)]
pub mod windows_ipc;

#[cfg(windows)]
pub mod windows_purge;

pub struct ControlService {
    store: ConfigStore,
    pub(crate) config: RwLock<AppConfig>,
    pub(crate) state: Arc<Mutex<StateMachine>>,
    pub(crate) mutation_lock: Arc<Mutex<()>>,
    vault: Arc<dyn SecretVault>,
    pub(crate) data_plane: Arc<Mutex<Option<ActiveDataPlane>>>,
    disconnect_cleanup: Mutex<Option<tokio::task::JoinHandle<Result<(), ControlServiceError>>>>,
    exit_probe_task: Mutex<Option<tokio::task::JoinHandle<()>>>,
    maintenance: maintenance::Maintenance,
    diagnostics: diagnostics::DiagnosticsManager,
    cache_dir: PathBuf,
    geo_progress_tx: tokio::sync::broadcast::Sender<v1::GeoRulesProgress>,
    network_quality_tx: watch::Sender<usque_transport::NetworkQualitySnapshot>,
    network_quality_relay: Mutex<Option<AbortOnDropHandle<()>>>,
    session_generation: AtomicU64,
    #[cfg(any(windows, test))]
    event_sequence: AtomicU64,
    #[cfg(test)]
    remote_license_unbinds: Mutex<Vec<Uuid>>,
}

fn consumer_license_presentation(entitlement: ConsumerEntitlement) -> (v1::LicenseState, String) {
    if entitlement.is_warp_plus() {
        (v1::LicenseState::WarpPlus, "WARP+".to_owned())
    } else {
        (v1::LicenseState::Free, "Free".to_owned())
    }
}

fn should_unbind_remote_license(identity: &WarpIdentity) -> bool {
    matches!(identity.provider(), IdentityProvider::Consumer)
        && identity.license().is_some()
        && identity.entitlement() != Some(ConsumerEntitlement::Free)
}

enum ProvisionedIdentity {
    Consumer(WarpIdentity),
    ZeroTrust {
        identity: WarpIdentity,
        endpoint_ips: ManagedEndpointIps,
    },
}

impl ProvisionedIdentity {
    fn consumer(identity: WarpIdentity) -> Self {
        Self::Consumer(identity)
    }

    fn zero_trust(identity: WarpIdentity, endpoint: &EndpointSettings) -> Self {
        Self::ZeroTrust {
            identity,
            endpoint_ips: ManagedEndpointIps::from_endpoint(endpoint),
        }
    }

    fn is_zero_trust(&self) -> bool {
        matches!(self, Self::ZeroTrust { .. })
    }

    fn identity(&self) -> &WarpIdentity {
        match self {
            Self::Consumer(identity) | Self::ZeroTrust { identity, .. } => identity,
        }
    }

    fn managed_endpoint_ips(&self) -> Option<&ManagedEndpointIps> {
        match self {
            Self::Consumer(_) => None,
            Self::ZeroTrust { endpoint_ips, .. } => Some(endpoint_ips),
        }
    }

    fn into_parts(self) -> (WarpIdentity, Option<ManagedEndpointIps>) {
        match self {
            Self::Consumer(identity) => (identity, None),
            Self::ZeroTrust {
                identity,
                endpoint_ips,
            } => (identity, Some(endpoint_ips)),
        }
    }
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

fn runtime_path_changed(snapshot: &ConnectionSnapshot, path: RuntimePath) -> bool {
    snapshot.transport != Some(path.transport)
        || snapshot.address_family != Some(path.endpoint_family)
        || snapshot.ipv4_available != path.ipv4_available
        || snapshot.ipv6_available != path.ipv6_available
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
        let cache_dir = store
            .path()
            .parent()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        let (geo_progress_tx, _) = tokio::sync::broadcast::channel(16);
        let (network_quality_tx, _) = watch::channel(network_quality::disconnected_snapshot());
        Ok(Self {
            maintenance: maintenance::Maintenance::new(store.path()),
            diagnostics: diagnostics::DiagnosticsManager::new(),
            store,
            config: RwLock::new(config),
            state: Arc::new(Mutex::new(StateMachine::default())),
            mutation_lock: Arc::new(Mutex::new(())),
            vault,
            data_plane: Arc::new(Mutex::new(None)),
            disconnect_cleanup: Mutex::new(None),
            exit_probe_task: Mutex::new(None),
            cache_dir,
            geo_progress_tx,
            network_quality_tx,
            network_quality_relay: Mutex::new(None),
            session_generation: AtomicU64::new(0),
            #[cfg(any(windows, test))]
            event_sequence: AtomicU64::new(0),
            #[cfg(test)]
            remote_license_unbinds: Mutex::new(Vec::new()),
        })
    }

    pub async fn config_snapshot(&self) -> AppConfig {
        self.config.read().await.clone()
    }

    #[cfg(any(windows, test))]
    pub(crate) fn subscribe_network_quality(
        &self,
    ) -> watch::Receiver<usque_transport::NetworkQualitySnapshot> {
        self.network_quality_tx.subscribe()
    }

    pub(crate) fn network_quality_snapshot(&self) -> usque_transport::NetworkQualitySnapshot {
        self.network_quality_tx.borrow().clone()
    }

    pub(crate) fn network_quality_payload(&self) -> Option<v1::NetworkQualitySnapshot> {
        network_quality::snapshot_payload(
            &self.network_quality_snapshot(),
            usque_transport::PRODUCTION_NETWORK_FEATURES.network_quality_metrics,
        )
    }

    async fn install_network_quality_source(
        &self,
        mut source: watch::Receiver<usque_transport::NetworkQualitySnapshot>,
    ) {
        let mut relay = self.network_quality_relay.lock().await;
        if let Some(task) = relay.take() {
            task.abort();
            let _ = task.await;
        }
        if !usque_transport::PRODUCTION_NETWORK_FEATURES.network_quality_metrics {
            return;
        }
        self.network_quality_tx
            .send_replace(source.borrow_and_update().clone());
        let destination = self.network_quality_tx.clone();
        let task = tokio::spawn(async move {
            while source.changed().await.is_ok() {
                destination.send_replace(source.borrow_and_update().clone());
            }
            destination.send_replace(network_quality::disconnected_snapshot());
        });
        *relay = Some(AbortOnDropHandle::new(task));
    }

    async fn clear_network_quality_source(&self) {
        let mut relay = self.network_quality_relay.lock().await;
        if let Some(task) = relay.take() {
            task.abort();
            let _ = task.await;
        }
        self.network_quality_tx
            .send_replace(network_quality::disconnected_snapshot());
    }

    pub(crate) fn snapshot_with_quality_to_proto(
        &self,
        snapshot: &ConnectionSnapshot,
    ) -> v1::ConnectionSnapshot {
        let mut proto = snapshot_to_proto(snapshot);
        proto.network_quality = self.network_quality_payload().map(Box::new);
        proto
    }

    /// Move schema-8 per-account proxy passwords into the single shared vault
    /// slot. Idempotent: a populated shared slot is kept, leftover account
    /// slots are deleted. Must succeed before JSON schema 9 is considered
    /// fully migrated.
    pub async fn migrate_shared_proxy_password(&self) -> Result<(), ControlServiceError> {
        let account_ids: Vec<Uuid> = {
            let config = self.config.read().await;
            config
                .profiles
                .iter()
                .map(|account| account.id)
                .chain(config.pending_identity_deletions.iter().copied())
                .collect()
        };
        let shared = self
            .vault
            .get(SHARED_NETWORK_SECRET_ID, SecretRecord::ProxyPassword)
            .await?;
        let missing_shared = match shared.as_ref() {
            None => true,
            Some(value) => value.is_empty(),
        };
        if missing_shared {
            for profile_id in &account_ids {
                let password = self
                    .vault
                    .get(*profile_id, SecretRecord::ProxyPassword)
                    .await?;
                if let Some(password) = password.filter(|value| !value.is_empty()) {
                    self.vault
                        .put(
                            SHARED_NETWORK_SECRET_ID,
                            SecretRecord::ProxyPassword,
                            &password,
                        )
                        .await?;
                    break;
                }
            }
        }
        let mut first_error = None;
        for profile_id in account_ids {
            if let Err(error) = self
                .vault
                .delete(profile_id, SecretRecord::ProxyPassword)
                .await
                && first_error.is_none()
            {
                first_error = Some(error);
            }
        }
        if let Some(error) = first_error {
            let shared = self
                .vault
                .get(SHARED_NETWORK_SECRET_ID, SecretRecord::ProxyPassword)
                .await?;
            if shared.as_ref().is_none_or(|value| value.is_empty()) {
                return Err(error.into());
            }
            tracing::warn!(
                %error,
                "old per-profile proxy passwords could not be removed after copying the shared slot"
            );
        }
        Ok(())
    }

    #[cfg(test)]
    async fn install_test_session(
        &self,
        profile: Profile,
        vpn: bool,
        reconnect_count: u32,
    ) -> Result<(), ControlServiceError> {
        let _mutation = self.mutation_lock.lock().await;
        let applied = self.upsert_profile_locked(profile.clone()).await?;
        {
            let mut state = self.state.lock().await;
            state.transition(ConnectionPhase::Preparing)?;
            state.transition(ConnectionPhase::ConnectingHttp3)?;
            state.mark_connected(Transport::Http3, AddressFamily::Ipv4, true, true)?;
            state.update_runtime_metadata(reconnect_count, Vec::new(), Vec::new());
        }
        let runtime = ActiveRuntime::Harness(active_runtime::HarnessRuntime::from_profile(
            &applied,
            vpn,
            reconnect_count,
        ));
        let quality_source = runtime.subscribe_network_quality();
        let frontends = applied.frontends;
        *self.data_plane.lock().await = Some(ActiveDataPlane {
            profile_id: applied.id,
            session_generation: self.next_session_generation(),
            frontends,
            connected_at: Instant::now(),
            last_sample_at: Instant::now(),
            last_bytes_sent: 0,
            last_bytes_received: 0,
            last_proxy_performance: ProxyPerformanceSnapshot::default(),
            runtime,
        });
        self.install_network_quality_source(quality_source).await;
        self.apply_hot_profile_state(&applied).await;
        Ok(())
    }

    #[cfg(test)]
    async fn test_harness_counts(&self) -> Option<(u32, u32, u32, u32, bool)> {
        let data_plane = self.data_plane.lock().await;
        match data_plane.as_ref().map(|active| &active.runtime) {
            Some(ActiveRuntime::Harness(runtime)) => Some((
                runtime.reconnect_count,
                runtime.reconfigure_count,
                runtime.attach_count,
                runtime.detach_count,
                runtime.vpn,
            )),
            _ => None,
        }
    }

    #[cfg(test)]
    async fn test_fail_system_proxy_after_detach(&self) {
        let mut data_plane = self.data_plane.lock().await;
        if let Some(active) = data_plane.as_mut()
            && let ActiveRuntime::Harness(runtime) = &mut active.runtime
        {
            runtime.fail_after_detach = true;
        }
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
        self.recover_pending_identity_replacements_locked().await?;
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
                    self.snapshot_with_quality_to_proto(&snapshot),
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
                    self.snapshot_with_quality_to_proto(&snapshot),
                )))
            }
            control_request::Payload::Connect(request) => {
                let id = parse_profile_id(&request.profile_id)?;
                let snapshot = self.connect(id).await?;
                Ok(control_response::Payload::Status(Box::new(
                    self.snapshot_with_quality_to_proto(&snapshot),
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
            control_request::Payload::ReconfigureActiveProfile(request) => {
                let profile = request
                    .profile
                    .ok_or_else(|| {
                        ControlServiceError::InvalidRequest(
                            "reconfigure profile payload is missing".to_owned(),
                        )
                    })
                    .and_then(profile_from_proto)?;
                let result = self.reconfigure_active_profile(profile).await?;
                Ok(control_response::Payload::Reconfigure(Box::new(result)))
            }
            control_request::Payload::CopyLicenseKey(request) => {
                self.copy_license_key(parse_profile_id(&request.profile_id)?)
                    .await?;
                Ok(control_response::Payload::Empty(v1::Empty {}))
            }
            control_request::Payload::UpdateLicenseKey(request) => {
                self.update_license_key(
                    parse_profile_id(&request.profile_id)?,
                    Zeroizing::new(request.license_key),
                )
                .await?;
                Ok(control_response::Payload::Empty(v1::Empty {}))
            }
            control_request::Payload::UnbindLicenseKey(request) => {
                self.unbind_license_key(parse_profile_id(&request.profile_id)?)
                    .await?;
                Ok(control_response::Payload::Empty(v1::Empty {}))
            }
            control_request::Payload::ExportWarpSecret(request) => {
                self.export_warp_secret(request).await?;
                Ok(control_response::Payload::Empty(v1::Empty {}))
            }
            control_request::Payload::UpdateProxyAuth(request) => {
                self.update_proxy_auth(request).await?;
                Ok(control_response::Payload::Empty(v1::Empty {}))
            }
            control_request::Payload::Retry(_) => {
                let snapshot = self.retry().await?;
                Ok(control_response::Payload::Status(Box::new(
                    self.snapshot_with_quality_to_proto(&snapshot),
                )))
            }
            control_request::Payload::ClearAllData(request) => {
                self.clear_all_data(request.confirmed).await?;
                Ok(control_response::Payload::Empty(v1::Empty {}))
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
                    package: update.package.map(|package| v1::UpdatePackage {
                        name: package.name,
                        download_url: package.download_url,
                        size: package.size,
                        sha256: package.sha256,
                        platform: package.platform,
                        variant: package.variant,
                    }),
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
                let diagnostic_session = self.diagnostics.get().await;
                if !request.diagnostic_session_id.trim().is_empty()
                    && diagnostic_session.as_ref().is_none_or(|session| {
                        session.session_id.to_string() != request.diagnostic_session_id.trim()
                    })
                {
                    return Err(ControlServiceError::InvalidRequest(
                        "the requested diagnostic session is unavailable".to_owned(),
                    ));
                }
                let timeline = self.connection_timeline_snapshot().await;
                self.maintenance
                    .export_diagnostics(
                        destination.into(),
                        config,
                        snapshot,
                        diagnostic_session,
                        timeline,
                    )
                    .await?;
                Ok(control_response::Payload::Empty(v1::Empty {}))
            }
            control_request::Payload::StartDiagnostics(request) => {
                let mode = match v1::DiagnosticMode::try_from(request.mode) {
                    Ok(v1::DiagnosticMode::Standard) => usque_core::DiagnosticMode::Standard,
                    Ok(v1::DiagnosticMode::Deep) => usque_core::DiagnosticMode::Deep,
                    _ => {
                        return Err(ControlServiceError::InvalidRequest(
                            "a supported diagnostic mode is required".to_owned(),
                        ));
                    }
                };
                let context = self.diagnostic_context(mode).await;
                let session = self.diagnostics.start(mode, context).await?;
                Ok(control_response::Payload::Diagnostics(
                    diagnostics::session_to_proto(&session),
                ))
            }
            control_request::Payload::CancelDiagnostics(request) => {
                let session_id = if request.session_id.trim().is_empty() {
                    None
                } else {
                    Some(Uuid::parse_str(request.session_id.trim()).map_err(|_| {
                        ControlServiceError::InvalidRequest(
                            "the diagnostic session id is invalid".to_owned(),
                        )
                    })?)
                };
                let session = self.diagnostics.cancel(session_id).await?;
                Ok(control_response::Payload::Diagnostics(
                    diagnostics::session_to_proto(&session),
                ))
            }
            control_request::Payload::GetDiagnostics(_) => {
                let session = self
                    .diagnostics
                    .get()
                    .await
                    .map(|session| diagnostics::session_to_proto(&session))
                    .unwrap_or_else(diagnostics::empty_session_to_proto);
                Ok(control_response::Payload::Diagnostics(session))
            }
            control_request::Payload::GetConnectionTimeline(_) => {
                let timeline = self.connection_timeline_snapshot().await;
                Ok(control_response::Payload::ConnectionTimeline(Box::new(
                    diagnostics::timeline_to_proto(&timeline),
                )))
            }
            control_request::Payload::GetNetworkQuality(_) => {
                let snapshot = self.network_quality_payload().ok_or_else(|| {
                    ControlServiceError::InvalidRequest(
                        "network quality metrics are unavailable in this build".to_owned(),
                    )
                })?;
                Ok(control_response::Payload::NetworkQuality(Box::new(
                    snapshot,
                )))
            }
            control_request::Payload::ListGeoRules(_) => Ok(
                control_response::Payload::GeoRulesList(self.list_geo_rules_locked()?),
            ),
            control_request::Payload::DownloadGeoRules(request) => {
                let results = self.download_geo_rules(&request.country_code).await?;
                Ok(control_response::Payload::GeoRulesUpdate(results))
            }
            control_request::Payload::UpdateAllGeoRules(_) => {
                let results = self.update_all_geo_rules().await?;
                Ok(control_response::Payload::GeoRulesUpdate(results))
            }
        }
    }

    pub(crate) async fn status_snapshot(&self) -> ConnectionSnapshot {
        let mut data_plane = self.data_plane.lock().await;
        let mut state = self.state.lock().await;
        if let Some(active) = data_plane.as_mut() {
            match active.runtime.health() {
                RuntimeHealth::Connected { path, .. }
                    if matches!(
                        state.snapshot().phase,
                        ConnectionPhase::Connected
                            | ConnectionPhase::Degraded
                            | ConnectionPhase::Reconnecting
                    ) && (state.snapshot().phase == ConnectionPhase::Reconnecting
                        || runtime_path_changed(state.snapshot(), path)) =>
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
                RuntimeHealth::Reconnecting { failure, .. } => {
                    if matches!(
                        state.snapshot().phase,
                        ConnectionPhase::Connected | ConnectionPhase::Degraded
                    ) && let Err(error) = state.transition(ConnectionPhase::Reconnecting)
                    {
                        state.mark_error(ConnectionError {
                            code: ErrorCode::Internal,
                            message: error.to_string(),
                            retryable: false,
                        });
                    } else {
                        state.update_failure(Some(failure));
                    }
                }
                RuntimeHealth::Failed {
                    message, failure, ..
                } => {
                    state.mark_failure(failure, message);
                }
                _ => {}
            }
            state.update_reconnect_count(active.runtime.health().reconnect_count());
            state.update_frontends(active.runtime.frontend_statuses(active.frontends));
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
            if let Some(performance) = active.runtime.proxy_performance()
                && performance != active.last_proxy_performance
            {
                tracing::debug!(
                    preferred_tcp_sockets = performance.preferred_tcp_sockets,
                    fallback_tcp_sockets = performance.fallback_tcp_sockets,
                    total_tcp_buffer_bytes = performance.total_tcp_buffer_bytes,
                    rejected_tcp_sockets = performance.rejected_tcp_sockets,
                    http_pool_hits = performance.http_pool_hits,
                    http_pool_misses = performance.http_pool_misses,
                    http_stale_retries = performance.http_stale_retries,
                    http_busy_rejections = performance.http_busy_rejections,
                    "proxy performance counters changed"
                );
                active.last_proxy_performance = performance;
            }
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

    #[cfg(any(windows, test))]
    pub(crate) async fn event_snapshot(&self) -> v1::ConnectionSnapshot {
        self.snapshot_with_quality_to_proto(&self.status_snapshot().await)
    }

    #[cfg(any(windows, test))]
    pub(crate) fn next_event_sequence(&self) -> u64 {
        self.event_sequence.fetch_add(1, Ordering::Relaxed) + 1
    }

    #[cfg(any(windows, test))]
    pub(crate) fn subscribe_geo_progress(
        &self,
    ) -> tokio::sync::broadcast::Receiver<v1::GeoRulesProgress> {
        self.geo_progress_tx.subscribe()
    }

    #[cfg(any(windows, test))]
    pub(crate) fn subscribe_diagnostics(
        &self,
    ) -> tokio::sync::broadcast::Receiver<diagnostics::DiagnosticEvent> {
        self.diagnostics.subscribe()
    }

    async fn connection_timeline_snapshot(&self) -> usque_transport::ConnectionTimelineSnapshot {
        self.data_plane
            .lock()
            .await
            .as_ref()
            .map(|active| active.runtime.connection_timeline())
            .unwrap_or_default()
    }

    async fn diagnostic_context(
        &self,
        mode: usque_core::DiagnosticMode,
    ) -> diagnostics::DiagnosticContext {
        let captured_at = tokio::time::Instant::now();
        // Unlike status polling, diagnostics must not reconcile or mutate the
        // runtime state machine as a side effect of a read-only Standard run.
        let connection = self.state.lock().await.snapshot().clone();
        let config = self.config.read().await.clone();
        let active_profile = config.active_profile();
        #[cfg(windows)]
        let platform_state = if mode == usque_core::DiagnosticMode::Deep {
            tokio::time::timeout(
                std::time::Duration::from_millis(250),
                windows_agent::inspect_platform_state_if_running(),
            )
            .await
            .ok()
            .and_then(Result::ok)
        } else {
            None
        };
        #[cfg(not(windows))]
        let platform_state = None;
        let probes = if mode == usque_core::DiagnosticMode::Deep {
            self.diagnostic_probe_context().await.map(Arc::new)
        } else {
            None
        };
        diagnostics::DiagnosticContext {
            connection,
            configuration_valid: config.validate().is_ok(),
            secure_storage_available: current_capabilities().secure_storage,
            kill_switch_expected: active_profile
                .as_ref()
                .is_some_and(|profile| profile.kill_switch),
            tunnel_dns_expected: active_profile
                .as_ref()
                .is_some_and(|profile| profile.dns_mode == DnsMode::Tunnel),
            system_proxy_expected: active_profile
                .as_ref()
                .is_some_and(|profile| profile.proxy.system_proxy),
            operating_system: std::env::consts::OS.to_owned(),
            timeline: self.connection_timeline_snapshot().await,
            platform_state,
            quality: self.network_quality_snapshot(),
            direct_dns: active_profile
                .as_ref()
                .map(|profile| profile.direct_dns.clone())
                .unwrap_or_default(),
            probes,
            captured_at,
        }
    }

    async fn diagnostic_probe_context(&self) -> Option<diagnostics::DiagnosticProbeContext> {
        let config = self.config.try_read().ok()?;
        let profile = config.active_profile()?;
        drop(config);
        {
            let data_plane = self.data_plane.try_lock().ok()?;
            if let Some(active) = data_plane.as_ref() {
                let (protector, runtime_cancel) = active.runtime.diagnostic_dns_context()?;
                return Some(diagnostics::DiagnosticProbeContext {
                    settings: profile.direct_dns.clone(),
                    protector,
                    runtime_cancel,
                    h3: None,
                    _lifecycle: None,
                });
            }
        }
        // Never wait behind a connect/reconfigure. No active path may appear
        // until the Deep session releases this guard (also on cancellation).
        let lifecycle = Arc::clone(&self.mutation_lock).try_lock_owned().ok()?;
        if self.data_plane.try_lock().ok()?.is_some()
            || self.state.try_lock().ok()?.snapshot().phase != ConnectionPhase::Disconnected
            || self.disconnect_cleanup.try_lock().ok()?.is_some()
        {
            return None;
        }
        let protector: Arc<dyn usque_transport::SocketProtector> = Arc::new(NoopSocketProtector);
        let identity = tokio::time::timeout(
            std::time::Duration::from_millis(250),
            self.load_diagnostic_tls_identity(profile.id),
        )
        .await
        .ok()
        .and_then(Result::ok);
        let endpoints = usque_transport::h3_probe_endpoints(&profile);
        Some(diagnostics::DiagnosticProbeContext {
            settings: profile.direct_dns.clone(),
            protector,
            runtime_cancel: tokio_util::sync::CancellationToken::new(),
            h3: identity
                .filter(|_| profile.transport != TransportPolicy::Http2)
                .map(|identity| (endpoints, profile.endpoint.sni.clone(), identity)),
            _lifecycle: Some(lifecycle),
        })
    }

    async fn load_diagnostic_tls_identity(
        &self,
        profile_id: Uuid,
    ) -> Result<MasqueTlsIdentity, ControlServiceError> {
        // A handshake needs no account token, device identifier or license.
        let private_key = self
            .required_secret(profile_id, SecretRecord::MasquePrivateKey)
            .await?;
        let pin = self
            .required_secret(profile_id, SecretRecord::EndpointPin)
            .await?;
        let ipv4 = self
            .required_secret(profile_id, SecretRecord::AssignedIpv4)
            .await?;
        let ipv6 = self
            .required_secret(profile_id, SecretRecord::AssignedIpv6)
            .await?;
        let ipv4 = std::str::from_utf8(&ipv4)
            .ok()
            .and_then(|value| value.parse().ok())
            .ok_or(ControlServiceError::InvalidStoredIdentity)?;
        let ipv6 = std::str::from_utf8(&ipv6)
            .ok()
            .and_then(|value| value.parse().ok())
            .ok_or(ControlServiceError::InvalidStoredIdentity)?;
        MasqueTlsIdentity::new(private_key, &pin, ipv4, ipv6).map_err(Into::into)
    }

    fn emit_geo_progress(&self, progress: GeoProgress) {
        let _ = self.geo_progress_tx.send(v1::GeoRulesProgress {
            current_file: progress.current_file,
            completed: progress.completed,
            total: progress.total,
        });
    }

    fn list_geo_rules_locked(&self) -> Result<v1::GeoRulesList, ControlServiceError> {
        let (entries, last_successful_update_unix_milliseconds) =
            list_geo_rules(&self.cache_dir).map_err(ControlServiceError::geo_rules)?;
        let (has_global_geosite, global_geosite_updated_unix_milliseconds) =
            usque_core::global_geosite_status(&self.cache_dir);
        Ok(v1::GeoRulesList {
            entries: entries
                .into_iter()
                .map(|entry| v1::GeoRulesEntry {
                    country_code: entry.country_code,
                    has_geoip: entry.has_geoip,
                    has_geosite: entry.has_geosite,
                    last_updated_unix_milliseconds: entry.last_updated_unix_milliseconds,
                })
                .collect(),
            last_successful_update_unix_milliseconds,
            has_global_geosite,
            global_geosite_updated_unix_milliseconds,
        })
    }

    fn geo_downloader(&self) -> Result<GeoDownloader<ReqwestFetch>, ControlServiceError> {
        let fetch = ReqwestFetch::new().map_err(ControlServiceError::geo_rules)?;
        Ok(GeoDownloader::new(fetch, self.cache_dir.clone()))
    }

    async fn download_geo_rules(
        &self,
        country_code: &str,
    ) -> Result<v1::GeoRulesUpdateResults, ControlServiceError> {
        let downloader = self.geo_downloader()?;
        let results = download_geo_rules(&downloader, country_code, |progress| {
            self.emit_geo_progress(progress);
        })
        .await
        .map_err(ControlServiceError::geo_rules)?;
        Ok(geo_results_to_proto(results))
    }

    async fn update_all_geo_rules(&self) -> Result<v1::GeoRulesUpdateResults, ControlServiceError> {
        let downloader = self.geo_downloader()?;
        let results = update_all_geo_rules(&downloader, |progress| {
            self.emit_geo_progress(progress);
        })
        .await
        .map_err(ControlServiceError::geo_rules)?;
        Ok(geo_results_to_proto(results))
    }

    fn next_session_generation(&self) -> u64 {
        self.session_generation.fetch_add(1, Ordering::Relaxed) + 1
    }

    async fn connect(&self, profile_id: Uuid) -> Result<ConnectionSnapshot, ControlServiceError> {
        let _mutation = self.mutation_lock.lock().await;
        self.connect_locked(profile_id).await
    }

    pub(crate) async fn connect_locked(
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

        let mut profile = {
            let config = self.config.read().await;
            if config.zero_trust_endpoint_needs_reauthentication(profile_id) {
                return Err(ControlServiceError::InvalidStoredIdentity);
            }
            config
                .runtime_profile(profile_id)
                .ok_or(ControlServiceError::ProfileNotFound(profile_id))?
        };
        self.attach_proxy_auth(&mut profile).await?;
        if !usque_transport::ENCRYPTED_DIRECT_DNS_ENABLED
            && profile.direct_dns.mode != ConfigDirectDnsMode::PhysicalSystem
        {
            return Err(ControlServiceError::FeatureUnavailable(
                "encrypted direct DNS is unavailable in this build",
            ));
        }
        if profile.frontends.tunnel && !cfg!(windows) {
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
        let geo_policy = load_geo_direct_policy(&profile, &self.cache_dir);
        if !profile.geo_direct_countries.is_empty() && !geo_policy.is_enabled() {
            let error = ControlServiceError::GeoRules(
                "the configured GeoIP/GeoSite cache is missing or invalid".to_owned(),
            );
            self.mark_connection_error(&error).await;
            return Err(error);
        }

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

        let runtime = if profile.frontends.tunnel {
            #[cfg(windows)]
            {
                match windows_agent::WindowsVpnRuntime::start(
                    &profile,
                    identity,
                    Arc::clone(&pin_refresher),
                    Arc::new(geo_policy.clone()),
                )
                .await
                {
                    Ok(runtime) => ActiveRuntime::Vpn(Box::new(runtime)),
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
            match ProxyRuntime::start_with_geo_policy(
                &profile,
                identity,
                Arc::new(NoopSocketProtector),
                Some(pin_refresher),
                geo_policy,
            )
            .await
            {
                Ok(runtime) => {
                    #[cfg(windows)]
                    let mut runtime = runtime;
                    #[cfg(windows)]
                    let system_proxy = if profile.frontends.http && profile.proxy.system_proxy {
                        let Some(listener) =
                            windows_agent::loopback_http_listener(runtime.http_listeners())
                        else {
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
                    ActiveRuntime::Proxy(Box::new(ActiveProxyRuntime {
                        runtime,
                        #[cfg(windows)]
                        system_proxy,
                    }))
                }
                Err(error) => {
                    let error = ControlServiceError::Transport(error);
                    self.mark_connection_error(&error).await;
                    return Err(error);
                }
            }
        };
        let path = runtime.path();
        let listener_auth = profile.proxy.listener_credentials().ok().flatten();
        let exit_probe = exit_probe_for_session(
            &profile,
            &runtime,
            self.store.path(),
            listener_auth.as_ref(),
        );
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
            let mut warnings = Vec::new();
            if (profile.frontends.socks5 && profile.proxy.socks5_exposes_lan())
                || (profile.frontends.http && profile.proxy.http_exposes_lan())
            {
                warnings.push(ConnectionWarning {
                    code: "LAN_EXPOSED".to_owned(),
                    message: if profile.proxy.listener_auth_username().is_some() {
                        "The proxy accepts authenticated non-loopback clients."
                            .to_owned()
                    } else {
                        "The proxy accepts non-loopback clients without username/password authentication."
                            .to_owned()
                    },
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
            if profile.frontends.tunnel && !profile.kill_switch {
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
            state.update_frontends(runtime.frontend_statuses(profile.frontends));
            state.update_safety_state(
                if profile.frontends.tunnel {
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
        let session_generation = self.next_session_generation();
        let quality_source = runtime.subscribe_network_quality();
        *self.data_plane.lock().await = Some(ActiveDataPlane {
            profile_id,
            session_generation,
            frontends: profile.frontends,
            connected_at: Instant::now(),
            last_sample_at: Instant::now(),
            last_bytes_sent: 0,
            last_bytes_received: 0,
            last_proxy_performance: ProxyPerformanceSnapshot::default(),
            runtime,
        });
        self.install_network_quality_source(quality_source).await;
        // Location is diagnostic: report Connected immediately and fill ip.sb
        // later, matching the Android runtime. Probe failure must not delay or
        // tear down a healthy session.
        self.spawn_exit_probe(exit_probe, profile_id, session_generation)
            .await;
        Ok(snapshot)
    }

    async fn spawn_exit_probe(
        &self,
        probe: Option<IpSbProbe>,
        profile_id: Uuid,
        session_generation: u64,
    ) {
        self.abort_exit_probe().await;
        let Some(probe) = probe else {
            return;
        };
        let state = Arc::clone(&self.state);
        let data_plane = Arc::clone(&self.data_plane);
        *self.exit_probe_task.lock().await = Some(tokio::spawn(async move {
            let Ok(exit) = probe.probe().await else {
                return;
            };
            apply_exit_info(&state, &data_plane, profile_id, session_generation, exit).await;
        }));
    }

    async fn abort_exit_probe(&self) {
        if let Some(task) = self.exit_probe_task.lock().await.take() {
            task.abort();
        }
    }

    async fn disconnect(&self) -> Result<ConnectionSnapshot, ControlServiceError> {
        let _mutation = self.mutation_lock.lock().await;
        self.disconnect_locked().await
    }

    pub(crate) async fn disconnect_locked(
        &self,
    ) -> Result<ConnectionSnapshot, ControlServiceError> {
        self.abort_exit_probe().await;
        self.clear_network_quality_source().await;
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
            .chain(config.pending_identity_local_deletions.iter().copied())
            .chain(
                config
                    .pending_identity_replacements
                    .values()
                    .filter_map(|replacement| replacement.backup_identity_id),
            )
            .collect::<std::collections::HashSet<_>>();
        drop(config);
        for profile_id in profile_ids {
            self.vault.delete_identity(profile_id).await?;
        }
        self.vault
            .delete(SHARED_NETWORK_SECRET_ID, SecretRecord::ProxyPassword)
            .await?;
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
        let metadata = self.load_identity_metadata(profile_id).await?;
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
            metadata.provider,
            metadata.entitlement,
            assigned_ipv4,
            assigned_ipv6,
        )
        .map_err(Into::into)
    }

    async fn load_identity_provider(
        &self,
        profile_id: Uuid,
    ) -> Result<IdentityProvider, ControlServiceError> {
        Ok(self.load_identity_metadata(profile_id).await?.provider)
    }

    async fn load_identity_metadata(
        &self,
        profile_id: Uuid,
    ) -> Result<IdentityMetadata, ControlServiceError> {
        let config = self.config.read().await;
        let binding = config.identity_bindings.get(&profile_id).cloned();
        drop(config);
        self.load_identity_metadata_for_profile(profile_id, binding.as_ref())
            .await
    }

    async fn load_identity_provider_for_profile(
        &self,
        profile_id: Uuid,
        binding: Option<&IdentityProvider>,
    ) -> Result<IdentityProvider, ControlServiceError> {
        Ok(self
            .load_identity_metadata_for_profile(profile_id, binding)
            .await?
            .provider)
    }

    async fn load_identity_metadata_for_profile(
        &self,
        profile_id: Uuid,
        binding: Option<&IdentityProvider>,
    ) -> Result<IdentityMetadata, ControlServiceError> {
        let metadata = self
            .vault
            .get(profile_id, SecretRecord::IdentityMetadata)
            .await?;
        match metadata {
            Some(metadata) => {
                let metadata = IdentityMetadata::from_json(&metadata)
                    .map_err(|_| ControlServiceError::InvalidStoredIdentity)?;
                if binding.is_some_and(|binding| binding != &metadata.provider) {
                    return Err(ControlServiceError::InvalidStoredIdentity);
                }
                Ok(metadata)
            }
            None if binding.is_some() => Err(ControlServiceError::InvalidStoredIdentity),
            None => Ok(IdentityMetadata {
                provider: IdentityProvider::Consumer,
                entitlement: None,
            }),
        }
    }

    async fn load_identity_boundary_for_repair(
        &self,
        profile_id: Uuid,
    ) -> Result<IdentityProvider, ControlServiceError> {
        let binding = self
            .config
            .read()
            .await
            .identity_bindings
            .get(&profile_id)
            .cloned();
        match self.load_identity_provider(profile_id).await {
            Ok(provider) => Ok(provider),
            Err(ControlServiceError::InvalidStoredIdentity) => {
                binding.ok_or(ControlServiceError::InvalidStoredIdentity)
            }
            Err(error) => Err(error),
        }
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
        let mut state = self.state.lock().await;
        if let ControlServiceError::Transport(transport) = error {
            state.mark_failure(
                transport.failure(None, None),
                error.as_structured_error().message,
            );
        } else {
            state.mark_error(connection_error_for(error));
        }
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

        let manual_secret = Zeroizing::new(request.warp_secret);
        if !manual_secret.is_empty() {
            return Err(ControlServiceError::FeatureRemoved("WARP Secret import"));
        }
        let license_key = Zeroizing::new(request.license_key);
        let method = v1::IdentityProvisioningMethod::try_from(request.method)
            .unwrap_or(v1::IdentityProvisioningMethod::Unspecified);
        let options = registration_options(request.device_name, request.locale);
        let client = ConsumerRegistrationClient::new()?;
        let existing_provider = self.load_identity_boundary_for_repair(profile_id).await?;
        let provisioned = match method {
            v1::IdentityProvisioningMethod::Register => {
                if !license_key.is_empty() {
                    return Err(ControlServiceError::InvalidRequest(
                        "registration provisioning must not contain a License Key".to_owned(),
                    ));
                }
                Self::require_consumer_identity(&existing_provider)?;
                ProvisionedIdentity::consumer(client.register(&options).await?)
            }
            v1::IdentityProvisioningMethod::RegisterWithLicense => {
                Self::require_consumer_identity(&existing_provider)?;
                let license = std::str::from_utf8(&license_key)
                    .map_err(|_| ControlServiceError::InvalidLicenseEncoding)?;
                ProvisionedIdentity::consumer(
                    client.register_with_license(&options, license).await?,
                )
            }
            v1::IdentityProvisioningMethod::RegisterZeroTrust => {
                if !license_key.is_empty() {
                    return Err(ControlServiceError::IdentityOperationUnsupported);
                }
                let enrollment = request.zero_trust.ok_or_else(|| {
                    ControlServiceError::InvalidRequest(
                        "Zero Trust enrollment details are missing".to_owned(),
                    )
                })?;
                let team = normalize_zero_trust_team(&enrollment.team_name)?;
                match &existing_provider {
                    IdentityProvider::ZeroTrust { organization } if organization == &team => {}
                    _ => return Err(ControlServiceError::IdentityProviderChangeUnsupported),
                }
                let callback = Zeroizing::new(enrollment.callback_uri);
                let callback = std::str::from_utf8(&callback)
                    .map_err(|_| RegistrationError::InvalidZeroTrustCallback)?;
                let result = client
                    .register_zero_trust(&options, &team, callback)
                    .await?;
                ProvisionedIdentity::zero_trust(result.identity, &result.endpoint)
            }
            v1::IdentityProvisioningMethod::ImportSecret => {
                return Err(ControlServiceError::FeatureRemoved("WARP Secret import"));
            }
            // Older clients did not send this field. Preserve their existing
            // license-key based Consumer provisioning behavior.
            v1::IdentityProvisioningMethod::Unspecified => {
                Self::require_consumer_identity(&existing_provider)?;
                if license_key.is_empty() {
                    ProvisionedIdentity::consumer(client.register(&options).await?)
                } else {
                    let license = std::str::from_utf8(&license_key)
                        .map_err(|_| ControlServiceError::InvalidLicenseEncoding)?;
                    ProvisionedIdentity::consumer(
                        client.register_with_license(&options, license).await?,
                    )
                }
            }
        };

        let is_zero_trust = provisioned.is_zero_trust();
        let _mutation = self.mutation_lock.lock().await;
        if let Err(error) = self.ensure_profile_exists(profile_id).await {
            return Err(Self::after_zero_trust_registration(error, is_zero_trust));
        }
        let current_provider = match self.load_identity_boundary_for_repair(profile_id).await {
            Ok(provider) => provider,
            Err(error) => {
                return Err(Self::after_zero_trust_registration(error, is_zero_trust));
            }
        };
        if current_provider != existing_provider {
            return Err(Self::after_zero_trust_registration(
                ControlServiceError::IdentityProviderChangeUnsupported,
                is_zero_trust,
            ));
        }
        let reconnect = self.connected_profile_id().await == Some(profile_id);
        if reconnect {
            if let Err(error) = self.disconnect_locked().await {
                return Err(Self::after_zero_trust_registration(error, is_zero_trust));
            }
            if let Err(error) = self.await_disconnect_cleanup().await {
                return Err(Self::after_zero_trust_registration(error, is_zero_trust));
            }
        }
        let (identity, managed_endpoint_ips) = provisioned.into_parts();
        if let Err(error) = self
            .replace_identity_locked(profile_id, identity, managed_endpoint_ips)
            .await
        {
            return Err(Self::after_zero_trust_registration(error, is_zero_trust));
        }
        if reconnect {
            self.connect_locked(profile_id).await?;
        }
        Ok(())
    }

    async fn create_profile_with_identity(
        &self,
        profile: Profile,
        provisioning: v1::IdentityProvisioning,
    ) -> Result<(), ControlServiceError> {
        profile
            .validate()
            .map_err(ControlServiceError::profile_configuration)?;
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
        let provisioned = match method {
            v1::IdentityProvisioningMethod::Register => {
                if !secret.is_empty() {
                    return Err(ControlServiceError::InvalidRequest(
                        "registration provisioning must not contain a WARP Secret".to_owned(),
                    ));
                }
                let options = registration_options(
                    provisioning.device_name.clone(),
                    provisioning.locale.clone(),
                );
                ProvisionedIdentity::consumer(
                    ConsumerRegistrationClient::new()?
                        .register(&options)
                        .await?,
                )
            }
            v1::IdentityProvisioningMethod::ImportSecret => {
                return Err(ControlServiceError::FeatureRemoved("WARP Secret import"));
            }
            v1::IdentityProvisioningMethod::RegisterWithLicense => {
                if !secret.is_empty() {
                    return Err(ControlServiceError::InvalidRequest(
                        "License provisioning must not contain a WARP Secret".to_owned(),
                    ));
                }
                let license_key = Zeroizing::new(provisioning.license_key);
                let license = std::str::from_utf8(&license_key)
                    .map_err(|_| ControlServiceError::InvalidLicenseEncoding)?;
                let options = registration_options(provisioning.device_name, provisioning.locale);
                ProvisionedIdentity::consumer(
                    ConsumerRegistrationClient::new()?
                        .register_with_license(&options, license)
                        .await?,
                )
            }
            v1::IdentityProvisioningMethod::RegisterZeroTrust => {
                if !secret.is_empty() || !provisioning.license_key.is_empty() {
                    return Err(ControlServiceError::IdentityOperationUnsupported);
                }
                let enrollment = provisioning.zero_trust.ok_or_else(|| {
                    ControlServiceError::InvalidRequest(
                        "Zero Trust enrollment details are missing".to_owned(),
                    )
                })?;
                let callback = Zeroizing::new(enrollment.callback_uri);
                let callback = std::str::from_utf8(&callback)
                    .map_err(|_| RegistrationError::InvalidZeroTrustCallback)?;
                let options = registration_options(provisioning.device_name, provisioning.locale);
                let result = ConsumerRegistrationClient::new()?
                    .register_zero_trust(&options, &enrollment.team_name, callback)
                    .await?;
                ProvisionedIdentity::zero_trust(result.identity, &result.endpoint)
            }
            v1::IdentityProvisioningMethod::Unspecified => {
                return Err(ControlServiceError::InvalidRequest(
                    "identity provisioning method is missing".to_owned(),
                ));
            }
        };

        let is_zero_trust = provisioned.is_zero_trust();
        let identity_provider = provisioned.identity().provider().clone();
        let managed_endpoint_ips = provisioned.managed_endpoint_ips().cloned();

        let _mutation = self.mutation_lock.lock().await;
        let mut pending = self.config.read().await.clone();
        if pending
            .profiles
            .iter()
            .any(|existing| existing.id == profile.id)
            || pending.pending_identity_creations.contains(&profile.id)
        {
            return Err(Self::after_zero_trust_registration(
                ControlServiceError::InvalidRequest(format!(
                    "profile already exists or is being created: {}",
                    profile.id
                )),
                is_zero_trust,
            ));
        }
        pending.pending_identity_creations.push(profile.id);
        if let Err(error) = self.persist(pending).await {
            return Err(Self::after_zero_trust_registration(error, is_zero_trust));
        }

        if let Err(error) = self
            .persist_identity(profile.id, provisioned.identity(), None)
            .await
        {
            self.abort_pending_identity_creation(profile.id).await;
            return Err(Self::after_zero_trust_registration(error, is_zero_trust));
        }

        self.commit_pending_profile_identity(
            profile,
            identity_provider,
            managed_endpoint_ips,
            is_zero_trust,
        )
        .await
    }

    async fn commit_pending_profile_identity(
        &self,
        profile: Profile,
        identity_provider: IdentityProvider,
        managed_endpoint_ips: Option<ManagedEndpointIps>,
        is_zero_trust: bool,
    ) -> Result<(), ControlServiceError> {
        let profile_id = profile.id;
        let mut committed = self.config.read().await.clone();
        committed
            .pending_identity_creations
            .retain(|profile_id| *profile_id != profile.id);
        committed
            .identity_bindings
            .insert(profile.id, identity_provider);
        if let Err(error) = committed.insert_account(profile.id, profile.name, managed_endpoint_ips)
        {
            self.abort_pending_identity_creation(profile.id).await;
            return Err(Self::after_zero_trust_registration(
                ControlServiceError::configuration(error),
                is_zero_trust,
            ));
        }
        if let Err(error) = self.persist(committed).await {
            let _ = self.vault.delete_identity(profile_id).await;
            self.abort_pending_identity_creation(profile_id).await;
            return Err(Self::after_zero_trust_registration(error, is_zero_trust));
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
        let cleanup_pending = !config.pending_identity_deletions.is_empty()
            || !config.pending_identity_local_deletions.is_empty()
            || !config.pending_identity_replacements.is_empty();
        for account in &config.profiles {
            let binding = config.identity_bindings.get(&account.id).cloned();
            let stored_provider = self
                .load_identity_provider_for_profile(account.id, binding.as_ref())
                .await
                .ok();
            let boundary_provider = stored_provider.clone().or(binding);
            let (mut state, mut license_state, mut account_type, provider) =
                match self.load_warp_identity(account.id).await {
                    Ok(identity)
                        if matches!(identity.provider(), IdentityProvider::ZeroTrust { .. }) =>
                    {
                        (
                            v1::ProfileIdentityState::Ready,
                            v1::LicenseState::NotApplicable,
                            "Zero Trust".to_owned(),
                            identity.provider().clone(),
                        )
                    }
                    Ok(identity) => {
                        let (license_state, account_type) = match identity.entitlement() {
                            Some(entitlement) => consumer_license_presentation(entitlement),
                            None => (v1::LicenseState::Unknown, String::new()),
                        };
                        (
                            v1::ProfileIdentityState::Ready,
                            license_state,
                            account_type,
                            IdentityProvider::Consumer,
                        )
                    }
                    Err(ControlServiceError::MissingCredential(_)) => (
                        v1::ProfileIdentityState::Missing,
                        v1::LicenseState::Unknown,
                        String::new(),
                        boundary_provider.clone().unwrap_or_default(),
                    ),
                    Err(_) => (
                        v1::ProfileIdentityState::Invalid,
                        v1::LicenseState::Unknown,
                        String::new(),
                        boundary_provider.clone().unwrap_or_default(),
                    ),
                };
            let zero_trust = matches!(provider, IdentityProvider::ZeroTrust { .. });
            if zero_trust {
                if account.managed_endpoint_ips.is_none()
                    && state == v1::ProfileIdentityState::Ready
                {
                    state = v1::ProfileIdentityState::Invalid;
                }
                license_state = v1::LicenseState::NotApplicable;
                if account_type.is_empty() {
                    account_type = "Zero Trust".to_owned();
                }
            }
            let organization = provider.organization().unwrap_or_default().to_owned();
            let provider = if zero_trust {
                v1::IdentityProvider::ZeroTrust
            } else {
                v1::IdentityProvider::Consumer
            };
            catalog.identity_statuses.push(v1::ProfileIdentityStatus {
                profile_id: account.id.to_string(),
                state: state as i32,
                license_state: license_state as i32,
                account_type,
                cleanup_pending,
                provider: provider as i32,
                organization,
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
        let mut records = Vec::with_capacity(9);
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
        records.push((SecretRecord::IdentityMetadata, identity.to_metadata_json()?));

        for (record, value) in records {
            if let Err(error) = self.vault.put(profile_id, record, &value).await {
                let _ = self.vault.delete_identity(profile_id).await;
                return Err(error.into());
            }
        }
        if manual_secret.is_none()
            && let Err(error) = self
                .vault
                .delete(profile_id, SecretRecord::WarpSecret)
                .await
        {
            let _ = self.vault.delete_identity(profile_id).await;
            return Err(error.into());
        }
        if identity.license().is_none()
            && let Err(error) = self.vault.delete(profile_id, SecretRecord::License).await
        {
            let _ = self.vault.delete_identity(profile_id).await;
            return Err(error.into());
        }
        Ok(())
    }

    async fn copy_license_key(&self, profile_id: Uuid) -> Result<(), ControlServiceError> {
        self.ensure_profile_exists(profile_id).await?;
        Self::require_consumer_identity(&self.load_identity_provider(profile_id).await?)?;
        let license = self
            .required_secret(profile_id, SecretRecord::License)
            .await?;
        sensitive_output::copy_sensitive_text(&license)
            .map_err(ControlServiceError::SensitiveOutput)
    }

    async fn update_license_key(
        &self,
        profile_id: Uuid,
        license_key: Zeroizing<Vec<u8>>,
    ) -> Result<(), ControlServiceError> {
        self.ensure_profile_exists(profile_id).await?;
        let license = std::str::from_utf8(&license_key)
            .map_err(|_| ControlServiceError::InvalidLicenseEncoding)?;
        let _mutation = self.mutation_lock.lock().await;
        Self::require_consumer_identity(&self.load_identity_provider(profile_id).await?)?;
        let reconnect = self.connected_profile_id().await == Some(profile_id);
        if reconnect {
            self.disconnect_locked().await?;
            self.await_disconnect_cleanup().await?;
        }
        let new_identity = ConsumerRegistrationClient::new()?
            .register_with_license(
                &registration_options(String::new(), "en_US".to_owned()),
                license,
            )
            .await?;
        self.replace_identity_locked(profile_id, new_identity, None)
            .await?;
        if reconnect {
            self.connect_locked(profile_id).await?;
        }
        Ok(())
    }

    async fn unbind_license_key(&self, profile_id: Uuid) -> Result<(), ControlServiceError> {
        self.ensure_profile_exists(profile_id).await?;
        let _mutation = self.mutation_lock.lock().await;
        Self::require_consumer_identity(&self.load_identity_provider(profile_id).await?)?;
        let reconnect = self.connected_profile_id().await == Some(profile_id);
        if reconnect {
            self.disconnect_locked().await?;
            self.await_disconnect_cleanup().await?;
        }
        let new_identity = ConsumerRegistrationClient::new()?
            .register(&registration_options(String::new(), "en_US".to_owned()))
            .await?;
        self.replace_identity_locked(profile_id, new_identity, None)
            .await?;
        if reconnect {
            self.connect_locked(profile_id).await?;
        }
        Ok(())
    }

    async fn export_warp_secret(
        &self,
        request: v1::ExportWarpSecretRequest,
    ) -> Result<(), ControlServiceError> {
        if !request.confirmed {
            return Err(ControlServiceError::ConfirmationRequired);
        }
        if request.destination.trim().is_empty() {
            return Err(ControlServiceError::InvalidRequest(
                "WARP Secret export destination is missing".to_owned(),
            ));
        }
        let profile_id = parse_profile_id(&request.profile_id)?;
        self.ensure_profile_exists(profile_id).await?;
        Self::require_consumer_identity(&self.load_identity_provider(profile_id).await?)?;
        let identity = self.load_warp_identity(profile_id).await?;
        let secret = identity.to_portable_secret_json()?;
        let destination = std::path::PathBuf::from(request.destination);
        tokio::task::spawn_blocking(move || {
            sensitive_output::export_secret_noclobber(&destination, &secret)
        })
        .await
        .map_err(|error| ControlServiceError::SensitiveOutputWorker(error.to_string()))?
        .map_err(ControlServiceError::SensitiveOutput)
    }

    async fn update_proxy_auth(
        &self,
        request: v1::UpdateProxyAuthRequest,
    ) -> Result<(), ControlServiceError> {
        if !request.confirmed {
            return Err(ControlServiceError::ConfirmationRequired);
        }
        let profile_id = parse_profile_id(&request.profile_id)?;
        self.ensure_profile_exists(profile_id).await?;
        let username = request.username;
        let password = Zeroizing::new(request.password);
        if username.is_empty() {
            if !password.is_empty() {
                return Err(ControlServiceError::InvalidProxyAuth(
                    "proxy password requires a username".to_owned(),
                ));
            }
        } else {
            validate_proxy_username(&username).map_err(ControlServiceError::invalid_proxy_auth)?;
            if password.is_empty() {
                return Err(ControlServiceError::InvalidProxyAuth(
                    ConfigError::ProxyAuthRequiresPassword.to_string(),
                ));
            }
            validate_proxy_password(&password).map_err(ControlServiceError::invalid_proxy_auth)?;
            let _ = ProxyAuthCredentials::parse(&username, &password)
                .map_err(ControlServiceError::invalid_proxy_auth)?;
        }

        let _mutation = self.mutation_lock.lock().await;
        self.migrate_shared_proxy_password().await?;
        let mut next = self.config.read().await.clone();
        if !next.profiles.iter().any(|profile| profile.id == profile_id) {
            return Err(ControlServiceError::ProfileNotFound(profile_id));
        }
        if username.is_empty() {
            next.network.proxy.auth_username = None;
            next.network.proxy.auth_password = None;
            self.vault
                .delete(SHARED_NETWORK_SECRET_ID, SecretRecord::ProxyPassword)
                .await?;
        } else {
            next.network.proxy.auth_username = Some(username);
            self.vault
                .put(
                    SHARED_NETWORK_SECRET_ID,
                    SecretRecord::ProxyPassword,
                    &password,
                )
                .await?;
        }
        self.persist(next).await
    }

    async fn attach_proxy_auth(&self, profile: &mut Profile) -> Result<(), ControlServiceError> {
        self.migrate_shared_proxy_password().await?;
        profile.proxy.normalize_auth();
        match profile.proxy.listener_auth_username() {
            None => {
                profile.proxy.auth_password = None;
                Ok(())
            }
            Some(_) => {
                let password = self
                    .vault
                    .get(SHARED_NETWORK_SECRET_ID, SecretRecord::ProxyPassword)
                    .await?;
                let Some(password) = password.filter(|value| !value.is_empty()) else {
                    return Err(ControlServiceError::InvalidProxyAuth(
                        ConfigError::ProxyAuthRequiresPassword.to_string(),
                    ));
                };
                profile.proxy.auth_password = Some(password);
                profile
                    .proxy
                    .listener_credentials()
                    .map_err(ControlServiceError::invalid_proxy_auth)?;
                Ok(())
            }
        }
    }

    async fn ensure_profile_exists(&self, profile_id: Uuid) -> Result<(), ControlServiceError> {
        if self
            .config
            .read()
            .await
            .profiles
            .iter()
            .any(|profile| profile.id == profile_id)
        {
            Ok(())
        } else {
            Err(ControlServiceError::ProfileNotFound(profile_id))
        }
    }

    fn require_consumer_identity(provider: &IdentityProvider) -> Result<(), ControlServiceError> {
        if matches!(provider, IdentityProvider::Consumer) {
            Ok(())
        } else {
            Err(ControlServiceError::IdentityOperationUnsupported)
        }
    }

    fn after_zero_trust_registration(
        error: ControlServiceError,
        zero_trust_registered: bool,
    ) -> ControlServiceError {
        if zero_trust_registered {
            tracing::warn!(%error, "local Zero Trust identity commit failed after remote registration");
            ControlServiceError::ZeroTrustLocalCommit
        } else {
            error
        }
    }

    async fn connected_profile_id(&self) -> Option<Uuid> {
        self.data_plane
            .lock()
            .await
            .as_ref()
            .map(|active| active.profile_id)
    }

    async fn replace_identity_locked(
        &self,
        profile_id: Uuid,
        new_identity: WarpIdentity,
        managed_endpoint_ips: Option<ManagedEndpointIps>,
    ) -> Result<(), ControlServiceError> {
        let new_provider = new_identity.provider().clone();
        let current = self.config.read().await.clone();
        if current.account(profile_id).is_none() {
            return Err(ControlServiceError::ProfileNotFound(profile_id));
        }
        if current
            .pending_identity_replacements
            .contains_key(&profile_id)
        {
            return Err(ControlServiceError::InvalidRequest(format!(
                "identity replacement is already pending for profile {profile_id}"
            )));
        }
        let mut next = current.clone();
        match (&new_provider, managed_endpoint_ips) {
            (IdentityProvider::ZeroTrust { .. }, Some(managed_endpoint_ips)) => {
                next.set_managed_endpoint_ips(profile_id, managed_endpoint_ips)
                    .map_err(ControlServiceError::configuration)?;
            }
            (IdentityProvider::Consumer, _) => {
                next.account_mut(profile_id)
                    .ok_or(ControlServiceError::ProfileNotFound(profile_id))?
                    .managed_endpoint_ips = None;
            }
            (IdentityProvider::ZeroTrust { .. }, None) => {}
        }
        next.identity_bindings.insert(profile_id, new_provider);

        let previous = match self.load_warp_identity(profile_id).await {
            Ok(identity) => Some(identity),
            Err(
                ControlServiceError::MissingCredential(_)
                | ControlServiceError::InvalidStoredIdentity
                | ControlServiceError::Identity(_),
            ) => None,
            Err(error) => return Err(error),
        };
        let cleanup_id = previous.as_ref().map(|_| Uuid::new_v4());
        if let (Some(cleanup_id), Some(previous)) = (cleanup_id, previous.as_ref()) {
            self.persist_identity(cleanup_id, previous, None).await?;
        }

        let mut staged = current;
        staged.pending_identity_replacements.insert(
            profile_id,
            PendingIdentityReplacement {
                backup_identity_id: cleanup_id,
                armed: true,
            },
        );
        if let Err(error) = self.persist(staged).await {
            if let Some(cleanup_id) = cleanup_id {
                let _ = self.vault.delete_identity(cleanup_id).await;
            }
            return Err(error);
        }

        if let Err(error) = self.persist_identity(profile_id, &new_identity, None).await {
            self.rollback_pending_identity_replacement_locked(
                profile_id,
                previous.as_ref(),
                cleanup_id,
            )
            .await?;
            return Err(error);
        }

        if let Some(cleanup_id) = cleanup_id {
            next.pending_identity_deletions.push(cleanup_id);
        }
        if let Err(error) = self.persist(next).await {
            if new_identity.license().is_some() {
                let _ = ConsumerRegistrationClient::new()?
                    .unbind_license(&new_identity)
                    .await;
            }
            self.rollback_pending_identity_replacement_locked(
                profile_id,
                previous.as_ref(),
                cleanup_id,
            )
            .await?;
            return Err(error);
        }

        if let Err(error) = self.reap_pending_identity_deletions_locked().await {
            tracing::warn!(%error, "old WARP device cleanup was queued for a later retry");
        }
        Ok(())
    }

    async fn rollback_pending_identity_replacement_locked(
        &self,
        profile_id: Uuid,
        previous: Option<&WarpIdentity>,
        backup_id: Option<Uuid>,
    ) -> Result<(), ControlServiceError> {
        if let Some(previous) = previous {
            self.persist_identity(profile_id, previous, None).await?;
        } else {
            self.vault.delete_identity(profile_id).await?;
        }

        let mut rollback = self.config.read().await.clone();
        rollback.pending_identity_replacements.remove(&profile_id);
        if let Some(backup_id) = backup_id
            && !rollback
                .pending_identity_local_deletions
                .contains(&backup_id)
        {
            rollback.pending_identity_local_deletions.push(backup_id);
        }
        self.persist(rollback).await?;
        if let Err(error) = self.reap_pending_identity_deletions_locked().await {
            tracing::warn!(%error, "identity replacement rollback cleanup was queued for retry");
        }
        Ok(())
    }

    async fn upsert_profile(&self, profile: Profile) -> Result<Profile, ControlServiceError> {
        profile
            .validate()
            .map_err(ControlServiceError::profile_configuration)?;
        let _mutation = self.mutation_lock.lock().await;
        self.upsert_profile_locked(profile).await
    }

    pub(crate) async fn upsert_profile_locked(
        &self,
        profile: Profile,
    ) -> Result<Profile, ControlServiceError> {
        let mut next = self.config.read().await.clone();
        let stored = next
            .upsert_runtime_profile(profile)
            .map_err(ControlServiceError::configuration)?;
        if next.active_profile_id.is_none() {
            next.active_profile_id = Some(stored.id);
        }
        self.persist(next).await?;
        Ok(stored)
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
            for profile in &profiles {
                if !incoming_ids.insert(profile.id) {
                    return Err(ControlServiceError::InvalidRequest(
                        "legacy profile IDs must be unique".to_owned(),
                    ));
                }
            }
            if let Some(active_profile_id) = active_profile_id {
                if !incoming_ids.contains(&active_profile_id)
                    && next.account(active_profile_id).is_none()
                {
                    return Err(ControlServiceError::InvalidRequest(
                        "legacy active profile does not exist".to_owned(),
                    ));
                }
                next.active_profile_id = Some(active_profile_id);
            }
            if let Some(active) = profiles
                .iter()
                .find(|profile| Some(profile.id) == next.active_profile_id)
            {
                let mut network = SharedNetworkSettings::from_profile(active);
                if active.endpoint.is_zero_trust_managed() {
                    network.endpoint = profiles
                        .iter()
                        .find(|profile| !profile.endpoint.is_zero_trust_managed())
                        .map(|profile| profile.endpoint.clone())
                        .unwrap_or_default();
                }
                next.network = network;
            }
            next.profiles.clear();
            for profile in profiles {
                let managed_endpoint_ips = profile
                    .endpoint
                    .is_zero_trust_managed()
                    .then(|| ManagedEndpointIps::from_endpoint(&profile.endpoint));
                next.insert_account(profile.id, profile.name, managed_endpoint_ips)
                    .map_err(ControlServiceError::configuration)?;
            }
            if next.active_profile().is_none() {
                next.active_profile_id = next.profiles.first().map(|account| account.id);
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
        next.identity_bindings.remove(&id);
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

    /// Roll back an identity replacement that was interrupted after its vault
    /// write but before the matching non-secret endpoint/configuration commit.
    /// The journal remains durable until the old identity has been restored.
    async fn recover_pending_identity_replacements_locked(
        &self,
    ) -> Result<(), ControlServiceError> {
        let snapshot = self.config.read().await.clone();
        if snapshot.pending_identity_replacements.is_empty() {
            return Ok(());
        }

        let mut completed = Vec::new();
        let mut first_error = None;
        for (profile_id, replacement) in &snapshot.pending_identity_replacements {
            if !replacement.armed {
                completed.push((*profile_id, replacement.backup_identity_id));
                continue;
            }
            let result = match replacement.backup_identity_id {
                Some(backup_id) => match self.load_warp_identity(backup_id).await {
                    Ok(previous) => self.persist_identity(*profile_id, &previous, None).await,
                    Err(error) => Err(error),
                },
                None => self
                    .vault
                    .delete_identity(*profile_id)
                    .await
                    .map_err(ControlServiceError::from),
            };
            match result {
                Ok(()) => completed.push((*profile_id, replacement.backup_identity_id)),
                Err(error) if first_error.is_none() => first_error = Some(error),
                Err(_) => {}
            }
        }

        if !completed.is_empty() {
            let mut next = self.config.read().await.clone();
            for (profile_id, backup_id) in completed {
                next.pending_identity_replacements.remove(&profile_id);
                if let Some(backup_id) = backup_id
                    && !next.pending_identity_local_deletions.contains(&backup_id)
                {
                    next.pending_identity_local_deletions.push(backup_id);
                }
            }
            self.persist(next).await?;
        }
        first_error.map_or(Ok(()), Err)
    }

    async fn reset_profile(&self, id: Uuid) -> Result<Profile, ControlServiceError> {
        let _mutation = self.mutation_lock.lock().await;
        self.migrate_shared_proxy_password().await?;
        let mut next = self.config.read().await.clone();
        if next.account(id).is_none() {
            return Err(ControlServiceError::ProfileNotFound(id));
        }
        next.network.reset_user_defaults();
        next.network.proxy.auth_username = None;
        self.vault
            .delete(SHARED_NETWORK_SECRET_ID, SecretRecord::ProxyPassword)
            .await?;
        let profile = next
            .runtime_profile(id)
            .ok_or(ControlServiceError::ProfileNotFound(id))?;
        self.persist(next).await?;
        Ok(profile)
    }

    async fn reap_pending_identity_deletions_locked(&self) -> Result<(), ControlServiceError> {
        let snapshot = self.config.read().await.clone();
        let pending = snapshot.pending_identity_deletions;
        let pending_local = snapshot.pending_identity_local_deletions;
        let pending_creations = snapshot.pending_identity_creations;
        if pending.is_empty() && pending_local.is_empty() && pending_creations.is_empty() {
            return Ok(());
        }

        let mut completed = std::collections::HashSet::new();
        let mut first_error = None;
        for profile_id in pending {
            if let Err(error) = self.cleanup_remote_license(profile_id).await {
                if first_error.is_none() {
                    first_error = Some(error);
                }
                continue;
            }
            match self.vault.delete_identity(profile_id).await {
                Ok(()) => {
                    completed.insert(profile_id);
                }
                Err(error) if first_error.is_none() => first_error = Some(error.into()),
                Err(_) => {}
            }
        }
        let mut completed_local = std::collections::HashSet::new();
        for profile_id in pending_local {
            match self.vault.delete_identity(profile_id).await {
                Ok(()) => {
                    completed_local.insert(profile_id);
                }
                Err(error) if first_error.is_none() => first_error = Some(error.into()),
                Err(_) => {}
            }
        }
        let mut completed_creations = std::collections::HashSet::new();
        for profile_id in pending_creations {
            if let Err(error) = self.cleanup_remote_license(profile_id).await {
                if first_error.is_none() {
                    first_error = Some(error);
                }
                continue;
            }
            match self.vault.delete_identity(profile_id).await {
                Ok(()) => {
                    completed_creations.insert(profile_id);
                }
                Err(error) if first_error.is_none() => first_error = Some(error.into()),
                Err(_) => {}
            }
        }
        if !completed.is_empty() || !completed_local.is_empty() || !completed_creations.is_empty() {
            let mut next = self.config.read().await.clone();
            next.pending_identity_deletions
                .retain(|profile_id| !completed.contains(profile_id));
            next.pending_identity_local_deletions
                .retain(|profile_id| !completed_local.contains(profile_id));
            next.pending_identity_creations
                .retain(|profile_id| !completed_creations.contains(profile_id));
            self.persist(next).await?;
        }
        first_error.map_or(Ok(()), Err)
    }

    async fn cleanup_remote_license(&self, profile_id: Uuid) -> Result<(), ControlServiceError> {
        match self.load_warp_identity(profile_id).await {
            Ok(identity) if should_unbind_remote_license(&identity) => {
                self.unbind_remote_consumer_license(profile_id, &identity)
                    .await
            }
            Ok(_) | Err(ControlServiceError::MissingCredential(_)) => Ok(()),
            Err(error) => Err(error),
        }
    }

    async fn unbind_remote_consumer_license(
        &self,
        profile_id: Uuid,
        identity: &WarpIdentity,
    ) -> Result<(), ControlServiceError> {
        #[cfg(test)]
        {
            let _ = identity;
            self.remote_license_unbinds.lock().await.push(profile_id);
            Ok(())
        }
        #[cfg(not(test))]
        {
            let _ = profile_id;
            ConsumerRegistrationClient::new()?
                .unbind_license(identity)
                .await?;
            Ok(())
        }
    }

    async fn persist(&self, next: AppConfig) -> Result<(), ControlServiceError> {
        next.validate()
            .map_err(ControlServiceError::profile_configuration)?;
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
    #[error("the requested feature is not available yet: {0}")]
    FeatureUnavailable(&'static str),
    #[error("the requested feature has been removed: {0}")]
    FeatureRemoved(&'static str),
    #[error("invalid profile configuration: {0}")]
    InvalidConfiguration(String),
    #[error("invalid direct DNS configuration: {message}")]
    InvalidDirectDnsConfiguration { code: &'static str, message: String },
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
    #[error("{message}")]
    PlatformRecovery { code: &'static str, message: String },
    #[error("Cloudflare terms must be accepted before identity provisioning")]
    TermsNotAccepted,
    #[error("the manually entered WARP Secret is not UTF-8")]
    InvalidManualSecretEncoding,
    #[error("the WARP License Key is not UTF-8")]
    InvalidLicenseEncoding,
    #[error(
        "changing a profile between Consumer WARP and Zero Trust organizations is not supported"
    )]
    IdentityProviderChangeUnsupported,
    #[error("this identity operation is not available for a Zero Trust profile")]
    IdentityOperationUnsupported,
    #[error(
        "Zero Trust registration completed remotely, but the local profile could not be committed; an organization administrator may need to remove the residual device"
    )]
    ZeroTrustLocalCommit,
    #[error("identity validation failed: {0}")]
    Identity(#[from] usque_core::IdentityError),
    #[error("Cloudflare device registration failed: {0}")]
    Registration(#[from] usque_core::RegistrationError),
    #[error("secure identity storage failed: {0}")]
    Vault(#[from] VaultError),
    #[error("configuration persistence failed: {0}")]
    Persistence(#[from] StoreError),
    #[error("configuration persistence worker failed: {0}")]
    PersistenceWorker(String),
    #[error("maintenance operation failed: {0}")]
    Maintenance(#[from] maintenance::MaintenanceError),
    #[error("diagnostics operation failed: {0}")]
    Diagnostics(#[from] diagnostics::DiagnosticsError),
    #[error("sensitive output operation failed: {0}")]
    SensitiveOutput(#[source] std::io::Error),
    #[error("sensitive output worker failed: {0}")]
    SensitiveOutputWorker(String),
    #[error("the previous disconnect cleanup failed: {0}")]
    DisconnectCleanup(String),
    #[error("proxy listener authentication is invalid: {0}")]
    InvalidProxyAuth(String),
    #[error("geo rules operation failed: {0}")]
    GeoRules(String),
}

impl ControlServiceError {
    pub(crate) fn configuration(error: impl std::fmt::Display) -> Self {
        Self::InvalidConfiguration(error.to_string())
    }

    fn profile_configuration(error: ConfigError) -> Self {
        match error.stable_code() {
            Some(code) => Self::InvalidDirectDnsConfiguration {
                code,
                message: error.to_string(),
            },
            None => Self::configuration(error),
        }
    }

    fn geo_rules(error: impl std::fmt::Display) -> Self {
        Self::GeoRules(error.to_string())
    }

    fn invalid_proxy_auth(error: impl std::fmt::Display) -> Self {
        Self::InvalidProxyAuth(error.to_string())
    }

    fn as_structured_error(&self) -> StructuredError {
        let (code, retryable) = match self {
            Self::InvalidRequest(_) | Self::InvalidConfiguration(_) => ("INVALID_ARGUMENT", false),
            Self::InvalidDirectDnsConfiguration { code, .. } => (*code, false),
            Self::InvalidProxyAuth(_) => ("CONFIGURATION_INVALID", false),
            Self::FeatureUnavailable(_) => ("FEATURE_UNAVAILABLE", false),
            Self::FeatureRemoved(_) => ("FEATURE_REMOVED", false),
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
                | TransportError::AllEndpointsFailed(_)
                | TransportError::AllTransportsFailed { .. },
            ) => ("ENDPOINT_UNREACHABLE", true),
            Self::Transport(TransportError::Dns(_)) => ("DNS_UNAVAILABLE", true),
            Self::Transport(
                TransportError::SocksListener { .. } | TransportError::HttpProxyListener { .. },
            ) => ("PROXY_LISTENER_FAILED", false),
            Self::Transport(_) => ("DATA_PLANE_FAILED", true),
            Self::PlatformVpn(_) => ("PLATFORM_VPN_FAILED", true),
            Self::PlatformRecovery { code, .. } => (*code, false),
            Self::TermsNotAccepted => ("TERMS_NOT_ACCEPTED", false),
            Self::InvalidManualSecretEncoding | Self::Identity(_) => ("INVALID_WARP_SECRET", false),
            Self::InvalidLicenseEncoding => ("INVALID_LICENSE_KEY", false),
            Self::IdentityProviderChangeUnsupported => {
                ("IDENTITY_PROVIDER_CHANGE_UNSUPPORTED", false)
            }
            Self::IdentityOperationUnsupported => ("IDENTITY_OPERATION_UNSUPPORTED", false),
            Self::ZeroTrustLocalCommit => ("ZERO_TRUST_LOCAL_COMMIT_FAILED", false),
            Self::Registration(RegistrationError::InvalidZeroTrustTeam) => {
                ("ZERO_TRUST_TEAM_INVALID", false)
            }
            Self::Registration(RegistrationError::InvalidZeroTrustCallback) => {
                ("ZERO_TRUST_CALLBACK_INVALID", false)
            }
            Self::Registration(RegistrationError::ZeroTrustLoginExpired) => {
                ("ZERO_TRUST_LOGIN_EXPIRED", false)
            }
            Self::Registration(RegistrationError::ZeroTrustLoginDenied) => {
                ("ZERO_TRUST_LOGIN_DENIED", false)
            }
            Self::Registration(RegistrationError::ZeroTrustContractChanged) => {
                ("ZERO_TRUST_CONTRACT_CHANGED", false)
            }
            Self::Registration(RegistrationError::ZeroTrustRegistrationFailed {
                status, ..
            }) if status.as_u16() == 401 => ("ZERO_TRUST_LOGIN_EXPIRED", false),
            Self::Registration(RegistrationError::ZeroTrustRegistrationFailed {
                status, ..
            }) => ("ZERO_TRUST_REGISTRATION_FAILED", status.is_server_error()),
            Self::Registration(RegistrationError::ZeroTrustNetwork { .. }) => {
                ("ZERO_TRUST_REGISTRATION_FAILED", true)
            }
            Self::Registration(_) => ("REGISTRATION_FAILED", true),
            Self::Vault(_) => ("SECURE_STORAGE_FAILED", false),
            Self::Persistence(_) | Self::PersistenceWorker(_) => ("PERSISTENCE_FAILED", true),
            Self::Maintenance(maintenance::MaintenanceError::Update(_)) => {
                ("UPDATE_CHECK_FAILED", true)
            }
            Self::Maintenance(_) => ("DIAGNOSTICS_EXPORT_FAILED", false),
            Self::Diagnostics(diagnostics::DiagnosticsError::AlreadyRunning) => {
                ("DIAGNOSTIC_ALREADY_RUNNING", false)
            }
            Self::Diagnostics(diagnostics::DiagnosticsError::NotStarted) => {
                ("DIAGNOSTICS_NOT_STARTED", false)
            }
            Self::Diagnostics(diagnostics::DiagnosticsError::SessionMismatch) => {
                ("DIAGNOSTIC_SESSION_MISMATCH", false)
            }
            Self::SensitiveOutput(_) | Self::SensitiveOutputWorker(_) => {
                ("SENSITIVE_OUTPUT_FAILED", false)
            }
            Self::DisconnectCleanup(_) => ("DISCONNECT_CLEANUP_FAILED", true),
            Self::GeoRules(_) => ("GEO_RULES_FAILED", true),
        };
        StructuredError {
            code: code.to_owned(),
            message: self.to_string(),
            retryable,
        }
    }
}

fn connection_error_wire_code(code: ErrorCode) -> String {
    match code {
        ErrorCode::WindowsRecoveryFailed => "WINDOWS_RECOVERY_FAILED".to_owned(),
        ErrorCode::WindowsRecoveryTimeout => "WINDOWS_RECOVERY_TIMEOUT".to_owned(),
        ErrorCode::WindowsRecoveryConflict => "WINDOWS_RECOVERY_CONFLICT".to_owned(),
        ErrorCode::WindowsRecoveryUnsupported => "WINDOWS_RECOVERY_UNSUPPORTED".to_owned(),
        // Preserve the pre-existing wire spelling for every other error.
        code => format!("{code:?}").to_ascii_uppercase(),
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
            | TransportError::AllEndpointsFailed(_)
            | TransportError::AllTransportsFailed { .. },
        ) => ErrorCode::EndpointUnreachable,
        ControlServiceError::Transport(TransportError::Dns(_)) => ErrorCode::DnsUnavailable,
        ControlServiceError::Transport(TransportError::Http3DatagramUnavailable)
        | ControlServiceError::OperatingModeUnavailable(_) => ErrorCode::TransportUnavailable,
        ControlServiceError::PlatformVpn(_) => ErrorCode::PlatformSetupFailed,
        ControlServiceError::PlatformRecovery { code, .. } => match *code {
            "WINDOWS_RECOVERY_TIMEOUT" => ErrorCode::WindowsRecoveryTimeout,
            "WINDOWS_RECOVERY_CONFLICT" => ErrorCode::WindowsRecoveryConflict,
            "WINDOWS_RECOVERY_UNSUPPORTED" => ErrorCode::WindowsRecoveryUnsupported,
            _ => ErrorCode::WindowsRecoveryFailed,
        },
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
pub(crate) fn map_windows_vpn_error(error: windows_agent::WindowsVpnError) -> ControlServiceError {
    let recovery_code = match &error {
        windows_agent::WindowsVpnError::RecoveryFailed
        | windows_agent::WindowsVpnError::RecoveryRequired { .. } => {
            Some("WINDOWS_RECOVERY_FAILED")
        }
        windows_agent::WindowsVpnError::RecoveryTimeout => Some("WINDOWS_RECOVERY_TIMEOUT"),
        windows_agent::WindowsVpnError::RecoveryConflict => Some("WINDOWS_RECOVERY_CONFLICT"),
        windows_agent::WindowsVpnError::RecoveryUnsupported => Some("WINDOWS_RECOVERY_UNSUPPORTED"),
        _ => None,
    };
    if let Some(code) = recovery_code {
        return ControlServiceError::PlatformRecovery {
            code,
            message: error.to_string(),
        };
    }
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

fn registration_options(device_name: String, locale: String) -> RegistrationOptions {
    RegistrationOptions {
        terms_accepted: true,
        model: desktop_registration_model().to_owned(),
        device_name: nonempty(device_name),
        locale: if locale.trim().is_empty() {
            "en_US".to_owned()
        } else {
            locale
        },
    }
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
    let mode = match source.mode {
        value if value == v1::OperatingMode::Unspecified as i32 => {
            OperatingMode::legacy_platform_default()
        }
        value if value == v1::OperatingMode::Vpn as i32 => OperatingMode::Vpn,
        value if value == v1::OperatingMode::Socks5 as i32 => OperatingMode::Socks5,
        value if value == v1::OperatingMode::HttpProxy as i32 => OperatingMode::HttpProxy,
        _ => {
            return Err(ControlServiceError::InvalidRequest(
                "unknown operating mode".to_owned(),
            ));
        }
    };
    let frontends = source
        .frontends
        .map(|frontends| FrontendSettings {
            tunnel: frontends.tunnel,
            socks5: frontends.socks5,
            http: frontends.http,
        })
        .unwrap_or_else(|| match mode {
            OperatingMode::Vpn => FrontendSettings {
                tunnel: true,
                socks5: false,
                http: false,
            },
            OperatingMode::Socks5 => FrontendSettings {
                tunnel: false,
                socks5: true,
                http: false,
            },
            OperatingMode::HttpProxy => FrontendSettings {
                tunnel: false,
                socks5: false,
                http: true,
            },
        });
    let mut direct_dns = match source.direct_dns {
        None => DirectDnsSettings::default(),
        Some(settings) => DirectDnsSettings {
            mode: match settings.mode {
                value if value == v1::DirectDnsMode::Unspecified as i32 => {
                    ConfigDirectDnsMode::PhysicalSystem
                }
                value if value == v1::DirectDnsMode::PhysicalSystem as i32 => {
                    ConfigDirectDnsMode::PhysicalSystem
                }
                value if value == v1::DirectDnsMode::Doh as i32 => ConfigDirectDnsMode::Doh,
                value if value == v1::DirectDnsMode::Dot as i32 => ConfigDirectDnsMode::Dot,
                _ => {
                    return Err(ControlServiceError::InvalidDirectDnsConfiguration {
                        code: "DIRECT_DNS_MODE_INVALID",
                        message: "unknown direct DNS mode".to_owned(),
                    });
                }
            },
            server_name: settings.server_name,
            doh_path: settings.doh_path,
            bootstrap_ips: settings
                .bootstrap_ips
                .iter()
                .map(|value| {
                    value.parse::<IpAddr>().map_err(|_| {
                        ControlServiceError::InvalidDirectDnsConfiguration {
                            code: "DIRECT_DNS_BOOTSTRAP_INVALID",
                            message: "direct DNS bootstrap IP is invalid".to_owned(),
                        }
                    })
                })
                .collect::<Result<Vec<_>, _>>()?,
            port: u16::try_from(settings.port).map_err(|_| {
                ControlServiceError::InvalidDirectDnsConfiguration {
                    code: "DIRECT_DNS_PORT_INVALID",
                    message: "direct DNS port exceeds 65535".to_owned(),
                }
            })?,
        },
    };
    direct_dns.canonicalize();

    let mut profile = Profile {
        id: parse_profile_id(&source.id)?,
        name: source.name,
        mode,
        frontends,
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
            dns_servers: if proxy.dns_servers.is_empty() {
                vec![
                    usque_core::config::DEFAULT_DNS_V4.into(),
                    usque_core::config::DEFAULT_DNS_V6.into(),
                ]
            } else {
                proxy
                    .dns_servers
                    .iter()
                    .map(|value| {
                        value
                            .parse::<IpAddr>()
                            .map_err(ControlServiceError::configuration)
                    })
                    .collect::<Result<Vec<_>, _>>()?
            },
            auth_username: if proxy.auth_username.is_empty() {
                None
            } else {
                Some(proxy.auth_username)
            },
            auth_password: None,
        },
        geo_direct_countries: source.geo_direct_countries,
        direct_dns,
    };
    profile.canonicalize_mode();
    profile
        .canonicalize_geo_direct()
        .map_err(ControlServiceError::configuration)?;
    profile
        .validate()
        .map_err(ControlServiceError::profile_configuration)?;
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

pub(crate) fn profile_to_proto(profile: &Profile) -> v1::Profile {
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
        frontends: Some(v1::FrontendSettings {
            tunnel: profile.frontends.tunnel,
            socks5: profile.frontends.socks5,
            http: profile.frontends.http,
        }),
        geo_direct_countries: profile.geo_direct_countries.clone(),
        direct_dns: Some(v1::DirectDnsSettings {
            mode: match profile.direct_dns.mode {
                ConfigDirectDnsMode::PhysicalSystem => v1::DirectDnsMode::PhysicalSystem as i32,
                ConfigDirectDnsMode::Doh => v1::DirectDnsMode::Doh as i32,
                ConfigDirectDnsMode::Dot => v1::DirectDnsMode::Dot as i32,
            },
            server_name: profile.direct_dns.server_name.clone(),
            doh_path: profile.direct_dns.doh_path.clone(),
            bootstrap_ips: profile
                .direct_dns
                .bootstrap_ips
                .iter()
                .map(ToString::to_string)
                .collect(),
            port: u32::from(profile.direct_dns.port),
        }),
    }
}

fn load_geo_direct_policy(profile: &Profile, cache_dir: &std::path::Path) -> GeoDirectPolicy {
    if profile.geo_direct_countries.is_empty() {
        return GeoDirectPolicy::disabled();
    }
    let countries = match profile
        .geo_direct_countries
        .iter()
        .map(|country| CountryCode::parse(country))
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(countries) => countries,
        Err(error) => {
            tracing::warn!(%error, "invalid GEO direct policy; using tunnel-only routing");
            return GeoDirectPolicy::disabled();
        }
    };
    match GeoDirectPolicy::load(cache_dir, countries) {
        Ok(policy) => policy,
        Err(error) => {
            tracing::warn!(%error, "GEO rule cache could not be loaded; using tunnel-only routing");
            GeoDirectPolicy::disabled()
        }
    }
}

fn geo_results_to_proto(results: Vec<usque_core::GeoRulesUpdate>) -> v1::GeoRulesUpdateResults {
    v1::GeoRulesUpdateResults {
        results: results
            .into_iter()
            .map(|result| {
                let (status, reason) = match result.status {
                    UpdateStatus::UpToDate => (v1::GeoRulesUpdateStatus::UpToDate, String::new()),
                    UpdateStatus::Updated => (v1::GeoRulesUpdateStatus::Updated, String::new()),
                    UpdateStatus::Failed { reason } => (v1::GeoRulesUpdateStatus::Failed, reason),
                };
                v1::GeoRulesUpdateResult {
                    country_code: result.country_code,
                    status: status as i32,
                    reason,
                    artifact_kind: result.artifact_kind,
                    artifact_scope: result.artifact_scope,
                }
            })
            .collect(),
    }
}

fn profile_list_to_proto(config: &AppConfig) -> v1::ProfileList {
    v1::ProfileList {
        profiles: config
            .runtime_profiles()
            .iter()
            .map(profile_to_proto)
            .collect(),
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
        dns_servers: proxy.dns_servers.iter().map(ToString::to_string).collect(),
        auth_username: proxy
            .listener_auth_username()
            .unwrap_or_default()
            .to_owned(),
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
        composable_frontends: true,
        hot_reconfigure: true,
        windows_tray: cfg!(windows),
        android_quick_settings_tile: cfg!(target_os = "android"),
        system_start: cfg!(any(windows, target_os = "android")),
        license_management: true,
        warp_secret_export: true,
        diagnostics_sessions: true,
        connection_timeline: true,
        deep_diagnostics: true,
        network_quality: usque_transport::PRODUCTION_NETWORK_FEATURES.network_quality_metrics,
        encrypted_direct_dns: usque_transport::ENCRYPTED_DIRECT_DNS_ENABLED,
        quic_migration: usque_transport::PRODUCTION_NETWORK_FEATURES.quic_migration,
        automatic_pmtu: usque_transport::PRODUCTION_NETWORK_FEATURES.automatic_pmtu,
    }
}

pub(crate) fn snapshot_to_proto(snapshot: &ConnectionSnapshot) -> v1::ConnectionSnapshot {
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
            code: connection_error_wire_code(error.code),
            message: error.message.clone(),
            retryable: error.retryable,
        }),
        kill_switch_state: match snapshot.kill_switch_state {
            KillSwitchState::NotApplicable => v1::KillSwitchState::NotApplicable as i32,
            KillSwitchState::Inactive => v1::KillSwitchState::Inactive as i32,
            KillSwitchState::Active => v1::KillSwitchState::Active as i32,
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
        frontends: snapshot
            .frontends
            .iter()
            .map(frontend_status_to_proto)
            .collect(),
        failure: snapshot.failure.as_ref().map(transport_failure_to_proto),
        network_quality: None,
    }
}

pub(crate) fn transport_failure_to_proto(failure: &TransportFailure) -> v1::TransportFailure {
    v1::TransportFailure {
        code: failure.code.as_str().to_owned(),
        stage: failure.stage.as_str().to_owned(),
        transport: failure
            .transport
            .map(|transport| match transport {
                Transport::Http3 => "h3",
                Transport::Http2 => "h2",
            })
            .unwrap_or_default()
            .to_owned(),
        address_family: failure
            .address_family
            .map(|family| match family {
                AddressFamily::Ipv4 => "ipv4",
                AddressFamily::Ipv6 => "ipv6",
            })
            .unwrap_or_default()
            .to_owned(),
        retryable: failure.retryable,
        fallback_allowed: failure.fallback_allowed,
        severity: match failure.severity {
            usque_core::FailureSeverity::Info => v1::FailureSeverity::Info as i32,
            usque_core::FailureSeverity::Warning => v1::FailureSeverity::Warning as i32,
            usque_core::FailureSeverity::Error => v1::FailureSeverity::Error as i32,
            usque_core::FailureSeverity::Critical => v1::FailureSeverity::Critical as i32,
        },
        remediation_key: failure.remediation_key.clone(),
        sanitized_detail: failure.sanitized_detail.clone().unwrap_or_default(),
    }
}

fn frontend_status_to_proto(status: &FrontendStatus) -> v1::FrontendStatus {
    v1::FrontendStatus {
        kind: match status.kind {
            FrontendKind::Tunnel => v1::FrontendKind::Tunnel as i32,
            FrontendKind::Socks5 => v1::FrontendKind::Socks5 as i32,
            FrontendKind::Http => v1::FrontendKind::Http as i32,
            FrontendKind::SystemProxy => v1::FrontendKind::SystemProxy as i32,
        },
        phase: match status.phase {
            FrontendPhase::Disabled => v1::FrontendPhase::Disabled as i32,
            FrontendPhase::Preparing => v1::FrontendPhase::Preparing as i32,
            FrontendPhase::Active => v1::FrontendPhase::Active as i32,
            FrontendPhase::Degraded => v1::FrontendPhase::Degraded as i32,
            FrontendPhase::Reconnecting => v1::FrontendPhase::Reconnecting as i32,
            FrontendPhase::Error => v1::FrontendPhase::Error as i32,
        },
        listeners: status.listeners.clone(),
        error: status.error.as_ref().map(|error| StructuredError {
            code: connection_error_wire_code(error.code),
            message: error.message.clone(),
            retryable: error.retryable,
        }),
    }
}

fn exit_probe_for_session(
    profile: &Profile,
    runtime: &ActiveRuntime,
    store_path: &std::path::Path,
    listener_auth: Option<&ProxyAuthCredentials>,
) -> Option<IpSbProbe> {
    let flag_cache = store_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("cache")
        .join("flag-icons-7.5.0");
    let loopback = runtime
        .listeners()
        .iter()
        .copied()
        .find(|address| address.ip().is_loopback());
    if profile.frontends.socks5 {
        loopback
            .and_then(|listener| IpSbProbe::through_socks_with_auth(listener, listener_auth).ok())
            .map(|probe| probe.with_flag_cache(&flag_cache))
    } else if profile.frontends.http {
        loopback
            .and_then(|listener| IpSbProbe::through_http_with_auth(listener, listener_auth).ok())
            .map(|probe| probe.with_flag_cache(&flag_cache))
    } else if profile.frontends.tunnel {
        IpSbProbe::new()
            .ok()
            .map(|probe| probe.with_flag_cache(&flag_cache))
    } else {
        None
    }
}

async fn apply_exit_info(
    state: &Mutex<StateMachine>,
    data_plane: &Mutex<Option<ActiveDataPlane>>,
    profile_id: Uuid,
    session_generation: u64,
    exit: ExitInfo,
) {
    let data_plane = data_plane.lock().await;
    let Some(active) = data_plane.as_ref() else {
        return;
    };
    if active.profile_id != profile_id || active.session_generation != session_generation {
        return;
    }
    let mut state = state.lock().await;
    if !matches!(
        state.snapshot().phase,
        ConnectionPhase::Connected | ConnectionPhase::Degraded | ConnectionPhase::Reconnecting
    ) {
        return;
    }
    state.set_exit_info(exit);
}

fn exit_to_proto(exit: &ExitInfo) -> v1::ExitInfo {
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

    #[tokio::test]
    async fn closing_or_replacing_quality_sources_cannot_leave_a_stale_connection() {
        use usque_transport::{NetworkQualitySampler, NetworkQualityTelemetry};

        let directory = tempfile::tempdir().unwrap();
        let service = ControlService::open_with_vault(
            ConfigStore::new(directory.path().join("config.json")),
            Arc::new(MemoryVault::default()),
        )
        .unwrap();
        let live = || {
            let quality = NetworkQualityTelemetry::default();
            quality.begin_connection(Transport::Http2, AddressFamily::Ipv4);
            NetworkQualitySampler::new(quality).sample()
        };
        let (old_sender, old_source) = watch::channel(live());
        service.install_network_quality_source(old_source).await;
        let next = live();
        let expected = next.connection_id;
        let (sender, source) = watch::channel(next);
        service.install_network_quality_source(source).await;
        drop(old_sender);
        tokio::task::yield_now().await;
        assert_eq!(service.network_quality_snapshot().connection_id, expected);

        let mut updates = service.subscribe_network_quality();
        drop(sender);
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            while updates.borrow_and_update().connection_id.is_some() {
                updates.changed().await.unwrap();
            }
        })
        .await
        .unwrap();
        assert!(service.network_quality_snapshot().connection_id.is_none());

        let (_sender, source) = watch::channel(live());
        service.install_network_quality_source(source).await;
        service.clear_network_quality_source().await;
        assert!(service.network_quality_snapshot().connection_id.is_none());
    }

    #[tokio::test]
    async fn standard_doctor_preserves_configuration_runtime_and_generations() {
        let directory = tempfile::tempdir().unwrap();
        let service = ControlService::open_with_vault(
            ConfigStore::new(directory.path().join("config.json")),
            Arc::new(MemoryVault::default()),
        )
        .unwrap();
        let profile = service.config_snapshot().await.active_profile().unwrap();
        service
            .install_test_session(profile, false, 0)
            .await
            .unwrap();
        let before_config = serde_json::to_value(service.config_snapshot().await).unwrap();
        let before_state = service.state.lock().await.snapshot().clone();
        let generation = service.session_generation.load(Ordering::Relaxed);
        let start = tokio::time::Instant::now();
        let context = service
            .diagnostic_context(usque_core::DiagnosticMode::Standard)
            .await;
        assert!(context.probes.is_none());
        assert!(context.platform_state.is_none());
        service
            .diagnostics
            .start(usque_core::DiagnosticMode::Standard, context)
            .await
            .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while service.diagnostics.get().await.unwrap().state.is_active() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert!(start.elapsed() < std::time::Duration::from_secs(2));
        assert_eq!(
            serde_json::to_value(service.config_snapshot().await).unwrap(),
            before_config
        );
        assert_eq!(*service.state.lock().await.snapshot(), before_state);
        assert_eq!(
            service.session_generation.load(Ordering::Relaxed),
            generation
        );
        assert!(service.data_plane.lock().await.is_some());
    }

    #[test]
    fn runtime_path_updates_are_edge_triggered() {
        let mut state = StateMachine::default();
        state
            .transition(ConnectionPhase::Preparing)
            .expect("prepare");
        state
            .transition(ConnectionPhase::ConnectingHttp3)
            .expect("connect H3");
        state
            .mark_connected(Transport::Http3, AddressFamily::Ipv4, true, true)
            .expect("connected");

        let unchanged = RuntimePath {
            transport: Transport::Http3,
            endpoint_family: AddressFamily::Ipv4,
            ipv4_available: true,
            ipv6_available: true,
        };
        assert!(!runtime_path_changed(state.snapshot(), unchanged));

        let peer_withdrew_ipv6 = RuntimePath {
            ipv6_available: false,
            ..unchanged
        };
        assert!(runtime_path_changed(state.snapshot(), peer_withdrew_ipv6));
    }

    #[tokio::test]
    async fn late_exit_probe_applies_only_while_the_same_session_is_up() {
        let directory = tempfile::tempdir().unwrap();
        let service = ControlService::open_with_vault(
            ConfigStore::new(directory.path().join("config.json")),
            Arc::new(MemoryVault::default()),
        )
        .unwrap();
        let profile = service.config_snapshot().await.active_profile().unwrap();
        service
            .install_test_session(profile.clone(), false, 0)
            .await
            .expect("install harness");
        let generation = service
            .data_plane
            .lock()
            .await
            .as_ref()
            .unwrap()
            .session_generation;

        let exit = ExitInfo {
            ipv4: Some(IpAddr::V4(Ipv4Addr::new(1, 2, 3, 4))),
            ipv6: None,
            ipv4_location: None,
            ipv6_location: None,
            checked_at: chrono::Utc::now(),
        };
        apply_exit_info(
            &service.state,
            &service.data_plane,
            profile.id,
            generation,
            exit.clone(),
        )
        .await;
        assert_eq!(
            service.state.lock().await.snapshot().exit.as_ref(),
            Some(&exit)
        );

        service.disconnect_locked().await.expect("disconnect");
        apply_exit_info(
            &service.state,
            &service.data_plane,
            profile.id,
            generation,
            exit,
        )
        .await;
        assert!(service.state.lock().await.snapshot().exit.is_none());
    }

    #[tokio::test]
    async fn late_exit_probe_from_a_prior_session_does_not_apply_after_reconnect() {
        let directory = tempfile::tempdir().unwrap();
        let service = ControlService::open_with_vault(
            ConfigStore::new(directory.path().join("config.json")),
            Arc::new(MemoryVault::default()),
        )
        .unwrap();
        let profile = service.config_snapshot().await.active_profile().unwrap();
        service
            .install_test_session(profile.clone(), false, 0)
            .await
            .expect("session A");
        let generation_a = service
            .data_plane
            .lock()
            .await
            .as_ref()
            .unwrap()
            .session_generation;

        service.disconnect_locked().await.expect("disconnect");
        service
            .install_test_session(profile.clone(), false, 0)
            .await
            .expect("session B");
        let generation_b = service
            .data_plane
            .lock()
            .await
            .as_ref()
            .unwrap()
            .session_generation;
        assert_ne!(generation_a, generation_b);
        assert_eq!(
            service.data_plane.lock().await.as_ref().unwrap().profile_id,
            profile.id
        );
        assert!(service.state.lock().await.snapshot().exit.is_none());

        let stale = ExitInfo {
            ipv4: Some(IpAddr::V4(Ipv4Addr::new(1, 2, 3, 4))),
            ipv6: None,
            ipv4_location: None,
            ipv6_location: None,
            checked_at: chrono::Utc::now(),
        };
        apply_exit_info(
            &service.state,
            &service.data_plane,
            profile.id,
            generation_a,
            stale,
        )
        .await;
        assert!(service.state.lock().await.snapshot().exit.is_none());

        let current = ExitInfo {
            ipv4: Some(IpAddr::V4(Ipv4Addr::new(5, 6, 7, 8))),
            ipv6: None,
            ipv4_location: None,
            ipv6_location: None,
            checked_at: chrono::Utc::now(),
        };
        apply_exit_info(
            &service.state,
            &service.data_plane,
            profile.id,
            generation_b,
            current.clone(),
        )
        .await;
        assert_eq!(
            service.state.lock().await.snapshot().exit.as_ref(),
            Some(&current)
        );
    }

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

    fn test_identity(provider: IdentityProvider, license: Option<&str>) -> WarpIdentity {
        let entitlement = match provider {
            IdentityProvider::ZeroTrust { .. } => None,
            IdentityProvider::Consumer if license.is_some() => Some(ConsumerEntitlement::WarpPlus),
            IdentityProvider::Consumer => Some(ConsumerEntitlement::Free),
        };
        test_identity_with_entitlement(provider, license, entitlement)
    }

    fn test_identity_with_entitlement(
        provider: IdentityProvider,
        license: Option<&str>,
        entitlement: Option<ConsumerEntitlement>,
    ) -> WarpIdentity {
        let endpoint_key = MasqueKeyPair::generate();
        WarpIdentity::from_secure_records(
            MasqueKeyPair::generate(),
            EndpointPin::from_spki_der(&endpoint_key.public_spki_der().unwrap()).unwrap(),
            format!("device-{}", Uuid::new_v4()),
            format!("token-{}", Uuid::new_v4()),
            license.map(ToOwned::to_owned),
            provider,
            entitlement,
            "172.16.0.2".parse().unwrap(),
            "2606:4700:110:8f13::2".parse().unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn consumer_entitlement_never_treats_a_missing_flag_as_plus() {
        assert_eq!(
            consumer_license_presentation(ConsumerEntitlement::WarpPlus),
            (v1::LicenseState::WarpPlus, "WARP+".to_owned())
        );
        assert_eq!(
            consumer_license_presentation(ConsumerEntitlement::Free),
            (v1::LicenseState::Free, "Free".to_owned())
        );
    }

    #[tokio::test]
    async fn profile_catalog_uses_derived_entitlement_not_a_stored_sharing_license() {
        let directory = tempfile::tempdir().expect("tempdir");
        let vault = Arc::new(MemoryVault::default());
        let service = ControlService::open_with_vault(
            ConfigStore::new(directory.path().join("config.json")),
            Arc::clone(&vault) as Arc<dyn SecretVault>,
        )
        .expect("service");
        let profile_id = service.config_snapshot().await.profiles[0].id;

        let free = test_identity_with_entitlement(
            IdentityProvider::Consumer,
            None,
            Some(ConsumerEntitlement::Free),
        );
        service
            .persist_identity(profile_id, &free, None)
            .await
            .unwrap();
        let catalog = service.profile_catalog().await;
        let status = catalog.identity_statuses.first().unwrap();
        assert_eq!(status.license_state, v1::LicenseState::Free as i32);
        assert_eq!(status.account_type, "Free");
        assert!(
            vault
                .get(profile_id, SecretRecord::License)
                .await
                .unwrap()
                .is_none()
        );

        let plus = test_identity(IdentityProvider::Consumer, Some("bound-license"));
        service
            .persist_identity(profile_id, &plus, None)
            .await
            .unwrap();
        let catalog = service.profile_catalog().await;
        let status = catalog.identity_statuses.first().unwrap();
        assert_eq!(status.license_state, v1::LicenseState::WarpPlus as i32);
        assert_eq!(status.account_type, "WARP+");

        let free_with_key = test_identity_with_entitlement(
            IdentityProvider::Consumer,
            Some("free-sharing-key"),
            Some(ConsumerEntitlement::Free),
        );
        service
            .persist_identity(profile_id, &free_with_key, None)
            .await
            .unwrap();
        let catalog = service.profile_catalog().await;
        let status = catalog.identity_statuses.first().unwrap();
        assert_eq!(status.license_state, v1::LicenseState::Free as i32);
        assert_eq!(status.account_type, "Free");

        let legacy = test_identity_with_entitlement(
            IdentityProvider::Consumer,
            Some("legacy-api-license"),
            None,
        );
        service
            .persist_identity(profile_id, &legacy, None)
            .await
            .unwrap();
        vault
            .put(
                profile_id,
                SecretRecord::IdentityMetadata,
                br#"{"version":1,"provider":"consumer","warp_plus":true}"#,
            )
            .await
            .unwrap();
        let loaded = service.load_warp_identity(profile_id).await.unwrap();
        assert_eq!(loaded.entitlement(), None);
        assert!(loaded.license().is_some());
        assert_eq!(
            consumer_license_presentation(
                loaded.entitlement().unwrap_or(ConsumerEntitlement::Free)
            ),
            (v1::LicenseState::Free, "Free".to_owned())
        );
    }

    #[tokio::test]
    async fn profile_catalog_reports_unknown_without_writing_missing_entitlement() {
        let directory = tempfile::tempdir().expect("tempdir");
        let vault = Arc::new(MemoryVault::default());
        let service = ControlService::open_with_vault(
            ConfigStore::new(directory.path().join("config.json")),
            Arc::clone(&vault) as Arc<dyn SecretVault>,
        )
        .expect("service");
        let profile_id = service.config_snapshot().await.profiles[0].id;
        let legacy = test_identity_with_entitlement(
            IdentityProvider::Consumer,
            Some("legacy-api-license"),
            None,
        );
        service
            .persist_identity(profile_id, &legacy, None)
            .await
            .unwrap();
        let metadata_before = vault
            .get(profile_id, SecretRecord::IdentityMetadata)
            .await
            .unwrap()
            .expect("metadata");
        let license_before = vault
            .get(profile_id, SecretRecord::License)
            .await
            .unwrap()
            .expect("license");

        let catalog = service.profile_catalog().await;
        let status = catalog.identity_statuses.first().unwrap();
        assert_eq!(status.state, v1::ProfileIdentityState::Ready as i32);
        assert_eq!(status.license_state, v1::LicenseState::Unknown as i32);
        assert!(status.account_type.is_empty());

        let metadata_after = vault
            .get(profile_id, SecretRecord::IdentityMetadata)
            .await
            .unwrap()
            .expect("metadata");
        let license_after = vault
            .get(profile_id, SecretRecord::License)
            .await
            .unwrap()
            .expect("license");
        assert_eq!(metadata_before.as_slice(), metadata_after.as_slice());
        assert_eq!(license_before.as_slice(), license_after.as_slice());
        assert_eq!(
            service
                .load_warp_identity(profile_id)
                .await
                .unwrap()
                .entitlement(),
            None
        );
    }

    #[test]
    fn legacy_license_unbinds_unless_entitlement_is_explicitly_free() {
        assert!(should_unbind_remote_license(
            &test_identity_with_entitlement(
                IdentityProvider::Consumer,
                Some("legacy-api-license"),
                None,
            )
        ));
        assert!(should_unbind_remote_license(&test_identity(
            IdentityProvider::Consumer,
            Some("bound-license"),
        )));
        assert!(!should_unbind_remote_license(
            &test_identity_with_entitlement(
                IdentityProvider::Consumer,
                Some("free-sharing-key"),
                Some(ConsumerEntitlement::Free),
            )
        ));
        assert!(!should_unbind_remote_license(
            &test_identity_with_entitlement(IdentityProvider::Consumer, None, None,)
        ));
        assert!(!should_unbind_remote_license(&test_identity(
            IdentityProvider::zero_trust("example-team").unwrap(),
            None,
        )));
    }

    #[tokio::test]
    async fn delete_unbinds_legacy_consumer_license_without_persisted_entitlement() {
        let directory = tempfile::tempdir().expect("tempdir");
        let vault = Arc::new(MemoryVault::default());
        let service = ControlService::open_with_vault(
            ConfigStore::new(directory.path().join("config.json")),
            Arc::clone(&vault) as Arc<dyn SecretVault>,
        )
        .expect("service");
        let original = service.config_snapshot().await.profiles[0].id;
        let extra = Profile {
            id: Uuid::new_v4(),
            name: "Keep".to_owned(),
            ..Profile::default()
        };
        service.upsert_profile(extra.clone()).await.unwrap();

        let legacy = test_identity_with_entitlement(
            IdentityProvider::Consumer,
            Some("legacy-api-license"),
            None,
        );
        service
            .persist_identity(original, &legacy, None)
            .await
            .unwrap();
        let loaded = service.load_warp_identity(original).await.unwrap();
        assert_eq!(loaded.entitlement(), None);
        assert!(loaded.license().is_some());
        assert!(should_unbind_remote_license(&loaded));

        service.delete_profile(original).await.unwrap();
        assert_eq!(*service.remote_license_unbinds.lock().await, vec![original]);
        assert!(
            vault
                .get(original, SecretRecord::License)
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            !service
                .config_snapshot()
                .await
                .pending_identity_deletions
                .contains(&original)
        );

        let free_with_key = test_identity_with_entitlement(
            IdentityProvider::Consumer,
            Some("free-sharing-key"),
            Some(ConsumerEntitlement::Free),
        );
        service
            .persist_identity(extra.id, &free_with_key, None)
            .await
            .unwrap();
        let spare = Profile {
            id: Uuid::new_v4(),
            name: "Spare".to_owned(),
            ..Profile::default()
        };
        service.upsert_profile(spare).await.unwrap();
        service.delete_profile(extra.id).await.unwrap();
        assert_eq!(*service.remote_license_unbinds.lock().await, vec![original]);
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
        assert_eq!(persisted.runtime_profiles(), vec![added]);
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
    async fn legacy_zero_trust_import_keeps_ips_with_shared_port_and_sni() {
        let directory = tempfile::tempdir().expect("tempdir");
        let service = ControlService::open(ConfigStore::new(directory.path().join("config.json")))
            .expect("service");
        let imported = Profile {
            id: Uuid::new_v4(),
            name: "Work".to_owned(),
            endpoint: EndpointSettings {
                ipv4: "162.159.197.2".parse().unwrap(),
                ipv6: "2606:4700:102::2".parse().unwrap(),
                port: 443,
                sni: usque_core::ZERO_TRUST_SNI.to_owned(),
            },
            ..Profile::default()
        };

        let response = service
            .handle(request(
                "import-zero-trust",
                control_request::Payload::ImportLegacyProfiles(v1::ImportLegacyProfilesRequest {
                    profiles: vec![profile_to_proto(&imported)],
                    active_profile_id: imported.id.to_string(),
                }),
            ))
            .await;

        assert!(response.error.is_none(), "{:?}", response.error);
        let config = service.config_snapshot().await;
        assert_eq!(config.network.endpoint, EndpointSettings::default());
        let runtime = config.runtime_profile(imported.id).unwrap();
        assert_eq!(runtime.endpoint.ipv4, imported.endpoint.ipv4);
        assert_eq!(runtime.endpoint.ipv6, imported.endpoint.ipv6);
        assert_eq!(runtime.endpoint.port, EndpointSettings::default().port);
        assert_eq!(runtime.endpoint.sni, EndpointSettings::default().sni);
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
        assert!(capabilities.hot_reconfigure);
        assert_eq!(capabilities.transports, ["h3", "h2"]);
        assert!(!capabilities.architecture.is_empty());
        assert!(capabilities.network_quality);
        assert!(capabilities.encrypted_direct_dns);
        assert!(capabilities.quic_migration);
        assert!(capabilities.automatic_pmtu);
    }

    #[tokio::test]
    async fn network_quality_request_and_status_are_available_while_disconnected() {
        let directory = tempfile::tempdir().unwrap();
        let service = ControlService::open_with_vault(
            ConfigStore::new(directory.path().join("config.json")),
            Arc::new(MemoryVault::default()),
        )
        .unwrap();

        let response = service
            .handle(request(
                "quality",
                control_request::Payload::GetNetworkQuality(v1::GetNetworkQualityRequest {}),
            ))
            .await;
        let Some(control_response::Payload::NetworkQuality(quality)) = response.payload else {
            panic!("missing network quality response");
        };
        assert_eq!(quality.level, v1::NetworkQualityLevel::Disconnected as i32);
        assert!(quality.connection_instance_id.is_empty());

        let status = service
            .handle(request(
                "status",
                control_request::Payload::GetStatus(v1::GetStatusRequest {}),
            ))
            .await;
        let Some(control_response::Payload::Status(status)) = status.payload else {
            panic!("missing status response");
        };
        assert_eq!(
            status.network_quality.as_ref().map(|quality| quality.level),
            Some(v1::NetworkQualityLevel::Disconnected as i32)
        );
    }

    #[tokio::test]
    async fn retry_and_clear_all_data_are_real_control_operations() {
        let directory = tempfile::tempdir().unwrap();
        let config_path = directory.path().join("config.json");
        let vault = Arc::new(MemoryVault::default());
        let service =
            ControlService::open_with_vault(ConfigStore::new(&config_path), vault.clone()).unwrap();

        let mut retry_profile = service
            .config_snapshot()
            .await
            .active_profile()
            .expect("active profile");
        retry_profile.mode = OperatingMode::Socks5;
        retry_profile.frontends = FrontendSettings {
            tunnel: false,
            socks5: true,
            http: true,
        };
        service
            .upsert_profile(retry_profile)
            .await
            .expect("save proxy-only retry profile");

        let retry = service
            .handle(request(
                "retry",
                control_request::Payload::Retry(v1::RetryRequest {}),
            ))
            .await;
        assert_eq!(
            retry.error.as_ref().map(|error| error.code.as_str()),
            Some("MISSING_CREDENTIAL")
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
    async fn reset_clears_proxy_username_and_vault_password_together() {
        let directory = tempfile::tempdir().expect("tempdir");
        let vault = Arc::new(MemoryVault::default());
        let service = ControlService::open_with_vault(
            ConfigStore::new(directory.path().join("config.json")),
            Arc::clone(&vault) as Arc<dyn SecretVault>,
        )
        .expect("service");
        let profile_id = service.config_snapshot().await.profiles[0].id;

        let stored = service
            .handle(request(
                "auth-set",
                control_request::Payload::UpdateProxyAuth(v1::UpdateProxyAuthRequest {
                    profile_id: profile_id.to_string(),
                    username: "lan-user".to_owned(),
                    password: b"s3cret".to_vec(),
                    confirmed: true,
                }),
            ))
            .await;
        assert!(stored.error.is_none(), "{stored:?}");
        assert_eq!(
            service
                .config_snapshot()
                .await
                .network
                .proxy
                .auth_username
                .as_deref(),
            Some("lan-user")
        );

        service.reset_profile(profile_id).await.unwrap();
        assert!(
            service
                .config_snapshot()
                .await
                .network
                .proxy
                .auth_username
                .is_none()
        );
        assert!(
            vault
                .get(SHARED_NETWORK_SECRET_ID, SecretRecord::ProxyPassword)
                .await
                .unwrap()
                .is_none()
        );

        let mut proxy_only = service.config_snapshot().await.active_profile().unwrap();
        proxy_only.mode = OperatingMode::Socks5;
        proxy_only.frontends = FrontendSettings {
            tunnel: false,
            socks5: true,
            http: true,
        };
        service
            .upsert_profile(proxy_only)
            .await
            .expect("save proxy-only profile after reset");

        let connected = service
            .handle(request(
                "connect",
                control_request::Payload::Connect(v1::ConnectRequest {
                    profile_id: profile_id.to_string(),
                }),
            ))
            .await;
        assert_ne!(
            connected.error.as_ref().map(|error| error.code.as_str()),
            Some("CONFIGURATION_INVALID"),
            "{connected:?}"
        );
        assert!(
            !connected.error.as_ref().is_some_and(|error| {
                error
                    .message
                    .contains(&ConfigError::ProxyAuthRequiresPassword.to_string())
            }),
            "{connected:?}"
        );
        assert_eq!(
            connected.error.as_ref().map(|error| error.code.as_str()),
            Some("MISSING_CREDENTIAL")
        );
    }

    #[tokio::test]
    async fn reset_restores_network_defaults_without_removing_profile_identity() {
        let directory = tempfile::tempdir().expect("tempdir");
        let service = ControlService::open_with_vault(
            ConfigStore::new(directory.path().join("config.json")),
            Arc::new(MemoryVault::default()),
        )
        .expect("service");
        let mut profile = service.config_snapshot().await.active_profile().unwrap();
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

        let reset = service.config_snapshot().await.active_profile().unwrap();
        assert_eq!(reset.id, id);
        assert_eq!(reset.name, "Keep this name");
        assert_eq!(reset.endpoint, EndpointSettings::default());
        assert_eq!(reset.mtu, 1280);
        assert_eq!(service.config_snapshot().await.network.mtu, 1280);
    }

    #[tokio::test]
    async fn upsert_applies_network_settings_to_every_account() {
        let directory = tempfile::tempdir().expect("tempdir");
        let service = ControlService::open_with_vault(
            ConfigStore::new(directory.path().join("config.json")),
            Arc::new(MemoryVault::default()),
        )
        .expect("service");
        let mut extra = service.config_snapshot().await.active_profile().unwrap();
        extra.id = Uuid::new_v4();
        extra.name = "Work".to_owned();
        extra.mtu = 1500;
        extra.endpoint.sni = "ignored.example.com".to_owned();
        let stored = service.upsert_profile(extra).await.expect("insert");
        assert_eq!(stored.mtu, 1280);
        assert_eq!(stored.endpoint.sni, EndpointSettings::default().sni);

        let mut home = service.config_snapshot().await.active_profile().unwrap();
        home.mtu = 1400;
        home.auto_connect = true;
        service.upsert_profile(home).await.expect("update");

        let config = service.config_snapshot().await;
        assert_eq!(config.network.mtu, 1400);
        assert!(config.network.auto_connect);
        assert!(
            config
                .runtime_profiles()
                .iter()
                .all(|profile| profile.mtu == 1400 && profile.auto_connect)
        );
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn connect_fails_closed_for_unavailable_vpn_mode() {
        let directory = tempfile::tempdir().expect("tempdir");
        let service = ControlService::open_with_vault(
            ConfigStore::new(directory.path().join("config.json")),
            Arc::new(MemoryVault::default()),
        )
        .expect("service");
        let mut profile = service.config_snapshot().await.active_profile().unwrap();
        profile.frontends = FrontendSettings {
            tunnel: true,
            socks5: false,
            http: false,
        };
        let profile_id = profile.id;
        service.upsert_profile(profile).await.expect("upsert");

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
        let profile = service.config_snapshot().await.active_profile().unwrap();

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

    #[test]
    fn direct_dns_profile_proto_is_canonical_and_missing_is_backward_compatible() {
        let profile = Profile {
            direct_dns: DirectDnsSettings {
                mode: ConfigDirectDnsMode::Doh,
                server_name: "dns.example.com".to_owned(),
                doh_path: "/dns-query".to_owned(),
                bootstrap_ips: vec!["192.0.2.53".parse().unwrap()],
                port: 443,
            },
            ..Profile::default()
        };
        let decoded = profile_from_proto(profile_to_proto(&profile)).unwrap();
        assert_eq!(decoded.direct_dns, profile.direct_dns);

        let mut legacy = profile_to_proto(&Profile::default());
        legacy.direct_dns = None;
        assert_eq!(
            profile_from_proto(legacy).unwrap().direct_dns,
            DirectDnsSettings::default()
        );
    }

    #[test]
    fn invalid_direct_dns_proto_returns_stable_validation_codes() {
        let mut unknown = profile_to_proto(&Profile::default());
        unknown.direct_dns = Some(v1::DirectDnsSettings {
            mode: 99,
            ..v1::DirectDnsSettings::default()
        });
        let error = profile_from_proto(unknown).unwrap_err();
        assert_eq!(error.as_structured_error().code, "DIRECT_DNS_MODE_INVALID");

        let mut dot = profile_to_proto(&Profile::default());
        dot.direct_dns = Some(v1::DirectDnsSettings {
            mode: v1::DirectDnsMode::Dot as i32,
            server_name: "dns.example.com".to_owned(),
            doh_path: "/dns-query".to_owned(),
            bootstrap_ips: vec!["192.0.2.53".to_owned()],
            port: 853,
        });
        let error = profile_from_proto(dot).unwrap_err();
        assert_eq!(
            error.as_structured_error().code,
            "DIRECT_DNS_DOT_PATH_FORBIDDEN"
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
                    license_key: Vec::new(),
                    method: v1::IdentityProvisioningMethod::Unspecified as i32,
                    zero_trust: None,
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
                    license_key: Vec::new(),
                    method: v1::IdentityProvisioningMethod::Unspecified as i32,
                    zero_trust: None,
                }),
            ))
            .await;
        assert_eq!(
            encoding.error.as_ref().map(|error| error.code.as_str()),
            Some("FEATURE_REMOVED")
        );
    }

    #[tokio::test]
    async fn removed_secret_import_cannot_leave_a_partial_profile_transaction() {
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
                            license_key: Vec::new(),
                            zero_trust: None,
                        }),
                    },
                )),
            ))
            .await;
        assert_eq!(
            rejected.error.as_ref().map(|error| error.code.as_str()),
            Some("FEATURE_REMOVED")
        );
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
            config.network.transport = match transport.as_str() {
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
        config.network.frontends.tunnel = false;
        config.network.frontends.socks5 = false;
        config.network.frontends.http = true;
        config.network.proxy.http_listeners = vec![listener];
        if let Ok(transport) = std::env::var("USQUE_LIVE_TRANSPORT") {
            config.network.transport = match transport.as_str() {
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

        let identity = usque_core::parse_manual_warp_secret(&secret).expect("legacy identity");
        service
            .persist_identity(profile_id, &identity, Some(secret.as_bytes()))
            .await
            .expect("persist legacy identity records");

        let records = vault.records.lock().await;
        for record in SecretRecord::ALL {
            if record == SecretRecord::ProxyPassword {
                assert!(
                    !records.contains_key(&(profile_id, record)),
                    "identity persist must not write a proxy password"
                );
                continue;
            }
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

    #[tokio::test]
    async fn identity_metadata_defaults_to_consumer_and_zero_trust_disables_license_surfaces() {
        let directory = tempfile::tempdir().expect("tempdir");
        let vault = Arc::new(MemoryVault::default());
        let service = ControlService::open_with_vault(
            ConfigStore::new(directory.path().join("config.json")),
            Arc::clone(&vault) as Arc<dyn SecretVault>,
        )
        .expect("service");
        let profile_id = service.config_snapshot().await.profiles[0].id;

        let consumer = test_identity(IdentityProvider::Consumer, None);
        service
            .persist_identity(profile_id, &consumer, None)
            .await
            .unwrap();
        vault
            .delete(profile_id, SecretRecord::IdentityMetadata)
            .await
            .unwrap();
        assert_eq!(
            service
                .load_warp_identity(profile_id)
                .await
                .unwrap()
                .provider(),
            &IdentityProvider::Consumer
        );

        let zero_trust = test_identity(IdentityProvider::zero_trust("example-team").unwrap(), None);
        service
            .persist_identity(profile_id, &zero_trust, None)
            .await
            .unwrap();
        let catalog = service.profile_catalog().await;
        let status = catalog.identity_statuses.first().unwrap();
        assert_eq!(status.provider, v1::IdentityProvider::ZeroTrust as i32);
        assert_eq!(status.organization, "example-team");
        assert_eq!(status.license_state, v1::LicenseState::NotApplicable as i32);

        assert!(matches!(
            service.copy_license_key(profile_id).await,
            Err(ControlServiceError::IdentityOperationUnsupported)
        ));
        assert!(matches!(
            service
                .export_warp_secret(v1::ExportWarpSecretRequest {
                    profile_id: profile_id.to_string(),
                    destination: directory.path().join("secret.json").display().to_string(),
                    confirmed: true,
                })
                .await,
            Err(ControlServiceError::IdentityOperationUnsupported)
        ));

        vault
            .delete(profile_id, SecretRecord::AccessToken)
            .await
            .unwrap();
        let missing_catalog = service.profile_catalog().await;
        let missing = missing_catalog.identity_statuses.first().unwrap();
        assert_eq!(missing.state, v1::ProfileIdentityState::Missing as i32);
        assert_eq!(
            missing.license_state,
            v1::LicenseState::NotApplicable as i32
        );
        assert_eq!(missing.account_type, "Zero Trust");
        assert_eq!(missing.organization, "example-team");
    }

    #[tokio::test]
    async fn shared_zero_trust_endpoint_does_not_classify_an_unbound_consumer() {
        let directory = tempfile::tempdir().expect("tempdir");
        let vault = Arc::new(MemoryVault::default());
        let service = ControlService::open_with_vault(
            ConfigStore::new(directory.path().join("config.json")),
            Arc::clone(&vault) as Arc<dyn SecretVault>,
        )
        .expect("service");
        let profile_id = service.config_snapshot().await.profiles[0].id;

        service
            .persist_identity(
                profile_id,
                &test_identity(IdentityProvider::Consumer, None),
                None,
            )
            .await
            .unwrap();
        vault
            .delete(profile_id, SecretRecord::IdentityMetadata)
            .await
            .unwrap();
        let mut config = service.config_snapshot().await;
        config.network.endpoint = EndpointSettings {
            ipv4: "162.159.197.8".parse().unwrap(),
            ipv6: "2606:4700:102::8".parse().unwrap(),
            port: 443,
            sni: usque_core::ZERO_TRUST_SNI.to_owned(),
        };
        service.persist(config).await.unwrap();

        assert_eq!(
            service
                .load_identity_boundary_for_repair(profile_id)
                .await
                .unwrap(),
            IdentityProvider::Consumer
        );
        assert_eq!(
            service
                .load_warp_identity(profile_id)
                .await
                .unwrap()
                .provider(),
            &IdentityProvider::Consumer
        );
        let catalog = service.profile_catalog().await;
        let status = catalog.identity_statuses.first().unwrap();
        assert_eq!(status.state, v1::ProfileIdentityState::Ready as i32);
        assert_eq!(status.provider, v1::IdentityProvider::Consumer as i32);
    }

    #[tokio::test]
    async fn missing_metadata_on_a_bound_zero_trust_identity_fails_closed() {
        let directory = tempfile::tempdir().expect("tempdir");
        let vault = Arc::new(MemoryVault::default());
        let service = ControlService::open_with_vault(
            ConfigStore::new(directory.path().join("config.json")),
            Arc::clone(&vault) as Arc<dyn SecretVault>,
        )
        .expect("service");
        let profile_id = service.config_snapshot().await.profiles[0].id;

        let consumer = test_identity(IdentityProvider::Consumer, None);
        service
            .persist_identity(profile_id, &consumer, None)
            .await
            .unwrap();
        vault
            .delete(profile_id, SecretRecord::IdentityMetadata)
            .await
            .unwrap();
        assert_eq!(
            service
                .load_warp_identity(profile_id)
                .await
                .unwrap()
                .provider(),
            &IdentityProvider::Consumer,
            "pre-metadata Consumer identities must remain compatible"
        );

        let provider = IdentityProvider::zero_trust("example-team").unwrap();
        service
            .persist_identity(profile_id, &test_identity(provider.clone(), None), None)
            .await
            .unwrap();
        let mut config = service.config_snapshot().await;
        config
            .identity_bindings
            .insert(profile_id, provider.clone());
        service.persist(config).await.unwrap();
        vault
            .delete(profile_id, SecretRecord::IdentityMetadata)
            .await
            .unwrap();

        assert!(matches!(
            service.load_warp_identity(profile_id).await,
            Err(ControlServiceError::InvalidStoredIdentity)
        ));
        assert!(matches!(
            service.copy_license_key(profile_id).await,
            Err(ControlServiceError::InvalidStoredIdentity)
        ));
        assert!(matches!(
            service
                .export_warp_secret(v1::ExportWarpSecretRequest {
                    profile_id: profile_id.to_string(),
                    destination: directory.path().join("secret.json").display().to_string(),
                    confirmed: true,
                })
                .await,
            Err(ControlServiceError::InvalidStoredIdentity)
        ));

        let catalog = service.profile_catalog().await;
        let status = catalog.identity_statuses.first().unwrap();
        assert_eq!(status.state, v1::ProfileIdentityState::Invalid as i32);
        assert_eq!(status.provider, v1::IdentityProvider::ZeroTrust as i32);
        assert_eq!(status.license_state, v1::LicenseState::NotApplicable as i32);
        assert_eq!(status.organization, "example-team");
        assert_eq!(
            service
                .load_identity_boundary_for_repair(profile_id)
                .await
                .unwrap(),
            provider
        );

        let mut edited = service.config_snapshot().await.active_profile().unwrap();
        edited.endpoint.sni = "shared.example.com".to_owned();
        let stored = service.upsert_profile(edited).await.unwrap();
        assert_eq!(stored.endpoint.sni, "shared.example.com");
        assert_eq!(
            service.reset_profile(profile_id).await.unwrap().endpoint,
            EndpointSettings::default()
        );
    }

    #[tokio::test]
    async fn zero_trust_repair_updates_registered_ips_and_rejects_provider_changes() {
        let directory = tempfile::tempdir().expect("tempdir");
        let vault = Arc::new(MemoryVault::default());
        let service = ControlService::open_with_vault(
            ConfigStore::new(directory.path().join("config.json")),
            Arc::clone(&vault) as Arc<dyn SecretVault>,
        )
        .expect("service");
        let profile_id = service.config_snapshot().await.profiles[0].id;
        let provider = IdentityProvider::zero_trust("example-team").unwrap();
        service
            .persist_identity(profile_id, &test_identity(provider.clone(), None), None)
            .await
            .unwrap();
        let mut config = service.config_snapshot().await;
        config.network.endpoint.port = 8443;
        config.network.endpoint.sni = "shared.example.com".to_owned();
        service.persist(config).await.unwrap();
        let first_ips = ManagedEndpointIps {
            ipv4: "162.159.197.8".parse().unwrap(),
            ipv6: "2606:4700:102::8".parse().unwrap(),
        };
        service
            .replace_identity_locked(
                profile_id,
                test_identity(provider.clone(), None),
                Some(first_ips.clone()),
            )
            .await
            .unwrap();
        let stored = service.config_snapshot().await.active_profile().unwrap();
        assert_eq!(stored.endpoint.ipv4, first_ips.ipv4);
        assert_eq!(stored.endpoint.ipv6, first_ips.ipv6);
        assert_eq!(stored.endpoint.port, 8443);
        assert_eq!(stored.endpoint.sni, "shared.example.com");

        vault.delete_identity(profile_id).await.unwrap();
        let metadata = provider.to_metadata_json().unwrap();
        vault
            .put(profile_id, SecretRecord::IdentityMetadata, &metadata)
            .await
            .unwrap();
        let repaired_ips = ManagedEndpointIps {
            ipv4: "162.159.197.9".parse().unwrap(),
            ipv6: "2606:4700:102::9".parse().unwrap(),
        };
        service
            .replace_identity_locked(
                profile_id,
                test_identity(provider, None),
                Some(repaired_ips.clone()),
            )
            .await
            .expect("missing credentials should be repairable");
        let repaired = service.config_snapshot().await.active_profile().unwrap();
        assert_eq!(repaired.endpoint.ipv4, repaired_ips.ipv4);
        assert_eq!(repaired.endpoint.ipv6, repaired_ips.ipv6);
        assert_eq!(repaired.endpoint.port, 8443);
        assert_eq!(repaired.endpoint.sni, "shared.example.com");

        let cross_team = service
            .provision_identity(v1::ProvisionIdentityRequest {
                profile_id: profile_id.to_string(),
                terms_accepted: true,
                method: v1::IdentityProvisioningMethod::RegisterZeroTrust as i32,
                zero_trust: Some(v1::ZeroTrustEnrollment {
                    team_name: "other-team".to_owned(),
                    callback_uri: b"not-consumed".to_vec(),
                }),
                ..Default::default()
            })
            .await;
        assert!(matches!(
            cross_team,
            Err(ControlServiceError::IdentityProviderChangeUnsupported)
        ));

        service
            .replace_identity_locked(
                profile_id,
                test_identity(IdentityProvider::Consumer, None),
                None,
            )
            .await
            .unwrap();
        let consumer_conversion = service
            .provision_identity(v1::ProvisionIdentityRequest {
                profile_id: profile_id.to_string(),
                terms_accepted: true,
                method: v1::IdentityProvisioningMethod::RegisterZeroTrust as i32,
                zero_trust: Some(v1::ZeroTrustEnrollment {
                    team_name: "example-team".to_owned(),
                    callback_uri: b"not-consumed".to_vec(),
                }),
                ..Default::default()
            })
            .await;
        assert!(matches!(
            consumer_conversion,
            Err(ControlServiceError::IdentityProviderChangeUnsupported)
        ));
    }

    #[tokio::test]
    async fn interrupted_identity_replacement_restores_the_old_identity_and_endpoint() {
        let directory = tempfile::tempdir().expect("tempdir");
        let vault = Arc::new(MemoryVault::default());
        let service = ControlService::open_with_vault(
            ConfigStore::new(directory.path().join("config.json")),
            Arc::clone(&vault) as Arc<dyn SecretVault>,
        )
        .expect("service");
        let profile_id = service.config_snapshot().await.profiles[0].id;
        let backup_id = Uuid::new_v4();
        let provider = IdentityProvider::zero_trust("example-team").unwrap();
        let previous = test_identity(provider.clone(), None);
        let replacement = test_identity(provider.clone(), None);
        service
            .persist_identity(backup_id, &previous, None)
            .await
            .unwrap();
        service
            .persist_identity(profile_id, &replacement, None)
            .await
            .unwrap();

        let previous_token = vault
            .get(backup_id, SecretRecord::AccessToken)
            .await
            .unwrap()
            .unwrap();
        let replacement_token = vault
            .get(profile_id, SecretRecord::AccessToken)
            .await
            .unwrap()
            .unwrap();
        assert_ne!(&*previous_token, &*replacement_token);

        let old_ips = ManagedEndpointIps {
            ipv4: "162.159.197.8".parse().unwrap(),
            ipv6: "2606:4700:102::8".parse().unwrap(),
        };
        let mut interrupted = service.config_snapshot().await;
        interrupted.identity_bindings.insert(profile_id, provider);
        interrupted
            .set_managed_endpoint_ips(profile_id, old_ips.clone())
            .unwrap();
        interrupted.pending_identity_replacements.insert(
            profile_id,
            PendingIdentityReplacement {
                backup_identity_id: Some(backup_id),
                armed: true,
            },
        );
        service.persist(interrupted).await.unwrap();

        service.reap_pending_identity_deletions().await.unwrap();

        let restored_token = vault
            .get(profile_id, SecretRecord::AccessToken)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&*restored_token, &*previous_token);
        assert!(
            vault
                .get(backup_id, SecretRecord::AccessToken)
                .await
                .unwrap()
                .is_none()
        );
        let recovered = service.config_snapshot().await;
        assert!(recovered.pending_identity_replacements.is_empty());
        assert!(recovered.pending_identity_local_deletions.is_empty());
        let endpoint = recovered.active_profile().unwrap().endpoint;
        assert_eq!(endpoint.ipv4, old_ips.ipv4);
        assert_eq!(endpoint.ipv6, old_ips.ipv6);
    }

    #[tokio::test]
    async fn rejected_zero_trust_endpoint_rolls_back_staged_profile_identity() {
        let directory = tempfile::tempdir().expect("tempdir");
        let vault = Arc::new(MemoryVault::default());
        let service = ControlService::open_with_vault(
            ConfigStore::new(directory.path().join("config.json")),
            Arc::clone(&vault) as Arc<dyn SecretVault>,
        )
        .expect("service");
        let profile_id = Uuid::new_v4();
        let managed = ManagedEndpointIps {
            ipv4: "162.159.197.8".parse().unwrap(),
            ipv6: "2606:4700:102::8".parse().unwrap(),
        };
        let mut pending = service.config_snapshot().await;
        pending.network.dns_servers = vec![IpAddr::V4(managed.ipv4)];
        pending.pending_identity_creations.push(profile_id);
        service.persist(pending).await.unwrap();
        service
            .persist_identity(
                profile_id,
                &test_identity(IdentityProvider::zero_trust("example-team").unwrap(), None),
                None,
            )
            .await
            .unwrap();

        let profile = Profile {
            id: profile_id,
            name: "Work".to_owned(),
            ..Profile::default()
        };
        let result = service
            .commit_pending_profile_identity(
                profile,
                IdentityProvider::zero_trust("example-team").unwrap(),
                Some(managed),
                true,
            )
            .await;

        assert!(matches!(
            result,
            Err(ControlServiceError::ZeroTrustLocalCommit)
        ));
        let config = service.config_snapshot().await;
        assert!(config.account(profile_id).is_none());
        assert!(!config.pending_identity_creations.contains(&profile_id));
        assert!(
            vault
                .get(profile_id, SecretRecord::AccessToken)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn zero_trust_without_a_recovered_endpoint_requires_reauthentication() {
        let directory = tempfile::tempdir().expect("tempdir");
        let vault = Arc::new(MemoryVault::default());
        let service = ControlService::open_with_vault(
            ConfigStore::new(directory.path().join("config.json")),
            Arc::clone(&vault) as Arc<dyn SecretVault>,
        )
        .expect("service");
        let profile_id = service.config_snapshot().await.profiles[0].id;
        let provider = IdentityProvider::zero_trust("example-team").unwrap();
        service
            .persist_identity(profile_id, &test_identity(provider.clone(), None), None)
            .await
            .unwrap();
        let mut config = service.config_snapshot().await;
        config.identity_bindings.insert(profile_id, provider);
        service.persist(config).await.unwrap();

        let catalog = service.profile_catalog().await;
        assert_eq!(
            catalog.identity_statuses[0].state,
            v1::ProfileIdentityState::Invalid as i32
        );
        assert!(matches!(
            service.connect_locked(profile_id).await,
            Err(ControlServiceError::InvalidStoredIdentity)
        ));
    }

    #[tokio::test]
    async fn update_proxy_auth_stores_password_in_the_vault_not_profile_json() {
        let directory = tempfile::tempdir().expect("tempdir");
        let config_path = directory.path().join("config.json");
        let vault = Arc::new(MemoryVault::default());
        let service = ControlService::open_with_vault(
            ConfigStore::new(&config_path),
            Arc::clone(&vault) as Arc<dyn SecretVault>,
        )
        .expect("service");
        let profile_id = service.config_snapshot().await.profiles[0].id;

        let missing_password = service
            .handle(request(
                "auth-missing",
                control_request::Payload::UpdateProxyAuth(v1::UpdateProxyAuthRequest {
                    profile_id: profile_id.to_string(),
                    username: "lan-user".to_owned(),
                    password: Vec::new(),
                    confirmed: true,
                }),
            ))
            .await;
        assert_eq!(
            missing_password
                .error
                .as_ref()
                .map(|error| error.code.as_str()),
            Some("CONFIGURATION_INVALID")
        );

        let stored = service
            .handle(request(
                "auth-set",
                control_request::Payload::UpdateProxyAuth(v1::UpdateProxyAuthRequest {
                    profile_id: profile_id.to_string(),
                    username: "lan-user".to_owned(),
                    password: b"s3cret".to_vec(),
                    confirmed: true,
                }),
            ))
            .await;
        assert!(stored.error.is_none(), "{stored:?}");
        assert_eq!(
            service
                .config_snapshot()
                .await
                .network
                .proxy
                .auth_username
                .as_deref(),
            Some("lan-user")
        );
        let password = vault
            .get(SHARED_NETWORK_SECRET_ID, SecretRecord::ProxyPassword)
            .await
            .unwrap()
            .expect("vault password");
        assert_eq!(password.as_slice(), b"s3cret");
        let json = std::fs::read_to_string(&config_path).expect("config");
        assert!(json.contains("lan-user"));
        assert!(!json.contains("s3cret"));
        assert!(!json.to_ascii_lowercase().contains("password"));

        service
            .handle(request(
                "auth-clear",
                control_request::Payload::UpdateProxyAuth(v1::UpdateProxyAuthRequest {
                    profile_id: profile_id.to_string(),
                    username: String::new(),
                    password: Vec::new(),
                    confirmed: true,
                }),
            ))
            .await;
        assert!(
            vault
                .get(SHARED_NETWORK_SECRET_ID, SecretRecord::ProxyPassword)
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            service
                .config_snapshot()
                .await
                .network
                .proxy
                .auth_username
                .is_none()
        );
    }

    #[tokio::test]
    async fn legacy_per_profile_proxy_password_moves_to_the_shared_slot() {
        let directory = tempfile::tempdir().expect("tempdir");
        let vault = Arc::new(MemoryVault::default());
        let service = ControlService::open_with_vault(
            ConfigStore::new(directory.path().join("config.json")),
            Arc::clone(&vault) as Arc<dyn SecretVault>,
        )
        .expect("service");
        let profile_id = service.config_snapshot().await.profiles[0].id;
        vault
            .put(profile_id, SecretRecord::ProxyPassword, b"legacy-secret")
            .await
            .unwrap();

        service
            .migrate_shared_proxy_password()
            .await
            .expect("migrate");

        assert_eq!(
            vault
                .get(SHARED_NETWORK_SECRET_ID, SecretRecord::ProxyPassword)
                .await
                .unwrap()
                .expect("shared password")
                .as_slice(),
            b"legacy-secret"
        );
        assert!(
            vault
                .get(profile_id, SecretRecord::ProxyPassword)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn deleting_an_account_keeps_the_shared_proxy_password() {
        let directory = tempfile::tempdir().expect("tempdir");
        let vault = Arc::new(MemoryVault::default());
        let service = ControlService::open_with_vault(
            ConfigStore::new(directory.path().join("config.json")),
            Arc::clone(&vault) as Arc<dyn SecretVault>,
        )
        .expect("service");
        let original = service.config_snapshot().await.profiles[0].id;
        service
            .handle(request(
                "auth-set",
                control_request::Payload::UpdateProxyAuth(v1::UpdateProxyAuthRequest {
                    profile_id: original.to_string(),
                    username: "lan-user".to_owned(),
                    password: b"s3cret".to_vec(),
                    confirmed: true,
                }),
            ))
            .await;
        let extra = Profile {
            id: Uuid::new_v4(),
            name: "Work".to_owned(),
            ..Profile::default()
        };
        service.upsert_profile(extra.clone()).await.unwrap();
        service.delete_profile(original).await.unwrap();

        assert_eq!(
            vault
                .get(SHARED_NETWORK_SECRET_ID, SecretRecord::ProxyPassword)
                .await
                .unwrap()
                .expect("shared password")
                .as_slice(),
            b"s3cret"
        );
        assert_eq!(
            service
                .config_snapshot()
                .await
                .network
                .proxy
                .auth_username
                .as_deref(),
            Some("lan-user")
        );
        assert!(
            vault
                .get(original, SecretRecord::ProxyPassword)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn reset_zero_trust_profile_keeps_registered_ips_and_resets_port_and_sni() {
        let directory = tempfile::tempdir().expect("tempdir");
        let service = ControlService::open_with_vault(
            ConfigStore::new(directory.path().join("config.json")),
            Arc::new(MemoryVault::default()),
        )
        .expect("service");
        let profile_id = service.config_snapshot().await.profiles[0].id;
        let custom = EndpointSettings {
            ipv4: "192.0.2.1".parse().unwrap(),
            ipv6: "2001:db8::1".parse().unwrap(),
            port: 8443,
            sni: "shared.example.com".to_owned(),
        };
        let managed = ManagedEndpointIps {
            ipv4: "162.159.197.8".parse().unwrap(),
            ipv6: "2606:4700:102::8".parse().unwrap(),
        };
        let mut config = service.config_snapshot().await;
        config.network.endpoint = custom;
        config.network.mtu = 1400;
        config.identity_bindings.insert(
            profile_id,
            IdentityProvider::zero_trust("example-team").unwrap(),
        );
        config
            .set_managed_endpoint_ips(profile_id, managed.clone())
            .unwrap();
        service.persist(config).await.unwrap();

        let reset = service.reset_profile(profile_id).await.unwrap();
        assert_eq!(reset.endpoint.ipv4, managed.ipv4);
        assert_eq!(reset.endpoint.ipv6, managed.ipv6);
        assert_eq!(reset.endpoint.port, EndpointSettings::default().port);
        assert_eq!(reset.endpoint.sni, EndpointSettings::default().sni);
        assert_eq!(reset.mtu, 1280);
        assert_eq!(service.config_snapshot().await.network.mtu, 1280);
        assert_eq!(
            service.config_snapshot().await.network.endpoint,
            EndpointSettings::default()
        );
    }

    fn reconfigure_snapshot(response: &ControlResponse) -> &usque_ipc::v1::ConnectionSnapshot {
        match response.payload.as_ref() {
            Some(control_response::Payload::Reconfigure(result)) => {
                result.snapshot.as_ref().expect("reconfigure snapshot")
            }
            other => panic!(
                "unexpected reconfigure payload: {other:?} {:?}",
                response.error
            ),
        }
    }

    #[tokio::test]
    async fn reconfigure_active_profile_keeps_masque_for_socks_http_and_tunnel_flips() {
        let directory = tempfile::tempdir().unwrap();
        let service = ControlService::open_with_vault(
            ConfigStore::new(directory.path().join("config.json")),
            Arc::new(MemoryVault::default()),
        )
        .unwrap();
        let mut profile = service.config_snapshot().await.active_profile().unwrap();
        profile.frontends = FrontendSettings {
            tunnel: true,
            socks5: true,
            http: true,
        };
        service
            .install_test_session(profile.clone(), true, 5)
            .await
            .expect("install harness");

        let mut socks = profile.clone();
        socks.proxy.socks5_listeners[0].set_port(1081);
        let socks_response = service
            .handle(request(
                "hot-socks",
                control_request::Payload::ReconfigureActiveProfile(Box::new(
                    v1::ReconfigureActiveProfileRequest {
                        profile: Some(profile_to_proto(&socks)),
                    },
                )),
            ))
            .await;
        assert!(socks_response.error.is_none(), "{:?}", socks_response.error);
        assert_eq!(reconfigure_snapshot(&socks_response).reconnect_count, 5);
        assert_eq!(
            service.test_harness_counts().await,
            Some((5, 1, 0, 0, true))
        );

        let mut system_proxy = socks.clone();
        system_proxy.proxy.system_proxy = true;
        let proxy_response = service
            .handle(request(
                "hot-system-proxy",
                control_request::Payload::ReconfigureActiveProfile(Box::new(
                    v1::ReconfigureActiveProfileRequest {
                        profile: Some(profile_to_proto(&system_proxy)),
                    },
                )),
            ))
            .await;
        assert!(proxy_response.error.is_none(), "{:?}", proxy_response.error);
        assert_eq!(reconfigure_snapshot(&proxy_response).reconnect_count, 5);
        assert_eq!(
            service.test_harness_counts().await,
            Some((5, 1, 0, 0, true))
        );

        let mut detached = system_proxy.clone();
        detached.frontends.tunnel = false;
        let detach_response = service
            .handle(request(
                "hot-detach",
                control_request::Payload::ReconfigureActiveProfile(Box::new(
                    v1::ReconfigureActiveProfileRequest {
                        profile: Some(profile_to_proto(&detached)),
                    },
                )),
            ))
            .await;
        assert!(
            detach_response.error.is_none(),
            "{:?}",
            detach_response.error
        );
        assert_eq!(reconfigure_snapshot(&detach_response).reconnect_count, 5);
        assert_eq!(
            service.test_harness_counts().await,
            Some((5, 1, 0, 1, false))
        );

        let mut attached = detached.clone();
        attached.frontends.tunnel = true;
        let attach_response = service
            .handle(request(
                "hot-attach",
                control_request::Payload::ReconfigureActiveProfile(Box::new(
                    v1::ReconfigureActiveProfileRequest {
                        profile: Some(profile_to_proto(&attached)),
                    },
                )),
            ))
            .await;
        assert!(
            attach_response.error.is_none(),
            "{:?}",
            attach_response.error
        );
        assert_eq!(reconfigure_snapshot(&attach_response).reconnect_count, 5);
        assert_eq!(
            service.test_harness_counts().await,
            Some((5, 1, 1, 1, true))
        );

        let mut cold = attached.clone();
        cold.mtu = 1400;
        let cold_response = service
            .handle(request(
                "cold-mtu",
                control_request::Payload::ReconfigureActiveProfile(Box::new(
                    v1::ReconfigureActiveProfileRequest {
                        profile: Some(profile_to_proto(&cold)),
                    },
                )),
            ))
            .await;
        assert_eq!(
            cold_response
                .error
                .as_ref()
                .map(|error| error.code.as_str()),
            if cfg!(windows) {
                Some("MISSING_CREDENTIAL")
            } else {
                Some("OPERATING_MODE_UNAVAILABLE")
            }
        );
        assert!(service.test_harness_counts().await.is_none());
    }

    #[tokio::test]
    async fn detach_keeps_proxy_profile_if_system_proxy_reapply_fails() {
        let directory = tempfile::tempdir().unwrap();
        let store = ConfigStore::new(directory.path().join("config.json"));
        let service =
            ControlService::open_with_vault(store.clone(), Arc::new(MemoryVault::default()))
                .unwrap();
        let mut profile = service.config_snapshot().await.active_profile().unwrap();
        profile.frontends = FrontendSettings {
            tunnel: true,
            socks5: true,
            http: true,
        };
        profile.proxy.system_proxy = true;
        service
            .install_test_session(profile.clone(), true, 5)
            .await
            .expect("install harness");
        service.test_fail_system_proxy_after_detach().await;

        let mut detached = profile.clone();
        detached.frontends.tunnel = false;
        let response = service
            .handle(request(
                "hot-detach-proxy-fail",
                control_request::Payload::ReconfigureActiveProfile(Box::new(
                    v1::ReconfigureActiveProfileRequest {
                        profile: Some(profile_to_proto(&detached)),
                    },
                )),
            ))
            .await;
        assert!(
            response.error.is_some(),
            "system-proxy failure must surface"
        );
        assert_eq!(
            service.test_harness_counts().await,
            Some((5, 0, 0, 1, false))
        );
        let persisted = ControlService::open(store)
            .expect("reopen")
            .config_snapshot()
            .await
            .active_profile()
            .unwrap();
        assert!(!persisted.frontends.tunnel);
    }

    fn request(id: &str, payload: control_request::Payload) -> ControlRequest {
        ControlRequest {
            request_id: id.to_owned(),
            payload: Some(payload),
        }
    }
}
