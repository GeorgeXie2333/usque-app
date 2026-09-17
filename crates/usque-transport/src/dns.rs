use std::collections::HashSet;
use std::future::Future;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket as StdUdpSocket};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU16, Ordering};
use std::time::Duration;

use tokio::net::{UdpSocket, lookup_host};
use tokio::time::{Instant, timeout_at};
use ts_netstack_smoltcp::netcore::Channel;
use usque_core::ProxyDnsMode;

use crate::h2::TransportError;
use crate::port_allocator::next_udp_port;
use crate::socket::{SocketProtector, socket_handle};

const DNS_TIMEOUT: Duration = Duration::from_secs(4);
const DNS_PORT: u16 = 53;
const MAX_DNS_PACKET: usize = 4096;
const MAX_RESULTS: usize = 32;
const TYPE_A: u16 = 1;
const TYPE_AAAA: u16 = 28;
const CLASS_IN: u16 = 1;

static NEXT_DNS_ID: AtomicU16 = AtomicU16::new(0x5173);
#[derive(Clone)]
pub(crate) struct Resolver {
    channel: Option<Channel>,
    stream_dns: Option<Arc<crate::dns_stream::StreamDns>>,
    assigned_ipv4: Ipv4Addr,
    assigned_ipv6: Ipv6Addr,
    servers: Vec<IpAddr>,
    mode: ProxyDnsMode,
    protector: Arc<dyn SocketProtector>,
}

impl Resolver {
    pub(crate) fn for_streams(
        stream_dns: Arc<crate::dns_stream::StreamDns>,
        servers: Vec<IpAddr>,
        mode: ProxyDnsMode,
        protector: Arc<dyn SocketProtector>,
    ) -> Self {
        Self {
            channel: None,
            stream_dns: Some(stream_dns),
            assigned_ipv4: Ipv4Addr::UNSPECIFIED,
            assigned_ipv6: Ipv6Addr::UNSPECIFIED,
            servers,
            mode,
            protector,
        }
    }

    pub(crate) fn new(
        channel: Channel,
        assigned_ipv4: Ipv4Addr,
        assigned_ipv6: Ipv6Addr,
        servers: Vec<IpAddr>,
        mode: ProxyDnsMode,
        protector: Arc<dyn SocketProtector>,
    ) -> Self {
        Self {
            channel: Some(channel),
            stream_dns: None,
            assigned_ipv4,
            assigned_ipv6,
            servers,
            mode,
            protector,
        }
    }

    pub(crate) async fn resolve(&self, name: &str) -> Result<Vec<IpAddr>, TransportError> {
        let mut resolution = self.resolve_candidates(name, Instant::now() + DNS_TIMEOUT)?;
        let mut addresses = Vec::new();
        let mut errors = Vec::new();
        while let Some(result) = resolution.next().await {
            match result {
                Ok(mut values) => addresses.append(&mut values),
                Err(error) => errors.push(error.to_string()),
            }
        }
        // Keep the legacy UDP caller's IPv4-first ordering. System resolution
        // retains the operating system's ordering instead.
        if matches!(
            self.mode,
            ProxyDnsMode::Remote | ProxyDnsMode::LocalConfigured
        ) {
            addresses.sort_by_key(IpAddr::is_ipv6);
        }
        deduplicate(&mut addresses);
        addresses.truncate(MAX_RESULTS);
        if addresses.is_empty() {
            return Err(TransportError::Dns(if errors.is_empty() {
                "no usable A or AAAA records".to_owned()
            } else {
                errors.join("; ")
            }));
        }
        Ok(addresses)
    }

    pub(crate) fn resolve_candidates(
        &self,
        name: &str,
        deadline: Instant,
    ) -> Result<CandidateResolution, TransportError> {
        if let Ok(address) = name.parse::<IpAddr>() {
            return Ok(CandidateResolution::from_addresses(vec![address]));
        }
        validate_name(name)?;
        let deadline = deadline.min(Instant::now() + DNS_TIMEOUT);
        match self.mode {
            ProxyDnsMode::Remote | ProxyDnsMode::LocalConfigured => {
                let make_query = |ipv4| {
                    let resolver = self.clone();
                    let name = name.to_owned();
                    QuerySlot {
                        ipv4: Some(ipv4),
                        future: bounded_query(
                            async move {
                                let query_type = if ipv4 { TYPE_A } else { TYPE_AAAA };
                                if resolver.mode == ProxyDnsMode::Remote {
                                    resolver
                                        .query_through_tunnel(&name, query_type, deadline)
                                        .await
                                } else {
                                    resolver
                                        .query_with_configured_servers(&name, query_type, deadline)
                                        .await
                                }
                            },
                            deadline,
                        ),
                    }
                };
                Ok(CandidateResolution {
                    ready: None,
                    first: Some(make_query(true)),
                    second: Some(make_query(false)),
                })
            }
            ProxyDnsMode::System => {
                let name = name.to_owned();
                Ok(CandidateResolution {
                    ready: None,
                    first: Some(QuerySlot {
                        ipv4: None,
                        future: bounded_query(
                            async move {
                                lookup_host((name.as_str(), 0))
                                    .await
                                    .map(|values| values.map(|address| address.ip()).collect())
                                    .map_err(|error| TransportError::Dns(error.to_string()))
                            },
                            deadline,
                        ),
                    }),
                    second: None,
                })
            }
            ProxyDnsMode::EdgeResolved => Err(TransportError::Dns(
                "edge-resolved names must be sent to CONNECT".to_owned(),
            )),
        }
    }

    async fn query_through_tunnel(
        &self,
        name: &str,
        query_type: u16,
        deadline: Instant,
    ) -> Result<Vec<IpAddr>, TransportError> {
        let transaction_id = NEXT_DNS_ID.fetch_add(1, Ordering::Relaxed);
        let query = encode_query(transaction_id, name, query_type)?;
        query_servers(&self.servers, deadline, |server| {
            let query = &query;
            async move {
                let remote = SocketAddr::new(server, DNS_PORT);
                if let Some(dns) = &self.stream_dns {
                    let response = dns
                        .query(remote, query, deadline)
                        .await
                        .map_err(|error| TransportError::Dns(error.to_string()))?;
                    return decode_query_response(query, &response, query_type);
                }
                let local_ip = if server.is_ipv4() {
                    IpAddr::V4(self.assigned_ipv4)
                } else {
                    IpAddr::V6(self.assigned_ipv6)
                };
                if local_ip.is_unspecified() {
                    return Err(TransportError::Dns(
                        "DNS server family is unavailable".to_owned(),
                    ));
                }
                let channel = self
                    .channel
                    .as_ref()
                    .ok_or_else(|| TransportError::Dns("DNS transport unavailable".to_owned()))?;
                let socket =
                    QuerySocket::bind(channel.clone(), SocketAddr::new(local_ip, next_udp_port()))
                        .await
                        .map_err(|error| TransportError::Dns(error.to_string()))?;
                socket
                    .send_to(remote, query)
                    .await
                    .map_err(|error| TransportError::Dns(error.to_string()))?;
                let (source, response) = socket
                    .recv_from_bytes()
                    .await
                    .map_err(|error| TransportError::Dns(error.to_string()))?;
                if source != remote {
                    return Err(TransportError::Dns(
                        "DNS response source mismatch".to_owned(),
                    ));
                }
                decode_query_response(query, &response, query_type)
            }
        })
        .await
    }

    async fn query_with_configured_servers(
        &self,
        name: &str,
        query_type: u16,
        deadline: Instant,
    ) -> Result<Vec<IpAddr>, TransportError> {
        let transaction_id = NEXT_DNS_ID.fetch_add(1, Ordering::Relaxed);
        let query = encode_query(transaction_id, name, query_type)?;
        query_servers(&self.servers, deadline, |server| {
            query_local_server(
                self.protector.as_ref(),
                SocketAddr::new(server, DNS_PORT),
                &query,
                query_type,
                deadline,
            )
        })
        .await
    }
}

// DNS cancellation can coincide with a full stack command queue. Retain the
// existing retrying cleanup path instead of the upstream best-effort drop.
struct QuerySocket {
    channel: Channel,
    handle: ts_netstack_smoltcp::netcore::smoltcp::iface::SocketHandle,
}

impl QuerySocket {
    async fn bind(
        channel: Channel,
        endpoint: SocketAddr,
    ) -> Result<Self, ts_netstack_smoltcp::netcore::Error> {
        use ts_netstack_smoltcp::netcore::{HasChannel, Response, udp};
        match channel
            .request(None, udp::Command::Bind { endpoint })
            .await?
        {
            Response::Udp(udp::Response::Bound { handle, .. }) => Ok(Self { channel, handle }),
            Response::Error(error) => Err(error),
            _ => Err(ts_netstack_smoltcp::netcore::Error::wrong_type()),
        }
    }

    async fn send_to(
        &self,
        endpoint: SocketAddr,
        bytes: &[u8],
    ) -> Result<(), ts_netstack_smoltcp::netcore::Error> {
        use ts_netstack_smoltcp::netcore::{HasChannel, udp};
        self.channel
            .request(
                Some(self.handle),
                udp::Command::Send {
                    endpoint,
                    buf: bytes::Bytes::copy_from_slice(bytes),
                },
            )
            .await?
            .to_ok()
    }

    async fn recv_from_bytes(
        &self,
    ) -> Result<(SocketAddr, bytes::Bytes), ts_netstack_smoltcp::netcore::Error> {
        use ts_netstack_smoltcp::netcore::{HasChannel, Response, udp};
        match self
            .channel
            .request(Some(self.handle), udp::Command::Recv { max_len: None })
            .await?
        {
            Response::Udp(udp::Response::RecvFrom { remote, buf, .. }) => Ok((remote, buf)),
            Response::Error(error) => Err(error),
            _ => Err(ts_netstack_smoltcp::netcore::Error::wrong_type()),
        }
    }
}

impl Drop for QuerySocket {
    fn drop(&mut self) {
        crate::stack_tcp::cleanup(&self.channel, Some(self.handle), || {
            ts_netstack_smoltcp::netcore::udp::Command::Close.into()
        });
    }
}

type QueryFuture = Pin<Box<dyn Future<Output = Result<Vec<IpAddr>, TransportError>> + Send>>;
struct QuerySlot {
    // None is the operating system's combined A/AAAA lookup.
    ipv4: Option<bool>,
    future: QueryFuture,
}

/// Owns unfinished lookups. Dropping this cancels owned I/O; system resolver
/// results that outlive their awaiter are never admitted into another request.
pub(crate) struct CandidateResolution {
    ready: Option<Vec<IpAddr>>,
    first: Option<QuerySlot>,
    second: Option<QuerySlot>,
}

impl CandidateResolution {
    pub(crate) fn from_addresses(mut addresses: Vec<IpAddr>) -> Self {
        deduplicate(&mut addresses);
        addresses.truncate(MAX_RESULTS);
        Self {
            ready: Some(addresses),
            first: None,
            second: None,
        }
    }

    pub(crate) fn pending_family(&self, ipv4: bool) -> bool {
        self.first
            .iter()
            .chain(self.second.iter())
            .any(|slot| slot.ipv4.is_none_or(|family| family == ipv4))
    }

    pub(crate) async fn next(&mut self) -> Option<Result<Vec<IpAddr>, TransportError>> {
        if let Some(addresses) = self.ready.take() {
            return Some(Ok(addresses));
        }
        if !self.pending_family(true) && !self.pending_family(false) {
            return None;
        }
        tokio::select! {
            result = wait_query(&mut self.first), if self.first.is_some() => {
                self.first.take();
                Some(result)
            }
            result = wait_query(&mut self.second), if self.second.is_some() => {
                self.second.take();
                Some(result)
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn test_queries(
        ipv4: impl Future<Output = Result<Vec<IpAddr>, TransportError>> + Send + 'static,
        ipv6: impl Future<Output = Result<Vec<IpAddr>, TransportError>> + Send + 'static,
    ) -> Self {
        let deadline = Instant::now() + DNS_TIMEOUT;
        Self {
            ready: None,
            first: Some(QuerySlot {
                ipv4: Some(true),
                future: bounded_query(ipv4, deadline),
            }),
            second: Some(QuerySlot {
                ipv4: Some(false),
                future: bounded_query(ipv6, deadline),
            }),
        }
    }
}

async fn wait_query(slot: &mut Option<QuerySlot>) -> Result<Vec<IpAddr>, TransportError> {
    match slot {
        Some(slot) => slot.future.as_mut().await,
        None => std::future::pending().await,
    }
}

fn dns_timeout() -> TransportError {
    TransportError::Dns("DNS query timed out".to_owned())
}

fn bounded_query(
    query: impl Future<Output = Result<Vec<IpAddr>, TransportError>> + Send + 'static,
    deadline: Instant,
) -> QueryFuture {
    Box::pin(async move {
        if Instant::now() >= deadline {
            return Err(dns_timeout());
        }
        let mut values = timeout_at(deadline, query)
            .await
            .map_err(|_| dns_timeout())??;
        deduplicate(&mut values);
        values.truncate(MAX_RESULTS);
        Ok(values)
    })
}

async fn query_servers<F, Fut>(
    servers: &[IpAddr],
    deadline: Instant,
    mut query: F,
) -> Result<Vec<IpAddr>, TransportError>
where
    F: FnMut(IpAddr) -> Fut,
    Fut: Future<Output = Result<Vec<IpAddr>, TransportError>>,
{
    let mut errors = Vec::new();
    for server in servers {
        if Instant::now() >= deadline {
            return Err(dns_timeout());
        }
        match timeout_at(deadline, query(*server)).await {
            // Valid NODATA/NXDOMAIN is terminal for this question, not a reason
            // to repeat it against every configured resolver.
            Ok(Ok(values)) => return Ok(values),
            Ok(Err(error)) => errors.push(error.to_string()),
            Err(_) => return Err(dns_timeout()),
        }
    }
    Err(TransportError::Dns(if errors.is_empty() {
        "no usable DNS servers".to_owned()
    } else {
        errors.join("; ")
    }))
}

async fn query_local_server(
    protector: &dyn SocketProtector,
    remote: SocketAddr,
    query: &[u8],
    query_type: u16,
    deadline: Instant,
) -> Result<Vec<IpAddr>, TransportError> {
    let work = async {
        let bind_address = if remote.is_ipv4() {
            SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0)
        } else {
            SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), 0)
        };
        let std_socket = StdUdpSocket::bind(bind_address)?;
        protector
            .protect(socket_handle(&std_socket))
            .map_err(TransportError::SocketProtection)?;
        std_socket.set_nonblocking(true)?;
        let socket = UdpSocket::from_std(std_socket)?;
        let sent = socket.send_to(query, remote).await?;
        if sent != query.len() {
            return Err(TransportError::Dns("partial DNS query send".to_owned()));
        }
        let mut response = [0u8; MAX_DNS_PACKET];
        let (length, source) = socket.recv_from(&mut response).await?;
        if source != remote {
            return Err(TransportError::Dns(
                "DNS response source mismatch".to_owned(),
            ));
        }
        decode_query_response(query, &response[..length], query_type)
    };
    timeout_at(deadline, work)
        .await
        .map_err(|_| dns_timeout())?
}

fn decode_query_response(
    query: &[u8],
    response: &[u8],
    query_type: u16,
) -> Result<Vec<IpAddr>, TransportError> {
    crate::split_dns::validate_response_bytes(query, response).map_err(TransportError::Dns)?;
    decode_response(response, read_u16(query, 0)?, query_type)
}

fn validate_name(name: &str) -> Result<(), TransportError> {
    if name.is_empty()
        || name.len() > 253
        || name.ends_with('.')
        || !name.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        })
    {
        return Err(TransportError::Dns("invalid DNS name".to_owned()));
    }
    Ok(())
}

fn encode_query(
    transaction_id: u16,
    name: &str,
    query_type: u16,
) -> Result<Vec<u8>, TransportError> {
    validate_name(name)?;
    let mut packet = Vec::with_capacity(name.len() + 18);
    packet.extend_from_slice(&transaction_id.to_be_bytes());
    packet.extend_from_slice(&0x0100u16.to_be_bytes());
    packet.extend_from_slice(&1u16.to_be_bytes());
    packet.extend_from_slice(&0u16.to_be_bytes());
    packet.extend_from_slice(&0u16.to_be_bytes());
    packet.extend_from_slice(&0u16.to_be_bytes());
    for label in name.split('.') {
        packet.push(label.len() as u8);
        packet.extend_from_slice(label.as_bytes());
    }
    packet.push(0);
    packet.extend_from_slice(&query_type.to_be_bytes());
    packet.extend_from_slice(&CLASS_IN.to_be_bytes());
    Ok(packet)
}

fn decode_response(
    packet: &[u8],
    transaction_id: u16,
    expected_type: u16,
) -> Result<Vec<IpAddr>, TransportError> {
    if packet.len() < 12 || packet.len() > MAX_DNS_PACKET {
        return Err(TransportError::Dns("malformed DNS response".to_owned()));
    }
    if read_u16(packet, 0)? != transaction_id {
        return Err(TransportError::Dns(
            "DNS transaction ID mismatch".to_owned(),
        ));
    }
    let flags = read_u16(packet, 2)?;
    if flags & 0x8000 == 0 {
        return Err(TransportError::Dns("not a DNS response".to_owned()));
    }
    if flags & 0x0200 != 0 {
        return Err(TransportError::Dns("truncated DNS response".to_owned()));
    }
    let rcode = flags & 0x000f;
    if !matches!(rcode, 0 | 3) {
        return Err(TransportError::Dns(format!("DNS rcode {rcode}")));
    }

    let question_count = usize::from(read_u16(packet, 4)?);
    let answer_count = usize::from(read_u16(packet, 6)?);
    let mut offset = 12;
    for _ in 0..question_count {
        offset = skip_name(packet, offset)?;
        offset = offset
            .checked_add(4)
            .filter(|value| *value <= packet.len())
            .ok_or_else(|| TransportError::Dns("truncated DNS question".to_owned()))?;
    }

    let mut addresses = Vec::new();
    for _ in 0..answer_count {
        offset = skip_name(packet, offset)?;
        if offset + 10 > packet.len() {
            return Err(TransportError::Dns("truncated DNS answer".to_owned()));
        }
        let record_type = read_u16(packet, offset)?;
        let class = read_u16(packet, offset + 2)?;
        let data_length = usize::from(read_u16(packet, offset + 8)?);
        offset += 10;
        let end = offset
            .checked_add(data_length)
            .filter(|value| *value <= packet.len())
            .ok_or_else(|| TransportError::Dns("truncated DNS record data".to_owned()))?;
        if class == CLASS_IN && record_type == expected_type {
            match (record_type, data_length) {
                (TYPE_A, 4) => addresses.push(IpAddr::V4(Ipv4Addr::new(
                    packet[offset],
                    packet[offset + 1],
                    packet[offset + 2],
                    packet[offset + 3],
                ))),
                (TYPE_AAAA, 16) => {
                    let octets: [u8; 16] = packet[offset..end]
                        .try_into()
                        .map_err(|_| TransportError::Dns("invalid AAAA record".to_owned()))?;
                    addresses.push(IpAddr::V6(Ipv6Addr::from(octets)));
                }
                _ => return Err(TransportError::Dns("invalid DNS address length".to_owned())),
            }
        }
        offset = end;
    }
    Ok(if rcode == 3 { Vec::new() } else { addresses })
}

fn skip_name(packet: &[u8], mut offset: usize) -> Result<usize, TransportError> {
    let mut labels = 0usize;
    loop {
        let length = *packet
            .get(offset)
            .ok_or_else(|| TransportError::Dns("truncated DNS name".to_owned()))?;
        if length & 0xc0 == 0xc0 {
            if offset + 2 > packet.len() {
                return Err(TransportError::Dns(
                    "truncated DNS compression pointer".to_owned(),
                ));
            }
            return Ok(offset + 2);
        }
        if length & 0xc0 != 0 {
            return Err(TransportError::Dns("invalid DNS label encoding".to_owned()));
        }
        offset += 1;
        if length == 0 {
            return Ok(offset);
        }
        offset = offset
            .checked_add(usize::from(length))
            .filter(|value| *value <= packet.len())
            .ok_or_else(|| TransportError::Dns("truncated DNS label".to_owned()))?;
        labels += 1;
        if labels > 127 {
            return Err(TransportError::Dns("too many DNS labels".to_owned()));
        }
    }
}

fn read_u16(packet: &[u8], offset: usize) -> Result<u16, TransportError> {
    let bytes = packet
        .get(offset..offset + 2)
        .ok_or_else(|| TransportError::Dns("truncated DNS field".to_owned()))?;
    Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
}

fn deduplicate(addresses: &mut Vec<IpAddr>) {
    let mut seen = HashSet::new();
    addresses.retain(|address| seen.insert(*address));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::socket::NoopSocketProtector;
    use std::sync::atomic::AtomicUsize;

    struct QueryGuard(Arc<AtomicUsize>);
    impl Drop for QueryGuard {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[tokio::test]
    async fn dns_socket_close_waits_for_command_capacity() {
        use ts_netstack_smoltcp::netcore::{
            Command, Config, HasChannel, Netstack, smoltcp::time::Instant as StackInstant,
            stack_control, udp,
        };
        let mut stack = Netstack::new(
            Config {
                command_channel_capacity: Some(1),
                ..Config::default()
            },
            StackInstant::from_millis(0),
        );
        let channel = stack.command_channel();
        let mut bind = Box::pin(QuerySocket::bind(
            channel.clone(),
            "127.0.0.1:50000".parse().unwrap(),
        ));
        assert!(
            std::future::poll_fn(|cx| std::task::Poll::Ready(bind.as_mut().poll(cx)))
                .await
                .is_pending()
        );
        stack.process_cmds();
        let socket = bind.await.unwrap();
        ts_netstack_smoltcp::netcore::try_request_nonblocking(
            &channel,
            None,
            stack_control::Command::SetIps {
                new_ips: vec!["127.0.0.1".parse().unwrap()],
            },
        )
        .unwrap();
        drop(socket);
        stack.process_cmds();
        let close = tokio::time::timeout(Duration::from_secs(1), stack.wait_for_cmd())
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(close.command, Command::Udp(udp::Command::Close)));
        stack.process_one_cmd(close);
    }

    #[tokio::test(start_paused = true)]
    async fn first_family_is_available_while_the_other_query_remains_owned() {
        for fast_v4 in [true, false] {
            let released = Arc::new(AtomicUsize::new(0));
            let slow_released = released.clone();
            let fast = async move {
                tokio::time::sleep(Duration::from_millis(10)).await;
                Ok(vec![if fast_v4 {
                    "198.51.100.1".parse().unwrap()
                } else {
                    "2001:db8::1".parse().unwrap()
                }])
            };
            let slow = async move {
                let _guard = QueryGuard(slow_released);
                std::future::pending::<Result<Vec<IpAddr>, TransportError>>().await
            };
            let deadline = Instant::now() + DNS_TIMEOUT;
            let mut resolution = CandidateResolution {
                ready: None,
                first: Some(QuerySlot {
                    ipv4: Some(fast_v4),
                    future: bounded_query(fast, deadline),
                }),
                second: Some(QuerySlot {
                    ipv4: Some(!fast_v4),
                    future: bounded_query(slow, deadline),
                }),
            };
            let started = Instant::now();
            let values = resolution.next().await.unwrap().unwrap();
            assert_eq!(values[0].is_ipv4(), fast_v4);
            assert_eq!(started.elapsed(), Duration::from_millis(10));
            assert!(resolution.pending_family(!fast_v4));
            drop(resolution);
            assert_eq!(released.load(Ordering::SeqCst), 1);
        }
    }

    #[tokio::test(start_paused = true)]
    async fn both_families_share_one_absolute_deadline() {
        let pending = || std::future::pending::<Result<Vec<IpAddr>, TransportError>>();
        let mut resolution = CandidateResolution::test_queries(pending(), pending());
        let started = Instant::now();
        assert!(resolution.next().await.unwrap().is_err());
        assert!(resolution.next().await.unwrap().is_err());
        assert!(resolution.next().await.is_none());
        assert_eq!(started.elapsed(), DNS_TIMEOUT);
    }

    #[tokio::test(start_paused = true)]
    async fn server_retries_cannot_restart_the_dns_deadline() {
        let servers = ["192.0.2.1".parse().unwrap(), "192.0.2.2".parse().unwrap()];
        let calls = AtomicUsize::new(0);
        let started = Instant::now();
        let result = query_servers(&servers, started + DNS_TIMEOUT, |_| {
            calls.fetch_add(1, Ordering::SeqCst);
            async {
                tokio::time::sleep(Duration::from_secs(3)).await;
                Err(TransportError::Dns("temporary failure".to_owned()))
            }
        })
        .await;
        assert!(result.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(started.elapsed(), DNS_TIMEOUT);
    }

    #[tokio::test(start_paused = true)]
    async fn blocked_udp_bind_is_already_inside_the_resolution_deadline() {
        let (channel, _requests) = ts_netstack_smoltcp::netcore::flume::bounded(1);
        let resolver = Resolver::new(
            channel.downgrade(),
            "172.16.0.2".parse().unwrap(),
            "2001:db8::2".parse().unwrap(),
            vec!["192.0.2.1".parse().unwrap()],
            ProxyDnsMode::Remote,
            Arc::new(NoopSocketProtector),
        );
        let started = Instant::now();
        assert!(resolver.resolve("example.test").await.is_err());
        assert_eq!(started.elapsed(), DNS_TIMEOUT);
    }

    #[tokio::test(start_paused = true)]
    async fn an_expired_deadline_never_polls_new_io() {
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        let result = bounded_query(
            async move {
                observed.fetch_add(1, Ordering::SeqCst);
                Ok(Vec::new())
            },
            Instant::now(),
        )
        .await;
        assert!(result.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn validated_negative_answers_stop_only_their_server_retry_chain() {
        let servers = ["192.0.2.1".parse().unwrap(), "192.0.2.2".parse().unwrap()];
        for query_type in [TYPE_A, TYPE_AAAA] {
            for flags in [0x8180u16, 0x8183] {
                let query = encode_query(0x1234, "example.test", query_type).unwrap();
                let mut response = query.clone();
                response[2..4].copy_from_slice(&flags.to_be_bytes());
                let calls = AtomicUsize::new(0);
                let result = query_servers(&servers, Instant::now() + DNS_TIMEOUT, |_| {
                    calls.fetch_add(1, Ordering::SeqCst);
                    std::future::ready(decode_query_response(&query, &response, query_type))
                })
                .await
                .unwrap();
                assert!(result.is_empty());
                assert_eq!(calls.load(Ordering::SeqCst), 1);
            }
        }
    }

    #[test]
    fn negative_answers_still_require_matching_questions_and_complete_records() {
        let query = encode_query(0x1234, "example.test", TYPE_A).unwrap();
        let mut response = query.clone();
        response[2..4].copy_from_slice(&0x8183u16.to_be_bytes());
        response[13] = b'x';
        assert!(decode_query_response(&query, &response, TYPE_A).is_err());
        let mut response = query.clone();
        response[2..4].copy_from_slice(&0x8183u16.to_be_bytes());
        response[8..10].copy_from_slice(&1u16.to_be_bytes());
        assert!(decode_query_response(&query, &response, TYPE_A).is_err());
    }

    proptest::proptest! {
        #[test]
        fn arbitrary_positive_and_negative_dns_records_never_panic(
            tail in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..4200),
            negative in proptest::prelude::any::<bool>(),
            count in 0u16..256,
        ) {
            let query = encode_query(0x1234, "example.test", TYPE_A).unwrap();
            let mut response = query.clone();
            response[2..4].copy_from_slice(&(if negative { 0x8183u16 } else { 0x8180 }).to_be_bytes());
            response[6..8].copy_from_slice(&count.to_be_bytes());
            response.extend_from_slice(&tail);
            let _ = decode_query_response(&query, &response, TYPE_A);
        }
    }

    #[test]
    fn encodes_bounded_dns_query() {
        let query = encode_query(0x1234, "example.com", TYPE_A).unwrap();
        assert_eq!(&query[..2], &[0x12, 0x34]);
        assert!(query.windows(7).any(|value| value == b"example"));
        assert_eq!(&query[query.len() - 4..], &[0, 1, 0, 1]);
    }

    #[test]
    fn decodes_a_response_with_compressed_answer_name() {
        let mut response = encode_query(0x1234, "example.com", TYPE_A).unwrap();
        response[2..4].copy_from_slice(&0x8180u16.to_be_bytes());
        response[6..8].copy_from_slice(&1u16.to_be_bytes());
        response.extend_from_slice(&[0xc0, 0x0c, 0, 1, 0, 1, 0, 0, 0, 60, 0, 4, 203, 0, 113, 9]);
        assert_eq!(
            decode_response(&response, 0x1234, TYPE_A).unwrap(),
            vec![IpAddr::V4(Ipv4Addr::new(203, 0, 113, 9))]
        );
    }

    #[tokio::test]
    async fn configured_query_uses_the_requested_dns_server() {
        let server = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let server_address = server.local_addr().unwrap();
        let responder = tokio::spawn(async move {
            let mut packet = [0u8; MAX_DNS_PACKET];
            let (length, client) = server.recv_from(&mut packet).await.unwrap();
            let mut response = packet[..length].to_vec();
            response[2..4].copy_from_slice(&0x8180u16.to_be_bytes());
            response[6..8].copy_from_slice(&1u16.to_be_bytes());
            response
                .extend_from_slice(&[0xc0, 0x0c, 0, 1, 0, 1, 0, 0, 0, 60, 0, 4, 203, 0, 113, 9]);
            server.send_to(&response, client).await.unwrap();
        });

        let query = encode_query(0x1234, "example.com", TYPE_A).unwrap();
        let addresses = query_local_server(
            &NoopSocketProtector,
            server_address,
            &query,
            TYPE_A,
            Instant::now() + DNS_TIMEOUT,
        )
        .await
        .unwrap();
        responder.await.unwrap();

        assert_eq!(addresses, vec![IpAddr::V4(Ipv4Addr::new(203, 0, 113, 9))]);
    }
}
