//! Bounded, cancellation-safe packet pipe for the local TCP stacks.
//!
//! A smoltcp egress pass can emit several packets after one readiness poll.
//! Every TxToken therefore owns its queue slot before smoltcp advances TCP.
//! There is no blocking send and no queue-full drop after accepting TCP bytes.
use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::{Bytes, BytesMut};
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
        let (a, b) = mpsc::channel(capacity);
        let (c, d) = mpsc::channel(capacity);
        (
            Self {
                tx: PacketSender(a),
                rx: PacketReceiver(d),
            },
            Self {
                tx: PacketSender(c),
                rx: PacketReceiver(b),
            },
        )
    }
}

#[derive(Clone)]
pub(crate) struct PacketSender(mpsc::Sender<Bytes>);
impl PacketSender {
    pub(crate) async fn send_async(&self, packet: &[u8]) {
        let _ = self.0.send(Bytes::copy_from_slice(packet)).await;
    }

    #[cfg(test)]
    pub(crate) fn try_send(&self, packet: &[u8]) -> bool {
        self.0.try_send(Bytes::copy_from_slice(packet)).is_ok()
    }
}

pub(crate) struct PacketReceiver(mpsc::Receiver<Bytes>);
impl PacketReceiver {
    pub(crate) fn rx_ready(&self) -> bool {
        !self.0.is_empty()
    }

    pub(crate) async fn recv_async(&mut self) -> Option<Bytes> {
        self.0.recv().await
    }
    pub(crate) fn try_recv(&mut self) -> Option<Bytes> {
        self.0.try_recv().ok()
    }
}

pub(crate) struct PacketDevice {
    tx: mpsc::Sender<Bytes>,
    tx_waiter: PollSender<Bytes>,
    rx: mpsc::Receiver<Bytes>,
    received: Option<Bytes>,
    mtu: usize,
}

impl PacketDevice {
    pub(crate) fn new(pipe: PacketPipe, mtu: usize) -> Self {
        Self {
            tx_waiter: PollSender::new(pipe.tx.0.clone()),
            tx: pipe.tx.0,
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

pub(crate) struct PacketTx(mpsc::OwnedPermit<Bytes>);
impl ts_netstack_smoltcp::netcore::smoltcp::phy::TxToken for PacketTx {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        let mut packet = BytesMut::zeroed(len);
        let result = f(&mut packet);
        self.0.send(packet.freeze());
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
        Some((PacketRx(packet), PacketTx(permit)))
    }

    fn transmit(&mut self, _timestamp: Instant) -> Option<PacketTx> {
        self.tx.clone().try_reserve_owned().ok().map(PacketTx)
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
