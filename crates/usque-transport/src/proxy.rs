use std::sync::Arc;

use usque_core::Profile;

use crate::h2::{MasqueTlsIdentity, TransportError};
use crate::masque_runtime::MasqueRuntime;
use crate::netstack::{ProxyPerformanceSnapshot, RuntimeHealth, RuntimePath, TrafficSnapshot};
use crate::pin_refresh::EndpointPinRefresher;
use crate::socket::{SocketProtector, noop_socket_protector};

/// One reconnecting MASQUE channel with zero, one, or both local proxy
/// frontends attached to the same userspace packet stack.
///
/// Listener sockets are reserved before the remote session is opened. This
/// keeps startup atomic and prevents SOCKS5 and HTTP from accidentally opening
/// separate MASQUE channels for the same active Profile.
pub struct ProxyRuntime {
    runtime: MasqueRuntime,
}

impl ProxyRuntime {
    pub async fn start(
        profile: &Profile,
        identity: MasqueTlsIdentity,
    ) -> Result<Self, TransportError> {
        Self::start_with_protector(profile, identity, noop_socket_protector()).await
    }

    pub async fn start_with_protector(
        profile: &Profile,
        identity: MasqueTlsIdentity,
        protector: Arc<dyn SocketProtector>,
    ) -> Result<Self, TransportError> {
        Self::start_with_refresh(profile, identity, protector, None).await
    }

    pub async fn start_with_refresh(
        profile: &Profile,
        identity: MasqueTlsIdentity,
        protector: Arc<dyn SocketProtector>,
        pin_refresher: Option<Arc<dyn EndpointPinRefresher>>,
    ) -> Result<Self, TransportError> {
        Ok(Self {
            runtime: MasqueRuntime::start_with_refresh(profile, identity, protector, pin_refresher)
                .await?,
        })
    }

    pub fn path(&self) -> RuntimePath {
        self.runtime.path()
    }

    pub fn listeners(&self) -> &[std::net::SocketAddr] {
        self.runtime.listeners()
    }

    pub fn socks5_listeners(&self) -> &[std::net::SocketAddr] {
        self.runtime.socks5_listeners()
    }

    pub fn http_listeners(&self) -> &[std::net::SocketAddr] {
        self.runtime.http_listeners()
    }

    pub fn health(&self) -> RuntimeHealth {
        self.runtime.health()
    }

    pub fn statistics(&self) -> TrafficSnapshot {
        self.runtime.statistics()
    }

    pub fn performance(&self) -> ProxyPerformanceSnapshot {
        self.runtime.performance()
    }

    pub fn failure(&self) -> Option<String> {
        self.runtime.failure()
    }

    pub fn cancel_immediately(&mut self) {
        self.runtime.cancel_immediately();
    }

    pub async fn shutdown(&mut self) {
        self.runtime.shutdown().await;
    }
}

impl Drop for ProxyRuntime {
    fn drop(&mut self) {
        self.cancel_immediately();
    }
}
