use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::exit_probe::ExitInfo;
use crate::failure::TransportFailure;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionPhase {
    Disconnected,
    Preparing,
    ConnectingHttp3,
    ConnectingHttp2,
    Connected,
    Degraded,
    Reconnecting,
    Disconnecting,
    Error,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Transport {
    Http3,
    Http2,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum AddressFamily {
    Ipv4,
    Ipv6,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct Statistics {
    pub connected_seconds: u64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub current_upload_bytes_per_second: u64,
    pub current_download_bytes_per_second: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConnectionSnapshot {
    pub phase: ConnectionPhase,
    pub changed_at: DateTime<Utc>,
    pub transport: Option<Transport>,
    pub address_family: Option<AddressFamily>,
    pub ipv4_available: bool,
    pub ipv6_available: bool,
    pub statistics: Statistics,
    pub exit: Option<ExitInfo>,
    pub error: Option<ConnectionError>,
    /// Structured, export-safe failure details for new control clients.
    /// `error` remains populated for backwards compatibility.
    pub failure: Option<TransportFailure>,
    pub kill_switch_state: KillSwitchState,
    pub lockdown_state: LockdownState,
    pub reconnect_count: u32,
    pub active_listeners: Vec<String>,
    pub warnings: Vec<ConnectionWarning>,
    pub frontends: Vec<FrontendStatus>,
}

impl Default for ConnectionSnapshot {
    fn default() -> Self {
        Self {
            phase: ConnectionPhase::Disconnected,
            changed_at: Utc::now(),
            transport: None,
            address_family: None,
            ipv4_available: false,
            ipv6_available: false,
            statistics: Statistics::default(),
            exit: None,
            error: None,
            failure: None,
            kill_switch_state: KillSwitchState::NotApplicable,
            lockdown_state: LockdownState::NotSupported,
            reconnect_count: 0,
            active_listeners: Vec::new(),
            warnings: Vec::new(),
            frontends: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConnectionError {
    pub code: ErrorCode,
    pub message: String,
    pub retryable: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum KillSwitchState {
    #[default]
    NotApplicable,
    Inactive,
    Active,
    Error,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum LockdownState {
    #[default]
    NotSupported,
    Disabled,
    Enabled,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConnectionWarning {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidConfiguration,
    MissingCredential,
    EndpointUnreachable,
    AuthenticationFailed,
    PinMismatch,
    PinRefreshFailed,
    TransportUnavailable,
    DnsUnavailable,
    PlatformPermissionDenied,
    PlatformSetupFailed,
    IpcUnavailable,
    Internal,
    WindowsRecoveryFailed,
    WindowsRecoveryExhausted,
    WindowsRecoveryBlocked,
    WindowsRecoveryTimeout,
    WindowsRecoveryConflict,
    WindowsRecoveryUnsupported,
}

#[derive(Debug, Clone, Default)]
pub struct StateMachine {
    snapshot: ConnectionSnapshot,
}

impl StateMachine {
    pub fn snapshot(&self) -> &ConnectionSnapshot {
        &self.snapshot
    }

    pub fn transition(
        &mut self,
        phase: ConnectionPhase,
    ) -> Result<&ConnectionSnapshot, TransitionError> {
        if !can_transition(self.snapshot.phase, phase) {
            return Err(TransitionError {
                from: self.snapshot.phase,
                to: phase,
            });
        }
        self.snapshot.phase = phase;
        self.snapshot.changed_at = Utc::now();
        if phase != ConnectionPhase::Error {
            self.snapshot.error = None;
            self.snapshot.failure = None;
        }
        if phase == ConnectionPhase::Disconnected {
            self.snapshot.transport = None;
            self.snapshot.address_family = None;
            self.snapshot.ipv4_available = false;
            self.snapshot.ipv6_available = false;
            self.snapshot.exit = None;
            self.snapshot.statistics = Statistics::default();
            self.snapshot.kill_switch_state = KillSwitchState::NotApplicable;
            self.snapshot.lockdown_state = LockdownState::NotSupported;
            self.snapshot.reconnect_count = 0;
            self.snapshot.active_listeners.clear();
            self.snapshot.warnings.clear();
            self.snapshot.frontends.clear();
        }
        Ok(&self.snapshot)
    }

    pub fn mark_connected(
        &mut self,
        transport: Transport,
        family: AddressFamily,
        ipv4_available: bool,
        ipv6_available: bool,
    ) -> Result<&ConnectionSnapshot, TransitionError> {
        let phase = if ipv4_available && ipv6_available {
            ConnectionPhase::Connected
        } else {
            ConnectionPhase::Degraded
        };
        self.transition(phase)?;
        self.snapshot.transport = Some(transport);
        self.snapshot.address_family = Some(family);
        self.snapshot.ipv4_available = ipv4_available;
        self.snapshot.ipv6_available = ipv6_available;
        Ok(&self.snapshot)
    }

    pub fn mark_error(&mut self, error: ConnectionError) -> &ConnectionSnapshot {
        self.snapshot.phase = ConnectionPhase::Error;
        self.snapshot.changed_at = Utc::now();
        self.snapshot.error = Some(error);
        self.snapshot.failure = None;
        &self.snapshot
    }

    pub fn mark_failure(
        &mut self,
        failure: TransportFailure,
        legacy_message: impl Into<String>,
    ) -> &ConnectionSnapshot {
        self.snapshot.phase = ConnectionPhase::Error;
        self.snapshot.changed_at = Utc::now();
        self.snapshot.error = Some(ConnectionError {
            code: failure.code.legacy_error_code(),
            message: legacy_message.into(),
            retryable: failure.retryable,
        });
        self.snapshot.failure = Some(failure);
        &self.snapshot
    }

    pub fn set_exit_info(&mut self, exit: ExitInfo) {
        self.snapshot.exit = Some(exit);
    }

    pub fn update_statistics(&mut self, statistics: Statistics) {
        self.snapshot.statistics = statistics;
    }

    pub fn update_runtime_metadata(
        &mut self,
        reconnect_count: u32,
        active_listeners: Vec<String>,
        warnings: Vec<ConnectionWarning>,
    ) {
        self.snapshot.reconnect_count = reconnect_count;
        self.snapshot.active_listeners = active_listeners;
        self.snapshot.warnings = warnings;
    }

    pub fn update_reconnect_count(&mut self, reconnect_count: u32) {
        self.snapshot.reconnect_count = reconnect_count;
    }

    pub fn update_failure(&mut self, failure: Option<TransportFailure>) {
        self.snapshot.failure = failure;
    }

    pub fn update_safety_state(
        &mut self,
        kill_switch_state: KillSwitchState,
        lockdown_state: LockdownState,
    ) {
        self.snapshot.kill_switch_state = kill_switch_state;
        self.snapshot.lockdown_state = lockdown_state;
    }

    pub fn update_frontends(&mut self, frontends: Vec<FrontendStatus>) {
        self.snapshot.frontends = frontends;
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum FrontendKind {
    Tunnel,
    Socks5,
    Http,
    SystemProxy,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum FrontendPhase {
    Disabled,
    Preparing,
    Active,
    Degraded,
    Reconnecting,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FrontendStatus {
    pub kind: FrontendKind,
    pub phase: FrontendPhase,
    pub listeners: Vec<String>,
    pub error: Option<ConnectionError>,
}

fn can_transition(from: ConnectionPhase, to: ConnectionPhase) -> bool {
    use ConnectionPhase::*;
    match from {
        Disconnected => matches!(to, Preparing | Reconnecting | Disconnected),
        Preparing => matches!(
            to,
            ConnectingHttp3 | ConnectingHttp2 | Reconnecting | Disconnecting | Error
        ),
        ConnectingHttp3 => matches!(
            to,
            ConnectingHttp2 | Connected | Degraded | Reconnecting | Disconnecting | Error
        ),
        ConnectingHttp2 => matches!(
            to,
            Connected | Degraded | Reconnecting | Disconnecting | Error
        ),
        Connected | Degraded => matches!(
            to,
            Degraded | Connected | Reconnecting | Disconnecting | Error
        ),
        Reconnecting => matches!(
            to,
            Preparing
                | ConnectingHttp3
                | ConnectingHttp2
                | Connected
                | Degraded
                | Disconnecting
                | Error
        ),
        Disconnecting => matches!(to, Disconnected | Error),
        Error => matches!(
            to,
            Preparing | Reconnecting | Disconnecting | Disconnected | Error
        ),
    }
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
#[error("invalid connection state transition from {from:?} to {to:?}")]
pub struct TransitionError {
    pub from: ConnectionPhase,
    pub to: ConnectionPhase,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn happy_path_and_disconnect_are_valid() {
        let mut state = StateMachine::default();
        state.transition(ConnectionPhase::Preparing).unwrap();
        state.transition(ConnectionPhase::ConnectingHttp3).unwrap();
        state
            .mark_connected(Transport::Http3, AddressFamily::Ipv6, true, true)
            .unwrap();
        state.transition(ConnectionPhase::Disconnecting).unwrap();
        state.transition(ConnectionPhase::Disconnected).unwrap();
        assert_eq!(state.snapshot().phase, ConnectionPhase::Disconnected);
        assert_eq!(state.snapshot().transport, None);
    }

    #[test]
    fn single_stack_is_degraded() {
        let mut state = StateMachine::default();
        state.transition(ConnectionPhase::Preparing).unwrap();
        state.transition(ConnectionPhase::ConnectingHttp2).unwrap();
        state
            .mark_connected(Transport::Http2, AddressFamily::Ipv4, true, false)
            .unwrap();
        assert_eq!(state.snapshot().phase, ConnectionPhase::Degraded);
    }

    #[test]
    fn cannot_skip_preparation() {
        let mut state = StateMachine::default();
        assert!(state.transition(ConnectionPhase::Connected).is_err());
    }

    #[test]
    fn bounded_platform_recovery_can_wait_then_restart_preparation() {
        let mut state = StateMachine::default();
        state.transition(ConnectionPhase::Reconnecting).unwrap();
        state.transition(ConnectionPhase::Preparing).unwrap();
        state.mark_error(ConnectionError {
            code: ErrorCode::WindowsRecoveryExhausted,
            message: "sanitized".to_owned(),
            retryable: true,
        });
        state.transition(ConnectionPhase::Reconnecting).unwrap();
        assert!(state.snapshot().error.is_none());
    }
}
