use std::net::{SocketAddr, ToSocketAddrs};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use async_trait::async_trait;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

/// Stable reason returned when exact-generation socket setup races a network
/// change. Callers may retry only by taking a fresh generation snapshot.
pub const STALE_GENERATION_REASON: &str = "stale_generation";

/// Platform-neutral representation of a socket before it connects to a
/// MASQUE endpoint.
///
/// Android uses the numeric file descriptor with `VpnService.protect(fd)` so
/// the tunnel transport cannot route back into its own TUN interface. Windows
/// VPN sockets use the Agent's exact-egress lease; desktop proxy mode may use
/// the no-op implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SocketHandle(u64);

impl SocketHandle {
    pub const fn value(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectProtocol {
    Tcp,
    Udp,
}

impl DirectProtocol {
    pub const fn iana_number(self) -> u8 {
        match self {
            Self::Tcp => 6,
            Self::Udp => 17,
        }
    }
}

/// Keeps a platform-specific exact-egress authorization alive for the socket
/// or flow. Dropping it releases the authorization.
#[derive(Default)]
pub struct DirectEgressLease {
    _resource: Option<Box<dyn Send + Sync>>,
    generation: Option<u64>,
}

impl DirectEgressLease {
    pub fn hold(resource: impl Send + Sync + 'static) -> Self {
        Self {
            _resource: Some(Box::new(resource)),
            generation: None,
        }
    }

    pub fn for_generation(generation: u64) -> Self {
        Self {
            _resource: None,
            generation: Some(generation),
        }
    }

    pub fn hold_for_generation(resource: impl Send + Sync + 'static, generation: u64) -> Self {
        Self {
            _resource: Some(Box::new(resource)),
            generation: Some(generation),
        }
    }

    pub const fn generation(&self) -> Option<u64> {
        self.generation
    }

    fn with_generation(mut self, generation: u64) -> Self {
        self.generation = Some(generation);
        self
    }
}

impl std::fmt::Debug for DirectEgressLease {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DirectEgressLease")
            .field("active", &self._resource.is_some())
            .field("generation", &self.generation)
            .finish()
    }
}

/// Keeps exact-egress authorization attached to a stream through TLS and
/// protocol-driver ownership. Field order closes the I/O before the lease.
pub(crate) struct LeasedIo<T> {
    inner: T,
    _egress_lease: DirectEgressLease,
}

impl<T> LeasedIo<T> {
    pub(crate) fn new(inner: T, egress_lease: DirectEgressLease) -> Self {
        Self {
            inner,
            _egress_lease: egress_lease,
        }
    }
}

impl<T: AsyncRead + Unpin> AsyncRead for LeasedIo<T> {
    fn poll_read(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_read(context, buffer)
    }
}

impl<T: AsyncWrite + Unpin> AsyncWrite for LeasedIo<T> {
    fn poll_write(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.get_mut().inner).poll_write(context, bytes)
    }

    fn poll_flush(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_flush(context)
    }

    fn poll_shutdown(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_shutdown(context)
    }

    fn is_write_vectored(&self) -> bool {
        self.inner.is_write_vectored()
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffers: &[std::io::IoSlice<'_>],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.get_mut().inner).poll_write_vectored(context, buffers)
    }
}

/// Called immediately after a socket is created and before any endpoint
/// connection or packet is attempted.
#[async_trait]
pub trait SocketProtector: Send + Sync {
    fn protect(&self, socket: SocketHandle) -> Result<(), String>;

    /// Protects and, where required, binds a socket for one exact physical
    /// destination. The returned lease must outlive all socket I/O.
    async fn protect_for_target(
        &self,
        socket: SocketHandle,
        _remote: SocketAddr,
        _protocol: DirectProtocol,
    ) -> Result<DirectEgressLease, String> {
        self.protect(socket)?;
        Ok(DirectEgressLease::default())
    }

    /// Protects one exact target on the caller-selected physical-network
    /// generation. Implementations must never silently bind to a newer
    /// generation. The default wraps the existing target-aware contract with
    /// before/after checks and tags the returned lease with the exact value.
    async fn protect_for_target_generation(
        &self,
        socket: SocketHandle,
        remote: SocketAddr,
        protocol: DirectProtocol,
        expected_generation: u64,
    ) -> Result<DirectEgressLease, String> {
        let before = self.network_generation().unwrap_or_default();
        if before != expected_generation {
            return Err(STALE_GENERATION_REASON.to_owned());
        }
        let lease = self.protect_for_target(socket, remote, protocol).await?;
        let after = self.network_generation().unwrap_or_default();
        if after != expected_generation {
            return Err(STALE_GENERATION_REASON.to_owned());
        }
        Ok(lease.with_generation(expected_generation))
    }

    /// Resolves a GeoSite-selected host using the platform's selected physical
    /// DNS path. Implementations must not fall back to the tunnel resolver.
    async fn resolve_direct(&self, host: &str, port: u16) -> Result<Vec<SocketAddr>, String> {
        self.resolve(host, port)
    }

    /// Whether protected sockets may intentionally carry TUN-selected direct
    /// flows without re-entering the VPN or weakening a platform kill switch.
    fn tun_direct_available(&self) -> bool {
        false
    }

    /// Returns whether the platform's selected physical path can currently
    /// carry this endpoint address family. `None` means the platform has not
    /// supplied authoritative link properties yet.
    fn endpoint_family_available(&self, _endpoint: SocketAddr) -> Option<bool> {
        None
    }

    /// Monotonically increasing generation for the selected physical network.
    /// H3 first validates a same-family candidate in the existing connection;
    /// unsupported or failed migration and H2 use complete reconnect without
    /// tearing down local proxy listeners.
    fn network_generation(&self) -> Option<u64> {
        None
    }

    /// Numeric DNS servers reported by the selected non-VPN physical network.
    /// Implementations must not return the DNS addresses configured on the TUN
    /// interface itself.
    fn physical_dns_servers(&self) -> Vec<SocketAddr> {
        Vec::new()
    }

    /// Resolves a control-plane host on the same physical network used by
    /// protected endpoint sockets.
    ///
    /// Android overrides this with `Network.getAllByName` so resolution cannot
    /// recurse through its own TUN. Desktop proxy mode uses the system
    /// resolver. The returned addresses are still authenticated by TLS.
    fn resolve(&self, host: &str, port: u16) -> Result<Vec<SocketAddr>, String> {
        let mut addresses = (host, port)
            .to_socket_addrs()
            .map_err(|error| format!("resolve {host}: {error}"))?
            .filter(|address| !address.ip().is_unspecified() && !address.ip().is_multicast())
            .collect::<Vec<_>>();
        addresses.sort();
        addresses.dedup();
        addresses.truncate(16);
        if addresses.is_empty() {
            return Err(format!("resolve {host}: no usable address"));
        }
        Ok(addresses)
    }
}

#[derive(Debug, Default)]
pub struct NoopSocketProtector;

impl SocketProtector for NoopSocketProtector {
    fn protect(&self, _socket: SocketHandle) -> Result<(), String> {
        Ok(())
    }
}

pub(crate) fn noop_socket_protector() -> Arc<dyn SocketProtector> {
    Arc::new(NoopSocketProtector)
}

#[cfg(unix)]
pub(crate) fn socket_handle<T: std::os::fd::AsRawFd>(socket: &T) -> SocketHandle {
    SocketHandle(socket.as_raw_fd() as u64)
}

#[cfg(windows)]
pub(crate) fn socket_handle<T: std::os::windows::io::AsRawSocket>(socket: &T) -> SocketHandle {
    SocketHandle(socket.as_raw_socket())
}

/// Bind local proxy listeners. IPv4 addresses are bound first.
///
/// IPv6 sockets are forced to V6-only before bind. Windows dual-stack sockets
/// otherwise occupy the matching IPv4 port, so `127.0.0.1:8080` fails with
/// WSAEADDRINUSE when `[::1]:8080` is already bound.
pub(crate) fn bind_tcp_listeners(
    addresses: &[SocketAddr],
) -> Result<Vec<tokio::net::TcpListener>, (SocketAddr, std::io::Error)> {
    let mut ordered = addresses.to_vec();
    ordered.sort_by_key(SocketAddr::is_ipv6);
    let mut bound = Vec::with_capacity(ordered.len());
    for address in ordered {
        match bind_tcp_listener(address) {
            Ok(listener) => bound.push(listener),
            Err(source) => return Err((address, source)),
        }
    }
    Ok(bound)
}

pub(crate) fn bind_tcp_listener(address: SocketAddr) -> std::io::Result<tokio::net::TcpListener> {
    use socket2::{Domain, Protocol, Socket, Type};

    let domain = if address.is_ipv4() {
        Domain::IPV4
    } else {
        Domain::IPV6
    };
    let socket = Socket::new(domain, Type::STREAM, Some(Protocol::TCP))?;
    if address.is_ipv6() {
        socket.set_only_v6(true)?;
    }
    socket.set_nonblocking(true)?;
    socket.bind(&address.into())?;
    socket.listen(256)?;
    tokio::net::TcpListener::from_std(socket.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

    struct LeaseDropFlag(Arc<AtomicBool>);

    impl Drop for LeaseDropFlag {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
        }
    }

    #[test]
    fn leased_io_closes_stream_before_releasing_authorization() {
        struct DropOrder(&'static str, Arc<std::sync::Mutex<Vec<&'static str>>>);

        impl Drop for DropOrder {
            fn drop(&mut self) {
                self.1.lock().unwrap().push(self.0);
            }
        }

        let order = Arc::new(std::sync::Mutex::new(Vec::new()));
        let stream = LeasedIo::new(
            DropOrder("socket", Arc::clone(&order)),
            DirectEgressLease::hold(DropOrder("lease", Arc::clone(&order))),
        );
        drop(stream);
        assert_eq!(*order.lock().unwrap(), ["socket", "lease"]);
    }

    #[tokio::test]
    async fn leased_io_forwards_duplex_io_and_retains_lease_until_drop() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let (client, mut peer) = tokio::io::duplex(64);
        let dropped = Arc::new(AtomicBool::new(false));
        let mut client = LeasedIo::new(
            client,
            DirectEgressLease::hold(LeaseDropFlag(Arc::clone(&dropped))),
        );
        assert!(client.is_write_vectored());
        assert_eq!(
            client
                .write_vectored(&[std::io::IoSlice::new(b"qu"), std::io::IoSlice::new(b"ery")])
                .await
                .unwrap(),
            5
        );
        client.flush().await.unwrap();
        let mut received = [0; 5];
        peer.read_exact(&mut received).await.unwrap();
        assert_eq!(&received, b"query");
        peer.write_all(b"reply").await.unwrap();
        client.read_exact(&mut received).await.unwrap();
        assert_eq!(&received, b"reply");
        client.shutdown().await.unwrap();
        assert_eq!(peer.read(&mut received).await.unwrap(), 0);
        assert!(!dropped.load(Ordering::Acquire));
        drop(client);
        assert!(dropped.load(Ordering::Acquire));
    }

    struct GenerationProtector {
        generation: AtomicU64,
        advance_during_protect: bool,
        lease_dropped: Arc<AtomicBool>,
    }

    #[async_trait]
    impl SocketProtector for GenerationProtector {
        fn protect(&self, _socket: SocketHandle) -> Result<(), String> {
            Ok(())
        }

        async fn protect_for_target(
            &self,
            _socket: SocketHandle,
            _remote: SocketAddr,
            _protocol: DirectProtocol,
        ) -> Result<DirectEgressLease, String> {
            let lease = DirectEgressLease::hold(LeaseDropFlag(Arc::clone(&self.lease_dropped)));
            if self.advance_during_protect {
                self.generation.fetch_add(1, Ordering::AcqRel);
            }
            Ok(lease)
        }

        fn network_generation(&self) -> Option<u64> {
            Some(self.generation.load(Ordering::Acquire))
        }
    }

    #[tokio::test]
    async fn ipv4_and_ipv6_loopback_can_share_a_port() {
        let v4 = bind_tcp_listener(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
            .expect("bind IPv4 loopback");
        let port = v4.local_addr().expect("IPv4 local addr").port();
        let v6 = bind_tcp_listener(SocketAddr::new(Ipv6Addr::LOCALHOST.into(), port));
        let Ok(v6) = v6 else {
            // Some CI images have IPv6 loopback disabled.
            return;
        };
        assert_eq!(v6.local_addr().expect("IPv6 local addr").port(), port);
    }

    #[tokio::test]
    async fn ipv6_loopback_first_does_not_steal_ipv4_loopback() {
        let v6 = bind_tcp_listener(SocketAddr::new(Ipv6Addr::LOCALHOST.into(), 0));
        let Ok(v6) = v6 else {
            return;
        };
        let port = v6.local_addr().expect("IPv6 local addr").port();
        bind_tcp_listener(SocketAddr::from((Ipv4Addr::LOCALHOST, port)))
            .expect("IPv4 loopback must bind after V6-only IPv6 on the same port");
    }

    #[tokio::test]
    async fn expected_generation_lease_is_tagged_and_stale_race_releases_it() {
        let target: SocketAddr = "192.0.2.1:443".parse().unwrap();
        let stable = GenerationProtector {
            generation: AtomicU64::new(7),
            advance_during_protect: false,
            lease_dropped: Arc::new(AtomicBool::new(false)),
        };
        let lease = stable
            .protect_for_target_generation(SocketHandle(1), target, DirectProtocol::Udp, 7)
            .await
            .unwrap();
        assert_eq!(lease.generation(), Some(7));

        let dropped = Arc::new(AtomicBool::new(false));
        let racing = GenerationProtector {
            generation: AtomicU64::new(9),
            advance_during_protect: true,
            lease_dropped: Arc::clone(&dropped),
        };
        assert_eq!(
            racing
                .protect_for_target_generation(SocketHandle(2), target, DirectProtocol::Udp, 9)
                .await
                .unwrap_err(),
            STALE_GENERATION_REASON
        );
        assert!(dropped.load(Ordering::Acquire));
    }
}
