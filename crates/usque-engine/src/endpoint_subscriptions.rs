//! Desktop endpoint subscription manager: catalog persistence, bounded downloads,
//! auto-refresh scheduling, tier-isolated activation, and MASQUE endpoint probing.

use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::path::Path;
use std::sync::{
    Arc, Weak,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::json;
use tokio::sync::{Mutex, Semaphore, broadcast};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tokio_util::task::AbortOnDropHandle;
use usque_core::endpoint_subscriptions::{
    EndpointSubscription, EndpointSubscriptionCatalogFile, EndpointSubscriptionCommand,
    EndpointSubscriptionStore, MAX_ENDPOINT_SUBSCRIPTIONS, MAX_SUBSCRIPTION_BODY_BYTES,
    MAX_SUBSCRIPTION_PROBE_TARGETS, ParsedSubscription, SubscriptionEndpoint,
    SubscriptionEndpointHost, SubscriptionError, SubscriptionRefreshInterval, catalog_revision,
    fetch_subscription_direct, parse_subscription_body, sha256_hex, validate_subscription_name,
    validate_subscription_url,
};
use usque_core::network_settings::{
    ApplyStatus, NetworkSettingsPatch, merge_patch, plan_application,
};
use usque_core::{
    AppConfig, ConnectionPhase, DataPlaneMode, EndpointPresetTier, EndpointSelection, Profile,
    ReconfigureClass, Transport, TransportPolicy, storage::StoreError,
};
use usque_ipc::v1;
use usque_transport::{
    MasqueTlsIdentity, NoopSocketProtector, ProbeLeaseTarget, ProbeTransportProtocol,
    SocketProtector, SubscriptionProbeOutcome, probe_resolved_subscription_target,
    probe_subscription_endpoint,
};
use uuid::Uuid;

use crate::{ControlService, ControlServiceError, ControlServiceState};

const FETCH_TOTAL_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_FETCH_REDIRECTS: usize = 3;
const PROBE_CONCURRENCY: usize = 4;
const PROBE_TARGET_TIMEOUT: Duration = Duration::from_millis(3500);
const MAX_AGENT_PROBE_LEASE_TARGETS: usize = 8;
const AUTO_REFRESH_POLL_INTERVAL: Duration = Duration::from_secs(60);
const PROBE_REPLACEMENT_WAIT: Duration = Duration::from_secs(5);
/// Domain candidates probed serially under one Agent lease. Their combined
/// per-target timeouts stay well inside the 15-second lease TTL.
const DOMAIN_CANDIDATES_PER_LEASE: usize = 3;

#[derive(Clone)]
struct ActiveProbeSession {
    probe_id: Uuid,
    subscription_id: Uuid,
    cancel: CancellationToken,
    /// Becomes true (or closes) after the task released every Agent lease.
    finished: tokio::sync::watch::Receiver<bool>,
}

/// Bounded wait for a replaced probe to release its Agent leases. Agents admit
/// one probe lease at a time, so a replacement must never overlap the old one.
async fn await_probe_finished(mut finished: tokio::sync::watch::Receiver<bool>) {
    let _ = tokio::time::timeout(PROBE_REPLACEMENT_WAIT, async {
        while !*finished.borrow_and_update() {
            if finished.changed().await.is_err() {
                break;
            }
        }
    })
    .await;
}

#[derive(Debug, Clone, Serialize)]
struct ActiveProbeInfo {
    probe_id: Uuid,
    subscription_id: Uuid,
}

/// Prior catalogue selection, restored when the account commit fails.
struct SelectionRollback {
    subscription_id: Uuid,
    selected_endpoint_id: Option<String>,
    retained_missing_endpoint: Option<SubscriptionEndpoint>,
}

impl SelectionRollback {
    fn apply(self, store: &EndpointSubscriptionStore) -> Result<(), StoreError> {
        store
            .update::<(), StoreError>(|catalog| {
                if let Some(sub) = catalog
                    .subscriptions
                    .iter_mut()
                    .find(|s| s.id == self.subscription_id)
                {
                    sub.selected_endpoint_id = self.selected_endpoint_id;
                    sub.retained_missing_endpoint = self.retained_missing_endpoint;
                    sub.revision = Uuid::new_v4();
                }
                Ok(())
            })
            .map(|_| ())
    }
}

struct UpdateSubscriptionParams {
    id: Uuid,
    name: String,
    url: String,
    account_tier: EndpointPresetTier,
    auto_refresh: SubscriptionRefreshInterval,
    expected_revision: Uuid,
}

#[derive(Debug, Clone, Serialize)]
struct ProbeEndpointResult {
    endpoint_id: String,
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    rtt_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    protocol: Option<&'static str>,
}

/// In-flight downloads keyed by subscription. Each registration carries a
/// ticket so a finishing task only retires its own token, never a newer one.
#[derive(Default)]
pub(crate) struct RefreshRegistry {
    next_ticket: u64,
    entries: HashMap<Uuid, (u64, CancellationToken)>,
}

impl RefreshRegistry {
    fn register(&mut self, id: Uuid) -> (u64, CancellationToken) {
        self.next_ticket = self.next_ticket.wrapping_add(1);
        let token = CancellationToken::new();
        if let Some((_, previous)) = self.entries.insert(id, (self.next_ticket, token.clone())) {
            previous.cancel();
        }
        (self.next_ticket, token)
    }

    fn finish(&mut self, id: Uuid, ticket: u64) {
        if self
            .entries
            .get(&id)
            .is_some_and(|(current, _)| *current == ticket)
        {
            self.entries.remove(&id);
        }
    }

    fn cancel(&mut self, id: Uuid) {
        if let Some((_, token)) = self.entries.remove(&id) {
            token.cancel();
        }
    }

    fn cancel_all(&mut self) {
        for (_, (_, token)) in self.entries.drain() {
            token.cancel();
        }
    }

    fn ids(&self) -> Vec<Uuid> {
        self.entries.keys().copied().collect()
    }
}

/// Clear All Data increments this epoch while holding the lock. Background
/// downloads commit only under the same lock and only for their own epoch, so
/// a finishing task cannot recreate a catalogue that was just removed.
#[derive(Clone, Default)]
pub(crate) struct StoreEpoch(Arc<std::sync::Mutex<u64>>);

impl StoreEpoch {
    fn current(&self) -> u64 {
        *self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn commit<T>(
        &self,
        expected: u64,
        operation: impl FnOnce() -> Result<T, ControlServiceError>,
    ) -> Result<T, ControlServiceError> {
        let guard = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if *guard != expected {
            return Err(SubscriptionError::Cancelled.into());
        }
        let result = operation();
        drop(guard);
        result
    }

    fn retire<T>(&self, operation: impl FnOnce() -> T) -> T {
        let mut guard = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *guard = guard.wrapping_add(1);
        operation()
    }
}

pub(crate) struct EndpointSubscriptionRuntime {
    store: EndpointSubscriptionStore,
    store_epoch: StoreEpoch,
    refresh_cancels: Mutex<RefreshRegistry>,
    active_probe: Mutex<Option<ActiveProbeSession>>,
    probe_progress_tx: broadcast::Sender<v1::EndpointSubscriptionProbeProgress>,
    auto_refresh_started: AtomicBool,
    auto_refresh_task: Mutex<Option<AbortOnDropHandle<()>>>,
}

impl EndpointSubscriptionRuntime {
    pub(crate) fn new(
        profile_dir: &Path,
        probe_progress_tx: broadcast::Sender<v1::EndpointSubscriptionProbeProgress>,
    ) -> Self {
        Self {
            store: EndpointSubscriptionStore::in_directory(profile_dir),
            store_epoch: StoreEpoch::default(),
            refresh_cancels: Mutex::new(RefreshRegistry::default()),
            active_probe: Mutex::new(None),
            probe_progress_tx,
            auto_refresh_started: AtomicBool::new(false),
            auto_refresh_task: Mutex::new(None),
        }
    }

    pub(crate) fn store(&self) -> &EndpointSubscriptionStore {
        &self.store
    }

    pub(crate) async fn cancel_active_tasks_and_clear(&self) -> Result<(), ControlServiceError> {
        self.refresh_cancels.lock().await.cancel_all();
        if let Some(session) = self.active_probe.lock().await.take() {
            session.cancel.cancel();
        }
        let store = self.store.clone();
        let epoch = self.store_epoch.clone();
        tokio::task::spawn_blocking(move || epoch.retire(|| store.clear()))
            .await
            .map_err(|err| ControlServiceError::PersistenceWorker(err.to_string()))??;
        Ok(())
    }
}

fn unix_ms_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub(crate) fn hydrate_profile_with_store(
    store: &EndpointSubscriptionStore,
    profile: &mut Profile,
    tier: Option<EndpointPresetTier>,
) -> Result<(), SubscriptionError> {
    let Some(sub_id) = profile.endpoint.subscription_id else {
        profile.subscription_endpoint = None;
        return Ok(());
    };
    let catalog = store
        .load_or_default()
        .map_err(|_| SubscriptionError::StorageUnavailable)?;
    let resolved = catalog.resolve_target(sub_id, tier)?;
    profile.subscription_endpoint = Some(resolved.endpoint);
    profile.endpoint.selection = EndpointSelection::Custom;
    profile.endpoint.preset_region = None;
    Ok(())
}

pub(crate) fn validate_scenario_subscription_at(
    profile_dir: &Path,
    profile: &Profile,
    tier: Option<EndpointPresetTier>,
) -> Result<Option<String>, ControlServiceError> {
    let Some(sub_id) = profile.endpoint.subscription_id else {
        return Ok(None);
    };
    let store = EndpointSubscriptionStore::in_directory(profile_dir);
    let catalog = store
        .load_or_default()
        .map_err(|_| ControlServiceError::ScenarioUnavailable("SCENARIO_DEPENDENCY_UNAVAILABLE"))?;
    let resolved = catalog
        .resolve_target(sub_id, tier)
        .map_err(|_| ControlServiceError::ScenarioUnavailable("SCENARIO_DEPENDENCY_UNAVAILABLE"))?;
    let rev = catalog
        .subscriptions
        .iter()
        .find(|s| s.id == sub_id)
        .map(|s| s.revision)
        .unwrap_or_else(Uuid::nil);
    Ok(Some(format!("sub:{sub_id}:{rev}:{}", resolved.endpoint.id)))
}

struct ListRevisionState<'a> {
    active_account_id: Option<Uuid>,
    active_tier: Option<EndpointPresetTier>,
    active_subscription_id: Option<Uuid>,
    active_endpoint_id: Option<&'a str>,
    refreshing_ids: &'a [Uuid],
    active_probe: Option<&'a ActiveProbeInfo>,
}

fn list_revision(
    catalog: &EndpointSubscriptionCatalogFile,
    state_view: ListRevisionState<'_>,
) -> String {
    let base = catalog_revision(catalog);
    let state = serde_json::to_vec(&(
        base,
        state_view.active_account_id,
        state_view.active_tier,
        state_view.active_subscription_id,
        state_view.active_endpoint_id,
        state_view.refreshing_ids,
        state_view.active_probe,
    ))
    .unwrap_or_default();
    sha256_hex(&state)
}

fn probe_outcome_to_result(
    endpoint_id: String,
    outcome: SubscriptionProbeOutcome,
) -> ProbeEndpointResult {
    match outcome {
        SubscriptionProbeOutcome::Passed {
            milliseconds,
            transport,
        } => ProbeEndpointResult {
            endpoint_id,
            status: "reachable",
            rtt_ms: Some(milliseconds),
            protocol: Some(match transport {
                Transport::Http3 => "http3",
                Transport::Http2 => "http2",
            }),
        },
        SubscriptionProbeOutcome::PinMismatch => ProbeEndpointResult {
            endpoint_id,
            status: "pin_mismatch",
            rtt_ms: None,
            protocol: None,
        },
        SubscriptionProbeOutcome::TimedOut => ProbeEndpointResult {
            endpoint_id,
            status: "timed_out",
            rtt_ms: None,
            protocol: None,
        },
        SubscriptionProbeOutcome::Cancelled => ProbeEndpointResult {
            endpoint_id,
            status: "cancelled",
            rtt_ms: None,
            protocol: None,
        },
        SubscriptionProbeOutcome::NetworkChanged => ProbeEndpointResult {
            endpoint_id,
            status: "network_changed",
            rtt_ms: None,
            protocol: None,
        },
        SubscriptionProbeOutcome::Failed => ProbeEndpointResult {
            endpoint_id,
            status: "failed",
            rtt_ms: None,
            protocol: None,
        },
    }
}

fn target_protocols(
    transport: TransportPolicy,
    data_plane: DataPlaneMode,
) -> &'static [ProbeTransportProtocol] {
    if data_plane == DataPlaneMode::L4Proxy || transport == TransportPolicy::Http3 {
        &[ProbeTransportProtocol::Udp]
    } else if transport == TransportPolicy::Http2 {
        &[ProbeTransportProtocol::Tcp]
    } else {
        &[ProbeTransportProtocol::Udp, ProbeTransportProtocol::Tcp]
    }
}

impl ControlService {
    pub(crate) fn subscribe_endpoint_subscription_probe_progress(
        &self,
    ) -> broadcast::Receiver<v1::EndpointSubscriptionProbeProgress> {
        self.endpoint_subscriptions.probe_progress_tx.subscribe()
    }

    /// Starts due-subscription refreshes with the Engine process, so an
    /// auto-connected session refreshes without waiting for a GUI request.
    pub fn start_endpoint_subscription_scheduler(&self) {
        self.ensure_subscription_auto_refresh();
    }

    pub(crate) fn ensure_subscription_auto_refresh(&self) {
        if self
            .endpoint_subscriptions
            .auto_refresh_started
            .swap(true, Ordering::SeqCst)
        {
            return;
        }
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            self.endpoint_subscriptions
                .auto_refresh_started
                .store(false, Ordering::SeqCst);
            return;
        };
        let weak: Weak<ControlServiceState> = Arc::downgrade(&self.inner);
        let task = AbortOnDropHandle::new(handle.spawn(async move {
            let mut ticker = tokio::time::interval(AUTO_REFRESH_POLL_INTERVAL);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                ticker.tick().await;
                let Some(inner) = weak.upgrade() else {
                    break;
                };
                let service = ControlService { inner };
                service.run_due_auto_refreshes().await;
            }
        }));
        if let Ok(mut guard) = self.endpoint_subscriptions.auto_refresh_task.try_lock() {
            *guard = Some(task);
        }
    }

    pub(crate) async fn sync_account_entitlement(&self, profile_id: Uuid) {
        let should_check = {
            let config = self.config.read().await;
            matches!(
                config.identity_bindings.get(&profile_id),
                Some(usque_core::IdentityProvider::Consumer)
            ) && !config.consumer_entitlements.contains_key(&profile_id)
        };
        if !should_check {
            return;
        }
        if let Ok(metadata) = self.load_identity_metadata(profile_id).await
            && metadata.entitlement.is_some()
        {
            let _ = self
                .update_config(move |latest| {
                    latest.set_consumer_entitlement(profile_id, metadata.entitlement);
                    Ok(())
                })
                .await;
        }
    }

    pub(crate) async fn hydrate_profile_subscription_endpoint(
        &self,
        profile: &mut Profile,
    ) -> Result<(), ControlServiceError> {
        self.sync_account_entitlement(profile.id).await;
        if profile.endpoint.subscription_id.is_none() {
            profile.subscription_endpoint = None;
            return Ok(());
        }
        let tier = self.config.read().await.account_tier(profile.id);
        let store = self.endpoint_subscriptions.store.clone();
        let mut copy = profile.clone();
        let hydrated = tokio::task::spawn_blocking(move || {
            hydrate_profile_with_store(&store, &mut copy, tier)?;
            Ok::<Profile, SubscriptionError>(copy)
        })
        .await
        .map_err(|err| ControlServiceError::PersistenceWorker(err.to_string()))??;
        *profile = hydrated;
        Ok(())
    }

    pub(crate) async fn current_subscription_endpoint(
        &self,
        config: &AppConfig,
    ) -> Option<SubscriptionEndpoint> {
        let active_id = config.active_profile_id?;
        let profile = config.runtime_profile(active_id)?;
        let sub_id = profile.endpoint.subscription_id?;
        let tier = config.account_tier(active_id);
        let store = self.endpoint_subscriptions.store.clone();
        tokio::task::spawn_blocking(move || {
            store
                .load_or_default()
                .ok()
                .and_then(|catalog| catalog.resolve_target(sub_id, tier).ok())
                .map(|resolved| resolved.endpoint)
        })
        .await
        .ok()
        .flatten()
    }

    async fn load_catalog(&self) -> Result<EndpointSubscriptionCatalogFile, ControlServiceError> {
        let store = self.endpoint_subscriptions.store.clone();
        let catalog = tokio::task::spawn_blocking(move || store.load_or_default())
            .await
            .map_err(|err| ControlServiceError::PersistenceWorker(err.to_string()))??;
        Ok(catalog)
    }

    /// Chooses the only permitted download path. Connected sessions must use
    /// the WARP tunnel; a disconnected Engine may use the physical network only
    /// while no data plane or retained platform protection exists. Every other
    /// phase fails closed instead of leaving the tunnel or Kill Switch.
    async fn subscription_fetch_route(
        &self,
    ) -> Result<Option<usque_transport::InternalNetwork>, SubscriptionError> {
        let phase = self.state.lock().await.snapshot().phase;
        let (data_plane_present, warp) = {
            let guard = self.data_plane.lock().await;
            (
                guard.is_some(),
                guard
                    .as_ref()
                    .filter(|active| {
                        matches!(
                            active.runtime.health(),
                            usque_transport::RuntimeHealth::Connected { .. }
                        )
                    })
                    .and_then(|active| active.runtime.internal_networks())
                    .map(|(_, warp)| warp),
            )
        };
        match phase {
            ConnectionPhase::Connected | ConnectionPhase::Degraded => warp
                .map(Some)
                .ok_or(SubscriptionError::ProtectedPathUnavailable),
            ConnectionPhase::Disconnected
                if !data_plane_present && self.subscription_physical_fetch_permitted().await =>
            {
                Ok(None)
            }
            _ => Err(SubscriptionError::ProtectedPathUnavailable),
        }
    }

    /// The same platform guards as VPN Gate catalogue refreshes: a retained
    /// Agent policy, pending recovery or reserved session keeps physical
    /// downloads unavailable.
    async fn subscription_physical_fetch_permitted(&self) -> bool {
        // Unit tests never query a production Agent on the host.
        #[cfg(all(windows, not(test)))]
        {
            if self.windows_recovery.lock().await.pending.is_some() {
                return false;
            }
            crate::windows_agent::catalogue_physical_network_permitted().await
        }
        #[cfg(all(target_os = "linux", not(test)))]
        {
            crate::linux_observation::disconnected_probe_safe().await
        }
        #[cfg(target_os = "macos")]
        {
            self.macos_reserved.lock().await.is_none()
                && self.macos_proxy_recovery_error.lock().await.is_none()
        }
        #[cfg(not(any(
            all(windows, not(test)),
            target_os = "macos",
            all(target_os = "linux", not(test))
        )))]
        {
            true
        }
    }

    async fn fetch_and_parse_subscription(
        &self,
        url: &str,
        cancel: &CancellationToken,
    ) -> Result<ParsedSubscription, SubscriptionError> {
        let bytes = match self.subscription_fetch_route().await? {
            Some(warp) => warp
                .get_https_bounded(
                    url,
                    MAX_SUBSCRIPTION_BODY_BYTES,
                    MAX_FETCH_REDIRECTS,
                    FETCH_TOTAL_TIMEOUT,
                    cancel,
                )
                .await
                .map_err(|err| err.to_subscription_error())?,
            None => fetch_subscription_direct(
                url,
                MAX_SUBSCRIPTION_BODY_BYTES,
                MAX_FETCH_REDIRECTS,
                FETCH_TOTAL_TIMEOUT,
                cancel,
            )
            .await
            .map_err(|err| err.to_subscription_error())?,
        };
        let parsed = parse_subscription_body(&bytes)?;
        if parsed.endpoints.is_empty() {
            return Err(SubscriptionError::Empty);
        }
        Ok(parsed)
    }

    async fn refresh_subscription_by_id(
        &self,
        id: Uuid,
    ) -> Result<EndpointSubscription, ControlServiceError> {
        let catalog = self.load_catalog().await?;
        let url = catalog
            .subscriptions
            .iter()
            .find(|sub| sub.id == id)
            .map(|sub| sub.url.clone())
            .ok_or(SubscriptionError::NotFound)?;

        let epoch = self.endpoint_subscriptions.store_epoch.clone();
        let started_epoch = epoch.current();
        let (ticket, cancel) = self
            .endpoint_subscriptions
            .refresh_cancels
            .lock()
            .await
            .register(id);

        let fetched = self.fetch_and_parse_subscription(&url, &cancel).await;

        self.endpoint_subscriptions
            .refresh_cancels
            .lock()
            .await
            .finish(id, ticket);

        let now_ms = unix_ms_now();
        let store = self.endpoint_subscriptions.store.clone();
        match fetched {
            Ok(parsed) => {
                let (_, updated) = tokio::task::spawn_blocking(move || {
                    epoch.commit(started_epoch, || {
                        store.update::<EndpointSubscription, ControlServiceError>(|catalog| {
                            let Some(sub) = catalog.subscriptions.iter_mut().find(|s| s.id == id)
                            else {
                                return Err(SubscriptionError::NotFound.into());
                            };
                            sub.reconcile_endpoints(parsed, now_ms);
                            Ok(sub.clone())
                        })
                    })
                })
                .await
                .map_err(|err| ControlServiceError::PersistenceWorker(err.to_string()))??;
                Ok(updated)
            }
            // A cancelled or fail-closed download is not a server failure and
            // must not schedule a backoff for the next protected attempt.
            Err(
                err @ (SubscriptionError::Cancelled | SubscriptionError::ProtectedPathUnavailable),
            ) => Err(err.into()),
            Err(err) => {
                let code = err.code().to_owned();
                let _ = tokio::task::spawn_blocking(move || {
                    epoch.commit(started_epoch, || {
                        store.update::<(), ControlServiceError>(|catalog| {
                            if let Some(sub) = catalog.subscriptions.iter_mut().find(|s| s.id == id)
                            {
                                sub.record_refresh_failure(&code, now_ms);
                            }
                            Ok(())
                        })
                    })
                })
                .await;
                Err(err.into())
            }
        }
    }

    async fn run_due_auto_refreshes(&self) {
        let Ok(catalog) = self.load_catalog().await else {
            return;
        };
        let now_ms = unix_ms_now();
        let due_ids: Vec<Uuid> = catalog
            .subscriptions
            .iter()
            .filter(|sub| {
                sub.auto_refresh != SubscriptionRefreshInterval::Off
                    && sub
                        .next_refresh_at_unix_ms
                        .is_none_or(|next| now_ms >= next)
            })
            .map(|sub| sub.id)
            .collect();
        for id in due_ids {
            // Retry on a later tick, after the protected path returns.
            if let Err(ControlServiceError::EndpointSubscription(
                SubscriptionError::ProtectedPathUnavailable | SubscriptionError::Cancelled,
            )) = self.refresh_subscription_by_id(id).await
            {
                break;
            }
        }
    }

    async fn build_list_json(
        &self,
        known_revision: Option<String>,
    ) -> Result<serde_json::Value, ControlServiceError> {
        let config = self.config.read().await.clone();
        let active_id = config.active_profile_id;
        if let Some(id) = active_id {
            drop(config);
            self.sync_account_entitlement(id).await;
        }
        let config = self.config.read().await.clone();
        let active_tier = active_id.and_then(|id| config.account_tier(id));
        let active_profile = config.active_profile();
        let active_subscription_id = active_profile
            .as_ref()
            .and_then(|profile| profile.endpoint.subscription_id);

        let catalog = self.load_catalog().await?;
        let active_endpoint_id = active_subscription_id.and_then(|sub_id| {
            catalog
                .subscriptions
                .iter()
                .find(|s| s.id == sub_id)
                .and_then(|s| s.active_endpoint().map(|(ep, _)| ep.id.clone()))
        });
        let mut refreshing_ids: Vec<Uuid> = self
            .endpoint_subscriptions
            .refresh_cancels
            .lock()
            .await
            .ids();
        refreshing_ids.sort();
        let active_probe = self
            .endpoint_subscriptions
            .active_probe
            .lock()
            .await
            .as_ref()
            .map(|p| ActiveProbeInfo {
                probe_id: p.probe_id,
                subscription_id: p.subscription_id,
            });
        let revision = list_revision(
            &catalog,
            ListRevisionState {
                active_account_id: active_id,
                active_tier,
                active_subscription_id,
                active_endpoint_id: active_endpoint_id.as_deref(),
                refreshing_ids: &refreshing_ids,
                active_probe: active_probe.as_ref(),
            },
        );
        let unchanged = known_revision.as_deref() == Some(revision.as_str());
        Ok(json!({
            "revision": revision,
            "unchanged": unchanged,
            "subscriptions": (!unchanged).then_some(&catalog.subscriptions),
            "active_account_id": active_id,
            "active_account_tier": active_tier,
            "active_subscription_id": active_subscription_id,
            "active_endpoint_id": active_endpoint_id,
            "refreshing_subscription_ids": refreshing_ids,
            "active_probe": active_probe,
        }))
    }

    pub(crate) async fn endpoint_subscription_command(
        &self,
        request: v1::EndpointSubscriptionRequest,
    ) -> Result<v1::EndpointSubscriptionResponse, ControlServiceError> {
        self.ensure_subscription_auto_refresh();
        let command = EndpointSubscriptionCommand::parse(&request.command_json)?;
        let metadata = match command {
            EndpointSubscriptionCommand::List { known_revision } => {
                self.build_list_json(known_revision).await?
            }
            EndpointSubscriptionCommand::Add {
                name,
                url,
                account_tier,
                auto_refresh,
            } => {
                self.add_subscription(name, url, account_tier, auto_refresh)
                    .await?
            }
            EndpointSubscriptionCommand::Update {
                id,
                name,
                url,
                account_tier,
                auto_refresh,
                expected_revision,
            } => {
                self.update_subscription(UpdateSubscriptionParams {
                    id,
                    name,
                    url,
                    account_tier,
                    auto_refresh,
                    expected_revision,
                })
                .await?
            }
            EndpointSubscriptionCommand::Delete {
                id,
                expected_revision,
            } => self.delete_subscription(id, expected_revision).await?,
            EndpointSubscriptionCommand::Refresh { id } => {
                let updated = self.refresh_subscription_by_id(id).await?;
                let mut payload = self.build_list_json(None).await?;
                payload["subscription"] =
                    serde_json::to_value(updated).map_err(|_| SubscriptionError::InvalidBody)?;
                payload
            }
            EndpointSubscriptionCommand::CancelRefresh { id } => {
                self.endpoint_subscriptions
                    .refresh_cancels
                    .lock()
                    .await
                    .cancel(id);
                self.build_list_json(None).await?
            }
            EndpointSubscriptionCommand::SelectEndpoint {
                subscription_id,
                endpoint_id,
                operation_id,
                zero_trust_risk_acknowledged,
            } => {
                self.select_subscription_endpoint(
                    subscription_id,
                    endpoint_id,
                    operation_id,
                    zero_trust_risk_acknowledged,
                )
                .await?
            }
            EndpointSubscriptionCommand::Deactivate { operation_id } => {
                self.deactivate_subscription(operation_id).await?
            }
            EndpointSubscriptionCommand::SetPinned {
                subscription_id,
                endpoint_id,
                pinned,
            } => {
                let store = self.endpoint_subscriptions.store.clone();
                tokio::task::spawn_blocking(move || {
                    store.update::<(), ControlServiceError>(|catalog| {
                        let sub = catalog
                            .subscriptions
                            .iter_mut()
                            .find(|s| s.id == subscription_id)
                            .ok_or(SubscriptionError::NotFound)?;
                        let ep = sub
                            .endpoints
                            .iter_mut()
                            .chain(sub.retained_missing_endpoint.iter_mut())
                            .find(|ep| ep.id == endpoint_id)
                            .ok_or(SubscriptionError::EndpointNotFound)?;
                        ep.pinned = pinned;
                        sub.revision = Uuid::new_v4();
                        Ok(())
                    })
                })
                .await
                .map_err(|err| ControlServiceError::PersistenceWorker(err.to_string()))??;
                self.build_list_json(None).await?
            }
            EndpointSubscriptionCommand::StartProbe {
                subscription_id,
                endpoint_ids,
                probe_id,
            } => {
                self.start_probe(subscription_id, endpoint_ids, probe_id)
                    .await?
            }
            EndpointSubscriptionCommand::CancelProbe { probe_id } => {
                let guard = self.endpoint_subscriptions.active_probe.lock().await;
                if let Some(session) = guard.as_ref()
                    && session.probe_id == probe_id
                {
                    session.cancel.cancel();
                }
                drop(guard);
                self.build_list_json(None).await?
            }
        };
        Ok(v1::EndpointSubscriptionResponse {
            metadata_json: serde_json::to_string(&metadata)
                .map_err(|_| ControlServiceError::configuration("invalid subscription JSON"))?,
        })
    }

    async fn add_subscription(
        &self,
        name: String,
        url: String,
        account_tier: EndpointPresetTier,
        auto_refresh: SubscriptionRefreshInterval,
    ) -> Result<serde_json::Value, ControlServiceError> {
        let name = validate_subscription_name(&name)?;
        let url = validate_subscription_url(&url)?;
        if self.load_catalog().await?.subscriptions.len() >= MAX_ENDPOINT_SUBSCRIPTIONS {
            return Err(SubscriptionError::Limit.into());
        }

        let id = Uuid::new_v4();
        let epoch = self.endpoint_subscriptions.store_epoch.clone();
        let started_epoch = epoch.current();
        let (ticket, cancel) = self
            .endpoint_subscriptions
            .refresh_cancels
            .lock()
            .await
            .register(id);
        let fetched = self.fetch_and_parse_subscription(&url, &cancel).await;
        self.endpoint_subscriptions
            .refresh_cancels
            .lock()
            .await
            .finish(id, ticket);
        let parsed = fetched?;

        let now_ms = unix_ms_now();
        let next_refresh_at_unix_ms = auto_refresh
            .interval_ms()
            .map(|interval| now_ms.saturating_add(interval));
        let selected_endpoint_id = parsed.endpoints.first().map(|ep| ep.id.clone());
        let subscription = EndpointSubscription {
            id,
            revision: Uuid::new_v4(),
            name,
            url,
            account_tier,
            auto_refresh,
            last_refreshed_at_unix_ms: Some(now_ms),
            next_refresh_at_unix_ms,
            last_refresh_error: None,
            skipped_line_count: parsed.skipped_line_count,
            endpoints: parsed.endpoints,
            selected_endpoint_id,
            retained_missing_endpoint: None,
        };
        let store = self.endpoint_subscriptions.store.clone();
        let to_insert = subscription.clone();
        tokio::task::spawn_blocking(move || {
            epoch.commit(started_epoch, || {
                store.update::<(), ControlServiceError>(|catalog| {
                    if catalog.subscriptions.len() >= MAX_ENDPOINT_SUBSCRIPTIONS {
                        return Err(SubscriptionError::Limit.into());
                    }
                    catalog.subscriptions.push(to_insert);
                    Ok(())
                })
            })
        })
        .await
        .map_err(|err| ControlServiceError::PersistenceWorker(err.to_string()))??;

        let mut payload = self.build_list_json(None).await?;
        payload["subscription"] =
            serde_json::to_value(subscription).map_err(|_| SubscriptionError::InvalidBody)?;
        Ok(payload)
    }

    async fn update_subscription(
        &self,
        params: UpdateSubscriptionParams,
    ) -> Result<serde_json::Value, ControlServiceError> {
        let UpdateSubscriptionParams {
            id,
            name,
            url,
            account_tier,
            auto_refresh,
            expected_revision,
        } = params;
        let name = validate_subscription_name(&name)?;
        let url = validate_subscription_url(&url)?;
        let existing = self
            .load_catalog()
            .await?
            .subscriptions
            .into_iter()
            .find(|sub| sub.id == id)
            .ok_or(SubscriptionError::NotFound)?;
        if existing.revision != expected_revision {
            return Err(SubscriptionError::StaleRevision.into());
        }

        let epoch = self.endpoint_subscriptions.store_epoch.clone();
        let started_epoch = epoch.current();
        let parsed = if existing.url != url {
            let (ticket, cancel) = self
                .endpoint_subscriptions
                .refresh_cancels
                .lock()
                .await
                .register(id);
            let result = self.fetch_and_parse_subscription(&url, &cancel).await;
            self.endpoint_subscriptions
                .refresh_cancels
                .lock()
                .await
                .finish(id, ticket);
            Some(result?)
        } else {
            None
        };

        let now_ms = unix_ms_now();
        let store = self.endpoint_subscriptions.store.clone();
        let (_, (updated, tier_changed)) = tokio::task::spawn_blocking(move || {
            epoch.commit(started_epoch, || {
                store.update::<(EndpointSubscription, bool), ControlServiceError>(|catalog| {
                    let sub = catalog
                        .subscriptions
                        .iter_mut()
                        .find(|sub| sub.id == id)
                        .ok_or(SubscriptionError::NotFound)?;
                    if sub.revision != expected_revision {
                        return Err(SubscriptionError::StaleRevision.into());
                    }
                    let tier_changed = sub.account_tier != account_tier;
                    sub.name = name;
                    sub.url = url;
                    sub.account_tier = account_tier;
                    sub.auto_refresh = auto_refresh;
                    if let Some(parsed) = parsed {
                        sub.reconcile_endpoints(parsed, now_ms);
                    } else {
                        sub.next_refresh_at_unix_ms = auto_refresh
                            .interval_ms()
                            .map(|interval| now_ms.saturating_add(interval));
                        sub.revision = Uuid::new_v4();
                    }
                    Ok((sub.clone(), tier_changed))
                })
            })
        })
        .await
        .map_err(|err| ControlServiceError::PersistenceWorker(err.to_string()))??;

        if tier_changed {
            self.cleanup_subscription_references(id, Some(account_tier))
                .await?;
        }

        let mut payload = self.build_list_json(None).await?;
        payload["subscription"] =
            serde_json::to_value(updated).map_err(|_| SubscriptionError::InvalidBody)?;
        Ok(payload)
    }

    async fn delete_subscription(
        &self,
        id: Uuid,
        expected_revision: Uuid,
    ) -> Result<serde_json::Value, ControlServiceError> {
        self.endpoint_subscriptions
            .refresh_cancels
            .lock()
            .await
            .cancel(id);
        {
            let guard = self.endpoint_subscriptions.active_probe.lock().await;
            if let Some(session) = guard.as_ref()
                && session.subscription_id == id
            {
                session.cancel.cancel();
            }
        }
        let store = self.endpoint_subscriptions.store.clone();
        tokio::task::spawn_blocking(move || {
            store.update::<(), ControlServiceError>(|catalog| {
                let idx = catalog
                    .subscriptions
                    .iter()
                    .position(|s| s.id == id)
                    .ok_or(SubscriptionError::NotFound)?;
                if catalog.subscriptions[idx].revision != expected_revision {
                    return Err(SubscriptionError::StaleRevision.into());
                }
                catalog.subscriptions.remove(idx);
                Ok(())
            })
        })
        .await
        .map_err(|err| ControlServiceError::PersistenceWorker(err.to_string()))??;

        self.cleanup_subscription_references(id, None).await?;
        self.build_list_json(None).await
    }

    async fn cleanup_subscription_references(
        &self,
        subscription_id: Uuid,
        keep_tier: Option<EndpointPresetTier>,
    ) -> Result<(), ControlServiceError> {
        let _submission = self.settings_submission.lock().await;
        let observed_intent = self.settings_intent.load(Ordering::SeqCst);
        let runtime = self.data_plane.try_lock().ok().and_then(|active| {
            active.as_ref().map(|active| {
                (
                    active.profile.clone(),
                    active.session_generation,
                    active.runtime.health(),
                )
            })
        });
        let phase = self.state.lock().await.snapshot().phase;
        let lifecycle = Arc::clone(&self.mutation_lock).try_lock_owned().ok();

        let (stored, shared, cleared_active) = {
            let mut config_guard = self.config.write().await;
            let store = self.store.clone();
            let next = tokio::task::spawn_blocking(move || {
                store.update::<bool, ControlServiceError>(|config| {
                    let mut touched = false;
                    if keep_tier != Some(EndpointPresetTier::Free)
                        && config.network.consumer_tier_subscriptions.free == Some(subscription_id)
                    {
                        config.network.consumer_tier_subscriptions.free = None;
                        touched = true;
                    }
                    if keep_tier != Some(EndpointPresetTier::Plus)
                        && config.network.consumer_tier_subscriptions.plus == Some(subscription_id)
                    {
                        config.network.consumer_tier_subscriptions.plus = None;
                        touched = true;
                    }
                    if keep_tier != Some(EndpointPresetTier::ZeroTrust) {
                        for account in &mut config.profiles {
                            if account.zero_trust_subscription_id == Some(subscription_id) {
                                account.zero_trust_subscription_id = None;
                                touched = true;
                            }
                        }
                    }
                    Ok(touched)
                })
            })
            .await
            .map_err(|err| ControlServiceError::PersistenceWorker(err.to_string()))??;
            let (updated_config, cleared) = next;
            let stored = updated_config.active_profile();
            let shared = updated_config
                .network
                .hydrate(&usque_core::config::Account::default_account());
            *config_guard = updated_config;
            (stored, shared, cleared)
        };

        if let Some(mut stored) = stored {
            let _ = self
                .hydrate_profile_subscription_endpoint(&mut stored)
                .await;
            let confirmed = matches!(
                phase,
                ConnectionPhase::Connected | ConnectionPhase::Degraded
            );
            let plan = if cleared_active
                && runtime
                    .as_ref()
                    .is_some_and(|(p, _, _)| p.endpoint.subscription_id == Some(subscription_id))
            {
                plan_application(
                    runtime.as_ref().filter(|_| confirmed).map(|(p, _, _)| p),
                    &stored,
                    &[String::from("endpoint.subscription_id")],
                    phase,
                    lifecycle.is_some()
                        && self.settings_intent.load(Ordering::SeqCst) == observed_intent,
                )
                .ok()
            } else {
                None
            };
            // A cleared active binding is a new settings operation; earlier
            // intents must not publish over it.
            let intent = if plan.is_some() {
                self.settings_intent.fetch_add(1, Ordering::SeqCst) + 1
            } else {
                observed_intent
            };
            let operation_id = Uuid::new_v4();
            let mut state = self.settings.lock().await;
            state.stored_profile = Some(stored.clone());
            state.shared_network_profile = Some(shared);
            if let Some(ref plan) = plan {
                state.operation_id = Some(operation_id);
                state.persisted = Some(true);
                state.record_persisted_operation(operation_id);
                state.apply_status = plan.status;
            }
            state.advance();
            self.settings_tx.send_replace(state.sequence);
            drop(state);

            if let (Some(plan), Some(lifecycle), Some((previous, generation, _))) =
                (plan, lifecycle, runtime)
                && let Some(target) = plan.target
            {
                // Shares the selection path's completion handling, including
                // failure reporting and a session generation renewed by a
                // cold reconnect.
                self.spawn_subscription_reconnect(
                    target,
                    previous,
                    plan.class,
                    generation,
                    intent,
                    operation_id,
                    lifecycle,
                );
            }
        }
        Ok(())
    }

    async fn select_subscription_endpoint(
        &self,
        subscription_id: Uuid,
        endpoint_id: String,
        operation_id: Uuid,
        zero_trust_risk_acknowledged: bool,
    ) -> Result<serde_json::Value, ControlServiceError> {
        if operation_id.is_nil() {
            return Err(SubscriptionError::InvalidRequest.into());
        }
        // Every check below and both commits run inside the settings
        // transaction, so an account switch, tier change or scenario
        // application cannot interleave with this binding.
        let _submission = self.settings_submission.lock().await;
        if self.scenario_applying.load(Ordering::SeqCst) {
            return Err(ControlServiceError::ScenarioUnavailable("SCENARIO_BUSY"));
        }
        let active_id = self
            .config
            .read()
            .await
            .active_profile_id
            .ok_or_else(|| ControlServiceError::InvalidRequest("no active account".into()))?;
        self.sync_account_entitlement(active_id).await;

        let (tier, previous_sub_id) = {
            let config = self.config.read().await;
            if config.active_profile_id != Some(active_id) {
                return Err(SubscriptionError::VerifyAccount.into());
            }
            let tier = config
                .account_tier(active_id)
                .ok_or(SubscriptionError::VerifyAccount)?;
            let prev = config
                .runtime_profile(active_id)
                .and_then(|p| p.endpoint.subscription_id);
            (tier, prev)
        };

        let catalog = self.load_catalog().await?;
        let sub = catalog
            .subscriptions
            .iter()
            .find(|s| s.id == subscription_id)
            .ok_or(SubscriptionError::NotFound)?;
        if sub.account_tier != tier {
            return Err(SubscriptionError::TierMismatch.into());
        }
        if !sub
            .endpoints
            .iter()
            .chain(sub.retained_missing_endpoint.iter())
            .any(|ep| ep.id == endpoint_id)
        {
            return Err(SubscriptionError::EndpointNotFound.into());
        }
        if tier == EndpointPresetTier::ZeroTrust
            && previous_sub_id != Some(subscription_id)
            && !zero_trust_risk_acknowledged
        {
            return Err(SubscriptionError::ZeroTrustRiskRequired.into());
        }

        let lifecycle = Arc::clone(&self.mutation_lock).try_lock_owned().ok();
        let runtime = self.data_plane.try_lock().ok().and_then(|active| {
            active.as_ref().map(|active| {
                (
                    active.profile.clone(),
                    active.session_generation,
                    active.runtime.health(),
                )
            })
        });
        let phase = self.state.lock().await.snapshot().phase;
        let confirmed = matches!(
            phase,
            ConnectionPhase::Connected | ConnectionPhase::Degraded
        );

        let store = self.endpoint_subscriptions.store.clone();
        let endpoint_id_copy = endpoint_id.clone();
        let (_, (selected_ep, rollback)) = tokio::task::spawn_blocking(move || {
            store.update::<(SubscriptionEndpoint, SelectionRollback), ControlServiceError>(
                |catalog| {
                    let sub = catalog
                        .subscriptions
                        .iter_mut()
                        .find(|s| s.id == subscription_id)
                        .ok_or(SubscriptionError::NotFound)?;
                    // The subscription itself may have been re-tiered after
                    // the unlocked catalogue read above.
                    if sub.account_tier != tier {
                        return Err(SubscriptionError::TierMismatch.into());
                    }
                    let ep = sub
                        .endpoints
                        .iter()
                        .find(|ep| ep.id == endpoint_id_copy)
                        .or_else(|| {
                            sub.retained_missing_endpoint
                                .as_ref()
                                .filter(|ep| ep.id == endpoint_id_copy)
                        })
                        .cloned()
                        .ok_or(SubscriptionError::EndpointNotFound)?;
                    let rollback = SelectionRollback {
                        subscription_id,
                        selected_endpoint_id: sub.selected_endpoint_id.clone(),
                        retained_missing_endpoint: sub.retained_missing_endpoint.clone(),
                    };
                    sub.selected_endpoint_id = Some(ep.id.clone());
                    if sub.endpoints.iter().any(|item| item.id == ep.id) {
                        sub.retained_missing_endpoint = None;
                    }
                    sub.revision = Uuid::new_v4();
                    Ok((ep, rollback))
                },
            )
        })
        .await
        .map_err(|err| ControlServiceError::PersistenceWorker(err.to_string()))??;

        let committed: Result<Profile, ControlServiceError> =
            if previous_sub_id != Some(subscription_id) {
                let mut config_guard = self.config.write().await;
                let cfg_store = self.store.clone();
                let mut patch_profile = config_guard
                    .runtime_profile(active_id)
                    .ok_or(ControlServiceError::ProfileNotFound(active_id))?;
                patch_profile.endpoint.subscription_id = Some(subscription_id);
                patch_profile.endpoint.selection = EndpointSelection::Custom;
                patch_profile.endpoint.preset_region = None;
                patch_profile.subscription_endpoint = Some(selected_ep.clone());
                let patch = NetworkSettingsPatch {
                    operation_id,
                    account_id: active_id,
                    values: patch_profile,
                    changed_fields: vec!["endpoint.subscription_id".into()],
                };
                let result = tokio::task::spawn_blocking(move || {
                    cfg_store.update(|latest| {
                        // merge_patch rejects a changed active account; the tier
                        // must also still match the validated subscription.
                        if latest.account_tier(active_id) != Some(tier) {
                            return Err(StoreError::NetworkSettings(
                                SubscriptionError::TierMismatch.code().into(),
                            ));
                        }
                        merge_patch(latest, &patch)
                            .map_err(|err| StoreError::NetworkSettings(err.to_string()))
                    })
                })
                .await
                .map_err(|err| ControlServiceError::PersistenceWorker(err.to_string()));
                match result {
                    Ok(Ok((next, mut merged))) => {
                        merged.subscription_endpoint = Some(selected_ep.clone());
                        *config_guard = next;
                        Ok(merged)
                    }
                    Ok(Err(error)) => Err(error.into()),
                    Err(error) => Err(error),
                }
            } else {
                let config = self.config.read().await;
                config
                    .runtime_profile(active_id)
                    .ok_or(ControlServiceError::ProfileNotFound(active_id))
                    .map(|mut p| {
                        p.subscription_endpoint = Some(selected_ep.clone());
                        p
                    })
            };
        let mut stored = match committed {
            Ok(stored) => stored,
            Err(error) => {
                // Restore the previous selection so the catalogue never names
                // an endpoint that the account binding did not adopt.
                let store = self.endpoint_subscriptions.store.clone();
                let _ = tokio::task::spawn_blocking(move || rollback.apply(&store)).await;
                return Err(error);
            }
        };
        // Only a committed selection supersedes earlier settings intents.
        let intent = self.settings_intent.fetch_add(1, Ordering::SeqCst) + 1;

        stored.endpoint.subscription_id = Some(subscription_id);
        let shared = self
            .config
            .read()
            .await
            .network
            .hydrate(&usque_core::config::Account::default_account());

        let stable = runtime.as_ref().is_some_and(|(_, _, health)| {
            matches!(health, usque_transport::RuntimeHealth::Connected { .. })
        });
        let plan = if confirmed {
            plan_application(
                runtime.as_ref().filter(|_| confirmed).map(|(p, _, _)| p),
                &stored,
                &[String::from("endpoint.subscription_id")],
                phase,
                lifecycle.is_some() && stable,
            )
            .ok()
        } else {
            None
        };

        let mut state = self.settings.lock().await;
        state.operation_id = Some(operation_id);
        state.persisted = Some(true);
        state.record_persisted_operation(operation_id);
        state.stored_profile = Some(stored.clone());
        state.shared_network_profile = Some(shared);
        state.error_code = None;
        state.apply_status = plan.as_ref().map_or(ApplyStatus::Deferred, |p| p.status);
        state.advance();
        self.settings_tx.send_replace(state.sequence);
        let status_val = serde_json::to_value(state.apply_status).unwrap_or_default();
        drop(state);

        if let (Some(plan), Some(lifecycle), Some((previous, generation, _))) =
            (plan, lifecycle, runtime)
            && let Some(target) = plan.target
        {
            self.spawn_subscription_reconnect(
                target,
                previous,
                plan.class,
                generation,
                intent,
                operation_id,
                lifecycle,
            );
        }

        let mut list = self.build_list_json(None).await?;
        list["apply_status"] = status_val;
        Ok(list)
    }

    async fn deactivate_subscription(
        &self,
        operation_id: Uuid,
    ) -> Result<serde_json::Value, ControlServiceError> {
        if operation_id.is_nil() {
            return Err(SubscriptionError::InvalidRequest.into());
        }
        let active_id = self
            .config
            .read()
            .await
            .active_profile_id
            .ok_or_else(|| ControlServiceError::InvalidRequest("no active account".into()))?;
        self.sync_account_entitlement(active_id).await;

        let _submission = self.settings_submission.lock().await;
        if self.scenario_applying.load(Ordering::SeqCst) {
            return Err(ControlServiceError::ScenarioUnavailable("SCENARIO_BUSY"));
        }
        let lifecycle = Arc::clone(&self.mutation_lock).try_lock_owned().ok();
        let runtime = self.data_plane.try_lock().ok().and_then(|active| {
            active.as_ref().map(|active| {
                (
                    active.profile.clone(),
                    active.session_generation,
                    active.runtime.health(),
                )
            })
        });
        let phase = self.state.lock().await.snapshot().phase;
        let confirmed = matches!(
            phase,
            ConnectionPhase::Connected | ConnectionPhase::Degraded
        );

        let (stored, shared) = {
            let mut config_guard = self.config.write().await;
            let mut patch_profile = config_guard
                .runtime_profile(active_id)
                .ok_or(ControlServiceError::ProfileNotFound(active_id))?;
            patch_profile.endpoint.subscription_id = None;
            patch_profile.subscription_endpoint = None;
            let cfg_store = self.store.clone();
            let patch = NetworkSettingsPatch {
                operation_id,
                account_id: active_id,
                values: patch_profile,
                changed_fields: vec!["endpoint.subscription_id".into()],
            };
            let (next, merged) = tokio::task::spawn_blocking(move || {
                cfg_store.update(|latest| {
                    merge_patch(latest, &patch)
                        .map_err(|err| StoreError::NetworkSettings(err.to_string()))
                })
            })
            .await
            .map_err(|err| ControlServiceError::PersistenceWorker(err.to_string()))??;
            let shared = next
                .network
                .hydrate(&usque_core::config::Account::default_account());
            *config_guard = next;
            (merged, shared)
        };
        let intent = self.settings_intent.fetch_add(1, Ordering::SeqCst) + 1;

        let stable = runtime.as_ref().is_some_and(|(_, _, health)| {
            matches!(health, usque_transport::RuntimeHealth::Connected { .. })
        });
        let plan = if confirmed {
            plan_application(
                runtime.as_ref().filter(|_| confirmed).map(|(p, _, _)| p),
                &stored,
                &[String::from("endpoint.subscription_id")],
                phase,
                lifecycle.is_some() && stable,
            )
            .ok()
        } else {
            None
        };

        let mut state = self.settings.lock().await;
        state.operation_id = Some(operation_id);
        state.persisted = Some(true);
        state.record_persisted_operation(operation_id);
        state.stored_profile = Some(stored);
        state.shared_network_profile = Some(shared);
        state.error_code = None;
        state.apply_status = plan.as_ref().map_or(ApplyStatus::Deferred, |p| p.status);
        state.advance();
        self.settings_tx.send_replace(state.sequence);
        let status_val = serde_json::to_value(state.apply_status).unwrap_or_default();
        drop(state);

        if let (Some(plan), Some(lifecycle), Some((previous, generation, _))) =
            (plan, lifecycle, runtime)
            && let Some(target) = plan.target
        {
            self.spawn_subscription_reconnect(
                target,
                previous,
                plan.class,
                generation,
                intent,
                operation_id,
                lifecycle,
            );
        }

        let mut list = self.build_list_json(None).await?;
        list["apply_status"] = status_val;
        Ok(list)
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "reconnect continuation tracks validated plan and operation metadata"
    )]
    fn spawn_subscription_reconnect(
        &self,
        target: Profile,
        previous: Profile,
        class: ReconfigureClass,
        generation: u64,
        intent: u64,
        operation_id: Uuid,
        lifecycle: tokio::sync::OwnedMutexGuard<()>,
    ) {
        self.settings_applying.store(true, Ordering::SeqCst);
        let service = self.clone();
        tokio::spawn(async move {
            let _lifecycle = lifecycle;
            let result = service
                .execute_settings_plan(&target, &previous, class, intent)
                .await;
            service.settings_applying.store(false, Ordering::SeqCst);
            if result.is_ok() {
                let superseded = service.settings_intent.load(Ordering::SeqCst) != intent;
                let updated_gen = service
                    .data_plane
                    .lock()
                    .await
                    .as_mut()
                    .filter(|active| {
                        matches!(
                            active.runtime.health(),
                            usque_transport::RuntimeHealth::Connected { .. }
                        )
                    })
                    .map(|active| {
                        // Record what the session actually runs, even when a
                        // newer intent owns the published application status.
                        active.profile = target.clone();
                        active.session_generation
                    });
                let _ = generation;
                *service.session_profile.lock().await = Some(target.clone());
                if superseded {
                    return;
                }
                service
                    .publish_settings_runtime(Some(target), updated_gen)
                    .await;
            } else {
                service
                    .finish_settings_error(
                        operation_id,
                        "NETWORK_SETTINGS_APPLY_FAILED",
                        ApplyStatus::Failed,
                        false,
                    )
                    .await;
            }
        });
    }

    async fn start_probe(
        &self,
        subscription_id: Uuid,
        endpoint_ids: Option<Vec<String>>,
        probe_id: Uuid,
    ) -> Result<serde_json::Value, ControlServiceError> {
        if probe_id.is_nil() {
            return Err(SubscriptionError::InvalidRequest.into());
        }
        let active_id = self
            .config
            .read()
            .await
            .active_profile_id
            .ok_or(SubscriptionError::VerifyAccount)?;
        self.sync_account_entitlement(active_id).await;

        let config = self.config.read().await.clone();
        let tier = config
            .account_tier(active_id)
            .ok_or(SubscriptionError::VerifyAccount)?;
        let catalog = self.load_catalog().await?;
        let subscription = catalog
            .subscriptions
            .into_iter()
            .find(|s| s.id == subscription_id)
            .ok_or(SubscriptionError::NotFound)?;
        if subscription.account_tier != tier {
            return Err(SubscriptionError::TierMismatch.into());
        }

        let warp_identity = self.load_warp_identity(active_id).await?;
        let identity = MasqueTlsIdentity::from_warp_identity(&warp_identity)?;
        let profile = config
            .runtime_profile(active_id)
            .ok_or(ControlServiceError::ProfileNotFound(active_id))?;
        let sni = if profile.data_plane == DataPlaneMode::L4Proxy {
            identity
                .l4_server_name()
                .ok_or(ControlServiceError::InvalidStoredIdentity)?
                .to_owned()
        } else {
            profile.endpoint.sni.clone()
        };

        let mut targets: Vec<SubscriptionEndpoint> = Vec::new();
        let mut seen = HashSet::new();
        let filter: Option<HashSet<String>> = endpoint_ids.map(|ids| ids.into_iter().collect());
        for endpoint in subscription.endpoints {
            if filter
                .as_ref()
                .is_some_and(|wanted| !wanted.contains(&endpoint.id))
            {
                continue;
            }
            if seen.insert(endpoint.id.clone()) {
                targets.push(endpoint);
                if targets.len() >= MAX_SUBSCRIPTION_PROBE_TARGETS {
                    break;
                }
            }
        }
        if targets.is_empty() {
            return Err(SubscriptionError::EndpointNotFound.into());
        }

        let (connected_tun, session_generation, initial_protector, runtime_cancel) = {
            let guard = self.data_plane.lock().await;
            if let Some(active) = guard.as_ref() {
                if active.runtime.is_vpn() {
                    if !active.runtime.connected_tun_probe_supported() {
                        return Err(ControlServiceError::FeatureUnavailable(
                            "connected TUN endpoint probing is unavailable on this platform",
                        ));
                    }
                    let (protector, cancel) = active.runtime.probe_protector(None).ok_or(
                        ControlServiceError::FeatureUnavailable(
                            "connected TUN endpoint probing is unavailable on this platform",
                        ),
                    )?;
                    (true, Some(active.session_generation), protector, cancel)
                } else {
                    let (protector, cancel) =
                        active.runtime.probe_protector(None).unwrap_or_else(|| {
                            (
                                Arc::new(NoopSocketProtector) as Arc<dyn SocketProtector>,
                                CancellationToken::new(),
                            )
                        });
                    (false, Some(active.session_generation), protector, cancel)
                }
            } else {
                (
                    false,
                    None,
                    Arc::new(NoopSocketProtector) as Arc<dyn SocketProtector>,
                    CancellationToken::new(),
                )
            }
        };
        // Without a data plane the probe is an ordinary physical connection.
        // Admit it only where a physical download would be admitted.
        if session_generation.is_none()
            && (self.state.lock().await.snapshot().phase != ConnectionPhase::Disconnected
                || !self.subscription_physical_fetch_permitted().await)
        {
            return Err(SubscriptionError::ProtectedPathUnavailable.into());
        }

        let cancel = CancellationToken::new();
        let (finished_tx, finished_rx) = tokio::sync::watch::channel(false);
        let previous_finished = {
            let mut active_probe = self.endpoint_subscriptions.active_probe.lock().await;
            active_probe
                .replace(ActiveProbeSession {
                    probe_id,
                    subscription_id,
                    cancel: cancel.clone(),
                    finished: finished_rx,
                })
                .map(|previous| {
                    previous.cancel.cancel();
                    previous.finished
                })
        };

        let total = targets.len();
        let service = self.clone();
        tokio::spawn(async move {
            // Dropping the sender also reports completion after a panic.
            let finished_tx = finished_tx;
            if let Some(previous) = previous_finished {
                await_probe_finished(previous).await;
            }
            // One token for user cancellation and runtime teardown, so DNS
            // and per-target probes observe both.
            let cancel = cancel.child_token();
            {
                let linked = cancel.clone();
                let runtime_cancel = runtime_cancel.clone();
                tokio::spawn(async move {
                    tokio::select! {
                        _ = runtime_cancel.cancelled() => linked.cancel(),
                        _ = linked.cancelled() => {}
                    }
                });
            }
            let progress_tx = &service.endpoint_subscriptions.probe_progress_tx;
            let mut results: Vec<ProbeEndpointResult> = Vec::with_capacity(total);
            let mut final_state = "completed";
            let error_code: Option<&'static str> = None;

            if connected_tun {
                let mut numeric = Vec::new();
                let mut domains = Vec::new();
                for ep in targets {
                    if ep.host.is_domain() {
                        domains.push(ep);
                    } else {
                        numeric.push(ep);
                    }
                }
                let protos = target_protocols(profile.transport, profile.data_plane);
                let chunk_cap = (MAX_AGENT_PROBE_LEASE_TARGETS / protos.len()).max(1);
                let session_id_str = probe_id.to_string();

                // A rejected lease (for example one address that collides
                // with a tunnel DNS server) is retried per endpoint, so one
                // entry fails without discarding the rest of the batch.
                let mut queue: std::collections::VecDeque<Vec<SubscriptionEndpoint>> = numeric
                    .chunks(chunk_cap)
                    .map(<[SubscriptionEndpoint]>::to_vec)
                    .collect();
                'tun_numeric: while let Some(chunk) = queue.pop_front() {
                    if cancel.is_cancelled() {
                        final_state = "cancelled";
                        break;
                    }
                    let same_session = {
                        let guard = service.data_plane.lock().await;
                        guard
                            .as_ref()
                            .is_some_and(|a| Some(a.session_generation) == session_generation)
                    };
                    if !same_session {
                        final_state = "cancelled";
                        break;
                    }
                    let lease_targets: Vec<ProbeLeaseTarget> = chunk
                        .iter()
                        .filter_map(|ep| ep.socket_addr(profile.endpoint.port))
                        .flat_map(|addr| {
                            protos
                                .iter()
                                .copied()
                                .map(move |protocol| ProbeLeaseTarget { addr, protocol })
                        })
                        .collect();
                    let generation = initial_protector.network_generation().unwrap_or_default();
                    let lease = if lease_targets.is_empty() {
                        Err(String::new())
                    } else {
                        initial_protector
                            .acquire_endpoint_probe_lease(
                                &session_id_str,
                                &lease_targets,
                                None,
                                generation,
                            )
                            .await
                    };
                    let lease = match lease {
                        Ok(lease) => lease,
                        Err(_) if chunk.len() > 1 => {
                            for (index, ep) in chunk.into_iter().enumerate() {
                                queue.insert(index, vec![ep]);
                            }
                            continue;
                        }
                        Err(_) => {
                            for ep in chunk {
                                results.push(probe_outcome_to_result(
                                    ep.id,
                                    SubscriptionProbeOutcome::Failed,
                                ));
                            }
                            emit_probe_progress(
                                progress_tx,
                                probe_id,
                                subscription_id,
                                ProbeProgressUpdate {
                                    state: "running",
                                    total,
                                    error_code: None,
                                    results: &results,
                                },
                            );
                            continue;
                        }
                    };

                    {
                        let mut join_set = JoinSet::new();
                        let sem = Arc::new(Semaphore::new(PROBE_CONCURRENCY));
                        for ep in chunk.iter().cloned() {
                            let target_addr = ep.socket_addr(profile.endpoint.port);
                            let sem = Arc::clone(&sem);
                            let sni = sni.clone();
                            let identity = identity.clone();
                            let protector = Arc::clone(&initial_protector);
                            let cancel = cancel.clone();
                            let transport = profile.transport;
                            let data_plane = profile.data_plane;
                            join_set.spawn(async move {
                                let _permit = sem.acquire_owned().await.ok();
                                if cancel.is_cancelled() {
                                    return probe_outcome_to_result(
                                        ep.id,
                                        SubscriptionProbeOutcome::Cancelled,
                                    );
                                }
                                let Some(addr) = target_addr else {
                                    return probe_outcome_to_result(
                                        ep.id,
                                        SubscriptionProbeOutcome::Failed,
                                    );
                                };
                                let child_cancel = cancel.child_token();
                                let outcome = tokio::select! {
                                    biased;
                                    _ = cancel.cancelled() => {
                                        child_cancel.cancel();
                                        SubscriptionProbeOutcome::Cancelled
                                    }
                                    out = probe_resolved_subscription_target(
                                        addr,
                                        &sni,
                                        transport,
                                        data_plane,
                                        &identity,
                                        protector,
                                        child_cancel.clone(),
                                        PROBE_TARGET_TIMEOUT,
                                    ) => out,
                                };
                                probe_outcome_to_result(ep.id, outcome)
                            });
                        }
                        while let Some(joined) = join_set.join_next().await {
                            if let Ok(item) = joined {
                                let is_cancelled = item.status == "cancelled";
                                results.push(item);
                                emit_probe_progress(
                                    progress_tx,
                                    probe_id,
                                    subscription_id,
                                    ProbeProgressUpdate {
                                        state: "running",
                                        total,
                                        error_code: None,
                                        results: &results,
                                    },
                                );
                                if is_cancelled {
                                    final_state = "cancelled";
                                }
                            }
                        }
                    }
                    let _ = initial_protector
                        .release_endpoint_probe_lease(&session_id_str, &lease.lease_id)
                        .await;
                    if final_state != "completed" {
                        break 'tun_numeric;
                    }
                }

                if final_state == "completed" {
                    for ep in domains {
                        if cancel.is_cancelled() {
                            final_state = "cancelled";
                            break;
                        }
                        let SubscriptionEndpointHost::Domain(host) = &ep.host else {
                            continue;
                        };
                        let dns_protector = {
                            let guard = service.data_plane.lock().await;
                            guard
                                .as_ref()
                                .filter(|a| Some(a.session_generation) == session_generation)
                                .and_then(|a| a.runtime.probe_protector(Some(host.clone())))
                                .map(|(p, _)| p)
                        };
                        let Some(dns_protector) = dns_protector else {
                            final_state = "cancelled";
                            break;
                        };
                        let generation = dns_protector.network_generation().unwrap_or_default();
                        let Ok(dns_lease) = dns_protector
                            .acquire_endpoint_probe_lease(
                                &session_id_str,
                                &[],
                                Some(host.as_str()),
                                generation,
                            )
                            .await
                        else {
                            results.push(probe_outcome_to_result(
                                ep.id,
                                SubscriptionProbeOutcome::Failed,
                            ));
                            emit_probe_progress(
                                progress_tx,
                                probe_id,
                                subscription_id,
                                ProbeProgressUpdate {
                                    state: "running",
                                    total,
                                    error_code: None,
                                    results: &results,
                                },
                            );
                            continue;
                        };
                        let port = ep.effective_port(profile.endpoint.port);
                        let resolved = tokio::select! {
                            biased;
                            _ = cancel.cancelled() => Err(String::new()),
                            result = tokio::time::timeout(
                                PROBE_TARGET_TIMEOUT,
                                dns_protector.resolve_endpoint_hostname(
                                    host,
                                    port,
                                    profile.ip_policy,
                                    generation,
                                ),
                            ) => result.unwrap_or_else(|_| Err(String::new())),
                        };
                        let _ = dns_protector
                            .release_endpoint_probe_lease(&session_id_str, &dns_lease.lease_id)
                            .await;
                        let Ok(candidates) = resolved else {
                            results.push(probe_outcome_to_result(
                                ep.id,
                                SubscriptionProbeOutcome::Failed,
                            ));
                            emit_probe_progress(
                                progress_tx,
                                probe_id,
                                subscription_id,
                                ProbeProgressUpdate {
                                    state: "running",
                                    total,
                                    error_code: None,
                                    results: &results,
                                },
                            );
                            continue;
                        };
                        if cancel.is_cancelled() {
                            final_state = "cancelled";
                            break;
                        }
                        let probe_candidates: Vec<SocketAddr> = candidates
                            .into_iter()
                            .take(chunk_cap.min(DOMAIN_CANDIDATES_PER_LEASE))
                            .collect();
                        if probe_candidates.is_empty() {
                            results.push(probe_outcome_to_result(
                                ep.id,
                                SubscriptionProbeOutcome::Failed,
                            ));
                            emit_probe_progress(
                                progress_tx,
                                probe_id,
                                subscription_id,
                                ProbeProgressUpdate {
                                    state: "running",
                                    total,
                                    error_code: None,
                                    results: &results,
                                },
                            );
                            continue;
                        }
                        let lease_targets: Vec<ProbeLeaseTarget> = probe_candidates
                            .iter()
                            .copied()
                            .flat_map(|addr| {
                                protos
                                    .iter()
                                    .copied()
                                    .map(move |protocol| ProbeLeaseTarget { addr, protocol })
                            })
                            .collect();
                        let generation = initial_protector.network_generation().unwrap_or_default();
                        let Ok(target_lease) = initial_protector
                            .acquire_endpoint_probe_lease(
                                &session_id_str,
                                &lease_targets,
                                None,
                                generation,
                            )
                            .await
                        else {
                            results.push(probe_outcome_to_result(
                                ep.id,
                                SubscriptionProbeOutcome::Failed,
                            ));
                            emit_probe_progress(
                                progress_tx,
                                probe_id,
                                subscription_id,
                                ProbeProgressUpdate {
                                    state: "running",
                                    total,
                                    error_code: None,
                                    results: &results,
                                },
                            );
                            continue;
                        };
                        let mut outcome = SubscriptionProbeOutcome::Failed;
                        {
                            for candidate in probe_candidates {
                                if cancel.is_cancelled() {
                                    outcome = SubscriptionProbeOutcome::Cancelled;
                                    break;
                                }
                                let out = probe_resolved_subscription_target(
                                    candidate,
                                    &sni,
                                    profile.transport,
                                    profile.data_plane,
                                    &identity,
                                    Arc::clone(&initial_protector),
                                    cancel.clone(),
                                    PROBE_TARGET_TIMEOUT,
                                )
                                .await;
                                outcome = out;
                                if matches!(
                                    out,
                                    SubscriptionProbeOutcome::Passed { .. }
                                        | SubscriptionProbeOutcome::PinMismatch
                                        | SubscriptionProbeOutcome::Cancelled
                                ) {
                                    break;
                                }
                            }
                        }
                        let _ = initial_protector
                            .release_endpoint_probe_lease(&session_id_str, &target_lease.lease_id)
                            .await;
                        results.push(probe_outcome_to_result(ep.id, outcome));
                        emit_probe_progress(
                            progress_tx,
                            probe_id,
                            subscription_id,
                            ProbeProgressUpdate {
                                state: "running",
                                total,
                                error_code: None,
                                results: &results,
                            },
                        );
                        if matches!(outcome, SubscriptionProbeOutcome::Cancelled) {
                            final_state = "cancelled";
                            break;
                        }
                    }
                }
            } else {
                let mut join_set = JoinSet::new();
                let sem = Arc::new(Semaphore::new(PROBE_CONCURRENCY));
                for ep in targets {
                    let sem = Arc::clone(&sem);
                    let sni = sni.clone();
                    let identity = identity.clone();
                    let protector = Arc::clone(&initial_protector);
                    let cancel = cancel.clone();
                    let runtime_cancel = runtime_cancel.clone();
                    let transport = profile.transport;
                    let data_plane = profile.data_plane;
                    let ip_policy = profile.ip_policy;
                    let port = ep.effective_port(profile.endpoint.port);
                    join_set.spawn(async move {
                        let _permit = sem.acquire_owned().await.ok();
                        if cancel.is_cancelled() || runtime_cancel.is_cancelled() {
                            return probe_outcome_to_result(
                                ep.id,
                                SubscriptionProbeOutcome::Cancelled,
                            );
                        }
                        let child_cancel = CancellationToken::new();
                        let outcome = tokio::select! {
                            biased;
                            _ = cancel.cancelled() => {
                                child_cancel.cancel();
                                SubscriptionProbeOutcome::Cancelled
                            }
                            _ = runtime_cancel.cancelled() => {
                                child_cancel.cancel();
                                SubscriptionProbeOutcome::Cancelled
                            }
                            out = probe_subscription_endpoint(
                                &ep.host,
                                port,
                                &sni,
                                transport,
                                data_plane,
                                ip_policy,
                                &identity,
                                protector,
                                child_cancel.clone(),
                                PROBE_TARGET_TIMEOUT,
                            ) => out,
                        };
                        probe_outcome_to_result(ep.id, outcome)
                    });
                }
                while let Some(joined) = join_set.join_next().await {
                    if let Ok(item) = joined {
                        if item.status == "cancelled" {
                            final_state = "cancelled";
                        }
                        results.push(item);
                        emit_probe_progress(
                            progress_tx,
                            probe_id,
                            subscription_id,
                            ProbeProgressUpdate {
                                state: "running",
                                total,
                                error_code: None,
                                results: &results,
                            },
                        );
                    }
                }
            }

            if cancel.is_cancelled() {
                final_state = "cancelled";
            }
            emit_probe_progress(
                progress_tx,
                probe_id,
                subscription_id,
                ProbeProgressUpdate {
                    state: final_state,
                    total,
                    error_code,
                    results: &results,
                },
            );
            let mut active = service.endpoint_subscriptions.active_probe.lock().await;
            if active.as_ref().is_some_and(|s| s.probe_id == probe_id) {
                *active = None;
            }
            drop(active);
            let _ = finished_tx.send(true);
        });

        let mut payload = self.build_list_json(None).await?;
        payload["probe_id"] = json!(probe_id);
        Ok(payload)
    }
}

struct ProbeProgressUpdate<'a> {
    state: &'static str,
    total: usize,
    error_code: Option<&'static str>,
    results: &'a [ProbeEndpointResult],
}

fn emit_probe_progress(
    tx: &broadcast::Sender<v1::EndpointSubscriptionProbeProgress>,
    probe_id: Uuid,
    subscription_id: Uuid,
    update: ProbeProgressUpdate<'_>,
) {
    let payload = json!({
        "probe_id": probe_id,
        "subscription_id": subscription_id,
        "state": update.state,
        "completed": update.results.len(),
        "total": update.total,
        "error_code": update.error_code,
        "results": update.results,
    });
    if let Ok(progress_json) = serde_json::to_string(&payload) {
        let _ = tx.send(v1::EndpointSubscriptionProbeProgress { progress_json });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use usque_core::endpoint_subscriptions::SubscriptionEndpointHost;
    use usque_core::storage::ConfigStore;
    use usque_core::{ConsumerEntitlement, IdentityProvider, ManagedEndpointIps};

    fn service_fixture() -> (tempfile::TempDir, ControlService) {
        let dir = tempfile::tempdir().unwrap();
        let service = ControlService::open_with_vault(
            ConfigStore::new(dir.path().join("config.json")),
            Arc::new(crate::tests::MemoryVault::default()),
        )
        .unwrap();
        (dir, service)
    }

    fn seed_subscription(dir: &Path, tier: EndpointPresetTier, url: &str) -> EndpointSubscription {
        let store = EndpointSubscriptionStore::in_directory(dir);
        let ep1 = SubscriptionEndpoint {
            id: "v4:162.159.193.10:2408".into(),
            host: SubscriptionEndpointHost::Ipv4("162.159.193.10".parse().unwrap()),
            port: Some(2408),
            note: "Edge A".into(),
            pinned: false,
        };
        let ep2 = SubscriptionEndpoint {
            id: "v6:2606:4700:103::2:default".into(),
            host: SubscriptionEndpointHost::Ipv6("2606:4700:103::2".parse().unwrap()),
            port: None,
            note: "Edge B".into(),
            pinned: false,
        };
        let sub = EndpointSubscription {
            id: Uuid::new_v4(),
            revision: Uuid::new_v4(),
            name: "Test Sub".into(),
            url: url.into(),
            account_tier: tier,
            auto_refresh: SubscriptionRefreshInterval::Off,
            last_refreshed_at_unix_ms: Some(1_000),
            next_refresh_at_unix_ms: None,
            last_refresh_error: None,
            skipped_line_count: 0,
            endpoints: vec![ep1.clone(), ep2],
            selected_endpoint_id: Some(ep1.id),
            retained_missing_endpoint: None,
        };
        let (_, inserted) = store
            .update::<EndpointSubscription, StoreError>(|catalog| {
                catalog.subscriptions.push(sub.clone());
                Ok(sub)
            })
            .unwrap();
        inserted
    }

    #[tokio::test]
    async fn downloads_fail_closed_outside_disconnected_or_protected_tunnel() {
        let (dir, service) = service_fixture();
        let sub = seed_subscription(
            dir.path(),
            EndpointPresetTier::Free,
            "https://example.com/f.txt",
        );
        // Disconnected without a data plane is the only physical route.
        assert!(matches!(service.subscription_fetch_route().await, Ok(None)));
        for phase in [ConnectionPhase::Preparing, ConnectionPhase::ConnectingHttp3] {
            service.state.lock().await.transition(phase).unwrap();
            assert!(matches!(
                service.subscription_fetch_route().await,
                Err(SubscriptionError::ProtectedPathUnavailable)
            ));
            let error = service
                .refresh_subscription_by_id(sub.id)
                .await
                .unwrap_err();
            assert!(matches!(
                error,
                ControlServiceError::EndpointSubscription(
                    SubscriptionError::ProtectedPathUnavailable
                )
            ));
        }
        // A fail-closed attempt is not a server failure and schedules no backoff.
        let stored = service.load_catalog().await.unwrap();
        let stored = stored
            .subscriptions
            .iter()
            .find(|s| s.id == sub.id)
            .unwrap();
        assert!(stored.last_refresh_error.is_none());
        assert!(
            service
                .endpoint_subscriptions
                .refresh_cancels
                .lock()
                .await
                .ids()
                .is_empty()
        );
    }

    #[test]
    fn refresh_registry_only_retires_its_own_ticket() {
        let mut registry = RefreshRegistry::default();
        let id = Uuid::new_v4();
        let (first, first_token) = registry.register(id);
        let (second, second_token) = registry.register(id);
        assert!(first_token.is_cancelled());
        registry.finish(id, first);
        assert_eq!(registry.ids(), vec![id]);
        registry.finish(id, second);
        assert!(registry.ids().is_empty());
        assert!(!second_token.is_cancelled());
    }

    #[test]
    fn retired_store_epoch_rejects_a_late_commit() {
        let epoch = StoreEpoch::default();
        let started = epoch.current();
        epoch.retire(|| ());
        assert!(matches!(
            epoch.commit(started, || Ok(())),
            Err(ControlServiceError::EndpointSubscription(
                SubscriptionError::Cancelled
            ))
        ));
        assert!(epoch.commit(epoch.current(), || Ok(())).is_ok());
    }

    #[tokio::test]
    async fn select_and_hydrate_enforces_tier_zt_warning_and_no_scene_drift() {
        let (dir, service) = service_fixture();
        let active_id = service.config_snapshot().await.active_profile_id.unwrap();
        let free_identity = crate::tests::test_identity_with_entitlement(
            IdentityProvider::Consumer,
            None,
            Some(ConsumerEntitlement::Free),
        );
        service
            .persist_identity(active_id, &free_identity, None)
            .await
            .unwrap();
        service
            .update_config(move |cfg| {
                cfg.identity_bindings
                    .insert(active_id, IdentityProvider::Consumer);
                cfg.set_consumer_entitlement(active_id, Some(ConsumerEntitlement::Free));
                Ok(())
            })
            .await
            .unwrap();

        let free_sub = seed_subscription(
            dir.path(),
            EndpointPresetTier::Free,
            "https://example.com/f.txt",
        );
        let plus_sub = seed_subscription(
            dir.path(),
            EndpointPresetTier::Plus,
            "https://example.com/p.txt",
        );

        // Tier mismatch must fail closed.
        let mismatch = service
            .endpoint_subscription_command(v1::EndpointSubscriptionRequest {
                command_json: json!({
                    "action": "select_endpoint",
                    "subscription_id": plus_sub.id,
                    "endpoint_id": "v4:162.159.193.10:2408",
                    "operation_id": Uuid::new_v4(),
                })
                .to_string(),
            })
            .await
            .unwrap_err();
        assert!(matches!(
            mismatch,
            ControlServiceError::EndpointSubscription(SubscriptionError::TierMismatch)
        ));

        // Selecting a Free subscription endpoint hydrates runtime profile cleanly.
        service
            .endpoint_subscription_command(v1::EndpointSubscriptionRequest {
                command_json: json!({
                    "action": "select_endpoint",
                    "subscription_id": free_sub.id,
                    "endpoint_id": "v4:162.159.193.10:2408",
                    "operation_id": Uuid::new_v4(),
                })
                .to_string(),
            })
            .await
            .unwrap();
        let mut hydrated = service
            .config_snapshot()
            .await
            .runtime_profile(active_id)
            .unwrap();
        service
            .hydrate_profile_subscription_endpoint(&mut hydrated)
            .await
            .unwrap();
        assert_eq!(
            hydrated.subscription_endpoint.as_ref().unwrap().id,
            "v4:162.159.193.10:2408"
        );

        // Saving a Scene on free_sub and then switching endpoints within the SAME
        // subscription MUST NOT cause Scene Modified drift!
        let mut settings = usque_core::scenarios::ScenarioSettings::from_profile(&hydrated);
        settings.endpoint.subscription_id = Some(free_sub.id);
        let save = service
            .scenario_command(v1::ScenarioRequest {
                command_json: json!({
                    "action": "save",
                    "name": "Work Sub Scene",
                    "settings": settings,
                })
                .to_string(),
            })
            .await
            .unwrap();
        let saved_json: serde_json::Value = serde_json::from_str(&save.metadata_json).unwrap();
        let scene_id = saved_json["saved_scenario_id"].as_str().unwrap();
        let scene_rev = saved_json["scenarios"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["id"].as_str() == Some(scene_id))
            .unwrap()["revision"]
            .as_str()
            .unwrap()
            .to_owned();
        let preview = service
            .scenario_command(v1::ScenarioRequest {
                command_json: json!({
                    "action": "preview",
                    "id": scene_id,
                    "expected_revision": scene_rev,
                })
                .to_string(),
            })
            .await
            .unwrap();
        let preview_json: serde_json::Value = serde_json::from_str(&preview.metadata_json).unwrap();
        let preview_token = preview_json["preview"]["preview_token"]
            .as_str()
            .unwrap()
            .to_owned();
        service
            .scenario_command(v1::ScenarioRequest {
                command_json: json!({
                    "action": "activate",
                    "id": scene_id,
                    "expected_revision": scene_rev,
                    "preview_token": preview_token,
                    "operation_id": Uuid::new_v4(),
                })
                .to_string(),
            })
            .await
            .unwrap();

        // Now quick-switch to Edge B inside the SAME subscription.
        service
            .endpoint_subscription_command(v1::EndpointSubscriptionRequest {
                command_json: json!({
                    "action": "select_endpoint",
                    "subscription_id": free_sub.id,
                    "endpoint_id": "v6:2606:4700:103::2:default",
                    "operation_id": Uuid::new_v4(),
                })
                .to_string(),
            })
            .await
            .unwrap();
        let list_scenes = service
            .scenario_command(v1::ScenarioRequest {
                command_json: json!({"action": "list"}).to_string(),
            })
            .await
            .unwrap();
        let list_json: serde_json::Value =
            serde_json::from_str(&list_scenes.metadata_json).unwrap();
        assert_eq!(list_json["state"]["modified"], false);

        // Zero Trust risk gate: switching onto ZT subscription requires explicit ack.
        let zt_id = Uuid::new_v4();
        let zt_provider = IdentityProvider::zero_trust("team-acme").unwrap();
        let zt_identity = crate::tests::test_identity(zt_provider.clone(), None);
        service
            .persist_identity(zt_id, &zt_identity, None)
            .await
            .unwrap();
        service
            .update_config(move |cfg| {
                cfg.insert_account(
                    zt_id,
                    "ZT".into(),
                    Some(ManagedEndpointIps {
                        ipv4: "162.159.197.2".parse().unwrap(),
                        ipv6: "2606:4700:102::2".parse().unwrap(),
                    }),
                )
                .unwrap();
                cfg.identity_bindings.insert(zt_id, zt_provider);
                cfg.active_profile_id = Some(zt_id);
                Ok(())
            })
            .await
            .unwrap();
        let zt_sub = seed_subscription(
            dir.path(),
            EndpointPresetTier::ZeroTrust,
            "https://example.com/z.txt",
        );
        let needs_ack = service
            .endpoint_subscription_command(v1::EndpointSubscriptionRequest {
                command_json: json!({
                    "action": "select_endpoint",
                    "subscription_id": zt_sub.id,
                    "endpoint_id": "v4:162.159.193.10:2408",
                    "operation_id": Uuid::new_v4(),
                })
                .to_string(),
            })
            .await
            .unwrap_err();
        assert!(matches!(
            needs_ack,
            ControlServiceError::EndpointSubscription(SubscriptionError::ZeroTrustRiskRequired)
        ));
    }

    #[tokio::test]
    async fn deleting_subscription_clears_active_binding_but_preserves_scene_dependency() {
        let (dir, service) = service_fixture();
        let active_id = service.config_snapshot().await.active_profile_id.unwrap();
        let free_identity = crate::tests::test_identity_with_entitlement(
            IdentityProvider::Consumer,
            None,
            Some(ConsumerEntitlement::Free),
        );
        service
            .persist_identity(active_id, &free_identity, None)
            .await
            .unwrap();
        service
            .update_config(move |cfg| {
                cfg.identity_bindings
                    .insert(active_id, IdentityProvider::Consumer);
                cfg.set_consumer_entitlement(active_id, Some(ConsumerEntitlement::Free));
                Ok(())
            })
            .await
            .unwrap();

        let free_sub = seed_subscription(
            dir.path(),
            EndpointPresetTier::Free,
            "https://example.com/del.txt",
        );
        let selected = service
            .endpoint_subscription_command(v1::EndpointSubscriptionRequest {
                command_json: json!({
                    "action": "select_endpoint",
                    "subscription_id": free_sub.id,
                    "endpoint_id": "v4:162.159.193.10:2408",
                    "operation_id": Uuid::new_v4(),
                })
                .to_string(),
            })
            .await
            .unwrap();
        let selected_json: serde_json::Value =
            serde_json::from_str(&selected.metadata_json).unwrap();
        let current_rev = selected_json["subscriptions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["id"].as_str() == Some(free_sub.id.to_string().as_str()))
            .unwrap()["revision"]
            .as_str()
            .unwrap()
            .to_owned();

        let hydrated = service
            .config_snapshot()
            .await
            .runtime_profile(active_id)
            .unwrap();
        let mut settings = usque_core::scenarios::ScenarioSettings::from_profile(&hydrated);
        settings.endpoint.subscription_id = Some(free_sub.id);
        let save = service
            .scenario_command(v1::ScenarioRequest {
                command_json: json!({
                    "action": "save",
                    "name": "Bound Sub Scene",
                    "settings": settings,
                })
                .to_string(),
            })
            .await
            .unwrap();
        let saved_json: serde_json::Value = serde_json::from_str(&save.metadata_json).unwrap();
        let scene_id = saved_json["saved_scenario_id"].as_str().unwrap().to_owned();
        let scene_rev = saved_json["scenarios"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["id"].as_str() == Some(scene_id.as_str()))
            .unwrap()["revision"]
            .as_str()
            .unwrap()
            .to_owned();

        // Delete the subscription.
        service
            .endpoint_subscription_command(v1::EndpointSubscriptionRequest {
                command_json: json!({
                    "action": "delete",
                    "id": free_sub.id,
                    "expected_revision": current_rev,
                })
                .to_string(),
            })
            .await
            .unwrap();

        // Active account subscription binding is cleared, while the saved scene retains its dependency ID.
        let cfg = service.config_snapshot().await;
        assert_eq!(cfg.network.consumer_tier_subscriptions.free, None);
        assert_eq!(
            cfg.runtime_profile(active_id)
                .unwrap()
                .endpoint
                .subscription_id,
            None
        );
        let scene = cfg
            .scenarios
            .iter()
            .find(|s| s.id.to_string() == scene_id)
            .unwrap();
        assert_eq!(scene.settings.endpoint.subscription_id, Some(free_sub.id));

        // Previewing the orphaned scene fails closed with SCENARIO_DEPENDENCY_UNAVAILABLE.
        let preview_err = service
            .scenario_command(v1::ScenarioRequest {
                command_json: json!({
                    "action": "preview",
                    "id": scene_id,
                    "expected_revision": scene_rev,
                })
                .to_string(),
            })
            .await
            .unwrap_err();
        assert!(matches!(
            preview_err,
            ControlServiceError::ScenarioUnavailable("SCENARIO_DEPENDENCY_UNAVAILABLE")
        ));
    }
}
