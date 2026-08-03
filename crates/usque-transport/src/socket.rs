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
