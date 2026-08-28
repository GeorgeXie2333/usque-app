use std::{net::SocketAddr, time::Instant};

use usque_core::{FrontendKind, FrontendPhase, FrontendSettings, FrontendStatus, Profile};
use usque_transport::{
    ConnectionTimelineSnapshot, ProxyPerformanceSnapshot, ProxyRuntime, RuntimeHealth, RuntimePath,
    TrafficSnapshot,
};

use crate::ControlServiceError;

#[cfg(windows)]
use crate::map_windows_vpn_error;

pub(crate) struct ActiveDataPlane {
    pub(crate) profile_id: uuid::Uuid,
    pub(crate) session_generation: u64,
    pub(crate) frontends: FrontendSettings,
    pub(crate) connected_at: Instant,
    pub(crate) last_sample_at: Instant,
    pub(crate) last_bytes_sent: u64,
    pub(crate) last_bytes_received: u64,
    pub(crate) last_proxy_performance: ProxyPerformanceSnapshot,
    pub(crate) runtime: ActiveRuntime,
}

pub(crate) struct ActiveProxyRuntime {
    pub(crate) runtime: ProxyRuntime,
    #[cfg(windows)]
    pub(crate) system_proxy: Option<crate::windows_agent::WindowsSystemProxyGuard>,
}

pub(crate) enum ActiveRuntime {
    Proxy(Box<ActiveProxyRuntime>),
    #[cfg(windows)]
    Vpn(Box<crate::windows_agent::WindowsVpnRuntime>),
    #[cfg(test)]
    Harness(HarnessRuntime),
}

#[cfg(test)]
pub(crate) struct HarnessRuntime {
    pub(crate) reconnect_count: u32,
    pub(crate) vpn: bool,
    pub(crate) listeners: Vec<SocketAddr>,
    socks5_listeners: Vec<SocketAddr>,
    http_listeners: Vec<SocketAddr>,
    pub(crate) reconfigure_count: u32,
    pub(crate) attach_count: u32,
    pub(crate) detach_count: u32,
    system_proxy: bool,
    pub(crate) system_proxy_apply_count: u32,
    pub(crate) fail_after_detach: bool,
}

#[cfg(test)]
impl HarnessRuntime {
    pub(crate) fn from_profile(profile: &Profile, vpn: bool, reconnect_count: u32) -> Self {
        let socks5_listeners = if profile.frontends.socks5 {
            profile.proxy.socks5_listeners.clone()
        } else {
            Vec::new()
        };
        let http_listeners = if profile.frontends.http {
            profile.proxy.http_listeners.clone()
        } else {
            Vec::new()
        };
        let mut listeners = socks5_listeners.clone();
        listeners.extend(http_listeners.iter().copied());
        Self {
            reconnect_count,
            vpn,
            listeners,
            socks5_listeners,
            http_listeners,
            reconfigure_count: 0,
            attach_count: 0,
            detach_count: 0,
            system_proxy: profile.frontends.http && profile.proxy.system_proxy,
            system_proxy_apply_count: 0,
            fail_after_detach: false,
        }
    }

    fn health(&self) -> RuntimeHealth {
        RuntimeHealth::Connected {
            path: RuntimePath {
                transport: usque_core::Transport::Http3,
                endpoint_family: usque_core::AddressFamily::Ipv4,
                ipv4_available: true,
                ipv6_available: true,
            },
            reconnect_count: self.reconnect_count,
        }
    }

    fn reconfigure_frontends(&mut self, profile: &Profile) {
        self.reconfigure_count = self.reconfigure_count.saturating_add(1);
        self.socks5_listeners = if profile.frontends.socks5 {
            profile.proxy.socks5_listeners.clone()
        } else {
            Vec::new()
        };
        self.http_listeners = if profile.frontends.http {
            profile.proxy.http_listeners.clone()
        } else {
            Vec::new()
        };
        self.listeners = self.socks5_listeners.clone();
        self.listeners.extend(self.http_listeners.iter().copied());
        self.system_proxy = profile.frontends.http && profile.proxy.system_proxy;
    }

    fn set_tunnel(&mut self, tunnel: bool) {
        if tunnel && !self.vpn {
            self.vpn = true;
            self.attach_count = self.attach_count.saturating_add(1);
        } else if !tunnel && self.vpn {
            self.vpn = false;
            self.detach_count = self.detach_count.saturating_add(1);
        }
    }
}

impl ActiveRuntime {
    pub(crate) fn cancel_immediately(&mut self) {
        match self {
            Self::Proxy(runtime) => runtime.runtime.cancel_immediately(),
            #[cfg(windows)]
            Self::Vpn(runtime) => runtime.cancel_immediately(),
            #[cfg(test)]
            Self::Harness(_) => {}
        }
    }

    pub(crate) fn path(&self) -> RuntimePath {
        match self {
            Self::Proxy(runtime) => runtime.runtime.path(),
            #[cfg(windows)]
            Self::Vpn(runtime) => runtime.path(),
            #[cfg(test)]
            Self::Harness(runtime) => runtime.health().path(),
        }
    }

    pub(crate) fn listeners(&self) -> &[SocketAddr] {
        match self {
            Self::Proxy(runtime) => runtime.runtime.listeners(),
            #[cfg(windows)]
            Self::Vpn(runtime) => runtime.listeners(),
            #[cfg(test)]
            Self::Harness(runtime) => &runtime.listeners,
        }
    }

    pub(crate) fn health(&self) -> RuntimeHealth {
        match self {
            Self::Proxy(runtime) => runtime.runtime.health(),
            #[cfg(windows)]
            Self::Vpn(runtime) => runtime.health(),
            #[cfg(test)]
            Self::Harness(runtime) => runtime.health(),
        }
    }

    pub(crate) fn statistics(&self) -> TrafficSnapshot {
        match self {
            Self::Proxy(runtime) => runtime.runtime.statistics(),
            #[cfg(windows)]
            Self::Vpn(runtime) => runtime.statistics(),
            #[cfg(test)]
            Self::Harness(_) => TrafficSnapshot::default(),
        }
    }

    pub(crate) fn connection_timeline(&self) -> ConnectionTimelineSnapshot {
        match self {
            Self::Proxy(runtime) => runtime.runtime.connection_timeline(),
            #[cfg(windows)]
            Self::Vpn(runtime) => runtime.connection_timeline(),
            #[cfg(test)]
            Self::Harness(_) => ConnectionTimelineSnapshot::default(),
        }
    }

    pub(crate) fn proxy_performance(&self) -> Option<ProxyPerformanceSnapshot> {
        match self {
            Self::Proxy(runtime) => Some(runtime.runtime.performance()),
            #[cfg(windows)]
            Self::Vpn(_) => None,
            #[cfg(test)]
            Self::Harness(_) => None,
        }
    }

    pub(crate) fn failure(&self) -> Option<String> {
        match self {
            Self::Proxy(runtime) => runtime.runtime.failure(),
            #[cfg(windows)]
            Self::Vpn(runtime) => runtime.failure(),
            #[cfg(test)]
            Self::Harness(_) => None,
        }
    }

    pub(crate) fn is_vpn(&self) -> bool {
        match self {
            Self::Proxy(_) => false,
            #[cfg(windows)]
            Self::Vpn(_) => true,
            #[cfg(test)]
            Self::Harness(runtime) => runtime.vpn,
        }
    }

    pub(crate) fn frontend_statuses(&self, configured: FrontendSettings) -> Vec<FrontendStatus> {
        let runtime_phase = match self.health() {
            RuntimeHealth::Connected { .. } => FrontendPhase::Active,
            RuntimeHealth::Reconnecting { .. } => FrontendPhase::Reconnecting,
            RuntimeHealth::Failed { .. } => FrontendPhase::Error,
        };
        let mut statuses = Vec::with_capacity(4);
        statuses.push(output_status(
            FrontendKind::Tunnel,
            configured.tunnel && self.is_vpn(),
            runtime_phase,
            &[],
        ));
        let (socks, http, system_proxy) = match self {
            Self::Proxy(runtime) => (
                runtime.runtime.socks5_listeners(),
                runtime.runtime.http_listeners(),
                {
                    #[cfg(windows)]
                    {
                        runtime.system_proxy.is_some()
                    }
                    #[cfg(not(windows))]
                    {
                        false
                    }
                },
            ),
            #[cfg(windows)]
            Self::Vpn(runtime) => (
                runtime.socks5_listeners(),
                runtime.http_listeners(),
                runtime.system_proxy_active(),
            ),
            #[cfg(test)]
            Self::Harness(runtime) => (
                runtime.socks5_listeners.as_slice(),
                runtime.http_listeners.as_slice(),
                runtime.system_proxy,
            ),
        };
        statuses.push(output_status(
            FrontendKind::Socks5,
            configured.socks5,
            runtime_phase,
            socks,
        ));
        statuses.push(output_status(
            FrontendKind::Http,
            configured.http,
            runtime_phase,
            http,
        ));
        statuses.push(output_status(
            FrontendKind::SystemProxy,
            system_proxy,
            runtime_phase,
            &[],
        ));
        statuses
    }

    #[cfg(windows)]
    pub(crate) fn requires_agent_reattach(&self) -> bool {
        matches!(self, Self::Vpn(runtime) if runtime.requires_agent_reattach())
    }

    #[cfg(windows)]
    pub(crate) async fn detach_for_agent_reattach(&mut self) -> Result<(), ControlServiceError> {
        match self {
            Self::Vpn(runtime) => runtime
                .detach_for_agent_reattach()
                .await
                .map_err(map_windows_vpn_error),
            _ => Err(ControlServiceError::InvalidRequest(
                "only an active Windows VPN can reattach to the Agent".to_owned(),
            )),
        }
    }

    pub(crate) async fn shutdown(&mut self) -> Result<(), ControlServiceError> {
        match self {
            Self::Proxy(runtime) => {
                #[cfg(windows)]
                let system_proxy_result = match runtime.system_proxy.as_mut() {
                    Some(system_proxy) => {
                        system_proxy.shutdown().await.map_err(map_windows_vpn_error)
                    }
                    None => Ok(()),
                };
                runtime.runtime.shutdown().await;
                #[cfg(windows)]
                system_proxy_result?;
                Ok(())
            }
            #[cfg(windows)]
            Self::Vpn(runtime) => runtime.shutdown().await.map_err(map_windows_vpn_error),
            #[cfg(test)]
            Self::Harness(_) => Ok(()),
        }
    }

    pub(crate) async fn reconfigure_frontends(
        &mut self,
        profile: &Profile,
    ) -> Result<(), ControlServiceError> {
        match self {
            Self::Proxy(runtime) => {
                runtime
                    .runtime
                    .reconfigure_frontends(profile)
                    .await
                    .map_err(ControlServiceError::Transport)?;
            }
            #[cfg(windows)]
            Self::Vpn(runtime) => {
                runtime
                    .reconfigure_frontends(profile)
                    .await
                    .map_err(map_windows_vpn_error)?;
            }
            #[cfg(test)]
            Self::Harness(runtime) => runtime.reconfigure_frontends(profile),
        }
        self.apply_system_proxy(profile).await?;
        Ok(())
    }

    pub(crate) async fn apply_system_proxy(
        &mut self,
        profile: &Profile,
    ) -> Result<(), ControlServiceError> {
        match self {
            Self::Proxy(runtime) => {
                #[cfg(windows)]
                {
                    crate::windows_agent::WindowsSystemProxyGuard::shutdown_slot(
                        &mut runtime.system_proxy,
                    )
                    .await
                    .map_err(map_windows_vpn_error)?;
                    runtime.system_proxy = if profile.frontends.http && profile.proxy.system_proxy {
                        let listener = crate::windows_agent::loopback_http_listener(
                            runtime.runtime.http_listeners(),
                        )
                        .ok_or_else(|| {
                            ControlServiceError::InvalidRequest(
                                "system proxy requires a loopback HTTP listener".to_owned(),
                            )
                        })?;
                        Some(
                            crate::windows_agent::WindowsSystemProxyGuard::start(listener)
                                .await
                                .map_err(map_windows_vpn_error)?,
                        )
                    } else {
                        None
                    };
                    Ok(())
                }
                #[cfg(not(windows))]
                {
                    let _ = (runtime, profile);
                    Ok(())
                }
            }
            #[cfg(windows)]
            Self::Vpn(runtime) => runtime
                .replace_system_proxy(profile)
                .await
                .map_err(map_windows_vpn_error),
            #[cfg(test)]
            Self::Harness(runtime) => {
                runtime.system_proxy = profile.frontends.http && profile.proxy.system_proxy;
                runtime.system_proxy_apply_count =
                    runtime.system_proxy_apply_count.saturating_add(1);
                Ok(())
            }
        }
    }

    pub(crate) async fn with_tunnel(
        self,
        profile: &Profile,
    ) -> Result<Self, (Self, ControlServiceError)> {
        #[cfg(test)]
        if let Self::Harness(mut harness) = self {
            harness.set_tunnel(profile.frontends.tunnel);
            if harness.fail_after_detach && !harness.vpn {
                return Err((
                    Self::Harness(harness),
                    ControlServiceError::InvalidRequest(
                        "system proxy requires a loopback HTTP listener".to_owned(),
                    ),
                ));
            }
            return Ok(Self::Harness(harness));
        }

        if profile.frontends.tunnel == self.is_vpn() {
            return Ok(self);
        }

        #[cfg(windows)]
        {
            return if profile.frontends.tunnel {
                attach_vpn(self, profile).await
            } else {
                detach_vpn(self, profile).await
            };
        }

        #[cfg(not(windows))]
        {
            Err((
                self,
                ControlServiceError::OperatingModeUnavailable(usque_core::OperatingMode::Vpn),
            ))
        }
    }
}

fn output_status(
    kind: FrontendKind,
    enabled: bool,
    phase: FrontendPhase,
    listeners: &[SocketAddr],
) -> FrontendStatus {
    FrontendStatus {
        kind,
        phase: if enabled {
            phase
        } else {
            FrontendPhase::Disabled
        },
        listeners: listeners.iter().map(ToString::to_string).collect(),
        error: None,
    }
}

#[cfg(windows)]
async fn attach_vpn(
    runtime: ActiveRuntime,
    profile: &Profile,
) -> Result<ActiveRuntime, (ActiveRuntime, ControlServiceError)> {
    let ActiveRuntime::Proxy(proxy) = runtime else {
        return Ok(runtime);
    };
    let mut proxy = *proxy;
    #[cfg(windows)]
    if let Some(mut guard) = proxy.system_proxy.take()
        && let Err(error) = guard.shutdown().await
    {
        let masque = proxy.runtime.into_masque();
        return Err((
            ActiveRuntime::Proxy(Box::new(ActiveProxyRuntime {
                runtime: ProxyRuntime::from_masque(masque),
                system_proxy: None,
            })),
            map_windows_vpn_error(error),
        ));
    }
    let masque = proxy.runtime.into_masque();
    match crate::windows_agent::WindowsVpnRuntime::attach_existing(profile, masque).await {
        Ok(vpn) => Ok(ActiveRuntime::Vpn(Box::new(vpn))),
        Err((masque, error)) => Err((
            ActiveRuntime::Proxy(Box::new(ActiveProxyRuntime {
                runtime: ProxyRuntime::from_masque(masque),
                system_proxy: None,
            })),
            map_windows_vpn_error(error),
        )),
    }
}

#[cfg(windows)]
async fn detach_vpn(
    runtime: ActiveRuntime,
    profile: &Profile,
) -> Result<ActiveRuntime, (ActiveRuntime, ControlServiceError)> {
    let ActiveRuntime::Vpn(mut vpn) = runtime else {
        return Ok(runtime);
    };
    let masque = match vpn.detach_into_masque().await {
        Ok(masque) => masque,
        Err(error) => {
            return Err((ActiveRuntime::Vpn(vpn), map_windows_vpn_error(error)));
        }
    };
    let proxy_runtime = ProxyRuntime::from_masque(masque);
    let system_proxy = if profile.frontends.http && profile.proxy.system_proxy {
        let Some(listener) =
            crate::windows_agent::loopback_http_listener(proxy_runtime.http_listeners())
        else {
            return Err((
                ActiveRuntime::Proxy(Box::new(ActiveProxyRuntime {
                    runtime: proxy_runtime,
                    system_proxy: None,
                })),
                ControlServiceError::InvalidRequest(
                    "system proxy requires a loopback HTTP listener".to_owned(),
                ),
            ));
        };
        match crate::windows_agent::WindowsSystemProxyGuard::start(listener).await {
            Ok(guard) => Some(guard),
            Err(error) => {
                return Err((
                    ActiveRuntime::Proxy(Box::new(ActiveProxyRuntime {
                        runtime: proxy_runtime,
                        system_proxy: None,
                    })),
                    map_windows_vpn_error(error),
                ));
            }
        }
    } else {
        None
    };
    Ok(ActiveRuntime::Proxy(Box::new(ActiveProxyRuntime {
        runtime: proxy_runtime,
        system_proxy,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn frontend_reconfigure_applies_system_proxy() {
        let mut profile = Profile {
            frontends: FrontendSettings {
                tunnel: false,
                socks5: true,
                http: true,
            },
            ..Profile::default()
        };
        profile.proxy.system_proxy = true;
        let mut runtime = ActiveRuntime::Harness(HarnessRuntime::from_profile(&profile, false, 0));
        profile.proxy.http_listeners[0].set_port(18081);
        runtime
            .reconfigure_frontends(&profile)
            .await
            .expect("reconfigure");
        let ActiveRuntime::Harness(harness) = runtime else {
            panic!("expected harness");
        };
        assert_eq!(harness.reconfigure_count, 1);
        assert_eq!(harness.system_proxy_apply_count, 1);
        assert_eq!(harness.http_listeners[0].port(), 18081);
        assert!(harness.system_proxy);
    }
}
