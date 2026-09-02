use std::collections::VecDeque;
use std::future::{Future, pending};
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant as StdInstant};

use tokio::sync::{mpsc, oneshot};
use tokio::time::{Instant, sleep_until, timeout_at};
use tokio_util::sync::CancellationToken;
use usque_core::TransportStage;

use super::{
    CidAvailability, H3QuicConnection, PreparedPathSocket, SOCKET_PREPARE_ATTEMPTS,
    SocketPrepareError, WireDatagram, WireSendOutcome, handle_pmtu_send_too_large,
    maintain_connection_ids, prepare_udp_for_generation, publish_pmtu_observation,
    recycle_wire_buffer, send_due_wire_datagrams, take_wire_buffer,
};
use crate::h2::TransportError;
use crate::migration_barrier::{
    MIGRATION_DRAIN_BUDGET, MIGRATION_DRAIN_PACKET_BUDGET, MigrationTxBarrier,
};
use crate::network_quality::{MigrationPhase, MigrationReasonCode, NetworkQualityTelemetry};
use crate::path_socket::{PathBinding, PathId, PathSocket, PathSocketRole, PathSocketSet};
use crate::pmtu::{PmtuController, PmtuPathKey, PmtuRevalidationAction};
use crate::queue_metrics::QueueMetrics;
use crate::socket::SocketProtector;
use crate::telemetry::{ConnectionAttemptTelemetry, ConnectionEventType};
use crate::udp_io::SendDatagram;

pub(super) const H3_CONTROL_CAPACITY: usize = 1;
pub(crate) const MIGRATION_TIMEOUT: Duration = Duration::from_secs(3);
const MIGRATION_PROBE_INTERVAL: Duration = Duration::from_millis(100);
const MIGRATION_STATE_POLL_INTERVAL: Duration = Duration::from_millis(20);
const RETIRING_GRACE: Duration = Duration::from_secs(2);
const MAX_PATH_EVENTS_PER_TURN: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum H3MigrationResult {
    Promoted { network_generation: u64 },
    StaleRequest,
    Failed(MigrationReasonCode),
}

pub(super) enum H3ControlCommand {
    Migrate {
        target_generation: u64,
        requested_at: Instant,
        deadline: Instant,
        reply: oneshot::Sender<H3MigrationResult>,
    },
}

/// A bounded control handle for the existing QUIC connection. It never
/// creates a second CONNECT-IP session or exposes path addresses in Debug.
#[derive(Clone)]
pub struct H3MigrationHandle {
    sender: mpsc::Sender<H3ControlCommand>,
    endpoint: SocketAddr,
    initial_generation: u64,
    enabled: bool,
}

impl H3MigrationHandle {
    pub(super) fn new(
        sender: mpsc::Sender<H3ControlCommand>,
        endpoint: SocketAddr,
        initial_generation: u64,
        enabled: bool,
    ) -> Self {
        Self {
            sender,
            endpoint,
            initial_generation,
            enabled,
        }
    }

    pub(crate) const fn endpoint(&self) -> SocketAddr {
        self.endpoint
    }

    pub(crate) const fn initial_generation(&self) -> u64 {
        self.initial_generation
    }

    pub(crate) const fn enabled(&self) -> bool {
        self.enabled
    }

    pub async fn migrate(&self, target_generation: u64) -> H3MigrationResult {
        self.start_migration(target_generation).await
    }

    pub(crate) fn start_migration(
        &self,
        target_generation: u64,
    ) -> Pin<Box<dyn Future<Output = H3MigrationResult> + Send + 'static>> {
        if !self.enabled {
            return Box::pin(async { H3MigrationResult::Failed(MigrationReasonCode::Unsupported) });
        }
        let sender = self.sender.clone();
        Box::pin(async move {
            let requested_at = Instant::now();
            let deadline = requested_at + MIGRATION_TIMEOUT;
            let (reply, response) = oneshot::channel();
            let operation = async move {
                if sender
                    .send(H3ControlCommand::Migrate {
                        target_generation,
                        requested_at,
                        deadline,
                        reply,
                    })
                    .await
                    .is_err()
                {
                    return H3MigrationResult::Failed(MigrationReasonCode::ConnectionClosed);
                }
                response.await.unwrap_or(H3MigrationResult::Failed(
                    MigrationReasonCode::ConnectionClosed,
                ))
            };
            timeout_at(deadline, operation)
                .await
                .unwrap_or(H3MigrationResult::Failed(
                    MigrationReasonCode::PathValidationTimeout,
                ))
        })
    }
}

type PrepareFuture =
    Pin<Box<dyn Future<Output = Result<PreparedPathSocket, SocketPrepareError>> + Send + 'static>>;

struct PendingMigration {
    target_generation: u64,
    requested_at: Instant,
    deadline: Instant,
    reply: Option<oneshot::Sender<H3MigrationResult>>,
    preparation: Option<PrepareFuture>,
    phase: MigrationPhase,
    next_probe_at: Instant,
}

pub(super) struct MigrationDrive<'a> {
    pub(super) connection: &'a mut H3QuicConnection,
    pub(super) paths: &'a mut PathSocketSet,
    pub(super) wire_datagrams: &'a mut VecDeque<WireDatagram>,
    pub(super) free_wire_buffers: &'a mut Vec<Vec<u8>>,
    pub(super) wire_queue: &'a Arc<QueueMetrics>,
    pub(super) pmtu: &'a mut PmtuController,
    pub(super) family_ceiling: usize,
    pub(super) io_cancel: &'a CancellationToken,
}

pub(super) struct MigrationActor {
    protector: Arc<dyn SocketProtector>,
    quality: NetworkQualityTelemetry,
    attempt: Option<ConnectionAttemptTelemetry>,
    pending: Option<PendingMigration>,
    highest_generation: u64,
    next_path_id: u64,
    retiring_deadline: Option<Instant>,
    barrier: MigrationTxBarrier,
    return_to_idle: bool,
}

impl MigrationActor {
    pub(super) fn new(
        protector: Arc<dyn SocketProtector>,
        quality: NetworkQualityTelemetry,
        attempt: Option<ConnectionAttemptTelemetry>,
        initial_generation: u64,
    ) -> Self {
        Self {
            protector,
            quality,
            attempt,
            pending: None,
            highest_generation: initial_generation,
            next_path_id: 1,
            retiring_deadline: None,
            barrier: MigrationTxBarrier::default(),
            return_to_idle: false,
        }
    }

    pub(super) fn allows_application_injection(&self) -> bool {
        self.barrier.allows_application_injection()
    }

    pub(super) fn is_preparing(&self) -> bool {
        self.pending
            .as_ref()
            .is_some_and(|attempt| attempt.preparation.is_some())
    }

    pub(super) fn next_wakeup(&self) -> Instant {
        let now = Instant::now();
        let migration = self.pending.as_ref().map(|attempt| {
            let wakeup = (now + MIGRATION_STATE_POLL_INTERVAL).min(attempt.deadline);
            if matches!(
                attempt.phase,
                MigrationPhase::Probing | MigrationPhase::Validated
            ) {
                wakeup.min(attempt.next_probe_at.max(now))
            } else {
                wakeup
            }
        });
        migration
            .into_iter()
            .chain(self.retiring_deadline)
            .min()
            .unwrap_or(now + Duration::from_secs(86_400))
    }

    pub(super) async fn handle_command(
        &mut self,
        command: H3ControlCommand,
        connection: &mut H3QuicConnection,
        paths: &mut PathSocketSet,
    ) {
        let H3ControlCommand::Migrate {
            target_generation,
            requested_at,
            deadline,
            reply,
        } = command;
        if reply.is_closed() {
            return;
        }
        if target_generation <= self.highest_generation
            || self
                .protector
                .network_generation()
                .is_some_and(|current| target_generation < current)
        {
            let _ = reply.send(H3MigrationResult::StaleRequest);
            return;
        }
        self.abort(MigrationReasonCode::Superseded, paths).await;
        paths.clear_retiring().await;
        self.retiring_deadline = None;
        self.highest_generation = target_generation;
        let now = Instant::now();
        self.pending = Some(PendingMigration {
            target_generation,
            requested_at,
            deadline: deadline.min(now + MIGRATION_TIMEOUT),
            reply: Some(reply),
            preparation: None,
            phase: MigrationPhase::PreparingSocket,
            next_probe_at: now + MIGRATION_STATE_POLL_INTERVAL,
        });
        self.return_to_idle = false;
        self.quality
            .set_migration_phase(MigrationPhase::PreparingSocket);
        self.record(
            ConnectionEventType::MigrationStarted,
            TransportStage::SocketConnect,
        );

        let Some(active) = paths.active().map(PathSocket::binding) else {
            self.abort(MigrationReasonCode::ConnectionClosed, paths)
                .await;
            return;
        };
        let reason = if !self.quality.features().quic_migration
            || self.protector.network_generation().is_none()
        {
            Some(MigrationReasonCode::Unsupported)
        } else if self.protector.endpoint_family_available(active.peer_addr) == Some(false) {
            Some(MigrationReasonCode::FamilyUnavailable)
        } else if !connection.is_established() || connection_stopping(connection) {
            Some(MigrationReasonCode::ConnectionClosed)
        } else {
            self.invalid_attempt_reason(now)
        };
        if let Some(reason) = reason {
            self.abort(reason, paths).await;
            return;
        }
        let availability = maintain_connection_ids(connection);
        #[cfg(any(test, feature = "fault-injection"))]
        let availability = if self
            .quality
            .take_fault(crate::fault_injection::FaultPoint::CandidateCid)
            .is_some()
        {
            Ok(CidAvailability::PeerUnavailable)
        } else {
            availability
        };
        let reason = match availability {
            Ok(CidAvailability::Ready) => None,
            Ok(CidAvailability::PeerUnavailable) => Some(MigrationReasonCode::PeerCidUnavailable),
            Ok(CidAvailability::LocalUnavailable) | Err(_) => {
                Some(MigrationReasonCode::LocalCidUnavailable)
            }
        };
        if let Some(reason) = reason {
            self.abort(reason, paths).await;
            return;
        }
        let protector = Arc::clone(&self.protector);
        let endpoint = active.peer_addr;
        self.pending
            .as_mut()
            .expect("new migration remains present")
            .preparation = Some(Box::pin(async move {
            for _ in 0..SOCKET_PREPARE_ATTEMPTS {
                match prepare_udp_for_generation(endpoint, target_generation, protector.as_ref())
                    .await
                {
                    Err(SocketPrepareError::StaleGeneration)
                        if protector.network_generation() == Some(target_generation) =>
                    {
                        continue;
                    }
                    result => return result,
                }
            }
            Err(SocketPrepareError::StaleGeneration)
        }));
    }

    pub(super) async fn wait_prepared(&mut self) -> Result<PreparedPathSocket, SocketPrepareError> {
        match self
            .pending
            .as_mut()
            .and_then(|attempt| attempt.preparation.as_mut())
        {
            Some(preparation) => preparation.as_mut().await,
            None => pending().await,
        }
    }

    pub(super) async fn on_prepared(
        &mut self,
        prepared: Result<PreparedPathSocket, SocketPrepareError>,
        paths: &mut PathSocketSet,
    ) {
        let Some(attempt) = self.pending.as_mut() else {
            return;
        };
        attempt.preparation.take();
        #[cfg(any(test, feature = "fault-injection"))]
        if self
            .quality
            .take_fault(crate::fault_injection::FaultPoint::CandidateSetup)
            .is_some()
        {
            drop(prepared);
            self.abort(MigrationReasonCode::GenerationChangedDuringSetup, paths)
                .await;
            return;
        }
        if let Some(reason) = self.invalid_attempt_reason(Instant::now()) {
            drop(prepared);
            self.abort(reason, paths).await;
            return;
        }
        let prepared = match prepared {
            Ok(prepared) => prepared,
            Err(SocketPrepareError::StaleGeneration) => {
                self.abort(MigrationReasonCode::GenerationChangedDuringSetup, paths)
                    .await;
                return;
            }
            Err(SocketPrepareError::Io(_) | SocketPrepareError::Protection(_)) => {
                self.abort(MigrationReasonCode::SocketProtectFailed, paths)
                    .await;
                return;
            }
        };
        let Some(next_path_id) = self.next_path_id.checked_add(1) else {
            drop(prepared);
            self.abort(MigrationReasonCode::Unsupported, paths).await;
            return;
        };
        let path = PathSocket::spawn(
            PathId::new(self.next_path_id),
            prepared.local_addr,
            prepared.peer_addr,
            prepared.network_generation,
            PathSocketRole::Candidate,
            prepared.socket,
            prepared.egress_lease,
            self.quality.clone(),
            paths.receive_pool(),
        );
        self.next_path_id = next_path_id;
        let inserted = path
            .map_err(|_| ())
            .and_then(|path| paths.insert(path).map_err(|_| ()));
        if inserted.is_err() {
            self.abort(MigrationReasonCode::SocketProtectFailed, paths)
                .await;
            return;
        }
        let attempt = self
            .pending
            .as_mut()
            .expect("prepared migration remains present");
        attempt.phase = MigrationPhase::Probing;
        attempt.next_probe_at = Instant::now();
        self.quality.set_migration_phase(MigrationPhase::Probing);
    }

    pub(super) async fn handle_receive_failure(
        &mut self,
        path_id: PathId,
        paths: &mut PathSocketSet,
    ) -> bool {
        if paths
            .candidate()
            .is_some_and(|path| path.path_id == path_id)
        {
            self.abort(MigrationReasonCode::PathProbeRejected, paths)
                .await;
            return true;
        }
        if paths.retiring().is_some_and(|path| path.path_id == path_id) {
            paths.clear_retiring().await;
            self.retiring_deadline = None;
            return true;
        }
        false
    }

    pub(super) async fn tick(
        &mut self,
        mut drive: MigrationDrive<'_>,
    ) -> Result<(), TransportError> {
        let now = Instant::now();
        if self.return_to_idle {
            self.quality.set_migration_phase(MigrationPhase::Idle);
            self.return_to_idle = false;
        }
        if self
            .retiring_deadline
            .is_some_and(|deadline| now >= deadline)
        {
            drive.paths.clear_retiring().await;
            self.retiring_deadline = None;
        }
        if connection_stopping(drive.connection) {
            self.abort(MigrationReasonCode::ConnectionClosed, drive.paths)
                .await;
            return Ok(());
        }
        if let Some(reason) = self.invalid_attempt_reason(now) {
            self.abort(reason, drive.paths).await;
        }
        self.process_path_events(drive.connection, drive.paths)
            .await;
        verify_active_binding(drive.connection, drive.paths)?;

        let Some(attempt) = self.pending.as_ref() else {
            return Ok(());
        };
        #[cfg(any(test, feature = "fault-injection"))]
        if attempt.phase == MigrationPhase::Probing
            && let Some(fault) = self
                .quality
                .take_fault(crate::fault_injection::FaultPoint::CandidateValidation)
        {
            let reason = match fault {
                crate::FaultKind::PathValidationTimeout => {
                    MigrationReasonCode::PathValidationTimeout
                }
                _ => MigrationReasonCode::PathProbeRejected,
            };
            self.abort(reason, drive.paths).await;
            return Ok(());
        }
        if attempt.phase == MigrationPhase::PreparingSocket
            || now < attempt.next_probe_at
            || drive
                .pmtu
                .send_suppressed_until(StdInstant::now())
                .is_some()
        {
            return Ok(());
        }
        let phase = attempt.phase;
        let cycle_deadline = (now + MIGRATION_DRAIN_BUDGET).min(attempt.deadline);
        let Some(active) = drive.paths.active().map(PathSocket::binding) else {
            self.abort(MigrationReasonCode::ConnectionClosed, drive.paths)
                .await;
            return Ok(());
        };
        let Some(candidate) = drive.paths.candidate().map(PathSocket::binding) else {
            self.abort(MigrationReasonCode::PathProbeRejected, drive.paths)
                .await;
            return Ok(());
        };
        if !self.barrier.begin(StdInstant::now()) {
            return Err(TransportError::Http3(
                "migration transmit barrier was re-entered".to_owned(),
            ));
        }
        let drain = timeout_at(
            cycle_deadline,
            drain_active_output(&mut drive, active, &self.quality),
        )
        .await;
        let packets = match drain {
            Ok(Ok(ActiveDrain::Drained(packets))) => packets,
            Ok(Ok(ActiveDrain::MessageTooLarge)) => {
                self.barrier.finish();
                handle_pmtu_send_too_large(
                    drive.connection,
                    drive.pmtu,
                    self.attempt.as_ref(),
                    &self.quality,
                )?;
                self.defer_probe();
                return Ok(());
            }
            Ok(Err(error)) => {
                self.barrier.finish();
                return Err(error);
            }
            Ok(Ok(ActiveDrain::Deferred)) | Err(_) => {
                self.barrier.finish();
                self.defer_probe();
                return Ok(());
            }
        };
        if !self
            .barrier
            .complete_active_drain(StdInstant::now(), packets, true)
        {
            self.defer_probe();
            return Ok(());
        }
        if let Some(reason) = self.invalid_attempt_reason(Instant::now()) {
            self.barrier.finish();
            self.abort(reason, drive.paths).await;
            return Ok(());
        }

        if phase == MigrationPhase::Validated {
            self.promote(&mut drive, candidate).await?;
            self.barrier.finish();
            return Ok(());
        }
        debug_assert!(self.barrier.candidate_send_allowed());
        let probe = drive
            .connection
            .probe_path(candidate.local_addr, candidate.peer_addr);
        if let Err(error) = probe {
            let reason = probe_failure_reason(error, drive.connection.available_dcids());
            self.barrier.finish();
            self.abort(reason, drive.paths).await;
            return Ok(());
        }
        let sent = send_candidate_control(
            &mut drive,
            candidate,
            cycle_deadline,
            MIGRATION_DRAIN_PACKET_BUDGET.saturating_sub(packets),
            &self.quality,
        )
        .await;
        self.barrier.finish();
        match sent {
            Ok(()) => self.defer_probe(),
            Err(reason) => self.abort(reason, drive.paths).await,
        }
        Ok(())
    }

    async fn process_path_events(
        &mut self,
        connection: &mut H3QuicConnection,
        paths: &mut PathSocketSet,
    ) {
        for _ in 0..MAX_PATH_EVENTS_PER_TURN {
            let Some(event) = connection.path_event_next() else {
                break;
            };
            match event {
                quiche::PathEvent::Validated(local, peer)
                    if candidate_matches(paths, local, peer) =>
                {
                    if let Some(reason) = self.invalid_attempt_reason(Instant::now()) {
                        self.abort(reason, paths).await;
                        continue;
                    }
                    if connection_stopping(connection)
                        || !paths.promotion_ready()
                        || connection.is_path_validated(local, peer) != Ok(true)
                    {
                        self.abort(MigrationReasonCode::PromotionFailed, paths)
                            .await;
                        continue;
                    }
                    if let Some(attempt) = self.pending.as_mut()
                        && attempt.phase == MigrationPhase::Probing
                    {
                        attempt.phase = MigrationPhase::Validated;
                        attempt.next_probe_at = Instant::now();
                        self.quality.set_migration_phase(MigrationPhase::Validated);
                        self.record(
                            ConnectionEventType::MigrationPathValidated,
                            TransportStage::QuicHandshake,
                        );
                    }
                }
                quiche::PathEvent::FailedValidation(local, peer)
                    if candidate_matches(paths, local, peer) =>
                {
                    self.abort(MigrationReasonCode::PathValidationTimeout, paths)
                        .await;
                }
                quiche::PathEvent::Closed(local, peer) => {
                    if candidate_matches(paths, local, peer) {
                        self.abort(MigrationReasonCode::PathProbeRejected, paths)
                            .await;
                    }
                    if paths
                        .retiring()
                        .is_some_and(|path| path.local_addr == local && path.peer_addr == peer)
                    {
                        paths.clear_retiring().await;
                        self.retiring_deadline = None;
                    }
                }
                _ => {}
            }
        }
    }

    async fn promote(
        &mut self,
        drive: &mut MigrationDrive<'_>,
        candidate: PathBinding,
    ) -> Result<(), TransportError> {
        if !drive.paths.promotion_ready()
            || drive
                .connection
                .is_path_validated(candidate.local_addr, candidate.peer_addr)
                != Ok(true)
            || self.protector.network_generation() != Some(candidate.network_generation)
            || connection_stopping(drive.connection)
            || !drive.wire_datagrams.is_empty()
        {
            self.abort(MigrationReasonCode::PromotionFailed, drive.paths)
                .await;
            return Ok(());
        }
        self.quality.set_migration_phase(MigrationPhase::Promoting);
        if drive
            .connection
            .migrate_source(candidate.local_addr)
            .is_err()
        {
            self.abort(MigrationReasonCode::PromotionFailed, drive.paths)
                .await;
            return Ok(());
        }
        // No await is allowed between quiche activation, the socket/lease/
        // generation role swap, and PMTU invalidation.
        let binding = drive.paths.promote_candidate().map_err(|_| {
            TransportError::Http3(
                "QUIC promotion could not atomically swap its socket binding".to_owned(),
            )
        })?;
        debug_assert_eq!(binding.path_id, candidate.path_id);
        let PmtuRevalidationAction::Revalidate(observation) = drive
            .pmtu
            .on_path_promoted(PmtuPathKey::new(binding.local_addr, binding.peer_addr))
        else {
            return Err(TransportError::Http3(
                "promoted QUIC path rejected PMTU revalidation".to_owned(),
            ));
        };
        if self.quality.features().automatic_pmtu {
            drive.connection.revalidate_pmtu();
            self.record(
                ConnectionEventType::PmtuRevalidationStarted,
                TransportStage::PacketSend,
            );
        }
        publish_pmtu_observation(&self.quality, observation);
        self.retiring_deadline = Some(Instant::now() + RETIRING_GRACE);
        let mut attempt = self
            .pending
            .take()
            .expect("promotion owns one migration attempt");
        self.quality
            .record_migration_success(attempt.requested_at.elapsed());
        self.record(
            ConnectionEventType::MigrationPromoted,
            TransportStage::PacketSend,
        );
        if let Some(reply) = attempt.reply.take() {
            let _ = reply.send(H3MigrationResult::Promoted {
                network_generation: binding.network_generation,
            });
        }
        Ok(())
    }

    async fn abort(&mut self, reason: MigrationReasonCode, paths: &mut PathSocketSet) {
        self.barrier.finish();
        let Some(mut attempt) = self.pending.take() else {
            return;
        };
        attempt.preparation.take();
        self.quality
            .record_migration_failure(attempt.requested_at.elapsed(), reason);
        self.record(
            ConnectionEventType::MigrationFailed,
            TransportStage::PacketSend,
        );
        paths.clear_candidate().await;
        self.return_to_idle = true;
        if let Some(reply) = attempt.reply.take() {
            let _ = reply.send(H3MigrationResult::Failed(reason));
        }
    }

    fn invalid_attempt_reason(&self, now: Instant) -> Option<MigrationReasonCode> {
        let attempt = self.pending.as_ref()?;
        if now >= attempt.deadline {
            return Some(MigrationReasonCode::PathValidationTimeout);
        }
        if attempt
            .reply
            .as_ref()
            .is_none_or(oneshot::Sender::is_closed)
        {
            return Some(MigrationReasonCode::Superseded);
        }
        let current = self.protector.network_generation();
        if current != Some(attempt.target_generation) {
            return Some(if attempt.phase == MigrationPhase::PreparingSocket {
                MigrationReasonCode::GenerationChangedDuringSetup
            } else if current.is_some_and(|generation| generation > attempt.target_generation) {
                MigrationReasonCode::Superseded
            } else {
                MigrationReasonCode::GenerationChangedDuringSetup
            });
        }
        None
    }

    fn defer_probe(&mut self) {
        if let Some(attempt) = self.pending.as_mut() {
            attempt.next_probe_at = Instant::now() + MIGRATION_PROBE_INTERVAL;
        }
    }

    fn record(&self, event: ConnectionEventType, stage: TransportStage) {
        if let Some(attempt) = &self.attempt {
            attempt.record(event, stage);
        }
    }
}

impl Drop for MigrationActor {
    fn drop(&mut self) {
        if let Some(mut attempt) = self.pending.take() {
            self.quality.record_migration_failure(
                attempt.requested_at.elapsed(),
                MigrationReasonCode::ConnectionClosed,
            );
            self.record(
                ConnectionEventType::MigrationFailed,
                TransportStage::PacketSend,
            );
            if let Some(reply) = attempt.reply.take() {
                let _ = reply.send(H3MigrationResult::Failed(
                    MigrationReasonCode::ConnectionClosed,
                ));
            }
        }
    }
}

fn candidate_matches(paths: &PathSocketSet, local: SocketAddr, peer: SocketAddr) -> bool {
    paths
        .candidate()
        .is_some_and(|path| path.local_addr == local && path.peer_addr == peer)
}

fn connection_stopping(connection: &H3QuicConnection) -> bool {
    connection.is_closed()
        || connection.is_draining()
        || connection.local_error().is_some()
        || connection.peer_error().is_some()
}

fn verify_active_binding(
    connection: &H3QuicConnection,
    paths: &PathSocketSet,
) -> Result<(), TransportError> {
    let Some(binding) = paths.active().map(PathSocket::binding) else {
        return Err(TransportError::Http3(
            "H3 has no active socket binding".to_owned(),
        ));
    };
    if !connection.path_stats().any(|path| {
        path.active && path.local_addr == binding.local_addr && path.peer_addr == binding.peer_addr
    }) {
        return Err(TransportError::Http3(
            "QUIC active path diverged from its socket binding".to_owned(),
        ));
    }
    Ok(())
}

fn probe_failure_reason(error: quiche::Error, available_peer_ids: usize) -> MigrationReasonCode {
    match error {
        quiche::Error::OutOfIdentifiers if available_peer_ids == 0 => {
            MigrationReasonCode::PeerCidUnavailable
        }
        quiche::Error::OutOfIdentifiers | quiche::Error::IdLimit => {
            MigrationReasonCode::LocalCidUnavailable
        }
        _ => MigrationReasonCode::PathProbeRejected,
    }
}

enum ActiveDrain {
    Drained(usize),
    Deferred,
    MessageTooLarge,
}

async fn drain_active_output(
    drive: &mut MigrationDrive<'_>,
    active: PathBinding,
    quality: &NetworkQualityTelemetry,
) -> Result<ActiveDrain, TransportError> {
    let mut packets = 0;
    loop {
        if packets >= MIGRATION_DRAIN_PACKET_BUDGET {
            return Ok(ActiveDrain::Deferred);
        }
        if let Some(first) = drive.wire_datagrams.front() {
            if first.send_info.from != active.local_addr || first.send_info.to != active.peer_addr {
                return Err(TransportError::Http3(
                    "migration drain found output for a non-active binding".to_owned(),
                ));
            }
            if first.send_info.at > StdInstant::now() {
                sleep_until(Instant::from_std(first.send_info.at)).await;
            }
            let before = drive.wire_datagrams.len();
            let sent = send_due_wire_datagrams(
                drive.paths,
                drive.wire_datagrams,
                drive.free_wire_buffers,
                drive.connection.send_quantum(),
                drive.wire_queue,
                quality,
                drive.io_cancel,
            )
            .await?;
            if sent == WireSendOutcome::MessageTooLarge {
                return Ok(ActiveDrain::MessageTooLarge);
            }
            let completed = before.saturating_sub(drive.wire_datagrams.len());
            if completed == 0 {
                return Ok(ActiveDrain::Deferred);
            }
            packets += completed;
            continue;
        }
        let mut bytes = take_wire_buffer(drive.free_wire_buffers, drive.family_ceiling, quality);
        match drive.connection.send_on_path(
            &mut bytes,
            Some(active.local_addr),
            Some(active.peer_addr),
        ) {
            Ok((length, send_info)) => {
                bytes.truncate(length);
                drive.wire_datagrams.push_back(WireDatagram {
                    bytes,
                    send_info,
                    queue_entry: drive.wire_queue.start_entry(length),
                });
            }
            Err(quiche::Error::Done) => {
                recycle_wire_buffer(drive.free_wire_buffers, bytes, quality);
                return Ok(ActiveDrain::Drained(packets));
            }
            Err(_) => {
                recycle_wire_buffer(drive.free_wire_buffers, bytes, quality);
                return Err(TransportError::Http3(
                    "migration active drain failed".to_owned(),
                ));
            }
        }
    }
}

async fn send_candidate_control(
    drive: &mut MigrationDrive<'_>,
    candidate: PathBinding,
    deadline: Instant,
    packet_budget: usize,
    quality: &NetworkQualityTelemetry,
) -> Result<(), MigrationReasonCode> {
    for _ in 0..packet_budget {
        if Instant::now() >= deadline {
            break;
        }
        let mut bytes = take_wire_buffer(drive.free_wire_buffers, drive.family_ceiling, quality);
        let (length, info) = match drive.connection.send_on_path(
            &mut bytes,
            Some(candidate.local_addr),
            Some(candidate.peer_addr),
        ) {
            Ok(output) => output,
            Err(quiche::Error::Done) => {
                recycle_wire_buffer(drive.free_wire_buffers, bytes, quality);
                break;
            }
            Err(error) => {
                recycle_wire_buffer(drive.free_wire_buffers, bytes, quality);
                return Err(probe_failure_reason(
                    error,
                    drive.connection.available_dcids(),
                ));
            }
        };
        if info.from != candidate.local_addr || info.to != candidate.peer_addr {
            recycle_wire_buffer(drive.free_wire_buffers, bytes, quality);
            return Err(MigrationReasonCode::PathProbeRejected);
        }
        let entry = drive.wire_queue.start_entry(length);
        let sent = timeout_at(deadline, async {
            if info.at > StdInstant::now() {
                sleep_until(Instant::from_std(info.at)).await;
            }
            let io = drive
                .paths
                .io_for_send(info.from, info.to)
                .map_err(|_| std::io::Error::other("candidate path routing failed"))?;
            io.send_batch(
                &[SendDatagram {
                    payload: &bytes[..length],
                    source: info.from,
                    destination: info.to,
                    due_at: info.at,
                }],
                drive.io_cancel,
            )
            .await
        })
        .await;
        recycle_wire_buffer(drive.free_wire_buffers, bytes, quality);
        match sent {
            Ok(Ok(1)) => entry.complete(),
            Err(_) => {
                // The locked-quiche contract proves this inactive-path output
                // contains no application data. A timed-out control packet is
                // dropped like ordinary UDP loss; the next bounded probe cycle
                // retries without touching queued application DATAGRAMs.
                entry.drop_item();
                break;
            }
            Ok(Ok(_) | Err(_)) => {
                entry.drop_item();
                return Err(MigrationReasonCode::PathProbeRejected);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

    use tokio::net::UdpSocket;
    use tokio::time::timeout;
    use usque_core::{AddressFamily, Transport};

    use super::*;
    use crate::h3::tests::{
        advance_test_pair, encode_for_test, ipv4_packet_with_length, test_quic_pair_at,
    };
    use crate::h3::{MAX_PENDING_WIRE_DATAGRAMS, generate_wire_datagrams, receive_quic_datagram};
    use crate::network_quality::{NetworkQualitySampler, PmtuPhase};
    use crate::path_socket::{PathReceiveEvent, PathSocketSetError};
    use crate::pmtu::IPV4_MAX_UDP_PAYLOAD;
    use crate::queue_metrics::QueueKind;
    use crate::socket::{DirectEgressLease, DirectProtocol, STALE_GENERATION_REASON, SocketHandle};
    use crate::telemetry::ConnectionTelemetry;
    use crate::udp_io::UdpReceivePool;

    struct LeaseDrop(Arc<AtomicUsize>);

    impl Drop for LeaseDrop {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::AcqRel);
        }
    }

    struct TestProtector {
        generation: AtomicU64,
        family_available: AtomicBool,
        fail_protection: AtomicBool,
        stall_preparation: AtomicBool,
        race_preparation: AtomicBool,
        calls: AtomicUsize,
        lease_drops: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl SocketProtector for TestProtector {
        fn protect(&self, _socket: SocketHandle) -> Result<(), String> {
            Err("un-targeted socket protection is forbidden in this test".to_owned())
        }

        async fn protect_for_target_generation(
            &self,
            _socket: SocketHandle,
            _remote: SocketAddr,
            _protocol: DirectProtocol,
            expected_generation: u64,
        ) -> Result<DirectEgressLease, String> {
            self.calls.fetch_add(1, Ordering::AcqRel);
            if self.generation.load(Ordering::Acquire) != expected_generation {
                return Err(STALE_GENERATION_REASON.to_owned());
            }
            if self.fail_protection.load(Ordering::Acquire) {
                return Err("test protection rejection".to_owned());
            }
            let lease = DirectEgressLease::hold_for_generation(
                LeaseDrop(Arc::clone(&self.lease_drops)),
                expected_generation,
            );
            if self.stall_preparation.load(Ordering::Acquire) {
                pending::<()>().await;
            }
            if self.race_preparation.load(Ordering::Acquire) {
                self.generation.fetch_add(1, Ordering::AcqRel);
            }
            Ok(lease)
        }

        fn network_generation(&self) -> Option<u64> {
            Some(self.generation.load(Ordering::Acquire))
        }

        fn endpoint_family_available(&self, _endpoint: SocketAddr) -> Option<bool> {
            Some(self.family_available.load(Ordering::Acquire))
        }
    }

    struct WireObservation {
        source: SocketAddr,
        application_datagrams: Vec<Vec<u8>>,
    }

    async fn migration_reply(response: oneshot::Receiver<H3MigrationResult>) -> H3MigrationResult {
        timeout(Duration::from_secs(1), response)
            .await
            .expect("actor reply is bounded")
            .unwrap()
    }

    struct Harness {
        client: H3QuicConnection,
        server: H3QuicConnection,
        server_socket: UdpSocket,
        paths: PathSocketSet,
        actor: MigrationActor,
        protector: Arc<TestProtector>,
        telemetry: ConnectionTelemetry,
        quality: NetworkQualityTelemetry,
        wire: VecDeque<WireDatagram>,
        free: Vec<Vec<u8>>,
        wire_queue: Arc<QueueMetrics>,
        pmtu: PmtuController,
        cancel: CancellationToken,
    }

    impl Harness {
        async fn new(with_spares: bool) -> Self {
            let client_socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
            let server_socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
            let client_addr = client_socket.local_addr().unwrap();
            let server_addr = server_socket.local_addr().unwrap();
            let (mut client, mut server, _, _) = test_quic_pair_at(client_addr, server_addr);
            advance_test_pair(&mut client, &mut server).unwrap();
            assert!(client.is_established() && server.is_established());
            if with_spares {
                maintain_connection_ids(&mut client).unwrap();
                maintain_connection_ids(&mut server).unwrap();
                advance_test_pair(&mut client, &mut server).unwrap();
                assert_eq!(
                    maintain_connection_ids(&mut client).unwrap(),
                    CidAvailability::Ready
                );
            }
            let telemetry = ConnectionTelemetry::default();
            telemetry.record_tunnel_ready(Transport::Http3, AddressFamily::Ipv4, Duration::ZERO);
            let quality = telemetry.network_quality();
            let lease_drops = Arc::new(AtomicUsize::new(0));
            let protector = Arc::new(TestProtector {
                generation: AtomicU64::new(1),
                family_available: AtomicBool::new(true),
                fail_protection: AtomicBool::new(false),
                stall_preparation: AtomicBool::new(false),
                race_preparation: AtomicBool::new(false),
                calls: AtomicUsize::new(0),
                lease_drops: Arc::clone(&lease_drops),
            });
            let active = PathSocket::spawn(
                PathId::new(0),
                client_addr,
                server_addr,
                1,
                PathSocketRole::Active,
                client_socket,
                DirectEgressLease::hold_for_generation(LeaseDrop(lease_drops), 1),
                quality.clone(),
                UdpReceivePool::default(),
            )
            .unwrap();
            let paths = PathSocketSet::with_active(active).unwrap();
            let actor = MigrationActor::new(
                protector.clone(),
                quality.clone(),
                Some(ConnectionAttemptTelemetry::new(
                    telemetry.clone(),
                    Transport::Http3,
                    AddressFamily::Ipv4,
                )),
                1,
            );
            let wire_queue = quality.register_queue(
                QueueKind::H3WireSend,
                MAX_PENDING_WIRE_DATAGRAMS,
                MAX_PENDING_WIRE_DATAGRAMS * IPV4_MAX_UDP_PAYLOAD,
            );
            Self {
                client,
                server,
                server_socket,
                paths,
                actor,
                protector,
                telemetry,
                quality,
                wire: VecDeque::new(),
                free: Vec::new(),
                wire_queue,
                pmtu: PmtuController::new(PmtuPathKey::new(client_addr, server_addr)),
                cancel: CancellationToken::new(),
            }
        }

        async fn request(&mut self, generation: u64) -> oneshot::Receiver<H3MigrationResult> {
            let now = Instant::now();
            let (reply, response) = oneshot::channel();
            self.actor
                .handle_command(
                    H3ControlCommand::Migrate {
                        target_generation: generation,
                        requested_at: now,
                        deadline: now + MIGRATION_TIMEOUT,
                        reply,
                    },
                    &mut self.client,
                    &mut self.paths,
                )
                .await;
            response
        }

        async fn prepare(&mut self) {
            assert!(self.actor.is_preparing());
            let result = self.actor.wait_prepared().await;
            self.actor.on_prepared(result, &mut self.paths).await;
        }

        async fn tick(&mut self) {
            self.actor
                .tick(MigrationDrive {
                    connection: &mut self.client,
                    paths: &mut self.paths,
                    wire_datagrams: &mut self.wire,
                    free_wire_buffers: &mut self.free,
                    wire_queue: &self.wire_queue,
                    pmtu: &mut self.pmtu,
                    family_ceiling: IPV4_MAX_UDP_PAYLOAD,
                    io_cancel: &self.cancel,
                })
                .await
                .unwrap();
        }

        async fn observe_peer_wire(&mut self) -> Vec<WireObservation> {
            let mut observed = Vec::new();
            for _ in 0..64 {
                let mut wire = vec![0_u8; 2_048];
                let Ok(received) = timeout(
                    Duration::from_millis(5),
                    self.server_socket.recv_from(&mut wire),
                )
                .await
                else {
                    break;
                };
                let (length, source) = received.unwrap();
                self.server
                    .recv(
                        &mut wire[..length],
                        quiche::RecvInfo {
                            from: source,
                            to: self.server_socket.local_addr().unwrap(),
                        },
                    )
                    .unwrap();
                let mut application_datagrams = Vec::new();
                while let Ok(datagram) = self.server.dgram_recv_buf() {
                    application_datagrams.push(datagram.as_ref().to_vec());
                }
                observed.push(WireObservation {
                    source,
                    application_datagrams,
                });
            }
            observed
        }

        async fn return_peer_flight(&mut self) {
            let mut sent = 0;
            for _ in 0..64 {
                let mut wire = vec![0_u8; 65_535];
                match self.server.send(&mut wire) {
                    Ok((length, info)) => {
                        self.server_socket
                            .send_to(&wire[..length], info.to)
                            .await
                            .unwrap();
                        sent += 1;
                    }
                    Err(quiche::Error::Done) => break,
                    Err(error) => panic!("test peer send failed: {error:?}"),
                }
            }
            let mut received = 0;
            while received < sent {
                let event = timeout(Duration::from_secs(1), self.paths.recv_any())
                    .await
                    .unwrap();
                match event {
                    PathReceiveEvent::Batch { mut batch, .. } => {
                        received += batch.len();
                        for mut datagram in batch.drain() {
                            let source = datagram.source;
                            let destination = datagram.destination;
                            receive_quic_datagram(
                                &mut self.client,
                                datagram.payload_mut(),
                                source,
                                destination,
                            )
                            .unwrap();
                        }
                    }
                    PathReceiveEvent::Failed { error, .. } => {
                        panic!("test client receive failed: {error}")
                    }
                }
            }
        }

        fn queue_application_marker(&mut self, marker: u8) -> Vec<u8> {
            let mut packet = ipv4_packet_with_length(64);
            packet[63] = marker;
            let encoded = encode_for_test(0, &packet);
            let expected = encoded.as_ref().to_vec();
            self.client.dgram_send(encoded.as_ref()).unwrap();
            expected
        }

        async fn flush_active(&mut self) {
            for _ in 0..64 {
                let active = self.paths.active().unwrap().binding();
                let quantum = self.client.send_quantum();
                generate_wire_datagrams(
                    &mut self.client,
                    &mut self.wire,
                    &mut self.free,
                    quantum,
                    IPV4_MAX_UDP_PAYLOAD,
                    &self.wire_queue,
                    &self.quality,
                    active,
                )
                .unwrap();
                if let Some(first) = self.wire.front() {
                    sleep_until(Instant::from_std(first.send_info.at)).await;
                    assert_eq!(
                        send_due_wire_datagrams(
                            &self.paths,
                            &mut self.wire,
                            &mut self.free,
                            quantum,
                            &self.wire_queue,
                            &self.quality,
                            &self.cancel
                        )
                        .await
                        .unwrap(),
                        WireSendOutcome::Sent
                    );
                }
                if self.wire.is_empty() && self.client.dgram_send_queue_len() == 0 {
                    return;
                }
            }
            panic!("test active output did not drain")
        }

        async fn close(mut self) -> usize {
            self.actor
                .abort(MigrationReasonCode::ConnectionClosed, &mut self.paths)
                .await;
            self.paths.shutdown_all().await;
            self.protector.lease_drops.load(Ordering::Acquire)
        }
    }

    #[tokio::test]
    async fn inv_migration_candidate_no_app_send_validated_atomic_promotion_and_grace() {
        let mut harness = Harness::new(true).await;
        let connection_id = NetworkQualitySampler::new(harness.quality.clone())
            .sample()
            .connection_id;
        let old = harness.paths.active().unwrap().binding();
        harness.protector.generation.store(2, Ordering::Release);
        let response = harness.request(2).await;
        harness.prepare().await;
        let candidate = harness.paths.candidate().unwrap().binding();
        assert_eq!(harness.paths.active().unwrap().network_generation, 1);
        let before_marker = harness.queue_application_marker(1);
        harness.tick().await;
        let observed = harness.observe_peer_wire().await;
        assert!(
            observed
                .iter()
                .any(|packet| packet.source.port() == old.local_addr.port()
                    && packet.application_datagrams.contains(&before_marker))
        );
        assert!(
            observed
                .iter()
                .any(|packet| packet.source.port() == candidate.local_addr.port())
        );
        assert!(
            observed
                .iter()
                .filter(|packet| packet.source.port() == candidate.local_addr.port())
                .all(|packet| packet.application_datagrams.is_empty())
        );
        assert_eq!(harness.paths.active().unwrap().network_generation, 1);
        harness.return_peer_flight().await;
        for _ in 0..8 {
            harness.tick().await;
            if harness.actor.pending.is_none() {
                break;
            }
            harness.observe_peer_wire().await;
            harness.return_peer_flight().await;
        }
        assert_eq!(
            migration_reply(response).await,
            H3MigrationResult::Promoted {
                network_generation: 2
            }
        );
        assert_eq!(harness.paths.active().unwrap().path_id, candidate.path_id);
        assert_eq!(harness.paths.active().unwrap().network_generation, 2);
        assert_eq!(harness.paths.retiring().unwrap().path_id, old.path_id);
        assert_eq!(
            harness
                .paths
                .io_for_send(old.local_addr, old.peer_addr)
                .unwrap_err(),
            PathSocketSetError::RetiringSendForbidden
        );
        let quality = NetworkQualitySampler::new(harness.quality.clone()).sample();
        assert_eq!(quality.connection_id, connection_id);
        assert_eq!(quality.migration.successes, 1);
        assert_eq!(quality.pmtu.phase, PmtuPhase::Revalidating);
        let old_sent = harness
            .client
            .path_stats()
            .find(|path| path.local_addr == old.local_addr)
            .unwrap()
            .sent;
        // The promotion drain can leave already-sent old-path ACKs in the
        // peer's UDP queue. Arrival after promotion is not a new old-path
        // send; account for that flight before adding the new marker.
        harness.observe_peer_wire().await;
        let after_marker = harness.queue_application_marker(2);
        harness.flush_active().await;
        let observed = harness.observe_peer_wire().await;
        assert!(
            observed
                .iter()
                .filter(|packet| packet.application_datagrams.contains(&after_marker))
                .all(|packet| packet.source.port() == candidate.local_addr.port())
        );
        assert!(
            observed
                .iter()
                .any(|packet| packet.source.port() == candidate.local_addr.port()
                    && packet.application_datagrams.contains(&after_marker))
        );
        assert_eq!(
            harness
                .client
                .path_stats()
                .find(|path| path.local_addr == old.local_addr)
                .unwrap()
                .sent,
            old_sent
        );
        let timeline = harness.telemetry.snapshot();
        let migration_events: Vec<_> = timeline
            .events
            .iter()
            .filter_map(|event| match event.event_type {
                ConnectionEventType::MigrationStarted
                | ConnectionEventType::MigrationPathValidated
                | ConnectionEventType::MigrationPromoted
                | ConnectionEventType::MigrationFailed => Some(event.event_type),
                _ => None,
            })
            .collect();
        assert_eq!(
            migration_events,
            [
                ConnectionEventType::MigrationStarted,
                ConnectionEventType::MigrationPathValidated,
                ConnectionEventType::MigrationPromoted
            ]
        );
        assert_eq!(
            timeline
                .events
                .iter()
                .filter(|event| event.event_type == ConnectionEventType::PmtuRevalidationStarted)
                .count(),
            1
        );
        harness.actor.retiring_deadline = Some(Instant::now());
        harness.tick().await;
        assert!(harness.paths.retiring().is_none());
        assert_eq!(harness.protector.lease_drops.load(Ordering::Acquire), 1);
        assert_eq!(harness.close().await, 2);
    }

    #[tokio::test]
    async fn family_withdrawal_and_peer_cid_shortage_fail_before_socket_preparation() {
        for (with_spares, family_available, reason) in [
            (true, false, MigrationReasonCode::FamilyUnavailable),
            (false, true, MigrationReasonCode::PeerCidUnavailable),
        ] {
            let mut harness = Harness::new(with_spares).await;
            harness.protector.generation.store(2, Ordering::Release);
            harness
                .protector
                .family_available
                .store(family_available, Ordering::Release);
            let response = harness.request(2).await;
            assert_eq!(
                migration_reply(response).await,
                H3MigrationResult::Failed(reason)
            );
            assert_eq!(harness.protector.calls.load(Ordering::Acquire), 0);
            assert_eq!(harness.paths.len(), 1);
            assert_eq!(harness.close().await, 1);
        }
    }

    #[tokio::test]
    async fn emsgsize_revalidation_failure_during_migration_releases_candidate_without_promotion() {
        use crate::{FaultKind, FaultScript, ScheduledFault};
        let mut harness = Harness::new(true).await;
        harness.protector.generation.store(2, Ordering::Release);
        let response = harness.request(2).await;
        harness.prepare().await;
        harness.quality.inject_fault_script(
            FaultScript::new(
                12,
                vec![ScheduledFault {
                    at: Duration::ZERO,
                    fault: FaultKind::PmtuRevalidationFailure,
                }],
            )
            .unwrap(),
        );
        assert!(matches!(
            crate::h3::handle_pmtu_send_too_large(
                &mut harness.client,
                &mut harness.pmtu,
                None,
                &harness.quality
            ),
            Err(TransportError::PmtuRevalidationExhausted)
        ));
        harness
            .actor
            .abort(MigrationReasonCode::ConnectionClosed, &mut harness.paths)
            .await;
        assert_eq!(
            migration_reply(response).await,
            H3MigrationResult::Failed(MigrationReasonCode::ConnectionClosed)
        );
        assert_eq!(
            NetworkQualitySampler::new(harness.quality.clone())
                .sample()
                .pmtu
                .revalidation_failure_count,
            1
        );
        assert_eq!(harness.paths.len(), 1);
        assert_eq!(harness.close().await, 2);
    }

    #[tokio::test]
    async fn canonical_fault_catalog_cleans_candidates_and_retains_active_path() {
        use crate::{FaultKind, FaultScript, ScheduledFault};
        for (fault, reason) in [
            (
                FaultKind::PeerCidUnavailable,
                MigrationReasonCode::PeerCidUnavailable,
            ),
            (
                FaultKind::GenerationDuringCandidateSetup,
                MigrationReasonCode::GenerationChangedDuringSetup,
            ),
            (
                FaultKind::PathValidationTimeout,
                MigrationReasonCode::PathValidationTimeout,
            ),
            (
                FaultKind::PathValidationRejected,
                MigrationReasonCode::PathProbeRejected,
            ),
        ] {
            let mut harness = Harness::new(true).await;
            harness.protector.generation.store(2, Ordering::Release);
            harness.quality.inject_fault_script(
                FaultScript::new(
                    12,
                    vec![ScheduledFault {
                        at: Duration::ZERO,
                        fault,
                    }],
                )
                .unwrap(),
            );
            let response = harness.request(2).await;
            if harness.actor.is_preparing() {
                harness.prepare().await;
            }
            if harness.paths.candidate().is_some() {
                harness.tick().await;
            }
            assert_eq!(
                migration_reply(response).await,
                H3MigrationResult::Failed(reason)
            );
            assert_eq!(harness.paths.len(), 1);
            assert_eq!(harness.paths.active().unwrap().network_generation, 1);
            assert!(harness.paths.retiring().is_none());
            assert_eq!(
                harness.close().await,
                if fault == FaultKind::PeerCidUnavailable {
                    1
                } else {
                    2
                }
            );
        }
    }

    #[tokio::test]
    async fn protection_failure_and_setup_generation_race_leave_only_active() {
        for race in [false, true] {
            let mut harness = Harness::new(true).await;
            harness.protector.generation.store(2, Ordering::Release);
            harness
                .protector
                .fail_protection
                .store(!race, Ordering::Release);
            harness
                .protector
                .race_preparation
                .store(race, Ordering::Release);
            let response = harness.request(2).await;
            harness.prepare().await;
            let reason = if race {
                MigrationReasonCode::GenerationChangedDuringSetup
            } else {
                MigrationReasonCode::SocketProtectFailed
            };
            assert_eq!(
                migration_reply(response).await,
                H3MigrationResult::Failed(reason)
            );
            assert_eq!(harness.paths.len(), 1);
            assert_eq!(harness.close().await, if race { 2 } else { 1 });
        }
    }

    #[tokio::test]
    async fn n_plus_two_supersedes_and_releases_n_plus_one_before_new_preparation() {
        let mut harness = Harness::new(true).await;
        harness.protector.generation.store(2, Ordering::Release);
        harness
            .protector
            .stall_preparation
            .store(true, Ordering::Release);
        let first = harness.request(2).await;
        let mut preparing = Box::pin(harness.actor.wait_prepared());
        tokio::select! {
            biased;
            result = &mut preparing => panic!("stalled preparation completed: {}", result.is_ok()),
            _ = tokio::task::yield_now() => {}
        }
        drop(preparing);
        assert_eq!(harness.protector.calls.load(Ordering::Acquire), 1);
        harness.protector.generation.store(3, Ordering::Release);
        harness
            .protector
            .stall_preparation
            .store(false, Ordering::Release);
        let second = harness.request(3).await;
        assert_eq!(
            migration_reply(first).await,
            H3MigrationResult::Failed(MigrationReasonCode::Superseded)
        );
        assert_eq!(harness.protector.lease_drops.load(Ordering::Acquire), 1);
        assert_eq!(harness.protector.calls.load(Ordering::Acquire), 1);
        harness.prepare().await;
        assert_eq!(harness.paths.candidate().unwrap().network_generation, 3);
        assert_eq!(
            migration_reply(harness.request(3).await).await,
            H3MigrationResult::StaleRequest
        );
        assert_eq!(
            migration_reply(harness.request(2).await).await,
            H3MigrationResult::StaleRequest
        );
        drop(second);
        harness.tick().await;
        assert!(harness.paths.candidate().is_none());
        assert_eq!(harness.close().await, 3);
    }

    #[tokio::test]
    async fn timeout_and_caller_cancellation_release_candidate_without_purging_application() {
        for timed_out in [false, true] {
            let mut harness = Harness::new(true).await;
            harness.protector.generation.store(2, Ordering::Release);
            let response = harness.request(2).await;
            harness.prepare().await;
            harness.queue_application_marker(3);
            if timed_out {
                harness.actor.pending.as_mut().unwrap().deadline = Instant::now();
                harness.tick().await;
                assert_eq!(
                    migration_reply(response).await,
                    H3MigrationResult::Failed(MigrationReasonCode::PathValidationTimeout)
                );
            } else {
                drop(response);
                harness.tick().await;
            }
            assert!(harness.paths.candidate().is_none());
            assert_eq!(harness.client.dgram_send_queue_len(), 1);
            assert!(harness.actor.allows_application_injection());
            assert_eq!(harness.close().await, 2);
        }
    }

    #[tokio::test(start_paused = true)]
    async fn active_drain_budget_preserves_unsent_output_and_skips_candidate() {
        let mut harness = Harness::new(true).await;
        harness.protector.generation.store(2, Ordering::Release);
        let response = harness.request(2).await;
        harness.prepare().await;
        let active = harness.paths.active().unwrap().binding();
        harness.wire.push_back(WireDatagram {
            bytes: vec![0x5a; 32],
            send_info: quiche::SendInfo {
                from: active.local_addr,
                to: active.peer_addr,
                at: StdInstant::now() + Duration::from_secs(1),
            },
            queue_entry: harness.wire_queue.start_entry(32),
        });
        harness.tick().await;
        assert_eq!(harness.wire.len(), 1);
        assert_eq!(harness.wire.front().unwrap().bytes, vec![0x5a; 32]);
        assert!(harness.actor.allows_application_injection());
        assert!(harness.actor.next_wakeup() > Instant::now());
        let mut wire = [0_u8; 64];
        assert_eq!(
            harness
                .server_socket
                .try_recv_from(&mut wire)
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::WouldBlock
        );
        drop(response);
        assert_eq!(harness.close().await, 2);
    }

    #[tokio::test]
    async fn a_late_validated_event_cannot_publish_readiness_for_an_old_generation() {
        let mut harness = Harness::new(true).await;
        harness.protector.generation.store(2, Ordering::Release);
        let response = harness.request(2).await;
        harness.prepare().await;
        let candidate = harness.paths.candidate().unwrap().binding();
        harness.tick().await;
        harness.observe_peer_wire().await;
        harness.return_peer_flight().await;
        assert_eq!(
            harness
                .client
                .is_path_validated(candidate.local_addr, candidate.peer_addr),
            Ok(true)
        );
        harness.protector.generation.store(3, Ordering::Release);
        harness
            .actor
            .process_path_events(&mut harness.client, &mut harness.paths)
            .await;
        assert_eq!(
            migration_reply(response).await,
            H3MigrationResult::Failed(MigrationReasonCode::Superseded)
        );
        assert_eq!(harness.paths.active().unwrap().network_generation, 1);
        assert!(!harness.telemetry.snapshot().events.iter().any(|event| {
            matches!(
                event.event_type,
                ConnectionEventType::MigrationPathValidated
                    | ConnectionEventType::MigrationPromoted
            )
        }));
        assert_eq!(harness.close().await, 2);
    }

    #[tokio::test]
    async fn connection_close_before_promotion_never_swaps_the_binding() {
        let mut harness = Harness::new(true).await;
        harness.protector.generation.store(2, Ordering::Release);
        let response = harness.request(2).await;
        harness.prepare().await;
        harness.tick().await;
        harness.observe_peer_wire().await;
        harness.return_peer_flight().await;
        harness.client.close(false, 0, b"").unwrap();
        harness.tick().await;
        assert_eq!(
            migration_reply(response).await,
            H3MigrationResult::Failed(MigrationReasonCode::ConnectionClosed)
        );
        assert_eq!(harness.paths.active().unwrap().network_generation, 1);
        assert!(harness.paths.retiring().is_none());
        assert_eq!(harness.close().await, 2);
    }

    #[tokio::test(start_paused = true)]
    async fn rollback_skips_migration_without_enqueuing_candidate_work() {
        let (sender, commands) = mpsc::channel(H3_CONTROL_CAPACITY);
        let handle = H3MigrationHandle::new(sender, "127.0.0.1:443".parse().unwrap(), 1, false);
        assert!(!handle.enabled());
        assert_eq!(
            handle.migrate(2).await,
            H3MigrationResult::Failed(MigrationReasonCode::Unsupported)
        );
        assert!(commands.is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn control_channel_is_capacity_one_and_caller_reply_wait_is_three_seconds() {
        let (sender, mut commands) = mpsc::channel(H3_CONTROL_CAPACITY);
        let handle = H3MigrationHandle::new(sender, "127.0.0.1:443".parse().unwrap(), 1, true);
        let started = Instant::now();
        assert_eq!(
            handle.migrate(2).await,
            H3MigrationResult::Failed(MigrationReasonCode::PathValidationTimeout)
        );
        assert_eq!(started.elapsed(), MIGRATION_TIMEOUT);
        assert_eq!(commands.len(), 1);
        let H3ControlCommand::Migrate { reply, .. } = commands.recv().await.unwrap();
        assert!(reply.is_closed());
    }

    #[test]
    fn out_of_identifiers_has_a_stable_peer_or_local_fallback_reason() {
        assert_eq!(
            probe_failure_reason(quiche::Error::OutOfIdentifiers, 0),
            MigrationReasonCode::PeerCidUnavailable
        );
        assert_eq!(
            probe_failure_reason(quiche::Error::OutOfIdentifiers, 1),
            MigrationReasonCode::LocalCidUnavailable
        );
        assert_eq!(
            probe_failure_reason(quiche::Error::InvalidState, 1),
            MigrationReasonCode::PathProbeRejected
        );
    }

    async fn echo_server(
        socket: UdpSocket,
        identity: crate::h2::MasqueTlsIdentity,
        cancellation: CancellationToken,
        connect_requests: Arc<AtomicUsize>,
    ) {
        let local = socket.local_addr().unwrap();
        let mut wire = vec![0_u8; 65_535];
        let (length, peer) = socket.recv_from(&mut wire).await.unwrap();
        let (mut config, _) = crate::h3::quic_config(&identity, IPV4_MAX_UDP_PAYLOAD).unwrap();
        let mut connection = quiche::accept_with_buf_factory::<crate::h3_buffer::H3BufferFactory>(
            &quiche::ConnectionId::from_ref(&[0x71; 20]),
            None,
            local,
            peer,
            &mut config,
        )
        .unwrap();
        connection
            .recv(
                &mut wire[..length],
                quiche::RecvInfo {
                    from: peer,
                    to: local,
                },
            )
            .unwrap();
        let mut h3_config = quiche::h3::Config::new().unwrap();
        h3_config.enable_extended_connect(true);
        let mut http3 = None;
        loop {
            if connection.is_established() {
                maintain_connection_ids(&mut connection).unwrap();
                if http3.is_none() {
                    http3 = Some(
                        quiche::h3::Connection::with_transport(&mut connection, &h3_config)
                            .unwrap(),
                    );
                }
            }
            if let Some(http3) = http3.as_mut() {
                for _ in 0..64 {
                    match http3.poll(&mut connection) {
                        Ok((stream, quiche::h3::Event::Headers { .. })) => {
                            connect_requests.fetch_add(1, Ordering::AcqRel);
                            http3
                                .send_response(
                                    &mut connection,
                                    stream,
                                    &[
                                        quiche::h3::Header::new(b":status", b"200"),
                                        quiche::h3::Header::new(b"capsule-protocol", b"?1"),
                                    ],
                                    false,
                                )
                                .unwrap();
                        }
                        Ok((stream, quiche::h3::Event::Data)) => {
                            let mut body = [0_u8; 2_048];
                            for _ in 0..64 {
                                match http3.recv_body(&mut connection, stream, &mut body) {
                                    Ok(0) | Err(quiche::h3::Error::Done) => break,
                                    Ok(_) => {}
                                    Err(error) => panic!("test CONNECT body failed: {error:?}"),
                                }
                            }
                        }
                        Ok(_) => {}
                        Err(quiche::h3::Error::Done) => break,
                        Err(error) => panic!("test HTTP/3 peer failed: {error:?}"),
                    }
                }
            }
            for _ in 0..64 {
                let Ok(datagram) = connection.dgram_recv_buf() else {
                    break;
                };
                connection.dgram_send_buf(datagram).unwrap();
            }
            for _ in 0..64 {
                match connection.send(&mut wire) {
                    Ok((length, info)) => {
                        socket.send_to(&wire[..length], info.to).await.unwrap();
                    }
                    Err(quiche::Error::Done) => break,
                    Err(error) => panic!("test QUIC peer failed: {error:?}"),
                }
            }
            if connection.is_closed() {
                return;
            }
            let deadline = Instant::now() + connection.timeout().unwrap_or(Duration::from_secs(1));
            tokio::select! {
                _ = cancellation.cancelled() => return,
                received = socket.recv_from(&mut wire) => {
                    let (length, from) = received.unwrap();
                    connection.recv(&mut wire[..length], quiche::RecvInfo { from, to: local }).unwrap();
                }
                _ = sleep_until(deadline) => connection.on_timeout(),
            }
        }
    }

    #[tokio::test]
    async fn full_h3_actor_keeps_one_connect_ip_session_across_migration_and_cleans_up() {
        let client_key = usque_core::MasqueKeyPair::generate();
        let server_key = usque_core::MasqueKeyPair::generate();
        let client_identity = crate::h2::MasqueTlsIdentity::new(
            client_key.private_sec1_der().unwrap(),
            &server_key.public_spki_der().unwrap(),
            "172.16.0.2".parse().unwrap(),
            "2606:4700:110:8f13::2".parse().unwrap(),
        )
        .unwrap();
        let server_identity = crate::h2::MasqueTlsIdentity::new(
            server_key.private_sec1_der().unwrap(),
            &client_key.public_spki_der().unwrap(),
            "172.16.0.3".parse().unwrap(),
            "2606:4700:110:8f13::3".parse().unwrap(),
        )
        .unwrap();
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let endpoint = socket.local_addr().unwrap();
        let server_cancel = CancellationToken::new();
        let connect_requests = Arc::new(AtomicUsize::new(0));
        let server = tokio_util::task::AbortOnDropHandle::new(tokio::spawn(echo_server(
            socket,
            server_identity,
            server_cancel.clone(),
            Arc::clone(&connect_requests),
        )));
        let protector = Arc::new(TestProtector {
            generation: AtomicU64::new(1),
            family_available: AtomicBool::new(true),
            fail_protection: AtomicBool::new(false),
            stall_preparation: AtomicBool::new(false),
            race_preparation: AtomicBool::new(false),
            calls: AtomicUsize::new(0),
            lease_drops: Arc::new(AtomicUsize::new(0)),
        });
        let telemetry = ConnectionTelemetry::default();
        let attempt = ConnectionAttemptTelemetry::new(
            telemetry.clone(),
            Transport::Http3,
            AddressFamily::Ipv4,
        );
        let tunnel = timeout(
            Duration::from_secs(3),
            crate::h3::connect_h3_with_protector(
                endpoint,
                "migration.test",
                &client_identity,
                1_400,
                protector.clone(),
                Some(&attempt),
            ),
        )
        .await
        .unwrap()
        .unwrap();
        telemetry.record_tunnel_ready(Transport::Http3, AddressFamily::Ipv4, Duration::ZERO);
        let quality = telemetry.network_quality();
        let connection_id = NetworkQualitySampler::new(quality.clone())
            .sample()
            .connection_id;
        let migration = tunnel.migration_handle();
        let (mut send, mut receive, driver, _control) = tunnel.into_parts();
        let mut packet = ipv4_packet_with_length(64);
        packet[63] = 1;
        send.send_packet(&packet).await.unwrap();
        assert_eq!(
            timeout(Duration::from_secs(1), receive.receive_packet())
                .await
                .unwrap()
                .unwrap()
                .as_ref(),
            packet
        );
        protector.generation.store(2, Ordering::Release);
        assert_eq!(
            migration.migrate(2).await,
            H3MigrationResult::Promoted {
                network_generation: 2
            }
        );
        packet[63] = 2;
        send.send_packet(&packet).await.unwrap();
        assert_eq!(
            timeout(Duration::from_secs(1), receive.receive_packet())
                .await
                .unwrap()
                .unwrap()
                .as_ref(),
            packet
        );
        assert_eq!(connect_requests.load(Ordering::Acquire), 1);
        assert_eq!(
            NetworkQualitySampler::new(quality).sample().connection_id,
            connection_id
        );
        send.close();
        drop(driver);
        server_cancel.cancel();
        timeout(Duration::from_secs(1), server)
            .await
            .unwrap()
            .unwrap();
        timeout(Duration::from_secs(1), async {
            while protector.lease_drops.load(Ordering::Acquire) != 2 {
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        })
        .await
        .unwrap();
    }
}
