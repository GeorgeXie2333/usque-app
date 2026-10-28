//! Engine half of the macOS provider boundary. The provider owns the only
//! MASQUE runtime; this process only owns credentials and OS-socket frontends.
use crate::{ControlService, ControlServiceError};
use async_trait::async_trait;
use std::{
    net::SocketAddr,
    path::PathBuf,
    sync::{Arc, Mutex as StdMutex},
};
use tokio::sync::{Mutex, watch};
use tokio_util::{sync::CancellationToken, task::AbortOnDropHandle};
use usque_core::{
    Profile, WarpIdentity,
    vpngate::{GateStage, GateStatus},
};
use usque_macos_runtime::{
    credentials::{CredentialBroker, PersistenceAck, SessionCredentials},
    protocol::{
        Command, Envelope, PROTOCOL_VERSION, Phase, Reply, SessionConfiguration, SessionKey,
        Snapshot,
    },
};
use usque_platform::{SecretRecord, SecretVault};
use usque_transport::{
    ConnectionTimelineSnapshot, RuntimeHealth, RuntimePath, SystemProxyRuntime, TrafficSnapshot,
    TransportError,
};
use uuid::Uuid;
use zeroize::Zeroizing;
mod helper;
pub use helper::BundledProviderControl;

/// Supplied by the native host after its XPC peer has been authenticated.
/// A successful exchange acknowledges a command, not a connected tunnel.
#[async_trait]
pub trait ProviderControl: Send + Sync {
    async fn exchange(
        &self,
        request: Envelope,
        broker: Arc<dyn CredentialBroker>,
    ) -> Result<Reply, String>;
    /// Production activation remains closed until signed routing, DNS and
    /// independent-filter acceptance has been recorded on both architectures.
    fn production_ready(&self) -> bool {
        false
    }
    async fn network_generation(&self) -> Result<u64, String> {
        Err("physical_network_unavailable".into())
    }
    async fn authorize_flow(
        &self,
        _session: &SessionKey,
        _intent: usque_transport::SystemFlowIntent,
    ) -> Result<(), String> {
        Err("macos_flow_authorizer_unavailable".into())
    }
    #[cfg(test)]
    fn isolated_test(&self) -> bool {
        false
    }
}

pub struct UnavailableProvider;
#[async_trait]
impl ProviderControl for UnavailableProvider {
    async fn exchange(&self, _: Envelope, _: Arc<dyn CredentialBroker>) -> Result<Reply, String> {
        Err("macos_provider_unavailable".into())
    }
}

fn error(code: impl Into<String>) -> ControlServiceError {
    // Provider errors are stable codes. Never interpolate a native message
    // containing credential material into Engine logs or IPC diagnostics.
    let code = code.into();
    let code = match code.as_str() {
        "macos_provider_unavailable" => "macos_provider_unavailable",
        "macos_provider_stale_reply" => "macos_provider_stale_reply",
        "macos_protection_not_confirmed" => "macos_protection_not_confirmed",
        "extension_version_mismatch" => "extension_version_mismatch",
        "permission_denied" => "permission_denied",
        "awaiting_approval" => "awaiting_approval",
        "activation_requires_reboot" => "activation_requires_reboot",
        "signature_required" => "signature_required",
        "production_evidence_required" => "production_evidence_required",
        _ => "macos_provider_control_failed",
    };
    ControlServiceError::PlatformVpn(code.into())
}

pub(crate) fn provider_permitted(provider: &dyn ProviderControl) -> bool {
    #[cfg(test)]
    if provider.isolated_test() {
        return true;
    }
    provider.production_ready()
}

pub(crate) struct EngineCredentialBroker {
    current: Arc<StdMutex<Option<SessionKey>>>,
    profile_id: Uuid,
    vault: Arc<dyn SecretVault>,
    pin_lock: Arc<Mutex<()>>,
    proton: Option<(Uuid, Arc<dyn usque_core::proton_vpn::LeasePersistence>)>,
}
impl EngineCredentialBroker {
    fn validate(&self, session: &SessionKey, profile_id: Uuid) -> Result<(), String> {
        if self.profile_id != profile_id
            || self
                .current
                .lock()
                .map_err(|_| "credential_broker_unavailable")?
                .as_ref()
                != Some(session)
        {
            return Err("stale_credential_session".into());
        }
        Ok(())
    }
}
#[async_trait]
impl CredentialBroker for EngineCredentialBroker {
    async fn persist_pin(
        &self,
        session: &SessionKey,
        profile_id: Uuid,
        previous: &WarpIdentity,
        next: &WarpIdentity,
    ) -> Result<PersistenceAck, String> {
        let _serial = self.pin_lock.lock().await;
        self.validate(session, profile_id)?;
        // A pin refresh is not authorization to replace an account or its key.
        if previous.device_id() != next.device_id()
            || previous.access_token() != next.access_token()
            || previous.provider() != next.provider()
            || previous.license() != next.license()
            || previous
                .key_pair
                .public_spki_der()
                .map_err(|_| "invalid_pin_update")?
                != next
                    .key_pair
                    .public_spki_der()
                    .map_err(|_| "invalid_pin_update")?
        {
            return Err("invalid_pin_update".into());
        }
        let updates = [
            (
                SecretRecord::EndpointPin,
                Zeroizing::new(next.endpoint_pin.spki_der().to_vec()),
            ),
            (
                SecretRecord::AssignedIpv4,
                Zeroizing::new(next.assigned_ipv4.to_string().into_bytes()),
            ),
            (
                SecretRecord::AssignedIpv6,
                Zeroizing::new(next.assigned_ipv6.to_string().into_bytes()),
            ),
        ];
        let mut old = Vec::new();
        for (record, _) in &updates {
            old.push((
                *record,
                self.vault
                    .get(profile_id, *record)
                    .await
                    .map_err(|_| "credential_read_failed")?,
            ));
        }
        if old[0].1.as_ref().map(|v| v.as_slice()) != Some(previous.endpoint_pin.spki_der()) {
            return Err("stale_pin_update".into());
        }
        let portable = self
            .vault
            .get(profile_id, SecretRecord::WarpSecret)
            .await
            .map_err(|_| "credential_read_failed")?;
        if let Some(value) = &portable {
            old.push((SecretRecord::WarpSecret, Some(value.clone())));
        }
        let persisted = async {
            for (record, value) in &updates {
                self.validate(session, profile_id)?;
                self.vault
                    .put(profile_id, *record, value)
                    .await
                    .map_err(|_| "credential_save_failed")?;
            }
            if portable.is_some() {
                let value = next
                    .to_portable_secret_json()
                    .map_err(|_| "invalid_pin_update")?;
                self.vault
                    .put(profile_id, SecretRecord::WarpSecret, value.as_bytes())
                    .await
                    .map_err(|_| "credential_save_failed")?;
            }
            self.validate(session, profile_id)?;
            Ok::<_, String>(())
        }
        .await;
        if let Err(failure) = persisted {
            let mut failed = false;
            for (record, value) in old {
                failed |= match value {
                    Some(value) => self.vault.put(profile_id, record, &value).await.is_err(),
                    None => self.vault.delete(profile_id, record).await.is_err(),
                };
            }
            return Err(if failed {
                "credential_rollback_failed".into()
            } else {
                failure
            });
        }
        Ok(PersistenceAck {
            session: session.clone(),
            profile_id,
            revision: Uuid::new_v4(),
        })
    }
    fn proton_persistence(
        &self,
        session: &SessionKey,
        profile_id: Uuid,
        initial: &usque_core::proton_vpn::Record,
    ) -> Result<Arc<dyn usque_core::proton_vpn::LeasePersistence>, String> {
        // Proton account IDs differ from WARP account IDs; the selected lease
        // itself is the capability and is never exposed as a directory/vault.
        if self
            .current
            .lock()
            .map_err(|_| "credential_broker_unavailable")?
            .as_ref()
            != Some(session)
        {
            return Err("stale_credential_session".into());
        }
        let (id, store) = self
            .proton
            .as_ref()
            .filter(|(id, _)| *id == profile_id)
            .ok_or("proton_lease_not_selected")?;
        let current = store.load().map_err(|_| "proton_load_failed")?;
        if current.id != *id || current.revision != initial.revision {
            return Err("stale_proton_lease".into());
        }
        Ok(Arc::new(BoundProtonLease {
            current: Arc::clone(&self.current),
            session: session.clone(),
            id: *id,
            inner: Arc::clone(store),
        }))
    }
}
struct BoundProtonLease {
    current: Arc<StdMutex<Option<SessionKey>>>,
    session: SessionKey,
    id: Uuid,
    inner: Arc<dyn usque_core::proton_vpn::LeasePersistence>,
}
impl BoundProtonLease {
    fn validate(&self) -> Result<(), usque_core::chain_exit::ImportError> {
        if self
            .current
            .lock()
            .ok()
            .and_then(|value| value.clone())
            .as_ref()
            != Some(&self.session)
        {
            return Err(usque_core::chain_exit::ImportError::new(
                0,
                "macos_broker",
                "stale_session",
            ));
        }
        Ok(())
    }
}
impl usque_core::proton_vpn::LeasePersistence for BoundProtonLease {
    fn load(&self) -> Result<usque_core::proton_vpn::Record, usque_core::chain_exit::ImportError> {
        self.validate()?;
        self.inner.load()
    }
    fn save(
        &self,
        record: &usque_core::proton_vpn::Record,
    ) -> Result<(), usque_core::chain_exit::ImportError> {
        let binding = self.current.lock().map_err(|_| {
            usque_core::chain_exit::ImportError::new(0, "macos_broker", "broker_unavailable")
        })?;
        if binding.as_ref() != Some(&self.session) {
            return Err(usque_core::chain_exit::ImportError::new(
                0,
                "macos_broker",
                "stale_session",
            ));
        }
        if record.id != self.id {
            return Err(usque_core::chain_exit::ImportError::new(
                0,
                "macos_broker",
                "account_mismatch",
            ));
        }
        self.inner.save(record)
    }
    fn lock(&self) -> Result<std::fs::File, usque_core::chain_exit::ImportError> {
        self.validate()?;
        self.inner.lock()
    }
}

pub(crate) struct MacVpnRuntime {
    provider: Arc<dyn ProviderControl>,
    broker: Arc<EngineCredentialBroker>,
    session: SessionKey,
    profile: Profile,
    cache_dir: PathBuf,
    snapshot: Arc<StdMutex<Snapshot>>,
    health: watch::Sender<RuntimeHealth>,
    frontends: SystemProxyRuntime,
    polling: Option<AbortOnDropHandle<()>>,
    local_failure: Option<String>,
    gate: GateStatus,
    detaching: Arc<std::sync::atomic::AtomicBool>,
}
pub(crate) struct ProviderStart {
    pub provider: Arc<dyn ProviderControl>,
    pub vault: Arc<dyn SecretVault>,
    pub cache_dir: PathBuf,
    pub session: SessionKey,
    pub detaching: Arc<std::sync::atomic::AtomicBool>,
}

fn requested_path(profile: &Profile) -> RuntimePath {
    RuntimePath {
        transport: if profile.transport == usque_core::TransportPolicy::Http2 {
            usque_core::Transport::Http2
        } else {
            usque_core::Transport::Http3
        },
        endpoint_family: if matches!(
            profile.ip_policy,
            usque_core::IpPolicy::Ipv6Only | usque_core::IpPolicy::PreferIpv6
        ) {
            usque_core::AddressFamily::Ipv6
        } else {
            usque_core::AddressFamily::Ipv4
        },
        ipv4_available: false,
        ipv6_available: false,
    }
}
fn snapshot_health(snapshot: &Snapshot, requested: RuntimePath) -> RuntimeHealth {
    if snapshot.phase == Phase::Connected
        && snapshot.protection_active
        && let Some(path) = &snapshot.selected_path
    {
        return RuntimeHealth::Connected {
            path: RuntimePath {
                transport: path.transport,
                endpoint_family: path.endpoint_family,
                ipv4_available: path.ipv4_available,
                ipv6_available: path.ipv6_available,
            },
            reconnect_count: 0,
        };
    }
    if matches!(
        snapshot.phase,
        Phase::Failed | Phase::Stopped | Phase::Stopping
    ) {
        let failure = TransportError::TunnelClosed;
        return RuntimeHealth::Failed {
            last_path: requested,
            reconnect_count: 0,
            message: "macos_provider_not_forwarding".into(),
            failure: failure.failure(None, None),
        };
    }
    RuntimeHealth::Connecting {
        requested,
        attempt: 1,
    }
}
async fn exchange(
    provider: &dyn ProviderControl,
    broker: Arc<dyn CredentialBroker>,
    session: &SessionKey,
    command: Command,
) -> Result<Snapshot, ControlServiceError> {
    let request = Envelope {
        version: PROTOCOL_VERSION,
        operation_id: Uuid::new_v4(),
        session: session.clone(),
        command,
    };
    let operation_id = request.operation_id;
    let reply = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        provider.exchange(request, broker),
    )
    .await
    .map_err(|_| error("timeout"))?
    .map_err(error)?;
    if reply.operation_id != operation_id
        || reply
            .snapshot
            .session
            .as_ref()
            .is_some_and(|key| key != session)
    {
        return Err(error("macos_provider_stale_reply"));
    }
    Ok(reply.snapshot)
}
fn observed_health(
    snapshot: &Snapshot,
    requested: RuntimePath,
    previous: &RuntimeHealth,
) -> RuntimeHealth {
    let next = snapshot_health(snapshot, requested);
    match next {
        RuntimeHealth::Connected { path, .. } => RuntimeHealth::Connected {
            path,
            reconnect_count: previous.reconnect_count()
                + u32::from(matches!(previous, RuntimeHealth::Reconnecting { .. })),
        },
        RuntimeHealth::Connecting { .. }
            if matches!(
                previous,
                RuntimeHealth::Connected { .. } | RuntimeHealth::Reconnecting { .. }
            ) =>
        {
            RuntimeHealth::Reconnecting {
                last_path: previous.path(),
                attempt: 1,
                reconnect_count: previous.reconnect_count(),
                reason: "macos_provider_reconnecting".into(),
                failure: TransportError::TunnelClosed.failure(None, None),
            }
        }
        other => other,
    }
}
async fn stop_confirmed(
    provider: &dyn ProviderControl,
    broker: Arc<dyn CredentialBroker>,
    session: &SessionKey,
) -> Result<Snapshot, ControlServiceError> {
    let mut snapshot = exchange(provider, broker.clone(), session, Command::Stop).await?;
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    while snapshot.phase != Phase::Stopped || snapshot.protection_active {
        if snapshot.phase == Phase::Failed || tokio::time::Instant::now() >= deadline {
            return Err(error("stop_not_confirmed"));
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        snapshot = exchange(provider, broker.clone(), session, Command::Snapshot).await?;
    }
    Ok(snapshot)
}
impl MacVpnRuntime {
    pub(crate) async fn start(
        context: ProviderStart,
        profile: &Profile,
        identity: &WarpIdentity,
        selected: Option<(
            usque_core::vpngate::ServerSummary,
            usque_core::vpngate::PreparedProfile,
        )>,
        cancellation: &CancellationToken,
    ) -> Result<Self, ControlServiceError> {
        let ProviderStart {
            provider,
            vault,
            cache_dir,
            session,
            detaching,
        } = context;
        if !provider_permitted(provider.as_ref()) {
            return Err(ControlServiceError::OperatingModeUnavailable(profile.mode));
        }
        let broker = Arc::new(EngineCredentialBroker {
            current: Arc::new(StdMutex::new(Some(session.clone()))),
            profile_id: profile.id,
            vault,
            pin_lock: Arc::new(Mutex::new(())),
            proton: selected
                .as_ref()
                .and_then(|(_, p)| Some((p.summary.as_ref()?.id, p.proton_persistence.clone()?))),
        });
        let mut credentials = SessionCredentials::from_identity(identity).map_err(error)?;
        credentials.chain = selected
            .as_ref()
            .map(|(server, prepared)| {
                usque_macos_runtime::credentials::ChainCredentials::from_prepared(server, prepared)
            })
            .transpose()
            .map_err(error)?;
        let snapshot = exchange(
            provider.as_ref(),
            broker.clone(),
            &session,
            Command::Start {
                configuration: SessionConfiguration::snapshot(profile.clone(), cache_dir.clone())
                    .map_err(error)?,
                credentials,
            },
        )
        .await?;
        let requested = requested_path(profile);
        let health = watch::channel(snapshot_health(&snapshot, requested)).0;
        // OS listeners are gated on the provider's actual Connected health.
        let authorizer = Arc::new(helper::FlowAuthorizer {
            provider: Arc::clone(&provider),
            session: Arc::clone(&broker.current),
        });
        let frontends =
            SystemProxyRuntime::start_with_authorizer(profile, health.subscribe(), authorizer)
                .await
                .map_err(ControlServiceError::Transport)?;
        let mut result = Self {
            provider,
            broker,
            session,
            profile: profile.clone(),
            cache_dir,
            snapshot: Arc::new(StdMutex::new(snapshot)),
            health,
            frontends,
            polling: None,
            local_failure: None,
            gate: GateStatus::default(),
            detaching,
        };
        if cancellation.is_cancelled() {
            result.detach().await;
            return Err(TransportError::TunnelClosed.into());
        }
        result.start_polling();
        result.frontends.activate_final();
        Ok(result)
    }
    fn start_polling(&mut self) {
        let provider = Arc::clone(&self.provider);
        let broker = self.broker.clone();
        let session = self.session.clone();
        let snapshot = Arc::clone(&self.snapshot);
        let health = self.health.clone();
        let requested = requested_path(&self.profile);
        self.polling = Some(AbortOnDropHandle::new(tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_millis(200));
            loop {
                interval.tick().await;
                match exchange(
                    provider.as_ref(),
                    broker.clone(),
                    &session,
                    Command::Snapshot,
                )
                .await
                {
                    Ok(value) => {
                        let previous = health.borrow().clone();
                        health.send_replace(observed_health(&value, requested, &previous));
                        if let Ok(mut state) = snapshot.lock() {
                            *state = value;
                        }
                    }
                    Err(_) => {
                        health.send_replace(RuntimeHealth::Failed {
                            last_path: requested,
                            reconnect_count: 0,
                            message: "macos_provider_unavailable".into(),
                            failure: TransportError::TunnelClosed.failure(None, None),
                        });
                        // Loss of control must never send Stop or release protection.
                        break;
                    }
                }
            }
        })));
    }
    pub(crate) fn protection_active(&self) -> bool {
        self.snapshot.lock().is_ok_and(|s| s.protection_active)
    }
    pub(crate) fn failure_retained(&self) -> bool {
        self.protection_active() && matches!(self.health(), RuntimeHealth::Failed { .. })
    }
    pub(crate) fn health(&self) -> RuntimeHealth {
        self.health.borrow().clone()
    }
    pub(crate) fn path(&self) -> RuntimePath {
        self.health().path()
    }
    pub(crate) fn listeners(&self) -> &[SocketAddr] {
        self.frontends.listeners()
    }
    pub(crate) fn socks5_listeners(&self) -> &[SocketAddr] {
        self.frontends.socks5_listeners()
    }
    pub(crate) fn http_listeners(&self) -> &[SocketAddr] {
        self.frontends.http_listeners()
    }
    pub(crate) fn statistics(&self) -> TrafficSnapshot {
        self.frontends.statistics()
    }
    pub(crate) fn connection_timeline(&self) -> ConnectionTimelineSnapshot {
        Default::default()
    }
    pub(crate) fn subscribe_network_quality(
        &self,
    ) -> watch::Receiver<usque_transport::NetworkQualitySnapshot> {
        watch::channel(crate::network_quality::disconnected_snapshot()).1
    }
    pub(crate) fn failure(&self) -> Option<String> {
        self.local_failure.clone().or_else(|| {
            matches!(self.health(), RuntimeHealth::Failed { .. })
                .then(|| "macos_provider_not_forwarding".into())
        })
    }
    pub(crate) fn gate_status(&self) -> GateStatus {
        self.gate.clone()
    }
    pub(crate) fn quiesce_final(&mut self) {
        self.frontends.quiesce_final();
    }
    pub(crate) async fn fail_gate(&mut self, reason: usque_core::vpngate::GateFailure) {
        self.quiesce_final();
        self.gate.stage = GateStage::Error;
        self.gate.failure = Some(reason);
    }
    pub(crate) async fn reconfigure_frontends(
        &mut self,
        profile: &Profile,
    ) -> Result<(), ControlServiceError> {
        self.frontends
            .reconfigure_frontends(profile)
            .await
            .map_err(ControlServiceError::Transport)
    }
    pub(crate) async fn replace(
        &mut self,
        profile: &Profile,
        identity: &WarpIdentity,
        selected: Option<(
            usque_core::vpngate::ServerSummary,
            usque_core::vpngate::PreparedProfile,
        )>,
        cancellation: &CancellationToken,
    ) -> Result<(), ControlServiceError> {
        let protective = exchange(
            self.provider.as_ref(),
            self.broker.clone(),
            &self.session,
            Command::Protect,
        )
        .await?;
        let protective = if protective.protection_active {
            protective
        } else {
            let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
            loop {
                if cancellation.is_cancelled() || tokio::time::Instant::now() >= deadline {
                    return Err(error("macos_protection_not_confirmed"));
                }
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                let value = exchange(
                    self.provider.as_ref(),
                    self.broker.clone(),
                    &self.session,
                    Command::Snapshot,
                )
                .await?;
                if value.protection_active {
                    break value;
                }
            }
        };
        *self.snapshot.lock().map_err(|_| error("snapshot_failed"))? = protective;
        self.quiesce_final();
        self.polling.take();
        let next = SessionKey {
            session_id: self.session.session_id,
            config_revision: self
                .session
                .config_revision
                .checked_add(1)
                .ok_or_else(|| error("revision_exhausted"))?,
            network_generation: self.provider.network_generation().await.map_err(error)?,
        };
        let mut credentials = SessionCredentials::from_identity(identity).map_err(error)?;
        credentials.chain = selected
            .as_ref()
            .map(|(server, prepared)| {
                usque_macos_runtime::credentials::ChainCredentials::from_prepared(server, prepared)
            })
            .transpose()
            .map_err(error)?;
        let broker = Arc::new(EngineCredentialBroker {
            current: Arc::clone(&self.broker.current),
            profile_id: profile.id,
            vault: Arc::clone(&self.broker.vault),
            pin_lock: Arc::clone(&self.broker.pin_lock),
            proton: selected
                .as_ref()
                .and_then(|(_, p)| Some((p.summary.as_ref()?.id, p.proton_persistence.clone()?))),
        });
        self.health.send_replace(RuntimeHealth::Connecting {
            requested: requested_path(profile),
            attempt: 1,
        });
        // The old broker is revoked before replacement. A lost reply cannot
        // authorize stale credentials or silently resume old local listeners.
        *self
            .broker
            .current
            .lock()
            .map_err(|_| error("broker_failed"))? = Some(next.clone());
        match exchange(
            self.provider.as_ref(),
            broker.clone(),
            &next,
            Command::Replace {
                configuration: SessionConfiguration::snapshot(
                    profile.clone(),
                    self.cache_dir.clone(),
                )
                .map_err(error)?,
                credentials,
            },
        )
        .await
        {
            Ok(snapshot) => {
                self.session = next;
                self.broker = broker;
                self.profile = profile.clone();
                self.health
                    .send_replace(snapshot_health(&snapshot, requested_path(profile)));
                *self.snapshot.lock().map_err(|_| error("snapshot_failed"))? = snapshot;
                self.frontends
                    .reconfigure_frontends(profile)
                    .await
                    .map_err(ControlServiceError::Transport)?;
                self.start_polling();
                self.frontends.activate_final();
                Ok(())
            }
            Err(failure) => {
                self.local_failure = Some("macos_replace_failed".into());
                Err(failure)
            }
        }
    }
    pub(crate) async fn shutdown(&mut self) -> Result<(), ControlServiceError> {
        if self.detaching.load(std::sync::atomic::Ordering::Acquire) {
            self.detach().await;
            return Ok(());
        }
        self.quiesce_final();
        self.polling.take();
        self.frontends.shutdown().await;
        let stopped =
            stop_confirmed(self.provider.as_ref(), self.broker.clone(), &self.session).await?;
        *self
            .broker
            .current
            .lock()
            .map_err(|_| error("broker_failed"))? = None;
        self.health
            .send_replace(snapshot_health(&stopped, requested_path(&self.profile)));
        *self.snapshot.lock().map_err(|_| error("snapshot_failed"))? = stopped;
        Ok(())
    }
    /// Process lifecycle detaches only local listeners. The system extension's
    /// session and physical-exit protection survive Engine/UI termination.
    pub(crate) async fn detach(&mut self) {
        self.quiesce_final();
        self.polling.take();
        self.frontends.shutdown().await;
    }
}
impl ControlService {
    /// The caller is the trusted native host, never a UI-supplied executable path.
    pub async fn set_macos_provider_control(
        &self,
        provider: Arc<dyn ProviderControl>,
    ) -> Result<(), ControlServiceError> {
        let _mutation = self.mutation_lock.lock().await;
        if self.data_plane.lock().await.is_some() || self.macos_reserved.lock().await.is_some() {
            return Err(ControlServiceError::InvalidRequest(
                "cannot replace an active macOS provider bridge".into(),
            ));
        }
        *self.macos_provider.write().await = provider;
        Ok(())
    }
    pub(crate) async fn reserve_macos_start(&self) -> Result<SessionKey, ControlServiceError> {
        if let Some(session) = self.macos_reserved.lock().await.as_ref() {
            return Ok(session.clone());
        }
        let session = SessionKey {
            session_id: Uuid::new_v4(),
            config_revision: 1,
            network_generation: self
                .macos_provider
                .read()
                .await
                .network_generation()
                .await
                .map_err(error)?,
        };
        *self.macos_reserved.lock().await = Some(session.clone());
        Ok(session)
    }
    pub(crate) async fn stop_reserved_macos(&self) -> Result<(), ControlServiceError> {
        let session = self.macos_reserved.lock().await.clone();
        if let Some(session) = session {
            let broker = Arc::new(usque_macos_runtime::credentials::UnavailableCredentialBroker);
            stop_confirmed(self.macos_provider.read().await.as_ref(), broker, &session).await?;
            *self.macos_reserved.lock().await = None;
        }
        Ok(())
    }
    pub(crate) async fn prepare_macos_mode_switch(
        &self,
        profile_id: Uuid,
        cancellation: &CancellationToken,
    ) -> Result<(), ControlServiceError> {
        let active_proxy = self
            .data_plane
            .lock()
            .await
            .as_ref()
            .is_some_and(|active| matches!(active.runtime, crate::ActiveRuntime::Proxy(_)));
        if !active_proxy {
            return Ok(());
        }
        let profile = self
            .config
            .read()
            .await
            .runtime_profile(profile_id)
            .ok_or(ControlServiceError::ProfileNotFound(profile_id))?;
        if !profile.frontends.tunnel {
            return Ok(());
        }
        let provider = Arc::clone(&*self.macos_provider.read().await);
        if !provider_permitted(provider.as_ref()) {
            return Err(ControlServiceError::OperatingModeUnavailable(profile.mode));
        }
        let session = self.reserve_macos_start().await?;
        let broker: Arc<dyn CredentialBroker> =
            Arc::new(usque_macos_runtime::credentials::UnavailableCredentialBroker);
        let mut state = exchange(
            provider.as_ref(),
            broker.clone(),
            &session,
            Command::Protect,
        )
        .await?;
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
        while !state.protection_active {
            if cancellation.is_cancelled() || tokio::time::Instant::now() >= deadline {
                return Err(error("macos_protection_not_confirmed"));
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            state = exchange(
                provider.as_ref(),
                broker.clone(),
                &session,
                Command::Snapshot,
            )
            .await?;
        }
        // Only confirmed independent protection authorizes ending the old
        // Engine-owned channel. Start later reuses exactly this reserved key.
        self.disconnect_locked().await?;
        self.await_disconnect_cleanup().await?;
        *self.session_profile.lock().await = None;
        *self.session_congestion_control.lock().await = None;
        Ok(())
    }
    pub(crate) async fn replace_macos_target(
        &self,
        profile_id: Uuid,
        cancellation: &CancellationToken,
    ) -> Result<Option<usque_core::ConnectionSnapshot>, ControlServiceError> {
        let replace = self
            .data_plane
            .lock()
            .await
            .as_ref()
            .is_some_and(|active| matches!(active.runtime, crate::ActiveRuntime::MacVpn(_)));
        if !replace {
            return Ok(None);
        }
        let mut profile = self
            .config
            .read()
            .await
            .runtime_profile(profile_id)
            .ok_or(ControlServiceError::ProfileNotFound(profile_id))?;
        self.attach_proxy_auth(&mut profile).await?;
        if !profile.frontends.tunnel {
            // Releasing protection for a proxy-only target is an explicit user
            // mode change. Complete teardown before opening a proxy session.
            self.disconnect_locked().await?;
            self.await_disconnect_cleanup().await?;
            return Ok(None);
        }
        let identity = self.load_warp_identity(profile_id).await?;
        let selected = self.prepare_gate_selection(&profile)?;
        let mut active = self.data_plane.lock().await;
        let active = active.as_mut().ok_or_else(|| error("runtime_missing"))?;
        let crate::ActiveRuntime::MacVpn(runtime) = &mut active.runtime else {
            return Ok(None);
        };
        runtime
            .replace(&profile, &identity, selected, cancellation)
            .await?;
        active.profile_id = profile.id;
        active.profile = profile.clone();
        active.frontends = profile.frontends;
        let mut state = self.state.lock().await;
        if matches!(
            state.snapshot().phase,
            usque_core::ConnectionPhase::Connected | usque_core::ConnectionPhase::Degraded
        ) {
            state.transition(usque_core::ConnectionPhase::Reconnecting)?;
        }
        Ok(Some(state.snapshot().clone()))
    }
    pub async fn detach_macos_process(&self) -> Result<(), ControlServiceError> {
        self.macos_detaching
            .store(true, std::sync::atomic::Ordering::Release);
        self.disable_continuous().await;
        self.retire_continuous_startup().await;
        self.abort_exit_probe().await;
        self.clear_network_quality_source().await;
        let _mutation = self.mutation_lock.lock().await;
        let mut active = self.data_plane.lock().await;
        if let Some(active) = active.as_mut()
            && let crate::ActiveRuntime::MacVpn(runtime) = &mut active.runtime
        {
            runtime.detach().await;
            return Ok(());
        }
        drop(active);
        self.disconnect_locked().await?;
        self.await_disconnect_cleanup().await
    }
}

#[cfg(test)]
mod tests;
