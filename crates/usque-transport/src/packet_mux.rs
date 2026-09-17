use std::collections::{HashMap, VecDeque};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::application_traffic::ApplicationTrafficPolicy;

const MAX_FLOWS: usize = 65_536;
const MAX_FRAGMENTS: usize = 8_192;
const MAINTENANCE_ITEMS: usize = 4_096;
const FLOW_IDLE: Duration = Duration::from_secs(5 * 60);
pub(crate) const MAINTENANCE_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum PacketOrigin {
    Tunnel,
    Proxy,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct FlowKey {
    origin: PacketOrigin,
    protocol: u8,
    local_address: IpAddr,
    local_id: u16,
    remote_address: IpAddr,
    remote_id: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct WireKey {
    protocol: u8,
    local_address: IpAddr,
    local_id: u16,
    remote_address: IpAddr,
    remote_id: u16,
}

#[derive(Debug, Clone)]
struct FlowMapping {
    wire: WireKey,
    original_id: u16,
    last_seen: Instant,
}

#[derive(Debug, Clone)]
struct ReverseMapping {
    flow: FlowKey,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct FragmentKey {
    source: IpAddr,
    destination: IpAddr,
    protocol: u8,
    identifier: u32,
    identifier_offset: usize,
    identifier_width: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct OriginFragmentKey {
    origin: PacketOrigin,
    fragment: FragmentKey,
}

#[derive(Debug, Clone)]
struct FragmentMapping {
    wire: FragmentKey,
    last_seen: Instant,
    application_quic: bool,
}

#[derive(Debug)]
pub(crate) struct PacketMuxTable {
    traffic_policy: Arc<ApplicationTrafficPolicy>,
    forward: HashMap<FlowKey, FlowMapping>,
    reverse: HashMap<WireKey, ReverseMapping>,
    outgoing_fragments: HashMap<OriginFragmentKey, FragmentMapping>,
    wire_fragments: HashMap<FragmentKey, OriginFragmentKey>,
    incoming_fragments: HashMap<FragmentKey, (PacketOrigin, Instant, bool)>,
    next_id: u16,
    next_fragment_id: u32,
    flow_scan: VecDeque<FlowKey>,
    outgoing_fragment_scan: VecDeque<OriginFragmentKey>,
    incoming_fragment_scan: VecDeque<FragmentKey>,
    flow_limit: usize,
    outgoing_fragment_limit: usize,
    incoming_fragment_limit: usize,
    rejections: [u64; 3],
    reported_rejections: [u64; 3],
    last_report: Instant,
}

pub(crate) struct OutgoingPacketInspection {
    origin: PacketOrigin,
    parsed: Option<ParsedPacket>,
    owned: bool,
}

impl OutgoingPacketInspection {
    pub(crate) fn is_owned(&self) -> bool {
        self.owned
    }
}

impl Default for PacketMuxTable {
    fn default() -> Self {
        Self {
            traffic_policy: Arc::default(),
            forward: HashMap::new(),
            reverse: HashMap::new(),
            outgoing_fragments: HashMap::new(),
            wire_fragments: HashMap::new(),
            incoming_fragments: HashMap::new(),
            next_id: 49_152,
            next_fragment_id: 0x8000_0000,
            flow_scan: VecDeque::new(),
            outgoing_fragment_scan: VecDeque::new(),
            incoming_fragment_scan: VecDeque::new(),
            flow_limit: MAX_FLOWS,
            outgoing_fragment_limit: MAX_FRAGMENTS,
            incoming_fragment_limit: MAX_FRAGMENTS,
            rejections: [0; 3],
            reported_rejections: [0; 3],
            last_report: Instant::now(),
        }
    }
}

impl PacketMuxTable {
    pub(crate) fn with_traffic_policy(traffic_policy: Arc<ApplicationTrafficPolicy>) -> Self {
        Self {
            traffic_policy,
            ..Self::default()
        }
    }
    /// Parses an outgoing packet and records whether this origin already owns
    /// its flow.
    ///
    /// The raw TUN mux uses this before offering a packet to an optional
    /// direct gateway so a flow that fell back to MASQUE cannot switch paths
    /// on a later retransmission. The returned parse result can then be reused
    /// when routing the unchanged packet through MASQUE.
    pub(crate) fn inspect_outgoing(
        &mut self,
        origin: PacketOrigin,
        packet: &[u8],
    ) -> OutgoingPacketInspection {
        let parsed = ParsedPacket::parse(packet, Direction::Outgoing);
        let owned = match parsed.as_ref() {
            Some(ParsedPacket::Flow { tuple, .. }) => {
                self.forward.contains_key(&flow_key(origin, *tuple))
            }
            Some(ParsedPacket::Fragment(fragment)) => {
                self.outgoing_fragments.contains_key(&OriginFragmentKey {
                    origin,
                    fragment: fragment.clone(),
                })
            }
            None => false,
        };
        OutgoingPacketInspection {
            origin,
            parsed,
            owned,
        }
    }

    pub(crate) fn route_outgoing(&mut self, origin: PacketOrigin, packet: &mut [u8]) -> bool {
        let parsed = ParsedPacket::parse(packet, Direction::Outgoing);
        self.route_parsed_outgoing(origin, packet, parsed)
    }

    pub(crate) fn route_inspected_outgoing(
        &mut self,
        packet: &mut [u8],
        inspection: OutgoingPacketInspection,
    ) -> bool {
        self.route_parsed_outgoing(inspection.origin, packet, inspection.parsed)
    }

    fn route_parsed_outgoing(
        &mut self,
        origin: PacketOrigin,
        packet: &mut [u8],
        parsed: Option<ParsedPacket>,
    ) -> bool {
        let Some(parsed) = parsed else {
            return is_outgoing_icmp_error(packet);
        };
        let (tuple, fragment) = match parsed {
            ParsedPacket::Flow { tuple, fragment } => (tuple, fragment),
            ParsedPacket::Fragment(fragment) => {
                return self.route_outgoing_fragment(origin, packet, fragment);
            }
        };
        let application_quic = tuple.protocol == 17 && tuple.remote_id == 443;
        if origin == PacketOrigin::Tunnel && application_quic && self.traffic_policy.blocks_udp(443)
        {
            // Invalidate an older allowed classification if an IP fragment ID
            // is reused. Do not allocate state for blocked new datagrams.
            if let Some(fragment) = &fragment
                && let Some(mapping) = self.outgoing_fragments.get_mut(&OriginFragmentKey {
                    origin,
                    fragment: fragment.clone(),
                })
            {
                mapping.application_quic = true;
            }
            return false;
        }
        let flow = flow_key(origin, tuple);
        if !self.forward.contains_key(&flow) && self.forward.len() >= self.flow_limit {
            self.rejections[0] = self.rejections[0].saturating_add(1);
            return false;
        }
        // Preflight every associated index before changing packet bytes or
        // publishing a flow. A full fragment table cannot leave half a mapping.
        let prepared_fragment = if let Some(fragment) = fragment {
            let Some(prepared) = self.prepare_outgoing_fragment(origin, fragment) else {
                return false;
            };
            Some(prepared)
        } else {
            None
        };
        let now = Instant::now();
        if let Some(mapping) = self.forward.get_mut(&flow) {
            mapping.last_seen = now;
            if mapping.wire.local_id != mapping.original_id {
                rewrite_identifier(packet, &tuple, mapping.wire.local_id);
            }
            if let Some((key, wire)) = prepared_fragment {
                self.commit_outgoing_fragment(packet, key, wire, now, application_quic);
            }
            return true;
        }

        let mut wire = WireKey {
            protocol: tuple.protocol,
            local_address: tuple.local_address,
            local_id: tuple.local_id,
            remote_address: tuple.remote_address,
            remote_id: tuple.remote_id,
        };
        if self.reverse.contains_key(&wire) {
            let Some(translated) = self.allocate_identifier(&wire) else {
                return false;
            };
            wire.local_id = translated;
            rewrite_identifier(packet, &tuple, translated);
        }
        self.flow_scan.push_back(flow.clone());
        self.forward.insert(
            flow.clone(),
            FlowMapping {
                wire: wire.clone(),
                original_id: tuple.local_id,
                last_seen: now,
            },
        );
        self.reverse.insert(wire, ReverseMapping { flow });
        if let Some((key, wire)) = prepared_fragment {
            self.commit_outgoing_fragment(packet, key, wire, now, application_quic);
        }
        true
    }

    pub(crate) fn route_incoming(&mut self, packet: &mut [u8]) -> Option<PacketOrigin> {
        let Some(parsed) = ParsedPacket::parse(packet, Direction::Incoming) else {
            return self.route_incoming_icmp_error(packet);
        };
        let (tuple, fragment) = match parsed {
            ParsedPacket::Flow { tuple, fragment } => (tuple, fragment),
            ParsedPacket::Fragment(fragment) => {
                let mapping = self.incoming_fragments.get_mut(&fragment)?;
                if mapping.0 == PacketOrigin::Tunnel
                    && mapping.2
                    && self.traffic_policy.blocks_udp(443)
                {
                    return None;
                }
                mapping.1 = Instant::now();
                return Some(mapping.0);
            }
        };
        let wire = WireKey {
            protocol: tuple.protocol,
            local_address: tuple.local_address,
            local_id: tuple.local_id,
            remote_address: tuple.remote_address,
            remote_id: tuple.remote_id,
        };
        if let Some(fragment) = &fragment
            && (!self.reverse.contains_key(&wire) || !self.admit_incoming_fragment(fragment))
        {
            return None;
        }
        let reverse = self.reverse.get(&wire)?;
        let flow = reverse.flow.clone();
        let application_quic = flow.protocol == 17 && flow.remote_id == 443;
        if flow.origin == PacketOrigin::Tunnel
            && application_quic
            && self.traffic_policy.blocks_udp(443)
        {
            if let Some(fragment) = &fragment
                && let Some(mapping) = self.incoming_fragments.get_mut(fragment)
            {
                mapping.0 = flow.origin;
                mapping.2 = true;
            }
            return None;
        }
        if wire.local_id != flow.local_id {
            rewrite_identifier(packet, &tuple, flow.local_id);
        }
        if let Some(forward) = self.forward.get_mut(&flow) {
            forward.last_seen = Instant::now();
        }
        if let Some(fragment) = fragment {
            self.record_incoming_fragment(fragment, flow.origin, Instant::now(), application_quic);
        }
        Some(flow.origin)
    }

    fn prepare_outgoing_fragment(
        &mut self,
        origin: PacketOrigin,
        fragment: FragmentKey,
    ) -> Option<(OriginFragmentKey, FragmentKey)> {
        let key = OriginFragmentKey {
            origin,
            fragment: fragment.clone(),
        };
        if let Some(mapping) = self.outgoing_fragments.get(&key) {
            return Some((key, mapping.wire.clone()));
        }
        if self.outgoing_fragments.len() >= self.outgoing_fragment_limit {
            self.rejections[1] = self.rejections[1].saturating_add(1);
            return None;
        }

        let mut wire = fragment.clone();
        if self.wire_fragments.contains_key(&wire) {
            let identifier = self.allocate_fragment_identifier(&wire)?;
            wire.identifier = identifier;
        }
        Some((key, wire))
    }

    fn commit_outgoing_fragment(
        &mut self,
        packet: &mut [u8],
        key: OriginFragmentKey,
        wire: FragmentKey,
        now: Instant,
        application_quic: bool,
    ) {
        if wire.identifier != key.fragment.identifier {
            rewrite_fragment_identifier(packet, &key.fragment, wire.identifier);
        }
        if let Some(mapping) = self.outgoing_fragments.get_mut(&key) {
            mapping.last_seen = now;
            mapping.application_quic = application_quic;
            return;
        }
        self.outgoing_fragment_scan.push_back(key.clone());
        self.outgoing_fragments.insert(
            key.clone(),
            FragmentMapping {
                wire: wire.clone(),
                last_seen: now,
                application_quic,
            },
        );
        self.wire_fragments.insert(wire, key);
    }

    fn admit_incoming_fragment(&mut self, fragment: &FragmentKey) -> bool {
        if !self.incoming_fragments.contains_key(fragment)
            && self.incoming_fragments.len() >= self.incoming_fragment_limit
        {
            self.rejections[2] = self.rejections[2].saturating_add(1);
            return false;
        }
        true
    }

    fn record_incoming_fragment(
        &mut self,
        fragment: FragmentKey,
        origin: PacketOrigin,
        now: Instant,
        application_quic: bool,
    ) {
        if !self.incoming_fragments.contains_key(&fragment) {
            self.incoming_fragment_scan.push_back(fragment.clone());
        }
        self.incoming_fragments
            .insert(fragment, (origin, now, application_quic));
    }

    fn route_outgoing_fragment(
        &mut self,
        origin: PacketOrigin,
        packet: &mut [u8],
        fragment: FragmentKey,
    ) -> bool {
        let key = OriginFragmentKey {
            origin,
            fragment: fragment.clone(),
        };
        let Some(mapping) = self.outgoing_fragments.get_mut(&key) else {
            // A non-initial fragment that arrived before its first fragment is
            // intentionally dropped because it cannot be attributed safely.
            return false;
        };
        if origin == PacketOrigin::Tunnel
            && mapping.application_quic
            && self.traffic_policy.blocks_udp(443)
        {
            return false;
        }
        mapping.last_seen = Instant::now();
        if mapping.wire.identifier != fragment.identifier {
            rewrite_fragment_identifier(packet, &fragment, mapping.wire.identifier);
        }
        true
    }

    fn allocate_identifier(&mut self, template: &WireKey) -> Option<u16> {
        for _ in 0..16_384 {
            let candidate = self.next_id;
            self.next_id = if self.next_id == 65_535 {
                49_152
            } else {
                self.next_id + 1
            };
            let mut key = template.clone();
            key.local_id = candidate;
            if !self.reverse.contains_key(&key) {
                return Some(candidate);
            }
        }
        None
    }

    fn allocate_fragment_identifier(&mut self, template: &FragmentKey) -> Option<u32> {
        let attempts = if template.identifier_width == 2 {
            u32::from(u16::MAX)
        } else {
            65_536
        };
        for _ in 0..attempts {
            let candidate = if template.identifier_width == 2 {
                self.next_fragment_id = (self.next_fragment_id + 1) & 0xffff;
                self.next_fragment_id
            } else {
                self.next_fragment_id = self.next_fragment_id.wrapping_add(1);
                self.next_fragment_id
            };
            let mut key = template.clone();
            key.identifier = candidate;
            if !self.wire_fragments.contains_key(&key) {
                return Some(candidate);
            }
        }
        None
    }

    fn route_incoming_icmp_error(&mut self, packet: &mut [u8]) -> Option<PacketOrigin> {
        let network = parse_network_packet(packet)?;
        let transport_offset = network.transport_offset?;
        if !is_icmp_error_type(network.protocol, *packet.get(transport_offset)?) {
            return None;
        }
        let inner_offset = transport_offset.checked_add(8)?;
        let inner = packet.get(inner_offset..)?;
        let ParsedPacket::Flow { tuple, .. } = ParsedPacket::parse(inner, Direction::Outgoing)?
        else {
            return None;
        };
        let wire = WireKey {
            protocol: tuple.protocol,
            local_address: tuple.local_address,
            local_id: tuple.local_id,
            remote_address: tuple.remote_address,
            remote_id: tuple.remote_id,
        };
        if let Some(fragment) = &network.fragment
            && (!self.reverse.contains_key(&wire) || !self.admit_incoming_fragment(fragment))
        {
            return None;
        }
        let reverse = self.reverse.get(&wire)?;
        let flow = reverse.flow.clone();
        if wire.local_id != flow.local_id {
            rewrite_embedded_identifier(
                packet,
                inner_offset,
                &tuple,
                transport_offset + 2,
                flow.local_id,
            );
        }
        if let Some(forward) = self.forward.get_mut(&flow) {
            forward.last_seen = Instant::now();
        }
        if let Some(fragment) = network.fragment {
            self.record_incoming_fragment(fragment, flow.origin, Instant::now(), false);
        }
        Some(flow.origin)
    }

    /// The owning mux calls this once per second. Each live mapping has exactly
    /// one scan entry; traffic refreshes timestamps without allocating entries.
    pub(crate) fn maintain(&mut self, now: Instant) {
        for _ in 0..self.flow_scan.len().min(MAINTENANCE_ITEMS) {
            let key = self.flow_scan.pop_front().expect("bounded flow scan");
            if self
                .forward
                .get(&key)
                .is_some_and(|m| now.saturating_duration_since(m.last_seen) > FLOW_IDLE)
            {
                if let Some(mapping) = self.forward.remove(&key) {
                    self.reverse.remove(&mapping.wire);
                }
            } else {
                self.flow_scan.push_back(key);
            }
        }
        for _ in 0..self.outgoing_fragment_scan.len().min(MAINTENANCE_ITEMS) {
            let key = self
                .outgoing_fragment_scan
                .pop_front()
                .expect("bounded fragment scan");
            if self
                .outgoing_fragments
                .get(&key)
                .is_some_and(|m| now.saturating_duration_since(m.last_seen) > FLOW_IDLE)
            {
                if let Some(mapping) = self.outgoing_fragments.remove(&key) {
                    self.wire_fragments.remove(&mapping.wire);
                }
            } else {
                self.outgoing_fragment_scan.push_back(key);
            }
        }
        for _ in 0..self.incoming_fragment_scan.len().min(MAINTENANCE_ITEMS) {
            let key = self
                .incoming_fragment_scan
                .pop_front()
                .expect("bounded incoming scan");
            if self
                .incoming_fragments
                .get(&key)
                .is_some_and(|(_, seen, _)| now.saturating_duration_since(*seen) > FLOW_IDLE)
            {
                self.incoming_fragments.remove(&key);
            } else {
                self.incoming_fragment_scan.push_back(key);
            }
        }
        if self.forward.is_empty() {
            self.forward = HashMap::new();
            self.reverse = HashMap::new();
            self.flow_scan = VecDeque::new();
        }
        if self.outgoing_fragments.is_empty() {
            self.outgoing_fragments = HashMap::new();
            self.wire_fragments = HashMap::new();
            self.outgoing_fragment_scan = VecDeque::new();
        }
        if self.incoming_fragments.is_empty() {
            self.incoming_fragments = HashMap::new();
            self.incoming_fragment_scan = VecDeque::new();
        }
        if self.rejections != self.reported_rejections
            && now.saturating_duration_since(self.last_report) >= Duration::from_secs(30)
        {
            tracing::warn!(
                reason_code = "packet_mux_capacity",
                flow_rejections = self.rejections[0],
                outgoing_fragment_rejections = self.rejections[1],
                incoming_fragment_rejections = self.rejections[2],
                "packet mux rejected new mappings at its resource limit"
            );
            self.reported_rejections = self.rejections;
            self.last_report = now;
        }
    }
}

fn flow_key(origin: PacketOrigin, tuple: PacketTuple) -> FlowKey {
    FlowKey {
        origin,
        protocol: tuple.protocol,
        local_address: tuple.local_address,
        local_id: tuple.local_id,
        remote_address: tuple.remote_address,
        remote_id: tuple.remote_id,
    }
}

#[derive(Debug, Clone, Copy)]
enum Direction {
    Outgoing,
    Incoming,
}

#[derive(Debug, Clone, Copy)]
struct PacketTuple {
    protocol: u8,
    local_address: IpAddr,
    local_id: u16,
    remote_address: IpAddr,
    remote_id: u16,
    identifier_offset: usize,
    checksum_offset: usize,
    checksum_optional: bool,
}

enum ParsedPacket {
    Flow {
        tuple: PacketTuple,
        fragment: Option<FragmentKey>,
    },
    Fragment(FragmentKey),
}

struct NetworkPacket {
    version: u8,
    protocol: u8,
    source: IpAddr,
    destination: IpAddr,
    transport_offset: Option<usize>,
    fragment: Option<FragmentKey>,
}

impl ParsedPacket {
    fn parse(packet: &[u8], direction: Direction) -> Option<Self> {
        let network = parse_network_packet(packet)?;
        let Some(transport_offset) = network.transport_offset else {
            return network.fragment.map(Self::Fragment);
        };
        let (source_id, destination_id, identifier_offset, checksum_offset, checksum_optional) =
            match network.protocol {
                6 => (
                    read_u16(packet, transport_offset)?,
                    read_u16(packet, transport_offset + 2)?,
                    match direction {
                        Direction::Outgoing => transport_offset,
                        Direction::Incoming => transport_offset + 2,
                    },
                    transport_offset + 16,
                    false,
                ),
                17 => (
                    read_u16(packet, transport_offset)?,
                    read_u16(packet, transport_offset + 2)?,
                    match direction {
                        Direction::Outgoing => transport_offset,
                        Direction::Incoming => transport_offset + 2,
                    },
                    transport_offset + 6,
                    network.version == 4,
                ),
                1 | 58 => {
                    let message_type = *packet.get(transport_offset)?;
                    let valid = matches!(
                        (network.protocol, direction, message_type),
                        (1, Direction::Outgoing, 8)
                            | (1, Direction::Incoming, 0)
                            | (58, Direction::Outgoing, 128)
                            | (58, Direction::Incoming, 129)
                    );
                    if !valid {
                        return None;
                    }
                    let identifier = read_u16(packet, transport_offset + 4)?;
                    (
                        identifier,
                        identifier,
                        transport_offset + 4,
                        transport_offset + 2,
                        false,
                    )
                }
                _ => return None,
            };
        let (local_address, local_id, remote_address, remote_id) = match direction {
            Direction::Outgoing => (
                network.source,
                source_id,
                network.destination,
                destination_id,
            ),
            Direction::Incoming => (
                network.destination,
                destination_id,
                network.source,
                source_id,
            ),
        };
        Some(Self::Flow {
            tuple: PacketTuple {
                protocol: network.protocol,
                local_address,
                local_id,
                remote_address,
                remote_id,
                identifier_offset,
                checksum_offset,
                checksum_optional,
            },
            fragment: network.fragment,
        })
    }
}

fn parse_network_packet(packet: &[u8]) -> Option<NetworkPacket> {
    match packet.first()? >> 4 {
        4 => parse_ipv4(packet),
        6 => parse_ipv6(packet),
        _ => None,
    }
}

fn parse_ipv4(packet: &[u8]) -> Option<NetworkPacket> {
    if packet.len() < 20 {
        return None;
    }
    let header_length = usize::from(packet[0] & 0x0f) * 4;
    if header_length < 20 || packet.len() < header_length {
        return None;
    }
    let source = IpAddr::V4(Ipv4Addr::new(
        packet[12], packet[13], packet[14], packet[15],
    ));
    let destination = IpAddr::V4(Ipv4Addr::new(
        packet[16], packet[17], packet[18], packet[19],
    ));
    let flags_offset = read_u16(packet, 6)?;
    let fragment_offset = flags_offset & 0x1fff;
    let more_fragments = flags_offset & 0x2000 != 0;
    let fragment = (fragment_offset != 0 || more_fragments).then(|| FragmentKey {
        source,
        destination,
        protocol: packet[9],
        identifier: u32::from(read_u16(packet, 4).unwrap_or_default()),
        identifier_offset: 4,
        identifier_width: 2,
    });
    Some(NetworkPacket {
        version: 4,
        protocol: packet[9],
        source,
        destination,
        transport_offset: (fragment_offset == 0).then_some(header_length),
        fragment,
    })
}

fn parse_ipv6(packet: &[u8]) -> Option<NetworkPacket> {
    if packet.len() < 40 {
        return None;
    }
    let source = IpAddr::V6(Ipv6Addr::from(<[u8; 16]>::try_from(&packet[8..24]).ok()?));
    let destination = IpAddr::V6(Ipv6Addr::from(<[u8; 16]>::try_from(&packet[24..40]).ok()?));
    let mut protocol = packet[6];
    let mut offset = 40usize;
    let mut fragment = None;
    for _ in 0..8 {
        match protocol {
            0 | 43 | 60 => {
                let next = *packet.get(offset)?;
                let length = (usize::from(*packet.get(offset + 1)?) + 1) * 8;
                offset = offset.checked_add(length)?;
                if offset > packet.len() {
                    return None;
                }
                protocol = next;
            }
            51 => {
                let next = *packet.get(offset)?;
                let length = (usize::from(*packet.get(offset + 1)?) + 2) * 4;
                offset = offset.checked_add(length)?;
                if offset > packet.len() {
                    return None;
                }
                protocol = next;
            }
            44 => {
                let fragment_protocol = *packet.get(offset)?;
                let flags_offset = read_u16(packet, offset + 2)?;
                let fragment_offset = (flags_offset & 0xfff8) >> 3;
                let more_fragments = flags_offset & 1 != 0;
                let key = FragmentKey {
                    source,
                    destination,
                    protocol: fragment_protocol,
                    identifier: read_u32(packet, offset + 4)?,
                    identifier_offset: offset + 4,
                    identifier_width: 4,
                };
                fragment = Some(key);
                offset = offset.checked_add(8)?;
                if offset > packet.len() {
                    return None;
                }
                if fragment_offset != 0 {
                    return Some(NetworkPacket {
                        version: 6,
                        protocol: fragment_protocol,
                        source,
                        destination,
                        transport_offset: None,
                        fragment,
                    });
                }
                protocol = fragment_protocol;
                if !more_fragments {
                    // Atomic fragments still retain their identity so a
                    // collision can be translated consistently.
                }
            }
            50 | 59 => return None,
            _ => break,
        }
    }
    Some(NetworkPacket {
        version: 6,
        protocol,
        source,
        destination,
        transport_offset: Some(offset),
        fragment,
    })
}

fn rewrite_identifier(packet: &mut [u8], tuple: &PacketTuple, new_id: u16) {
    let Some(old_id) = read_u16(packet, tuple.identifier_offset) else {
        return;
    };
    let Some(checksum) = read_u16(packet, tuple.checksum_offset) else {
        return;
    };
    write_u16(packet, tuple.identifier_offset, new_id);
    if tuple.checksum_optional && checksum == 0 {
        return;
    }
    write_u16(
        packet,
        tuple.checksum_offset,
        update_checksum(checksum, old_id, new_id),
    );
}

fn rewrite_embedded_identifier(
    packet: &mut [u8],
    inner_offset: usize,
    tuple: &PacketTuple,
    outer_checksum_offset: usize,
    new_id: u16,
) {
    let identifier_offset = inner_offset + tuple.identifier_offset;
    let Some(old_id) = read_u16(packet, identifier_offset) else {
        return;
    };
    let Some(mut outer_checksum) = read_u16(packet, outer_checksum_offset) else {
        return;
    };
    write_u16(packet, identifier_offset, new_id);
    outer_checksum = update_checksum(outer_checksum, old_id, new_id);

    let inner_checksum_offset = inner_offset + tuple.checksum_offset;
    if let Some(old_inner_checksum) = read_u16(packet, inner_checksum_offset)
        && !(tuple.checksum_optional && old_inner_checksum == 0)
    {
        let new_inner_checksum = update_checksum(old_inner_checksum, old_id, new_id);
        write_u16(packet, inner_checksum_offset, new_inner_checksum);
        outer_checksum = update_checksum(outer_checksum, old_inner_checksum, new_inner_checksum);
    }
    write_u16(packet, outer_checksum_offset, outer_checksum);
}

fn rewrite_fragment_identifier(packet: &mut [u8], fragment: &FragmentKey, new_id: u32) {
    match fragment.identifier_width {
        2 => {
            let Some(old_id) = read_u16(packet, fragment.identifier_offset) else {
                return;
            };
            let Some(header_checksum) = read_u16(packet, 10) else {
                return;
            };
            write_u16(packet, fragment.identifier_offset, new_id as u16);
            write_u16(
                packet,
                10,
                update_checksum(header_checksum, old_id, new_id as u16),
            );
        }
        4 if packet
            .get_mut(fragment.identifier_offset..fragment.identifier_offset + 4)
            .is_some() =>
        {
            packet[fragment.identifier_offset..fragment.identifier_offset + 4]
                .copy_from_slice(&new_id.to_be_bytes());
        }
        _ => {}
    }
}

fn is_outgoing_icmp_error(packet: &[u8]) -> bool {
    let Some(network) = parse_network_packet(packet) else {
        return false;
    };
    let Some(offset) = network.transport_offset else {
        return false;
    };
    packet
        .get(offset)
        .is_some_and(|message_type| is_icmp_error_type(network.protocol, *message_type))
}

fn is_icmp_error_type(protocol: u8, message_type: u8) -> bool {
    match protocol {
        1 => matches!(message_type, 3 | 4 | 5 | 11 | 12),
        58 => matches!(message_type, 1..=4),
        _ => false,
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

fn read_u32(packet: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_be_bytes([
        *packet.get(offset)?,
        *packet.get(offset + 1)?,
        *packet.get(offset + 2)?,
        *packet.get(offset + 3)?,
    ]))
}

fn write_u16(packet: &mut [u8], offset: usize, value: u16) {
    if let Some(target) = packet.get_mut(offset..offset + 2) {
        target.copy_from_slice(&value.to_be_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_flow_table_preserves_packet_bytes_and_existing_routes() {
        let mut table = PacketMuxTable {
            flow_limit: 1,
            ..Default::default()
        };
        let original = udp_packet(50_000, 443, false);
        assert!(table.route_outgoing(PacketOrigin::Tunnel, &mut original.clone()));
        let mut rejected = original.clone();
        assert!(!table.route_outgoing(PacketOrigin::Proxy, &mut rejected));
        assert_eq!(rejected, original);
        assert_eq!(
            (
                table.forward.len(),
                table.reverse.len(),
                table.flow_scan.len()
            ),
            (1, 1, 1)
        );
        for _ in 0..1000 {
            assert!(table.route_outgoing(PacketOrigin::Tunnel, &mut original.clone()));
        }
        assert_eq!(table.flow_scan.len(), 1);
        assert_eq!(
            table.route_incoming(&mut udp_packet(443, 50_000, true)),
            Some(PacketOrigin::Tunnel)
        );
        assert_eq!(table.rejections[0], 1);
    }

    #[test]
    fn fragment_rejection_cannot_publish_a_flow_or_rewrite_an_existing_one() {
        let mut table = PacketMuxTable {
            outgoing_fragment_limit: 0,
            ..Default::default()
        };
        let mut first = udp_packet(50_000, 443, false);
        fragment(&mut first, 7, 0, true);
        let original = first.clone();
        assert!(!table.route_outgoing(PacketOrigin::Tunnel, &mut first));
        assert_eq!(first, original);
        assert!(table.forward.is_empty() && table.reverse.is_empty() && table.flow_scan.is_empty());
        assert!(table.outgoing_fragments.is_empty() && table.wire_fragments.is_empty());
        assert!(table.route_outgoing(PacketOrigin::Tunnel, &mut udp_packet(50_000, 443, false)));
        assert!(table.route_outgoing(PacketOrigin::Proxy, &mut udp_packet(50_000, 443, false)));
        assert!(!table.route_outgoing(PacketOrigin::Proxy, &mut first));
        assert_eq!(
            first, original,
            "reject before the translated port is written"
        );
        assert_eq!(
            (
                table.forward.len(),
                table.reverse.len(),
                table.flow_scan.len()
            ),
            (2, 2, 2)
        );
    }

    #[test]
    fn full_fragment_tables_keep_existing_fragments_without_growing_scan_queues() {
        let mut table = PacketMuxTable {
            outgoing_fragment_limit: 1,
            incoming_fragment_limit: 1,
            ..Default::default()
        };
        let mut first = udp_packet(50_000, 443, false);
        fragment(&mut first, 7, 0, true);
        assert!(table.route_outgoing(PacketOrigin::Tunnel, &mut first));
        let mut incoming = udp_packet(443, 50_000, true);
        fragment(&mut incoming, 8, 0, true);
        assert_eq!(
            table.route_incoming(&mut incoming),
            Some(PacketOrigin::Tunnel)
        );
        for _ in 0..1000 {
            assert!(table.route_outgoing(PacketOrigin::Tunnel, &mut first.clone()));
            assert_eq!(
                table.route_incoming(&mut incoming.clone()),
                Some(PacketOrigin::Tunnel)
            );
        }
        let mut denied = udp_packet(50_001, 443, false);
        fragment(&mut denied, 9, 0, true);
        let original = denied.clone();
        assert!(!table.route_outgoing(PacketOrigin::Tunnel, &mut denied));
        assert_eq!(denied, original);
        assert_eq!(table.forward.len(), 1);
        let mut denied = incoming.clone();
        fragment(&mut denied, 9, 0, true);
        let original = denied.clone();
        assert_eq!(table.route_incoming(&mut denied), None);
        assert_eq!(denied, original);
        assert!(table.route_outgoing(PacketOrigin::Tunnel, &mut later_udp_fragment(7, false)));
        assert_eq!(
            table.route_incoming(&mut later_udp_fragment(8, true)),
            Some(PacketOrigin::Tunnel)
        );
        assert_eq!(
            (
                table.outgoing_fragment_scan.len(),
                table.incoming_fragment_scan.len()
            ),
            (1, 1)
        );
        assert_eq!(
            (table.outgoing_fragments.len(), table.wire_fragments.len()),
            (1, 1)
        );
    }

    #[test]
    fn incoming_fragment_limit_precedes_port_and_icmp_quote_restoration() {
        let mut table = PacketMuxTable {
            incoming_fragment_limit: 0,
            ..Default::default()
        };
        let mut tunnel = udp_packet(50_000, 443, false);
        let mut proxy = tunnel.clone();
        assert!(table.route_outgoing(PacketOrigin::Tunnel, &mut tunnel));
        assert!(table.route_outgoing(PacketOrigin::Proxy, &mut proxy));
        let wire_port = read_u16(&proxy, 20).unwrap();
        assert_ne!(wire_port, 50_000);
        let mut response = udp_packet(443, wire_port, true);
        fragment(&mut response, 12, 0, true);
        let original = response.clone();
        assert_eq!(table.route_incoming(&mut response), None);
        assert_eq!(response, original);
        let mut icmp = icmp_unreachable(&proxy);
        fragment(&mut icmp, 13, 0, true);
        let original = icmp.clone();
        assert_eq!(table.route_incoming(&mut icmp), None);
        assert_eq!(icmp, original);
        assert_eq!(
            table.route_incoming(&mut udp_packet(443, wire_port, true)),
            Some(PacketOrigin::Proxy)
        );
    }

    #[test]
    fn maintenance_has_a_work_bound_and_releases_empty_backing_tables() {
        let mut table = PacketMuxTable::default();
        let count = MAINTENANCE_ITEMS + 7;
        for port in 1..=count {
            assert!(table.route_outgoing(
                PacketOrigin::Tunnel,
                &mut udp_packet(port as u16, 443, false)
            ));
        }
        let observed = Instant::now();
        for mapping in table.forward.values_mut() {
            mapping.last_seen = observed;
        }
        table.maintain(observed + FLOW_IDLE);
        assert_eq!(
            table.forward.len(),
            count,
            "retain the five-minute boundary"
        );
        table.maintain(observed + FLOW_IDLE + Duration::from_secs(1));
        assert_eq!(
            (
                table.forward.len(),
                table.reverse.len(),
                table.flow_scan.len()
            ),
            (7, 7, 7)
        );
        table.maintain(observed + FLOW_IDLE + Duration::from_secs(2));
        assert_eq!(
            (
                table.forward.capacity(),
                table.reverse.capacity(),
                table.flow_scan.capacity()
            ),
            (0, 0, 0)
        );
        assert!(table.route_outgoing(PacketOrigin::Tunnel, &mut udp_packet(50_000, 443, false)));
    }

    #[test]
    fn fragment_maintenance_is_bounded_and_keeps_refreshed_entries() {
        let mut table = PacketMuxTable::default();
        for id in 0..=MAINTENANCE_ITEMS {
            let mut out = udp_packet(50_000, 443, false);
            fragment(&mut out, id as u16, 0, true);
            assert!(table.route_outgoing(PacketOrigin::Tunnel, &mut out));
            let mut incoming = udp_packet(443, 50_000, true);
            fragment(&mut incoming, id as u16, 0, true);
            assert_eq!(
                table.route_incoming(&mut incoming),
                Some(PacketOrigin::Tunnel)
            );
        }
        let observed = Instant::now();
        for mapping in table.outgoing_fragments.values_mut() {
            mapping.last_seen = observed;
        }
        for (_, seen, _) in table.incoming_fragments.values_mut() {
            *seen = observed;
        }
        table.maintain(observed + FLOW_IDLE + Duration::from_secs(1));
        assert_eq!(
            (
                table.outgoing_fragments.len(),
                table.wire_fragments.len(),
                table.incoming_fragments.len()
            ),
            (1, 1, 1)
        );
        // Refresh the remaining entries between maintenance rounds.
        for mapping in table.outgoing_fragments.values_mut() {
            mapping.last_seen = observed + FLOW_IDLE;
        }
        for (_, seen, _) in table.incoming_fragments.values_mut() {
            *seen = observed + FLOW_IDLE;
        }
        table.maintain(observed + FLOW_IDLE + Duration::from_secs(2));
        assert_eq!(
            (
                table.outgoing_fragment_scan.len(),
                table.incoming_fragment_scan.len()
            ),
            (1, 1)
        );
        table.maintain(observed + FLOW_IDLE * 2 + Duration::from_secs(1));
        assert_eq!(
            (
                table.outgoing_fragments.capacity(),
                table.wire_fragments.capacity(),
                table.incoming_fragments.capacity()
            ),
            (0, 0, 0)
        );
        assert_eq!(
            (
                table.outgoing_fragment_scan.capacity(),
                table.incoming_fragment_scan.capacity()
            ),
            (0, 0)
        );
    }

    #[test]
    fn production_flow_cap_rejects_the_65537th_mapping_on_every_platform() {
        let mut table = PacketMuxTable::default();
        assert_eq!(table.flow_limit, 65_536);
        assert_eq!(
            (table.outgoing_fragment_limit, table.incoming_fragment_limit),
            (8_192, 8_192)
        );
        for index in 0..=MAX_FLOWS {
            let mut packet = udp_packet(50_000, 443, false);
            packet[16..20].copy_from_slice(&[
                198,
                18 + (index >> 16) as u8,
                (index >> 8) as u8,
                index as u8,
            ]);
            assert_eq!(
                table.route_outgoing(PacketOrigin::Tunnel, &mut packet),
                index < MAX_FLOWS
            );
        }
        assert_eq!(
            (
                table.forward.len(),
                table.reverse.len(),
                table.flow_scan.len()
            ),
            (MAX_FLOWS, MAX_FLOWS, MAX_FLOWS)
        );
    }

    fn udp_packet(source_port: u16, destination_port: u16, reverse: bool) -> Vec<u8> {
        let mut packet = vec![0u8; 28];
        packet[0] = 0x45;
        packet[2..4].copy_from_slice(&28u16.to_be_bytes());
        packet[8] = 64;
        packet[9] = 17;
        let (source, destination) = if reverse {
            ([203, 0, 113, 8], [172, 16, 0, 2])
        } else {
            ([172, 16, 0, 2], [203, 0, 113, 8])
        };
        packet[12..16].copy_from_slice(&source);
        packet[16..20].copy_from_slice(&destination);
        packet[20..22].copy_from_slice(&source_port.to_be_bytes());
        packet[22..24].copy_from_slice(&destination_port.to_be_bytes());
        packet[24..26].copy_from_slice(&8u16.to_be_bytes());
        packet[26..28].copy_from_slice(&0x1234u16.to_be_bytes());
        packet
    }

    fn fragment(packet: &mut [u8], identifier: u16, offset: u16, more: bool) {
        packet[4..6].copy_from_slice(&identifier.to_be_bytes());
        let flags_offset = offset | if more { 0x2000 } else { 0 };
        packet[6..8].copy_from_slice(&flags_offset.to_be_bytes());
        packet[10..12].copy_from_slice(&0x4321u16.to_be_bytes());
    }

    fn later_udp_fragment(identifier: u16, reverse: bool) -> Vec<u8> {
        let mut packet = vec![0u8; 24];
        packet[0] = 0x45;
        packet[2..4].copy_from_slice(&24u16.to_be_bytes());
        packet[8] = 64;
        packet[9] = 17;
        let (source, destination) = if reverse {
            ([203, 0, 113, 8], [172, 16, 0, 2])
        } else {
            ([172, 16, 0, 2], [203, 0, 113, 8])
        };
        packet[12..16].copy_from_slice(&source);
        packet[16..20].copy_from_slice(&destination);
        fragment(&mut packet, identifier, 1, false);
        packet
    }

    fn icmp_unreachable(embedded: &[u8]) -> Vec<u8> {
        let mut packet = vec![0u8; 28 + embedded.len()];
        packet[0] = 0x45;
        let length = packet.len() as u16;
        packet[2..4].copy_from_slice(&length.to_be_bytes());
        packet[8] = 64;
        packet[9] = 1;
        packet[12..16].copy_from_slice(&[203, 0, 113, 8]);
        packet[16..20].copy_from_slice(&[172, 16, 0, 2]);
        packet[20] = 3;
        packet[21] = 1;
        packet[22..24].copy_from_slice(&0x2222u16.to_be_bytes());
        packet[28..].copy_from_slice(embedded);
        packet
    }

    #[test]
    fn colliding_tunnel_and_proxy_flows_are_translated_and_restored() {
        let mut table = PacketMuxTable::default();
        let mut tunnel = udp_packet(50_000, 443, false);
        let mut proxy = tunnel.clone();
        assert!(table.route_outgoing(PacketOrigin::Tunnel, &mut tunnel));
        assert!(table.route_outgoing(PacketOrigin::Proxy, &mut proxy));
        assert_eq!(read_u16(&tunnel, 20), Some(50_000));
        let translated = read_u16(&proxy, 20).expect("translated port");
        assert_ne!(translated, 50_000);

        let mut tunnel_reply = udp_packet(443, 50_000, true);
        assert_eq!(
            table.route_incoming(&mut tunnel_reply),
            Some(PacketOrigin::Tunnel)
        );
        let mut proxy_reply = udp_packet(443, translated, true);
        assert_eq!(
            table.route_incoming(&mut proxy_reply),
            Some(PacketOrigin::Proxy)
        );
        assert_eq!(read_u16(&proxy_reply, 22), Some(50_000));
    }

    #[test]
    fn outgoing_flow_ownership_is_sticky_per_origin() {
        let mut table = PacketMuxTable::default();
        let mut packet = udp_packet(50_000, 443, false);

        let inspection = table.inspect_outgoing(PacketOrigin::Tunnel, &packet);
        assert!(!inspection.is_owned());
        assert!(table.route_inspected_outgoing(&mut packet, inspection));
        assert!(
            table
                .inspect_outgoing(PacketOrigin::Tunnel, &packet)
                .is_owned()
        );
        assert!(
            !table
                .inspect_outgoing(PacketOrigin::Proxy, &packet)
                .is_owned()
        );

        let retransmission = udp_packet(50_000, 443, false);
        assert!(
            table
                .inspect_outgoing(PacketOrigin::Tunnel, &retransmission)
                .is_owned()
        );
    }

    #[test]
    fn unknown_return_packets_are_dropped() {
        let mut table = PacketMuxTable::default();
        let mut packet = udp_packet(443, 55_000, true);
        assert_eq!(table.route_incoming(&mut packet), None);
    }

    #[test]
    fn fragmented_colliding_flows_keep_their_origin_and_wire_identifier() {
        let mut table = PacketMuxTable::default();
        let mut tunnel_first = udp_packet(50_000, 443, false);
        fragment(&mut tunnel_first, 7, 0, true);
        let mut proxy_first = tunnel_first.clone();
        assert!(table.route_outgoing(PacketOrigin::Tunnel, &mut tunnel_first));
        assert!(table.route_outgoing(PacketOrigin::Proxy, &mut proxy_first));
        let proxy_fragment_id = read_u16(&proxy_first, 4).expect("fragment id");
        assert_ne!(proxy_fragment_id, 7);

        let mut proxy_later = later_udp_fragment(7, false);
        assert!(table.route_outgoing(PacketOrigin::Proxy, &mut proxy_later));
        assert_eq!(read_u16(&proxy_later, 4), Some(proxy_fragment_id));

        let mut reply_first = udp_packet(443, read_u16(&proxy_first, 20).unwrap(), true);
        fragment(&mut reply_first, 42, 0, true);
        assert_eq!(
            table.route_incoming(&mut reply_first),
            Some(PacketOrigin::Proxy)
        );
        let mut reply_later = later_udp_fragment(42, true);
        assert_eq!(
            table.route_incoming(&mut reply_later),
            Some(PacketOrigin::Proxy)
        );
    }

    #[test]
    fn icmp_error_quotes_are_attributed_and_restore_the_original_port() {
        let mut table = PacketMuxTable::default();
        let mut tunnel = udp_packet(50_000, 443, false);
        let mut proxy = tunnel.clone();
        assert!(table.route_outgoing(PacketOrigin::Tunnel, &mut tunnel));
        assert!(table.route_outgoing(PacketOrigin::Proxy, &mut proxy));
        assert_ne!(read_u16(&proxy, 20), Some(50_000));

        let mut error = icmp_unreachable(&proxy);
        assert_eq!(table.route_incoming(&mut error), Some(PacketOrigin::Proxy));
        assert_eq!(read_u16(&error, 28 + 20), Some(50_000));
    }
}
