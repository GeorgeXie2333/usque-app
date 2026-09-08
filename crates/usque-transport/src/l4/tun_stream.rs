//! Single-owner adapter over the pinned stack's typed command API. Unlike the
//! upstream socket wrapper, shutdown really queues FIN and abort is explicit.
use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
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
    if channel.request_nonblocking(handle, command()).is_err()
        && let Ok(runtime) = tokio::runtime::Handle::try_current()
    {
        // A bounded command queue can be full during a cancellation burst.
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
            let channel = self.channel.clone();
            let handle = self.handle;
            let bytes = Bytes::copy_from_slice(&bytes[..bytes.len().min(super::CHUNK_SIZE)]);
            self.write = Some(Box::pin(async move {
                channel
                    .request(Some(handle), tcp::stream::Command::Send { buf: bytes })
                    .await
            }));
        }
        let result = std::task::ready!(
            self.write
                .as_mut()
                .expect("write request")
                .as_mut()
                .poll(cx)
        );
        self.write = None;
        match result {
            Ok(Response::TcpStream(tcp::stream::Response::Sent { n })) => Poll::Ready(Ok(n)),
            _ => Poll::Ready(Err(io::ErrorKind::ConnectionReset.into())),
        }
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
