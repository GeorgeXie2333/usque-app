//! Numeric provider context only; never remote descriptions or credentials.
use crate::{TransportFailure, TransportFailureCode, TransportStage};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureKind {
    Authentication,
    Certificate,
    Restricted,
    MaxSessions,
    KeyReused,
    Policy,
    Server,
    RateLimited,
    Api,
    Protocol,
    Transport,
    Reconnect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionFailure {
    pub kind: FailureKind,
    pub code: Option<u32>,
    pub http_status: Option<u16>,
}
impl ConnectionFailure {
    pub const fn new(kind: FailureKind) -> Self {
        Self {
            kind,
            code: None,
            http_status: None,
        }
    }
    pub const fn agent(code: u32) -> Self {
        let kind = match code {
            86100 | 86106 | 86120..=86122 => FailureKind::Authentication,
            86101 | 86102 | 86105 => FailureKind::Certificate,
            86103 => FailureKind::KeyReused,
            86104 => FailureKind::Restricted,
            86110..=86115 => FailureKind::MaxSessions,
            86151..=86154 => FailureKind::Policy,
            _ => FailureKind::Server,
        };
        Self {
            kind,
            code: Some(code),
            http_status: None,
        }
    }
    pub const fn retryable(self) -> bool {
        matches!(
            self.kind,
            FailureKind::Transport | FailureKind::Restricted | FailureKind::Reconnect
        )
    }
    pub fn failure(self) -> TransportFailure {
        use TransportFailureCode as Code;
        let code = match self.kind {
            FailureKind::Authentication => Code::ProtonAuthenticationFailed,
            FailureKind::Certificate => Code::ProtonCertificateFailed,
            FailureKind::Restricted => Code::ProtonServerRestricted,
            FailureKind::MaxSessions => Code::ProtonSessionLimit,
            FailureKind::KeyReused => Code::ProtonKeyReused,
            FailureKind::Policy => Code::ProtonPolicyRestricted,
            FailureKind::Server => Code::ProtonServerFailed,
            FailureKind::RateLimited => Code::ProtonRateLimited,
            FailureKind::Api => Code::ProtonApiFailed,
            FailureKind::Protocol => Code::ProtonProtocolFailed,
            FailureKind::Transport => Code::ProtonTransportFailed,
            FailureKind::Reconnect => Code::ProtonReconnectRequired,
        };
        let mut failure = TransportFailure::new(
            code,
            if self.http_status.is_some() {
                TransportStage::TunnelStartup
            } else {
                TransportStage::TlsHandshake
            },
        );
        failure.retryable = self.retryable();
        failure.fallback_allowed = false;
        match (self.code, self.http_status) {
            (Some(code), Some(status)) => {
                failure.with_sanitized_detail(format!("proton api {code} http {status}"))
            }
            (Some(code), None) => failure.with_sanitized_detail(format!("proton agent {code}")),
            _ => failure,
        }
    }
}

pub fn valid_label(label: &str) -> bool {
    !label.is_empty()
        && label.len() <= 64
        && label
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn numeric_context_round_trips_without_remote_descriptions_or_warp_recovery() {
        for code in [
            86100, 86101, 86102, 86103, 86104, 86105, 86111, 86151, 99000,
        ] {
            let reason = ConnectionFailure::agent(code);
            let wire = serde_json::to_string(&reason).unwrap();
            assert_eq!(
                serde_json::from_str::<ConnectionFailure>(&wire).unwrap(),
                reason
            );
            let failure = reason.failure();
            assert!(failure.code.as_str().starts_with("PROTON_"));
            assert_eq!(
                failure.sanitized_detail,
                Some(format!("proton agent {code}"))
            );
            assert!(!failure.fallback_allowed);
            assert!(!failure.remediation_key.contains("identity"));
        }
        for private in [
            "proton agent uid",
            "proton agent 4294967296",
            "proton api 401 http 401 token",
            "proton api 1 http 700",
            "proton api 1 http 0",
        ] {
            assert!(!TransportFailure::sanitized_detail_is_safe(private));
        }
    }
}
