use std::future::Future;
use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket as StdUdpSocket};
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::{TcpSocket, TcpStream, UdpSocket};
use tokio::time::timeout;
use ts_netstack_smoltcp::netsock::TcpStream as StackTcpStream;
use usque_geo::{CountryCode, GeoClassifier, GeoError};

use crate::netstack::TrafficCounters;
use crate::socket::{SocketProtector, socket_handle};

const DIRECT_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_DIRECT_ADDRESSES: usize = 16;

/// The route selected by [`GeoDirectPolicy`] for one proxy destination.
///
/// `Tunnel` is deliberately the default: absent, incomplete, or non-matching
/// Geo data can never cause traffic to bypass MASQUE.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GeoRoute {
    Tunnel,
    Direct,
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

/// Immutable GEO split-routing policy for proxy and platform traffic.
///
/// A hostname is evaluated only with GeoSite, while an IP literal is evaluated
/// only with GeoIP. A missing classifier, an empty country list, and every
/// unknown result route through the tunnel (fail closed).
#[derive(Clone)]
pub struct GeoDirectPolicy {
    classifier: Option<Arc<dyn GeoDirectClassifier>>,
    countries: Vec<CountryCode>,
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
            countries: Vec::new(),
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
        }
    }

    /// Returns the configured country codes in their caller-provided order.
    pub fn countries(&self) -> &[CountryCode] {
        &self.countries
    }

    /// Returns whether this policy can select a direct route.
    pub fn is_enabled(&self) -> bool {
        self.classifier.is_some() && !self.countries.is_empty()
    }

    /// Selects a route for a hostname using GeoSite only.
    pub fn route_host(&self, host: &str) -> GeoRoute {
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

    /// Selects a route for an IP literal using GeoIP only.
    pub fn route_ip(&self, ip: IpAddr) -> GeoRoute {
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

    fn label(self) -> String {
        match self {
            Self::Host(host) => host.to_owned(),
            Self::Ip(ip) => ip.to_string(),
        }
    }
}

/// A TCP stream connected either through the userspace tunnel or directly on
/// the protected physical network.
pub(crate) enum RoutedTcpStream {
    Tunnel(StackTcpStream),
    Direct {
        stream: TcpStream,
        counters: Arc<TrafficCounters>,
    },
}

impl RoutedTcpStream {
    pub(crate) fn local_addr(&self) -> io::Result<SocketAddr> {
        match self {
            Self::Tunnel(stream) => Ok(stream.local_addr()),
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
            Self::Direct { stream, counters } => {
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
            Self::Direct { stream, counters } => {
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
            Self::Direct { stream, counters } => {
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
    Direct(TcpStream),
    Fallback(T),
}

/// Attempts the selected direct path first, then invokes `fallback` for every
/// direct error. The fallback is intentionally not attempted for an unknown
/// destination until the policy has chosen the tunnel route.
async fn connect_with_geo_fallback<T, E, F, Fut>(
    policy: &GeoDirectPolicy,
    protector: &dyn SocketProtector,
    target: GeoTarget<'_>,
    port: u16,
    fallback: F,
) -> Result<DirectFallback<T>, E>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<T, E>>,
{
    if target.route(policy) == GeoRoute::Direct {
        match connect_direct(protector, target, port).await {
            Ok(stream) => return Ok(DirectFallback::Direct(stream)),
            Err(error) => {
                let target = target.label();
                tracing::debug!(%target, %error, "GEO direct TCP connect failed; falling back to tunnel");
            }
        }
    }
    fallback().await.map(DirectFallback::Fallback)
}

pub(crate) async fn connect_routed<E, F, Fut>(
    policy: &GeoDirectPolicy,
    protector: &dyn SocketProtector,
    counters: Arc<TrafficCounters>,
    target: GeoTarget<'_>,
    port: u16,
    tunnel: F,
) -> Result<RoutedTcpStream, E>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<StackTcpStream, E>>,
{
    connect_with_geo_fallback(policy, protector, target, port, tunnel)
        .await
        .map(|stream| match stream {
            DirectFallback::Direct(stream) => RoutedTcpStream::Direct { stream, counters },
            DirectFallback::Fallback(stream) => RoutedTcpStream::Tunnel(stream),
        })
}

async fn connect_direct(
    protector: &dyn SocketProtector,
    target: GeoTarget<'_>,
    port: u16,
) -> Result<TcpStream, String> {
    let addresses = match target {
        GeoTarget::Host(host) => protector.resolve(host, port)?,
        GeoTarget::Ip(ip) => vec![SocketAddr::new(ip, port)],
    };
    let mut failures = Vec::new();
    for address in addresses.into_iter().take(MAX_DIRECT_ADDRESSES) {
        let remote = SocketAddr::new(address.ip(), port);
        if remote.ip().is_unspecified() || remote.ip().is_multicast() {
            failures.push(format!("{remote}: unusable address"));
            continue;
        }
        match connect_direct_address(protector, remote).await {
            Ok(stream) => return Ok(stream),
            Err(error) => failures.push(format!("{remote}: {error}")),
        }
    }
    Err(if failures.is_empty() {
        "no usable direct target address".to_owned()
    } else {
        failures.join("; ")
    })
}

async fn connect_direct_address(
    protector: &dyn SocketProtector,
    remote: SocketAddr,
) -> Result<TcpStream, String> {
    let socket = if remote.is_ipv4() {
        TcpSocket::new_v4()
    } else {
        TcpSocket::new_v6()
    }
    .map_err(|error| error.to_string())?;
    protector
        .protect(socket_handle(&socket))
        .map_err(|error| format!("protect direct socket: {error}"))?;
    let stream = timeout(DIRECT_CONNECT_TIMEOUT, socket.connect(remote))
        .await
        .map_err(|_| format!("connect to {remote} timed out"))?
        .map_err(|error| error.to_string())?;
    stream
        .set_nodelay(true)
        .map_err(|error| error.to_string())?;
    Ok(stream)
}

pub(crate) async fn connect_direct_ip(
    protector: &dyn SocketProtector,
    remote: SocketAddr,
) -> Result<TcpStream, String> {
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

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::{
        CountryCode, GeoDirectClassifier, GeoDirectPolicy, GeoRoute, GeoTarget,
        connect_with_geo_fallback,
    };
    use crate::socket::{SocketHandle, SocketProtector};

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
            move || async move {
                fallback_observed.store(true, Ordering::SeqCst);
                Ok(())
            },
        )
        .await;
        assert!(matches!(result.unwrap(), super::DirectFallback::Direct(_)));
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
            || async { Ok("tunnel") },
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
