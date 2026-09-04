//! Deterministic, test-only transport fault orchestration.
//!
//! Production builds exclude this module. Explicit lab builds must retain debug
//! assertions. The model harness never touches sockets; instance-local hooks
//! can drive the real loopback data plane, never platform network mutation.

use std::{
    collections::VecDeque,
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::Duration,
};

use async_trait::async_trait;
use usque_core::{
    AddressFamily, Transport, TransportFailure, TransportFailureCode, TransportStage,
};

pub trait TransportClock: Send + Sync {
    fn elapsed(&self) -> Duration;
}

#[async_trait]
pub trait EndpointResolver: Send + Sync {
    async fn resolve(&self) -> Result<Vec<SocketAddr>, TransportFailure>;
}

pub trait NetworkGenerationSource: Send + Sync {
    fn generation(&self) -> u64;
}

#[async_trait]
pub trait SocketFactory: Send + Sync {
    async fn create(&self, datagram: bool) -> Result<(), TransportFailure>;
}

#[async_trait]
pub trait ConnectorFactory: Send + Sync {
    async fn connect(&self, http3: bool) -> Result<(), TransportFailure>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultKind {
    DropAllUdp,
    DelayPackets(Duration),
    ReorderDatagrams,
    DuplicateDatagrams,
    BlackholeAfter(u64),
    NetworkChange(u64),
    RemoveIpv4,
    RemoveIpv6,
    StallReads,
    StallWrites,
    FailSocketProtect,
    FailDnsAttempts(u8),
    FailFirstH3Handshake,
    StallQuicHandshake,
    DisableH3Datagrams,
    FailDnsApply,
    FailRouteApply,
    FillSendQueue,
    ForceH2GoAway,
    ForceH2StreamReset,
    DisconnectAgentIpc,
    TerminateVpnProcess,
    EndpointPinMismatch,
    H2PingTimeout,
    H2CapacityWait(Duration),
    SendMmsgPartial(u8),
    SendMmsgUnsupported,
    RecvMmsgTruncated,
    RecvMmsgWouldBlock,
    BufferPoolExhausted,
    PeerCidUnavailable,
    PathValidationTimeout,
    PathValidationRejected,
    GenerationDuringCandidateSetup,
    SendMessageTooLarge,
    PmtuRevalidationFailure,
    DohTlsFailure,
    DohHttpFailure,
    DohBodyFailure,
    DotPrefixFailure,
    DotEof,
    DnsPoolCancellation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FaultPoint {
    H2Ping,
    H2Capacity,
    UdpSend,
    UdpReceive,
    CandidateCid,
    CandidateSetup,
    CandidateValidation,
    Pmtu,
    DnsTls,
    DohHttp,
    DohBody,
    DotPrefix,
    DotBody,
    DnsPool,
}

impl FaultKind {
    fn point(self) -> Option<FaultPoint> {
        Some(match self {
            Self::H2PingTimeout => FaultPoint::H2Ping,
            Self::H2CapacityWait(_) => FaultPoint::H2Capacity,
            Self::SendMmsgPartial(_) | Self::SendMmsgUnsupported | Self::SendMessageTooLarge => {
                FaultPoint::UdpSend
            }
            Self::RecvMmsgTruncated | Self::RecvMmsgWouldBlock | Self::BufferPoolExhausted => {
                FaultPoint::UdpReceive
            }
            Self::PeerCidUnavailable => FaultPoint::CandidateCid,
            Self::GenerationDuringCandidateSetup => FaultPoint::CandidateSetup,
            Self::PathValidationTimeout | Self::PathValidationRejected => {
                FaultPoint::CandidateValidation
            }
            Self::PmtuRevalidationFailure => FaultPoint::Pmtu,
            Self::DohTlsFailure => FaultPoint::DnsTls,
            Self::DohHttpFailure => FaultPoint::DohHttp,
            Self::DohBodyFailure => FaultPoint::DohBody,
            Self::DotPrefixFailure => FaultPoint::DotPrefix,
            Self::DotEof => FaultPoint::DotBody,
            Self::DnsPoolCancellation => FaultPoint::DnsPool,
            _ => return None,
        })
    }
}

/// Per-runtime, not global. Each event is consumed at most once; clones share
/// the same bounded script and Tokio test/lab clock.
#[derive(Clone, Debug)]
pub(crate) struct NetworkFaults {
    started: tokio::time::Instant,
    events: Arc<Mutex<Vec<ScheduledFault>>>,
}

impl NetworkFaults {
    pub(crate) fn new(script: FaultScript) -> Self {
        Self {
            started: tokio::time::Instant::now(),
            events: Arc::new(Mutex::new(script.events)),
        }
    }

    pub(crate) fn take(&self, point: FaultPoint) -> Option<FaultKind> {
        let mut events = self.events.lock().expect("fault script mutex poisoned");
        let elapsed = self.started.elapsed();
        let index = events
            .iter()
            .position(|event| event.at <= elapsed && event.fault.point() == Some(point))?;
        Some(events.remove(index).fault)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScheduledFault {
    pub at: Duration,
    pub fault: FaultKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FaultScript {
    seed: u64,
    events: Vec<ScheduledFault>,
}

impl FaultScript {
    pub const MAX_EVENTS: usize = 256;

    pub fn new(seed: u64, mut events: Vec<ScheduledFault>) -> Result<Self, &'static str> {
        if events.len() > Self::MAX_EVENTS {
            return Err("fault script exceeds the bounded event capacity");
        }
        if events.iter().any(|event| {
            matches!(event.fault, FaultKind::SendMmsgPartial(0 | 65..=u8::MAX))
                || matches!(event.fault,
            FaultKind::H2CapacityWait(delay) if delay > Duration::from_secs(10))
        }) {
            return Err("fault exceeds the bounded injection limit");
        }
        events.sort_by_key(|event| event.at);
        Ok(Self { seed, events })
    }

    pub const fn seed(&self) -> u64 {
        self.seed
    }
}

#[derive(Debug)]
pub struct FaultHarness {
    pending: VecDeque<ScheduledFault>,
    active: Vec<FaultKind>,
    seed: u64,
    packets_seen: u64,
    h3_attempts: u32,
    h2_attempts: u32,
    dns_failures_remaining: u8,
    network_generation: u64,
    send_queue_capacity: usize,
    send_queue_depth: usize,
    send_queue_high_watermark: usize,
    send_queue_waits: u64,
    send_queue_drops: u64,
}

impl FaultHarness {
    pub fn new(script: FaultScript, send_queue_capacity: usize) -> Result<Self, &'static str> {
        if send_queue_capacity == 0 {
            return Err("send queue capacity must be non-zero");
        }
        Ok(Self {
            pending: script.events.into(),
            active: Vec::new(),
            seed: script.seed,
            packets_seen: 0,
            h3_attempts: 0,
            h2_attempts: 0,
            dns_failures_remaining: 0,
            network_generation: 0,
            send_queue_capacity,
            send_queue_depth: 0,
            send_queue_high_watermark: 0,
            send_queue_waits: 0,
            send_queue_drops: 0,
        })
    }

    pub const fn seed(&self) -> u64 {
        self.seed
    }

    pub fn advance_to(&mut self, elapsed: Duration) {
        while self
            .pending
            .front()
            .is_some_and(|event| event.at <= elapsed)
        {
            let event = self.pending.pop_front().expect("front checked");
            if let FaultKind::NetworkChange(generation) = event.fault {
                self.network_generation = self.network_generation.max(generation);
            }
            if let FaultKind::FailDnsAttempts(attempts) = event.fault {
                self.dns_failures_remaining = self.dns_failures_remaining.max(attempts);
            }
            self.active.push(event.fault);
        }
    }

    pub fn h3_connect(&mut self) -> Result<(), TransportFailure> {
        self.h3_attempts = self.h3_attempts.saturating_add(1);
        if self.active.contains(&FaultKind::EndpointPinMismatch) {
            return Err(TransportFailure::new(
                TransportFailureCode::EndpointPinMismatch,
                TransportStage::TlsHandshake,
            ));
        }
        if self.active.contains(&FaultKind::DropAllUdp) {
            return Err(TransportFailure::new(
                TransportFailureCode::H3UdpUnreachable,
                TransportStage::QuicHandshake,
            ));
        }
        if self.active.contains(&FaultKind::StallQuicHandshake)
            || (self.active.contains(&FaultKind::FailFirstH3Handshake) && self.h3_attempts == 1)
        {
            return Err(TransportFailure::new(
                TransportFailureCode::H3HandshakeTimeout,
                TransportStage::QuicHandshake,
            ));
        }
        if self.active.contains(&FaultKind::DisableH3Datagrams) {
            return Err(TransportFailure::new(
                TransportFailureCode::H3DatagramUnavailable,
                TransportStage::PeerSettings,
            ));
        }
        if self.active.contains(&FaultKind::FailSocketProtect) {
            return Err(TransportFailure::new(
                TransportFailureCode::SocketProtectionFailed,
                TransportStage::SocketProtection,
            ));
        }
        Ok(())
    }

    pub fn h2_connect(&mut self) -> Result<(), TransportFailure> {
        self.h2_attempts = self.h2_attempts.saturating_add(1);
        if self.active.contains(&FaultKind::ForceH2GoAway) && self.h2_attempts == 1 {
            return Err(TransportFailure::new(
                TransportFailureCode::H2GoAway,
                TransportStage::MasqueConnect,
            ));
        }
        if self.active.contains(&FaultKind::ForceH2StreamReset) && self.h2_attempts == 1 {
            return Err(TransportFailure::new(
                TransportFailureCode::H2StreamClosed,
                TransportStage::PacketReceive,
            ));
        }
        Ok(())
    }

    pub fn resolve_endpoint(&mut self) -> Result<(), TransportFailure> {
        if self.dns_failures_remaining > 0 {
            self.dns_failures_remaining -= 1;
            return Err(TransportFailure::new(
                TransportFailureCode::PhysicalDnsUnavailable,
                TransportStage::EndpointResolution,
            ));
        }
        Ok(())
    }

    pub fn select_endpoint_family(
        &self,
        preferred: AddressFamily,
    ) -> Result<AddressFamily, TransportFailure> {
        let ipv4_available = !self.active.contains(&FaultKind::RemoveIpv4);
        let ipv6_available = !self.active.contains(&FaultKind::RemoveIpv6);
        match (preferred, ipv4_available, ipv6_available) {
            (AddressFamily::Ipv6, _, true) => Ok(AddressFamily::Ipv6),
            (AddressFamily::Ipv4, true, _) | (AddressFamily::Ipv6, true, false) => {
                Ok(AddressFamily::Ipv4)
            }
            (AddressFamily::Ipv4, false, true) => Ok(AddressFamily::Ipv6),
            (AddressFamily::Ipv4, false, false) => Err(TransportFailure::new(
                TransportFailureCode::PhysicalIpv4Unavailable,
                TransportStage::EndpointResolution,
            )),
            (AddressFamily::Ipv6, false, false) => Err(TransportFailure::new(
                TransportFailureCode::PhysicalIpv6Unavailable,
                TransportStage::EndpointResolution,
            )),
        }
    }

    pub fn path_accepts_packets(&self, generation: u64) -> bool {
        generation == self.network_generation
    }

    pub fn agent_ipc(&self) -> Result<(), TransportFailure> {
        if self.active.contains(&FaultKind::DisconnectAgentIpc) {
            return Err(TransportFailure::new(
                TransportFailureCode::AgentUnreachable,
                TransportStage::PlatformRecovery,
            ));
        }
        Ok(())
    }

    pub fn vpn_process(&self) -> Result<(), TransportFailure> {
        if self.active.contains(&FaultKind::TerminateVpnProcess) {
            return Err(TransportFailure::new(
                TransportFailureCode::VpnServiceUnavailable,
                TransportStage::PlatformRecovery,
            ));
        }
        Ok(())
    }

    pub fn recovery_probe_preserves_active_path(
        &mut self,
        active: Transport,
    ) -> (Transport, Result<(), TransportFailure>) {
        (active, self.h3_connect())
    }

    pub fn send_packet(&mut self) -> Result<(), TransportFailure> {
        self.packets_seen = self.packets_seen.saturating_add(1);
        if self.active.iter().any(
            |fault| matches!(fault, FaultKind::BlackholeAfter(limit) if self.packets_seen > *limit),
        ) {
            return Err(TransportFailure::new(
                TransportFailureCode::PacketReceiveStalled,
                TransportStage::PacketReceive,
            ));
        }
        if self.active.contains(&FaultKind::StallWrites) {
            return Err(TransportFailure::new(
                TransportFailureCode::PacketSendTimeout,
                TransportStage::PacketSend,
            ));
        }
        if self.active.contains(&FaultKind::FillSendQueue)
            || self.send_queue_depth == self.send_queue_capacity
        {
            self.send_queue_waits = self.send_queue_waits.saturating_add(1);
            return Ok(());
        }
        self.send_queue_depth += 1;
        self.send_queue_high_watermark = self.send_queue_high_watermark.max(self.send_queue_depth);
        Ok(())
    }

    pub fn complete_send(&mut self) {
        if self.send_queue_waits != 0 {
            self.send_queue_waits -= 1;
        } else {
            self.send_queue_depth = self.send_queue_depth.saturating_sub(1);
        }
    }

    pub fn receive_packet(&self) -> Result<(), TransportFailure> {
        if self.active.contains(&FaultKind::StallReads) {
            return Err(TransportFailure::new(
                TransportFailureCode::PacketReceiveStalled,
                TransportStage::PacketReceive,
            ));
        }
        Ok(())
    }

    pub fn dns_apply(&self) -> Result<(), TransportFailure> {
        if self.active.contains(&FaultKind::FailDnsApply) {
            return Err(TransportFailure::new(
                TransportFailureCode::DnsApplyFailed,
                TransportStage::DnsApply,
            ));
        }
        Ok(())
    }

    pub fn route_apply(&self) -> Result<(), TransportFailure> {
        if self.active.contains(&FaultKind::FailRouteApply) {
            return Err(TransportFailure::new(
                TransportFailureCode::RouteApplyFailed,
                TransportStage::RouteApply,
            ));
        }
        Ok(())
    }

    pub const fn network_generation(&self) -> u64 {
        self.network_generation
    }

    pub const fn send_queue_high_watermark(&self) -> usize {
        self.send_queue_high_watermark
    }

    pub const fn send_queue_drops(&self) -> u64 {
        self.send_queue_drops
    }

    pub const fn send_queue_waits(&self) -> u64 {
        self.send_queue_waits
    }

    pub const fn send_queue_depth(&self) -> usize {
        self.send_queue_depth
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn harness(events: Vec<ScheduledFault>) -> FaultHarness {
        FaultHarness::new(FaultScript::new(0x5eed, events).unwrap(), 2).unwrap()
    }

    #[test]
    fn new_fault_limits_cannot_create_unbounded_delays_or_invalid_partial_sends() {
        for fault in [
            FaultKind::SendMmsgPartial(0),
            FaultKind::SendMmsgPartial(65),
            FaultKind::H2CapacityWait(Duration::from_secs(11)),
        ] {
            assert!(
                FaultScript::new(
                    12,
                    vec![ScheduledFault {
                        at: Duration::ZERO,
                        fault
                    }]
                )
                .is_err()
            );
        }
    }

    #[tokio::test(start_paused = true)]
    async fn network_fault_scripts_are_instance_local_timed_and_single_use() {
        let script = FaultScript::new(
            12,
            vec![ScheduledFault {
                at: Duration::from_secs(1),
                fault: FaultKind::SendMessageTooLarge,
            }],
        )
        .unwrap();
        let faults = NetworkFaults::new(script);
        let separate = NetworkFaults::new(FaultScript::new(13, vec![]).unwrap());
        assert_eq!(faults.take(FaultPoint::UdpSend), None);
        tokio::time::advance(Duration::from_secs(1)).await;
        assert_eq!(
            faults.clone().take(FaultPoint::UdpSend),
            Some(FaultKind::SendMessageTooLarge)
        );
        assert_eq!(faults.take(FaultPoint::UdpSend), None);
        assert_eq!(separate.take(FaultPoint::UdpSend), None);
    }

    #[test]
    fn udp_blackhole_is_typed_and_allows_h2_fallback() {
        let mut harness = harness(vec![ScheduledFault {
            at: Duration::ZERO,
            fault: FaultKind::DropAllUdp,
        }]);
        harness.advance_to(Duration::ZERO);
        let failure = harness.h3_connect().expect_err("H3 must fail");
        assert_eq!(failure.code, TransportFailureCode::H3UdpUnreachable);
        assert!(failure.fallback_allowed);
        harness.h2_connect().expect("H2 remains available");
    }

    #[test]
    fn endpoint_pin_mismatch_never_allows_transport_fallback() {
        let mut harness = harness(vec![ScheduledFault {
            at: Duration::ZERO,
            fault: FaultKind::EndpointPinMismatch,
        }]);
        harness.advance_to(Duration::ZERO);
        let failure = harness.h3_connect().expect_err("pin must fail");
        assert_eq!(failure.code, TransportFailureCode::EndpointPinMismatch);
        assert!(!failure.fallback_allowed);
    }

    #[test]
    fn stalled_h3_handshake_times_out_and_remains_fallback_eligible() {
        let mut harness = harness(vec![ScheduledFault {
            at: Duration::ZERO,
            fault: FaultKind::StallQuicHandshake,
        }]);
        harness.advance_to(Duration::ZERO);
        let failure = harness.h3_connect().expect_err("handshake must time out");
        assert_eq!(failure.code, TransportFailureCode::H3HandshakeTimeout);
        assert_eq!(failure.stage, TransportStage::QuicHandshake);
        assert!(failure.retryable && failure.fallback_allowed);
    }

    #[test]
    fn unavailable_h3_datagrams_fall_back_to_h2() {
        let mut harness = harness(vec![ScheduledFault {
            at: Duration::ZERO,
            fault: FaultKind::DisableH3Datagrams,
        }]);
        harness.advance_to(Duration::ZERO);
        assert_eq!(
            harness.h3_connect().unwrap_err().code,
            TransportFailureCode::H3DatagramUnavailable
        );
        harness.h2_connect().expect("H2 remains available");
    }

    #[test]
    fn h2_stream_close_reconnects_on_the_next_bounded_attempt() {
        let mut harness = harness(vec![ScheduledFault {
            at: Duration::ZERO,
            fault: FaultKind::ForceH2StreamReset,
        }]);
        harness.advance_to(Duration::ZERO);
        assert_eq!(
            harness.h2_connect().unwrap_err().code,
            TransportFailureCode::H2StreamClosed
        );
        harness.h2_connect().expect("one reconnect is healthy");
    }

    #[test]
    fn transient_dns_failure_recovers_within_the_scripted_retry_bound() {
        let mut harness = harness(vec![ScheduledFault {
            at: Duration::ZERO,
            fault: FaultKind::FailDnsAttempts(2),
        }]);
        harness.advance_to(Duration::ZERO);
        for _ in 0..2 {
            assert_eq!(
                harness.resolve_endpoint().unwrap_err().code,
                TransportFailureCode::PhysicalDnsUnavailable
            );
        }
        harness
            .resolve_endpoint()
            .expect("the third resolution attempt recovers");
    }

    #[test]
    fn withdrawn_ipv6_selects_ipv4_without_reusing_the_old_family() {
        let mut harness = harness(vec![ScheduledFault {
            at: Duration::ZERO,
            fault: FaultKind::RemoveIpv6,
        }]);
        harness.advance_to(Duration::ZERO);
        assert_eq!(
            harness.select_endpoint_family(AddressFamily::Ipv6).unwrap(),
            AddressFamily::Ipv4
        );
    }

    #[test]
    fn network_change_uses_only_the_latest_generation() {
        let mut harness = harness(vec![
            ScheduledFault {
                at: Duration::from_millis(10),
                fault: FaultKind::NetworkChange(4),
            },
            ScheduledFault {
                at: Duration::from_millis(20),
                fault: FaultKind::NetworkChange(7),
            },
        ]);
        harness.advance_to(Duration::from_millis(20));
        assert_eq!(harness.network_generation(), 7);
        assert!(!harness.path_accepts_packets(4));
        assert!(harness.path_accepts_packets(7));
    }

    #[test]
    fn send_queue_saturation_waits_without_dropping_or_failing() {
        let mut harness = harness(Vec::new());
        harness.send_packet().unwrap();
        harness.send_packet().unwrap();
        harness.send_packet().unwrap();
        assert_eq!(harness.send_queue_high_watermark(), 2);
        assert_eq!(harness.send_queue_waits(), 1);
        assert_eq!(harness.send_queue_drops(), 0);
        harness.complete_send();
        assert_eq!(harness.send_queue_depth(), 2);
        assert_eq!(harness.send_queue_waits(), 0);
        harness.complete_send();
        harness.complete_send();
        assert_eq!(harness.send_queue_depth(), 0);
    }

    #[test]
    fn burst_beyond_1024_packets_remains_recoverable() {
        let script = FaultScript::new(0x5eed, Vec::new()).unwrap();
        let mut harness = FaultHarness::new(script, 1_024).unwrap();
        for _ in 0..=1_024 {
            harness.send_packet().unwrap();
        }
        assert_eq!(harness.send_queue_depth(), 1_024);
        assert_eq!(harness.send_queue_waits(), 1);
        assert_eq!(harness.send_queue_drops(), 0);

        for _ in 0..=1_024 {
            harness.complete_send();
        }
        assert_eq!(harness.send_queue_depth(), 0);
        assert_eq!(harness.send_queue_waits(), 0);
    }

    #[test]
    fn stalled_reads_and_writes_have_stable_codes() {
        let mut harness = harness(vec![
            ScheduledFault {
                at: Duration::ZERO,
                fault: FaultKind::StallReads,
            },
            ScheduledFault {
                at: Duration::ZERO,
                fault: FaultKind::StallWrites,
            },
        ]);
        harness.advance_to(Duration::ZERO);
        assert_eq!(
            harness.receive_packet().unwrap_err().code,
            TransportFailureCode::PacketReceiveStalled
        );
        assert_eq!(
            harness.send_packet().unwrap_err().code,
            TransportFailureCode::PacketSendTimeout
        );
    }

    #[test]
    fn disconnected_agent_ipc_is_typed_for_fail_closed_recovery() {
        let mut harness = harness(vec![ScheduledFault {
            at: Duration::ZERO,
            fault: FaultKind::DisconnectAgentIpc,
        }]);
        harness.advance_to(Duration::ZERO);
        let failure = harness.agent_ipc().unwrap_err();
        assert_eq!(failure.code, TransportFailureCode::AgentUnreachable);
        assert!(!failure.fallback_allowed);
    }

    #[test]
    fn failed_h3_recovery_probe_leaves_the_h2_data_path_selected() {
        let mut harness = harness(vec![ScheduledFault {
            at: Duration::ZERO,
            fault: FaultKind::DropAllUdp,
        }]);
        harness.advance_to(Duration::ZERO);
        let (active, probe) = harness.recovery_probe_preserves_active_path(Transport::Http2);
        assert_eq!(active, Transport::Http2);
        assert_eq!(
            probe.unwrap_err().code,
            TransportFailureCode::H3UdpUnreachable
        );
    }

    #[test]
    fn scripts_have_a_hard_event_limit_and_retain_the_seed() {
        assert!(
            FaultScript::new(
                1,
                vec![
                    ScheduledFault {
                        at: Duration::ZERO,
                        fault: FaultKind::DropAllUdp,
                    };
                    FaultScript::MAX_EVENTS + 1
                ]
            )
            .is_err()
        );
        assert_eq!(FaultScript::new(42, Vec::new()).unwrap().seed(), 42);
    }
}
