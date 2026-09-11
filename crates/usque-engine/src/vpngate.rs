use crate::{ControlService, ControlServiceError, VaultEndpointPinRefresher};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use usque_core::vpngate::*;
use usque_ipc::v1;
use usque_transport::{
    DataPlaneRuntime, GeoDirectPolicy, InternalNetwork, MasqueTlsIdentity, NoopSocketProtector,
    RuntimeHealth,
};

pub(crate) struct FetchTask {
    task: tokio::task::JoinHandle<()>,
    cancellation: CancellationToken,
}

impl ControlService {
    /// A fresh explicit request may start after a completed cancellation.
    /// Internal retries never reset the user's stop signal.
    pub(crate) async fn gate_connection_request(&self) -> CancellationToken {
        let mut cancellation = self.gate_startup_cancel.lock().await;
        if cancellation.is_cancelled() {
            *cancellation = CancellationToken::new();
        }
        cancellation.clone()
    }
    pub(crate) async fn ensure_gate_supervisor(&self) {
        let mut task = self.gate_supervisor.lock().await;
        if task.is_some() {
            return;
        }
        let weak = Arc::downgrade(&self.inner);
        *task = Some(tokio_util::task::AbortOnDropHandle::new(tokio::spawn(
            async move {
                let mut tick = tokio::time::interval(std::time::Duration::from_secs(1));
                tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                let mut retry = usque_transport::VpnGateRetry::default();
                let mut target = None;
                loop {
                    tick.tick().await;
                    let Some(inner) = weak.upgrade() else {
                        break;
                    };
                    let service = ControlService { inner };
                    let Ok(_mutation) = service.mutation_lock.clone().try_lock_owned() else {
                        continue;
                    };
                    let candidate = {
                        let active = service.data_plane.lock().await;
                        active
                            .as_ref()
                            .filter(|active| active.profile.vpn_gate.enabled)
                            .map(|active| {
                                let ready = active.runtime.needs_warp_bootstrap()
                                    || active.runtime.internal_networks().is_some_and(
                                        |(_, warp)| {
                                            matches!(
                                                warp.health_snapshot(),
                                                RuntimeHealth::Connected { .. }
                                            )
                                        },
                                    );
                                (active.profile.clone(), active.runtime.gate_status(), ready)
                            })
                    };
                    let Some((profile, status, warp_ready)) = candidate else {
                        target = None;
                        retry = Default::default();
                        continue;
                    };
                    let key = (profile.id, profile.vpn_gate.clone());
                    if target.as_ref() != Some(&key) {
                        target = Some(key);
                        retry = Default::default();
                    }
                    if retry.due(&status, warp_ready, tokio::time::Instant::now()) {
                        service.cancel_gate_refresh().await;
                        let _ = service.hot_replace_gate(&profile).await;
                    }
                }
            },
        )));
    }
    pub(crate) async fn hot_replace_gate(
        &self,
        profile: &usque_core::Profile,
    ) -> Result<(), ControlServiceError> {
        let startup_cancel = self.gate_startup_cancel.lock().await.clone();
        self.hot_replace_gate_with_cancellation(profile, &startup_cancel)
            .await
    }
    pub(crate) async fn hot_replace_gate_with_cancellation(
        &self,
        profile: &usque_core::Profile,
        startup_cancel: &CancellationToken,
    ) -> Result<(), ControlServiceError> {
        if startup_cancel.is_cancelled() {
            return Err(ControlServiceError::Transport(
                usque_transport::TransportError::TunnelClosed,
            ));
        }
        self.abort_exit_probe().await;
        let mut active = self
            .data_plane
            .lock()
            .await
            .take()
            .ok_or_else(|| error(DirectoryError::Unavailable))?;
        active.runtime.quiesce_final();
        active.profile = profile.clone();
        active.session_generation = self.next_session_generation();
        *self.session_profile.lock().await = Some(profile.clone());
        {
            let mut state = self.state.lock().await;
            state.clear_exit_info();
            let _ = state.transition(usque_core::ConnectionPhase::Reconnecting);
        }
        let result = async {
            let selected = self.prepare_gate_selection(profile)?;
            let policy = Arc::new(crate::load_geo_direct_policy(profile, &self.cache_dir));
            match &mut active.runtime {
                crate::active_runtime::ActiveRuntime::Proxy(runtime) => {
                    runtime
                        .runtime
                        .replace_gate(
                            profile,
                            selected,
                            policy,
                            self.gate_status.clone(),
                            startup_cancel,
                        )
                        .await
                        .map_err(ControlServiceError::Transport)?;
                    #[cfg(windows)]
                    if profile.frontends.http
                        && profile.proxy.system_proxy
                        && runtime.system_proxy.is_none()
                    {
                        let listener = crate::windows_agent::loopback_http_listener(
                            runtime.runtime.http_listeners(),
                        )
                        .ok_or_else(|| {
                            ControlServiceError::PlatformVpn(
                                "system proxy requires a loopback HTTP listener".into(),
                            )
                        })?;
                        runtime.system_proxy = Some(
                            crate::windows_agent::WindowsSystemProxyGuard::start(listener)
                                .await
                                .map_err(crate::map_windows_vpn_error)?,
                        );
                    }
                    Ok(())
                }
                #[cfg(windows)]
                crate::active_runtime::ActiveRuntime::Vpn(runtime) => runtime
                    .replace_gate(
                        profile,
                        selected,
                        policy,
                        self.gate_status.clone(),
                        startup_cancel,
                    )
                    .await
                    .map_err(crate::map_windows_vpn_error),
                #[cfg(test)]
                crate::active_runtime::ActiveRuntime::Harness(_) => Ok(()),
            }
        }
        .await;
        if let Err(error) = &result {
            let reason = match error {
                ControlServiceError::Transport(usque_transport::TransportError::VpnGate(
                    reason,
                )) => *reason,
                ControlServiceError::VpnGate(_) => GateFailure::Configuration,
                ControlServiceError::Transport(error) if !error.failure(None, None).retryable => {
                    GateFailure::Configuration
                }
                _ => GateFailure::Transport,
            };
            active.runtime.fail_gate(reason).await;
            self.gate_status.send_modify(|s| {
                s.stage = GateStage::Error;
                s.failure = Some(reason);
                s.network = None;
            });
        }
        let generation = active.session_generation;
        let quality = active.runtime.subscribe_network_quality();
        *self.data_plane.lock().await = Some(active);
        if let Err(error) = result {
            self.mark_connection_error(&error).await;
            return Err(error);
        }
        self.install_network_quality_source(quality).await;
        self.spawn_gate_exit_probe(profile.id, generation).await;
        Ok(())
    }
    pub(crate) async fn spawn_gate_exit_probe(&self, profile_id: uuid::Uuid, generation: u64) {
        self.abort_exit_probe().await;
        let network = self
            .data_plane
            .lock()
            .await
            .as_ref()
            .filter(|active| matches!(active.runtime.health(), RuntimeHealth::Connected { .. }))
            .and_then(|active| active.runtime.internal_networks())
            .map(|(network, _)| network);
        let Some(network) = network else {
            return;
        };
        let state = self.state.clone();
        let data_plane = self.data_plane.clone();
        *self.exit_probe_task.lock().await = Some(tokio::spawn(async move {
            if let Ok(exit) = network.probe_exit().await {
                crate::apply_exit_info(&state, &data_plane, profile_id, generation, exit).await;
            }
        }));
    }
    pub(crate) async fn cancel_gate_refresh(&self) {
        let mut running = self.gate_fetch_task.lock().await;
        if let Some(job) = running.take() {
            job.cancellation.cancel();
            let _ = job.task.await;
        }
    }
    pub(crate) async fn list_vpn_gate(
        &self,
        request: v1::ListVpnGateRequest,
    ) -> Result<v1::VpnGateDirectory, ControlServiceError> {
        let query = ListQuery {
            country_code: (!request.country_code.is_empty()).then_some(request.country_code),
            unknown_country: request.unknown_country,
            offset: request.offset as usize,
            limit: request.limit as usize,
            include_unsupported: request.include_unsupported,
        };
        let store = CatalogueStore::new(&self.cache_dir);
        let selected = self.config.read().await.network.vpn_gate.selection.clone();
        let (list, saved) = tokio::task::spawn_blocking(move || {
            let saved = selected.and_then(|selection| {
                store
                    .load_selection(&selection)
                    .ok()
                    .map(|(server, _)| server)
            });
            store.list(&query).map(|list| (list, saved))
        })
        .await
        .map_err(|_| error(DirectoryError::CacheIo))?
        .map_err(error)?;
        let progress = self.gate_directory.progress();
        let (status, warp_stage) = if let Ok(active) = self.data_plane.try_lock()
            && let Some(active) = active.as_ref()
        {
            (
                active.runtime.gate_status(),
                active
                    .runtime
                    .internal_networks()
                    .map(|(_, warp)| health_name(&warp.health_snapshot())),
            )
        } else {
            (self.gate_status.borrow().clone(), None)
        };
        Ok(v1::VpnGateDirectory {
            saved_server: saved.as_ref().map(server_to_proto),
            servers: list.servers.iter().map(server_to_proto).collect(),
            countries: list
                .countries
                .iter()
                .map(|country| v1::VpnGateCountry {
                    country_code: country.country_code.clone().unwrap_or_default(),
                    country_name: country.country_name.clone().unwrap_or_default(),
                    server_count: country.server_count as u32,
                })
                .collect(),
            total: list.total as u32,
            source_server_count: list.source_server_count as u32,
            fetched_at_unix_ms: list.fetched_at_unix_ms,
            source_url: list.source_url.unwrap_or_default(),
            refresh_stage: enum_name(progress.stage),
            refresh_failures: progress
                .failures
                .iter()
                .map(|failure| {
                    format!(
                        "{}: {}: {}",
                        enum_name(failure.stage),
                        failure.source_url.as_deref().unwrap_or("WARP"),
                        failure.error
                    )
                })
                .collect(),
            cached: list.fetched_at_unix_ms.is_some() && progress.stage != DownloadStage::Complete,
            status: Some(status_to_proto(&status, warp_stage)),
        })
    }

    pub(crate) async fn refresh_vpn_gate(&self) -> Result<(), ControlServiceError> {
        let mut running = self.gate_fetch_task.lock().await;
        if running.as_ref().is_some_and(|job| !job.task.is_finished()) {
            return Ok(());
        }
        // The normal lifecycle cancels this job before waiting on the lock.
        // Account replacement and temporary-session identity use cannot race.
        let lifecycle = self
            .mutation_lock
            .clone()
            .try_lock_owned()
            .map_err(|_| error(DirectoryError::Unavailable))?;
        let profile = self
            .config
            .read()
            .await
            .active_profile()
            .ok_or_else(|| error(DirectoryError::Unavailable))?;
        let networks = self
            .data_plane
            .lock()
            .await
            .as_ref()
            .and_then(|active| active.runtime.internal_networks());
        let allow_physical = if networks.is_none() {
            #[cfg(windows)]
            {
                crate::windows_agent::catalogue_physical_network_permitted().await
            }
            #[cfg(not(windows))]
            {
                true
            }
        } else {
            false
        };
        let primary: Arc<dyn CatalogueHttp> = match &networks {
            Some((network, _))
                if matches!(network.health_snapshot(), RuntimeHealth::Connected { .. }) =>
            {
                Arc::new(network.clone())
            }
            _ if allow_physical => Arc::new(DirectCatalogueHttp::new().map_err(error)?),
            _ => Arc::new(UnavailableHttp),
        };
        let service = self.clone();
        let cancellation = CancellationToken::new();
        let parent = cancellation.clone();
        *running = Some(FetchTask {
            cancellation,
            task: tokio::spawn(async move {
                let _lifecycle = lifecycle;
                let source = WarpSource {
                    service: service.clone(),
                    profile,
                    existing: networks.map(|(_, warp)| warp),
                    allow_physical,
                };
                let _ = service
                    .gate_directory
                    .refresh(primary, &source, &parent)
                    .await;
            }),
        });
        Ok(())
    }

    pub(crate) fn prepare_gate_selection(
        &self,
        profile: &usque_core::Profile,
    ) -> Result<Option<(ServerSummary, PreparedProfile)>, ControlServiceError> {
        if !profile.vpn_gate.enabled {
            return Ok(None);
        }
        let selection = profile
            .vpn_gate
            .selection
            .as_ref()
            .ok_or_else(|| error(DirectoryError::StaleSelection))?;
        CatalogueStore::new(&self.cache_dir)
            .load_selection(selection)
            .map(Some)
            .map_err(error)
    }

    pub(crate) async fn pin_gate_settings(
        &self,
        requested: &VpnGateSettings,
    ) -> Result<(), ControlServiceError> {
        requested.validate().map_err(error)?;
        let Some(selection) = &requested.selection else {
            return Ok(());
        };
        let previous = self.config.read().await.network.vpn_gate.selection.clone();
        let selection = selection.clone();
        let store = CatalogueStore::new(&self.cache_dir);
        tokio::task::spawn_blocking(move || {
            if previous.as_ref() == Some(&selection) {
                store.load_selection(&selection).map(|_| ())
            } else {
                store.pin(&selection).map(|_| ())
            }
        })
        .await
        .map_err(|_| error(DirectoryError::CacheIo))?
        .map_err(error)
    }
}

struct UnavailableHttp;
#[async_trait::async_trait]
impl CatalogueHttp for UnavailableHttp {
    async fn get(&self, _: &str, _: &CancellationToken) -> Result<Vec<u8>, DirectoryError> {
        Err(DirectoryError::Unavailable)
    }
}
struct WarpSource {
    service: ControlService,
    profile: usque_core::Profile,
    existing: Option<InternalNetwork>,
    allow_physical: bool,
}
#[async_trait::async_trait]
impl WarpCatalogueSource for WarpSource {
    async fn open(
        &self,
        cancel: &CancellationToken,
    ) -> Result<Arc<dyn CatalogueHttp>, DirectoryError> {
        if let Some(existing) = &self.existing {
            return Ok(Arc::new(existing.clone()));
        }
        if !self.allow_physical {
            return Err(DirectoryError::Unavailable);
        }
        let warp = self
            .service
            .load_warp_identity(self.profile.id)
            .await
            .map_err(|_| DirectoryError::Unavailable)?;
        let identity = MasqueTlsIdentity::from_warp_identity(&warp)
            .map_err(|_| DirectoryError::Unavailable)?;
        let refresher = Arc::new(VaultEndpointPinRefresher {
            profile_id: self.profile.id,
            vault: self.service.vault.clone(),
            identity: Mutex::new(warp),
        });
        let profile = DataPlaneRuntime::headless_profile(&self.profile);
        let runtime = tokio::select! {
            _ = cancel.cancelled() => return Err(DirectoryError::Cancelled),
            result = DataPlaneRuntime::start_with_geo_policy(&profile, identity, Arc::new(NoopSocketProtector),
                Some(refresher), Arc::new(GeoDirectPolicy::disabled())) => result.map_err(|_| DirectoryError::Unavailable)?,
        };
        Ok(Arc::new(OwnedHttp {
            network: runtime.internal_network(),
            runtime: Mutex::new(Some(runtime)),
        }))
    }
}
struct OwnedHttp {
    network: InternalNetwork,
    runtime: Mutex<Option<DataPlaneRuntime>>,
}
#[async_trait::async_trait]
impl CatalogueHttp for OwnedHttp {
    async fn get(
        &self,
        url: &str,
        cancellation: &CancellationToken,
    ) -> Result<Vec<u8>, DirectoryError> {
        self.network.get(url, cancellation).await
    }
    async fn close(&self) {
        if let Some(mut runtime) = self.runtime.lock().await.take() {
            runtime.shutdown().await;
        }
    }
}

fn error(error: DirectoryError) -> ControlServiceError {
    ControlServiceError::VpnGate(error)
}
fn enum_name(value: impl serde::Serialize) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}
fn health_name(health: &RuntimeHealth) -> &'static str {
    match health {
        RuntimeHealth::Connected { .. } => "connected",
        RuntimeHealth::Reconnecting { .. } => "reconnecting",
        RuntimeHealth::Failed { .. } => "error",
    }
}
pub(crate) fn settings_from_proto(
    value: Option<v1::VpnGateSettings>,
) -> Result<VpnGateSettings, ControlServiceError> {
    let Some(value) = value else {
        return Ok(Default::default());
    };
    let settings = VpnGateSettings {
        enabled: value.enabled,
        selection: if value.server_id.is_empty() && value.config_sha256.is_empty() {
            None
        } else {
            Some(Selection {
                server_id: value.server_id,
                config_sha256: value.config_sha256,
            })
        },
    };
    settings.validate().map_err(error)?;
    Ok(settings)
}
pub(crate) fn settings_to_proto(value: &VpnGateSettings) -> v1::VpnGateSettings {
    v1::VpnGateSettings {
        enabled: value.enabled,
        server_id: value
            .selection
            .as_ref()
            .map(|s| s.server_id.clone())
            .unwrap_or_default(),
        config_sha256: value
            .selection
            .as_ref()
            .map(|s| s.config_sha256.clone())
            .unwrap_or_default(),
    }
}
fn server_to_proto(server: &ServerSummary) -> v1::VpnGateServer {
    v1::VpnGateServer {
        id: server.id.clone(),
        hostname: server.hostname.clone(),
        ip: server.ip.to_string(),
        country_code: server.country_code.clone().unwrap_or_default(),
        country_name: server.country_name.clone().unwrap_or_default(),
        score: server.score,
        ping_ms: server.ping_ms,
        speed_bps: server.speed_bps,
        num_vpn_sessions: server.num_vpn_sessions,
        config_sha256: server.config_sha256.clone(),
        unsupported_reason: server.unsupported_reason.map(enum_name).unwrap_or_default(),
    }
}
pub(crate) fn status_to_proto(status: &GateStatus, warp_stage: Option<&str>) -> v1::VpnGateStatus {
    v1::VpnGateStatus {
        stage: enum_name(status.stage),
        generation: status.generation,
        current_server: status.current_server.as_ref().map(server_to_proto),
        network: status.network.as_ref().map(|n| v1::FinalNetworkParameters {
            ipv4: n.ipv4.map(|ip| ip.to_string()).unwrap_or_default(),
            ipv6: n.ipv6.map(|ip| ip.to_string()).unwrap_or_default(),
            dns_servers: n.dns_servers.iter().map(ToString::to_string).collect(),
            mtu: n.mtu.into(),
        }),
        failure: status.failure.map(enum_name).unwrap_or_default(),
        warp_stage: warp_stage
            .or(status.warp_stage.as_deref())
            .unwrap_or_default()
            .to_owned(),
    }
}
