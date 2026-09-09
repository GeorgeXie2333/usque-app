//! Bounded, cancellation-safe packet pipe for the local TCP stacks.
//!
//! A smoltcp egress pass can emit several packets after one readiness poll.
//! Every TxToken therefore owns its queue slot before smoltcp advances TCP.
//! There is no blocking send and no queue-full drop after accepting TCP bytes.
use std::pin::Pin;
use std::task::{Context, Poll};

use crate::l4::performance::{QueuePerformance, QueuedPacket};
use bytes::{Bytes, BytesMut};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::PollSender;
use ts_netstack_smoltcp::netcore::{
    AsyncWakeDevice,
    smoltcp::{
        phy::{ChecksumCapabilities, Device, DeviceCapabilities, Medium},
        time::Instant,
    },
};

pub(crate) struct PacketPipe {
    pub(crate) tx: PacketSender,
    pub(crate) rx: PacketReceiver,
}

impl PacketPipe {
    pub(crate) fn bounded(capacity: usize) -> (Self, Self) {
        Self::observed(capacity, None, None)
    }

    pub(crate) fn observed(
        capacity: usize,
        to_first: Option<Arc<QueuePerformance>>,
        from_first: Option<Arc<QueuePerformance>>,
    ) -> (Self, Self) {
        let (a, b) = mpsc::channel(capacity);
        let (c, d) = mpsc::channel(capacity);
        (
            Self {
                tx: PacketSender {
                    sender: a,
                    meter: from_first,
                },
                rx: PacketReceiver(d),
            },
            Self {
                tx: PacketSender {
                    sender: c,
                    meter: to_first,
                },
                rx: PacketReceiver(b),
            },
        )
    }
}

#[derive(Clone)]
pub(crate) struct PacketSender {
    sender: mpsc::Sender<QueuedPacket>,
    meter: Option<Arc<QueuePerformance>>,
}
impl PacketSender {
    pub(crate) async fn send_async(&self, packet: &[u8]) {
        if let Ok(permit) = self.sender.reserve().await {
            permit.send(QueuedPacket::new(
                Bytes::copy_from_slice(packet),
                self.meter.as_ref(),
            ));
        }
    }

    #[cfg(test)]
    pub(crate) fn try_send(&self, packet: &[u8]) -> bool {
        match self.sender.try_reserve() {
            Ok(permit) => {
                permit.send(QueuedPacket::new(
                    Bytes::copy_from_slice(packet),
                    self.meter.as_ref(),
                ));
                true
            }
            Err(_) => false,
        }
    }
}

pub(crate) struct PacketReceiver(mpsc::Receiver<QueuedPacket>);
impl PacketReceiver {
    pub(crate) fn rx_ready(&self) -> bool {
        !self.0.is_empty()
    }

    pub(crate) async fn recv_async(&mut self) -> Option<Bytes> {
        self.0.recv().await.map(QueuedPacket::into_bytes)
    }
    pub(crate) fn try_recv(&mut self) -> Option<Bytes> {
        self.0.try_recv().ok().map(QueuedPacket::into_bytes)
    }
}

pub(crate) struct PacketDevice {
    tx: mpsc::Sender<QueuedPacket>,
    tx_meter: Option<Arc<QueuePerformance>>,
    tx_waiter: PollSender<QueuedPacket>,
    rx: mpsc::Receiver<QueuedPacket>,
    received: Option<QueuedPacket>,
    mtu: usize,
}

impl PacketDevice {
    pub(crate) fn new(pipe: PacketPipe, mtu: usize) -> Self {
        Self {
            tx_waiter: PollSender::new(pipe.tx.sender.clone()),
            tx: pipe.tx.sender,
            tx_meter: pipe.tx.meter,
            rx: pipe.rx.0,
            received: None,
            mtu,
        }
    }
}

impl AsyncWakeDevice for PacketDevice {
    fn poll_rx(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        if self.received.is_none() {
            match self.rx.poll_recv(cx) {
                Poll::Ready(Some(packet)) => self.received = Some(packet),
                Poll::Ready(None) | Poll::Pending => return Poll::Pending,
            }
        }
        self.poll_tx(cx)
    }

    fn poll_tx(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        match self.tx_waiter.poll_reserve(cx) {
            Poll::Ready(Ok(())) => {
                // This poll installs the capacity waker only. Each actual
                // TxToken below reserves its own slot, including later packets
                // emitted by the same synchronous smoltcp egress pass.
                self.tx_waiter.abort_send();
                Poll::Ready(())
            }
            Poll::Ready(Err(_)) | Poll::Pending => Poll::Pending,
        }
    }
}

pub(crate) struct PacketTx(
    mpsc::OwnedPermit<QueuedPacket>,
    Option<Arc<QueuePerformance>>,
);
impl ts_netstack_smoltcp::netcore::smoltcp::phy::TxToken for PacketTx {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        let mut packet = BytesMut::zeroed(len);
        let result = f(&mut packet);
        self.0
            .send(QueuedPacket::new(packet.freeze(), self.1.as_ref()));
        result
    }
}

pub(crate) struct PacketRx(Bytes);
impl ts_netstack_smoltcp::netcore::smoltcp::phy::RxToken for PacketRx {
    fn consume<R, F>(self, f: F) -> R
    where
        F: FnOnce(&[u8]) -> R,
    {
        f(&self.0)
    }
}

impl Device for PacketDevice {
    type RxToken<'a> = PacketRx;
    type TxToken<'a> = PacketTx;

    fn receive(&mut self, _timestamp: Instant) -> Option<(PacketRx, PacketTx)> {
        let permit = self.tx.clone().try_reserve_owned().ok()?;
        let packet = self.received.take().or_else(|| self.rx.try_recv().ok())?;
        Some((
            PacketRx(packet.into_bytes()),
            PacketTx(permit, self.tx_meter.clone()),
        ))
    }

    fn transmit(&mut self, _timestamp: Instant) -> Option<PacketTx> {
        self.tx
            .clone()
            .try_reserve_owned()
            .ok()
            .map(|permit| PacketTx(permit, self.tx_meter.clone()))
    }

    fn capabilities(&self) -> DeviceCapabilities {
        let mut caps = DeviceCapabilities::default();
        caps.max_transmission_unit = self.mtu;
        caps.medium = Medium::Ip;
        caps.checksum = ChecksumCapabilities::ignored();
        caps
    }
}

#[cfg(test)]
mod tests;
