use std::io;
use std::mem;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};
use std::os::fd::AsRawFd;
use std::ptr;

use tokio::net::UdpSocket;

use super::{
    ReceivedDatagram, RecvBatch, SendDatagram, UDP_ACTOR_DRAIN_LIMIT, UDP_BATCH_SIZE,
    UDP_RECEIVE_SLOT_SIZE, receive_too_large_error,
};
use crate::network_quality::NetworkQualityTelemetry;

pub(super) fn try_recv_batch(
    socket: &UdpSocket,
    local_address: SocketAddr,
    output: &mut RecvBatch,
    quality: &NetworkQualityTelemetry,
) -> io::Result<usize> {
    while output.len() < UDP_ACTOR_DRAIN_LIMIT {
        let requested = UDP_BATCH_SIZE.min(UDP_ACTOR_DRAIN_LIMIT - output.len());
        let mut buffers: [Option<super::PooledUdpBuffer>; UDP_BATCH_SIZE] =
            std::array::from_fn(|_| Some(output.acquire_buffer(quality)));
        let mut addresses: [libc::sockaddr_storage; UDP_BATCH_SIZE] =
            std::array::from_fn(|_| zeroed_sockaddr_storage());
        let mut iovecs: [libc::iovec; UDP_BATCH_SIZE] = std::array::from_fn(|index| {
            let buffer = buffers[index]
                .as_mut()
                .expect("receive buffer slot is initialized");
            libc::iovec {
                iov_base: buffer.batch_storage_mut().as_mut_ptr().cast(),
                iov_len: UDP_RECEIVE_SLOT_SIZE,
            }
        });
        let mut messages: [libc::mmsghdr; UDP_BATCH_SIZE] =
            std::array::from_fn(|index| libc::mmsghdr {
                msg_hdr: libc::msghdr {
                    msg_name: ptr::from_mut(&mut addresses[index]).cast(),
                    msg_namelen: mem::size_of::<libc::sockaddr_storage>() as libc::socklen_t,
                    msg_iov: ptr::from_mut(&mut iovecs[index]),
                    msg_iovlen: 1,
                    msg_control: ptr::null_mut(),
                    msg_controllen: 0,
                    msg_flags: 0,
                },
                msg_len: 0,
            });
        // SAFETY: `socket` owns a valid nonblocking UDP fd for the duration of
        // this call. `messages[..requested]`, their one-element iovec entries,
        // sockaddr storage, and boxed receive buffers are initialized, pinned
        // by local ownership, mutually disjoint, and live until `recvmmsg`
        // returns. Every iovec length is exactly the 2048-byte payload bound.
        let received = unsafe {
            libc::recvmmsg(
                socket.as_raw_fd(),
                messages.as_mut_ptr(),
                requested as libc::c_uint,
                libc::MSG_DONTWAIT | libc::MSG_TRUNC,
                ptr::null_mut(),
            )
        };
        if received < 0 {
            let error = io::Error::last_os_error();
            quality.record_udp_recv(0);
            return if output.is_empty() {
                Err(error)
            } else {
                Ok(output.len())
            };
        }
        let received = received as usize;
        if received > requested {
            quality.record_udp_recv(0);
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "recvmmsg returned more datagrams than requested",
            ));
        }
        if received == 0 {
            quality.record_udp_recv(0);
            return if output.is_empty() {
                Err(io::Error::from(io::ErrorKind::WouldBlock))
            } else {
                Ok(output.len())
            };
        }
        quality.record_udp_recv(received as u64);
        let mut truncated = false;
        for index in 0..received {
            let message = &messages[index];
            if message.msg_hdr.msg_flags & libc::MSG_TRUNC != 0
                || message.msg_len as usize > UDP_RECEIVE_SLOT_SIZE
            {
                quality.record_udp_receive_truncation();
                truncated = true;
                continue;
            }
            let source = sockaddr_to_socket_addr(&addresses[index], message.msg_hdr.msg_namelen)?;
            output.push(ReceivedDatagram {
                buffer: buffers[index]
                    .take()
                    .expect("received UDP buffer remains initialized"),
                length: message.msg_len as usize,
                source,
                destination: local_address,
            });
        }
        if truncated {
            return Err(receive_too_large_error());
        }
        if received < requested {
            break;
        }
    }
    Ok(output.len())
}

pub(super) fn try_send_batch(
    socket: &UdpSocket,
    batch: &[SendDatagram<'_>],
    quality: &NetworkQualityTelemetry,
) -> io::Result<usize> {
    let mut total_sent = 0;
    while total_sent < batch.len().min(UDP_ACTOR_DRAIN_LIMIT) {
        let remaining = &batch[total_sent..batch.len().min(UDP_ACTOR_DRAIN_LIMIT)];
        let requested = remaining.len().min(UDP_BATCH_SIZE);
        let mut addresses: [libc::sockaddr_storage; UDP_BATCH_SIZE] =
            std::array::from_fn(|_| zeroed_sockaddr_storage());
        let mut address_lengths = [0 as libc::socklen_t; UDP_BATCH_SIZE];
        for index in 0..requested {
            let (address, length) = socket_addr_to_storage(remaining[index].destination);
            addresses[index] = address;
            address_lengths[index] = length;
        }
        let mut iovecs: [libc::iovec; UDP_BATCH_SIZE] = std::array::from_fn(|index| {
            let payload = remaining.get(index).map_or(&[][..], |item| item.payload);
            libc::iovec {
                iov_base: payload.as_ptr().cast_mut().cast(),
                iov_len: payload.len(),
            }
        });
        let mut messages: [libc::mmsghdr; UDP_BATCH_SIZE] =
            std::array::from_fn(|index| libc::mmsghdr {
                msg_hdr: libc::msghdr {
                    msg_name: ptr::from_mut(&mut addresses[index]).cast(),
                    msg_namelen: address_lengths[index],
                    msg_iov: ptr::from_mut(&mut iovecs[index]),
                    msg_iovlen: 1,
                    msg_control: ptr::null_mut(),
                    msg_controllen: 0,
                    msg_flags: 0,
                },
                msg_len: 0,
            });
        // SAFETY: `socket` owns a valid nonblocking UDP fd for the duration of
        // this call. `messages[..requested]`, each one-element iovec, and every
        // initialized sockaddr are disjoint and remain alive until `sendmmsg`
        // returns. Iovecs borrow immutable payload slices for the entire call;
        // the kernel reads but never mutates those bytes. Each message retains
        // its own destination, so no address or path is inferred or merged.
        let sent = unsafe {
            libc::sendmmsg(
                socket.as_raw_fd(),
                messages.as_mut_ptr(),
                requested as libc::c_uint,
                libc::MSG_DONTWAIT,
            )
        };
        if sent < 0 {
            let error = io::Error::last_os_error();
            quality.record_udp_send(0);
            return if total_sent == 0 {
                Err(error)
            } else {
                Ok(total_sent)
            };
        }
        let sent = sent as usize;
        if sent > requested {
            quality.record_udp_send(0);
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "sendmmsg returned more datagrams than requested",
            ));
        }
        if sent == 0 {
            quality.record_udp_send(0);
            return if total_sent == 0 {
                Err(io::Error::from(io::ErrorKind::WouldBlock))
            } else {
                Ok(total_sent)
            };
        }
        quality.record_udp_send(sent as u64);
        total_sent += sent;
        if sent < requested {
            break;
        }
    }
    Ok(total_sent)
}

fn zeroed_sockaddr_storage() -> libc::sockaddr_storage {
    // SAFETY: all-zero bytes are a valid initialized representation for
    // `sockaddr_storage`; the family-specific value is written before send or
    // filled by the kernel before receive parsing.
    unsafe { mem::zeroed() }
}

fn socket_addr_to_storage(address: SocketAddr) -> (libc::sockaddr_storage, libc::socklen_t) {
    let mut storage = zeroed_sockaddr_storage();
    match address {
        SocketAddr::V4(address) => {
            let value = libc::sockaddr_in {
                sin_family: libc::AF_INET as libc::sa_family_t,
                sin_port: address.port().to_be(),
                sin_addr: libc::in_addr {
                    s_addr: u32::from_ne_bytes(address.ip().octets()),
                },
                sin_zero: [0; 8],
            };
            // SAFETY: `sockaddr_storage` is aligned and large enough for
            // `sockaddr_in`; `storage` is uniquely borrowed and fully
            // initialized with the IPv4 value before its pointer is exposed.
            unsafe {
                ptr::write(
                    ptr::from_mut(&mut storage).cast::<libc::sockaddr_in>(),
                    value,
                );
            }
            (
                storage,
                mem::size_of::<libc::sockaddr_in>() as libc::socklen_t,
            )
        }
        SocketAddr::V6(address) => {
            let value = libc::sockaddr_in6 {
                sin6_family: libc::AF_INET6 as libc::sa_family_t,
                sin6_port: address.port().to_be(),
                sin6_flowinfo: address.flowinfo().to_be(),
                sin6_addr: libc::in6_addr {
                    s6_addr: address.ip().octets(),
                },
                sin6_scope_id: address.scope_id(),
            };
            // SAFETY: `sockaddr_storage` is aligned and large enough for
            // `sockaddr_in6`; `storage` is uniquely borrowed and fully
            // initialized with the IPv6 value before its pointer is exposed.
            unsafe {
                ptr::write(
                    ptr::from_mut(&mut storage).cast::<libc::sockaddr_in6>(),
                    value,
                );
            }
            (
                storage,
                mem::size_of::<libc::sockaddr_in6>() as libc::socklen_t,
            )
        }
    }
}

fn sockaddr_to_socket_addr(
    storage: &libc::sockaddr_storage,
    length: libc::socklen_t,
) -> io::Result<SocketAddr> {
    match storage.ss_family as libc::c_int {
        libc::AF_INET if length as usize >= mem::size_of::<libc::sockaddr_in>() => {
            // SAFETY: the kernel set AF_INET and reported a length large enough
            // for `sockaddr_in`; `storage` remains alive and properly aligned
            // while the family-specific value is copied out.
            let value = unsafe { ptr::read(ptr::from_ref(storage).cast::<libc::sockaddr_in>()) };
            Ok(SocketAddr::V4(SocketAddrV4::new(
                Ipv4Addr::from(value.sin_addr.s_addr.to_ne_bytes()),
                u16::from_be(value.sin_port),
            )))
        }
        libc::AF_INET6 if length as usize >= mem::size_of::<libc::sockaddr_in6>() => {
            // SAFETY: the kernel set AF_INET6 and reported a length large
            // enough for `sockaddr_in6`; `storage` remains alive and properly
            // aligned while the family-specific value is copied out.
            let value = unsafe { ptr::read(ptr::from_ref(storage).cast::<libc::sockaddr_in6>()) };
            Ok(SocketAddr::V6(SocketAddrV6::new(
                Ipv6Addr::from(value.sin6_addr.s6_addr),
                u16::from_be(value.sin6_port),
                u32::from_be(value.sin6_flowinfo),
                value.sin6_scope_id,
            )))
        }
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "UDP batch receive returned an invalid source address",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn socket_address_storage_round_trips_v4_and_v6_boundaries() {
        for address in [
            "127.0.0.1:65535".parse().unwrap(),
            "[::1]:1".parse().unwrap(),
            SocketAddr::V6(SocketAddrV6::new("fe80::1234".parse().unwrap(), 443, 17, 7)),
        ] {
            let (storage, length) = socket_addr_to_storage(address);
            assert_eq!(sockaddr_to_socket_addr(&storage, length).unwrap(), address);
        }
    }

    #[test]
    fn short_or_unknown_sockaddr_is_rejected() {
        let mut storage = zeroed_sockaddr_storage();
        storage.ss_family = libc::AF_INET as libc::sa_family_t;
        assert!(sockaddr_to_socket_addr(&storage, 1).is_err());
        storage.ss_family = 255;
        assert!(
            sockaddr_to_socket_addr(
                &storage,
                mem::size_of::<libc::sockaddr_storage>() as libc::socklen_t,
            )
            .is_err()
        );
    }
}
