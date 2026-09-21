//! Catalogue jobs belong to the VpnService process. No TUN, proxy listener,
//! or system setting is created for a temporary download session.
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use usque_core::{Profile, WarpIdentity, storage::ConfigStore, vpngate::*};
use usque_transport::{
    DataPlaneRuntime, GeoDirectPolicy, InternalNetwork, MasqueTlsIdentity, RuntimeHealth,
    SocketProtector,
};

static CATALOGUE: OnceLock<Mutex<Option<Controller>>> = OnceLock::new();
struct Controller {
    path: PathBuf,
    downloader: Arc<DirectoryDownloader>,
    job: Option<Job>,
}
struct Job {
    cancellation: CancellationToken,
    done: Arc<(Mutex<bool>, Condvar)>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Job {
    fn finished(&self) -> bool {
        *self.done.0.lock().unwrap_or_else(|e| e.into_inner())
    }
    fn stop(&mut self) -> bool {
        self.cancellation.cancel();
        let done = self.done.0.lock().unwrap_or_else(|e| e.into_inner());
        let (done, _) = self
            .done
            .1
            .wait_timeout_while(done, Duration::from_secs(45), |done| !*done)
            .unwrap_or_else(|e| e.into_inner());
        if !*done {
            return false;
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        true
    }
}

pub(crate) fn cancel_refresh() -> bool {
    let Some(controller) = CATALOGUE.get() else {
        return true;
    };
    let Ok(mut controller) = controller.lock() else {
        return false;
    };
    let Some(controller) = controller.as_mut() else {
        return true;
    };
    if let Some(job) = &mut controller.job
        && !job.stop()
    {
        return false;
    }
    controller.job = None;
    true
}

pub(crate) fn signal_cancel() {
    if let Some(controller) = CATALOGUE.get()
        && let Ok(controller) = controller.try_lock()
        && let Some(controller) = controller.as_ref()
        && let Some(job) = &controller.job
    {
        job.cancellation.cancel();
    }
}

#[derive(Deserialize)]
pub(crate) struct Request {
    pub command: String,
    pub chain_profile: Option<usque_core::chain_exit::ChainProfileRequest>,
    #[serde(default)]
    pub cancel: bool,
    #[serde(flatten)]
    query: ListQuery,
    #[serde(flatten)]
    node: NodeRequest,
}
impl Request {
    pub fn needs_fetch(&self) -> bool {
        (self.command == "refresh" && !self.cancel)
            || (self.command == "node"
                && matches!(
                    self.node.action,
                    NodeAction::Prepare | NodeAction::Favorite | NodeAction::UpdateFavorite
                ))
    }
}
pub(crate) fn parse_request(json: &str) -> Result<Request, String> {
    if json.len() > 256 * 1024 {
        return Err("VPN_GATE_REQUEST_INVALID".into());
    }
    let request: Request = serde_json::from_str(json).map_err(|_| "VPN_GATE_REQUEST_INVALID")?;
    if request.command != "chain_profile" && json.len() > 4096 {
        return Err("VPN_GATE_REQUEST_INVALID".into());
    }
    if request.command == "node" {
        request
            .node
            .validate()
            .map_err(|_| "VPN_GATE_REQUEST_INVALID")?;
    }
    Ok(request)
}

pub(crate) struct FetchContext {
    pub identity: Option<WarpIdentity>,
    pub protector: Arc<dyn SocketProtector>,
    pub networks: Option<(InternalNetwork, InternalNetwork)>,
    pub allow_physical: bool,
    pub refresher: Option<Arc<dyn usque_transport::EndpointPinRefresher>>,
}

pub(crate) fn command(
    path: &Path,
    request: Request,
    fetch: Option<FetchContext>,
    status: GateStatus,
) -> Result<String, String> {
    if !path.is_absolute() || path.file_name().and_then(|s| s.to_str()) != Some("profiles-v2.json")
    {
        return Err("VPN_GATE_REQUEST_INVALID".into());
    }
    if request.cancel && !cancel_refresh() {
        return Err("VPN_GATE_CLEANUP_PENDING".into());
    }
    let cache = path.parent().ok_or("VPN_GATE_REQUEST_INVALID")?;
    if request.command == "chain_profile" {
        return crate::chain_exit::command(
            path,
            request.chain_profile.ok_or("CHAIN_REQUEST_INVALID")?,
            &status,
        );
    }
    let mut slot = CATALOGUE
        .get_or_init(|| Mutex::new(None))
        .lock()
        .map_err(|_| "VPN_GATE_UNAVAILABLE")?;
    if slot.is_none() {
        let config = ConfigStore::new(path)
            .load()
            .map_err(|_| "VPN_GATE_UNAVAILABLE")?;
        let mut retained: Vec<_> = config.network.vpn_gate.selection.into_iter().collect();
        if let Some(server) = &status.current_server {
            retained.push(Selection {
                server_id: server.id.clone(),
                config_sha256: server.config_sha256.clone(),
            });
        }
        let _ = CatalogueStore::new(cache).recover_references(&retained);
        *slot = Some(Controller {
            path: path.into(),
            downloader: Arc::new(DirectoryDownloader::new(CatalogueStore::new(cache))),
            job: None,
        });
    }
    let controller = slot.as_mut().ok_or("VPN_GATE_UNAVAILABLE")?;
    if controller.path != path {
        return Err("VPN_GATE_REQUEST_INVALID".into());
    }
    if request.command == "node" {
        match request.node.action {
            NodeAction::Cancel => controller
                .downloader
                .cancel_node(&request.node.operation_id),
            NodeAction::Release => {
                CatalogueStore::new(cache).release_prepared(&request.node.selection());
            }
            NodeAction::RemoveFavorite => {
                controller
                    .downloader
                    .cancel_node_for(&request.node.server_id);
                let mut retained: Vec<_> = ConfigStore::new(path)
                    .load()
                    .map_err(|_| "VPN_GATE_UNAVAILABLE")?
                    .network
                    .vpn_gate
                    .selection
                    .into_iter()
                    .collect();
                if let Some(server) = &status.current_server {
                    retained.push(Selection {
                        server_id: server.id.clone(),
                        config_sha256: server.config_sha256.clone(),
                    });
                }
                CatalogueStore::new(cache)
                    .remove_favorite(&request.node, &retained)
                    .map_err(|e| e.to_string())?;
            }
            _ => {
                if let Some(mut previous) = controller.job.take()
                    && !previous.stop()
                {
                    controller.job = Some(previous);
                    return Err("VPN_GATE_CLEANUP_PENDING".into());
                }
                controller.downloader.begin_node(&request.node);
            }
        }
    }
    let node_request = (request.command == "node").then(|| request.node.clone());
    match request.command.as_str() {
        "list" => {}
        "refresh" if request.cancel => {}
        "node" if !request.needs_fetch() => {}
        "refresh" | "node" => {
            if !controller.job.as_ref().is_some_and(|job| !job.finished()) {
                if let Some(mut previous) = controller.job.take() {
                    previous.stop();
                }
                let context = fetch.ok_or("VPN_GATE_UNAVAILABLE")?;
                let store = ConfigStore::new(path);
                let config = store
                    .load_or_default()
                    .map_err(|_| "VPN_GATE_UNAVAILABLE")?;
                let profile = config.active_profile().ok_or("VPN_GATE_UNAVAILABLE")?;
                if config
                    .pending_identity_replacements
                    .contains_key(&profile.id)
                {
                    return Err("VPN_GATE_UNAVAILABLE".into());
                }
                let binding = config.identity_bindings.get(&profile.id).cloned();
                let cancellation = CancellationToken::new();
                let done = Arc::new((Mutex::new(false), Condvar::new()));
                let finished = done.clone();
                let cancel = cancellation.clone();
                let downloader = controller.downloader.clone();
                let thread = std::thread::Builder::new().name("usque-catalogue".into()).spawn(move || {
                    // Even an unexpected panic must release the lifecycle waiter.
                    struct Complete(Arc<(Mutex<bool>, Condvar)>);
                    impl Drop for Complete {
                        fn drop(&mut self) { *self.0.0.lock().unwrap_or_else(|e| e.into_inner()) = true; self.0.1.notify_all(); }
                    }
                    let _complete = Complete(finished);
                    if let Ok(runtime) = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build() {
                        runtime.block_on(async move {
                            let primary: Arc<dyn CatalogueHttp> = match &context.networks {
                                Some((network, _)) if matches!(network.health_snapshot(), RuntimeHealth::Connected { .. }) => Arc::new(network.clone()),
                                _ if context.allow_physical => match DirectCatalogueHttp::new() { Ok(http) => Arc::new(http), Err(_) => Arc::new(Unavailable) },
                                _ => Arc::new(Unavailable),
                            };
                            let source = Source { profile: profile.clone(), identity: tokio::sync::Mutex::new(context.identity),
                                protector: context.protector, existing: context.networks.map(|(_, warp)| warp), allow_physical: context.allow_physical, refresher: context.refresher };
                            let watch_cancel = cancel.clone();
                            let _watch = tokio_util::task::AbortOnDropHandle::new(tokio::spawn(async move {
                                loop {
                                    tokio::select! {
                                        _ = watch_cancel.cancelled() => break,
                                        _ = tokio::time::sleep(Duration::from_millis(200)) => {}
                                    }
                                    let store = store.clone();
                                    let current = tokio::task::spawn_blocking(move || store.load()).await;
                                    let same = current.ok().and_then(Result::ok).is_some_and(|config|
                                        config.active_profile().as_ref() == Some(&profile)
                                        && config.identity_bindings.get(&profile.id) == binding.as_ref()
                                        && !config.pending_identity_replacements.contains_key(&profile.id));
                                    if !same { watch_cancel.cancel(); break; }
                                }
                            }));
                            if let Some(request) = node_request {
                                let _ = downloader.node_operation(&request, primary, &source, &cancel).await;
                            } else {
                                let _ = downloader.refresh(primary, &source, &cancel).await;
                            }
                        });
                    }
                }).map_err(|_| "VPN_GATE_UNAVAILABLE")?;
                controller.job = Some(Job {
                    cancellation,
                    done,
                    thread: Some(thread),
                });
            }
        }
        _ => return Err("VPN_GATE_REQUEST_INVALID".into()),
    }
    let store = CatalogueStore::new(cache);
    // Node commands acknowledge a local mutation/job dispatch. A corrupt
    // disposable pool cache must not turn that successful command into an error.
    let mut query = request.query;
    if request.command == "node" {
        query.favorites_only = true;
        query.limit = 1;
        query.status_only = true;
    }
    let list = match store.list(&query) {
        Ok(list) => list,
        Err(_) if request.cancel => Default::default(),
        Err(_) => return Err("VPN_GATE_CACHE_INVALID".into()),
    };
    let mut progress = controller.downloader.progress();
    if request.cancel {
        progress.stage = DownloadStage::Cancelled;
    } else if controller.job.as_ref().is_some_and(Job::finished)
        && !matches!(
            progress.stage,
            DownloadStage::Complete | DownloadStage::Failed | DownloadStage::Cancelled
        )
    {
        progress.stage = if controller
            .job
            .as_ref()
            .is_some_and(|job| job.cancellation.is_cancelled())
        {
            DownloadStage::Cancelled
        } else {
            DownloadStage::Failed
        };
        progress.failures.push(StageFailure {
            stage: progress.stage,
            source_url: None,
            error: "VPN_GATE_UNAVAILABLE".into(),
        });
    }
    let mut value = serde_json::to_value(&list).map_err(|_| "VPN_GATE_UNAVAILABLE")?;
    value["node_progress"] = json!(controller.downloader.node_progress());
    value["refresh_stage"] = json!(progress.stage);
    value["refresh_failures"] = json!(
        progress
            .failures
            .iter()
            .map(|f| format!(
                "{:?}: {}: {}",
                f.stage,
                f.source_url.as_deref().unwrap_or("WARP"),
                f.error
            ))
            .collect::<Vec<_>>()
    );
    value["cached"] = json!(
        list.fetched_at_unix_ms.is_some()
            && (progress.stage != DownloadStage::Complete
                || progress.fetched_at_unix_ms != list.fetched_at_unix_ms)
    );
    value["status"] = serde_json::to_value(status).unwrap_or(Value::Null);
    let saved = if query.status_only {
        None
    } else {
        ConfigStore::new(path)
            .load()
            .ok()
            .and_then(|config| config.network.vpn_gate.selection)
            .and_then(|selection| {
                store
                    .load_selection(&selection)
                    .ok()
                    .map(|(server, _)| server)
            })
    };
    value["saved_server"] = json!(saved);
    serde_json::to_string(&value).map_err(|_| "VPN_GATE_UNAVAILABLE".into())
}

struct Unavailable;
#[async_trait::async_trait]
impl CatalogueHttp for Unavailable {
    async fn get(&self, _: &str, _: &CancellationToken) -> Result<Vec<u8>, DirectoryError> {
        Err(DirectoryError::Unavailable)
    }
}
struct Source {
    profile: Profile,
    identity: tokio::sync::Mutex<Option<WarpIdentity>>,
    protector: Arc<dyn SocketProtector>,
    existing: Option<InternalNetwork>,
    allow_physical: bool,
    refresher: Option<Arc<dyn usque_transport::EndpointPinRefresher>>,
}
#[async_trait::async_trait]
impl WarpCatalogueSource for Source {
    async fn open(
        &self,
        cancel: &CancellationToken,
    ) -> Result<Arc<dyn CatalogueHttp>, DirectoryError> {
        if let Some(network) = &self.existing {
            return Ok(Arc::new(network.clone()));
        }
        if !self.allow_physical {
            return Err(DirectoryError::Unavailable);
        }
        let identity = self
            .identity
            .lock()
            .await
            .take()
            .ok_or(DirectoryError::Unavailable)?;
        let identity = MasqueTlsIdentity::from_warp_identity(&identity)
            .map_err(|_| DirectoryError::Unavailable)?;
        let profile = DataPlaneRuntime::headless_profile(&self.profile);
        let runtime = tokio::select! {
            _ = cancel.cancelled() => return Err(DirectoryError::Cancelled),
            result = Box::pin(DataPlaneRuntime::start_with_geo_policy(&profile, identity, self.protector.clone(), self.refresher.clone(), Arc::new(GeoDirectPolicy::disabled()))) => result.map_err(|_| DirectoryError::Unavailable)?,
        };
        Ok(Arc::new(OwnedHttp {
            network: runtime.warp_internal_network(),
            runtime: tokio::sync::Mutex::new(Some(runtime)),
        }))
    }
}
struct OwnedHttp {
    network: InternalNetwork,
    runtime: tokio::sync::Mutex<Option<DataPlaneRuntime>>,
}
#[async_trait::async_trait]
impl CatalogueHttp for OwnedHttp {
    async fn get(&self, url: &str, cancel: &CancellationToken) -> Result<Vec<u8>, DirectoryError> {
        self.network.get(url, cancel).await
    }
    async fn close(&self) {
        if let Some(mut runtime) = self.runtime.lock().await.take() {
            runtime.shutdown().await;
        }
    }
}

pub(crate) fn pin_settings(
    path: &Path,
    requested: &VpnGateSettings,
    previous: &VpnGateSettings,
) -> Result<(), String> {
    requested
        .validate()
        .map_err(|_| "VPN_GATE_SELECTION_STALE")?;
    if let Some(selection) = &requested.selection {
        let store = CatalogueStore::new(path.parent().ok_or("VPN_GATE_SELECTION_STALE")?);
        if previous.selection.as_ref() == Some(selection) {
            store.load_selection(selection).map(|_| ())
        } else {
            store.pin(selection).map(|_| ())
        }
        .map_err(|_| "VPN_GATE_SELECTION_STALE")?;
    }
    Ok(())
}
