use std::collections::VecDeque;
use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket as StdUdpSocket};
use std::sync::Arc;
use std::time::{Duration, Instant as StdInstant};

use boring::ssl::{SslContextBuilder, SslMethod};
use bytes::{Buf, Bytes, BytesMut};
use quiche::h3::NameValue;
use tokio::net::UdpSocket;
use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::JoinHandle;
use tokio::time::{Instant, MissedTickBehavior, interval_at, sleep_until, timeout};
use tokio_util::task::AbortOnDropHandle;
use usque_protocol::{
    AddressAssign, CapsuleEffect, ConnectIpCapsule, IpDatagram, IpPrefix, MAX_CAPSULE_PAYLOAD,
    PeerNetworkState,
};

use crate::h2::{
    MasqueTlsIdentity, PinState, TransportError, configure_client_identity_and_pin,
    validate_ip_packet,
};
use crate::socket::{SocketProtector, noop_socket_protector, socket_handle};

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
const DATAGRAM_CHANNEL_CAPACITY: usize = 1_024;
const MAX_PENDING_WIRE_DATAGRAMS: usize = 64;

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
    sender: Option<mpsc::Sender<OutgoingPacket>>,
}

impl H3SendHalf {
    pub async fn send_packet(&mut self, packet: &[u8]) -> Result<(), TransportError> {
        validate_ip_packet(packet)?;
        let (completion_tx, completion_rx) = oneshot::channel();
        self.sender
            .as_ref()
            .ok_or(TransportError::TunnelClosed)?
            .send(OutgoingPacket {
                packet: Bytes::copy_from_slice(packet),
                completion: completion_tx,
            })
            .await
            .map_err(|_| TransportError::TunnelClosed)?;
        match completion_rx.await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(DatagramSendFailure::TooLarge {
                maximum_packet_size,
            })) => Err(TransportError::Http3DatagramTooLarge {
                maximum_packet_size,
            }),
            Err(_) => Err(TransportError::TunnelClosed),
        }
    }

    pub fn close(&mut self) {
        self.sender.take();
    }
}

struct OutgoingPacket {
    packet: Bytes,
    completion: oneshot::Sender<Result<(), DatagramSendFailure>>,
}

enum DatagramSendFailure {
    TooLarge { maximum_packet_size: usize },
}

pub struct H3ReceiveHalf {
    receiver: mpsc::Receiver<Bytes>,
}

impl H3ReceiveHalf {
    pub async fn receive_packet(&mut self) -> Result<Bytes, TransportError> {
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
        self.task
            .take()
            .expect("H3 driver task is present until wait")
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
    connect_h3_with_protector(endpoint, sni, identity, noop_socket_protector().as_ref()).await
}

pub(crate) async fn connect_h3_with_protector(
    endpoint: SocketAddr,
    sni: &str,
    identity: &MasqueTlsIdentity,
    protector: &dyn SocketProtector,
) -> Result<H3Tunnel, TransportError> {
    let first = connect_h3_once(endpoint, sni, identity, protector).await;
    match first {
        Err(TransportError::Http3ProtocolViolation(_)) => {
            // The Go oracle retries this specific Cloudflare interoperability
            // failure once. All other failures preserve normal fallback rules.
            connect_h3_once(endpoint, sni, identity, protector).await
        }
        result => result,
    }
}

async fn connect_h3_once(
    endpoint: SocketAddr,
    sni: &str,
    identity: &MasqueTlsIdentity,
    protector: &dyn SocketProtector,
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

    let (outgoing_tx, outgoing_rx) = mpsc::channel(DATAGRAM_CHANNEL_CAPACITY);
    let (incoming_tx, incoming_rx) = mpsc::channel(DATAGRAM_CHANNEL_CAPACITY);
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
    )));

    let startup = timeout(CONNECT_TIMEOUT, startup_rx).await;
    match startup {
        Ok(Ok(Ok(()))) => Ok(H3Tunnel {
            send: H3SendHalf {
                sender: Some(outgoing_tx),
            },
            receive: H3ReceiveHalf {
                receiver: incoming_rx,
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
    config.enable_dgram(true, DATAGRAM_CHANNEL_CAPACITY, DATAGRAM_CHANNEL_CAPACITY);
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

async fn run_h3_actor(
    socket: UdpSocket,
    connection: quiche::Connection,
    h3_config: quiche::h3::Config,
    outgoing_rx: mpsc::Receiver<OutgoingPacket>,
    incoming_tx: mpsc::Sender<Bytes>,
    control_tx: watch::Sender<PeerNetworkState>,
    startup_tx: oneshot::Sender<Result<(), StartupFailure>>,
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

#[allow(clippy::too_many_arguments)]
async fn drive_h3_actor(
    socket: UdpSocket,
    mut connection: quiche::Connection,
    h3_config: quiche::h3::Config,
    mut outgoing_rx: mpsc::Receiver<OutgoingPacket>,
    incoming_tx: mpsc::Sender<Bytes>,
    control_tx: watch::Sender<PeerNetworkState>,
    startup_tx: &mut Option<oneshot::Sender<Result<(), StartupFailure>>>,
) -> Result<(), TransportError> {
    let local_address = socket.local_addr()?;
    let mut http3 = None;
    let mut request_stream_id = None;
    let mut response_accepted = false;
    let mut ready = false;
    let mut control = H3ControlPlane::new(control_tx);
    let mut pending_packet: Option<OutgoingPacket> = None;
    let mut wire_datagrams = VecDeque::new();
    let mut receive_buffer = vec![0u8; 65_535];
    let mut keepalive = interval_at(Instant::now() + KEEPALIVE_INTERVAL, KEEPALIVE_INTERVAL);
    keepalive.set_missed_tick_behavior(MissedTickBehavior::Delay);

    loop {
        if connection.is_established() && http3.is_none() {
            http3 = Some(
                quiche::h3::Connection::with_transport(&mut connection, &h3_config).map_err(
                    |error| TransportError::Http3(format!("start HTTP/3 session: {error:?}")),
                )?,
            );
        }

        if let Some(http3) = http3.as_mut() {
            process_http3_events(
                http3,
                &mut connection,
                request_stream_id,
                &mut response_accepted,
                &mut control,
            )?;

            if let Some(stream_id) = request_stream_id {
                flush_control_capsules(http3, &mut connection, stream_id, &mut control.pending)?;
            }

            if request_stream_id.is_none() && http3.peer_settings_raw().is_some() {
                if !http3.dgram_enabled_by_peer(&connection) {
                    return Err(TransportError::Http3DatagramUnavailable);
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
                &mut receive_buffer,
            )?;
        }

        if ready
            && let (Some(stream_id), Some(packet)) = (request_stream_id, pending_packet.as_ref())
        {
            let datagram = encode_http_datagram(stream_id, &packet.packet)?;
            let datagram_overhead = datagram.len().saturating_sub(packet.packet.len());
            let maximum_packet_size = connection
                .dgram_max_writable_len()
                .map(|maximum| maximum.saturating_sub(datagram_overhead))
                .unwrap_or_default();
            match connection.dgram_send(&datagram) {
                Ok(()) => {
                    if let Some(packet) = pending_packet.take() {
                        let _ = packet.completion.send(Ok(()));
                    }
                }
                Err(quiche::Error::Done) => {}
                Err(quiche::Error::BufferTooShort) => {
                    if let Some(packet) = pending_packet.take() {
                        let _ = packet.completion.send(Err(DatagramSendFailure::TooLarge {
                            maximum_packet_size,
                        }));
                    }
                }
                Err(error) => {
                    return Err(TransportError::Http3(format!(
                        "queue CONNECT-IP datagram: {error:?}"
                    )));
                }
            }
        }

        generate_wire_datagrams(&mut connection, &mut wire_datagrams)?;
        send_due_wire_datagrams(&socket, &mut wire_datagrams).await?;

        if connection.is_closed() {
            return Err(connection_closed_error(&connection));
        }

        let quic_deadline =
            Instant::now() + connection.timeout().unwrap_or(Duration::from_secs(60));
        let wire_deadline = wire_datagrams
            .front()
            .map(|datagram| Instant::from_std(datagram.send_info.at))
            .unwrap_or_else(|| Instant::now() + Duration::from_secs(86_400));

        tokio::select! {
            received = socket.recv_from(&mut receive_buffer) => {
                let (length, from) = received?;
                let info = quiche::RecvInfo {
                    from,
                    to: local_address,
                };
                match connection.recv(&mut receive_buffer[..length], info) {
                    Ok(_) | Err(quiche::Error::Done) => {}
                    Err(error) => {
                        return Err(TransportError::Http3(format!(
                            "receive QUIC packet: {error:?}"
                        )));
                    }
                }
            }
            packet = outgoing_rx.recv(), if ready && pending_packet.is_none() => {
                match packet {
                    Some(packet) => pending_packet = Some(packet),
                    None => return Ok(()),
                }
            }
            _ = sleep_until(wire_deadline), if !wire_datagrams.is_empty() => {}
            _ = sleep_until(quic_deadline) => connection.on_timeout(),
            _ = keepalive.tick(), if connection.is_established() => {
                connection
                    .send_ack_eliciting()
                    .map_err(|error| TransportError::Http3(format!(
                        "queue QUIC keepalive: {error:?}"
                    )))?;
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
    control: &mut H3ControlPlane,
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
                            drain_control_capsules(
                                &mut control.buffer,
                                &mut control.state,
                                &control.state_tx,
                                &mut control.pending,
                            )?;
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

struct PendingControlCapsule {
    bytes: Bytes,
    offset: usize,
}

struct H3ControlPlane {
    buffer: BytesMut,
    state: PeerNetworkState,
    state_tx: watch::Sender<PeerNetworkState>,
    pending: VecDeque<PendingControlCapsule>,
}

impl H3ControlPlane {
    fn new(state_tx: watch::Sender<PeerNetworkState>) -> Self {
        Self {
            buffer: BytesMut::with_capacity(4_096),
            state: PeerNetworkState::default(),
            state_tx,
            pending: VecDeque::new(),
        }
    }
}

fn drain_control_capsules(
    buffer: &mut BytesMut,
    state: &mut PeerNetworkState,
    state_tx: &watch::Sender<PeerNetworkState>,
    pending: &mut VecDeque<PendingControlCapsule>,
) -> Result<(), TransportError> {
    loop {
        let mut cursor = buffer.clone().freeze();
        let Some(capsule) = ConnectIpCapsule::decode_if_complete(&mut cursor)? else {
            return Ok(());
        };
        let consumed = buffer.len() - cursor.len();
        buffer.advance(consumed);

        match state.apply(&capsule) {
            CapsuleEffect::AssignmentsReplaced | CapsuleEffect::RoutesReplaced => {
                state_tx.send_replace(state.clone());
            }
            CapsuleEffect::AddressRequested(request) => {
                let addresses = request
                    .addresses
                    .into_iter()
                    .map(|requested| IpPrefix {
                        request_id: requested.request_id,
                        address: match requested.address {
                            IpAddr::V4(_) => IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                            IpAddr::V6(_) => IpAddr::V6(Ipv6Addr::UNSPECIFIED),
                        },
                        prefix_len: match requested.address {
                            IpAddr::V4(_) => 32,
                            IpAddr::V6(_) => 128,
                        },
                    })
                    .collect();
                let rejection =
                    ConnectIpCapsule::AddressAssign(AddressAssign { addresses }).encode()?;
                pending.push_back(PendingControlCapsule {
                    bytes: rejection,
                    offset: 0,
                });
            }
            CapsuleEffect::UnknownIgnored(capsule_type) => {
                tracing::debug!(
                    capsule_type,
                    "ignored an unknown CONNECT-IP control capsule"
                );
            }
        }
    }
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
    incoming_tx: &mpsc::Sender<Bytes>,
    buffer: &mut [u8],
) -> Result<(), TransportError> {
    loop {
        let length = match connection.dgram_recv(buffer) {
            Ok(length) => length,
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
        let Some(packet) = decode_http_datagram(request_stream_id, &buffer[..length])? else {
            continue;
        };
        match incoming_tx.try_send(packet) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                tracing::warn!("dropping an inbound H3 packet because the netstack is congested");
            }
            Err(TrySendError::Closed(_)) => return Ok(()),
        }
    }
    Ok(())
}

struct WireDatagram {
    bytes: Vec<u8>,
    send_info: quiche::SendInfo,
}

fn generate_wire_datagrams(
    connection: &mut quiche::Connection,
    pending: &mut VecDeque<WireDatagram>,
) -> Result<(), TransportError> {
    while pending.len() < MAX_PENDING_WIRE_DATAGRAMS {
        let mut bytes = vec![0u8; MAX_UDP_PAYLOAD_SIZE];
        match connection.send(&mut bytes) {
            Ok((length, send_info)) => {
                bytes.truncate(length);
                pending.push_back(WireDatagram { bytes, send_info });
            }
            Err(quiche::Error::Done) => break,
            Err(error) => {
                return Err(TransportError::Http3(format!(
                    "generate QUIC packet: {error:?}"
                )));
            }
        }
    }
    Ok(())
}

async fn send_due_wire_datagrams(
    socket: &UdpSocket,
    pending: &mut VecDeque<WireDatagram>,
) -> Result<(), TransportError> {
    while pending
        .front()
        .is_some_and(|datagram| datagram.send_info.at <= StdInstant::now())
    {
        let datagram = pending
            .pop_front()
            .expect("front exists while draining due QUIC packets");
        let sent = socket
            .send_to(&datagram.bytes, datagram.send_info.to)
            .await?;
        if sent != datagram.bytes.len() {
            return Err(io::Error::new(
                io::ErrorKind::WriteZero,
                "UDP socket sent a partial QUIC datagram",
            )
            .into());
        }
    }
    Ok(())
}

fn encode_http_datagram(stream_id: u64, packet: &[u8]) -> Result<Vec<u8>, TransportError> {
    validate_ip_packet(packet)?;
    let payload = IpDatagram::new(Bytes::copy_from_slice(packet)).encode()?;
    let mut encoded = Vec::with_capacity(payload.len() + 8);
    encode_varint(stream_id / 4, &mut encoded)?;
    encoded.extend_from_slice(&payload);
    Ok(encoded)
}

fn decode_http_datagram(
    request_stream_id: u64,
    datagram: &[u8],
) -> Result<Option<Bytes>, TransportError> {
    let Some((quarter_stream_id, stream_bytes)) = decode_varint(datagram)? else {
        return Err(TransportError::MalformedIpPacket);
    };
    if quarter_stream_id != request_stream_id / 4 {
        return Ok(None);
    }
    let payload = datagram
        .get(stream_bytes..)
        .ok_or(TransportError::MalformedIpPacket)?;
    let datagram = IpDatagram::decode(Bytes::copy_from_slice(payload))?;
    if datagram.context_id != usque_protocol::DEFAULT_CONTEXT_ID {
        return Ok(None);
    }
    validate_ip_packet(&datagram.packet)?;
    Ok(Some(datagram.packet))
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
    use usque_protocol::{AddressRequest, IpAddressRange, RouteAdvertisement};

    fn ipv4_packet() -> [u8; 20] {
        [
            0x45, 0, 0, 20, 0, 0, 0, 0, 64, 17, 0, 0, 1, 1, 1, 1, 8, 8, 8, 8,
        ]
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

    #[test]
    fn control_capsules_replace_peer_state_across_fragmented_reads() {
        let assignment = ConnectIpCapsule::AddressAssign(AddressAssign {
            addresses: vec![IpPrefix {
                request_id: 0,
                address: "172.16.0.2".parse().unwrap(),
                prefix_len: 32,
            }],
        })
        .encode()
        .unwrap();
        let routes = ConnectIpCapsule::RouteAdvertisement(RouteAdvertisement {
            ranges: vec![IpAddressRange {
                start: "0.0.0.0".parse().unwrap(),
                end: "255.255.255.255".parse().unwrap(),
                protocol: 0,
            }],
        })
        .encode()
        .unwrap();
        let mut wire = BytesMut::new();
        wire.extend_from_slice(&assignment);
        wire.extend_from_slice(&routes);

        let (state_tx, state_rx) = watch::channel(PeerNetworkState::default());
        let mut state = PeerNetworkState::default();
        let mut pending = VecDeque::new();
        let mut buffer = BytesMut::new();
        for byte in wire {
            buffer.extend_from_slice(&[byte]);
            drain_control_capsules(&mut buffer, &mut state, &state_tx, &mut pending).unwrap();
        }

        assert!(buffer.is_empty());
        assert!(pending.is_empty());
        assert_eq!(state_rx.borrow().assigned_addresses.len(), 1);
        assert_eq!(state_rx.borrow().available_routes.len(), 1);
    }

    #[test]
    fn address_requests_are_rejected_without_assigning_peer_addresses() {
        let request = ConnectIpCapsule::AddressRequest(AddressRequest {
            addresses: vec![
                IpPrefix {
                    request_id: 7,
                    address: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                    prefix_len: 32,
                },
                IpPrefix {
                    request_id: 8,
                    address: IpAddr::V6(Ipv6Addr::UNSPECIFIED),
                    prefix_len: 128,
                },
            ],
        })
        .encode()
        .unwrap();
        let (state_tx, _state_rx) = watch::channel(PeerNetworkState::default());
        let mut state = PeerNetworkState::default();
        let mut pending = VecDeque::new();
        let mut buffer = BytesMut::from(request.as_ref());

        drain_control_capsules(&mut buffer, &mut state, &state_tx, &mut pending).unwrap();
        let mut rejection = pending.pop_front().unwrap().bytes;
        let ConnectIpCapsule::AddressAssign(rejection) =
            ConnectIpCapsule::decode(&mut rejection).unwrap()
        else {
            panic!("expected ADDRESS_ASSIGN rejection");
        };
        assert_eq!(rejection.addresses[0].request_id, 7);
        assert_eq!(
            rejection.addresses[0].address,
            IpAddr::V4(Ipv4Addr::UNSPECIFIED)
        );
        assert_eq!(rejection.addresses[0].prefix_len, 32);
        assert_eq!(rejection.addresses[1].request_id, 8);
        assert_eq!(
            rejection.addresses[1].address,
            IpAddr::V6(Ipv6Addr::UNSPECIFIED)
        );
        assert_eq!(rejection.addresses[1].prefix_len, 128);
    }
}
