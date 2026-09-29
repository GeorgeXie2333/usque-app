use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::diagnostics_contract_generated::{EVIDENCE_KEYS, EVIDENCE_TOKENS};
use crate::failure::{FailureSeverity, TransportFailure};

/// How a finding's evidence was obtained. This is independent of its severity.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticObservationSource {
    #[default]
    Unknown,
    Config,
    Runtime,
    Platform,
    ActiveProbe,
    Frontend,
}

impl DiagnosticObservationSource {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Config => "config",
            Self::Runtime => "runtime",
            Self::Platform => "platform",
            Self::ActiveProbe => "active_probe",
            Self::Frontend => "frontend",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticObservationAvailability {
    Observed,
    Inferred,
    #[default]
    Unavailable,
    Stale,
    NotApplicable,
}

impl DiagnosticObservationAvailability {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Observed => "observed",
            Self::Inferred => "inferred",
            Self::Unavailable => "unavailable",
            Self::Stale => "stale",
            Self::NotApplicable => "not_applicable",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct DiagnosticObservation {
    pub source: DiagnosticObservationSource,
    pub availability: DiagnosticObservationAvailability,
    pub age_milliseconds: u64,
    /// Random runtime identity, never an account or device identifier.
    pub connection_instance_id: Option<Uuid>,
    pub network_generation: Option<u64>,
}

/// Public evidence accepts only contract-defined facts and unsigned numbers.
/// Private fields require producers to use the validated compatibility parser.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiagnosticEvidence {
    key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    number: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    token: Option<String>,
}

impl DiagnosticEvidence {
    pub fn from_legacy(value: &str) -> Option<Self> {
        if let Some((key, value)) = value.split_once('=') {
            if !EVIDENCE_KEYS.contains(&key)
                || value.is_empty()
                || !value.bytes().all(|byte| byte.is_ascii_digit())
            {
                return None;
            }
            Some(Self {
                key: key.to_owned(),
                number: Some(value.parse().ok()?),
                token: None,
            })
        } else if EVIDENCE_TOKENS.contains(&value) {
            Some(Self {
                key: "fact".to_owned(),
                number: None,
                token: Some(value.to_owned()),
            })
        } else {
            None
        }
    }

    pub fn is_export_safe(&self) -> bool {
        match (self.number, self.token.as_deref()) {
            (Some(_), None) => EVIDENCE_KEYS.contains(&self.key.as_str()),
            (None, Some(token)) => self.key == "fact" && EVIDENCE_TOKENS.contains(&token),
            _ => false,
        }
    }

    pub fn key(&self) -> &str {
        &self.key
    }
    pub const fn number(&self) -> Option<u64> {
        self.number
    }
    pub fn token(&self) -> Option<&str> {
        self.token.as_deref()
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticMode {
    Standard,
    Deep,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticSessionState {
    Pending,
    Running,
    Cancelling,
    Completed,
    Failed,
    Cancelled,
}

impl DiagnosticSessionState {
    pub const fn is_active(self) -> bool {
        matches!(self, Self::Pending | Self::Running | Self::Cancelling)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticCheckStatus {
    Pending,
    Running,
    Passed,
    Warning,
    Failed,
    Skipped,
    Cancelled,
}

impl DiagnosticCheckStatus {
    pub const fn satisfies_dependency(self) -> bool {
        matches!(self, Self::Passed | Self::Warning)
    }

    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Passed | Self::Warning | Self::Failed | Self::Skipped | Self::Cancelled
        )
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticCategory {
    LocalComponent,
    PhysicalNetwork,
    Transport,
    Tunnel,
    Protection,
    Recovery,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiagnosticFinding {
    pub check_id: String,
    pub category: DiagnosticCategory,
    pub status: DiagnosticCheckStatus,
    pub failure: Option<TransportFailure>,
    pub severity: FailureSeverity,
    pub summary_key: String,
    pub remediation_key: String,
    pub sanitized_evidence: Vec<String>,
    pub started_at: Option<DateTime<Utc>>,
    pub duration_milliseconds: Option<u64>,
    pub dependency_reason: Option<String>,
    #[serde(default)]
    pub observation: Option<DiagnosticObservation>,
    #[serde(default)]
    pub evidence: Vec<DiagnosticEvidence>,
}

impl DiagnosticFinding {
    pub fn pending(check_id: impl Into<String>, category: DiagnosticCategory) -> Self {
        Self {
            check_id: check_id.into(),
            category,
            status: DiagnosticCheckStatus::Pending,
            failure: None,
            severity: FailureSeverity::Info,
            summary_key: "diagnostic_pending".to_owned(),
            remediation_key: "none".to_owned(),
            sanitized_evidence: Vec::new(),
            started_at: None,
            duration_milliseconds: None,
            dependency_reason: None,
            observation: None,
            evidence: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiagnosticSummary {
    pub passed: u32,
    pub warnings: u32,
    pub failed: u32,
    pub skipped: u32,
    pub cancelled: u32,
}

impl DiagnosticSummary {
    pub fn from_findings(findings: &[DiagnosticFinding]) -> Self {
        let mut summary = Self::default();
        for finding in findings {
            match finding.status {
                DiagnosticCheckStatus::Passed => summary.passed += 1,
                DiagnosticCheckStatus::Warning => summary.warnings += 1,
                DiagnosticCheckStatus::Failed => summary.failed += 1,
                DiagnosticCheckStatus::Skipped => summary.skipped += 1,
                DiagnosticCheckStatus::Cancelled => summary.cancelled += 1,
                DiagnosticCheckStatus::Pending | DiagnosticCheckStatus::Running => {}
            }
        }
        summary
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiagnosticSession {
    pub session_id: Uuid,
    pub state: DiagnosticSessionState,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub mode: DiagnosticMode,
    pub current_check: Option<String>,
    pub progress_percent: u32,
    pub findings: Vec<DiagnosticFinding>,
    pub summary: DiagnosticSummary,
}

impl DiagnosticSession {
    pub fn pending(mode: DiagnosticMode, findings: Vec<DiagnosticFinding>) -> Self {
        Self {
            session_id: Uuid::new_v4(),
            state: DiagnosticSessionState::Pending,
            started_at: Utc::now(),
            completed_at: None,
            mode,
            current_check: None,
            progress_percent: 0,
            findings,
            summary: DiagnosticSummary::default(),
        }
    }

    pub fn recompute_summary(&mut self) {
        self.summary = DiagnosticSummary::from_findings(&self.findings);
        let terminal = self
            .findings
            .iter()
            .filter(|finding| finding.status.is_terminal())
            .count();
        self.progress_percent = if self.findings.is_empty() {
            100
        } else {
            ((terminal * 100) / self.findings.len()) as u32
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_evidence_rejects_private_or_ambiguous_values() {
        for value in [
            "rtt_ms=secret",
            "rtt_ms=-1",
            "endpoint=123",
            "rtt_ms=1\nsecret",
            "private.example",
            "rtt_ms=18446744073709551616",
        ] {
            assert!(DiagnosticEvidence::from_legacy(value).is_none());
        }
        for value in ["rtt_ms=0", "queue_drops=42", "schema_valid"] {
            assert!(
                DiagnosticEvidence::from_legacy(value)
                    .unwrap()
                    .is_export_safe()
            );
        }
        let hostile: DiagnosticEvidence =
            serde_json::from_str(r#"{"key":"password","number":1,"token":null}"#).unwrap();
        assert!(!hostile.is_export_safe());
    }

    #[test]
    fn inv_diagnostics_session_progress_is_bounded_and_recoverable() {
        let mut session = DiagnosticSession::pending(
            DiagnosticMode::Standard,
            vec![
                DiagnosticFinding::pending(
                    "engine.control_channel",
                    DiagnosticCategory::LocalComponent,
                ),
                DiagnosticFinding::pending(
                    "physical.network_present",
                    DiagnosticCategory::PhysicalNetwork,
                ),
            ],
        );
        session.findings[0].status = DiagnosticCheckStatus::Passed;
        session.recompute_summary();
        assert_eq!(session.progress_percent, 50);

        let encoded = serde_json::to_vec(&session).expect("serialize session");
        let recovered: DiagnosticSession =
            serde_json::from_slice(&encoded).expect("recover session");
        assert_eq!(recovered, session);
    }
}
