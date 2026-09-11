//! Stream-level proxy boundary. No platform mutations or implicit fallbacks.
use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;

use async_trait::async_trait;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::time::{Instant, timeout_at};
use tokio_util::sync::CancellationToken;
use ts_netstack_smoltcp::CreateSocket;
use ts_netstack_smoltcp::netcore::Channel;
use ts_netstack_smoltcp::netsock::TcpStream as StackTcpStream;

#[derive(Clone, Debug, PartialEq, Eq)]
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
            self.ipv4.into()
        } else {
            self.ipv6.into()
        };
        let local = SocketAddr::new(ip, crate::port_allocator::next_tcp_port());
        tokio::select! {
            _ = cancellation.cancelled() => Err(DialError::Cancelled),
            result = timeout_at(deadline, self.channel.tcp_connect(local, remote)) => match result {
                Ok(Ok(stream)) => Ok(Box::new(stream)),
                Ok(Err(error)) if error.is_tcp_buffer_budget_exhausted() => Err(DialError::Budget),
                Ok(Err(_)) => Err(DialError::Refused),
                Err(_) => Err(DialError::Timeout),
            }
        }
    }
}

/// Shared frontend context; UDP is present only on the CONNECT-IP backend.
#[derive(Clone)]
pub(crate) struct ProxyServices {
    pub(crate) admission: Option<Arc<FrontendAdmission>>,
    pub(crate) dialer: Arc<dyn TcpDialer>,
    pub(crate) udp: Option<Channel>,
    pub(crate) resolver: crate::dns::Resolver,
    pub(crate) protector: Arc<dyn crate::socket::SocketProtector>,
    pub(crate) geo_policy: Arc<crate::geo_direct::GeoDirectPolicy>,
    pub(crate) counters: Arc<crate::netstack::TrafficCounters>,
    pub(crate) ipv4: Ipv4Addr,
    pub(crate) ipv6: Ipv6Addr,
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
            dialer: Arc::new(StackDialer {
                channel: stack.channel.clone(),
                ipv4,
                ipv6,
            }),
            udp: Some(stack.channel.clone()),
            resolver: crate::dns::Resolver::new(
                stack.channel.clone(),
                ipv4,
                ipv6,
                servers,
                profile.proxy.dns_mode,
                Arc::clone(&stack.protector),
            ),
            protector: Arc::clone(&stack.protector),
            geo_policy: Arc::clone(&stack.geo_policy),
            counters: Arc::clone(&stack.counters),
            ipv4,
            ipv6,
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
}

#[cfg(test)]
mod tests {
    use super::*;
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
