use alloc::vec;
use core::net::SocketAddr;

use bytes::Bytes;
use smoltcp::{iface::SocketHandle, socket::udp, wire::IpVersion};

use crate::command::{
    Error, Response,
    udp::{Command as UdpCommand, Response as UdpResponse},
};

impl crate::Netstack {
    /// Process a UDP socket command.
    #[tracing::instrument(skip_all, fields(?handle, ?cmd), level = "debug")]
    pub(crate) fn process_udp(
        &mut self,
        cmd: UdpCommand,
        handle: Option<SocketHandle>,
    ) -> Response {
        let receive_override = match &cmd {
            UdpCommand::BindWithReceiveBuffer {
                receive_buffer_size,
                receive_message_count,
                ..
            } => Some((*receive_buffer_size, *receive_message_count)),
            _ => None,
        };
        match cmd {
            UdpCommand::Bind { endpoint } | UdpCommand::BindWithReceiveBuffer { endpoint, .. } => {
                if endpoint.port() == 0 {
                    tracing::error!(?endpoint, "udp bind: zero port");
                    return Response::Error(Error::unaddressable());
                }
                let receive = if let Some((bytes, packets)) = receive_override {
                    if bytes == 0 || bytes > 128 * 1024 || packets == 0 || packets > 512 {
                        return Response::Error(Error::big_packet());
                    }
                    udp::PacketBuffer::new(
                        vec![udp::PacketMetadata::EMPTY; packets],
                        vec![0; bytes],
                    )
                } else {
                    self.udp_buffer()
                };
                let mut sock = udp::Socket::new(receive, self.udp_buffer());

                // The two possible failure cases for `bind` are that the port is zero or the socket
                // was already open. Those are handled, so failure is impossible here.
                sock.bind(endpoint).unwrap();

                let handle = self.socket_set.add(sock);

                UdpResponse::Bound {
                    local: endpoint,
                    handle,
                }
                .into()
            }
            UdpCommand::Send { endpoint, buf } => {
                let handle = handle.unwrap();

                let sock = self.socket_set.get_mut::<udp::Socket>(handle);
                let sock_is_v4 = sock
                    .endpoint()
                    .addr
                    .is_some_and(|ep| ep.version() == IpVersion::Ipv4);

                if endpoint.is_ipv4() != sock_is_v4 {
                    return Response::Error(Error::wrong_ip_version());
                }

                if buf.len() > sock.payload_send_capacity() {
                    tracing::error!(
                        len = buf.len(),
                        socket_capacity = sock.payload_send_capacity(),
                        "requested message size overflows socket capacity",
                    );

                    return Response::Error(Error::big_packet());
                }

                match sock.send_slice(&buf, endpoint) {
                    Ok(_n) => Response::Ok,
                    // This means that the _current_ buffer is too full, but since we checked if we
                    // had send capacity, it should be available in the future, so just punt and
                    // wouldblock until then.
                    Err(udp::SendError::BufferFull) => Response::WouldBlock {
                        command: UdpCommand::Send { buf, endpoint }.into(),
                        handle: Some(handle),
                    },
                    Err(udp::SendError::Unaddressable) => {
                        tracing::error!(?endpoint, "invalid endpoint");
                        Response::Error(Error::unaddressable())
                    }
                }
            }
            UdpCommand::Recv { max_len } => {
                let sock = self
                    .socket_set
                    .get_mut::<udp::Socket>(unwrap_handle!(handle));

                match sock.recv() {
                    Ok((b, meta)) => {
                        let mut len = b.len();
                        let mut truncated = None;

                        if let Some(max_len) = max_len {
                            let max_len = max_len.get();

                            if len > max_len {
                                truncated = Some(len);
                                tracing::warn!(len, max_len, "udp read truncated");
                            }

                            len = max_len.min(len);
                        }

                        UdpResponse::RecvFrom {
                            remote: SocketAddr::new(meta.endpoint.addr.into(), meta.endpoint.port),
                            buf: Bytes::copy_from_slice(&b[..len]),
                            truncated,
                        }
                        .into()
                    }
                    Err(udp::RecvError::Exhausted) => Response::WouldBlock {
                        command: UdpCommand::Recv { max_len }.into(),
                        handle,
                    },
                    Err(udp::RecvError::Truncated) => {
                        // this can't occur for recv() as we have a view into the backing
                        // socketbuffer storage. truncated only occurs for recv_slice().
                        unreachable!()
                    }
                }
            }
            UdpCommand::Close => {
                // NOTE(npry): smoltcp supports socket reuse via `socket.close()`, which puts the
                // socket in a valid state to re-bound. We don't support that for API simplicity,
                // but we could in principle if there was a motivating reason.
                self.socket_set.remove(unwrap_handle!(handle));

                Response::Ok
            }
        }
    }

    fn udp_buffer(&self) -> udp::PacketBuffer<'static> {
        udp::PacketBuffer::new(
            vec![udp::PacketMetadata::EMPTY; self.config.udp_message_count],
            vec![0; self.config.udp_buffer_size],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Command, Config, Netstack, Request, Response, flume};

    fn stack() -> Netstack {
        Netstack::new(Config::default(), smoltcp::time::Instant::from_millis(0))
    }

    #[test]
    fn receive_override_preserves_transmit_and_ordinary_bind_capacities() {
        let mut stack = stack();
        let Response::Udp(UdpResponse::Bound { handle, .. }) = stack.process_udp(
            UdpCommand::BindWithReceiveBuffer {
                endpoint: "192.0.2.1:40001".parse().unwrap(),
                receive_buffer_size: 128 * 1024,
                receive_message_count: 512,
            },
            None,
        ) else {
            panic!("bounded receive bind must succeed");
        };
        let socket = stack.socket_set.get::<udp::Socket>(handle);
        assert_eq!(socket.payload_recv_capacity(), 128 * 1024);
        assert_eq!(socket.packet_recv_capacity(), 512);
        assert_eq!(socket.payload_send_capacity(), stack.config.udp_buffer_size);
        assert_eq!(
            socket.packet_send_capacity(),
            stack.config.udp_message_count
        );
        let Response::Udp(UdpResponse::Bound { handle, .. }) = stack.process_udp(
            UdpCommand::Bind {
                endpoint: "192.0.2.1:40002".parse().unwrap(),
            },
            None,
        ) else {
            panic!("ordinary bind must succeed");
        };
        let socket = stack.socket_set.get::<udp::Socket>(handle);
        assert_eq!(socket.payload_recv_capacity(), stack.config.udp_buffer_size);
        assert_eq!(
            socket.packet_recv_capacity(),
            stack.config.udp_message_count
        );
    }

    #[test]
    fn invalid_receive_override_allocates_no_socket() {
        let mut stack = stack();
        for (bytes, packets) in [(0, 1), (1, 0), (128 * 1024 + 1, 1), (1, 513)] {
            assert!(matches!(
                stack.process_udp(
                    UdpCommand::BindWithReceiveBuffer {
                        endpoint: "192.0.2.1:40001".parse().unwrap(),
                        receive_buffer_size: bytes,
                        receive_message_count: packets
                    },
                    None
                ),
                Response::Error(_)
            ));
            assert_eq!(stack.socket_set.iter().count(), 0);
        }
    }

    #[test]
    fn cancelled_receive_override_bind_reclaims_queued_response_socket() {
        let mut stack = stack();
        let command = || {
            Command::Udp(UdpCommand::BindWithReceiveBuffer {
                endpoint: "192.0.2.1:40001".parse().unwrap(),
                receive_buffer_size: 128 * 1024,
                receive_message_count: 512,
            })
        };
        let (response, result) = flume::bounded(1);
        drop(result);
        stack.process_one_cmd(Request {
            handle: None,
            command: command(),
            resp: response,
        });
        assert_eq!(stack.socket_set.iter().count(), 0);
        let (response, result) = flume::bounded(1);
        stack.process_one_cmd(Request {
            handle: None,
            command: command(),
            resp: response,
        });
        assert_eq!(stack.socket_set.iter().count(), 1);
        drop(result);
        // The real async request cancellation guard enqueues this wake. A raw
        // flume receiver drop alone cannot wake an otherwise idle actor.
        let (response, _ignored) = flume::bounded(1);
        stack.process_one_cmd(Request {
            handle: None,
            command: Command::ReapCancelled,
            resp: response,
        });
        assert_eq!(stack.socket_set.iter().count(), 0);
    }

}
