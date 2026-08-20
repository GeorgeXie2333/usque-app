use std::net::{SocketAddr, ToSocketAddrs};
use std::sync::Arc;

/// Platform-neutral representation of a socket before it connects to a
/// MASQUE endpoint.
///
/// Android uses the numeric file descriptor with `VpnService.protect(fd)` so
/// the tunnel transport cannot route back into its own TUN interface. Windows
/// and desktop proxy mode use the no-op implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SocketHandle(u64);

impl SocketHandle {
    pub const fn value(self) -> u64 {
        self.0
    }
}

/// Called immediately after a socket is created and before any endpoint
/// connection or packet is attempted.
pub trait SocketProtector: Send + Sync {
    fn protect(&self, socket: SocketHandle) -> Result<(), String>;

    /// Returns whether the platform's selected physical path can currently
    /// carry this endpoint address family. `None` means the platform has not
    /// supplied authoritative link properties yet.
    fn endpoint_family_available(&self, _endpoint: SocketAddr) -> Option<bool> {
        None
    }

    /// Monotonically increasing generation for the selected physical network.
    /// A change tells the transport supervisor to discard the old channel and
    /// create fresh endpoint sockets without tearing down local proxy listeners.
    fn network_generation(&self) -> Option<u64> {
        None
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

/// Bind a local proxy listener.
///
/// IPv6 sockets are forced to V6-only before bind. Windows dual-stack sockets
/// otherwise occupy the matching IPv4 port, so `127.0.0.1:8080` fails with
/// WSAEADDRINUSE when `[::1]:8080` is already bound.
pub(crate) fn bind_tcp_listener(
    address: SocketAddr,
) -> std::io::Result<tokio::net::TcpListener> {
    use tokio::net::TcpSocket;

    let socket = if address.is_ipv4() {
        TcpSocket::new_v4()?
    } else {
        let socket = TcpSocket::new_v6()?;
        force_ipv6_only(&socket)?;
        socket
    };
    socket.bind(address)?;
    socket.listen(256)
}

#[cfg(windows)]
fn force_ipv6_only(socket: &tokio::net::TcpSocket) -> std::io::Result<()> {
    use std::os::windows::io::AsRawSocket;
    use windows_sys::Win32::Networking::WinSock::{
        setsockopt, IPPROTO_IPV6, IPV6_V6ONLY, SOCKET_ERROR,
    };

    let enabled: i32 = 1;
    // SAFETY: `socket` is an open IPv6 TCP socket and `enabled` lives for
    // the duration of this setsockopt call.
    let result = unsafe {
        setsockopt(
            socket.as_raw_socket() as _,
            IPPROTO_IPV6 as i32,
            IPV6_V6ONLY,
            (&raw const enabled).cast(),
            i32::try_from(size_of_val(&enabled)).expect("IPV6_V6ONLY fits in i32"),
        )
    };
    if result == SOCKET_ERROR {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn force_ipv6_only(_socket: &tokio::net::TcpSocket) -> std::io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};

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
}
