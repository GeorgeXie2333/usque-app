use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use tokio_util::sync::CancellationToken;
use usque_core::{
    ConnectionPhase, ConnectionSnapshot, DiagnosticCategory, DiagnosticCheckStatus,
    DiagnosticFinding, DiagnosticMode, FailureSeverity, FrontendKind, FrontendPhase,
    KillSwitchState, Transport, TransportFailure, TransportFailureCode, TransportStage,
};
use usque_ipc::agent_v1::PlatformState;
use usque_transport::{ConnectionEventType, ConnectionTimelineSnapshot};

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
    pub quality: usque_transport::NetworkQualitySnapshot,
    pub direct_dns: usque_core::DirectDnsSettings,
    pub probes: Option<Arc<super::probes::DiagnosticProbeContext>>,
    pub captured_at: tokio::time::Instant,
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

        match self.kind {
            Kind::QualityRtt
            | Kind::QualityLoss
            | Kind::QualityQueues
            | Kind::QualityPmtu
            | Kind::MigrationCapability
            | Kind::EncryptedDnsConfiguration
            | Kind::EncryptedDnsRuntime => super::quality::evaluate(self, self.kind, context),
            Kind::ControlChannel => passed(self, "diagnostic_engine_control_ok", ["responsive"]),
            Kind::EventStream => passed(self, "diagnostic_event_stream_ok", ["recoverable"]),
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
            Kind::PhysicalNetwork if connected_or_reconnecting(&context.connection) => passed(
                self,
                "diagnostic_physical_network_present",
                ["runtime_path"],
            ),
            Kind::PhysicalNetwork => warning(
                self,
                TransportFailureCode::PhysicalNetworkChanged,
                TransportStage::EndpointResolution,
                "diagnostic_physical_network_not_observed",
                "connect_or_run_deep_diagnostics",
            ),
            Kind::Ipv4Route if context.connection.ipv4_available => {
                passed(self, "diagnostic_ipv4_route_available", ["payload_family"])
            }
            Kind::Ipv4Route if connected_or_reconnecting(&context.connection) => warning(
                self,
                TransportFailureCode::PhysicalIpv4Unavailable,
                TransportStage::EndpointResolution,
                "diagnostic_ipv4_route_unavailable",
                "check_physical_network",
            ),
            Kind::Ipv4Route => skipped(self, "diagnostic_ipv4_route_unknown", "no_active_runtime"),
            Kind::Ipv6Route if context.connection.ipv6_available => {
                passed(self, "diagnostic_ipv6_route_available", ["payload_family"])
            }
            Kind::Ipv6Route if connected_or_reconnecting(&context.connection) => warning(
                self,
                TransportFailureCode::PhysicalIpv6Unavailable,
                TransportStage::EndpointResolution,
                "diagnostic_ipv6_route_unavailable",
                "check_physical_network",
            ),
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
                } else if connected_or_reconnecting(&context.connection) {
                    passed(
                        self,
                        "diagnostic_physical_dns_available",
                        ["runtime_started"],
                    )
                } else {
                    skipped(self, "diagnostic_physical_dns_unknown", "no_active_runtime")
                }
            }
            Kind::NetworkGeneration => passed(
                self,
                "diagnostic_network_generation_observed",
                [if context.timeline.metrics.network_change_count == 0 {
                    "stable"
                } else {
                    "changed"
                }],
            ),
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
                let invalid = context.timeline.events.iter().any(|event| {
                    event.event_type == ConnectionEventType::FallbackStarted
                        && event
                            .failure
                            .as_ref()
                            .is_some_and(|failure| !failure.fallback_allowed)
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
            Kind::TunnelRoutes if connected_or_reconnecting(&context.connection) => passed(
                self,
                "diagnostic_tunnel_routes_consistent",
                ["family_flags"],
            ),
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
            Kind::Ipv4Egress if context.connection.ipv4_available => warning(
                self,
                TransportFailureCode::PhysicalIpv4Unavailable,
                TransportStage::PacketReceive,
                "diagnostic_ipv4_egress_requires_external_observer",
                "run_release_leak_gate",
            ),
            Kind::Ipv6Egress if context.connection.ipv6_available => warning(
                self,
                TransportFailureCode::PhysicalIpv6Unavailable,
                TransportStage::PacketReceive,
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
            Kind::RouteOwnership if context.operating_system == "windows" => warning(
                self,
                TransportFailureCode::PlatformRecoveryPending,
                TransportStage::RouteApply,
                "diagnostic_route_ownership_actual_state_unknown",
                "inspect_platform_state",
            ),
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
                failed(
                    self,
                    TransportFailureCode::PlatformRecoveryPending,
                    TransportStage::PlatformRecovery,
                    "diagnostic_recovery_journal_pending_cleanup",
                )
            }
            Kind::RecoveryJournal if context.platform_state.is_some() => passed(
                self,
                "diagnostic_recovery_journal_consistent",
                ["agent_read_only_inspection"],
            ),
            Kind::RecoveryJournal if context.operating_system == "windows" => warning(
                self,
                TransportFailureCode::AgentUnreachable,
                TransportStage::PlatformRecovery,
                "diagnostic_recovery_journal_agent_unavailable",
                "inspect_platform_state",
            ),
            Kind::RecoveryJournal => skipped(
                self,
                "diagnostic_recovery_journal_not_supported",
                "platform_capability_unavailable",
            ),
        }
    }
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
        self.evaluate(context)
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
    }
}

#[cfg(test)]
mod tests {
    use usque_core::{FrontendStatus, Statistics};

    use super::*;

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
}
