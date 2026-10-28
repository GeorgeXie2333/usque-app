use std::future::Future;
use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket as StdUdpSocket};
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use crate::tcp::TcpStream as TunnelTcpStream;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::{TcpSocket, TcpStream, UdpSocket};
use tokio::time::timeout;
use usque_geo::{CountryCode, GeoClassifier, GeoError};

use crate::live_routing::{LiveRouting, RoutingSnapshot};
pub use crate::live_routing::{PreparedRoutingUpdate, RoutingUpdateError};
use crate::netstack::TrafficCounters;
use crate::socket::{DirectEgressLease, DirectProtocol, SocketProtector, socket_handle};

const DIRECT_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_DIRECT_ADDRESSES: usize = 16;

pub(crate) const DIRECT_FAMILY_UNAVAILABLE: &str = "direct_address_family_unavailable";

/// Physical-family knowledge constrains DIRECT, never PROXY. Unknown platforms
/// still attempt the protected socket; IPv4-mapped addresses use IPv4 knowledge.
pub(crate) fn direct_family_available(protector: &dyn SocketProtector, remote: SocketAddr) -> bool {
    let ip = remote.ip().to_canonical();
    // Platform family masks describe default Internet paths. A missing default
    // route does not rule out loopback or on-link/ULA access on that family.
    ip.is_loopback()
        || usque_core::config::is_lan_bypass_address(ip)
        || protector.endpoint_family_available(SocketAddr::new(ip, remote.port())) != Some(false)
}

/// The route selected by [`GeoDirectPolicy`] for one proxy destination.
///
/// `Tunnel` is deliberately the default: absent, incomplete, or non-matching
/// Geo data can never cause traffic to bypass MASQUE.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GeoRoute {
    Tunnel,
    Direct,
    Reject,
}

impl From<usque_core::RoutingAction> for GeoRoute {
    fn from(action: usque_core::RoutingAction) -> Self {
        match action {
            usque_core::RoutingAction::Direct => Self::Direct,
            usque_core::RoutingAction::Reject => Self::Reject,
            usque_core::RoutingAction::Proxy => Self::Tunnel,
        }
    }
}

/// Read-only Geo matching interface used by [`GeoDirectPolicy`].
///
/// [`GeoClassifier`] implements this directly. Keeping the interface small
/// lets embedders inject a classifier that has already been loaded from their
/// approved cache, and lets transport tests remain fully offline.
pub trait GeoDirectClassifier: Send + Sync {
    fn host_matches(&self, host: &str, country: &CountryCode) -> bool;

    fn ip_matches(&self, ip: IpAddr, country: &CountryCode) -> bool;
}

impl GeoDirectClassifier for GeoClassifier {
    fn host_matches(&self, host: &str, country: &CountryCode) -> bool {
        Self::host_matches(self, host, country)
    }

    fn ip_matches(&self, ip: IpAddr, country: &CountryCode) -> bool {
        Self::ip_matches(self, ip, country)
    }
}

/// Immutable GEO/bypass configuration with session-shared routing revisions.
///
/// Hostnames match custom suffixes or GeoSite; numerical addresses match custom
/// networks or GeoIP. Explicit rules work without a GEO catalog. Unknown targets
/// always route through the tunnel; loading an enabled but invalid GEO catalog
/// fails before callers can attach custom rules.
#[derive(Clone)]
pub struct GeoDirectPolicy {
    classifier: Option<Arc<dyn GeoDirectClassifier>>,
    countries: Arc<[CountryCode]>,
    allow_lan: bool,
    networks: Arc<[usque_core::config::IpNet]>,
    domains: Arc<[String]>,
    routing: Arc<LiveRouting>,
    frozen_routing: Option<Arc<RoutingSnapshot>>,
    ads_enabled: bool,
    ads: Arc<crate::live_ads::LiveAds>,
}

impl Default for GeoDirectPolicy {
    fn default() -> Self {
        Self::disabled()
    }
}

impl GeoDirectPolicy {
    /// Creates a disabled policy that always uses the tunnel.
    pub fn disabled() -> Self {
        Self {
            classifier: None,
            countries: Arc::default(),
            allow_lan: false,
            networks: Arc::default(),
            domains: Arc::default(),
            routing: Default::default(),
            frozen_routing: None,
            ads_enabled: false,
            ads: Arc::default(),
        }
    }

    /// Creates a policy from a previously loaded [`GeoClassifier`].
    pub fn new(
        classifier: Arc<GeoClassifier>,
        countries: impl IntoIterator<Item = CountryCode>,
    ) -> Self {
        Self::with_classifier(classifier, countries)
    }

    /// Loads an immutable policy from the verified GEO cache layout.
    pub fn load(
        cache_dir: impl AsRef<Path>,
        countries: impl IntoIterator<Item = CountryCode>,
    ) -> Result<Self, GeoError> {
        let countries = countries.into_iter().collect::<Vec<_>>();
        if countries.is_empty() {
            return Ok(Self::disabled());
        }
        let classifier = GeoClassifier::load(cache_dir, &countries)?;
        Ok(Self::new(Arc::new(classifier), countries))
    }

    /// Creates a policy from an injected Geo matcher.
    ///
    /// This is primarily useful for platform-owned cache adapters. Matchers
    /// should return `false` for malformed or unknown data.
    pub fn with_classifier<C>(
        classifier: Arc<C>,
        countries: impl IntoIterator<Item = CountryCode>,
    ) -> Self
    where
        C: GeoDirectClassifier + 'static,
    {
        let classifier: Arc<dyn GeoDirectClassifier> = classifier;
        Self {
            classifier: Some(classifier),
            countries: countries.into_iter().collect(),
            allow_lan: false,
            networks: Arc::default(),
            domains: Arc::default(),
            routing: Default::default(),
            frozen_routing: None,
            ads_enabled: false,
            ads: Arc::default(),
        }
    }

    /// Add validated explicit rules independently of the optional GEO catalog.
    pub fn with_custom_rules(
        mut self,
        profile: &usque_core::Profile,
    ) -> Result<Self, usque_core::ConfigError> {
        self.allow_lan = profile.allow_lan;
        self.domains =
            usque_core::config::normalize_bypass_domains(&profile.bypass_domains)?.into();
        self.networks = profile.split_exclusions.clone().into();
        // A builder creates an independent policy even when called on a clone
        // of an active runtime. Only explicit prepare/publish changes readers.
        self.routing = Arc::new(LiveRouting::new(RoutingSnapshot::new(&profile.routing, 1)?));
        self.frozen_routing = None;
        self.ads_enabled = profile.routing.ads_enabled;
        Ok(self)
    }

    /// Capture one complete rule revision for a new flow or DNS request.
    pub fn freeze_routing(&self) -> Self {
        let mut frozen = self.clone();
        frozen.frozen_routing = Some(self.routing_snapshot());
        frozen
    }

    fn routing_snapshot(&self) -> Arc<RoutingSnapshot> {
        self.frozen_routing
            .clone()
            .unwrap_or_else(|| self.routing.snapshot())
    }

    pub fn routing_revision(&self) -> u64 {
        self.routing_snapshot().revision
    }

    /// Unlike routing_revision, this always checks the owning runtime's head.
    pub fn routing_is_current(&self, revision: u64) -> bool {
        self.with_current_routing(revision, || ()).is_some()
    }

    pub(crate) fn with_current_routing<T>(
        &self,
        revision: u64,
        operation: impl FnOnce() -> T,
    ) -> Option<T> {
        self.routing.with_current(revision, operation)
    }

    pub fn prepare_routing_update(
        &self,
        profile: &usque_core::Profile,
    ) -> Result<PreparedRoutingUpdate, usque_core::ConfigError> {
        let current = self.routing.snapshot();
        let base_revision = current.revision;
        let next = Arc::new(RoutingSnapshot::new(
            &profile.routing,
            base_revision.checked_add(1).unwrap_or(base_revision),
        )?);
        let changes_rules = next.settings.rules != current.settings.rules;
        let next = if changes_rules { next } else { current };
        let mut candidate = self.clone();
        candidate.frozen_routing = Some(next.clone());
        Ok(PreparedRoutingUpdate {
            owner: Arc::clone(&self.routing),
            base_revision,
            next,
            changes_rules,
            direct_routes: candidate.has_direct_routes(),
            direct_dns: candidate.needs_direct_dns(),
        })
    }

    pub fn publish_routing_update(
        &self,
        prepared: PreparedRoutingUpdate,
    ) -> Result<(), RoutingUpdateError> {
        if !Arc::ptr_eq(&self.routing, &prepared.owner) {
            return Err(RoutingUpdateError::OwnerChanged);
        }
        self.routing.publish(prepared.base_revision, prepared.next)
    }

    pub fn with_ads(mut self, cache_dir: &Path) -> Self {
        self.ads = crate::live_ads::LiveAds::watch(cache_dir, self.ads_enabled);
        self
    }

    pub fn ads_revision(&self) -> Option<String> {
        self.ads.rules().map(|ads| ads.revision().to_owned())
    }

    pub fn update_ads_enabled(&self, enabled: bool) {
        self.ads.set_enabled(enabled);
    }

    pub fn supports_live_ads(&self) -> bool {
        self.ads.supported()
    }
    pub(crate) fn start_ads_watcher(&self) {
        // Android constructs this policy on a JNI thread before Tokio exists.
        if self.supports_live_ads() {
            self.ads.start_watching();
        }
    }

    pub fn needs_direct_dns(&self) -> bool {
        !self.countries.is_empty()
            || !self.domains.is_empty()
            || self.routing_snapshot().settings.has_direct_domains()
    }
    pub fn has_direct_routes(&self) -> bool {
        self.allow_lan
            || !self.countries.is_empty()
            || !self.networks.is_empty()
            || !self.domains.is_empty()
            || self.routing_snapshot().settings.has_direct_rules()
    }
    pub(crate) fn has_ip_rules(&self) -> bool {
        self.allow_lan || !self.routing_snapshot().networks.is_empty()
    }
    pub(crate) fn custom_host(&self, host: &str) -> Option<GeoRoute> {
        let host = usque_core::config::canonical_bypass_domain(host)?;
        self.routing_snapshot()
            .domains
            .iter()
            .find(|(domain, _)| {
                host == *domain
                    || host
                        .strip_suffix(domain.as_str())
                        .is_some_and(|prefix| prefix.ends_with('.'))
            })
            .map(|(_, action)| (*action).into())
    }
    pub(crate) fn custom_ip(&self, ip: IpAddr) -> Option<GeoRoute> {
        let ip = ip.to_canonical();
        self.routing_snapshot()
            .networks
            .iter()
            .find(|(net, _)| net.contains(&ip))
            .map(|(_, action)| (*action).into())
    }
    pub(crate) fn rejects_ip(&self, ip: IpAddr) -> bool {
        self.custom_ip(ip) == Some(GeoRoute::Reject)
    }
    pub(crate) fn resolved_route(&self, host: &str, ip: IpAddr) -> GeoRoute {
        if self.frozen_routing.is_none() {
            return self.freeze_routing().resolved_route(host, ip);
        }
        let policy = self;
        if policy.rejects_ip(ip) {
            return GeoRoute::Reject;
        }
        if let Some(route) = policy.custom_host(host) {
            return route;
        }
        if let Some(route) = policy.custom_ip(ip) {
            return route;
        }
        let route = policy.route_host(host);
        if route != GeoRoute::Reject && policy.is_lan_direct(ip) {
            GeoRoute::Direct
        } else {
            route
        }
    }

    /// Returns the configured country codes in their caller-provided order.
    pub fn countries(&self) -> &[CountryCode] {
        &self.countries
    }

    /// Returns whether LAN bypass, application routing or Ads was configured.
    pub fn is_enabled(&self) -> bool {
        self.allow_lan
            || (self.classifier.is_some() && !self.countries.is_empty())
            || !self.networks.is_empty()
            || !self.domains.is_empty()
            || !self.routing_snapshot().settings.rules.is_empty()
            || self.supports_live_ads()
            || self.ads_enabled
    }

    /// Selects a route for a hostname using custom suffixes and GeoSite.
    pub fn route_host(&self, host: &str) -> GeoRoute {
        let canonical = usque_core::config::canonical_bypass_domain(host);
        let host = canonical.as_deref().unwrap_or(host);
        if let Some(route) = self.custom_host(host) {
            return route;
        }
        if self.ads.rules().is_some_and(|ads| ads.contains(host)) {
            return GeoRoute::Reject;
        }
        if !self.domains.is_empty()
            && let Some(host) = usque_core::config::canonical_bypass_domain(host)
            && self.domains.iter().any(|domain| {
                host == *domain
                    || host
                        .strip_suffix(domain)
                        .is_some_and(|prefix| prefix.ends_with('.'))
            })
        {
            return GeoRoute::Direct;
        }
        let Some(classifier) = &self.classifier else {
            return GeoRoute::Tunnel;
        };
        if self.countries.is_empty() || !valid_host(host) {
            return GeoRoute::Tunnel;
        }
        if self
            .countries
            .iter()
            .any(|country| classifier.host_matches(host, country))
        {
            GeoRoute::Direct
        } else {
            GeoRoute::Tunnel
        }
    }

    /// Selects a route using explicit rules, LAN bypass, then networks and GeoIP.
    pub fn route_ip(&self, ip: IpAddr) -> GeoRoute {
        let ip = ip.to_canonical();
        if let Some(route) = self.custom_ip(ip) {
            return route;
        }
        if self.is_lan_direct(ip) || self.networks.iter().any(|network| network.contains(&ip)) {
            return GeoRoute::Direct;
        }
        let Some(classifier) = &self.classifier else {
            return GeoRoute::Tunnel;
        };
        if self.countries.is_empty() {
            return GeoRoute::Tunnel;
        }
        if self
            .countries
            .iter()
            .any(|country| classifier.ip_matches(ip, country))
        {
            GeoRoute::Direct
        } else {
            GeoRoute::Tunnel
        }
    }

    fn is_lan_direct(&self, ip: IpAddr) -> bool {
        self.allow_lan && usque_core::config::is_lan_bypass_address(ip)
    }
}

fn valid_host(host: &str) -> bool {
    let host = host.trim().trim_end_matches('.');
    !host.is_empty()
        && host.len() <= 253
        && !host.contains('/')
        && !host.contains(char::is_whitespace)
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum GeoTarget<'a> {
    Host(&'a str),
    Ip(IpAddr),
}

impl<'a> GeoTarget<'a> {
    pub(crate) fn from_host(host: &'a str) -> Self {
        host.parse()
            .map(Self::Ip)
            .unwrap_or_else(|_| Self::Host(host))
    }

    pub(crate) fn route(self, policy: &GeoDirectPolicy) -> GeoRoute {
        match self {
            Self::Host(host) => policy.route_host(host),
            Self::Ip(ip) => policy.route_ip(ip),
        }
    }
}

/// A TCP stream connected either through the userspace tunnel or directly on
/// the protected physical network.
pub(crate) enum RoutedTcpStream {
    Tunnel(TunnelTcpStream),
    Direct {
        stream: TcpStream,
        counters: Arc<TrafficCounters>,
        _lease: DirectEgressLease,
    },
}

impl RoutedTcpStream {
    pub(crate) fn local_addr(&self) -> io::Result<SocketAddr> {
        match self {
            Self::Tunnel(stream) => stream.local_addr(),
            Self::Direct { stream, .. } => stream.local_addr(),
        }
    }
}

impl AsyncRead for RoutedTcpStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Self::Tunnel(stream) => Pin::new(stream).poll_read(cx, buf),
            Self::Direct {
                stream, counters, ..
            } => {
                let before = buf.filled().len();
                let result = Pin::new(stream).poll_read(cx, buf);
                if matches!(result, Poll::Ready(Ok(()))) {
                    counters.record_received(buf.filled().len().saturating_sub(before));
                }
                result
            }
        }
    }
}

impl AsyncWrite for RoutedTcpStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self.get_mut() {
            Self::Tunnel(stream) => Pin::new(stream).poll_write(cx, buf),
            Self::Direct {
                stream, counters, ..
            } => {
                let result = Pin::new(stream).poll_write(cx, buf);
                if let Poll::Ready(Ok(written)) = result {
                    counters.record_sent(written);
                }
                result
            }
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Self::Tunnel(stream) => Pin::new(stream).poll_flush(cx),
            Self::Direct { stream, .. } => Pin::new(stream).poll_flush(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Self::Tunnel(stream) => Pin::new(stream).poll_shutdown(cx),
            Self::Direct { stream, .. } => Pin::new(stream).poll_shutdown(cx),
        }
    }

    fn is_write_vectored(&self) -> bool {
        match self {
            Self::Tunnel(stream) => stream.is_write_vectored(),
            Self::Direct { stream, .. } => stream.is_write_vectored(),
        }
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        match self.get_mut() {
            Self::Tunnel(stream) => Pin::new(stream).poll_write_vectored(cx, bufs),
            Self::Direct {
                stream, counters, ..
            } => {
                let result = Pin::new(stream).poll_write_vectored(cx, bufs);
                if let Poll::Ready(Ok(written)) = result {
                    counters.record_sent(written);
                }
                result
            }
        }
    }
}

enum DirectFallback<T> {
    Direct(TcpStream, DirectEgressLease),
    Fallback(T),
    Failed(&'static str),
    Rejected,
}

enum DirectConnectFailure {
    Dns,
    Rejected,
    Unavailable,
    Connect(Vec<IpAddr>),
}

/// System-mode failures retain the existing tunnel fallback. Encrypted DNS
/// failure is terminal. After a successful answer, fallback receives only the
/// physically usable candidates, so resolving again cannot reintroduce a family
/// that DIRECT cannot carry. Encrypted answers must never be re-resolved in plaintext.
async fn connect_with_geo_fallback<T, E, F, Fut>(
    policy: &GeoDirectPolicy,
    protector: &dyn SocketProtector,
    target: GeoTarget<'_>,
    port: u16,
    fallback: F,
) -> Result<DirectFallback<T>, E>
where
    F: FnOnce(Option<Vec<IpAddr>>) -> Fut,
    Fut: Future<Output = Result<T, E>>,
{
    let frozen = policy.freeze_routing();
    let policy = &frozen;
    if target.route(policy) == GeoRoute::Reject {
        return Ok(DirectFallback::Rejected);
    }
    let mut resolved = None;
    if target.route(policy) == GeoRoute::Direct {
        match connect_direct(protector, target, port, policy).await {
            Ok((stream, lease)) => return Ok(DirectFallback::Direct(stream, lease)),
            Err(DirectConnectFailure::Rejected) => return Ok(DirectFallback::Rejected),
            Err(DirectConnectFailure::Unavailable) => {
                return Ok(DirectFallback::Failed(DIRECT_FAMILY_UNAVAILABLE));
            }
            Err(DirectConnectFailure::Dns) if protector.direct_dns_resolver().is_some() => {
                return Ok(DirectFallback::Failed("encrypted_direct_dns_failed"));
            }
            Err(DirectConnectFailure::Dns)
                if !direct_family_available(protector, (Ipv4Addr::UNSPECIFIED, port).into())
                    || !direct_family_available(
                        protector,
                        (Ipv6Addr::UNSPECIFIED, port).into(),
                    ) =>
            {
                // An unchecked hostname fallback could select the unavailable
                // family at the exit when physical DNS has failed.
                return Ok(DirectFallback::Failed("direct_resolution_failed"));
            }
            Err(failure) => {
                if let DirectConnectFailure::Connect(addresses) = failure {
                    resolved = Some(addresses);
                }
                tracing::debug!(
                    reason_code = "direct_connect_failed",
                    "GEO direct TCP connect failed; falling back to tunnel"
                );
            }
        }
    }
    fallback(resolved).await.map(DirectFallback::Fallback)
}

/// System direct-DNS failure retains the old tunnel-resolution fallback, but
/// every returned address is still checked. Encrypted DNS failures are terminal.
pub(crate) async fn resolve_routing_host(
    policy: &GeoDirectPolicy,
    protector: &dyn SocketProtector,
    resolver: &crate::dns::Resolver,
    host: &str,
    port: u16,
) -> Result<(Vec<IpAddr>, bool), String> {
    let frozen = policy.freeze_routing();
    let policy = &frozen;
    let direct = policy.route_host(host) == GeoRoute::Direct;
    if direct {
        match crate::split_dns::resolve_direct_routed(protector, host, port, policy).await {
            Ok(addresses) => {
                return Ok((
                    addresses.into_iter().map(|address| address.ip()).collect(),
                    false,
                ));
            }
            Err(error)
                if error == "routing_rejected"
                    || error == DIRECT_FAMILY_UNAVAILABLE
                    || protector.direct_dns_resolver().is_some() =>
            {
                return Err(error);
            }
            Err(_) => {}
        }
    }
    resolver
        .resolve_for_policy_snapshot(host, policy)
        .await
        .map(|addresses| (addresses, direct))
        .map_err(|error| error.to_string())
}

pub(crate) async fn connect_routed<E, F, Fut>(
    policy: &GeoDirectPolicy,
    protector: &dyn SocketProtector,
    counters: Arc<TrafficCounters>,
    destination: (GeoTarget<'_>, u16, Option<&crate::dns::Resolver>),
    failures: (impl FnOnce(&'static str) -> E, impl Fn() -> E),
    tunnel: F,
) -> Result<RoutedTcpStream, E>
where
    F: FnOnce(Option<Vec<IpAddr>>) -> Fut,
    Fut: Future<Output = Result<TunnelTcpStream, E>>,
{
    let frozen = policy.freeze_routing();
    let policy = &frozen;
    let (target, port, resolver) = destination;
    let (direct_failure, rejected) = failures;
    if target.route(policy) == GeoRoute::Reject {
        return Err(rejected());
    }
    if policy.has_ip_rules()
        && let GeoTarget::Host(host) = target
    {
        let Some(resolver) = resolver else {
            return Err(rejected());
        };
        let (addresses, tunnel_only) =
            match resolve_routing_host(policy, protector, resolver, host, port).await {
                Ok(result) => result,
                Err(error) if error.contains("routing_rejected") => return Err(rejected()),
                Err(error) if error == DIRECT_FAMILY_UNAVAILABLE => {
                    return Err(direct_failure(DIRECT_FAMILY_UNAVAILABLE));
                }
                Err(_) => return Err(direct_failure("encrypted_direct_dns_failed")),
            };
        let mut allowed = Vec::new();
        let mut unavailable = false;
        let addresses = addresses
            .into_iter()
            .filter(|ip| !policy.rejects_ip(*ip))
            .filter(|ip| {
                let usable = policy.resolved_route(host, *ip) != GeoRoute::Direct
                    || direct_family_available(protector, SocketAddr::new(*ip, port));
                unavailable |= !usable;
                usable
            })
            .take(MAX_DIRECT_ADDRESSES)
            .collect::<Vec<_>>();
        for ip in addresses {
            let route = if tunnel_only {
                GeoRoute::Tunnel
            } else {
                policy.resolved_route(host, ip)
            };
            match route {
                GeoRoute::Reject => continue,
                GeoRoute::Direct => {
                    if let Ok((stream, lease)) =
                        connect_direct_ip(protector, SocketAddr::new(ip, port)).await
                    {
                        return Ok(RoutedTcpStream::Direct {
                            stream,
                            counters,
                            _lease: lease,
                        });
                    }
                    if !direct_family_available(protector, SocketAddr::new(ip, port)) {
                        unavailable = true;
                        continue;
                    }
                }
                GeoRoute::Tunnel => {}
            }
            allowed.push(ip);
        }
        if allowed.is_empty() {
            if unavailable {
                return Err(direct_failure(DIRECT_FAMILY_UNAVAILABLE));
            }
            return Err(rejected());
        }
        return tunnel(Some(allowed)).await.map(RoutedTcpStream::Tunnel);
    }
    connect_with_geo_fallback(policy, protector, target, port, tunnel)
        .await
        .and_then(|stream| match stream {
            DirectFallback::Direct(stream, lease) => Ok(RoutedTcpStream::Direct {
                stream,
                counters,
                _lease: lease,
            }),
            DirectFallback::Fallback(stream) => Ok(RoutedTcpStream::Tunnel(stream)),
            DirectFallback::Failed(reason) => Err(direct_failure(reason)),
            DirectFallback::Rejected => Err(rejected()),
        })
}

async fn connect_direct(
    protector: &dyn SocketProtector,
    target: GeoTarget<'_>,
    port: u16,
    policy: &GeoDirectPolicy,
) -> Result<(TcpStream, DirectEgressLease), DirectConnectFailure> {
    let addresses = match target {
        GeoTarget::Host(host) => {
            crate::split_dns::resolve_direct_routed(protector, host, port, policy)
                .await
                .map_err(|error| {
                    if error == "routing_rejected" {
                        DirectConnectFailure::Rejected
                    } else if error == DIRECT_FAMILY_UNAVAILABLE {
                        DirectConnectFailure::Unavailable
                    } else {
                        DirectConnectFailure::Dns
                    }
                })?
        }
        GeoTarget::Ip(ip) => vec![SocketAddr::new(ip, port)],
    };
    let mut unavailable = false;
    let addresses = addresses
        .into_iter()
        .filter(|address| !address.ip().is_unspecified() && !address.ip().is_multicast())
        .filter(|address| {
            let usable = direct_family_available(protector, *address);
            unavailable |= !usable;
            usable
        })
        .take(MAX_DIRECT_ADDRESSES)
        .collect::<Vec<_>>();
    for address in &addresses {
        let remote = SocketAddr::new(address.ip(), port);
        if let Ok(stream) = connect_direct_address(protector, remote).await {
            return Ok(stream);
        }
    }
    let addresses = addresses
        .into_iter()
        .filter(|address| {
            let usable = direct_family_available(protector, *address);
            unavailable |= !usable;
            usable
        })
        .map(|address| address.ip())
        .collect::<Vec<_>>();
    if addresses.is_empty() && unavailable {
        Err(DirectConnectFailure::Unavailable)
    } else {
        Err(DirectConnectFailure::Connect(addresses))
    }
}

async fn connect_direct_address(
    protector: &dyn SocketProtector,
    remote: SocketAddr,
) -> Result<(TcpStream, DirectEgressLease), String> {
    if !direct_family_available(protector, remote) {
        return Err(DIRECT_FAMILY_UNAVAILABLE.into());
    }
    let socket = if remote.is_ipv4() {
        TcpSocket::new_v4()
    } else {
        TcpSocket::new_v6()
    }
    .map_err(|error| error.to_string())?;
    let lease = protector
        .protect_for_target(socket_handle(&socket), remote, DirectProtocol::Tcp)
        .await
        .map_err(|error| format!("protect direct socket: {error}"))?;
    let stream = timeout(DIRECT_CONNECT_TIMEOUT, socket.connect(remote))
        .await
        .map_err(|_| format!("connect to {remote} timed out"))?
        .map_err(|error| error.to_string())?;
    stream
        .set_nodelay(true)
        .map_err(|error| error.to_string())?;
    Ok((stream, lease))
}

pub(crate) async fn connect_direct_ip(
    protector: &dyn SocketProtector,
    remote: SocketAddr,
) -> Result<(TcpStream, DirectEgressLease), String> {
    connect_direct_address(protector, remote).await
}

pub(crate) fn bind_protected_udp(
    protector: &dyn SocketProtector,
    ipv6: bool,
) -> Result<UdpSocket, String> {
    let bind = if ipv6 {
        SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), 0)
    } else {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0)
    };
    let socket = StdUdpSocket::bind(bind).map_err(|error| error.to_string())?;
    protector
        .protect(socket_handle(&socket))
        .map_err(|error| format!("protect direct UDP socket: {error}"))?;
    socket
        .set_nonblocking(true)
        .map_err(|error| error.to_string())?;
    UdpSocket::from_std(socket).map_err(|error| error.to_string())
}

pub(crate) async fn bind_direct_udp(
    protector: &dyn SocketProtector,
    remote: SocketAddr,
) -> Result<(UdpSocket, DirectEgressLease), String> {
    let bind = if remote.is_ipv6() {
        SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), 0)
    } else {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0)
    };
    let socket = StdUdpSocket::bind(bind).map_err(|error| error.to_string())?;
    let lease = protector
        .protect_for_target(socket_handle(&socket), remote, DirectProtocol::Udp)
        .await
        .map_err(|error| format!("protect direct UDP socket: {error}"))?;
    socket
        .set_nonblocking(true)
        .map_err(|error| error.to_string())?;
    let socket = UdpSocket::from_std(socket).map_err(|error| error.to_string())?;
    Ok((socket, lease))
}

#[cfg(test)]
pub(crate) fn lan_test_policy(allow_lan: bool) -> GeoDirectPolicy {
    GeoDirectPolicy::disabled()
        .with_custom_rules(&usque_core::Profile {
            allow_lan,
            ..Default::default()
        })
        .unwrap()
}

/// Records physical attempts but refuses them before any network I/O.
#[cfg(test)]
#[derive(Default)]
pub(crate) struct LanProbeProtector {
    pub(crate) attempts: std::sync::Mutex<Vec<(SocketAddr, DirectProtocol)>>,
    pub(crate) family_mask: Option<u8>,
    pub(crate) resolved: Option<Vec<IpAddr>>,
    pub(crate) generation: Option<u64>,
}

#[cfg(test)]
#[async_trait::async_trait]
impl SocketProtector for LanProbeProtector {
    fn protect(&self, _socket: crate::socket::SocketHandle) -> Result<(), String> {
        Ok(())
    }

    fn resolve(&self, _host: &str, port: u16) -> Result<Vec<SocketAddr>, String> {
        self.resolved
            .as_ref()
            .map(|addresses| {
                addresses
                    .iter()
                    .map(|ip| SocketAddr::new(*ip, port))
                    .collect()
            })
            .ok_or_else(|| "test forbids physical DNS".into())
    }

    fn endpoint_family_available(&self, endpoint: SocketAddr) -> Option<bool> {
        self.family_mask
            .map(|mask| mask & if endpoint.is_ipv4() { 1 } else { 2 } != 0)
    }

    fn tun_direct_available(&self) -> bool {
        true
    }

    fn network_generation(&self) -> Option<u64> {
        self.generation
    }

    async fn protect_for_target(
        &self,
        _socket: crate::socket::SocketHandle,
        remote: SocketAddr,
        protocol: DirectProtocol,
    ) -> Result<DirectEgressLease, String> {
        self.attempts.lock().unwrap().push((remote, protocol));
        Err("test direct protection rejection".into())
    }
}

#[cfg(test)]
pub(crate) fn routing_test_policy(
    entries: &[(&str, usque_core::RoutingAction)],
) -> GeoDirectPolicy {
    GeoDirectPolicy::disabled()
        .with_custom_rules(&routing_test_profile(entries))
        .unwrap()
}

#[cfg(test)]
pub(crate) fn routing_test_profile(
    entries: &[(&str, usque_core::RoutingAction)],
) -> usque_core::Profile {
    usque_core::Profile {
        allow_lan: false,
        routing: usque_core::RoutingSettings {
            rules: entries
                .iter()
                .map(|(target, action)| usque_core::RoutingRule {
                    id: uuid::Uuid::new_v4(),
                    kind: if target.contains('/') || target.parse::<IpAddr>().is_ok() {
                        usque_core::RoutingMatch::Cidr
                    } else {
                        usque_core::RoutingMatch::Domain
                    },
                    target: (*target).into(),
                    action: *action,
                })
                .collect(),
            ads_enabled: false,
        },
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::{
        CountryCode, DirectFallback, GeoDirectClassifier, GeoDirectPolicy, GeoRoute, GeoTarget,
        connect_with_geo_fallback,
    };
    use crate::socket::{SocketHandle, SocketProtector};

    #[test]
    fn hot_routing_publication_updates_readers_and_preserves_captured_flows() {
        use usque_core::RoutingAction::{Direct, Proxy, Reject};
        let policy = super::routing_test_policy(&[
            ("hot.test", Direct),
            ("192.0.2.0/24", Proxy),
            ("2001:db8::/64", Direct),
        ]);
        let observer = policy.clone();
        let captured = policy.freeze_routing();
        let revision = policy.routing_revision();
        let candidate = super::routing_test_profile(&[
            ("hot.test", Reject),
            ("192.0.2.0/24", Direct),
            ("2001:db8::/64", Reject),
        ]);
        let prepared = policy.prepare_routing_update(&candidate).unwrap();
        assert!(prepared.has_direct_routes());
        assert!(!prepared.needs_direct_dns());
        assert_eq!(observer.route_host("hot.test"), GeoRoute::Direct);
        policy.publish_routing_update(prepared).unwrap();
        assert_eq!(observer.routing_revision(), revision + 1);
        assert_eq!(observer.route_host("hot.test"), GeoRoute::Reject);
        assert_eq!(captured.route_host("hot.test"), GeoRoute::Direct);
        assert_eq!(captured.routing_revision(), revision);
        assert!(!captured.routing_is_current(revision));
        for ip in ["192.0.2.7", "::ffff:192.0.2.7"] {
            assert_eq!(observer.route_ip(ip.parse().unwrap()), GeoRoute::Direct);
            assert_eq!(captured.route_ip(ip.parse().unwrap()), GeoRoute::Tunnel);
        }
        assert_eq!(
            observer.route_ip("2001:db8::7".parse().unwrap()),
            GeoRoute::Reject
        );
        assert_eq!(
            captured.route_ip("2001:db8::7".parse().unwrap()),
            GeoRoute::Direct
        );
        let removed = super::routing_test_profile(&[]);
        observer
            .publish_routing_update(observer.prepare_routing_update(&removed).unwrap())
            .unwrap();
        assert!(!policy.has_direct_routes());
        assert_eq!(policy.route_host("hot.test"), GeoRoute::Tunnel);
    }

    #[test]
    fn hot_routing_rejects_stale_foreign_and_invalid_updates_without_mutation() {
        use usque_core::RoutingAction::{Direct, Reject};
        let policy = super::routing_test_policy(&[("hot.test", Direct)]);
        let profile = super::routing_test_profile(&[("hot.test", Reject)]);
        let stale = policy.prepare_routing_update(&profile).unwrap();
        let first = policy.prepare_routing_update(&profile).unwrap();
        let foreign = super::routing_test_policy(&[("hot.test", Direct)]);
        assert_eq!(
            foreign.publish_routing_update(first),
            Err(super::RoutingUpdateError::OwnerChanged)
        );
        assert_eq!(foreign.route_host("hot.test"), GeoRoute::Direct);
        let independent = policy.clone().with_custom_rules(&profile).unwrap();
        assert_eq!(independent.route_host("hot.test"), GeoRoute::Reject);
        assert_eq!(policy.route_host("hot.test"), GeoRoute::Direct);
        policy
            .publish_routing_update(policy.prepare_routing_update(&profile).unwrap())
            .unwrap();
        assert_eq!(
            policy.publish_routing_update(stale),
            Err(super::RoutingUpdateError::RevisionChanged)
        );
        let revision = policy.routing_revision();
        let mut invalid = profile;
        invalid.routing.rules[0].target = "https://invalid.test".into();
        assert!(policy.prepare_routing_update(&invalid).is_err());
        assert_eq!(policy.routing_revision(), revision);
        assert_eq!(policy.route_host("hot.test"), GeoRoute::Reject);
    }

    #[test]
    fn prepared_direct_requirements_include_retained_cold_routes() {
        let profile = usque_core::Profile {
            allow_lan: true,
            bypass_domains: vec!["retained.test".into()],
            split_exclusions: vec!["192.0.2.0/24".parse().unwrap()],
            ..Default::default()
        };
        let policy = GeoDirectPolicy::disabled()
            .with_custom_rules(&profile)
            .unwrap();
        let prepared = policy
            .prepare_routing_update(&super::routing_test_profile(&[]))
            .unwrap();
        assert!(prepared.has_direct_routes());
        assert!(prepared.needs_direct_dns());
        policy.publish_routing_update(prepared).unwrap();
        assert_eq!(policy.route_host("retained.test"), GeoRoute::Direct);
        assert_eq!(
            policy.route_ip("192.0.2.7".parse().unwrap()),
            GeoRoute::Direct
        );
        assert_eq!(
            policy.route_ip("10.1.2.3".parse().unwrap()),
            GeoRoute::Direct
        );
        assert_eq!(
            policy.route_ip("127.0.0.1".parse().unwrap()),
            GeoRoute::Tunnel
        );
    }

    #[test]
    fn exhausted_rule_revision_still_distinguishes_changes_from_noop_transactions() {
        let mut policy = GeoDirectPolicy::disabled();
        policy.routing = Arc::new(super::LiveRouting::new(
            super::RoutingSnapshot::new(&usque_core::RoutingSettings::default(), u64::MAX).unwrap(),
        ));
        let prepared = policy
            .prepare_routing_update(&super::routing_test_profile(&[]))
            .unwrap();
        assert!(!prepared.changes_rules());
        policy.publish_routing_update(prepared).unwrap();
        let next =
            super::routing_test_profile(&[("blocked.test", usque_core::RoutingAction::Reject)]);
        let prepared = policy.prepare_routing_update(&next).unwrap();
        assert!(prepared.changes_rules());
        assert_eq!(
            policy.publish_routing_update(prepared),
            Err(super::RoutingUpdateError::RevisionExhausted)
        );
        assert_eq!(policy.routing_revision(), u64::MAX);
        assert_eq!(policy.route_host("blocked.test"), GeoRoute::Tunnel);
    }

    struct FakeClassifier {
        host_hit: bool,
        ip_hit: bool,
    }

    impl GeoDirectClassifier for FakeClassifier {
        fn host_matches(&self, host: &str, country: &CountryCode) -> bool {
            self.host_hit && host == "direct.test" && country.as_str() == "CN"
        }

        fn ip_matches(&self, ip: IpAddr, country: &CountryCode) -> bool {
            self.ip_hit && ip == Ipv4Addr::new(203, 0, 113, 7) && country.as_str() == "CN"
        }
    }

    struct FakeProtector {
        resolved: SocketAddr,
        reject_protect: bool,
        protect_calls: AtomicUsize,
        resolve_calls: AtomicUsize,
    }

    impl SocketProtector for FakeProtector {
        fn protect(&self, _socket: SocketHandle) -> Result<(), String> {
            self.protect_calls.fetch_add(1, Ordering::SeqCst);
            if self.reject_protect {
                Err("test protection rejection".to_owned())
            } else {
                Ok(())
            }
        }

        fn resolve(&self, host: &str, port: u16) -> Result<Vec<SocketAddr>, String> {
            self.resolve_calls.fetch_add(1, Ordering::SeqCst);
            if host != "direct.test" || port != self.resolved.port() {
                return Err("unexpected direct resolver input".to_owned());
            }
            Ok(vec![self.resolved])
        }
    }

    fn policy(host_hit: bool, ip_hit: bool) -> GeoDirectPolicy {
        GeoDirectPolicy::with_classifier(
            Arc::new(FakeClassifier { host_hit, ip_hit }),
            [CountryCode::parse("CN").unwrap()],
        )
    }

    #[test]
    fn geoip_only_cache_loads_and_unknown_targets_stay_in_the_tunnel() {
        let directory = tempfile::tempdir().unwrap();
        let country = CountryCode::parse("CN").unwrap();
        let path = usque_geo::geoip_cache_path(directory.path(), &country);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            include_bytes!("../../usque-geo/tests/fixtures/geoip-cn.dat"),
        )
        .unwrap();
        let policy = GeoDirectPolicy::load(directory.path(), [country.clone()]).unwrap();
        assert_eq!(
            policy.route_ip("1.2.3.4".parse().unwrap()),
            GeoRoute::Direct
        );
        assert_eq!(
            policy.route_ip("192.0.2.1".parse().unwrap()),
            GeoRoute::Tunnel
        );
        assert_eq!(policy.route_host("example.cn"), GeoRoute::Tunnel);
        assert_eq!(
            policy.resolved_route("example.cn", "1.2.3.4".parse().unwrap()),
            GeoRoute::Tunnel
        );
        std::fs::write(&path, b"invalid").unwrap();
        assert!(GeoDirectPolicy::load(directory.path(), [country.clone()]).is_err());
        std::fs::remove_file(path).unwrap();
        assert!(GeoDirectPolicy::load(directory.path(), [country]).is_err());
    }

    #[tokio::test]
    async fn unavailable_direct_family_never_enters_tcp_tunnel_fallback() {
        use ts_netstack_smoltcp::netcore::HasChannel;
        let (stack, _pipe) = crate::netstack::bounded_piped(Default::default());
        let resolver = crate::dns::Resolver::new(
            stack.command_channel(),
            Ipv4Addr::UNSPECIFIED,
            std::net::Ipv6Addr::UNSPECIFIED,
            vec![],
            usque_core::ProxyDnsMode::Remote,
            Arc::new(crate::socket::NoopSocketProtector),
        );
        for (mask, ip) in [(1, "2001:db8::7"), (2, "192.0.2.7")] {
            let ip: IpAddr = ip.parse().unwrap();
            let protector = super::LanProbeProtector {
                family_mask: Some(mask),
                resolved: Some(vec![ip]),
                ..Default::default()
            };
            for with_ip_rules in [false, true] {
                let mut entries = vec![("direct.test", usque_core::RoutingAction::Direct)];
                let ip_text = ip.to_string();
                if with_ip_rules {
                    entries.push((&ip_text, usque_core::RoutingAction::Direct));
                }
                let policy = super::routing_test_policy(&entries);
                for target in [GeoTarget::Host("direct.test"), GeoTarget::Ip(ip)] {
                    if matches!(target, GeoTarget::Ip(_)) && !with_ip_rules {
                        continue;
                    }
                    let result = super::connect_routed(
                        &policy,
                        &protector,
                        Arc::default(),
                        (target, 443, Some(&resolver)),
                        (|_| "unavailable", || "rejected"),
                        |_| async { panic!("unavailable DIRECT family reached the tunnel") },
                    )
                    .await;
                    assert!(result.is_err());
                }
            }
            assert!(protector.attempts.lock().unwrap().is_empty());
        }
    }

    #[tokio::test]
    async fn direct_tcp_fallback_keeps_only_physically_available_candidates() {
        let ipv4: IpAddr = "192.0.2.7".parse().unwrap();
        let protector = super::LanProbeProtector {
            family_mask: Some(1),
            resolved: Some(vec!["2001:db8::7".parse().unwrap(), ipv4]),
            ..Default::default()
        };
        let result: Result<_, ()> = connect_with_geo_fallback(
            &policy(true, false),
            &protector,
            GeoTarget::Host("direct.test"),
            443,
            |addresses| async move {
                assert_eq!(addresses, Some(vec![ipv4]));
                Ok(())
            },
        )
        .await;
        assert!(matches!(result, Ok(DirectFallback::Fallback(()))));
        assert_eq!(
            *protector.attempts.lock().unwrap(),
            [(
                SocketAddr::new(ipv4, 443),
                crate::socket::DirectProtocol::Tcp
            )]
        );
    }

    #[test]
    fn direct_family_knowledge_preserves_unknown_local_and_mapped_addresses() {
        let protector = super::LanProbeProtector {
            family_mask: Some(1),
            ..Default::default()
        };
        for address in [
            "[::1]:443",
            "[fe80::1]:443",
            "[fd00::1]:443",
            "[::ffff:192.0.2.7]:443",
        ] {
            assert!(super::direct_family_available(
                &protector,
                address.parse().unwrap()
            ));
        }
        assert!(!super::direct_family_available(
            &protector,
            "[2001:db8::7]:443".parse().unwrap()
        ));
        assert!(super::direct_family_available(
            &crate::socket::NoopSocketProtector,
            "[2001:db8::7]:443".parse().unwrap()
        ));
    }

    #[test]
    fn allow_lan_matches_only_private_and_link_local_ranges_in_both_families() {
        for allow_lan in [false, true] {
            let policy = super::lan_test_policy(allow_lan);
            assert_eq!(policy.is_enabled(), allow_lan);
            assert_eq!(policy.has_direct_routes(), allow_lan);
            assert_eq!(policy.has_ip_rules(), allow_lan);
            assert!(!policy.needs_direct_dns());
            for ip in [
                "10.0.0.0",
                "10.255.255.255",
                "172.16.0.0",
                "172.31.255.255",
                "192.168.0.0",
                "192.168.255.255",
                "169.254.0.0",
                "169.254.255.255",
                "fc00::",
                "fdff:ffff:ffff:ffff:ffff:ffff:ffff:ffff",
                "fe80::",
                "febf:ffff:ffff:ffff:ffff:ffff:ffff:ffff",
                "::ffff:192.168.1.10",
            ] {
                let expected = if allow_lan {
                    GeoRoute::Direct
                } else {
                    GeoRoute::Tunnel
                };
                assert_eq!(policy.route_ip(ip.parse().unwrap()), expected, "{ip}");
                assert_eq!(
                    policy.resolved_route("nas.example", ip.parse().unwrap()),
                    expected,
                    "{ip}"
                );
            }
            for ip in [
                "9.255.255.255",
                "11.0.0.0",
                "172.15.255.255",
                "172.32.0.0",
                "192.167.255.255",
                "192.169.0.0",
                "169.253.255.255",
                "169.255.0.0",
                "100.64.0.1",
                "127.0.0.1",
                "0.0.0.0",
                "224.0.0.1",
                "fbff::1",
                "fe00::1",
                "fe7f::1",
                "fec0::1",
                "2001:db8::1",
                "::1",
                "ff02::1",
                "::ffff:1.1.1.1",
            ] {
                assert_eq!(
                    policy.route_ip(ip.parse().unwrap()),
                    GeoRoute::Tunnel,
                    "{ip}"
                );
                assert_eq!(
                    policy.resolved_route("public.example", ip.parse().unwrap()),
                    GeoRoute::Tunnel,
                    "{ip}"
                );
            }
            assert_eq!(policy.route_host("nas.example"), GeoRoute::Tunnel);
        }
    }

    #[test]
    fn allow_lan_preserves_explicit_domain_and_address_rules() {
        use usque_core::RoutingAction::{Proxy, Reject};
        let mut policy = super::routing_test_policy(&[
            ("192.168.1.0/24", Reject),
            ("192.168.1.7", Proxy),
            ("fd00::/8", Proxy),
            ("proxy.example", Proxy),
            ("blocked.example", Reject),
        ]);
        policy.allow_lan = true;
        for (ip, expected) in [
            ("192.168.1.8", GeoRoute::Reject),
            ("::ffff:192.168.1.8", GeoRoute::Reject),
            ("192.168.1.7", GeoRoute::Tunnel),
            ("fd00::1", GeoRoute::Tunnel),
        ] {
            assert_eq!(policy.route_ip(ip.parse().unwrap()), expected);
            assert_eq!(
                policy.resolved_route("nas.example", ip.parse().unwrap()),
                expected
            );
        }
        assert_eq!(
            policy.resolved_route("proxy.example", "10.0.0.2".parse().unwrap()),
            GeoRoute::Tunnel
        );
        assert_eq!(
            policy.resolved_route("blocked.example", "10.0.0.2".parse().unwrap()),
            GeoRoute::Reject
        );
        assert_eq!(
            policy.resolved_route("proxy.example", "192.168.1.8".parse().unwrap()),
            GeoRoute::Reject
        );
    }

    #[tokio::test]
    async fn allow_lan_checks_tunnel_dns_answers_without_physical_resolution() {
        use tokio_util::task::AbortOnDropHandle;
        use ts_netstack_smoltcp::CreateSocket;
        use ts_netstack_smoltcp::netcore::{HasChannel, NetstackControl};

        let (client, server) = ts_netstack_smoltcp::piped_pair(Default::default());
        let client_channel = client.command_channel();
        let server_channel = server.command_channel();
        let _client_task = AbortOnDropHandle::new(client.spawn_tokio());
        let _server_task = AbortOnDropHandle::new(server.spawn_tokio());
        let client_ip: Ipv4Addr = "192.0.2.1".parse().unwrap();
        let dns_ip: IpAddr = "203.0.113.53".parse().unwrap();
        client_channel.set_ips([client_ip.into()]).await.unwrap();
        server_channel.set_ips([dns_ip]).await.unwrap();
        let dns = server_channel
            .udp_bind(SocketAddr::new(dns_ip, 53))
            .await
            .unwrap();
        let _dns_task = AbortOnDropHandle::new(tokio::spawn(async move {
            for _ in 0..2 {
                let (from, query) = dns.recv_from_bytes().await.unwrap();
                let value = match &query[query.len() - 4..query.len() - 2] {
                    [0, 1] => "10.0.0.2".parse::<Ipv4Addr>().unwrap().octets().to_vec(),
                    [0, 28] => "fd00::2"
                        .parse::<std::net::Ipv6Addr>()
                        .unwrap()
                        .octets()
                        .to_vec(),
                    _ => panic!("unexpected DNS type"),
                };
                let mut reply = query.to_vec();
                reply[2..4].copy_from_slice(&[0x81, 0x80]);
                reply[6..8].copy_from_slice(&1_u16.to_be_bytes());
                reply.extend_from_slice(&[0xc0, 0x0c]);
                reply.extend_from_slice(&query[query.len() - 4..]);
                reply.extend_from_slice(&60_u32.to_be_bytes());
                reply.extend_from_slice(&(value.len() as u16).to_be_bytes());
                reply.extend(value);
                dns.send_to(from, &reply).await.unwrap();
            }
        }));
        let policy = Arc::new(super::lan_test_policy(true));
        let protector = Arc::new(super::LanProbeProtector::default());
        let resolver = crate::dns::Resolver::new(
            client_channel,
            client_ip,
            std::net::Ipv6Addr::UNSPECIFIED,
            vec![dns_ip],
            usque_core::ProxyDnsMode::Remote,
            protector.clone(),
        )
        .with_routing(policy.clone());
        let expected: Vec<IpAddr> = vec!["10.0.0.2".parse().unwrap(), "fd00::2".parse().unwrap()];
        let checked = expected.clone();
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            super::connect_routed(
                &policy,
                protector.as_ref(),
                Arc::default(),
                (GeoTarget::Host("nas.example"), 8080, Some(&resolver)),
                (|_| "dns_failed", || "rejected"),
                |addresses| async move {
                    assert_eq!(addresses, Some(checked));
                    Err("protected_fallback")
                },
            ),
        )
        .await
        .unwrap();
        assert!(matches!(result, Err("protected_fallback")));
        assert_eq!(
            *protector.attempts.lock().unwrap(),
            expected
                .into_iter()
                .map(|ip| (
                    SocketAddr::new(ip, 8080),
                    crate::socket::DirectProtocol::Tcp
                ))
                .collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn rejected_resolved_addresses_cannot_open_a_direct_socket_or_fall_back() {
        use ts_netstack_smoltcp::netcore::HasChannel;
        let (stack, _pipe) = crate::netstack::bounded_piped(Default::default());
        let resolver = crate::dns::Resolver::new(
            stack.command_channel(),
            Ipv4Addr::UNSPECIFIED,
            std::net::Ipv6Addr::UNSPECIFIED,
            vec![],
            usque_core::ProxyDnsMode::Remote,
            Arc::new(crate::socket::NoopSocketProtector),
        );
        let policy = super::routing_test_policy(&[
            ("direct.test", usque_core::RoutingAction::Direct),
            ("192.0.2.0/24", usque_core::RoutingAction::Reject),
        ]);
        for address in ["192.0.2.17:443", "[::ffff:192.0.2.17]:443"] {
            let protector = FakeProtector {
                resolved: address.parse().unwrap(),
                reject_protect: true,
                protect_calls: AtomicUsize::new(0),
                resolve_calls: AtomicUsize::new(0),
            };
            let fallback = AtomicBool::new(false);
            let result = super::connect_routed(
                &policy,
                &protector,
                Arc::default(),
                (GeoTarget::Host("direct.test"), 443, Some(&resolver)),
                (|_| "dns_failed", || "rejected"),
                |_| async {
                    fallback.store(true, Ordering::SeqCst);
                    Err("fallback")
                },
            )
            .await;
            assert!(matches!(result, Err("rejected")));
            assert!(!fallback.load(Ordering::SeqCst));
            assert_eq!(protector.protect_calls.load(Ordering::SeqCst), 0);
            assert_eq!(protector.resolve_calls.load(Ordering::SeqCst), 1);
        }
    }

    #[tokio::test]
    async fn rejected_targets_never_resolve_or_enter_fallback() {
        use usque_core::RoutingAction::{Direct, Proxy, Reject};
        let policy = super::routing_test_policy(&[
            ("example.test", Direct),
            ("ads.example.test", Reject),
            ("proxy.example.test", Proxy),
            ("192.0.2.0/24", Reject),
            ("192.0.2.7", Direct),
        ]);
        for target in [
            GeoTarget::Host("ads.example.test"),
            GeoTarget::Ip("192.0.2.8".parse().unwrap()),
        ] {
            let result = connect_with_geo_fallback(
                &policy,
                &crate::socket::NoopSocketProtector,
                target,
                443,
                |_| async { Err::<(), ()>(()) },
            )
            .await
            .unwrap();
            assert!(matches!(result, DirectFallback::Rejected));
        }
        assert_eq!(
            policy.resolved_route("example.test", "192.0.2.8".parse().unwrap()),
            GeoRoute::Reject
        );
        assert_eq!(
            policy.resolved_route("proxy.example.test", "192.0.2.7".parse().unwrap()),
            GeoRoute::Tunnel
        );
        assert_eq!(
            policy.route_ip("192.0.2.7".parse().unwrap()),
            GeoRoute::Direct
        );
        let reject_only = super::routing_test_policy(&[("ads.test", Reject)]);
        assert!(!reject_only.needs_direct_dns());
        assert!(!reject_only.has_direct_routes());
    }

    #[test]
    fn custom_targets_work_without_geo_and_match_label_and_network_boundaries() {
        let profile = usque_core::Profile {
            bypass_domains: vec!["Example.COM.".into(), "bücher.example".into()],
            split_exclusions: vec![
                "192.0.2.0/24".parse().unwrap(),
                "2001:db8::1/128".parse().unwrap(),
            ],
            ..Default::default()
        };
        let policy = GeoDirectPolicy::disabled()
            .with_custom_rules(&profile)
            .unwrap();
        assert!(policy.is_enabled());
        for host in [
            "example.com",
            "A.example.com.",
            "deep.a.example.com",
            "xn--bcher-kva.example",
            "bücher.example",
        ] {
            assert_eq!(policy.route_host(host), GeoRoute::Direct, "{host}");
        }
        for host in [
            "notexample.com",
            "example.com.evil",
            "unknown.test",
            "https://example.com",
        ] {
            assert_eq!(policy.route_host(host), GeoRoute::Tunnel, "{host}");
        }
        for ip in ["192.0.2.0", "192.0.2.255", "2001:db8::1"] {
            assert_eq!(policy.route_ip(ip.parse().unwrap()), GeoRoute::Direct);
        }
        for ip in ["192.0.3.0", "2001:db8::2"] {
            assert_eq!(policy.route_ip(ip.parse().unwrap()), GeoRoute::Tunnel);
        }
        let removed = GeoDirectPolicy::disabled()
            .with_custom_rules(&usque_core::Profile {
                allow_lan: false,
                ..Default::default()
            })
            .unwrap();
        assert!(!removed.is_enabled());
        assert_eq!(removed.route_host("example.com"), GeoRoute::Tunnel);
    }

    #[test]
    fn classifier_routes_hosts_via_geosite_ips_via_geoip_and_unknowns_to_tunnel() {
        let policy = policy(true, true);
        assert_eq!(policy.route_host("direct.test"), GeoRoute::Direct);
        assert_eq!(
            policy.route_ip(Ipv4Addr::new(203, 0, 113, 7).into()),
            GeoRoute::Direct
        );
        assert_eq!(policy.route_host("unknown.test"), GeoRoute::Tunnel);
        assert_eq!(
            policy.route_ip(Ipv4Addr::new(203, 0, 113, 8).into()),
            GeoRoute::Tunnel
        );
        assert_eq!(
            GeoDirectPolicy::disabled().route_host("direct.test"),
            GeoRoute::Tunnel
        );
    }

    #[tokio::test]
    async fn direct_hostname_uses_protected_resolver_and_loopback_socket() {
        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let address = listener.local_addr().unwrap();
        let protector = FakeProtector {
            resolved: address,
            reject_protect: false,
            protect_calls: AtomicUsize::new(0),
            resolve_calls: AtomicUsize::new(0),
        };
        let fallback_called = Arc::new(AtomicBool::new(false));
        let fallback_observed = Arc::clone(&fallback_called);
        let result: Result<_, ()> = connect_with_geo_fallback(
            &policy(true, false),
            &protector,
            GeoTarget::Host("direct.test"),
            address.port(),
            move |_| async move {
                fallback_observed.store(true, Ordering::SeqCst);
                Ok(())
            },
        )
        .await;
        assert!(matches!(
            result.unwrap(),
            super::DirectFallback::Direct(_, _)
        ));
        assert_eq!(protector.resolve_calls.load(Ordering::SeqCst), 1);
        assert_eq!(protector.protect_calls.load(Ordering::SeqCst), 1);
        assert!(!fallback_called.load(Ordering::SeqCst));
        let _ = listener.accept().await.unwrap();
    }

    #[tokio::test]
    async fn direct_failure_falls_back_without_opening_an_unprotected_socket() {
        let protector = FakeProtector {
            resolved: SocketAddr::from((Ipv4Addr::LOCALHOST, 443)),
            reject_protect: true,
            protect_calls: AtomicUsize::new(0),
            resolve_calls: AtomicUsize::new(0),
        };
        let result: Result<_, ()> = connect_with_geo_fallback(
            &policy(true, false),
            &protector,
            GeoTarget::Host("direct.test"),
            443,
            |_| async { Ok("tunnel") },
        )
        .await;
        assert!(matches!(
            result.unwrap(),
            super::DirectFallback::Fallback("tunnel")
        ));
        assert_eq!(protector.resolve_calls.load(Ordering::SeqCst), 1);
        assert_eq!(protector.protect_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn custom_hostname_uses_protected_resolver_and_loopback_socket() {
        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let address = listener.local_addr().unwrap();
        let protector = FakeProtector {
            resolved: address,
            reject_protect: false,
            protect_calls: AtomicUsize::new(0),
            resolve_calls: AtomicUsize::new(0),
        };
        let fallback_called = Arc::new(AtomicBool::new(false));
        let fallback_observed = Arc::clone(&fallback_called);
        let result: Result<_, ()> = connect_with_geo_fallback(
            &GeoDirectPolicy::disabled()
                .with_custom_rules(&usque_core::Profile {
                    bypass_domains: vec!["direct.test".into()],
                    ..Default::default()
                })
                .unwrap(),
            &protector,
            GeoTarget::Host("direct.test"),
            address.port(),
            move |_| async move {
                fallback_observed.store(true, Ordering::SeqCst);
                Ok(())
            },
        )
        .await;
        assert!(matches!(
            result.unwrap(),
            super::DirectFallback::Direct(_, _)
        ));
        assert_eq!(protector.resolve_calls.load(Ordering::SeqCst), 1);
        assert_eq!(protector.protect_calls.load(Ordering::SeqCst), 1);
        assert!(!fallback_called.load(Ordering::SeqCst));
        let _ = listener.accept().await.unwrap();
    }

    #[tokio::test]
    async fn custom_failure_does_not_open_an_unprotected_socket() {
        let protector = FakeProtector {
            resolved: SocketAddr::from((Ipv4Addr::LOCALHOST, 443)),
            reject_protect: true,
            protect_calls: AtomicUsize::new(0),
            resolve_calls: AtomicUsize::new(0),
        };
        let result: Result<_, ()> = connect_with_geo_fallback(
            &GeoDirectPolicy::disabled()
                .with_custom_rules(&usque_core::Profile {
                    bypass_domains: vec!["direct.test".into()],
                    ..Default::default()
                })
                .unwrap(),
            &protector,
            GeoTarget::Host("direct.test"),
            443,
            |_| async { Ok("tunnel") },
        )
        .await;
        assert!(matches!(
            result.unwrap(),
            super::DirectFallback::Fallback("tunnel")
        ));
        assert_eq!(protector.resolve_calls.load(Ordering::SeqCst), 1);
        assert_eq!(protector.protect_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn direct_tcp_stream_records_only_physical_payload_bytes() {
        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let client = tokio::net::TcpStream::connect(listener.local_addr().unwrap())
            .await
            .unwrap();
        let (mut server, _) = listener.accept().await.unwrap();
        let server_task = tokio::spawn(async move {
            let mut request = [0u8; 4];
            server.read_exact(&mut request).await.unwrap();
            assert_eq!(&request, b"ping");
            server.write_all(b"pong").await.unwrap();
        });
        let counters = Arc::new(crate::netstack::TrafficCounters::default());
        let mut stream = super::RoutedTcpStream::Direct {
            stream: client,
            counters: Arc::clone(&counters),
            _lease: crate::socket::DirectEgressLease::default(),
        };
        stream.write_all(b"ping").await.unwrap();
        let mut response = [0u8; 4];
        stream.read_exact(&mut response).await.unwrap();
        assert_eq!(&response, b"pong");
        server_task.await.unwrap();
        assert_eq!(
            counters.snapshot(),
            crate::netstack::TrafficSnapshot {
                bytes_sent: 4,
                bytes_received: 4,
            }
        );
    }
}
