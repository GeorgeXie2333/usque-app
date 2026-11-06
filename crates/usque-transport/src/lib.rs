//! Audited MASQUE transports and proxy-mode data plane.
//!
//! This crate intentionally contains no platform route, DNS, or TUN mutation.
//! Desktop proxy modes can therefore be exercised without changing the host's
//! network configuration.

mod application_traffic;
mod chain_mss;
mod chain_raw;
mod chain_session;
mod chain_udp;
#[cfg(test)]
mod chrome_fingerprint_tests;
mod connect_ip_control;
mod data_plane;
mod diagnostic_probe;
mod direct_diagnostics;
mod direct_gateway;
mod dns;
mod dns_stream;
mod encrypted_dns;
mod endpoint_preset;
mod endpoint_race;
mod feature_flags;
mod final_dns;
mod geo_direct;
mod h2;
mod h3;
mod h3_buffer;
mod http_proxy;
mod icmp;
mod internal_network;
mod l4;
mod live_ads;
mod live_routing;
mod masque_runtime;
mod migration_barrier;
mod netstack;
mod network_quality;
mod outbound_packet;
mod tls_fingerprint;
#[cfg(feature = "wireguard")]
pub mod warp_wireguard;
#[cfg(feature = "wireguard")]
mod wireguard;
// Compile the actual Android slab producer in memory-only transport tests.
#[cfg(test)]
#[path = "../../usque-android/src/tun_read_slab.rs"]
mod android_tun_read_slab;
mod packet_batch;
mod packet_mux;
mod packet_pipe;
mod path_socket;
mod pin_refresh;
mod pmtu;
mod port_allocator;
mod proxy;
mod proxy_exit;
mod proxy_udp;
mod queue_metrics;
mod recovery_policy;
mod relay;
mod routing_reject;
mod socket;
mod socks5;
mod split_dns;
mod stack_tcp;
mod stream_client;
mod system_proxy;
mod tcp;
mod tcp_candidates;
mod tcp_packet_budget;
mod telemetry;
mod transport_performance;
mod tunnel;
pub use system_proxy::{
    SystemFlowAuthorizer, SystemFlowIntent, SystemFlowProtocol, SystemProxyRuntime,
};
pub use transport_performance::{
    H2ReceivePerformance, H3SendPerformance, QueueBackpressureSnapshot,
    TransportPerformanceSnapshot,
};
mod udp_io;
mod udp_options;
mod vpngate;

#[cfg(any(test, feature = "fault-injection"))]
mod fault_injection;

#[cfg(all(feature = "fault-injection", not(debug_assertions), not(test)))]
compile_error!("fault-injection is restricted to test/debug lab builds");

pub use data_plane::{
    ChainExitStart, DataPlaneRuntime, PreparedTrafficPolicyUpdate, TunPacketIo, VpnGateStart,
};
pub use diagnostic_probe::{
    NetworkProbeResult, PROBE_IO_TIMEOUT as NETWORK_PROBE_IO_TIMEOUT, ResolvedH3ProbeEndpoints,
    h3_probe_endpoints, h3_probe_endpoints_resolved, probe_encrypted_dns, probe_h3_handshake,
    probe_h3_handshake_candidates,
};
pub use direct_diagnostics::DirectGatewaySnapshot;
pub use encrypted_dns::{DirectDnsError, DirectDnsQueryContext, DirectDnsResolver};
pub use endpoint_race::excludes_dns_server as excludes_automatic_endpoint_dns_server;
pub use feature_flags::{
    ENCRYPTED_DIRECT_DNS_ENABLED, NetworkFeatureFlags, PRODUCTION_NETWORK_FEATURES,
};
pub use geo_direct::{
    GeoDirectClassifier, GeoDirectPolicy, GeoRoute, PreparedRoutingUpdate, RoutingUpdateError,
};
pub use h2::{
    H2Driver, H2ReceiveHalf, H2SendHalf, H2Tunnel, MasqueTlsIdentity, TransportError, connect_h2,
};
pub use h3::{
    H3Driver, H3MigrationHandle, H3MigrationResult, H3ReceiveHalf, H3SendHalf, H3Tunnel, connect_h3,
};
pub use http_proxy::HttpProxyRuntime;
pub use internal_network::InternalNetwork;
#[cfg(feature = "wireguard")]
pub mod proton_vpn;
pub use l4::performance::{TunWriteObserver, TunWriteSample};
pub use masque_runtime::{MasqueRuntime, MasqueTunIo};
pub use netstack::{
    ManagedTunnelMonitor, ManagedTunnelRuntime, ManagedTunnelSender, ProxyPerformanceSnapshot,
    RuntimeHealth, RuntimePath, TrafficSnapshot,
};
pub use network_quality::{
    AllocationQuality, CongestionQuality, ConnectionInstanceId, DirectDnsMode, DirectDnsPhase,
    DirectDnsQuality, DirectDnsReasonCode, H2FlowControlQuality, LossQuality, MetricAvailability,
    MetricValue, MigrationPhase, MigrationQuality, MigrationReasonCode, NetworkQualityLevel,
    NetworkQualitySample, NetworkQualitySampler, NetworkQualitySnapshot, NetworkQualityTelemetry,
    PmtuPhase, PmtuQuality, QueueQuality, RttQuality, SocketReceiveQuality, UdpIoQuality,
    spawn_network_quality_sampler,
};
pub use pin_refresh::{EndpointPinRefresher, refresh_endpoint_pin_over_protected_socket};
pub use proxy::ProxyRuntime;
pub use queue_metrics::QueueKind;
pub use socket::{
    DirectEgressLease, DirectProtocol, NoopSocketProtector, PhysicalNetworkAvailability,
    PhysicalNetworkSnapshot, STALE_GENERATION_REASON, SocketHandle, SocketProtector,
};
pub use socks5::Socks5Runtime;
pub use split_dns::{SPLIT_DNS_IPV4, SPLIT_DNS_IPV6};
pub use split_dns::{resolve_endpoint_preset_aaaa, resolve_physical_host};
pub use telemetry::{
    CONNECTION_TIMELINE_CAPACITY, ConnectionEvent, ConnectionEventPath, ConnectionEventType,
    ConnectionMetrics, ConnectionTelemetry, ConnectionTimelineSnapshot,
};
pub use udp_io::{
    PooledUdpBuffer, ReceivedDatagram, RecvBatch, SendDatagram, UDP_RECEIVE_SLOT_SIZE,
    UdpBatchFallbackReason, UdpBatchIo, UdpBatchMode,
};
pub use usque_protocol::PeerNetworkState;

#[cfg(any(test, feature = "fault-injection"))]
pub use fault_injection::{
    ConnectorFactory, EndpointResolver, FaultHarness, FaultKind, FaultScript,
    NetworkGenerationSource, ScheduledFault, SocketFactory, TransportClock,
};
