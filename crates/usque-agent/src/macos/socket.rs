//! Socket creation runs as root. PF checks the socket's creation credentials,
//! exact tuple and interface; descriptors never authorize a different target.
use super::{firewall::Permit, network::Network};
use socket2::{Domain, Protocol, Socket, Type};
use std::{
    io,
    net::{Shutdown, SocketAddr},
    os::fd::{AsRawFd, OwnedFd},
    sync::atomic::{AtomicBool, Ordering},
};
pub struct PhysicalSocket {
    socket: Socket,
    revoked: AtomicBool,
    pub permit: Permit,
}
impl PhysicalSocket {
    pub fn create(network: &Network, remote: SocketAddr, protocol: u8) -> io::Result<Self> {
        // SAFETY: read process credential before allocating anything.
        if unsafe { libc::geteuid() } != 0 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "only Agent may create physical sockets",
            ));
        }
        network.validate()?;
        if remote.port() == 0 || !super::resources::usable_address(remote.ip()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid physical destination",
            ));
        }
        let path = network.path(remote)?;
        let source = path.source(remote)?;
        let (kind, transport) = match protocol {
            6 => (Type::STREAM, Protocol::TCP),
            17 => (Type::DGRAM, Protocol::UDP),
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "invalid physical protocol",
                ));
            }
        };
        let socket = {
            let _serial = super::command::DESCRIPTOR_EXEC_LOCK
                .lock()
                .map_err(|_| io::Error::other("descriptor guard poisoned"))?;
            let socket = Socket::new(
                if remote.is_ipv4() {
                    Domain::IPV4
                } else {
                    Domain::IPV6
                },
                kind,
                Some(transport),
            )?;
            socket.set_nonblocking(true)?;
            if remote.is_ipv6() {
                socket.set_only_v6(true)?;
            }
            let (level, option) = if remote.is_ipv4() {
                (libc::IPPROTO_IP, libc::IP_BOUND_IF)
            } else {
                (libc::IPPROTO_IPV6, libc::IPV6_BOUND_IF)
            };
            // SAFETY: the kernel copies a u32 interface index from live storage.
            if unsafe {
                libc::setsockopt(
                    socket.as_raw_fd(),
                    level,
                    option,
                    (&path.index as *const u32).cast(),
                    std::mem::size_of::<u32>() as _,
                )
            } != 0
            {
                return Err(io::Error::last_os_error());
            }
            let mut local = SocketAddr::new(source, 0);
            if let SocketAddr::V6(address) = &mut local
                && address.ip().is_unicast_link_local()
            {
                address.set_scope_id(path.index);
            }
            socket.bind(&local.into())?;
            socket
        };
        let local = socket
            .local_addr()?
            .as_socket()
            .ok_or_else(|| io::Error::other("invalid physical socket address"))?;
        if local.ip() != source || local.port() == 0 {
            return Err(io::Error::other("physical socket bind mismatch"));
        }
        Ok(Self {
            socket,
            revoked: AtomicBool::new(false),
            permit: Permit {
                local,
                remote,
                protocol,
                interface: path.name.clone(),
            },
        })
    }
    /// Begin the exact authorized connection before handing out a descriptor.
    /// Unconnected Darwin sockets cannot be revoked with shutdown(2).
    pub fn connect(&self) -> io::Result<()> {
        let mut remote = self.permit.remote;
        if let SocketAddr::V6(address) = &mut remote
            && address.ip().is_unicast_link_local()
            && address.scope_id() == 0
        {
            let name =
                std::ffi::CString::new(self.permit.interface.as_str()).map_err(io::Error::other)?;
            // SAFETY: read-only query using a terminated interface name.
            let index = unsafe { libc::if_nametoindex(name.as_ptr()) };
            if index == 0 {
                return Err(io::Error::other("physical interface disappeared"));
            }
            address.set_scope_id(index);
        }
        match self.socket.connect(&remote.into()) {
            Ok(()) => Ok(()),
            Err(e) if self.permit.protocol == 6 && e.raw_os_error() == Some(libc::EINPROGRESS) => {
                Ok(())
            }
            Err(e) => Err(e),
        }
    }
    pub fn duplicate(&self) -> io::Result<OwnedFd> {
        Ok(self.socket.try_clone()?.into())
    }
    /// Stop shared I/O even if PF/journal cleanup fails. Keep the original FD
    /// pinned until permit removal is confirmed. ENOTCONN is not proof.
    pub fn shutdown(&self) -> io::Result<()> {
        if !self.revoked.load(Ordering::Acquire) {
            self.socket.shutdown(Shutdown::Both)?;
            self.revoked.store(true, Ordering::Release);
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_ordinary_user_before_opening_any_physical_socket() {
        // SAFETY: querying the current process credentials has no preconditions.
        if unsafe { libc::geteuid() } == 0 {
            return;
        }
        let network = Network {
            paths: vec![],
            dns: vec![],
        };
        let error = PhysicalSocket::create(&network, "192.0.2.1:443".parse().unwrap(), 6)
            .err()
            .unwrap();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    }
    fn loopback(protocol: u8) -> (PhysicalSocket, OwnedFd) {
        let remote = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP)).unwrap();
        socket
            .bind(&"127.0.0.1:0".parse::<SocketAddr>().unwrap().into())
            .unwrap();
        let local = socket.local_addr().unwrap().as_socket().unwrap();
        let physical = PhysicalSocket {
            socket,
            revoked: AtomicBool::new(false),
            permit: Permit {
                local,
                remote: remote.local_addr().unwrap(),
                protocol,
                interface: "lo0".into(),
            },
        };
        (physical, remote.into())
    }
    #[test]
    fn connected_udp_revocation_is_shared_and_idempotent() {
        let (physical, _peer) = loopback(17);
        physical.connect().unwrap();
        let duplicate = Socket::from(physical.duplicate().unwrap());
        assert_eq!(duplicate.send(b"before").unwrap(), 6);
        physical.shutdown().unwrap();
        physical.shutdown().unwrap();
        assert!(duplicate.send(b"after").is_err());
        assert!(
            duplicate
                .send_to(b"after", &physical.permit.remote.into())
                .is_err()
        );
    }
    #[test]
    fn unconnected_shutdown_failure_is_not_revocation_proof() {
        let (physical, _peer) = loopback(17);
        assert_eq!(
            physical.shutdown().unwrap_err().raw_os_error(),
            Some(libc::ENOTCONN)
        );
        assert!(!physical.revoked.load(Ordering::Acquire));
        physical.connect().unwrap();
        physical.shutdown().unwrap();
    }
}
