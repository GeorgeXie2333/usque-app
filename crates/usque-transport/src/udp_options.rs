//! Per-socket QUIC fragmentation policy. No route/interface/global mutations.
use std::io;
use std::net::UdpSocket;

/// Let DPLPMTUD probe beyond the cached path estimate, but never allow the
/// kernel to fragment a probe and turn reassembly into false PMTU evidence.
pub(crate) fn configure_quic_socket(socket: &UdpSocket) -> io::Result<()> {
    let ipv4 = socket.local_addr()?.is_ipv4();
    let (level, name, value) = fragmentation_option(ipv4)?;
    set_option(socket, level, name, value)?;
    #[cfg(any(target_os = "linux", target_os = "android"))]
    if !ipv4 {
        // IPv6 PMTUDISC_PROBE selects the interface MTU; Linux additionally
        // needs DONTFRAG to reject oversized UDP writes instead of fragmenting.
        set_option(socket, libc::IPPROTO_IPV6, libc::IPV6_DONTFRAG, 1)?;
        // Also cover IPv4-mapped destinations on a dual-stack UDP socket.
        set_option(
            socket,
            libc::IPPROTO_IP,
            libc::IP_MTU_DISCOVER,
            libc::IP_PMTUDISC_PROBE,
        )?;
    }
    Ok(())
}

#[cfg(windows)]
fn fragmentation_option(ipv4: bool) -> io::Result<(i32, i32, i32)> {
    use windows_sys::Win32::Networking::WinSock::{
        IP_MTU_DISCOVER, IP_PMTUDISC_PROBE, IPPROTO_IP, IPPROTO_IPV6, IPV6_MTU_DISCOVER,
    };
    Ok(if ipv4 {
        (IPPROTO_IP, IP_MTU_DISCOVER, IP_PMTUDISC_PROBE)
    } else {
        (IPPROTO_IPV6, IPV6_MTU_DISCOVER, IP_PMTUDISC_PROBE)
    })
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn fragmentation_option(ipv4: bool) -> io::Result<(i32, i32, i32)> {
    Ok(if ipv4 {
        (
            libc::IPPROTO_IP,
            libc::IP_MTU_DISCOVER,
            libc::IP_PMTUDISC_PROBE,
        )
    } else {
        (
            libc::IPPROTO_IPV6,
            libc::IPV6_MTU_DISCOVER,
            libc::IPV6_PMTUDISC_PROBE,
        )
    })
}

#[cfg(any(target_os = "macos", target_os = "ios", target_os = "freebsd"))]
fn fragmentation_option(ipv4: bool) -> io::Result<(i32, i32, i32)> {
    Ok(if ipv4 {
        (libc::IPPROTO_IP, libc::IP_DONTFRAG, 1)
    } else {
        (libc::IPPROTO_IPV6, libc::IPV6_DONTFRAG, 1)
    })
}

#[cfg(not(any(
    windows,
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd"
)))]
fn fragmentation_option(_ipv4: bool) -> io::Result<(i32, i32, i32)> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "QUIC fragmentation policy is unavailable",
    ))
}

#[cfg(windows)]
fn set_option(socket: &UdpSocket, level: i32, name: i32, value: i32) -> io::Result<()> {
    use std::os::windows::io::AsRawSocket;
    use windows_sys::Win32::Networking::WinSock::{SOCKET_ERROR, WSAGetLastError, setsockopt};
    // SAFETY: socket remains owned/open; value points to an initialized integer
    // with the exact option size, and Winsock borrows it only for this call.
    let result = unsafe {
        setsockopt(
            socket.as_raw_socket() as usize,
            level,
            name,
            (&value as *const i32).cast(),
            size_of::<i32>() as i32,
        )
    };
    if result == SOCKET_ERROR {
        // SAFETY: WSAGetLastError reads this thread's last Winsock error.
        return Err(io::Error::from_raw_os_error(unsafe { WSAGetLastError() }));
    }
    Ok(())
}

#[cfg(unix)]
fn set_option(socket: &UdpSocket, level: i32, name: i32, value: i32) -> io::Result<()> {
    use std::os::fd::AsRawFd;
    // SAFETY: socket stays owned/open; value is initialized and the supplied
    // length is exactly its size. setsockopt does not retain this pointer.
    let result = unsafe {
        libc::setsockopt(
            socket.as_raw_fd(),
            level,
            name,
            (&value as *const i32).cast(),
            size_of::<i32>() as libc::socklen_t,
        )
    };
    if result == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(not(any(windows, unix)))]
fn set_option(_socket: &UdpSocket, _level: i32, _name: i32, _value: i32) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "QUIC socket options are unavailable",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quic_sockets_disable_fragmentation_for_both_families() {
        for address in ["127.0.0.1:0", "[::1]:0"] {
            let socket = UdpSocket::bind(address).unwrap();
            configure_quic_socket(&socket).unwrap();
            let (level, name, expected) =
                fragmentation_option(socket.local_addr().unwrap().is_ipv4()).unwrap();
            let mut actual = 0_i32;
            #[cfg(windows)]
            {
                use std::os::windows::io::AsRawSocket;
                let mut length = size_of::<i32>() as i32;
                // SAFETY: the live socket and correctly sized initialized
                // output buffers remain valid throughout this synchronous read.
                let result = unsafe {
                    windows_sys::Win32::Networking::WinSock::getsockopt(
                        socket.as_raw_socket() as usize,
                        level,
                        name,
                        (&mut actual as *mut i32).cast(),
                        &mut length,
                    )
                };
                assert_eq!(result, 0);
            }
            #[cfg(unix)]
            {
                use std::os::fd::AsRawFd;
                let mut length = size_of::<i32>() as libc::socklen_t;
                // SAFETY: the socket remains owned and the initialized output
                // buffers match the option's integer type and length.
                let result = unsafe {
                    libc::getsockopt(
                        socket.as_raw_fd(),
                        level,
                        name,
                        (&mut actual as *mut i32).cast(),
                        &mut length,
                    )
                };
                assert_eq!(result, 0);
            }
            assert_eq!(actual, expected);
            #[cfg(any(target_os = "linux", target_os = "android"))]
            if socket.local_addr().unwrap().is_ipv6() {
                use std::os::fd::AsRawFd;
                for (level, name, expected) in [
                    (libc::IPPROTO_IPV6, libc::IPV6_DONTFRAG, 1),
                    (
                        libc::IPPROTO_IP,
                        libc::IP_MTU_DISCOVER,
                        libc::IP_PMTUDISC_PROBE,
                    ),
                ] {
                    let mut actual = 0_i32;
                    let mut length = size_of::<i32>() as libc::socklen_t;
                    // SAFETY: the live socket and initialized integer output
                    // buffers remain valid for the synchronous getsockopt call.
                    let result = unsafe {
                        libc::getsockopt(
                            socket.as_raw_fd(),
                            level,
                            name,
                            (&mut actual as *mut i32).cast(),
                            &mut length,
                        )
                    };
                    assert_eq!(result, 0);
                    assert_eq!(actual, expected);
                }
            }
        }
    }
}
