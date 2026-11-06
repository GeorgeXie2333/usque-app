//! Pure presentation state. No registry, file deletion, or installer calls.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Preview {
    #[default]
    Confirm,
    Progress,
    /// Replays [`PREVIEW_UNINSTALL`] through the real presentation model.
    Replay,
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
            "replay" => Self::Replay,
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

/// End of MSI script generation, in permille of the presented operation.
pub const GENERATION_END: u16 = 100;
/// End of MSI script execution; Burn registration cleanup uses the rest.
pub const EXECUTION_END: u16 = 900;
/// The range of the presented bar.
pub const BAR_COMPLETE: u16 = 1000;

/// What the progress bar presents. Raw MSI totals never reach the control.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bar {
    /// No MSI progress has arrived yet (verification, elevation, costing).
    Waiting,
    /// Permille of the whole uninstall. It never decreases within an attempt.
    Advancing(u16),
    /// Rollback is undoing changes. The last forward value stays frozen.
    Halted(u16),
}

/// Numeric MSI progress counters for exported details. They contain no
/// record text, product data or paths.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProgressStats {
    pub generation_resets: u16,
    pub execution_resets: u16,
    pub rollback_resets: u16,
    pub additions: u16,
    /// MSI's own count moved backward and the bar held its value instead.
    pub held_reversals: u16,
}

/// Tracks MSI's own progress units and maps them onto one forward-only bar.
///
/// MSI restarts its count for every progress session (script generation,
/// each script execution, rollback) and may enlarge a session's total while
/// it runs. Presenting those raw fractions directly moved the bar back and
/// forth. Each forward session instead fills the remaining part of a fixed
/// phase range, and the presented value only ever increases. A generation
/// phase is not execution proof.
#[derive(Clone, Debug, Default)]
pub struct Progress {
    total: i64,
    current: i64,
    backward: bool,
    action_ticks: i64,
    initialized: bool,
    pub generating: bool,
    executing: bool,
    segment: (u16, u16),
    shown: u16,
    pub stats: ProgressStats,
}

impl Progress {
    pub fn record(&mut self, fields: [i32; 4]) {
        let [kind, value, flag, script] = fields;
        let before = self.session_permille();
        let mut restarted = false;
        match kind {
            0 if value >= 0 && matches!(flag, 0 | 1) && matches!(script, 0 | 1) => {
                // Only a repeated execution session would have restarted the
                // old bar; generation is followed by execution by design.
                restarted = self.executing && !self.backward && before > 0;
                self.total = i64::from(value);
                self.backward = flag == 1;
                self.current = if self.backward { self.total } else { 0 };
                self.generating = script == 1;
                self.action_ticks = 0;
                self.initialized = true;
                let stats = &mut self.stats;
                if self.backward {
                    stats.rollback_resets = stats.rollback_resets.saturating_add(1);
                } else {
                    let counter = if self.generating {
                        &mut stats.generation_resets
                    } else {
                        &mut stats.execution_resets
                    };
                    *counter = counter.saturating_add(1);
                    // Generation after execution has begun (or any later
                    // session) shares the execution range; it never restarts
                    // the bar.
                    let (start, end) = if self.generating && !self.executing {
                        (0, GENERATION_END)
                    } else {
                        (GENERATION_END, EXECUTION_END)
                    };
                    self.executing |= !self.generating;
                    self.segment = (self.shown.max(start), end.max(self.shown));
                }
            }
            1 if self.initialized && value >= 0 => {
                self.action_ticks = if flag == 1 { i64::from(value) } else { 0 };
            }
            2 if self.initialized && value >= 0 => self.advance(i64::from(value)),
            3 if self.initialized && value >= 0 => {
                self.total = self.total.saturating_add(i64::from(value));
                self.stats.additions = self.stats.additions.saturating_add(1);
            }
            _ => {}
        }
        let reversed = if kind == 0 {
            restarted
        } else {
            self.session_permille() < before
        };
        if reversed && !self.backward {
            self.stats.held_reversals = self.stats.held_reversals.saturating_add(1);
        }
        self.present();
    }

    pub fn action_data(&mut self) {
        self.advance(self.action_ticks);
        self.present();
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

    fn session_permille(&self) -> i64 {
        if self.total > 0 {
            self.current.saturating_mul(i64::from(BAR_COMPLETE)) / self.total
        } else {
            0
        }
    }

    fn present(&mut self) {
        // Rollback freezes the bar; it is presented as stopped, not reversed.
        if !self.initialized || self.backward {
            return;
        }
        let (start, end) = self.segment;
        let span = i64::from(end.saturating_sub(start));
        let position = i64::from(start) + span * self.session_permille() / i64::from(BAR_COMPLETE);
        self.shown = self
            .shown
            .max(position.clamp(0, i64::from(EXECUTION_END)) as u16);
    }

    /// Whether any MSI progress session has started.
    pub fn started(&self) -> bool {
        self.initialized
    }

    /// The forward-only presented value, in permille.
    pub fn shown(&self) -> u16 {
        self.shown
    }
}

/// One message of the inert preview's simulated uninstall.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreviewStep {
    Record([i32; 4]),
    Action(&'static str),
    /// `PurgeUserData`, only when deletion was selected.
    Purge,
    ActionData,
    CancelAllowed(bool),
    Registration,
    Finish,
}

/// `(timer tick, step)` pairs at the preview's 100 ms timer. The sequence is
/// shaped like an MSI uninstall and includes the record patterns that used to
/// move the bar backward: a progress addition during execution and a second
/// execution session. Only the inert preview and unit tests replay it.
pub const PREVIEW_UNINSTALL: &[(u32, PreviewStep)] = {
    use PreviewStep::*;
    const STEP: PreviewStep = Record([2, 4, 0, 0]);
    const LATE: PreviewStep = Record([2, 12, 0, 0]);
    &[
        (5, CancelAllowed(true)),
        (5, Action("InstallValidate")),
        (6, Record([0, 40, 0, 1])),
        (7, STEP),
        (8, STEP),
        (9, STEP),
        (10, STEP),
        (11, STEP),
        (12, STEP),
        (13, STEP),
        (14, STEP),
        (15, STEP),
        (16, STEP),
        (18, Record([0, 300, 0, 0])),
        (19, Action("StopServices")),
        (19, Record([1, 10, 1, 0])),
        (20, ActionData),
        (21, ActionData),
        (22, ActionData),
        (23, ActionData),
        (24, ActionData),
        (25, Action("EmergencyRemoveKillSwitch")),
        (25, Record([2, 20, 0, 0])),
        (27, Action("RecoverAgentState")),
        (30, Record([2, 10, 0, 0])),
        (33, Record([2, 10, 0, 0])),
        (34, Record([3, 120, 0, 0])),
        (35, Purge),
        (36, Action("RemoveFiles")),
        (36, Record([1, 15, 1, 0])),
        (37, ActionData),
        (38, ActionData),
        (39, ActionData),
        (40, ActionData),
        (41, ActionData),
        (42, ActionData),
        (43, ActionData),
        (44, ActionData),
        (45, ActionData),
        (46, ActionData),
        (47, Record([0, 60, 0, 0])),
        (48, LATE),
        (49, LATE),
        (50, CancelAllowed(false)),
        (50, LATE),
        (51, LATE),
        (52, LATE),
        (53, Action("FinalizeAgentUninstall")),
        (55, Registration),
        (66, Finish),
    ]
};

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

    /// The presented progress for the running page.
    pub fn bar(&self) -> Bar {
        let shown = self.progress.shown();
        if self.rollback_started {
            Bar::Halted(shown)
        } else if self.stage == Stage::Registration {
            // MSI has committed; Burn's cleanup reports no measurable units.
            Bar::Advancing(shown.max(EXECUTION_END))
        } else if self.progress.started() {
            Bar::Advancing(shown)
        } else {
            Bar::Waiting
        }
    }

    /// Applies one step of the inert preview's simulated uninstall.
    pub fn apply_preview_step(&mut self, step: PreviewStep, remove_user_data: bool) {
        match step {
            PreviewStep::Record(fields) => self.progress_record(fields),
            PreviewStep::Action(name) => self.action(name),
            PreviewStep::Purge if remove_user_data => self.action("PurgeUserData"),
            PreviewStep::ActionData => self.progress.action_data(),
            PreviewStep::CancelAllowed(allowed) => self.common_data(2, i32::from(allowed)),
            PreviewStep::Registration => self.stage = Stage::Registration,
            PreviewStep::Purge | PreviewStep::Finish => {}
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
        assert_eq!(state.bar(), Bar::Advancing(EXECUTION_END));
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
    fn progress_maps_generation_additions_and_rollback_without_moving_back() {
        let mut progress = Progress::default();
        progress.record([2, 50, 0, 0]);
        assert!(!progress.started());
        assert_eq!(progress.shown(), 0);
        progress.record([0, 100, 0, 1]);
        progress.record([2, 50, 0, 0]);
        assert_eq!(progress.shown(), GENERATION_END / 2);
        // Execution restarts MSI's count; the bar continues from generation.
        progress.record([0, 100, 0, 0]);
        assert_eq!(progress.shown(), GENERATION_END);
        progress.record([1, 10, 1, 0]);
        progress.action_data();
        assert_eq!(progress.shown(), 180);
        // A larger total halves MSI's fraction. The bar holds instead.
        progress.record([3, 100, 0, 0]);
        assert_eq!(progress.shown(), 180);
        progress.record([2, 5, 0, 0]);
        assert_eq!(progress.shown(), 180);
        progress.record([2, 35, 0, 0]);
        assert_eq!(progress.shown(), 300);
        // Rollback counts down from its own total; the bar stays frozen.
        progress.record([0, 100, 1, 0]);
        progress.record([2, 25, 0, 0]);
        progress.record([2, -1, 0, 0]);
        assert_eq!(progress.shown(), 300);
        assert_eq!(
            progress.stats,
            ProgressStats {
                generation_resets: 1,
                execution_resets: 1,
                rollback_resets: 1,
                additions: 1,
                held_reversals: 1,
            }
        );
    }

    #[test]
    fn bar_waits_for_msi_then_never_returns_to_an_indeterminate_state() {
        let mut state = Lifecycle::default();
        assert_eq!(state.bar(), Bar::Waiting);
        state.action("InstallValidate");
        assert_eq!(state.bar(), Bar::Waiting);
        state.progress_record([0, 0, 0, 1]);
        assert_eq!(state.bar(), Bar::Advancing(0));
        state.progress_record([0, 0, 0, 0]);
        assert_eq!(state.bar(), Bar::Advancing(GENERATION_END));
        state.stage = Stage::Registration;
        assert_eq!(state.bar(), Bar::Advancing(EXECUTION_END));
        // A registration-only retry starts from committed MSI work.
        let retry = Lifecycle {
            stage: Stage::Registration,
            ..Lifecycle::default()
        };
        assert_eq!(retry.bar(), Bar::Advancing(EXECUTION_END));
    }

    #[test]
    fn rollback_halts_the_bar_at_its_last_forward_value() {
        let mut state = Lifecycle::default();
        state.progress_record([0, 200, 0, 0]);
        state.progress_record([2, 100, 0, 0]);
        let Bar::Advancing(value) = state.bar() else {
            panic!("execution must be determinate");
        };
        state.action("Rollback");
        assert_eq!(state.bar(), Bar::Halted(value));
        state.progress_record([0, 100, 1, 0]);
        state.progress_record([2, 90, 0, 0]);
        state.progress.action_data();
        assert_eq!(state.bar(), Bar::Halted(value));
    }

    #[test]
    fn preview_uninstall_replays_former_reversals_as_a_forward_only_bar() {
        for remove_user_data in [false, true] {
            let mut state = Lifecycle::default();
            let mut previous = 0;
            let mut started = false;
            let mut finished = false;
            let last_tick = PREVIEW_UNINSTALL.last().unwrap().0;
            for tick in 1..=last_tick {
                for (_, step) in PREVIEW_UNINSTALL.iter().filter(|(at, _)| *at == tick) {
                    state.apply_preview_step(*step, remove_user_data);
                    finished |= *step == PreviewStep::Finish;
                }
                match state.bar() {
                    Bar::Waiting => assert!(!started, "tick {tick} returned to waiting"),
                    Bar::Advancing(value) => {
                        started = true;
                        assert!(value >= previous, "tick {tick}: {value} < {previous}");
                        assert!(value <= BAR_COMPLETE);
                        previous = value;
                    }
                    Bar::Halted(_) => panic!("the preview never rolls back"),
                }
            }
            assert!(finished);
            assert_eq!(state.stage, Stage::Registration);
            assert_eq!(state.bar(), Bar::Advancing(EXECUTION_END));
            assert_eq!(state.purge_started, remove_user_data);
            // The addition and the second execution session both moved the
            // former raw bar backward; both are now held.
            assert_eq!(state.progress.stats.held_reversals, 2);
            assert_eq!(state.progress.stats.execution_resets, 2);
        }
    }

    #[test]
    fn arbitrary_forward_records_never_move_the_bar_backward() {
        // Deterministic LCG; no randomness or dependency in the test build.
        let mut seed = 0x2545_f491_u32;
        let mut next = |bound: u32| {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 8) % bound
        };
        for _ in 0..200 {
            let mut state = Lifecycle::default();
            let mut previous = 0;
            for _ in 0..400 {
                let kind = next(5) as i32;
                if kind == 4 {
                    state.progress.action_data();
                } else {
                    let value = next(400) as i32 - 20;
                    let flag = if kind == 1 { next(2) as i32 } else { 0 };
                    state.progress_record([kind, value, flag, next(2) as i32]);
                }
                if let Bar::Advancing(value) = state.bar() {
                    assert!(value >= previous && value <= EXECUTION_END);
                    previous = value;
                }
            }
        }
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
