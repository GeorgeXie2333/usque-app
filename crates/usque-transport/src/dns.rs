use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket as StdUdpSocket};
use std::sync::Arc;
use std::sync::atomic::{AtomicU16, Ordering};
use std::time::Duration;

use tokio::net::{UdpSocket, lookup_host};
use tokio::time::timeout;
use ts_netstack_smoltcp::CreateSocket;
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
    channel: Channel,
    assigned_ipv4: Ipv4Addr,
    assigned_ipv6: Ipv6Addr,
    servers: Vec<IpAddr>,
    mode: ProxyDnsMode,
    protector: Arc<dyn SocketProtector>,
}

impl Resolver {
    pub(crate) fn new(
        channel: Channel,
        assigned_ipv4: Ipv4Addr,
        assigned_ipv6: Ipv6Addr,
        servers: Vec<IpAddr>,
        mode: ProxyDnsMode,
        protector: Arc<dyn SocketProtector>,
    ) -> Self {
        Self {
            channel,
            assigned_ipv4,
            assigned_ipv6,
            servers,
            mode,
            protector,
        }
    }

    pub(crate) async fn resolve(&self, name: &str) -> Result<Vec<IpAddr>, TransportError> {
        if let Ok(address) = name.parse::<IpAddr>() {
            return Ok(vec![address]);
        }
        validate_name(name)?;

        let mut addresses = match self.mode {
            ProxyDnsMode::Remote => self.resolve_through_tunnel(name).await?,
            ProxyDnsMode::LocalConfigured => self.resolve_with_configured_servers(name).await?,
            ProxyDnsMode::System => lookup_host((name, 0))
                .await
                .map_err(|error| TransportError::Dns(error.to_string()))?
                .map(|address| address.ip())
                .collect(),
        };
        deduplicate(&mut addresses);
        if addresses.is_empty() {
            return Err(TransportError::Dns(format!(
                "no usable A or AAAA records were returned for {name}"
            )));
        }
        addresses.truncate(MAX_RESULTS);
        Ok(addresses)
    }

    async fn resolve_through_tunnel(&self, name: &str) -> Result<Vec<IpAddr>, TransportError> {
        let query_v4 = self.query_through_tunnel(name, TYPE_A);
        let query_v6 = self.query_through_tunnel(name, TYPE_AAAA);
        let (v4, v6) = tokio::join!(query_v4, query_v6);
        merge_query_results(v4, v6)
    }

    async fn resolve_with_configured_servers(
        &self,
        name: &str,
    ) -> Result<Vec<IpAddr>, TransportError> {
        let query_v4 = self.query_configured_server(name, TYPE_A);
        let query_v6 = self.query_configured_server(name, TYPE_AAAA);
        let (v4, v6) = tokio::join!(query_v4, query_v6);
        merge_query_results(v4, v6)
    }

    async fn query_through_tunnel(
        &self,
        name: &str,
        query_type: u16,
    ) -> Result<Vec<IpAddr>, TransportError> {
        let transaction_id = NEXT_DNS_ID.fetch_add(1, Ordering::Relaxed);
        let query = encode_query(transaction_id, name, query_type)?;
        let mut errors = Vec::new();

        for server in &self.servers {
            let local_ip = match server {
                IpAddr::V4(_) => IpAddr::V4(self.assigned_ipv4),
                IpAddr::V6(_) => IpAddr::V6(self.assigned_ipv6),
            };
            let local = SocketAddr::new(local_ip, next_udp_port());
            let remote = SocketAddr::new(*server, DNS_PORT);
            let socket = match self.channel.udp_bind(local).await {
                Ok(socket) => socket,
                Err(error) => {
                    errors.push(format!("{server}: bind failed: {error}"));
                    continue;
                }
            };
            if let Err(error) = socket.send_to(remote, &query).await {
                errors.push(format!("{server}: send failed: {error}"));
                continue;
            }
            let response = timeout(DNS_TIMEOUT, socket.recv_from_bytes()).await;
            match response {
                Ok(Ok((source, response))) if source.ip() == *server => {
                    match decode_response(&response, transaction_id, query_type) {
                        Ok(addresses) if !addresses.is_empty() => return Ok(addresses),
                        Ok(_) => errors.push(format!("{server}: empty response")),
                        Err(error) => errors.push(format!("{server}: {error}")),
                    }
                }
                Ok(Ok((source, _))) => {
                    errors.push(format!("{server}: response came from {source}"));
                }
                Ok(Err(error)) => errors.push(format!("{server}: receive failed: {error}")),
                Err(_) => errors.push(format!("{server}: timed out")),
            }
        }

        Err(TransportError::Dns(if errors.is_empty() {
            "no DNS server matches an assigned address family".to_owned()
        } else {
            errors.join("; ")
        }))
    }

    async fn query_configured_server(
        &self,
        name: &str,
        query_type: u16,
    ) -> Result<Vec<IpAddr>, TransportError> {
        let transaction_id = NEXT_DNS_ID.fetch_add(1, Ordering::Relaxed);
        let query = encode_query(transaction_id, name, query_type)?;
        let mut errors = Vec::new();

        for server in &self.servers {
            let remote = SocketAddr::new(*server, DNS_PORT);
            match query_local_server(
                self.protector.as_ref(),
                remote,
                &query,
                transaction_id,
                query_type,
            )
            .await
            {
                Ok(addresses) if !addresses.is_empty() => return Ok(addresses),
                Ok(_) => errors.push(format!("{server}: empty response")),
                Err(error) => errors.push(format!("{server}: {error}")),
            }
        }

        Err(TransportError::Dns(if errors.is_empty() {
            "no configured DNS servers are available".to_owned()
        } else {
            errors.join("; ")
        }))
    }
}

fn merge_query_results(
    v4: Result<Vec<IpAddr>, TransportError>,
    v6: Result<Vec<IpAddr>, TransportError>,
) -> Result<Vec<IpAddr>, TransportError> {
    let mut addresses = Vec::new();
    let mut errors = Vec::new();
    match v4 {
        Ok(mut values) => addresses.append(&mut values),
        Err(error) => errors.push(error.to_string()),
    }
    match v6 {
        Ok(mut values) => addresses.append(&mut values),
        Err(error) => errors.push(error.to_string()),
    }
    if addresses.is_empty() {
        return Err(TransportError::Dns(errors.join("; ")));
    }
    Ok(addresses)
}

async fn query_local_server(
    protector: &dyn SocketProtector,
    remote: SocketAddr,
    query: &[u8],
    transaction_id: u16,
    query_type: u16,
) -> Result<Vec<IpAddr>, TransportError> {
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

    let sent = timeout(DNS_TIMEOUT, socket.send_to(query, remote))
        .await
        .map_err(|_| TransportError::Dns(format!("send to {remote} timed out")))??;
    if sent != query.len() {
        return Err(TransportError::Dns(format!(
            "send to {remote} wrote only {sent} of {} bytes",
            query.len()
        )));
    }

    let mut response = [0u8; MAX_DNS_PACKET];
    let (length, source) = timeout(DNS_TIMEOUT, socket.recv_from(&mut response))
        .await
        .map_err(|_| TransportError::Dns(format!("response from {remote} timed out")))??;
    if source.ip() != remote.ip() || source.port() != remote.port() {
        return Err(TransportError::Dns(format!(
            "response came from {source} instead of {remote}"
        )));
    }
    decode_response(&response[..length], transaction_id, query_type)
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
    if rcode != 0 {
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
                _ => {}
            }
        }
        offset = end;
    }
    Ok(addresses)
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
        let addresses =
            query_local_server(&NoopSocketProtector, server_address, &query, 0x1234, TYPE_A)
                .await
                .unwrap();
        responder.await.unwrap();

        assert_eq!(addresses, vec![IpAddr::V4(Ipv4Addr::new(203, 0, 113, 9))]);
    }
}
