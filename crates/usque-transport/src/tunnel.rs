use bytes::Bytes;
use tokio::sync::watch;
use usque_core::Transport;
use usque_protocol::PeerNetworkState;

use crate::h2::{H2Driver, H2ReceiveHalf, H2SendHalf, H2Tunnel, TransportError};
use crate::h3::{H3Driver, H3ReceiveHalf, H3SendHalf, H3Tunnel};

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
    pub(crate) async fn send_owned_packet(&mut self, packet: Bytes) -> Result<(), TransportError> {
        match self {
            Self::Http3(send) => send.send_owned_packet(packet).await,
            Self::Http2(send) => send.send_packet(&packet).await,
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
    pub(crate) async fn receive_packet(&mut self) -> Result<Bytes, TransportError> {
        match self {
            Self::Http3(receive) => receive.receive_packet().await,
            Self::Http2(receive) => receive.receive_packet().await,
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
