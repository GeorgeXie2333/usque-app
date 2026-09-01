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
use tokio::net::UdpSocket;
use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::JoinHandle;
use tokio::time::{Instant, MissedTickBehavior, interval_at, sleep_until, timeout};
use tokio_util::task::AbortOnDropHandle;
use usque_core::TransportStage;
use usque_protocol::{IpDatagram, MAX_CAPSULE_PAYLOAD, PeerNetworkState};

use crate::connect_ip_control::{ConnectIpControlPlane, PendingControlCapsule};
use crate::h2::{
    MasqueTlsIdentity, PinState, TransportError, configure_client_identity_and_pin,
    validate_ip_packet,
};
use crate::network_quality::{H3MetricsSample, NetworkQualityTelemetry};
use crate::packet_batch::{MAX_PACKET_BATCH_PACKETS, PacketBatch, PacketBatchResult};
use crate::queue_metrics::{QueueEntry, QueueKind, QueueMetrics};
use crate::socket::{SocketProtector, noop_socket_protector, socket_handle};
use crate::telemetry::{ConnectionAttemptTelemetry, ConnectionEventType};

const CONNECT_AUTHORITY: &[u8] = b"cloudflareaccess.com";
const CONNECT_PATH: &[u8] = b"/";
const CONNECT_PROTOCOL: &[u8] = b"cf-connect-ip";
const CAPSULE_PROTOCOL_HEADER: &[u8] = b"capsule-protocol";
const CAPSULE_PROTOCOL_VALUE: &[u8] = b"?1";
const CONNECTION_ID_LENGTH: usize = 20;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(8);
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(30);
const MAX_IDLE_TIMEOUT_MS: u64 = 90_000;
const MAX_UDP_PAYLOAD_SIZE: usize = 1_350;
const DATAGRAM_SEND_QUEUE_CAPACITY: usize = 1_024;
const DATAGRAM_RECV_QUEUE_CAPACITY: usize = MAX_PACKET_BATCH_PACKETS;
const INBOUND_PACKET_CAPACITY: usize = 1_024;
const INBOUND_RESERVED_BATCHES: usize = 3;
const INCOMING_BATCH_CHANNEL_CAPACITY: usize =
    INBOUND_PACKET_CAPACITY / MAX_PACKET_BATCH_PACKETS - INBOUND_RESERVED_BATCHES;
const OUTGOING_BATCH_CHANNEL_CAPACITY: usize = 1;
const MAX_PENDING_WIRE_DATAGRAMS: usize = 64;
const MAX_SOCKET_DRAIN: usize = 64;
const PACKET_SEND_TIMEOUT: Duration = Duration::from_secs(10);
const QUALITY_SAMPLE_INTERVAL: Duration = Duration::from_secs(1);

/// An established Cloudflare CONNECT-IP stream over HTTP/3 and QUIC.
pub struct H3Tunnel {
    send: H3SendHalf,
    receive: H3ReceiveHalf,
    driver: H3Driver,
    control: watch::Receiver<PeerNetworkState>,
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

pub async fn connect_h3(
    endpoint: SocketAddr,
    sni: &str,
    identity: &MasqueTlsIdentity,
) -> Result<H3Tunnel, TransportError> {
    connect_h3_with_protector(
        endpoint,
        sni,
        identity,
        noop_socket_protector().as_ref(),
        None,
    )
    .await
}

pub(crate) async fn connect_h3_with_protector(
    endpoint: SocketAddr,
    sni: &str,
    identity: &MasqueTlsIdentity,
    protector: &dyn SocketProtector,
    attempt: Option<&ConnectionAttemptTelemetry>,
) -> Result<H3Tunnel, TransportError> {
    let first = connect_h3_once(endpoint, sni, identity, protector, attempt).await;
    match first {
        Err(TransportError::Http3ProtocolViolation(_)) => {
            // The Go oracle retries this specific Cloudflare interoperability
            // failure once. All other failures preserve normal fallback rules.
            connect_h3_once(endpoint, sni, identity, protector, attempt).await
        }
        result => result,
    }
}

async fn connect_h3_once(
    endpoint: SocketAddr,
    sni: &str,
    identity: &MasqueTlsIdentity,
    protector: &dyn SocketProtector,
    attempt: Option<&ConnectionAttemptTelemetry>,
) -> Result<H3Tunnel, TransportError> {
    let bind_address = match endpoint {
        SocketAddr::V4(_) => SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0),
        SocketAddr::V6(_) => SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), 0),
    };
    let std_socket = StdUdpSocket::bind(bind_address)?;
    protector
        .protect(socket_handle(&std_socket))
        .map_err(TransportError::SocketProtection)?;
    std_socket.set_nonblocking(true)?;
    let socket = UdpSocket::from_std(std_socket)?;
    let local_address = socket.local_addr()?;
    if let Some(attempt) = attempt {
        attempt.record(
            ConnectionEventType::SocketConnected,
            TransportStage::SocketConnect,
        );
    }

    let (mut quic_config, pin_state) = quic_config(identity)?;
    let mut source_connection_id = [0u8; CONNECTION_ID_LENGTH];
    boring::rand::rand_bytes(&mut source_connection_id)?;
    let source_connection_id = quiche::ConnectionId::from_ref(&source_connection_id);
    let connection = quiche::connect(
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

    let quality = attempt
        .map(ConnectionAttemptTelemetry::quality)
        .unwrap_or_default();
    let datagram_queue = quality.register_queue(
        QueueKind::H3DatagramSend,
        DATAGRAM_SEND_QUEUE_CAPACITY,
        DATAGRAM_SEND_QUEUE_CAPACITY * MAX_UDP_PAYLOAD_SIZE,
    );
    let wire_queue = quality.register_queue(
        QueueKind::H3WireSend,
        MAX_PENDING_WIRE_DATAGRAMS,
        MAX_PENDING_WIRE_DATAGRAMS * MAX_UDP_PAYLOAD_SIZE,
    );

    let (outgoing_tx, outgoing_rx) = mpsc::channel(OUTGOING_BATCH_CHANNEL_CAPACITY);
    let (incoming_tx, incoming_rx) = mpsc::channel(INCOMING_BATCH_CHANNEL_CAPACITY);
    let (control_tx, control_rx) = watch::channel(PeerNetworkState::default());
    let (startup_tx, startup_rx) = oneshot::channel();
    let task = AbortOnDropHandle::new(tokio::spawn(run_h3_actor(
        socket,
        connection,
        h3_config,
        outgoing_rx,
        incoming_tx,
        control_tx,
        startup_tx,
        attempt.cloned(),
        quality,
        datagram_queue,
        wire_queue,
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
) -> Result<(quiche::Config, Arc<PinState>), TransportError> {
    let mut tls = SslContextBuilder::new(SslMethod::tls())?;
    let pin_state = configure_client_identity_and_pin(&mut tls, identity)?;
    let mut config = quiche::Config::with_boring_ssl_ctx_builder(quiche::PROTOCOL_VERSION, tls)
        .map_err(|error| TransportError::Http3(format!("create QUIC config: {error:?}")))?;
    config
        .set_application_protos(quiche::h3::APPLICATION_PROTOCOL)
        .map_err(|error| TransportError::Http3(format!("configure H3 ALPN: {error:?}")))?;
    config.set_max_idle_timeout(MAX_IDLE_TIMEOUT_MS);
    config.set_max_recv_udp_payload_size(MAX_UDP_PAYLOAD_SIZE);
    config.set_max_send_udp_payload_size(MAX_UDP_PAYLOAD_SIZE);
    config.set_initial_max_data(10_000_000);
    config.set_initial_max_stream_data_bidi_local(1_000_000);
    config.set_initial_max_stream_data_bidi_remote(1_000_000);
    config.set_initial_max_stream_data_uni(1_000_000);
    config.set_initial_max_streams_bidi(16);
    config.set_initial_max_streams_uni(16);
    config.set_disable_active_migration(true);
    config.enable_dgram(
        true,
        DATAGRAM_RECV_QUEUE_CAPACITY,
        DATAGRAM_SEND_QUEUE_CAPACITY,
    );
    config.set_cc_algorithm(quiche::CongestionControlAlgorithm::CUBIC);
    config.enable_pacing(true);
    Ok((config, pin_state))
}

#[derive(Debug)]
enum StartupFailure {
    ConnectRejected(u16),
    DatagramUnavailable,
    ProtocolViolation(String),
    Other(String),
}

impl StartupFailure {
    fn from_transport_error(error: &TransportError) -> Self {
        match error {
            TransportError::Http3ConnectRejected(status) => Self::ConnectRejected(*status),
            TransportError::Http3DatagramUnavailable => Self::DatagramUnavailable,
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
    socket: UdpSocket,
    connection: quiche::Connection,
    h3_config: quiche::h3::Config,
    outgoing_rx: mpsc::Receiver<OutgoingBatch>,
    incoming_tx: mpsc::Sender<PacketBatch>,
    control_tx: watch::Sender<PeerNetworkState>,
    startup_tx: oneshot::Sender<Result<(), StartupFailure>>,
    attempt: Option<ConnectionAttemptTelemetry>,
    quality: NetworkQualityTelemetry,
    datagram_queue: Arc<QueueMetrics>,
    wire_queue: Arc<QueueMetrics>,
) -> Result<(), TransportError> {
    let mut startup_tx = Some(startup_tx);
    let result = drive_h3_actor(
        socket,
        connection,
        h3_config,
        outgoing_rx,
        incoming_tx,
        control_tx,
        &mut startup_tx,
        attempt.as_ref(),
        &quality,
        &datagram_queue,
        &wire_queue,
    )
    .await;
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
    socket: UdpSocket,
    mut connection: quiche::Connection,
    h3_config: quiche::h3::Config,
    mut outgoing_rx: mpsc::Receiver<OutgoingBatch>,
    incoming_tx: mpsc::Sender<PacketBatch>,
    control_tx: watch::Sender<PeerNetworkState>,
    startup_tx: &mut Option<oneshot::Sender<Result<(), StartupFailure>>>,
    attempt: Option<&ConnectionAttemptTelemetry>,
    quality: &NetworkQualityTelemetry,
    datagram_queue: &Arc<QueueMetrics>,
    wire_queue: &Arc<QueueMetrics>,
) -> Result<(), TransportError> {
    let local_address = socket.local_addr()?;
    let mut http3 = None;
    let mut request_stream_id = None;
    let mut response_accepted = false;
    let mut peer_settings_recorded = false;
    let mut ready = false;
    let mut control = ConnectIpControlPlane::new(control_tx);
    let mut pending_batch: Option<OutgoingBatch> = None;
    let mut wire_datagrams = VecDeque::with_capacity(MAX_PENDING_WIRE_DATAGRAMS);
    let mut datagram_entries = VecDeque::with_capacity(DATAGRAM_SEND_QUEUE_CAPACITY);
    let mut free_wire_buffers = Vec::new();
    quality.record_fresh_allocation();
    let mut receive_buffer = vec![0u8; 65_535];
    let mut incoming_batch = PacketBatch::new();
    let mut inbound_queue_drop_count = 0_u64;
    let mut keepalive = interval_at(Instant::now() + KEEPALIVE_INTERVAL, KEEPALIVE_INTERVAL);
    keepalive.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut quality_tick = interval_at(
        Instant::now() + QUALITY_SAMPLE_INTERVAL,
        QUALITY_SAMPLE_INTERVAL,
    );
    quality_tick.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
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

        if ready && let Some(stream_id) = request_stream_id {
            queue_pending_batch(
                &mut connection,
                stream_id,
                &mut pending_batch,
                &mut datagram_entries,
                datagram_queue,
                quality,
            )?;
        }

        let send_quantum = connection.send_quantum();
        generate_wire_datagrams(
            &mut connection,
            &mut wire_datagrams,
            &mut free_wire_buffers,
            send_quantum,
            wire_queue,
            quality,
        )?;
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

        tokio::select! {
            received = socket.recv_from(&mut receive_buffer) => {
                let (length, from) = received?;
                quality.record_udp_recv(1);
                let dropped = receive_quic_datagram(
                    &mut connection,
                    &mut receive_buffer[..length],
                    from,
                    local_address,
                )?;
                record_inbound_queue_drops(dropped, &mut inbound_queue_drop_count);
                for _ in 1..MAX_SOCKET_DRAIN {
                    match socket.try_recv_from(&mut receive_buffer) {
                        Ok((length, from)) => {
                            quality.record_udp_recv(1);
                            let dropped = receive_quic_datagram(
                                &mut connection,
                                &mut receive_buffer[..length],
                                from,
                                local_address,
                            )?;
                            record_inbound_queue_drops(
                                dropped,
                                &mut inbound_queue_drop_count,
                            );
                        }
                        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                            quality.record_udp_recv(0);
                            break;
                        }
                        Err(error) => {
                            quality.record_udp_recv(0);
                            return Err(error.into());
                        }
                    }
                }
            }
            batch = outgoing_rx.recv(), if ready && pending_batch.is_none() => {
                match batch {
                    Some(batch) => pending_batch = Some(batch),
                    None => return Ok(()),
                }
            }
            writable = socket.writable(), if wire_is_due => {
                writable?;
                send_due_wire_datagrams(
                    &socket,
                    &mut wire_datagrams,
                    &mut free_wire_buffers,
                    send_quantum,
                    wire_queue,
                    quality,
                )?;
            }
            _ = sleep_until(wire_deadline), if !wire_datagrams.is_empty() && !wire_is_due => {}
            _ = sleep_until(quic_deadline) => connection.on_timeout(),
            _ = keepalive.tick(), if connection.is_established() => {
                connection
                    .send_ack_eliciting()
                    .map_err(|error| TransportError::Http3(format!(
                        "queue QUIC keepalive: {error:?}"
                    )))?;
            }
            _ = quality_tick.tick(), if connection.is_established() => {
                if let Some(attempt) = attempt {
                    observe_h3_metrics(&connection, attempt, inbound_queue_drop_count);
                }
            }
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
    connection: &mut quiche::Connection,
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
    connection: &mut quiche::Connection,
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
    connection: &mut quiche::Connection,
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
        let Some(packet) = decode_http_datagram_bytes(request_stream_id, Bytes::from(datagram))?
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

fn queue_pending_batch(
    connection: &mut quiche::Connection,
    stream_id: u64,
    pending_batch: &mut Option<OutgoingBatch>,
    datagram_entries: &mut VecDeque<QueueEntry>,
    datagram_queue: &Arc<QueueMetrics>,
    quality: &NetworkQualityTelemetry,
) -> Result<(), TransportError> {
    let completed = {
        let Some(outgoing) = pending_batch.as_mut() else {
            return Ok(());
        };
        for _ in 0..MAX_SOCKET_DRAIN {
            if connection.is_dgram_send_queue_full() {
                break;
            }
            let Some(packet) = outgoing.batch.front() else {
                break;
            };
            let packet_len = packet.len();
            let datagram = encode_http_datagram(stream_id, packet)?;
            let datagram_overhead = datagram.len().saturating_sub(packet_len);
            let datagram_len = datagram.len();
            quality.record_fresh_allocation();
            quality.record_datagram_header_copy(datagram_overhead);
            let maximum_packet_size = connection
                .dgram_max_writable_len()
                .map(|maximum| maximum.saturating_sub(datagram_overhead))
                .unwrap_or_default();
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

fn reconcile_datagram_queue(
    connection: &quiche::Connection,
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

fn observe_h3_metrics(
    connection: &quiche::Connection,
    attempt: &ConnectionAttemptTelemetry,
    datagram_receive_drops: u64,
) {
    let Some(path) = connection.path_stats().find(|path| path.active) else {
        return;
    };
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
        pmtu_bytes: u32::try_from(path.pmtu).unwrap_or(u32::MAX),
    });
}

fn usize_to_u64(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

fn receive_quic_datagram(
    connection: &mut quiche::Connection,
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

fn generate_wire_datagrams(
    connection: &mut quiche::Connection,
    pending: &mut VecDeque<WireDatagram>,
    free_buffers: &mut Vec<Vec<u8>>,
    send_quantum: usize,
    wire_queue: &Arc<QueueMetrics>,
    quality: &NetworkQualityTelemetry,
) -> Result<(), TransportError> {
    if send_quantum < MAX_UDP_PAYLOAD_SIZE {
        return Ok(());
    }
    let mut generated_bytes = 0usize;
    while pending.len() < MAX_PENDING_WIRE_DATAGRAMS
        && generated_bytes.saturating_add(MAX_UDP_PAYLOAD_SIZE) <= send_quantum
    {
        let mut bytes = take_wire_buffer(free_buffers, quality);
        match connection.send(&mut bytes) {
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

fn send_due_wire_datagrams(
    socket: &UdpSocket,
    pending: &mut VecDeque<WireDatagram>,
    free_buffers: &mut Vec<Vec<u8>>,
    send_quantum: usize,
    wire_queue: &QueueMetrics,
    quality: &NetworkQualityTelemetry,
) -> Result<(), TransportError> {
    if !pending
        .front()
        .is_some_and(|datagram| datagram.send_info.at <= StdInstant::now())
    {
        return Ok(());
    }
    let mut sent_bytes = 0usize;
    for _ in 0..MAX_SOCKET_DRAIN {
        let Some(datagram) = pending.front() else {
            break;
        };
        if datagram.send_info.at > StdInstant::now() {
            break;
        }
        if sent_bytes.saturating_add(datagram.bytes.len()) > send_quantum {
            break;
        }
        match socket.try_send_to(&datagram.bytes, datagram.send_info.to) {
            Ok(sent) if sent == datagram.bytes.len() => {
                quality.record_udp_send(1);
                sent_bytes = sent_bytes.saturating_add(sent);
                let datagram = pending
                    .pop_front()
                    .expect("front exists after successful QUIC send");
                datagram.queue_entry.complete();
                if let Some(next) = pending.front() {
                    wire_queue.observe_oldest_entry(&next.queue_entry);
                }
                recycle_wire_buffer(free_buffers, datagram.bytes, quality);
            }
            Ok(_) => {
                quality.record_udp_send(0);
                quality.record_udp_partial_batch();
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "UDP socket sent a partial QUIC datagram",
                )
                .into());
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                quality.record_udp_send(0);
                break;
            }
            Err(error) => {
                quality.record_udp_send(0);
                return Err(error.into());
            }
        }
    }
    Ok(())
}

fn take_wire_buffer(free_buffers: &mut Vec<Vec<u8>>, quality: &NetworkQualityTelemetry) -> Vec<u8> {
    let mut bytes = match free_buffers.pop() {
        Some(bytes) => {
            quality.record_packet_buffer_pool_hit();
            quality.record_encode_buffer_reuse();
            bytes
        }
        None => {
            quality.record_packet_buffer_pool_miss();
            quality.record_fresh_allocation();
            Vec::with_capacity(MAX_UDP_PAYLOAD_SIZE)
        }
    };
    bytes.resize(MAX_UDP_PAYLOAD_SIZE, 0);
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

fn encode_http_datagram(stream_id: u64, packet: &[u8]) -> Result<Vec<u8>, TransportError> {
    validate_ip_packet(packet)?;
    let mut encoded = Vec::with_capacity(packet.len() + 16);
    encode_varint(stream_id / 4, &mut encoded)?;
    encode_varint(usque_protocol::DEFAULT_CONTEXT_ID, &mut encoded)?;
    encoded.extend_from_slice(packet);
    Ok(encoded)
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
    let length = match value {
        0..=63 => 1,
        64..=16_383 => 2,
        16_384..=1_073_741_823 => 4,
        1_073_741_824..=4_611_686_018_427_387_903 => 8,
        _ => return Err(TransportError::InvalidVarint),
    };
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

fn connection_closed_error(connection: &quiche::Connection) -> TransportError {
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
    use super::*;

    struct DropSignal(Option<oneshot::Sender<()>>);

    impl Drop for DropSignal {
        fn drop(&mut self) {
            if let Some(sender) = self.0.take() {
                let _ = sender.send(());
            }
        }
    }

    fn ipv4_packet() -> [u8; 20] {
        [
            0x45, 0, 0, 20, 0, 0, 0, 0, 64, 17, 0, 0, 1, 1, 1, 1, 8, 8, 8, 8,
        ]
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
        let encoded = encode_http_datagram(8, &packet).unwrap();
        assert_eq!(decode_varint(&encoded).unwrap(), Some((2, 1)));
        assert_eq!(decode_varint(&encoded[1..]).unwrap(), Some((0, 1)));
        assert_eq!(
            decode_http_datagram(8, &encoded).unwrap().unwrap().as_ref(),
            packet
        );
        assert!(decode_http_datagram(4, &encoded).unwrap().is_none());
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

            assert_eq!(encode_http_datagram(stream_id, &packet).unwrap(), reference);
        }
    }

    #[test]
    fn owned_http_datagram_decode_reuses_the_receive_allocation() {
        let packet = ipv4_packet();
        let encoded = encode_http_datagram(8, &packet).unwrap();
        let allocation = encoded.as_ptr() as usize;
        let (_, stream_prefix) = decode_varint(&encoded).unwrap().unwrap();
        let (_, context_prefix) = decode_varint(&encoded[stream_prefix..]).unwrap().unwrap();
        let expected_payload = allocation + stream_prefix + context_prefix;

        let decoded = decode_http_datagram_bytes(8, Bytes::from(encoded))
            .unwrap()
            .unwrap();
        assert_eq!(decoded.as_ptr() as usize, expected_payload);
        assert_eq!(decoded.as_ref(), packet);
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
    fn wire_datagram_buffers_are_reused_with_a_fixed_bound() {
        let mut free = Vec::new();
        let quality = NetworkQualityTelemetry::default();
        let first = take_wire_buffer(&mut free, &quality);
        assert_eq!(first.len(), MAX_UDP_PAYLOAD_SIZE);
        let allocation = first.as_ptr();
        recycle_wire_buffer(&mut free, first, &quality);

        let reused = take_wire_buffer(&mut free, &quality);
        assert_eq!(reused.len(), MAX_UDP_PAYLOAD_SIZE);
        assert_eq!(reused.as_ptr(), allocation);
        recycle_wire_buffer(&mut free, reused, &quality);

        for _ in 0..=MAX_PENDING_WIRE_DATAGRAMS {
            recycle_wire_buffer(
                &mut free,
                Vec::with_capacity(MAX_UDP_PAYLOAD_SIZE),
                &quality,
            );
        }
        assert_eq!(free.len(), MAX_PENDING_WIRE_DATAGRAMS);
    }

    #[tokio::test]
    async fn udp_send_drain_respects_send_quantum_and_pacing_deadline() {
        let receiver = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let sender = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let from = sender.local_addr().unwrap();
        let to = receiver.local_addr().unwrap();
        let due = StdInstant::now();
        let future = due + Duration::from_secs(60);
        let quality = NetworkQualityTelemetry::default();
        let wire_queue = QueueMetrics::new(
            QueueKind::H3WireSend,
            MAX_PENDING_WIRE_DATAGRAMS,
            MAX_PENDING_WIRE_DATAGRAMS * MAX_UDP_PAYLOAD_SIZE,
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

        sender.writable().await.unwrap();
        send_due_wire_datagrams(&sender, &mut pending, &mut free, 150, &wire_queue, &quality)
            .unwrap();
        assert_eq!(pending.len(), 2);
        let mut received = [0u8; 128];
        let length = timeout(Duration::from_secs(1), receiver.recv(&mut received))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&received[..length], &[1; 100]);

        sender.writable().await.unwrap();
        send_due_wire_datagrams(&sender, &mut pending, &mut free, 500, &wire_queue, &quality)
            .unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending.front().unwrap().bytes, vec![3; 100]);
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
