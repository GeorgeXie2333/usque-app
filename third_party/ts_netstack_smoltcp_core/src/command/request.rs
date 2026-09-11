//! Helpers to simplify sending requests.

use core::borrow::Borrow;

use smoltcp::iface::SocketHandle;

use crate::{
    Error,
    command::{Command, Request, Response},
};

/// Helper trait to abstract over [`flume::Sender`] and [`flume::WeakSender`].
pub trait UpgradableChannel {
    /// Attempt to upgrade this value to [`flume::Sender`].
    fn upgrade(&self) -> Result<impl Borrow<flume::Sender<Request>>, ChannelClosedError>;
}

/// An error signifying that the remote end of the channel has closed.
#[derive(Debug, thiserror::Error, Clone, Copy, Eq, PartialEq)]
#[error("the remote end of the channel has closed")]
pub struct ChannelClosedError;

impl From<ChannelClosedError> for Error {
    fn from(_: ChannelClosedError) -> Self {
        Error::Internal(crate::InternalErrorKind::InternalChannelClosed)
    }
}

#[cfg(feature = "std")]
impl From<ChannelClosedError> for std::io::Error {
    fn from(_: ChannelClosedError) -> Self {
        std::io::ErrorKind::BrokenPipe.into()
    }
}

impl UpgradableChannel for flume::Sender<Request> {
    fn upgrade(&self) -> Result<impl Borrow<flume::Sender<Request>>, ChannelClosedError> {
        Ok(self)
    }
}

impl UpgradableChannel for flume::WeakSender<Request> {
    fn upgrade(&self) -> Result<impl Borrow<flume::Sender<Request>>, ChannelClosedError> {
        flume::WeakSender::<Request>::upgrade(self).ok_or(ChannelClosedError)
    }
}

impl<T> UpgradableChannel for &T
where
    T: UpgradableChannel + ?Sized,
{
    fn upgrade(&self) -> Result<impl Borrow<flume::Sender<Request>>, ChannelClosedError> {
        T::upgrade(self)
    }
}

/// Synchronously make a request over the given command channel.
///
/// Blocks on command submission and on receipt of a [`Response`].
pub fn request_blocking(
    command_tx: impl UpgradableChannel,
    handle: Option<SocketHandle>,
    command: impl Into<Command>,
) -> Result<Response, ChannelClosedError> {
    // wrapper to minimize monomorphization impact
    fn _request_blocking(
        command_tx: &flume::Sender<Request>,
        handle: Option<SocketHandle>,
        command: Command,
    ) -> Result<Response, ChannelClosedError> {
        let (resp_tx, resp_rx) = flume::bounded(1);

        command_tx
            .send(Request {
                handle,
                command,
                resp: resp_tx,
            })
            .map_err(|_| ChannelClosedError)?;

        resp_rx.recv().map_err(|_| ChannelClosedError)
    }

    let ch = command_tx.upgrade()?;
    let ch = ch.borrow();

    _request_blocking(ch, handle, command.into())
}

/// Make a request over the given command channel.
///
/// Blocks on command submission and the receipt of a [`Response`].
pub fn request(
    command_tx: impl UpgradableChannel,
    handle: Option<SocketHandle>,
    command: impl Into<Command>,
) -> impl Future<Output = Result<Response, Error>> + Send {
    // wrapper to minimize monomorphization impact
    async fn _request(
        command_tx: &flume::Sender<Request>,
        handle: Option<SocketHandle>,
        command: Command,
    ) -> Result<Response, Error> {
        let (resp_tx, resp_rx) = flume::bounded(1);

        command_tx
            .send_async(Request {
                handle,
                command,
                resp: resp_tx,
            })
            .await?;

        resp_rx.recv_async().await.map_err(Error::from)
    }

    // impl Future and the returned async block below are required to do this bit of work upgrading
    // the channel and converting the command outside the async context for lifetime reasons
    let ch = command_tx.upgrade().map(|x| x.borrow().clone());
    let command = command.into();

    async move {
        let ch = ch?;
        _request(&ch, handle, command).await
    }
}

/// Send a request without blocking on the response.
///
/// Returns an error only if the request can't be sent.
///
/// Mainly intended for use in [`Drop`] implementations, as they can't be `async`
/// and it would be frequently be surprising if they blocked the calling thread.
pub fn request_nonblocking(
    command_tx: impl UpgradableChannel,
    handle: Option<SocketHandle>,
    command: impl Into<Command>,
) -> Result<(), ChannelClosedError> {
    // wrapper to minimize monomorphization impact
    fn _request_nonblocking(
        command_tx: &flume::Sender<Request>,
        handle: Option<SocketHandle>,
        command: Command,
    ) -> Result<(), ChannelClosedError> {
        let (resp_tx, _resp_rx) = flume::bounded(1);

        match command_tx.try_send(Request {
            handle,
            command,
            resp: resp_tx,
        }) {
            Ok(_) | Err(flume::TrySendError::Full(_)) => Ok(()),
            Err(flume::TrySendError::Disconnected(_)) => Err(ChannelClosedError),
        }
    }

    let ch = command_tx.upgrade()?;
    let ch = ch.borrow();

    _request_nonblocking(ch, handle, command.into())
}

/// Failure to enqueue a request. Unlike the legacy best-effort drop helper,
/// this distinguishes saturation from a closed stack and never reports a drop
/// as success. A cleanup owner can retry `Full` asynchronously.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum TryRequestError {
    /// The live command queue has no free slot; the request was not enqueued.
    Full,
    /// The stack no longer accepts commands.
    Closed,
}

/// Try to enqueue exactly one request without blocking or silently dropping it.
pub fn try_request_nonblocking(
    command_tx: impl UpgradableChannel,
    handle: Option<SocketHandle>,
    command: impl Into<Command>,
) -> Result<(), TryRequestError> {
    let channel = command_tx.upgrade().map_err(|_| TryRequestError::Closed)?;
    let (resp, _receiver) = flume::bounded(1);
    match channel.borrow().try_send(Request {
        handle,
        command: command.into(),
        resp,
    }) {
        Ok(()) => Ok(()),
        Err(flume::TrySendError::Full(_)) => Err(TryRequestError::Full),
        Err(flume::TrySendError::Disconnected(_)) => Err(TryRequestError::Closed),
    }
}

#[cfg(test)]
mod enqueue_tests {
    use super::*;

    #[test]
    fn cleanup_queue_full_is_distinct_from_success_and_closed() {
        for capacity in [1, 256] {
            let (sender, receiver) = flume::bounded(capacity);
            for _ in 0..capacity {
                assert_eq!(
                    try_request_nonblocking(&sender, None, crate::tcp::stream::Command::Close),
                    Ok(())
                );
            }
            assert_eq!(
                try_request_nonblocking(&sender, None, crate::tcp::stream::Command::Abort),
                Err(TryRequestError::Full)
            );
            receiver.try_recv().unwrap();
            assert_eq!(
                try_request_nonblocking(&sender, None, crate::tcp::stream::Command::Abort),
                Ok(())
            );
            drop(receiver);
            assert_eq!(
                try_request_nonblocking(&sender, None, crate::tcp::stream::Command::Close),
                Err(TryRequestError::Closed)
            );
        }
    }
}
