use std::sync::Arc;

use usque_core::Profile;

use crate::geo_direct::GeoDirectPolicy;
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
    runtime: Option<MasqueRuntime>,
}

impl ProxyRuntime {
    fn inner(&self) -> &MasqueRuntime {
        self.runtime.as_ref().expect("proxy MASQUE runtime")
    }

    fn inner_mut(&mut self) -> &mut MasqueRuntime {
        self.runtime.as_mut().expect("proxy MASQUE runtime")
    }

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
        Self::start_with_geo_policy(
            profile,
            identity,
            protector,
            pin_refresher,
            GeoDirectPolicy::disabled(),
        )
        .await
    }

    /// Starts proxy frontends with an immutable GEO direct-routing policy.
    ///
    /// A disabled or incomplete policy always falls back to the MASQUE path.
    pub async fn start_with_geo_policy(
        profile: &Profile,
        identity: MasqueTlsIdentity,
        protector: Arc<dyn SocketProtector>,
        pin_refresher: Option<Arc<dyn EndpointPinRefresher>>,
        geo_policy: GeoDirectPolicy,
    ) -> Result<Self, TransportError> {
        Ok(Self {
            runtime: Some(
                MasqueRuntime::start_with_geo_policy(
                    profile,
                    identity,
                    protector,
                    pin_refresher,
                    Arc::new(geo_policy),
                )
                .await?,
            ),
        })
    }

    pub fn path(&self) -> RuntimePath {
        self.inner().path()
    }

    pub fn listeners(&self) -> &[std::net::SocketAddr] {
        self.inner().listeners()
    }

    pub fn socks5_listeners(&self) -> &[std::net::SocketAddr] {
        self.inner().socks5_listeners()
    }

    pub fn http_listeners(&self) -> &[std::net::SocketAddr] {
        self.inner().http_listeners()
    }

    pub fn health(&self) -> RuntimeHealth {
        self.inner().health()
    }

    pub fn statistics(&self) -> TrafficSnapshot {
        self.inner().statistics()
    }

    pub fn performance(&self) -> ProxyPerformanceSnapshot {
        self.inner().performance()
    }

    pub fn failure(&self) -> Option<String> {
        self.inner().failure()
    }

    pub async fn reconfigure_frontends(&mut self, profile: &Profile) -> Result<(), TransportError> {
        self.inner_mut().reconfigure_frontends(profile).await
    }

    pub fn into_masque(mut self) -> MasqueRuntime {
        self.runtime.take().expect("proxy MASQUE runtime")
    }

    pub fn from_masque(runtime: MasqueRuntime) -> Self {
        Self {
            runtime: Some(runtime),
        }
    }

    pub fn cancel_immediately(&mut self) {
        if let Some(runtime) = self.runtime.as_mut() {
            runtime.cancel_immediately();
        }
    }

    pub async fn shutdown(&mut self) {
        if let Some(runtime) = self.runtime.as_mut() {
            runtime.shutdown().await;
        }
    }
}

impl Drop for ProxyRuntime {
    fn drop(&mut self) {
        self.cancel_immediately();
    }
}
