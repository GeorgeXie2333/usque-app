use std::net::{SocketAddr, ToSocketAddrs};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use async_trait::async_trait;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::sync::watch;

/// A scheduling hint, never an authorization to bypass socket protection.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PhysicalNetworkAvailability {
    #[default]
    Unknown,
    Offline,
    Online {
        ipv4: bool,
        ipv6: bool,
    },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PhysicalNetworkSnapshot {
    pub generation: u64,
    pub availability: PhysicalNetworkAvailability,
}

impl PhysicalNetworkSnapshot {
    /// None means lack of knowledge, not absence of a usable path.
    pub fn usable_for(self, policy: usque_core::IpPolicy) -> Option<bool> {
        match self.availability {
            PhysicalNetworkAvailability::Unknown => None,
            PhysicalNetworkAvailability::Offline => Some(false),
            PhysicalNetworkAvailability::Online { ipv4, ipv6 } => Some(match policy {
                usque_core::IpPolicy::Ipv4Only => ipv4,
                usque_core::IpPolicy::Ipv6Only => ipv6,
                _ => ipv4 || ipv6,
            }),
        }
    }
}

/// Stable reason returned when exact-generation socket setup races a network
/// change. Callers may retry only by taking a fresh generation snapshot.
pub const STALE_GENERATION_REASON: &str = "stale_generation";
/// An authorized physical connect failed and its unpublished permit was revoked.
pub const ENDPOINT_UNREACHABLE_REASON: &str = "physical_endpoint_unreachable";

#[derive(Debug, thiserror::Error)]
pub(crate) enum SocketPrepareError {
    #[error("stale_generation")]
    StaleGeneration,
    #[error("socket preparation failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("socket protection failed: {0}")]
    Protection(String),
}

impl From<String> for SocketPrepareError {
    fn from(reason: String) -> Self {
        match reason.as_str() {
            STALE_GENERATION_REASON => Self::StaleGeneration,
            ENDPOINT_UNREACHABLE_REASON => {
                Self::Io(std::io::Error::other("physical endpoint connect failed"))
            }
            _ => Self::Protection(reason),
        }
    }
}

impl SocketPrepareError {
    pub(crate) fn into_transport_error(self) -> crate::TransportError {
        match self {
            Self::StaleGeneration => crate::TransportError::UnderlyingNetworkChanged,
            Self::Io(error) => crate::TransportError::Io(error),
            Self::Protection(error) => crate::TransportError::SocketProtection(error),
        }
    }
}

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

/// The Agent authorizes each purpose independently, including during startup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhysicalSocketPurpose {
    Masque,
    BootstrapDns,
    DirectDns,
    Direct,
    Control,
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

/// A zero scope in a link-local packet is resolved by the Agent's verified
/// interface binding. Explicit caller scopes must still match exactly.
pub(crate) fn connected_peer_matches(peer: SocketAddr, expected: SocketAddr) -> bool {
    if peer == expected {
        return true;
    }
    #[cfg(target_os = "macos")]
    if matches!((peer, expected), (SocketAddr::V6(a), SocketAddr::V6(b))
        if b.ip().is_unicast_link_local() && b.scope_id() == 0
            && a.ip() == b.ip() && a.port() == b.port())
    {
        return true;
    }
    false
}

/// Darwin rejects sendto(destination) on a connected UDP socket. Agent-owned
/// sockets are connected before handoff, so keep the exact peer and use send.
pub(crate) async fn send_udp_to(
    socket: &tokio::net::UdpSocket,
    bytes: &[u8],
    remote: SocketAddr,
) -> std::io::Result<usize> {
    #[cfg(target_os = "macos")]
    if let Ok(peer) = socket.peer_addr() {
        if !connected_peer_matches(peer, remote) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "physical UDP peer mismatch",
            ));
        }
        return socket.send(bytes).await;
    }
    socket.send_to(bytes, remote).await
}

/// A platform may initiate a root-owned connection before descriptor handoff.
/// Waiting for it must not call connect a second time (EISCONN/EALREADY).
pub enum PreparedTcpSocket {
    Unconnected(tokio::net::TcpSocket),
    Connecting {
        stream: tokio::net::TcpStream,
        remote: SocketAddr,
    },
}
impl PreparedTcpSocket {
    pub async fn connect(self, remote: SocketAddr) -> std::io::Result<tokio::net::TcpStream> {
        match self {
            Self::Unconnected(socket) => socket.connect(remote).await,
            Self::Connecting {
                stream,
                remote: expected,
            } => {
                if remote != expected {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        "physical TCP destination changed",
                    ));
                }
                stream.writable().await?;
                if let Some(error) = stream.take_error()? {
                    return Err(error);
                }
                let peer = stream.peer_addr()?;
                #[cfg(target_os = "macos")]
                let matches = connected_peer_matches(peer, remote);
                #[cfg(not(target_os = "macos"))]
                let matches = peer == remote;
                if !matches {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        "physical TCP peer mismatch",
                    ));
                }
                Ok(stream)
            }
        }
    }
}

/// Called immediately after a socket is created and before any endpoint
/// connection or packet is attempted.
#[async_trait]
pub trait SocketProtector: Send + Sync {
    fn protect(&self, socket: SocketHandle) -> Result<(), String>;

    /// Platforms whose firewall attributes ownership at creation must supply
    /// the socket itself. Callers must never fall back to a user-created FD.
    fn requires_owned_sockets(&self) -> bool {
        false
    }

    async fn prepare_tcp(
        &self,
        remote: SocketAddr,
        purpose: PhysicalSocketPurpose,
        generation: u64,
    ) -> Result<(PreparedTcpSocket, DirectEgressLease), String> {
        let socket = if remote.is_ipv4() {
            tokio::net::TcpSocket::new_v4()
        } else {
            tokio::net::TcpSocket::new_v6()
        }
        .map_err(|e| e.to_string())?;
        let lease = self
            .authorize_created_socket(
                socket_handle(&socket),
                remote,
                DirectProtocol::Tcp,
                purpose,
                generation,
            )
            .await?;
        Ok((PreparedTcpSocket::Unconnected(socket), lease))
    }

    async fn prepare_udp(
        &self,
        remote: SocketAddr,
        purpose: PhysicalSocketPurpose,
        generation: u64,
    ) -> Result<(std::net::UdpSocket, DirectEgressLease), String> {
        let socket = std::net::UdpSocket::bind(if remote.is_ipv4() {
            "0.0.0.0:0"
        } else {
            "[::]:0"
        })
        .map_err(|e| e.to_string())?;
        socket.set_nonblocking(true).map_err(|e| e.to_string())?;
        let lease = self
            .authorize_created_socket(
                socket_handle(&socket),
                remote,
                DirectProtocol::Udp,
                purpose,
                generation,
            )
            .await?;
        Ok((socket, lease))
    }

    async fn authorize_created_socket(
        &self,
        socket: SocketHandle,
        remote: SocketAddr,
        protocol: DirectProtocol,
        purpose: PhysicalSocketPurpose,
        generation: u64,
    ) -> Result<DirectEgressLease, String> {
        if self.requires_owned_sockets() {
            return Err("platform requires an Agent-created socket".into());
        }
        match purpose {
            PhysicalSocketPurpose::Masque => {
                self.protect_masque_endpoint_generation(socket, remote, protocol, generation)
                    .await
            }
            PhysicalSocketPurpose::BootstrapDns => {
                self.protect_endpoint_preset_dns_generation(socket, remote, protocol, generation)
                    .await
            }
            PhysicalSocketPurpose::Direct => {
                if self.network_generation().unwrap_or_default() != generation {
                    return Err(STALE_GENERATION_REASON.into());
                }
                let lease = self.protect_for_target(socket, remote, protocol).await?;
                if self.network_generation().unwrap_or_default() != generation {
                    return Err(STALE_GENERATION_REASON.into());
                }
                Ok(lease.with_generation(generation))
            }
            PhysicalSocketPurpose::DirectDns | PhysicalSocketPurpose::Control => {
                self.protect_for_target_generation(socket, remote, protocol, generation)
                    .await
            }
        }
    }

    /// Latest-value observations for reconnect scheduling. Unsupported platforms
    /// retain timed retries. Closing a subscription means Unknown, not Offline.
    fn subscribe_physical_network(&self) -> Option<watch::Receiver<PhysicalNetworkSnapshot>> {
        None
    }

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

    /// Protects a MASQUE ingress separately from ordinary direct egress. An
    /// automatic endpoint policy must be enforced in both bootstrap and active
    /// phases, without falling through to generic direct-target authorization.
    async fn protect_masque_endpoint_generation(
        &self,
        socket: SocketHandle,
        remote: SocketAddr,
        protocol: DirectProtocol,
        expected_generation: u64,
    ) -> Result<DirectEgressLease, String> {
        self.protect_for_target_generation(socket, remote, protocol, expected_generation)
            .await
    }

    /// Bootstrap DNS for an allowlisted built-in endpoint. Windows authorizes
    /// this purpose separately while the VPN is still being prepared.
    async fn protect_endpoint_preset_dns_generation(
        &self,
        socket: SocketHandle,
        remote: SocketAddr,
        protocol: DirectProtocol,
        expected_generation: u64,
    ) -> Result<DirectEgressLease, String> {
        self.protect_for_target_generation(socket, remote, protocol, expected_generation)
            .await
    }

    /// Resolves only AAAA records on the selected physical network. Configured
    /// application DNS and the tunnel resolver must not service this lookup.
    async fn resolve_endpoint_preset(
        &self,
        host: &str,
        port: u16,
        expected_generation: u64,
    ) -> Result<Vec<SocketAddr>, String> {
        crate::split_dns::resolve_endpoint_preset_aaaa(self, host, port, expected_generation).await
    }

    /// Publishes a finite candidate set before any MASQUE socket is opened.
    /// Empty sets revoke old candidates while retaining the saved IPv4 fallback.
    async fn authorize_endpoint_preset_candidates(
        &self,
        _host: &str,
        _candidates: &[SocketAddr],
        expected_generation: u64,
    ) -> Result<(), String> {
        if self.network_generation().unwrap_or_default() != expected_generation {
            return Err(STALE_GENERATION_REASON.to_owned());
        }
        Ok(())
    }

    /// Resolves a GeoSite-selected host using the platform's selected physical
    /// DNS path. Implementations must not fall back to the tunnel resolver.
    async fn resolve_direct(&self, host: &str, port: u16) -> Result<Vec<SocketAddr>, String> {
        self.resolve(host, port)
    }

    /// The runtime-owned direct DNS policy, if explicitly configured. None
    /// preserves the platform's existing physical-system behavior.
    fn direct_dns_resolver(&self) -> Option<Arc<crate::encrypted_dns::DirectDnsResolver>> {
        None
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

    /// Resolves a subscription endpoint hostname on the active physical network
    /// for a specific generation, honoring `ip_policy`.
    async fn resolve_endpoint_hostname(
        &self,
        host: &str,
        port: u16,
        ip_policy: usque_core::IpPolicy,
        expected_generation: u64,
    ) -> Result<Vec<SocketAddr>, String> {
        crate::endpoint_subscription::resolve_physical_endpoint_hostname(
            self,
            host,
            port,
            ip_policy,
            expected_generation,
        )
        .await
    }

    /// Authorizes dynamic IP candidates for an active subscription hostname in
    /// the platform firewall / tunnel plan on `expected_generation`.
    async fn authorize_endpoint_hostname_candidates(
        &self,
        _host: &str,
        _candidates: &[SocketAddr],
        expected_generation: u64,
    ) -> Result<(), String> {
        if self.network_generation().unwrap_or_default() != expected_generation {
            return Err(STALE_GENERATION_REASON.to_owned());
        }
        Ok(())
    }

    /// Acquires a bounded volatile firewall permit lease for probing MASQUE
    /// targets while TUN/Kill Switch is connected.
    async fn acquire_endpoint_probe_lease(
        &self,
        _probe_session_id: &str,
        _targets: &[ProbeLeaseTarget],
        _dns_hostname: Option<&str>,
        expected_generation: u64,
    ) -> Result<EndpointProbeLeaseHandle, String> {
        if self.requires_owned_sockets() {
            return Err("AGENT_CAPABILITY_REQUIRED:connected_endpoint_probe_leases".to_owned());
        }
        if self.network_generation().unwrap_or_default() != expected_generation {
            return Err(STALE_GENERATION_REASON.to_owned());
        }
        Ok(EndpointProbeLeaseHandle {
            lease_id: String::new(),
            expires_at_unix_ms: 0,
        })
    }

    /// Releases a previously acquired endpoint probe lease.
    async fn release_endpoint_probe_lease(
        &self,
        _probe_session_id: &str,
        _lease_id: &str,
    ) -> Result<(), String> {
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeTransportProtocol {
    Udp,
    Tcp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProbeLeaseTarget {
    pub addr: SocketAddr,
    pub protocol: ProbeTransportProtocol,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndpointProbeLeaseHandle {
    pub lease_id: String,
    pub expires_at_unix_ms: u64,
}

#[derive(Debug, Default)]
pub struct NoopSocketProtector;

#[async_trait]
impl SocketProtector for NoopSocketProtector {
    fn protect(&self, _socket: SocketHandle) -> Result<(), String> {
        Ok(())
    }

    async fn resolve_endpoint_preset(
        &self,
        host: &str,
        port: u16,
        _expected_generation: u64,
    ) -> Result<Vec<SocketAddr>, String> {
        crate::endpoint_preset::resolve_system_ipv6(host, port).await
    }

    async fn resolve_endpoint_hostname(
        &self,
        host: &str,
        port: u16,
        ip_policy: usque_core::IpPolicy,
        _expected_generation: u64,
    ) -> Result<Vec<SocketAddr>, String> {
        crate::endpoint_subscription::resolve_system_endpoint_hostname(host, port, ip_policy).await
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
    // Unix needs this on both the old and new listener to rebind after an
    // accepted connection enters TIME_WAIT during frontend reconfiguration.
    // Do not enable it on Windows, where it allows competing live binds.
    #[cfg(unix)]
    socket.set_reuse_address(true)?;
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

    #[tokio::test]
    async fn owned_socket_platform_never_falls_back_to_borrowed_fd_protection() {
        struct OwnedOnly;
        #[async_trait]
        impl SocketProtector for OwnedOnly {
            fn protect(&self, _: SocketHandle) -> Result<(), String> {
                panic!("borrowed protection bypass")
            }
            fn requires_owned_sockets(&self) -> bool {
                true
            }
        }
        for purpose in [
            PhysicalSocketPurpose::Masque,
            PhysicalSocketPurpose::BootstrapDns,
            PhysicalSocketPurpose::Direct,
            PhysicalSocketPurpose::Control,
        ] {
            assert!(
                OwnedOnly
                    .prepare_tcp("127.0.0.1:443".parse().unwrap(), purpose, 0)
                    .await
                    .is_err()
            );
            assert!(
                OwnedOnly
                    .prepare_udp("127.0.0.1:53".parse().unwrap(), purpose, 0)
                    .await
                    .is_err()
            );
        }
    }

    #[tokio::test]
    async fn socket_factory_rejects_a_generation_change_during_authorization() {
        let protector = GenerationProtector {
            generation: AtomicU64::new(7),
            advance_during_protect: true,
            lease_dropped: Arc::new(AtomicBool::new(false)),
        };
        let result = protector
            .prepare_udp(
                "127.0.0.1:53".parse().unwrap(),
                PhysicalSocketPurpose::Direct,
                7,
            )
            .await;
        assert_eq!(result.unwrap_err(), STALE_GENERATION_REASON);
        assert!(protector.lease_dropped.load(Ordering::Acquire));
    }

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
    async fn tcp_listener_rejects_a_competing_live_bind() {
        let listener = bind_tcp_listener(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).unwrap();
        let address = listener.local_addr().unwrap();
        assert_eq!(
            bind_tcp_listener(address).unwrap_err().kind(),
            std::io::ErrorKind::AddrInUse
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn tcp_listener_rebinds_after_server_initiated_close() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            let listener = bind_tcp_listener(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).unwrap();
            let address = listener.local_addr().unwrap();
            let mut client = tokio::net::TcpStream::connect(address).await.unwrap();
            let (mut accepted, _) = listener.accept().await.unwrap();
            // Force the server to send the first FIN, so its local port is
            // held in TIME_WAIT after the client's FIN is acknowledged.
            accepted.shutdown().await.unwrap();
            assert_eq!(client.read(&mut [0]).await.unwrap(), 0);
            client.shutdown().await.unwrap();
            assert_eq!(accepted.read(&mut [0]).await.unwrap(), 0);
            drop(accepted);
            drop(client);
            drop(listener);

            let replacement = bind_tcp_listener(address).expect("rebind despite TIME_WAIT");
            let _client = tokio::net::TcpStream::connect(address).await.unwrap();
            replacement.accept().await.unwrap();
        })
        .await
        .expect("loopback close and rebind must finish");
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
    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn connected_agent_udp_preserves_exact_peer() {
        let peer = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let socket = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let remote = peer.local_addr().unwrap();
        socket.connect(remote).await.unwrap();
        send_udp_to(&socket, b"packet", remote).await.unwrap();
        let mut bytes = [0; 8];
        let (n, _) = peer.recv_from(&mut bytes).await.unwrap();
        assert_eq!(&bytes[..n], b"packet");
        let other = SocketAddr::new(remote.ip(), remote.port().wrapping_add(1));
        assert_eq!(
            send_udp_to(&socket, b"wrong", other)
                .await
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::PermissionDenied
        );
    }
    #[tokio::test]
    async fn agent_started_tcp_finishes_without_reconnecting() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let remote = listener.local_addr().unwrap();
        let stream = tokio::net::TcpStream::connect(remote).await.unwrap();
        let (mut peer, _) = listener.accept().await.unwrap();
        let mut stream = PreparedTcpSocket::Connecting { stream, remote }
            .connect(remote)
            .await
            .unwrap();
        stream.write_all(b"ok").await.unwrap();
        let mut bytes = [0; 2];
        peer.read_exact(&mut bytes).await.unwrap();
        assert_eq!(&bytes, b"ok");
        assert!(
            PreparedTcpSocket::Connecting { stream, remote }
                .connect(SocketAddr::new(remote.ip(), remote.port().wrapping_add(1)))
                .await
                .is_err()
        );
    }
    #[cfg(target_os = "macos")]
    #[test]
    fn link_local_peer_uses_bound_scope_but_never_changes_ip_port_or_explicit_scope() {
        let packet = "[fe80::1]:443".parse().unwrap();
        let scoped = "[fe80::1%4]:443".parse().unwrap();
        assert!(connected_peer_matches(scoped, packet));
        for other in ["[fe80::1%5]:443", "[fe80::2]:443", "[fe80::1]:53"] {
            assert!(!connected_peer_matches(scoped, other.parse().unwrap()));
        }
        let mut targets = std::collections::HashMap::new();
        targets.insert(packet, 7);
        let normalized = connected_peer_matches(scoped, packet)
            .then_some(packet)
            .unwrap();
        assert_eq!(targets[&normalized], 7);
    }
}
