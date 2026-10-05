//! Stream-level proxy boundary. No platform mutations or implicit fallbacks.
use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;

use async_trait::async_trait;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::time::{Instant, timeout_at};
use tokio_util::sync::CancellationToken;
use ts_netstack_smoltcp::netcore::Channel;
use ts_netstack_smoltcp::netsock::TcpStream as StackTcpStream;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct TcpTarget {
    authority: String,
    address: Option<SocketAddr>,
}

impl TcpTarget {
    pub(crate) fn new(host: &str, port: u16) -> Result<Self, DialError> {
        if port == 0 {
            return Err(DialError::InvalidTarget);
        }
        if let Ok(ip) = host.parse::<IpAddr>() {
            return Ok(Self::address(SocketAddr::new(ip, port)));
        }
        if host.is_empty()
            || host.len() > 253
            || !host.split('.').all(|label| {
                !label.is_empty()
                    && label.len() <= 63
                    && label
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            })
        {
            return Err(DialError::InvalidTarget);
        }
        Ok(Self {
            authority: format!("{host}:{port}"),
            address: None,
        })
    }

    pub(crate) fn address(address: SocketAddr) -> Self {
        Self {
            authority: address.to_string(),
            address: Some(address),
        }
    }

    pub(crate) fn authority(&self) -> &str {
        &self.authority
    }
    pub(crate) fn socket_address(&self) -> Option<SocketAddr> {
        self.address
    }
    pub(crate) fn host_port(&self) -> (&str, u16) {
        let (host, port) = self.authority.rsplit_once(':').expect("validated target");
        (
            host.trim_start_matches('[').trim_end_matches(']'),
            port.parse().expect("validated port"),
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum DialError {
    #[error("invalid TCP target")]
    InvalidTarget,
    #[error("TCP connect timed out")]
    Timeout,
    #[error("TCP connect cancelled")]
    Cancelled,
    #[error("TCP connection refused")]
    Refused,
    #[error("proxy resource budget exhausted")]
    Budget,
    #[error("L4 CONNECT rejected with HTTP {0}")]
    Rejected(u16),
    #[error("proxy session closed")]
    Closed,
    #[error("TCP network unavailable")]
    Network,
    #[error("L4 protocol error")]
    Protocol,
}

impl From<DialError> for io::Error {
    fn from(error: DialError) -> Self {
        let kind = match error {
            DialError::Timeout => io::ErrorKind::TimedOut,
            DialError::Cancelled => io::ErrorKind::Interrupted,
            DialError::Refused => io::ErrorKind::ConnectionRefused,
            DialError::Closed => io::ErrorKind::ConnectionReset,
            DialError::Budget => io::ErrorKind::WouldBlock,
            DialError::InvalidTarget | DialError::Protocol => io::ErrorKind::InvalidData,
            DialError::Rejected(401 | 403) => io::ErrorKind::PermissionDenied,
            DialError::Rejected(_) | DialError::Network => io::ErrorKind::NotConnected,
        };
        Self::new(kind, error)
    }
}

pub(crate) trait TcpIo: AsyncRead + AsyncWrite + Send + Unpin {
    fn local_addr(&self) -> io::Result<SocketAddr>;
    fn session_generation(&self) -> Option<u64> {
        None
    }
    fn has_owned_read(&self) -> bool {
        false
    }
    /// An empty chunk is EOF. Pending retains no caller-owned buffer.
    fn poll_read_owned(
        &mut self,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<io::Result<bytes::Bytes>> {
        std::task::Poll::Ready(Err(io::ErrorKind::Unsupported.into()))
    }
}

/// The caller retains the same unconsumed chunk until the command completes.
pub(crate) trait OwnedTcpWrite: AsyncRead + AsyncWrite + Unpin {
    fn poll_write_owned(
        &mut self,
        cx: &mut std::task::Context<'_>,
        bytes: &bytes::Bytes,
    ) -> std::task::Poll<io::Result<usize>>;
}

impl TcpIo for StackTcpStream {
    fn local_addr(&self) -> io::Result<SocketAddr> {
        Ok(Self::local_addr(self))
    }
}

pub(crate) type TcpStream = Box<dyn TcpIo>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FlowClass {
    Business,
    Dns,
}

#[async_trait]
pub(crate) trait TcpDialer: Send + Sync {
    fn is_ready(&self) -> bool {
        true
    }
    fn session_generation(&self) -> Option<u64> {
        None
    }
    async fn connect(
        &self,
        target: TcpTarget,
        deadline: Instant,
        cancellation: &CancellationToken,
        class: FlowClass,
    ) -> Result<TcpStream, DialError>;
}

pub(crate) struct StackDialer {
    pub(crate) channel: Channel,
    pub(crate) ipv4: Ipv4Addr,
    pub(crate) ipv6: Ipv6Addr,
}

/// Encrypted DNS pools follow the packet runtime's reconnect epoch. Numeric
/// bootstrap connections still use the private stack without direct rules.
pub(crate) struct RuntimeStackDialer {
    pub(crate) inner: StackDialer,
    pub(crate) health: tokio::sync::watch::Receiver<crate::netstack::RuntimeHealth>,
    pub(crate) cancellation: CancellationToken,
}

#[async_trait]
impl TcpDialer for RuntimeStackDialer {
    fn is_ready(&self) -> bool {
        !self.cancellation.is_cancelled()
            && matches!(
                *self.health.borrow(),
                crate::netstack::RuntimeHealth::Connected { .. }
            )
    }

    fn session_generation(&self) -> Option<u64> {
        if !self.is_ready() {
            return None;
        }
        match *self.health.borrow() {
            crate::netstack::RuntimeHealth::Connected {
                reconnect_count, ..
            } => Some(u64::from(reconnect_count)),
            _ => None,
        }
    }

    async fn connect(
        &self,
        target: TcpTarget,
        deadline: Instant,
        cancellation: &CancellationToken,
        class: FlowClass,
    ) -> Result<TcpStream, DialError> {
        let generation = self.session_generation().ok_or(DialError::Closed)?;
        let mut health = self.health.clone();
        let operation = self.inner.connect(target, deadline, cancellation, class);
        tokio::pin!(operation);
        loop {
            tokio::select! {
                biased;
                _ = self.cancellation.cancelled() => return Err(DialError::Cancelled),
                changed = health.changed() => {
                    if changed.is_err() || self.session_generation() != Some(generation) {
                        return Err(DialError::Closed);
                    }
                },
                result = &mut operation => {
                    if self.session_generation() != Some(generation) {
                        return Err(DialError::Closed);
                    }
                    return result;
                },
            }
        }
    }
}

#[async_trait]
impl TcpDialer for StackDialer {
    async fn connect(
        &self,
        target: TcpTarget,
        deadline: Instant,
        cancellation: &CancellationToken,
        _class: FlowClass,
    ) -> Result<TcpStream, DialError> {
        let remote = target
            .address
            .filter(|v| v.port() != 0)
            .ok_or(DialError::InvalidTarget)?;
        let ip = if remote.is_ipv4() {
            IpAddr::V4(self.ipv4)
        } else {
            IpAddr::V6(self.ipv6)
        };
        if ip.is_unspecified() {
            return Err(DialError::Network);
        }
        let local = SocketAddr::new(ip, crate::port_allocator::next_tcp_port());
        tokio::select! {
            _ = cancellation.cancelled() => Err(DialError::Cancelled),
            result = timeout_at(deadline, crate::stack_tcp::StackTcpStream::connect(self.channel.clone(), local, remote)) => match result {
                Ok(Ok(stream)) => Ok(Box::new(stream)),
                Ok(Err(error)) if error.is_tcp_buffer_budget_exhausted() => Err(DialError::Budget),
                Ok(Err(_)) => Err(DialError::Refused),
                Err(_) => Err(DialError::Timeout),
            }
        }
    }
}

/// Shared frontend context; an optional factory owns each final UDP association.
#[derive(Clone)]
pub(crate) struct ProxyServices {
    pub(crate) traffic_policy: Arc<crate::application_traffic::ApplicationTrafficPolicy>,
    pub(crate) admission: Option<Arc<FrontendAdmission>>,
    pub(crate) dialer: Arc<dyn TcpDialer>,
    pub(crate) udp: Option<Arc<dyn crate::proxy_udp::UdpFactory>>,
    pub(crate) resolver: crate::dns::Resolver,
    pub(crate) protector: Arc<dyn crate::socket::SocketProtector>,
    pub(crate) geo_policy: Arc<crate::geo_direct::GeoDirectPolicy>,
    pub(crate) counters: Arc<crate::netstack::TrafficCounters>,
    pub(crate) cancellation: CancellationToken,
    pub(crate) health: tokio::sync::watch::Receiver<crate::netstack::RuntimeHealth>,
}

impl ProxyServices {
    pub(crate) fn from_stack(
        profile: &usque_core::Profile,
        ipv4: Ipv4Addr,
        ipv6: Ipv6Addr,
        stack: &crate::netstack::PacketStack,
    ) -> Self {
        let servers = if profile.proxy.dns_mode == usque_core::ProxyDnsMode::LocalConfigured {
            profile.proxy.dns_servers.clone()
        } else {
            profile.dns_servers.clone()
        };
        Self {
            admission: None,
            traffic_policy: Arc::clone(&stack.traffic_policy),
            dialer: Arc::new(StackDialer {
                channel: stack.channel.clone(),
                ipv4,
                ipv6,
            }),
            udp: Some(crate::proxy_udp::StackFactory::shared(
                stack.channel.clone(),
                ipv4,
                ipv6,
            )),
            resolver: crate::dns::Resolver::new(
                stack.channel.clone(),
                ipv4,
                ipv6,
                servers,
                profile.proxy.dns_mode,
                Arc::clone(&stack.protector),
            )
            .with_final_exit(profile.chain_enabled(), stack.cancellation.clone())
            .with_warp_dns(stack.warp_dns.clone()),
            protector: Arc::clone(&stack.protector),
            geo_policy: Arc::clone(&stack.geo_policy),
            counters: Arc::clone(&stack.counters),
            cancellation: stack.cancellation.clone(),
            health: stack.subscribe_health(),
        }
    }
}

/// Admission before parsing/authentication; shared by both L4 frontends.
pub(crate) struct FrontendAdmission {
    permits: Arc<tokio::sync::Semaphore>,
    budget: Arc<crate::l4::BufferBudget>,
}

pub(crate) struct FrontendPermit {
    _permit: tokio::sync::OwnedSemaphorePermit,
    _buffers: crate::l4::stream::BufferLease,
}

impl FrontendAdmission {
    pub(crate) fn new(budget: Arc<crate::l4::BufferBudget>, capacity: usize) -> Self {
        Self {
            permits: Arc::new(tokio::sync::Semaphore::new(capacity)),
            budget,
        }
    }

    pub(crate) fn acquire(&self) -> Option<FrontendPermit> {
        let permit = self.permits.clone().try_acquire_owned().ok();
        let buffers = self
            .budget
            .reserve_admission(2 * crate::relay::RELAY_BUFFER_SIZE);
        match (permit, buffers) {
            (Some(_permit), Some(_buffers)) => Some(FrontendPermit { _permit, _buffers }),
            _ => {
                self.budget.metrics.update(|m| m.budget_rejections += 1);
                None
            }
        }
    }

    pub(crate) fn reserve_udp_buffers(&self) -> Option<crate::l4::stream::BufferLease> {
        self.budget.reserve_admission(16 * 16 * 1024 * 2)
    }

    pub(crate) fn reserve_udp_dns_query(
        &self,
        query_bytes: usize,
    ) -> Option<crate::l4::stream::BufferLease> {
        // One owned query, the maximum framed TCP DNS answer, and the bounded
        // SOCKS reply coexist briefly. Include the maximum domain target too.
        self.budget
            .reserve_admission(query_bytes + usize::from(u16::MAX) + 16 * 1024 + 256)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn encrypted_stack_dialer_stops_during_reconnect_and_tracks_epochs() {
        use crate::netstack::{RuntimeHealth, RuntimePath};
        use ts_netstack_smoltcp::netcore::HasChannel;
        let (stack, _pipe) = crate::netstack::bounded_piped(Default::default());
        let path = RuntimePath {
            transport: usque_core::Transport::Http2,
            endpoint_family: usque_core::AddressFamily::Ipv4,
            ipv4_available: true,
            ipv6_available: true,
        };
        let (sender, health) = tokio::sync::watch::channel(RuntimeHealth::Connected {
            path,
            reconnect_count: 3,
        });
        let cancellation = CancellationToken::new();
        let dialer = RuntimeStackDialer {
            inner: StackDialer {
                channel: stack.command_channel(),
                ipv4: Ipv4Addr::new(172, 16, 0, 2),
                ipv6: Ipv6Addr::UNSPECIFIED,
            },
            health,
            cancellation: cancellation.clone(),
        };
        assert!(dialer.is_ready());
        assert_eq!(dialer.session_generation(), Some(3));
        sender.send_replace(RuntimeHealth::Reconnecting {
            last_path: path,
            attempt: 1,
            reconnect_count: 3,
            reason: "test_reconnect".into(),
            failure: usque_core::TransportFailure::new(
                usque_core::TransportFailureCode::PhysicalNetworkChanged,
                usque_core::TransportStage::SocketConnect,
            ),
        });
        assert!(!dialer.is_ready());
        assert_eq!(dialer.session_generation(), None);
        assert_eq!(
            dialer
                .connect(
                    TcpTarget::address("1.1.1.1:853".parse().unwrap()),
                    Instant::now() + std::time::Duration::from_secs(1),
                    &cancellation,
                    FlowClass::Dns
                )
                .await
                .err(),
            Some(DialError::Closed)
        );
        sender.send_replace(RuntimeHealth::Connected {
            path,
            reconnect_count: 4,
        });
        assert!(dialer.is_ready());
        assert_eq!(dialer.session_generation(), Some(4));
        cancellation.cancel();
        assert!(!dialer.is_ready());
        assert_eq!(dialer.session_generation(), None);
    }
    #[test]
    fn authority_is_bounded_and_cannot_inject_headers() {
        assert_eq!(TcpTarget::new("::1", 443).unwrap().authority(), "[::1]:443");
        assert_eq!(
            TcpTarget::new("example.com", 443).unwrap().authority(),
            "example.com:443"
        );
        for name in ["a@b", "a/b", "a\r\nb", "a b", "[::1]", "", "a..b"] {
            assert_eq!(TcpTarget::new(name, 443), Err(DialError::InvalidTarget));
        }
        assert_eq!(
            TcpTarget::new("example.com", 0),
            Err(DialError::InvalidTarget)
        );
    }
}
