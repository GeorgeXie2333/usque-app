//! Final HTTP/SOCKS exits. Every server connection belongs to the WARP underlay.
use crate::tcp::{DialError, FlowClass, TcpDialer, TcpIo, TcpStream, TcpTarget};
#[cfg(test)]
mod tests;
use async_trait::async_trait;
use base64::Engine;
use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, watch};
use tokio::time::{Instant, timeout_at};
use tokio_util::sync::CancellationToken;
use usque_core::chain_exit::{ChainProtocol, ImportSecrets, ProxyAuthMode, ProxyProfile};
use usque_core::vpngate::{GateFailure, GateStage, GateStatus, PreparedProfile};

pub(crate) struct ProxyDialer {
    pub(crate) network: crate::InternalNetwork,
    pub(crate) config: ProxyProfile,
    credentials: Box<ImportSecrets>,
    pub(crate) cancellation: CancellationToken,
    pub(crate) status: watch::Sender<GateStatus>,
    pub(crate) admitted: AtomicBool,
    pub(crate) active: Arc<Semaphore>,
    pub(crate) pending: Arc<Semaphore>,
    pub(crate) budget: Arc<crate::l4::BufferBudget>,
    pub(crate) counters: Arc<crate::netstack::TrafficCounters>,
}

impl ProxyDialer {
    #[expect(
        clippy::too_many_arguments,
        reason = "startup binds the sealed profile to one underlay and resource budget"
    )]
    pub(crate) async fn start(
        prepared: &PreparedProfile,
        network: crate::InternalNetwork,
        status: Option<watch::Sender<GateStatus>>,
        cancellation: CancellationToken,
        budget: Arc<crate::l4::BufferBudget>,
        counters: Arc<crate::netstack::TrafficCounters>,
        deadline: Instant,
    ) -> Result<Arc<Self>, DialError> {
        let deadline = deadline.min(Instant::now() + std::time::Duration::from_secs(10));
        let Some(usque_core::chain_exit::ValidatedProfile::Proxy(config)) =
            prepared.custom.as_deref()
        else {
            return Err(DialError::InvalidTarget);
        };
        let status = status.unwrap_or_else(|| watch::channel(GateStatus::default()).0);
        status.send_modify(|s| {
            s.stage = GateStage::ConnectingServer;
            s.current_profile = prepared.summary.clone();
            s.attempting_endpoint = Some(config.endpoint.clone());
            s.attempt_count = 1;
            s.candidate_count = 1;
            s.proxy_udp = Some(
                if config.protocol == ChainProtocol::Socks5 && network.supports_udp() {
                    "unknown"
                } else {
                    "unavailable"
                }
                .into(),
            );
        });
        let limits = crate::l4::Limits::platform();
        let result = Arc::new(Self {
            network,
            config: config.clone(),
            credentials: prepared.credentials.clone(),
            cancellation,
            status,
            admitted: AtomicBool::new(false),
            active: Arc::new(Semaphore::new(limits.active)),
            pending: Arc::new(Semaphore::new(limits.pending)),
            budget,
            counters,
        });
        let (mut stream, _) = result.server(deadline, &result.cancellation).await?;
        if result.config.protocol == ChainProtocol::Socks5 {
            result
                .authenticate(&mut stream, deadline, &result.cancellation)
                .await?;
        }
        drop(stream);
        result.status.send_modify(|s| {
            s.stage = GateStage::ConfiguringNetwork;
            s.attempting_endpoint = None;
        });
        Ok(result)
    }

    pub(crate) fn fail(&self, failure: GateFailure) {
        self.status.send_modify(|s| {
            s.failure = Some(failure);
            s.stage = GateStage::Error;
            s.network = None;
        });
        self.cancellation.cancel();
    }
    pub(crate) async fn server(
        &self,
        deadline: Instant,
        cancel: &CancellationToken,
    ) -> Result<(TcpStream, SocketAddr), DialError> {
        tokio::select! {
            biased;
            _ = cancel.cancelled() => Err(DialError::Cancelled),
            _ = self.cancellation.cancelled() => Err(DialError::Closed),
            result = timeout_at(deadline, async {
                let address = self.network.resolve_endpoint(&self.config.endpoint, None, cancel).await?;
                let stream = self.network.connect_address(address, cancel, deadline).await?;
                self.status.send_modify(|s| s.active_endpoint = Some(address));
                Ok((stream, address))
            }) => result.map_err(|_| DialError::Timeout)?,
        }
    }
    pub(crate) async fn authenticate(
        &self,
        stream: &mut TcpStream,
        deadline: Instant,
        cancel: &CancellationToken,
    ) -> Result<(), DialError> {
        let result = tokio::select! {
            biased;
            _ = cancel.cancelled() => Err(DialError::Cancelled),
            _ = self.cancellation.cancelled() => Err(DialError::Closed),
            result = timeout_at(deadline, socks_auth(stream, &self.credentials, self.config.auth_mode)) => result.map_err(|_| DialError::Timeout)?,
        };
        if matches!(result, Err(DialError::Rejected(407))) {
            self.fail(GateFailure::Authentication);
        }
        result
    }
}

#[async_trait]
impl TcpDialer for ProxyDialer {
    fn session_generation(&self) -> Option<u64> {
        self.network.session_generation()
    }
    fn is_ready(&self) -> bool {
        self.admitted.load(Ordering::Acquire) && !self.cancellation.is_cancelled()
    }
    async fn connect(
        &self,
        target: TcpTarget,
        deadline: Instant,
        cancel: &CancellationToken,
        _class: FlowClass,
    ) -> Result<TcpStream, DialError> {
        if !self.admitted.load(Ordering::Acquire) {
            return Err(DialError::Closed);
        }
        let _pending = self
            .pending
            .clone()
            .try_acquire_owned()
            .map_err(|_| DialError::Budget)?;
        let permit = self
            .active
            .clone()
            .try_acquire_owned()
            .map_err(|_| DialError::Budget)?;
        let lease = self
            .budget
            .reserve_admission(crate::l4::Limits::platform().relay * 2)
            .ok_or(DialError::Budget)?;
        let result = tokio::select! {
            biased;
            _ = cancel.cancelled() => Err(DialError::Cancelled),
            _ = self.cancellation.cancelled() => Err(DialError::Closed),
            result = timeout_at(deadline, async {
                let (mut stream, _) = self.server(deadline, cancel).await?;
                if self.config.protocol == ChainProtocol::HttpConnect {
                    http_connect(&mut stream, &target, &self.credentials, self.config.auth_mode).await?;
                } else {
                    self.authenticate(&mut stream, deadline, cancel).await?;
                    socks_command(&mut stream, 1, &target).await?;
                }
                Ok(stream)
            }) => result.map_err(|_| DialError::Timeout)?,
        };
        match result {
            Ok(stream) => {
                self.status.send_modify(|s| s.tcp_connect_verified = true);
                Ok(Box::new(ProxyStream {
                    stream,
                    _permit: permit,
                    _lease: lease,
                    cancelled: Box::pin(self.cancellation.clone().cancelled_owned()),
                    counters: self.counters.clone(),
                }))
            }
            Err(DialError::Rejected(407)) => {
                self.fail(GateFailure::Authentication);
                Err(DialError::Rejected(407))
            }
            Err(error) => Err(error),
        }
    }
}

struct ProxyStream {
    stream: TcpStream,
    _permit: OwnedSemaphorePermit,
    _lease: crate::l4::stream::BufferLease,
    cancelled: Pin<Box<dyn std::future::Future<Output = ()> + Send>>,
    counters: Arc<crate::netstack::TrafficCounters>,
}
impl TcpIo for ProxyStream {
    fn local_addr(&self) -> io::Result<SocketAddr> {
        self.stream.local_addr()
    }
    fn session_generation(&self) -> Option<u64> {
        self.stream.session_generation()
    }
}
impl AsyncRead for ProxyStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if self.cancelled.as_mut().poll(cx).is_ready() {
            return Poll::Ready(Err(io::ErrorKind::ConnectionAborted.into()));
        }
        let before = buf.filled().len();
        let result = Pin::new(&mut self.stream).poll_read(cx, buf);
        self.counters.record_received(buf.filled().len() - before);
        result
    }
}
impl AsyncWrite for ProxyStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        if self.cancelled.as_mut().poll(cx).is_ready() {
            return Poll::Ready(Err(io::ErrorKind::ConnectionAborted.into()));
        }
        let result = Pin::new(&mut self.stream).poll_write(cx, buf);
        if let Poll::Ready(Ok(n)) = result {
            self.counters.record_sent(n);
        }
        result
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stream).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stream).poll_shutdown(cx)
    }
}

async fn http_connect(
    stream: &mut TcpStream,
    target: &TcpTarget,
    credentials: &ImportSecrets,
    auth: ProxyAuthMode,
) -> Result<(), DialError> {
    let mut request = zeroize::Zeroizing::new(format!(
        "CONNECT {} HTTP/1.1\r\nHost: {}\r\n",
        target.authority(),
        target.authority()
    ));
    if auth == ProxyAuthMode::UsernamePassword {
        let plain =
            zeroize::Zeroizing::new(format!("{}:{}", credentials.username, credentials.password));
        let encoded = zeroize::Zeroizing::new(
            base64::engine::general_purpose::STANDARD.encode(plain.as_bytes()),
        );
        request.push_str("Proxy-Authorization: Basic ");
        request.push_str(&encoded);
        request.push_str("\r\n");
    }
    request.push_str("\r\n");
    stream
        .write_all(request.as_bytes())
        .await
        .map_err(|_| DialError::Closed)?;
    // Read exactly the header: bytes following it already belong to the target.
    for _ in 0..8 {
        let mut header = Vec::new();
        loop {
            if header.len() >= 16 * 1024 {
                return Err(DialError::Protocol);
            }
            header.push(stream.read_u8().await.map_err(|_| DialError::Closed)?);
            if header.ends_with(b"\r\n\r\n") {
                break;
            }
        }
        let line = header
            .split(|b| *b == b'\n')
            .next()
            .ok_or(DialError::Protocol)?;
        let line = std::str::from_utf8(line).map_err(|_| DialError::Protocol)?;
        let mut parts = line.split_ascii_whitespace();
        if !matches!(parts.next(), Some("HTTP/1.0" | "HTTP/1.1")) {
            return Err(DialError::Protocol);
        }
        let fields = header.split(|b| *b == b'\n').skip(1);
        let mut count = 0;
        for field in fields {
            let field = field.strip_suffix(b"\r").unwrap_or(field);
            if field.is_empty() {
                continue;
            }
            count += 1;
            let Some(colon) = field.iter().position(|b| *b == b':') else {
                return Err(DialError::Protocol);
            };
            if count > 128
                || colon == 0
                || !field[..colon]
                    .iter()
                    .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_\x60|~".contains(b))
                || field[colon + 1..]
                    .iter()
                    .any(|b| *b < 32 && *b != b'\t' || *b == 127)
            {
                return Err(DialError::Protocol);
            }
        }
        let code = parts
            .next()
            .filter(|s| s.len() == 3)
            .and_then(|s| s.parse::<u16>().ok())
            .ok_or(DialError::Protocol)?;
        if (200..300).contains(&code) {
            return Ok(());
        }
        if (100..200).contains(&code) && code != 101 {
            continue;
        }
        return Err(DialError::Rejected(code));
    }
    Err(DialError::Protocol)
}

async fn socks_auth(
    stream: &mut TcpStream,
    credentials: &ImportSecrets,
    auth: ProxyAuthMode,
) -> Result<(), DialError> {
    let method = if auth == ProxyAuthMode::None { 0 } else { 2 };
    stream
        .write_all(&[5, 1, method])
        .await
        .map_err(|_| DialError::Closed)?;
    let mut response = [0; 2];
    stream
        .read_exact(&mut response)
        .await
        .map_err(|_| DialError::Closed)?;
    if response[0] != 5 {
        return Err(DialError::Protocol);
    }
    if response[1] != method {
        return Err(DialError::Rejected(407));
    }
    if method == 2 {
        let mut request = zeroize::Zeroizing::new(vec![
            1,
            u8::try_from(credentials.username.len()).map_err(|_| DialError::InvalidTarget)?,
        ]);
        request.extend_from_slice(credentials.username.as_bytes());
        request
            .push(u8::try_from(credentials.password.len()).map_err(|_| DialError::InvalidTarget)?);
        request.extend_from_slice(credentials.password.as_bytes());
        stream
            .write_all(&request)
            .await
            .map_err(|_| DialError::Closed)?;
        stream
            .read_exact(&mut response)
            .await
            .map_err(|_| DialError::Closed)?;
        if response[0] != 1 {
            return Err(DialError::Protocol);
        }
        if response[1] != 0 {
            return Err(DialError::Rejected(407));
        }
    }
    Ok(())
}

pub(crate) fn encode_target(target: &TcpTarget, output: &mut Vec<u8>) -> Result<(), DialError> {
    let (host, port) = target.host_port();
    match target.socket_address() {
        Some(SocketAddr::V4(addr)) => {
            output.push(1);
            output.extend_from_slice(&addr.ip().octets());
        }
        Some(SocketAddr::V6(addr)) => {
            output.push(4);
            output.extend_from_slice(&addr.ip().octets());
        }
        None => {
            output.extend_from_slice(&[
                3,
                u8::try_from(host.len()).map_err(|_| DialError::InvalidTarget)?,
            ]);
            output.extend_from_slice(host.as_bytes());
        }
    }
    output.extend_from_slice(&port.to_be_bytes());
    Ok(())
}

pub(crate) async fn socks_command(
    stream: &mut TcpStream,
    command: u8,
    target: &TcpTarget,
) -> Result<(String, u16), DialError> {
    let mut request = vec![5, command, 0];
    encode_target(target, &mut request)?;
    stream
        .write_all(&request)
        .await
        .map_err(|_| DialError::Closed)?;
    let mut header = [0; 4];
    stream
        .read_exact(&mut header)
        .await
        .map_err(|_| DialError::Closed)?;
    if header[0] != 5 || header[2] != 0 || header[1] > 8 {
        return Err(DialError::Protocol);
    }
    let size = match header[3] {
        1 => 4,
        4 => 16,
        3 => usize::from(stream.read_u8().await.map_err(|_| DialError::Closed)?),
        _ => return Err(DialError::Protocol),
    };
    if size == 0 {
        return Err(DialError::Protocol);
    }
    let mut bytes = vec![0; size];
    stream
        .read_exact(&mut bytes)
        .await
        .map_err(|_| DialError::Closed)?;
    let host = match header[3] {
        1 => std::net::Ipv4Addr::from(
            <[u8; 4]>::try_from(bytes.as_slice()).map_err(|_| DialError::Protocol)?,
        )
        .to_string(),
        4 => std::net::Ipv6Addr::from(
            <[u8; 16]>::try_from(bytes.as_slice()).map_err(|_| DialError::Protocol)?,
        )
        .to_string(),
        _ => String::from_utf8(bytes).map_err(|_| DialError::Protocol)?,
    };
    let port = stream.read_u16().await.map_err(|_| DialError::Closed)?;
    // Classify only a complete, valid command reply. A malformed or truncated
    // denial must not enable a DNS-only UDP association.
    if header[1] != 0 {
        return Err(if header[1] == 7 {
            DialError::Rejected(7)
        } else {
            DialError::Refused
        });
    }
    Ok((host, port))
}
