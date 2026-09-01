use serde::{Deserialize, Serialize};

use crate::state::{AddressFamily, ErrorCode, Transport};

/// A stable, export-safe catalogue of failures that may affect connectivity.
///
/// Variant names are part of the public diagnostics contract. Add new values;
/// do not rename or reuse an existing value for a different condition.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TransportFailureCode {
    EngineUnavailable,
    AgentUnreachable,
    VpnServiceUnavailable,
    ProxyPortInUse,
    PhysicalIpv4Unavailable,
    PhysicalIpv6Unavailable,
    PhysicalDnsUnavailable,
    PhysicalNetworkChanged,
    H3UdpUnreachable,
    H3HandshakeTimeout,
    H3ProtocolError,
    H3DatagramUnavailable,
    H3ConnectionClosed,
    PmtuRevalidationExhausted,
    H2TcpConnectFailed,
    H2TlsFailed,
    H2StreamClosed,
    H2ConnectRejected,
    H2GoAway,
    AllTransportsFailed,
    EndpointPinMismatch,
    IdentityInvalid,
    AuthenticationFailed,
    ConfigurationInvalid,
    ConnectIpRejected,
    AddressAssignmentInvalid,
    TunAddressMissing,
    SocketProtectionFailed,
    SocketAffinityInvalid,
    DnsApplyFailed,
    RouteApplyFailed,
    KillSwitchApplyFailed,
    KillSwitchStateMismatch,
    SystemProxyStateMismatch,
    RouteRestoreIncomplete,
    DnsRestoreIncomplete,
    SystemProxyStale,
    PlatformRecoveryPending,
    PacketSendFailed,
    PacketSendTimeout,
    PacketReceiveFailed,
    PacketReceiveStalled,
    SendQueueFull,
    DiagnosticAlreadyRunning,
    DiagnosticTimeout,
    DiagnosticCancelled,
    DiagnosticDependencyFailed,
    Internal,
}

impl TransportFailureCode {
    pub const ALL: [Self; 48] = [
        Self::EngineUnavailable,
        Self::AgentUnreachable,
        Self::VpnServiceUnavailable,
        Self::ProxyPortInUse,
        Self::PhysicalIpv4Unavailable,
        Self::PhysicalIpv6Unavailable,
        Self::PhysicalDnsUnavailable,
        Self::PhysicalNetworkChanged,
        Self::H3UdpUnreachable,
        Self::H3HandshakeTimeout,
        Self::H3ProtocolError,
        Self::H3DatagramUnavailable,
        Self::H3ConnectionClosed,
        Self::PmtuRevalidationExhausted,
        Self::H2TcpConnectFailed,
        Self::H2TlsFailed,
        Self::H2StreamClosed,
        Self::H2ConnectRejected,
        Self::H2GoAway,
        Self::AllTransportsFailed,
        Self::EndpointPinMismatch,
        Self::IdentityInvalid,
        Self::AuthenticationFailed,
        Self::ConfigurationInvalid,
        Self::ConnectIpRejected,
        Self::AddressAssignmentInvalid,
        Self::TunAddressMissing,
        Self::SocketProtectionFailed,
        Self::SocketAffinityInvalid,
        Self::DnsApplyFailed,
        Self::RouteApplyFailed,
        Self::KillSwitchApplyFailed,
        Self::KillSwitchStateMismatch,
        Self::SystemProxyStateMismatch,
        Self::RouteRestoreIncomplete,
        Self::DnsRestoreIncomplete,
        Self::SystemProxyStale,
        Self::PlatformRecoveryPending,
        Self::PacketSendFailed,
        Self::PacketSendTimeout,
        Self::PacketReceiveFailed,
        Self::PacketReceiveStalled,
        Self::SendQueueFull,
        Self::DiagnosticAlreadyRunning,
        Self::DiagnosticTimeout,
        Self::DiagnosticCancelled,
        Self::DiagnosticDependencyFailed,
        Self::Internal,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::EngineUnavailable => "ENGINE_UNAVAILABLE",
            Self::AgentUnreachable => "AGENT_UNREACHABLE",
            Self::VpnServiceUnavailable => "VPN_SERVICE_UNAVAILABLE",
            Self::ProxyPortInUse => "PROXY_PORT_IN_USE",
            Self::PhysicalIpv4Unavailable => "PHYSICAL_IPV4_UNAVAILABLE",
            Self::PhysicalIpv6Unavailable => "PHYSICAL_IPV6_UNAVAILABLE",
            Self::PhysicalDnsUnavailable => "PHYSICAL_DNS_UNAVAILABLE",
            Self::PhysicalNetworkChanged => "PHYSICAL_NETWORK_CHANGED",
            Self::H3UdpUnreachable => "H3_UDP_UNREACHABLE",
            Self::H3HandshakeTimeout => "H3_HANDSHAKE_TIMEOUT",
            Self::H3ProtocolError => "H3_PROTOCOL_ERROR",
            Self::H3DatagramUnavailable => "H3_DATAGRAM_UNAVAILABLE",
            Self::H3ConnectionClosed => "H3_CONNECTION_CLOSED",
            Self::PmtuRevalidationExhausted => "PMTU_REVALIDATION_EXHAUSTED",
            Self::H2TcpConnectFailed => "H2_TCP_CONNECT_FAILED",
            Self::H2TlsFailed => "H2_TLS_FAILED",
            Self::H2StreamClosed => "H2_STREAM_CLOSED",
            Self::H2ConnectRejected => "H2_CONNECT_REJECTED",
            Self::H2GoAway => "H2_GOAWAY",
            Self::AllTransportsFailed => "ALL_TRANSPORTS_FAILED",
            Self::EndpointPinMismatch => "ENDPOINT_PIN_MISMATCH",
            Self::IdentityInvalid => "IDENTITY_INVALID",
            Self::AuthenticationFailed => "AUTHENTICATION_FAILED",
            Self::ConfigurationInvalid => "CONFIGURATION_INVALID",
            Self::ConnectIpRejected => "CONNECT_IP_REJECTED",
            Self::AddressAssignmentInvalid => "ADDRESS_ASSIGNMENT_INVALID",
            Self::TunAddressMissing => "TUN_ADDRESS_MISSING",
            Self::SocketProtectionFailed => "SOCKET_PROTECTION_FAILED",
            Self::SocketAffinityInvalid => "SOCKET_AFFINITY_INVALID",
            Self::DnsApplyFailed => "DNS_APPLY_FAILED",
            Self::RouteApplyFailed => "ROUTE_APPLY_FAILED",
            Self::KillSwitchApplyFailed => "KILL_SWITCH_APPLY_FAILED",
            Self::KillSwitchStateMismatch => "KILL_SWITCH_STATE_MISMATCH",
            Self::SystemProxyStateMismatch => "SYSTEM_PROXY_STATE_MISMATCH",
            Self::RouteRestoreIncomplete => "ROUTE_RESTORE_INCOMPLETE",
            Self::DnsRestoreIncomplete => "DNS_RESTORE_INCOMPLETE",
            Self::SystemProxyStale => "SYSTEM_PROXY_STALE",
            Self::PlatformRecoveryPending => "PLATFORM_RECOVERY_PENDING",
            Self::PacketSendFailed => "PACKET_SEND_FAILED",
            Self::PacketSendTimeout => "PACKET_SEND_TIMEOUT",
            Self::PacketReceiveFailed => "PACKET_RECEIVE_FAILED",
            Self::PacketReceiveStalled => "PACKET_RECEIVE_STALLED",
            Self::SendQueueFull => "SEND_QUEUE_FULL",
            Self::DiagnosticAlreadyRunning => "DIAGNOSTIC_ALREADY_RUNNING",
            Self::DiagnosticTimeout => "DIAGNOSTIC_TIMEOUT",
            Self::DiagnosticCancelled => "DIAGNOSTIC_CANCELLED",
            Self::DiagnosticDependencyFailed => "DIAGNOSTIC_DEPENDENCY_FAILED",
            Self::Internal => "INTERNAL",
        }
    }

    pub const fn metadata(self) -> FailureMetadata {
        use FailureAction::{FallbackToH2, Retry, RetryAfterNetworkChange, Stop};
        use FailureSeverity::{Critical, Error, Warning};
        match self {
            Self::H3UdpUnreachable
            | Self::H3HandshakeTimeout
            | Self::H3ProtocolError
            | Self::H3DatagramUnavailable
            | Self::H3ConnectionClosed
            | Self::PmtuRevalidationExhausted => {
                FailureMetadata::new(Warning, true, true, FallbackToH2, "try_http2", true)
            }
            Self::PhysicalIpv4Unavailable
            | Self::PhysicalIpv6Unavailable
            | Self::PhysicalDnsUnavailable
            | Self::PhysicalNetworkChanged
            | Self::SocketAffinityInvalid => FailureMetadata::new(
                Warning,
                true,
                false,
                RetryAfterNetworkChange,
                "check_physical_network",
                true,
            ),
            Self::EndpointPinMismatch => FailureMetadata::new(
                Critical,
                false,
                false,
                Stop,
                "refresh_or_replace_identity",
                true,
            ),
            Self::IdentityInvalid | Self::AuthenticationFailed => {
                FailureMetadata::new(Error, false, false, Stop, "replace_identity", true)
            }
            Self::ConfigurationInvalid | Self::AddressAssignmentInvalid => {
                FailureMetadata::new(Error, false, false, Stop, "review_configuration", true)
            }
            Self::SocketProtectionFailed
            | Self::DnsApplyFailed
            | Self::RouteApplyFailed
            | Self::KillSwitchApplyFailed
            | Self::KillSwitchStateMismatch
            | Self::SystemProxyStateMismatch
            | Self::RouteRestoreIncomplete
            | Self::DnsRestoreIncomplete
            | Self::SystemProxyStale => {
                FailureMetadata::new(Critical, false, false, Stop, "restore_platform_state", true)
            }
            Self::DiagnosticCancelled => {
                FailureMetadata::new(Warning, false, false, Stop, "none", true)
            }
            Self::DiagnosticDependencyFailed => {
                FailureMetadata::new(Warning, false, false, Stop, "resolve_dependency", true)
            }
            Self::Internal => {
                FailureMetadata::new(Critical, false, false, Stop, "export_diagnostics", true)
            }
            _ => FailureMetadata::new(Error, true, false, Retry, "retry", true),
        }
    }

    pub const fn legacy_error_code(self) -> ErrorCode {
        match self {
            Self::EndpointPinMismatch => ErrorCode::PinMismatch,
            Self::IdentityInvalid => ErrorCode::MissingCredential,
            Self::AuthenticationFailed => ErrorCode::AuthenticationFailed,
            Self::ConfigurationInvalid => ErrorCode::InvalidConfiguration,
            Self::PhysicalDnsUnavailable | Self::DnsApplyFailed | Self::DnsRestoreIncomplete => {
                ErrorCode::DnsUnavailable
            }
            Self::AgentUnreachable | Self::EngineUnavailable => ErrorCode::IpcUnavailable,
            Self::VpnServiceUnavailable
            | Self::SocketProtectionFailed
            | Self::RouteApplyFailed
            | Self::KillSwitchApplyFailed
            | Self::KillSwitchStateMismatch
            | Self::SystemProxyStateMismatch
            | Self::RouteRestoreIncomplete
            | Self::SystemProxyStale
            | Self::PlatformRecoveryPending => ErrorCode::PlatformSetupFailed,
            Self::Internal => ErrorCode::Internal,
            _ => ErrorCode::TransportUnavailable,
        }
    }
}

impl std::fmt::Display for TransportFailureCode {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum TransportStage {
    EndpointResolution,
    SocketCreation,
    SocketProtection,
    SocketConnect,
    TlsHandshake,
    QuicHandshake,
    MasqueConnect,
    PeerSettings,
    AddressAssignment,
    TunnelStartup,
    PacketSend,
    PacketReceive,
    DnsApply,
    RouteApply,
    KillSwitchApply,
    PlatformRecovery,
    Diagnostics,
}

impl TransportStage {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::EndpointResolution => "endpoint_resolution",
            Self::SocketCreation => "socket_creation",
            Self::SocketProtection => "socket_protection",
            Self::SocketConnect => "socket_connect",
            Self::TlsHandshake => "tls_handshake",
            Self::QuicHandshake => "quic_handshake",
            Self::MasqueConnect => "masque_connect",
            Self::PeerSettings => "peer_settings",
            Self::AddressAssignment => "address_assignment",
            Self::TunnelStartup => "tunnel_startup",
            Self::PacketSend => "packet_send",
            Self::PacketReceive => "packet_receive",
            Self::DnsApply => "dns_apply",
            Self::RouteApply => "route_apply",
            Self::KillSwitchApply => "kill_switch_apply",
            Self::PlatformRecovery => "platform_recovery",
            Self::Diagnostics => "diagnostics",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum FailureSeverity {
    Info,
    Warning,
    Error,
    Critical,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum FailureAction {
    Retry,
    FallbackToH2,
    RetryAfterNetworkChange,
    Stop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FailureMetadata {
    pub severity: FailureSeverity,
    pub retryable: bool,
    pub fallback_allowed: bool,
    pub action: FailureAction,
    pub default_remediation: &'static str,
    pub safe_to_export: bool,
}

impl FailureMetadata {
    const fn new(
        severity: FailureSeverity,
        retryable: bool,
        fallback_allowed: bool,
        action: FailureAction,
        default_remediation: &'static str,
        safe_to_export: bool,
    ) -> Self {
        Self {
            severity,
            retryable,
            fallback_allowed,
            action,
            default_remediation,
            safe_to_export,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TransportFailure {
    pub code: TransportFailureCode,
    pub stage: TransportStage,
    pub transport: Option<Transport>,
    pub address_family: Option<AddressFamily>,
    pub retryable: bool,
    pub fallback_allowed: bool,
    pub severity: FailureSeverity,
    pub remediation_key: String,
    pub sanitized_detail: Option<String>,
}

impl TransportFailure {
    pub fn new(code: TransportFailureCode, stage: TransportStage) -> Self {
        let metadata = code.metadata();
        Self {
            code,
            stage,
            transport: None,
            address_family: None,
            retryable: metadata.retryable,
            fallback_allowed: metadata.fallback_allowed,
            severity: metadata.severity,
            remediation_key: metadata.default_remediation.to_owned(),
            sanitized_detail: None,
        }
    }

    pub const fn action(&self) -> FailureAction {
        self.code.metadata().action
    }

    pub fn on_path(mut self, transport: Transport, family: AddressFamily) -> Self {
        self.transport = Some(transport);
        self.address_family = Some(family);
        self
    }

    /// Attach only a pre-sanitized, non-identifying detail token.
    ///
    /// This deliberately rejects characters used by endpoints, hostnames and
    /// filesystem paths. Full internal error chains belong in filtered logs,
    /// never in the control protocol or diagnostic JSON.
    pub fn with_sanitized_detail(mut self, detail: impl Into<String>) -> Self {
        let detail = detail.into();
        if Self::sanitized_detail_is_safe(&detail) {
            self.sanitized_detail = Some(detail);
        }
        self
    }

    /// True only for the small numeric context grammar accepted by public
    /// diagnostics. Arbitrary strings are never considered sanitized merely
    /// because they omit punctuation.
    pub fn sanitized_detail_is_safe(detail: &str) -> bool {
        if detail.is_empty() || detail.len() > 64 || !detail.is_ascii() {
            return false;
        }
        ["attempt ", "status ", "generation ", "queue depth "]
            .iter()
            .any(|prefix| {
                detail.strip_prefix(prefix).is_some_and(|value| {
                    !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
                })
            })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn inv_failure_catalog_codes_are_unique_and_complete() {
        let codes: HashSet<_> = TransportFailureCode::ALL
            .iter()
            .map(|code| code.as_str())
            .collect();
        assert_eq!(codes.len(), TransportFailureCode::ALL.len());
        assert!(codes.iter().all(|code| !code.is_empty()));
    }

    #[test]
    fn inv_h3_fallback_matrix_never_masks_identity_or_platform_failures() {
        for code in [
            TransportFailureCode::EndpointPinMismatch,
            TransportFailureCode::IdentityInvalid,
            TransportFailureCode::AuthenticationFailed,
            TransportFailureCode::ConfigurationInvalid,
            TransportFailureCode::AddressAssignmentInvalid,
            TransportFailureCode::SocketProtectionFailed,
            TransportFailureCode::KillSwitchApplyFailed,
        ] {
            assert!(!code.metadata().fallback_allowed, "{code}");
            assert_ne!(
                code.metadata().action,
                FailureAction::FallbackToH2,
                "{code}"
            );
        }
    }

    #[test]
    fn pmtu_revalidation_exhaustion_has_a_stable_fallback_code() {
        let code = TransportFailureCode::PmtuRevalidationExhausted;
        assert_eq!(code.as_str(), "PMTU_REVALIDATION_EXHAUSTED");
        assert!(code.metadata().fallback_allowed);
        assert_eq!(code.metadata().action, FailureAction::FallbackToH2);
    }

    #[test]
    fn inv_export_sanitized_detail_rejects_endpoints_and_paths() {
        for private in [
            "198.51.100.7",
            "private.example",
            r"C:\\Users\\private",
            "token:secret",
            "supersecret",
        ] {
            let failure =
                TransportFailure::new(TransportFailureCode::Internal, TransportStage::Diagnostics)
                    .with_sanitized_detail(private);
            assert!(failure.sanitized_detail.is_none(), "accepted {private}");
        }
        let safe = TransportFailure::new(
            TransportFailureCode::H3HandshakeTimeout,
            TransportStage::QuicHandshake,
        )
        .with_sanitized_detail("attempt 2");
        assert_eq!(safe.sanitized_detail.as_deref(), Some("attempt 2"));
    }
}
