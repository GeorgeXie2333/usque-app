use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::failure::{FailureSeverity, TransportFailure};

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
