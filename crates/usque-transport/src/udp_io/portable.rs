use std::io;
use std::net::SocketAddr;

use tokio::net::UdpSocket;

use super::{
    ReceivedDatagram, RecvBatch, SendDatagram, UDP_ACTOR_DRAIN_LIMIT, UDP_RECEIVE_SLOT_SIZE,
    is_message_too_long, receive_pool_exhausted_error, receive_too_large_error,
};
use crate::network_quality::NetworkQualityTelemetry;

pub(super) fn try_recv_batch(
    socket: &UdpSocket,
    local_address: SocketAddr,
    output: &mut RecvBatch,
    quality: &NetworkQualityTelemetry,
) -> io::Result<usize> {
    while output.len() < UDP_ACTOR_DRAIN_LIMIT {
        let Some(mut buffer) = output.acquire_buffer(quality) else {
            return if output.is_empty() {
                Err(receive_pool_exhausted_error())
            } else {
                Ok(output.len())
            };
        };
        match socket.try_recv_from(buffer.portable_storage_mut()) {
            Ok((length, source)) => {
                quality.record_udp_recv(1);
                if length > UDP_RECEIVE_SLOT_SIZE {
                    quality.record_udp_receive_truncation();
                    return Err(receive_too_large_error());
                }
                output.push(ReceivedDatagram {
                    buffer,
                    length,
                    source,
                    destination: local_address,
                });
            }
            Err(error) if is_message_too_long(&error) => {
                quality.record_udp_recv(0);
                quality.record_udp_receive_truncation();
                return Err(receive_too_large_error());
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                quality.record_udp_recv(0);
                return if output.is_empty() {
                    Err(error)
                } else {
                    Ok(output.len())
                };
            }
            Err(error) => {
                quality.record_udp_recv(0);
                return Err(error);
            }
        }
    }
    Ok(output.len())
}

pub(super) fn try_send_batch(
    socket: &UdpSocket,
    batch: &[SendDatagram<'_>],
    quality: &NetworkQualityTelemetry,
) -> io::Result<usize> {
    let mut sent = 0;
    for datagram in batch.iter().take(UDP_ACTOR_DRAIN_LIMIT) {
        match socket.try_send_to(datagram.payload, datagram.destination) {
            Ok(length) if length == datagram.payload.len() => {
                quality.record_udp_send(1);
                sent += 1;
            }
            Ok(_) => {
                quality.record_udp_send(0);
                quality.record_udp_partial_batch();
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "UDP socket sent a partial datagram",
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                quality.record_udp_send(0);
                return if sent == 0 { Err(error) } else { Ok(sent) };
            }
            Err(error) => {
                quality.record_udp_send(0);
                return if sent == 0 { Err(error) } else { Ok(sent) };
            }
        }
    }
    Ok(sent)
}
