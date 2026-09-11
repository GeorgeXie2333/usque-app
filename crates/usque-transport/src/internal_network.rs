//! Private network clients never pass through a frontend's direct-rule policy.
//! A handle is bound to one runtime and cannot silently obtain another exit.
use crate::dns::Resolver;
use crate::netstack::{PacketStack, RuntimeHealth};
use crate::tcp::{DialError, FlowClass, StackDialer, TcpDialer, TcpStream, TcpTarget};
use bytes::Bytes;
use http_body_util::{BodyExt, Empty};
use hyper_util::rt::TokioIo;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use tokio::sync::watch;
use tokio::time::{Instant, timeout_at};
use tokio_util::sync::CancellationToken;
use usque_core::vpngate::{
    CONNECT_TIMEOUT, CatalogueHttp, DirectoryError, MAX_DIRECTORY_BYTES, RESPONSE_TIMEOUT,
    approved_url,
};

#[derive(Clone)]
pub struct InternalNetwork {
    dialer: Arc<dyn TcpDialer>,
    resolver: Option<Resolver>,
    health: watch::Receiver<RuntimeHealth>,
    cancellation: CancellationToken,
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
        let (ipv4, ipv6) = tokio::join!(
            self.probe_ip("https://api-ipv4.ip.sb/ip", false, &cancel),
            self.probe_ip("https://api-ipv6.ip.sb/ip", true, &cancel),
        );
        if ipv4.is_none() && ipv6.is_none() {
            return Err(DirectoryError::Request);
        }
        let (ipv4_location, ipv6_location) =
            tokio::join!(self.probe_geo(ipv4, &cancel), self.probe_geo(ipv6, &cancel));
        Ok(usque_core::ExitInfo {
            ipv4,
            ipv6,
            ipv4_location,
            ipv6_location,
            checked_at: std::time::SystemTime::now().into(),
        })
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
            resolver: Some(Resolver::new(
                stack.channel.clone(),
                ipv4,
                ipv6,
                profile.dns_servers.clone(),
                usque_core::ProxyDnsMode::Remote,
                stack.protector.clone(),
            )),
            health: stack.subscribe_health(),
            cancellation: stack.cancellation.clone(),
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
        }
    }
    pub(crate) fn health(&self) -> watch::Receiver<RuntimeHealth> {
        self.health.clone()
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
    ) -> Result<TcpStream, DirectoryError> {
        if self.cancellation.is_cancelled() || cancel.is_cancelled() {
            return Err(DirectoryError::Cancelled);
        }
        if let Some(resolver) = &self.resolver {
            let addresses = tokio::select! {
                biased;
                _ = self.cancellation.cancelled() => return Err(DirectoryError::Cancelled),
                _ = cancel.cancelled() => return Err(DirectoryError::Cancelled),
                result = timeout_at(deadline, resolver.resolve(host)) =>
                    result.map_err(|_| DirectoryError::Timeout)?.map_err(|_| DirectoryError::Request)?,
            };
            for ip in addresses {
                if let Ok(stream) = self
                    .connect_address(SocketAddr::new(ip, port), cancel, deadline)
                    .await
                {
                    return Ok(stream);
                }
            }
            Err(DirectoryError::Request)
        } else {
            let target = TcpTarget::new(host, port).map_err(|_| DirectoryError::Request)?;
            self.connect(target, cancel, deadline)
                .await
                .map_err(|_| DirectoryError::Request)
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
        if limit == 0 || limit > MAX_DIRECTORY_BYTES {
            return Err(DirectoryError::SizeLimit);
        }
        let uri: http::Uri = url.parse().map_err(|_| DirectoryError::Request)?;
        if uri.scheme_str() != Some("https")
            || uri.authority().is_none_or(|a| a.as_str().contains('@'))
        {
            return Err(DirectoryError::Request);
        }
        let host = uri.host().ok_or(DirectoryError::Request)?.to_owned();
        let port = uri.port_u16().unwrap_or(443);
        let connect_deadline = Instant::now() + CONNECT_TIMEOUT;
        let request = async {
            let establish = async {
                let stream = self
                    .connect_host(&host, port, cancel, connect_deadline)
                    .await?;
                let roots = rustls::RootCertStore::from_iter(
                    webpki_roots::TLS_SERVER_ROOTS.iter().cloned(),
                );
                let config = rustls::ClientConfig::builder()
                    .with_root_certificates(roots)
                    .with_no_client_auth();
                let server_name = rustls::pki_types::ServerName::try_from(host.clone())
                    .map_err(|_| DirectoryError::Request)?;
                tokio_rustls::TlsConnector::from(Arc::new(config))
                    .connect(server_name, stream)
                    .await
                    .map_err(|_| DirectoryError::Request)
            };
            let stream = timeout_at(connect_deadline, establish)
                .await
                .map_err(|_| DirectoryError::Timeout)??;
            let (mut sender, connection) =
                hyper::client::conn::http1::handshake(TokioIo::new(stream))
                    .await
                    .map_err(|_| DirectoryError::Request)?;
            let _driver = tokio_util::task::AbortOnDropHandle::new(tokio::spawn(async move {
                let _ = connection.await;
            }));
            let request = http::Request::builder()
                .method("GET")
                .uri(uri.path_and_query().map_or("/", |v| v.as_str()))
                .header(
                    http::header::HOST,
                    uri.authority().ok_or(DirectoryError::Request)?.as_str(),
                )
                .header(http::header::ACCEPT_ENCODING, "identity")
                .header(http::header::CONNECTION, "close")
                .body(Empty::<Bytes>::new())
                .map_err(|_| DirectoryError::Request)?;
            let mut response = sender
                .send_request(request)
                .await
                .map_err(|_| DirectoryError::Request)?;
            if response.status() != http::StatusCode::OK {
                return Err(DirectoryError::Request);
            }
            if response
                .headers()
                .get(http::header::CONTENT_ENCODING)
                .is_some_and(|v| v != "identity")
            {
                return Err(DirectoryError::InvalidDirectory);
            }
            if response
                .headers()
                .get(http::header::CONTENT_LENGTH)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok())
                .is_some_and(|n| n > limit as u64)
            {
                return Err(DirectoryError::SizeLimit);
            }
            let mut bytes = Vec::new();
            while let Some(frame) = response.body_mut().frame().await {
                let frame = frame.map_err(|_| DirectoryError::Request)?;
                if let Some(data) = frame.data_ref() {
                    if data.len() > limit.saturating_sub(bytes.len()) {
                        return Err(DirectoryError::SizeLimit);
                    }
                    bytes.extend_from_slice(data);
                }
            }
            Ok(bytes)
        };
        tokio::select! {
            biased;
            _ = cancel.cancelled() => Err(DirectoryError::Cancelled),
            _ = self.cancellation.cancelled() => Err(DirectoryError::Cancelled),
            result = tokio::time::timeout(RESPONSE_TIMEOUT, request) => result.map_err(|_| DirectoryError::Timeout)?,
        }
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
        self.get_https(url, MAX_DIRECTORY_BYTES, cancellation).await
    }
}
