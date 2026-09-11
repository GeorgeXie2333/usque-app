//! Single-owner adapter over the pinned stack's typed command API. Unlike the
//! upstream socket wrapper, shutdown really queues FIN and abort is explicit.
use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::{Arc, atomic::Ordering};
use std::task::{Context, Poll};

use bytes::{Buf, Bytes};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use ts_netstack_smoltcp::netcore::{
    Channel, HasChannel, Response, TcpListenerHandle, smoltcp::iface::SocketHandle, tcp,
};

type CommandFuture =
    Pin<Box<dyn Future<Output = Result<Response, ts_netstack_smoltcp::netcore::Error>> + Send>>;

pub(crate) struct OnceListener {
    channel: Channel,
    handle: TcpListenerHandle,
    local: SocketAddr,
    transferred: bool,
}

impl OnceListener {
    pub(crate) async fn bind(channel: Channel, local: SocketAddr) -> io::Result<Self> {
        let result = channel
            .request(
                None,
                tcp::listen::Command::ListenOnce {
                    local_endpoint: local,
                },
            )
            .await
            .map_err(io::Error::other)?;
        match result {
            Response::TcpListen(tcp::listen::Response::Listening { handle }) => Ok(Self {
                channel,
                handle,
                local,
                transferred: false,
            }),
            _ => Err(io::Error::other("TUN listener unavailable")),
        }
    }

    pub(crate) async fn accept(mut self, expected: SocketAddr) -> io::Result<TunStream> {
        let result = self
            .channel
            .request(
                None,
                tcp::listen::Command::Accept {
                    handle: self.handle,
                },
            )
            .await
            .map_err(io::Error::other)?;
        match result {
            Response::TcpListen(tcp::listen::Response::Accepted { handle, remote }) => {
                self.transferred = true;
                let stream = TunStream {
                    listener: self.handle,
                    channel: self.channel.clone(),
                    handle,
                    local: self.local,
                    read: None,
                    write: None,
                    shutdown: None,
                    buffer: Bytes::new(),
                    write_closed: false,
                    performance: None,
                    write_size: 0,
                };
                if remote != expected {
                    return Err(io::Error::other("TUN flow peer mismatch"));
                }
                Ok(stream)
            }
            _ => Err(io::Error::other("TUN accept failed")),
        }
    }
}

impl Drop for OnceListener {
    fn drop(&mut self) {
        if self.transferred {
            return;
        }
        let handle = self.handle;
        cleanup(&self.channel, None, move || {
            tcp::listen::Command::Close { handle }.into()
        });
    }
}

fn cleanup(
    channel: &Channel,
    handle: Option<SocketHandle>,
    command: impl Fn() -> ts_netstack_smoltcp::netcore::Command + Send + 'static,
) {
    if matches!(
        ts_netstack_smoltcp::netcore::try_request_nonblocking(channel, handle, command()),
        Err(ts_netstack_smoltcp::netcore::TryRequestError::Full)
    ) && let Ok(runtime) = tokio::runtime::Handle::try_current()
    {
        // A bounded command queue can be full during a cancellation burst.
        // The legacy request_nonblocking helper reports Full as success.
        // Cleanup must wait for capacity instead of orphaning a live socket.
        // These tasks are bounded by the stack's allocated socket budget; a
        // stopped stack closes the receiver and releases all remaining work.
        let channel = channel.clone();
        runtime.spawn(async move {
            let _ = channel.request(handle, command()).await;
        });
    }
}

pub(crate) struct TunStream {
    listener: TcpListenerHandle,
    channel: Channel,
    handle: SocketHandle,
    local: SocketAddr,
    read: Option<CommandFuture>,
    write: Option<CommandFuture>,
    shutdown: Option<CommandFuture>,
    buffer: Bytes,
    write_closed: bool,
    performance: Option<Arc<super::performance::Performance>>,
    write_size: usize,
}

impl TunStream {
    pub(crate) fn observe(&mut self, performance: Arc<super::performance::Performance>) {
        self.performance = Some(performance);
    }
    fn start_write(&mut self, bytes: Bytes) {
        self.write_size = bytes.len();
        let channel = self.channel.clone();
        let handle = self.handle;
        let performance = self.performance.clone();
        let started = performance.as_ref().and_then(|p| p.command_wait.begin());
        if let Some(p) = &performance {
            p.tcp_write_calls.fetch_add(1, Ordering::Relaxed);
        }
        self.write = Some(Box::pin(async move {
            let result = channel
                .request(Some(handle), tcp::stream::Command::Send { buf: bytes })
                .await;
            if let Some(p) = performance {
                p.command_wait.finish(started);
            }
            result
        }));
    }
    fn poll_sent(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<usize>> {
        let result = std::task::ready!(
            self.write
                .as_mut()
                .expect("write request")
                .as_mut()
                .poll(cx)
        );
        self.write = None;
        match result {
            Ok(Response::TcpStream(tcp::stream::Response::Sent { n })) if n <= self.write_size => {
                if let Some(p) = &self.performance {
                    p.tcp_accepted_bytes.fetch_add(n as u64, Ordering::Relaxed);
                    if n < self.write_size {
                        p.tcp_partial_writes.fetch_add(1, Ordering::Relaxed);
                    }
                }
                Poll::Ready(Ok(n))
            }
            _ => Poll::Ready(Err(io::ErrorKind::ConnectionReset.into())),
        }
    }
}

impl crate::tcp::OwnedTcpWrite for TunStream {
    fn poll_write_owned(&mut self, cx: &mut Context<'_>, bytes: &Bytes) -> Poll<io::Result<usize>> {
        if self.write_closed {
            return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()));
        }
        if bytes.is_empty() {
            return Poll::Ready(Ok(0));
        }
        if self.write.is_none() {
            // Clones only the reference-counted handle, never the payload.
            self.start_write(bytes.slice(..bytes.len().min(super::CHUNK_SIZE)));
        }
        self.poll_sent(cx)
    }
}

impl crate::tcp::TcpIo for TunStream {
    fn local_addr(&self) -> io::Result<SocketAddr> {
        Ok(self.local)
    }
}

impl AsyncRead for TunStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        out: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if out.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        if self.buffer.is_empty() {
            if self.read.is_none() {
                let channel = self.channel.clone();
                let handle = self.handle;
                let size = out.remaining().min(super::CHUNK_SIZE);
                self.read = Some(Box::pin(async move {
                    channel
                        .request(
                            Some(handle),
                            tcp::stream::Command::Recv {
                                max_len: Some(size),
                            },
                        )
                        .await
                }));
            }
            let result =
                std::task::ready!(self.read.as_mut().expect("read request").as_mut().poll(cx));
            self.read = None;
            match result {
                Ok(Response::TcpStream(tcp::stream::Response::Recv { buf })) => self.buffer = buf,
                Ok(Response::TcpStream(tcp::stream::Response::Finished)) => {
                    return Poll::Ready(Ok(()));
                }
                _ => return Poll::Ready(Err(io::ErrorKind::ConnectionReset.into())),
            }
        }
        let n = out.remaining().min(self.buffer.len());
        out.put_slice(&self.buffer[..n]);
        self.buffer.advance(n);
        Poll::Ready(Ok(()))
    }
}

impl AsyncWrite for TunStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        if self.write_closed {
            return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()));
        }
        if bytes.is_empty() {
            return Poll::Ready(Ok(0));
        }
        if self.write.is_none() {
            let bytes = Bytes::copy_from_slice(&bytes[..bytes.len().min(super::CHUNK_SIZE)]);
            if let Some(p) = &self.performance {
                p.adapter_copied_bytes
                    .fetch_add(bytes.len() as u64, Ordering::Relaxed);
            }
            self.start_write(bytes);
        }
        self.poll_sent(cx)
    }
    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if self.write_closed {
            return Poll::Ready(Ok(()));
        }
        if self.shutdown.is_none() {
            let channel = self.channel.clone();
            let handle = self.handle;
            self.shutdown = Some(Box::pin(async move {
                channel
                    .request(Some(handle), tcp::stream::Command::Close)
                    .await
            }));
        }
        let result = std::task::ready!(
            self.shutdown
                .as_mut()
                .expect("shutdown request")
                .as_mut()
                .poll(cx)
        );
        self.shutdown = None;
        match result {
            Ok(Response::Ok) => {
                self.write_closed = true;
                Poll::Ready(Ok(()))
            }
            _ => Poll::Ready(Err(io::ErrorKind::ConnectionReset.into())),
        }
    }
}

impl Drop for TunStream {
    fn drop(&mut self) {
        self.read.take();
        self.write.take();
        self.shutdown.take();
        let handle = self.listener;
        cleanup(&self.channel, None, move || {
            tcp::listen::Command::Close { handle }.into()
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tcp::OwnedTcpWrite;
    use ts_netstack_smoltcp::netcore::{
        Config, Netstack, Request, TcpBufferMetrics, TcpBufferPolicy, TcpBufferTier, flume,
        stack_control, try_request_nonblocking,
    };

    #[tokio::test]
    async fn owned_partial_commands_retain_allocation_and_remove_repeated_adapter_copies() {
        for owned in [false, true] {
            let metrics = Arc::new(super::super::performance::Performance::default());
            let mut stack = Netstack::new(
                Config::default(),
                ts_netstack_smoltcp::netcore::smoltcp::time::Instant::from_millis(0),
            );
            let local: SocketAddr = "127.0.0.1:40001".parse().unwrap();
            let (resp, reply) = flume::bounded(1);
            stack.process_one_cmd(Request {
                handle: None,
                command: tcp::listen::Command::ListenOnce {
                    local_endpoint: local,
                }
                .into(),
                resp,
            });
            let Response::TcpListen(tcp::listen::Response::Listening { handle: listener }) =
                reply.recv().unwrap()
            else {
                panic!("listener");
            };
            let mut sockets = ts_netstack_smoltcp::netcore::smoltcp::iface::SocketSet::new(vec![]);
            let handle =
                sockets.add(
                    ts_netstack_smoltcp::netcore::smoltcp::socket::tcp::Socket::new(
                        ts_netstack_smoltcp::netcore::smoltcp::socket::tcp::SocketBuffer::new(
                            vec![0u8; 1],
                        ),
                        ts_netstack_smoltcp::netcore::smoltcp::socket::tcp::SocketBuffer::new(
                            vec![0u8; 1],
                        ),
                    ),
                );
            let (tx, rx) = flume::bounded::<Request>(1);
            let mut stream = TunStream {
                listener,
                handle,
                local,
                channel: tx.downgrade(),
                read: None,
                write: None,
                shutdown: None,
                buffer: Bytes::new(),
                write_closed: false,
                performance: Some(metrics.clone()),
                write_size: 0,
            };
            let original = Bytes::from((0..32768).map(|i| i as u8).collect::<Vec<_>>());
            let pointer = original.as_ptr() as usize;
            let server = tokio::spawn(async move {
                let mut output = Vec::new();
                while output.len() < 32768 {
                    let request = rx.recv_async().await.unwrap();
                    let ts_netstack_smoltcp::netcore::Command::TcpStream(
                        tcp::stream::Command::Send { buf },
                    ) = request.command
                    else {
                        panic!("send command");
                    };
                    if owned {
                        assert_eq!(buf.as_ptr() as usize, pointer + output.len());
                    }
                    let n = buf.len().min(1024);
                    output.extend_from_slice(&buf[..n]);
                    request
                        .resp
                        .send(Response::TcpStream(tcp::stream::Response::Sent { n }))
                        .unwrap();
                }
                output
            });
            let mut remaining = original.clone();
            while !remaining.is_empty() {
                let n = std::future::poll_fn(|cx| {
                    if owned {
                        stream.poll_write_owned(cx, &remaining)
                    } else {
                        Pin::new(&mut stream).poll_write(cx, &remaining)
                    }
                })
                .await
                .unwrap();
                remaining.advance(n);
            }
            assert_eq!(server.await.unwrap(), original);
            let p = metrics.sample();
            assert_eq!(p.tcp_accepted_bytes, 32768);
            assert_eq!((p.tcp_write_calls, p.tcp_partial_writes), (32, 31));
            assert_eq!(
                p.adapter_copied_bytes,
                if owned {
                    0
                } else {
                    1024 * (1..=32).sum::<u64>()
                }
            );
        }
    }

    #[tokio::test]
    async fn full_command_queue_cleanup_releases_listener_without_network_progress() {
        let metrics = TcpBufferMetrics::default();
        let config = Config {
            command_channel_capacity: Some(1),
            tcp_buffer_metrics: Some(metrics.clone()),
            tcp_buffer_policy: Some(TcpBufferPolicy {
                preferred: TcpBufferTier {
                    receive: 16384,
                    transmit: 16384,
                },
                fallback: TcpBufferTier {
                    receive: 16384,
                    transmit: 16384,
                },
                preferred_budget: 32768,
                total_budget: 32768,
            }),
            ..Config::default()
        };
        let mut stack = Netstack::new(
            config,
            ts_netstack_smoltcp::netcore::smoltcp::time::Instant::from_millis(0),
        );
        let channel = stack.command_channel();
        for port in [40001, 40002] {
            let local = SocketAddr::from(([127, 0, 0, 1], port));
            let (resp, result) = flume::bounded(1);
            stack.process_one_cmd(Request {
                handle: None,
                command: tcp::listen::Command::ListenOnce {
                    local_endpoint: local,
                }
                .into(),
                resp,
            });
            let Response::TcpListen(tcp::listen::Response::Listening { handle }) =
                result.try_recv().unwrap()
            else {
                panic!("listener must fit after previous cleanup");
            };
            assert_eq!(metrics.snapshot().total_bytes, 32768);
            try_request_nonblocking(
                &channel,
                None,
                stack_control::Command::SetIps { new_ips: vec![] },
            )
            .unwrap();
            drop(OnceListener {
                channel: channel.clone(),
                handle,
                local,
                transferred: false,
            });
            tokio::task::yield_now().await;
            assert_eq!(metrics.snapshot().total_bytes, 32768);
            tokio::time::timeout(std::time::Duration::from_secs(1), async {
                while metrics.snapshot().total_bytes != 0 {
                    stack.process_cmds();
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("cleanup must retry a full command queue and reclaim without I/O");
        }
    }
}
