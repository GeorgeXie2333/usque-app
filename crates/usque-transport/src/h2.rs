use std::collections::VecDeque;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
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
use bytes::{Buf, Bytes, BytesMut};
use h2::{RecvStream, SendStream};
use http::{Method, Request, StatusCode, Version};
use p256::SecretKey;
use p256::pkcs8::EncodePrivateKey;
use thiserror::Error;
use tokio::net::TcpSocket;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::JoinHandle;
use tokio::time::timeout;
use tokio_util::task::AbortOnDropHandle;
use usque_core::{
    AddressFamily, EndpointPin, Transport, TransportFailure, TransportFailureCode, TransportStage,
};
use usque_protocol::{ConnectIpCapsule, MAX_CAPSULE_PAYLOAD, PeerNetworkState};
use zeroize::Zeroizing;

use crate::connect_ip_control::ConnectIpControlPlane;
use crate::socket::{SocketProtector, noop_socket_protector, socket_handle};
use crate::telemetry::{ConnectionAttemptTelemetry, ConnectionEventType};

const CONNECT_URI: &str = "https://cloudflareaccess.com/";
const H2_ALPN: &[u8] = b"\x02h2";
const DATAGRAM_CAPSULE_TYPE: u64 = 0;
const MAX_CAPSULE_BYTES: usize = 65_535;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(8);
const H2_OUTGOING_CAPACITY: usize = 1_024;
const PACKET_SEND_TIMEOUT: Duration = Duration::from_secs(10);

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
}

struct H2Outgoing {
    bytes: Bytes,
    completion: oneshot::Sender<Result<(), TransportError>>,
}

pub struct H2SendHalf {
    sender: Option<mpsc::Sender<H2Outgoing>>,
    _writer: AbortOnDropHandle<Result<(), TransportError>>,
}

impl H2SendHalf {
    /// Sends one raw IP packet as an HTTP Capsule DATAGRAM.
    pub async fn send_packet(&mut self, packet: &[u8]) -> Result<(), TransportError> {
        validate_ip_packet(packet)?;
        self.send_capsule(encode_datagram_capsule(packet)?).await
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

    /// Sends an owned packet under the transport supervisor's outer deadline.
    pub(crate) async fn send_owned_packet(&mut self, packet: Bytes) -> Result<(), TransportError> {
        validate_ip_packet(&packet)?;
        self.send_capsule_inner(encode_datagram_capsule(&packet)?)
            .await
    }

    async fn send_capsule_inner(&mut self, capsule: Bytes) -> Result<(), TransportError> {
        let (completion_tx, completion_rx) = oneshot::channel();
        self.sender
            .as_ref()
            .ok_or(TransportError::TunnelClosed)?
            .try_send(H2Outgoing {
                bytes: capsule,
                completion: completion_tx,
            })
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => TransportError::SendQueueFull,
                mpsc::error::TrySendError::Closed(_) => TransportError::TunnelClosed,
            })?;
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
    rejections: mpsc::UnboundedSender<Bytes>,
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

    fn drain_ready_capsules(&mut self) -> Result<(), TransportError> {
        loop {
            let mut cursor = self.control.buffer.clone().freeze();
            let Some(capsule) = ConnectIpCapsule::decode_if_complete(&mut cursor)? else {
                return Ok(());
            };
            let consumed = self.control.buffer.len() - cursor.len();
            self.control.buffer.advance(consumed);
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
            self.rejections
                .send(bytes)
                .map_err(|_| TransportError::TunnelClosed)?;
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
        self.task
            .take()
            .expect("H2 driver task is present until wait")
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
    let socket = if endpoint.is_ipv4() {
        TcpSocket::new_v4()
    } else {
        TcpSocket::new_v6()
    }?;
    protector
        .protect(socket_handle(&socket))
        .map_err(TransportError::SocketProtection)?;
    let tcp = timeout(CONNECT_TIMEOUT, socket.connect(endpoint))
        .await
        .map_err(|_| TransportError::EndpointTimeout(endpoint))??;
    tcp.set_nodelay(true)?;
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

    let (mut sender, connection) = h2::client::handshake(tls).await?;
    let task = tokio::spawn(connection);
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
    Ok(h2_tunnel_from_streams(stream, receive, task))
}

fn h2_tunnel_from_streams(
    send: SendStream<Bytes>,
    receive: RecvStream,
    connection: JoinHandle<Result<(), h2::Error>>,
) -> H2Tunnel {
    let (control_tx, control_rx) = watch::channel(PeerNetworkState::default());
    let (outgoing_tx, outgoing_rx) = mpsc::channel(H2_OUTGOING_CAPACITY);
    let (rejection_tx, rejection_rx) = mpsc::unbounded_channel();
    let writer =
        AbortOnDropHandle::new(tokio::spawn(run_h2_writer(send, outgoing_rx, rejection_rx)));
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
        },
        driver: H2Driver {
            task: Some(connection),
        },
        control: control_rx,
    }
}

async fn run_h2_writer(
    mut stream: SendStream<Bytes>,
    mut outgoing: mpsc::Receiver<H2Outgoing>,
    mut rejections: mpsc::UnboundedReceiver<Bytes>,
) -> Result<(), TransportError> {
    let mut rejections_open = true;
    loop {
        tokio::select! {
            biased;
            rejection = rejections.recv(), if rejections_open => {
                match rejection {
                    Some(bytes) => write_h2_data(&mut stream, bytes).await?,
                    None => rejections_open = false,
                }
            }
            item = outgoing.recv() => {
                match item {
                    Some(H2Outgoing { bytes, completion }) => {
                        let result = write_h2_data(&mut stream, bytes).await;
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
) -> Result<(), TransportError> {
    while !encoded.is_empty() {
        stream.reserve_capacity(encoded.len());
        let capacity = std::future::poll_fn(|context| stream.poll_capacity(context))
            .await
            .ok_or(TransportError::TunnelClosed)??;
        let length = capacity.min(encoded.len());
        if length == 0 {
            return Err(TransportError::TunnelClosed);
        }
        stream.send_data(encoded.split_to(length), false)?;
    }
    Ok(())
}

fn encode_datagram_capsule(packet: &[u8]) -> Result<Bytes, TransportError> {
    let mut encoded = BytesMut::with_capacity(packet.len() + 16);
    encode_varint(DATAGRAM_CAPSULE_TYPE, &mut encoded)?;
    encode_varint(packet.len() as u64, &mut encoded)?;
    encoded.extend_from_slice(packet);
    Ok(encoded.freeze())
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

#[cfg(test)]
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
    use usque_core::MasqueKeyPair;

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
        _client_driver: H2Driver,
        _server: JoinHandle<Result<(), h2::Error>>,
    }

    async fn connect_h2_loopback() -> H2Loopback {
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

        let (mut sender, connection) = h2::client::handshake(client_io)
            .await
            .expect("client handshake");
        let driver = tokio::spawn(connection);
        sender = sender.ready().await.expect("client ready");
        let (response, send) = sender
            .send_request(connect_request().expect("CONNECT"), false)
            .expect("send CONNECT");
        let response = response.await.expect("CONNECT response");
        assert_eq!(response.status(), StatusCode::OK);
        let receive = response.into_body();
        let (peer_send, peer_recv) = streams_rx.await.expect("server streams");
        let tunnel = h2_tunnel_from_streams(send, receive, driver);
        let (send, receive, driver, control) = tunnel.into_parts();
        H2Loopback {
            send,
            receive,
            control,
            peer_send,
            peer_recv,
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
