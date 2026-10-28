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
use ts_netstack_smoltcp::netsock::UdpSocket as StackUdpSocket;
use usque_core::Profile;

use crate::direct_diagnostics::{DirectFallbackReason, DirectGatewayDiagnostics};
use crate::geo_direct::{GeoDirectPolicy, GeoRoute, bind_direct_udp, connect_direct_ip};
use crate::h2::TransportError;
use crate::netstack::{TrafficCounters, bounded_piped, direct_netstack_config};
use crate::network_quality::NetworkQualityTelemetry;
use crate::socket::DirectEgressLease;
use crate::socket::SocketProtector;
use crate::split_dns::{
    DnsRouteCache, SPLIT_DNS_IPV4, SPLIT_DNS_IPV6, SplitDnsConfig, SplitDnsRuntime,
};
use crate::stack_tcp::OnceListener;

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

/// A reservation is not a live flow until its first packet is admitted. An
/// attachment may be cancelled while physical authorization or stack creation
/// is pending, so cleanup must follow future ownership rather than an error
/// branch that cancellation can skip.
struct NatReservation {
    flows: Arc<Mutex<NatTable>>,
    flow: FlowKey,
    id: u64,
    committed: bool,
}

impl NatReservation {
    fn new(flows: &Arc<Mutex<NatTable>>, flow: &FlowKey, id: u64) -> Self {
        Self {
            flows: Arc::clone(flows),
            flow: flow.clone(),
            id,
            committed: false,
        }
    }

    fn commit(&mut self) {
        self.committed = true;
    }
}

impl Drop for NatReservation {
    fn drop(&mut self) {
        if !self.committed
            && let Ok(mut flows) = self.flows.lock()
        {
            flows.remove_if(&self.flow, self.id);
        }
    }
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
    stack_incoming: Option<crate::packet_pipe::PacketSender>,
    policy: Arc<GeoDirectPolicy>,
    protector: Arc<dyn SocketProtector>,
    counters: Arc<TrafficCounters>,
    diagnostics: Arc<DirectGatewayDiagnostics>,
    flows: Arc<Mutex<NatTable>>,
    cancellation: CancellationToken,
    stack_task: Option<JoinHandle<()>>,
    incoming_task: Option<JoinHandle<()>>,
    split_dns: Option<SplitDnsRuntime>,
    dns_hints: Arc<DnsRouteCache>,
    rejection_tx: mpsc::Sender<Bytes>,
    rejector: crate::routing_reject::RoutingRejector,
    #[cfg(test)]
    tcp_metrics: ts_netstack_smoltcp::netcore::TcpBufferMetrics,
}

impl DirectGatewayRouter {
    #[cfg(test)]
    pub(crate) async fn start(
        profile: &Profile,
        policy: Arc<GeoDirectPolicy>,
        protector: Arc<dyn SocketProtector>,
        counters: Arc<TrafficCounters>,
        tunnel_dns: Option<(Channel, (Ipv4Addr, Ipv6Addr))>,
        parent_cancellation: &CancellationToken,
    ) -> Result<(Self, mpsc::Receiver<Bytes>), TransportError> {
        Self::start_with_quality(
            profile,
            policy,
            protector,
            counters,
            tunnel_dns,
            None,
            parent_cancellation,
            NetworkQualityTelemetry::default(),
        )
        .await
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "the direct gateway owns policy, protection, counters, DNS, cancellation, and quality state"
    )]
    pub(crate) async fn start_with_quality(
        profile: &Profile,
        policy: Arc<GeoDirectPolicy>,
        protector: Arc<dyn SocketProtector>,
        counters: Arc<TrafficCounters>,
        tunnel_dns: Option<(Channel, (Ipv4Addr, Ipv6Addr))>,
        warp_dns: Option<Arc<crate::encrypted_dns::FinalDohResolver>>,
        parent_cancellation: &CancellationToken,
        quality: NetworkQualityTelemetry,
    ) -> Result<(Self, mpsc::Receiver<Bytes>), TransportError> {
        if profile.uses_encrypted_warp_dns() && warp_dns.is_none() {
            return Err(TransportError::Dns("encrypted_dns_unavailable".into()));
        }
        let (incoming_tx, incoming_rx) = mpsc::channel(DIRECT_PACKET_CAPACITY);
        let rejection_tx = incoming_tx.clone();
        let cancellation = parent_cancellation.child_token();
        let flows = Arc::new(Mutex::new(NatTable::default()));
        // A pushed resolver may be inside a LAN bypass. Publish our synthetic
        // DNS address to the OS and forward its queries inside the final stack.
        let gate_dns = profile.frontends.tunnel
            && (profile.uses_encrypted_warp_dns()
                || (profile.chain_enabled()
                    && (profile.dns_mode == usque_core::DnsMode::Tunnel
                        || profile.custom_chain().is_some())));
        // Prepare Ads DNS for a later hot TUN attachment. Domain-direct proxy
        // sessions always reconnect before attaching TUN, and their system
        // resolver need not expose the numeric servers Split DNS requires.
        let split_dns_enabled = (profile.frontends.tunnel && tunnel_dns.is_some())
            || (policy.supports_live_ads()
                && (profile.frontends.tunnel || !policy.needs_direct_dns()))
            || gate_dns
            || (profile.frontends.tunnel && profile.needs_domain_routing() && policy.is_enabled());
        if profile.frontends.tunnel
            && split_dns_enabled
            && policy.has_direct_routes()
            && !protector.tun_direct_available()
        {
            return Err(TransportError::Dns(
                "platform cannot safely bypass the TUN for Split DNS".to_owned(),
            ));
        }
        if (!policy.is_enabled() && !split_dns_enabled && !protector.tun_direct_available())
            || (!protector.tun_direct_available() && !split_dns_enabled)
        {
            return Ok((
                Self {
                    channel: None,
                    stack_incoming: None,
                    policy,
                    protector,
                    counters,
                    diagnostics: quality.register_direct_gateway(),
                    flows,
                    cancellation,
                    stack_task: None,
                    incoming_task: None,
                    split_dns: None,
                    dns_hints: Arc::new(DnsRouteCache::default()),
                    rejection_tx,
                    rejector: Default::default(),
                    #[cfg(test)]
                    tcp_metrics: Default::default(),
                },
                incoming_rx,
            ));
        }

        let (config, _tcp_metrics) = direct_netstack_config(profile);
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
        let crate::packet_pipe::PacketPipe {
            mut rx,
            tx: stack_incoming,
        } = pipe;
        let split_dns = if split_dns_enabled {
            let Some((tunnel_channel, assigned_addresses)) = tunnel_dns else {
                stack_task.abort();
                return Err(TransportError::Dns(
                    "Split DNS is missing the MASQUE resolver channel".to_owned(),
                ));
            };
            match SplitDnsRuntime::start(
                &channel,
                SplitDnsConfig::new(
                    tunnel_channel,
                    assigned_addresses,
                    &profile.dns_servers,
                    Arc::clone(&policy),
                    Arc::clone(&protector),
                    quality.clone(),
                )
                .with_final_exit(profile.chain_enabled())
                .with_doh(warp_dns),
                &cancellation,
            )
            .await
            {
                Ok(runtime) => Some(runtime),
                Err(error) => {
                    stack_task.abort();
                    return Err(TransportError::Dns(format!(
                        "start Split DNS gateway: {error}"
                    )));
                }
            }
        } else {
            None
        };
        let dns_hints = split_dns.as_ref().map_or_else(
            || Arc::new(DnsRouteCache::default()),
            |dns| Arc::clone(&dns.hints),
        );
        let incoming_flows = Arc::clone(&flows);
        let incoming_cancel = cancellation.clone();
        let incoming_counters = Arc::clone(&counters);
        let diagnostics = quality.register_direct_gateway();
        let incoming_diagnostics = Arc::clone(&diagnostics);
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
                if route_incoming(&incoming_flows, &mut packet)
                    || is_split_dns_server_packet(&packet)
                {
                    incoming_counters.record_received(packet.len());
                    if !try_send_return_packet(&incoming_tx, packet.freeze(), &incoming_diagnostics)
                    {
                        break;
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
                diagnostics,
                flows,
                cancellation,
                stack_task: Some(stack_task),
                incoming_task: Some(incoming_task),
                split_dns,
                dns_hints,
                rejection_tx,
                rejector: Default::default(),
                #[cfg(test)]
                tcp_metrics: _tcp_metrics,
            },
            incoming_rx,
        ))
    }

    /// Returns true when the packet was consumed by the direct gateway.
    pub(crate) async fn route_outgoing(&mut self, packet: &mut BytesMut) -> bool {
        let parsed = NatPacket::parse(packet);
        let network_generation = self.protector.network_generation();
        let routing = self.policy.freeze_routing();
        let flow = parsed.as_ref().map(|parsed| FlowKey {
            protocol: parsed.protocol,
            client: SocketAddr::new(parsed.source, parsed.source_port),
            remote: SocketAddr::new(parsed.destination, parsed.destination_port),
        });
        let existing = flow.as_ref().and_then(|flow| {
            self.flows
                .lock()
                .ok()
                .and_then(|mut flows| flows.existing(flow, network_generation))
        });
        let admitted_flow = existing.is_some();
        let internal_dns =
            self.split_dns.is_some() && parsed.as_ref().is_some_and(is_split_dns_client_packet);
        if existing.is_none()
            && !internal_dns
            && crate::routing_reject::destination(packet).is_some_and(|ip| routing.rejects_ip(ip))
        {
            if let Some(reply) = self.rejector.reply(packet) {
                try_send_return_packet(&self.rejection_tx, reply, &self.diagnostics);
            }
            return true;
        }
        let worker_failed = self
            .stack_task
            .as_ref()
            .is_some_and(JoinHandle::is_finished)
            || self
                .incoming_task
                .as_ref()
                .is_some_and(JoinHandle::is_finished);
        let Some(parsed) = parsed else {
            return false;
        };
        if !matches!(parsed.protocol, 6 | 17)
            || parsed.source.is_ipv4() != parsed.destination.is_ipv4()
        {
            return false;
        }
        let split_dns_client = self.split_dns.is_some() && is_split_dns_client_packet(&parsed);
        let route = if existing.is_some() {
            GeoRoute::Direct
        } else {
            self.dns_hints.route_ip(
                parsed.destination,
                self.protector.network_generation(),
                &routing,
            )
        };
        if !split_dns_client && route == GeoRoute::Reject {
            if let Some(reply) = self.rejector.reply(packet) {
                try_send_return_packet(&self.rejection_tx, reply, &self.diagnostics);
            }
            return true;
        }
        if !split_dns_client
            && route == GeoRoute::Direct
            && !crate::geo_direct::direct_family_available(
                self.protector.as_ref(),
                SocketAddr::new(parsed.destination, parsed.destination_port),
            )
        {
            // Consume before allocation or any worker-failure fallback. In
            // particular, failed UDP setup must not send IPv6 through WARP
            // when the selected physical network only carries IPv4.
            if let Some(reply) = self.rejector.reply(packet) {
                try_send_return_packet(&self.rejection_tx, reply, &self.diagnostics);
            }
            return true;
        }
        // Unsupported destinations cannot acquire a physical-egress lease.
        // Keep the existing tunnel fallback without repeatedly authenticating
        // an Agent request that will be refused, including mapped IPv4 targets.
        let target = parsed.destination.to_canonical();
        if !split_dns_client
            && route == GeoRoute::Direct
            && (parsed.destination_port == 0 || target.is_unspecified() || target.is_multicast())
        {
            return self.fallback(DirectFallbackReason::UnsupportedTarget);
        }
        if worker_failed {
            if existing.is_some() {
                if let Some(flow) = &flow
                    && let Ok(mut flows) = self.flows.lock()
                {
                    flows.remove(flow);
                }
                if let Some(reply) = self.rejector.reply(packet) {
                    try_send_return_packet(&self.rejection_tx, reply, &self.diagnostics);
                }
                return true;
            }
            // Never let an Engine-internal DNS query fall through into MASQUE
            // if its local listener or packet pump has failed.
            if !split_dns_client && route == GeoRoute::Direct {
                self.diagnostics
                    .record_fallback(DirectFallbackReason::WorkerFailed);
            }
            return split_dns_client;
        }
        let Some(channel) = self.channel.as_ref() else {
            if !split_dns_client && route == GeoRoute::Direct {
                self.diagnostics
                    .record_fallback(DirectFallbackReason::Unavailable);
            }
            return false;
        };
        let Some(stack_incoming) = self.stack_incoming.as_ref() else {
            if !split_dns_client && route == GeoRoute::Direct {
                self.diagnostics
                    .record_fallback(DirectFallbackReason::Unavailable);
            }
            return false;
        };
        if split_dns_client {
            stack_incoming.send_async(packet).await;
            self.counters.record_sent(packet.len());
            return true;
        }
        // A prepared DNS listener does not authorize physical packet egress.
        // Keep the original TUN protection boundary after a hot attachment.
        if route != GeoRoute::Direct {
            return false;
        }
        if !self.protector.tun_direct_available() {
            return self.fallback(DirectFallbackReason::Unavailable);
        }
        let flow = flow.expect("parsed packet has a flow key");
        let (mapping, mut reservation, socket) = if let Some(mapping) = existing {
            (mapping, None, None)
        } else {
            let Some(mapping) = self.flows.lock().ok().and_then(|mut flows| {
                flows.reserve(flow.clone(), network_generation, &self.cancellation)
            }) else {
                return self.fallback(DirectFallbackReason::FlowCapacity);
            };
            let reservation = NatReservation::new(&self.flows, &flow, mapping.id);
            let setup = match parsed.protocol {
                6 => OnceListener::bind(channel.clone(), mapping.gateway)
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
                    return self.fallback(DirectFallbackReason::SetupFailed);
                }
            };
            (mapping, Some(reservation), Some(socket))
        };

        // Authorization can await an Agent or a stack command. Neither a stale
        // generation nor a retired/replaced reservation may become a new flow
        // when that wait completes. RAII releases both socket and reservation.
        if mapping.cancel.is_cancelled()
            || self.cancellation.is_cancelled()
            || mapping.network_generation != self.protector.network_generation()
            || !self.flows.lock().is_ok_and(|flows| {
                flows
                    .forward
                    .get(&flow)
                    .is_some_and(|current| current.id == mapping.id)
            })
        {
            if let Ok(mut flows) = self.flows.lock() {
                flows.remove_if(&flow, mapping.id);
            }
            if admitted_flow {
                if let Some(reply) = self.rejector.reply(packet) {
                    try_send_return_packet(&self.rejection_tx, reply, &self.diagnostics);
                }
                return true;
            }
            return self.fallback(DirectFallbackReason::SetupFailed);
        }
        if !rewrite_destination(packet, &parsed, mapping.gateway) {
            if admitted_flow {
                if let Ok(mut flows) = self.flows.lock() {
                    flows.remove_if(&flow, mapping.id);
                }
                if let Some(reply) = self.rejector.reply(packet) {
                    try_send_return_packet(&self.rejection_tx, reply, &self.diagnostics);
                }
                return true;
            }
            return self.fallback(DirectFallbackReason::RewriteFailed);
        }
        if let Some(socket) = socket {
            self.spawn_flow(flow.clone(), mapping.clone(), socket);
        }
        stack_incoming.send_async(packet).await;
        if let Some(reservation) = reservation.as_mut() {
            reservation.commit();
        }
        self.counters.record_sent(packet.len());
        true
    }

    fn fallback(&self, reason: DirectFallbackReason) -> bool {
        self.diagnostics.record_fallback(reason);
        false
    }

    async fn setup_udp(
        &self,
        channel: &Channel,
        flow: &FlowKey,
        mapping: &Mapping,
    ) -> Result<DirectSocket, String> {
        let (physical, lease) = bind_direct_udp(self.protector.as_ref(), flow.remote).await?;
        physical
            .connect(flow.remote)
            .await
            .map_err(|error| error.to_string())?;
        let local = channel
            .udp_bind(mapping.gateway)
            .await
            .map_err(|error| error.to_string())?;
        Ok(DirectSocket::Udp {
            local,
            physical,
            _lease: lease,
        })
    }

    fn spawn_flow(&self, flow: FlowKey, mapping: Mapping, socket: DirectSocket) {
        let protector = Arc::clone(&self.protector);
        let flows = Arc::clone(&self.flows);
        let diagnostics = Arc::clone(&self.diagnostics);
        tokio::spawn(async move {
            let result = match socket {
                DirectSocket::Tcp(listener) => {
                    run_tcp_flow(listener, &flow, protector, &mapping.cancel).await
                }
                DirectSocket::Udp {
                    local,
                    physical,
                    _lease,
                } => run_udp_flow(local, physical, _lease, &flow, &mapping.cancel).await,
            };
            if let Ok(mut flows) = flows.lock() {
                flows.remove_if(&flow, mapping.id);
            }
            if let Err(error) = result {
                diagnostics.record_flow_failure();
                tracing::debug!(%error, remote = %flow.remote, "GEO direct flow ended");
            }
        });
    }
}

/// Full queues drop one packet but remain usable; closed queues end the pump.
fn try_send_return_packet(
    sender: &mpsc::Sender<Bytes>,
    packet: Bytes,
    diagnostics: &DirectGatewayDiagnostics,
) -> bool {
    match sender.try_send(packet) {
        Ok(()) => true,
        Err(mpsc::error::TrySendError::Full(packet)) => {
            diagnostics.record_return_queue_drop(packet.len());
            true
        }
        Err(mpsc::error::TrySendError::Closed(_)) => false,
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
        self.split_dns.take();
    }
}

fn is_split_dns_client_packet(packet: &NatPacket) -> bool {
    packet.destination_port == 53
        && matches!(
            packet.destination,
            IpAddr::V4(SPLIT_DNS_IPV4) | IpAddr::V6(SPLIT_DNS_IPV6)
        )
}

fn is_split_dns_server_packet(packet: &[u8]) -> bool {
    NatPacket::parse(packet).is_some_and(|packet| {
        packet.source_port == 53
            && matches!(
                packet.source,
                IpAddr::V4(SPLIT_DNS_IPV4) | IpAddr::V6(SPLIT_DNS_IPV6)
            )
    })
}

enum DirectSocket {
    Tcp(OnceListener),
    Udp {
        local: StackUdpSocket,
        physical: tokio::net::UdpSocket,
        _lease: DirectEgressLease,
    },
}

async fn run_tcp_flow(
    listener: OnceListener,
    flow: &FlowKey,
    protector: Arc<dyn SocketProtector>,
    cancellation: &CancellationToken,
) -> Result<(), String> {
    let accepted = tokio::select! {
        _ = cancellation.cancelled() => return Ok(()),
        accepted = listener.accept(flow.client) => accepted.map_err(|error| error.to_string())?,
    };
    let mut local = accepted;
    let (mut physical, _lease) = tokio::select! {
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
    _lease: DirectEgressLease,
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
pub(crate) struct NatPacket {
    pub(crate) version: u8,
    pub(crate) protocol: u8,
    pub(crate) source: IpAddr,
    pub(crate) destination: IpAddr,
    pub(crate) source_port: u16,
    pub(crate) destination_port: u16,
    pub(crate) transport_offset: usize,
    pub(crate) checksum_offset: usize,
    pub(crate) checksum_optional: bool,
}

impl NatPacket {
    pub(crate) fn parse(packet: &[u8]) -> Option<Self> {
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

pub(crate) fn rewrite_destination(
    packet: &mut [u8],
    parsed: &NatPacket,
    gateway: SocketAddr,
) -> bool {
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

pub(crate) fn rewrite_source(packet: &mut [u8], parsed: &NatPacket, remote: SocketAddr) -> bool {
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
    // Keep every fallible check ahead of the first write. `route_outgoing`
    // relies on a false result leaving the packet byte-for-byte unchanged so
    // it can safely fall back to MASQUE without retaining a full packet copy.
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
    let (old_bytes, old_length) = ip_bytes(old_ip);
    let (new_bytes, new_length) = ip_bytes(new_ip);
    if old_length != address_length || new_length != address_length {
        return false;
    }
    let transport_checksum = read_u16(packet, parsed.checksum_offset).unwrap_or_default();
    let has_transport_checksum = !(parsed.checksum_optional && transport_checksum == 0);
    let mut updated_transport = transport_checksum;
    let mut updated_ipv4 = (parsed.version == 4).then(|| read_u16(packet, 10).unwrap_or_default());
    for (old, new) in old_bytes[..old_length]
        .chunks_exact(2)
        .zip(new_bytes[..new_length].chunks_exact(2))
    {
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
    packet[address_offset..address_offset + address_length]
        .copy_from_slice(&new_bytes[..new_length]);
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

fn ip_bytes(ip: IpAddr) -> ([u8; 16], usize) {
    let mut bytes = [0_u8; 16];
    match ip {
        IpAddr::V4(ip) => {
            bytes[..4].copy_from_slice(&ip.octets());
            (bytes, 4)
        }
        IpAddr::V6(ip) => {
            bytes.copy_from_slice(&ip.octets());
            (bytes, 16)
        }
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
    use std::future::Future;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, UdpSocket};
    use tokio::time::timeout;
    use ts_netstack_smoltcp::netcore::{HasChannel, NetstackControl};
    use usque_geo::CountryCode;

    use super::*;
    use crate::geo_direct::GeoDirectClassifier;
    use crate::netstack::proxy_netstack_config;
    use crate::socket::SocketHandle;

    const CLIENT_IPV4: Ipv4Addr = Ipv4Addr::new(10, 0, 0, 2);
    const CLIENT_IPV6: Ipv6Addr = Ipv6Addr::new(0xfd00, 0, 0, 0, 0, 0, 0, 2);
    const TEST_TIMEOUT: Duration = Duration::from_secs(3);

    struct LeaseDrop(Arc<AtomicUsize>);
    impl Drop for LeaseDrop {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    struct PausedProtector {
        calls: AtomicUsize,
        generation: std::sync::atomic::AtomicU64,
        release: tokio::sync::Semaphore,
        dropped_leases: Arc<AtomicUsize>,
    }

    impl PausedProtector {
        fn new() -> Self {
            Self {
                calls: AtomicUsize::new(0),
                generation: std::sync::atomic::AtomicU64::new(7),
                release: tokio::sync::Semaphore::new(0),
                dropped_leases: Arc::new(AtomicUsize::new(0)),
            }
        }
    }

    #[async_trait::async_trait]
    impl SocketProtector for PausedProtector {
        fn protect(&self, _socket: SocketHandle) -> Result<(), String> {
            Ok(())
        }

        fn tun_direct_available(&self) -> bool {
            true
        }

        fn network_generation(&self) -> Option<u64> {
            Some(self.generation.load(Ordering::SeqCst))
        }

        async fn protect_for_target(
            &self,
            _socket: SocketHandle,
            _remote: SocketAddr,
            _protocol: crate::DirectProtocol,
        ) -> Result<DirectEgressLease, String> {
            let generation = self.generation.load(Ordering::SeqCst);
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.release.acquire().await.unwrap().forget();
            Ok(DirectEgressLease::hold_for_generation(
                LeaseDrop(Arc::clone(&self.dropped_leases)),
                generation,
            ))
        }
    }

    async fn paused_gateway(
        protector: Arc<PausedProtector>,
    ) -> (DirectGatewayRouter, mpsc::Receiver<Bytes>) {
        DirectGatewayRouter::start(
            &Profile::default(),
            direct_policy(),
            protector,
            Arc::default(),
            None,
            &CancellationToken::new(),
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn cancelling_pending_udp_setup_releases_only_its_uncommitted_reservation() {
        let first_remote = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let second_remote = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let protector = Arc::new(PausedProtector::new());
        let (mut gateway, _incoming) = paused_gateway(Arc::clone(&protector)).await;
        // Retain the gateway output consumer so an established worker stays healthy.
        // The first packet is admitted before we pause a different new flow.
        protector.release.add_permits(1);
        let first = diagnostic_datagram(first_remote.local_addr().unwrap());
        assert!(gateway.route_outgoing(&mut first.clone()).await);
        let flows = Arc::clone(&gateway.flows);
        assert_eq!(flows.lock().unwrap().forward.len(), 1);
        let mut second = diagnostic_datagram(second_remote.local_addr().unwrap());
        let original = second.clone();
        let mut pending = Box::pin(gateway.route_outgoing(&mut second));
        assert!(
            std::future::poll_fn(|cx| std::task::Poll::Ready(pending.as_mut().poll(cx)))
                .await
                .is_pending()
        );
        assert_eq!(flows.lock().unwrap().forward.len(), 2);
        drop(pending);
        assert_eq!(second, original);
        {
            let table = flows.lock().unwrap();
            assert_eq!(table.forward.len(), 1);
            assert_eq!(table.reverse.len(), 1);
            assert_eq!(table.ports.len(), 1);
            assert_eq!(table.udp_flows, 1);
        }
        assert!(!gateway.cancellation.is_cancelled());
        assert!(gateway.route_outgoing(&mut first.clone()).await);
        assert_eq!(protector.calls.load(Ordering::SeqCst), 2);
        drop(gateway);
        timeout(TEST_TIMEOUT, async {
            while protector.dropped_leases.load(Ordering::SeqCst) != 1 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("established flow lease did not close after gateway shutdown");
    }

    #[tokio::test]
    async fn late_udp_setup_cannot_commit_after_network_or_reservation_changes() {
        for replace_mapping in [false, true] {
            let remote = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
            let protector = Arc::new(PausedProtector::new());
            let (mut gateway, _incoming) = paused_gateway(Arc::clone(&protector)).await;
            let flows = Arc::clone(&gateway.flows);
            let cancellation = gateway.cancellation.clone();
            let mut packet = diagnostic_datagram(remote.local_addr().unwrap());
            let original = packet.clone();
            let mut pending = Box::pin(gateway.route_outgoing(&mut packet));
            assert!(
                std::future::poll_fn(|cx| std::task::Poll::Ready(pending.as_mut().poll(cx)))
                    .await
                    .is_pending()
            );
            let replacement_id = if replace_mapping {
                let mut table = flows.lock().unwrap();
                let key = table.forward.keys().next().unwrap().clone();
                table.remove(&key);
                Some(table.reserve(key, Some(7), &cancellation).unwrap().id)
            } else {
                protector.generation.store(8, Ordering::SeqCst);
                None
            };
            protector.release.add_permits(1);
            assert!(!pending.await);
            assert_eq!(packet, original);
            assert_eq!(protector.dropped_leases.load(Ordering::SeqCst), 1);
            let table = flows.lock().unwrap();
            if let Some(id) = replacement_id {
                assert_eq!(table.forward.len(), 1);
                assert_eq!(table.forward.values().next().unwrap().id, id);
                assert_eq!(table.ports.len(), 1);
            } else {
                assert!(table.forward.is_empty());
                assert!(table.reverse.is_empty());
                assert!(table.ports.is_empty());
                assert_eq!(table.udp_flows, 0);
            }
        }
    }

    #[tokio::test]
    async fn custom_rule_publication_preserves_direct_nat_and_rejects_only_new_flows() {
        let first = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let second = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let protector = Arc::new(PausedProtector::new());
        let (mut gateway, mut incoming) = paused_gateway(protector.clone()).await;
        protector.release.add_permits(1);
        let packet = diagnostic_datagram(first.local_addr().unwrap());
        assert!(gateway.route_outgoing(&mut packet.clone()).await);
        let mut profile = Profile::default();
        profile.routing.rules.push(usque_core::RoutingRule {
            id: uuid::Uuid::new_v4(),
            kind: usque_core::RoutingMatch::Cidr,
            target: "127.0.0.1/32".into(),
            action: usque_core::RoutingAction::Reject,
        });
        gateway
            .policy
            .publish_routing_update(gateway.policy.prepare_routing_update(&profile).unwrap())
            .unwrap();
        assert!(gateway.route_outgoing(&mut packet.clone()).await);
        assert_eq!(gateway.flows.lock().unwrap().forward.len(), 1);
        assert_eq!(protector.calls.load(Ordering::SeqCst), 1);
        let mut rejected = diagnostic_datagram(second.local_addr().unwrap());
        assert!(gateway.route_outgoing(&mut rejected).await);
        let response = timeout(TEST_TIMEOUT, incoming.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(response[9], 1);
        assert_eq!(gateway.flows.lock().unwrap().forward.len(), 1);
        assert_eq!(protector.calls.load(Ordering::SeqCst), 1);
        // Rule changes preserve NAT; physical-generation changes still revoke it.
        protector.generation.store(8, Ordering::SeqCst);
        assert!(gateway.route_outgoing(&mut packet.clone()).await);
        assert!(gateway.flows.lock().unwrap().forward.is_empty());
    }

    #[tokio::test]
    async fn admitted_direct_flow_never_falls_back_when_its_stack_worker_exits() {
        let remote = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let protector = Arc::new(PausedProtector::new());
        let (mut gateway, mut incoming) = paused_gateway(protector.clone()).await;
        protector.release.add_permits(1);
        let packet = diagnostic_datagram(remote.local_addr().unwrap());
        assert!(gateway.route_outgoing(&mut packet.clone()).await);
        let task = gateway.stack_task.as_mut().unwrap();
        task.abort();
        let _ = task.await;
        assert!(gateway.route_outgoing(&mut packet.clone()).await);
        assert!(gateway.flows.lock().unwrap().forward.is_empty());
        assert_eq!(gateway.diagnostics.snapshot().fallback_total, 0);
        assert_eq!(protector.calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            timeout(TEST_TIMEOUT, incoming.recv())
                .await
                .unwrap()
                .unwrap()[9],
            1
        );
    }

    #[tokio::test]
    async fn empty_tun_policy_has_resources_for_first_direct_rule_and_retains_last_flow() {
        let profile = Profile::default();
        let (tunnel_stack, _pipe) = bounded_piped(proxy_netstack_config(&profile).0);
        let channel = tunnel_stack.command_channel();
        let tunnel_task = tokio_util::task::AbortOnDropHandle::new(tunnel_stack.spawn_tokio());
        let addresses = (Ipv4Addr::new(192, 0, 2, 1), "2001:db8::1".parse().unwrap());
        channel
            .set_ips([IpAddr::V4(addresses.0), IpAddr::V6(addresses.1)])
            .await
            .unwrap();
        let policy = Arc::new(GeoDirectPolicy::disabled());
        let protector = Arc::new(PausedProtector::new());
        let (mut gateway, _incoming) = DirectGatewayRouter::start(
            &profile,
            policy.clone(),
            protector.clone(),
            Arc::default(),
            Some((channel, addresses)),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        assert!(gateway.channel.is_some());
        assert!(gateway.split_dns.is_some());
        let remote = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let packet = diagnostic_datagram(remote.local_addr().unwrap());
        let mut updated = profile.clone();
        updated.routing.rules.push(usque_core::RoutingRule {
            id: uuid::Uuid::new_v4(),
            kind: usque_core::RoutingMatch::Cidr,
            target: "127.0.0.1/32".into(),
            action: usque_core::RoutingAction::Direct,
        });
        policy
            .publish_routing_update(policy.prepare_routing_update(&updated).unwrap())
            .unwrap();
        protector.release.add_permits(1);
        assert!(gateway.route_outgoing(&mut packet.clone()).await);
        updated.routing.rules.clear();
        policy
            .publish_routing_update(policy.prepare_routing_update(&updated).unwrap())
            .unwrap();
        assert!(gateway.route_outgoing(&mut packet.clone()).await);
        let new_remote = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        assert!(
            !gateway
                .route_outgoing(&mut diagnostic_datagram(new_remote.local_addr().unwrap()))
                .await
        );
        assert_eq!(gateway.flows.lock().unwrap().forward.len(), 1);
        assert_eq!(protector.calls.load(Ordering::SeqCst), 1);
        drop(gateway);
        drop(tunnel_task);
    }

    #[tokio::test]
    async fn publication_during_route_lookup_preserves_one_captured_decision() {
        struct UpdatingProtector {
            policy: Arc<GeoDirectPolicy>,
            armed: std::sync::atomic::AtomicBool,
            observations: AtomicUsize,
        }
        impl SocketProtector for UpdatingProtector {
            fn protect(&self, _: SocketHandle) -> Result<(), String> {
                Ok(())
            }
            fn network_generation(&self) -> Option<u64> {
                if self.observations.fetch_add(1, Ordering::SeqCst) == 1
                    && self.armed.swap(false, Ordering::SeqCst)
                {
                    let mut profile = Profile::default();
                    profile.routing.rules.push(usque_core::RoutingRule {
                        id: uuid::Uuid::new_v4(),
                        kind: usque_core::RoutingMatch::Cidr,
                        target: "127.0.0.1/32".into(),
                        action: usque_core::RoutingAction::Reject,
                    });
                    self.policy
                        .publish_routing_update(
                            self.policy.prepare_routing_update(&profile).unwrap(),
                        )
                        .unwrap();
                }
                None
            }
            fn tun_direct_available(&self) -> bool {
                true
            }
        }
        let mut profile = Profile::default();
        profile.frontends.tunnel = false;
        profile.routing.rules.push(usque_core::RoutingRule {
            id: uuid::Uuid::new_v4(),
            kind: usque_core::RoutingMatch::Cidr,
            target: "127.0.0.1/32".into(),
            action: usque_core::RoutingAction::Direct,
        });
        let policy = Arc::new(
            GeoDirectPolicy::disabled()
                .with_custom_rules(&profile)
                .unwrap(),
        );
        let protector = Arc::new(UpdatingProtector {
            policy: policy.clone(),
            armed: false.into(),
            observations: 0.into(),
        });
        let (mut gateway, mut incoming) = DirectGatewayRouter::start(
            &profile,
            policy,
            protector.clone(),
            Arc::default(),
            None,
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        let remote = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let new_remote = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        protector.observations.store(0, Ordering::SeqCst);
        protector.armed.store(true, Ordering::SeqCst);
        let mut packet = diagnostic_datagram(remote.local_addr().unwrap());
        assert!(
            gateway.route_outgoing(&mut packet).await,
            "a captured DIRECT decision cannot be reinterpreted as tunnel fallback"
        );
        assert_eq!(gateway.flows.lock().unwrap().forward.len(), 1);
        assert_eq!(
            protector.policy.route_ip(Ipv4Addr::LOCALHOST.into()),
            GeoRoute::Reject
        );
        // The next flow captures the published REJECT and is consumed without
        // creating another mapping. The admitted first flow retains DIRECT.
        assert!(
            gateway
                .route_outgoing(&mut diagnostic_datagram(new_remote.local_addr().unwrap()))
                .await
        );
        assert_eq!(
            timeout(TEST_TIMEOUT, incoming.recv())
                .await
                .unwrap()
                .unwrap()[9],
            1
        );
        assert_eq!(gateway.flows.lock().unwrap().forward.len(), 1);
    }

    #[test]
    fn ordinary_and_one_shot_listeners_share_the_direct_buffer_budget() {
        use ts_netstack_smoltcp::netcore::{Netstack, Request, Response, flume, tcp};
        let (mut config, metrics) = direct_netstack_config(&Profile::default());
        let policy = config.tcp_buffer_policy.as_mut().unwrap();
        policy.preferred_budget = 2 * 1024 * 1024;
        policy.total_budget = 4 * 1024 * 1024;
        let mut stack = Netstack::new(
            config,
            ts_netstack_smoltcp::netcore::smoltcp::time::Instant::from_millis(0),
        );
        let mut command = |command: tcp::listen::Command| {
            let (resp, reply) = flume::bounded(1);
            stack.process_one_cmd(Request {
                handle: None,
                command: command.into(),
                resp,
            });
            reply.try_recv().unwrap()
        };
        let Response::TcpListen(tcp::listen::Response::Listening { handle: ordinary }) =
            command(tcp::listen::Command::Listen {
                local_endpoint: ([127, 0, 0, 1], 53).into(),
            })
        else {
            panic!("ordinary DNS listener must fit");
        };
        let Response::TcpListen(tcp::listen::Response::Listening { handle: once }) =
            command(tcp::listen::Command::ListenOnce {
                local_endpoint: ([127, 0, 0, 1], 40001).into(),
            })
        else {
            panic!("one-shot direct listener must fit");
        };
        assert_eq!(metrics.snapshot().total_bytes, 4 * 1024 * 1024);
        assert_eq!(metrics.snapshot().preferred_sockets, 1);
        assert_eq!(metrics.snapshot().fallback_sockets, 1);
        assert!(matches!(command(tcp::listen::Command::ListenOnce {
            local_endpoint: ([127, 0, 0, 1], 40002).into(),
        }), Response::Error(error) if error.is_tcp_buffer_budget_exhausted()));
        assert_eq!(metrics.snapshot().total_bytes, 4 * 1024 * 1024);
        assert!(matches!(
            command(tcp::listen::Command::Close { handle: once }),
            Response::Ok
        ));
        assert!(matches!(
            command(tcp::listen::Command::Close { handle: ordinary }),
            Response::Ok
        ));
        assert_eq!(metrics.snapshot().total_bytes, 0);
        assert!(matches!(
            command(tcp::listen::Command::ListenOnce {
                local_endpoint: ([127, 0, 0, 1], 40003).into(),
            }),
            Response::TcpListen(_)
        ));
    }

    #[tokio::test]
    async fn proxy_session_prepares_ads_dns_but_tun_direct_still_requires_protection() {
        let dir = tempfile::tempdir().unwrap();
        let mut profile = Profile::default();
        profile.frontends.tunnel = false;
        profile.endpoint.selection = usque_core::EndpointSelection::Custom;
        let (stack, _pipe) = bounded_piped(proxy_netstack_config(&profile).0);
        let channel = stack.command_channel();
        let task = stack.spawn_tokio();
        let addresses = (Ipv4Addr::new(192, 0, 2, 1), "2001:db8::1".parse().unwrap());
        channel
            .set_ips([IpAddr::V4(addresses.0), IpAddr::V6(addresses.1)])
            .await
            .unwrap();
        let cancel = CancellationToken::new();
        for ads_enabled in [false, true] {
            for allow_lan in [false, true] {
                profile.frontends.tunnel = false;
                profile.routing.ads_enabled = ads_enabled;
                profile.allow_lan = allow_lan;
                let policy = Arc::new(
                    GeoDirectPolicy::disabled()
                        .with_custom_rules(&profile)
                        .unwrap()
                        .with_ads(dir.path()),
                );
                let (mut gateway, _) = DirectGatewayRouter::start(
                    &profile,
                    policy.clone(),
                    crate::socket::noop_socket_protector(),
                    Arc::new(TrafficCounters::default()),
                    Some((channel.clone(), addresses)),
                    &cancel,
                )
                .await
                .unwrap();
                assert!(gateway.split_dns.is_some());
                assert!(!gateway.protector.tun_direct_available());
                let mut without_ads = profile.clone();
                without_ads.routing.ads_enabled = false;
                let mut attached = without_ads.clone();
                attached.frontends.tunnel = true;
                assert_eq!(
                    usque_core::classify_reconfigure(&without_ads, &attached),
                    usque_core::ReconfigureClass::HotTunnelAttach,
                );
                // The listener must survive Ads being disabled before a hot
                // attachment and re-enabled afterward.
                policy.update_ads_enabled(false);
                policy.update_ads_enabled(true);
                assert!(gateway.split_dns.is_some());
                let worker = gateway.incoming_task.as_mut().unwrap();
                worker.abort();
                let _ = worker.await;
                let mut internal_dns = diagnostic_datagram(SocketAddr::from((SPLIT_DNS_IPV4, 53)));
                assert!(gateway.route_outgoing(&mut internal_dns).await);
                assert_eq!(gateway.diagnostics.snapshot().fallback_total, 0);
                drop(gateway);
                profile.frontends.tunnel = true;
                let result = DirectGatewayRouter::start(
                    &profile,
                    policy,
                    crate::socket::noop_socket_protector(),
                    Arc::new(TrafficCounters::default()),
                    Some((channel.clone(), addresses)),
                    &cancel,
                )
                .await;
                if allow_lan {
                    assert!(matches!(result, Err(TransportError::Dns(reason))
                        if reason == "platform cannot safely bypass the TUN for Split DNS"));
                } else {
                    let (gateway, _) = result.unwrap();
                    assert!(gateway.split_dns.is_some());
                }
            }
        }
        cancel.cancel();
        task.abort();
    }

    #[tokio::test]
    async fn proxy_direct_domains_do_not_require_numeric_system_dns_but_tun_does() {
        let dir = tempfile::tempdir().unwrap();
        let (stack, _pipe) = bounded_piped(proxy_netstack_config(&Profile::default()).0);
        let channel = stack.command_channel();
        let task = stack.spawn_tokio();
        let addresses = (Ipv4Addr::new(192, 0, 2, 1), "2001:db8::1".parse().unwrap());
        channel
            .set_ips([IpAddr::V4(addresses.0), IpAddr::V6(addresses.1)])
            .await
            .unwrap();
        let cancel = CancellationToken::new();
        for ads_enabled in [false, true] {
            for route in ["bypass", "custom", "country"] {
                let mut profile = Profile::default();
                profile.frontends.tunnel = false;
                profile.endpoint.selection = usque_core::EndpointSelection::Custom;
                profile.routing.ads_enabled = ads_enabled;
                let policy = match route {
                    "bypass" => {
                        profile.bypass_domains.push("direct.test".into());
                        GeoDirectPolicy::disabled()
                    }
                    "custom" => {
                        profile.routing.rules.push(usque_core::RoutingRule {
                            id: uuid::Uuid::new_v4(),
                            kind: usque_core::RoutingMatch::Domain,
                            target: "direct.test".into(),
                            action: usque_core::RoutingAction::Direct,
                        });
                        GeoDirectPolicy::disabled()
                    }
                    "country" => {
                        profile.geo_direct_countries.push("CN".into());
                        GeoDirectPolicy::with_classifier(
                            Arc::new(LoopbackClassifier),
                            [CountryCode::parse("CN").unwrap()],
                        )
                    }
                    _ => unreachable!(),
                };
                let policy = Arc::new(
                    policy
                        .with_custom_rules(&profile)
                        .unwrap()
                        .with_ads(dir.path()),
                );
                let (gateway, _) = DirectGatewayRouter::start(
                    &profile,
                    policy.clone(),
                    crate::socket::noop_socket_protector(),
                    Arc::new(TrafficCounters::default()),
                    Some((channel.clone(), addresses)),
                    &cancel,
                )
                .await
                .unwrap();
                assert!(gateway.split_dns.is_none(), "{route}, Ads={ads_enabled}");
                drop(gateway);

                let mut tunnel_profile = profile.clone();
                tunnel_profile.frontends.tunnel = true;
                // Omitting the proxy's unused listener is safe only because
                // TUN rebuilds its gateway and protected physical DNS path.
                assert_eq!(
                    usque_core::classify_reconfigure(&profile, &tunnel_profile),
                    usque_core::ReconfigureClass::ColdReconnect,
                );
                let unprotected = DirectGatewayRouter::start(
                    &tunnel_profile,
                    policy.clone(),
                    crate::socket::noop_socket_protector(),
                    Arc::new(TrafficCounters::default()),
                    Some((channel.clone(), addresses)),
                    &cancel,
                )
                .await;
                assert!(matches!(unprotected, Err(TransportError::Dns(reason))
                    if reason == "platform cannot safely bypass the TUN for Split DNS"));
                let protected_without_dns = DirectGatewayRouter::start(
                    &tunnel_profile,
                    policy,
                    Arc::new(TestProtector {
                        direct_available: true,
                        reject_protection: false,
                        protect_calls: AtomicUsize::new(0),
                    }),
                    Arc::new(TrafficCounters::default()),
                    Some((channel.clone(), addresses)),
                    &cancel,
                )
                .await;
                assert!(
                    matches!(protected_without_dns, Err(TransportError::Dns(reason))
                    if reason == "start Split DNS gateway: the selected physical network has no DNS server")
                );
            }
        }
        cancel.cancel();
        task.abort();
    }

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

    async fn diagnostic_gateway(
        rules: &[(&str, usque_core::RoutingAction)],
        available: bool,
    ) -> (
        DirectGatewayRouter,
        Arc<TestProtector>,
        mpsc::Receiver<Bytes>,
    ) {
        let protector = Arc::new(TestProtector {
            direct_available: available,
            reject_protection: true,
            protect_calls: AtomicUsize::new(0),
        });
        let (gateway, incoming) = DirectGatewayRouter::start(
            &Profile::default(),
            Arc::new(crate::geo_direct::routing_test_policy(rules)),
            protector.clone(),
            Arc::default(),
            None,
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        (gateway, protector, incoming)
    }

    fn diagnostic_datagram(remote: SocketAddr) -> BytesMut {
        let ipv6 = remote.is_ipv6();
        let response_meta = NatPacket {
            version: if ipv6 { 6 } else { 4 },
            protocol: 17,
            source: remote.ip(),
            destination: if ipv6 {
                CLIENT_IPV6.into()
            } else {
                CLIENT_IPV4.into()
            },
            source_port: remote.port(),
            destination_port: 50_030,
            transport_offset: if ipv6 { 40 } else { 20 },
            checksum_offset: if ipv6 { 46 } else { 26 },
            checksum_optional: !ipv6,
        };
        BytesMut::from(crate::l4::tun_wire::udp_response(&response_meta, b"direct-test").as_ref())
    }

    #[tokio::test]
    async fn unsupported_direct_bursts_preserve_packets_without_nat_or_protection() {
        use usque_core::RoutingAction::Direct;

        let (mut gateway, protector, _incoming) = diagnostic_gateway(
            &[
                ("0.0.0.0/0", Direct),
                ("::/0", Direct),
                ("224.0.0.0/24", Direct),
                ("239.255.255.250", Direct),
                ("ff00::/8", Direct),
            ],
            true,
        )
        .await;
        let targets = [
            "224.0.0.251:5353",
            "239.255.255.250:1900",
            "[ff02::fb]:5353",
            "0.0.0.0:53",
            "[::]:53",
            "[::ffff:224.0.0.251]:5353",
            "[::ffff:0.0.0.0]:53",
            "127.0.0.1:0",
        ];
        for target in targets {
            for _ in 0..4 {
                let original = diagnostic_datagram(target.parse().unwrap());
                let mut packet = original.clone();
                assert!(!gateway.route_outgoing(&mut packet).await, "{target}");
                assert_eq!(packet, original, "{target}");
            }
        }
        let snapshot = gateway.diagnostics.snapshot();
        assert_eq!(
            snapshot.fallback_unsupported_target,
            targets.len() as u64 * 4
        );
        assert_eq!(
            snapshot.fallback_total,
            snapshot.fallback_unsupported_target
        );
        assert_eq!(snapshot.flow_failures, 0);
        assert_eq!(protector.protect_calls.load(Ordering::SeqCst), 0);
        let flows = gateway.flows.lock().unwrap();
        assert!(flows.forward.is_empty());
        assert!(flows.reverse.is_empty());
        assert!(flows.ports.is_empty());
    }

    #[tokio::test]
    async fn unsupported_preflight_preserves_proxy_reject_and_unicast_direct() {
        use usque_core::RoutingAction::{Direct, Proxy, Reject};

        let (mut gateway, protector, _incoming) = diagnostic_gateway(
            &[
                ("0.0.0.0/0", Direct),
                ("239.255.255.250", Proxy),
                ("224.0.0.251", Reject),
            ],
            true,
        )
        .await;
        let mut proxy = diagnostic_datagram("239.255.255.250:1900".parse().unwrap());
        let original = proxy.clone();
        assert!(!gateway.route_outgoing(&mut proxy).await);
        assert_eq!(proxy, original);
        let mut rejected = diagnostic_datagram("224.0.0.251:5353".parse().unwrap());
        assert!(gateway.route_outgoing(&mut rejected).await);
        assert_eq!(gateway.diagnostics.snapshot().fallback_total, 0);
        assert_eq!(protector.protect_calls.load(Ordering::SeqCst), 0);

        // A broad DIRECT rule still admits unicast: the fake protector, rather
        // than the unsupported-target preflight, rejects its physical setup.
        let mut unicast = diagnostic_datagram("127.0.0.1:9".parse().unwrap());
        let original = unicast.clone();
        assert!(!gateway.route_outgoing(&mut unicast).await);
        assert_eq!(unicast, original);
        assert_eq!(protector.protect_calls.load(Ordering::SeqCst), 1);
        let snapshot = gateway.diagnostics.snapshot();
        assert_eq!(snapshot.fallback_total, 1);
        assert_eq!(snapshot.fallback_setup_failed, 1);
        assert_eq!(snapshot.fallback_unsupported_target, 0);
        assert_eq!(snapshot.flow_failures, 0);
        assert!(gateway.flows.lock().unwrap().forward.is_empty());
    }

    #[tokio::test]
    async fn unavailable_gateway_counts_only_matching_direct_fallback() {
        use usque_core::RoutingAction::{Direct, Proxy};

        let (mut gateway, protector, _incoming) =
            diagnostic_gateway(&[("127.0.0.0/8", Direct), ("127.0.0.2", Proxy)], false).await;
        assert!(gateway.channel.is_none());
        for (target, expected) in [("127.0.0.1:9", 1), ("127.0.0.2:9", 1)] {
            let mut packet = diagnostic_datagram(target.parse().unwrap());
            let original = packet.clone();
            assert!(!gateway.route_outgoing(&mut packet).await);
            assert_eq!(packet, original);
            let snapshot = gateway.diagnostics.snapshot();
            assert_eq!(snapshot.fallback_total, expected);
            assert_eq!(snapshot.fallback_unavailable, expected);
        }
        assert!(
            !gateway
                .route_outgoing(&mut BytesMut::from(&b"invalid"[..]))
                .await
        );
        assert_eq!(gateway.diagnostics.snapshot().fallback_total, 1);
        assert_eq!(protector.protect_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn failed_gateway_workers_count_only_matching_direct_fallback() {
        use usque_core::RoutingAction::{Direct, Proxy};

        for incoming_failed in [false, true] {
            let (mut gateway, protector, _incoming) =
                diagnostic_gateway(&[("127.0.0.0/8", Direct), ("127.0.0.2", Proxy)], true).await;
            let worker = if incoming_failed {
                gateway.incoming_task.as_mut().unwrap()
            } else {
                gateway.stack_task.as_mut().unwrap()
            };
            worker.abort();
            let _ = worker.await;
            for (target, expected) in [("127.0.0.1:9", 1), ("127.0.0.2:9", 1)] {
                let mut packet = diagnostic_datagram(target.parse().unwrap());
                let original = packet.clone();
                assert!(!gateway.route_outgoing(&mut packet).await);
                assert_eq!(packet, original);
                let snapshot = gateway.diagnostics.snapshot();
                assert_eq!(snapshot.fallback_total, expected);
                assert_eq!(snapshot.fallback_worker_failed, expected);
            }
            assert_eq!(protector.protect_calls.load(Ordering::SeqCst), 0);
        }
    }

    #[tokio::test]
    async fn exhausted_flow_table_reports_fallback_without_a_setup_attempt() {
        use usque_core::RoutingAction::Direct;

        let (mut gateway, protector, _incoming) =
            diagnostic_gateway(&[("127.0.0.0/8", Direct)], true).await;
        let remote: SocketAddr = "127.0.0.1:9".parse().unwrap();
        {
            let mut flows = gateway.flows.lock().unwrap();
            for index in 0..MAX_DIRECT_UDP_FLOWS {
                assert!(
                    flows
                        .reserve(
                            FlowKey {
                                protocol: 17,
                                client: SocketAddr::from((
                                    CLIENT_IPV4,
                                    10_000 + u16::try_from(index).unwrap()
                                )),
                                remote,
                            },
                            None,
                            &gateway.cancellation
                        )
                        .is_some()
                );
            }
        }
        let mut packet = diagnostic_datagram(remote);
        let original = packet.clone();
        assert!(!gateway.route_outgoing(&mut packet).await);
        assert_eq!(packet, original);
        let snapshot = gateway.diagnostics.snapshot();
        assert_eq!(snapshot.fallback_total, 1);
        assert_eq!(snapshot.fallback_flow_capacity, 1);
        assert_eq!(protector.protect_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn invalid_existing_mapping_is_consumed_and_retired_without_tunnel_fallback() {
        use usque_core::RoutingAction::Direct;

        let (mut gateway, protector, mut incoming) =
            diagnostic_gateway(&[("127.0.0.0/8", Direct)], true).await;
        let remote: SocketAddr = "127.0.0.1:9".parse().unwrap();
        let flow = FlowKey {
            protocol: 17,
            client: SocketAddr::from((CLIENT_IPV4, 50_030)),
            remote,
        };
        {
            let mut flows = gateway.flows.lock().unwrap();
            flows
                .reserve(flow.clone(), None, &gateway.cancellation)
                .unwrap();
            flows.forward.get_mut(&flow).unwrap().gateway =
                SocketAddr::from((GATEWAY_IPV6, FIRST_DYNAMIC_PORT));
        }
        let mut packet = diagnostic_datagram(remote);
        let original = packet.clone();
        assert!(gateway.route_outgoing(&mut packet).await);
        assert_eq!(packet, original);
        let snapshot = gateway.diagnostics.snapshot();
        assert_eq!(snapshot.fallback_total, 0);
        assert_eq!(snapshot.fallback_rewrite_failed, 0);
        assert!(gateway.flows.lock().unwrap().forward.is_empty());
        assert_eq!(incoming.try_recv().unwrap()[9], 1);
        assert_eq!(protector.protect_calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn return_queue_counts_full_drops_and_stops_only_when_closed() {
        let diagnostics = DirectGatewayDiagnostics::default();
        let (sender, mut receiver) = mpsc::channel(1);
        assert!(try_send_return_packet(
            &sender,
            Bytes::from_static(b"first"),
            &diagnostics
        ));
        assert!(try_send_return_packet(
            &sender,
            Bytes::from_static(b"lost"),
            &diagnostics
        ));
        assert_eq!(receiver.try_recv().unwrap(), Bytes::from_static(b"first"));
        assert!(try_send_return_packet(
            &sender,
            Bytes::from_static(b"next"),
            &diagnostics
        ));
        let snapshot = diagnostics.snapshot();
        assert_eq!(snapshot.return_queue_drop_packets, 1);
        assert_eq!(snapshot.return_queue_drop_bytes, 4);
        assert_eq!(snapshot.fallback_total, 0);
        drop(receiver);
        assert!(!try_send_return_packet(
            &sender,
            Bytes::from_static(b"closed"),
            &diagnostics
        ));
        assert_eq!(diagnostics.snapshot(), snapshot);
    }

    #[tokio::test]
    async fn local_rejection_uses_the_same_return_queue_drop_accounting() {
        use usque_core::RoutingAction::Reject;

        let (mut gateway, protector, _incoming) =
            diagnostic_gateway(&[("192.0.2.7", Reject)], false).await;
        let (sender, _receiver) = mpsc::channel(1);
        sender.try_send(Bytes::from_static(b"occupied")).unwrap();
        gateway.rejection_tx = sender;
        let mut packet = diagnostic_datagram("192.0.2.7:9".parse().unwrap());
        assert!(gateway.route_outgoing(&mut packet).await);
        let snapshot = gateway.diagnostics.snapshot();
        assert_eq!(snapshot.return_queue_drop_packets, 1);
        assert!(snapshot.return_queue_drop_bytes > 0);
        assert_eq!(snapshot.fallback_total, 0);
        assert_eq!(protector.protect_calls.load(Ordering::SeqCst), 0);
    }

    struct TestNetwork {
        channel: Channel,
        counters: Arc<TrafficCounters>,
        protector: Arc<TestProtector>,
        flows: Arc<Mutex<NatTable>>,
        client_task: JoinHandle<()>,
        pump_task: JoinHandle<()>,
        cancellation: CancellationToken,
        tcp_metrics: ts_netstack_smoltcp::netcore::TcpBufferMetrics,
        diagnostics: Arc<DirectGatewayDiagnostics>,
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
                None,
                &cancellation,
            )
            .await
            .unwrap();
            let flows = Arc::clone(&gateway.flows);
            let tcp_metrics = gateway.tcp_metrics.clone();
            let diagnostics = Arc::clone(&gateway.diagnostics);
            let crate::packet_pipe::PacketPipe {
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
                tcp_metrics,
                diagnostics,
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
    async fn gate_private_dns_uses_final_packets_without_physical_dns_or_lan_egress() {
        let mut profile = Profile {
            allow_lan: true,
            dns_servers: vec!["10.8.0.1".parse().unwrap()],
            ..Profile::default()
        };
        profile.frontends.tunnel = true;
        profile.vpn_gate.enabled = true;
        let (client, mut client_pipe) = bounded_piped(proxy_netstack_config(&profile).0);
        let client_channel = client.command_channel();
        let client_task = tokio_util::task::AbortOnDropHandle::new(client.spawn_tokio());
        client_channel
            .set_ips([IpAddr::V4(CLIENT_IPV4)])
            .await
            .unwrap();
        let (final_stack, mut final_pipe) = bounded_piped(proxy_netstack_config(&profile).0);
        let final_channel = final_stack.command_channel();
        let final_task = tokio_util::task::AbortOnDropHandle::new(final_stack.spawn_tokio());
        let final_ipv4: Ipv4Addr = "10.8.0.2".parse().unwrap();
        final_channel
            .set_ips([IpAddr::V4(final_ipv4)])
            .await
            .unwrap();
        let protector = Arc::new(TestProtector {
            direct_available: false,
            reject_protection: true,
            protect_calls: AtomicUsize::new(0),
        });
        let cancellation = CancellationToken::new();
        let (mut gateway, _incoming) = DirectGatewayRouter::start(
            &profile,
            Arc::new(GeoDirectPolicy::disabled()),
            protector.clone(),
            Arc::new(TrafficCounters::default()),
            Some((final_channel, (final_ipv4, Ipv6Addr::UNSPECIFIED))),
            &cancellation,
        )
        .await
        .unwrap();
        let query = [
            0x12, 0x34, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 1, b'a', 0, 0, 1, 0, 1,
        ];
        let socket = client_channel
            .udp_bind(SocketAddr::new(CLIENT_IPV4.into(), 50_003))
            .await
            .unwrap();
        socket
            .send_to(SocketAddr::new(SPLIT_DNS_IPV4.into(), 53), &query)
            .await
            .unwrap();
        let packet = timeout(TEST_TIMEOUT, client_pipe.rx.recv_async())
            .await
            .unwrap()
            .unwrap();
        assert!(
            gateway
                .route_outgoing(&mut BytesMut::from(packet.as_ref()))
                .await
        );
        let forwarded = timeout(TEST_TIMEOUT, final_pipe.rx.recv_async())
            .await
            .unwrap()
            .unwrap();
        let parsed = NatPacket::parse(&forwarded).unwrap();
        assert_eq!(parsed.source, IpAddr::V4(final_ipv4));
        assert_eq!(parsed.destination, "10.8.0.1".parse::<IpAddr>().unwrap());
        assert!(forwarded.ends_with(&query));
        assert_eq!(protector.protect_calls.load(Ordering::SeqCst), 0);
        cancellation.cancel();
        drop((gateway, client_task, final_task));
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
        // The connected one-shot socket owns exactly the legacy two 1 MiB
        // buffers, with no additional accept socket waiting behind it.
        assert_eq!(network.tcp_metrics.snapshot().total_bytes, 2 * 1024 * 1024);
        timeout(TEST_TIMEOUT, server)
            .await
            .expect("TCP echo task timed out")
            .unwrap();
        assert!(network.protector.protect_calls.load(Ordering::SeqCst) >= 1);
        let snapshot = network.counters.snapshot();
        assert!(snapshot.bytes_sent > 0);
        assert!(snapshot.bytes_received > 0);
        drop(stream);
        timeout(TEST_TIMEOUT, async {
            while network.tcp_metrics.snapshot().total_bytes != 0
                || !network.flows.lock().unwrap().forward.is_empty()
            {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("normal TCP close must reclaim the socket and NAT reservation");
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
    async fn rejection_consumes_fragmented_and_other_ip_packets_without_a_direct_gateway() {
        let policy = Arc::new(crate::geo_direct::routing_test_policy(&[
            ("192.0.2.0/24", usque_core::RoutingAction::Reject),
            ("2001:db8::/32", usque_core::RoutingAction::Reject),
        ]));
        let protector = Arc::new(TestProtector {
            direct_available: false,
            reject_protection: true,
            protect_calls: AtomicUsize::new(0),
        });
        let (mut gateway, mut incoming) = DirectGatewayRouter::start(
            &Profile::default(),
            policy,
            protector.clone(),
            Arc::default(),
            None,
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        for protocol in [1, 6, 17] {
            let mut packet = vec![0; 28];
            packet[0] = 0x46;
            packet[2..4].copy_from_slice(&28_u16.to_be_bytes());
            packet[6] = 0x20;
            packet[9] = protocol;
            packet[16..20].copy_from_slice(&[192, 0, 2, 7]);
            let original = BytesMut::from(packet.as_slice());
            let mut packet = original.clone();
            assert!(gateway.route_outgoing(&mut packet).await);
            assert_eq!(packet, original);
        }
        let mut v6 = vec![0; 48];
        v6[0] = 0x60;
        v6[4..6].copy_from_slice(&8_u16.to_be_bytes());
        v6[6] = 44;
        v6[24..40].copy_from_slice(&"2001:db8::7".parse::<Ipv6Addr>().unwrap().octets());
        assert!(
            gateway
                .route_outgoing(&mut BytesMut::from(v6.as_slice()))
                .await
        );
        assert_eq!(protector.protect_calls.load(Ordering::SeqCst), 0);
        assert!(gateway.flows.lock().unwrap().forward.is_empty());
        assert!(incoming.try_recv().is_err());
    }

    #[tokio::test]
    async fn unavailable_direct_family_is_consumed_before_nat_or_tunnel_fallback() {
        let protector = Arc::new(crate::geo_direct::LanProbeProtector {
            family_mask: Some(1),
            ..Default::default()
        });
        let policy = Arc::new(crate::geo_direct::routing_test_policy(&[(
            "2001:db8::7",
            usque_core::RoutingAction::Direct,
        )]));
        let (mut gateway, mut incoming) = DirectGatewayRouter::start(
            &Profile::default(),
            policy,
            protector.clone(),
            Arc::default(),
            None,
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        let reply_meta = NatPacket {
            version: 6,
            protocol: 17,
            source: "2001:db8::7".parse().unwrap(),
            destination: CLIENT_IPV6.into(),
            source_port: 443,
            destination_port: 50_000,
            transport_offset: 40,
            checksum_offset: 46,
            checksum_optional: false,
        };
        let original = crate::l4::tun_wire::udp_response(&reply_meta, b"direct");
        let mut packet = BytesMut::from(original.as_ref());
        assert!(gateway.route_outgoing(&mut packet).await);
        assert_eq!(packet.as_ref(), original.as_ref());
        let reply = incoming.try_recv().expect("local IPv6 unreachable reply");
        assert_eq!(reply[6], 58);
        assert!(gateway.flows.lock().unwrap().forward.is_empty());
        assert!(protector.attempts.lock().unwrap().is_empty());

        // The dual-stack tunnel remains usable for an unmatched IPv6 target.
        let mut proxy_meta = reply_meta;
        proxy_meta.source = "2001:db8::8".parse().unwrap();
        let mut packet =
            BytesMut::from(crate::l4::tun_wire::udp_response(&proxy_meta, b"proxy").as_ref());
        assert!(!gateway.route_outgoing(&mut packet).await);
        assert_eq!(gateway.diagnostics.snapshot().fallback_total, 0);
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
            None,
            &cancellation,
        )
        .await
        .unwrap();
        let original = BytesMut::from(&[0x45, 0, 0, 20][..]);
        let mut packet = original.clone();
        assert!(!gateway.route_outgoing(&mut packet).await);
        assert_eq!(packet, original);
        drop(gateway);
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
                if attempted && empty && network.diagnostics.snapshot().flow_failures > 0 {
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
        assert!(network.diagnostics.snapshot().flow_failures > 0);
        assert_eq!(network.diagnostics.snapshot().fallback_total, 0);
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
        let (old_bytes, old_length) = ip_bytes(old_ip);
        let (new_bytes, new_length) = ip_bytes(new_ip);
        let initial_checksum = (1..=u16::MAX)
            .find(|candidate| {
                let mut updated = *candidate;
                for (old, new) in old_bytes[..old_length]
                    .chunks_exact(2)
                    .zip(new_bytes[..new_length].chunks_exact(2))
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
    fn failed_nat_rewrite_never_mutates_the_packet() {
        let mut packet = vec![0_u8; 28];
        packet[0] = 0x45;
        packet[2..4].copy_from_slice(&28_u16.to_be_bytes());
        packet[9] = 17;
        packet[12..16].copy_from_slice(&CLIENT_IPV4.octets());
        packet[16..20].copy_from_slice(&Ipv4Addr::LOCALHOST.octets());
        packet[20..22].copy_from_slice(&50_007_u16.to_be_bytes());
        packet[22..24].copy_from_slice(&53_u16.to_be_bytes());
        packet[24..26].copy_from_slice(&8_u16.to_be_bytes());
        let parsed = NatPacket::parse(&packet).expect("valid UDP packet");

        let original = packet.clone();
        assert!(!rewrite_destination(
            &mut packet,
            &parsed,
            SocketAddr::new(IpAddr::V6(GATEWAY_IPV6), 60_000),
        ));
        assert_eq!(packet, original);

        for truncated_length in 0..original.len() {
            let mut truncated = original[..truncated_length].to_vec();
            let before = truncated.clone();
            assert!(!rewrite_destination(
                &mut truncated,
                &parsed,
                SocketAddr::new(IpAddr::V4(GATEWAY_IPV4), 60_000),
            ));
            assert_eq!(truncated, before);
        }
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
