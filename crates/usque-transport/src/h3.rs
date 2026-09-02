use std::collections::VecDeque;
use std::future::Future;
use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket as StdUdpSocket};
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant as StdInstant};

use boring::ssl::{SslContextBuilder, SslMethod};
use bytes::Bytes;
use quiche::h3::NameValue;
use thiserror::Error;
use tokio::net::UdpSocket;
use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::JoinHandle;
use tokio::time::{Instant, MissedTickBehavior, interval_at, sleep_until, timeout};
use tokio_util::sync::CancellationToken;
use tokio_util::task::AbortOnDropHandle;
use usque_core::TransportStage;
use usque_protocol::{IpDatagram, MAX_CAPSULE_PAYLOAD, PeerNetworkState};

use crate::connect_ip_control::{ConnectIpControlPlane, PendingControlCapsule};
use crate::h2::{
    MasqueTlsIdentity, PinState, TransportError, configure_client_identity_and_pin,
    validate_ip_packet,
};
use crate::h3_buffer::{
    DatagramEncodePool, H3BufferFactory, HTTP_DATAGRAM_BUFFER_CAPACITY, PooledDatagramBuffer,
};
#[cfg(test)]
use crate::migration_barrier::MigrationTxBarrier;
use crate::network_quality::{H3MetricsSample, MigrationReasonCode, NetworkQualityTelemetry};
use crate::packet_batch::{MAX_PACKET_BATCH_PACKETS, PacketBatch, PacketBatchResult};
use crate::path_socket::{
    PathId, PathReceiveEvent, PathSocket, PathSocketRole, PathSocketSet, PathSocketSetError,
};
use crate::pmtu::{
    INITIAL_SAFE_UDP_PAYLOAD, PMTUD_MAX_PROBES, PmtuController, PmtuObservation, PmtuPathKey,
    PmtuRevalidationAction, family_udp_payload_ceiling,
};
use crate::queue_metrics::{QueueEntry, QueueKind, QueueMetrics};
use crate::socket::{
    DirectEgressLease, DirectProtocol, STALE_GENERATION_REASON, SocketProtector,
    noop_socket_protector, socket_handle,
};
use crate::telemetry::{ConnectionAttemptTelemetry, ConnectionEventType};
use crate::udp_io::{SendDatagram, UDP_ACTOR_DRAIN_LIMIT, UdpReceivePool, is_message_too_long};

pub(crate) mod diagnostic;
#[cfg(test)]
mod diagnostic_tests;
mod migration;
#[cfg(test)]
mod pmtu_tests;
use migration::{H3_CONTROL_CAPACITY, H3ControlCommand, MigrationActor, MigrationDrive};
pub use migration::{H3MigrationHandle, H3MigrationResult};

const CONNECT_AUTHORITY: &[u8] = b"cloudflareaccess.com";
const CONNECT_PATH: &[u8] = b"/";
const CONNECT_PROTOCOL: &[u8] = b"cf-connect-ip";
const CAPSULE_PROTOCOL_HEADER: &[u8] = b"capsule-protocol";
const CAPSULE_PROTOCOL_VALUE: &[u8] = b"?1";
const CONNECTION_ID_LENGTH: usize = 20;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(8);
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(30);
const MAX_IDLE_TIMEOUT_MS: u64 = 90_000;
const DATAGRAM_SEND_QUEUE_CAPACITY: usize = 1_024;
const DATAGRAM_RECV_QUEUE_CAPACITY: usize = MAX_PACKET_BATCH_PACKETS;
const INBOUND_PACKET_CAPACITY: usize = 1_024;
const INBOUND_RESERVED_BATCHES: usize = 3;
const INCOMING_BATCH_CHANNEL_CAPACITY: usize =
    INBOUND_PACKET_CAPACITY / MAX_PACKET_BATCH_PACKETS - INBOUND_RESERVED_BATCHES;
const OUTGOING_BATCH_CHANNEL_CAPACITY: usize = 1;
const MAX_PENDING_WIRE_DATAGRAMS: usize = 64;
const PACKET_SEND_TIMEOUT: Duration = Duration::from_secs(10);
const QUALITY_SAMPLE_INTERVAL: Duration = Duration::from_secs(1);
const SOCKET_PREPARE_ATTEMPTS: usize = 2;
const ACTIVE_CONNECTION_ID_LIMIT: u64 = 4;
const SPARE_CONNECTION_ID_TARGET: usize = 3;
const CID_GENERATION_ATTEMPT_LIMIT: usize = 6;

type H3QuicConnection = quiche::Connection<H3BufferFactory>;

/// An established Cloudflare CONNECT-IP stream over HTTP/3 and QUIC.
pub struct H3Tunnel {
    send: H3SendHalf,
    receive: H3ReceiveHalf,
    driver: H3Driver,
    control: watch::Receiver<PeerNetworkState>,
    migration: H3MigrationHandle,
    attempt: Option<ConnectionAttemptTelemetry>,
}

impl H3Tunnel {
    pub fn into_parts(
        self,
    ) -> (
        H3SendHalf,
        H3ReceiveHalf,
        H3Driver,
        watch::Receiver<PeerNetworkState>,
    ) {
        (self.send, self.receive, self.driver, self.control)
    }

    pub fn control_state(&self) -> PeerNetworkState {
        self.control.borrow().clone()
    }

    pub fn migration_handle(&self) -> H3MigrationHandle {
        self.migration.clone()
    }

    pub(crate) fn activate_network_quality(&self) {
        if let Some(attempt) = &self.attempt {
            attempt.promote();
        }
    }
}

pub struct H3SendHalf {
    sender: Option<mpsc::Sender<OutgoingBatch>>,
}

impl H3SendHalf {
    pub async fn send_packet(&mut self, packet: &[u8]) -> Result<(), TransportError> {
        match timeout(
            PACKET_SEND_TIMEOUT,
            self.send_owned_packet_inner(Bytes::copy_from_slice(packet)),
        )
        .await
        {
            Ok(result) => result,
            Err(_) => Err(TransportError::SendTimeout),
        }
    }

    async fn send_owned_packet_inner(&mut self, packet: Bytes) -> Result<(), TransportError> {
        validate_ip_packet(&packet)?;
        let mut result = self.send_owned_batch(PacketBatch::single(packet)).await?;
        if let Some((_packet, maximum_packet_size)) = result.oversized.pop() {
            return Err(TransportError::Http3DatagramTooLarge {
                maximum_packet_size,
            });
        }
        Ok(())
    }

    pub(crate) async fn send_owned_batch(
        &self,
        batch: PacketBatch,
    ) -> Result<PacketBatchResult, TransportError> {
        self.start_owned_batch(batch).await
    }

    pub(crate) fn start_owned_batch(
        &self,
        batch: PacketBatch,
    ) -> Pin<Box<dyn Future<Output = Result<PacketBatchResult, TransportError>> + Send + 'static>>
    {
        let sender = self.sender.clone();
        Box::pin(async move {
            if batch.is_empty() {
                return Ok(PacketBatchResult::default());
            }
            for packet in batch.iter() {
                validate_ip_packet(packet)?;
            }
            let (completion_tx, completion_rx) = oneshot::channel();
            let sender = sender.ok_or(TransportError::TunnelClosed)?;
            let permit = sender
                .reserve()
                .await
                .map_err(|_| TransportError::TunnelClosed)?;
            permit.send(OutgoingBatch {
                batch,
                result: PacketBatchResult::default(),
                completion: completion_tx,
            });
            match completion_rx.await {
                Ok(result) => Ok(result),
                Err(_) => Err(TransportError::TunnelClosed),
            }
        })
    }

    pub fn close(&mut self) {
        self.sender.take();
    }
}

struct OutgoingBatch {
    batch: PacketBatch,
    result: PacketBatchResult,
    completion: oneshot::Sender<PacketBatchResult>,
}

pub struct H3ReceiveHalf {
    receiver: mpsc::Receiver<PacketBatch>,
    pending: PacketBatch,
}

impl H3ReceiveHalf {
    pub async fn receive_packet(&mut self) -> Result<Bytes, TransportError> {
        loop {
            if let Some(packet) = self.pending.pop_front() {
                return Ok(packet);
            }
            self.pending = self
                .receiver
                .recv()
                .await
                .ok_or(TransportError::TunnelClosed)?;
        }
    }

    pub(crate) async fn receive_batch(&mut self) -> Result<PacketBatch, TransportError> {
        if !self.pending.is_empty() {
            return Ok(std::mem::take(&mut self.pending));
        }
        self.receiver
            .recv()
            .await
            .ok_or(TransportError::TunnelClosed)
    }
}

pub struct H3Driver {
    task: Option<JoinHandle<Result<(), TransportError>>>,
}

impl H3Driver {
    pub async fn wait(mut self) -> Result<(), TransportError> {
        let task = self
            .task
            .take()
            .expect("H3 driver task is present until wait");
        AbortOnDropHandle::new(task)
            .await
            .map_err(|error| TransportError::Http3(format!("driver task failed: {error}")))?
    }

    pub fn abort(&self) {
        if let Some(task) = &self.task {
            task.abort();
        }
    }
}

impl Drop for H3Driver {
    fn drop(&mut self) {
        if let Some(task) = &self.task {
            task.abort();
        }
    }
}

struct PreparedPathSocket {
    socket: UdpSocket,
    local_addr: SocketAddr,
    peer_addr: SocketAddr,
    network_generation: u64,
    egress_lease: DirectEgressLease,
}

#[derive(Debug, Error)]
enum SocketPrepareError {
    #[error("stale_generation")]
    StaleGeneration,
    #[error("socket preparation failed: {0}")]
    Io(#[from] io::Error),
    #[error("socket protection failed: {0}")]
    Protection(String),
}

impl SocketPrepareError {
    fn into_transport_error(self) -> TransportError {
        match self {
            Self::StaleGeneration => TransportError::UnderlyingNetworkChanged,
            Self::Io(error) => TransportError::Io(error),
            Self::Protection(error) => TransportError::SocketProtection(error),
        }
    }
}

async fn prepare_initial_udp_socket(
    target: SocketAddr,
    protector: &dyn SocketProtector,
) -> Result<PreparedPathSocket, SocketPrepareError> {
    for _ in 0..SOCKET_PREPARE_ATTEMPTS {
        let expected_generation = protector.network_generation().unwrap_or_default();
        match prepare_udp_for_generation(target, expected_generation, protector).await {
            Ok(prepared) => return Ok(prepared),
            Err(SocketPrepareError::StaleGeneration) => continue,
            Err(error) => return Err(error),
        }
    }
    Err(SocketPrepareError::StaleGeneration)
}

async fn prepare_udp_for_generation(
    target: SocketAddr,
    expected_generation: u64,
    protector: &dyn SocketProtector,
) -> Result<PreparedPathSocket, SocketPrepareError> {
    if protector.network_generation().unwrap_or_default() != expected_generation {
        return Err(SocketPrepareError::StaleGeneration);
    }
    let bind_address = match target {
        SocketAddr::V4(_) => SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0),
        SocketAddr::V6(_) => SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), 0),
    };
    let std_socket = StdUdpSocket::bind(bind_address)?;
    std_socket.set_nonblocking(true)?;
    let egress_lease = protector
        .protect_for_target_generation(
            socket_handle(&std_socket),
            target,
            DirectProtocol::Udp,
            expected_generation,
        )
        .await
        .map_err(|error| {
            if error == STALE_GENERATION_REASON {
                SocketPrepareError::StaleGeneration
            } else {
                SocketPrepareError::Protection(error)
            }
        })?;
    if protector.network_generation().unwrap_or_default() != expected_generation
        || egress_lease.generation() != Some(expected_generation)
    {
        drop(std_socket);
        drop(egress_lease);
        return Err(SocketPrepareError::StaleGeneration);
    }
    let socket = UdpSocket::from_std(std_socket)?;
    let local_addr = socket.local_addr()?;
    if protector.network_generation().unwrap_or_default() != expected_generation {
        drop(socket);
        drop(egress_lease);
        return Err(SocketPrepareError::StaleGeneration);
    }
    Ok(PreparedPathSocket {
        socket,
        local_addr,
        peer_addr: target,
        network_generation: expected_generation,
        egress_lease,
    })
}

pub async fn connect_h3(
    endpoint: SocketAddr,
    sni: &str,
    identity: &MasqueTlsIdentity,
) -> Result<H3Tunnel, TransportError> {
    connect_h3_with_protector(
        endpoint,
        sni,
        identity,
        usize::from(usque_core::config::DEFAULT_MTU),
        noop_socket_protector(),
        None,
    )
    .await
}

pub(crate) async fn connect_h3_with_protector(
    endpoint: SocketAddr,
    sni: &str,
    identity: &MasqueTlsIdentity,
    profile_inner_mtu: usize,
    protector: Arc<dyn SocketProtector>,
    attempt: Option<&ConnectionAttemptTelemetry>,
) -> Result<H3Tunnel, TransportError> {
    let first = connect_h3_once(
        endpoint,
        sni,
        identity,
        profile_inner_mtu,
        Arc::clone(&protector),
        attempt,
    )
    .await;
    match first {
        Err(TransportError::Http3ProtocolViolation(_)) => {
            // The Go oracle retries this specific Cloudflare interoperability
            // failure once. All other failures preserve normal fallback rules.
            connect_h3_once(
                endpoint,
                sni,
                identity,
                profile_inner_mtu,
                protector,
                attempt,
            )
            .await
        }
        result => result,
    }
}

async fn connect_h3_once(
    endpoint: SocketAddr,
    sni: &str,
    identity: &MasqueTlsIdentity,
    profile_inner_mtu: usize,
    protector: Arc<dyn SocketProtector>,
    attempt: Option<&ConnectionAttemptTelemetry>,
) -> Result<H3Tunnel, TransportError> {
    let prepared = prepare_initial_udp_socket(endpoint, protector.as_ref())
        .await
        .map_err(SocketPrepareError::into_transport_error)?;
    let local_address = prepared.local_addr;
    let initial_generation = prepared.network_generation;
    let migration_generation = protector.network_generation().map(|_| initial_generation);
    if let Some(attempt) = attempt {
        attempt.record(
            ConnectionEventType::SocketConnected,
            TransportStage::SocketConnect,
        );
    }

    let family_ceiling = family_udp_payload_ceiling(endpoint);
    let quality = attempt
        .map(ConnectionAttemptTelemetry::quality)
        .unwrap_or_default();
    let features = quality.features();
    let (mut quic_config, pin_state) =
        quic_config_with_features(identity, family_ceiling, features)?;
    let mut source_connection_id = [0u8; CONNECTION_ID_LENGTH];
    boring::rand::rand_bytes(&mut source_connection_id)?;
    let source_connection_id = quiche::ConnectionId::from_ref(&source_connection_id);
    let connection = quiche::connect_with_buffer_factory::<H3BufferFactory>(
        Some(sni),
        &source_connection_id,
        local_address,
        endpoint,
        &mut quic_config,
    )
    .map_err(|error| TransportError::Http3(format!("create QUIC connection: {error:?}")))?;

    let mut h3_config = quiche::h3::Config::new()
        .map_err(|error| TransportError::Http3(format!("create HTTP/3 config: {error:?}")))?;
    h3_config.enable_extended_connect(true);
    // Match the oracle's DisableCompression behavior.
    h3_config.set_qpack_max_table_capacity(0);
    h3_config.set_qpack_blocked_streams(0);

    let datagram_queue = quality.register_queue(
        QueueKind::H3DatagramSend,
        DATAGRAM_SEND_QUEUE_CAPACITY,
        DATAGRAM_SEND_QUEUE_CAPACITY * family_ceiling,
    );
    let wire_queue = quality.register_queue(
        QueueKind::H3WireSend,
        MAX_PENDING_WIRE_DATAGRAMS,
        MAX_PENDING_WIRE_DATAGRAMS * family_ceiling,
    );

    let (outgoing_tx, outgoing_rx) = mpsc::channel(OUTGOING_BATCH_CHANNEL_CAPACITY);
    let (migration_tx, migration_rx) = mpsc::channel(H3_CONTROL_CAPACITY);
    let (incoming_tx, incoming_rx) = mpsc::channel(INCOMING_BATCH_CHANNEL_CAPACITY);
    let (control_tx, control_rx) = watch::channel(PeerNetworkState::default());
    let (startup_tx, startup_rx) = oneshot::channel();
    let active_path = PathSocket::spawn(
        PathId::new(0),
        prepared.local_addr,
        prepared.peer_addr,
        prepared.network_generation,
        PathSocketRole::Active,
        prepared.socket,
        prepared.egress_lease,
        quality.clone(),
        UdpReceivePool::default(),
    )?;
    let path_sockets = PathSocketSet::with_active(active_path)
        .map_err(|error| TransportError::Http3(error.to_string()))?;
    let task = AbortOnDropHandle::new(tokio::spawn(run_h3_actor(
        path_sockets,
        connection,
        h3_config,
        outgoing_rx,
        migration_rx,
        protector,
        incoming_tx,
        control_tx,
        startup_tx,
        attempt.cloned(),
        quality,
        datagram_queue,
        wire_queue,
        profile_inner_mtu,
        family_ceiling,
        PmtuPathKey::new(local_address, endpoint),
    )));

    let startup = timeout(CONNECT_TIMEOUT, startup_rx).await;
    match startup {
        Ok(Ok(Ok(()))) => Ok(H3Tunnel {
            send: H3SendHalf {
                sender: Some(outgoing_tx),
            },
            receive: H3ReceiveHalf {
                receiver: incoming_rx,
                pending: PacketBatch::new(),
            },
            driver: H3Driver {
                task: Some(task.detach()),
            },
            control: control_rx,
            migration: H3MigrationHandle::new(
                migration_tx,
                endpoint,
                migration_generation,
                features.quic_migration,
            ),
            attempt: attempt.cloned(),
        }),
        Ok(Ok(Err(failure))) => {
            task.abort();
            let _ = task.await;
            if pin_state.rejected() {
                Err(TransportError::EndpointPinMismatch)
            } else {
                Err(failure.into_transport_error())
            }
        }
        Ok(Err(_)) => {
            let result = task
                .await
                .map_err(|error| TransportError::Http3(format!("driver task failed: {error}")))?;
            if pin_state.rejected() {
                Err(TransportError::EndpointPinMismatch)
            } else {
                match result {
                    Ok(()) => Err(TransportError::Http3(
                        "connection ended before CONNECT-IP became ready".to_owned(),
                    )),
                    Err(error) => Err(error),
                }
            }
        }
        Err(_) => {
            task.abort();
            let _ = task.await;
            if pin_state.rejected() {
                Err(TransportError::EndpointPinMismatch)
            } else {
                Err(TransportError::EndpointTimeout(endpoint))
            }
        }
    }
}

fn quic_config(
    identity: &MasqueTlsIdentity,
    family_ceiling: usize,
) -> Result<(quiche::Config, Arc<PinState>), TransportError> {
    quic_config_with_features(identity, family_ceiling, crate::PRODUCTION_NETWORK_FEATURES)
}

fn quic_config_with_features(
    identity: &MasqueTlsIdentity,
    family_ceiling: usize,
    features: crate::NetworkFeatureFlags,
) -> Result<(quiche::Config, Arc<PinState>), TransportError> {
    let mut tls = SslContextBuilder::new(SslMethod::tls())?;
    let pin_state = configure_client_identity_and_pin(&mut tls, identity)?;
    let mut config = quiche::Config::with_boring_ssl_ctx_builder(quiche::PROTOCOL_VERSION, tls)
        .map_err(|error| TransportError::Http3(format!("create QUIC config: {error:?}")))?;
    config
        .set_application_protos(quiche::h3::APPLICATION_PROTOCOL)
        .map_err(|error| TransportError::Http3(format!("configure H3 ALPN: {error:?}")))?;
    config.set_max_idle_timeout(MAX_IDLE_TIMEOUT_MS);
    config.set_max_recv_udp_payload_size(family_ceiling);
    config.set_max_send_udp_payload_size(if features.automatic_pmtu {
        family_ceiling
    } else {
        INITIAL_SAFE_UDP_PAYLOAD
    });
    config.discover_pmtu(features.automatic_pmtu);
    config.set_pmtud_max_probes(PMTUD_MAX_PROBES);
    config.set_initial_max_data(10_000_000);
    config.set_initial_max_stream_data_bidi_local(1_000_000);
    config.set_initial_max_stream_data_bidi_remote(1_000_000);
    config.set_initial_max_stream_data_uni(1_000_000);
    config.set_initial_max_streams_bidi(16);
    config.set_initial_max_streams_uni(16);
    config.set_disable_active_migration(!features.quic_migration);
    config.set_active_connection_id_limit(ACTIVE_CONNECTION_ID_LIMIT);
    config.enable_dgram(
        true,
        DATAGRAM_RECV_QUEUE_CAPACITY,
        DATAGRAM_SEND_QUEUE_CAPACITY,
    );
    config.set_cc_algorithm(quiche::CongestionControlAlgorithm::CUBIC);
    config.enable_pacing(true);
    Ok((config, pin_state))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CidAvailability {
    Ready,
    PeerUnavailable,
    LocalUnavailable,
}

fn maintain_connection_ids(
    connection: &mut H3QuicConnection,
) -> Result<CidAvailability, TransportError> {
    while connection.retired_scid_next().is_some() {}

    let target_active = SPARE_CONNECTION_ID_TARGET + 1;
    let mut attempts = 0usize;
    while connection.active_scids() < target_active && connection.scids_left() > 0 {
        if attempts >= CID_GENERATION_ATTEMPT_LIMIT {
            return Ok(CidAvailability::LocalUnavailable);
        }
        attempts += 1;
        let mut source_id = [0u8; CONNECTION_ID_LENGTH];
        let mut reset_token = [0u8; 16];
        boring::rand::rand_bytes(&mut source_id)?;
        boring::rand::rand_bytes(&mut reset_token)?;
        match connection.new_scid(
            &quiche::ConnectionId::from_ref(&source_id),
            u128::from_be_bytes(reset_token),
            false,
        ) {
            Ok(_) => {}
            Err(error) => return map_cid_provisioning_error(error),
        }
    }

    if connection.available_dcids() == 0 {
        Ok(CidAvailability::PeerUnavailable)
    } else if connection.active_scids() <= 1 {
        Ok(CidAvailability::LocalUnavailable)
    } else {
        Ok(CidAvailability::Ready)
    }
}

fn map_cid_provisioning_error(error: quiche::Error) -> Result<CidAvailability, TransportError> {
    match error {
        quiche::Error::IdLimit | quiche::Error::OutOfIdentifiers => {
            Ok(CidAvailability::LocalUnavailable)
        }
        error => Err(TransportError::Http3(format!(
            "provision QUIC source connection ID: {error:?}"
        ))),
    }
}

fn migration_availability_reason(
    availability: CidAvailability,
    platform_supported: bool,
) -> Option<MigrationReasonCode> {
    if !platform_supported {
        return Some(MigrationReasonCode::Unsupported);
    }
    match availability {
        CidAvailability::Ready => None,
        CidAvailability::PeerUnavailable => Some(MigrationReasonCode::PeerCidUnavailable),
        CidAvailability::LocalUnavailable => Some(MigrationReasonCode::LocalCidUnavailable),
    }
}

#[derive(Debug)]
enum StartupFailure {
    ConnectRejected(u16),
    DatagramUnavailable,
    PmtuRevalidationExhausted,
    ProtocolViolation(String),
    Other(String),
}

impl StartupFailure {
    fn from_transport_error(error: &TransportError) -> Self {
        match error {
            TransportError::Http3ConnectRejected(status) => Self::ConnectRejected(*status),
            TransportError::Http3DatagramUnavailable => Self::DatagramUnavailable,
            TransportError::PmtuRevalidationExhausted => Self::PmtuRevalidationExhausted,
            TransportError::Http3ProtocolViolation(message) => {
                Self::ProtocolViolation(message.clone())
            }
            _ => Self::Other(error.to_string()),
        }
    }

    fn into_transport_error(self) -> TransportError {
        match self {
            Self::ConnectRejected(status) => TransportError::Http3ConnectRejected(status),
            Self::DatagramUnavailable => TransportError::Http3DatagramUnavailable,
            Self::PmtuRevalidationExhausted => TransportError::PmtuRevalidationExhausted,
            Self::ProtocolViolation(message) => TransportError::Http3ProtocolViolation(message),
            Self::Other(message) => TransportError::Http3(message),
        }
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "H3 entrypoint threads socket, connection, channels, and startup oneshot into the actor"
)]
async fn run_h3_actor(
    mut path_sockets: PathSocketSet,
    connection: H3QuicConnection,
    h3_config: quiche::h3::Config,
    outgoing_rx: mpsc::Receiver<OutgoingBatch>,
    migration_rx: mpsc::Receiver<H3ControlCommand>,
    protector: Arc<dyn SocketProtector>,
    incoming_tx: mpsc::Sender<PacketBatch>,
    control_tx: watch::Sender<PeerNetworkState>,
    startup_tx: oneshot::Sender<Result<(), StartupFailure>>,
    attempt: Option<ConnectionAttemptTelemetry>,
    quality: NetworkQualityTelemetry,
    datagram_queue: Arc<QueueMetrics>,
    wire_queue: Arc<QueueMetrics>,
    profile_inner_mtu: usize,
    family_ceiling: usize,
    initial_path: PmtuPathKey,
) -> Result<(), TransportError> {
    let mut startup_tx = Some(startup_tx);
    let result = drive_h3_actor(
        &mut path_sockets,
        connection,
        h3_config,
        outgoing_rx,
        migration_rx,
        protector,
        incoming_tx,
        control_tx,
        &mut startup_tx,
        attempt.as_ref(),
        &quality,
        &datagram_queue,
        &wire_queue,
        profile_inner_mtu,
        family_ceiling,
        initial_path,
    )
    .await;
    path_sockets.shutdown_all().await;
    if let Some(startup_tx) = startup_tx.take() {
        let failure = match &result {
            Ok(()) => {
                StartupFailure::Other("connection ended before CONNECT-IP became ready".to_owned())
            }
            Err(error) => StartupFailure::from_transport_error(error),
        };
        let _ = startup_tx.send(Err(failure));
    }
    result
}

#[expect(
    clippy::too_many_arguments,
    reason = "H3 actor owns the socket, connection, packet channels, control plane, and startup handshake together"
)]
async fn drive_h3_actor(
    path_sockets: &mut PathSocketSet,
    mut connection: H3QuicConnection,
    h3_config: quiche::h3::Config,
    mut outgoing_rx: mpsc::Receiver<OutgoingBatch>,
    mut migration_rx: mpsc::Receiver<H3ControlCommand>,
    protector: Arc<dyn SocketProtector>,
    incoming_tx: mpsc::Sender<PacketBatch>,
    control_tx: watch::Sender<PeerNetworkState>,
    startup_tx: &mut Option<oneshot::Sender<Result<(), StartupFailure>>>,
    attempt: Option<&ConnectionAttemptTelemetry>,
    quality: &NetworkQualityTelemetry,
    datagram_queue: &Arc<QueueMetrics>,
    wire_queue: &Arc<QueueMetrics>,
    profile_inner_mtu: usize,
    family_ceiling: usize,
    initial_path: PmtuPathKey,
) -> Result<(), TransportError> {
    let mut http3 = None;
    let mut request_stream_id = None;
    let mut response_accepted = false;
    let mut peer_settings_recorded = false;
    let mut ready = false;
    let mut control = ConnectIpControlPlane::new(control_tx);
    let mut pending_batch: Option<OutgoingBatch> = None;
    let mut wire_datagrams = VecDeque::with_capacity(MAX_PENDING_WIRE_DATAGRAMS);
    let mut datagram_entries = VecDeque::with_capacity(DATAGRAM_SEND_QUEUE_CAPACITY);
    let encode_pool = DatagramEncodePool::new(quality.clone());
    let mut free_wire_buffers = Vec::new();
    let io_cancel = CancellationToken::new();
    let mut incoming_batch = PacketBatch::new();
    let mut inbound_queue_drop_count = 0_u64;
    let mut pmtu = PmtuController::with_automatic(initial_path, quality.features().automatic_pmtu);
    let migration_platform_supported = protector.network_generation().is_some();
    let mut migration = MigrationActor::new(
        protector,
        quality.clone(),
        attempt.cloned(),
        path_sockets
            .active()
            .expect("H3 starts with an active socket")
            .network_generation,
    );
    let mut migration_commands_open = true;
    let mut keepalive = interval_at(Instant::now() + KEEPALIVE_INTERVAL, KEEPALIVE_INTERVAL);
    keepalive.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut quality_tick = interval_at(
        Instant::now() + QUALITY_SAMPLE_INTERVAL,
        QUALITY_SAMPLE_INTERVAL,
    );
    quality_tick.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        if ready && migration_commands_open {
            match migration_rx.try_recv() {
                Ok(command) => {
                    migration
                        .handle_command(command, &mut connection, path_sockets)
                        .await
                }
                Err(mpsc::error::TryRecvError::Disconnected) => migration_commands_open = false,
                Err(mpsc::error::TryRecvError::Empty) => {}
            }
        }
        migration
            .tick(MigrationDrive {
                connection: &mut connection,
                paths: path_sockets,
                wire_datagrams: &mut wire_datagrams,
                free_wire_buffers: &mut free_wire_buffers,
                wire_queue,
                pmtu: &mut pmtu,
                family_ceiling,
                io_cancel: &io_cancel,
            })
            .await?;
        if connection.is_established() {
            quality.set_migration_availability_reason(migration_availability_reason(
                maintain_connection_ids(&mut connection)?,
                migration_platform_supported && quality.features().quic_migration,
            ));
        }
        if connection.is_established() && http3.is_none() {
            if let Some(attempt) = attempt {
                attempt.record(
                    ConnectionEventType::QuicReady,
                    TransportStage::QuicHandshake,
                );
            }
            http3 = Some(
                quiche::h3::Connection::with_transport(&mut connection, &h3_config).map_err(
                    |error| TransportError::Http3(format!("start HTTP/3 session: {error:?}")),
                )?,
            );
        }

        if let Some(http3) = http3.as_mut() {
            let response_was_accepted = response_accepted;
            process_http3_events(
                http3,
                &mut connection,
                request_stream_id,
                &mut response_accepted,
                &mut control,
            )?;
            if !response_was_accepted
                && response_accepted
                && let Some(attempt) = attempt
            {
                attempt.record(
                    ConnectionEventType::MasqueAccepted,
                    TransportStage::MasqueConnect,
                );
            }

            if let Some(stream_id) = request_stream_id {
                flush_control_capsules(http3, &mut connection, stream_id, &mut control.pending)?;
            }

            if request_stream_id.is_none() && http3.peer_settings_raw().is_some() {
                if !http3.dgram_enabled_by_peer(&connection) {
                    return Err(TransportError::Http3DatagramUnavailable);
                }
                if !peer_settings_recorded {
                    peer_settings_recorded = true;
                    if let Some(attempt) = attempt {
                        attempt.record(
                            ConnectionEventType::PeerSettingsReceived,
                            TransportStage::PeerSettings,
                        );
                    }
                }
                match http3.send_request(&mut connection, &connect_headers(), false) {
                    Ok(stream_id) => request_stream_id = Some(stream_id),
                    Err(quiche::h3::Error::StreamBlocked) => {}
                    Err(error) => {
                        return Err(TransportError::Http3(format!(
                            "send CONNECT-IP request: {error:?}"
                        )));
                    }
                }
            }

            if response_accepted && http3.dgram_enabled_by_peer(&connection) && !ready {
                ready = true;
                if let Some(startup_tx) = startup_tx.take() {
                    let _ = startup_tx.send(Ok(()));
                }
            }
        }

        if let Some(stream_id) = request_stream_id {
            drain_received_datagrams(
                &mut connection,
                stream_id,
                ready,
                &incoming_tx,
                &mut incoming_batch,
            )?;
        }

        if ready
            && migration.allows_application_injection()
            && let Some(stream_id) = request_stream_id
        {
            queue_pending_batch(
                &mut connection,
                stream_id,
                &mut pending_batch,
                &mut datagram_entries,
                datagram_queue,
                quality,
                &encode_pool,
                profile_inner_mtu,
            )?;
        }

        let send_quantum = connection.send_quantum();
        let pmtu_suppressed_until = pmtu.send_suppressed_until(StdInstant::now());
        if pmtu_suppressed_until.is_none() {
            let active = path_sockets
                .active()
                .expect("live H3 has an active socket")
                .binding();
            generate_wire_datagrams(
                &mut connection,
                &mut wire_datagrams,
                &mut free_wire_buffers,
                send_quantum,
                family_ceiling,
                wire_queue,
                quality,
                active,
            )?;
        }
        reconcile_datagram_queue(&connection, &mut datagram_entries, datagram_queue);

        if connection.is_closed() {
            return Err(connection_closed_error(&connection));
        }

        let quic_deadline =
            Instant::now() + connection.timeout().unwrap_or(Duration::from_secs(60));
        let wire_deadline = wire_datagrams
            .front()
            .map(|datagram| Instant::from_std(datagram.send_info.at))
            .unwrap_or_else(|| Instant::now() + Duration::from_secs(86_400));
        let wire_is_due = wire_datagrams
            .front()
            .is_some_and(|datagram| datagram.send_info.at <= StdInstant::now());
        let wire_fits_quantum = wire_datagrams
            .front()
            .is_some_and(|datagram| datagram.bytes.len() <= send_quantum);
        let migration_wakeup = migration.next_wakeup();
        let preparing_migration = migration.is_preparing();

        tokio::select! {
            received = path_sockets.recv_any() => {
                match received {
                    PathReceiveEvent::Batch { path_id, mut batch }
                        if path_sockets.contains(path_id) =>
                    {
                        for mut datagram in batch.drain() {
                            let source = datagram.source;
                            let destination = datagram.destination;
                            let dropped = receive_quic_datagram(
                                &mut connection,
                                datagram.payload_mut(),
                                source,
                                destination,
                            )?;
                            record_inbound_queue_drops(dropped, &mut inbound_queue_drop_count);
                        }
                    }
                    PathReceiveEvent::Failed { path_id, error }
                        if path_sockets.contains(path_id) => {
                        if !migration.handle_receive_failure(path_id, path_sockets).await {
                            return Err(error.into());
                        }
                    }
                    PathReceiveEvent::Batch { .. } | PathReceiveEvent::Failed { .. } => {
                        // A role may have been removed after its bounded receiver
                        // completed. Stale path events never reach quiche.
                    }
                }
            }
            batch = outgoing_rx.recv(), if ready
                && pending_batch.is_none()
                && migration.allows_application_injection() => {
                match batch {
                    Some(batch) => pending_batch = Some(batch),
                    None => return Ok(()),
                }
            }
            command = migration_rx.recv(), if migration_commands_open && ready => {
                match command {
                    Some(command) => migration.handle_command(command, &mut connection, path_sockets).await,
                    None => migration_commands_open = false,
                }
            }
            prepared = migration.wait_prepared(), if preparing_migration => {
                migration.on_prepared(prepared, path_sockets).await;
            }
            _ = sleep_until(migration_wakeup) => {}
            sent = send_due_wire_datagrams(
                path_sockets,
                &mut wire_datagrams,
                &mut free_wire_buffers,
                send_quantum,
                wire_queue,
                quality,
                &io_cancel,
            ), if wire_is_due && wire_fits_quantum && pmtu_suppressed_until.is_none() => {
                if sent? == WireSendOutcome::MessageTooLarge {
                    handle_pmtu_send_too_large(&mut connection, &mut pmtu, attempt, quality)?;
                }
            }
            _ = sleep_until(wire_deadline), if !wire_datagrams.is_empty() && !wire_is_due => {}
            _ = sleep_until(Instant::from_std(pmtu_suppressed_until.unwrap_or_else(StdInstant::now))), if pmtu_suppressed_until.is_some() => {}
            _ = sleep_until(quic_deadline) => connection.on_timeout(),
            _ = keepalive.tick(), if connection.is_established() => {
                connection
                    .send_ack_eliciting()
                    .map_err(|error| TransportError::Http3(format!(
                        "queue QUIC keepalive: {error:?}"
                    )))?;
            }
            _ = quality_tick.tick(), if connection.is_established() => {
                observe_h3_metrics(
                    &connection,
                    &mut pmtu,
                    attempt,
                    quality,
                    request_stream_id,
                    profile_inner_mtu,
                    inbound_queue_drop_count,
                )?;
            }
        }
    }
}

fn handle_pmtu_send_too_large(
    connection: &mut H3QuicConnection,
    pmtu: &mut PmtuController,
    attempt: Option<&ConnectionAttemptTelemetry>,
    quality: &NetworkQualityTelemetry,
) -> Result<(), TransportError> {
    let Some(path) = connection.path_stats().find(|path| path.active) else {
        return Err(TransportError::Http3(
            "active QUIC path disappeared after UDP EMSGSIZE".to_owned(),
        ));
    };
    let key = PmtuPathKey::new(path.local_addr, path.peer_addr);
    quality.record_pmtu_send_too_large();
    let action = pmtu.on_send_too_large(key, connection.pmtu(), StdInstant::now());
    #[cfg(any(test, feature = "fault-injection"))]
    let action = if quality
        .take_fault(crate::fault_injection::FaultPoint::Pmtu)
        .is_some()
    {
        match action {
            PmtuRevalidationAction::ContinueDiscovery(mut value)
            | PmtuRevalidationAction::Revalidate(mut value)
            | PmtuRevalidationAction::Exhausted(mut value) => {
                value.phase = crate::PmtuPhase::Degraded;
                PmtuRevalidationAction::Exhausted(value)
            }
        }
    } else {
        action
    };
    match action {
        PmtuRevalidationAction::ContinueDiscovery(observation) => {
            publish_pmtu_observation(quality, observation);
            Ok(())
        }
        PmtuRevalidationAction::Revalidate(observation) => {
            publish_pmtu_observation(quality, observation);
            record_pmtu_change_if_needed(attempt, observation);
            if quality.features().automatic_pmtu {
                connection.revalidate_pmtu();
            }
            if let Some(attempt) = attempt {
                attempt.record(
                    ConnectionEventType::PmtuRevalidationStarted,
                    TransportStage::PacketSend,
                );
            }
            Ok(())
        }
        PmtuRevalidationAction::Exhausted(observation) => {
            publish_pmtu_observation(quality, observation);
            record_pmtu_change_if_needed(attempt, observation);
            quality.record_pmtu_revalidation_failure();
            if let Some(attempt) = attempt {
                attempt.record(
                    ConnectionEventType::PmtuRevalidationFailed,
                    TransportStage::PacketSend,
                );
            }
            Err(TransportError::PmtuRevalidationExhausted)
        }
    }
}

fn connect_headers() -> Vec<quiche::h3::Header> {
    vec![
        quiche::h3::Header::new(b":method", b"CONNECT"),
        quiche::h3::Header::new(b":scheme", b"https"),
        quiche::h3::Header::new(b":authority", CONNECT_AUTHORITY),
        quiche::h3::Header::new(b":path", CONNECT_PATH),
        quiche::h3::Header::new(b":protocol", CONNECT_PROTOCOL),
        quiche::h3::Header::new(b"user-agent", b""),
        quiche::h3::Header::new(CAPSULE_PROTOCOL_HEADER, CAPSULE_PROTOCOL_VALUE),
    ]
}

fn process_http3_events(
    http3: &mut quiche::h3::Connection,
    connection: &mut H3QuicConnection,
    request_stream_id: Option<u64>,
    response_accepted: &mut bool,
    control: &mut ConnectIpControlPlane,
) -> Result<(), TransportError> {
    let mut body = [0u8; 4_096];
    loop {
        match http3.poll(connection) {
            Ok((
                stream_id,
                quiche::h3::Event::Headers {
                    list,
                    more_frames: _,
                },
            )) if Some(stream_id) == request_stream_id => {
                if let Some(status) = response_status(&list)? {
                    if (200..300).contains(&status) {
                        *response_accepted = true;
                    } else if status >= 200 {
                        return Err(TransportError::Http3ConnectRejected(status));
                    }
                } else if !*response_accepted {
                    return Err(TransportError::Http3(
                        "CONNECT-IP response omitted :status".to_owned(),
                    ));
                }
            }
            Ok((stream_id, quiche::h3::Event::Data)) => loop {
                match http3.recv_body(connection, stream_id, &mut body) {
                    Ok(0) => break,
                    Ok(length) => {
                        if Some(stream_id) == request_stream_id {
                            if control.buffer.len().saturating_add(length)
                                > MAX_CAPSULE_PAYLOAD + 16
                            {
                                return Err(TransportError::CapsuleTooLarge);
                            }
                            control.buffer.extend_from_slice(&body[..length]);
                            control.drain()?;
                        }
                    }
                    Err(quiche::h3::Error::Done) => break,
                    Err(error) => {
                        return Err(TransportError::Http3(format!(
                            "receive HTTP/3 response body: {error:?}"
                        )));
                    }
                }
            },
            Ok((stream_id, quiche::h3::Event::Finished))
                if Some(stream_id) == request_stream_id =>
            {
                return Err(TransportError::TunnelClosed);
            }
            Ok((stream_id, quiche::h3::Event::Reset(code)))
                if Some(stream_id) == request_stream_id =>
            {
                return Err(TransportError::Http3(format!(
                    "CONNECT-IP stream reset with code {code}"
                )));
            }
            Ok((_stream_id, quiche::h3::Event::GoAway)) => {
                return Err(TransportError::Http3("peer sent HTTP/3 GOAWAY".to_owned()));
            }
            Ok((_stream_id, quiche::h3::Event::PriorityUpdate))
            | Ok((_stream_id, quiche::h3::Event::Headers { .. }))
            | Ok((_stream_id, quiche::h3::Event::Finished))
            | Ok((_stream_id, quiche::h3::Event::Reset(_))) => {}
            Err(quiche::h3::Error::Done) => break,
            Err(error) => {
                return Err(TransportError::Http3(format!(
                    "process HTTP/3 event: {error:?}"
                )));
            }
        }
    }
    Ok(())
}

fn flush_control_capsules(
    http3: &mut quiche::h3::Connection,
    connection: &mut H3QuicConnection,
    stream_id: u64,
    pending: &mut VecDeque<PendingControlCapsule>,
) -> Result<(), TransportError> {
    while let Some(capsule) = pending.front_mut() {
        let remaining = &capsule.bytes[capsule.offset..];
        match http3.send_body(connection, stream_id, remaining, false) {
            Ok(0) | Err(quiche::h3::Error::Done) => return Ok(()),
            Ok(written) => {
                capsule.offset += written;
                if capsule.offset == capsule.bytes.len() {
                    pending.pop_front();
                }
            }
            Err(error) => {
                return Err(TransportError::Http3(format!(
                    "send CONNECT-IP control capsule: {error:?}"
                )));
            }
        }
    }
    Ok(())
}

fn response_status(headers: &[quiche::h3::Header]) -> Result<Option<u16>, TransportError> {
    let Some(value) = headers
        .iter()
        .find(|header| header.name() == b":status")
        .map(NameValue::value)
    else {
        return Ok(None);
    };
    let value = std::str::from_utf8(value)
        .map_err(|_| TransportError::Http3("response :status is not UTF-8".to_owned()))?;
    value
        .parse()
        .map(Some)
        .map_err(|_| TransportError::Http3("response :status is not numeric".to_owned()))
}

fn drain_received_datagrams(
    connection: &mut H3QuicConnection,
    request_stream_id: u64,
    ready: bool,
    incoming_tx: &mpsc::Sender<PacketBatch>,
    incoming_batch: &mut PacketBatch,
) -> Result<(), TransportError> {
    if ready && !flush_incoming_batch(incoming_tx, incoming_batch)? {
        return Ok(());
    }
    while let Some(front_len) = connection.dgram_recv_front_len() {
        if ready
            && !incoming_batch.is_empty()
            && !incoming_batch.can_accept(front_len)
            && !flush_incoming_batch(incoming_tx, incoming_batch)?
        {
            break;
        }
        let datagram = match connection.dgram_recv_buf() {
            Ok(datagram) => datagram,
            Err(quiche::Error::Done) => break,
            Err(error) => {
                return Err(TransportError::Http3(format!(
                    "receive CONNECT-IP datagram: {error:?}"
                )));
            }
        };
        if !ready {
            continue;
        }
        let Some(packet) = decode_http_datagram_bytes(request_stream_id, datagram.into_bytes())?
        else {
            continue;
        };
        if let Err(packet) = incoming_batch.push_back(packet) {
            if !flush_incoming_batch(incoming_tx, incoming_batch)? {
                return Err(TransportError::Http3(
                    "incoming batch capacity accounting failed".to_owned(),
                ));
            }
            incoming_batch.push_back(packet).map_err(|_| {
                TransportError::Http3("an inbound datagram exceeded the batch bound".to_owned())
            })?;
        }
    }
    if ready {
        let _ = flush_incoming_batch(incoming_tx, incoming_batch)?;
    }
    Ok(())
}

fn flush_incoming_batch(
    incoming_tx: &mpsc::Sender<PacketBatch>,
    incoming_batch: &mut PacketBatch,
) -> Result<bool, TransportError> {
    if incoming_batch.is_empty() {
        return Ok(true);
    }
    match incoming_tx.try_send(std::mem::take(incoming_batch)) {
        Ok(()) => Ok(true),
        Err(TrySendError::Full(batch)) => {
            *incoming_batch = batch;
            Ok(false)
        }
        Err(TrySendError::Closed(_)) => Err(TransportError::TunnelClosed),
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "the H3 queue step keeps connection, ordered accounting, telemetry, and its bounded encode pool explicit"
)]
fn queue_pending_batch(
    connection: &mut H3QuicConnection,
    stream_id: u64,
    pending_batch: &mut Option<OutgoingBatch>,
    datagram_entries: &mut VecDeque<QueueEntry>,
    datagram_queue: &Arc<QueueMetrics>,
    quality: &NetworkQualityTelemetry,
    encode_pool: &DatagramEncodePool,
    profile_inner_mtu: usize,
) -> Result<(), TransportError> {
    if pending_batch
        .as_ref()
        .is_some_and(|outgoing| outgoing.completion.is_closed())
    {
        // A send timeout/cancellation must release a batch waiting on PMTUD
        // and let the actor observe a closed producer without waiting for ACKs.
        pending_batch.take();
        return Ok(());
    }
    let completed = {
        let Some(outgoing) = pending_batch.as_mut() else {
            return Ok(());
        };
        // Visit each original entry at most once. Deferred IPv6 entries rotate
        // behind smaller packets without adding a queue or spinning on them.
        for _ in 0..outgoing.batch.len().min(UDP_ACTOR_DRAIN_LIMIT) {
            if connection.is_dgram_send_queue_full() {
                break;
            }
            let Some(packet) = outgoing.batch.front() else {
                break;
            };
            let packet_len = packet.len();
            let (maximum_packet_size, datagram_overhead) =
                connect_ip_payload_limit(connection, stream_id, profile_inner_mtu)?;
            if packet_len > maximum_packet_size {
                if quality.features().automatic_pmtu
                    && connection.pmtu().is_none()
                    && maximum_packet_size < crate::pmtu::IPV6_MINIMUM_INNER_MTU
                    && packet.first().is_some_and(|byte| byte >> 4 == 6)
                {
                    // A probing floor is not evidence that IPv6's minimum MTU
                    // is unavailable. Keep the original packet for a later
                    // probe ACK; a completed low PMTU still uses the existing
                    // fail-closed PTB/error path below. The caller's send
                    // deadline and cancellation continue to bound this wait.
                    let packet = outgoing.batch.pop_front().expect("front packet exists");
                    assert!(
                        outgoing.batch.push_back(packet).is_ok(),
                        "rotating a packet preserves the batch capacity"
                    );
                    continue;
                }
                let packet = outgoing
                    .batch
                    .pop_front()
                    .expect("front packet remains until DATAGRAM is rejected");
                outgoing
                    .result
                    .oversized
                    .push((packet, maximum_packet_size));
                continue;
            }
            let Some(datagram) = encode_http_datagram(encode_pool, stream_id, packet)? else {
                break;
            };
            let datagram_len = datagram.as_ref().len();
            quality.record_datagram_header_copy(datagram_overhead);
            match connection.dgram_send_buf(datagram) {
                Ok(()) => {
                    datagram_entries.push_back(datagram_queue.start_entry(datagram_len));
                    let packet = outgoing
                        .batch
                        .pop_front()
                        .expect("front packet remains until DATAGRAM is accepted");
                    outgoing.result.accepted_bytes =
                        outgoing.result.accepted_bytes.saturating_add(packet.len());
                }
                Err(quiche::Error::Done) => break,
                Err(quiche::Error::BufferTooShort) => {
                    let packet = outgoing
                        .batch
                        .pop_front()
                        .expect("front packet remains until DATAGRAM is rejected");
                    outgoing
                        .result
                        .oversized
                        .push((packet, maximum_packet_size));
                }
                Err(error) => {
                    return Err(TransportError::Http3(format!(
                        "queue CONNECT-IP datagram: {error:?}"
                    )));
                }
            }
        }
        outgoing.batch.is_empty()
    };
    if completed {
        let outgoing = pending_batch
            .take()
            .expect("completed outgoing batch remains present");
        let _ = outgoing.completion.send(outgoing.result);
    }
    Ok(())
}

fn connect_ip_payload_limit(
    connection: &H3QuicConnection,
    stream_id: u64,
    profile_inner_mtu: usize,
) -> Result<(usize, usize), TransportError> {
    let datagram_overhead = encoded_varint_len(stream_id / 4)?
        + encoded_varint_len(usque_protocol::DEFAULT_CONTEXT_ID)?;
    let maximum_datagram_size = connection.dgram_max_writable_len().ok_or_else(|| {
        TransportError::Http3("HTTP Datagram writable length became unavailable".to_owned())
    })?;
    Ok((
        effective_connect_ip_payload(profile_inner_mtu, maximum_datagram_size, datagram_overhead),
        datagram_overhead,
    ))
}

fn effective_connect_ip_payload(
    profile_inner_mtu: usize,
    datagram_writable_len: usize,
    context_overhead: usize,
) -> usize {
    profile_inner_mtu.min(datagram_writable_len.saturating_sub(context_overhead))
}

fn reconcile_datagram_queue(
    connection: &H3QuicConnection,
    entries: &mut VecDeque<QueueEntry>,
    metrics: &QueueMetrics,
) {
    let remaining = connection.dgram_send_queue_len();
    while entries.len() > remaining {
        if let Some(entry) = entries.pop_front() {
            entry.complete();
        }
    }
    if let Some(entry) = entries.front() {
        metrics.observe_oldest_entry(entry);
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "the one-hertz H3 observation keeps path state, negotiated limits, and telemetry explicit"
)]
fn observe_h3_metrics(
    connection: &H3QuicConnection,
    pmtu: &mut PmtuController,
    attempt: Option<&ConnectionAttemptTelemetry>,
    quality: &NetworkQualityTelemetry,
    request_stream_id: Option<u64>,
    profile_inner_mtu: usize,
    datagram_receive_drops: u64,
) -> Result<(), TransportError> {
    let Some(path) = connection.path_stats().find(|path| path.active) else {
        return Ok(());
    };
    let effective_payload = request_stream_id
        .map(|stream_id| connect_ip_payload_limit(connection, stream_id, profile_inner_mtu))
        .transpose()?
        .map(|(payload, _)| payload);
    let observation = pmtu.observe_active_path(
        PmtuPathKey::new(path.local_addr, path.peer_addr),
        connection.pmtu(),
        path.pmtu,
        effective_payload,
    );
    publish_pmtu_observation(quality, observation);
    record_pmtu_change_if_needed(attempt, observation);

    if let Some(attempt) = attempt {
        attempt.observe_h3(H3MetricsSample {
            rtt: path.rtt,
            min_rtt: path.min_rtt,
            rtt_variance: path.rttvar,
            congestion_window_bytes: usize_to_u64(path.cwnd),
            send_rate_bytes_per_second: path.delivery_rate,
            sent_packets: usize_to_u64(path.sent),
            received_packets: usize_to_u64(path.recv),
            lost_packets: usize_to_u64(path.lost),
            sent_bytes: path.sent_bytes,
            received_bytes: path.recv_bytes,
            lost_bytes: path.lost_bytes,
            pto_count: usize_to_u64(path.total_pto_count),
            datagrams_sent: usize_to_u64(path.dgram_sent),
            datagrams_received: usize_to_u64(path.dgram_recv),
            datagrams_lost: usize_to_u64(path.dgram_lost),
            datagram_receive_drops,
        });
    }
    Ok(())
}

fn publish_pmtu_observation(quality: &NetworkQualityTelemetry, observation: PmtuObservation) {
    quality.observe_pmtu(
        observation.phase,
        observation
            .outer_payload_bytes
            .map(|value| u32::try_from(value).unwrap_or(u32::MAX)),
        observation
            .effective_connect_ip_payload_bytes
            .map(|value| u32::try_from(value).unwrap_or(u32::MAX)),
    );
}

fn record_pmtu_change_if_needed(
    attempt: Option<&ConnectionAttemptTelemetry>,
    observation: PmtuObservation,
) {
    if observation.numeric_changed
        && let Some(attempt) = attempt
    {
        attempt.record(ConnectionEventType::PmtuChanged, TransportStage::PacketSend);
    }
}

fn usize_to_u64(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

fn receive_quic_datagram(
    connection: &mut H3QuicConnection,
    datagram: &mut [u8],
    from: SocketAddr,
    to: SocketAddr,
) -> Result<usize, TransportError> {
    let queued_before = connection.dgram_recv_queue_len();
    let received_before = connection.stats().dgram_recv;
    let info = quiche::RecvInfo { from, to };
    match connection.recv(datagram, info) {
        Ok(_) | Err(quiche::Error::Done) => {
            let received = connection
                .stats()
                .dgram_recv
                .saturating_sub(received_before);
            Ok(inbound_queue_overflow(queued_before, received))
        }
        Err(error) => Err(TransportError::Http3(format!(
            "receive QUIC packet: {error:?}"
        ))),
    }
}

fn inbound_queue_overflow(queued_before: usize, received: usize) -> usize {
    received.saturating_sub(DATAGRAM_RECV_QUEUE_CAPACITY.saturating_sub(queued_before))
}

fn record_inbound_queue_drops(dropped: usize, total: &mut u64) {
    for _ in 0..dropped {
        *total = total.saturating_add(1);
        if total.is_power_of_two() {
            tracing::warn!(
                dropped_datagrams = *total,
                queue_capacity = DATAGRAM_RECV_QUEUE_CAPACITY,
                "dropped inbound CONNECT-IP payload after the bounded queue saturated"
            );
        }
    }
}

struct WireDatagram {
    bytes: Vec<u8>,
    send_info: quiche::SendInfo,
    queue_entry: QueueEntry,
}

#[expect(
    clippy::too_many_arguments,
    reason = "wire generation keeps the family ceiling and bounded buffer ownership explicit"
)]
fn generate_wire_datagrams(
    connection: &mut H3QuicConnection,
    pending: &mut VecDeque<WireDatagram>,
    free_buffers: &mut Vec<Vec<u8>>,
    send_quantum: usize,
    wire_payload_capacity: usize,
    wire_queue: &Arc<QueueMetrics>,
    quality: &NetworkQualityTelemetry,
    active: crate::path_socket::PathBinding,
) -> Result<(), TransportError> {
    if send_quantum < INITIAL_SAFE_UDP_PAYLOAD {
        return Ok(());
    }
    let mut generated_bytes = 0usize;
    while pending.len() < MAX_PENDING_WIRE_DATAGRAMS
        && generated_bytes.saturating_add(wire_payload_capacity) <= send_quantum
    {
        let mut bytes = take_wire_buffer(free_buffers, wire_payload_capacity, quality);
        match connection.send_on_path(&mut bytes, Some(active.local_addr), Some(active.peer_addr)) {
            Ok((length, send_info)) => {
                generated_bytes = generated_bytes.saturating_add(length);
                bytes.truncate(length);
                let queue_entry = wire_queue.start_entry(length);
                pending.push_back(WireDatagram {
                    bytes,
                    send_info,
                    queue_entry,
                });
            }
            Err(quiche::Error::Done) => {
                recycle_wire_buffer(free_buffers, bytes, quality);
                break;
            }
            Err(error) => {
                return Err(TransportError::Http3(format!(
                    "generate QUIC packet: {error:?}"
                )));
            }
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WireSendOutcome {
    Sent,
    MessageTooLarge,
}

#[expect(
    clippy::too_many_arguments,
    reason = "the actor send step keeps socket, ordered queues, quantum, telemetry, and cancellation explicit"
)]
async fn send_due_wire_datagrams(
    path_sockets: &PathSocketSet,
    pending: &mut VecDeque<WireDatagram>,
    free_buffers: &mut Vec<Vec<u8>>,
    send_quantum: usize,
    wire_queue: &QueueMetrics,
    quality: &NetworkQualityTelemetry,
    cancel: &CancellationToken,
) -> Result<WireSendOutcome, TransportError> {
    let Some(first) = pending.front() else {
        return Ok(WireSendOutcome::Sent);
    };
    let now = StdInstant::now();
    if first.send_info.at > now {
        return Ok(WireSendOutcome::Sent);
    }
    let source = first.send_info.from;
    let destination = first.send_info.to;
    let udp_io = path_sockets
        .io_for_send(source, destination)
        .map_err(path_socket_routing_error)?;
    let mut sent_bytes = 0usize;
    let empty = SendDatagram {
        payload: &[],
        source,
        destination,
        due_at: now,
    };
    let mut batch = [empty; UDP_ACTOR_DRAIN_LIMIT];
    let mut batch_len = 0;
    for datagram in pending.iter().take(UDP_ACTOR_DRAIN_LIMIT) {
        if datagram.send_info.at > now
            || datagram.send_info.from != source
            || datagram.send_info.to != destination
        {
            break;
        }
        if sent_bytes.saturating_add(datagram.bytes.len()) > send_quantum {
            break;
        }
        batch[batch_len] = SendDatagram {
            payload: &datagram.bytes,
            source: datagram.send_info.from,
            destination: datagram.send_info.to,
            due_at: datagram.send_info.at,
        };
        batch_len += 1;
        sent_bytes = sent_bytes.saturating_add(datagram.bytes.len());
    }
    if batch_len == 0 {
        return Ok(WireSendOutcome::Sent);
    }
    let send_result = udp_io.send_batch(&batch[..batch_len], cancel).await;
    let sent = match send_result {
        Ok(sent) => sent,
        Err(error) if is_message_too_long(&error) => {
            discard_pending_wire_datagrams(pending, free_buffers, wire_queue, quality);
            return Ok(WireSendOutcome::MessageTooLarge);
        }
        Err(error) => return Err(error.into()),
    };
    if sent > batch_len {
        return Err(TransportError::Http3(
            "UDP batch backend reported more sends than requested".to_owned(),
        ));
    }
    if sent < batch_len {
        quality.record_udp_partial_batch();
    }
    complete_wire_sends(pending, free_buffers, sent, wire_queue, quality);
    Ok(WireSendOutcome::Sent)
}

fn path_socket_routing_error(error: PathSocketSetError) -> TransportError {
    TransportError::Http3(error.to_string())
}

fn discard_pending_wire_datagrams(
    pending: &mut VecDeque<WireDatagram>,
    free_buffers: &mut Vec<Vec<u8>>,
    wire_queue: &QueueMetrics,
    quality: &NetworkQualityTelemetry,
) {
    while let Some(datagram) = pending.pop_front() {
        datagram.queue_entry.complete();
        recycle_wire_buffer(free_buffers, datagram.bytes, quality);
    }
    debug_assert_eq!(wire_queue.snapshot(Instant::now()).current_items, 0);
}

fn complete_wire_sends(
    pending: &mut VecDeque<WireDatagram>,
    free_buffers: &mut Vec<Vec<u8>>,
    sent: usize,
    wire_queue: &QueueMetrics,
    quality: &NetworkQualityTelemetry,
) {
    for _ in 0..sent {
        let datagram = pending
            .pop_front()
            .expect("UDP batch completion cannot exceed its requested prefix");
        datagram.queue_entry.complete();
        recycle_wire_buffer(free_buffers, datagram.bytes, quality);
    }
    if let Some(next) = pending.front() {
        wire_queue.observe_oldest_entry(&next.queue_entry);
    }
}

fn take_wire_buffer(
    free_buffers: &mut Vec<Vec<u8>>,
    wire_payload_capacity: usize,
    quality: &NetworkQualityTelemetry,
) -> Vec<u8> {
    let mut bytes = match free_buffers.pop() {
        Some(bytes) => {
            quality.record_packet_buffer_pool_hit();
            quality.record_encode_buffer_reuse();
            bytes
        }
        None => {
            quality.record_packet_buffer_pool_miss();
            quality.record_fresh_allocation();
            Vec::with_capacity(wire_payload_capacity)
        }
    };
    if bytes.capacity() < wire_payload_capacity {
        quality.record_fresh_allocation();
        bytes.reserve_exact(wire_payload_capacity - bytes.capacity());
    }
    bytes.resize(wire_payload_capacity, 0);
    bytes
}

fn recycle_wire_buffer(
    free_buffers: &mut Vec<Vec<u8>>,
    mut bytes: Vec<u8>,
    quality: &NetworkQualityTelemetry,
) {
    bytes.clear();
    if free_buffers.len() < MAX_PENDING_WIRE_DATAGRAMS {
        free_buffers.push(bytes);
        quality.record_buffer_recycle();
    }
}

fn encode_http_datagram(
    pool: &DatagramEncodePool,
    stream_id: u64,
    packet: &[u8],
) -> Result<Option<PooledDatagramBuffer>, TransportError> {
    validate_ip_packet(packet)?;
    let Some(mut encoded) = pool.take() else {
        return Ok(None);
    };
    let required = encoded_varint_len(stream_id / 4)?
        .saturating_add(encoded_varint_len(usque_protocol::DEFAULT_CONTEXT_ID)?)
        .saturating_add(packet.len());
    if required > HTTP_DATAGRAM_BUFFER_CAPACITY {
        return Err(TransportError::Http3(
            "HTTP Datagram exceeded the bounded encode buffer".to_owned(),
        ));
    }
    let target = encoded.bytes_mut();
    debug_assert!(target.is_empty());
    encode_varint(stream_id / 4, target)?;
    encode_varint(usque_protocol::DEFAULT_CONTEXT_ID, target)?;
    target.extend_from_slice(packet);
    Ok(Some(encoded))
}

fn decode_http_datagram_bytes(
    request_stream_id: u64,
    datagram: Bytes,
) -> Result<Option<Bytes>, TransportError> {
    let Some((quarter_stream_id, stream_bytes)) = decode_varint(&datagram)? else {
        return Err(TransportError::MalformedIpPacket);
    };
    if quarter_stream_id != request_stream_id / 4 {
        return Ok(None);
    }
    let payload = datagram
        .get(stream_bytes..)
        .ok_or(TransportError::MalformedIpPacket)?;
    let datagram = IpDatagram::decode(datagram.slice(stream_bytes..stream_bytes + payload.len()))?;
    if datagram.context_id != usque_protocol::DEFAULT_CONTEXT_ID {
        return Ok(None);
    }
    validate_ip_packet(&datagram.packet)?;
    Ok(Some(datagram.packet))
}

#[cfg(test)]
fn decode_http_datagram(
    request_stream_id: u64,
    datagram: &[u8],
) -> Result<Option<Bytes>, TransportError> {
    decode_http_datagram_bytes(request_stream_id, Bytes::copy_from_slice(datagram))
}

fn decode_varint(buffer: &[u8]) -> Result<Option<(u64, usize)>, TransportError> {
    let Some(first) = buffer.first().copied() else {
        return Ok(None);
    };
    let length = 1usize << (first >> 6);
    if buffer.len() < length {
        return Ok(None);
    }
    let mut value = u64::from(first & 0x3f);
    for byte in &buffer[1..length] {
        value = (value << 8) | u64::from(*byte);
    }
    Ok(Some((value, length)))
}

fn encode_varint(value: u64, target: &mut Vec<u8>) -> Result<(), TransportError> {
    let length = encoded_varint_len(value)?;
    let prefix = match length {
        1 => 0,
        2 => 1,
        4 => 2,
        8 => 3,
        _ => unreachable!(),
    };
    let mut bytes = [0u8; 8];
    let mut encoded = value;
    for index in (0..length).rev() {
        bytes[index] = encoded as u8;
        encoded >>= 8;
    }
    bytes[0] |= prefix << 6;
    target.extend_from_slice(&bytes[..length]);
    Ok(())
}

fn encoded_varint_len(value: u64) -> Result<usize, TransportError> {
    Ok(match value {
        0..=63 => 1,
        64..=16_383 => 2,
        16_384..=1_073_741_823 => 4,
        1_073_741_824..=4_611_686_018_427_387_903 => 8,
        _ => return Err(TransportError::InvalidVarint),
    })
}

fn connection_closed_error(connection: &H3QuicConnection) -> TransportError {
    if let Some(peer_error) = connection.peer_error() {
        let reason = String::from_utf8_lossy(&peer_error.reason);
        if !peer_error.is_app && peer_error.error_code == 0x0a {
            return TransportError::Http3ProtocolViolation(reason.into_owned());
        }
        return TransportError::Http3(format!(
            "peer closed QUIC (application={}, code={}, reason={reason})",
            peer_error.is_app, peer_error.error_code
        ));
    }
    if let Some(local_error) = connection.local_error() {
        let reason = String::from_utf8_lossy(&local_error.reason);
        return TransportError::Http3(format!(
            "local QUIC close (application={}, code={}, reason={reason})",
            local_error.is_app, local_error.error_code
        ));
    }
    TransportError::TunnelClosed
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

    use super::*;
    use usque_core::MasqueKeyPair;

    #[test]
    fn startup_preserves_the_typed_pmtu_exhaustion_failure() {
        let error =
            StartupFailure::from_transport_error(&TransportError::PmtuRevalidationExhausted)
                .into_transport_error();
        assert!(matches!(error, TransportError::PmtuRevalidationExhausted));
        let failure = error.failure(Some(usque_core::Transport::Http3), None);
        assert_eq!(
            failure.code,
            usque_core::TransportFailureCode::PmtuRevalidationExhausted
        );
        assert!(failure.fallback_allowed);
    }

    struct DropSignal(Option<oneshot::Sender<()>>);

    impl Drop for DropSignal {
        fn drop(&mut self) {
            if let Some(sender) = self.0.take() {
                let _ = sender.send(());
            }
        }
    }

    struct LeaseDropCounter(Arc<AtomicUsize>);

    impl Drop for LeaseDropCounter {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::AcqRel);
        }
    }

    struct RacingSocketProtector {
        generation: AtomicU64,
        calls: AtomicUsize,
        race_every_attempt: bool,
        lease_drops: Arc<AtomicUsize>,
    }

    impl RacingSocketProtector {
        fn new(race_every_attempt: bool) -> Self {
            Self {
                generation: AtomicU64::new(0),
                calls: AtomicUsize::new(0),
                race_every_attempt,
                lease_drops: Arc::new(AtomicUsize::new(0)),
            }
        }
    }

    #[async_trait::async_trait]
    impl SocketProtector for RacingSocketProtector {
        fn protect(&self, _socket: crate::socket::SocketHandle) -> Result<(), String> {
            Ok(())
        }

        async fn protect_for_target_generation(
            &self,
            _socket: crate::socket::SocketHandle,
            _remote: SocketAddr,
            _protocol: DirectProtocol,
            expected_generation: u64,
        ) -> Result<DirectEgressLease, String> {
            let call = self.calls.fetch_add(1, Ordering::AcqRel);
            if self.race_every_attempt || call == 0 {
                self.generation.fetch_add(1, Ordering::AcqRel);
            }
            Ok(DirectEgressLease::hold_for_generation(
                LeaseDropCounter(Arc::clone(&self.lease_drops)),
                expected_generation,
            ))
        }

        fn network_generation(&self) -> Option<u64> {
            Some(self.generation.load(Ordering::Acquire))
        }
    }

    fn test_quic_pair() -> (H3QuicConnection, H3QuicConnection, SocketAddr, SocketAddr) {
        test_quic_pair_at(
            "127.0.0.1:12340".parse().unwrap(),
            "127.0.0.1:44330".parse().unwrap(),
        )
    }

    pub(super) fn test_quic_pair_at(
        client_addr: SocketAddr,
        server_addr: SocketAddr,
    ) -> (H3QuicConnection, H3QuicConnection, SocketAddr, SocketAddr) {
        test_quic_pair_with_config(client_addr, server_addr, |_, _| {})
    }

    pub(super) fn test_quic_pair_with_config(
        client_addr: SocketAddr,
        server_addr: SocketAddr,
        configure: impl FnOnce(&mut quiche::Config, &mut quiche::Config),
    ) -> (H3QuicConnection, H3QuicConnection, SocketAddr, SocketAddr) {
        test_quic_pair_with_server_config(
            client_addr,
            server_addr,
            |identity| {
                quic_config(identity, crate::pmtu::IPV4_MAX_UDP_PAYLOAD)
                    .unwrap()
                    .0
            },
            configure,
        )
    }

    pub(super) fn test_quic_pair_with_server_config(
        client_addr: SocketAddr,
        server_addr: SocketAddr,
        server_config: impl FnOnce(&MasqueTlsIdentity) -> quiche::Config,
        configure: impl FnOnce(&mut quiche::Config, &mut quiche::Config),
    ) -> (H3QuicConnection, H3QuicConnection, SocketAddr, SocketAddr) {
        let client_key = MasqueKeyPair::generate();
        let server_key = MasqueKeyPair::generate();
        let client_identity = MasqueTlsIdentity::new(
            client_key.private_sec1_der().unwrap(),
            &server_key.public_spki_der().unwrap(),
            Ipv4Addr::new(172, 16, 0, 2),
            "2606:4700:110:8f13::2".parse().unwrap(),
        )
        .unwrap();
        let server_identity = MasqueTlsIdentity::new(
            server_key.private_sec1_der().unwrap(),
            &client_key.public_spki_der().unwrap(),
            Ipv4Addr::new(172, 16, 0, 3),
            "2606:4700:110:8f13::3".parse().unwrap(),
        )
        .unwrap();
        let (mut client_config, _) =
            quic_config(&client_identity, crate::pmtu::IPV4_MAX_UDP_PAYLOAD).unwrap();
        let mut server_config = server_config(&server_identity);
        configure(&mut client_config, &mut server_config);
        let client_scid = [0xc1; CONNECTION_ID_LENGTH];
        let server_scid = [0x51; CONNECTION_ID_LENGTH];
        let client = quiche::connect_with_buffer_factory::<H3BufferFactory>(
            Some("migration.test"),
            &quiche::ConnectionId::from_ref(&client_scid),
            client_addr,
            server_addr,
            &mut client_config,
        )
        .unwrap();
        let server = quiche::accept_with_buf_factory::<H3BufferFactory>(
            &quiche::ConnectionId::from_ref(&server_scid),
            None,
            server_addr,
            client_addr,
            &mut server_config,
        )
        .unwrap();
        (client, server, client_addr, server_addr)
    }

    fn transfer_test_flight(
        source: &mut H3QuicConnection,
        destination: &mut H3QuicConnection,
        from: Option<SocketAddr>,
        to: Option<SocketAddr>,
    ) -> Result<usize, quiche::Error> {
        let mut packets = 0;
        loop {
            let mut wire = vec![0_u8; 65_535];
            let (written, send_info) = match source.send_on_path(&mut wire, from, to) {
                Ok(output) => output,
                Err(quiche::Error::Done) => break,
                Err(error) => return Err(error),
            };
            if let Some(expected) = from {
                assert_eq!(send_info.from, expected);
            }
            if let Some(expected) = to {
                assert_eq!(send_info.to, expected);
            }
            destination.recv(
                &mut wire[..written],
                quiche::RecvInfo {
                    from: send_info.from,
                    to: send_info.to,
                },
            )?;
            packets += 1;
        }
        Ok(packets)
    }

    pub(super) fn advance_test_pair(
        client: &mut H3QuicConnection,
        server: &mut H3QuicConnection,
    ) -> Result<(), quiche::Error> {
        for _ in 0..64 {
            let client_packets = transfer_test_flight(client, server, None, None)?;
            let server_packets = transfer_test_flight(server, client, None, None)?;
            if client_packets == 0 && server_packets == 0 {
                return Ok(());
            }
        }
        Err(quiche::Error::InvalidState)
    }

    fn established_test_pair() -> (H3QuicConnection, H3QuicConnection, SocketAddr, SocketAddr) {
        let (mut client, mut server, client_addr, server_addr) = test_quic_pair();
        for _ in 0..8 {
            advance_test_pair(&mut client, &mut server).unwrap();
            if client.is_established() && server.is_established() {
                return (client, server, client_addr, server_addr);
            }
        }
        panic!("locked quiche client/server pair did not establish")
    }

    fn ipv4_packet() -> [u8; 20] {
        [
            0x45, 0, 0, 20, 0, 0, 0, 0, 64, 17, 0, 0, 1, 1, 1, 1, 8, 8, 8, 8,
        ]
    }

    pub(super) fn ipv4_packet_with_length(length: usize) -> Vec<u8> {
        assert!((20..=u16::MAX as usize).contains(&length));
        let mut packet = vec![0_u8; length];
        packet[0] = 0x45;
        packet[2..4].copy_from_slice(&(length as u16).to_be_bytes());
        packet[8] = 64;
        packet[9] = 17;
        packet[12..16].copy_from_slice(&[1, 1, 1, 1]);
        packet[16..20].copy_from_slice(&[8, 8, 8, 8]);
        packet
    }

    pub(super) fn encode_for_test(stream_id: u64, packet: &[u8]) -> PooledDatagramBuffer {
        let pool = DatagramEncodePool::new(NetworkQualityTelemetry::default());
        encode_http_datagram(&pool, stream_id, packet)
            .unwrap()
            .expect("test encode pool has capacity")
    }

    #[tokio::test]
    async fn dropping_driver_wait_aborts_the_old_actor() {
        let (started_tx, started_rx) = oneshot::channel();
        let (dropped_tx, dropped_rx) = oneshot::channel();
        let task = tokio::spawn(async move {
            let _drop_signal = DropSignal(Some(dropped_tx));
            let _ = started_tx.send(());
            std::future::pending::<Result<(), TransportError>>().await
        });
        started_rx.await.unwrap();
        let driver = H3Driver { task: Some(task) };
        let mut wait = Box::pin(driver.wait());
        tokio::select! {
            result = &mut wait => panic!("driver wait completed early: {result:?}"),
            () = tokio::time::sleep(Duration::from_millis(10)) => {}
        }
        drop(wait);
        timeout(Duration::from_secs(1), dropped_rx)
            .await
            .expect("dropping the wait future did not abort the old actor")
            .unwrap();
    }

    #[test]
    fn http_datagram_contains_quarter_stream_and_context_ids() {
        let packet = ipv4_packet();
        let encoded = encode_for_test(8, &packet);
        assert_eq!(decode_varint(encoded.as_ref()).unwrap(), Some((2, 1)));
        assert_eq!(decode_varint(&encoded.as_ref()[1..]).unwrap(), Some((0, 1)));
        assert_eq!(
            decode_http_datagram(8, encoded.as_ref())
                .unwrap()
                .unwrap()
                .as_ref(),
            packet
        );
        assert!(decode_http_datagram(4, encoded.as_ref()).unwrap().is_none());
    }

    #[test]
    fn http_datagram_encoding_matches_protocol_composition() {
        let packet = ipv4_packet();
        for quarter_stream_id in [
            0,
            63,
            64,
            16_383,
            16_384,
            1_073_741_823,
            1_073_741_824,
            (1_u64 << 60) - 1,
        ] {
            let stream_id = quarter_stream_id * 4;
            let payload = IpDatagram::new(Bytes::copy_from_slice(&packet))
                .encode()
                .unwrap();
            let mut reference = Vec::with_capacity(payload.len() + 8);
            encode_varint(quarter_stream_id, &mut reference).unwrap();
            reference.extend_from_slice(&payload);

            assert_eq!(encode_for_test(stream_id, &packet).as_ref(), reference);
        }
    }

    #[test]
    fn owned_http_datagram_decode_reuses_the_receive_allocation() {
        let packet = ipv4_packet();
        let encoded = encode_for_test(8, &packet);
        let received = PooledDatagramBuffer::from(encoded.as_ref().to_vec()).into_bytes();
        let allocation = received.as_ptr() as usize;
        let (_, stream_prefix) = decode_varint(&received).unwrap().unwrap();
        let (_, context_prefix) = decode_varint(&received[stream_prefix..]).unwrap().unwrap();
        let expected_payload = allocation + stream_prefix + context_prefix;

        let decoded = decode_http_datagram_bytes(8, received).unwrap().unwrap();
        assert_eq!(decoded.as_ptr() as usize, expected_payload);
        assert_eq!(decoded.as_ref(), packet);
    }

    #[test]
    fn encode_pool_handles_maximum_empty_and_oversized_packets() {
        let pool = DatagramEncodePool::new(NetworkQualityTelemetry::default());
        assert!(encode_http_datagram(&pool, 0, &[]).is_err());

        let maximum = ipv4_packet_with_length(HTTP_DATAGRAM_BUFFER_CAPACITY - 2);
        let encoded = encode_http_datagram(&pool, 0, &maximum)
            .unwrap()
            .expect("pool has one buffer");
        assert_eq!(encoded.as_ref().len(), HTTP_DATAGRAM_BUFFER_CAPACITY);
        drop(encoded);

        let oversized = ipv4_packet_with_length(HTTP_DATAGRAM_BUFFER_CAPACITY - 1);
        assert!(encode_http_datagram(&pool, 0, &oversized).is_err());
    }

    #[tokio::test]
    async fn expected_generation_setup_retries_once_and_releases_the_stale_lease() {
        let protector = RacingSocketProtector::new(false);
        let target: SocketAddr = "127.0.0.1:443".parse().unwrap();
        let prepared = prepare_initial_udp_socket(target, &protector)
            .await
            .unwrap();

        assert_eq!(protector.calls.load(Ordering::Acquire), 2);
        assert_eq!(prepared.network_generation, 1);
        assert_eq!(prepared.egress_lease.generation(), Some(1));
        assert_eq!(protector.lease_drops.load(Ordering::Acquire), 1);
        drop(prepared);
        assert_eq!(protector.lease_drops.load(Ordering::Acquire), 2);
    }

    #[tokio::test]
    async fn expected_generation_setup_stops_after_two_racing_attempts() {
        let protector = RacingSocketProtector::new(true);
        let target: SocketAddr = "127.0.0.1:443".parse().unwrap();
        assert!(matches!(
            prepare_initial_udp_socket(target, &protector).await,
            Err(SocketPrepareError::StaleGeneration)
        ));
        assert_eq!(
            protector.calls.load(Ordering::Acquire),
            SOCKET_PREPARE_ATTEMPTS
        );
        assert_eq!(
            protector.lease_drops.load(Ordering::Acquire),
            SOCKET_PREPARE_ATTEMPTS
        );
    }

    #[test]
    fn cid_readiness_does_not_claim_platform_migration_support() {
        for availability in [
            CidAvailability::Ready,
            CidAvailability::PeerUnavailable,
            CidAvailability::LocalUnavailable,
        ] {
            assert_eq!(
                migration_availability_reason(availability, false),
                Some(MigrationReasonCode::Unsupported)
            );
        }
        assert_eq!(
            migration_availability_reason(CidAvailability::Ready, true),
            None
        );
    }

    #[test]
    fn cid_capacity_errors_map_to_the_stable_local_unavailable_reason() {
        assert_eq!(
            map_cid_provisioning_error(quiche::Error::OutOfIdentifiers).unwrap(),
            CidAvailability::LocalUnavailable
        );
        assert_eq!(
            map_cid_provisioning_error(quiche::Error::IdLimit).unwrap(),
            CidAvailability::LocalUnavailable
        );
    }

    #[test]
    fn cid_retirement_is_replenished_to_three_spares() {
        let (mut client, mut server, _, _) = established_test_pair();
        assert_eq!(
            maintain_connection_ids(&mut client).unwrap(),
            CidAvailability::PeerUnavailable
        );
        assert_eq!(
            maintain_connection_ids(&mut server).unwrap(),
            CidAvailability::PeerUnavailable
        );
        assert_eq!(client.active_scids(), SPARE_CONNECTION_ID_TARGET + 1);
        assert_eq!(server.active_scids(), SPARE_CONNECTION_ID_TARGET + 1);
        advance_test_pair(&mut client, &mut server).unwrap();
        assert_eq!(
            maintain_connection_ids(&mut client).unwrap(),
            CidAvailability::Ready
        );
        assert_eq!(
            maintain_connection_ids(&mut server).unwrap(),
            CidAvailability::Ready
        );

        server.retire_dcid(1).unwrap();
        advance_test_pair(&mut client, &mut server).unwrap();
        assert_eq!(client.retired_scids(), 1);
        assert_eq!(client.active_scids(), SPARE_CONNECTION_ID_TARGET);

        assert_eq!(
            maintain_connection_ids(&mut client).unwrap(),
            CidAvailability::Ready
        );
        assert_eq!(client.retired_scids(), 0);
        assert_eq!(client.active_scids(), SPARE_CONNECTION_ID_TARGET + 1);
        advance_test_pair(&mut client, &mut server).unwrap();
        assert_eq!(server.available_dcids(), SPARE_CONNECTION_ID_TARGET);
    }

    #[test]
    fn migration_barrier_keeps_http_datagram_off_candidate_before_promotion() {
        let (mut client, mut server, active_addr, server_addr) = established_test_pair();
        assert_eq!(
            maintain_connection_ids(&mut client).unwrap(),
            CidAvailability::PeerUnavailable
        );
        assert_eq!(
            maintain_connection_ids(&mut server).unwrap(),
            CidAvailability::PeerUnavailable
        );
        advance_test_pair(&mut client, &mut server).unwrap();
        assert_eq!(
            maintain_connection_ids(&mut client).unwrap(),
            CidAvailability::Ready
        );

        let marker_packet = ipv4_packet_with_length(64);
        let encoded = encode_for_test(0, &marker_packet);
        let expected = encoded.as_ref().to_vec();
        client.dgram_send(encoded.as_ref()).unwrap();
        let mut barrier = MigrationTxBarrier::default();
        let started_at = StdInstant::now();
        assert!(barrier.begin(started_at));
        assert!(!barrier.allows_application_injection());
        assert!(!barrier.candidate_send_allowed());

        let active_packets = transfer_test_flight(
            &mut client,
            &mut server,
            Some(active_addr),
            Some(server_addr),
        )
        .unwrap();
        assert!(active_packets > 0);
        assert_eq!(client.dgram_send_queue_len(), 0);
        assert!(barrier.complete_active_drain(StdInstant::now(), active_packets, true));
        assert!(barrier.candidate_send_allowed());
        let received = server.dgram_recv_buf().unwrap();
        assert_eq!(received.as_ref(), expected);
        assert!(matches!(server.dgram_recv_buf(), Err(quiche::Error::Done)));

        let candidate_addr: SocketAddr = "127.0.0.1:12341".parse().unwrap();
        client.probe_path(candidate_addr, server_addr).unwrap();
        let candidate_packets = transfer_test_flight(
            &mut client,
            &mut server,
            Some(candidate_addr),
            Some(server_addr),
        )
        .unwrap();
        assert!(candidate_packets > 0);
        assert!(matches!(server.dgram_recv_buf(), Err(quiche::Error::Done)));
        barrier.finish();
        assert!(barrier.allows_application_injection());
    }

    #[test]
    fn inbound_queue_overflow_counts_only_payloads_beyond_the_bound() {
        assert_eq!(inbound_queue_overflow(0, DATAGRAM_RECV_QUEUE_CAPACITY), 0);
        assert_eq!(
            inbound_queue_overflow(DATAGRAM_RECV_QUEUE_CAPACITY - 1, 1),
            0
        );
        assert_eq!(
            inbound_queue_overflow(DATAGRAM_RECV_QUEUE_CAPACITY - 1, 3),
            2
        );
        assert_eq!(inbound_queue_overflow(DATAGRAM_RECV_QUEUE_CAPACITY, 4), 4);
    }

    #[test]
    fn inbound_batch_storage_is_bounded_to_1024_packets() {
        let application_channel = INCOMING_BATCH_CHANNEL_CAPACITY * MAX_PACKET_BATCH_PACKETS;
        assert_eq!(
            application_channel
                + MAX_PACKET_BATCH_PACKETS // receive-half pending batch
                + MAX_PACKET_BATCH_PACKETS // actor pending batch
                + DATAGRAM_RECV_QUEUE_CAPACITY,
            INBOUND_PACKET_CAPACITY
        );
    }

    #[test]
    fn effective_payload_uses_profile_and_real_writable_bounds() {
        assert_eq!(effective_connect_ip_payload(1_400, 1_452, 2), 1_400);
        assert_eq!(effective_connect_ip_payload(9_000, 1_452, 2), 1_450);
        assert_eq!(effective_connect_ip_payload(1_280, 1, 2), 0);
    }

    #[test]
    fn wire_datagram_buffers_are_reused_with_a_fixed_bound() {
        let mut free = Vec::new();
        let quality = NetworkQualityTelemetry::default();
        let capacity = crate::pmtu::IPV4_MAX_UDP_PAYLOAD;
        let first = take_wire_buffer(&mut free, capacity, &quality);
        assert_eq!(first.len(), capacity);
        let allocation = first.as_ptr();
        recycle_wire_buffer(&mut free, first, &quality);

        let reused = take_wire_buffer(&mut free, capacity, &quality);
        assert_eq!(reused.len(), capacity);
        assert_eq!(reused.as_ptr(), allocation);
        recycle_wire_buffer(&mut free, reused, &quality);

        for _ in 0..=MAX_PENDING_WIRE_DATAGRAMS {
            recycle_wire_buffer(&mut free, Vec::with_capacity(capacity), &quality);
        }
        assert_eq!(free.len(), MAX_PENDING_WIRE_DATAGRAMS);
    }

    #[tokio::test]
    async fn udp_send_drain_respects_send_quantum_and_pacing_deadline() {
        let receiver = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let sender_socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let from = sender_socket.local_addr().unwrap();
        let to = receiver.local_addr().unwrap();
        let due = StdInstant::now();
        let future = due + Duration::from_secs(60);
        let quality = NetworkQualityTelemetry::default();
        let sender = PathSocket::spawn(
            PathId::new(0),
            from,
            to,
            0,
            PathSocketRole::Active,
            sender_socket,
            DirectEgressLease::for_generation(0),
            quality.clone(),
            UdpReceivePool::default(),
        )
        .unwrap();
        let mut sender = PathSocketSet::with_active(sender).unwrap();
        let cancel = CancellationToken::new();
        let wire_queue = QueueMetrics::new(
            QueueKind::H3WireSend,
            MAX_PENDING_WIRE_DATAGRAMS,
            MAX_PENDING_WIRE_DATAGRAMS * crate::pmtu::IPV4_MAX_UDP_PAYLOAD,
        );
        let mut pending = VecDeque::from([
            WireDatagram {
                bytes: vec![1; 100],
                send_info: quiche::SendInfo { from, to, at: due },
                queue_entry: wire_queue.start_entry(100),
            },
            WireDatagram {
                bytes: vec![2; 100],
                send_info: quiche::SendInfo { from, to, at: due },
                queue_entry: wire_queue.start_entry(100),
            },
            WireDatagram {
                bytes: vec![3; 100],
                send_info: quiche::SendInfo {
                    from,
                    to,
                    at: future,
                },
                queue_entry: wire_queue.start_entry(100),
            },
        ]);
        let mut free = Vec::new();

        assert_eq!(
            send_due_wire_datagrams(
                &sender,
                &mut pending,
                &mut free,
                150,
                &wire_queue,
                &quality,
                &cancel,
            )
            .await
            .unwrap(),
            WireSendOutcome::Sent,
        );
        assert_eq!(pending.len(), 2);
        let mut received = [0u8; 128];
        let length = timeout(Duration::from_secs(1), receiver.recv(&mut received))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&received[..length], &[1; 100]);

        assert_eq!(
            send_due_wire_datagrams(
                &sender,
                &mut pending,
                &mut free,
                500,
                &wire_queue,
                &quality,
                &cancel,
            )
            .await
            .unwrap(),
            WireSendOutcome::Sent,
        );
        assert_eq!(pending.len(), 1);
        assert_eq!(pending.front().unwrap().bytes, vec![3; 100]);
        sender.shutdown_all().await;
    }

    #[test]
    fn partial_batch_completion_pops_only_zero_one_or_n_sent_items() {
        let quality = NetworkQualityTelemetry::default();
        let wire_queue = QueueMetrics::new(QueueKind::H3WireSend, 8, 8_000);
        let from: SocketAddr = "127.0.0.1:10000".parse().unwrap();
        let to: SocketAddr = "127.0.0.1:20000".parse().unwrap();
        let mut pending = VecDeque::new();
        for marker in 1..=3 {
            pending.push_back(WireDatagram {
                bytes: vec![marker; 100],
                send_info: quiche::SendInfo {
                    from,
                    to,
                    at: StdInstant::now(),
                },
                queue_entry: wire_queue.start_entry(100),
            });
        }
        let mut free = Vec::new();

        complete_wire_sends(&mut pending, &mut free, 0, &wire_queue, &quality);
        assert_eq!(pending.len(), 3);
        assert_eq!(pending.front().unwrap().bytes[0], 1);

        complete_wire_sends(&mut pending, &mut free, 1, &wire_queue, &quality);
        assert_eq!(pending.len(), 2);
        assert_eq!(pending.front().unwrap().bytes[0], 2);

        complete_wire_sends(&mut pending, &mut free, 2, &wire_queue, &quality);
        assert!(pending.is_empty());
        assert_eq!(free.len(), 3);
    }

    #[test]
    fn emsgsize_discard_path_never_retries_the_same_wire_packet() {
        let quality = NetworkQualityTelemetry::default();
        let wire_queue = QueueMetrics::new(QueueKind::H3WireSend, 3, 4_500);
        let from: SocketAddr = "127.0.0.1:10000".parse().unwrap();
        let to: SocketAddr = "127.0.0.1:20000".parse().unwrap();
        let mut pending = VecDeque::new();
        for marker in 1..=3 {
            pending.push_back(WireDatagram {
                bytes: vec![marker; 1_400],
                send_info: quiche::SendInfo {
                    from,
                    to,
                    at: StdInstant::now(),
                },
                queue_entry: wire_queue.start_entry(1_400),
            });
        }
        let mut free = Vec::new();

        discard_pending_wire_datagrams(&mut pending, &mut free, &wire_queue, &quality);

        assert!(pending.is_empty());
        assert_eq!(free.len(), 3);
        assert_eq!(wire_queue.snapshot(Instant::now()).current_items, 0);
    }

    #[tokio::test]
    async fn inbound_batch_is_retained_until_channel_capacity_returns() {
        let (incoming_tx, mut incoming_rx) = mpsc::channel(1);
        incoming_tx
            .try_send(PacketBatch::single(Bytes::from_static(b"queued")))
            .unwrap();
        let mut pending = PacketBatch::single(Bytes::from_static(b"pending"));

        assert!(!flush_incoming_batch(&incoming_tx, &mut pending).unwrap());
        assert_eq!(pending.len(), 1);
        assert_eq!(
            incoming_rx.recv().await.unwrap().pop_front().unwrap(),
            Bytes::from_static(b"queued")
        );

        assert!(flush_incoming_batch(&incoming_tx, &mut pending).unwrap());
        assert!(pending.is_empty());
        assert_eq!(
            incoming_rx.recv().await.unwrap().pop_front().unwrap(),
            Bytes::from_static(b"pending")
        );
    }

    #[test]
    fn connect_headers_match_the_cloudflare_oracle() {
        let headers = connect_headers();
        let find = |name: &[u8]| {
            headers
                .iter()
                .find(|header| header.name() == name)
                .map(NameValue::value)
        };
        assert_eq!(find(b":method"), Some(b"CONNECT".as_slice()));
        assert_eq!(find(b":authority"), Some(CONNECT_AUTHORITY));
        assert_eq!(find(b":protocol"), Some(CONNECT_PROTOCOL));
        assert_eq!(find(CAPSULE_PROTOCOL_HEADER), Some(CAPSULE_PROTOCOL_VALUE));
        assert_eq!(find(b"user-agent"), Some(b"".as_slice()));
    }

    #[test]
    fn quic_varint_round_trips_boundaries() {
        for value in [
            0,
            63,
            64,
            16_383,
            16_384,
            1_073_741_823,
            1_073_741_824,
            4_611_686_018_427_387_903,
        ] {
            let mut encoded = Vec::new();
            encode_varint(value, &mut encoded).unwrap();
            assert_eq!(
                decode_varint(&encoded).unwrap(),
                Some((value, encoded.len()))
            );
        }
    }
}
