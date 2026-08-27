use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket as StdUdpSocket};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpSocket, UdpSocket};
use tokio::sync::{Semaphore, mpsc};
use tokio::task::JoinHandle;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;
use ts_netstack_smoltcp::CreateSocket;
use ts_netstack_smoltcp::netcore::Channel;
use ts_netstack_smoltcp::netsock::{TcpListener as StackTcpListener, UdpSocket as StackUdpSocket};

use crate::geo_direct::{GeoDirectPolicy, GeoRoute};
use crate::port_allocator::{next_tcp_port, next_udp_port};
use crate::socket::{DirectProtocol, SocketProtector, socket_handle};

pub const SPLIT_DNS_IPV4: Ipv4Addr = Ipv4Addr::new(198, 18, 0, 1);
pub const SPLIT_DNS_IPV6: Ipv6Addr = Ipv6Addr::new(0xfd00, 0, 0, 0, 0, 0, 0, 1);

const DNS_PORT: u16 = 53;
const DNS_TIMEOUT: Duration = Duration::from_secs(4);
const MAX_IN_FLIGHT: usize = 512;
const MAX_UDP_MESSAGE: usize = 4096;
const MAX_TCP_MESSAGE: usize = u16::MAX as usize;
const MAX_QUESTIONS: usize = 8;
const MAX_NAME_JUMPS: usize = 32;
const MAX_CNAME_DEPTH: usize = 16;
const MAX_RESOURCE_RECORDS: usize = 1024;
const MAX_HINTS: usize = 8192;
const MAX_HINT_TTL: Duration = Duration::from_secs(60 * 60);
const RCODE_FORMERR: u16 = 1;
const RCODE_SERVFAIL: u16 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum QueryTransport {
    Udp,
    Tcp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum QueryRoute {
    Direct,
    Tunnel,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Question {
    name: String,
    query_type: u16,
    query_class: u16,
}

#[derive(Clone, Debug)]
struct ParsedQuery {
    id: u16,
    flags: u16,
    questions: Vec<Question>,
    question_end: usize,
}

#[derive(Clone, Copy, Debug)]
struct HintState {
    direct_until: Option<Instant>,
    tunnel_until: Option<Instant>,
    last_seen: Instant,
}

#[derive(Debug, Default)]
struct HintTable {
    generation: Option<u64>,
    hints: HashMap<IpAddr, HintState>,
}

/// Bounded, non-persistent DNS-to-IP route hints shared by Split DNS and TUN
/// routing. A conflicting direct/tunnel observation deliberately falls back to
/// GeoIP rather than guessing which hostname an IP flow belongs to.
#[derive(Debug, Default)]
pub(crate) struct DnsRouteCache {
    inner: Mutex<HintTable>,
}

impl DnsRouteCache {
    pub(crate) fn route_ip(
        &self,
        ip: IpAddr,
        generation: Option<u64>,
        policy: &GeoDirectPolicy,
    ) -> GeoRoute {
        let now = Instant::now();
        let mut table = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        sync_generation(&mut table, generation);
        prune_hints(&mut table, now);
        let dns_direct = table.hints.get(&ip).is_some_and(|hint| {
            hint.direct_until.is_some_and(|until| until > now)
                && !hint.tunnel_until.is_some_and(|until| until > now)
        });
        if dns_direct {
            GeoRoute::Direct
        } else {
            policy.route_ip(ip)
        }
    }

    fn observe(
        &self,
        response: &[u8],
        query: &ParsedQuery,
        route: QueryRoute,
        generation: Option<u64>,
    ) {
        let Ok(records) = response_hints(response, query) else {
            return;
        };
        let now = Instant::now();
        let mut table = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        sync_generation(&mut table, generation);
        prune_hints(&mut table, now);
        for (ip, ttl) in records {
            if ttl == 0 {
                continue;
            }
            if table.hints.len() >= MAX_HINTS
                && !table.hints.contains_key(&ip)
                && let Some(oldest) = table
                    .hints
                    .iter()
                    .min_by_key(|(_, hint)| hint.last_seen)
                    .map(|(ip, _)| *ip)
            {
                table.hints.remove(&oldest);
            }
            let until = now + Duration::from_secs(u64::from(ttl)).min(MAX_HINT_TTL);
            let hint = table.hints.entry(ip).or_insert(HintState {
                direct_until: None,
                tunnel_until: None,
                last_seen: now,
            });
            match route {
                QueryRoute::Direct => hint.direct_until = Some(until),
                QueryRoute::Tunnel => hint.tunnel_until = Some(until),
            }
            hint.last_seen = now;
        }
    }
}

fn sync_generation(table: &mut HintTable, generation: Option<u64>) {
    if table.generation != generation {
        table.generation = generation;
        table.hints.clear();
    }
}

fn prune_hints(table: &mut HintTable, now: Instant) {
    table.hints.retain(|_, hint| {
        hint.direct_until.is_some_and(|until| until > now)
            || hint.tunnel_until.is_some_and(|until| until > now)
    });
}

#[derive(Clone)]
struct SplitDnsResolver {
    tunnel_channel: Channel,
    assigned_ipv4: Ipv4Addr,
    assigned_ipv6: Ipv6Addr,
    tunnel_servers: Vec<SocketAddr>,
    policy: Arc<GeoDirectPolicy>,
    protector: Arc<dyn SocketProtector>,
    hints: Arc<DnsRouteCache>,
    permits: Arc<Semaphore>,
}

impl SplitDnsResolver {
    async fn handle(&self, query_bytes: &[u8], transport: QueryTransport) -> Vec<u8> {
        let network_generation = self.protector.network_generation();
        let query = match parse_query(query_bytes) {
            Ok(query) => query,
            Err(_) => return error_response(query_bytes, RCODE_FORMERR),
        };
        let route = match classify_query(&query, &self.policy) {
            Ok(route) => route,
            Err(()) => return error_response(query_bytes, RCODE_SERVFAIL),
        };
        let Ok(_permit) = self.permits.clone().try_acquire_owned() else {
            return error_response(query_bytes, RCODE_SERVFAIL);
        };

        let response = timeout(DNS_TIMEOUT, async {
            match route {
                QueryRoute::Direct => self.query_direct(query_bytes, &query, transport).await,
                QueryRoute::Tunnel => self.query_tunnel(query_bytes, &query, transport).await,
            }
        })
        .await
        .map_err(|_| "complete DNS query timed out".to_owned())
        .and_then(|result| result);
        match response {
            Ok(response) => {
                if self.protector.network_generation() != network_generation {
                    return error_response(query_bytes, RCODE_SERVFAIL);
                }
                self.hints
                    .observe(&response, &query, route, network_generation);
                response
            }
            Err(error) => {
                tracing::debug!(%error, ?route, "Split DNS query failed");
                error_response(query_bytes, RCODE_SERVFAIL)
            }
        }
    }

    async fn query_direct(
        &self,
        query_bytes: &[u8],
        query: &ParsedQuery,
        transport: QueryTransport,
    ) -> Result<Vec<u8>, String> {
        let servers = self.protector.physical_dns_servers();
        if servers.is_empty() {
            return Err("physical network supplied no DNS server".to_owned());
        }
        self.query_servers(query_bytes, query, transport, &servers, true)
            .await
    }

    async fn query_tunnel(
        &self,
        query_bytes: &[u8],
        query: &ParsedQuery,
        transport: QueryTransport,
    ) -> Result<Vec<u8>, String> {
        self.query_servers(query_bytes, query, transport, &self.tunnel_servers, false)
            .await
    }

    async fn query_servers(
        &self,
        query_bytes: &[u8],
        query: &ParsedQuery,
        transport: QueryTransport,
        servers: &[SocketAddr],
        direct: bool,
    ) -> Result<Vec<u8>, String> {
        let mut failures = Vec::new();
        for server in servers {
            let response = match (transport, direct) {
                (QueryTransport::Udp, true) => {
                    direct_udp(self.protector.as_ref(), *server, query_bytes).await
                }
                (QueryTransport::Tcp, true) => {
                    direct_tcp(self.protector.as_ref(), *server, query_bytes).await
                }
                (QueryTransport::Udp, false) => self.tunnel_udp(*server, query_bytes).await,
                (QueryTransport::Tcp, false) => self.tunnel_tcp(*server, query_bytes).await,
            };
            let mut response = match response {
                Ok(response) => response,
                Err(error) => {
                    failures.push(format!("{server}: {error}"));
                    continue;
                }
            };
            if let Err(error) = validate_response(query, &response) {
                failures.push(format!("{server}: {error}"));
                continue;
            }
            if transport == QueryTransport::Udp && response_is_truncated(&response) {
                let retry = if direct {
                    direct_tcp(self.protector.as_ref(), *server, query_bytes).await
                } else {
                    self.tunnel_tcp(*server, query_bytes).await
                };
                response = match retry {
                    Ok(response) => response,
                    Err(error) => {
                        failures.push(format!("{server}: TCP retry failed: {error}"));
                        continue;
                    }
                };
                if let Err(error) = validate_response(query, &response) {
                    failures.push(format!("{server}: TCP retry validation failed: {error}"));
                    continue;
                }
            }
            if let Err(error) = response_hints(&response, query) {
                failures.push(format!("{server}: {error}"));
                continue;
            }
            if transport == QueryTransport::Udp && response.len() > MAX_UDP_MESSAGE {
                return Ok(truncated_response(query_bytes));
            }
            return Ok(response);
        }
        Err(if failures.is_empty() {
            "no DNS server matches an available address family".to_owned()
        } else {
            failures.join("; ")
        })
    }

    async fn tunnel_udp(&self, server: SocketAddr, query: &[u8]) -> Result<Vec<u8>, String> {
        let local = SocketAddr::new(
            if server.is_ipv4() {
                IpAddr::V4(self.assigned_ipv4)
            } else {
                IpAddr::V6(self.assigned_ipv6)
            },
            next_udp_port(),
        );
        let socket = self
            .tunnel_channel
            .udp_bind(local)
            .await
            .map_err(|error| error.to_string())?;
        socket
            .send_to(server, query)
            .await
            .map_err(|error| error.to_string())?;
        let (source, response) = timeout(DNS_TIMEOUT, socket.recv_from_bytes())
            .await
            .map_err(|_| "timed out".to_owned())?
            .map_err(|error| error.to_string())?;
        if source != server {
            return Err(format!("response came from {source}"));
        }
        Ok(response.to_vec())
    }

    async fn tunnel_tcp(&self, server: SocketAddr, query: &[u8]) -> Result<Vec<u8>, String> {
        let local = SocketAddr::new(
            if server.is_ipv4() {
                IpAddr::V4(self.assigned_ipv4)
            } else {
                IpAddr::V6(self.assigned_ipv6)
            },
            next_tcp_port(),
        );
        let stream = timeout(DNS_TIMEOUT, self.tunnel_channel.tcp_connect(local, server))
            .await
            .map_err(|_| "connect timed out".to_owned())?
            .map_err(|error| error.to_string())?;
        exchange_tcp(stream, query).await
    }
}

pub(crate) struct SplitDnsRuntime {
    pub(crate) hints: Arc<DnsRouteCache>,
    tasks: Vec<JoinHandle<()>>,
}

pub(crate) struct SplitDnsConfig {
    tunnel_channel: Channel,
    assigned_addresses: (Ipv4Addr, Ipv6Addr),
    tunnel_dns_servers: Vec<IpAddr>,
    policy: Arc<GeoDirectPolicy>,
    protector: Arc<dyn SocketProtector>,
}

impl SplitDnsConfig {
    pub(crate) fn new(
        tunnel_channel: Channel,
        assigned_addresses: (Ipv4Addr, Ipv6Addr),
        tunnel_dns_servers: &[IpAddr],
        policy: Arc<GeoDirectPolicy>,
        protector: Arc<dyn SocketProtector>,
    ) -> Self {
        Self {
            tunnel_channel,
            assigned_addresses,
            tunnel_dns_servers: tunnel_dns_servers.to_vec(),
            policy,
            protector,
        }
    }
}

impl SplitDnsRuntime {
    pub(crate) async fn start(
        internal_channel: &Channel,
        config: SplitDnsConfig,
        cancellation: &CancellationToken,
    ) -> Result<Self, String> {
        if config.tunnel_dns_servers.is_empty() {
            return Err("the WARP DNS server list is empty".to_owned());
        }
        if config.protector.physical_dns_servers().is_empty() {
            return Err("the selected physical network has no DNS server".to_owned());
        }
        let udp_v4 = internal_channel
            .udp_bind(SocketAddr::new(SPLIT_DNS_IPV4.into(), DNS_PORT))
            .await
            .map_err(|error| error.to_string())?;
        let udp_v6 = internal_channel
            .udp_bind(SocketAddr::new(SPLIT_DNS_IPV6.into(), DNS_PORT))
            .await
            .map_err(|error| error.to_string())?;
        let tcp_v4 = internal_channel
            .tcp_listen(SocketAddr::new(SPLIT_DNS_IPV4.into(), DNS_PORT))
            .await
            .map_err(|error| error.to_string())?;
        let tcp_v6 = internal_channel
            .tcp_listen(SocketAddr::new(SPLIT_DNS_IPV6.into(), DNS_PORT))
            .await
            .map_err(|error| error.to_string())?;

        let hints = Arc::new(DnsRouteCache::default());
        let resolver = SplitDnsResolver {
            tunnel_channel: config.tunnel_channel,
            assigned_ipv4: config.assigned_addresses.0,
            assigned_ipv6: config.assigned_addresses.1,
            tunnel_servers: config
                .tunnel_dns_servers
                .into_iter()
                .map(|server| SocketAddr::new(server, DNS_PORT))
                .collect(),
            policy: config.policy,
            protector: config.protector,
            hints: Arc::clone(&hints),
            permits: Arc::new(Semaphore::new(MAX_IN_FLIGHT)),
        };
        let tasks = vec![
            tokio::spawn(run_udp_server(
                udp_v4,
                resolver.clone(),
                cancellation.child_token(),
            )),
            tokio::spawn(run_udp_server(
                udp_v6,
                resolver.clone(),
                cancellation.child_token(),
            )),
            tokio::spawn(run_tcp_server(
                tcp_v4,
                resolver.clone(),
                cancellation.child_token(),
            )),
            tokio::spawn(run_tcp_server(tcp_v6, resolver, cancellation.child_token())),
        ];
        Ok(Self { hints, tasks })
    }
}

impl Drop for SplitDnsRuntime {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}

async fn run_udp_server(
    socket: StackUdpSocket,
    resolver: SplitDnsResolver,
    cancellation: CancellationToken,
) {
    let (responses_tx, mut responses_rx) = mpsc::channel::<(SocketAddr, Vec<u8>)>(MAX_IN_FLIGHT);
    loop {
        tokio::select! {
            _ = cancellation.cancelled() => break,
            response = responses_rx.recv() => {
                let Some((client, response)) = response else { break; };
                let _ = socket.send_to(client, &response).await;
            }
            query = socket.recv_from_bytes() => {
                let Ok((client, query)) = query else { break; };
                if query.len() > MAX_UDP_MESSAGE {
                    let _ = socket.send_to(client, &error_response(&query, RCODE_FORMERR)).await;
                    continue;
                }
                let resolver = resolver.clone();
                let responses = responses_tx.clone();
                tokio::spawn(async move {
                    let response = resolver.handle(&query, QueryTransport::Udp).await;
                    let _ = responses.send((client, response)).await;
                });
            }
        }
    }
}

async fn run_tcp_server(
    listener: StackTcpListener,
    resolver: SplitDnsResolver,
    cancellation: CancellationToken,
) {
    loop {
        let accepted = tokio::select! {
            _ = cancellation.cancelled() => break,
            accepted = listener.accept() => accepted,
        };
        let Ok(mut stream) = accepted else {
            break;
        };
        let resolver = resolver.clone();
        let child = cancellation.child_token();
        tokio::spawn(async move {
            loop {
                let length = tokio::select! {
                    _ = child.cancelled() => return,
                    length = stream.read_u16() => match length {
                        Ok(length) => usize::from(length),
                        Err(_) => return,
                    },
                };
                if length == 0 || length > MAX_TCP_MESSAGE {
                    return;
                }
                let mut query = vec![0_u8; length];
                if stream.read_exact(&mut query).await.is_err() {
                    return;
                }
                let response = resolver.handle(&query, QueryTransport::Tcp).await;
                let Ok(response_len) = u16::try_from(response.len()) else {
                    return;
                };
                if stream.write_u16(response_len).await.is_err()
                    || stream.write_all(&response).await.is_err()
                {
                    return;
                }
            }
        });
    }
}

async fn direct_udp(
    protector: &dyn SocketProtector,
    server: SocketAddr,
    query: &[u8],
) -> Result<Vec<u8>, String> {
    let bind = SocketAddr::new(
        if server.is_ipv4() {
            IpAddr::V4(Ipv4Addr::UNSPECIFIED)
        } else {
            IpAddr::V6(Ipv6Addr::UNSPECIFIED)
        },
        0,
    );
    let socket = StdUdpSocket::bind(bind).map_err(|error| error.to_string())?;
    let _lease = protector
        .protect_for_target(socket_handle(&socket), server, DirectProtocol::Udp)
        .await
        .map_err(|error| format!("protect DNS UDP socket: {error}"))?;
    socket
        .set_nonblocking(true)
        .map_err(|error| error.to_string())?;
    let socket = UdpSocket::from_std(socket).map_err(|error| error.to_string())?;
    let sent = timeout(DNS_TIMEOUT, socket.send_to(query, server))
        .await
        .map_err(|_| "send timed out".to_owned())?
        .map_err(|error| error.to_string())?;
    if sent != query.len() {
        return Err(format!("sent {sent} of {} bytes", query.len()));
    }
    let mut response = vec![0_u8; MAX_UDP_MESSAGE];
    let (length, source) = timeout(DNS_TIMEOUT, socket.recv_from(&mut response))
        .await
        .map_err(|_| "receive timed out".to_owned())?
        .map_err(|error| error.to_string())?;
    if source != server {
        return Err(format!("response came from {source}"));
    }
    response.truncate(length);
    Ok(response)
}

async fn direct_tcp(
    protector: &dyn SocketProtector,
    server: SocketAddr,
    query: &[u8],
) -> Result<Vec<u8>, String> {
    let socket = if server.is_ipv4() {
        TcpSocket::new_v4()
    } else {
        TcpSocket::new_v6()
    }
    .map_err(|error| error.to_string())?;
    let _lease = protector
        .protect_for_target(socket_handle(&socket), server, DirectProtocol::Tcp)
        .await
        .map_err(|error| format!("protect DNS TCP socket: {error}"))?;
    let stream = timeout(DNS_TIMEOUT, socket.connect(server))
        .await
        .map_err(|_| "connect timed out".to_owned())?
        .map_err(|error| error.to_string())?;
    exchange_tcp(stream, query).await
}

async fn exchange_tcp<S>(mut stream: S, query: &[u8]) -> Result<Vec<u8>, String>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let length = u16::try_from(query.len()).map_err(|_| "DNS query is too large".to_owned())?;
    timeout(DNS_TIMEOUT, async {
        stream.write_u16(length).await?;
        stream.write_all(query).await?;
        stream.flush().await?;
        let response_len = usize::from(stream.read_u16().await?);
        if response_len == 0 || response_len > MAX_TCP_MESSAGE {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "invalid DNS TCP response length",
            ));
        }
        let mut response = vec![0_u8; response_len];
        stream.read_exact(&mut response).await?;
        Ok::<_, std::io::Error>(response)
    })
    .await
    .map_err(|_| "DNS TCP exchange timed out".to_owned())?
    .map_err(|error| error.to_string())
}

fn classify_query(query: &ParsedQuery, policy: &GeoDirectPolicy) -> Result<QueryRoute, ()> {
    let direct = query
        .questions
        .iter()
        .filter(|question| policy.route_host(&question.name) == GeoRoute::Direct)
        .count();
    if direct == query.questions.len() {
        Ok(QueryRoute::Direct)
    } else if direct == 0 {
        Ok(QueryRoute::Tunnel)
    } else {
        Err(())
    }
}

fn parse_query(packet: &[u8]) -> Result<ParsedQuery, String> {
    if packet.len() < 12 || packet.len() > MAX_TCP_MESSAGE {
        return Err("invalid DNS query length".to_owned());
    }
    let flags = read_u16(packet, 2)?;
    if flags & 0x8000 != 0 || flags & 0x7800 != 0 {
        return Err("not a standard DNS query".to_owned());
    }
    let question_count = usize::from(read_u16(packet, 4)?);
    if question_count == 0 || question_count > MAX_QUESTIONS {
        return Err("DNS question count is outside the supported bound".to_owned());
    }
    let (questions, question_end) = parse_questions(packet, question_count)?;
    let resource_count = usize::from(read_u16(packet, 6)?)
        .saturating_add(usize::from(read_u16(packet, 8)?))
        .saturating_add(usize::from(read_u16(packet, 10)?));
    if resource_count > MAX_RESOURCE_RECORDS {
        return Err("DNS query record count exceeds the supported bound".to_owned());
    }
    let mut end = question_end;
    for _ in 0..resource_count {
        end = skip_resource_record(packet, end)?;
    }
    if end != packet.len() {
        return Err("DNS query has trailing or unparsed bytes".to_owned());
    }
    Ok(ParsedQuery {
        id: read_u16(packet, 0)?,
        flags,
        questions,
        question_end,
    })
}

fn parse_questions(packet: &[u8], count: usize) -> Result<(Vec<Question>, usize), String> {
    let mut questions = Vec::with_capacity(count);
    let mut offset = 12;
    for _ in 0..count {
        let (name, next) = read_name(packet, offset)?;
        if next + 4 > packet.len() {
            return Err("truncated DNS question".to_owned());
        }
        questions.push(Question {
            name,
            query_type: read_u16(packet, next)?,
            query_class: read_u16(packet, next + 2)?,
        });
        offset = next + 4;
    }
    Ok((questions, offset))
}

fn validate_response(query: &ParsedQuery, response: &[u8]) -> Result<(), String> {
    if response.len() < 12 || response.len() > MAX_TCP_MESSAGE {
        return Err("invalid DNS response length".to_owned());
    }
    let response_flags = read_u16(response, 2)?;
    if read_u16(response, 0)? != query.id
        || response_flags & 0x8000 == 0
        || response_flags & 0x7800 != query.flags & 0x7800
    {
        return Err("DNS response header does not match query".to_owned());
    }
    let question_count = usize::from(read_u16(response, 4)?);
    if question_count != query.questions.len() {
        return Err("DNS response question count mismatch".to_owned());
    }
    let (questions, _) = parse_questions(response, question_count)?;
    if questions != query.questions {
        return Err("DNS response question mismatch".to_owned());
    }
    Ok(())
}

fn response_is_truncated(response: &[u8]) -> bool {
    read_u16(response, 2).is_ok_and(|flags| flags & 0x0200 != 0)
}

fn response_hints(response: &[u8], query: &ParsedQuery) -> Result<Vec<(IpAddr, u32)>, String> {
    validate_response(query, response)?;
    let answer_count = usize::from(read_u16(response, 6)?);
    let authority_count = usize::from(read_u16(response, 8)?);
    let additional_count = usize::from(read_u16(response, 10)?);
    if answer_count
        .saturating_add(authority_count)
        .saturating_add(additional_count)
        > MAX_RESOURCE_RECORDS
    {
        return Err("DNS response record count exceeds the supported bound".to_owned());
    }
    let (_, mut offset) = parse_questions(response, query.questions.len())?;
    let mut cnames: HashMap<String, (String, u32)> = HashMap::new();
    let mut addresses: HashMap<String, Vec<(IpAddr, u32)>> = HashMap::new();
    for _ in 0..answer_count {
        let (owner, next) = read_name(response, offset)?;
        if next + 10 > response.len() {
            return Err("truncated DNS answer".to_owned());
        }
        let record_type = read_u16(response, next)?;
        let class = read_u16(response, next + 2)?;
        let ttl = read_u32(response, next + 4)?;
        let data_len = usize::from(read_u16(response, next + 8)?);
        let data = next + 10;
        let end = data
            .checked_add(data_len)
            .filter(|end| *end <= response.len())
            .ok_or_else(|| "truncated DNS record data".to_owned())?;
        if class == 1 {
            match (record_type, data_len) {
                (1, 4) => {
                    addresses.entry(owner).or_default().push((
                        IpAddr::V4(Ipv4Addr::new(
                            response[data],
                            response[data + 1],
                            response[data + 2],
                            response[data + 3],
                        )),
                        ttl,
                    ));
                }
                (28, 16) => {
                    let octets: [u8; 16] = response[data..end]
                        .try_into()
                        .map_err(|_| "invalid AAAA record".to_owned())?;
                    addresses
                        .entry(owner)
                        .or_default()
                        .push((IpAddr::V6(Ipv6Addr::from(octets)), ttl));
                }
                (5, _) => {
                    let (target, consumed) = read_name(response, data)?;
                    if consumed != end {
                        return Err("CNAME data length does not match its encoded name".to_owned());
                    }
                    cnames.insert(owner, (target, ttl));
                }
                _ => {}
            }
        }
        offset = end;
    }
    let remaining = authority_count.saturating_add(additional_count);
    for _ in 0..remaining {
        offset = skip_resource_record(response, offset)?;
    }
    if offset != response.len() {
        return Err("DNS response has trailing or unparsed bytes".to_owned());
    }

    let mut hints = HashMap::<IpAddr, u32>::new();
    for question in &query.questions {
        let mut name = question.name.clone();
        let mut chain_ttl = u32::MAX;
        let mut seen = HashSet::new();
        for _ in 0..=MAX_CNAME_DEPTH {
            if !seen.insert(name.clone()) {
                break;
            }
            if let Some(values) = addresses.get(&name) {
                for (ip, ttl) in values {
                    let ttl = (*ttl).min(chain_ttl);
                    hints
                        .entry(*ip)
                        .and_modify(|existing| *existing = (*existing).max(ttl))
                        .or_insert(ttl);
                }
            }
            let Some((target, ttl)) = cnames.get(&name) else {
                break;
            };
            chain_ttl = chain_ttl.min(*ttl);
            name.clone_from(target);
        }
    }
    if read_u16(response, 2)? & 0x000f == 0 {
        Ok(hints.into_iter().collect())
    } else {
        Ok(Vec::new())
    }
}

fn skip_resource_record(packet: &[u8], offset: usize) -> Result<usize, String> {
    let next = skip_dns_name(packet, offset)?;
    if next + 10 > packet.len() {
        return Err("truncated DNS resource record".to_owned());
    }
    let data_len = usize::from(read_u16(packet, next + 8)?);
    (next + 10)
        .checked_add(data_len)
        .filter(|end| *end <= packet.len())
        .ok_or_else(|| "truncated DNS resource record data".to_owned())
}

fn skip_dns_name(packet: &[u8], start: usize) -> Result<usize, String> {
    let mut cursor = start;
    let mut next = None;
    let mut jumps = 0;
    loop {
        let length = *packet
            .get(cursor)
            .ok_or_else(|| "truncated DNS name".to_owned())?;
        if length & 0xc0 == 0xc0 {
            let second = *packet
                .get(cursor + 1)
                .ok_or_else(|| "truncated DNS compression pointer".to_owned())?;
            next.get_or_insert(cursor + 2);
            cursor = usize::from((u16::from(length & 0x3f) << 8) | u16::from(second));
            jumps += 1;
            if jumps > MAX_NAME_JUMPS {
                return Err("DNS compression pointer loop".to_owned());
            }
            continue;
        }
        if length & 0xc0 != 0 || length > 63 {
            return Err("invalid DNS label encoding".to_owned());
        }
        cursor += 1;
        if length == 0 {
            return Ok(next.unwrap_or(cursor));
        }
        cursor = cursor
            .checked_add(usize::from(length))
            .filter(|cursor| *cursor <= packet.len())
            .ok_or_else(|| "truncated DNS label".to_owned())?;
    }
}

/// Resolves a GeoSite-selected proxy hostname exclusively through the current
/// physical-network DNS servers. It shares the Split DNS validation, CNAME
/// reachability, truncation retry, timeout, and exact-egress lease behavior.
pub async fn resolve_physical_host(
    protector: &dyn SocketProtector,
    host: &str,
    port: u16,
) -> Result<Vec<SocketAddr>, String> {
    if port == 0 {
        return Err("physical DNS target port is zero".to_owned());
    }
    let generation = protector.network_generation();
    let servers = protector.physical_dns_servers();
    if servers.is_empty() {
        return Err("physical network supplied no DNS server".to_owned());
    }
    let mut addresses = Vec::new();
    let mut failures = Vec::new();
    for query_type in [1_u16, 28_u16] {
        let query_bytes = build_host_query(host, query_type, next_udp_port())?;
        let query = parse_query(&query_bytes)?;
        let mut answered = false;
        for server in &servers {
            let response = match direct_udp(protector, *server, &query_bytes).await {
                Ok(response) if response_is_truncated(&response) => {
                    direct_tcp(protector, *server, &query_bytes).await
                }
                result => result,
            };
            let response = match response {
                Ok(response) => response,
                Err(error) => {
                    failures.push(format!("{server}: {error}"));
                    continue;
                }
            };
            if protector.network_generation() != generation {
                return Err("physical network changed during DNS resolution".to_owned());
            }
            if let Err(error) = validate_response(&query, &response) {
                failures.push(format!("{server}: {error}"));
                continue;
            }
            let rcode = read_u16(&response, 2)? & 0x000f;
            if !matches!(rcode, 0 | 3) {
                failures.push(format!("{server}: DNS RCODE {rcode}"));
                continue;
            }
            addresses.extend(
                response_hints(&response, &query)?
                    .into_iter()
                    .map(|(address, _)| SocketAddr::new(address, port)),
            );
            answered = true;
            break;
        }
        if !answered && failures.is_empty() {
            failures.push(format!(
                "no physical DNS server answered QTYPE {query_type}"
            ));
        }
    }
    addresses.retain(|address| !address.ip().is_unspecified() && !address.ip().is_multicast());
    addresses.sort();
    addresses.dedup();
    addresses.truncate(16);
    if protector.network_generation() != generation {
        return Err("physical network changed during DNS resolution".to_owned());
    }
    if addresses.is_empty() {
        Err(if failures.is_empty() {
            format!("physical DNS returned no address for {host}")
        } else {
            failures.join("; ")
        })
    } else {
        Ok(addresses)
    }
}

fn build_host_query(host: &str, query_type: u16, id: u16) -> Result<Vec<u8>, String> {
    let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
    if host.is_empty() || host.len() > 253 || !matches!(query_type, 1 | 28) {
        return Err("invalid physical DNS hostname or query type".to_owned());
    }
    let mut query = Vec::with_capacity(12 + host.len() + 6);
    query.extend_from_slice(&id.to_be_bytes());
    query.extend_from_slice(&0x0100_u16.to_be_bytes());
    query.extend_from_slice(&1_u16.to_be_bytes());
    query.extend_from_slice(&[0_u8; 6]);
    for label in host.split('.') {
        if label.is_empty()
            || label.len() > 63
            || !label
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        {
            return Err("physical DNS hostname contains an invalid label".to_owned());
        }
        query.push(label.len() as u8);
        query.extend_from_slice(label.as_bytes());
    }
    query.push(0);
    query.extend_from_slice(&query_type.to_be_bytes());
    query.extend_from_slice(&1_u16.to_be_bytes());
    Ok(query)
}

fn read_name(packet: &[u8], start: usize) -> Result<(String, usize), String> {
    let mut labels = Vec::new();
    let mut cursor = start;
    let mut next = None;
    let mut jumps = 0;
    loop {
        let length = *packet
            .get(cursor)
            .ok_or_else(|| "truncated DNS name".to_owned())?;
        if length & 0xc0 == 0xc0 {
            let second = *packet
                .get(cursor + 1)
                .ok_or_else(|| "truncated DNS compression pointer".to_owned())?;
            next.get_or_insert(cursor + 2);
            cursor = usize::from((u16::from(length & 0x3f) << 8) | u16::from(second));
            jumps += 1;
            if jumps > MAX_NAME_JUMPS {
                return Err("DNS compression pointer loop".to_owned());
            }
            continue;
        }
        if length & 0xc0 != 0 {
            return Err("invalid DNS label encoding".to_owned());
        }
        cursor += 1;
        if length == 0 {
            let end = next.unwrap_or(cursor);
            let name = labels.join(".").to_ascii_lowercase();
            if name.is_empty() || name.len() > 253 {
                return Err("invalid DNS name".to_owned());
            }
            return Ok((name, end));
        }
        let length = usize::from(length);
        if length > 63 || cursor + length > packet.len() {
            return Err("truncated DNS label".to_owned());
        }
        let label = std::str::from_utf8(&packet[cursor..cursor + length])
            .map_err(|_| "non-ASCII DNS label".to_owned())?;
        if !label.is_ascii() {
            return Err("non-ASCII DNS label".to_owned());
        }
        labels.push(label.to_owned());
        cursor += length;
        if labels.len() > 127 {
            return Err("too many DNS labels".to_owned());
        }
    }
}

fn error_response(query: &[u8], rcode: u16) -> Vec<u8> {
    let Ok(parsed) = parse_query(query) else {
        let mut response = vec![0_u8; 12];
        if query.len() >= 2 {
            response[..2].copy_from_slice(&query[..2]);
        }
        response[2..4].copy_from_slice(&(0x8000 | rcode).to_be_bytes());
        return response;
    };
    let mut response = query[..parsed.question_end].to_vec();
    let flags = 0x8000 | 0x0080 | (parsed.flags & 0x0110) | rcode;
    response[2..4].copy_from_slice(&flags.to_be_bytes());
    response[6..12].fill(0);
    response
}

fn truncated_response(query: &[u8]) -> Vec<u8> {
    let mut response = error_response(query, 0);
    if response.len() >= 4 {
        let flags = u16::from_be_bytes([response[2], response[3]]) | 0x0200;
        response[2..4].copy_from_slice(&flags.to_be_bytes());
    }
    response
}

fn read_u16(packet: &[u8], offset: usize) -> Result<u16, String> {
    let bytes = packet
        .get(offset..offset + 2)
        .ok_or_else(|| "truncated DNS field".to_owned())?;
    Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
}

fn read_u32(packet: &[u8], offset: usize) -> Result<u32, String> {
    let bytes = packet
        .get(offset..offset + 4)
        .ok_or_else(|| "truncated DNS field".to_owned())?;
    Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geo_direct::GeoDirectClassifier;
    use crate::socket::{SocketHandle, SocketProtector};
    use std::sync::atomic::{AtomicU64, Ordering};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use usque_geo::CountryCode;

    struct Classifier;

    impl GeoDirectClassifier for Classifier {
        fn host_matches(&self, host: &str, country: &CountryCode) -> bool {
            country.as_str() == "CN" && host.ends_with(".cn")
        }

        fn ip_matches(&self, ip: IpAddr, _country: &CountryCode) -> bool {
            ip == IpAddr::V4(Ipv4Addr::new(203, 0, 113, 9))
        }
    }

    fn policy() -> GeoDirectPolicy {
        GeoDirectPolicy::with_classifier(Arc::new(Classifier), [CountryCode::parse("CN").unwrap()])
    }

    fn query(id: u16, name: &str) -> Vec<u8> {
        let mut packet = Vec::new();
        packet.extend_from_slice(&id.to_be_bytes());
        packet.extend_from_slice(&0x0100_u16.to_be_bytes());
        packet.extend_from_slice(&1_u16.to_be_bytes());
        packet.extend_from_slice(&[0; 6]);
        for label in name.split('.') {
            packet.push(label.len() as u8);
            packet.extend_from_slice(label.as_bytes());
        }
        packet.push(0);
        packet.extend_from_slice(&1_u16.to_be_bytes());
        packet.extend_from_slice(&1_u16.to_be_bytes());
        packet
    }

    fn append_name(packet: &mut Vec<u8>, name: &str) {
        for label in name.split('.') {
            packet.push(label.len() as u8);
            packet.extend_from_slice(label.as_bytes());
        }
        packet.push(0);
    }

    fn mixed_query() -> Vec<u8> {
        let mut packet = Vec::new();
        packet.extend_from_slice(&21_u16.to_be_bytes());
        packet.extend_from_slice(&0x0100_u16.to_be_bytes());
        packet.extend_from_slice(&2_u16.to_be_bytes());
        packet.extend_from_slice(&[0; 6]);
        for name in ["direct.example.cn", "tunnel.example.com"] {
            append_name(&mut packet, name);
            packet.extend_from_slice(&1_u16.to_be_bytes());
            packet.extend_from_slice(&1_u16.to_be_bytes());
        }
        packet
    }

    fn cname_response_with_unrelated_additional(query: &[u8]) -> Vec<u8> {
        let parsed = parse_query(query).unwrap();
        let mut response = query[..parsed.question_end].to_vec();
        response[2..4].copy_from_slice(&0x8180_u16.to_be_bytes());
        response[6..8].copy_from_slice(&2_u16.to_be_bytes());
        response[10..12].copy_from_slice(&1_u16.to_be_bytes());

        response.extend_from_slice(&[0xc0, 0x0c, 0, 5, 0, 1]);
        response.extend_from_slice(&120_u32.to_be_bytes());
        let mut target = Vec::new();
        append_name(&mut target, "edge.example.cn");
        response.extend_from_slice(&(target.len() as u16).to_be_bytes());
        response.extend_from_slice(&target);

        append_name(&mut response, "edge.example.cn");
        response.extend_from_slice(&[0, 1, 0, 1]);
        response.extend_from_slice(&60_u32.to_be_bytes());
        response.extend_from_slice(&4_u16.to_be_bytes());
        response.extend_from_slice(&[198, 51, 100, 9]);

        append_name(&mut response, "injected.example");
        response.extend_from_slice(&[0, 1, 0, 1]);
        response.extend_from_slice(&300_u32.to_be_bytes());
        response.extend_from_slice(&4_u16.to_be_bytes());
        response.extend_from_slice(&[203, 0, 113, 66]);
        response
    }

    fn a_response(query: &[u8], ttl: u32, ip: [u8; 4]) -> Vec<u8> {
        let parsed = parse_query(query).unwrap();
        let mut response = query[..parsed.question_end].to_vec();
        response[2..4].copy_from_slice(&0x8180_u16.to_be_bytes());
        response[6..8].copy_from_slice(&1_u16.to_be_bytes());
        response.extend_from_slice(&[0xc0, 0x0c, 0, 1, 0, 1]);
        response.extend_from_slice(&ttl.to_be_bytes());
        response.extend_from_slice(&4_u16.to_be_bytes());
        response.extend_from_slice(&ip);
        response
    }

    #[test]
    fn classifies_before_upstream_selection() {
        let direct = parse_query(&query(1, "www.example.cn")).unwrap();
        let tunnel = parse_query(&query(2, "www.example.com")).unwrap();
        assert_eq!(classify_query(&direct, &policy()), Ok(QueryRoute::Direct));
        assert_eq!(classify_query(&tunnel, &policy()), Ok(QueryRoute::Tunnel));
    }

    #[test]
    fn mixed_questions_fail_closed_before_any_upstream() {
        let parsed = parse_query(&mixed_query()).unwrap();
        assert_eq!(classify_query(&parsed, &policy()), Err(()));
    }

    #[test]
    fn edns_query_is_preserved_and_malformed_query_sections_are_rejected() {
        let mut request = query(4, "www.example.cn");
        request[10..12].copy_from_slice(&1_u16.to_be_bytes());
        // Root owner, OPT, 4096-byte UDP payload, DNSSEC OK, empty option data.
        request.extend_from_slice(&[0, 0, 41, 0x10, 0, 0, 0, 0x80, 0, 0, 0]);
        let parsed = parse_query(&request).unwrap();
        assert!(parsed.question_end < request.len());

        let mut truncated = request;
        truncated.pop();
        assert!(parse_query(&truncated).is_err());

        let mut trailing = query(5, "www.example.cn");
        trailing.push(0xff);
        assert!(parse_query(&trailing).is_err());
    }

    #[test]
    fn response_validation_and_hint_extraction_are_question_bound() {
        let request = query(7, "www.example.cn");
        let parsed = parse_query(&request).unwrap();
        let response = a_response(&request, 60, [198, 51, 100, 7]);
        validate_response(&parsed, &response).unwrap();
        assert_eq!(
            response_hints(&response, &parsed).unwrap(),
            vec![(IpAddr::V4(Ipv4Addr::new(198, 51, 100, 7)), 60)]
        );
    }

    #[test]
    fn cname_hints_ignore_unrelated_additional_records_and_reject_trailing_bytes() {
        let request = query(8, "www.example.cn");
        let parsed = parse_query(&request).unwrap();
        let response = cname_response_with_unrelated_additional(&request);
        assert_eq!(
            response_hints(&response, &parsed).unwrap(),
            vec![(IpAddr::V4(Ipv4Addr::new(198, 51, 100, 9)), 60)]
        );
        let mut malformed = response;
        malformed.push(0xff);
        assert!(response_hints(&malformed, &parsed).is_err());
    }

    #[test]
    fn nxdomain_is_valid_but_never_creates_a_hint() {
        let request = query(10, "missing.example.cn");
        let parsed = parse_query(&request).unwrap();
        let mut response = request.clone();
        response[2..4].copy_from_slice(&0x8183_u16.to_be_bytes());
        assert!(response_hints(&response, &parsed).unwrap().is_empty());
    }

    #[test]
    fn conflicting_dns_hint_falls_back_to_geoip() {
        let request = query(9, "www.example.cn");
        let parsed = parse_query(&request).unwrap();
        let response = a_response(&request, 60, [198, 51, 100, 7]);
        let cache = DnsRouteCache::default();
        cache.observe(&response, &parsed, QueryRoute::Direct, Some(1));
        assert_eq!(
            cache.route_ip("198.51.100.7".parse().unwrap(), Some(1), &policy()),
            GeoRoute::Direct
        );
        cache.observe(&response, &parsed, QueryRoute::Tunnel, Some(1));
        assert_eq!(
            cache.route_ip("198.51.100.7".parse().unwrap(), Some(1), &policy()),
            GeoRoute::Tunnel
        );
    }

    #[test]
    fn generation_change_clears_dns_hints() {
        let request = query(11, "www.example.cn");
        let parsed = parse_query(&request).unwrap();
        let response = a_response(&request, 60, [198, 51, 100, 7]);
        let cache = DnsRouteCache::default();
        cache.observe(&response, &parsed, QueryRoute::Direct, Some(1));
        assert_eq!(
            cache.route_ip("198.51.100.7".parse().unwrap(), Some(2), &policy()),
            GeoRoute::Tunnel
        );
    }

    #[test]
    fn ttl_zero_is_not_cached_and_errors_keep_the_question() {
        let request = query(13, "www.example.cn");
        let parsed = parse_query(&request).unwrap();
        let response = a_response(&request, 0, [198, 51, 100, 7]);
        let cache = DnsRouteCache::default();
        cache.observe(&response, &parsed, QueryRoute::Direct, None);
        assert_eq!(
            cache.route_ip("198.51.100.7".parse().unwrap(), None, &policy()),
            GeoRoute::Tunnel
        );
        let failure = error_response(&request, RCODE_SERVFAIL);
        assert_eq!(read_u16(&failure, 0).unwrap(), 13);
        assert_eq!(read_u16(&failure, 2).unwrap() & 0xf, RCODE_SERVFAIL);
        assert_eq!(&failure[12..], &request[12..]);
    }

    struct LocalDnsProtector {
        server: SocketAddr,
        generation: Arc<AtomicU64>,
    }

    impl SocketProtector for LocalDnsProtector {
        fn protect(&self, _socket: SocketHandle) -> Result<(), String> {
            Ok(())
        }

        fn physical_dns_servers(&self) -> Vec<SocketAddr> {
            vec![self.server]
        }

        fn network_generation(&self) -> Option<u64> {
            Some(self.generation.load(Ordering::Acquire))
        }
    }

    #[tokio::test]
    async fn physical_resolver_retries_truncated_udp_over_tcp() {
        let udp = tokio::net::UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let port = udp.local_addr().unwrap().port();
        let tcp = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, port))
            .await
            .unwrap();
        let udp_task = tokio::spawn(async move {
            let mut packet = [0_u8; 512];
            for _ in 0..2 {
                let (length, client) = udp.recv_from(&mut packet).await.unwrap();
                let response = truncated_response(&packet[..length]);
                udp.send_to(&response, client).await.unwrap();
            }
        });
        let tcp_task = tokio::spawn(async move {
            for _ in 0..2 {
                let (mut stream, _) = tcp.accept().await.unwrap();
                let length = usize::from(stream.read_u16().await.unwrap());
                let mut request = vec![0_u8; length];
                stream.read_exact(&mut request).await.unwrap();
                let parsed = parse_query(&request).unwrap();
                let response = if parsed.questions[0].query_type == 1 {
                    a_response(&request, 60, [127, 0, 0, 42])
                } else {
                    let mut response = request[..parsed.question_end].to_vec();
                    response[2..4].copy_from_slice(&0x8180_u16.to_be_bytes());
                    response
                };
                stream.write_u16(response.len() as u16).await.unwrap();
                stream.write_all(&response).await.unwrap();
            }
        });
        let protector = LocalDnsProtector {
            server: SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
            generation: Arc::new(AtomicU64::new(1)),
        };
        let addresses = resolve_physical_host(&protector, "DIRECT.EXAMPLE.CN.", 443)
            .await
            .unwrap();
        assert_eq!(
            addresses,
            vec![SocketAddr::from((Ipv4Addr::new(127, 0, 0, 42), 443))]
        );
        udp_task.await.unwrap();
        tcp_task.await.unwrap();
    }

    #[tokio::test]
    async fn physical_resolver_rejects_a_response_from_an_old_network_generation() {
        let udp = tokio::net::UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let server = udp.local_addr().unwrap();
        let generation = Arc::new(AtomicU64::new(1));
        let changed_generation = Arc::clone(&generation);
        let server_task = tokio::spawn(async move {
            let mut packet = [0_u8; 512];
            let (length, client) = udp.recv_from(&mut packet).await.unwrap();
            let response = a_response(&packet[..length], 60, [127, 0, 0, 42]);
            changed_generation.store(2, Ordering::Release);
            udp.send_to(&response, client).await.unwrap();
        });
        let protector = LocalDnsProtector { server, generation };
        let error = resolve_physical_host(&protector, "direct.example.cn", 443)
            .await
            .unwrap_err();
        assert!(error.contains("physical network changed"));
        server_task.await.unwrap();
    }
}
