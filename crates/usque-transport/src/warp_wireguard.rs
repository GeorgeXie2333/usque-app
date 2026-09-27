//! Configuration generation uses the MASQUE network without a frontend or
//! host-network fallback and keeps the outer account identity unchanged.
mod registration;
pub(crate) mod registration_tls;
#[cfg(test)]
mod tests;
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
use usque_core::{DataPlaneMode, Profile, WarpIdentity, chain_exit::*, warp_wireguard::*};

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
        request.validate()?;
        let mut slot = self.running.lock().map_err(|_| error("unavailable"))?;
        if request.action == "cancel" {
            let run = slot.as_ref().ok_or_else(|| error("job_not_found"))?;
            if request.job_id != Some(run.job.lock().map_err(|_| error("unavailable"))?.id) {
                return Err(error("job_not_found"));
            }
            run.cancel.cancel();
        }
        if request.needs_network() {
            if slot
                .as_ref()
                .is_some_and(|r| !r.done.load(Ordering::Acquire))
            {
                return Err(error("generation_busy"));
            }
            let context = context.ok_or_else(|| error("underlay_unavailable"))?;
            if context.profile.data_plane != DataPlaneMode::ConnectIp {
                return Err(error("connect_ip_required"));
            }
            let job = Job::generation();
            let shared = Arc::new(Mutex::new(job));
            let cancel = CancellationToken::new();
            let done = Arc::new(AtomicBool::new(false));
            let manager = self.clone();
            let thread_job = shared.clone();
            let thread_cancel = cancel.clone();
            let thread_done = done.clone();
            let name = request.name.clone();
            std::thread::Builder::new()
                .name("warp-generation".into())
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
                            &name,
                        )))
                    })();
                    if let Err(failure) = result
                        && let Ok(mut job) = thread_job.lock()
                    {
                        let state = if thread_cancel.is_cancelled() {
                            "cancelled"
                        } else {
                            "failed"
                        };
                        job.state = state.into();
                        job.failure = Some(failure.reason);
                    }
                    thread_done.store(true, Ordering::Release);
                })
                .map_err(|_| error("unavailable"))?;
            *slot = Some(Running {
                job: shared,
                cancel,
                done,
            });
        }
        let job = slot
            .as_ref()
            .map(|r| {
                r.job
                    .lock()
                    .map(|j| j.clone())
                    .map_err(|_| error("unavailable"))
            })
            .transpose()?;
        if !request.needs_network()
            && request
                .job_id
                .is_some_and(|id| job.as_ref().is_none_or(|job| job.id != id))
        {
            return Err(error("job_not_found"));
        }
        Ok(Response { job, error: None })
    }
    async fn run(
        &self,
        shared: &Mutex<Job>,
        context: Context,
        cancel: &CancellationToken,
        name: &str,
    ) -> Result<(), ImportError> {
        let mut job = shared.lock().map_err(|_| error("unavailable"))?.clone();
        let mut outer = Box::pin(Outer::open(context, cancel)).await?;
        let result = self.run_open(&mut job, &outer, cancel, name).await;
        outer.close().await;
        result?;
        job.state = "completed".into();
        *shared.lock().map_err(|_| error("unavailable"))? = job;
        Ok(())
    }
    async fn run_open(
        &self,
        job: &mut Job,
        outer: &Outer,
        cancel: &CancellationToken,
        name: &str,
    ) -> Result<(), ImportError> {
        if !matches!(
            outer.network.health_snapshot(),
            RuntimeHealth::Connected { .. }
        ) {
            return Err(error("underlay_unavailable"));
        }
        let secrets = registration::register(&outer.network, cancel).await?;
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
        Ok(())
    }
}
struct Outer {
    network: InternalNetwork,
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
            result=DataPlaneRuntime::start_with_geo_policy(&profile,identity,context.protector,context.refresher,Arc::new(GeoDirectPolicy::disabled()))=>result.map_err(|_| error("underlay_start_failed"))?,
        };
        Ok(Self {
            network: runtime.warp_internal_network(),
            runtime: Some(runtime),
        })
    }
    async fn close(&mut self) {
        if let Some(runtime) = self.runtime.as_mut() {
            runtime.shutdown().await;
        }
    }
}
