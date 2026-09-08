use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use usque_core::{Profile, ProxyDnsMode};

use super::L4Client;
use super::tun::TunBridge;
use crate::dns::Resolver;
use crate::dns_stream::StreamDns;
use crate::geo_direct::GeoDirectPolicy;
use crate::h2::{MasqueTlsIdentity, TransportError};
use crate::http_proxy::HttpProxyFrontend;
use crate::masque_runtime::{FrontendSpec, listeners_overlap};
use crate::netstack::{
    ManagedTunnelMonitor, ProxyPerformanceSnapshot, RuntimeHealth, TrafficCounters,
};
use crate::pin_refresh::EndpointPinRefresher;
use crate::socket::SocketProtector;
use crate::socks5::Socks5Frontend;
use crate::tcp::{ProxyServices, TcpDialer};
use crate::telemetry::ConnectionTelemetry;

/// One account-bound, TCP-only runtime shared by TUN and local proxy listeners.
pub(crate) struct L4Runtime {
    pub(crate) client: Arc<L4Client>,
    pub(crate) monitor: ManagedTunnelMonitor,
    pub(crate) bridge: Option<TunBridge>,
    pub(crate) assigned_ipv4: Ipv4Addr,
    pub(crate) assigned_ipv6: Ipv6Addr,
    socks5: Option<Socks5Frontend>,
    http: Option<HttpProxyFrontend>,
    socks5_spec: Option<FrontendSpec>,
    http_spec: Option<FrontendSpec>,
    listeners: Vec<SocketAddr>,
    dns: Arc<StreamDns>,
    services: ProxyServices,
    cancellation: CancellationToken,
    tasks: Vec<JoinHandle<()>>,
}

impl L4Runtime {
    pub(crate) async fn start(
        profile: &Profile,
        identity: MasqueTlsIdentity,
        protector: Arc<dyn SocketProtector>,
        pin_refresher: Option<Arc<dyn EndpointPinRefresher>>,
        geo_policy: Arc<GeoDirectPolicy>,
    ) -> Result<Self, TransportError> {
        profile
            .validate()
            .map_err(|_| TransportError::UnsupportedOperatingMode)?;
        if matches!(
            identity.provider.as_ref(),
            Some(usque_core::IdentityProvider::ZeroTrust { .. })
        ) && !usque_core::ManagedEndpointIps::from_endpoint(&profile.endpoint)
            .matches_zero_trust_contract()
        {
            return Err(TransportError::InvalidIdentity);
        }
        crate::encrypted_dns::validate_direct_dns_support(&profile.direct_dns)?;
        profile
            .proxy
            .listener_credentials()
            .map_err(|_| TransportError::InvalidIdentity)?;
        let socks_bound = profile
            .frontends
            .socks5
            .then(|| Socks5Frontend::prebind(profile))
            .transpose()?;
        let http_bound = profile
            .frontends
            .http
            .then(|| HttpProxyFrontend::prebind(profile))
            .transpose()?;
        let cancellation = CancellationToken::new();
        let startup_guard = cancellation.clone().drop_guard();
        let telemetry = ConnectionTelemetry::default();
        let quality = telemetry.network_quality();
        quality.use_stream_data_plane();
        let protector = crate::encrypted_dns::configure_direct_dns(
            &profile.direct_dns,
            protector,
            quality.clone(),
            &cancellation,
        )?;
        if profile.frontends.tunnel
            && geo_policy.is_enabled()
            && protector.direct_dns_resolver().is_none()
            && protector.physical_dns_servers().is_empty()
        {
            return Err(TransportError::Dns(
                "physical DNS unavailable for configured direct routes".to_owned(),
            ));
        }
        let ipv4 = identity.assigned_ipv4;
        let ipv6 = identity.assigned_ipv6;
        let counters = Arc::new(TrafficCounters::default());
        let client = L4Client::start(
            profile.clone(),
            identity,
            protector.clone(),
            pin_refresher,
            telemetry.clone(),
            counters.clone(),
            &cancellation,
        )
        .await
        .map_err(super::transport_error)?;
        let (quality_rx, sampler) =
            crate::network_quality::spawn_network_quality_sampler_with_counters(
                quality.clone(),
                counters.clone(),
                cancellation.child_token(),
            );
        let sampler = tokio_util::task::AbortOnDropHandle::new(sampler);
        let dialer: Arc<dyn TcpDialer> = client.clone();
        let dns = Arc::new(StreamDns::new(
            dialer.clone(),
            protector.clone(),
            cancellation.child_token(),
            client.metrics.clone(),
        ));
        let pool = dns.clone();
        let pool_cancel = cancellation.child_token();
        let pool_maintenance = tokio_util::task::AbortOnDropHandle::new(tokio::spawn(async move {
            let mut tick = tokio::time::interval(std::time::Duration::from_secs(1));
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tokio::select! {
                    _ = pool_cancel.cancelled() => break,
                    _ = tick.tick() => pool.prune(),
                }
            }
        }));
        let servers = dns_servers(profile);
        let services = ProxyServices {
            admission: Some(Arc::new(crate::tcp::FrontendAdmission::new(
                client.budget.clone(),
                super::Limits::platform().active + super::Limits::platform().pending,
            ))),
            dialer,
            udp: None,
            resolver: Resolver::for_streams(
                dns.clone(),
                servers,
                profile.proxy.dns_mode,
                protector.clone(),
            ),
            protector,
            geo_policy,
            counters: counters.clone(),
            ipv4,
            ipv6,
            cancellation: cancellation.clone(),
            health: client.health.clone(),
        };
        let bridge = if profile.frontends.tunnel {
            Some(
                TunBridge::start(
                    profile,
                    services.clone(),
                    dns.clone(),
                    client.budget.clone(),
                    client.metrics.clone(),
                    quality,
                )
                .await?,
            )
        } else {
            None
        };
        let socks5 = socks_bound
            .map(|bound| Socks5Frontend::activate_services(profile, services.clone(), bound))
            .transpose()?;
        let http = http_bound
            .map(|bound| HttpProxyFrontend::activate_services(profile, services.clone(), bound))
            .transpose()?;
        let socks5_spec = socks5
            .as_ref()
            .and_then(|f| FrontendSpec::from_socks5_frontend(f, profile));
        let http_spec = http
            .as_ref()
            .and_then(|f| FrontendSpec::from_http_frontend(f, profile));
        let monitor = ManagedTunnelMonitor::for_streams(
            client.health.clone(),
            counters,
            telemetry,
            quality_rx,
        );
        let mut runtime = Self {
            client,
            monitor,
            bridge,
            assigned_ipv4: ipv4,
            assigned_ipv6: ipv6,
            socks5,
            http,
            socks5_spec,
            http_spec,
            listeners: Vec::new(),
            dns,
            services,
            cancellation,
            tasks: vec![sampler.detach(), pool_maintenance.detach()],
        };
        runtime.refresh_listeners();
        startup_guard.disarm();
        Ok(runtime)
    }

    fn services_for(&self, profile: &Profile) -> ProxyServices {
        let mut services = self.services.clone();
        services.resolver = Resolver::for_streams(
            self.dns.clone(),
            dns_servers(profile),
            profile.proxy.dns_mode,
            services.protector.clone(),
        );
        services
    }

    pub(crate) async fn reconfigure_frontends(
        &mut self,
        profile: &Profile,
    ) -> Result<(), TransportError> {
        if (profile.frontends.socks5 || profile.frontends.http)
            && let Err(error) = profile.proxy.listener_credentials()
        {
            self.refresh_listeners();
            return Err(if profile.frontends.socks5 {
                TransportError::Socks5(error.to_string())
            } else {
                TransportError::HttpProxy(error.to_string())
            });
        }

        let keep_socks5 = profile.frontends.socks5
            && self.socks5.is_some()
            && self.socks5_spec.as_ref() == FrontendSpec::from_socks5_profile(profile).as_ref();
        let keep_http = profile.frontends.http
            && self.http.is_some()
            && self.http_spec.as_ref() == FrontendSpec::from_http_profile(profile).as_ref();

        let add_socks5 = profile.frontends.socks5 && !keep_socks5;
        let add_http = profile.frontends.http && !keep_http;
        let socks5_rebind_same = add_socks5
            && self.socks5.as_ref().is_some_and(|frontend| {
                listeners_overlap(frontend.listeners(), &profile.proxy.socks5_listeners)
            });
        let http_rebind_same = add_http
            && self.http.as_ref().is_some_and(|frontend| {
                listeners_overlap(frontend.listeners(), &profile.proxy.http_listeners)
            });

        let mut socks5_bound = None;
        if add_socks5 && !socks5_rebind_same {
            match Socks5Frontend::prebind(profile) {
                Ok(bound) => socks5_bound = Some(bound),
                Err(error) => {
                    self.refresh_listeners();
                    return Err(error);
                }
            }
        }
        let mut http_bound = None;
        if add_http && !http_rebind_same {
            match HttpProxyFrontend::prebind(profile) {
                Ok(bound) => http_bound = Some(bound),
                Err(error) => {
                    self.refresh_listeners();
                    return Err(error);
                }
            }
        }

        if socks5_rebind_same && let Some(mut frontend) = self.socks5.take() {
            self.socks5_spec.take();
            frontend.shutdown().await;
        }
        if http_rebind_same && let Some(mut frontend) = self.http.take() {
            self.http_spec.take();
            frontend.shutdown().await;
        }

        if add_socks5 && socks5_rebind_same {
            match Socks5Frontend::prebind(profile) {
                Ok(bound) => socks5_bound = Some(bound),
                Err(error) => {
                    self.refresh_listeners();
                    return Err(error);
                }
            }
        }
        if add_http && http_rebind_same {
            match HttpProxyFrontend::prebind(profile) {
                Ok(bound) => http_bound = Some(bound),
                Err(error) => {
                    self.refresh_listeners();
                    return Err(error);
                }
            }
        }

        if !keep_socks5
            && !socks5_rebind_same
            && let Some(mut frontend) = self.socks5.take()
        {
            self.socks5_spec.take();
            frontend.shutdown().await;
        }
        if !keep_http
            && !http_rebind_same
            && let Some(mut frontend) = self.http.take()
        {
            self.http_spec.take();
            frontend.shutdown().await;
        }

        if let Some(bound) = socks5_bound {
            match Socks5Frontend::activate_services(profile, self.services_for(profile), bound) {
                Ok(frontend) => {
                    self.socks5_spec = FrontendSpec::from_socks5_frontend(&frontend, profile);
                    self.socks5 = Some(frontend);
                }
                Err(error) => {
                    self.refresh_listeners();
                    return Err(error);
                }
            }
        }
        if let Some(bound) = http_bound {
            match HttpProxyFrontend::activate_services(profile, self.services_for(profile), bound) {
                Ok(frontend) => {
                    self.http_spec = FrontendSpec::from_http_frontend(&frontend, profile);
                    self.http = Some(frontend);
                }
                Err(error) => {
                    self.refresh_listeners();
                    return Err(error);
                }
            }
        }

        self.refresh_listeners();
        tokio::task::yield_now().await;
        if let Some(message) = self.socks5.as_ref().and_then(Socks5Frontend::failure) {
            return Err(TransportError::Socks5(message));
        }
        if let Some(message) = self.http.as_ref().and_then(HttpProxyFrontend::failure) {
            return Err(TransportError::HttpProxy(message));
        }
        Ok(())
    }

    fn refresh_listeners(&mut self) {
        self.listeners = self
            .socks5
            .iter()
            .flat_map(|frontend| frontend.listeners().iter().copied())
            .chain(
                self.http
                    .iter()
                    .flat_map(|frontend| frontend.listeners().iter().copied()),
            )
            .collect();
    }

    pub(crate) fn listeners(&self) -> &[SocketAddr] {
        &self.listeners
    }
    pub(crate) fn socks5_listeners(&self) -> &[SocketAddr] {
        self.socks5.as_ref().map_or(&[], |f| f.listeners())
    }
    pub(crate) fn http_listeners(&self) -> &[SocketAddr] {
        self.http.as_ref().map_or(&[], |f| f.listeners())
    }
    pub(crate) fn performance(&self) -> ProxyPerformanceSnapshot {
        let mut result = ProxyPerformanceSnapshot::default();
        if let Some(http) = &self.http {
            http.augment_performance(&mut result);
        }
        result
    }
    pub(crate) fn failure(&self) -> Option<String> {
        self.socks5
            .as_ref()
            .and_then(Socks5Frontend::failure)
            .or_else(|| self.http.as_ref().and_then(HttpProxyFrontend::failure))
            .or_else(|| match self.monitor.health() {
                RuntimeHealth::Failed { message, .. } => Some(message),
                _ => None,
            })
    }
    pub(crate) fn diagnostic_dns_context(&self) -> (Arc<dyn SocketProtector>, CancellationToken) {
        (self.services.protector.clone(), self.cancellation.clone())
    }
    pub(crate) fn cancel_immediately(&mut self) {
        if let Some(frontend) = self.socks5.as_mut() {
            frontend.cancel_immediately();
        }
        if let Some(frontend) = self.http.as_mut() {
            frontend.cancel_immediately();
        }
        if let Some(bridge) = self.bridge.as_ref() {
            bridge.cancel();
        }
        self.cancellation.cancel();
        self.client.cancel();
    }
    pub(crate) async fn shutdown(&mut self) {
        self.cancel_immediately();
        if let Some(frontend) = self.socks5.as_mut() {
            frontend.shutdown().await;
        }
        if let Some(frontend) = self.http.as_mut() {
            frontend.shutdown().await;
        }
        if let Some(bridge) = self.bridge.as_mut() {
            bridge.shutdown().await;
        }
        self.bridge.take();
        self.dns.clear();
        self.client.shutdown().await;
        for task in self.tasks.drain(..) {
            task.abort();
            let _ = task.await;
        }
    }
}
impl Drop for L4Runtime {
    fn drop(&mut self) {
        self.cancel_immediately();
        for task in &self.tasks {
            task.abort();
        }
    }
}

fn dns_servers(profile: &Profile) -> Vec<IpAddr> {
    if profile.proxy.dns_mode == ProxyDnsMode::LocalConfigured {
        profile.proxy.dns_servers.clone()
    } else {
        profile.dns_servers.clone()
    }
}
