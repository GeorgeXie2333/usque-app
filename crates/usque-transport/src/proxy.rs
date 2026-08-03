use std::net::SocketAddr;
use std::sync::Arc;

use usque_core::{OperatingMode, Profile};

use crate::h2::{MasqueTlsIdentity, TransportError};
use crate::http_proxy::HttpProxyRuntime;
use crate::netstack::{RuntimeHealth, RuntimePath, TrafficSnapshot};
use crate::pin_refresh::EndpointPinRefresher;
use crate::socket::{SocketProtector, noop_socket_protector};
use crate::socks5::Socks5Runtime;

/// The single active proxy-mode data plane. Both variants share the same
/// MASQUE transport, endpoint selection, pinning, DNS, and packet stack.
pub enum ProxyRuntime {
    Socks5(Socks5Runtime),
    Http(HttpProxyRuntime),
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
        match profile.mode {
            OperatingMode::Socks5 => {
                Socks5Runtime::start_with_refresh(profile, identity, protector, pin_refresher)
                    .await
                    .map(Self::Socks5)
            }
            OperatingMode::HttpProxy => {
                HttpProxyRuntime::start_with_refresh(profile, identity, protector, pin_refresher)
                    .await
                    .map(Self::Http)
            }
            OperatingMode::Vpn => Err(TransportError::UnsupportedOperatingMode),
        }
    }

    pub fn path(&self) -> RuntimePath {
        match self {
            Self::Socks5(runtime) => runtime.path(),
            Self::Http(runtime) => runtime.path(),
        }
    }

    pub fn listeners(&self) -> &[SocketAddr] {
        match self {
            Self::Socks5(runtime) => runtime.listeners(),
            Self::Http(runtime) => runtime.listeners(),
        }
    }

    pub fn health(&self) -> RuntimeHealth {
        match self {
            Self::Socks5(runtime) => runtime.health(),
            Self::Http(runtime) => runtime.health(),
        }
    }

    pub fn statistics(&self) -> TrafficSnapshot {
        match self {
            Self::Socks5(runtime) => runtime.statistics(),
            Self::Http(runtime) => runtime.statistics(),
        }
    }

    pub fn failure(&self) -> Option<String> {
        match self {
            Self::Socks5(runtime) => runtime.failure(),
            Self::Http(runtime) => runtime.failure(),
        }
    }

    pub fn cancel_immediately(&mut self) {
        match self {
            Self::Socks5(runtime) => runtime.cancel_immediately(),
            Self::Http(runtime) => runtime.cancel_immediately(),
        }
    }

    pub async fn shutdown(&mut self) {
        match self {
            Self::Socks5(runtime) => runtime.shutdown().await,
            Self::Http(runtime) => runtime.shutdown().await,
        }
    }
}
