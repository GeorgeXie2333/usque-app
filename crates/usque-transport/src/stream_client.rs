//! Shared stream frontend ownership; only L4 clients own an outer QUIC session.
use crate::l4::{BufferBudget, L4Client, L4Metrics};
use crate::netstack::RuntimeHealth;
use crate::tcp::{DialError, FlowClass, TcpDialer, TcpStream, TcpTarget};
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::watch;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

pub(crate) struct StreamClient {
    pub(crate) l4: Option<Arc<L4Client>>,
    pub(crate) proxy: Option<Arc<crate::proxy_exit::ProxyDialer>>,
    pub(crate) budget: Arc<BufferBudget>,
    pub(crate) metrics: Arc<L4Metrics>,
    pub(crate) health: watch::Receiver<RuntimeHealth>,
}
impl StreamClient {
    pub(crate) fn l4(client: Arc<L4Client>) -> Arc<Self> {
        Arc::new(Self {
            budget: client.budget.clone(),
            metrics: client.metrics.clone(),
            health: client.health.clone(),
            l4: Some(client),
            proxy: None,
        })
    }
    pub(crate) fn snapshot(&self) -> usque_core::L4Snapshot {
        self.l4
            .as_ref()
            .map_or_else(Default::default, |c| c.snapshot())
    }
    pub(crate) fn cancel(&self) {
        if let Some(c) = &self.l4 {
            c.cancel();
        }
        if let Some(c) = &self.proxy {
            c.cancellation.cancel();
        }
    }
    pub(crate) async fn shutdown(&self) {
        self.cancel();
        if let Some(c) = &self.l4 {
            c.shutdown().await;
        }
    }
}
#[async_trait]
impl TcpDialer for StreamClient {
    fn is_ready(&self) -> bool {
        self.proxy.as_ref().is_none_or(|p| p.is_ready())
    }
    fn session_generation(&self) -> Option<u64> {
        self.l4
            .as_ref()
            .and_then(|c| c.session_generation())
            .or_else(|| self.proxy.as_ref().and_then(|p| p.session_generation()))
    }
    async fn connect(
        &self,
        target: TcpTarget,
        deadline: Instant,
        cancel: &CancellationToken,
        class: FlowClass,
    ) -> Result<TcpStream, DialError> {
        if let Some(c) = &self.l4 {
            c.connect(target, deadline, cancel, class).await
        } else if let Some(c) = &self.proxy {
            c.connect(target, deadline, cancel, class).await
        } else {
            Err(DialError::Closed)
        }
    }
}
