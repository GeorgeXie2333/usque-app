use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bytes::{Bytes, BytesMut};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use ts_netstack_smoltcp::CreateSocket;
use ts_netstack_smoltcp::netcore::{Channel, HasChannel, NetstackControl};
use ts_netstack_smoltcp::netsock::{TcpListener as StackTcpListener, UdpSocket as StackUdpSocket};
use usque_core::Profile;

use crate::geo_direct::{GeoDirectPolicy, GeoRoute, bind_protected_udp, connect_direct_ip};
use crate::h2::TransportError;
use crate::netstack::{TrafficCounters, bounded_piped, proxy_netstack_config};
use crate::socket::SocketProtector;

const GATEWAY_IPV4: Ipv4Addr = Ipv4Addr::new(198, 18, 0, 1);
const GATEWAY_IPV6: Ipv6Addr = Ipv6Addr::new(0xfd00, 0, 0, 0, 0, 0, 0, 1);
const FIRST_DYNAMIC_PORT: u16 = 49_152;
const LAST_DYNAMIC_PORT: u16 = 65_534;
const MAX_DIRECT_FLOWS: usize = 4_096;
const MAX_DIRECT_UDP_FLOWS: usize = 256;
const FLOW_IDLE_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const SWEEP_INTERVAL: Duration = Duration::from_secs(30);
const DIRECT_PACKET_CAPACITY: usize = 1_024;
const MAX_UDP_DATAGRAM: usize = 65_535;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct FlowKey {
    protocol: u8,
    client: SocketAddr,
    remote: SocketAddr,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct PortKey {
    protocol: u8,
    ipv6: bool,
    port: u16,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct ReverseKey {
    protocol: u8,
    gateway: SocketAddr,
    client: SocketAddr,
}

#[derive(Clone)]
struct Mapping {
    id: u64,
    gateway: SocketAddr,
    network_generation: Option<u64>,
    cancel: CancellationToken,
    last_seen: Instant,
}

struct NatTable {
    forward: HashMap<FlowKey, Mapping>,
    reverse: HashMap<ReverseKey, FlowKey>,
    ports: HashMap<PortKey, u64>,
    udp_flows: usize,
    next_port: u16,
    next_id: u64,
    last_sweep: Instant,
}

impl Default for NatTable {
    fn default() -> Self {
        Self {
            forward: HashMap::new(),
            reverse: HashMap::new(),
            ports: HashMap::new(),
            udp_flows: 0,
            next_port: FIRST_DYNAMIC_PORT,
            next_id: 1,
            last_sweep: Instant::now(),
        }
    }
}

impl NatTable {
    fn existing(&mut self, flow: &FlowKey, network_generation: Option<u64>) -> Option<Mapping> {
        self.sweep_if_needed();
        if self
            .forward
            .get(flow)
            .is_some_and(|mapping| mapping.network_generation != network_generation)
        {
            self.remove(flow);
            return None;
        }
        let mapping = self.forward.get_mut(flow)?;
        mapping.last_seen = Instant::now();
        Some(mapping.clone())
    }

    fn reserve(
        &mut self,
        flow: FlowKey,
        network_generation: Option<u64>,
        parent: &CancellationToken,
    ) -> Option<Mapping> {
        self.sweep_if_needed();
        if self.forward.len() >= MAX_DIRECT_FLOWS
            || flow.protocol == 17 && self.udp_flows >= MAX_DIRECT_UDP_FLOWS
        {
            return None;
        }
        let ipv6 = flow.client.is_ipv6();
        let is_udp = flow.protocol == 17;
        let port = self.allocate_port(flow.protocol, ipv6)?;
        let gateway = SocketAddr::new(gateway_ip(ipv6), port);
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        let mapping = Mapping {
            id,
            gateway,
            network_generation,
            cancel: parent.child_token(),
            last_seen: Instant::now(),
        };
        self.ports.insert(
            PortKey {
                protocol: flow.protocol,
                ipv6,
                port,
            },
            id,
        );
        self.reverse.insert(
            ReverseKey {
                protocol: flow.protocol,
                gateway,
                client: flow.client,
            },
            flow.clone(),
        );
        self.forward.insert(flow, mapping.clone());
        if is_udp {
            self.udp_flows += 1;
        }
        Some(mapping)
    }

    fn reverse(&mut self, packet: &NatPacket) -> Option<FlowKey> {
        self.sweep_if_needed();
        let key = ReverseKey {
            protocol: packet.protocol,
            gateway: SocketAddr::new(packet.source, packet.source_port),
            client: SocketAddr::new(packet.destination, packet.destination_port),
        };
        let flow = self.reverse.get(&key)?.clone();
        self.forward.get_mut(&flow)?.last_seen = Instant::now();
        Some(flow)
    }

    fn remove_if(&mut self, flow: &FlowKey, id: u64) {
        if self
            .forward
            .get(flow)
            .is_some_and(|mapping| mapping.id == id)
        {
            self.remove(flow);
        }
    }

    fn remove(&mut self, flow: &FlowKey) {
        let Some(mapping) = self.forward.remove(flow) else {
            return;
        };
        mapping.cancel.cancel();
        self.reverse.remove(&ReverseKey {
            protocol: flow.protocol,
            gateway: mapping.gateway,
            client: flow.client,
        });
        self.ports.remove(&PortKey {
            protocol: flow.protocol,
            ipv6: mapping.gateway.is_ipv6(),
            port: mapping.gateway.port(),
        });
        if flow.protocol == 17 {
            self.udp_flows = self.udp_flows.saturating_sub(1);
        }
    }

    fn allocate_port(&mut self, protocol: u8, ipv6: bool) -> Option<u16> {
        for _ in FIRST_DYNAMIC_PORT..=LAST_DYNAMIC_PORT {
            let candidate = self.next_port;
            self.next_port = if self.next_port == LAST_DYNAMIC_PORT {
                FIRST_DYNAMIC_PORT
            } else {
                self.next_port + 1
            };
            if !self.ports.contains_key(&PortKey {
                protocol,
                ipv6,
                port: candidate,
            }) {
                return Some(candidate);
            }
        }
        None
    }

    fn sweep_if_needed(&mut self) {
        let now = Instant::now();
        if now.duration_since(self.last_sweep) < SWEEP_INTERVAL {
            return;
        }
        self.last_sweep = now;
        let stale = self
            .forward
            .iter()
            .filter(|(_, mapping)| now.duration_since(mapping.last_seen) >= FLOW_IDLE_TIMEOUT)
            .map(|(flow, _)| flow.clone())
            .collect::<Vec<_>>();
        for flow in stale {
            self.remove(&flow);
        }
    }

    fn cancel_all(&mut self) {
        for mapping in self.forward.values() {
            mapping.cancel.cancel();
        }
        self.forward.clear();
        self.reverse.clear();
        self.ports.clear();
        self.udp_flows = 0;
    }
}

/// Owns the Android-safe userspace NAT used for IP-classified TUN flows.
///
/// Platforms opt in through [`SocketProtector::tun_direct_available`]. An
/// unavailable gateway returns every packet to the MASQUE path.
pub(crate) struct DirectGatewayRouter {
    channel: Option<Channel>,
    stack_incoming: Option<ts_netstack_smoltcp::WakingPipeSender>,
    policy: Arc<GeoDirectPolicy>,
    protector: Arc<dyn SocketProtector>,
    counters: Arc<TrafficCounters>,
    flows: Arc<Mutex<NatTable>>,
    cancellation: CancellationToken,
    stack_task: Option<JoinHandle<()>>,
    incoming_task: Option<JoinHandle<()>>,
}

impl DirectGatewayRouter {
    pub(crate) async fn start(
        profile: &Profile,
        policy: Arc<GeoDirectPolicy>,
        protector: Arc<dyn SocketProtector>,
        counters: Arc<TrafficCounters>,
        parent_cancellation: &CancellationToken,
    ) -> Result<(Self, mpsc::Receiver<Bytes>), TransportError> {
        let (incoming_tx, incoming_rx) = mpsc::channel(DIRECT_PACKET_CAPACITY);
        let cancellation = parent_cancellation.child_token();
        let flows = Arc::new(Mutex::new(NatTable::default()));
        if !policy.is_enabled() || !protector.tun_direct_available() {
            return Ok((
                Self {
                    channel: None,
                    stack_incoming: None,
                    policy,
                    protector,
                    counters,
                    flows,
                    cancellation,
                    stack_task: None,
                    incoming_task: None,
                },
                incoming_rx,
            ));
        }

        let (config, _) = proxy_netstack_config(profile);
        let (stack, pipe) = bounded_piped(config);
        let channel = stack.command_channel();
        let stack_task = stack.spawn_tokio();
        if let Err(error) = channel
            .set_ips([IpAddr::V4(GATEWAY_IPV4), IpAddr::V6(GATEWAY_IPV6)])
            .await
        {
            stack_task.abort();
            return Err(TransportError::Netstack(error.to_string()));
        }
        let ts_netstack_smoltcp::WakingPipe {
            mut rx,
            tx: stack_incoming,
        } = pipe;
        let incoming_flows = Arc::clone(&flows);
        let incoming_cancel = cancellation.clone();
        let incoming_counters = Arc::clone(&counters);
        let incoming_task = tokio::spawn(async move {
            loop {
                let packet = tokio::select! {
                    _ = incoming_cancel.cancelled() => break,
                    packet = rx.recv_async() => packet,
                };
                let Some(packet) = packet else {
                    break;
                };
                let mut packet = packet
                    .try_into_mut()
                    .unwrap_or_else(|packet| BytesMut::from(packet.as_ref()));
                if route_incoming(&incoming_flows, &mut packet) {
                    incoming_counters.record_received(packet.len());
                    match incoming_tx.try_send(packet.freeze()) {
                        Ok(()) => {}
                        Err(mpsc::error::TrySendError::Full(_)) => {}
                        Err(mpsc::error::TrySendError::Closed(_)) => break,
                    }
                }
            }
        });

        Ok((
            Self {
                channel: Some(channel),
                stack_incoming: Some(stack_incoming),
                policy,
                protector,
                counters,
                flows,
                cancellation,
                stack_task: Some(stack_task),
                incoming_task: Some(incoming_task),
            },
            incoming_rx,
        ))
    }

    /// Returns true when the packet was consumed by the direct gateway.
    pub(crate) async fn route_outgoing(&mut self, packet: &mut BytesMut) -> bool {
        if self
            .stack_task
            .as_ref()
            .is_some_and(JoinHandle::is_finished)
            || self
                .incoming_task
                .as_ref()
                .is_some_and(JoinHandle::is_finished)
        {
            return false;
        }
        let Some(channel) = self.channel.as_ref() else {
            return false;
        };
        let Some(stack_incoming) = self.stack_incoming.as_ref() else {
            return false;
        };
        let Some(parsed) = NatPacket::parse(packet) else {
            return false;
        };
        if !matches!(parsed.protocol, 6 | 17)
            || parsed.source.is_ipv4() != parsed.destination.is_ipv4()
            || self.policy.route_ip(parsed.destination) != GeoRoute::Direct
        {
            return false;
        }
        let flow = FlowKey {
            protocol: parsed.protocol,
            client: SocketAddr::new(parsed.source, parsed.source_port),
            remote: SocketAddr::new(parsed.destination, parsed.destination_port),
        };
        let network_generation = self.protector.network_generation();
        let existing = self
            .flows
            .lock()
            .ok()
            .and_then(|mut flows| flows.existing(&flow, network_generation));
        let (mapping, newly_reserved) = if let Some(mapping) = existing {
            (mapping, false)
        } else {
            let Some(mapping) = self.flows.lock().ok().and_then(|mut flows| {
                flows.reserve(flow.clone(), network_generation, &self.cancellation)
            }) else {
                return false;
            };
            let setup = match parsed.protocol {
                6 => channel
                    .tcp_listen(mapping.gateway)
                    .await
                    .map(DirectSocket::Tcp)
                    .map_err(|error| error.to_string()),
                17 => self.setup_udp(channel, &flow, &mapping).await,
                _ => unreachable!(),
            };
            let socket = match setup {
                Ok(socket) => socket,
                Err(error) => {
                    tracing::debug!(%error, remote = %flow.remote, "could not create GEO direct flow; using tunnel");
                    if let Ok(mut flows) = self.flows.lock() {
                        flows.remove_if(&flow, mapping.id);
                    }
                    return false;
                }
            };
            self.spawn_flow(flow.clone(), mapping.clone(), socket);
            (mapping, true)
        };

        let original = packet.clone();
        if !rewrite_destination(packet, &parsed, mapping.gateway) {
            if newly_reserved && let Ok(mut flows) = self.flows.lock() {
                flows.remove_if(&flow, mapping.id);
            }
            *packet = original;
            return false;
        }
        stack_incoming.send_async(packet).await;
        self.counters.record_sent(packet.len());
        true
    }

    async fn setup_udp(
        &self,
        channel: &Channel,
        flow: &FlowKey,
        mapping: &Mapping,
    ) -> Result<DirectSocket, String> {
        let physical = bind_protected_udp(self.protector.as_ref(), flow.remote.is_ipv6())?;
        physical
            .connect(flow.remote)
            .await
            .map_err(|error| error.to_string())?;
        let local = channel
            .udp_bind(mapping.gateway)
            .await
            .map_err(|error| error.to_string())?;
        Ok(DirectSocket::Udp { local, physical })
    }

    fn spawn_flow(&self, flow: FlowKey, mapping: Mapping, socket: DirectSocket) {
        let protector = Arc::clone(&self.protector);
        let flows = Arc::clone(&self.flows);
        tokio::spawn(async move {
            let result = match socket {
                DirectSocket::Tcp(listener) => {
                    run_tcp_flow(listener, &flow, protector, &mapping.cancel).await
                }
                DirectSocket::Udp { local, physical } => {
                    run_udp_flow(local, physical, &flow, &mapping.cancel).await
                }
            };
            if let Err(error) = result {
                if let Ok(mut flows) = flows.lock() {
                    flows.remove_if(&flow, mapping.id);
                }
                tracing::debug!(%error, remote = %flow.remote, "GEO direct flow ended");
            }
        });
    }
}

impl Drop for DirectGatewayRouter {
    fn drop(&mut self) {
        self.cancellation.cancel();
        if let Ok(mut flows) = self.flows.lock() {
            flows.cancel_all();
        }
        if let Some(task) = self.stack_task.as_ref() {
            task.abort();
        }
        if let Some(task) = self.incoming_task.as_ref() {
            task.abort();
        }
    }
}

enum DirectSocket {
    Tcp(StackTcpListener),
    Udp {
        local: StackUdpSocket,
        physical: tokio::net::UdpSocket,
    },
}

async fn run_tcp_flow(
    listener: StackTcpListener,
    flow: &FlowKey,
    protector: Arc<dyn SocketProtector>,
    cancellation: &CancellationToken,
) -> Result<(), String> {
    let accepted = tokio::select! {
        _ = cancellation.cancelled() => return Ok(()),
        accepted = listener.accept() => accepted.map_err(|error| error.to_string())?,
    };
    if accepted.remote_addr() != flow.client {
        return Err(format!(
            "direct TCP peer {} did not match {}",
            accepted.remote_addr(),
            flow.client
        ));
    }
    let mut local = accepted;
    let mut physical = tokio::select! {
        _ = cancellation.cancelled() => return Ok(()),
        connected = connect_direct_ip(protector.as_ref(), flow.remote) => connected?,
    };
    tokio::select! {
        _ = cancellation.cancelled() => Ok(()),
        result = crate::relay::copy_bidirectional(&mut local, &mut physical) => {
            result.map(|_| ()).map_err(|error| error.to_string())
        }
    }
}

async fn run_udp_flow(
    local: StackUdpSocket,
    physical: tokio::net::UdpSocket,
    flow: &FlowKey,
    cancellation: &CancellationToken,
) -> Result<(), String> {
    let mut buffer = vec![0u8; MAX_UDP_DATAGRAM];
    loop {
        tokio::select! {
            _ = cancellation.cancelled() => return Ok(()),
            received = local.recv_from_bytes() => {
                let (source, payload) = received.map_err(|error| error.to_string())?;
                if source != flow.client {
                    continue;
                }
                let written = physical.send(&payload).await.map_err(|error| error.to_string())?;
                if written != payload.len() {
                    return Err(format!("direct UDP wrote {written} of {} bytes", payload.len()));
                }
            }
            received = physical.recv(&mut buffer) => {
                let length = received.map_err(|error| error.to_string())?;
                local
                    .send_to(flow.client, &buffer[..length])
                    .await
                    .map_err(|error| error.to_string())?;
            }
        }
    }
}

fn route_incoming(flows: &Arc<Mutex<NatTable>>, packet: &mut BytesMut) -> bool {
    let Some(parsed) = NatPacket::parse(packet) else {
        return false;
    };
    let Some(flow) = flows
        .lock()
        .ok()
        .and_then(|mut flows| flows.reverse(&parsed))
    else {
        return false;
    };
    rewrite_source(packet, &parsed, flow.remote)
}

#[derive(Clone, Copy, Debug)]
struct NatPacket {
    version: u8,
    protocol: u8,
    source: IpAddr,
    destination: IpAddr,
    source_port: u16,
    destination_port: u16,
    transport_offset: usize,
    checksum_offset: usize,
    checksum_optional: bool,
}

impl NatPacket {
    fn parse(packet: &[u8]) -> Option<Self> {
        let (version, protocol, source, destination, transport_offset) = match packet.first()? >> 4
        {
            4 => parse_ipv4(packet)?,
            6 => parse_ipv6(packet)?,
            _ => return None,
        };
        let (checksum_offset, checksum_optional, minimum_header) = match protocol {
            6 => (transport_offset.checked_add(16)?, false, 20),
            17 => (transport_offset.checked_add(6)?, version == 4, 8),
            _ => return None,
        };
        if packet.len() < transport_offset.checked_add(minimum_header)? {
            return None;
        }
        Some(Self {
            version,
            protocol,
            source,
            destination,
            source_port: read_u16(packet, transport_offset)?,
            destination_port: read_u16(packet, transport_offset + 2)?,
            transport_offset,
            checksum_offset,
            checksum_optional,
        })
    }
}

fn parse_ipv4(packet: &[u8]) -> Option<(u8, u8, IpAddr, IpAddr, usize)> {
    if packet.len() < 20 {
        return None;
    }
    let header_length = usize::from(packet[0] & 0x0f).checked_mul(4)?;
    // IPv4 source-routing and other options can change the address used by
    // the transport pseudo-header. Keep every option-bearing packet on the
    // unmodified tunnel path.
    if header_length != 20 || packet.len() < header_length {
        return None;
    }
    let flags_offset = read_u16(packet, 6)?;
    if flags_offset & 0xbfff != 0 {
        return None;
    }
    let source = IpAddr::V4(Ipv4Addr::new(
        packet[12], packet[13], packet[14], packet[15],
    ));
    let destination = IpAddr::V4(Ipv4Addr::new(
        packet[16], packet[17], packet[18], packet[19],
    ));
    Some((4, packet[9], source, destination, header_length))
}

fn parse_ipv6(packet: &[u8]) -> Option<(u8, u8, IpAddr, IpAddr, usize)> {
    if packet.len() < 40 {
        return None;
    }
    let source = IpAddr::V6(Ipv6Addr::from(<[u8; 16]>::try_from(&packet[8..24]).ok()?));
    let destination = IpAddr::V6(Ipv6Addr::from(<[u8; 16]>::try_from(&packet[24..40]).ok()?));
    // Routing, AH, Home Address, fragmentation, and other extension-header
    // semantics cannot be preserved by this endpoint-only NAT. Unknown and
    // extended packets therefore stay on MASQUE unchanged.
    let protocol = packet[6];
    matches!(protocol, 6 | 17).then_some((6, protocol, source, destination, 40))
}

fn rewrite_destination(packet: &mut [u8], parsed: &NatPacket, gateway: SocketAddr) -> bool {
    rewrite_endpoint(
        packet,
        parsed,
        parsed.destination,
        gateway.ip(),
        parsed.destination_port,
        gateway.port(),
        false,
    )
}

fn rewrite_source(packet: &mut [u8], parsed: &NatPacket, remote: SocketAddr) -> bool {
    rewrite_endpoint(
        packet,
        parsed,
        parsed.source,
        remote.ip(),
        parsed.source_port,
        remote.port(),
        true,
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "checksum-safe NAT rewrite needs old/new address, old/new port, and source/destination direction"
)]
fn rewrite_endpoint(
    packet: &mut [u8],
    parsed: &NatPacket,
    old_ip: IpAddr,
    new_ip: IpAddr,
    old_port: u16,
    new_port: u16,
    source: bool,
) -> bool {
    if old_ip.is_ipv4() != new_ip.is_ipv4() {
        return false;
    }
    let (address_offset, address_length) = match (parsed.version, source) {
        (4, true) => (12, 4),
        (4, false) => (16, 4),
        (6, true) => (8, 16),
        (6, false) => (24, 16),
        _ => return false,
    };
    let port_offset = parsed.transport_offset + if source { 0 } else { 2 };
    if packet
        .get(address_offset..address_offset + address_length)
        .is_none()
        || packet.get(port_offset..port_offset + 2).is_none()
        || packet
            .get(parsed.checksum_offset..parsed.checksum_offset + 2)
            .is_none()
        || parsed.version == 4 && packet.get(10..12).is_none()
    {
        return false;
    }
    let old_bytes = ip_bytes(old_ip);
    let new_bytes = ip_bytes(new_ip);
    if old_bytes.len() != address_length || new_bytes.len() != address_length {
        return false;
    }
    let transport_checksum = read_u16(packet, parsed.checksum_offset).unwrap_or_default();
    let has_transport_checksum = !(parsed.checksum_optional && transport_checksum == 0);
    let mut updated_transport = transport_checksum;
    let mut updated_ipv4 = (parsed.version == 4).then(|| read_u16(packet, 10).unwrap_or_default());
    for (old, new) in old_bytes.chunks_exact(2).zip(new_bytes.chunks_exact(2)) {
        let old = u16::from_be_bytes([old[0], old[1]]);
        let new = u16::from_be_bytes([new[0], new[1]]);
        if has_transport_checksum {
            updated_transport = update_checksum(updated_transport, old, new);
        }
        if let Some(checksum) = updated_ipv4.as_mut() {
            *checksum = update_checksum(*checksum, old, new);
        }
    }
    if has_transport_checksum {
        updated_transport = update_checksum(updated_transport, old_port, new_port);
        if parsed.protocol == 17 && updated_transport == 0 {
            updated_transport = u16::MAX;
        }
        write_u16(packet, parsed.checksum_offset, updated_transport);
    }
    if let Some(checksum) = updated_ipv4 {
        write_u16(packet, 10, checksum);
    }
    packet[address_offset..address_offset + address_length].copy_from_slice(&new_bytes);
    write_u16(packet, port_offset, new_port);
    true
}

fn gateway_ip(ipv6: bool) -> IpAddr {
    if ipv6 {
        IpAddr::V6(GATEWAY_IPV6)
    } else {
        IpAddr::V4(GATEWAY_IPV4)
    }
}

fn ip_bytes(ip: IpAddr) -> Vec<u8> {
    match ip {
        IpAddr::V4(ip) => ip.octets().to_vec(),
        IpAddr::V6(ip) => ip.octets().to_vec(),
    }
}

fn update_checksum(checksum: u16, old: u16, new: u16) -> u16 {
    let mut sum = u32::from(!checksum) + u32::from(!old) + u32::from(new);
    while sum >> 16 != 0 {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}

fn read_u16(packet: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_be_bytes([
        *packet.get(offset)?,
        *packet.get(offset + 1)?,
    ]))
}

fn write_u16(packet: &mut [u8], offset: usize, value: u16) {
    if let Some(bytes) = packet.get_mut(offset..offset + 2) {
        bytes.copy_from_slice(&value.to_be_bytes());
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, UdpSocket};
    use tokio::time::timeout;
    use ts_netstack_smoltcp::netcore::{HasChannel, NetstackControl};
    use usque_geo::CountryCode;

    use super::*;
    use crate::geo_direct::GeoDirectClassifier;
    use crate::socket::SocketHandle;

    const CLIENT_IPV4: Ipv4Addr = Ipv4Addr::new(10, 0, 0, 2);
    const CLIENT_IPV6: Ipv6Addr = Ipv6Addr::new(0xfd00, 0, 0, 0, 0, 0, 0, 2);
    const TEST_TIMEOUT: Duration = Duration::from_secs(3);

    struct LoopbackClassifier;

    impl GeoDirectClassifier for LoopbackClassifier {
        fn host_matches(&self, _host: &str, _country: &CountryCode) -> bool {
            false
        }

        fn ip_matches(&self, ip: IpAddr, country: &CountryCode) -> bool {
            ip.is_loopback() && country.as_str() == "CN"
        }
    }

    struct TestProtector {
        direct_available: bool,
        reject_protection: bool,
        protect_calls: AtomicUsize,
    }

    impl SocketProtector for TestProtector {
        fn protect(&self, _socket: SocketHandle) -> Result<(), String> {
            self.protect_calls.fetch_add(1, Ordering::SeqCst);
            if self.reject_protection {
                Err("test protection rejection".to_owned())
            } else {
                Ok(())
            }
        }

        fn tun_direct_available(&self) -> bool {
            self.direct_available
        }
    }

    fn direct_policy() -> Arc<GeoDirectPolicy> {
        Arc::new(GeoDirectPolicy::with_classifier(
            Arc::new(LoopbackClassifier),
            [CountryCode::parse("CN").unwrap()],
        ))
    }

    struct TestNetwork {
        channel: Channel,
        counters: Arc<TrafficCounters>,
        protector: Arc<TestProtector>,
        flows: Arc<Mutex<NatTable>>,
        client_task: JoinHandle<()>,
        pump_task: JoinHandle<()>,
        cancellation: CancellationToken,
    }

    impl TestNetwork {
        async fn start(client_ip: IpAddr) -> Self {
            Self::start_with_protection(client_ip, false).await
        }

        async fn start_with_protection(client_ip: IpAddr, reject_protection: bool) -> Self {
            let profile = Profile::default();
            let (client_stack, client_pipe) = bounded_piped(proxy_netstack_config(&profile).0);
            let channel = client_stack.command_channel();
            let client_task = client_stack.spawn_tokio();
            channel.set_ips([client_ip]).await.unwrap();

            let protector = Arc::new(TestProtector {
                direct_available: true,
                reject_protection,
                protect_calls: AtomicUsize::new(0),
            });
            let counters = Arc::new(TrafficCounters::default());
            let cancellation = CancellationToken::new();
            let protector_trait: Arc<dyn SocketProtector> = protector.clone();
            let (mut gateway, mut gateway_incoming) = DirectGatewayRouter::start(
                &profile,
                direct_policy(),
                protector_trait,
                Arc::clone(&counters),
                &cancellation,
            )
            .await
            .unwrap();
            let flows = Arc::clone(&gateway.flows);
            let ts_netstack_smoltcp::WakingPipe {
                mut rx,
                tx: client_incoming,
            } = client_pipe;
            let pump_cancel = cancellation.clone();
            let pump_task = tokio::spawn(async move {
                loop {
                    tokio::select! {
                        _ = pump_cancel.cancelled() => break,
                        packet = rx.recv_async() => {
                            let Some(packet) = packet else { break; };
                            let mut packet = packet
                                .try_into_mut()
                                .unwrap_or_else(|packet| BytesMut::from(packet.as_ref()));
                            assert!(gateway.route_outgoing(&mut packet).await);
                        }
                        packet = gateway_incoming.recv() => {
                            let Some(packet) = packet else { break; };
                            client_incoming.send_async(&packet).await;
                        }
                    }
                }
            });

            Self {
                channel,
                counters,
                protector,
                flows,
                client_task,
                pump_task,
                cancellation,
            }
        }
    }

    impl Drop for TestNetwork {
        fn drop(&mut self) {
            self.cancellation.cancel();
            self.client_task.abort();
            self.pump_task.abort();
        }
    }

    #[tokio::test]
    async fn protected_tcp_flow_round_trips_through_userspace_nat() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let remote = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut payload = [0u8; 6];
            stream.read_exact(&mut payload).await.unwrap();
            assert_eq!(&payload, b"direct");
            stream.write_all(b"return").await.unwrap();
        });
        let network = TestNetwork::start(IpAddr::V4(CLIENT_IPV4)).await;
        let local = SocketAddr::new(IpAddr::V4(CLIENT_IPV4), 50_001);
        let mut stream = timeout(TEST_TIMEOUT, network.channel.tcp_connect(local, remote))
            .await
            .expect("direct TCP handshake timed out")
            .unwrap();
        stream.write_all(b"direct").await.unwrap();
        let mut response = [0u8; 6];
        timeout(TEST_TIMEOUT, stream.read_exact(&mut response))
            .await
            .expect("direct TCP response timed out")
            .unwrap();
        assert_eq!(&response, b"return");
        timeout(TEST_TIMEOUT, server)
            .await
            .expect("TCP echo task timed out")
            .unwrap();
        assert!(network.protector.protect_calls.load(Ordering::SeqCst) >= 1);
        let snapshot = network.counters.snapshot();
        assert!(snapshot.bytes_sent > 0);
        assert!(snapshot.bytes_received > 0);
    }

    #[tokio::test]
    async fn protected_udp_flow_restores_the_original_remote_endpoint() {
        let server = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let remote = server.local_addr().unwrap();
        let server_task = tokio::spawn(async move {
            let mut payload = [0u8; 16];
            let (length, source) = server.recv_from(&mut payload).await.unwrap();
            assert_eq!(&payload[..length], b"direct-udp");
            server.send_to(b"return-udp", source).await.unwrap();
        });
        let network = TestNetwork::start(IpAddr::V4(CLIENT_IPV4)).await;
        let local = SocketAddr::new(IpAddr::V4(CLIENT_IPV4), 50_002);
        let socket = network.channel.udp_bind(local).await.unwrap();
        socket.send_to(remote, b"direct-udp").await.unwrap();
        let mut response = [0u8; 16];
        let (source, length) = timeout(TEST_TIMEOUT, socket.recv_from(&mut response))
            .await
            .expect("direct UDP response timed out")
            .unwrap();
        assert_eq!(source, remote);
        assert_eq!(&response[..length], b"return-udp");
        timeout(TEST_TIMEOUT, server_task)
            .await
            .expect("UDP echo task timed out")
            .unwrap();
        assert!(network.protector.protect_calls.load(Ordering::SeqCst) >= 1);
        let snapshot = network.counters.snapshot();
        assert!(snapshot.bytes_sent > 0);
        assert!(snapshot.bytes_received > 0);
    }

    #[tokio::test]
    async fn protected_ipv6_udp_flow_repairs_the_pseudo_header_checksum() {
        let server = UdpSocket::bind((Ipv6Addr::LOCALHOST, 0)).await.unwrap();
        let remote = server.local_addr().unwrap();
        let server_task = tokio::spawn(async move {
            let mut payload = [0u8; 16];
            let (length, source) = server.recv_from(&mut payload).await.unwrap();
            assert_eq!(&payload[..length], b"direct-v6");
            server.send_to(b"return-v6", source).await.unwrap();
        });
        let network = TestNetwork::start(IpAddr::V6(CLIENT_IPV6)).await;
        let local = SocketAddr::new(IpAddr::V6(CLIENT_IPV6), 50_004);
        let socket = network.channel.udp_bind(local).await.unwrap();
        socket.send_to(remote, b"direct-v6").await.unwrap();
        let mut response = [0u8; 16];
        let (source, length) = timeout(TEST_TIMEOUT, socket.recv_from(&mut response))
            .await
            .expect("direct IPv6 UDP response timed out")
            .unwrap();
        assert_eq!(source, remote);
        assert_eq!(&response[..length], b"return-v6");
        timeout(TEST_TIMEOUT, server_task)
            .await
            .expect("IPv6 UDP echo task timed out")
            .unwrap();
        assert!(network.protector.protect_calls.load(Ordering::SeqCst) >= 1);
    }

    #[tokio::test]
    async fn platform_without_safe_tun_bypass_never_consumes_packets() {
        let profile = Profile::default();
        let protector = Arc::new(TestProtector {
            direct_available: false,
            reject_protection: false,
            protect_calls: AtomicUsize::new(0),
        });
        let protector_trait: Arc<dyn SocketProtector> = protector.clone();
        let cancellation = CancellationToken::new();
        let (mut gateway, mut incoming) = DirectGatewayRouter::start(
            &profile,
            direct_policy(),
            protector_trait,
            Arc::new(TrafficCounters::default()),
            &cancellation,
        )
        .await
        .unwrap();
        let original = BytesMut::from(&[0x45, 0, 0, 20][..]);
        let mut packet = original.clone();
        assert!(!gateway.route_outgoing(&mut packet).await);
        assert_eq!(packet, original);
        assert!(incoming.recv().await.is_none());
        assert_eq!(protector.protect_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn failed_direct_worker_releases_its_nat_mapping() {
        let network = TestNetwork::start_with_protection(IpAddr::V4(CLIENT_IPV4), true).await;
        let local = SocketAddr::new(IpAddr::V4(CLIENT_IPV4), 50_005);
        let remote = SocketAddr::from((Ipv4Addr::LOCALHOST, 9));
        let channel = network.channel.clone();
        let connect = tokio::spawn(async move { channel.tcp_connect(local, remote).await });

        timeout(TEST_TIMEOUT, async {
            loop {
                let attempted = network.protector.protect_calls.load(Ordering::SeqCst) > 0;
                let empty = network
                    .flows
                    .lock()
                    .is_ok_and(|flows| flows.forward.is_empty());
                if attempted && empty {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("failed direct flow kept its NAT mapping");
        connect.abort();

        let flows = network.flows.lock().expect("NAT table");
        assert!(flows.reverse.is_empty());
        assert!(flows.ports.is_empty());
    }

    #[test]
    fn nat_parser_keeps_options_extensions_and_fragments_tunneled() {
        let mut ipv4_options = vec![0u8; 44];
        ipv4_options[0] = 0x46;
        ipv4_options[9] = 6;
        assert!(NatPacket::parse(&ipv4_options).is_none());

        let mut ipv4_fragment = vec![0u8; 28];
        ipv4_fragment[0] = 0x45;
        ipv4_fragment[6..8].copy_from_slice(&0x2000u16.to_be_bytes());
        ipv4_fragment[9] = 17;
        assert!(NatPacket::parse(&ipv4_fragment).is_none());

        let mut ipv6_extension = vec![0u8; 68];
        ipv6_extension[0] = 0x60;
        ipv6_extension[6] = 60;
        ipv6_extension[40] = 17;
        assert!(NatPacket::parse(&ipv6_extension).is_none());

        let mut plain_ipv6_tcp = vec![0u8; 60];
        plain_ipv6_tcp[0] = 0x60;
        plain_ipv6_tcp[6] = 6;
        assert!(NatPacket::parse(&plain_ipv6_tcp).is_some());
    }

    #[test]
    fn udp_checksum_zero_is_encoded_as_all_ones_after_nat() {
        let old_ip = IpAddr::V6(Ipv6Addr::LOCALHOST);
        let new_ip = IpAddr::V6(GATEWAY_IPV6);
        let old_port = 53;
        let new_port = 60_000;
        let initial_checksum = (1..=u16::MAX)
            .find(|candidate| {
                let mut updated = *candidate;
                for (old, new) in ip_bytes(old_ip)
                    .chunks_exact(2)
                    .zip(ip_bytes(new_ip).chunks_exact(2))
                {
                    updated = update_checksum(
                        updated,
                        u16::from_be_bytes([old[0], old[1]]),
                        u16::from_be_bytes([new[0], new[1]]),
                    );
                }
                update_checksum(updated, old_port, new_port) == 0
            })
            .expect("checksum yielding positive zero");

        let mut packet = vec![0u8; 48];
        packet[0] = 0x60;
        packet[4..6].copy_from_slice(&8u16.to_be_bytes());
        packet[6] = 17;
        packet[8..24].copy_from_slice(&CLIENT_IPV6.octets());
        packet[24..40].copy_from_slice(&Ipv6Addr::LOCALHOST.octets());
        packet[40..42].copy_from_slice(&50_006u16.to_be_bytes());
        packet[42..44].copy_from_slice(&old_port.to_be_bytes());
        packet[44..46].copy_from_slice(&8u16.to_be_bytes());
        packet[46..48].copy_from_slice(&initial_checksum.to_be_bytes());
        let parsed = NatPacket::parse(&packet).expect("IPv6 UDP packet");

        assert!(rewrite_destination(
            &mut packet,
            &parsed,
            SocketAddr::new(new_ip, new_port),
        ));
        assert_eq!(read_u16(&packet, parsed.checksum_offset), Some(u16::MAX));
    }

    #[test]
    fn network_change_cancels_and_replaces_a_direct_mapping() {
        let flow = FlowKey {
            protocol: 17,
            client: SocketAddr::from((CLIENT_IPV4, 50_003)),
            remote: SocketAddr::from((Ipv4Addr::LOCALHOST, 53)),
        };
        let cancellation = CancellationToken::new();
        let mut table = NatTable::default();
        let first = table.reserve(flow.clone(), Some(1), &cancellation).unwrap();
        assert_eq!(
            table.existing(&flow, Some(1)).map(|mapping| mapping.id),
            Some(first.id)
        );
        assert!(table.existing(&flow, Some(2)).is_none());
        assert!(first.cancel.is_cancelled());

        let second = table.reserve(flow, Some(2), &cancellation).unwrap();
        assert_ne!(second.id, first.id);
        assert_eq!(second.network_generation, Some(2));
    }

    #[test]
    fn udp_flow_table_has_a_memory_bound() {
        let cancellation = CancellationToken::new();
        let mut table = NatTable::default();
        for index in 0..MAX_DIRECT_UDP_FLOWS {
            let source_port = 10_000 + u16::try_from(index).unwrap();
            let flow = FlowKey {
                protocol: 17,
                client: SocketAddr::from((CLIENT_IPV4, source_port)),
                remote: SocketAddr::from((Ipv4Addr::LOCALHOST, 53)),
            };
            assert!(table.reserve(flow, Some(1), &cancellation).is_some());
        }
        let overflow = FlowKey {
            protocol: 17,
            client: SocketAddr::from((CLIENT_IPV4, 20_000)),
            remote: SocketAddr::from((Ipv4Addr::LOCALHOST, 53)),
        };
        assert!(table.reserve(overflow, Some(1), &cancellation).is_none());
        assert_eq!(table.udp_flows, MAX_DIRECT_UDP_FLOWS);
    }
}
