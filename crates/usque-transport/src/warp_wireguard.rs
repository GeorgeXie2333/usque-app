//! Discovery sessions have no frontend or host-network fallback. One scanner
//! identity is used serially, separately from the user's live exit identity.
mod probe;
#[cfg(test)]
mod tests;
pub(crate) async fn observe_exit(
    network: &crate::InternalNetwork,
) -> usque_core::warp_wireguard::ExitObservation {
    probe::observe_exit(network).await
}
use crate::{
    DataPlaneRuntime, EndpointPinRefresher, GeoDirectPolicy, InternalNetwork, MasqueTlsIdentity,
    RuntimeHealth, SocketProtector,
};
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;
use usque_core::{
    DataPlaneMode, Profile, WarpIdentity, chain_exit::*, storage::ConfigStore, warp_wireguard::*,
};

pub struct Context {
    pub profile: Profile,
    pub existing: Option<InternalNetwork>,
    pub identity: Option<WarpIdentity>,
    pub protector: Arc<dyn SocketProtector>,
    pub refresher: Option<Arc<dyn EndpointPinRefresher>>,
    pub allow_physical: bool,
}
struct Running {
    job: Arc<Mutex<Job>>,
    cancel: CancellationToken,
    stop: Arc<Mutex<&'static str>>,
    done: Arc<AtomicBool>,
}
pub struct Manager {
    path: PathBuf,
    cipher: Arc<dyn store::ProfileCipher>,
    running: Mutex<Option<Running>>,
}
impl Manager {
    pub fn new(path: PathBuf, cipher: Arc<dyn store::ProfileCipher>) -> Self {
        Self {
            path,
            cipher,
            running: Mutex::new(None),
        }
    }
    fn store(&self) -> Store<'_> {
        Store::new(
            self.path.parent().expect("absolute configuration path"),
            &*self.cipher,
        )
    }
    pub fn signal_stop(&self) {
        if let Ok(slot) = self.running.lock()
            && let Some(running) = slot.as_ref()
        {
            running.cancel.cancel();
        }
    }
    pub fn stop_blocking(&self) -> bool {
        self.signal_stop();
        let done = self
            .running
            .lock()
            .ok()
            .and_then(|s| s.as_ref().map(|r| r.done.clone()));
        let deadline = Instant::now() + Duration::from_secs(45);
        while done.as_ref().is_some_and(|d| !d.load(Ordering::Acquire)) {
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        true
    }
    pub fn command(
        self: &Arc<Self>,
        request: Request,
        context: Option<Context>,
    ) -> Result<Response, ImportError> {
        let mut slot = self.running.lock().map_err(|_| error("unavailable"))?;
        if matches!(request.action.as_str(), "pause" | "cancel") {
            if let Some(run) = slot.as_ref()
                && !run.done.load(Ordering::Acquire)
            {
                if request.job_id != Some(run.job.lock().map_err(|_| error("unavailable"))?.id) {
                    return Err(error("job_not_found"));
                }
                *run.stop.lock().map_err(|_| error("unavailable"))? = if request.action == "cancel"
                {
                    "cancelled"
                } else {
                    "paused"
                };
                run.cancel.cancel();
            } else if let Some(id) = request.job_id {
                let mut job = self.store().job(id)?;
                if !matches!(job.state.as_str(), "completed" | "cancelled") {
                    job.state = if request.action == "cancel" {
                        "cancelled"
                    } else {
                        "paused"
                    }
                    .into();
                    self.store().save(&job)?;
                    if let Some(run) = slot.as_ref() {
                        let mut snapshot = run.job.lock().map_err(|_| error("unavailable"))?;
                        if snapshot.id == id {
                            *snapshot = job;
                        }
                    }
                }
            }
        }
        let mut selected = request.job_id;
        if request.needs_network() {
            if slot
                .as_ref()
                .is_some_and(|r| !r.done.load(Ordering::Acquire))
            {
                return Err(error("scan_busy"));
            }
            let context = context.ok_or_else(|| error("underlay_unavailable"))?;
            if context.profile.data_plane != DataPlaneMode::ConnectIp {
                return Err(error("connect_ip_required"));
            }
            let fingerprint = usque_core::warp_wireguard::context(&context.profile)?;
            let mut job = if request.action == "resume" {
                let mut job = self
                    .store()
                    .job(request.job_id.ok_or_else(|| error("job_not_found"))?)?;
                if matches!(job.state.as_str(), "completed" | "cancelled") || job.kind == "generate"
                {
                    return Err(error("cannot_resume"));
                }
                if job.context != fingerprint {
                    return Err(error("context_changed"));
                }
                job.state = "running".into();
                job.failure = None;
                self.store().save(&job)?;
                job
            } else {
                let job = Job::new(&request, fingerprint)?;
                self.store().insert(&job)?;
                job
            };
            selected = Some(job.id);
            job.state = "running".into();
            let shared = Arc::new(Mutex::new(job));
            let cancel = CancellationToken::new();
            let done = Arc::new(AtomicBool::new(false));
            let stop = Arc::new(Mutex::new("paused"));
            let manager = self.clone();
            let thread_job = shared.clone();
            let thread_cancel = cancel.clone();
            let thread_done = done.clone();
            let thread_stop = stop.clone();
            std::thread::Builder::new()
                .name("warp-discovery".into())
                .spawn(move || {
                    let result = (|| {
                        let runtime = tokio::runtime::Builder::new_current_thread()
                            .enable_all()
                            .build()
                            .map_err(|_| error("unavailable"))?;
                        runtime.block_on(Box::pin(manager.run(
                            &thread_job,
                            context,
                            &thread_cancel,
                            &thread_stop,
                            &request.name,
                        )))
                    })();
                    if let Err(failure) = result
                        && let Ok(mut job) = thread_job.lock()
                    {
                        let state = if thread_cancel.is_cancelled() {
                            thread_stop.lock().map_or("paused", |s| *s)
                        } else {
                            "failed"
                        };
                        manager.record_failure(&mut job, state, &failure.reason);
                    }
                    thread_done.store(true, Ordering::Release);
                })
                .map_err(|_| error("unavailable"))?;
            *slot = Some(Running {
                job: shared,
                cancel,
                stop,
                done,
            });
        }
        let live = slot
            .as_ref()
            .map(|r| {
                r.job
                    .lock()
                    .map(|j| j.clone())
                    .map_err(|_| error("unavailable"))
            })
            .transpose()?;
        drop(slot);
        let ids = self.store().jobs()?;
        let selected = selected.or_else(|| ids.first().copied());
        let mut response = Response::default();
        for id in ids {
            let mut job = self.store().job(id)?;
            if let Some(current) = live.as_ref().filter(|j| j.id == id) {
                // Only the snapshot uses volatile progress; pages use committed chunks.
                job = current.clone();
            } else if job.state == "running" {
                job.state = "paused".into();
            }
            let snapshot = job.snapshot();
            if selected == Some(id) {
                let committed = self.store().job(id)?;
                let (rows, next) =
                    self.store()
                        .page(&committed, request.cursor, request.country.as_deref())?;
                response.job = Some(snapshot.clone());
                response.results = rows;
                response.next_cursor = next;
            }
            response.history.push(snapshot);
        }
        Ok(response)
    }
    async fn run(
        &self,
        shared: &Mutex<Job>,
        context: Context,
        cancel: &CancellationToken,
        stop: &Mutex<&str>,
        name: &str,
    ) -> Result<(), ImportError> {
        let mut job = shared.lock().map_err(|_| error("unavailable"))?.clone();
        let mut outer = Box::pin(Outer::open(context, cancel)).await?;
        let result = self.run_open(&mut job, shared, &outer, cancel, name).await;
        outer.close().await;
        if let Err(failure) = &result {
            let state = if cancel.is_cancelled() {
                *stop.lock().map_err(|_| error("unavailable"))?
            } else {
                "failed"
            };
            self.record_failure(&mut job, state, &failure.reason);
            *shared.lock().map_err(|_| error("unavailable"))? = job;
            return result;
        }
        if cancel.is_cancelled() {
            job.state = stop.lock().map_err(|_| error("unavailable"))?.to_string();
        } else {
            job.state = "completed".into();
        }
        self.store().save(&job)?;
        *shared.lock().map_err(|_| error("unavailable"))? = job;
        result
    }
    // Volatile progress may include a chunk whose checkpoint failed. Never
    // promote that cursor while trying to persist the terminal error.
    fn record_failure(&self, volatile: &mut Job, state: &str, reason: &str) {
        if let Ok(mut committed) = self.store().job(volatile.id) {
            committed.state = state.into();
            committed.failure = Some(reason.into());
            let _ = self.store().save(&committed);
            *volatile = committed;
        } else {
            volatile.state = state.into();
            volatile.failure = Some(reason.into());
        }
    }
    async fn run_open(
        &self,
        job: &mut Job,
        shared: &Mutex<Job>,
        outer: &Outer,
        cancel: &CancellationToken,
        name: &str,
    ) -> Result<(), ImportError> {
        let baseline = outer.network.health_snapshot();
        if !matches!(baseline, RuntimeHealth::Connected { .. }) {
            return Err(error("underlay_unavailable"));
        }
        let mut health = outer.network.health();
        let observed = probe::outer_address(&outer.network, cancel).await;
        if job.next > 0 && (observed.is_none() || job.outer != observed) {
            return Err(error("context_changed"));
        }
        job.outer = observed;
        self.store().save(job)?;
        if job.kind == "generate" {
            let secrets = probe::register(&outer.network, cancel).await?;
            if cancel.is_cancelled() {
                return Err(error("cancelled"));
            }
            let parent = self.path.parent().ok_or_else(|| error("unavailable"))?;
            let summary = store::ChainProfileStore::new(parent, &*self.cipher).import(
                ChainSource::WarpWireguard,
                if name.is_empty() {
                    "WARP via WireGuard"
                } else {
                    name
                },
                secrets,
            )?;
            job.profile_id = Some(summary.id);
            return Ok(());
        }
        let secrets = match self.store().identity()? {
            Some(secrets) => secrets,
            None => {
                let secrets = probe::register(&outer.network, cancel).await?;
                self.store().save_identity(&secrets)?;
                secrets
            }
        };
        let mut rows = Vec::new();
        let mut last = Instant::now();
        while let Some(endpoint) = job.endpoint(job.next) {
            if cancel.is_cancelled() {
                break;
            }
            let current = ConfigStore::new(&self.path)
                .load()
                .ok()
                .and_then(|c| c.active_profile());
            if current
                .as_ref()
                .and_then(|p| usque_core::warp_wireguard::context(p).ok())
                .as_deref()
                != Some(&job.context)
            {
                job.failure = Some("context_changed".into());
                break;
            }
            if outer.network.health_snapshot() != baseline {
                job.failure = Some("underlay_unavailable".into());
                break;
            }
            let candidate_cancel = cancel.child_token();
            let pending = probe::endpoint(
                &outer.network,
                &outer.profile,
                &secrets,
                endpoint,
                job.next,
                &candidate_cancel,
            );
            tokio::pin!(pending);
            let mut changed = false;
            let row = tokio::select! {
                biased;
                _ = underlay_changed(&mut health, &baseline) => {
                    changed = true;
                    candidate_cancel.cancel();
                    pending.await
                },
                result = &mut pending => result,
            };
            if row.failure.as_deref() == Some("cleanup_pending") {
                job.failure = Some("cleanup_pending".into());
                break;
            }
            if cancel.is_cancelled() {
                break;
            }
            if changed {
                job.failure = Some("context_changed".into());
                break;
            }
            if row.failure.is_none() {
                job.working += 1;
            }
            for observation in [&row.ipv4, &row.ipv6].into_iter().flatten() {
                if let Some(country) = &observation.country {
                    job.countries.insert(country.clone());
                }
            }
            rows.push(row);
            job.next += 1;
            if rows.len() == CHUNK_SIZE || last.elapsed() >= Duration::from_secs(5) {
                self.store().checkpoint(job, &mut rows)?;
                last = Instant::now();
            }
            *shared.lock().map_err(|_| error("unavailable"))? = job.clone();
        }
        self.store().checkpoint(job, &mut rows)?;
        if let Some(failure) = &job.failure {
            return Err(error(failure));
        }
        Ok(())
    }
}
async fn underlay_changed(
    health: &mut tokio::sync::watch::Receiver<RuntimeHealth>,
    baseline: &RuntimeHealth,
) {
    loop {
        if &*health.borrow_and_update() != baseline {
            return;
        }
        if health.changed().await.is_err() {
            return;
        }
    }
}
struct Outer {
    network: InternalNetwork,
    profile: Profile,
    runtime: Option<DataPlaneRuntime>,
}
impl Outer {
    async fn open(context: Context, cancel: &CancellationToken) -> Result<Self, ImportError> {
        let profile = DataPlaneRuntime::headless_profile(&context.profile);
        if let Some(network) = context.existing {
            if !matches!(network.health_snapshot(), RuntimeHealth::Connected { .. }) {
                return Err(error("underlay_unavailable"));
            }
            return Ok(Self {
                network,
                profile,
                runtime: None,
            });
        }
        if !context.allow_physical {
            return Err(error("underlay_unavailable"));
        }
        let identity = MasqueTlsIdentity::from_warp_identity(
            &context.identity.ok_or_else(|| error("identity_required"))?,
        )
        .map_err(|_| error("identity_required"))?;
        let runtime = tokio::select! {
            biased;
            _=cancel.cancelled()=>return Err(error("cancelled")),
            result=DataPlaneRuntime::start_with_geo_policy(&profile,identity,context.protector,context.refresher,Arc::new(GeoDirectPolicy::disabled()))=>result.map_err(|_| error("underlay_unavailable"))?,
        };
        Ok(Self {
            network: runtime.warp_internal_network(),
            profile,
            runtime: Some(runtime),
        })
    }
    async fn close(&mut self) {
        if let Some(runtime) = self.runtime.as_mut() {
            runtime.shutdown().await;
        }
    }
}
