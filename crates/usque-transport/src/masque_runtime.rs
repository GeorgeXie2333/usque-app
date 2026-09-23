use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;

use crate::outbound_packet::OutboundPacket;
use crate::packet_pipe::PacketPipe as WakingPipe;
use bytes::{Bytes, BytesMut};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use usque_core::{Profile, ProxyAuthCredentials, ProxyDnsMode};

use crate::direct_gateway::DirectGatewayRouter;
use crate::geo_direct::GeoDirectPolicy;
use crate::h2::{MasqueTlsIdentity, TransportError};
use crate::http_proxy::HttpProxyFrontend;
use crate::netstack::{
    ManagedTunnelMonitor, ManagedTunnelRuntime, PacketStack, ProxyPerformanceSnapshot,
    RuntimeHealth, RuntimePath, TrafficSnapshot,
};
use crate::network_quality::NetworkQualityTelemetry;
use crate::packet_batch::{MAX_PACKET_BATCH_BYTES, PACKET_BATCH_CHANNEL_CAPACITY, PacketBatch};
use crate::packet_mux::{PacketMuxTable, PacketOrigin};
use crate::pin_refresh::EndpointPinRefresher;
use crate::queue_metrics::{
    QueueKind, TrackedReceiver, TrackedSendErrorKind, TrackedSender, tracked_channel,
};
use crate::socket::{SocketProtector, noop_socket_protector};
use crate::socks5::Socks5Frontend;
use crate::telemetry::ConnectionTimelineSnapshot;

const PACKET_QUEUE_CAPACITY: usize = 1_024;
const PACKET_QUEUE_BYTE_CAPACITY: usize = PACKET_QUEUE_CAPACITY * u16::MAX as usize;

/// Exclusive TUN packet I/O for one attach lifetime.
///
/// Dropping this detaches TUN from the mux without closing MASQUE. Inbound
/// TUN-origin packets are discarded until [`MasqueRuntime::attach_tun`].
pub struct MasqueTunIo {
    outgoing: TrackedSender<TunOutbound>,
    incoming: TrackedReceiver<PacketBatch>,
    pending_incoming: PacketBatch,
    cancellation: CancellationToken,
    quality: NetworkQualityTelemetry,
}

struct TunOutbound {
    packet: OutboundPacket,
    attachment: CancellationToken,
}

impl MasqueTunIo {
    /// Own the send state so platform packet pumps can continue receiving and
    /// handling control events while this bounded enqueue waits for capacity.
    pub(crate) fn start_send_owned_packet(
        &self,
        packet: Bytes,
    ) -> impl std::future::Future<Output = Result<(), TransportError>> + Send + use<> {
        self.start_send_packet(OutboundPacket::Shared(packet))
    }

    pub(crate) fn start_send_mut_packet(
        &self,
        packet: BytesMut,
    ) -> impl std::future::Future<Output = Result<(), TransportError>> + Send + use<> {
        self.start_send_packet(OutboundPacket::Mutable(packet))
    }

    fn start_send_packet(
        &self,
        packet: OutboundPacket,
    ) -> impl std::future::Future<Output = Result<(), TransportError>> + Send + use<> {
        let outgoing = self.outgoing.clone();
        let cancellation = self.cancellation.clone();
        async move {
            crate::h2::validate_ip_packet(&packet)?;
            let bytes = packet.len();
            outgoing
                .send_cancellable(
                    TunOutbound {
                        packet,
                        attachment: cancellation.clone(),
                    },
                    bytes,
                    &cancellation,
                )
                .await
                .map_err(|_| TransportError::TunnelClosed)
        }
    }

    /// Borrowed convenience path for low-frequency callers and tests. Platform
    /// packet pumps must prefer [`Self::send_owned_packet`].
    pub async fn send_packet(&self, packet: &[u8]) -> Result<(), TransportError> {
        crate::h2::validate_ip_packet(packet)?;
        self.quality.record_borrowed_to_owned_copy(packet.len());
        self.send_owned_packet(Bytes::copy_from_slice(packet)).await
    }

    /// Transfers an already-owned packet without allocating or copying it.
    pub async fn send_owned_packet(&self, packet: Bytes) -> Result<(), TransportError> {
        crate::h2::validate_ip_packet(&packet)?;
        let packet_len = packet.len();
        self.outgoing
            .send_cancellable(
                TunOutbound {
                    packet: packet.into(),
                    attachment: self.cancellation.clone(),
                },
                packet_len,
                &self.cancellation,
            )
            .await
            .map_err(|error| match error.kind {
                TrackedSendErrorKind::Closed
                | TrackedSendErrorKind::Cancelled
                | TrackedSendErrorKind::Full
                | TrackedSendErrorKind::ByteLimit => TransportError::TunnelClosed,
            })
    }

    pub fn record_platform_packet_buffer_allocation(&self) {
        self.quality.record_fresh_allocation();
    }

    pub async fn receive_packet(&mut self) -> Result<Bytes, TransportError> {
        loop {
            if let Some(packet) = self.pending_incoming.pop_front() {
                return Ok(packet);
            }
            self.pending_incoming = self
                .incoming
                .recv()
                .await
                .ok_or(TransportError::TunnelClosed)?;
        }
    }

    /// Receives an already queued packet without waiting. This lets platform
    /// packet pumps publish a bounded batch under one kernel wakeup.
    pub fn try_receive_packet(&mut self) -> Result<Option<Bytes>, TransportError> {
        if let Some(packet) = self.pending_incoming.pop_front() {
            return Ok(Some(packet));
        }
        match self.incoming.try_recv() {
            Ok(batch) => {
                self.pending_incoming = batch;
                Ok(self.pending_incoming.pop_front())
            }
            Err(mpsc::error::TryRecvError::Empty) => Ok(None),
            Err(mpsc::error::TryRecvError::Disconnected) => Err(TransportError::TunnelClosed),
        }
    }
}

/// One reconnecting MASQUE connection shared by the platform TUN/VPN and the
/// optional SOCKS5/HTTP listeners.
pub struct MasqueRuntime {
    monitor: ManagedTunnelMonitor,
    stack: PacketStack,
    socks5: Option<Socks5Frontend>,
    socks5_spec: Option<FrontendSpec>,
    http: Option<HttpProxyFrontend>,
    http_spec: Option<FrontendSpec>,
    listeners: Vec<SocketAddr>,
    raw_outgoing: Option<TrackedSender<TunOutbound>>,
    tun_cancellation: CancellationToken,
    tun_sink: watch::Sender<Option<TrackedSender<PacketBatch>>>,
    _tun_sink_rx: watch::Receiver<Option<TrackedSender<PacketBatch>>>,
    quality: NetworkQualityTelemetry,
    cancellation: CancellationToken,
    mux_task: Option<JoinHandle<()>>,
    assigned_ipv4: Ipv4Addr,
    assigned_ipv6: Ipv6Addr,
    internal_network: crate::InternalNetwork,
}

struct BoundFrontends {
    socks5: Option<Vec<tokio::net::TcpListener>>,
    http: Option<Vec<tokio::net::TcpListener>>,
    credentials: Option<ProxyAuthCredentials>,
}

impl MasqueRuntime {
    /// Update all existing application forwarding tasks without replacing flows.
    pub fn update_traffic_policy(&self, disable_quic: bool) {
        self.stack.traffic_policy.set_disable_quic(disable_quic);
    }
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
        Self::start_with_geo_policy(
            profile,
            identity,
            protector,
            pin_refresher,
            Arc::new(GeoDirectPolicy::disabled()),
        )
        .await
    }

    pub async fn start_with_geo_policy(
        profile: &Profile,
        identity: MasqueTlsIdentity,
        protector: Arc<dyn SocketProtector>,
        pin_refresher: Option<Arc<dyn EndpointPinRefresher>>,
        geo_policy: Arc<GeoDirectPolicy>,
    ) -> Result<Self, TransportError> {
        Self::start_configured(
            profile,
            identity,
            protector,
            pin_refresher,
            geo_policy,
            crate::PRODUCTION_NETWORK_FEATURES,
        )
        .await
    }

    #[cfg(any(test, feature = "fault-injection"))]
    pub async fn start_with_features(
        profile: &Profile,
        identity: MasqueTlsIdentity,
        protector: Arc<dyn SocketProtector>,
        pin_refresher: Option<Arc<dyn EndpointPinRefresher>>,
        geo_policy: Arc<GeoDirectPolicy>,
        features: crate::NetworkFeatureFlags,
    ) -> Result<Self, TransportError> {
        Self::start_configured(
            profile,
            identity,
            protector,
            pin_refresher,
            geo_policy,
            features,
        )
        .await
    }

    async fn start_configured(
        profile: &Profile,
        identity: MasqueTlsIdentity,
        protector: Arc<dyn SocketProtector>,
        pin_refresher: Option<Arc<dyn EndpointPinRefresher>>,
        geo_policy: Arc<GeoDirectPolicy>,
        features: crate::NetworkFeatureFlags,
    ) -> Result<Self, TransportError> {
        crate::encrypted_dns::validate_direct_dns_support(&profile.direct_dns)?;
        let credentials = match profile.proxy.listener_credentials() {
            Ok(credentials) => credentials,
            Err(error) => {
                return Err(if profile.frontends.socks5 {
                    TransportError::Socks5(error.to_string())
                } else {
                    TransportError::HttpProxy(error.to_string())
                });
            }
        };

        // Reserve every requested local resource before opening the remote
        // session, so listener conflicts cannot leave a partial runtime.
        let socks5_bound = if profile.frontends.socks5 {
            Some(Socks5Frontend::prebind(profile)?)
        } else {
            None
        };
        let http_bound = if profile.frontends.http {
            Some(HttpProxyFrontend::prebind(profile)?)
        } else {
            None
        };

        let assigned_ipv4 = identity.assigned_ipv4;
        let assigned_ipv6 = identity.assigned_ipv6;
        let tunnel = ManagedTunnelRuntime::start_with_network_features(
            profile,
            identity,
            Arc::clone(&protector),
            pin_refresher,
            features,
        )
        .await?;
        Self::start_bound(
            profile,
            tunnel,
            (assigned_ipv4, assigned_ipv6),
            protector,
            geo_policy,
            BoundFrontends {
                socks5: socks5_bound,
                http: http_bound,
                credentials,
            },
        )
        .await
    }

    /// Reuses the final packet stack, routing and frontend machinery for an
    /// embedded VPN. There is still exactly one system TUN consumer.
    pub(crate) async fn start_over_tunnel(
        profile: &Profile,
        tunnel: ManagedTunnelRuntime,
        addresses: (Ipv4Addr, Ipv6Addr),
        protector: Arc<dyn SocketProtector>,
        policy: Arc<GeoDirectPolicy>,
    ) -> Result<Self, TransportError> {
        let credentials = profile
            .proxy
            .listener_credentials()
            .map_err(|_| TransportError::InvalidIdentity)?;
        let socks5 = profile
            .frontends
            .socks5
            .then(|| Socks5Frontend::prebind(profile))
            .transpose()?;
        let http = profile
            .frontends
            .http
            .then(|| HttpProxyFrontend::prebind(profile))
            .transpose()?;
        Self::start_bound(
            profile,
            tunnel,
            addresses,
            protector,
            policy,
            BoundFrontends {
                socks5,
                http,
                credentials,
            },
        )
        .await
    }

    async fn start_bound(
        profile: &Profile,
        mut tunnel: ManagedTunnelRuntime,
        addresses: (Ipv4Addr, Ipv6Addr),
        protector: Arc<dyn SocketProtector>,
        geo_policy: Arc<GeoDirectPolicy>,
        bound: BoundFrontends,
    ) -> Result<Self, TransportError> {
        let (assigned_ipv4, assigned_ipv6) = addresses;
        let BoundFrontends {
            socks5: socks5_bound,
            http: http_bound,
            credentials,
        } = bound;
        let monitor = tunnel.monitor();
        let quality = monitor.network_quality_telemetry();
        let cancellation = CancellationToken::new();
        let gateway_policy = Arc::clone(&geo_policy);
        let (mut stack, proxy_pipe) = PacketStack::start_detached(
            profile,
            (assigned_ipv4, assigned_ipv6),
            &monitor,
            &cancellation,
            protector,
            geo_policy,
        )
        .await?;
        let internal_network =
            crate::InternalNetwork::for_stack(profile, &stack, assigned_ipv4, assigned_ipv6);
        let gateway_protector = Arc::clone(&stack.protector);
        let (direct_gateway, direct_incoming) = match DirectGatewayRouter::start_with_quality(
            profile,
            gateway_policy,
            gateway_protector,
            Arc::clone(&stack.counters),
            Some((stack.channel.clone(), (assigned_ipv4, assigned_ipv6))),
            &cancellation,
            quality.clone(),
        )
        .await
        {
            Ok(gateway) => gateway,
            Err(error) => {
                stack.shutdown().await;
                tunnel.shutdown().await;
                return Err(error);
            }
        };
        let direct_gateway = DirectGatewayMux {
            router: direct_gateway,
            incoming: direct_incoming,
        };

        let socks5 = socks5_bound
            .map(|bound| {
                Socks5Frontend::activate(profile, assigned_ipv4, assigned_ipv6, &stack, bound)
            })
            .transpose()?;
        let http = http_bound
            .map(|bound| {
                HttpProxyFrontend::activate(profile, assigned_ipv4, assigned_ipv6, &stack, bound)
            })
            .transpose()?;
        let socks5_spec = socks5.as_ref().map(|frontend| {
            FrontendSpec::socks5(frontend.listeners(), profile, credentials.clone())
        });
        let http_spec = http
            .as_ref()
            .map(|frontend| FrontendSpec::http(frontend.listeners(), profile, credentials));
        let listeners = socks5
            .iter()
            .flat_map(|frontend| frontend.listeners().iter().copied())
            .chain(
                http.iter()
                    .flat_map(|frontend| frontend.listeners().iter().copied()),
            )
            .collect();

        tokio::task::yield_now().await;
        if let Some(message) = socks5.as_ref().and_then(Socks5Frontend::failure) {
            stack.shutdown().await;
            tunnel.shutdown().await;
            return Err(TransportError::Socks5(message));
        }
        if let Some(message) = http.as_ref().and_then(HttpProxyFrontend::failure) {
            stack.shutdown().await;
            tunnel.shutdown().await;
            return Err(TransportError::HttpProxy(message));
        }

        let raw_outgoing_metrics = quality.register_queue(
            QueueKind::TunToTransport,
            PACKET_QUEUE_CAPACITY,
            PACKET_QUEUE_BYTE_CAPACITY,
        );
        let (raw_outgoing, raw_outgoing_rx) = tracked_channel(raw_outgoing_metrics);
        let (tun_sink, tun_sink_rx) = watch::channel(None);
        let mux_tun_sink = tun_sink.clone();
        let mux_cancel = cancellation.clone();
        let mux_quality = quality.clone();
        let traffic_policy = Arc::clone(&stack.traffic_policy);
        let mux_task = tokio::spawn(async move {
            run_packet_mux(
                &mut tunnel,
                proxy_pipe,
                raw_outgoing_rx,
                direct_gateway,
                mux_tun_sink,
                &mux_cancel,
                mux_quality,
                traffic_policy,
            )
            .await;
            tunnel.shutdown().await;
        });

        Ok(Self {
            monitor,
            stack,
            socks5,
            socks5_spec,
            http,
            http_spec,
            listeners,
            raw_outgoing: Some(raw_outgoing),
            tun_cancellation: cancellation.child_token(),
            tun_sink,
            _tun_sink_rx: tun_sink_rx,
            quality,
            cancellation,
            mux_task: Some(mux_task),
            assigned_ipv4,
            assigned_ipv6,
            internal_network,
        })
    }

    pub fn internal_network(&self) -> crate::InternalNetwork {
        self.internal_network.clone()
    }

    /// Replace SOCKS5/HTTP listeners without tearing the MASQUE mux.
    ///
    /// A frontend is kept only when its bound addresses and hot-reconfigure
    /// identity (credentials, proxy DNS, and SOCKS UDP idle) still match.
    /// New sockets are bound before any removed frontend is shut down so a
    /// later bind failure can restore from the still-held listeners. Identity
    /// changes on the same addresses still release **that** protocol first,
    /// because Windows will not let a second socket claim the same address;
    /// the other protocol stays live until every bind succeeds.
    pub async fn reconfigure_frontends(&mut self, profile: &Profile) -> Result<(), TransportError> {
        if (profile.frontends.socks5 || profile.frontends.http)
            && let Err(error) = profile.proxy.listener_credentials()
        {
            self.refresh_listeners();
            return Err(if profile.frontends.socks5 {
                TransportError::Socks5(error.to_string())
            } else {
                TransportError::HttpProxy(error.to_string())
            });
        }

        let keep_socks5 = profile.frontends.socks5
            && self.socks5.is_some()
            && self.socks5_spec.as_ref() == FrontendSpec::from_socks5_profile(profile).as_ref();
        let keep_http = profile.frontends.http
            && self.http.is_some()
            && self.http_spec.as_ref() == FrontendSpec::from_http_profile(profile).as_ref();

        let add_socks5 = profile.frontends.socks5 && !keep_socks5;
        let add_http = profile.frontends.http && !keep_http;
        let socks5_rebind_same = add_socks5
            && self.socks5.as_ref().is_some_and(|frontend| {
                listeners_overlap(frontend.listeners(), &profile.proxy.socks5_listeners)
            });
        let http_rebind_same = add_http
            && self.http.as_ref().is_some_and(|frontend| {
                listeners_overlap(frontend.listeners(), &profile.proxy.http_listeners)
            });

        let mut socks5_bound = None;
        if add_socks5 && !socks5_rebind_same {
            match Socks5Frontend::prebind(profile) {
                Ok(bound) => socks5_bound = Some(bound),
                Err(error) => {
                    self.refresh_listeners();
                    return Err(error);
                }
            }
        }
        let mut http_bound = None;
        if add_http && !http_rebind_same {
            match HttpProxyFrontend::prebind(profile) {
                Ok(bound) => http_bound = Some(bound),
                Err(error) => {
                    self.refresh_listeners();
                    return Err(error);
                }
            }
        }

        if socks5_rebind_same && let Some(mut frontend) = self.socks5.take() {
            self.socks5_spec.take();
            frontend.shutdown().await;
        }
        if http_rebind_same && let Some(mut frontend) = self.http.take() {
            self.http_spec.take();
            frontend.shutdown().await;
        }

        if add_socks5 && socks5_rebind_same {
            match Socks5Frontend::prebind(profile) {
                Ok(bound) => socks5_bound = Some(bound),
                Err(error) => {
                    self.refresh_listeners();
                    return Err(error);
                }
            }
        }
        if add_http && http_rebind_same {
            match HttpProxyFrontend::prebind(profile) {
                Ok(bound) => http_bound = Some(bound),
                Err(error) => {
                    self.refresh_listeners();
                    return Err(error);
                }
            }
        }

        if !keep_socks5
            && !socks5_rebind_same
            && let Some(mut frontend) = self.socks5.take()
        {
            self.socks5_spec.take();
            frontend.shutdown().await;
        }
        if !keep_http
            && !http_rebind_same
            && let Some(mut frontend) = self.http.take()
        {
            self.http_spec.take();
            frontend.shutdown().await;
        }

        if let Some(bound) = socks5_bound {
            match Socks5Frontend::activate(
                profile,
                self.assigned_ipv4,
                self.assigned_ipv6,
                &self.stack,
                bound,
            ) {
                Ok(frontend) => {
                    self.socks5_spec = FrontendSpec::from_socks5_frontend(&frontend, profile);
                    self.socks5 = Some(frontend);
                }
                Err(error) => {
                    self.refresh_listeners();
                    return Err(error);
                }
            }
        }
        if let Some(bound) = http_bound {
            match HttpProxyFrontend::activate(
                profile,
                self.assigned_ipv4,
                self.assigned_ipv6,
                &self.stack,
                bound,
            ) {
                Ok(frontend) => {
                    self.http_spec = FrontendSpec::from_http_frontend(&frontend, profile);
                    self.http = Some(frontend);
                }
                Err(error) => {
                    self.refresh_listeners();
                    return Err(error);
                }
            }
        }

        self.refresh_listeners();
        tokio::task::yield_now().await;
        if let Some(message) = self.socks5.as_ref().and_then(Socks5Frontend::failure) {
            return Err(TransportError::Socks5(message));
        }
        if let Some(message) = self.http.as_ref().and_then(HttpProxyFrontend::failure) {
            return Err(TransportError::HttpProxy(message));
        }
        Ok(())
    }

    fn refresh_listeners(&mut self) {
        self.listeners = self
            .socks5
            .iter()
            .flat_map(|frontend| frontend.listeners().iter().copied())
            .chain(
                self.http
                    .iter()
                    .flat_map(|frontend| frontend.listeners().iter().copied()),
            )
            .collect();
    }

    /// Attach TUN I/O. Replaces any previous attach; the old receiver closes.
    pub fn attach_tun(&mut self) -> Result<MasqueTunIo, TransportError> {
        self.tun_cancellation.cancel();
        self.tun_cancellation = self.cancellation.child_token();
        let outgoing = self
            .raw_outgoing
            .clone()
            .ok_or(TransportError::TunnelClosed)?;
        let incoming_metrics = self.quality.register_queue(
            QueueKind::TransportToTun,
            PACKET_BATCH_CHANNEL_CAPACITY,
            PACKET_BATCH_CHANNEL_CAPACITY * MAX_PACKET_BATCH_BYTES,
        );
        let (incoming_tx, incoming) = tracked_channel(incoming_metrics);
        self.tun_sink.send_replace(Some(incoming_tx));
        Ok(MasqueTunIo {
            outgoing,
            incoming,
            pending_incoming: PacketBatch::new(),
            cancellation: self.tun_cancellation.clone(),
            quality: self.quality.clone(),
        })
    }

    /// Stop delivering TUN-origin packets. SOCKS/HTTP and MASQUE stay up.
    pub fn detach_tun(&mut self) {
        self.tun_cancellation.cancel();
        self.tun_sink.send_replace(None);
    }

    /// Borrowed convenience path; steady-state producers should transfer
    /// ownership with [`Self::send_owned_packet`].
    pub async fn send_packet(&self, packet: &[u8]) -> Result<(), TransportError> {
        crate::h2::validate_ip_packet(packet)?;
        self.quality.record_borrowed_to_owned_copy(packet.len());
        self.send_owned_packet(Bytes::copy_from_slice(packet)).await
    }

    pub async fn send_owned_packet(&self, packet: Bytes) -> Result<(), TransportError> {
        crate::h2::validate_ip_packet(&packet)?;
        let packet_len = packet.len();
        self.raw_outgoing
            .as_ref()
            .ok_or(TransportError::TunnelClosed)?
            .send_cancellable(
                TunOutbound {
                    packet: packet.into(),
                    attachment: self.tun_cancellation.clone(),
                },
                packet_len,
                &self.tun_cancellation,
            )
            .await
            .map_err(|error| match error.kind {
                TrackedSendErrorKind::Closed
                | TrackedSendErrorKind::Cancelled
                | TrackedSendErrorKind::Full
                | TrackedSendErrorKind::ByteLimit => TransportError::TunnelClosed,
            })
    }

    pub fn assigned_ipv4(&self) -> Ipv4Addr {
        self.assigned_ipv4
    }

    pub fn assigned_ipv6(&self) -> Ipv6Addr {
        self.assigned_ipv6
    }

    pub fn monitor(&self) -> ManagedTunnelMonitor {
        self.monitor.clone()
    }

    pub fn path(&self) -> RuntimePath {
        self.monitor.path()
    }

    pub fn health(&self) -> RuntimeHealth {
        self.monitor.health()
    }

    pub fn statistics(&self) -> TrafficSnapshot {
        self.monitor.statistics()
    }

    pub fn connection_timeline(&self) -> ConnectionTimelineSnapshot {
        self.monitor.connection_timeline()
    }

    pub fn network_quality(&self) -> crate::NetworkQualitySnapshot {
        self.monitor.network_quality()
    }

    pub fn diagnostic_dns_context(&self) -> (Arc<dyn SocketProtector>, CancellationToken) {
        (
            Arc::clone(&self.stack.protector),
            self.cancellation.child_token(),
        )
    }

    pub fn subscribe_network_quality(&self) -> watch::Receiver<crate::NetworkQualitySnapshot> {
        self.monitor.subscribe_network_quality()
    }

    pub fn performance(&self) -> ProxyPerformanceSnapshot {
        let mut snapshot = self.stack.performance();
        if let Some(http) = &self.http {
            http.augment_performance(&mut snapshot);
        }
        snapshot
    }

    pub fn failure(&self) -> Option<String> {
        self.monitor
            .failure()
            .or_else(|| self.socks5.as_ref().and_then(Socks5Frontend::failure))
            .or_else(|| self.http.as_ref().and_then(HttpProxyFrontend::failure))
    }

    pub fn listeners(&self) -> &[SocketAddr] {
        &self.listeners
    }

    pub fn socks5_listeners(&self) -> &[SocketAddr] {
        self.socks5.as_ref().map_or(&[], Socks5Frontend::listeners)
    }

    pub fn http_listeners(&self) -> &[SocketAddr] {
        self.http.as_ref().map_or(&[], HttpProxyFrontend::listeners)
    }

    pub fn cancel_immediately(&mut self) {
        // Cut every ingress before any slower platform cleanup begins.
        self.raw_outgoing.take();
        if let Some(frontend) = self.socks5.as_mut() {
            frontend.cancel_immediately();
        }
        if let Some(frontend) = self.http.as_mut() {
            frontend.cancel_immediately();
        }
        self.stack.cancel_immediately();
        self.cancellation.cancel();
        if let Some(task) = self.mux_task.as_ref() {
            task.abort();
        }
    }

    /// Revoke user entry points while keeping the internal WARP stack alive.
    pub(crate) fn quiesce_frontends(&mut self) {
        self.detach_tun();
        if let Some(frontend) = self.socks5.as_mut() {
            frontend.cancel_immediately();
        }
        if let Some(frontend) = self.http.as_mut() {
            frontend.cancel_immediately();
        }
        self.listeners.clear();
    }

    pub(crate) async fn suspend_frontends(&mut self) {
        self.quiesce_frontends();
        if let Some(mut frontend) = self.socks5.take() {
            frontend.shutdown().await;
        }
        if let Some(mut frontend) = self.http.take() {
            frontend.shutdown().await;
        }
        self.socks5_spec = None;
        self.http_spec = None;
    }

    pub async fn shutdown(&mut self) {
        self.cancel_immediately();
        if let Some(frontend) = self.socks5.as_mut() {
            frontend.shutdown().await;
        }
        if let Some(frontend) = self.http.as_mut() {
            frontend.shutdown().await;
        }
        self.stack.shutdown().await;
        if let Some(task) = self.mux_task.take() {
            let _ = task.await;
        }
    }
}

impl Drop for MasqueRuntime {
    fn drop(&mut self) {
        self.cancel_immediately();
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "the single mux actor owns tunnel, TUN, proxy, direct, sink, cancellation, and metrics boundaries"
)]
async fn run_packet_mux(
    tunnel: &mut ManagedTunnelRuntime,
    proxy_pipe: WakingPipe,
    mut raw_outgoing: TrackedReceiver<TunOutbound>,
    direct_gateway: DirectGatewayMux,
    tun_sink: watch::Sender<Option<TrackedSender<PacketBatch>>>,
    cancellation: &CancellationToken,
    quality: NetworkQualityTelemetry,
    traffic_policy: Arc<crate::application_traffic::ApplicationTrafficPolicy>,
) {
    let DirectGatewayMux {
        mut router,
        mut incoming,
    } = direct_gateway;
    let WakingPipe {
        mut rx,
        tx: proxy_incoming,
    } = proxy_pipe;
    let sender = match tunnel.packet_sender() {
        Ok(sender) => sender,
        Err(_) => return,
    };
    let mut flows = PacketMuxTable::with_traffic_policy(traffic_policy);
    let mut maintenance = tokio::time::interval_at(
        tokio::time::Instant::now() + crate::packet_mux::MAINTENANCE_INTERVAL,
        crate::packet_mux::MAINTENANCE_INTERVAL,
    );
    maintenance.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut direct_incoming_open = true;
    let mut tun_dropped_batches = 0u64;
    let mut tun_dropped_packets = 0u64;
    let proxy_outgoing = quality.register_queue(
        QueueKind::ProxyToTransport,
        PACKET_QUEUE_CAPACITY,
        PACKET_QUEUE_BYTE_CAPACITY,
    );
    let proxy_incoming_metrics = quality.register_queue(
        QueueKind::TransportToProxy,
        PACKET_QUEUE_CAPACITY,
        PACKET_QUEUE_BYTE_CAPACITY,
    );

    // These own packets and accounting, never a mutable flow-table borrow.
    // No per-packet allocation/task is needed to retain an admission future.
    let mut pending_send = std::pin::pin!(None);
    let mut pending_proxy = std::pin::pin!(None);
    loop {
        let can_send = pending_send.as_ref().get_ref().is_none();
        let can_receive = pending_proxy.as_ref().get_ref().is_none();
        // Cancellation is prioritized, while data directions remain fair.
        let event = tokio::select! {
            biased;
            _ = cancellation.cancelled() => break,
            event = async {
                tokio::select! {
                    _ = maintenance.tick() => MuxEvent::Maintain,
                    packet = raw_outgoing.recv(), if can_send => MuxEvent::Tun(packet),
                    packet = rx.recv_async(), if can_send => MuxEvent::Proxy(packet),
                    batch = tunnel.receive_batch(), if can_receive => MuxEvent::Incoming(batch),
                    packet = incoming.recv(), if direct_incoming_open => MuxEvent::Direct(packet),
                    result = wait_mux_pending(pending_send.as_mut()) => MuxEvent::Sent(result),
                    () = wait_mux_pending(pending_proxy.as_mut()) => MuxEvent::Delivered,
                }
            } => event,
        };
        match event {
            MuxEvent::Maintain => {
                maintain_mux_flows(&mut flows, can_send, std::time::Instant::now())
            }
            MuxEvent::Sent(result) => {
                pending_send.set(None);
                match result {
                    None | Some(Ok(())) => {}
                    Some(Err(TransportError::TunnelClosed)) => break,
                    Some(Err(error)) => {
                        tracing::warn!(%error, "discarded a packet rejected by the MASQUE sender")
                    }
                }
            }
            MuxEvent::Delivered => pending_proxy.set(None),
            MuxEvent::Tun(packet) => {
                let Some(packet) = packet else {
                    break;
                };
                let TunOutbound { packet, attachment } = packet;
                if attachment.is_cancelled() {
                    continue;
                }
                let mut packet = packet.into_mut();
                let inspection = flows.inspect_outgoing(PacketOrigin::Tunnel, &packet);
                if !inspection.is_owned() {
                    // GEO / internal DNS connection creation retains its existing
                    // async lifecycle; it is separate from queue admission.
                    let direct = tokio::select! {
                        biased;
                        _ = cancellation.cancelled() => break,
                        _ = attachment.cancelled() => continue,
                        direct = router.route_outgoing(&mut packet) => direct,
                    };
                    if direct {
                        continue;
                    }
                }
                if flows.route_inspected_outgoing(&mut packet, inspection) {
                    pending_send.set(Some(send_mux_packet(
                        sender.clone(),
                        packet,
                        Some(attachment),
                        None,
                    )));
                }
            }
            MuxEvent::Proxy(packet) => {
                let Some(packet) = packet else {
                    break;
                };
                let queue_entry = proxy_outgoing.start_entry(packet.len());
                let mut packet = packet
                    .try_into_mut()
                    .unwrap_or_else(|packet| bytes::BytesMut::from(packet.as_ref()));
                if flows.route_outgoing(PacketOrigin::Proxy, &mut packet) {
                    pending_send.set(Some(send_mux_packet(
                        sender.clone(),
                        packet,
                        None,
                        Some(queue_entry),
                    )));
                }
                // Rejected packets and cancelled admissions release accounting by RAII.
            }
            MuxEvent::Incoming(batch) => {
                let Ok(mut batch) = batch else {
                    break;
                };
                let mut tun_batch = PacketBatch::new();
                let mut proxy_batch = Vec::new();
                let mut copied_bytes = 0;
                while let Some(packet) = batch.pop_front() {
                    let Some(routed) = flows.route_owned_incoming(packet) else {
                        continue;
                    };
                    copied_bytes += routed.copied_bytes;
                    let packet = routed.packet;
                    match routed.origin {
                        PacketOrigin::Tunnel => tun_batch
                            .push_back(packet)
                            .expect("a subset fits the original bounded batch"),
                        PacketOrigin::Proxy => {
                            let entry = proxy_incoming_metrics.start_entry(packet.len());
                            proxy_batch.push((packet, entry));
                        }
                    }
                }
                if copied_bytes != 0 {
                    crate::transport_performance::add(
                        &quality.performance().incoming_copy_bytes,
                        copied_bytes as u64,
                    );
                }
                // Classify synchronously, deliver all TUN packets before any
                // proxy wait. The pending proxy subset is at most one batch.
                record_tun_sink_drop(
                    dispatch_tun_incoming_batch(&tun_sink, tun_batch),
                    &mut tun_dropped_batches,
                    &mut tun_dropped_packets,
                );
                if !proxy_batch.is_empty() {
                    pending_proxy.set(Some(deliver_proxy_batch(
                        proxy_incoming.clone(),
                        proxy_batch,
                    )));
                }
            }
            MuxEvent::Direct(packet) => match packet {
                Some(packet) => record_tun_sink_drop(
                    dispatch_tun_incoming(&tun_sink, packet),
                    &mut tun_dropped_batches,
                    &mut tun_dropped_packets,
                ),
                None => direct_incoming_open = false,
            },
        }
    }
    pending_send.set(None);
    pending_proxy.set(None);
    if cancellation.is_cancelled() {
        raw_outgoing.cancel();
    }
}

enum MuxEvent {
    Maintain,
    Tun(Option<TunOutbound>),
    Proxy(Option<Bytes>),
    Incoming(Result<PacketBatch, TransportError>),
    Direct(Option<Bytes>),
    Sent(Option<Result<(), TransportError>>),
    Delivered,
}

fn maintain_mux_flows(flows: &mut PacketMuxTable, send_idle: bool, now: std::time::Instant) {
    // A routed packet already contains its wire identifier. Do not remove its
    // reverse mapping while capacity admission still owns that packet.
    if send_idle {
        flows.maintain(now);
    }
}

async fn wait_mux_pending<F: std::future::Future>(
    pending: std::pin::Pin<&mut Option<F>>,
) -> F::Output {
    match pending.as_pin_mut() {
        Some(future) => future.await,
        None => std::future::pending().await,
    }
}

async fn send_mux_packet(
    sender: crate::netstack::ManagedTunnelSender,
    packet: BytesMut,
    attachment: Option<CancellationToken>,
    entry: Option<crate::queue_metrics::QueueEntry>,
) -> Option<Result<(), TransportError>> {
    let cancelled = async {
        match attachment {
            Some(token) => token.cancelled().await,
            None => std::future::pending().await,
        }
    };
    let result = tokio::select! {
        biased;
        () = cancelled => return None,
        result = sender.send_mut_packet(packet) => result,
    };
    if result.is_ok()
        && let Some(entry) = entry
    {
        entry.complete();
    }
    Some(result)
}

async fn deliver_proxy_batch(
    sender: crate::packet_pipe::PacketSender,
    batch: Vec<(Bytes, crate::queue_metrics::QueueEntry)>,
) {
    for (packet, entry) in batch {
        if sender.send_owned_checked(packet).await {
            entry.complete();
        }
    }
}

struct DirectGatewayMux {
    router: DirectGatewayRouter,
    incoming: mpsc::Receiver<Bytes>,
}

/// Deliver a TUN-destined packet, or drop it when TUN is detached.
///
/// A closed or full TUN sink must not tear the MASQUE mux: SOCKS/HTTP still
/// need the session.
fn dispatch_tun_incoming(
    tun_sink: &watch::Sender<Option<TrackedSender<PacketBatch>>>,
    packet: Bytes,
) -> usize {
    dispatch_tun_incoming_batch(tun_sink, PacketBatch::single(packet))
}

fn dispatch_tun_incoming_batch(
    tun_sink: &watch::Sender<Option<TrackedSender<PacketBatch>>>,
    batch: PacketBatch,
) -> usize {
    if batch.is_empty() {
        return 0;
    }
    let sink = tun_sink.borrow().clone();
    let Some(sink) = sink else {
        return 0;
    };
    let bytes = batch.bytes();
    match sink.try_send(batch, bytes) {
        Ok(()) => 0,
        Err(error) => match error.kind {
            TrackedSendErrorKind::Full | TrackedSendErrorKind::ByteLimit => error.value.len(),
            TrackedSendErrorKind::Closed | TrackedSendErrorKind::Cancelled => {
                tun_sink.send_replace(None);
                0
            }
        },
    }
}

fn record_tun_sink_drop(dropped: usize, batches: &mut u64, packets: &mut u64) {
    if dropped == 0 {
        return;
    }
    *batches = batches.saturating_add(1);
    *packets = packets.saturating_add(dropped as u64);
    if batches.is_power_of_two() {
        tracing::warn!(
            dropped_batches = *batches,
            dropped_packets = *packets,
            "dropping inbound TUN batches because the platform packet pump is congested"
        );
    }
}

pub(crate) fn listeners_overlap(active: &[SocketAddr], wanted: &[SocketAddr]) -> bool {
    let active: HashSet<SocketAddr> = active.iter().copied().collect();
    wanted.iter().any(|address| active.contains(address))
}

/// Identity that must match for a hot-reconfigure to keep a live frontend.
///
/// Listener addresses alone are not enough: auth, proxy DNS, and SOCKS UDP
/// idle are also applied at `activate` time and live in the accept-loop
/// context until the frontend is rebuilt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FrontendSpec {
    listeners: HashSet<SocketAddr>,
    credentials: Option<ProxyAuthCredentials>,
    dns_mode: ProxyDnsMode,
    dns_servers: Vec<IpAddr>,
    udp_idle_timeout_seconds: Option<u32>,
}

impl FrontendSpec {
    pub(crate) fn socks5(
        listeners: &[SocketAddr],
        profile: &Profile,
        credentials: Option<ProxyAuthCredentials>,
    ) -> Self {
        Self {
            listeners: listeners.iter().copied().collect(),
            credentials,
            dns_mode: profile.proxy.dns_mode,
            dns_servers: profile.proxy.dns_servers.clone(),
            udp_idle_timeout_seconds: Some(profile.proxy.udp_idle_timeout_seconds),
        }
    }

    pub(crate) fn http(
        listeners: &[SocketAddr],
        profile: &Profile,
        credentials: Option<ProxyAuthCredentials>,
    ) -> Self {
        Self {
            listeners: listeners.iter().copied().collect(),
            credentials,
            dns_mode: profile.proxy.dns_mode,
            dns_servers: profile.proxy.dns_servers.clone(),
            udp_idle_timeout_seconds: None,
        }
    }

    pub(crate) fn from_socks5_profile(profile: &Profile) -> Option<Self> {
        Some(Self::socks5(
            &profile.proxy.socks5_listeners,
            profile,
            profile.proxy.listener_credentials().ok()?,
        ))
    }

    pub(crate) fn from_http_profile(profile: &Profile) -> Option<Self> {
        Some(Self::http(
            &profile.proxy.http_listeners,
            profile,
            profile.proxy.listener_credentials().ok()?,
        ))
    }

    pub(crate) fn from_socks5_frontend(
        frontend: &Socks5Frontend,
        profile: &Profile,
    ) -> Option<Self> {
        Some(Self::socks5(
            frontend.listeners(),
            profile,
            profile.proxy.listener_credentials().ok()?,
        ))
    }

    pub(crate) fn from_http_frontend(
        frontend: &HttpProxyFrontend,
        profile: &Profile,
    ) -> Option<Self> {
        Some(Self::http(
            frontend.listeners(),
            profile,
            profile.proxy.listener_credentials().ok()?,
        ))
    }
}

#[cfg(test)]
#[path = "masque_runtime/mux_progress_tests.rs"]
mod mux_progress_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::queue_metrics::QueueMetrics;
    use std::collections::HashSet;
    use std::sync::{Mutex, OnceLock};
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::time::timeout;
    use usque_core::FrontendSettings;
    use zeroize::Zeroizing;

    fn tracked_bytes(
        kind: QueueKind,
        capacity: usize,
    ) -> (TrackedSender<TunOutbound>, TrackedReceiver<TunOutbound>) {
        tracked_channel(QueueMetrics::new(
            kind,
            capacity,
            capacity * u16::MAX as usize,
        ))
    }

    fn tracked_batches(
        kind: QueueKind,
        capacity: usize,
    ) -> (TrackedSender<PacketBatch>, TrackedReceiver<PacketBatch>) {
        tracked_channel(QueueMetrics::new(
            kind,
            capacity,
            capacity * MAX_PACKET_BATCH_BYTES,
        ))
    }

    pub(super) fn test_tun_io(
        outgoing_capacity: usize,
        incoming_capacity: usize,
    ) -> (
        MasqueTunIo,
        TrackedReceiver<TunOutbound>,
        TrackedSender<PacketBatch>,
    ) {
        let (outgoing, outgoing_rx) = tracked_bytes(QueueKind::TunToTransport, outgoing_capacity);
        let (incoming_tx, incoming) = tracked_batches(QueueKind::TransportToTun, incoming_capacity);
        (
            MasqueTunIo {
                outgoing,
                incoming,
                pending_incoming: PacketBatch::new(),
                cancellation: CancellationToken::new(),
                quality: NetworkQualityTelemetry::default(),
            },
            outgoing_rx,
            incoming_tx,
        )
    }

    pub(super) fn mux_udp_packet(source_port: u16) -> Bytes {
        let mut packet = vec![0u8; 28];
        packet[0] = 0x45;
        packet[2..4].copy_from_slice(&28_u16.to_be_bytes());
        packet[8] = 64;
        packet[9] = 17;
        packet[12..16].copy_from_slice(&[172, 16, 0, 2]);
        packet[16..20].copy_from_slice(&[198, 51, 100, 1]);
        packet[20..22].copy_from_slice(&source_port.to_be_bytes());
        packet[22..24].copy_from_slice(&443_u16.to_be_bytes());
        packet[24..26].copy_from_slice(&8_u16.to_be_bytes());
        let mut sum = 0u32;
        for word in packet[..20].chunks_exact(2) {
            sum += u32::from(u16::from_be_bytes([word[0], word[1]]));
        }
        while sum > 0xffff {
            sum = (sum & 0xffff) + (sum >> 16);
        }
        packet[10..12].copy_from_slice(&(!(sum as u16)).to_be_bytes());
        Bytes::from(packet)
    }

    #[test]
    fn frontend_spec_includes_auth_dns_and_idle() {
        let profile = Profile::default();
        let socks = FrontendSpec::from_socks5_profile(&profile).unwrap();
        let http = FrontendSpec::from_http_profile(&profile).unwrap();

        let mut auth = profile.clone();
        auth.proxy.auth_username = Some("lan-user".to_owned());
        auth.proxy.auth_password = Some(Zeroizing::new(b"s3cret".to_vec()));
        assert_ne!(socks, FrontendSpec::from_socks5_profile(&auth).unwrap());
        assert_ne!(http, FrontendSpec::from_http_profile(&auth).unwrap());

        let mut dns = profile.clone();
        dns.proxy.dns_mode = ProxyDnsMode::System;
        assert_ne!(socks, FrontendSpec::from_socks5_profile(&dns).unwrap());
        assert_ne!(http, FrontendSpec::from_http_profile(&dns).unwrap());

        let mut servers = profile.clone();
        servers.proxy.dns_servers = vec!["8.8.8.8".parse().unwrap()];
        assert_ne!(socks, FrontendSpec::from_socks5_profile(&servers).unwrap());
        assert_ne!(http, FrontendSpec::from_http_profile(&servers).unwrap());

        let mut idle = profile.clone();
        idle.proxy.udp_idle_timeout_seconds = 12;
        assert_ne!(socks, FrontendSpec::from_socks5_profile(&idle).unwrap());
        assert_eq!(http, FrontendSpec::from_http_profile(&idle).unwrap());
    }

    #[tokio::test]
    async fn reconfigure_rebuilds_auth_on_identical_ports() {
        let socks_addr = free_loopback();
        let http_addr = free_loopback();
        let profile = proxy_profile(socks_addr, http_addr);
        let mut runtime = start_local(&profile).await;

        assert_eq!(socks_no_auth_method(socks_addr).await, 0);

        let mut authed = profile.clone();
        authed.proxy.auth_username = Some("lan-user".to_owned());
        authed.proxy.auth_password = Some(Zeroizing::new(b"s3cret".to_vec()));
        runtime.reconfigure_frontends(&authed).await.unwrap();
        assert_eq!(runtime.socks5_listeners(), &[socks_addr]);
        assert_eq!(runtime.http_listeners(), &[http_addr]);

        assert_eq!(socks_no_auth_method(socks_addr).await, 0xff);
        assert_eq!(
            socks_userpass_status(socks_addr, b"lan-user", b"wrong").await,
            1
        );
        assert_eq!(
            socks_userpass_status(socks_addr, b"lan-user", b"s3cret").await,
            0
        );
        assert_eq!(http_status(http_addr, None).await, 407);
        assert_eq!(
            http_status(http_addr, Some("Basic bGFuLXVzZXI6d3Jvbmc=")).await,
            407
        );

        authed.proxy.auth_password = Some(Zeroizing::new(b"new".to_vec()));
        runtime.reconfigure_frontends(&authed).await.unwrap();
        assert_eq!(
            socks_userpass_status(socks_addr, b"lan-user", b"s3cret").await,
            1
        );
        assert_eq!(
            socks_userpass_status(socks_addr, b"lan-user", b"new").await,
            0
        );
        assert_eq!(
            http_status(http_addr, Some("Basic bGFuLXVzZXI6czNjcmV0")).await,
            407
        );

        runtime.shutdown().await;
    }

    #[tokio::test]
    async fn reconfigure_bind_failure_leaves_previous_listeners() {
        let socks_addr = free_loopback();
        let http_addr = free_loopback();
        let profile = proxy_profile(socks_addr, http_addr);
        let mut runtime = start_local(&profile).await;
        let previous = runtime.listeners().to_vec();
        let previous_socks = runtime.socks5_listeners().to_vec();
        let previous_http = runtime.http_listeners().to_vec();

        let occupied = std::net::TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
            .expect("hold HTTP port");
        let occupied_addr = occupied.local_addr().expect("occupied addr");
        let next_socks = free_loopback();
        let mut next = profile.clone();
        next.proxy.socks5_listeners = vec![next_socks];
        next.proxy.http_listeners = vec![occupied_addr];

        let error = runtime
            .reconfigure_frontends(&next)
            .await
            .expect_err("HTTP bind must fail");
        assert!(matches!(
            error,
            TransportError::HttpProxyListener { address, .. } if address == occupied_addr
        ));
        assert_eq!(runtime.listeners(), previous.as_slice());
        assert_eq!(runtime.socks5_listeners(), previous_socks.as_slice());
        assert_eq!(runtime.http_listeners(), previous_http.as_slice());
        assert_eq!(socks_no_auth_method(socks_addr).await, 0);
        tokio::net::TcpStream::connect(http_addr)
            .await
            .expect("previous HTTP still accepts");
        std::net::TcpListener::bind(next_socks).expect("failed SOCKS bind must not keep the port");

        drop(occupied);
        runtime.shutdown().await;
    }

    #[tokio::test]
    async fn reconfigure_mixed_overlap_bind_failure_keeps_disjoint_listeners() {
        let socks_addr = free_loopback();
        let http_addr = free_loopback();
        let profile = proxy_profile(socks_addr, http_addr);
        let mut runtime = start_local(&profile).await;
        let previous_socks = runtime.socks5_listeners().to_vec();

        let occupied = std::net::TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
            .expect("hold extra HTTP port");
        let occupied_addr = occupied.local_addr().expect("occupied addr");
        let next_socks = free_loopback();
        let mut next = profile.clone();
        next.proxy.socks5_listeners = vec![next_socks];
        next.proxy.http_listeners = vec![http_addr, occupied_addr];

        let error = runtime
            .reconfigure_frontends(&next)
            .await
            .expect_err("HTTP overlap bind must fail");
        assert!(matches!(
            error,
            TransportError::HttpProxyListener { address, .. } if address == occupied_addr
        ));
        assert_eq!(runtime.socks5_listeners(), previous_socks.as_slice());
        assert_eq!(runtime.listeners(), previous_socks.as_slice());
        assert_eq!(socks_no_auth_method(socks_addr).await, 0);
        std::net::TcpListener::bind(next_socks).expect("failed SOCKS bind must not keep the port");
        // Same-port HTTP expansion had to release that protocol; SOCKS must stay.

        drop(occupied);
        runtime.shutdown().await;
    }

    #[tokio::test]
    async fn reconfigure_rejects_missing_password_without_dropping_listeners() {
        let socks_addr = free_loopback();
        let http_addr = free_loopback();
        let profile = proxy_profile(socks_addr, http_addr);
        let mut runtime = start_local(&profile).await;
        let previous = runtime.listeners().to_vec();

        let mut missing = profile.clone();
        missing.proxy.auth_username = Some("lan-user".to_owned());
        let error = runtime
            .reconfigure_frontends(&missing)
            .await
            .expect_err("missing password must fail");
        assert!(matches!(error, TransportError::Socks5(_)));
        assert_eq!(runtime.listeners(), previous.as_slice());
        assert_eq!(socks_no_auth_method(socks_addr).await, 0);

        runtime.shutdown().await;
    }

    #[tokio::test]
    async fn activate_rejects_missing_listener_password() {
        let profile = proxy_profile(free_loopback(), free_loopback());
        let mut runtime = start_local(&profile).await;

        let mut missing = profile.clone();
        missing.proxy.auth_username = Some("lan-user".to_owned());
        missing.proxy.socks5_listeners = vec![free_loopback()];
        missing.proxy.http_listeners = vec![free_loopback()];
        let socks_bound = Socks5Frontend::prebind(&missing).expect("bind SOCKS5");
        let http_bound = HttpProxyFrontend::prebind(&missing).expect("bind HTTP");
        assert!(matches!(
            Socks5Frontend::activate(
                &missing,
                runtime.assigned_ipv4,
                runtime.assigned_ipv6,
                &runtime.stack,
                socks_bound,
            ),
            Err(TransportError::Socks5(_))
        ));
        assert!(matches!(
            HttpProxyFrontend::activate(
                &missing,
                runtime.assigned_ipv4,
                runtime.assigned_ipv6,
                &runtime.stack,
                http_bound,
            ),
            Err(TransportError::HttpProxy(_))
        ));

        runtime.shutdown().await;
    }

    #[tokio::test]
    async fn detached_tun_sink_drops_packets_without_closing_the_channel() {
        let (tun_sink, _rx) = watch::channel(None);
        let (tx, mut rx) = tracked_batches(QueueKind::TransportToTun, 4);
        tun_sink.send_replace(Some(tx.clone()));
        assert_eq!(
            dispatch_tun_incoming(&tun_sink, Bytes::from_static(b"keep")),
            0
        );
        assert_eq!(
            rx.recv().await.unwrap().pop_front().unwrap(),
            Bytes::from_static(b"keep")
        );

        tun_sink.send_replace(None);
        assert_eq!(
            dispatch_tun_incoming(&tun_sink, Bytes::from_static(b"drop")),
            0
        );
        assert!(rx.try_recv().is_err());
        assert!(tun_sink.borrow().is_none());

        drop(rx);
        tun_sink.send_replace(Some(tx));
        assert_eq!(
            dispatch_tun_incoming(&tun_sink, Bytes::from_static(b"closed")),
            0
        );
        assert!(tun_sink.borrow().is_none());
    }

    #[tokio::test]
    async fn saturated_tun_sink_drops_only_payload_and_recovers() {
        let (tun_sink, _watch) = watch::channel(None);
        let (tx, mut rx) = tracked_batches(QueueKind::TransportToTun, 1);
        tun_sink.send_replace(Some(tx));
        assert_eq!(
            dispatch_tun_incoming(&tun_sink, Bytes::from_static(b"first")),
            0
        );
        assert_eq!(
            dispatch_tun_incoming(&tun_sink, Bytes::from_static(b"overflow")),
            1
        );
        assert!(tun_sink.borrow().is_some());
        assert_eq!(
            rx.recv().await.unwrap().pop_front().unwrap(),
            Bytes::from_static(b"first")
        );
        assert_eq!(
            dispatch_tun_incoming(&tun_sink, Bytes::from_static(b"recovered")),
            0
        );
        assert_eq!(
            rx.recv().await.unwrap().pop_front().unwrap(),
            Bytes::from_static(b"recovered")
        );
    }

    #[tokio::test]
    async fn tun_quic_policy_preserves_live_geo_flow_and_filters_only_tunnel_packets() {
        struct LocalGeo;
        impl crate::GeoDirectClassifier for LocalGeo {
            fn host_matches(&self, _: &str, _: &usque_geo::CountryCode) -> bool {
                false
            }
            fn ip_matches(&self, ip: std::net::IpAddr, _: &usque_geo::CountryCode) -> bool {
                ip.is_loopback()
            }
        }
        struct Protector;
        impl SocketProtector for Protector {
            fn protect(&self, _: crate::SocketHandle) -> Result<(), String> {
                Ok(())
            }
            fn tun_direct_available(&self) -> bool {
                true
            }
        }
        fn wire(remote: [u8; 4], local_port: u16, remote_port: u16, reply: bool) -> Bytes {
            let mut packet = mux_udp_packet(local_port).to_vec();
            packet[16..20].copy_from_slice(&remote);
            packet[22..24].copy_from_slice(&remote_port.to_be_bytes());
            if reply {
                for n in 0..4 {
                    packet.swap(12 + n, 16 + n);
                }
                for n in 0..2 {
                    packet.swap(20 + n, 22 + n);
                }
            }
            packet[10..12].fill(0);
            let mut sum: u32 = packet[..20]
                .chunks_exact(2)
                .map(|w| u32::from(u16::from_be_bytes([w[0], w[1]])))
                .sum();
            while sum > 0xffff {
                sum = (sum & 0xffff) + (sum >> 16);
            }
            packet[10..12].copy_from_slice(&(!(sum as u16)).to_be_bytes());
            Bytes::from(packet)
        }

        let server = tokio::net::UdpSocket::bind((Ipv4Addr::new(127, 0, 0, 2), 443))
            .await
            .unwrap();
        let (mut tunnel, mut inner_rx, managed_incoming) =
            ManagedTunnelRuntime::packet_mux_test_channels(8);
        let (mut io, raw_rx, incoming) = test_tun_io(8, 8);
        let (tun_sink, _watch) = watch::channel(Some(incoming));
        let (proxy_pipe, _proxy_client) = WakingPipe::bounded(4);
        let cancellation = CancellationToken::new();
        let policy = Arc::new(crate::application_traffic::ApplicationTrafficPolicy::default());
        let geo = Arc::new(GeoDirectPolicy::with_classifier(
            Arc::new(LocalGeo),
            [usque_geo::CountryCode::parse("JP").unwrap()],
        ));
        let (router, incoming) = DirectGatewayRouter::start(
            &Profile::default(),
            geo,
            Arc::new(Protector),
            Arc::new(crate::netstack::TrafficCounters::default()),
            None,
            &cancellation,
        )
        .await
        .unwrap();
        let task_cancel = cancellation.clone();
        let task_policy = policy.clone();
        let task = tokio::spawn(async move {
            run_packet_mux(
                &mut tunnel,
                proxy_pipe,
                raw_rx,
                DirectGatewayMux { router, incoming },
                tun_sink,
                &task_cancel,
                NetworkQualityTelemetry::default(),
                task_policy,
            )
            .await;
        });
        let mut original_peer = None;
        for blocked in [false, true, false, true] {
            policy.set_disable_quic(blocked);
            io.send_owned_packet(wire([127, 0, 0, 2], 50000, 443, false))
                .await
                .unwrap();
            let mut bytes = [0u8; 32];
            let (_, peer) = timeout(Duration::from_secs(2), server.recv_from(&mut bytes))
                .await
                .unwrap()
                .unwrap();
            assert_eq!(
                *original_peer.get_or_insert(peer),
                peer,
                "do not replace the GEO socket"
            );
            server.send_to(b"geo", peer).await.unwrap();
            let direct_reply = timeout(Duration::from_secs(2), io.receive_packet())
                .await
                .unwrap()
                .unwrap();
            assert_eq!(&direct_reply[28..], b"geo");
            assert!(
                inner_rx.try_recv().is_err(),
                "GEO packet must never fall into the tunnel"
            );

            io.send_owned_packet(wire([198, 51, 100, 1], 50001, 443, false))
                .await
                .unwrap();
            assert_eq!(
                timeout(Duration::from_millis(100), inner_rx.recv())
                    .await
                    .is_err(),
                blocked
            );
            let reply = wire([198, 51, 100, 1], 50001, 443, true);
            let length = reply.len();
            managed_incoming
                .send(PacketBatch::single(reply), length)
                .await
                .unwrap();
            assert_eq!(
                timeout(Duration::from_millis(100), io.receive_packet())
                    .await
                    .is_err(),
                blocked
            );

            io.send_owned_packet(wire([198, 51, 100, 1], 50002, 53, false))
                .await
                .unwrap();
            assert!(
                timeout(Duration::from_secs(1), inner_rx.recv())
                    .await
                    .unwrap()
                    .is_some()
            );
            assert!(!task.is_finished());
        }
        cancellation.cancel();
        timeout(Duration::from_secs(1), task)
            .await
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn packet_mux_survives_inner_backpressure_for_tun_and_proxy_sources() {
        let (mut tunnel, mut inner_rx, managed_incoming) =
            ManagedTunnelRuntime::packet_mux_test_channels(1);
        let (raw_tx, raw_rx) = tracked_bytes(QueueKind::TunToTransport, 4);
        let (_tun_incoming_tx, tun_incoming) = tracked_batches(QueueKind::TransportToTun, 1);
        let tun_io = MasqueTunIo {
            outgoing: raw_tx,
            incoming: tun_incoming,
            pending_incoming: PacketBatch::new(),
            cancellation: CancellationToken::new(),
            quality: NetworkQualityTelemetry::default(),
        };
        // A packet queued by a detached system TUN must not enter this mux
        // when a later attachment starts, even if the transport was saturated.
        let old_attachment = CancellationToken::new();
        old_attachment.cancel();
        let old_packet = mux_udp_packet(49_999);
        let length = old_packet.len();
        tun_io
            .outgoing
            .send(
                TunOutbound {
                    packet: old_packet.into(),
                    attachment: old_attachment,
                },
                length,
            )
            .await
            .unwrap();
        let (proxy_pipe, proxy_client) = WakingPipe::bounded(4);
        let WakingPipe {
            rx: _proxy_responses,
            tx: proxy_outgoing,
        } = proxy_client;
        let cancellation = CancellationToken::new();
        let (router, direct_incoming) = DirectGatewayRouter::start(
            &Profile::default(),
            Arc::new(GeoDirectPolicy::disabled()),
            noop_socket_protector(),
            Arc::new(crate::netstack::TrafficCounters::default()),
            None,
            &cancellation,
        )
        .await
        .unwrap();
        let direct_gateway = DirectGatewayMux {
            router,
            incoming: direct_incoming,
        };
        let (tun_sink, _tun_sink_rx) = watch::channel(None);
        let quality = NetworkQualityTelemetry::default();
        let task_cancellation = cancellation.clone();
        let task = tokio::spawn(async move {
            let _managed_incoming = managed_incoming;
            run_packet_mux(
                &mut tunnel,
                proxy_pipe,
                raw_rx,
                direct_gateway,
                tun_sink,
                &task_cancellation,
                quality,
                Arc::default(),
            )
            .await;
        });

        tun_io.send_packet(&mux_udp_packet(50_000)).await.unwrap();
        tun_io.send_packet(&mux_udp_packet(50_001)).await.unwrap();
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert!(!task.is_finished());
        let first = timeout(Duration::from_secs(1), inner_rx.recv())
            .await
            .unwrap()
            .unwrap();
        let second = timeout(Duration::from_secs(1), inner_rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(u16::from_be_bytes([first[20], first[21]]), 50_000);
        assert_eq!(u16::from_be_bytes([second[20], second[21]]), 50_001);

        proxy_outgoing.send_async(&mux_udp_packet(50_002)).await;
        let proxy = timeout(Duration::from_secs(1), inner_rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(u16::from_be_bytes([proxy[20], proxy[21]]), 50_002);
        assert!(!task.is_finished());

        cancellation.cancel();
        timeout(Duration::from_secs(1), task)
            .await
            .expect("packet mux did not stop after cancellation")
            .unwrap();
    }

    #[tokio::test]
    async fn tun_send_queue_saturation_backpressures_and_resumes_in_order() {
        let (io, mut outgoing_rx, _incoming_tx) = test_tun_io(1, 1);
        let packet = [
            0x45, 0, 0, 20, 0, 0, 0, 0, 64, 17, 0, 0, 1, 1, 1, 1, 8, 8, 8, 8,
        ];

        io.send_packet(&packet).await.unwrap();
        let second_send = io.send_packet(&packet);
        tokio::pin!(second_send);
        tokio::select! {
            result = &mut second_send => panic!("saturated send completed early: {result:?}"),
            () = tokio::time::sleep(Duration::from_millis(10)) => {}
        }

        assert_eq!(outgoing_rx.recv().await.unwrap().packet.as_ref(), packet);
        second_send.await.unwrap();
        assert_eq!(outgoing_rx.recv().await.unwrap().packet.as_ref(), packet);
    }

    #[tokio::test]
    async fn owned_tun_send_preserves_allocation_and_skips_borrowed_copy_metric() {
        let (io, mut outgoing_rx, _incoming_tx) = test_tun_io(2, 1);
        let quality = io.quality.clone();
        let packet = mux_udp_packet(50_000);
        let allocation = packet.as_ptr();

        io.send_owned_packet(packet).await.unwrap();
        let received = outgoing_rx.recv().await.unwrap().packet;

        assert_eq!(received.as_ptr(), allocation);
        assert_eq!(
            crate::network_quality::NetworkQualitySampler::new(quality)
                .sample()
                .allocations
                .borrowed_to_owned_copy_bytes,
            0
        );

        let borrowed = mux_udp_packet(50_001);
        io.send_packet(&borrowed).await.unwrap();
        let _ = outgoing_rx.recv().await.unwrap();
        assert_eq!(
            crate::network_quality::NetworkQualitySampler::new(io.quality.clone())
                .sample()
                .allocations
                .borrowed_to_owned_copy_bytes,
            borrowed.len() as u64
        );
    }

    #[tokio::test]
    async fn closing_tun_send_queue_releases_a_backpressured_sender() {
        let (io, mut outgoing_rx, _incoming_tx) = test_tun_io(1, 1);
        let packet = [
            0x45, 0, 0, 20, 0, 0, 0, 0, 64, 17, 0, 0, 1, 1, 1, 1, 8, 8, 8, 8,
        ];

        io.send_packet(&packet).await.unwrap();
        let blocked_send = io.send_packet(&packet);
        tokio::pin!(blocked_send);
        tokio::select! {
            result = &mut blocked_send => panic!("saturated send completed early: {result:?}"),
            () = tokio::time::sleep(Duration::from_millis(10)) => {}
        }

        outgoing_rx.close();
        assert!(matches!(
            blocked_send.await,
            Err(TransportError::TunnelClosed)
        ));
    }

    #[tokio::test]
    async fn mutable_slab_backpressure_cancellation_and_close_preserve_queue_capacity() {
        for ending in 0..3 {
            let (io, mut receiver, _incoming) = test_tun_io(1, 1);
            let mut slab = crate::android_tun_read_slab::TunReadSlab::new();
            let mut take = |port| {
                let original = mux_udp_packet(port);
                slab.prepare(1280).unwrap();
                slab.read_buffer()[..original.len()].copy_from_slice(&original);
                slab.take_packet(original.len()).unwrap()
            };
            let first = take(50000);
            let pointer = first.as_ptr() as usize;
            io.start_send_mut_packet(first).await.unwrap();
            let mut waiting = Box::pin(io.start_send_mut_packet(take(50001)));
            assert!(
                std::future::poll_fn(|cx| std::task::Poll::Ready(waiting.as_mut().poll(cx)))
                    .await
                    .is_pending()
            );
            match ending {
                0 => drop(waiting),
                1 => {
                    io.cancellation.cancel();
                    assert!(matches!(waiting.await, Err(TransportError::TunnelClosed)));
                }
                _ => {
                    receiver.close();
                    assert!(matches!(waiting.await, Err(TransportError::TunnelClosed)));
                }
            }
            let delivered = receiver.recv().await.unwrap().packet;
            assert_eq!(delivered.as_ptr() as usize, pointer);
            assert!(matches!(delivered, OutboundPacket::Mutable(_)));
            assert!(receiver.try_recv().is_err());
            if ending == 0 {
                let third = take(50002);
                let pointer = third.as_ptr() as usize;
                io.start_send_mut_packet(third).await.unwrap();
                assert_eq!(
                    receiver.recv().await.unwrap().packet.as_ptr() as usize,
                    pointer
                );
            }
        }
    }

    #[tokio::test]
    async fn cancellation_drops_a_backpressured_tun_send_without_enqueuing_it() {
        let (io, mut outgoing_rx, _incoming_tx) = test_tun_io(1, 1);
        let packet = [
            0x45, 0, 0, 20, 0, 0, 0, 0, 64, 17, 0, 0, 1, 1, 1, 1, 8, 8, 8, 8,
        ];
        io.send_packet(&packet).await.unwrap();
        let cancellation = CancellationToken::new();
        let blocked_send = async {
            tokio::select! {
                biased;
                _ = cancellation.cancelled() => None,
                result = io.send_packet(&packet) => Some(result),
            }
        };
        tokio::pin!(blocked_send);
        tokio::select! {
            result = &mut blocked_send => panic!("saturated send completed early: {result:?}"),
            () = tokio::time::sleep(Duration::from_millis(10)) => {}
        }

        cancellation.cancel();
        assert!(blocked_send.await.is_none());
        assert_eq!(outgoing_rx.recv().await.unwrap().packet.as_ref(), packet);
        assert!(outgoing_rx.try_recv().is_err());
    }

    #[test]
    fn tun_receive_queue_can_be_drained_without_waiting() {
        let (mut io, _outgoing_rx, incoming_tx) = test_tun_io(1, 2);

        incoming_tx
            .try_send(
                PacketBatch::single(Bytes::from_static(b"packet")),
                b"packet".len(),
            )
            .expect("queue packet");
        assert_eq!(
            io.try_receive_packet().expect("queued packet"),
            Some(Bytes::from_static(b"packet"))
        );
        assert_eq!(io.try_receive_packet().expect("empty queue"), None);

        drop(incoming_tx);
        assert!(matches!(
            io.try_receive_packet(),
            Err(TransportError::TunnelClosed)
        ));
    }

    fn free_loopback() -> SocketAddr {
        static USED_PORTS: OnceLock<Mutex<HashSet<u16>>> = OnceLock::new();
        loop {
            let bound = std::net::TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
                .expect("ephemeral loopback");
            let address = bound.local_addr().expect("local addr");
            if USED_PORTS
                .get_or_init(|| Mutex::new(HashSet::new()))
                .lock()
                .expect("test port set")
                .insert(address.port())
            {
                return address;
            }
        }
    }

    fn proxy_profile(socks5: SocketAddr, http: SocketAddr) -> Profile {
        let mut profile = Profile {
            frontends: FrontendSettings {
                tunnel: false,
                socks5: true,
                http: true,
            },
            ..Profile::default()
        };
        profile.proxy.socks5_listeners = vec![socks5];
        profile.proxy.http_listeners = vec![http];
        profile
    }

    async fn start_local(profile: &Profile) -> MasqueRuntime {
        let credentials = profile
            .proxy
            .listener_credentials()
            .expect("test listener credentials");
        let assigned_ipv4 = Ipv4Addr::new(172, 16, 0, 2);
        let assigned_ipv6 = Ipv6Addr::new(0x2606, 0x4700, 0, 0, 0, 0, 0, 2);
        let socks5_bound = profile
            .frontends
            .socks5
            .then(|| Socks5Frontend::prebind(profile).expect("bind SOCKS5"));
        let http_bound = profile
            .frontends
            .http
            .then(|| HttpProxyFrontend::prebind(profile).expect("bind HTTP"));
        let monitor = ManagedTunnelMonitor::stub();
        let quality = monitor.network_quality_telemetry();
        let cancellation = CancellationToken::new();
        let (stack, _pipe) = PacketStack::start_detached(
            profile,
            (assigned_ipv4, assigned_ipv6),
            &monitor,
            &cancellation,
            crate::socket::noop_socket_protector(),
            Arc::new(GeoDirectPolicy::disabled()),
        )
        .await
        .expect("local packet stack");
        let socks5 = socks5_bound.map(|bound| {
            Socks5Frontend::activate(profile, assigned_ipv4, assigned_ipv6, &stack, bound)
                .expect("test listener credentials")
        });
        let http = http_bound.map(|bound| {
            HttpProxyFrontend::activate(profile, assigned_ipv4, assigned_ipv6, &stack, bound)
                .expect("test listener credentials")
        });
        let socks5_spec = socks5.as_ref().map(|frontend| {
            FrontendSpec::socks5(frontend.listeners(), profile, credentials.clone())
        });
        let http_spec = http
            .as_ref()
            .map(|frontend| FrontendSpec::http(frontend.listeners(), profile, credentials));
        let listeners = socks5
            .iter()
            .flat_map(|frontend| frontend.listeners().iter().copied())
            .chain(
                http.iter()
                    .flat_map(|frontend| frontend.listeners().iter().copied()),
            )
            .collect();
        let (tun_sink, tun_sink_rx) = watch::channel(None);
        tokio::task::yield_now().await;
        MasqueRuntime {
            internal_network: crate::InternalNetwork::for_stack(
                profile,
                &stack,
                assigned_ipv4,
                assigned_ipv6,
            ),
            monitor,
            stack,
            socks5,
            socks5_spec,
            http,
            http_spec,
            listeners,
            raw_outgoing: None,
            tun_cancellation: cancellation.child_token(),
            tun_sink,
            _tun_sink_rx: tun_sink_rx,
            quality,
            cancellation,
            mux_task: None,
            assigned_ipv4,
            assigned_ipv6,
        }
    }

    async fn socks_no_auth_method(address: SocketAddr) -> u8 {
        let mut stream = tokio::net::TcpStream::connect(address)
            .await
            .expect("connect SOCKS5");
        stream.write_all(&[5, 1, 0]).await.expect("SOCKS greeting");
        let mut reply = [0u8; 2];
        stream.read_exact(&mut reply).await.expect("SOCKS method");
        reply[1]
    }

    async fn socks_userpass_status(address: SocketAddr, username: &[u8], password: &[u8]) -> u8 {
        let mut stream = tokio::net::TcpStream::connect(address)
            .await
            .expect("connect SOCKS5");
        stream.write_all(&[5, 1, 2]).await.expect("SOCKS greeting");
        let mut method = [0u8; 2];
        stream.read_exact(&mut method).await.expect("SOCKS method");
        assert_eq!(method, [5, 2]);
        let mut request = vec![1, username.len() as u8];
        request.extend_from_slice(username);
        request.push(password.len() as u8);
        request.extend_from_slice(password);
        stream.write_all(&request).await.expect("userpass");
        let mut status = [0u8; 2];
        stream
            .read_exact(&mut status)
            .await
            .expect("userpass status");
        status[1]
    }

    async fn http_status(address: SocketAddr, proxy_authorization: Option<&str>) -> u16 {
        let mut stream = tokio::net::TcpStream::connect(address)
            .await
            .expect("connect HTTP");
        let mut request = String::from("GET http://example.com/ HTTP/1.1\r\nHost: example.com\r\n");
        if let Some(value) = proxy_authorization {
            request.push_str("Proxy-Authorization: ");
            request.push_str(value);
            request.push_str("\r\n");
        }
        request.push_str("Connection: close\r\n\r\n");
        stream
            .write_all(request.as_bytes())
            .await
            .expect("HTTP request");
        let mut buf = [0u8; 128];
        let n = tokio::time::timeout(Duration::from_secs(2), stream.read(&mut buf))
            .await
            .expect("HTTP response deadline")
            .expect("HTTP response");
        let text = String::from_utf8_lossy(&buf[..n]);
        text.split_whitespace()
            .nth(1)
            .expect("HTTP status")
            .parse()
            .expect("status code")
    }
}
