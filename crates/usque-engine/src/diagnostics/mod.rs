mod catalog;
mod checks;
mod report;
mod runner;

use std::sync::Arc;

use chrono::Utc;
use thiserror::Error;
use tokio::sync::Mutex;
#[cfg(any(windows, test))]
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use usque_core::{
    DiagnosticCheckStatus, DiagnosticFinding, DiagnosticMode, DiagnosticSession,
    DiagnosticSessionState,
};

pub(crate) use checks::DiagnosticContext;
#[cfg(any(windows, test))]
pub(crate) use report::finding_to_proto;
pub(crate) use report::{empty_session_to_proto, session_to_proto, timeline_to_proto};

use catalog::diagnostic_catalog;
use checks::{DiagnosticCheck, pending_finding};

#[cfg(any(windows, test))]
#[derive(Debug, Clone)]
pub(crate) enum DiagnosticEvent {
    SessionStarted(DiagnosticSession),
    CheckStarted {
        session_id: uuid::Uuid,
        finding: DiagnosticFinding,
    },
    CheckCompleted {
        session_id: uuid::Uuid,
        finding: DiagnosticFinding,
    },
    SessionCompleted(DiagnosticSession),
    SessionCancelled(DiagnosticSession),
}

#[derive(Clone)]
pub(crate) struct DiagnosticsManager {
    inner: Arc<Mutex<DiagnosticsState>>,
    // Only Windows exposes the live event stream; tests exercise it on hosts.
    #[cfg(any(windows, test))]
    events: broadcast::Sender<DiagnosticEvent>,
}

struct DiagnosticsState {
    session: Option<DiagnosticSession>,
    cancellation: Option<CancellationToken>,
}

impl Default for DiagnosticsManager {
    fn default() -> Self {
        Self::new()
    }
}

impl DiagnosticsManager {
    pub(crate) fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(DiagnosticsState {
                session: None,
                cancellation: None,
            })),
            #[cfg(any(windows, test))]
            events: broadcast::channel(128).0,
        }
    }

    pub(crate) async fn start(
        &self,
        mode: DiagnosticMode,
        context: DiagnosticContext,
    ) -> Result<DiagnosticSession, DiagnosticsError> {
        let checks = diagnostic_catalog();
        self.start_with_checks(mode, context, checks).await
    }

    async fn start_with_checks(
        &self,
        mode: DiagnosticMode,
        context: DiagnosticContext,
        checks: Vec<Arc<dyn DiagnosticCheck>>,
    ) -> Result<DiagnosticSession, DiagnosticsError> {
        let cancellation = CancellationToken::new();
        let session = {
            let mut state = self.inner.lock().await;
            if state
                .session
                .as_ref()
                .is_some_and(|session| session.state.is_active())
            {
                return Err(DiagnosticsError::AlreadyRunning);
            }
            let findings = checks.iter().map(pending_finding).collect();
            let mut session = DiagnosticSession::pending(mode, findings);
            session.state = DiagnosticSessionState::Running;
            state.session = Some(session.clone());
            state.cancellation = Some(cancellation.clone());
            session
        };
        #[cfg(any(windows, test))]
        let _ = self
            .events
            .send(DiagnosticEvent::SessionStarted(session.clone()));

        let manager = self.clone();
        tokio::spawn(async move {
            runner::run(manager, checks, Arc::new(context), cancellation).await;
        });
        Ok(session)
    }

    pub(crate) async fn cancel(
        &self,
        requested_session_id: Option<uuid::Uuid>,
    ) -> Result<DiagnosticSession, DiagnosticsError> {
        let (session, cancellation) = {
            let mut state = self.inner.lock().await;
            let session = state.session.as_mut().ok_or(DiagnosticsError::NotStarted)?;
            if let Some(requested) = requested_session_id
                && requested != session.session_id
            {
                return Err(DiagnosticsError::SessionMismatch);
            }
            if !session.state.is_active() {
                return Ok(session.clone());
            }
            session.state = DiagnosticSessionState::Cancelling;
            (session.clone(), state.cancellation.clone())
        };
        if let Some(cancellation) = cancellation {
            cancellation.cancel();
        }
        Ok(session)
    }

    pub(crate) async fn get(&self) -> Option<DiagnosticSession> {
        self.inner.lock().await.session.clone()
    }

    #[cfg(any(windows, test))]
    pub(crate) fn subscribe(&self) -> broadcast::Receiver<DiagnosticEvent> {
        self.events.subscribe()
    }

    pub(super) async fn session_snapshot(&self) -> Option<DiagnosticSession> {
        self.get().await
    }

    pub(super) async fn check_started(&self, check_id: &str) {
        {
            let mut state = self.inner.lock().await;
            let Some(session) = state.session.as_mut() else {
                return;
            };
            let Some(finding) = session
                .findings
                .iter_mut()
                .find(|finding| finding.check_id == check_id)
            else {
                return;
            };
            finding.status = DiagnosticCheckStatus::Running;
            finding.started_at = Some(Utc::now());
            session.current_check = Some(check_id.to_owned());
            #[cfg(any(windows, test))]
            let _ = self.events.send(DiagnosticEvent::CheckStarted {
                session_id: session.session_id,
                finding: finding.clone(),
            });
        }
    }

    pub(super) async fn check_completed(&self, mut completed: DiagnosticFinding) {
        {
            let mut state = self.inner.lock().await;
            let Some(session) = state.session.as_mut() else {
                return;
            };
            let Some(finding) = session
                .findings
                .iter_mut()
                .find(|finding| finding.check_id == completed.check_id)
            else {
                return;
            };
            if completed.started_at.is_none() {
                completed.started_at = finding.started_at;
            }
            *finding = completed.clone();
            session.current_check = None;
            session.recompute_summary();
            #[cfg(any(windows, test))]
            let _ = self.events.send(DiagnosticEvent::CheckCompleted {
                session_id: session.session_id,
                finding: completed,
            });
        }
    }

    pub(super) async fn finish(&self, cancelled: bool) {
        let session = {
            let mut state = self.inner.lock().await;
            let Some(session) = state.session.as_mut() else {
                return;
            };
            if cancelled {
                for finding in &mut session.findings {
                    if matches!(
                        finding.status,
                        DiagnosticCheckStatus::Pending | DiagnosticCheckStatus::Running
                    ) {
                        finding.status = DiagnosticCheckStatus::Cancelled;
                    }
                }
                session.state = DiagnosticSessionState::Cancelled;
            } else {
                session.state = DiagnosticSessionState::Completed;
            }
            session.current_check = None;
            session.completed_at = Some(Utc::now());
            session.recompute_summary();
            let session = session.clone();
            state.cancellation = None;
            session
        };
        #[cfg(any(windows, test))]
        let _ = self.events.send(if cancelled {
            DiagnosticEvent::SessionCancelled(session.clone())
        } else {
            DiagnosticEvent::SessionCompleted(session.clone())
        });
        tracing::debug!(
            session_id = %session.session_id,
            state = ?session.state,
            "diagnostic session finished"
        );
    }
}

#[derive(Debug, Error)]
pub enum DiagnosticsError {
    #[error("a diagnostic session is already running")]
    AlreadyRunning,
    #[error("no diagnostic session has been started")]
    NotStarted,
    #[error("the requested diagnostic session does not match the active session")]
    SessionMismatch,
}

#[cfg(test)]
mod tests {
    use std::{future::pending, time::Duration};

    use async_trait::async_trait;
    use usque_core::{
        ConnectionSnapshot, DiagnosticCategory, DiagnosticCheckStatus, DiagnosticFinding,
        DiagnosticMode, DiagnosticSessionState,
    };
    use usque_transport::ConnectionTimelineSnapshot;

    use super::*;

    #[derive(Clone, Copy)]
    enum Behavior {
        Pass,
        Fail,
        WaitForCancellation,
        Never,
    }

    struct TestCheck {
        id: &'static str,
        dependencies: &'static [&'static str],
        behavior: Behavior,
        timeout: Duration,
    }

    #[async_trait]
    impl DiagnosticCheck for TestCheck {
        fn id(&self) -> &'static str {
            self.id
        }

        fn category(&self) -> DiagnosticCategory {
            DiagnosticCategory::LocalComponent
        }

        fn dependencies(&self) -> &'static [&'static str] {
            self.dependencies
        }

        fn minimum_mode(&self) -> DiagnosticMode {
            DiagnosticMode::Standard
        }

        fn resource_group(&self) -> &'static str {
            self.id
        }

        fn timeout(&self) -> Duration {
            self.timeout
        }

        async fn run(
            &self,
            _context: &DiagnosticContext,
            cancellation: CancellationToken,
        ) -> DiagnosticFinding {
            let mut finding = DiagnosticFinding::pending(self.id, self.category());
            finding.status = match self.behavior {
                Behavior::Pass => DiagnosticCheckStatus::Passed,
                Behavior::Fail => DiagnosticCheckStatus::Failed,
                Behavior::WaitForCancellation => {
                    cancellation.cancelled().await;
                    DiagnosticCheckStatus::Cancelled
                }
                Behavior::Never => {
                    pending::<()>().await;
                    unreachable!()
                }
            };
            finding
        }
    }

    fn context() -> DiagnosticContext {
        DiagnosticContext {
            connection: ConnectionSnapshot::default(),
            configuration_valid: true,
            secure_storage_available: true,
            kill_switch_expected: false,
            tunnel_dns_expected: false,
            system_proxy_expected: false,
            operating_system: "test".to_owned(),
            timeline: ConnectionTimelineSnapshot::default(),
            platform_state: None,
        }
    }

    async fn wait_terminal(manager: &DiagnosticsManager) -> DiagnosticSession {
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if let Some(session) = manager.get().await
                    && !session.state.is_active()
                {
                    return session;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("diagnostic session should terminate")
    }

    #[tokio::test]
    async fn inv_diagnostics_read_only_duplicate_start_is_rejected() {
        let manager = DiagnosticsManager::new();
        let checks: Vec<Arc<dyn DiagnosticCheck>> = vec![Arc::new(TestCheck {
            id: "blocking",
            dependencies: &[],
            behavior: Behavior::WaitForCancellation,
            timeout: Duration::from_secs(10),
        })];
        let started = manager
            .start_with_checks(DiagnosticMode::Standard, context(), checks.clone())
            .await
            .expect("first session");
        assert!(matches!(
            manager
                .start_with_checks(DiagnosticMode::Standard, context(), checks)
                .await,
            Err(DiagnosticsError::AlreadyRunning)
        ));
        manager.cancel(Some(started.session_id)).await.unwrap();
        assert_eq!(
            wait_terminal(&manager).await.state,
            DiagnosticSessionState::Cancelled
        );
    }

    #[tokio::test]
    async fn diagnostic_timeout_has_a_stable_terminal_finding() {
        let manager = DiagnosticsManager::new();
        manager
            .start_with_checks(
                DiagnosticMode::Standard,
                context(),
                vec![Arc::new(TestCheck {
                    id: "timeout",
                    dependencies: &[],
                    behavior: Behavior::Never,
                    timeout: Duration::from_millis(5),
                })],
            )
            .await
            .unwrap();
        let session = wait_terminal(&manager).await;
        assert_eq!(session.state, DiagnosticSessionState::Completed);
        assert_eq!(session.findings[0].status, DiagnosticCheckStatus::Failed);
        assert_eq!(
            session.findings[0]
                .failure
                .as_ref()
                .map(|failure| failure.code),
            Some(usque_core::TransportFailureCode::DiagnosticTimeout)
        );
    }

    #[tokio::test]
    async fn failed_dependency_is_skipped_with_the_dependency_id() {
        let manager = DiagnosticsManager::new();
        manager
            .start_with_checks(
                DiagnosticMode::Standard,
                context(),
                vec![
                    Arc::new(TestCheck {
                        id: "parent",
                        dependencies: &[],
                        behavior: Behavior::Fail,
                        timeout: Duration::from_secs(1),
                    }),
                    Arc::new(TestCheck {
                        id: "child",
                        dependencies: &["parent"],
                        behavior: Behavior::Pass,
                        timeout: Duration::from_secs(1),
                    }),
                ],
            )
            .await
            .unwrap();
        let session = wait_terminal(&manager).await;
        let child = session
            .findings
            .iter()
            .find(|finding| finding.check_id == "child")
            .unwrap();
        assert_eq!(child.status, DiagnosticCheckStatus::Skipped);
        assert_eq!(child.dependency_reason.as_deref(), Some("parent"));
    }
}
