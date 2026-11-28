//! Android endpoint subscription storage, bounded downloads, auto-refresh,
//! MASQUE latency probing, and tier-isolated activation.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use usque_core::endpoint_subscriptions::{
    EndpointSubscription, EndpointSubscriptionCatalogFile, EndpointSubscriptionCommand,
    EndpointSubscriptionStore, MAX_ENDPOINT_SUBSCRIPTIONS, MAX_SUBSCRIPTION_BODY_BYTES,
    MAX_SUBSCRIPTION_PROBE_TARGETS, ParsedSubscription, SubscriptionEndpoint, SubscriptionError,
    SubscriptionRefreshInterval, catalog_revision, fetch_subscription_direct,
    parse_subscription_body, sha256_hex, validate_subscription_name, validate_subscription_url,
};
use usque_core::network_settings::{ApplyStatus, NetworkSettingsPatch, merge_patch};
use usque_core::{
    AppConfig, ConnectionPhase, DataPlaneMode, EndpointPresetTier, EndpointSelection, Profile,
    Transport, WarpIdentity, storage::ConfigStore, storage::StoreError,
};
use usque_transport::{
    InternalNetwork, MasqueTlsIdentity, NoopSocketProtector, SocketProtector,
    SubscriptionProbeOutcome, probe_subscription_endpoint,
};
use uuid::Uuid;

const FETCH_TOTAL_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_FETCH_REDIRECTS: usize = 3;
const PROBE_CONCURRENCY: usize = 4;
const PROBE_TARGET_TIMEOUT: Duration = Duration::from_millis(3500);
pub(crate) const MAX_REQUEST_BYTES: usize = 256 * 1024;

#[derive(Clone)]
struct ActiveProbeSession {
    probe_id: Uuid,
    subscription_id: Uuid,
    cancel: CancellationToken,
}

#[derive(Debug, Clone, Serialize)]
struct ActiveProbeInfo {
    probe_id: Uuid,
    subscription_id: Uuid,
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
/// ticket so a finishing download retires only its own token.
#[derive(Default)]
struct RefreshRegistry {
    next_ticket: u64,
    entries: HashMap<Uuid, (u64, CancellationToken)>,
}

static REFRESH_CANCELS: OnceLock<Mutex<RefreshRegistry>> = OnceLock::new();
static ACTIVE_PROBE: OnceLock<Mutex<Option<ActiveProbeSession>>> = OnceLock::new();
static AUTO_REFRESH_RUNNING: AtomicBool = AtomicBool::new(false);
/// Clear All Data advances this epoch while holding it; downloads commit only
/// under the same lock for their starting epoch and never recreate the file.
static STORE_EPOCH: Mutex<u64> = Mutex::new(0);

fn refresh_cancels() -> &'static Mutex<RefreshRegistry> {
    REFRESH_CANCELS.get_or_init(|| Mutex::new(RefreshRegistry::default()))
}

fn register_refresh(id: Uuid) -> (u64, CancellationToken) {
    let token = CancellationToken::new();
    let Ok(mut registry) = refresh_cancels().lock() else {
        // An unusable registry cannot be cancelled; refuse to start.
        token.cancel();
        return (0, token);
    };
    registry.next_ticket = registry.next_ticket.wrapping_add(1);
    let ticket = registry.next_ticket;
    if let Some((_, previous)) = registry.entries.insert(id, (ticket, token.clone())) {
        previous.cancel();
    }
    (ticket, token)
}

fn finish_refresh(id: Uuid, ticket: u64) {
    if let Ok(mut registry) = refresh_cancels().lock()
        && registry
            .entries
            .get(&id)
            .is_some_and(|(current, _)| *current == ticket)
    {
        registry.entries.remove(&id);
    }
}

fn cancel_refresh(id: Uuid) {
    if let Ok(mut registry) = refresh_cancels().lock()
        && let Some((_, token)) = registry.entries.remove(&id)
    {
        token.cancel();
    }
}

fn store_epoch() -> u64 {
    *STORE_EPOCH
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Runs a catalogue write only if no Clear All Data happened since `expected`.
fn commit_for_epoch<T>(
    expected: u64,
    operation: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    let guard = STORE_EPOCH
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if *guard != expected {
        return Err(SubscriptionError::Cancelled.code().to_owned());
    }
    let result = operation();
    drop(guard);
    result
}

fn active_probe_slot() -> &'static Mutex<Option<ActiveProbeSession>> {
    ACTIVE_PROBE.get_or_init(|| Mutex::new(None))
}

fn unix_ms_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub(crate) fn signal_cancel() {
    if let Ok(registry) = refresh_cancels().lock() {
        for (_, token) in registry.entries.values() {
            token.cancel();
        }
    }
    if let Ok(slot) = active_probe_slot().lock()
        && let Some(probe) = slot.as_ref()
    {
        probe.cancel.cancel();
    }
}

pub(crate) fn cancel_all_and_clear(dir: &Path) -> Result<(), String> {
    signal_cancel();
    // Join cancelled downloads first. Their commits are epoch-checked too, so
    // a download outliving this bound still cannot recreate the catalogue.
    let _ = join_in_flight_downloads(CLEAR_DOWNLOAD_JOIN);
    let mut epoch = STORE_EPOCH
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *epoch = epoch.wrapping_add(1);
    EndpointSubscriptionStore::in_directory(dir)
        .clear()
        .map_err(|err| err.to_string())
}

pub(crate) fn hydrate_profile_in_dir(
    dir: &Path,
    config: Option<&AppConfig>,
    profile: &mut Profile,
) -> Result<(), String> {
    let Some(sub_id) = profile.endpoint.subscription_id else {
        profile.subscription_endpoint = None;
        return Ok(());
    };
    let loaded_config;
    let config_ref = match config {
        Some(cfg) => cfg,
        None => {
            loaded_config = ConfigStore::new(dir.join("profiles-v2.json"))
                .load_or_default()
                .map_err(|_| SubscriptionError::StorageUnavailable.code().to_owned())?;
            &loaded_config
        }
    };
    let tier = config_ref.account_tier(profile.id);
    let catalog = EndpointSubscriptionStore::in_directory(dir)
        .load_or_default()
        .map_err(|_| SubscriptionError::StorageUnavailable.code().to_owned())?;
    let resolved = catalog
        .resolve_target(sub_id, tier)
        .map_err(|err| err.code().to_owned())?;
    profile.subscription_endpoint = Some(resolved.endpoint);
    profile.endpoint.selection = EndpointSelection::Custom;
    profile.endpoint.preset_region = None;
    Ok(())
}

pub(crate) fn current_subscription_endpoint(
    dir: &Path,
    config: &AppConfig,
) -> Option<SubscriptionEndpoint> {
    let active_id = config.active_profile_id?;
    let profile = config.runtime_profile(active_id)?;
    let sub_id = profile.endpoint.subscription_id?;
    let tier = config.account_tier(active_id);
    EndpointSubscriptionStore::in_directory(dir)
        .load_or_default()
        .ok()
        .and_then(|catalog| catalog.resolve_target(sub_id, tier).ok())
        .map(|resolved| resolved.endpoint)
}

pub(crate) fn validate_scenario_subscription_at(
    dir: &Path,
    profile: &Profile,
    tier: Option<EndpointPresetTier>,
) -> Result<Option<String>, String> {
    let Some(sub_id) = profile.endpoint.subscription_id else {
        return Ok(None);
    };
    let catalog = EndpointSubscriptionStore::in_directory(dir)
        .load_or_default()
        .map_err(|_| "SCENARIO_DEPENDENCY_UNAVAILABLE".to_owned())?;
    let resolved = catalog
        .resolve_target(sub_id, tier)
        .map_err(|_| "SCENARIO_DEPENDENCY_UNAVAILABLE".to_owned())?;
    let rev = catalog
        .subscriptions
        .iter()
        .find(|s| s.id == sub_id)
        .map(|s| s.revision)
        .unwrap_or_else(Uuid::nil);
    Ok(Some(format!("sub:{sub_id}:{rev}:{}", resolved.endpoint.id)))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    request: Value,
    #[serde(default)]
    phase: String,
    #[serde(default)]
    available: bool,
    #[serde(default)]
    session_id: String,
    #[serde(default)]
    allow_physical: bool,
}

pub(crate) struct AndroidFetchContext {
    pub warp: Option<InternalNetwork>,
    pub allow_physical: bool,
    pub allow_auto_refresh: bool,
    pub warp_identity: Option<WarpIdentity>,
    pub protector: Arc<dyn SocketProtector>,
    pub runtime_cancel: Option<CancellationToken>,
    pub on_probe_progress: Option<ProbeProgressCallback>,
}

pub(crate) type ProbeProgressCallback = Arc<dyn Fn(&str) + Send + Sync>;

impl Default for AndroidFetchContext {
    fn default() -> Self {
        Self {
            warp: None,
            allow_physical: false,
            allow_auto_refresh: false,
            warp_identity: None,
            protector: Arc::new(NoopSocketProtector),
            runtime_cancel: None,
            on_probe_progress: None,
        }
    }
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

fn build_list_json(
    dir: &Path,
    config: &AppConfig,
    known_revision: Option<String>,
) -> Result<Value, String> {
    let active_id = config.active_profile_id;
    let active_tier = active_id.and_then(|id| config.account_tier(id));
    let active_profile = config.active_profile();
    let active_subscription_id = active_profile
        .as_ref()
        .and_then(|profile| profile.endpoint.subscription_id);

    let catalog = EndpointSubscriptionStore::in_directory(dir)
        .load_or_default()
        .map_err(|_| SubscriptionError::StorageUnavailable.code().to_owned())?;
    let active_endpoint_id = active_subscription_id.and_then(|sub_id| {
        catalog
            .subscriptions
            .iter()
            .find(|s| s.id == sub_id)
            .and_then(|s| s.active_endpoint().map(|(ep, _)| ep.id.clone()))
    });
    let mut refreshing_ids: Vec<Uuid> = refresh_cancels()
        .lock()
        .map(|registry| registry.entries.keys().copied().collect())
        .unwrap_or_default();
    refreshing_ids.sort();
    let active_probe = active_probe_slot().lock().ok().and_then(|slot| {
        slot.as_ref().map(|p| ActiveProbeInfo {
            probe_id: p.probe_id,
            subscription_id: p.subscription_id,
        })
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

/// Downloads in flight. Clear All Data cancels them and waits here before
/// removing the catalogue (the store epoch also rejects any late commit).
static IN_FLIGHT_DOWNLOADS: (Mutex<usize>, std::sync::Condvar) =
    (Mutex::new(0), std::sync::Condvar::new());
const CLEAR_DOWNLOAD_JOIN: Duration = Duration::from_secs(20);

struct InFlightDownload;

impl InFlightDownload {
    fn begin() -> Self {
        *IN_FLIGHT_DOWNLOADS
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) += 1;
        Self
    }
}

impl Drop for InFlightDownload {
    fn drop(&mut self) {
        let mut count = IN_FLIGHT_DOWNLOADS
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *count = count.saturating_sub(1);
        IN_FLIGHT_DOWNLOADS.1.notify_all();
    }
}

/// Waits (bounded) until every cancelled download has returned. Returns
/// whether they all did.
fn join_in_flight_downloads(limit: Duration) -> bool {
    let guard = IN_FLIGHT_DOWNLOADS
        .0
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    IN_FLIGHT_DOWNLOADS
        .1
        .wait_timeout_while(guard, limit, |count| *count > 0)
        .is_ok_and(|(count, _)| *count == 0)
}

fn fetch_and_parse_sync(
    url: &str,
    cancel: &CancellationToken,
    warp: Option<&InternalNetwork>,
    allow_physical: bool,
) -> Result<ParsedSubscription, SubscriptionError> {
    let _in_flight = InFlightDownload::begin();
    if warp.is_none() && !allow_physical {
        return Err(SubscriptionError::Network);
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| SubscriptionError::Network)?;
    let bytes = runtime.block_on(async {
        if let Some(warp) = warp {
            warp.get_https_bounded(
                url,
                MAX_SUBSCRIPTION_BODY_BYTES,
                MAX_FETCH_REDIRECTS,
                FETCH_TOTAL_TIMEOUT,
                cancel,
            )
            .await
            .map_err(|err| err.to_subscription_error())
        } else {
            fetch_subscription_direct(
                url,
                MAX_SUBSCRIPTION_BODY_BYTES,
                MAX_FETCH_REDIRECTS,
                FETCH_TOTAL_TIMEOUT,
                cancel,
            )
            .await
            .map_err(|err| err.to_subscription_error())
        }
    })?;
    let parsed = parse_subscription_body(&bytes)?;
    if parsed.endpoints.is_empty() {
        return Err(SubscriptionError::Empty);
    }
    Ok(parsed)
}

fn maybe_spawn_auto_refresh(dir: PathBuf, warp: Option<InternalNetwork>, allow_physical: bool) {
    if warp.is_none() && !allow_physical {
        return;
    }
    if AUTO_REFRESH_RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("usque-sub-autorefresh".to_owned())
        .spawn(move || {
            struct ResetFlag;
            impl Drop for ResetFlag {
                fn drop(&mut self) {
                    AUTO_REFRESH_RUNNING.store(false, Ordering::SeqCst);
                }
            }
            let _reset = ResetFlag;
            let store = EndpointSubscriptionStore::in_directory(&dir);
            let Ok(catalog) = store.load_or_default() else {
                return;
            };
            let now_ms = unix_ms_now();
            let due: Vec<(Uuid, String)> = catalog
                .subscriptions
                .into_iter()
                .filter(|sub| {
                    sub.auto_refresh != SubscriptionRefreshInterval::Off
                        && sub
                            .next_refresh_at_unix_ms
                            .is_none_or(|next| now_ms >= next)
                })
                .map(|sub| (sub.id, sub.url))
                .collect();
            let epoch = store_epoch();
            for (id, url) in due {
                let (ticket, cancel) = register_refresh(id);
                let result = fetch_and_parse_sync(&url, &cancel, warp.as_ref(), allow_physical);
                finish_refresh(id, ticket);
                let done_ms = unix_ms_now();
                let update = |apply: &dyn Fn(&mut EndpointSubscription)| {
                    commit_for_epoch(epoch, || {
                        // A subscription deleted meanwhile leaves the file as is.
                        let present = store
                            .load_or_default()
                            .is_ok_and(|catalog| catalog.subscriptions.iter().any(|s| s.id == id));
                        if !present {
                            return Ok(());
                        }
                        store
                            .update::<(), StoreError>(|catalog| {
                                if let Some(sub) =
                                    catalog.subscriptions.iter_mut().find(|s| s.id == id)
                                {
                                    apply(sub);
                                }
                                Ok(())
                            })
                            .map(|_| ())
                            .map_err(|err| err.to_string())
                    })
                };
                match result {
                    Ok(parsed) => {
                        let parsed = std::cell::RefCell::new(Some(parsed));
                        let _ = update(&|sub| {
                            if let Some(parsed) = parsed.borrow_mut().take() {
                                sub.reconcile_endpoints(parsed, done_ms);
                            }
                        });
                    }
                    // Clear All Data, stop or a network change ends the batch.
                    Err(SubscriptionError::Cancelled) => break,
                    Err(err) => {
                        let code = err.code().to_owned();
                        let _ = update(&|sub| sub.record_refresh_failure(&code, done_ms));
                    }
                }
                if store_epoch() != epoch {
                    break;
                }
            }
        });
    if spawned.is_err() {
        AUTO_REFRESH_RUNNING.store(false, Ordering::SeqCst);
    }
}

pub(crate) fn command(
    config_path: &str,
    envelope_json: &str,
    context: AndroidFetchContext,
) -> Result<String, String> {
    let path = Path::new(config_path);
    if path.as_os_str().len() > 4096
        || !path.is_absolute()
        || path.file_name().and_then(|name| name.to_str()) != Some("profiles-v2.json")
        || envelope_json.len() > MAX_REQUEST_BYTES
    {
        return Err(SubscriptionError::InvalidRequest.code().to_owned());
    }
    let dir = path
        .parent()
        .ok_or_else(|| SubscriptionError::InvalidRequest.code().to_owned())?;
    let envelope: Envelope = serde_json::from_str(envelope_json)
        .map_err(|_| SubscriptionError::InvalidRequest.code().to_owned())?;
    if envelope.phase.len() > 32 || envelope.session_id.len() > 64 {
        return Err(SubscriptionError::InvalidRequest.code().to_owned());
    }
    let command = EndpointSubscriptionCommand::parse(&envelope.request.to_string())
        .map_err(|err| err.code().to_owned())?;
    let config_store = ConfigStore::new(path);
    let sub_store = EndpointSubscriptionStore::in_directory(dir);

    let result = match command {
        EndpointSubscriptionCommand::List { known_revision } => {
            if context.allow_auto_refresh {
                maybe_spawn_auto_refresh(
                    dir.to_path_buf(),
                    context.warp.clone(),
                    envelope.allow_physical && context.allow_physical,
                );
            }
            let _lock = config_store
                .lock_exclusive()
                .map_err(|_| SubscriptionError::StorageUnavailable.code().to_owned())?;
            let config = config_store
                .load_or_default()
                .map_err(|_| SubscriptionError::StorageUnavailable.code().to_owned())?;
            build_list_json(dir, &config, known_revision)?
        }
        EndpointSubscriptionCommand::Add {
            name,
            url,
            account_tier,
            auto_refresh,
        } => {
            let name = validate_subscription_name(&name).map_err(|e| e.code().to_owned())?;
            let url = validate_subscription_url(&url).map_err(|e| e.code().to_owned())?;
            let current = sub_store
                .load_or_default()
                .map_err(|_| SubscriptionError::StorageUnavailable.code().to_owned())?;
            if current.subscriptions.len() >= MAX_ENDPOINT_SUBSCRIPTIONS {
                return Err(SubscriptionError::Limit.code().to_owned());
            }
            // Registered so Clear All Data, stop and network changes cancel it.
            let epoch = store_epoch();
            let pending_id = Uuid::new_v4();
            let (ticket, cancel) = register_refresh(pending_id);
            let fetched = fetch_and_parse_sync(
                &url,
                &cancel,
                context.warp.as_ref(),
                envelope.allow_physical && context.allow_physical,
            );
            finish_refresh(pending_id, ticket);
            let parsed = fetched.map_err(|e| e.code().to_owned())?;
            let now_ms = unix_ms_now();
            let next_refresh_at_unix_ms = auto_refresh
                .interval_ms()
                .map(|interval| now_ms.saturating_add(interval));
            let selected_endpoint_id = parsed.endpoints.first().map(|ep| ep.id.clone());
            let sub = EndpointSubscription {
                id: pending_id,
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
            let to_insert = sub.clone();
            commit_for_epoch(epoch, || {
                sub_store
                    .update::<(), SubscriptionError>(|catalog| {
                        if catalog.subscriptions.len() >= MAX_ENDPOINT_SUBSCRIPTIONS {
                            return Err(SubscriptionError::Limit);
                        }
                        catalog.subscriptions.push(to_insert);
                        Ok(())
                    })
                    .map(|_| ())
                    .map_err(|err| err.to_string())
            })?;
            let _lock = config_store
                .lock_exclusive()
                .map_err(|_| SubscriptionError::StorageUnavailable.code().to_owned())?;
            let config = config_store
                .load_or_default()
                .map_err(|_| SubscriptionError::StorageUnavailable.code().to_owned())?;
            let mut list = build_list_json(dir, &config, None)?;
            list["subscription"] = serde_json::to_value(sub)
                .map_err(|_| SubscriptionError::InvalidBody.code().to_owned())?;
            list
        }
        EndpointSubscriptionCommand::Update {
            id,
            name,
            url,
            account_tier,
            auto_refresh,
            expected_revision,
        } => {
            let name = validate_subscription_name(&name).map_err(|e| e.code().to_owned())?;
            let url = validate_subscription_url(&url).map_err(|e| e.code().to_owned())?;
            let existing = sub_store
                .load_or_default()
                .map_err(|_| SubscriptionError::StorageUnavailable.code().to_owned())?
                .subscriptions
                .into_iter()
                .find(|s| s.id == id)
                .ok_or_else(|| SubscriptionError::NotFound.code().to_owned())?;
            if existing.revision != expected_revision {
                return Err(SubscriptionError::StaleRevision.code().to_owned());
            }
            let epoch = store_epoch();
            let parsed = if existing.url != url {
                let (ticket, cancel) = register_refresh(id);
                let res = fetch_and_parse_sync(
                    &url,
                    &cancel,
                    context.warp.as_ref(),
                    envelope.allow_physical && context.allow_physical,
                );
                // Retire only this registration, never a newer refresh's token.
                finish_refresh(id, ticket);
                Some(res.map_err(|e| e.code().to_owned())?)
            } else {
                None
            };
            let now_ms = unix_ms_now();
            let (_, (updated, tier_changed)) = commit_for_epoch(epoch, || {
                sub_store
                    .update::<(EndpointSubscription, bool), SubscriptionError>(|catalog| {
                        let sub = catalog
                            .subscriptions
                            .iter_mut()
                            .find(|s| s.id == id)
                            .ok_or(SubscriptionError::NotFound)?;
                        if sub.revision != expected_revision {
                            return Err(SubscriptionError::StaleRevision);
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
                    .map_err(|err| err.to_string())
            })?;
            let mutation = if tier_changed {
                cleanup_subscription_references(&config_store, &envelope, id, Some(account_tier))?
            } else {
                None
            };
            let config = {
                let _lock = config_store
                    .lock_exclusive()
                    .map_err(|_| SubscriptionError::StorageUnavailable.code().to_owned())?;
                config_store
                    .load_or_default()
                    .map_err(|_| SubscriptionError::StorageUnavailable.code().to_owned())?
            };
            let mut list = build_list_json(dir, &config, None)?;
            list["subscription"] = serde_json::to_value(updated)
                .map_err(|_| SubscriptionError::InvalidBody.code().to_owned())?;
            attach_mutation(&mut list, mutation);
            list
        }
        EndpointSubscriptionCommand::Delete {
            id,
            expected_revision,
        } => {
            cancel_refresh(id);
            if let Ok(slot) = active_probe_slot().lock()
                && let Some(session) = slot.as_ref()
                && session.subscription_id == id
            {
                session.cancel.cancel();
            }
            sub_store
                .update::<(), SubscriptionError>(|catalog| {
                    let idx = catalog
                        .subscriptions
                        .iter()
                        .position(|s| s.id == id)
                        .ok_or(SubscriptionError::NotFound)?;
                    if catalog.subscriptions[idx].revision != expected_revision {
                        return Err(SubscriptionError::StaleRevision);
                    }
                    catalog.subscriptions.remove(idx);
                    Ok(())
                })
                .map_err(|err| err.to_string())?;
            let mutation = cleanup_subscription_references(&config_store, &envelope, id, None)?;
            let config = {
                let _lock = config_store
                    .lock_exclusive()
                    .map_err(|_| SubscriptionError::StorageUnavailable.code().to_owned())?;
                config_store
                    .load_or_default()
                    .map_err(|_| SubscriptionError::StorageUnavailable.code().to_owned())?
            };
            let mut list = build_list_json(dir, &config, None)?;
            attach_mutation(&mut list, mutation);
            list
        }
        EndpointSubscriptionCommand::Refresh { id } => {
            let url = sub_store
                .load_or_default()
                .map_err(|_| SubscriptionError::StorageUnavailable.code().to_owned())?
                .subscriptions
                .into_iter()
                .find(|s| s.id == id)
                .map(|s| s.url)
                .ok_or_else(|| SubscriptionError::NotFound.code().to_owned())?;
            let epoch = store_epoch();
            let (ticket, cancel) = register_refresh(id);
            let fetched = fetch_and_parse_sync(
                &url,
                &cancel,
                context.warp.as_ref(),
                envelope.allow_physical && context.allow_physical,
            );
            finish_refresh(id, ticket);
            let now_ms = unix_ms_now();
            let updated = match fetched {
                Ok(parsed) => commit_for_epoch(epoch, || {
                    sub_store
                        .update::<EndpointSubscription, SubscriptionError>(|catalog| {
                            let sub = catalog
                                .subscriptions
                                .iter_mut()
                                .find(|s| s.id == id)
                                .ok_or(SubscriptionError::NotFound)?;
                            sub.reconcile_endpoints(parsed, now_ms);
                            Ok(sub.clone())
                        })
                        .map(|(_, updated)| updated)
                        .map_err(|err| err.to_string())
                })?,
                Err(SubscriptionError::Cancelled) => {
                    return Err(SubscriptionError::Cancelled.code().to_owned());
                }
                Err(err) => {
                    let code = err.code().to_owned();
                    let _ = commit_for_epoch(epoch, || {
                        sub_store
                            .update::<(), StoreError>(|catalog| {
                                if let Some(sub) =
                                    catalog.subscriptions.iter_mut().find(|s| s.id == id)
                                {
                                    sub.record_refresh_failure(&code, now_ms);
                                }
                                Ok(())
                            })
                            .map(|_| ())
                            .map_err(|err| err.to_string())
                    });
                    return Err(code);
                }
            };
            let _lock = config_store
                .lock_exclusive()
                .map_err(|_| SubscriptionError::StorageUnavailable.code().to_owned())?;
            let config = config_store
                .load_or_default()
                .map_err(|_| SubscriptionError::StorageUnavailable.code().to_owned())?;
            let mut list = build_list_json(dir, &config, None)?;
            list["subscription"] = serde_json::to_value(updated)
                .map_err(|_| SubscriptionError::InvalidBody.code().to_owned())?;
            list
        }
        EndpointSubscriptionCommand::CancelRefresh { id } => {
            cancel_refresh(id);
            let _lock = config_store
                .lock_exclusive()
                .map_err(|_| SubscriptionError::StorageUnavailable.code().to_owned())?;
            let config = config_store
                .load_or_default()
                .map_err(|_| SubscriptionError::StorageUnavailable.code().to_owned())?;
            build_list_json(dir, &config, None)?
        }
        EndpointSubscriptionCommand::SelectEndpoint {
            subscription_id,
            endpoint_id,
            operation_id,
            zero_trust_risk_acknowledged,
        } => {
            if operation_id.is_nil() {
                return Err(SubscriptionError::InvalidRequest.code().to_owned());
            }
            let (tier, previous_sub_id, active_id) = {
                let _lock = config_store
                    .lock_exclusive()
                    .map_err(|_| SubscriptionError::StorageUnavailable.code().to_owned())?;
                let config = config_store
                    .load_or_default()
                    .map_err(|_| SubscriptionError::StorageUnavailable.code().to_owned())?;
                let active_id = config
                    .active_profile_id
                    .ok_or_else(|| SubscriptionError::VerifyAccount.code().to_owned())?;
                let tier = config
                    .account_tier(active_id)
                    .ok_or_else(|| SubscriptionError::VerifyAccount.code().to_owned())?;
                let prev = config
                    .runtime_profile(active_id)
                    .and_then(|p| p.endpoint.subscription_id);
                (tier, prev, active_id)
            };
            let catalog = sub_store
                .load_or_default()
                .map_err(|_| SubscriptionError::StorageUnavailable.code().to_owned())?;
            let sub = catalog
                .subscriptions
                .iter()
                .find(|s| s.id == subscription_id)
                .ok_or_else(|| SubscriptionError::NotFound.code().to_owned())?;
            if sub.account_tier != tier {
                return Err(SubscriptionError::TierMismatch.code().to_owned());
            }
            if !sub
                .endpoints
                .iter()
                .chain(sub.retained_missing_endpoint.iter())
                .any(|ep| ep.id == endpoint_id)
            {
                return Err(SubscriptionError::EndpointNotFound.code().to_owned());
            }
            if tier == EndpointPresetTier::ZeroTrust
                && previous_sub_id != Some(subscription_id)
                && !zero_trust_risk_acknowledged
            {
                return Err(SubscriptionError::ZeroTrustRiskRequired.code().to_owned());
            }

            let endpoint_id_copy = endpoint_id.clone();
            let mut rollback = None;
            let (_, selected_ep) = sub_store
                .update::<SubscriptionEndpoint, SubscriptionError>(|catalog| {
                    let sub = catalog
                        .subscriptions
                        .iter_mut()
                        .find(|s| s.id == subscription_id)
                        .ok_or(SubscriptionError::NotFound)?;
                    // The subscription may have been re-tiered since the read.
                    if sub.account_tier != tier {
                        return Err(SubscriptionError::TierMismatch);
                    }
                    rollback = Some((
                        sub.selected_endpoint_id.clone(),
                        sub.retained_missing_endpoint.clone(),
                    ));
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
                    sub.selected_endpoint_id = Some(ep.id.clone());
                    if sub.endpoints.iter().any(|item| item.id == ep.id) {
                        sub.retained_missing_endpoint = None;
                    }
                    sub.revision = Uuid::new_v4();
                    Ok(ep)
                })
                .map_err(|err| err.to_string())?;

            let committed = if previous_sub_id != Some(subscription_id) {
                config_store
                    .update(|latest| {
                        // Bind only the validated account, and only while it
                        // still has the tier the subscription and the Zero
                        // Trust confirmation were checked against.
                        if latest.active_profile_id != Some(active_id)
                            || latest.account_tier(active_id) != Some(tier)
                        {
                            return Err(StoreError::NetworkSettings(
                                SubscriptionError::VerifyAccount.code().into(),
                            ));
                        }
                        let current_active = active_id;
                        let mut patch_profile = latest
                            .runtime_profile(current_active)
                            .ok_or_else(|| StoreError::NetworkSettings("verify".into()))?;
                        patch_profile.endpoint.subscription_id = Some(subscription_id);
                        patch_profile.endpoint.selection = EndpointSelection::Custom;
                        patch_profile.endpoint.preset_region = None;
                        patch_profile.subscription_endpoint = Some(selected_ep.clone());
                        let patch = NetworkSettingsPatch {
                            operation_id,
                            account_id: current_active,
                            values: patch_profile,
                            changed_fields: vec!["endpoint.subscription_id".into()],
                        };
                        merge_patch(latest, &patch)
                            .map_err(|err| StoreError::NetworkSettings(err.to_string()))
                    })
                    .map_err(|error| match error {
                        StoreError::NetworkSettings(code)
                            if code == SubscriptionError::VerifyAccount.code() =>
                        {
                            code
                        }
                        _ => SubscriptionError::StorageUnavailable.code().to_owned(),
                    })
            } else {
                (|| {
                    let _lock = config_store
                        .lock_exclusive()
                        .map_err(|_| SubscriptionError::StorageUnavailable.code().to_owned())?;
                    let current = config_store
                        .load_or_default()
                        .map_err(|_| SubscriptionError::StorageUnavailable.code().to_owned())?;
                    let p = current
                        .runtime_profile(active_id)
                        .ok_or_else(|| SubscriptionError::VerifyAccount.code().to_owned())?;
                    Ok((current, p))
                })()
            };
            let (config, mut stored) = match committed {
                Ok(committed) => committed,
                Err(code) => {
                    // Restore the previous selection; the binding was not adopted.
                    if let Some((selected, retained)) = rollback {
                        let _ = sub_store.update::<(), StoreError>(|catalog| {
                            if let Some(sub) = catalog
                                .subscriptions
                                .iter_mut()
                                .find(|s| s.id == subscription_id)
                            {
                                sub.selected_endpoint_id = selected;
                                sub.retained_missing_endpoint = retained;
                                sub.revision = Uuid::new_v4();
                            }
                            Ok(())
                        });
                    }
                    return Err(code);
                }
            };
            stored.endpoint.subscription_id = Some(subscription_id);
            stored.subscription_endpoint = Some(selected_ep);

            let (status, target, settings_json) =
                crate::network_settings::record_subscription_mutation(
                    &config,
                    &stored,
                    operation_id,
                    &envelope.phase,
                    envelope.available,
                    &envelope.session_id,
                )?;
            let mut list = build_list_json(dir, &config, None)?;
            list["apply_status"] = serde_json::to_value(status).unwrap_or_default();
            attach_mutation(&mut list, Some((target, settings_json)));
            list
        }
        EndpointSubscriptionCommand::Deactivate { operation_id } => {
            if operation_id.is_nil() {
                return Err(SubscriptionError::InvalidRequest.code().to_owned());
            }
            let (config, stored) = config_store
                .update(|latest| {
                    let active_id = latest
                        .active_profile_id
                        .ok_or_else(|| StoreError::NetworkSettings("verify".into()))?;
                    let mut patch_profile = latest
                        .runtime_profile(active_id)
                        .ok_or_else(|| StoreError::NetworkSettings("verify".into()))?;
                    patch_profile.endpoint.subscription_id = None;
                    patch_profile.subscription_endpoint = None;
                    let patch = NetworkSettingsPatch {
                        operation_id,
                        account_id: active_id,
                        values: patch_profile,
                        changed_fields: vec!["endpoint.subscription_id".into()],
                    };
                    merge_patch(latest, &patch)
                        .map_err(|err| StoreError::NetworkSettings(err.to_string()))
                })
                .map_err(|_| SubscriptionError::StorageUnavailable.code().to_owned())?;
            let (status, target, settings_json) =
                crate::network_settings::record_subscription_mutation(
                    &config,
                    &stored,
                    operation_id,
                    &envelope.phase,
                    envelope.available,
                    &envelope.session_id,
                )?;
            let mut list = build_list_json(dir, &config, None)?;
            list["apply_status"] = serde_json::to_value(status).unwrap_or_default();
            attach_mutation(&mut list, Some((target, settings_json)));
            list
        }
        EndpointSubscriptionCommand::SetPinned {
            subscription_id,
            endpoint_id,
            pinned,
        } => {
            sub_store
                .update::<(), SubscriptionError>(|catalog| {
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
                .map_err(|err| err.to_string())?;
            let _lock = config_store
                .lock_exclusive()
                .map_err(|_| SubscriptionError::StorageUnavailable.code().to_owned())?;
            let config = config_store
                .load_or_default()
                .map_err(|_| SubscriptionError::StorageUnavailable.code().to_owned())?;
            build_list_json(dir, &config, None)?
        }
        EndpointSubscriptionCommand::StartProbe {
            subscription_id,
            endpoint_ids,
            probe_id,
        } => {
            if probe_id.is_nil() {
                return Err(SubscriptionError::InvalidRequest.code().to_owned());
            }
            let _lock = config_store
                .lock_exclusive()
                .map_err(|_| SubscriptionError::StorageUnavailable.code().to_owned())?;
            let config = config_store
                .load_or_default()
                .map_err(|_| SubscriptionError::StorageUnavailable.code().to_owned())?;
            let active_id = config
                .active_profile_id
                .ok_or_else(|| SubscriptionError::VerifyAccount.code().to_owned())?;
            let tier = config
                .account_tier(active_id)
                .ok_or_else(|| SubscriptionError::VerifyAccount.code().to_owned())?;
            let profile = config
                .runtime_profile(active_id)
                .ok_or_else(|| SubscriptionError::VerifyAccount.code().to_owned())?;
            let catalog = sub_store
                .load_or_default()
                .map_err(|_| SubscriptionError::StorageUnavailable.code().to_owned())?;
            let subscription = catalog
                .subscriptions
                .into_iter()
                .find(|s| s.id == subscription_id)
                .ok_or_else(|| SubscriptionError::NotFound.code().to_owned())?;
            if subscription.account_tier != tier {
                return Err(SubscriptionError::TierMismatch.code().to_owned());
            }
            let warp_identity = context
                .warp_identity
                .ok_or_else(|| "WARP_IDENTITY_REQUIRED".to_owned())?;
            let identity = MasqueTlsIdentity::from_warp_identity(&warp_identity)
                .map_err(|_| "WARP_IDENTITY_REQUIRED".to_owned())?;
            let sni = if profile.data_plane == DataPlaneMode::L4Proxy {
                identity
                    .l4_server_name()
                    .ok_or_else(|| "WARP_IDENTITY_REQUIRED".to_owned())?
                    .to_owned()
            } else {
                profile.endpoint.sni.clone()
            };

            let mut targets = Vec::new();
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
                return Err(SubscriptionError::EndpointNotFound.code().to_owned());
            }
            if context.runtime_cancel.is_none()
                && !(envelope.allow_physical && context.allow_physical)
            {
                return Err("SUBSCRIPTION_PROBE_UNAVAILABLE".to_owned());
            }

            let cancel = CancellationToken::new();
            if let Ok(mut slot) = active_probe_slot().lock()
                && let Some(prev) = slot.replace(ActiveProbeSession {
                    probe_id,
                    subscription_id,
                    cancel: cancel.clone(),
                })
            {
                prev.cancel.cancel();
            }

            let total = targets.len();
            let protector = context.protector;
            let runtime_cancel = context.runtime_cancel.unwrap_or_default();
            let emitter = context.on_probe_progress;
            let transport = profile.transport;
            let data_plane = profile.data_plane;
            let ip_policy = profile.ip_policy;
            let default_port = profile.endpoint.port;

            let spawned = std::thread::Builder::new()
                .name("usque-android-sub-probe".to_owned())
                .spawn(move || {
                    let Ok(runtime) = tokio::runtime::Builder::new_multi_thread()
                        .worker_threads(2)
                        .enable_all()
                        .build()
                    else {
                        if let Some(emit) = emitter.as_ref() {
                            emit_probe_progress(
                                emit.as_ref(),
                                probe_id,
                                subscription_id,
                                ProbeProgressUpdate {
                                    state: "failed",
                                    total,
                                    error_code: Some("SUBSCRIPTION_PROBE_UNAVAILABLE"),
                                    results: &[],
                                },
                            );
                        }
                        if let Ok(mut slot) = active_probe_slot().lock()
                            && slot.as_ref().is_some_and(|s| s.probe_id == probe_id)
                        {
                            *slot = None;
                        }
                        return;
                    };
                    runtime.block_on(async move {
                        let mut join_set = JoinSet::new();
                        let sem = Arc::new(Semaphore::new(PROBE_CONCURRENCY));
                        let mut results = Vec::with_capacity(total);
                        let mut final_state = "completed";
                        for ep in targets {
                            let sem = Arc::clone(&sem);
                            let sni = sni.clone();
                            let identity = identity.clone();
                            let protector = Arc::clone(&protector);
                            let cancel = cancel.clone();
                            let runtime_cancel = runtime_cancel.clone();
                            let port = ep.effective_port(default_port);
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
                                if let Some(emit) = emitter.as_ref() {
                                    emit_probe_progress(
                                        emit.as_ref(),
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
                        if cancel.is_cancelled() || runtime_cancel.is_cancelled() {
                            final_state = "cancelled";
                        }
                        if let Some(emit) = emitter.as_ref() {
                            emit_probe_progress(
                                emit.as_ref(),
                                probe_id,
                                subscription_id,
                                ProbeProgressUpdate {
                                    state: final_state,
                                    total,
                                    error_code: None,
                                    results: &results,
                                },
                            );
                        }
                        if let Ok(mut slot) = active_probe_slot().lock()
                            && slot.as_ref().is_some_and(|s| s.probe_id == probe_id)
                        {
                            *slot = None;
                        }
                    });
                });
            if spawned.is_err() {
                // No task owns this session; never leave it reported as running.
                if let Ok(mut slot) = active_probe_slot().lock()
                    && slot.as_ref().is_some_and(|s| s.probe_id == probe_id)
                {
                    *slot = None;
                }
                return Err("SUBSCRIPTION_PROBE_UNAVAILABLE".to_owned());
            }

            let mut list = build_list_json(dir, &config, None)?;
            list["probe_id"] = json!(probe_id);
            list
        }
        EndpointSubscriptionCommand::CancelProbe { probe_id } => {
            if let Ok(slot) = active_probe_slot().lock()
                && let Some(session) = slot.as_ref()
                && session.probe_id == probe_id
            {
                session.cancel.cancel();
            }
            let _lock = config_store
                .lock_exclusive()
                .map_err(|_| SubscriptionError::StorageUnavailable.code().to_owned())?;
            let config = config_store
                .load_or_default()
                .map_err(|_| SubscriptionError::StorageUnavailable.code().to_owned())?;
            build_list_json(dir, &config, None)?
        }
    };

    serde_json::to_string(&result).map_err(|_| SubscriptionError::InvalidRequest.code().to_owned())
}

fn attach_mutation(list: &mut Value, mutation: Option<(Option<Profile>, Value)>) {
    if let Some((target, settings_json)) = mutation {
        list["target"] = target
            .as_ref()
            .map(|p| crate::android_profile_value(p, None, false))
            .unwrap_or(Value::Null);
        list["network_settings"] = settings_json;
    }
}

fn cleanup_subscription_references(
    config_store: &ConfigStore,
    envelope: &Envelope,
    subscription_id: Uuid,
    keep_tier: Option<EndpointPresetTier>,
) -> Result<Option<(Option<Profile>, Value)>, String> {
    let (config, cleared_active) = config_store
        .update::<bool, StoreError>(|config| {
            let was_active = config
                .active_profile()
                .is_some_and(|p| p.endpoint.subscription_id == Some(subscription_id));
            if keep_tier != Some(EndpointPresetTier::Free)
                && config.network.consumer_tier_subscriptions.free == Some(subscription_id)
            {
                config.network.consumer_tier_subscriptions.free = None;
            }
            if keep_tier != Some(EndpointPresetTier::Plus)
                && config.network.consumer_tier_subscriptions.plus == Some(subscription_id)
            {
                config.network.consumer_tier_subscriptions.plus = None;
            }
            if keep_tier != Some(EndpointPresetTier::ZeroTrust) {
                for account in &mut config.profiles {
                    if account.zero_trust_subscription_id == Some(subscription_id) {
                        account.zero_trust_subscription_id = None;
                    }
                }
            }
            Ok(was_active)
        })
        .map_err(|_| SubscriptionError::StorageUnavailable.code().to_owned())?;
    if !cleared_active {
        return Ok(None);
    }
    let Some(mut stored) = config.active_profile() else {
        return Ok(None);
    };
    stored.subscription_endpoint = None;
    let operation_id = Uuid::new_v4();
    let (_, target, settings_json) = crate::network_settings::record_subscription_mutation(
        &config,
        &stored,
        operation_id,
        &envelope.phase,
        envelope.available,
        &envelope.session_id,
    )?;
    Ok(Some((target, settings_json)))
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

struct ProbeProgressUpdate<'a> {
    state: &'static str,
    total: usize,
    error_code: Option<&'static str>,
    results: &'a [ProbeEndpointResult],
}

fn emit_probe_progress(
    emit: &(dyn Fn(&str) + Send + Sync),
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
        emit(&progress_json);
    }
}

pub(crate) fn parse_phase(phase: &str) -> ConnectionPhase {
    match phase {
        "connected" => ConnectionPhase::Connected,
        "degraded" => ConnectionPhase::Degraded,
        "reconnecting" => ConnectionPhase::Reconnecting,
        _ => ConnectionPhase::Disconnected,
    }
}

pub(crate) fn plan_requires_runtime_target(
    applied: Option<&Profile>,
    stored: &Profile,
    phase: &str,
    available: bool,
) -> (ApplyStatus, Option<Profile>) {
    let conn_phase = parse_phase(phase);
    let confirmed = available
        && matches!(
            conn_phase,
            ConnectionPhase::Connected | ConnectionPhase::Degraded
        );
    if !confirmed {
        return (ApplyStatus::Deferred, None);
    }
    match usque_core::network_settings::plan_application(
        applied,
        stored,
        &[String::from("endpoint.subscription_id")],
        conn_phase,
        true,
    ) {
        Ok(plan) if plan.class != usque_core::ReconfigureClass::Reject => {
            (plan.status, plan.target)
        }
        Ok(_) => (ApplyStatus::Failed, None),
        Err(_) => (ApplyStatus::Failed, None),
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn clearing_joins_downloads_that_are_still_returning() {
        let download = std::thread::spawn(|| {
            let _in_flight = InFlightDownload::begin();
            std::thread::sleep(Duration::from_millis(150));
        });
        std::thread::sleep(Duration::from_millis(20));
        assert!(join_in_flight_downloads(Duration::from_secs(5)));
        download.join().unwrap();
        assert!(join_in_flight_downloads(Duration::ZERO));
    }
    use super::*;
    use usque_core::endpoint_subscriptions::SubscriptionEndpointHost;
    use usque_core::{ConsumerEntitlement, IdentityProvider, ManagedEndpointIps};

    fn seed_sub(dir: &Path, tier: EndpointPresetTier) -> EndpointSubscription {
        let store = EndpointSubscriptionStore::in_directory(dir);
        let ep1 = SubscriptionEndpoint {
            id: "v4:162.159.193.10:2408".into(),
            host: SubscriptionEndpointHost::Ipv4("162.159.193.10".parse().unwrap()),
            port: Some(2408),
            note: "Edge A".into(),
            pinned: false,
        };
        let sub = EndpointSubscription {
            id: Uuid::new_v4(),
            revision: Uuid::new_v4(),
            name: "Android Sub".into(),
            url: "https://example.com/android.txt".into(),
            account_tier: tier,
            auto_refresh: SubscriptionRefreshInterval::Off,
            last_refreshed_at_unix_ms: Some(1_000),
            next_refresh_at_unix_ms: None,
            last_refresh_error: None,
            skipped_line_count: 0,
            endpoints: vec![ep1.clone()],
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

    #[test]
    fn android_command_enforces_tier_zt_gate_and_scene_dependency_preservation() {
        let _guard = crate::network_settings::TEST_STATE.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profiles-v2.json");
        let path_str = path.to_str().unwrap();

        let mut cfg = AppConfig::default();
        let active_id = cfg.active_profile_id.unwrap();
        cfg.identity_bindings
            .insert(active_id, IdentityProvider::Consumer);
        cfg.set_consumer_entitlement(active_id, Some(ConsumerEntitlement::Free));
        ConfigStore::new(&path).save(&cfg).unwrap();
        crate::network_settings::command(path_str, r#"{"command":"reset"}"#).unwrap();

        let free_sub = seed_sub(dir.path(), EndpointPresetTier::Free);
        let plus_sub = seed_sub(dir.path(), EndpointPresetTier::Plus);

        // Tier mismatch fails closed.
        let mismatch = command(
            path_str,
            &json!({
                "request": {
                    "action": "select_endpoint",
                    "subscription_id": plus_sub.id,
                    "endpoint_id": "v4:162.159.193.10:2408",
                    "operation_id": Uuid::new_v4(),
                }
            })
            .to_string(),
            AndroidFetchContext::default(),
        )
        .unwrap_err();
        assert_eq!(mismatch, "SUBSCRIPTION_TIER_MISMATCH");

        // Selecting Free subscription succeeds and hydrates runtime profile.
        let selected: Value = serde_json::from_str(
            &command(
                path_str,
                &json!({
                    "request": {
                        "action": "select_endpoint",
                        "subscription_id": free_sub.id,
                        "endpoint_id": "v4:162.159.193.10:2408",
                        "operation_id": Uuid::new_v4(),
                    }
                })
                .to_string(),
                AndroidFetchContext::default(),
            )
            .unwrap(),
        )
        .unwrap();
        let current_rev = selected["subscriptions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["id"].as_str() == Some(free_sub.id.to_string().as_str()))
            .unwrap()["revision"]
            .as_str()
            .unwrap()
            .to_owned();

        let mut loaded_cfg = ConfigStore::new(&path).load().unwrap();
        let mut hydrated = loaded_cfg.runtime_profile(active_id).unwrap();
        hydrate_profile_in_dir(dir.path(), Some(&loaded_cfg), &mut hydrated).unwrap();
        assert_eq!(
            hydrated.subscription_endpoint.as_ref().unwrap().id,
            "v4:162.159.193.10:2408"
        );

        // Delete preserves scene subscription_id while clearing active binding.
        let mut settings = usque_core::scenarios::ScenarioSettings::from_profile(&hydrated);
        settings.endpoint.subscription_id = Some(free_sub.id);
        loaded_cfg
            .scenarios
            .push(usque_core::scenarios::ConnectionScenario {
                id: Uuid::new_v4(),
                name: "Android Scene".into(),
                revision: Uuid::new_v4(),
                bound_account_id: None,
                settings,
            });
        ConfigStore::new(&path).save(&loaded_cfg).unwrap();

        command(
            path_str,
            &json!({
                "request": {
                    "action": "delete",
                    "id": free_sub.id,
                    "expected_revision": current_rev,
                }
            })
            .to_string(),
            AndroidFetchContext::default(),
        )
        .unwrap();

        let after_del = ConfigStore::new(&path).load().unwrap();
        assert_eq!(after_del.network.consumer_tier_subscriptions.free, None);
        assert_eq!(
            after_del.scenarios[0].settings.endpoint.subscription_id,
            Some(free_sub.id)
        );
        assert_eq!(
            validate_scenario_subscription_at(
                dir.path(),
                &hydrated,
                Some(EndpointPresetTier::Free)
            )
            .unwrap_err(),
            "SCENARIO_DEPENDENCY_UNAVAILABLE"
        );

        // ZT risk gate fails closed without explicit acknowledgement.
        let zt_id = Uuid::new_v4();
        let zt_sub = seed_sub(dir.path(), EndpointPresetTier::ZeroTrust);
        let mut zt_cfg = after_del;
        zt_cfg
            .insert_account(
                zt_id,
                "ZT".into(),
                Some(ManagedEndpointIps {
                    ipv4: "162.159.197.2".parse().unwrap(),
                    ipv6: "2606:4700:102::2".parse().unwrap(),
                }),
            )
            .unwrap();
        zt_cfg
            .identity_bindings
            .insert(zt_id, IdentityProvider::zero_trust("team-acme").unwrap());
        zt_cfg.active_profile_id = Some(zt_id);
        ConfigStore::new(&path).save(&zt_cfg).unwrap();

        let zt_err = command(
            path_str,
            &json!({
                "request": {
                    "action": "select_endpoint",
                    "subscription_id": zt_sub.id,
                    "endpoint_id": "v4:162.159.193.10:2408",
                    "operation_id": Uuid::new_v4(),
                }
            })
            .to_string(),
            AndroidFetchContext::default(),
        )
        .unwrap_err();
        assert_eq!(zt_err, "SUBSCRIPTION_ZT_RISK_REQUIRED");
    }
}
