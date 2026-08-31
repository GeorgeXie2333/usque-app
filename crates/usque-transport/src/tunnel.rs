use std::future::Future;
use std::pin::Pin;

use tokio::sync::watch;
use usque_core::Transport;
use usque_protocol::PeerNetworkState;

use crate::h2::{H2Driver, H2ReceiveHalf, H2SendHalf, H2Tunnel, TransportError};
use crate::h3::{H3Driver, H3ReceiveHalf, H3SendHalf, H3Tunnel};
use crate::packet_batch::{PacketBatch, PacketBatchResult};

pub(crate) type BatchSendFuture =
    Pin<Box<dyn Future<Output = Result<PacketBatchResult, TransportError>> + Send + 'static>>;

/// One active MASQUE data channel. The enum deliberately has no fan-out or
/// multipath variant: Auto may replace a channel, but only one channel carries
/// packets at a time.
pub(crate) enum MasqueTunnel {
    Http3(H3Tunnel),
    Http2(H2Tunnel),
}

impl MasqueTunnel {
    pub(crate) fn transport(&self) -> Transport {
        match self {
            Self::Http3(_) => Transport::Http3,
            Self::Http2(_) => Transport::Http2,
        }
    }

    pub(crate) fn into_parts(
        self,
    ) -> (
        MasqueSendHalf,
        MasqueReceiveHalf,
        MasqueDriver,
        Option<watch::Receiver<PeerNetworkState>>,
    ) {
        match self {
            Self::Http3(tunnel) => {
                let (send, receive, driver, control) = tunnel.into_parts();
                (
                    MasqueSendHalf::Http3(send),
                    MasqueReceiveHalf::Http3(receive),
                    MasqueDriver::Http3(driver),
                    Some(control),
                )
            }
            Self::Http2(tunnel) => {
                let (send, receive, driver, control) = tunnel.into_parts();
                (
                    MasqueSendHalf::Http2(send),
                    MasqueReceiveHalf::Http2(receive),
                    MasqueDriver::Http2(driver),
                    Some(control),
                )
            }
        }
    }
}

pub(crate) enum MasqueSendHalf {
    Http3(H3SendHalf),
    Http2(H2SendHalf),
}

impl MasqueSendHalf {
    pub(crate) fn start_owned_batch(&self, batch: PacketBatch) -> BatchSendFuture {
        match self {
            Self::Http3(send) => send.start_owned_batch(batch),
            Self::Http2(send) => send.start_owned_batch(batch),
        }
    }

    pub(crate) fn close(&mut self) {
        match self {
            Self::Http3(send) => send.close(),
            Self::Http2(send) => send.close(),
        }
    }
}

pub(crate) enum MasqueReceiveHalf {
    Http3(H3ReceiveHalf),
    Http2(H2ReceiveHalf),
}

impl MasqueReceiveHalf {
    pub(crate) async fn receive_batch(&mut self) -> Result<PacketBatch, TransportError> {
        match self {
            Self::Http3(receive) => receive.receive_batch().await,
            Self::Http2(receive) => receive.receive_batch().await,
        }
    }
}

pub(crate) enum MasqueDriver {
    Http3(H3Driver),
    Http2(H2Driver),
}

impl MasqueDriver {
    pub(crate) async fn wait(self) -> Result<(), TransportError> {
        match self {
            Self::Http3(driver) => driver.wait().await,
            Self::Http2(driver) => driver.wait().await,
        }
    }
}
