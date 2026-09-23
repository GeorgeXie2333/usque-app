//! Private network clients never pass through a frontend's direct-rule policy.
//! A handle is bound to one runtime and cannot silently obtain another exit.
use crate::chain_raw::RawSocket;
use crate::dns::{QuerySocket as UdpSocket, Resolver};
use crate::netstack::{PacketStack, RuntimeHealth};
use crate::tcp::{DialError, FlowClass, StackDialer, TcpDialer, TcpStream, TcpTarget};
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper_util::rt::TokioIo;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use tokio::sync::{mpsc, watch};
use tokio::time::{Instant, timeout_at};
use tokio_util::sync::CancellationToken;
use tokio_util::task::AbortOnDropHandle;
use ts_netstack_smoltcp::netcore::Channel;
use usque_core::vpngate::{
    CONNECT_TIMEOUT, CatalogueHttp, DirectoryError, MAX_DIRECTORY_BYTES, RESPONSE_TIMEOUT,
    approved_url,
};

trait HttpsIo: tokio::io::AsyncRead + tokio::io::AsyncWrite + Send + Unpin {}
impl<T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Send + Unpin> HttpsIo for T {}
#[derive(Clone, Copy, PartialEq, Eq)]
enum HttpsPurpose {
    General,
    #[cfg(feature = "wireguard")]
    WarpRegistration,
}

#[derive(Clone)]
pub struct InternalNetwork {
    dialer: Arc<dyn TcpDialer>,
    resolver: Option<Resolver>,
    health: watch::Receiver<RuntimeHealth>,
    cancellation: CancellationToken,
    packet_channel: Option<(Channel, Ipv4Addr, Ipv6Addr)>,
}

pub(crate) struct InternalRequest<'a> {
    pub url: &'a str,
    pub method: http::Method,
    pub headers: Vec<(&'a str, &'a str)>,
    pub body: Bytes,
    pub limit: usize,
    pub ipv6: Option<bool>,
}

/// Stable, non-sensitive failures for private HTTPS callers. Never contains a
/// URL, request/response body, bearer token, endpoint address or TLS error text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InternalHttpError {
    Cancelled,
    Timeout,
    Dns,
    Connect,
    Tls,
    Protocol,
    SizeLimit,
    Encoding,
    HttpStatus(u16),
}
impl InternalHttpError {
    #[cfg(feature = "wireguard")]
    pub(crate) fn code(self) -> String {
        match self {
            Self::Cancelled => "cancelled".into(),
            Self::Timeout => "timeout".into(),
            Self::Dns => "dns".into(),
            Self::Connect => "connect".into(),
            Self::Tls => "tls".into(),
            Self::Protocol => "protocol".into(),
            Self::SizeLimit => "size_limit".into(),
            Self::Encoding => "encoding".into(),
            Self::HttpStatus(status) => format!("http_{status}"),
        }
    }
}
impl From<InternalHttpError> for DirectoryError {
    fn from(value: InternalHttpError) -> Self {
        match value {
            InternalHttpError::Cancelled => Self::Cancelled,
            InternalHttpError::Timeout => Self::Timeout,
            InternalHttpError::SizeLimit => Self::SizeLimit,
            InternalHttpError::Encoding => Self::InvalidDirectory,
            _ => Self::Request,
        }
    }
}

impl InternalNetwork {
    /// A failed or changing final exit must never expose the healthy underlay
    /// through the ordinary network accessor. Explicit bootstrap still uses
    /// the separate WARP handle.
    pub(crate) fn blocked(&self) -> Self {
        let mut network = self.clone();
        network.cancellation = self.cancellation.child_token();
        network.cancellation.cancel();
        network
    }
    /// Exit diagnostics bypass frontend direct rules and remain bound to this
    /// exact session. Failure never substitutes the directory's endpoint IP.
    pub async fn probe_exit(&self) -> Result<usque_core::ExitInfo, DirectoryError> {
        let cancel = self.cancellation.child_token();
        let probe = usque_core::exit_probe::probe_exit_with_retry(
            |family| match family {
                usque_core::AddressFamily::Ipv4 => {
                    self.probe_ip("https://api-ipv4.ip.sb/ip", false, &cancel)
                }
                usque_core::AddressFamily::Ipv6 => {
                    self.probe_ip("https://api-ipv6.ip.sb/ip", true, &cancel)
                }
            },
            |ip| self.probe_geo(Some(ip), &cancel),
        );
        tokio::select! {
            biased;
            _ = cancel.cancelled() => Err(DirectoryError::Cancelled),
            result = probe => result.map_err(|_| DirectoryError::Request),
        }
    }
    async fn probe_ip(
        &self,
        url: &str,
        ipv6: bool,
        cancel: &CancellationToken,
    ) -> Option<std::net::IpAddr> {
        let body = self.get_https(url, 128, cancel).await.ok()?;
        let ip: std::net::IpAddr = std::str::from_utf8(&body).ok()?.trim().parse().ok()?;
        (ip.is_ipv6() == ipv6 && !ip.is_unspecified() && !ip.is_loopback() && !ip.is_multicast())
            .then_some(ip)
    }
    async fn probe_geo(
        &self,
        ip: Option<std::net::IpAddr>,
        cancel: &CancellationToken,
    ) -> Option<usque_core::GeoLocation> {
        let ip = ip?;
        let body = self
            .get_https(&format!("https://api.ip.sb/geoip/{ip}"), 16 * 1024, cancel)
            .await
            .ok()?;
        let mut location: usque_core::GeoLocation = serde_json::from_slice(&body).ok()?;
        if location.ip != ip {
            return None;
        }
        location.flag_svg = None;
        for field in [
            &location.country_code,
            &location.country,
            &location.region,
            &location.city,
            &location.organization,
            &location.timezone,
        ] {
            if field
                .as_ref()
                .is_some_and(|s| s.len() > 256 || s.chars().any(char::is_control))
            {
                return None;
            }
        }
        if location
            .country_code
            .as_ref()
            .is_some_and(|c| c.len() != 2 || !c.bytes().all(|b| b.is_ascii_alphabetic()))
        {
            return None;
        }
        Some(location)
    }
    pub fn health_snapshot(&self) -> RuntimeHealth {
        self.health.borrow().clone()
    }
    pub(crate) fn for_stack(
        profile: &usque_core::Profile,
        stack: &PacketStack,
        ipv4: Ipv4Addr,
        ipv6: Ipv6Addr,
    ) -> Self {
        Self {
            dialer: Arc::new(StackDialer {
                channel: stack.channel.clone(),
                ipv4,
                ipv6,
            }),
            resolver: Some(
                Resolver::new(
                    stack.channel.clone(),
                    ipv4,
                    ipv6,
                    profile.dns_servers.clone(),
                    usque_core::ProxyDnsMode::Remote,
                    stack.protector.clone(),
                )
                .with_final_exit(profile.chain_enabled(), stack.cancellation.clone()),
            ),
            health: stack.subscribe_health(),
            cancellation: stack.cancellation.clone(),
            packet_channel: Some((stack.channel.clone(), ipv4, ipv6)),
        }
    }
    pub(crate) fn for_streams(
        dialer: Arc<dyn TcpDialer>,
        health: watch::Receiver<RuntimeHealth>,
        cancellation: CancellationToken,
    ) -> Self {
        // CONNECT names are resolved within the WARP L4 session.
        Self {
            dialer,
            resolver: None,
            health,
            cancellation,
            packet_channel: None,
        }
    }
    pub(crate) fn health(&self) -> watch::Receiver<RuntimeHealth> {
        self.health.clone()
    }
    pub(crate) fn with_resolver(mut self, resolver: Resolver) -> Self {
        self.resolver = Some(resolver);
        self
    }

    pub(crate) async fn resolve_endpoint(
        &self,
        endpoint: &usque_core::chain_exit::Endpoint,
        ipv6: Option<bool>,
        cancel: &CancellationToken,
    ) -> Result<SocketAddr, DialError> {
        if let Some(address) = endpoint.address() {
            return Ok(address);
        }
        let resolver = self.resolver.as_ref().ok_or(DialError::Closed)?;
        tokio::select! {
            biased;
            _ = cancel.cancelled() => Err(DialError::Cancelled),
            _ = self.cancellation.cancelled() => Err(DialError::Closed),
            result = tokio::time::timeout(CONNECT_TIMEOUT, resolver.resolve(&endpoint.host)) => {
                let addresses = result.map_err(|_| DialError::Timeout)?.map_err(|_| DialError::Closed)?;
                addresses.into_iter().find(|ip| ipv6.is_none_or(|v6| ip.is_ipv6() == v6) && !ip.is_unspecified() && !ip.is_multicast() && !ip.is_loopback())
                    .map(|ip| SocketAddr::new(ip, endpoint.port)).ok_or(DialError::Closed)
            }
        }
    }
    pub(crate) async fn bind_udp(
        &self,
        remote: SocketAddr,
        cancel: &CancellationToken,
    ) -> Result<InternalUdp, DialError> {
        let (channel, ipv4, ipv6) = self.packet_channel.as_ref().ok_or(DialError::Closed)?;
        if !matches!(self.health_snapshot(), RuntimeHealth::Connected { .. }) {
            return Err(DialError::Closed);
        }
        let local = SocketAddr::new(
            if remote.is_ipv4() {
                (*ipv4).into()
            } else {
                (*ipv6).into()
            },
            crate::port_allocator::next_udp_port(),
        );
        let socket = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(DialError::Cancelled),
            _ = self.cancellation.cancelled() => return Err(DialError::Closed),
            result = UdpSocket::bind(channel.clone(), local) => result.map_err(|_| DialError::Closed)?,
        };
        let fragments = if remote.is_ipv6() {
            Some(tokio::select! {
                biased;
                _ = cancel.cancelled() => return Err(DialError::Cancelled),
                _ = self.cancellation.cancelled() => return Err(DialError::Closed),
                result = RawSocket::open(channel.clone()) => result.map_err(|_| DialError::Closed)?,
            })
        } else {
            None
        };
        Ok(InternalUdp::new(
            socket,
            remote,
            &self.cancellation,
            fragments,
        ))
    }

    pub(crate) async fn connect_address(
        &self,
        address: SocketAddr,
        cancel: &CancellationToken,
        deadline: Instant,
    ) -> Result<TcpStream, DialError> {
        self.connect(TcpTarget::address(address), cancel, deadline)
            .await
    }
    async fn connect(
        &self,
        target: TcpTarget,
        cancel: &CancellationToken,
        deadline: Instant,
    ) -> Result<TcpStream, DialError> {
        if !matches!(*self.health.borrow(), RuntimeHealth::Connected { .. }) {
            return Err(DialError::Closed);
        }
        tokio::select! {
            biased;
            _ = cancel.cancelled() => Err(DialError::Cancelled),
            _ = self.cancellation.cancelled() => Err(DialError::Closed),
            result = self.dialer.connect(target, deadline, cancel, FlowClass::Business) => result,
        }
    }
    async fn connect_host(
        &self,
        host: &str,
        port: u16,
        cancel: &CancellationToken,
        deadline: Instant,
        ipv6: Option<bool>,
    ) -> Result<TcpStream, InternalHttpError> {
        if self.cancellation.is_cancelled() || cancel.is_cancelled() {
            return Err(InternalHttpError::Cancelled);
        }
        if let Some(resolver) = &self.resolver {
            let addresses = tokio::select! {
                biased;
                _ = self.cancellation.cancelled() => return Err(InternalHttpError::Cancelled),
                _ = cancel.cancelled() => return Err(InternalHttpError::Cancelled),
                result = timeout_at(deadline, resolver.resolve(host)) =>
                    result.map_err(|_| InternalHttpError::Timeout)?.map_err(|_| InternalHttpError::Dns)?,
            };
            let mut failure = InternalHttpError::Connect;
            for ip in addresses {
                if ipv6.is_some_and(|v6| ip.is_ipv6() != v6) {
                    continue;
                }
                match self
                    .connect_address(SocketAddr::new(ip, port), cancel, deadline)
                    .await
                {
                    Ok(stream) => return Ok(stream),
                    Err(DialError::Cancelled) => return Err(InternalHttpError::Cancelled),
                    Err(DialError::Timeout) => failure = InternalHttpError::Timeout,
                    Err(_) => {}
                }
            }
            Err(failure)
        } else {
            let target = TcpTarget::new(host, port).map_err(|_| InternalHttpError::Dns)?;
            self.connect(target, cancel, deadline)
                .await
                .map_err(|error| match error {
                    DialError::Timeout => InternalHttpError::Timeout,
                    DialError::Cancelled => InternalHttpError::Cancelled,
                    _ => InternalHttpError::Connect,
                })
        }
    }

    /// Bounded HTTPS over this exact network. No redirect, system proxy,
    /// physical DNS, or physical TCP connection is available on this path.
    pub async fn get_https(
        &self,
        url: &str,
        limit: usize,
        cancel: &CancellationToken,
    ) -> Result<Vec<u8>, DirectoryError> {
        self.request_https(
            InternalRequest {
                url,
                method: http::Method::GET,
                headers: vec![],
                body: Bytes::new(),
                limit,
                ipv6: None,
            },
            cancel,
        )
        .await
        .map_err(DirectoryError::from)
    }
    pub(crate) async fn request_https(
        &self,
        options: InternalRequest<'_>,
        cancel: &CancellationToken,
    ) -> Result<Vec<u8>, InternalHttpError> {
        self.request_https_for(options, cancel, HttpsPurpose::General)
            .await
    }
    #[cfg(feature = "wireguard")]
    pub(crate) async fn request_warp_registration(
        &self,
        options: InternalRequest<'_>,
        cancel: &CancellationToken,
    ) -> Result<Vec<u8>, InternalHttpError> {
        self.request_https_for(options, cancel, HttpsPurpose::WarpRegistration)
            .await
    }
    async fn request_https_for(
        &self,
        options: InternalRequest<'_>,
        cancel: &CancellationToken,
        purpose: HttpsPurpose,
    ) -> Result<Vec<u8>, InternalHttpError> {
        let InternalRequest {
            url,
            method,
            headers,
            body,
            limit,
            ipv6,
        } = options;
        if limit == 0 || limit > MAX_DIRECTORY_BYTES {
            return Err(InternalHttpError::SizeLimit);
        }
        let uri: http::Uri = url.parse().map_err(|_| InternalHttpError::Protocol)?;
        if uri.scheme_str() != Some("https")
            || uri.authority().is_none_or(|a| a.as_str().contains('@'))
        {
            return Err(InternalHttpError::Protocol);
        }
        let host = uri.host().ok_or(InternalHttpError::Protocol)?.to_owned();
        let port = uri.port_u16().unwrap_or(443);
        #[cfg(feature = "wireguard")]
        if purpose == HttpsPurpose::WarpRegistration
            && (host != crate::warp_wireguard::registration_tls::HOST || port != 443)
        {
            return Err(InternalHttpError::Protocol);
        }
        let connect_budget = if purpose == HttpsPurpose::General {
            CONNECT_TIMEOUT
        } else {
            std::time::Duration::from_secs(10)
        };
        let connect_deadline = Instant::now() + connect_budget;
        let request = async {
            let establish = async {
                let stage_started = Instant::now();
                let stream = self
                    .connect_host(&host, port, cancel, connect_deadline, ipv6)
                    .await?;
                tracing::debug!(
                    probe_stage = "tcp",
                    elapsed_us = stage_started.elapsed().as_micros(),
                    "Internal HTTPS timing"
                );
                #[cfg(feature = "wireguard")]
                if purpose == HttpsPurpose::WarpRegistration {
                    let stream = crate::warp_wireguard::registration_tls::connect(stream).await?;
                    return Ok::<Box<dyn HttpsIo>, InternalHttpError>(Box::new(stream));
                }
                let roots = rustls::RootCertStore::from_iter(
                    webpki_roots::TLS_SERVER_ROOTS.iter().cloned(),
                );
                let config = rustls::ClientConfig::builder()
                    .with_root_certificates(roots)
                    .with_no_client_auth();
                let server_name = rustls::pki_types::ServerName::try_from(host.clone())
                    .map_err(|_| InternalHttpError::Protocol)?;
                let stage_started = Instant::now();
                let result = tokio_rustls::TlsConnector::from(Arc::new(config))
                    .connect(server_name, stream)
                    .await
                    .map_err(|_| InternalHttpError::Tls);
                tracing::debug!(
                    probe_stage = "tls",
                    elapsed_us = stage_started.elapsed().as_micros(),
                    ok = result.is_ok(),
                    "Internal HTTPS timing"
                );
                result.map(|stream| Box::new(stream) as Box<dyn HttpsIo>)
            };
            let stream = timeout_at(connect_deadline, establish)
                .await
                .map_err(|_| InternalHttpError::Timeout)??;
            let (mut sender, connection) =
                hyper::client::conn::http1::handshake(TokioIo::new(stream))
                    .await
                    .map_err(|_| InternalHttpError::Protocol)?;
            let _driver = tokio_util::task::AbortOnDropHandle::new(tokio::spawn(async move {
                let _ = connection.await;
            }));
            let mut request = http::Request::builder()
                .method(method)
                .uri(uri.path_and_query().map_or("/", |v| v.as_str()))
                .header(
                    http::header::HOST,
                    uri.authority().ok_or(InternalHttpError::Protocol)?.as_str(),
                )
                .header(http::header::ACCEPT_ENCODING, "identity")
                .header(
                    http::header::CONNECTION,
                    if purpose == HttpsPurpose::General {
                        "close"
                    } else {
                        "Keep-Alive"
                    },
                );
            for (name, value) in headers {
                request = request.header(name, value);
            }
            let request = request
                .body(Full::new(body))
                .map_err(|_| InternalHttpError::Protocol)?;
            let stage_started = Instant::now();
            let mut response = sender
                .send_request(request)
                .await
                .map_err(|_| InternalHttpError::Protocol)?;
            tracing::debug!(
                probe_stage = "headers",
                elapsed_us = stage_started.elapsed().as_micros(),
                "Internal HTTPS timing"
            );
            if !response.status().is_success() {
                return Err(InternalHttpError::HttpStatus(response.status().as_u16()));
            }
            if response
                .headers()
                .get(http::header::CONTENT_ENCODING)
                .is_some_and(|v| v != "identity")
            {
                return Err(InternalHttpError::Encoding);
            }
            if response
                .headers()
                .get(http::header::CONTENT_LENGTH)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok())
                .is_some_and(|n| n > limit as u64)
            {
                return Err(InternalHttpError::SizeLimit);
            }
            let mut bytes = Vec::new();
            while let Some(frame) = response.body_mut().frame().await {
                let frame = frame.map_err(|_| InternalHttpError::Protocol)?;
                if let Some(data) = frame.data_ref() {
                    if data.len() > limit.saturating_sub(bytes.len()) {
                        return Err(InternalHttpError::SizeLimit);
                    }
                    bytes.extend_from_slice(data);
                }
            }
            Ok(bytes)
        };
        tokio::select! {
            biased;
            _ = cancel.cancelled() => Err(InternalHttpError::Cancelled),
            _ = self.cancellation.cancelled() => Err(InternalHttpError::Cancelled),
            result = tokio::time::timeout(RESPONSE_TIMEOUT, request) => result.map_err(|_| InternalHttpError::Timeout)?,
        }
    }
}

pub(crate) struct InternalUdp {
    socket: Arc<UdpSocket>,
    remote: SocketAddr,
    cancellation: CancellationToken,
    fragments: Option<Arc<RawSocket>>,
    received: tokio::sync::Mutex<mpsc::Receiver<Result<Bytes, DialError>>>,
    readers: Vec<AbortOnDropHandle<()>>,
}
impl InternalUdp {
    fn new(
        socket: UdpSocket,
        remote: SocketAddr,
        parent: &CancellationToken,
        fragments: Option<RawSocket>,
    ) -> Self {
        let cancellation = parent.child_token();
        let socket = Arc::new(socket);
        let fragments = fragments.map(Arc::new);
        let (packets, received) = mpsc::channel(16);
        let mut readers = Vec::new();
        let udp = socket.clone();
        let cancel = cancellation.clone();
        let output = packets.clone();
        readers.push(AbortOnDropHandle::new(tokio::spawn(async move {
            let work = async {
                loop {
                    match udp.recv_from_bytes().await {
                        Ok((source, packet)) if source == remote => {
                            if output.send(Ok(packet)).await.is_err() {
                                break;
                            }
                        }
                        Ok(_) => {}
                        Err(_) => {
                            let _ = output.send(Err(DialError::Closed)).await;
                            break;
                        }
                    }
                }
            };
            tokio::select! { biased; _ = cancel.cancelled() => {}, _ = work => {} }
        })));
        if let (Some(raw), SocketAddr::V6(local), SocketAddr::V6(remote)) =
            (fragments.clone(), socket.local_addr(), remote)
        {
            let cancel = cancellation.clone();
            readers.push(AbortOnDropHandle::new(tokio::spawn(async move {
                let work = async {
                    let mut reassembly = crate::chain_udp::Reassembler::default();
                    loop {
                        match raw.recv_bytes().await {
                            Ok(packet) => {
                                if let Some(packet) = reassembly.receive(&packet, local, remote)
                                    && packets.send(Ok(packet)).await.is_err()
                                {
                                    break;
                                }
                            }
                            Err(_) => {
                                let _ = packets.send(Err(DialError::Closed)).await;
                                break;
                            }
                        }
                    }
                };
                tokio::select! { biased; _ = cancel.cancelled() => {}, _ = work => {} }
            })));
        }
        Self {
            socket,
            remote,
            cancellation,
            fragments,
            received: tokio::sync::Mutex::new(received),
            readers,
        }
    }
    pub(crate) async fn send(&self, packet: &[u8]) -> Result<(), DialError> {
        if packet.len() > 16 * 1024 - 48 {
            return Err(DialError::Protocol);
        }
        if packet.len() + 48 > 1280
            && let (Some(raw), SocketAddr::V6(local), SocketAddr::V6(remote)) =
                (&self.fragments, self.socket.local_addr(), self.remote)
        {
            static NEXT_FRAGMENT: std::sync::atomic::AtomicU32 =
                std::sync::atomic::AtomicU32::new(1);
            let fragments = crate::chain_udp::fragments(
                local,
                remote,
                packet,
                NEXT_FRAGMENT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            )
            .ok_or(DialError::Protocol)?;
            for fragment in fragments {
                tokio::select! {
                    biased;
                    _ = self.cancellation.cancelled() => return Err(DialError::Closed),
                    result = raw.send(&fragment) => result.map_err(|_| DialError::Closed)?,
                }
            }
            return Ok(());
        }
        tokio::select! {
            biased;
            _ = self.cancellation.cancelled() => Err(DialError::Closed),
            result = self.socket.send_to(self.remote, packet) => result.map_err(|_| DialError::Closed),
        }
    }
    /// Only this cancellation-safe channel receive is exposed to callers.
    /// Native socket RPCs stay owned by persistent readers until completion.
    pub(crate) async fn recv(&self) -> Result<Bytes, DialError> {
        tokio::select! {
            biased;
            _ = self.cancellation.cancelled() => Err(DialError::Closed),
            packet = async { self.received.lock().await.recv().await } => packet.unwrap_or(Err(DialError::Closed)),
        }
    }
}
impl Drop for InternalUdp {
    fn drop(&mut self) {
        self.cancellation.cancel();
        self.readers.clear();
    }
}

#[async_trait::async_trait]
impl CatalogueHttp for InternalNetwork {
    async fn get(
        &self,
        url: &str,
        cancellation: &CancellationToken,
    ) -> Result<Vec<u8>, DirectoryError> {
        if !approved_url(url) {
            return Err(DirectoryError::Request);
        }
        self.get_https(url, usque_core::vpngate::response_limit(url), cancellation)
            .await
    }
}

#[cfg(test)]
mod catalogue_tests {
    use super::*;
    use std::time::Duration;
    use ts_netstack_smoltcp::HasChannel;
    use ts_netstack_smoltcp::netcore::NetstackControl;
    #[cfg(feature = "wireguard")]
    async fn wireguard_through_fragmented_udp(left: &InternalUdp, right: &InternalUdp) {
        use boringtun::x25519::{PublicKey, StaticSecret};
        use usque_core::chain_exit::{Endpoint, WireGuardProfile};
        use usque_openvpn::Event;
        use zeroize::Zeroizing;
        let key = |byte| StaticSecret::from([byte; 32]);
        let profile = |own, peer| WireGuardProfile {
            private_key: Zeroizing::new(key(own).to_bytes()),
            public_key: PublicKey::from(&key(peer)).to_bytes(),
            preshared_key: None,
            endpoint: Endpoint::parse("192.0.2.1", "51820", 0).unwrap(),
            addresses: vec![format!("10.8.0.{own}/32").parse().unwrap()],
            dns_servers: vec![],
            allowed_ips: vec!["10.8.0.0/24".parse().unwrap()],
            mtu: 9000,
            keepalive: None,
        };
        let mut a = crate::wireguard::Session::start(profile(1, 2));
        let mut b = crate::wireguard::Session::start(profile(2, 1));
        let ai = a.input();
        let bi = b.input();
        let (mut at, mut ap) = a.split_packet_outputs().unwrap();
        let (mut bt, mut bp) = b.split_packet_outputs().unwrap();
        let transport = async {
            tokio::join!(
                async {
                    while let Some(Event::TransportPacket { packet, .. }) = at.recv().await {
                        left.send(&packet).await.unwrap();
                    }
                },
                async {
                    while let Some(Event::TransportPacket { packet, .. }) = bt.recv().await {
                        right.send(&packet).await.unwrap();
                    }
                },
                async {
                    loop {
                        ai.push_owned(1, 1, left.recv().await.unwrap())
                            .await
                            .unwrap();
                    }
                },
                async {
                    loop {
                        bi.push_owned(1, 1, right.recv().await.unwrap())
                            .await
                            .unwrap();
                    }
                },
            );
        };
        async fn connected(session: &mut crate::wireguard::Session) {
            loop {
                match session.next_event().await.unwrap() {
                    Event::Dial { generation } => {
                        session.input().push(3, generation, &[]).await.unwrap()
                    }
                    Event::State {
                        name, error, fatal, ..
                    } => {
                        assert!(!error && !fatal, "{name}");
                        if name == "CONNECTED" {
                            break;
                        }
                    }
                    _ => {}
                }
            }
        }
        let work = async {
            tokio::join!(connected(&mut a), connected(&mut b));
            // Both directions share the real bounded outer IP stack and raw
            // fragment reassembly, without any OS sockets or VPN interface.
            for round in 0..32 {
                for size in [64, 1280, 1420, 1500, 9000] {
                    let packet = |source, destination| {
                        let mut bytes = vec![round; size];
                        bytes[0] = 0x45;
                        bytes[2..4].copy_from_slice(&(size as u16).to_be_bytes());
                        bytes[9] = 17;
                        bytes[12..16].copy_from_slice(&[10, 8, 0, source]);
                        bytes[16..20].copy_from_slice(&[10, 8, 0, destination]);
                        Bytes::from(bytes)
                    };
                    let outgoing_a = packet(1, 2);
                    let outgoing_b = packet(2, 1);
                    let (sent_a, sent_b, received_a, received_b) = tokio::join!(
                        ai.push_owned(2, 1, outgoing_a.clone()),
                        bi.push_owned(2, 1, outgoing_b.clone()),
                        ap.recv(),
                        bp.recv(),
                    );
                    sent_a.unwrap();
                    sent_b.unwrap();
                    let Some(Event::IpPacket {
                        packet: received_a, ..
                    }) = received_a
                    else {
                        panic!("authenticated packet")
                    };
                    let Some(Event::IpPacket {
                        packet: received_b, ..
                    }) = received_b
                    else {
                        panic!("authenticated packet")
                    };
                    assert_eq!(received_a, outgoing_b);
                    assert_eq!(received_b, outgoing_a);
                }
            }
        };
        tokio::time::timeout(Duration::from_secs(10), async {
            tokio::select! { _ = work => {}, _ = transport => panic!("transport stopped before load completed") }
        }).await.expect("fragmented WireGuard duplex transfer must advance");
        a.shutdown().await.unwrap();
        b.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn memory_only_udp_crosses_1280_mtu_stacks_in_both_families() {
        for (a, b) in [
            ("192.0.2.1:40001", "192.0.2.2:51820"),
            ("[2001:db8::1]:40001", "[2001:db8::2]:51820"),
        ] {
            let a: SocketAddr = a.parse().unwrap();
            let b: SocketAddr = b.parse().unwrap();
            let profile = usque_core::Profile {
                mtu: 1280,
                ..Default::default()
            };
            let (config, _) = crate::netstack::proxy_netstack_config(&profile);
            let (left, mut left_pipe) = crate::netstack::bounded_piped(config);
            let (config, _) = crate::netstack::proxy_netstack_config(&profile);
            let (right, mut right_pipe) = crate::netstack::bounded_piped(config);
            let left_channel = left.command_channel();
            let right_channel = right.command_channel();
            let left_task = left.spawn_tokio();
            let right_task = right.spawn_tokio();
            left_channel.set_ips([a.ip()]).await.unwrap();
            right_channel.set_ips([b.ip()]).await.unwrap();
            let bridge = tokio::spawn(async move {
                loop {
                    tokio::select! {
                        Some(packet) = left_pipe.rx.recv_async() => {
                            assert!(packet.len() <= 1280);
                            right_pipe.tx.send_owned_async(packet).await;
                        }
                        Some(packet) = right_pipe.rx.recv_async() => {
                            assert!(packet.len() <= 1280);
                            left_pipe.tx.send_owned_async(packet).await;
                        }
                        else => break,
                    }
                }
            });
            let cancel = CancellationToken::new();
            let left = InternalUdp::new(
                UdpSocket::bind(left_channel.clone(), a).await.unwrap(),
                b,
                &cancel,
                if a.is_ipv6() {
                    Some(RawSocket::open(left_channel.clone()).await.unwrap())
                } else {
                    None
                },
            );
            let right = InternalUdp::new(
                UdpSocket::bind(right_channel.clone(), b).await.unwrap(),
                a,
                &cancel,
                if b.is_ipv6() {
                    Some(RawSocket::open(right_channel.clone()).await.unwrap())
                } else {
                    None
                },
            );
            for size in [32, 1312, 9032] {
                let bytes = vec![0x57; size];
                // Submit a receive RPC and abandon only its public waiter
                // after the datagram can arrive. Persistent readers must keep
                // the result; the former select-owned socket RPC lost it here.
                let mut abandoned = Box::pin(right.recv());
                std::future::poll_fn(|cx| {
                    use std::future::Future;
                    assert!(abandoned.as_mut().poll(cx).is_pending());
                    std::task::Poll::Ready(())
                })
                .await;
                left.send(&bytes).await.unwrap();
                for _ in 0..8 {
                    tokio::task::yield_now().await;
                }
                drop(abandoned);
                assert_eq!(
                    tokio::time::timeout(Duration::from_secs(2), right.recv())
                        .await
                        .unwrap()
                        .unwrap(),
                    bytes
                );
                right.send(&bytes).await.unwrap();
                assert_eq!(
                    tokio::time::timeout(Duration::from_secs(2), left.recv())
                        .await
                        .unwrap()
                        .unwrap(),
                    bytes
                );
            }
            #[cfg(feature = "wireguard")]
            wireguard_through_fragmented_udp(&left, &right).await;
            cancel.cancel();
            assert!(left.recv().await.is_err());
            assert!(right.send(&[0]).await.is_err());
            bridge.abort();
            left_task.abort();
            right_task.abort();
        }
    }
    struct FailureDialer(DialError);
    #[async_trait::async_trait]
    impl TcpDialer for FailureDialer {
        async fn connect(
            &self,
            _: TcpTarget,
            _: Instant,
            _: &CancellationToken,
            _: FlowClass,
        ) -> Result<TcpStream, DialError> {
            Err(self.0)
        }
    }
    #[tokio::test]
    async fn immediate_underlay_timeouts_and_cancellation_keep_their_type() {
        for (dial, expected) in [
            (DialError::Timeout, DirectoryError::Timeout),
            (DialError::Cancelled, DirectoryError::Cancelled),
            (DialError::Refused, DirectoryError::Request),
        ] {
            let (_sender, health) = watch::channel(RuntimeHealth::Connected {
                path: crate::netstack::RuntimePath {
                    transport: usque_core::Transport::Http3,
                    endpoint_family: usque_core::AddressFamily::Ipv4,
                    ipv4_available: true,
                    ipv6_available: false,
                },
                reconnect_count: 0,
            });
            let network = InternalNetwork::for_streams(
                Arc::new(FailureDialer(dial)),
                health,
                CancellationToken::new(),
            );
            assert_eq!(
                network
                    .get(usque_core::vpngate::RAW_URL, &CancellationToken::new())
                    .await,
                Err(expected)
            );
        }
    }
}
