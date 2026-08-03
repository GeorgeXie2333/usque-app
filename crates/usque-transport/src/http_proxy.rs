use std::collections::HashSet;
use std::io::Cursor;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU16, Ordering};
use std::time::Duration;

use http::uri::Authority;
use tokio::io::{AsyncBufRead, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpSocket, TcpStream};
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio::time::timeout;
use ts_netstack_smoltcp::CreateSocket;
use ts_netstack_smoltcp::netcore::Channel;
use ts_netstack_smoltcp::netsock::TcpStream as StackTcpStream;
use usque_core::{IpPolicy, OperatingMode, Profile};

use crate::dns::Resolver;
use crate::h2::{MasqueTlsIdentity, TransportError};
use crate::netstack::{PacketStack, RuntimeHealth, RuntimePath, TrafficSnapshot};
use crate::pin_refresh::EndpointPinRefresher;
use crate::socket::{SocketProtector, noop_socket_protector};

const MAX_HEADER_BYTES: usize = 64 * 1024;
const MAX_HEADERS: usize = 128;
const MAX_CHUNK_LINE_BYTES: usize = 8 * 1024;
const HEADER_TIMEOUT: Duration = Duration::from_secs(15);
const REMOTE_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_TARGET_ADDRESSES: usize = 16;

static NEXT_TCP_PORT: AtomicU16 = AtomicU16::new(49_152);

pub struct HttpProxyRuntime {
    stack: PacketStack,
    listener_tasks: Vec<JoinHandle<()>>,
    listeners: Vec<SocketAddr>,
}

impl HttpProxyRuntime {
    pub async fn start(
        profile: &Profile,
        identity: MasqueTlsIdentity,
    ) -> Result<Self, TransportError> {
        Self::start_with_protector(profile, identity, noop_socket_protector()).await
    }

    pub async fn start_with_protector(
        profile: &Profile,
        identity: MasqueTlsIdentity,
        protector: Arc<dyn SocketProtector>,
    ) -> Result<Self, TransportError> {
        Self::start_with_refresh(profile, identity, protector, None).await
    }

    pub async fn start_with_refresh(
        profile: &Profile,
        identity: MasqueTlsIdentity,
        protector: Arc<dyn SocketProtector>,
        pin_refresher: Option<Arc<dyn EndpointPinRefresher>>,
    ) -> Result<Self, TransportError> {
        if profile.mode != OperatingMode::HttpProxy {
            return Err(TransportError::UnsupportedOperatingMode);
        }

        let mut bound = Vec::with_capacity(profile.proxy.http_listeners.len());
        for address in &profile.proxy.http_listeners {
            bound.push(bind_listener(*address)?);
        }

        let assigned_ipv4 = identity.assigned_ipv4;
        let assigned_ipv6 = identity.assigned_ipv6;
        let mut stack =
            PacketStack::start_with_refresh(profile, Arc::new(identity), protector, pin_refresher)
                .await?;
        let resolver = Resolver::new(
            stack.channel.clone(),
            assigned_ipv4,
            assigned_ipv6,
            profile.dns_servers.clone(),
            profile.proxy.dns_mode,
            profile.ip_policy,
        );
        let context = Arc::new(HttpContext {
            channel: stack.channel.clone(),
            resolver,
            assigned_ipv4,
            assigned_ipv6,
            ip_policy: profile.ip_policy,
            cancellation: stack.cancellation.clone(),
            failure: stack.failure_tx.clone(),
            health: stack.subscribe_health(),
        });

        let listeners = bound
            .iter()
            .filter_map(|listener| listener.local_addr().ok())
            .collect::<Vec<_>>();
        let mut listener_tasks = Vec::with_capacity(bound.len());
        for listener in bound {
            let context = Arc::clone(&context);
            listener_tasks.push(tokio::spawn(async move {
                run_listener(listener, context).await;
            }));
        }

        tokio::task::yield_now().await;
        let startup_failure = stack.failure.borrow().clone();
        if let Some(message) = startup_failure {
            stack.shutdown().await;
            return Err(TransportError::HttpProxy(message));
        }

        Ok(Self {
            stack,
            listener_tasks,
            listeners,
        })
    }

    pub fn path(&self) -> RuntimePath {
        self.stack.path()
    }

    pub fn health(&self) -> RuntimeHealth {
        self.stack.health()
    }

    pub fn listeners(&self) -> &[SocketAddr] {
        &self.listeners
    }

    pub fn statistics(&self) -> TrafficSnapshot {
        self.stack.counters.snapshot()
    }

    pub fn failure(&self) -> Option<String> {
        self.stack.failure.borrow().clone()
    }

    pub fn cancel_immediately(&mut self) {
        self.stack.cancel_immediately();
        for task in &self.listener_tasks {
            task.abort();
        }
    }

    pub async fn shutdown(&mut self) {
        self.cancel_immediately();
        for task in self.listener_tasks.drain(..) {
            let _ = task.await;
        }
        self.stack.shutdown().await;
    }
}

impl Drop for HttpProxyRuntime {
    fn drop(&mut self) {
        self.cancel_immediately();
    }
}

struct HttpContext {
    channel: Channel,
    resolver: Resolver,
    assigned_ipv4: Ipv4Addr,
    assigned_ipv6: Ipv6Addr,
    ip_policy: IpPolicy,
    cancellation: tokio_util::sync::CancellationToken,
    failure: watch::Sender<Option<String>>,
    health: watch::Receiver<RuntimeHealth>,
}

fn bind_listener(address: SocketAddr) -> Result<TcpListener, TransportError> {
    let socket = if address.is_ipv4() {
        TcpSocket::new_v4()
    } else {
        TcpSocket::new_v6()
    }
    .map_err(|source| TransportError::HttpProxyListener { address, source })?;
    socket
        .bind(address)
        .map_err(|source| TransportError::HttpProxyListener { address, source })?;
    socket
        .listen(256)
        .map_err(|source| TransportError::HttpProxyListener { address, source })
}

async fn run_listener(listener: TcpListener, context: Arc<HttpContext>) {
    loop {
        let accepted = tokio::select! {
            _ = context.cancellation.cancelled() => break,
            accepted = listener.accept() => accepted,
        };
        let (stream, peer) = match accepted {
            Ok(value) => value,
            Err(error) => {
                tracing::error!(%error, "HTTP proxy listener stopped");
                if !context.cancellation.is_cancelled() && context.failure.borrow().is_none() {
                    let _ = context
                        .failure
                        .send(Some(format!("HTTP proxy listener failed: {error}")));
                }
                break;
            }
        };
        if !peer.ip().is_loopback()
            && stream
                .local_addr()
                .is_ok_and(|address| address.ip().is_loopback())
        {
            tracing::warn!(%peer, "rejected non-loopback peer on a loopback HTTP proxy listener");
            continue;
        }
        let connection_context = Arc::clone(&context);
        tokio::spawn(async move {
            if let Err(error) = serve_client(stream, connection_context).await {
                tracing::debug!(%peer, %error, "HTTP proxy session ended");
            }
        });
    }
}

async fn serve_client(
    mut client: TcpStream,
    context: Arc<HttpContext>,
) -> Result<(), TransportError> {
    let (head, buffered_body) = match timeout(HEADER_TIMEOUT, read_request_head(&mut client)).await
    {
        Ok(Ok(value)) => value,
        Ok(Err(error)) => {
            let _ = send_error(&mut client, 400, "Bad Request").await;
            return Err(error);
        }
        Err(_) => {
            let _ = send_error(&mut client, 408, "Request Timeout").await;
            return Err(TransportError::HttpProxy(
                "HTTP proxy request header timed out".to_owned(),
            ));
        }
    };
    let request = match parse_proxy_request(&head) {
        Ok(request) => request,
        Err(error) => {
            let _ = send_error(&mut client, error.status, error.reason).await;
            return Err(TransportError::HttpProxy(error.message));
        }
    };
    if matches!(
        request.kind,
        RequestKind::Forward {
            body: BodyFraming::None,
            ..
        }
    ) && !buffered_body.is_empty()
    {
        let _ = send_error(&mut client, 400, "Bad Request").await;
        return Err(TransportError::HttpProxy(
            "unexpected bytes after a bodyless proxy request".to_owned(),
        ));
    }
    if let RequestKind::Forward {
        body: BodyFraming::ContentLength(length),
        ..
    } = request.kind
        && buffered_body.len() as u64 > length
    {
        let _ = send_error(&mut client, 400, "Bad Request").await;
        return Err(TransportError::HttpProxy(
            "pipelined bytes after the declared HTTP request body are not allowed".to_owned(),
        ));
    }

    if !matches!(&*context.health.borrow(), RuntimeHealth::Connected { .. }) {
        let _ = send_error(&mut client, 503, "Service Unavailable").await;
        return Err(TransportError::HttpProxy(
            "the MASQUE channel is reconnecting".to_owned(),
        ));
    }

    let addresses = match context.resolver.resolve(&request.destination.host).await {
        Ok(addresses) => addresses,
        Err(error) => {
            let _ = send_error(&mut client, 502, "Bad Gateway").await;
            return Err(error);
        }
    };
    let mut remote = match connect_remote(&context, &addresses, request.destination.port).await {
        Ok(remote) => remote,
        Err(error) => {
            let _ = send_error(&mut client, 502, "Bad Gateway").await;
            return Err(TransportError::HttpProxy(error));
        }
    };

    match request.kind {
        RequestKind::Connect => {
            client
                .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
                .await?;
            if !buffered_body.is_empty() {
                remote.write_all(&buffered_body).await.map_err(|error| {
                    TransportError::HttpProxy(format!("write early CONNECT data: {error}"))
                })?;
            }
            tokio::select! {
                _ = context.cancellation.cancelled() => Ok(()),
                result = tokio::io::copy_bidirectional(&mut client, &mut remote) => {
                    result
                        .map(|_| ())
                        .map_err(|error| TransportError::HttpProxy(error.to_string()))
                }
            }
        }
        RequestKind::Forward {
            rewritten_head,
            body,
        } => {
            remote
                .write_all(&rewritten_head)
                .await
                .map_err(|error| TransportError::HttpProxy(error.to_string()))?;
            relay_forward_request(client, remote, buffered_body, body, &context).await
        }
    }
}

async fn relay_forward_request(
    client: TcpStream,
    remote: StackTcpStream,
    buffered_body: Vec<u8>,
    body: BodyFraming,
    context: &HttpContext,
) -> Result<(), TransportError> {
    let (client_read, mut client_write) = tokio::io::split(client);
    let (mut remote_read, mut remote_write) = tokio::io::split(remote);
    let initial = Cursor::new(buffered_body);
    let mut source = BufReader::new(initial.chain(client_read));

    let upload = async {
        match body {
            BodyFraming::None => {}
            BodyFraming::ContentLength(length) => {
                let mut body = (&mut source).take(length);
                let copied = tokio::io::copy(&mut body, &mut remote_write)
                    .await
                    .map_err(|error| TransportError::HttpProxy(error.to_string()))?;
                if copied != length {
                    return Err(TransportError::HttpProxy(
                        "HTTP request body ended before Content-Length".to_owned(),
                    ));
                }
            }
            BodyFraming::Chunked => {
                relay_chunked_body(&mut source, &mut remote_write).await?;
            }
        }
        remote_write
            .shutdown()
            .await
            .map_err(|error| TransportError::HttpProxy(error.to_string()))
    };
    let download = async {
        tokio::io::copy(&mut remote_read, &mut client_write)
            .await
            .map_err(|error| TransportError::HttpProxy(error.to_string()))?;
        client_write
            .shutdown()
            .await
            .map_err(|error| TransportError::HttpProxy(error.to_string()))
    };

    tokio::select! {
        _ = context.cancellation.cancelled() => Ok(()),
        result = async { tokio::try_join!(upload, download).map(|_| ()) } => result,
    }
}

async fn relay_chunked_body<R, W>(source: &mut R, destination: &mut W) -> Result<(), TransportError>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    loop {
        let line = read_bounded_line(source).await?;
        let size = parse_chunk_size(&line)?;
        destination.write_all(&line).await?;
        if size == 0 {
            loop {
                let trailer = read_bounded_line(source).await?;
                destination.write_all(&trailer).await?;
                if trailer == b"\r\n" {
                    return Ok(());
                }
            }
        }

        let mut chunk = (&mut *source).take(size);
        let copied = tokio::io::copy(&mut chunk, &mut *destination).await?;
        if copied != size {
            return Err(TransportError::HttpProxy(
                "chunked HTTP body ended inside a chunk".to_owned(),
            ));
        }
        let mut terminator = [0u8; 2];
        source.read_exact(&mut terminator).await?;
        if terminator != *b"\r\n" {
            return Err(TransportError::HttpProxy(
                "invalid HTTP chunk terminator".to_owned(),
            ));
        }
        destination.write_all(&terminator).await?;
    }
}

async fn read_bounded_line<R>(source: &mut R) -> Result<Vec<u8>, TransportError>
where
    R: AsyncRead + Unpin,
{
    let mut line = Vec::with_capacity(64);
    while line.len() <= MAX_CHUNK_LINE_BYTES {
        let byte = source.read_u8().await?;
        line.push(byte);
        if byte == b'\n' {
            if line.len() < 2 || line[line.len() - 2] != b'\r' {
                return Err(TransportError::HttpProxy(
                    "HTTP chunk line is not CRLF-terminated".to_owned(),
                ));
            }
            return Ok(line);
        }
    }
    Err(TransportError::HttpProxy(
        "HTTP chunk line exceeds the safety limit".to_owned(),
    ))
}

fn parse_chunk_size(line: &[u8]) -> Result<u64, TransportError> {
    let text = std::str::from_utf8(
        line.strip_suffix(b"\r\n")
            .ok_or_else(|| TransportError::HttpProxy("invalid HTTP chunk line".to_owned()))?,
    )
    .map_err(|_| TransportError::HttpProxy("non-UTF-8 HTTP chunk line".to_owned()))?;
    let size = text.split(';').next().unwrap_or_default().trim();
    if size.is_empty() || size.len() > 16 {
        return Err(TransportError::HttpProxy(
            "invalid HTTP chunk size".to_owned(),
        ));
    }
    u64::from_str_radix(size, 16)
        .map_err(|_| TransportError::HttpProxy("invalid HTTP chunk size".to_owned()))
}

async fn read_request_head(client: &mut TcpStream) -> Result<(Vec<u8>, Vec<u8>), TransportError> {
    let mut bytes = Vec::with_capacity(4096);
    loop {
        if let Some(end) = find_header_end(&bytes) {
            let body = bytes.split_off(end);
            return Ok((bytes, body));
        }
        if bytes.len() >= MAX_HEADER_BYTES {
            return Err(TransportError::HttpProxy(
                "HTTP proxy request headers exceed the safety limit".to_owned(),
            ));
        }
        let read = client
            .read_buf(&mut bytes)
            .await
            .map_err(TransportError::Io)?;
        if read == 0 {
            return Err(TransportError::HttpProxy(
                "HTTP proxy client closed before finishing headers".to_owned(),
            ));
        }
        if bytes.len() > MAX_HEADER_BYTES {
            return Err(TransportError::HttpProxy(
                "HTTP proxy request headers exceed the safety limit".to_owned(),
            ));
        }
    }
}

fn find_header_end(bytes: &[u8]) -> Option<usize> {
    bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| index + 4)
}

struct ProxyRequest {
    destination: Destination,
    kind: RequestKind,
}

struct Destination {
    host: String,
    port: u16,
    authority: String,
}

enum RequestKind {
    Connect,
    Forward {
        rewritten_head: Vec<u8>,
        body: BodyFraming,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BodyFraming {
    None,
    ContentLength(u64),
    Chunked,
}

#[derive(Debug)]
struct RequestError {
    status: u16,
    reason: &'static str,
    message: String,
}

impl RequestError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: 400,
            reason: "Bad Request",
            message: message.into(),
        }
    }

    fn not_implemented(message: impl Into<String>) -> Self {
        Self {
            status: 501,
            reason: "Not Implemented",
            message: message.into(),
        }
    }
}

fn parse_proxy_request(head: &[u8]) -> Result<ProxyRequest, RequestError> {
    let mut headers = [httparse::EMPTY_HEADER; MAX_HEADERS];
    let mut request = httparse::Request::new(&mut headers);
    let parsed = request
        .parse(head)
        .map_err(|error| RequestError::bad_request(format!("parse HTTP request: {error}")))?;
    if !parsed.is_complete() {
        return Err(RequestError::bad_request("incomplete HTTP request"));
    }
    let method = request
        .method
        .ok_or_else(|| RequestError::bad_request("HTTP method is missing"))?;
    let target = request
        .path
        .ok_or_else(|| RequestError::bad_request("HTTP request target is missing"))?;
    let version = request
        .version
        .ok_or_else(|| RequestError::bad_request("HTTP version is missing"))?;
    if version > 1 {
        return Err(RequestError::bad_request(
            "only HTTP/1.0 and HTTP/1.1 are supported",
        ));
    }

    let metadata = inspect_headers(request.headers)?;
    if method.eq_ignore_ascii_case("CONNECT") {
        if metadata.body != BodyFraming::None {
            return Err(RequestError::bad_request(
                "CONNECT requests cannot contain a framed request body",
            ));
        }
        return Ok(ProxyRequest {
            destination: parse_destination(target, 443)?,
            kind: RequestKind::Connect,
        });
    }

    let (destination, origin_form) = parse_forward_target(target, metadata.host.as_deref())?;
    let rewritten_head = rewrite_forward_head(
        method,
        version,
        &origin_form,
        &destination.authority,
        request.headers,
        &metadata,
    )?;
    Ok(ProxyRequest {
        destination,
        kind: RequestKind::Forward {
            rewritten_head,
            body: metadata.body,
        },
    })
}

struct HeaderMetadata {
    host: Option<String>,
    body: BodyFraming,
    connection_tokens: HashSet<String>,
}

fn inspect_headers(headers: &[httparse::Header<'_>]) -> Result<HeaderMetadata, RequestError> {
    let mut hosts = Vec::new();
    let mut content_lengths = Vec::new();
    let mut transfer_encodings = Vec::new();
    let mut connection_tokens = HashSet::new();

    for header in headers {
        let value = std::str::from_utf8(header.value)
            .map_err(|_| RequestError::bad_request("HTTP header value is not UTF-8"))?
            .trim();
        match header.name.to_ascii_lowercase().as_str() {
            "host" => hosts.push(value.to_owned()),
            "content-length" => {
                let length = value
                    .parse::<u64>()
                    .map_err(|_| RequestError::bad_request("invalid HTTP Content-Length header"))?;
                content_lengths.push(length);
            }
            "transfer-encoding" => {
                transfer_encodings.extend(
                    value
                        .split(',')
                        .map(|token| token.trim().to_ascii_lowercase()),
                );
            }
            "connection" => {
                connection_tokens.extend(value.split(',').filter_map(normalized_header_token));
            }
            _ => {}
        }
    }

    if hosts
        .windows(2)
        .any(|pair| !pair[0].eq_ignore_ascii_case(&pair[1]))
    {
        return Err(RequestError::bad_request("conflicting HTTP Host headers"));
    }
    if content_lengths.windows(2).any(|pair| pair[0] != pair[1]) {
        return Err(RequestError::bad_request(
            "conflicting HTTP Content-Length headers",
        ));
    }
    if !content_lengths.is_empty() && !transfer_encodings.is_empty() {
        return Err(RequestError::bad_request(
            "Content-Length and Transfer-Encoding cannot be combined",
        ));
    }
    let body = if !transfer_encodings.is_empty() {
        if transfer_encodings != ["chunked"] {
            return Err(RequestError::not_implemented(
                "only a single chunked Transfer-Encoding is supported",
            ));
        }
        BodyFraming::Chunked
    } else {
        match content_lengths.first().copied().unwrap_or(0) {
            0 => BodyFraming::None,
            length => BodyFraming::ContentLength(length),
        }
    };

    Ok(HeaderMetadata {
        host: hosts.first().cloned(),
        body,
        connection_tokens,
    })
}

fn normalized_header_token(value: &str) -> Option<String> {
    let token = value.trim().to_ascii_lowercase();
    if token.is_empty()
        || !token
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte))
    {
        None
    } else {
        Some(token)
    }
}

fn parse_forward_target(
    target: &str,
    host_header: Option<&str>,
) -> Result<(Destination, String), RequestError> {
    if target.starts_with('/') || target == "*" {
        let host = host_header.ok_or_else(|| {
            RequestError::bad_request("origin-form proxy request requires a Host header")
        })?;
        return Ok((parse_destination(host, 80)?, target.to_owned()));
    }

    let uri = http::Uri::from_str(target)
        .map_err(|_| RequestError::bad_request("invalid absolute-form request target"))?;
    let scheme = uri
        .scheme_str()
        .ok_or_else(|| RequestError::bad_request("proxy target must be absolute-form"))?;
    if scheme != "http" {
        return Err(RequestError::not_implemented(
            "ordinary forwarding supports only http://; use CONNECT for TLS",
        ));
    }
    let authority = uri
        .authority()
        .ok_or_else(|| RequestError::bad_request("absolute URI has no authority"))?
        .as_str();
    let destination = parse_destination(authority, 80)?;
    let origin_form = uri
        .path_and_query()
        .map(|value| value.as_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("/")
        .to_owned();
    Ok((destination, origin_form))
}

fn parse_destination(value: &str, default_port: u16) -> Result<Destination, RequestError> {
    if value.contains('@') {
        return Err(RequestError::bad_request(
            "userinfo is not allowed in proxy authorities",
        ));
    }
    let authority = Authority::from_str(value)
        .map_err(|_| RequestError::bad_request("invalid proxy authority"))?;
    let host = authority.host();
    if host.is_empty() {
        return Err(RequestError::bad_request("proxy authority host is empty"));
    }
    let port = authority.port_u16().unwrap_or(default_port);
    if port == 0 {
        return Err(RequestError::bad_request("proxy target port is zero"));
    }
    Ok(Destination {
        host: host.to_owned(),
        port,
        authority: authority.to_string(),
    })
}

fn rewrite_forward_head(
    method: &str,
    version: u8,
    origin_form: &str,
    authority: &str,
    headers: &[httparse::Header<'_>],
    metadata: &HeaderMetadata,
) -> Result<Vec<u8>, RequestError> {
    let mut rewritten = Vec::with_capacity(1024);
    rewritten.extend_from_slice(method.as_bytes());
    rewritten.push(b' ');
    rewritten.extend_from_slice(origin_form.as_bytes());
    rewritten.extend_from_slice(if version == 0 {
        b" HTTP/1.0\r\n"
    } else {
        b" HTTP/1.1\r\n"
    });
    rewritten.extend_from_slice(b"Host: ");
    rewritten.extend_from_slice(authority.as_bytes());
    rewritten.extend_from_slice(b"\r\n");

    for header in headers {
        let name = header.name.to_ascii_lowercase();
        if matches!(
            name.as_str(),
            "host"
                | "connection"
                | "proxy-connection"
                | "proxy-authorization"
                | "keep-alive"
                | "upgrade"
                | "content-length"
                | "transfer-encoding"
        ) || metadata.connection_tokens.contains(&name)
        {
            continue;
        }
        rewritten.extend_from_slice(header.name.as_bytes());
        rewritten.extend_from_slice(b": ");
        rewritten.extend_from_slice(header.value);
        rewritten.extend_from_slice(b"\r\n");
    }
    match metadata.body {
        BodyFraming::None => {}
        BodyFraming::ContentLength(length) => {
            rewritten.extend_from_slice(format!("Content-Length: {length}\r\n").as_bytes());
        }
        BodyFraming::Chunked => {
            rewritten.extend_from_slice(b"Transfer-Encoding: chunked\r\n");
        }
    }
    rewritten.extend_from_slice(b"Connection: close\r\n\r\n");
    if rewritten.len() > MAX_HEADER_BYTES {
        return Err(RequestError::bad_request(
            "rewritten HTTP request exceeds the safety limit",
        ));
    }
    Ok(rewritten)
}

async fn connect_remote(
    context: &HttpContext,
    addresses: &[IpAddr],
    port: u16,
) -> Result<StackTcpStream, String> {
    let mut failures = Vec::new();
    for address in addresses
        .iter()
        .filter(|address| allows_address(context.ip_policy, **address))
        .take(MAX_TARGET_ADDRESSES)
    {
        let local_ip = match address {
            IpAddr::V4(_) => IpAddr::V4(context.assigned_ipv4),
            IpAddr::V6(_) => IpAddr::V6(context.assigned_ipv6),
        };
        let local = SocketAddr::new(local_ip, next_tcp_port());
        let remote = SocketAddr::new(*address, port);
        match timeout(
            REMOTE_CONNECT_TIMEOUT,
            context.channel.tcp_connect(local, remote),
        )
        .await
        {
            Ok(Ok(stream)) => return Ok(stream),
            Ok(Err(error)) => failures.push(format!("{remote}: {error}")),
            Err(_) => failures.push(format!("{remote}: timed out")),
        }
    }
    Err(if failures.is_empty() {
        "no usable target address".to_owned()
    } else {
        failures.join("; ")
    })
}

fn allows_address(policy: IpPolicy, address: IpAddr) -> bool {
    match policy {
        IpPolicy::Ipv4Only => address.is_ipv4(),
        IpPolicy::Ipv6Only => address.is_ipv6(),
        _ => true,
    }
}

fn next_tcp_port() -> u16 {
    NEXT_TCP_PORT
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
            Some(if value >= 65_534 { 49_152 } else { value + 1 })
        })
        .unwrap_or(49_152)
}

async fn send_error(
    client: &mut TcpStream,
    status: u16,
    reason: &str,
) -> Result<(), std::io::Error> {
    let body = format!("{status} {reason}\n");
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    client.write_all(response.as_bytes()).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_connect_and_absolute_forward_targets() {
        let connect =
            parse_proxy_request(b"CONNECT example.com:443 HTTP/1.1\r\nHost: ignored\r\n\r\n")
                .unwrap();
        assert!(matches!(connect.kind, RequestKind::Connect));
        assert_eq!(connect.destination.host, "example.com");
        assert_eq!(connect.destination.port, 443);

        let forward = parse_proxy_request(
            b"GET http://example.com:8080/a?q=1 HTTP/1.1\r\nHost: wrong.example\r\nProxy-Connection: keep-alive\r\n\r\n",
        )
        .unwrap();
        assert_eq!(forward.destination.host, "example.com");
        assert_eq!(forward.destination.port, 8080);
        let RequestKind::Forward { rewritten_head, .. } = forward.kind else {
            panic!("expected forward request");
        };
        let rewritten = String::from_utf8(rewritten_head).unwrap();
        assert!(rewritten.starts_with("GET /a?q=1 HTTP/1.1\r\n"));
        assert!(rewritten.contains("Host: example.com:8080\r\n"));
        assert!(!rewritten.to_ascii_lowercase().contains("proxy-connection"));
        assert!(rewritten.ends_with("Connection: close\r\n\r\n"));
    }

    #[test]
    fn rejects_request_smuggling_framing() {
        let error = parse_proxy_request(
            b"POST http://example.com/ HTTP/1.1\r\nHost: example.com\r\nContent-Length: 4\r\nTransfer-Encoding: chunked\r\n\r\n",
        )
        .err()
        .expect("ambiguous framing must fail");
        assert_eq!(error.status, 400);

        let error = parse_proxy_request(
            b"POST / HTTP/1.1\r\nHost: example.com\r\nContent-Length: 4\r\nContent-Length: 5\r\n\r\n",
        )
        .err()
        .expect("conflicting lengths must fail");
        assert_eq!(error.status, 400);
    }

    #[test]
    fn parses_chunk_sizes_with_extensions_but_bounds_them() {
        assert_eq!(parse_chunk_size(b"a;name=value\r\n").unwrap(), 10);
        assert!(parse_chunk_size(b"\r\n").is_err());
        assert!(parse_chunk_size(b"1234567890abcdef0\r\n").is_err());
    }

    #[test]
    fn header_end_is_exact() {
        assert_eq!(find_header_end(b"GET / HTTP/1.1\r\n\r\nbody"), Some(18));
        assert_eq!(find_header_end(b"GET / HTTP/1.1\r\n"), None);
    }
}
