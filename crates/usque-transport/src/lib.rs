//! Audited MASQUE transports and proxy-mode data plane.
//!
//! This crate intentionally contains no platform route, DNS, or TUN mutation.
//! Desktop proxy modes can therefore be exercised without changing the host's
//! network configuration.

mod connect_ip_control;
mod direct_gateway;
mod dns;
mod geo_direct;
mod h2;
mod h3;
mod http_proxy;
mod icmp;
mod masque_runtime;
mod netstack;
mod network_quality;
mod packet_batch;
mod packet_mux;
mod pin_refresh;
mod port_allocator;
mod proxy;
mod queue_metrics;
mod relay;
mod socket;
mod socks5;
mod split_dns;
mod telemetry;
mod tunnel;
mod udp_io;

#[cfg(any(test, feature = "fault-injection"))]
mod fault_injection;

pub use geo_direct::{GeoDirectClassifier, GeoDirectPolicy, GeoRoute};
pub use h2::{
    H2Driver, H2ReceiveHalf, H2SendHalf, H2Tunnel, MasqueTlsIdentity, TransportError, connect_h2,
};
pub use h3::{H3Driver, H3ReceiveHalf, H3SendHalf, H3Tunnel, connect_h3};
pub use http_proxy::HttpProxyRuntime;
pub use masque_runtime::{MasqueRuntime, MasqueTunIo};
pub use netstack::{
    ManagedTunnelMonitor, ManagedTunnelRuntime, ManagedTunnelSender, ProxyPerformanceSnapshot,
    RuntimeHealth, RuntimePath, TrafficSnapshot,
};
pub use network_quality::{
    AllocationQuality, CongestionQuality, ConnectionInstanceId, DirectDnsMode, DirectDnsPhase,
    DirectDnsQuality, DirectDnsReasonCode, H2FlowControlQuality, LossQuality, MetricAvailability,
    MetricValue, MigrationPhase, MigrationQuality, MigrationReasonCode, NetworkQualityLevel,
    NetworkQualitySampler, NetworkQualitySnapshot, NetworkQualityTelemetry, PmtuPhase, PmtuQuality,
    QueueQuality, RttQuality, UdpIoQuality, spawn_network_quality_sampler,
};
pub use pin_refresh::{EndpointPinRefresher, refresh_endpoint_pin_over_protected_socket};
pub use proxy::ProxyRuntime;
pub use queue_metrics::QueueKind;
pub use socket::{
    DirectEgressLease, DirectProtocol, NoopSocketProtector, SocketHandle, SocketProtector,
};
pub use socks5::Socks5Runtime;
pub use split_dns::resolve_physical_host;
pub use split_dns::{SPLIT_DNS_IPV4, SPLIT_DNS_IPV6};
pub use telemetry::{
    CONNECTION_TIMELINE_CAPACITY, ConnectionEvent, ConnectionEventType, ConnectionMetrics,
    ConnectionTelemetry, ConnectionTimelineSnapshot,
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
