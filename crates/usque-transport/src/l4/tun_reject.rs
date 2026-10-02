//! One bounded ICMP rejection budget shared by the TUN pump and UDP workers.
use super::L4Metrics;
use super::performance::MeasuredSender;
use crate::direct_gateway::NatPacket;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub(super) struct UdpRejector {
    replies: MeasuredSender,
    metrics: Arc<L4Metrics>,
    cancellation: CancellationToken,
    window: Arc<Mutex<(Instant, u16)>>,
}
impl UdpRejector {
    pub(super) fn new(
        replies: MeasuredSender,
        metrics: Arc<L4Metrics>,
        cancellation: CancellationToken,
    ) -> Self {
        Self {
            replies,
            metrics,
            cancellation,
            window: Arc::new(Mutex::new((Instant::now(), 0))),
        }
    }
    pub(super) fn reject(&self, packet: &[u8], meta: &NatPacket) {
        if self.cancellation.is_cancelled() {
            return;
        }
        self.metrics.update(|m| m.udp_rejected += 1);
        let mut window = self.window.lock().unwrap_or_else(|e| e.into_inner());
        if window.0.elapsed() >= Duration::from_secs(1) {
            *window = (Instant::now(), 0);
        }
        if window.1 < 32
            && !self.cancellation.is_cancelled()
            && let Some(reply) = super::tun_wire::udp_unreachable(packet, meta)
        {
            window.1 += 1;
            let _ = self.replies.try_send(reply);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;

    #[tokio::test(start_paused = true)]
    async fn pump_and_workers_share_one_rate_limit_and_cancel_stops_replies() {
        let (sender, mut replies) = mpsc::channel(64);
        let cancellation = CancellationToken::new();
        let metrics = Arc::new(L4Metrics::default());
        let pump = UdpRejector::new(
            MeasuredSender::new(sender, Arc::default()),
            metrics.clone(),
            cancellation.clone(),
        );
        let mut udp = vec![0; 8];
        udp[..2].copy_from_slice(&42000_u16.to_be_bytes());
        udp[2..4].copy_from_slice(&443_u16.to_be_bytes());
        udp[4..6].copy_from_slice(&8_u16.to_be_bytes());
        let packet = super::super::tun_wire::ip_packet(
            "192.0.2.1".parse().unwrap(),
            "203.0.113.1".parse().unwrap(),
            17,
            udp,
        );
        let meta = NatPacket::parse(&packet).unwrap();
        for _ in 0..40 {
            let worker = pump.clone();
            worker.reject(&packet, &meta);
        }
        for _ in 0..32 {
            assert!(replies.try_recv().is_ok());
        }
        assert!(replies.try_recv().is_err());
        tokio::time::advance(Duration::from_secs(1)).await;
        pump.reject(&packet, &meta);
        assert!(replies.try_recv().is_ok());
        cancellation.cancel();
        pump.reject(&packet, &meta);
        assert!(replies.try_recv().is_err());
        assert_eq!(metrics.snapshot().udp_rejected, 41);
    }
}
