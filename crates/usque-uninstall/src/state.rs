//! Pure presentation state. No registry, file deletion, or installer calls.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Preview {
    #[default]
    Confirm,
    Progress,
    Success,
    Reboot,
    Cancelled,
    Failure,
    PartialData,
    RegistrationFailure,
    FilesInUse,
    Rollback,
    RestartFailure,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreviewTheme {
    Light,
    Dark,
    HighContrast,
}

impl PreviewTheme {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "light" => Some(Self::Light),
            "dark" => Some(Self::Dark),
            "high-contrast" => Some(Self::HighContrast),
            _ => None,
        }
    }
}

impl Preview {
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "confirm" => Self::Confirm,
            "progress" => Self::Progress,
            "success" => Self::Success,
            "reboot" => Self::Reboot,
            "cancelled" => Self::Cancelled,
            "failure" => Self::Failure,
            "partial-data" => Self::PartialData,
            "registration-failure" => Self::RegistrationFailure,
            "files-in-use" => Self::FilesInUse,
            "rollback" => Self::Rollback,
            "restart-failure" => Self::RestartFailure,
            _ => return None,
        })
    }
}

/// A reboot is a separate, explicitly confirmed operation after MSI succeeds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RestartFlow {
    #[default]
    Idle,
    Confirming,
    Requested,
    Failed(u32),
    Cancelled,
    Previewed,
}

impl RestartFlow {
    pub fn begin(&mut self, reboot_required: bool) {
        if reboot_required && matches!(self, Self::Idle | Self::Failed(_) | Self::Cancelled) {
            *self = Self::Confirming;
        }
    }

    pub fn back(&mut self) {
        if *self == Self::Confirming {
            *self = Self::Idle;
        }
    }

    pub fn confirm(&mut self, preview: bool, request: impl FnOnce() -> Result<(), u32>) {
        if *self != Self::Confirming {
            return;
        }
        // The injected OS operation is never evaluated in any preview binary.
        *self = if preview {
            Self::Previewed
        } else {
            match request() {
                Ok(()) => Self::Requested,
                Err(code) => Self::Failed(code),
            }
        };
    }

    pub fn cancelled_by_windows(&mut self) {
        if *self == Self::Requested {
            *self = Self::Cancelled;
        }
    }

    pub fn message_key(self) -> Option<&'static str> {
        match self {
            Self::Idle => None,
            Self::Confirming => Some("save_work"),
            Self::Requested => Some("restart_requested"),
            Self::Failed(_) => Some("restart_failed"),
            Self::Cancelled => Some("restart_cancelled"),
            Self::Previewed => Some("preview"),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Stage {
    #[default]
    Preparing,
    ClosingApps,
    RestoringNetwork,
    DeletingData,
    RemovingFiles,
    Registration,
    RollingBack,
}

impl Stage {
    pub fn key(self) -> &'static str {
        match self {
            Self::Preparing => "uninstall_preparing",
            Self::ClosingApps => "uninstall_closing_apps",
            Self::RestoringNetwork => "uninstall_restoring_network",
            Self::DeletingData => "uninstall_deleting_data",
            Self::RemovingFiles => "uninstall_removing_files",
            Self::Registration => "uninstall_registration",
            Self::RollingBack => "uninstall_rolling_back",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Success,
    RebootRequired,
    Cancelled,
    PermissionDenied,
    MsiFailed,
    NetworkFailed,
    DataMayBeDeleted,
    RegistrationFailed,
}

impl Outcome {
    pub fn title_key(self) -> &'static str {
        match self {
            Self::Success => "uninstall_complete_title",
            Self::RebootRequired => "reboot_title",
            Self::Cancelled => "uninstall_cancelled_title",
            Self::DataMayBeDeleted => "uninstall_partial_data_title",
            _ => "uninstall_failed_title",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::Success => "uninstall_success",
            Self::RebootRequired => "uninstall_reboot",
            Self::Cancelled => "uninstall_cancelled",
            Self::PermissionDenied => "uninstall_permission_denied",
            Self::MsiFailed => "uninstall_failed",
            Self::NetworkFailed => "uninstall_network_failed",
            Self::DataMayBeDeleted => "uninstall_partial_data",
            Self::RegistrationFailed => "uninstall_registration_failed",
        }
    }

    pub fn can_return_to_confirmation(self) -> bool {
        matches!(
            self,
            Self::MsiFailed
                | Self::NetworkFailed
                | Self::Cancelled
                | Self::PermissionDenied
                | Self::DataMayBeDeleted
        )
    }
}

/// Presentation-only guidance for a numeric installer error. Never format an
/// MSI record, product code, or a path into user-facing failure guidance.
pub fn error_hint_key(code: u32) -> &'static str {
    match code {
        5 | 1303 | 1310 | 1314 | 1402 | 1406 | 1925 => "error_permission_hint",
        1618 => "error_busy_hint",
        13 | 1605 | 1612 | 1619 | 1620 | 1635 | 1636 | 1706 => "error_source_hint",
        112 | 1307 => "error_disk_hint",
        _ => "error_generic_hint",
    }
}

/// Tracks MSI's own progress units. A generation phase is not execution proof.
#[derive(Clone, Debug, Default)]
pub struct Progress {
    total: i64,
    current: i64,
    backward: bool,
    action_ticks: i64,
    initialized: bool,
    pub generating: bool,
}

impl Progress {
    pub fn record(&mut self, fields: [i32; 4]) {
        let [kind, value, flag, script] = fields;
        match kind {
            0 if value >= 0 && matches!(flag, 0 | 1) && matches!(script, 0 | 1) => {
                self.total = i64::from(value);
                self.backward = flag == 1;
                self.current = if self.backward { self.total } else { 0 };
                self.generating = script == 1;
                self.action_ticks = 0;
                self.initialized = true;
            }
            1 if self.initialized && value >= 0 => {
                self.action_ticks = if flag == 1 { i64::from(value) } else { 0 };
            }
            2 if self.initialized && value >= 0 => self.advance(i64::from(value)),
            3 if self.initialized && value >= 0 => {
                self.total = self.total.saturating_add(i64::from(value));
            }
            _ => {}
        }
    }

    pub fn action_data(&mut self) {
        self.advance(self.action_ticks);
    }

    pub fn new_action(&mut self) {
        self.action_ticks = 0;
    }

    fn advance(&mut self, ticks: i64) {
        self.current = if self.backward {
            self.current.saturating_sub(ticks)
        } else {
            self.current.saturating_add(ticks)
        }
        .clamp(0, self.total.max(0));
    }

    pub fn percent(&self) -> Option<u16> {
        if !self.initialized || self.generating || self.total == 0 {
            None
        } else {
            Some((self.current.saturating_mul(100) / self.total).clamp(0, 100) as u16)
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Lifecycle {
    pub stage: Stage,
    pub progress: Progress,
    pub purge_started: bool,
    pub execution_started: bool,
    pub cancel_requested: bool,
    msi_allows_cancel: bool,
    rollback_started: bool,
    recovery_failure: bool,
}

impl Lifecycle {
    pub fn action(&mut self, name: &str) {
        self.progress.new_action();
        // Purge disables cancellation conservatively even if MSI is still
        // scheduling this action. A scheduling notification is never success.
        if name == "PurgeUserData" {
            self.purge_started = true;
        }
        if matches!(name, "Rollback" | "RollbackCleanup") {
            self.rollback_started = true;
            self.stage = Stage::RollingBack;
        } else if !self.rollback_started {
            self.stage = match name {
                "InstallValidate" | "StopServices" => Stage::ClosingApps,
                "EmergencyRemoveKillSwitch" | "RecoverAgentState" => Stage::RestoringNetwork,
                "PurgeUserData" => Stage::DeletingData,
                "RemoveFiles"
                | "DeleteServices"
                | "RemoveUserStartupRegistration"
                | "FinalizeAgentUninstall" => Stage::RemovingFiles,
                _ => self.stage,
            };
        }
    }

    pub fn progress_record(&mut self, fields: [i32; 4]) {
        self.progress.record(fields);
        if fields[0] == 0 && fields[3] == 0 && fields[1] >= 0 {
            self.execution_started = true;
        }
        if fields[0] == 0 && fields[2] == 1 && !self.progress.generating {
            self.rollback_started = true;
            self.stage = Stage::RollingBack;
        }
    }

    pub fn common_data(&mut self, kind: i32, value: i32) {
        if kind == 2 {
            self.msi_allows_cancel = value == 1;
        }
    }

    pub fn action_failed(&mut self, action: &str) {
        if matches!(action, "EmergencyRemoveKillSwitch" | "RecoverAgentState") {
            self.recovery_failure = true;
        }
    }

    pub fn can_cancel(&self) -> bool {
        self.msi_allows_cancel
            && !self.purge_started
            && !self.rollback_started
            && self.stage != Stage::Registration
            && !self.cancel_requested
    }

    pub fn request_cancel(&mut self) -> bool {
        if !self.can_cancel() {
            return false;
        }
        self.cancel_requested = true;
        true
    }

    pub fn cancel_at_callback(&self) -> bool {
        self.cancel_requested
            && self.msi_allows_cancel
            && !self.purge_started
            && !self.rollback_started
            && self.stage != Stage::Registration
    }

    pub fn user_declined_prompt(&mut self) {
        self.cancel_requested = true;
    }

    pub fn finish(&self, code: u32) -> Outcome {
        match code {
            0 => Outcome::Success,
            3010 | 1641 => Outcome::RebootRequired,
            _ if self.purge_started => Outcome::DataMayBeDeleted,
            5 | 1925 => Outcome::PermissionDenied,
            1602 if !self.execution_started && !self.cancel_requested => Outcome::PermissionDenied,
            1602 => Outcome::Cancelled,
            _ if self.recovery_failure => Outcome::NetworkFailed,
            _ => Outcome::MsiFailed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn results_have_distinct_titles_and_actionable_numeric_error_guidance() {
        assert_eq!(Outcome::Cancelled.title_key(), "uninstall_cancelled_title");
        assert_eq!(
            Outcome::DataMayBeDeleted.title_key(),
            "uninstall_partial_data_title"
        );
        for outcome in [
            Outcome::PermissionDenied,
            Outcome::MsiFailed,
            Outcome::NetworkFailed,
            Outcome::RegistrationFailed,
        ] {
            assert_eq!(outcome.title_key(), "uninstall_failed_title");
        }
        for code in [5, 1303, 1310, 1314, 1402, 1406, 1925] {
            assert_eq!(error_hint_key(code), "error_permission_hint");
        }
        assert_eq!(error_hint_key(1618), "error_busy_hint");
        for code in [13, 1605, 1612, 1619, 1620, 1635, 1636, 1706] {
            assert_eq!(error_hint_key(code), "error_source_hint");
        }
        for code in [112, 1307] {
            assert_eq!(error_hint_key(code), "error_disk_hint");
        }
        assert_eq!(error_hint_key(1603), "error_generic_hint");
        assert_eq!(error_hint_key(u32::MAX), "error_generic_hint");
    }

    #[test]
    fn restart_requires_a_reboot_result_and_a_distinct_final_confirmation() {
        let mut flow = RestartFlow::default();
        flow.begin(false);
        flow.confirm(false, || panic!("unconfirmed restart must never run"));
        assert_eq!(flow, RestartFlow::Idle);
        flow.begin(true);
        assert_eq!(flow, RestartFlow::Confirming);
        assert_eq!(flow.message_key(), Some("save_work"));
        flow.back();
        flow.confirm(false, || {
            panic!("Back must revoke the pending confirmation")
        });
        assert_eq!(flow, RestartFlow::Idle);
        flow.begin(true);
        flow.confirm(false, || Ok(()));
        assert_eq!(flow, RestartFlow::Requested);
        flow.confirm(false, || panic!("a request must not be replayed"));
    }

    #[test]
    fn restart_preview_never_evaluates_the_system_operation() {
        let mut flow = RestartFlow::default();
        flow.begin(true);
        flow.confirm(true, || {
            panic!("preview must not touch privileges or restart APIs")
        });
        assert_eq!(flow, RestartFlow::Previewed);
        assert_eq!(flow.message_key(), Some("preview"));
    }

    #[test]
    fn restart_failure_and_windows_cancellation_keep_an_explicit_retry_boundary() {
        let mut flow = RestartFlow::default();
        flow.begin(true);
        flow.confirm(false, || Err(1314));
        assert_eq!(flow, RestartFlow::Failed(1314));
        flow.confirm(false, || {
            panic!("failed request must return through confirmation")
        });
        flow.begin(true);
        flow.confirm(false, || Ok(()));
        flow.cancelled_by_windows();
        assert_eq!(flow, RestartFlow::Cancelled);
        flow.begin(true);
        assert_eq!(flow, RestartFlow::Confirming);
    }

    #[test]
    fn cancellation_requires_msi_permission_and_stops_before_irreversible_work() {
        let mut state = Lifecycle::default();
        assert!(!state.can_cancel());
        state.common_data(2, 1);
        assert!(state.can_cancel());
        state.action("PurgeUserData");
        state.common_data(2, 1);
        assert!(!state.request_cancel());
        assert_eq!(state.finish(1602), Outcome::DataMayBeDeleted);
        assert_eq!(state.finish(1603), Outcome::DataMayBeDeleted);
        assert!(Outcome::DataMayBeDeleted.can_return_to_confirmation());
    }

    #[test]
    fn rollback_cannot_reenable_cancellation_or_erase_purge_warning() {
        let mut state = Lifecycle::default();
        state.action("PurgeUserData");
        state.action("Rollback");
        state.action("RemoveFiles");
        state.common_data(2, 1);
        assert_eq!(state.stage, Stage::RollingBack);
        assert!(!state.can_cancel());
        assert_eq!(state.finish(1603), Outcome::DataMayBeDeleted);
    }

    #[test]
    fn only_final_installer_code_establishes_success_or_reboot() {
        let mut state = Lifecycle::default();
        state.progress_record([0, 100, 0, 0]);
        state.progress_record([2, 100, 0, 0]);
        assert_eq!(state.progress.percent(), Some(100));
        assert_eq!(state.finish(1603), Outcome::MsiFailed);
        assert_eq!(state.finish(0), Outcome::Success);
        assert_eq!(state.finish(3010), Outcome::RebootRequired);
        assert_eq!(state.finish(1641), Outcome::RebootRequired);
    }

    #[test]
    fn failed_recovery_keeps_data_claim_only_before_purge() {
        let mut state = Lifecycle::default();
        state.action_failed("RecoverAgentState");
        state.action("Rollback");
        assert_eq!(state.finish(1603), Outcome::NetworkFailed);
        state.action("PurgeUserData");
        assert_eq!(state.finish(1603), Outcome::DataMayBeDeleted);
    }

    #[test]
    fn msi_can_withdraw_cancellation_before_the_next_callback() {
        let mut state = Lifecycle::default();
        state.common_data(2, 1);
        assert!(state.request_cancel());
        assert!(state.cancel_at_callback());
        state.common_data(2, 0);
        assert!(!state.cancel_at_callback());
    }

    #[test]
    fn cancelling_a_files_in_use_prompt_is_not_permission_denial() {
        let mut state = Lifecycle::default();
        state.user_declined_prompt();
        assert_eq!(state.finish(1602), Outcome::Cancelled);
    }

    #[test]
    fn progress_handles_script_generation_additions_and_reverse_execution() {
        let mut progress = Progress::default();
        progress.record([2, 50, 0, 0]);
        assert_eq!(progress.percent(), None);
        progress.record([0, 100, 0, 1]);
        progress.record([2, 50, 0, 0]);
        assert_eq!(progress.percent(), None);
        progress.record([0, 100, 0, 0]);
        progress.record([1, 10, 1, 0]);
        progress.action_data();
        assert_eq!(progress.percent(), Some(10));
        progress.record([3, 100, 0, 0]);
        assert_eq!(progress.percent(), Some(5));
        progress.record([0, 100, 1, 0]);
        progress.record([2, 25, 0, 0]);
        assert_eq!(progress.percent(), Some(75));
        progress.record([2, -1, 0, 0]);
        assert_eq!(progress.percent(), Some(75));
    }

    #[test]
    fn cancel_is_a_request_and_permission_denial_can_return_to_confirmation() {
        let mut state = Lifecycle::default();
        assert_eq!(state.finish(1602), Outcome::PermissionDenied);
        state.common_data(2, 1);
        assert!(state.request_cancel());
        assert!(!state.can_cancel());
        assert_eq!(state.finish(1602), Outcome::Cancelled);
        assert_eq!(state.finish(1603), Outcome::MsiFailed);
        assert!(Outcome::PermissionDenied.can_return_to_confirmation());
        assert!(!Outcome::RegistrationFailed.can_return_to_confirmation());
    }
}
