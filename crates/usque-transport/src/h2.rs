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
use tokio::task::JoinHandle;
use tokio::time::timeout;
use usque_core::EndpointPin;
use zeroize::Zeroizing;

use crate::socket::{SocketProtector, noop_socket_protector, socket_handle};

const CONNECT_URI: &str = "https://cloudflareaccess.com/";
const H2_ALPN: &[u8] = b"\x02h2";
const DATAGRAM_CAPSULE_TYPE: u64 = 0;
const MAX_CAPSULE_BYTES: usize = 65_535;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(8);

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
}

impl H2Tunnel {
    pub fn into_parts(self) -> (H2SendHalf, H2ReceiveHalf, H2Driver) {
        (self.send, self.receive, self.driver)
    }
}

pub struct H2SendHalf {
    stream: SendStream<Bytes>,
}

impl H2SendHalf {
    /// Sends one raw IP packet as an HTTP Capsule DATAGRAM.
    pub async fn send_packet(&mut self, packet: &[u8]) -> Result<(), TransportError> {
        validate_ip_packet(packet)?;
        let mut encoded = BytesMut::with_capacity(packet.len() + 16);
        encode_varint(DATAGRAM_CAPSULE_TYPE, &mut encoded)?;
        encode_varint(packet.len() as u64, &mut encoded)?;
        encoded.extend_from_slice(packet);
        let mut encoded = encoded.freeze();

        while !encoded.is_empty() {
            self.stream.reserve_capacity(encoded.len());
            let capacity = std::future::poll_fn(|context| self.stream.poll_capacity(context))
                .await
                .ok_or(TransportError::TunnelClosed)??;
            let length = capacity.min(encoded.len());
            if length == 0 {
                return Err(TransportError::TunnelClosed);
            }
            self.stream.send_data(encoded.split_to(length), false)?;
        }
        Ok(())
    }

    pub fn close(&mut self) {
        let _ = self.stream.send_data(Bytes::new(), true);
    }
}

pub struct H2ReceiveHalf {
    stream: RecvStream,
    buffer: BytesMut,
}

impl H2ReceiveHalf {
    /// Receives the next raw IP packet, transparently handling capsules split
    /// across or coalesced within HTTP/2 DATA frames.
    pub async fn receive_packet(&mut self) -> Result<Bytes, TransportError> {
        loop {
            if let Some((capsule_type, payload, consumed)) = decode_capsule(&self.buffer)? {
                self.buffer.advance(consumed);
                if capsule_type != DATAGRAM_CAPSULE_TYPE {
                    continue;
                }
                validate_ip_packet(&payload)?;
                return Ok(payload);
            }

            let chunk = self
                .stream
                .data()
                .await
                .ok_or(TransportError::TunnelClosed)??;
            if self.buffer.len().saturating_add(chunk.len()) > MAX_CAPSULE_BYTES + 16 {
                return Err(TransportError::CapsuleTooLarge);
            }
            let length = chunk.len();
            self.buffer.extend_from_slice(&chunk);
            self.stream.flow_control().release_capacity(length)?;
        }
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
    connect_h2_with_protector(endpoint, sni, identity, noop_socket_protector().as_ref()).await
}

pub(crate) async fn connect_h2_with_protector(
    endpoint: SocketAddr,
    sni: &str,
    identity: &MasqueTlsIdentity,
    protector: &dyn SocketProtector,
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

    let (mut sender, connection) = h2::client::handshake(tls).await?;
    let task = tokio::spawn(connection);
    sender = sender.ready().await?;

    let request = connect_request()?;
    let (response, stream) = sender.send_request(request, false)?;
    let response = timeout(CONNECT_TIMEOUT, response)
        .await
        .map_err(|_| TransportError::ConnectTimeout)??;
    if response.status() != StatusCode::OK {
        return Err(TransportError::ConnectRejected(response.status()));
    }
    let receive = response.into_body();

    Ok(H2Tunnel {
        send: H2SendHalf { stream },
        receive: H2ReceiveHalf {
            stream: receive,
            buffer: BytesMut::with_capacity(4096),
        },
        driver: H2Driver { task: Some(task) },
    })
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
    #[error("the QUIC path can carry only {0} bytes and violates the IPv6 minimum tunnel MTU")]
    Ipv6MinimumMtuUnavailable(usize),
    #[error("the operating mode is not supported by this proxy data plane")]
    UnsupportedOperatingMode,
    #[error("all configured MASQUE endpoints failed: {0}")]
    AllEndpointsFailed(String),
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
}
