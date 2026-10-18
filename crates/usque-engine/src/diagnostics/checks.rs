use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use tokio_util::sync::CancellationToken;
use usque_core::{
    ConnectionPhase, ConnectionSnapshot, DiagnosticCategory, DiagnosticCheckStatus,
    DiagnosticFinding, DiagnosticMode, DiagnosticObservation, DiagnosticObservationAvailability,
    DiagnosticObservationSource, FailureSeverity, FrontendKind, FrontendPhase, KillSwitchState,
    Transport, TransportFailure, TransportFailureCode, TransportStage,
};
use usque_ipc::agent_v1::{AutomaticRecoveryPhase, PlatformState};
use usque_transport::{ConnectionEventType, ConnectionTimelineSnapshot};

/// Linux observations are separate from Windows WFP/HANDLE receipts. Only
/// bounded classifications are copied into exported diagnostic findings.
#[derive(Clone)]
pub(crate) struct LinuxPlatformObservation {
    pub state: usque_ipc::agent_v1::AgentState,
    pub physical: Option<usque_ipc::agent_v1::PhysicalNetworkInfo>,
    pub captured_at: tokio::time::Instant,
}

#[derive(Clone)]
pub(crate) struct DiagnosticContext {
    pub connection: ConnectionSnapshot,
    pub configuration_valid: bool,
    pub secure_storage_available: bool,
    pub kill_switch_expected: bool,
    pub tunnel_dns_expected: bool,
    pub system_proxy_expected: bool,
    pub operating_system: String,
    pub timeline: ConnectionTimelineSnapshot,
    pub platform_state: Option<PlatformState>,
    pub linux_platform: Option<LinuxPlatformObservation>,
    pub quality: usque_transport::NetworkQualitySnapshot,
    pub direct_dns: usque_core::DirectDnsSettings,
    pub probes: Option<Arc<super::probes::DiagnosticProbeContext>>,
    pub captured_at: tokio::time::Instant,
}

impl DiagnosticContext {
    pub(super) fn annotate(&self, mut finding: DiagnosticFinding) -> DiagnosticFinding {
        use DiagnosticObservationAvailability as Availability;
        use DiagnosticObservationSource as Source;
        let source = match finding.check_id.as_str() {
            "engine.configuration" | "dns.direct_encrypted_configuration" => Source::Config,
            "engine.event_stream" => Source::Frontend,
            "transport.h3_path_validation_probe" | "dns.direct_encrypted_reachability" => {
                Source::ActiveProbe
            }
            "protection.kill_switch"
            | "protection.dns_path"
            | "protection.route_ownership"
            | "protection.recovery_journal"
            | "tunnel.routes" => {
                if self.platform_state.is_some() || self.linux_platform.is_some() {
                    Source::Platform
                } else {
                    Source::Runtime
                }
            }
            "frontend.system_proxy_state" if self.platform_state.is_some() => Source::Platform,
            "physical.network_present"
            | "physical.ipv4_route"
            | "physical.ipv6_route"
            | "physical.dns_available"
            | "physical.network_generation"
                if self
                    .linux_platform
                    .as_ref()
                    .is_some_and(|state| state.physical.is_some()) =>
            {
                Source::Platform
            }
            _ => Source::Runtime,
        };
        let availability = if finding.summary_key == "nq_finding_stale" {
            Availability::Stale
        } else if finding.status == DiagnosticCheckStatus::Skipped {
            if finding.dependency_reason.as_deref().is_some_and(|reason| {
                matches!(reason, "not_applicable" | "not_configured" | "h3_active")
            }) {
                Availability::NotApplicable
            } else {
                Availability::Unavailable
            }
        } else if source == Source::Runtime
            && !finding.check_id.starts_with("quality.")
            && finding.failure.is_none()
            && finding.check_id != "engine.control_channel"
            && finding.check_id != "tunnel.first_packet"
        {
            Availability::Inferred
        } else if finding.summary_key.ends_with("unknown")
            || finding.summary_key.ends_with("lease_only")
        {
            Availability::Unavailable
        } else {
            Availability::Observed
        };
        finding.observation = Some(DiagnosticObservation {
            source,
            availability,
            age_milliseconds: if source == Source::Platform
                && let Some(platform) = &self.linux_platform
            {
                platform
                    .captured_at
                    .elapsed()
                    .as_millis()
                    .min(u128::from(u64::MAX)) as u64
            } else if finding.check_id.starts_with("quality.") {
                self.quality
                    .sampled_at
                    .elapsed()
                    .as_millis()
                    .min(u128::from(u64::MAX)) as u64
            } else {
                self.captured_at
                    .elapsed()
                    .as_millis()
                    .min(u128::from(u64::MAX)) as u64
            },
            connection_instance_id: if matches!(
                source,
                Source::Runtime | Source::Platform | Source::ActiveProbe
            ) {
                self.quality.connection_id.map(|id| id.0)
            } else {
                None
            },
            network_generation: self
                .linux_platform
                .as_ref()
                .and_then(|state| state.physical.as_ref())
                .map(|physical| physical.generation),
        });
        finding.evidence = finding
            .sanitized_evidence
            .iter()
            .filter_map(|value| usque_core::DiagnosticEvidence::from_legacy(value))
            .take(16)
            .collect();
        finding
    }
}

#[async_trait]
pub(crate) trait DiagnosticCheck: Send + Sync {
    fn id(&self) -> &'static str;
    fn category(&self) -> DiagnosticCategory;
    fn dependencies(&self) -> &'static [&'static str];
    fn minimum_mode(&self) -> DiagnosticMode;
    fn resource_group(&self) -> &'static str;
    fn timeout(&self) -> Duration;

    async fn run(
        &self,
        context: &DiagnosticContext,
        cancellation: CancellationToken,
    ) -> DiagnosticFinding;
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum PassiveCheckKind {
    ControlChannel,
    EventStream,
    Capabilities,
    Configuration,
    SecureStorage,
    SocksPort,
    HttpPort,
    SystemProxy,
    PhysicalNetwork,
    Ipv4Route,
    Ipv6Route,
    PhysicalDns,
    NetworkGeneration,
    H3Connect,
    H3Datagram,
    H2Tcp,
    H2Tls,
    H2Connect,
    EndpointPin,
    FallbackPolicy,
    AddressAssignment,
    TunnelRoutes,
    TunnelDns,
    FirstPacket,
    Ipv4Egress,
    Ipv6Egress,
    KillSwitch,
    DnsPath,
    RouteOwnership,
    RecoveryJournal,
    QualityRtt,
    QualityLoss,
    QualityQueues,
    QualityPmtu,
    MigrationCapability,
    EncryptedDnsConfiguration,
    EncryptedDnsRuntime,
}

pub(crate) struct PassiveCheck {
    id: &'static str,
    category: DiagnosticCategory,
    dependencies: &'static [&'static str],
    minimum_mode: DiagnosticMode,
    resource_group: &'static str,
    kind: PassiveCheckKind,
}

impl PassiveCheck {
    pub(crate) const fn new(
        id: &'static str,
        category: DiagnosticCategory,
        dependencies: &'static [&'static str],
        minimum_mode: DiagnosticMode,
        resource_group: &'static str,
        kind: PassiveCheckKind,
    ) -> Self {
        Self {
            id,
            category,
            dependencies,
            minimum_mode,
            resource_group,
            kind,
        }
    }

    fn evaluate(&self, context: &DiagnosticContext) -> DiagnosticFinding {
        use PassiveCheckKind as Kind;

        if let Some(finding) = self.evaluate_linux(context) {
            return finding;
        }

        if matches!(
            self.kind,
            Kind::H3Connect
                | Kind::H3Datagram
                | Kind::H2Tcp
                | Kind::H2Tls
                | Kind::H2Connect
                | Kind::EndpointPin
        ) && !matches!(
            context.connection.phase,
            ConnectionPhase::Connected | ConnectionPhase::Degraded
        ) {
            if let Some(failure) = &context.connection.failure {
                let relevant = match self.kind {
                    Kind::H3Connect => context.connection.transport == Some(Transport::Http3),
                    Kind::H2Connect => context.connection.transport == Some(Transport::Http2),
                    Kind::EndpointPin => failure.code == TransportFailureCode::EndpointPinMismatch,
                    _ => false,
                };
                if relevant {
                    let summary = match self.kind {
                        Kind::EndpointPin => "diagnostic_endpoint_pin_mismatch",
                        Kind::H2Connect => "diagnostic_h2_not_tested",
                        _ => "diagnostic_h3_not_active",
                    };
                    let mut result = failed(self, failure.code, failure.stage, summary);
                    result.failure = Some(failure.clone());
                    return result;
                }
            }
            let summary = match self.kind {
                Kind::EndpointPin => "diagnostic_endpoint_pin_not_tested",
                Kind::H2Tcp | Kind::H2Tls | Kind::H2Connect => "diagnostic_h2_not_tested",
                Kind::H3Datagram => "diagnostic_h3_datagram_not_tested",
                _ => "diagnostic_h3_not_tested",
            };
            return skipped(self, summary, "no_active_runtime");
        }

        match self.kind {
            Kind::QualityRtt
            | Kind::QualityLoss
            | Kind::QualityQueues
            | Kind::QualityPmtu
            | Kind::MigrationCapability
            | Kind::EncryptedDnsConfiguration
            | Kind::EncryptedDnsRuntime => super::quality::evaluate(self, self.kind, context),
            Kind::ControlChannel => passed(self, "diagnostic_engine_control_ok", ["responsive"]),
            Kind::EventStream => skipped(
                self,
                "diagnostic_event_stream_unknown",
                "frontend_observation_required",
            ),
            Kind::Capabilities => passed(self, "diagnostic_capabilities_ok", ["append_only_api"]),
            Kind::Configuration if context.configuration_valid => {
                passed(self, "diagnostic_configuration_ok", ["schema_valid"])
            }
            Kind::Configuration => failed(
                self,
                TransportFailureCode::ConfigurationInvalid,
                TransportStage::TunnelStartup,
                "diagnostic_configuration_invalid",
            ),
            Kind::SecureStorage if context.secure_storage_available => passed(
                self,
                "diagnostic_secure_storage_available",
                ["metadata_only"],
            ),
            Kind::SecureStorage => skipped(
                self,
                "diagnostic_secure_storage_not_supported",
                "platform_capability_unavailable",
            ),
            Kind::SocksPort => frontend_check(self, context, FrontendKind::Socks5),
            Kind::HttpPort => frontend_check(self, context, FrontendKind::Http),
            Kind::SystemProxy => {
                if !context.system_proxy_expected {
                    skipped(self, "diagnostic_system_proxy_disabled", "not_configured")
                } else if context
                    .platform_state
                    .as_ref()
                    .is_some_and(|state| !state.system_proxy_lease)
                {
                    failed(
                        self,
                        TransportFailureCode::SystemProxyStateMismatch,
                        TransportStage::PlatformRecovery,
                        "diagnostic_system_proxy_lease_missing",
                    )
                } else if !context.connection.frontends.iter().any(|frontend| {
                    frontend.kind == FrontendKind::SystemProxy
                        && matches!(
                            frontend.phase,
                            FrontendPhase::Active | FrontendPhase::Degraded
                        )
                        && frontend.error.is_none()
                }) {
                    failed(
                        self,
                        TransportFailureCode::SystemProxyStateMismatch,
                        TransportStage::PlatformRecovery,
                        "diagnostic_system_proxy_runtime_mismatch",
                    )
                } else if context.platform_state.is_some() {
                    warning(
                        self,
                        TransportFailureCode::PlatformRecoveryPending,
                        TransportStage::PlatformRecovery,
                        "diagnostic_system_proxy_lease_only",
                        "inspect_platform_state",
                    )
                } else {
                    warning(
                        self,
                        TransportFailureCode::AgentUnreachable,
                        TransportStage::PlatformRecovery,
                        "diagnostic_system_proxy_actual_state_unknown",
                        "inspect_platform_state",
                    )
                }
            }
            Kind::PhysicalNetwork if connected_or_reconnecting(&context.connection) => finding(
                self,
                DiagnosticCheckStatus::Warning,
                FailureSeverity::Warning,
                None,
                FindingContent {
                    summary: "diagnostic_physical_network_present",
                    remediation: "none",
                    evidence: ["runtime_path"],
                },
            ),
            Kind::PhysicalNetwork => skipped(
                self,
                "diagnostic_physical_network_not_observed",
                "no_active_runtime",
            ),
            Kind::Ipv4Route => skipped(self, "diagnostic_ipv4_route_unknown", "no_active_runtime"),
            Kind::Ipv6Route => skipped(self, "diagnostic_ipv6_route_unknown", "no_active_runtime"),
            Kind::PhysicalDns => {
                if context.connection.failure.as_ref().is_some_and(|failure| {
                    matches!(
                        failure.code,
                        TransportFailureCode::PhysicalDnsUnavailable
                            | TransportFailureCode::DnsApplyFailed
                            | TransportFailureCode::DnsRestoreIncomplete
                    )
                }) {
                    failed(
                        self,
                        TransportFailureCode::PhysicalDnsUnavailable,
                        TransportStage::EndpointResolution,
                        "diagnostic_physical_dns_unavailable",
                    )
                } else {
                    skipped(self, "diagnostic_physical_dns_unknown", "no_active_runtime")
                }
            }
            Kind::NetworkGeneration if context.quality.connection_id.is_none() => skipped(
                self,
                "diagnostic_network_generation_unknown",
                "no_active_runtime",
            ),
            Kind::NetworkGeneration => passed(
                self,
                "diagnostic_network_generation_observed",
                [if context.timeline.metrics.network_change_count == 0 {
                    "stable"
                } else {
                    "changed"
                }],
            ),
            Kind::H3Connect
                if context.connection.data_plane == Some(usque_core::DataPlaneMode::L4Proxy)
                    && !context
                        .connection
                        .l4
                        .as_ref()
                        .is_some_and(|l4| l4.connect_verified) =>
            {
                skipped(self, "diagnostic_h3_not_tested", "no_application_traffic")
            }
            Kind::H3Connect if context.connection.transport == Some(Transport::Http3) => {
                passed(self, "diagnostic_h3_connected", ["active_path"])
            }
            Kind::H3Connect if context.connection.transport == Some(Transport::Http2) => warning(
                self,
                context
                    .timeline
                    .metrics
                    .last_failure_code
                    .unwrap_or(TransportFailureCode::H3UdpUnreachable),
                TransportStage::QuicHandshake,
                "diagnostic_h3_not_active",
                "http2_fallback_active",
            ),
            Kind::H3Connect => skipped(
                self,
                "diagnostic_h3_not_tested",
                "active_probe_requires_disconnected_deep_mode",
            ),
            Kind::H3Datagram
                if context.connection.data_plane == Some(usque_core::DataPlaneMode::L4Proxy) =>
            {
                skipped(self, "diagnostic_h3_datagram_not_tested", "not_applicable")
            }
            Kind::H3Datagram if context.connection.transport == Some(Transport::Http3) => {
                passed(self, "diagnostic_h3_datagram_available", ["active_path"])
            }
            Kind::H3Datagram => skipped(self, "diagnostic_h3_datagram_not_tested", "h3_not_active"),
            Kind::H2Tcp | Kind::H2Tls | Kind::H2Connect
                if context.connection.transport == Some(Transport::Http2) =>
            {
                passed(self, "diagnostic_h2_stage_ready", ["active_path"])
            }
            Kind::H2Tcp | Kind::H2Tls | Kind::H2Connect
                if context.connection.transport == Some(Transport::Http3) =>
            {
                skipped(self, "diagnostic_h2_not_required", "h3_active")
            }
            Kind::H2Tcp | Kind::H2Tls | Kind::H2Connect => skipped(
                self,
                "diagnostic_h2_not_tested",
                "active_probe_requires_disconnected_deep_mode",
            ),
            Kind::EndpointPin
                if context.connection.failure.as_ref().is_some_and(|failure| {
                    failure.code == TransportFailureCode::EndpointPinMismatch
                }) =>
            {
                failed(
                    self,
                    TransportFailureCode::EndpointPinMismatch,
                    TransportStage::TlsHandshake,
                    "diagnostic_endpoint_pin_mismatch",
                )
            }
            Kind::EndpointPin if connected_or_reconnecting(&context.connection) => passed(
                self,
                "diagnostic_endpoint_pin_valid",
                ["verified_before_ready"],
            ),
            Kind::EndpointPin => skipped(
                self,
                "diagnostic_endpoint_pin_not_tested",
                "no_transport_handshake",
            ),
            Kind::FallbackPolicy => {
                let invalid = (context.connection.data_plane
                    == Some(usque_core::DataPlaneMode::L4Proxy)
                    && context.connection.transport == Some(Transport::Http2))
                    || context.timeline.events.iter().any(|event| {
                        event.event_type == ConnectionEventType::FallbackStarted
                            && !fallback_event_permitted(event)
                    });
                if invalid {
                    failed(
                        self,
                        TransportFailureCode::Internal,
                        TransportStage::TunnelStartup,
                        "diagnostic_fallback_policy_violation",
                    )
                } else {
                    passed(self, "diagnostic_fallback_policy_valid", ["typed_matrix"])
                }
            }
            Kind::AddressAssignment
                if context.connection.data_plane == Some(usque_core::DataPlaneMode::L4Proxy) =>
            {
                skipped(
                    self,
                    "diagnostic_address_assignment_unknown",
                    "not_applicable",
                )
            }
            Kind::AddressAssignment
                if connected_or_reconnecting(&context.connection)
                    && (context.connection.ipv4_available || context.connection.ipv6_available) =>
            {
                passed(
                    self,
                    "diagnostic_address_assignment_valid",
                    ["family_flags"],
                )
            }
            Kind::AddressAssignment if connected_or_reconnecting(&context.connection) => failed(
                self,
                TransportFailureCode::AddressAssignmentInvalid,
                TransportStage::AddressAssignment,
                "diagnostic_address_assignment_missing",
            ),
            Kind::AddressAssignment => skipped(
                self,
                "diagnostic_address_assignment_unknown",
                "no_active_tunnel",
            ),
            Kind::TunnelRoutes
                if context.platform_state.as_ref().is_some_and(|state| {
                    state.active_tunnel_lease
                        && state.actual_route_count_known
                        && state.actual_route_count == state.expected_route_count
                }) =>
            {
                passed(
                    self,
                    "diagnostic_tunnel_routes_consistent",
                    ["agent_read_only_inspection"],
                )
            }
            Kind::TunnelRoutes => {
                skipped(self, "diagnostic_tunnel_routes_unknown", "no_active_tunnel")
            }
            Kind::TunnelDns
                if context.tunnel_dns_expected
                    && connected_or_reconnecting(&context.connection) =>
            {
                passed(
                    self,
                    "diagnostic_tunnel_dns_configured",
                    ["configuration_consistent_not_leak_test"],
                )
            }
            Kind::TunnelDns if context.tunnel_dns_expected => {
                skipped(self, "diagnostic_tunnel_dns_unknown", "no_active_tunnel")
            }
            Kind::TunnelDns => skipped(self, "diagnostic_tunnel_dns_disabled", "not_configured"),
            Kind::FirstPacket
                if context.connection.data_plane == Some(usque_core::DataPlaneMode::L4Proxy) =>
            {
                if context.connection.statistics.bytes_sent != 0
                    && context.connection.statistics.bytes_received != 0
                {
                    passed(self, "diagnostic_first_packet_observed", ["bidirectional"])
                } else {
                    skipped(
                        self,
                        "diagnostic_first_packet_unknown",
                        "no_application_traffic",
                    )
                }
            }
            Kind::FirstPacket
                if context.connection.statistics.bytes_sent > 0
                    && context.connection.statistics.bytes_received > 0 =>
            {
                passed(self, "diagnostic_first_packet_observed", ["bidirectional"])
            }
            Kind::FirstPacket if connected_or_reconnecting(&context.connection) => warning(
                self,
                TransportFailureCode::PacketReceiveStalled,
                TransportStage::PacketReceive,
                "diagnostic_first_packet_not_observed",
                "generate_tunnel_traffic",
            ),
            Kind::FirstPacket => {
                skipped(self, "diagnostic_first_packet_unknown", "no_active_tunnel")
            }
            Kind::Ipv4Egress if context.connection.ipv4_available => skipped(
                self,
                "diagnostic_ipv4_egress_requires_external_observer",
                "run_release_leak_gate",
            ),
            Kind::Ipv6Egress if context.connection.ipv6_available => skipped(
                self,
                "diagnostic_ipv6_egress_requires_external_observer",
                "run_release_leak_gate",
            ),
            Kind::Ipv4Egress | Kind::Ipv6Egress => skipped(
                self,
                "diagnostic_egress_family_unavailable",
                "payload_family_unavailable",
            ),
            Kind::KillSwitch if !context.kill_switch_expected => {
                skipped(self, "diagnostic_kill_switch_disabled", "not_configured")
            }
            Kind::KillSwitch if context.connection.kill_switch_state != KillSwitchState::Active => {
                failed(
                    self,
                    TransportFailureCode::KillSwitchStateMismatch,
                    TransportStage::KillSwitchApply,
                    "diagnostic_kill_switch_state_mismatch",
                )
            }
            Kind::KillSwitch
                if context.platform_state.as_ref().is_some_and(|state| {
                    state.actual_wfp_state != "unknown"
                        && state.actual_wfp_state == state.expected_wfp_state
                }) =>
            {
                passed(
                    self,
                    "diagnostic_kill_switch_state_consistent",
                    ["agent_read_only_inspection"],
                )
            }
            Kind::KillSwitch
                if context.platform_state.as_ref().is_some_and(|state| {
                    state.actual_wfp_state != "unknown"
                        && state.actual_wfp_state != state.expected_wfp_state
                }) =>
            {
                failed(
                    self,
                    TransportFailureCode::KillSwitchStateMismatch,
                    TransportStage::KillSwitchApply,
                    "diagnostic_kill_switch_state_mismatch",
                )
            }
            Kind::KillSwitch => warning(
                self,
                TransportFailureCode::PlatformRecoveryPending,
                TransportStage::KillSwitchApply,
                "diagnostic_kill_switch_actual_state_unknown",
                "inspect_platform_state",
            ),
            Kind::DnsPath
                if context.tunnel_dns_expected
                    && context.platform_state.as_ref().is_some_and(|state| {
                        state.actual_dns_state != "unknown"
                            && state.actual_dns_state == state.expected_dns_state
                    }) =>
            {
                passed(
                    self,
                    "diagnostic_dns_path_consistent",
                    ["agent_read_only_inspection"],
                )
            }
            Kind::DnsPath
                if context.tunnel_dns_expected
                    && context.platform_state.as_ref().is_some_and(|state| {
                        state.actual_dns_state != "unknown"
                            && state.actual_dns_state != state.expected_dns_state
                    }) =>
            {
                failed(
                    self,
                    TransportFailureCode::DnsRestoreIncomplete,
                    TransportStage::DnsApply,
                    "diagnostic_dns_path_mismatch",
                )
            }
            Kind::DnsPath if context.tunnel_dns_expected => warning(
                self,
                TransportFailureCode::PlatformRecoveryPending,
                TransportStage::DnsApply,
                "diagnostic_dns_path_actual_state_unknown",
                "inspect_platform_state",
            ),
            Kind::DnsPath => skipped(self, "diagnostic_dns_path_not_tunnel", "not_configured"),
            Kind::RouteOwnership
                if context.platform_state.as_ref().is_some_and(|state| {
                    state.actual_route_count_known
                        && state.actual_route_count == state.expected_route_count
                }) =>
            {
                passed(
                    self,
                    "diagnostic_route_ownership_consistent",
                    ["agent_read_only_inspection"],
                )
            }
            Kind::RouteOwnership
                if context.platform_state.as_ref().is_some_and(|state| {
                    state.actual_route_count_known
                        && state.actual_route_count != state.expected_route_count
                }) =>
            {
                failed(
                    self,
                    TransportFailureCode::RouteRestoreIncomplete,
                    TransportStage::RouteApply,
                    "diagnostic_route_ownership_mismatch",
                )
            }
            Kind::RouteOwnership
                if matches!(context.operating_system.as_str(), "windows" | "linux") =>
            {
                warning(
                    self,
                    TransportFailureCode::PlatformRecoveryPending,
                    TransportStage::RouteApply,
                    "diagnostic_route_ownership_actual_state_unknown",
                    "inspect_platform_state",
                )
            }
            Kind::RouteOwnership => skipped(
                self,
                "diagnostic_route_ownership_not_supported",
                "platform_capability_unavailable",
            ),
            Kind::RecoveryJournal
                if context
                    .platform_state
                    .as_ref()
                    .is_some_and(|state| state.pending_cleanup) =>
            {
                let mut finding = failed(
                    self,
                    TransportFailureCode::PlatformRecoveryPending,
                    TransportStage::PlatformRecovery,
                    "diagnostic_recovery_journal_pending_cleanup",
                );
                if let Some(state) = context.platform_state.as_ref() {
                    finding.sanitized_evidence = automatic_recovery_evidence(state);
                }
                finding
            }
            Kind::RecoveryJournal
                if context.platform_state.as_ref().is_some_and(|state| {
                    matches!(
                        usque_ipc::agent_v1::AgentPhase::try_from(state.agent_phase),
                        Ok(usque_ipc::agent_v1::AgentPhase::Clean
                            | usque_ipc::agent_v1::AgentPhase::Active)
                    ) && state
                        .recovery_diagnostics
                        .as_ref()
                        .and_then(|diagnostics| diagnostics.current.as_ref())
                        .is_none_or(|sample| {
                            matches!(
                                usque_ipc::agent_v1::RecoverySampleStatus::try_from(sample.status),
                                Ok(usque_ipc::agent_v1::RecoverySampleStatus::Complete
                                    | usque_ipc::agent_v1::RecoverySampleStatus::NoReceipt)
                            )
                        })
                }) =>
            {
                let mut finding = passed(
                    self,
                    "diagnostic_recovery_journal_consistent",
                    ["agent_read_only_inspection"],
                );
                finding
                    .sanitized_evidence
                    .extend(automatic_recovery_evidence(
                        context.platform_state.as_ref().unwrap(),
                    ));
                finding
            }
            Kind::RecoveryJournal
                if matches!(context.operating_system.as_str(), "windows" | "linux") =>
            {
                let mut finding = warning(
                    self,
                    TransportFailureCode::AgentUnreachable,
                    TransportStage::PlatformRecovery,
                    "diagnostic_recovery_journal_agent_unavailable",
                    "inspect_platform_state",
                );
                if let Some(state) = &context.platform_state {
                    finding
                        .sanitized_evidence
                        .extend(automatic_recovery_evidence(state));
                }
                finding
            }
            Kind::RecoveryJournal => skipped(
                self,
                "diagnostic_recovery_journal_not_supported",
                "platform_capability_unavailable",
            ),
        }
    }

    fn evaluate_linux(&self, context: &DiagnosticContext) -> Option<DiagnosticFinding> {
        use PassiveCheckKind as Kind;
        let observed = context.linux_platform.as_ref()?;
        let state = &observed.state;
        let platform = state.linux_platform.as_ref();
        let inspected = ["agent_read_only_inspection"];
        Some(match self.kind {
            Kind::TunnelRoutes if platform.is_some_and(|p| p.tun_known && !p.tun_present) => {
                skipped(self, "diagnostic_tunnel_routes_unknown", "no_active_tunnel")
            }
            Kind::PhysicalNetwork
                if observed
                    .physical
                    .as_ref()
                    .is_some_and(|p| !p.interfaces.is_empty()) =>
            {
                passed(self, "diagnostic_physical_network_present", inspected)
            }
            Kind::Ipv4Route | Kind::Ipv6Route if observed.physical.is_some() => {
                let ipv4 = matches!(self.kind, Kind::Ipv4Route);
                let present = observed
                    .physical
                    .as_ref()
                    .unwrap()
                    .interfaces
                    .iter()
                    .any(|i| i.address_family_mask & if ipv4 { 1 } else { 2 } != 0);
                if present {
                    passed(
                        self,
                        if ipv4 {
                            "diagnostic_ipv4_route_available"
                        } else {
                            "diagnostic_ipv6_route_available"
                        },
                        inspected,
                    )
                } else {
                    warning(
                        self,
                        if ipv4 {
                            TransportFailureCode::PhysicalIpv4Unavailable
                        } else {
                            TransportFailureCode::PhysicalIpv6Unavailable
                        },
                        TransportStage::EndpointResolution,
                        if ipv4 {
                            "diagnostic_ipv4_route_unavailable"
                        } else {
                            "diagnostic_ipv6_route_unavailable"
                        },
                        "inspect_platform_state",
                    )
                }
            }
            Kind::PhysicalDns if observed.physical.is_some() => {
                if observed
                    .physical
                    .as_ref()
                    .unwrap()
                    .interfaces
                    .iter()
                    .any(|i| !i.dns_servers.is_empty())
                {
                    passed(self, "diagnostic_physical_dns_available", inspected)
                } else {
                    failed(
                        self,
                        TransportFailureCode::PhysicalDnsUnavailable,
                        TransportStage::EndpointResolution,
                        "diagnostic_physical_dns_unavailable",
                    )
                }
            }
            Kind::NetworkGeneration if observed.physical.is_some() => {
                passed(self, "diagnostic_network_generation_observed", inspected)
            }
            Kind::KillSwitch if context.kill_switch_expected || state.kill_switch_active => {
                if state.kill_switch_active {
                    passed(self, "diagnostic_kill_switch_state_consistent", inspected)
                } else {
                    failed(
                        self,
                        TransportFailureCode::KillSwitchStateMismatch,
                        TransportStage::KillSwitchApply,
                        "diagnostic_kill_switch_state_mismatch",
                    )
                }
            }
            Kind::TunnelRoutes | Kind::RouteOwnership
                if platform.is_some_and(|p| p.routes_known) =>
            {
                if platform.unwrap().routes_consistent {
                    passed(
                        self,
                        if matches!(self.kind, Kind::TunnelRoutes) {
                            "diagnostic_tunnel_routes_consistent"
                        } else {
                            "diagnostic_route_ownership_consistent"
                        },
                        inspected,
                    )
                } else {
                    failed(
                        self,
                        TransportFailureCode::RouteRestoreIncomplete,
                        TransportStage::RouteApply,
                        "diagnostic_route_ownership_mismatch",
                    )
                }
            }
            Kind::DnsPath
                if context.tunnel_dns_expected && platform.is_some_and(|p| p.dns_known) =>
            {
                if platform.unwrap().dns_consistent {
                    passed(self, "diagnostic_dns_path_consistent", inspected)
                } else {
                    failed(
                        self,
                        TransportFailureCode::DnsRestoreIncomplete,
                        TransportStage::DnsApply,
                        "diagnostic_dns_path_mismatch",
                    )
                }
            }
            Kind::RecoveryJournal => {
                let mut finding = if matches!(
                    usque_ipc::agent_v1::AgentPhase::try_from(state.phase),
                    Ok(usque_ipc::agent_v1::AgentPhase::Clean
                        | usque_ipc::agent_v1::AgentPhase::Active)
                ) {
                    passed(self, "diagnostic_recovery_journal_consistent", inspected)
                } else {
                    failed(
                        self,
                        TransportFailureCode::PlatformRecoveryPending,
                        TransportStage::PlatformRecovery,
                        "diagnostic_recovery_journal_pending_cleanup",
                    )
                };
                finding
                    .sanitized_evidence
                    .extend(automatic_recovery_status_evidence(
                        state.automatic_recovery.as_ref(),
                    ));
                finding
            }
            _ => return None,
        })
    }
}

fn automatic_recovery_evidence(state: &PlatformState) -> Vec<String> {
    let mut evidence = Vec::new();
    if let Some(diagnostics) = &state.recovery_diagnostics {
        if let Some(sample) = &diagnostics.current {
            if let Ok(status) = usque_ipc::agent_v1::RecoverySampleStatus::try_from(sample.status) {
                evidence.push(format!("recovery_sample_status={}", status as i32));
            }
            evidence.push(format!(
                "recovery_sample_time_ms={}",
                sample.sampled_at_unix_ms
            ));
            evidence.push(format!(
                "recovery_sample_generation={}",
                sample.journal_generation
            ));
        }
        evidence.push(format!(
            "recovery_history_count={}",
            diagnostics.history.len().min(32)
        ));
    }
    evidence.extend(automatic_recovery_status_evidence(
        state.automatic_recovery.as_ref(),
    ));
    evidence
}

fn automatic_recovery_status_evidence(
    status: Option<&usque_ipc::agent_v1::AutomaticRecoveryStatus>,
) -> Vec<String> {
    let mut evidence = Vec::new();
    let Some(status) = status else {
        return evidence;
    };
    let Ok(phase) = AutomaticRecoveryPhase::try_from(status.phase) else {
        return evidence;
    };
    if phase == AutomaticRecoveryPhase::Unspecified
        || status.attempt_limit == 0
        || status.attempt_limit > 16
        || status.attempts_completed > status.attempt_limit
    {
        return evidence;
    }
    evidence.extend([
        format!("automatic_recovery_phase={}", phase as i32),
        format!(
            "automatic_recovery_attempts_completed={}",
            status.attempts_completed
        ),
        format!("automatic_recovery_attempt_limit={}", status.attempt_limit),
    ]);
    evidence
}

#[async_trait]
impl DiagnosticCheck for PassiveCheck {
    fn id(&self) -> &'static str {
        self.id
    }

    fn category(&self) -> DiagnosticCategory {
        self.category
    }

    fn dependencies(&self) -> &'static [&'static str] {
        self.dependencies
    }

    fn minimum_mode(&self) -> DiagnosticMode {
        self.minimum_mode
    }

    fn resource_group(&self) -> &'static str {
        self.resource_group
    }

    fn timeout(&self) -> Duration {
        Duration::from_secs(3)
    }

    async fn run(
        &self,
        context: &DiagnosticContext,
        cancellation: CancellationToken,
    ) -> DiagnosticFinding {
        if cancellation.is_cancelled() {
            return cancelled(self);
        }
        context.annotate(self.evaluate(context))
    }
}

pub(crate) fn pending_finding(check: &Arc<dyn DiagnosticCheck>) -> DiagnosticFinding {
    DiagnosticFinding::pending(check.id(), check.category())
}

pub(crate) fn cancelled_finding(check: &dyn DiagnosticCheck) -> DiagnosticFinding {
    cancelled(check)
}

pub(crate) fn timed_out_finding(check: &dyn DiagnosticCheck) -> DiagnosticFinding {
    failed(
        check,
        TransportFailureCode::DiagnosticTimeout,
        TransportStage::Diagnostics,
        "diagnostic_check_timed_out",
    )
}

pub(crate) fn internal_finding(check: &dyn DiagnosticCheck) -> DiagnosticFinding {
    failed(
        check,
        TransportFailureCode::Internal,
        TransportStage::Diagnostics,
        "diagnostic_check_failed_internally",
    )
}

pub(crate) fn dependency_skipped_finding(
    check: &dyn DiagnosticCheck,
    dependency: &str,
) -> DiagnosticFinding {
    let mut finding = skipped(check, "diagnostic_dependency_failed", "resolve_dependency");
    finding.failure = Some(TransportFailure::new(
        TransportFailureCode::DiagnosticDependencyFailed,
        TransportStage::Diagnostics,
    ));
    finding.dependency_reason = Some(dependency.to_owned());
    finding
}

pub(crate) fn mode_skipped_finding(check: &dyn DiagnosticCheck) -> DiagnosticFinding {
    skipped(
        check,
        "diagnostic_requires_deep_mode",
        "run_deep_diagnostics",
    )
}

fn fallback_event_permitted(event: &usque_transport::ConnectionEvent) -> bool {
    let Some(failure) = &event.failure else {
        return true;
    };
    // fallback_allowed describes H3 -> H2, not a preset's policy-checked
    // IPv6 -> IPv4 attempt. Keep the original failure metadata authoritative.
    let address_family_fallback = event.stage == Some(TransportStage::EndpointResolution)
        && event.transport.is_none()
        && event.address_family == Some(usque_core::AddressFamily::Ipv4)
        && failure.address_family == Some(usque_core::AddressFamily::Ipv6);
    if address_family_fallback {
        failure.retryable
            && failure.code != TransportFailureCode::PhysicalNetworkChanged
            && failure.action() != usque_core::FailureAction::Stop
            && failure.code != TransportFailureCode::EndpointPinMismatch
    } else {
        failure.fallback_allowed
    }
}

fn connected_or_reconnecting(snapshot: &ConnectionSnapshot) -> bool {
    matches!(
        snapshot.phase,
        ConnectionPhase::Connected | ConnectionPhase::Degraded | ConnectionPhase::Reconnecting
    )
}

fn frontend_check(
    check: &dyn DiagnosticCheck,
    context: &DiagnosticContext,
    kind: FrontendKind,
) -> DiagnosticFinding {
    let Some(frontend) = context
        .connection
        .frontends
        .iter()
        .find(|frontend| frontend.kind == kind)
    else {
        return skipped(
            check,
            "diagnostic_frontend_not_configured",
            "not_configured",
        );
    };
    if frontend.error.is_some() || frontend.phase == FrontendPhase::Error {
        failed(
            check,
            TransportFailureCode::ProxyPortInUse,
            TransportStage::TunnelStartup,
            "diagnostic_frontend_listener_failed",
        )
    } else if frontend.phase == FrontendPhase::Disabled {
        skipped(check, "diagnostic_frontend_disabled", "not_configured")
    } else {
        passed(
            check,
            "diagnostic_frontend_listener_ok",
            ["listener_active"],
        )
    }
}

fn passed<const N: usize>(
    check: &dyn DiagnosticCheck,
    summary: &str,
    evidence: [&str; N],
) -> DiagnosticFinding {
    finding(
        check,
        DiagnosticCheckStatus::Passed,
        FailureSeverity::Info,
        None,
        FindingContent {
            summary,
            remediation: "none",
            evidence,
        },
    )
}

fn warning(
    check: &dyn DiagnosticCheck,
    code: TransportFailureCode,
    stage: TransportStage,
    summary: &str,
    remediation: &str,
) -> DiagnosticFinding {
    finding(
        check,
        DiagnosticCheckStatus::Warning,
        FailureSeverity::Warning,
        Some(TransportFailure::new(code, stage)),
        FindingContent {
            summary,
            remediation,
            evidence: [],
        },
    )
}

fn failed(
    check: &dyn DiagnosticCheck,
    code: TransportFailureCode,
    stage: TransportStage,
    summary: &str,
) -> DiagnosticFinding {
    let failure = TransportFailure::new(code, stage);
    let severity = failure.severity;
    let remediation = failure.remediation_key.clone();
    finding(
        check,
        DiagnosticCheckStatus::Failed,
        severity,
        Some(failure),
        FindingContent {
            summary,
            remediation: &remediation,
            evidence: [],
        },
    )
}

fn skipped(check: &dyn DiagnosticCheck, summary: &str, reason: &str) -> DiagnosticFinding {
    let mut finding = finding(
        check,
        DiagnosticCheckStatus::Skipped,
        FailureSeverity::Info,
        None,
        FindingContent {
            summary,
            remediation: "none",
            evidence: [],
        },
    );
    finding.dependency_reason = Some(reason.to_owned());
    finding
}

fn cancelled(check: &dyn DiagnosticCheck) -> DiagnosticFinding {
    finding(
        check,
        DiagnosticCheckStatus::Cancelled,
        FailureSeverity::Warning,
        Some(TransportFailure::new(
            TransportFailureCode::DiagnosticCancelled,
            TransportStage::Diagnostics,
        )),
        FindingContent {
            summary: "diagnostic_cancelled",
            remediation: "none",
            evidence: [],
        },
    )
}

struct FindingContent<'a, const N: usize> {
    summary: &'a str,
    remediation: &'a str,
    evidence: [&'a str; N],
}

fn finding<const N: usize>(
    check: &dyn DiagnosticCheck,
    status: DiagnosticCheckStatus,
    severity: FailureSeverity,
    failure: Option<TransportFailure>,
    content: FindingContent<'_, N>,
) -> DiagnosticFinding {
    DiagnosticFinding {
        check_id: check.id().to_owned(),
        category: check.category(),
        status,
        failure,
        severity,
        summary_key: content.summary.to_owned(),
        remediation_key: content.remediation.to_owned(),
        sanitized_evidence: content
            .evidence
            .iter()
            .map(|value| (*value).to_owned())
            .collect(),
        started_at: Some(Utc::now()),
        duration_milliseconds: None,
        dependency_reason: None,
        observation: None,
        evidence: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use usque_core::{FrontendStatus, Statistics};

    use super::*;

    #[tokio::test]
    async fn preset_family_fallback_is_distinct_from_protocol_fallback() {
        for code in [
            TransportFailureCode::PhysicalIpv6Unavailable,
            TransportFailureCode::PhysicalDnsUnavailable,
            TransportFailureCode::H2TcpConnectFailed,
            TransportFailureCode::H2TlsFailed,
        ] {
            let failure = TransportFailure::new(code, TransportStage::EndpointResolution)
                .on_path(Transport::Http2, usque_core::AddressFamily::Ipv6);
            assert!(!failure.fallback_allowed);
            let telemetry = usque_transport::ConnectionTelemetry::default();
            telemetry.record(
                ConnectionEventType::FallbackStarted,
                Some(TransportStage::EndpointResolution),
                usque_transport::ConnectionEventPath::new(
                    None,
                    Some(usque_core::AddressFamily::Ipv4),
                ),
                None,
                Some(failure),
            );
            let mut context = context_with_unknown_platform_state();
            context.timeline = telemetry.snapshot();
            assert_eq!(
                check(PassiveCheckKind::FallbackPolicy)
                    .run(&context, CancellationToken::new())
                    .await
                    .status,
                DiagnosticCheckStatus::Passed,
                "{code:?} permits a different endpoint family",
            );
            context.timeline.events[0].stage = Some(TransportStage::SocketConnect);
            assert_eq!(
                check(PassiveCheckKind::FallbackPolicy)
                    .run(&context, CancellationToken::new())
                    .await
                    .status,
                DiagnosticCheckStatus::Failed,
                "{code:?} still forbids a protocol fallback",
            );
        }
    }

    #[tokio::test]
    async fn preset_family_fallback_never_hides_terminal_or_stale_failures() {
        for code in [
            TransportFailureCode::EndpointPinMismatch,
            TransportFailureCode::IdentityInvalid,
            TransportFailureCode::AuthenticationFailed,
            TransportFailureCode::SocketProtectionFailed,
            TransportFailureCode::PhysicalNetworkChanged,
        ] {
            let telemetry = usque_transport::ConnectionTelemetry::default();
            telemetry.record(
                ConnectionEventType::FallbackStarted,
                Some(TransportStage::EndpointResolution),
                usque_transport::ConnectionEventPath::new(
                    None,
                    Some(usque_core::AddressFamily::Ipv4),
                ),
                None,
                Some(
                    TransportFailure::new(code, TransportStage::EndpointResolution)
                        .on_path(Transport::Http3, usque_core::AddressFamily::Ipv6),
                ),
            );
            let mut context = context_with_unknown_platform_state();
            context.timeline = telemetry.snapshot();
            let finding = check(PassiveCheckKind::FallbackPolicy)
                .run(&context, CancellationToken::new())
                .await;
            assert_eq!(finding.status, DiagnosticCheckStatus::Failed, "{code:?}");
            assert_eq!(
                finding.failure.unwrap().code,
                TransportFailureCode::Internal
            );
        }
    }

    #[test]
    fn platform_and_probe_findings_keep_the_actual_runtime_identity() {
        let mut context = context_with_unknown_platform_state();
        let id = uuid::Uuid::new_v4();
        context.quality.connection_id = Some(usque_transport::ConnectionInstanceId(id));
        context.platform_state = Some(Default::default());
        for check_id in [
            "protection.kill_switch",
            "dns.direct_encrypted_reachability",
        ] {
            let finding = context.annotate(DiagnosticFinding::pending(
                check_id,
                DiagnosticCategory::Protection,
            ));
            assert_eq!(
                finding.observation.unwrap().connection_instance_id,
                Some(id)
            );
        }
        let finding = context.annotate(DiagnosticFinding::pending(
            "engine.configuration",
            DiagnosticCategory::LocalComponent,
        ));
        assert_eq!(finding.observation.unwrap().connection_instance_id, None);
    }

    #[tokio::test]
    async fn inferred_connectivity_never_passes_unobserved_dns_routes_or_event_delivery() {
        let mut context = context_with_unknown_platform_state();
        context.connection.ipv4_available = true;
        context.connection.ipv6_available = true;
        for kind in [
            PassiveCheckKind::EventStream,
            PassiveCheckKind::PhysicalDns,
            PassiveCheckKind::Ipv4Route,
            PassiveCheckKind::Ipv6Route,
            PassiveCheckKind::TunnelRoutes,
        ] {
            let finding = check(kind).run(&context, CancellationToken::new()).await;
            assert_eq!(finding.status, DiagnosticCheckStatus::Skipped);
            assert!(finding.failure.is_none());
            assert_eq!(
                finding.observation.unwrap().availability,
                DiagnosticObservationAvailability::Unavailable
            );
        }
        for kind in [PassiveCheckKind::Ipv4Egress, PassiveCheckKind::Ipv6Egress] {
            let finding = check(kind).run(&context, CancellationToken::new()).await;
            assert_eq!(finding.status, DiagnosticCheckStatus::Skipped);
            assert!(
                finding.failure.is_none(),
                "missing external observation is not an unavailable payload family"
            );
        }
    }

    #[tokio::test]
    async fn failed_or_reconnecting_transport_never_claims_a_current_ready_path() {
        let mut context = context_with_unknown_platform_state();
        context.connection.transport = Some(Transport::Http3);
        context.connection.failure = Some(TransportFailure::new(
            TransportFailureCode::PacketReceiveFailed,
            TransportStage::PacketReceive,
        ));
        for phase in [ConnectionPhase::Error, ConnectionPhase::Reconnecting] {
            context.connection.phase = phase;
            let finding = check(PassiveCheckKind::H3Connect)
                .run(&context, CancellationToken::new())
                .await;
            assert_eq!(finding.status, DiagnosticCheckStatus::Failed);
            assert_eq!(
                finding.failure.unwrap().code,
                TransportFailureCode::PacketReceiveFailed
            );
            assert_eq!(
                check(PassiveCheckKind::H3Datagram)
                    .run(&context, CancellationToken::new())
                    .await
                    .status,
                DiagnosticCheckStatus::Skipped
            );
        }
    }

    #[tokio::test]
    async fn non_ready_checks_keep_their_own_protocol_and_pin_explanations() {
        let mut context = context_with_unknown_platform_state();
        context.connection.phase = ConnectionPhase::Disconnected;
        let h2 = check(PassiveCheckKind::H2Connect)
            .run(&context, CancellationToken::new())
            .await;
        assert_eq!(h2.summary_key, "diagnostic_h2_not_tested");
        context.connection.phase = ConnectionPhase::Reconnecting;
        context.connection.transport = Some(Transport::Http2);
        context.connection.failure = Some(TransportFailure::new(
            TransportFailureCode::H2TcpConnectFailed,
            TransportStage::SocketConnect,
        ));
        let h2 = check(PassiveCheckKind::H2Connect)
            .run(&context, CancellationToken::new())
            .await;
        assert_eq!(h2.status, DiagnosticCheckStatus::Failed);
        assert_eq!(h2.summary_key, "diagnostic_h2_not_tested");
        assert_eq!(
            h2.failure.unwrap().code,
            TransportFailureCode::H2TcpConnectFailed
        );
        context.connection.phase = ConnectionPhase::Error;
        context.connection.failure = Some(TransportFailure::new(
            TransportFailureCode::EndpointPinMismatch,
            TransportStage::TlsHandshake,
        ));
        let pin = check(PassiveCheckKind::EndpointPin)
            .run(&context, CancellationToken::new())
            .await;
        assert_eq!(pin.summary_key, "diagnostic_endpoint_pin_mismatch");
        assert_eq!(
            pin.failure.unwrap().code,
            TransportFailureCode::EndpointPinMismatch
        );
    }

    #[tokio::test]
    async fn l4_does_not_claim_connect_ip_negotiation_or_unverified_connect_success() {
        let mut context = context_with_unknown_platform_state();
        context.connection.data_plane = Some(usque_core::DataPlaneMode::L4Proxy);
        context.connection.transport = Some(Transport::Http3);
        context.connection.l4 = Some(usque_core::L4Snapshot::default());
        for kind in [
            PassiveCheckKind::H3Connect,
            PassiveCheckKind::H3Datagram,
            PassiveCheckKind::AddressAssignment,
        ] {
            let result = check(kind).run(&context, CancellationToken::new()).await;
            assert_eq!(result.status, DiagnosticCheckStatus::Skipped);
        }
        context.connection.l4.as_mut().unwrap().connect_verified = true;
        assert_eq!(
            check(PassiveCheckKind::H3Connect)
                .run(&context, CancellationToken::new())
                .await
                .status,
            DiagnosticCheckStatus::Passed
        );
    }

    fn check(kind: PassiveCheckKind) -> PassiveCheck {
        PassiveCheck::new(
            "test.platform_state",
            DiagnosticCategory::Protection,
            &[],
            DiagnosticMode::Standard,
            "platform",
            kind,
        )
    }

    #[tokio::test]
    async fn linux_native_observations_keep_windows_receipts_empty_and_detect_mismatches() {
        let mut context = context_with_unknown_platform_state();
        context.operating_system = "linux".into();
        context.platform_state = None;
        context.connection.kill_switch_state = KillSwitchState::Error;
        context.linux_platform = Some(LinuxPlatformObservation {
            state: usque_ipc::agent_v1::AgentState {
                phase: usque_ipc::agent_v1::AgentPhase::Active as i32,
                kill_switch_active: true,
                linux_platform: Some(usque_ipc::agent_v1::LinuxPlatformState {
                    routes_known: true,
                    routes_consistent: true,
                    dns_known: true,
                    dns_consistent: true,
                    tun_known: true,
                    tun_present: true,
                }),
                ..Default::default()
            },
            physical: Some(usque_ipc::agent_v1::PhysicalNetworkInfo {
                generation: 17,
                interfaces: vec![usque_ipc::agent_v1::PhysicalInterface {
                    address_family_mask: 3,
                    dns_servers: vec!["192.0.2.53".into()],
                    ..Default::default()
                }],
            }),
            captured_at: tokio::time::Instant::now(),
        });
        for kind in [
            PassiveCheckKind::KillSwitch,
            PassiveCheckKind::RouteOwnership,
            PassiveCheckKind::DnsPath,
            PassiveCheckKind::RecoveryJournal,
            PassiveCheckKind::Ipv4Route,
            PassiveCheckKind::Ipv6Route,
            PassiveCheckKind::PhysicalDns,
        ] {
            let result = check(kind).run(&context, CancellationToken::new()).await;
            assert_eq!(result.status, DiagnosticCheckStatus::Passed, "{kind:?}");
            assert!(
                !serde_json::to_string(&result)
                    .unwrap()
                    .contains("192.0.2.53")
            );
        }
        let observed = context.linux_platform.as_mut().unwrap();
        observed.state.kill_switch_active = false;
        observed.state.phase = usque_ipc::agent_v1::AgentPhase::RecoveryRequired as i32;
        let native = observed.state.linux_platform.as_mut().unwrap();
        native.routes_consistent = false;
        native.dns_consistent = false;
        for kind in [
            PassiveCheckKind::KillSwitch,
            PassiveCheckKind::RouteOwnership,
            PassiveCheckKind::DnsPath,
            PassiveCheckKind::RecoveryJournal,
        ] {
            assert_eq!(
                check(kind)
                    .run(&context, CancellationToken::new())
                    .await
                    .status,
                DiagnosticCheckStatus::Failed,
                "{kind:?}"
            );
        }
        let finding = context.annotate(DiagnosticFinding::pending(
            "physical.network_generation",
            DiagnosticCategory::PhysicalNetwork,
        ));
        let observation = finding.observation.unwrap();
        assert_eq!(observation.source, DiagnosticObservationSource::Platform);
        assert_eq!(observation.network_generation, Some(17));
        assert!(context.platform_state.is_none());
    }

    fn context_with_unknown_platform_state() -> DiagnosticContext {
        let mut connection = ConnectionSnapshot {
            phase: ConnectionPhase::Connected,
            kill_switch_state: KillSwitchState::Active,
            statistics: Statistics::default(),
            ..ConnectionSnapshot::default()
        };
        connection.frontends.push(FrontendStatus {
            kind: FrontendKind::SystemProxy,
            phase: FrontendPhase::Active,
            listeners: Vec::new(),
            error: None,
        });
        DiagnosticContext {
            connection,
            configuration_valid: true,
            secure_storage_available: true,
            kill_switch_expected: true,
            tunnel_dns_expected: true,
            system_proxy_expected: true,
            operating_system: "windows".to_owned(),
            timeline: ConnectionTimelineSnapshot::default(),
            platform_state: Some(PlatformState {
                expected_dns_state: "configured".to_owned(),
                actual_dns_state: "unknown".to_owned(),
                expected_wfp_state: "active".to_owned(),
                actual_wfp_state: "unknown".to_owned(),
                system_proxy_lease: true,
                ..PlatformState::default()
            }),
            linux_platform: None,
            quality: crate::network_quality::disconnected_snapshot(),
            direct_dns: usque_core::DirectDnsSettings::default(),
            probes: None,
            captured_at: tokio::time::Instant::now(),
        }
    }

    #[test]
    fn internal_state_never_claims_platform_protection_was_observed() {
        let context = context_with_unknown_platform_state();
        for kind in [
            PassiveCheckKind::SystemProxy,
            PassiveCheckKind::KillSwitch,
            PassiveCheckKind::DnsPath,
            PassiveCheckKind::RouteOwnership,
        ] {
            let finding = check(kind).evaluate(&context);
            assert_eq!(
                finding.status,
                DiagnosticCheckStatus::Warning,
                "{kind:?} must remain unknown without an actual OS observation"
            );
            assert_eq!(
                finding.failure.as_ref().map(|failure| failure.code),
                Some(TransportFailureCode::PlatformRecoveryPending)
            );
        }
    }

    #[test]
    fn recovery_diagnostics_expose_only_bounded_automatic_status_numbers() {
        let mut context = context_with_unknown_platform_state();
        let platform = context.platform_state.as_mut().unwrap();
        platform.pending_cleanup = true;
        platform.automatic_recovery = Some(usque_ipc::agent_v1::AutomaticRecoveryStatus {
            phase: AutomaticRecoveryPhase::Exhausted as i32,
            attempts_completed: 3,
            attempt_limit: 3,
            terminal_error: Some(usque_ipc::agent_v1::AgentError {
                code: "AGENT_AUTOMATIC_RECOVERY_EXHAUSTED".to_owned(),
                message: "must not be copied into evidence".to_owned(),
                retryable: true,
            }),
        });

        let finding = check(PassiveCheckKind::RecoveryJournal).evaluate(&context);
        assert_eq!(
            finding.sanitized_evidence,
            [
                "automatic_recovery_phase=4",
                "automatic_recovery_attempts_completed=3",
                "automatic_recovery_attempt_limit=3",
            ]
        );
        assert!(!format!("{finding:?}").contains("must not be copied"));
    }
}
