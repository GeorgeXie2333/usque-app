//! Single-owner duplex relay. Downstream holds one owned DATA chunk; upload
//! keeps the existing bounded relay allocation. Neither side can hog a poll.
use crate::tcp::{OwnedTcpWrite, TcpIo};
use bytes::{Buf, Bytes};
use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};
use tokio::io::ReadBuf;

#[derive(Default)]
struct Download {
    remaining: Bytes,
    eof: bool,
    closed: bool,
}
impl Download {
    fn poll(
        &mut self,
        cx: &mut Context<'_>,
        local: &mut impl OwnedTcpWrite,
        remote: &mut dyn TcpIo,
    ) -> Poll<io::Result<()>> {
        if self.closed {
            return Poll::Pending;
        }
        if !self.remaining.is_empty() {
            let n = std::task::ready!(local.poll_write_owned(cx, &self.remaining))?;
            if n == 0 || n > self.remaining.len() {
                return Poll::Ready(Err(io::ErrorKind::WriteZero.into()));
            }
            self.remaining.advance(n);
        } else if self.eof {
            std::task::ready!(Pin::new(local).poll_shutdown(cx))?;
            self.closed = true;
        } else {
            self.remaining = std::task::ready!(remote.poll_read_owned(cx))?;
            self.eof = self.remaining.is_empty();
        }
        Poll::Ready(Ok(()))
    }
}

pub(super) async fn copy_owned(
    local: &mut impl OwnedTcpWrite,
    remote: &mut dyn TcpIo,
    upload_capacity: usize,
) -> io::Result<()> {
    let mut download = Download::default();
    let mut upload = vec![0u8; upload_capacity];
    let (mut start, mut end) = (0, 0);
    let (mut eof, mut closed) = (false, false);
    std::future::poll_fn(|cx| {
        for _ in 0..16 {
            let down = download.poll(cx, local, remote);
            if let Poll::Ready(Err(error)) = down {
                return Poll::Ready(Err(error));
            }
            let up = if closed {
                Poll::Pending
            } else if start != end {
                match Pin::new(&mut *remote).poll_write(cx, &upload[start..end]) {
                    Poll::Ready(Ok(0)) => return Poll::Ready(Err(io::ErrorKind::WriteZero.into())),
                    Poll::Ready(Ok(n)) => {
                        start += n;
                        Poll::Ready(Ok(()))
                    }
                    other => other.map_ok(|_| ()),
                }
            } else if eof {
                match Pin::new(&mut *remote).poll_shutdown(cx) {
                    Poll::Ready(Ok(())) => {
                        closed = true;
                        Poll::Ready(Ok(()))
                    }
                    other => other,
                }
            } else {
                let mut buf = ReadBuf::new(&mut upload);
                match Pin::new(&mut *local).poll_read(cx, &mut buf) {
                    Poll::Ready(Ok(())) => {
                        start = 0;
                        end = buf.filled().len();
                        eof = end == 0;
                        Poll::Ready(Ok(()))
                    }
                    other => other,
                }
            };
            if let Poll::Ready(Err(error)) = up {
                return Poll::Ready(Err(error));
            }
            if closed && download.closed {
                return Poll::Ready(Ok(()));
            }
            if up.is_pending() && down.is_pending() {
                return Poll::Pending;
            }
        }
        cx.waker().wake_by_ref();
        Poll::Pending
    })
    .await
}
