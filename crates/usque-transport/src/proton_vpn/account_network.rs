//! One-way, task-owned recovery. System sockets retain OS policy; WARP control
//! traffic cannot use a final chain exit or fall back to physical sockets.
use super::{
    Failure,
    api::{Http, HttpFailure},
    system_http::SystemHttp,
    verification_proxy::{Descriptor, VerificationProxy},
    warp_http::WarpHttp,
};
use crate::{
    InternalNetwork, RuntimeHealth,
    internal_network::{InternalHttpError, InternalRequest, InternalResponse},
    warp_wireguard::{Context, Outer},
};
use async_trait::async_trait;
use bytes::Bytes;
use http::Method;
use std::sync::{
    Arc, RwLock,
    atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
};
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::{Instant, timeout};
use tokio_util::sync::CancellationToken;
use usque_core::chain_exit::ImportError;

/// Lazily supplies the selected account and current WARP network. Implementors
/// must preserve OS routing and must not create an identity or a system VPN.
#[async_trait]
pub trait AccountWarpProvider: Send + Sync {
    async fn context(&self, cancel: &CancellationToken) -> Result<Context, ImportError>;
}

struct Route {
    http: Arc<dyn Http>,
    warp: Option<(InternalNetwork, u64)>,
}

pub(super) struct AccountNetworkSession {
    route: RwLock<Route>,
    provider: Arc<dyn AccountWarpProvider>,
    prepare_lock: Mutex<()>,
    switch_lock: Mutex<()>,
    prepared: AtomicBool,
    attempted: AtomicBool,
    phase: AtomicU8,
    used_ms: AtomicU64,
    job_cancel: CancellationToken,
    owned: Mutex<Option<Outer>>,
    proxy: Mutex<Option<VerificationProxy>>,
}

impl AccountNetworkSession {
    pub fn new(
        provider: Arc<dyn AccountWarpProvider>,
        cancel: CancellationToken,
    ) -> Result<Self, Failure> {
        Ok(Self::with_system(
            provider,
            cancel,
            Arc::new(SystemHttp::new()?),
        ))
    }
    fn with_system(
        provider: Arc<dyn AccountWarpProvider>,
        job_cancel: CancellationToken,
        http: Arc<dyn Http>,
    ) -> Self {
        Self {
            route: RwLock::new(Route { http, warp: None }),
            provider,
            prepare_lock: Mutex::new(()),
            switch_lock: Mutex::new(()),
            prepared: AtomicBool::new(false),
            attempted: AtomicBool::new(false),
            phase: AtomicU8::new(0),
            used_ms: AtomicU64::new(0),
            job_cancel,
            owned: Mutex::new(None),
            proxy: Mutex::new(None),
        }
    }
    fn remaining(&self) -> Result<Duration, Failure> {
        let remaining = 120_000u64.saturating_sub(self.used_ms.load(Ordering::Acquire));
        if remaining == 0 {
            Err(Failure::new("account_network_budget_exhausted"))
        } else {
            Ok(Duration::from_millis(remaining))
        }
    }
    fn charge(&self, start: Instant) {
        self.used_ms.fetch_add(
            start.elapsed().as_millis().min(u64::MAX as u128) as u64,
            Ordering::AcqRel,
        );
    }
    fn active(&self) -> Result<Arc<dyn Http>, Failure> {
        if self.phase.load(Ordering::Acquire) >= 4 {
            return Err(Failure::new("account_network_unavailable"));
        }
        let route = self.route.read().map_err(|_| Failure::new("unavailable"))?;
        if route
            .warp
            .as_ref()
            .is_some_and(|(network, generation)| network.control_generation() != Some(*generation))
        {
            return Err(Failure::new("underlay_changed"));
        }
        Ok(route.http.clone())
    }
    fn retryable(error: &HttpFailure, request: &InternalRequest<'_>) -> bool {
        matches!(
            error.kind,
            InternalHttpError::Dns
                | InternalHttpError::Connect
                | InternalHttpError::Timeout
                | InternalHttpError::Tls
        ) && (error.not_submitted
            || request.method == Method::GET
            || request.method == Method::POST
                && request.url == "https://vpn-api.proton.me/auth/v4/info")
    }
    async fn attempt(
        &self,
        request: InternalRequest<'_>,
        cancel: &CancellationToken,
    ) -> Result<InternalResponse, HttpFailure> {
        let http = self.active().map_err(|e| HttpFailure {
            kind: InternalHttpError::Connect,
            not_submitted: true,
            reason: Some(e.reason),
        })?;
        let remaining = self.remaining().map_err(|e| HttpFailure {
            kind: InternalHttpError::Timeout,
            not_submitted: true,
            reason: Some(e.reason),
        })?;
        let start = Instant::now();
        let result = timeout(remaining, http.exchange_delivery(request, cancel))
            .await
            .unwrap_or_else(|_| Err(InternalHttpError::Timeout.into()));
        self.charge(start);
        // Preserve known responses (especially token rotations); callers still
        // checkpoint the lease before continuing, as with the runtime client.
        result
    }
}

#[async_trait]
impl Http for AccountNetworkSession {
    async fn prepare(&self, cancel: &CancellationToken) -> Result<(), Failure> {
        let _lock = self.prepare_lock.lock().await;
        if self.prepared.swap(true, Ordering::AcqRel) {
            return self.active().map(|_| ());
        }
        let start = Instant::now();
        let http = self.active()?;
        let probe = http.exchange_delivery(
            InternalRequest {
                url: "https://vpn-api.proton.me/tests/ping",
                method: Method::GET,
                headers: vec![],
                body: Bytes::new(),
                limit: 8192,
                ipv6: None,
            },
            cancel,
        );
        let result = timeout(Duration::from_secs(5), probe)
            .await
            .unwrap_or_else(|_| Err(InternalHttpError::Timeout.into()));
        self.charge(start);
        match result {
            Ok(_) => {
                self.phase.store(1, Ordering::Release);
                Ok(())
            }
            Err(e)
                if matches!(
                    e.kind,
                    InternalHttpError::Dns
                        | InternalHttpError::Connect
                        | InternalHttpError::Timeout
                        | InternalHttpError::Tls
                ) =>
            {
                self.recover(cancel).await.map(|_| ())
            }
            Err(_) if cancel.is_cancelled() => Err(Failure::new("cancelled")),
            // A reachable HTTP server need not implement ping or return JSON.
            Err(_) => {
                self.phase.store(1, Ordering::Release);
                Ok(())
            }
        }
    }
    async fn recover(&self, cancel: &CancellationToken) -> Result<bool, Failure> {
        let _lock = self.switch_lock.lock().await;
        if self.attempted.swap(true, Ordering::AcqRel) {
            return Ok(false);
        }
        if cancel.is_cancelled() || self.job_cancel.is_cancelled() {
            return Err(Failure::new("cancelled"));
        }
        self.phase.store(2, Ordering::Release);
        let start = Instant::now();
        let budget = self.remaining()?.min(Duration::from_secs(30));
        let acquire_cancel = cancel.child_token();
        let acquire = async {
            let context = self.provider.context(&acquire_cancel).await?;
            let outer = Outer::open(context, &acquire_cancel).await?;
            *self.owned.lock().await = Some(outer);
            loop {
                if acquire_cancel.is_cancelled() {
                    return Err(usque_core::proton_vpn::error("cancelled"));
                }
                let ready = self.owned.lock().await.as_ref().is_some_and(|o| {
                    matches!(o.network.health_snapshot(), RuntimeHealth::Connected { .. })
                });
                if ready {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            let network = self
                .owned
                .lock()
                .await
                .as_ref()
                .expect("owned WARP")
                .network
                .clone();
            let generation = network
                .control_generation()
                .ok_or_else(|| usque_core::proton_vpn::error("underlay_changed"))?;
            let http = Arc::new(WarpHttp::new(network.clone()));
            *self
                .route
                .write()
                .map_err(|_| usque_core::proton_vpn::error("unavailable"))? = Route {
                http,
                warp: Some((network, generation)),
            };
            Ok::<_, ImportError>(())
        };
        let result = tokio::select! { biased;
            _ = cancel.cancelled() => Err(Failure::new("cancelled")),
            result = timeout(budget, acquire) => match result { Ok(r) => r.map_err(Failure::from), Err(_) => Err(Failure::new("warp_fallback_timeout")) },
        };
        self.charge(start);
        if let Err(error) = result {
            acquire_cancel.cancel();
            self.phase.store(4, Ordering::Release);
            return Err(error);
        }
        self.phase.store(3, Ordering::Release);
        Ok(true)
    }
    async fn exchange(
        &self,
        request: InternalRequest<'_>,
        cancel: &CancellationToken,
    ) -> Result<InternalResponse, InternalHttpError> {
        self.exchange_delivery(request, cancel)
            .await
            .map_err(|e| e.kind)
    }
    async fn exchange_delivery(
        &self,
        request: InternalRequest<'_>,
        cancel: &CancellationToken,
    ) -> Result<InternalResponse, HttpFailure> {
        let result = self.attempt(request.clone(), cancel).await;
        if let Err(ref error) = result
            && !self.attempted.load(Ordering::Acquire)
            && Self::retryable(error, &request)
        {
            // A human-verification answer belongs to its original route. Let
            // Client obtain a new challenge instead of replaying that answer.
            if request
                .headers
                .iter()
                .any(|(name, _)| name.eq_ignore_ascii_case("x-pm-human-verification-token"))
            {
                return Err(HttpFailure {
                    reason: Some("verification_network_failed".into()),
                    ..error.clone()
                });
            }
            self.recover(cancel).await.map_err(|e| HttpFailure {
                kind: error.kind,
                not_submitted: error.not_submitted,
                reason: Some(e.reason),
            })?;
            return self.attempt(request, cancel).await;
        }
        result
    }
    async fn verification_proxy(
        &self,
        cancel: &CancellationToken,
    ) -> Result<Option<Descriptor>, Failure> {
        self.active()?;
        let warp = self
            .route
            .read()
            .map_err(|_| Failure::new("unavailable"))?
            .warp
            .clone();
        // Reserve the native window's worst-case network-loading budget. This
        // keeps the task bound without counting the user's puzzle-solving time
        // or trusting an unverified elapsed-time value from the UI.
        let load_ms = if warp.is_some() { 20_000 } else { 15_000 };
        self.used_ms
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                used.checked_add(load_ms).filter(|total| *total <= 120_000)
            })
            .map_err(|_| Failure::new("account_network_budget_exhausted"))?;
        let Some((network, generation)) = warp else {
            return Ok(None);
        };
        let mut slot = self.proxy.lock().await;
        if let Some(mut old) = slot.take() {
            old.close().await;
        }
        let proxy = VerificationProxy::start(network, generation, cancel.child_token()).await?;
        let descriptor = proxy.descriptor();
        *slot = Some(proxy);
        Ok(Some(descriptor))
    }
    fn network_status(&self) -> Option<(&'static str, &'static str)> {
        Some(match self.phase.load(Ordering::Acquire) {
            0 => ("system", "checking"),
            1 => ("system", "active"),
            2 => ("warp", "starting"),
            3 => ("warp", "active"),
            _ => ("warp", "unavailable"),
        })
    }
    fn generation(&self) -> Option<u64> {
        Some(0)
    }
    fn current(&self, generation: Option<u64>) -> bool {
        generation == Some(0) && self.active().is_ok()
    }
    async fn close_verification(&self) {
        if let Some(mut proxy) = self.proxy.lock().await.take() {
            proxy.close().await;
        }
    }
    async fn shutdown(&self) {
        self.phase.store(4, Ordering::Release);
        if let Some(mut proxy) = self.proxy.lock().await.take() {
            proxy.close().await;
        }
        let http = self.route.read().ok().map(|r| r.http.clone());
        if let Some(http) = http {
            http.shutdown().await;
        }
        if let Some(mut outer) = self.owned.lock().await.take() {
            outer.close().await;
        }
    }
}

#[cfg(test)]
mod tests;
