use std::future::Future;
use std::pin::Pin;

use tokio::sync::watch;
use usque_core::Transport;
use usque_protocol::PeerNetworkState;

use crate::h2::{H2Driver, H2ReceiveHalf, H2SendHalf, H2Tunnel, TransportError};
use crate::h3::{H3Driver, H3MigrationHandle, H3ReceiveHalf, H3SendHalf, H3Tunnel};
use crate::packet_batch::{PacketBatch, PacketBatchResult};

pub(crate) type BatchSendFuture =
    Pin<Box<dyn Future<Output = Result<PacketBatchResult, TransportError>> + Send + 'static>>;

/// One managed data plane, optionally containing a bounded set of flow-pinned
/// MASQUE connections. Auto replaces the complete set as one owned unit.
pub(crate) enum MasqueTunnel {
    Http3(Box<H3Tunnel>),
    Http2(Box<H2Tunnel>),
    Parallel(Box<crate::parallel_tunnel::ParallelTunnel>),
}

impl MasqueTunnel {
    pub(crate) fn control_state(&self) -> PeerNetworkState {
        match self {
            Self::Http3(tunnel) => tunnel.control_state(),
            Self::Http2(tunnel) => tunnel.control_state(),
            Self::Parallel(tunnel) => tunnel.control_state(),
        }
    }
    pub(crate) fn packet_budget(
        &self,
    ) -> Option<watch::Receiver<Option<crate::tcp_packet_budget::H3PacketBudget>>> {
        match self {
            Self::Http3(tunnel) => Some(tunnel.packet_budget()),
            Self::Http2(_) => None,
            Self::Parallel(tunnel) => tunnel.packet_budget(),
        }
    }
    pub(crate) fn transport(&self) -> Transport {
        match self {
            Self::Http3(_) => Transport::Http3,
            Self::Http2(_) => Transport::Http2,
            Self::Parallel(tunnel) => tunnel.transport(),
        }
    }

    pub(crate) fn activate_network_quality(&self) {
        match self {
            Self::Http2(tunnel) => tunnel.activate_network_quality(),
            Self::Http3(tunnel) => tunnel.activate_network_quality(),
            Self::Parallel(tunnel) => tunnel.activate_network_quality(),
        }
    }

    pub(crate) fn migration_handle(&self) -> Option<H3MigrationHandle> {
        match self {
            Self::Http3(tunnel) => {
                let handle = tunnel.migration_handle();
                handle.enabled().then_some(handle)
            }
            Self::Http2(_) => None,
            // A physical generation change replaces every member together.
            Self::Parallel(_) => None,
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
                    MasqueReceiveHalf::Http2(Box::new(receive)),
                    MasqueDriver::Http2(driver),
                    Some(control),
                )
            }
            Self::Parallel(tunnel) => tunnel.into_parts(),
        }
    }
}

#[async_trait::async_trait]
impl crate::endpoint_race::RaceConnection for MasqueTunnel {
    async fn shutdown(self) {
        match self {
            Self::Http3(tunnel) => tunnel.shutdown().await,
            Self::Http2(tunnel) => tunnel.shutdown().await,
            Self::Parallel(tunnel) => tunnel.shutdown().await,
        }
    }

    fn is_alive(&self) -> bool {
        match self {
            Self::Http3(tunnel) => tunnel.is_alive(),
            Self::Http2(tunnel) => tunnel.is_alive(),
            Self::Parallel(tunnel) => tunnel.is_alive(),
        }
    }
}

#[async_trait::async_trait]
impl crate::endpoint_race::RaceConnection for (MasqueTunnel, usque_core::AddressFamily) {
    async fn shutdown(self) {
        crate::endpoint_race::RaceConnection::shutdown(self.0).await;
    }
    fn is_alive(&self) -> bool {
        crate::endpoint_race::RaceConnection::is_alive(&self.0)
    }
}

pub(crate) enum MasqueSendHalf {
    Http3(H3SendHalf),
    Http2(H2SendHalf),
    Parallel(crate::parallel_tunnel::ParallelSendHalf),
}

impl MasqueSendHalf {
    pub(crate) fn start_owned_batch(&self, batch: PacketBatch) -> BatchSendFuture {
        match self {
            Self::Http3(send) => send.start_owned_batch(batch),
            Self::Http2(send) => send.start_owned_batch(batch),
            Self::Parallel(send) => send.start_owned_batch(batch),
        }
    }

    pub(crate) fn close(&mut self) {
        match self {
            Self::Http3(send) => send.close(),
            Self::Http2(send) => send.close(),
            Self::Parallel(send) => send.close(),
        }
    }
}

pub(crate) enum MasqueReceiveHalf {
    Http3(H3ReceiveHalf),
    Http2(Box<H2ReceiveHalf>),
    Parallel(tokio::sync::mpsc::Receiver<PacketBatch>),
}

impl MasqueReceiveHalf {
    pub(crate) async fn receive_batch(&mut self) -> Result<PacketBatch, TransportError> {
        match self {
            Self::Http3(receive) => receive.receive_batch().await,
            Self::Http2(receive) => receive.receive_batch().await,
            Self::Parallel(receive) => receive.recv().await.ok_or(TransportError::TunnelClosed),
        }
    }
}

pub(crate) enum MasqueDriver {
    Http3(H3Driver),
    Http2(H2Driver),
    Parallel(crate::parallel_tunnel::ParallelDriver),
}

impl MasqueDriver {
    pub(crate) fn abort_handle(&self) -> tokio::task::AbortHandle {
        match self {
            Self::Http3(driver) => driver.abort_handle(),
            Self::Http2(driver) => driver.abort_handle(),
            Self::Parallel(driver) => driver.abort_handle(),
        }
    }
    pub(crate) async fn wait(self) -> Result<(), TransportError> {
        match self {
            Self::Http3(driver) => driver.wait().await,
            Self::Http2(driver) => driver.wait().await,
            Self::Parallel(driver) => driver.wait().await,
        }
    }
}
