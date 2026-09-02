use std::collections::VecDeque;
use std::future::Future;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use boring::asn1::{Asn1Integer, Asn1Time};
use boring::bn::BigNum;
use boring::error::ErrorStack;
use boring::hash::MessageDigest;
use boring::pkey::{PKey, Private};
use boring::ssl::{
    SslAlert, SslConnector, SslContextBuilder, SslMethod, SslVerifyError, SslVerifyMode,
};
use boring::x509::{X509, X509NameBuilder};
#[cfg(test)]
use bytes::Buf;
use bytes::{Bytes, BytesMut};
use h2::{Ping, PingPong, RecvStream, SendStream};
use http::{Method, Request, StatusCode, Version};
use p256::SecretKey;
use p256::pkcs8::EncodePrivateKey;
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpSocket;
use tokio::sync::{OwnedSemaphorePermit, Semaphore, mpsc, oneshot, watch};
use tokio::task::JoinHandle;
use tokio::time::{Instant, MissedTickBehavior, interval_at, timeout};
use tokio_util::task::AbortOnDropHandle;
use usque_core::{
    AddressFamily, EndpointPin, Transport, TransportFailure, TransportFailureCode, TransportStage,
};
use usque_protocol::{ConnectIpCapsule, MAX_CAPSULE_PAYLOAD, PeerNetworkState};
use zeroize::Zeroizing;

use crate::connect_ip_control::{
    ConnectIpControlPlane, MAX_PENDING_CONTROL_BYTES, MAX_PENDING_CONTROL_CAPSULES,
};
use crate::network_quality::NetworkQualityTelemetry;
use crate::packet_batch::{PacketBatch, PacketBatchResult};
use crate::socket::{
    DirectProtocol, LeasedIo, STALE_GENERATION_REASON, SocketProtector, noop_socket_protector,
    socket_handle,
};
use crate::telemetry::{ConnectionAttemptTelemetry, ConnectionEventType};

const CONNECT_URI: &str = "https://cloudflareaccess.com/";
const H2_ALPN: &[u8] = b"\x02h2";
const DATAGRAM_CAPSULE_TYPE: u64 = 0;
const MAX_CAPSULE_BYTES: usize = 65_535;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(8);
const H2_OUTGOING_CAPACITY: usize = 1_024;
const H2_PACKET_QUEUE_CAPACITY: usize = 1_024;
const PACKET_SEND_TIMEOUT: Duration = Duration::from_secs(10);
const H2_PING_INTERVAL: Duration = Duration::from_secs(5);
const H2_PING_DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);
const H2_PING_MIN_TIMEOUT: Duration = Duration::from_secs(2);
const H2_PING_MAX_TIMEOUT: Duration = Duration::from_secs(10);
const H2_CAPACITY_STALL_THRESHOLD: Duration = Duration::from_millis(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct H2FlowControlConfig {
    stream_receive_window: u32,
    connection_receive_window: u32,
}

impl Default for H2FlowControlConfig {
    fn default() -> Self {
        Self::for_features(crate::PRODUCTION_NETWORK_FEATURES)
    }
}

impl H2FlowControlConfig {
    const fn for_features(features: crate::NetworkFeatureFlags) -> Self {
        Self {
            stream_receive_window: if features.h2_tuned_flow_control {
                4 * 1024 * 1024
            } else {
                65_535
            },
            connection_receive_window: if features.h2_tuned_flow_control {
                8 * 1024 * 1024
            } else {
                65_535
            },
        }
    }
}

/// Secret and enrolled identity material required by a MASQUE TLS session.
///
/// The SEC1 key bytes remain zeroizing from secure-vault read through BoringSSL
/// import. Public pin and assigned addresses are safe to retain for the session.
pub struct MasqueTlsIdentity {
    private_key_sec1_der: Zeroizing<Vec<u8>>,
    endpoint_pin: EndpointPin,
    pub assigned_ipv4: Ipv4Addr,
    pub assigned_ipv6: Ipv6Addr,
}

impl MasqueTlsIdentity {
    pub fn new(
        private_key_sec1_der: Zeroizing<Vec<u8>>,
        endpoint_pin_spki_der: &[u8],
        assigned_ipv4: Ipv4Addr,
        assigned_ipv6: Ipv6Addr,
    ) -> Result<Self, TransportError> {
        // Validate before retaining material so malformed vault records fail
        // deterministically rather than surfacing as an opaque TLS error.
        SecretKey::from_sec1_der(&private_key_sec1_der)
            .map_err(|_| TransportError::InvalidPrivateKey)?;
        let endpoint_pin = EndpointPin::from_spki_der(endpoint_pin_spki_der)
            .map_err(|_| TransportError::InvalidEndpointPin)?;
        Ok(Self {
            private_key_sec1_der,
            endpoint_pin,
            assigned_ipv4,
            assigned_ipv6,
        })
    }
}

/// An established Cloudflare CONNECT-IP stream over HTTP/2.
pub struct H2Tunnel {
    send: H2SendHalf,
    receive: H2ReceiveHalf,
    driver: H2Driver,
    control: watch::Receiver<PeerNetworkState>,
    flow_control: H2FlowControlConfig,
    ping_supported: bool,
    quality: NetworkQualityTelemetry,
    attempt: Option<ConnectionAttemptTelemetry>,
}

impl H2Tunnel {
    pub fn into_parts(
        self,
    ) -> (
        H2SendHalf,
        H2ReceiveHalf,
        H2Driver,
        watch::Receiver<PeerNetworkState>,
    ) {
        (self.send, self.receive, self.driver, self.control)
    }

    pub fn control_state(&self) -> PeerNetworkState {
        self.control.borrow().clone()
    }

    pub(crate) fn activate_network_quality(&self) {
        self.quality.configure_h2_connection(
            self.flow_control.stream_receive_window,
            self.flow_control.connection_receive_window,
            self.ping_supported,
        );
        if let Some(attempt) = &self.attempt {
            attempt.promote();
        }
    }
}

struct H2Outgoing {
    bytes: Bytes,
    accepted_bytes: usize,
    completion: oneshot::Sender<Result<usize, TransportError>>,
}

struct H2Rejection {
    bytes: Bytes,
    _byte_permit: OwnedSemaphorePermit,
}

pub struct H2SendHalf {
    sender: Option<mpsc::Sender<H2Outgoing>>,
    _writer: AbortOnDropHandle<Result<(), TransportError>>,
}

impl H2SendHalf {
    /// Sends one raw IP packet as an HTTP Capsule DATAGRAM.
    pub async fn send_packet(&mut self, packet: &[u8]) -> Result<(), TransportError> {
        validate_ip_packet(packet)?;
        match timeout(
            PACKET_SEND_TIMEOUT,
            self.send_owned_batch(PacketBatch::single(Bytes::copy_from_slice(packet))),
        )
        .await
        {
            Ok(result) => result.map(|_| ()),
            Err(_) => Err(TransportError::SendTimeout),
        }
    }

    /// Writes one already-framed Capsule Protocol record on the CONNECT-IP
    /// request stream. ADDRESS_REQUEST rejections use this path so the receive
    /// half never touches `SendStream`.
    pub async fn send_capsule(&mut self, capsule: Bytes) -> Result<(), TransportError> {
        match timeout(PACKET_SEND_TIMEOUT, self.send_capsule_inner(capsule)).await {
            Ok(result) => result,
            Err(_) => Err(TransportError::SendTimeout),
        }
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
            let (encoded, accepted_bytes) = encode_datagram_batch(&batch)?;
            let accepted_bytes = Self::send_encoded(sender, encoded, accepted_bytes).await?;
            Ok(PacketBatchResult {
                accepted_bytes,
                oversized: Vec::new(),
            })
        })
    }

    async fn send_capsule_inner(&self, capsule: Bytes) -> Result<(), TransportError> {
        Self::send_encoded(self.sender.clone(), capsule, 0).await?;
        Ok(())
    }

    async fn send_encoded(
        sender: Option<mpsc::Sender<H2Outgoing>>,
        bytes: Bytes,
        accepted_bytes: usize,
    ) -> Result<usize, TransportError> {
        let (completion_tx, completion_rx) = oneshot::channel();
        let sender = sender.ok_or(TransportError::TunnelClosed)?;
        let permit = sender
            .reserve()
            .await
            .map_err(|_| TransportError::TunnelClosed)?;
        permit.send(H2Outgoing {
            bytes,
            accepted_bytes,
            completion: completion_tx,
        });
        match completion_rx.await {
            Ok(result) => result,
            Err(_) => Err(TransportError::TunnelClosed),
        }
    }

    pub fn close(&mut self) {
        self.sender.take();
    }
}

pub struct H2ReceiveHalf {
    stream: RecvStream,
    control: ConnectIpControlPlane,
    packets: VecDeque<Bytes>,
    rejections: mpsc::Sender<H2Rejection>,
    rejection_bytes: Arc<Semaphore>,
}

impl H2ReceiveHalf {
    /// Receives the next raw IP packet, transparently handling capsules split
    /// across or coalesced within HTTP/2 DATA frames.
    pub async fn receive_packet(&mut self) -> Result<Bytes, TransportError> {
        loop {
            self.drain_ready_capsules()?;
            if let Some(packet) = self.packets.pop_front() {
                return Ok(packet);
            }

            let chunk = self
                .stream
                .data()
                .await
                .ok_or(TransportError::TunnelClosed)??;
            if self.control.buffer.len().saturating_add(chunk.len()) > MAX_CAPSULE_PAYLOAD + 16 {
                return Err(TransportError::CapsuleTooLarge);
            }
            let length = chunk.len();
            self.control.buffer.extend_from_slice(&chunk);
            self.stream.flow_control().release_capacity(length)?;
        }
    }

    pub(crate) async fn receive_batch(&mut self) -> Result<PacketBatch, TransportError> {
        let first = self.receive_packet().await?;
        let mut batch = PacketBatch::single(first);
        while let Some(packet) = self.packets.pop_front() {
            if let Err(packet) = batch.push_back(packet) {
                self.packets.push_front(packet);
                break;
            }
        }
        Ok(batch)
    }

    fn drain_ready_capsules(&mut self) -> Result<(), TransportError> {
        loop {
            if self.packets.len() >= H2_PACKET_QUEUE_CAPACITY {
                return Ok(());
            }
            let Some(capsule) = take_complete_capsule(&mut self.control.buffer)? else {
                return Ok(());
            };
            if let ConnectIpCapsule::Unknown {
                capsule_type: DATAGRAM_CAPSULE_TYPE,
                payload,
            } = &capsule
            {
                validate_ip_packet(payload)?;
                self.packets.push_back(payload.clone());
                continue;
            }
            self.control.apply(&capsule)?;
            self.flush_pending_rejections()?;
        }
    }

    fn flush_pending_rejections(&mut self) -> Result<(), TransportError> {
        while let Some(pending) = self.control.pending.pop_front() {
            let bytes = pending.bytes.slice(pending.offset..);
            if bytes.len() > MAX_PENDING_CONTROL_BYTES {
                return Err(TransportError::SendQueueFull);
            }
            let permits = u32::try_from(bytes.len()).map_err(|_| TransportError::SendQueueFull)?;
            let byte_permit = Arc::clone(&self.rejection_bytes)
                .try_acquire_many_owned(permits)
                .map_err(|_| TransportError::SendQueueFull)?;
            match self.rejections.try_send(H2Rejection {
                bytes,
                _byte_permit: byte_permit,
            }) {
                Ok(()) => {}
                Err(mpsc::error::TrySendError::Full(_)) => {
                    return Err(TransportError::SendQueueFull);
                }
                Err(mpsc::error::TrySendError::Closed(_)) => {
                    return Err(TransportError::TunnelClosed);
                }
            }
        }
        Ok(())
    }
}

/// Drives the underlying HTTP/2 connection. Dropping or aborting this handle
/// immediately tears down the transport.
pub struct H2Driver {
    task: Option<JoinHandle<Result<(), h2::Error>>>,
}

impl H2Driver {
    pub async fn wait(mut self) -> Result<(), TransportError> {
        let task = self
            .task
            .take()
            .expect("H2 driver task is present until wait");
        AbortOnDropHandle::new(task)
            .await
            .map_err(|error| TransportError::Driver(error.to_string()))?
            .map_err(TransportError::Http2)
    }

    pub fn abort(&self) {
        if let Some(task) = &self.task {
            task.abort();
        }
    }
}

impl Drop for H2Driver {
    fn drop(&mut self) {
        if let Some(task) = &self.task {
            task.abort();
        }
    }
}

/// Establishes the Cloudflare-specific HTTP/2 CONNECT-IP variant used by the
/// Go oracle. The TCP socket is pinned to `endpoint`; SNI is independent.
pub async fn connect_h2(
    endpoint: SocketAddr,
    sni: &str,
    identity: &MasqueTlsIdentity,
) -> Result<H2Tunnel, TransportError> {
    connect_h2_with_protector(
        endpoint,
        sni,
        identity,
        noop_socket_protector().as_ref(),
        None,
    )
    .await
}

pub(crate) async fn connect_h2_with_protector(
    endpoint: SocketAddr,
    sni: &str,
    identity: &MasqueTlsIdentity,
    protector: &dyn SocketProtector,
    attempt: Option<&ConnectionAttemptTelemetry>,
) -> Result<H2Tunnel, TransportError> {
    let expected_generation = protector.network_generation().unwrap_or_default();
    let socket = if endpoint.is_ipv4() {
        TcpSocket::new_v4()
    } else {
        TcpSocket::new_v6()
    }?;
    let egress_lease = protector
        .protect_for_target_generation(
            socket_handle(&socket),
            endpoint,
            DirectProtocol::Tcp,
            expected_generation,
        )
        .await
        .map_err(|error| {
            if error == STALE_GENERATION_REASON {
                TransportError::UnderlyingNetworkChanged
            } else {
                TransportError::SocketProtection(error)
            }
        })?;
    if protector.network_generation().unwrap_or_default() != expected_generation
        || egress_lease.generation() != Some(expected_generation)
    {
        drop(socket);
        drop(egress_lease);
        return Err(TransportError::UnderlyingNetworkChanged);
    }
    let tcp = timeout(CONNECT_TIMEOUT, socket.connect(endpoint))
        .await
        .map_err(|_| TransportError::EndpointTimeout(endpoint))??;
    tcp.set_nodelay(true)?;
    if protector.network_generation().unwrap_or_default() != expected_generation {
        return Err(TransportError::UnderlyingNetworkChanged);
    }
    if let Some(attempt) = attempt {
        attempt.record(
            ConnectionEventType::SocketConnected,
            TransportStage::SocketConnect,
        );
    }

    let (connector, pin_state) = tls_connector(identity)?;
    let config = connector
        .configure()?
        // The enrolled public-key pin is the trust anchor. The configurable
        // fronting SNI is intentionally not the certificate hostname.
        .verify_hostname(false);
    let tls = match timeout(CONNECT_TIMEOUT, tokio_boring::connect(config, sni, tcp)).await {
        Ok(Ok(stream)) => stream,
        Ok(Err(error)) => {
            if pin_state.checked.load(Ordering::SeqCst) && !pin_state.matched.load(Ordering::SeqCst)
            {
                return Err(TransportError::EndpointPinMismatch);
            }
            return Err(TransportError::TlsHandshake(error.to_string()));
        }
        Err(_) => return Err(TransportError::EndpointTimeout(endpoint)),
    };
    // The Cloudflare MASQUE TCP endpoint currently accepts the HTTP/2
    // connection preface but does not echo ALPN. Go's http2.Transport follows
    // the same behavior when DialTLSContext is supplied. Reject an explicitly
    // different protocol, while accepting `h2` or no selection.
    if let Some(protocol) = tls.ssl().selected_alpn_protocol()
        && protocol != b"h2"
    {
        return Err(TransportError::AlpnMismatch);
    }
    if let Some(attempt) = attempt {
        attempt.record(ConnectionEventType::TlsReady, TransportStage::TlsHandshake);
    }
    if protector.network_generation().unwrap_or_default() != expected_generation {
        return Err(TransportError::UnderlyingNetworkChanged);
    }
    let tls = LeasedIo::new(tls, egress_lease);

    let quality = attempt
        .map(ConnectionAttemptTelemetry::quality)
        .unwrap_or_default();
    let flow_control = H2FlowControlConfig::for_features(quality.features());
    let builder = connect_ip_h2_builder(flow_control);
    let (mut sender, mut connection) = builder.handshake(tls).await?;
    let ping_pong = connection.ping_pong();
    let ping_supported = ping_pong.is_some();
    if !ping_supported && !H2_PING_UNSUPPORTED_REPORTED.swap(true, Ordering::Relaxed) {
        tracing::warn!(
            code = "H2_PING_UNSUPPORTED",
            "the HTTP/2 build does not expose protocol PING observation"
        );
    }
    let task = AbortOnDropHandle::new(spawn_h2_driver(
        connection,
        ping_pong,
        quality.clone(),
        attempt.cloned(),
    ));
    sender = sender.ready().await?;
    if let Some(attempt) = attempt {
        attempt.record(
            ConnectionEventType::PeerSettingsReceived,
            TransportStage::PeerSettings,
        );
    }

    let request = connect_request()?;
    let (response, stream) = sender.send_request(request, false)?;
    let response = timeout(CONNECT_TIMEOUT, response)
        .await
        .map_err(|_| TransportError::ConnectTimeout)??;
    if response.status() != StatusCode::OK {
        return Err(TransportError::ConnectRejected(response.status()));
    }
    if let Some(attempt) = attempt {
        attempt.record(
            ConnectionEventType::MasqueAccepted,
            TransportStage::MasqueConnect,
        );
    }
    let receive = response.into_body();
    let mut tunnel = h2_tunnel_from_streams(
        stream,
        receive,
        task.detach(),
        quality,
        flow_control,
        ping_supported,
    );
    tunnel.attempt = attempt.cloned();
    Ok(tunnel)
}

static H2_PING_UNSUPPORTED_REPORTED: AtomicBool = AtomicBool::new(false);

fn connect_ip_h2_builder(config: H2FlowControlConfig) -> h2::client::Builder {
    let mut builder = h2::client::Builder::new();
    builder
        .initial_window_size(config.stream_receive_window)
        .initial_connection_window_size(config.connection_receive_window)
        .enable_push(false);
    builder
}

fn spawn_h2_driver<T>(
    connection: h2::client::Connection<T, Bytes>,
    ping_pong: Option<PingPong>,
    quality: NetworkQualityTelemetry,
    attempt: Option<ConnectionAttemptTelemetry>,
) -> JoinHandle<Result<(), h2::Error>>
where
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let ping_task = ping_pong.map(|ping_pong| {
            AbortOnDropHandle::new(tokio::spawn(run_h2_ping(ping_pong, quality, attempt)))
        });
        let result = connection.await;
        drop(ping_task);
        result
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct H2RttSample {
    latest: Duration,
    smoothed: Duration,
    minimum: Duration,
    variance: Duration,
}

#[derive(Debug, Default)]
struct H2RttEstimator {
    smoothed: Option<Duration>,
    minimum: Option<Duration>,
    variance: Option<Duration>,
}

impl H2RttEstimator {
    fn observe(&mut self, latest: Duration) -> H2RttSample {
        let previous_smoothed = self.smoothed.unwrap_or(latest);
        let smoothed = if self.smoothed.is_some() {
            duration_ewma(previous_smoothed, latest)
        } else {
            latest
        };
        let deviation = previous_smoothed.abs_diff(latest);
        let variance = self
            .variance
            .map_or(deviation, |previous| duration_ewma(previous, deviation));
        let minimum = self.minimum.map_or(latest, |minimum| minimum.min(latest));
        self.smoothed = Some(smoothed);
        self.minimum = Some(minimum);
        self.variance = Some(variance);
        H2RttSample {
            latest,
            smoothed,
            minimum,
            variance,
        }
    }

    fn timeout(&self) -> Duration {
        self.smoothed.map_or(H2_PING_DEFAULT_TIMEOUT, |smoothed| {
            smoothed
                .saturating_mul(3)
                .clamp(H2_PING_MIN_TIMEOUT, H2_PING_MAX_TIMEOUT)
        })
    }
}

fn duration_ewma(previous: Duration, sample: Duration) -> Duration {
    let nanos = previous
        .as_nanos()
        .saturating_mul(7)
        .saturating_add(sample.as_nanos())
        / 8;
    let seconds = u64::try_from(nanos / 1_000_000_000).unwrap_or(u64::MAX);
    let subsecond_nanos = u32::try_from(nanos % 1_000_000_000).unwrap_or(u32::MAX);
    Duration::new(seconds, subsecond_nanos)
}

struct H2PingTaskGuard(NetworkQualityTelemetry);

impl H2PingTaskGuard {
    fn new(quality: NetworkQualityTelemetry) -> Self {
        quality.h2_ping_task_started();
        Self(quality)
    }
}

impl Drop for H2PingTaskGuard {
    fn drop(&mut self) {
        self.0.h2_ping_task_finished();
    }
}

async fn run_h2_ping(
    mut ping_pong: PingPong,
    quality: NetworkQualityTelemetry,
    attempt: Option<ConnectionAttemptTelemetry>,
) {
    let _task = H2PingTaskGuard::new(quality.clone());
    let mut estimator = H2RttEstimator::default();
    let mut ticker = interval_at(Instant::now() + H2_PING_INTERVAL, H2_PING_INTERVAL);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        ticker.tick().await;
        let started = Instant::now();
        #[cfg(any(test, feature = "fault-injection"))]
        if quality
            .take_fault(crate::fault_injection::FaultPoint::H2Ping)
            .is_some()
        {
            let _ = h2_ping_with_timeout(
                std::future::pending::<Result<(), h2::Error>>(),
                estimator.timeout(),
            )
            .await;
            quality.record_h2_ping_timeout();
            continue;
        }
        match h2_ping_with_timeout(ping_pong.ping(Ping::opaque()), estimator.timeout()).await {
            Ok(Some(_)) => {
                let sample = estimator.observe(started.elapsed());
                if let Some(attempt) = &attempt {
                    attempt.observe_h2_rtt(
                        sample.latest,
                        sample.smoothed,
                        sample.minimum,
                        sample.variance,
                    );
                } else {
                    quality.observe_h2_rtt(
                        sample.latest,
                        sample.smoothed,
                        sample.minimum,
                        sample.variance,
                    );
                }
            }
            Err(_) => {
                quality.record_h2_ping_error();
                return;
            }
            Ok(None) => {
                quality.record_h2_ping_timeout();
                // h2 0.4 permits only one outstanding user PING. The timed-out
                // future has already sent it, so drain that eventual PONG before
                // another interval can send a new opaque PING.
                if std::future::poll_fn(|context| ping_pong.poll_pong(context))
                    .await
                    .is_err()
                {
                    return;
                }
            }
        }
    }
}

async fn h2_ping_with_timeout<F, T, E>(future: F, limit: Duration) -> Result<Option<T>, E>
where
    F: Future<Output = Result<T, E>>,
{
    match timeout(limit, future).await {
        Ok(result) => result.map(Some),
        Err(_) => Ok(None),
    }
}

fn h2_tunnel_from_streams(
    send: SendStream<Bytes>,
    receive: RecvStream,
    connection: JoinHandle<Result<(), h2::Error>>,
    quality: NetworkQualityTelemetry,
    flow_control: H2FlowControlConfig,
    ping_supported: bool,
) -> H2Tunnel {
    let (control_tx, control_rx) = watch::channel(PeerNetworkState::default());
    let (outgoing_tx, outgoing_rx) = mpsc::channel(H2_OUTGOING_CAPACITY);
    let (rejection_tx, rejection_rx) = mpsc::channel(MAX_PENDING_CONTROL_CAPSULES);
    let rejection_bytes = Arc::new(Semaphore::new(MAX_PENDING_CONTROL_BYTES));
    let writer = AbortOnDropHandle::new(tokio::spawn(run_h2_writer(
        send,
        outgoing_rx,
        rejection_rx,
        quality.clone(),
    )));
    H2Tunnel {
        send: H2SendHalf {
            sender: Some(outgoing_tx),
            _writer: writer,
        },
        receive: H2ReceiveHalf {
            stream: receive,
            control: ConnectIpControlPlane::new(control_tx),
            packets: VecDeque::new(),
            rejections: rejection_tx,
            rejection_bytes,
        },
        driver: H2Driver {
            task: Some(connection),
        },
        control: control_rx,
        flow_control,
        ping_supported,
        quality,
        attempt: None,
    }
}

async fn run_h2_writer(
    mut stream: SendStream<Bytes>,
    mut outgoing: mpsc::Receiver<H2Outgoing>,
    mut rejections: mpsc::Receiver<H2Rejection>,
    quality: NetworkQualityTelemetry,
) -> Result<(), TransportError> {
    let mut rejections_open = true;
    loop {
        tokio::select! {
            biased;
            rejection = rejections.recv(), if rejections_open => {
                match rejection {
                    Some(H2Rejection { bytes, .. }) => {
                        write_h2_data(&mut stream, bytes, &quality).await?
                    }
                    None => rejections_open = false,
                }
            }
            item = outgoing.recv() => {
                match item {
                    Some(H2Outgoing { bytes, accepted_bytes, completion }) => {
                        let result = write_h2_data(&mut stream, bytes, &quality)
                            .await
                            .map(|()| accepted_bytes);
                        let _ = completion.send(result);
                    }
                    None => {
                        let _ = stream.send_data(Bytes::new(), true);
                        return Ok(());
                    }
                }
            }
        }
    }
}

async fn write_h2_data(
    stream: &mut SendStream<Bytes>,
    mut encoded: Bytes,
    quality: &NetworkQualityTelemetry,
) -> Result<(), TransportError> {
    while !encoded.is_empty() {
        stream.reserve_capacity(encoded.len());
        let wait = H2CapacityWait::new(quality);
        #[cfg(any(test, feature = "fault-injection"))]
        if let Some(crate::FaultKind::H2CapacityWait(delay)) =
            quality.take_fault(crate::fault_injection::FaultPoint::H2Capacity)
        {
            tokio::time::sleep(delay).await;
        }
        let capacity = match std::future::poll_fn(|context| stream.poll_capacity(context)).await {
            Some(Ok(capacity)) if capacity > 0 => {
                wait.succeeded();
                capacity
            }
            Some(Ok(_)) | None => {
                wait.failed();
                return Err(TransportError::TunnelClosed);
            }
            Some(Err(error)) => {
                wait.failed();
                return Err(TransportError::Http2(error));
            }
        };
        let length = capacity.min(encoded.len());
        stream.send_data(encoded.split_to(length), false)?;
    }
    Ok(())
}

struct H2CapacityWait<'a> {
    quality: &'a NetworkQualityTelemetry,
    started: Instant,
    resolved: bool,
}

impl<'a> H2CapacityWait<'a> {
    fn new(quality: &'a NetworkQualityTelemetry) -> Self {
        Self {
            quality,
            started: Instant::now(),
            resolved: false,
        }
    }

    fn succeeded(mut self) {
        let duration = self.started.elapsed();
        if duration > H2_CAPACITY_STALL_THRESHOLD {
            self.quality.record_h2_capacity_stall(duration);
        }
        self.resolved = true;
    }

    fn failed(mut self) {
        self.quality.record_h2_capacity_wait_error();
        self.resolved = true;
    }
}

impl Drop for H2CapacityWait<'_> {
    fn drop(&mut self) {
        if !self.resolved {
            self.quality.record_h2_capacity_wait_cancelled();
        }
    }
}

#[cfg(test)]
fn encode_datagram_capsule(packet: &[u8]) -> Result<Bytes, TransportError> {
    let mut encoded = BytesMut::with_capacity(packet.len() + 16);
    encode_datagram_capsule_into(packet, &mut encoded)?;
    Ok(encoded.freeze())
}

fn encode_datagram_batch(batch: &PacketBatch) -> Result<(Bytes, usize), TransportError> {
    let accepted_bytes = batch.bytes();
    let mut encoded =
        BytesMut::with_capacity(accepted_bytes.saturating_add(batch.len().saturating_mul(16)));
    for packet in batch.iter() {
        validate_ip_packet(packet)?;
        encode_datagram_capsule_into(packet, &mut encoded)?;
    }
    Ok((encoded.freeze(), accepted_bytes))
}

fn encode_datagram_capsule_into(
    packet: &[u8],
    encoded: &mut BytesMut,
) -> Result<(), TransportError> {
    encode_varint(DATAGRAM_CAPSULE_TYPE, encoded)?;
    encode_varint(packet.len() as u64, encoded)?;
    encoded.extend_from_slice(packet);
    Ok(())
}

fn connect_request() -> Result<Request<()>, http::Error> {
    Request::builder()
        .method(Method::CONNECT)
        .version(Version::HTTP_2)
        .uri(CONNECT_URI)
        .header("user-agent", "")
        .header("cf-connect-proto", "cf-connect-ip")
        .header("pq-enabled", "false")
        .body(())
}

pub(crate) struct PinState {
    checked: AtomicBool,
    matched: AtomicBool,
}

impl PinState {
    pub(crate) fn rejected(&self) -> bool {
        self.checked.load(Ordering::SeqCst) && !self.matched.load(Ordering::SeqCst)
    }
}

fn tls_connector(
    identity: &MasqueTlsIdentity,
) -> Result<(SslConnector, Arc<PinState>), TransportError> {
    let mut builder = SslConnector::builder(SslMethod::tls())?;
    let pin_state = configure_client_identity_and_pin(&mut builder, identity)?;
    builder.set_alpn_protos(H2_ALPN)?;

    Ok((builder.build(), pin_state))
}

pub(crate) fn configure_client_identity_and_pin(
    builder: &mut SslContextBuilder,
    identity: &MasqueTlsIdentity,
) -> Result<Arc<PinState>, TransportError> {
    // `p256` emits the compact SEC1 form used by the Go oracle. BoringSSL's
    // `d2i_ECPrivateKey` cannot infer a curve when optional SEC1 parameters
    // are absent, so normalize it to PKCS#8 before import.
    let secret_key = SecretKey::from_sec1_der(&identity.private_key_sec1_der)
        .map_err(|_| TransportError::InvalidPrivateKey)?;
    let private_key_pkcs8 = secret_key
        .to_pkcs8_der()
        .map_err(|_| TransportError::InvalidPrivateKey)?;
    let private_key = PKey::private_key_from_der(private_key_pkcs8.as_bytes())
        .map_err(|_| TransportError::InvalidPrivateKey)?;
    let certificate = self_signed_certificate(&private_key)?;

    builder.set_certificate(&certificate)?;
    builder.set_private_key(&private_key)?;
    builder.check_private_key()?;

    let endpoint_pin = identity.endpoint_pin.clone();
    let pin_state = Arc::new(PinState {
        checked: AtomicBool::new(false),
        matched: AtomicBool::new(false),
    });
    let callback_state = Arc::clone(&pin_state);
    builder.set_custom_verify_callback(SslVerifyMode::PEER, move |ssl| {
        callback_state.checked.store(true, Ordering::SeqCst);
        let matched = ssl
            .peer_certificate()
            .and_then(|certificate| certificate.public_key().ok())
            .and_then(|public_key| public_key.public_key_to_der().ok())
            .is_some_and(|spki| endpoint_pin.verify_peer_spki(&spki).is_ok());
        callback_state.matched.store(matched, Ordering::SeqCst);
        if matched {
            Ok(())
        } else {
            Err(SslVerifyError::Invalid(SslAlert::BAD_CERTIFICATE))
        }
    });

    Ok(pin_state)
}

fn self_signed_certificate(private_key: &PKey<Private>) -> Result<X509, ErrorStack> {
    let mut certificate = X509::builder()?;
    certificate.set_version(2)?;
    let serial: Asn1Integer = BigNum::from_u32(0)?.to_asn1_integer()?;
    certificate.set_serial_number(&serial)?;
    let name = X509NameBuilder::new()?.build();
    certificate.set_subject_name(&name)?;
    certificate.set_issuer_name(&name)?;
    certificate.set_pubkey(private_key)?;
    let not_before = Asn1Time::days_from_now(0)?;
    let not_after = Asn1Time::days_from_now(1)?;
    certificate.set_not_before(&not_before)?;
    certificate.set_not_after(&not_after)?;
    certificate.sign(private_key, MessageDigest::sha256())?;
    Ok(certificate.build())
}

fn take_complete_capsule(
    buffer: &mut BytesMut,
) -> Result<Option<ConnectIpCapsule>, TransportError> {
    let Some((_, type_length)) = decode_varint(buffer)? else {
        return Ok(None);
    };
    let Some((payload_length, length_length)) = decode_varint(&buffer[type_length..])? else {
        return Ok(None);
    };
    let payload_length =
        usize::try_from(payload_length).map_err(|_| TransportError::CapsuleTooLarge)?;
    if payload_length > MAX_CAPSULE_PAYLOAD {
        return Err(TransportError::CapsuleTooLarge);
    }
    let frame_length = type_length
        .checked_add(length_length)
        .and_then(|header_length| header_length.checked_add(payload_length))
        .ok_or(TransportError::CapsuleTooLarge)?;
    if buffer.len() < frame_length {
        return Ok(None);
    }

    // A complete malformed capsule is terminal for this H2 tunnel. Splitting
    // only after framing is complete preserves fragmented-input semantics while
    // allowing successful DATAGRAM payloads to remain zero-copy `Bytes` views.
    let mut frame = buffer.split_to(frame_length).freeze();
    let capsule = ConnectIpCapsule::decode(&mut frame)?;
    debug_assert!(frame.is_empty());
    Ok(Some(capsule))
}

#[cfg(test)]
fn decode_capsule(buffer: &[u8]) -> Result<Option<(u64, Bytes, usize)>, TransportError> {
    let Some((capsule_type, type_length)) = decode_varint(buffer)? else {
        return Ok(None);
    };
    let Some((payload_length, length_length)) = decode_varint(&buffer[type_length..])? else {
        return Ok(None);
    };
    let payload_length =
        usize::try_from(payload_length).map_err(|_| TransportError::CapsuleTooLarge)?;
    if payload_length > MAX_CAPSULE_BYTES {
        return Err(TransportError::CapsuleTooLarge);
    }
    let header_length = type_length + length_length;
    let total_length = header_length
        .checked_add(payload_length)
        .ok_or(TransportError::CapsuleTooLarge)?;
    if buffer.len() < total_length {
        return Ok(None);
    }
    Ok(Some((
        capsule_type,
        Bytes::copy_from_slice(&buffer[header_length..total_length]),
        total_length,
    )))
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

fn encode_varint(value: u64, target: &mut BytesMut) -> Result<(), TransportError> {
    let length = match value {
        0..=63 => 1,
        64..=16_383 => 2,
        16_384..=1_073_741_823 => 4,
        1_073_741_824..=4_611_686_018_427_387_903 => 8,
        _ => return Err(TransportError::InvalidVarint),
    };
    let mut encoded = value;
    let prefix = match length {
        1 => 0b00,
        2 => 0b01,
        4 => 0b10,
        8 => 0b11,
        _ => unreachable!(),
    };
    let mut bytes = [0u8; 8];
    for index in (0..length).rev() {
        bytes[index] = encoded as u8;
        encoded >>= 8;
    }
    bytes[0] |= prefix << 6;
    target.extend_from_slice(&bytes[..length]);
    Ok(())
}

pub(crate) fn validate_ip_packet(packet: &[u8]) -> Result<(), TransportError> {
    let Some(first) = packet.first() else {
        return Err(TransportError::MalformedIpPacket);
    };
    if packet.len() > MAX_CAPSULE_BYTES {
        return Err(TransportError::MalformedIpPacket);
    }
    match first >> 4 {
        4 => {
            if packet.len() < 20 {
                return Err(TransportError::MalformedIpPacket);
            }
            let header_length = usize::from(first & 0x0f) * 4;
            let total_length = usize::from(u16::from_be_bytes([packet[2], packet[3]]));
            if header_length < 20
                || header_length > packet.len()
                || total_length < header_length
                || total_length != packet.len()
            {
                return Err(TransportError::MalformedIpPacket);
            }
            Ok(())
        }
        6 => {
            if packet.len() < 40 {
                return Err(TransportError::MalformedIpPacket);
            }
            let payload_length = usize::from(u16::from_be_bytes([packet[4], packet[5]]));
            if 40_usize.saturating_add(payload_length) != packet.len() {
                return Err(TransportError::MalformedIpPacket);
            }
            Ok(())
        }
        _ => Err(TransportError::MalformedIpPacket),
    }
}

#[derive(Debug, Error)]
pub enum TransportError {
    #[error("the secure identity records are incomplete or invalid")]
    InvalidIdentity,
    #[error("the enrolled MASQUE private key is not valid P-256 SEC1 DER")]
    InvalidPrivateKey,
    #[error("the enrolled MASQUE endpoint pin is not valid P-256 SPKI DER")]
    InvalidEndpointPin,
    #[error("the MASQUE endpoint {0} did not respond before the connection deadline")]
    EndpointTimeout(SocketAddr),
    #[error("the selected physical network has no {0:?} endpoint route")]
    EndpointFamilyUnavailable(usque_core::AddressFamily),
    #[error("the selected physical network changed while connecting")]
    UnderlyingNetworkChanged,
    #[error("the endpoint certificate public key does not match the enrolled pin")]
    EndpointPinMismatch,
    #[error("authenticated endpoint-pin refresh failed: {0}")]
    EndpointPinRefresh(String),
    #[error(
        "the authenticated enrollment changed the assigned tunnel addresses; restart the platform tunnel before retrying"
    )]
    EndpointAssignmentChanged,
    #[error("the TLS handshake failed: {0}")]
    TlsHandshake(String),
    #[error("the platform refused to protect an endpoint socket: {0}")]
    SocketProtection(String),
    #[error("the endpoint did not negotiate HTTP/2")]
    AlpnMismatch,
    #[error("the CONNECT-IP request timed out")]
    ConnectTimeout,
    #[error("the CONNECT-IP endpoint rejected the request with HTTP {0}")]
    ConnectRejected(StatusCode),
    #[error("HTTP/3 failed: {0}")]
    Http3(String),
    #[error("the HTTP/3 peer closed the connection with PROTOCOL_VIOLATION: {0}")]
    Http3ProtocolViolation(String),
    #[error("the HTTP/3 endpoint rejected CONNECT-IP with status {0}")]
    Http3ConnectRejected(u16),
    #[error("the HTTP/3 peer did not enable datagrams")]
    Http3DatagramUnavailable,
    #[error(
        "an IP packet is too large for the negotiated HTTP/3 datagram (maximum {maximum_packet_size} bytes)"
    )]
    Http3DatagramTooLarge { maximum_packet_size: usize },
    #[error("pmtu_revalidation_exhausted")]
    PmtuRevalidationExhausted,
    #[error("the QUIC path can carry only {0} bytes and violates the IPv6 minimum tunnel MTU")]
    Ipv6MinimumMtuUnavailable(usize),
    #[error("the operating mode is not supported by this proxy data plane")]
    UnsupportedOperatingMode,
    #[error("all configured MASQUE endpoints failed: {0}")]
    AllEndpointsFailed(String),
    #[error("both HTTP/3 and HTTP/2 connection attempts failed")]
    AllTransportsFailed {
        h3: Box<TransportFailure>,
        h2: Box<TransportFailure>,
    },
    #[error("the userspace network stack failed: {0}")]
    Netstack(String),
    #[error("SOCKS5 listener {address} failed: {source}")]
    SocksListener {
        address: SocketAddr,
        source: std::io::Error,
    },
    #[error("SOCKS5 failed: {0}")]
    Socks5(String),
    #[error("HTTP proxy listener {address} failed: {source}")]
    HttpProxyListener {
        address: SocketAddr,
        source: std::io::Error,
    },
    #[error("HTTP proxy failed: {0}")]
    HttpProxy(String),
    #[error("tunnel DNS failed: {0}")]
    Dns(String),
    #[error("the CONNECT-IP tunnel closed")]
    TunnelClosed,
    #[error("the bounded tunnel send queue is full")]
    SendQueueFull,
    #[error("the tunnel packet send operation timed out")]
    SendTimeout,
    #[error("the HTTP/2 driver stopped: {0}")]
    Driver(String),
    #[error("a received HTTP capsule exceeded the safety limit")]
    CapsuleTooLarge,
    #[error("a QUIC variable-length integer was out of range")]
    InvalidVarint,
    #[error("a CONNECT-IP datagram did not contain a valid IP packet")]
    MalformedIpPacket,
    #[error("CONNECT-IP wire protocol failed: {0}")]
    Protocol(#[from] usque_protocol::ProtocolError),
    #[error("TCP I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("TLS setup failed: {0}")]
    Tls(#[from] ErrorStack),
    #[error("HTTP/2 failed: {0}")]
    Http2(#[from] h2::Error),
    #[error("the CONNECT-IP request was invalid: {0}")]
    Http(#[from] http::Error),
}

impl TransportError {
    /// Converts internal transport errors to the stable, export-safe failure
    /// contract used for retry/fallback decisions and diagnostics.
    pub fn failure(
        &self,
        transport: Option<Transport>,
        family: Option<AddressFamily>,
    ) -> TransportFailure {
        use TransportFailureCode as Code;
        use TransportStage as Stage;

        let (code, stage) = match self {
            Self::InvalidIdentity | Self::InvalidPrivateKey | Self::InvalidEndpointPin => {
                (Code::IdentityInvalid, Stage::TunnelStartup)
            }
            Self::EndpointTimeout(_) => match transport {
                Some(Transport::Http3) => (Code::H3HandshakeTimeout, Stage::QuicHandshake),
                _ => (Code::H2TcpConnectFailed, Stage::SocketConnect),
            },
            Self::EndpointFamilyUnavailable(AddressFamily::Ipv4) => {
                (Code::PhysicalIpv4Unavailable, Stage::EndpointResolution)
            }
            Self::EndpointFamilyUnavailable(AddressFamily::Ipv6) => {
                (Code::PhysicalIpv6Unavailable, Stage::EndpointResolution)
            }
            Self::UnderlyingNetworkChanged => (Code::PhysicalNetworkChanged, Stage::SocketConnect),
            Self::EndpointPinMismatch | Self::EndpointPinRefresh(_) => {
                (Code::EndpointPinMismatch, Stage::TlsHandshake)
            }
            Self::EndpointAssignmentChanged => {
                (Code::AddressAssignmentInvalid, Stage::AddressAssignment)
            }
            Self::TlsHandshake(_) | Self::AlpnMismatch | Self::Tls(_) => match transport {
                Some(Transport::Http3) => (Code::H3ProtocolError, Stage::QuicHandshake),
                _ => (Code::H2TlsFailed, Stage::TlsHandshake),
            },
            Self::SocketProtection(_) => (Code::SocketProtectionFailed, Stage::SocketProtection),
            Self::ConnectTimeout => match transport {
                Some(Transport::Http3) => (Code::H3HandshakeTimeout, Stage::MasqueConnect),
                _ => (Code::H2ConnectRejected, Stage::MasqueConnect),
            },
            Self::ConnectRejected(status) if matches!(status.as_u16(), 401 | 403) => {
                (Code::AuthenticationFailed, Stage::MasqueConnect)
            }
            Self::ConnectRejected(_) => (Code::H2ConnectRejected, Stage::MasqueConnect),
            Self::Http3(_) | Self::Http3ProtocolViolation(_) => {
                (Code::H3ProtocolError, Stage::QuicHandshake)
            }
            Self::Http3ConnectRejected(401 | 403) => {
                (Code::AuthenticationFailed, Stage::MasqueConnect)
            }
            Self::Http3ConnectRejected(_) => (Code::H3ProtocolError, Stage::MasqueConnect),
            Self::Http3DatagramUnavailable => (Code::H3DatagramUnavailable, Stage::PeerSettings),
            Self::PmtuRevalidationExhausted => (Code::PmtuRevalidationExhausted, Stage::PacketSend),
            Self::Http3DatagramTooLarge { .. }
            | Self::Ipv6MinimumMtuUnavailable(_)
            | Self::MalformedIpPacket => (Code::PacketSendFailed, Stage::PacketSend),
            Self::UnsupportedOperatingMode => (Code::ConfigurationInvalid, Stage::TunnelStartup),
            Self::AllEndpointsFailed(_) => match transport {
                Some(Transport::Http3) => (Code::H3UdpUnreachable, Stage::SocketConnect),
                _ => (Code::H2TcpConnectFailed, Stage::SocketConnect),
            },
            Self::AllTransportsFailed { .. } => (Code::AllTransportsFailed, Stage::TunnelStartup),
            Self::Netstack(_) => (Code::Internal, Stage::TunnelStartup),
            Self::SocksListener { .. } | Self::HttpProxyListener { .. } => {
                (Code::ProxyPortInUse, Stage::TunnelStartup)
            }
            Self::Socks5(_) | Self::HttpProxy(_) => (Code::Internal, Stage::TunnelStartup),
            Self::Dns(_) => (Code::PhysicalDnsUnavailable, Stage::EndpointResolution),
            Self::TunnelClosed => match transport {
                Some(Transport::Http3) => (Code::H3ConnectionClosed, Stage::PacketReceive),
                _ => (Code::H2StreamClosed, Stage::PacketReceive),
            },
            Self::SendQueueFull => (Code::SendQueueFull, Stage::PacketSend),
            Self::SendTimeout => (Code::PacketSendTimeout, Stage::PacketSend),
            Self::Driver(_) | Self::Http2(_) => (Code::H2StreamClosed, Stage::PacketReceive),
            Self::CapsuleTooLarge | Self::InvalidVarint | Self::Protocol(_) | Self::Http(_) => {
                (Code::ConnectIpRejected, Stage::PeerSettings)
            }
            Self::Io(_) => match transport {
                Some(Transport::Http3) => (Code::H3UdpUnreachable, Stage::SocketConnect),
                _ => (Code::H2TcpConnectFailed, Stage::SocketConnect),
            },
        };

        let failure = TransportFailure::new(code, stage);
        match (transport, family) {
            (Some(transport), Some(family)) => failure.on_path(transport, family),
            _ => failure,
        }
    }

    pub fn exhausted_transport_failures(&self) -> Option<(&TransportFailure, &TransportFailure)> {
        match self {
            Self::AllTransportsFailed { h3, h2 } => Some((h3, h2)),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network_quality::{MetricAvailability, NetworkQualitySampler};
    use crate::socket::{DirectEgressLease, SocketHandle};
    use std::sync::atomic::{AtomicU64, AtomicUsize};
    use usque_core::MasqueKeyPair;

    struct TargetLeaseProtector {
        endpoint: SocketAddr,
        generation: AtomicU64,
        advance_during_protect: bool,
        calls: AtomicUsize,
        lease_dropped: Arc<AtomicBool>,
    }

    struct TargetLeaseDrop(Arc<AtomicBool>);

    impl Drop for TargetLeaseDrop {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
        }
    }

    #[async_trait::async_trait]
    impl SocketProtector for TargetLeaseProtector {
        fn protect(&self, _socket: SocketHandle) -> Result<(), String> {
            panic!("H2 must request an exact target lease");
        }

        async fn protect_for_target(
            &self,
            _socket: SocketHandle,
            remote: SocketAddr,
            protocol: DirectProtocol,
        ) -> Result<DirectEgressLease, String> {
            assert_eq!(remote, self.endpoint);
            assert_eq!(protocol, DirectProtocol::Tcp);
            self.calls.fetch_add(1, Ordering::AcqRel);
            if self.advance_during_protect {
                self.generation.fetch_add(1, Ordering::AcqRel);
            }
            Ok(DirectEgressLease::hold(TargetLeaseDrop(Arc::clone(
                &self.lease_dropped,
            ))))
        }

        fn network_generation(&self) -> Option<u64> {
            Some(self.generation.load(Ordering::Acquire))
        }
    }

    fn loopback_identity() -> MasqueTlsIdentity {
        let identity_key = MasqueKeyPair::generate();
        let endpoint_key = MasqueKeyPair::generate();
        MasqueTlsIdentity::new(
            identity_key.private_sec1_der().unwrap(),
            &endpoint_key.public_spki_der().unwrap(),
            Ipv4Addr::new(172, 16, 0, 2),
            "2606:4700:110:8f13::2".parse().unwrap(),
        )
        .unwrap()
    }

    #[tokio::test]
    async fn h2_generation_race_releases_lease_before_any_connection() {
        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let endpoint = listener.local_addr().unwrap();
        let protector = TargetLeaseProtector {
            endpoint,
            generation: AtomicU64::new(7),
            advance_during_protect: true,
            calls: AtomicUsize::new(0),
            lease_dropped: Arc::new(AtomicBool::new(false)),
        };
        assert!(matches!(
            connect_h2_with_protector(
                endpoint,
                "localhost",
                &loopback_identity(),
                &protector,
                None
            )
            .await,
            Err(TransportError::UnderlyingNetworkChanged)
        ));
        assert_eq!(protector.calls.load(Ordering::Acquire), 1);
        assert!(protector.lease_dropped.load(Ordering::Acquire));
        assert!(
            timeout(Duration::from_millis(25), listener.accept())
                .await
                .is_err()
        );
    }

    #[test]
    fn oracle_sec1_identity_normalizes_for_boringssl() {
        let identity_key = MasqueKeyPair::generate();
        let endpoint_key = MasqueKeyPair::generate();
        let identity = MasqueTlsIdentity::new(
            identity_key.private_sec1_der().unwrap(),
            &endpoint_key.public_spki_der().unwrap(),
            Ipv4Addr::new(172, 16, 0, 2),
            "2606:4700:110:8f13::2".parse().unwrap(),
        )
        .unwrap();
        tls_connector(&identity).expect("SEC1 identity imports through PKCS#8 normalization");
    }

    #[test]
    fn h2_connect_request_matches_the_sanitized_go_oracle_fixture() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../../oracle/fixtures/h2-connect.json"))
                .expect("parse sanitized H2 oracle fixture");
        assert_eq!(fixture["schema_version"], 1);

        let request = connect_request().expect("build H2 CONNECT request");
        assert_eq!(
            request.method().as_str(),
            fixture["method"].as_str().expect("method")
        );
        assert_eq!(
            request.uri().to_string(),
            fixture["uri"].as_str().expect("URI")
        );
        assert_eq!(request.version(), Version::HTTP_2);
        assert_eq!(fixture["http_version"], "2");

        let headers = fixture["headers"].as_object().expect("headers");
        for (name, expected) in headers {
            assert_eq!(
                request
                    .headers()
                    .get(name)
                    .expect("oracle header")
                    .to_str()
                    .expect("ASCII header"),
                expected.as_str().expect("header string"),
                "{name}"
            );
        }
        assert_eq!(fixture["capsule_datagram_type"], DATAGRAM_CAPSULE_TYPE);
        assert_eq!(
            fixture["tls"]["client_certificate"],
            "self-signed-p256-from-enrolled-private-key"
        );
        assert_eq!(fixture["tls"]["trust"], "enrolled-endpoint-spki-pin");
        assert_eq!(fixture["tls"]["hostname_verification"], false);
    }

    #[test]
    fn aggregate_transport_failure_preserves_both_structured_causes() {
        let h3 = TransportFailure::new(
            TransportFailureCode::H3HandshakeTimeout,
            TransportStage::QuicHandshake,
        );
        let h2 = TransportFailure::new(
            TransportFailureCode::H2TlsFailed,
            TransportStage::TlsHandshake,
        );
        let error = TransportError::AllTransportsFailed {
            h3: Box::new(h3.clone()),
            h2: Box::new(h2.clone()),
        };

        let aggregate = error.failure(None, None);
        assert_eq!(aggregate.code, TransportFailureCode::AllTransportsFailed);
        assert!(!aggregate.fallback_allowed);
        let (recorded_h3, recorded_h2) = error
            .exhausted_transport_failures()
            .expect("aggregate failure keeps both transport causes");
        assert_eq!(recorded_h3, &h3);
        assert_eq!(recorded_h2, &h2);
    }

    #[test]
    fn pmtu_revalidation_exhaustion_preserves_its_reason_code() {
        let error = TransportError::PmtuRevalidationExhausted;
        assert_eq!(error.to_string(), "pmtu_revalidation_exhausted");
        assert_eq!(
            error
                .failure(Some(Transport::Http3), Some(AddressFamily::Ipv4))
                .code,
            TransportFailureCode::PmtuRevalidationExhausted
        );
    }

    #[test]
    fn capsule_codec_handles_fragmentation_and_coalescing() {
        let packet_v4 = [
            0x45, 0, 0, 20, 0, 0, 0, 0, 64, 17, 0, 0, 1, 1, 1, 1, 8, 8, 8, 8,
        ];
        let packet_v6 = {
            let mut packet = [0u8; 40];
            packet[0] = 0x60;
            packet
        };
        let mut encoded = BytesMut::new();
        encode_varint(0, &mut encoded).unwrap();
        encode_varint(packet_v4.len() as u64, &mut encoded).unwrap();
        encoded.extend_from_slice(&packet_v4);
        encode_varint(0, &mut encoded).unwrap();
        encode_varint(packet_v6.len() as u64, &mut encoded).unwrap();
        encoded.extend_from_slice(&packet_v6);

        assert_eq!(decode_capsule(&encoded[..1]).unwrap(), None);
        let (_, first, consumed) = decode_capsule(&encoded).unwrap().unwrap();
        assert_eq!(first.as_ref(), packet_v4);
        let (_, second, _) = decode_capsule(&encoded[consumed..]).unwrap().unwrap();
        assert_eq!(second.as_ref(), packet_v6);
    }

    #[test]
    fn h2_batch_preserves_packet_order_and_capsule_wire_format() {
        let mut ipv4 = vec![0u8; 20];
        ipv4[0] = 0x45;
        ipv4[2..4].copy_from_slice(&20_u16.to_be_bytes());
        ipv4[8] = 64;
        ipv4[9] = 17;
        let mut ipv6 = vec![0u8; 40];
        ipv6[0] = 0x60;
        ipv6[7] = 64;

        let mut batch = PacketBatch::new();
        batch.push_back(Bytes::from(ipv4.clone())).unwrap();
        batch.push_back(Bytes::from(ipv6.clone())).unwrap();
        let (encoded, accepted_bytes) = encode_datagram_batch(&batch).unwrap();
        assert_eq!(accepted_bytes, ipv4.len() + ipv6.len());

        let (first_type, first, consumed) = decode_capsule(&encoded).unwrap().unwrap();
        assert_eq!(first_type, DATAGRAM_CAPSULE_TYPE);
        assert_eq!(first.as_ref(), ipv4);
        let (second_type, second, final_consumed) =
            decode_capsule(&encoded[consumed..]).unwrap().unwrap();
        assert_eq!(second_type, DATAGRAM_CAPSULE_TYPE);
        assert_eq!(second.as_ref(), ipv6);
        assert_eq!(consumed + final_consumed, encoded.len());
    }

    #[test]
    fn streaming_capsule_take_is_transactional_until_a_frame_is_complete() {
        let first = ConnectIpCapsule::Unknown {
            capsule_type: 42,
            payload: Bytes::from_static(b"first"),
        }
        .encode()
        .unwrap();
        let second = ConnectIpCapsule::Unknown {
            capsule_type: 43,
            payload: Bytes::from_static(b"second"),
        }
        .encode()
        .unwrap();
        let split = first.len() - 1;
        let mut buffer = BytesMut::from(&first[..split]);
        let incomplete = buffer.clone();

        assert!(take_complete_capsule(&mut buffer).unwrap().is_none());
        assert_eq!(buffer, incomplete);

        buffer.extend_from_slice(&first[split..]);
        buffer.extend_from_slice(&second);
        assert!(matches!(
            take_complete_capsule(&mut buffer).unwrap(),
            Some(ConnectIpCapsule::Unknown { capsule_type: 42, payload })
                if payload == Bytes::from_static(b"first")
        ));
        assert!(matches!(
            take_complete_capsule(&mut buffer).unwrap(),
            Some(ConnectIpCapsule::Unknown { capsule_type: 43, payload })
                if payload == Bytes::from_static(b"second")
        ));
        assert!(buffer.is_empty());
    }

    #[test]
    fn complete_malformed_capsule_is_consumed_as_a_terminal_error() {
        let mut malformed = BytesMut::from(
            &[
                usque_protocol::ADDRESS_ASSIGN_CAPSULE_TYPE as u8,
                3,
                0,
                4,
                192,
            ][..],
        );
        assert!(matches!(
            take_complete_capsule(&mut malformed),
            Err(TransportError::Protocol(
                usque_protocol::ProtocolError::TruncatedCapsuleEntry
            ))
        ));
        assert!(malformed.is_empty());
    }

    #[test]
    fn varint_round_trips_boundaries() {
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
            let mut encoded = BytesMut::new();
            encode_varint(value, &mut encoded).unwrap();
            assert_eq!(
                decode_varint(&encoded).unwrap(),
                Some((value, encoded.len()))
            );
        }
    }

    #[test]
    fn rejects_oversized_capsules_before_allocation() {
        let mut encoded = BytesMut::new();
        encode_varint(0, &mut encoded).unwrap();
        encode_varint((MAX_CAPSULE_BYTES + 1) as u64, &mut encoded).unwrap();
        assert!(matches!(
            decode_capsule(&encoded),
            Err(TransportError::CapsuleTooLarge)
        ));
    }

    #[test]
    fn packet_validation_accepts_icmp_and_rejects_length_mismatch() {
        let mut ipv4_icmp = [0u8; 28];
        ipv4_icmp[0] = 0x45;
        ipv4_icmp[2..4].copy_from_slice(&28_u16.to_be_bytes());
        ipv4_icmp[8] = 64;
        ipv4_icmp[9] = 1;
        assert!(validate_ip_packet(&ipv4_icmp).is_ok());
        ipv4_icmp[3] -= 1;
        assert!(matches!(
            validate_ip_packet(&ipv4_icmp),
            Err(TransportError::MalformedIpPacket)
        ));

        let mut ipv6_icmp = [0u8; 48];
        ipv6_icmp[0] = 0x60;
        ipv6_icmp[4..6].copy_from_slice(&8u16.to_be_bytes());
        ipv6_icmp[6] = 58;
        ipv6_icmp[7] = 64;
        assert!(validate_ip_packet(&ipv6_icmp).is_ok());
    }

    fn ipv4_packet() -> [u8; 20] {
        [
            0x45, 0, 0, 20, 0, 0, 0, 0, 64, 17, 0, 0, 1, 1, 1, 1, 8, 8, 8, 8,
        ]
    }

    fn sized_ipv4_packet(length: usize) -> Bytes {
        assert!((20..=usize::from(u16::MAX)).contains(&length));
        let mut packet = vec![0_u8; length];
        packet[0] = 0x45;
        packet[2..4].copy_from_slice(&(length as u16).to_be_bytes());
        packet[8] = 64;
        packet[9] = 17;
        packet[12..16].copy_from_slice(&[1, 1, 1, 1]);
        packet[16..20].copy_from_slice(&[8, 8, 8, 8]);
        Bytes::from(packet)
    }

    fn ipv4_only_assignment() -> ConnectIpCapsule {
        use usque_protocol::{AddressAssign, IpPrefix};

        ConnectIpCapsule::AddressAssign(AddressAssign {
            addresses: vec![IpPrefix {
                request_id: 0,
                address: "172.16.0.2".parse().unwrap(),
                prefix_len: 32,
            }],
        })
    }

    struct H2Loopback {
        send: H2SendHalf,
        receive: H2ReceiveHalf,
        control: watch::Receiver<PeerNetworkState>,
        peer_send: SendStream<Bytes>,
        peer_recv: RecvStream,
        quality: NetworkQualityTelemetry,
        _client_driver: H2Driver,
        _server: JoinHandle<Result<(), h2::Error>>,
    }

    async fn connect_h2_loopback() -> H2Loopback {
        connect_h2_loopback_with_config(H2FlowControlConfig::default()).await
    }

    async fn connect_h2_loopback_with_config(config: H2FlowControlConfig) -> H2Loopback {
        use http::Response;
        use tokio::sync::oneshot;

        let (client_io, server_io) = tokio::io::duplex(1024 * 1024);
        let (streams_tx, streams_rx) = oneshot::channel();
        let server = tokio::spawn(async move {
            let mut connection = h2::server::handshake(server_io)
                .await
                .expect("server handshake");
            let (request, mut respond) = connection
                .accept()
                .await
                .expect("accept stream")
                .expect("CONNECT request");
            let response = Response::builder()
                .status(StatusCode::OK)
                .body(())
                .expect("ok response");
            let send = respond.send_response(response, false).expect("send 200");
            let recv = request.into_body();
            let _ = streams_tx.send((send, recv));
            while connection.accept().await.is_some() {}
            Ok(())
        });

        let quality = NetworkQualityTelemetry::default();
        quality.begin_connection(Transport::Http2, AddressFamily::Ipv4);
        let builder = connect_ip_h2_builder(config);
        let (mut sender, mut connection) = builder
            .handshake(client_io)
            .await
            .expect("client handshake");
        let ping_pong = connection.ping_pong();
        let ping_supported = ping_pong.is_some();
        let driver = AbortOnDropHandle::new(spawn_h2_driver(
            connection,
            ping_pong,
            quality.clone(),
            None,
        ));
        sender = sender.ready().await.expect("client ready");
        let (response, send) = sender
            .send_request(connect_request().expect("CONNECT"), false)
            .expect("send CONNECT");
        let response = response.await.expect("CONNECT response");
        assert_eq!(response.status(), StatusCode::OK);
        let receive = response.into_body();
        let (peer_send, peer_recv) = streams_rx.await.expect("server streams");
        let tunnel = h2_tunnel_from_streams(
            send,
            receive,
            driver.detach(),
            quality.clone(),
            config,
            ping_supported,
        );
        tunnel.activate_network_quality();
        let (send, receive, driver, control) = tunnel.into_parts();
        H2Loopback {
            send,
            receive,
            control,
            peer_send,
            peer_recv,
            quality,
            _client_driver: driver,
            _server: server,
        }
    }

    async fn peer_send_all(stream: &mut SendStream<Bytes>, mut encoded: Bytes) {
        while !encoded.is_empty() {
            stream.reserve_capacity(encoded.len());
            let capacity = std::future::poll_fn(|context| stream.poll_capacity(context))
                .await
                .expect("peer capacity")
                .expect("peer window");
            let length = capacity.min(encoded.len());
            stream
                .send_data(encoded.split_to(length), false)
                .expect("peer send");
        }
    }

    async fn peer_recv_capsule(stream: &mut RecvStream, buffer: &mut BytesMut) -> ConnectIpCapsule {
        loop {
            let mut cursor = buffer.clone().freeze();
            if let Some(capsule) = ConnectIpCapsule::decode_if_complete(&mut cursor).unwrap() {
                let consumed = buffer.len() - cursor.len();
                buffer.advance(consumed);
                return capsule;
            }
            let chunk = stream.data().await.expect("peer data").expect("peer frame");
            let length = chunk.len();
            buffer.extend_from_slice(&chunk);
            stream.flow_control().release_capacity(length).unwrap();
        }
    }

    async fn hold_peer_receive_capacity(stream: &mut RecvStream, target: usize) -> usize {
        let mut received = 0;
        while received < target {
            let chunk = stream
                .data()
                .await
                .expect("request body open")
                .expect("request data");
            received += chunk.len();
        }
        received
    }

    #[test]
    fn every_feature_combination_preserves_explicit_h2_window_rollback() {
        for bits in 0..32 {
            let features = crate::NetworkFeatureFlags {
                h2_tuned_flow_control: bits & 1 != 0,
                network_quality_metrics: bits & 2 != 0,
                udp_batch_io: bits & 4 != 0,
                automatic_pmtu: bits & 8 != 0,
                quic_migration: bits & 16 != 0,
            };
            let config = H2FlowControlConfig::for_features(features);
            assert_eq!(
                config.stream_receive_window,
                if features.h2_tuned_flow_control {
                    4 * 1024 * 1024
                } else {
                    65_535
                }
            );
            assert_eq!(
                config.connection_receive_window,
                if features.h2_tuned_flow_control {
                    8 * 1024 * 1024
                } else {
                    65_535
                }
            );
        }
    }

    #[test]
    fn h2_rtt_ewma_and_adaptive_timeout_are_bounded() {
        let mut estimator = H2RttEstimator::default();
        assert_eq!(estimator.timeout(), Duration::from_secs(5));

        let first = estimator.observe(Duration::from_millis(100));
        assert_eq!(first.smoothed, Duration::from_millis(100));
        assert_eq!(first.minimum, Duration::from_millis(100));
        assert_eq!(first.variance, Duration::ZERO);
        assert_eq!(estimator.timeout(), Duration::from_secs(2));

        let second = estimator.observe(Duration::from_millis(180));
        assert_eq!(second.smoothed, Duration::from_millis(110));
        assert_eq!(second.minimum, Duration::from_millis(100));
        assert_eq!(second.variance, Duration::from_millis(10));

        estimator.smoothed = Some(Duration::from_secs(1));
        assert_eq!(estimator.timeout(), Duration::from_secs(3));
        estimator.smoothed = Some(Duration::from_secs(4));
        assert_eq!(estimator.timeout(), Duration::from_secs(10));
    }

    #[tokio::test(start_paused = true)]
    async fn canonical_h2_ping_and_capacity_faults_are_observed_and_cancelled() {
        use crate::{FaultKind, FaultScript, ScheduledFault};
        let mut loopback = connect_h2_loopback_with_config(H2FlowControlConfig::for_features(
            crate::NetworkFeatureFlags {
                h2_tuned_flow_control: false,
                ..crate::PRODUCTION_NETWORK_FEATURES
            },
        ))
        .await;
        let quality = loopback.quality.clone();
        quality.inject_fault_script(
            FaultScript::new(
                12,
                vec![
                    ScheduledFault {
                        at: Duration::ZERO,
                        fault: FaultKind::H2PingTimeout,
                    },
                    ScheduledFault {
                        at: Duration::ZERO,
                        fault: FaultKind::H2CapacityWait(Duration::from_secs(10)),
                    },
                ],
            )
            .unwrap(),
        );
        tokio::task::yield_now().await;
        tokio::time::advance(H2_PING_INTERVAL).await;
        tokio::task::yield_now().await;
        tokio::time::advance(H2_PING_DEFAULT_TIMEOUT).await;
        tokio::task::yield_now().await;
        assert_eq!(
            NetworkQualitySampler::new(quality.clone())
                .sample()
                .h2_flow_control
                .ping_timeout_count,
            1
        );
        {
            let send = loopback.send.send_capsule(Bytes::from_static(b"test"));
            tokio::pin!(send);
            assert!(
                timeout(Duration::from_millis(100), &mut send)
                    .await
                    .is_err()
            );
        }
        drop(loopback);
        for _ in 0..8 {
            tokio::task::yield_now().await;
        }
        assert_eq!(quality.active_h2_ping_tasks(), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn h2_ping_timeout_uses_the_paused_tokio_clock() {
        let task = tokio::spawn(h2_ping_with_timeout(
            std::future::pending::<Result<(), ()>>(),
            H2_PING_DEFAULT_TIMEOUT,
        ));
        tokio::task::yield_now().await;
        tokio::time::advance(H2_PING_DEFAULT_TIMEOUT - Duration::from_millis(1)).await;
        assert!(!task.is_finished());
        tokio::time::advance(Duration::from_millis(1)).await;
        assert_eq!(task.await.expect("timeout task"), Ok(None));
    }

    #[tokio::test(start_paused = true)]
    async fn h2_ping_becomes_available_on_the_five_second_interval() {
        let loopback = connect_h2_loopback().await;
        let quality = loopback.quality.clone();
        tokio::task::yield_now().await;
        assert_eq!(quality.active_h2_ping_tasks(), 1);

        tokio::time::advance(H2_PING_INTERVAL).await;
        for _ in 0..8 {
            tokio::task::yield_now().await;
        }
        let snapshot = NetworkQualitySampler::new(quality.clone()).sample();
        assert_eq!(
            snapshot.rtt.smoothed.availability,
            MetricAvailability::Available
        );
        assert!(snapshot.rtt.smoothed.value.is_some());
        assert_eq!(
            snapshot.loss.interval_basis_points.availability,
            MetricAvailability::Unsupported
        );
        assert_eq!(
            snapshot.congestion.congestion_window_bytes.availability,
            MetricAvailability::Unsupported
        );
        assert_eq!(
            snapshot.pmtu.current_bytes.availability,
            MetricAvailability::Unsupported
        );

        loopback._client_driver.abort();
        for _ in 0..4 {
            tokio::task::yield_now().await;
        }
        assert_eq!(quality.active_h2_ping_tasks(), 0);
    }

    #[tokio::test]
    async fn h2_driver_connection_close_releases_the_ping_task() {
        let loopback = connect_h2_loopback().await;
        let H2Loopback {
            quality,
            _client_driver: driver,
            _server: server,
            ..
        } = loopback;
        tokio::task::yield_now().await;
        assert_eq!(quality.active_h2_ping_tasks(), 1);

        server.abort();
        let _ = timeout(Duration::from_secs(2), driver.wait())
            .await
            .expect("client driver notices the closed peer");
        for _ in 0..4 {
            tokio::task::yield_now().await;
        }
        assert_eq!(quality.active_h2_ping_tasks(), 0);
    }

    #[tokio::test]
    async fn h2_handshake_error_never_starts_a_ping_task() {
        let quality = NetworkQualityTelemetry::default();
        let (client_io, server_io) = tokio::io::duplex(64);
        drop(server_io);
        let builder = connect_ip_h2_builder(H2FlowControlConfig::default());
        assert!(builder.handshake::<_, Bytes>(client_io).await.is_err());
        assert_eq!(quality.active_h2_ping_tasks(), 0);
    }

    #[tokio::test]
    async fn h2_tls_error_occurs_before_any_ping_task_exists() {
        use tokio::io::AsyncWriteExt;

        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("bind loopback");
        let endpoint = listener.local_addr().expect("loopback address");
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept");
            stream.write_all(b"not tls").await.expect("write garbage");
            stream.shutdown().await.expect("close");
        });
        let identity_key = MasqueKeyPair::generate();
        let endpoint_key = MasqueKeyPair::generate();
        let identity = MasqueTlsIdentity::new(
            identity_key.private_sec1_der().unwrap(),
            &endpoint_key.public_spki_der().unwrap(),
            Ipv4Addr::new(172, 16, 0, 2),
            "2606:4700:110:8f13::2".parse().unwrap(),
        )
        .unwrap();
        let telemetry = crate::telemetry::ConnectionTelemetry::default();
        let attempt =
            ConnectionAttemptTelemetry::new(telemetry, Transport::Http2, AddressFamily::Ipv4);
        let quality = attempt.quality();

        let protector = TargetLeaseProtector {
            endpoint,
            generation: AtomicU64::new(7),
            advance_during_protect: false,
            calls: AtomicUsize::new(0),
            lease_dropped: Arc::new(AtomicBool::new(false)),
        };

        let result = timeout(
            Duration::from_secs(2),
            connect_h2_with_protector(endpoint, "localhost", &identity, &protector, Some(&attempt)),
        )
        .await
        .expect("TLS failure timed out");
        assert!(matches!(result, Err(TransportError::TlsHandshake(_))));
        server.await.expect("loopback server");
        assert_eq!(quality.active_h2_ping_tasks(), 0);
        assert_eq!(protector.calls.load(Ordering::Acquire), 1);
        assert!(protector.lease_dropped.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn h2_large_receive_stream_exceeds_sixteen_mib_without_truncation() {
        let mut loopback = connect_h2_loopback().await;
        let packet = sized_ipv4_packet(16 * 1024);
        let capsule = encode_datagram_capsule(&packet).expect("encode packet");
        let count = (16 * 1024 * 1024 / capsule.len()) + 2;
        let total = capsule.len() * count;
        assert!(total > 16 * 1024 * 1024);

        let peer_send = &mut loopback.peer_send;
        let receive = &mut loopback.receive;
        let ((), ()) = timeout(Duration::from_secs(20), async {
            tokio::join!(
                async {
                    for _ in 0..count {
                        peer_send_all(peer_send, capsule.clone()).await;
                    }
                },
                async {
                    for _ in 0..count {
                        let received = receive.receive_packet().await.expect("receive packet");
                        assert_eq!(received, packet);
                    }
                }
            )
        })
        .await
        .expect("large H2 body timed out");
    }

    #[tokio::test(start_paused = true)]
    async fn h2_delayed_window_update_records_capacity_stall() {
        let mut loopback = connect_h2_loopback().await;
        let payload = Bytes::from(vec![0x5a; 256 * 1024]);
        let payload_len = payload.len();
        let mut send = loopback.send;
        let send_task = tokio::spawn(async move { send.send_capsule(payload).await });

        let mut received = hold_peer_receive_capacity(&mut loopback.peer_recv, 65_535).await;
        tokio::time::advance(Duration::from_millis(2)).await;
        loopback
            .peer_recv
            .flow_control()
            .release_capacity(received)
            .expect("release delayed capacity");
        while received < payload_len {
            let chunk = loopback
                .peer_recv
                .data()
                .await
                .expect("request body open")
                .expect("request data");
            received += chunk.len();
            loopback
                .peer_recv
                .flow_control()
                .release_capacity(chunk.len())
                .expect("release capacity");
        }
        send_task.await.expect("writer task").expect("write body");

        let snapshot = NetworkQualitySampler::new(loopback.quality).sample();
        assert!(snapshot.h2_flow_control.capacity_stall_count >= 1);
        assert!(snapshot.h2_flow_control.capacity_stall_total > Duration::from_millis(1));
        assert!(snapshot.h2_flow_control.capacity_stall_max > Duration::from_millis(1));
    }

    #[tokio::test(start_paused = true)]
    async fn h2_cancelled_capacity_wait_is_counted_without_a_stall() {
        let mut loopback = connect_h2_loopback().await;
        let quality = loopback.quality.clone();
        let mut send = loopback.send;
        let send_task =
            tokio::spawn(
                async move { send.send_capsule(Bytes::from(vec![0x5a; 256 * 1024])).await },
            );
        hold_peer_receive_capacity(&mut loopback.peer_recv, 65_535).await;
        tokio::time::advance(Duration::from_millis(2)).await;
        send_task.abort();
        let _ = send_task.await;
        for _ in 0..4 {
            tokio::task::yield_now().await;
        }

        let snapshot = NetworkQualitySampler::new(quality).sample();
        assert_eq!(snapshot.h2_flow_control.capacity_stall_count, 0);
        assert!(snapshot.h2_flow_control.capacity_wait_cancelled >= 1);
    }

    #[tokio::test]
    async fn h2_capacity_error_is_separate_from_successful_stalls() {
        let mut loopback = connect_h2_loopback().await;
        let quality = loopback.quality.clone();
        let mut send = loopback.send;
        let send_task =
            tokio::spawn(
                async move { send.send_capsule(Bytes::from(vec![0x5a; 256 * 1024])).await },
            );
        hold_peer_receive_capacity(&mut loopback.peer_recv, 65_535).await;
        drop(loopback.peer_recv);
        loopback._server.abort();
        assert!(
            timeout(Duration::from_secs(2), send_task)
                .await
                .expect("capacity error timed out")
                .expect("writer task")
                .is_err()
        );

        let snapshot = NetworkQualitySampler::new(quality).sample();
        assert_eq!(snapshot.h2_flow_control.capacity_stall_count, 0);
        assert!(snapshot.h2_flow_control.capacity_wait_errors >= 1);
    }

    #[tokio::test]
    async fn h2_client_disables_server_push() {
        use http::Response;
        use tokio::sync::oneshot;

        let (client_io, server_io) = tokio::io::duplex(64 * 1024);
        let (push_tx, push_rx) = oneshot::channel();
        let server = tokio::spawn(async move {
            let mut connection = h2::server::handshake(server_io).await.expect("server");
            let (_, mut respond) = connection.accept().await.expect("accept").expect("request");
            let push = Request::builder()
                .method(Method::GET)
                .uri("https://cloudflareaccess.com/push")
                .body(())
                .expect("push request");
            let _ = push_tx.send(respond.push_request(push).is_err());
            respond
                .send_response(Response::new(()), true)
                .expect("response");
            while connection.accept().await.is_some() {}
            Ok::<(), h2::Error>(())
        });

        let builder = connect_ip_h2_builder(H2FlowControlConfig::default());
        let (mut sender, connection) = builder
            .handshake::<_, Bytes>(client_io)
            .await
            .expect("client");
        let driver = tokio::spawn(connection);
        sender = sender.ready().await.expect("ready");
        let (response, _) = sender
            .send_request(connect_request().expect("CONNECT"), true)
            .expect("request");
        assert!(push_rx.await.expect("push result"));
        assert!(response.await.is_ok());
        drop(sender);
        let _ = driver.await;
        let _ = server.await;
    }

    #[tokio::test]
    async fn h2_parsed_packet_queue_stops_at_1024_without_consuming_the_tail() {
        let mut loopback = connect_h2_loopback().await;
        let mut wire = BytesMut::new();
        let mut final_capsule = Bytes::new();
        for sequence in 0..=H2_PACKET_QUEUE_CAPACITY as u16 {
            let mut packet = ipv4_packet();
            packet[4..6].copy_from_slice(&sequence.to_be_bytes());
            let capsule = encode_datagram_capsule(&packet).unwrap();
            if usize::from(sequence) == H2_PACKET_QUEUE_CAPACITY {
                final_capsule = capsule.clone();
            }
            wire.extend_from_slice(&capsule);
        }
        loopback.receive.control.buffer.extend_from_slice(&wire);

        loopback.receive.drain_ready_capsules().unwrap();
        assert_eq!(loopback.receive.packets.len(), H2_PACKET_QUEUE_CAPACITY);
        assert_eq!(loopback.receive.control.buffer, final_capsule);
        assert_eq!(
            u16::from_be_bytes([
                loopback.receive.packets.front().unwrap()[4],
                loopback.receive.packets.front().unwrap()[5],
            ]),
            0
        );

        loopback.receive.packets.pop_front();
        loopback.receive.drain_ready_capsules().unwrap();
        assert_eq!(loopback.receive.packets.len(), H2_PACKET_QUEUE_CAPACITY);
        assert!(loopback.receive.control.buffer.is_empty());
        assert_eq!(
            u16::from_be_bytes([
                loopback.receive.packets.back().unwrap()[4],
                loopback.receive.packets.back().unwrap()[5],
            ]),
            H2_PACKET_QUEUE_CAPACITY as u16
        );
    }

    #[tokio::test]
    async fn h2_interleaved_datagram_and_address_assign_matches_h3_peer_state() {
        use crate::netstack::apply_peer_network_state;
        use usque_core::{AddressFamily, Transport};

        let mut loopback = connect_h2_loopback().await;
        let assignment = ipv4_only_assignment();
        let encoded_assign = assignment.encode().unwrap();
        let packet = ipv4_packet();
        let mut wire = BytesMut::new();
        wire.extend_from_slice(&encode_datagram_capsule(&packet).unwrap());
        wire.extend_from_slice(&encoded_assign);
        peer_send_all(&mut loopback.peer_send, wire.freeze()).await;

        let received = timeout(Duration::from_secs(2), loopback.receive.receive_packet())
            .await
            .expect("datagram timed out")
            .expect("datagram");
        assert_eq!(received.as_ref(), packet);

        let h2_state = loopback.control.borrow().clone();
        let (state_tx, state_rx) = watch::channel(PeerNetworkState::default());
        let mut plane = ConnectIpControlPlane::new(state_tx);
        plane.buffer.extend_from_slice(&encoded_assign);
        plane.drain().unwrap();
        assert_eq!(h2_state, plane.state);
        assert_eq!(h2_state, *state_rx.borrow());

        let path = apply_peer_network_state(
            crate::netstack::RuntimePath {
                transport: Transport::Http2,
                endpoint_family: AddressFamily::Ipv4,
                ipv4_available: true,
                ipv6_available: true,
            },
            &h2_state,
            Ipv4Addr::new(172, 16, 0, 2),
            "2606:4700:110::2".parse().unwrap(),
        );
        assert!(path.ipv4_available);
        assert!(!path.ipv6_available);
        let _ = loopback.send;
    }

    #[tokio::test]
    async fn h2_address_request_writes_unspecified_assign_on_request_stream() {
        use std::net::IpAddr;
        use usque_protocol::{AddressRequest, IpPrefix};

        let mut loopback = connect_h2_loopback().await;
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
        peer_send_all(&mut loopback.peer_send, request).await;

        let mut receive = loopback.receive;
        let mut peer_buffer = BytesMut::new();
        let rejection = {
            let receive_packet = receive.receive_packet();
            tokio::pin!(receive_packet);
            timeout(Duration::from_secs(2), async {
                tokio::select! {
                    result = &mut receive_packet => {
                        panic!("receive_packet should wait after ADDRESS_REQUEST, got {result:?}");
                    }
                    capsule = peer_recv_capsule(&mut loopback.peer_recv, &mut peer_buffer) => capsule,
                }
            })
            .await
            .expect("ADDRESS_ASSIGN rejection timed out")
        };
        let ConnectIpCapsule::AddressAssign(rejection) = rejection else {
            panic!("expected unspecified ADDRESS_ASSIGN, got {rejection:?}");
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
        assert!(!loopback.control.borrow().assignments_advertised);
        drop((loopback.send, receive));
    }

    #[tokio::test]
    async fn h2_unknown_capsule_does_not_desync_subsequent_datagram() {
        let mut loopback = connect_h2_loopback().await;
        let unknown = ConnectIpCapsule::Unknown {
            capsule_type: 42,
            payload: Bytes::from_static(b"future"),
        }
        .encode()
        .unwrap();
        let packet = ipv4_packet();
        let datagram = encode_datagram_capsule(&packet).unwrap();

        peer_send_all(&mut loopback.peer_send, unknown.slice(..3)).await;
        peer_send_all(&mut loopback.peer_send, unknown.slice(3..)).await;
        peer_send_all(&mut loopback.peer_send, datagram).await;

        let received = timeout(Duration::from_secs(2), loopback.receive.receive_packet())
            .await
            .expect("datagram after unknown timed out")
            .expect("datagram after unknown");
        assert_eq!(received.as_ref(), packet);
        assert_eq!(*loopback.control.borrow(), PeerNetworkState::default());
        let _ = loopback.send;
    }

    #[tokio::test]
    async fn h2_send_capsule_writes_framed_control_on_request_stream() {
        let mut loopback = connect_h2_loopback().await;
        let assignment = ipv4_only_assignment().encode().unwrap();
        timeout(
            Duration::from_secs(2),
            loopback.send.send_capsule(assignment.clone()),
        )
        .await
        .expect("send_capsule timed out")
        .expect("send_capsule");

        let mut peer_buffer = BytesMut::new();
        let received = timeout(
            Duration::from_secs(2),
            peer_recv_capsule(&mut loopback.peer_recv, &mut peer_buffer),
        )
        .await
        .expect("peer capsule timed out");
        assert_eq!(received.encode().unwrap(), assignment);
    }
}
