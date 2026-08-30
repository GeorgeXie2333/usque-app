use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

use tokio::task::JoinSet;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;
use usque_core::{DiagnosticCheckStatus, DiagnosticMode};

use super::DiagnosticsManager;
use super::checks::{
    DiagnosticCheck, DiagnosticContext, cancelled_finding, dependency_skipped_finding,
    internal_finding, mode_skipped_finding, timed_out_finding,
};

const MAX_CONCURRENT_CHECKS: usize = 4;

pub(super) async fn run(
    manager: DiagnosticsManager,
    checks: Vec<Arc<dyn DiagnosticCheck>>,
    context: Arc<DiagnosticContext>,
    cancellation: CancellationToken,
) {
    loop {
        if cancellation.is_cancelled() {
            manager.finish(true).await;
            return;
        }
        let Some(session) = manager.session_snapshot().await else {
            return;
        };
        let statuses: HashMap<&str, DiagnosticCheckStatus> = session
            .findings
            .iter()
            .map(|finding| (finding.check_id.as_str(), finding.status))
            .collect();
        let pending: Vec<_> = checks
            .iter()
            .filter(|check| statuses.get(check.id()) == Some(&DiagnosticCheckStatus::Pending))
            .cloned()
            .collect();
        if pending.is_empty() {
            manager.finish(false).await;
            return;
        }

        let mut made_progress = false;
        for check in &pending {
            if requires_deep(check.minimum_mode(), session.mode) {
                manager
                    .check_completed(mode_skipped_finding(check.as_ref()))
                    .await;
                made_progress = true;
                continue;
            }
            if let Some(dependency) = failed_dependency(check.as_ref(), &statuses) {
                manager
                    .check_completed(dependency_skipped_finding(check.as_ref(), dependency))
                    .await;
                made_progress = true;
            }
        }
        if made_progress {
            continue;
        }

        let mut groups = HashSet::new();
        let ready: Vec<_> = pending
            .into_iter()
            .filter(|check| dependencies_satisfied(check.as_ref(), &statuses))
            .filter(|check| groups.insert(check.resource_group()))
            .take(MAX_CONCURRENT_CHECKS)
            .collect();

        if ready.is_empty() {
            // A missing dependency or cycle is an internal catalogue error.
            // Terminate every remaining node rather than leaving the session
            // permanently running.
            for check in checks
                .iter()
                .filter(|check| statuses.get(check.id()) == Some(&DiagnosticCheckStatus::Pending))
            {
                manager
                    .check_completed(internal_finding(check.as_ref()))
                    .await;
            }
            manager.finish(false).await;
            return;
        }

        let mut tasks = JoinSet::new();
        for check in ready {
            manager.check_started(check.id()).await;
            let context = Arc::clone(&context);
            let cancellation = cancellation.clone();
            tasks.spawn(async move {
                let started = Instant::now();
                let result = tokio::select! {
                    _ = cancellation.cancelled() => cancelled_finding(check.as_ref()),
                    result = timeout(
                        check.timeout(),
                        check.run(context.as_ref(), cancellation.child_token()),
                    ) => match result {
                        Ok(finding) => finding,
                        Err(_) => timed_out_finding(check.as_ref()),
                    },
                };
                (check, result, started.elapsed())
            });
        }

        while let Some(result) = tasks.join_next().await {
            match result {
                Ok((check, mut finding, elapsed)) => {
                    finding.duration_milliseconds =
                        Some(elapsed.as_millis().min(u128::from(u64::MAX)) as u64);
                    manager.check_completed(finding).await;
                    drop(check);
                }
                Err(error) => {
                    tracing::warn!(%error, "diagnostic check task failed");
                }
            }
        }
    }
}

fn requires_deep(minimum: DiagnosticMode, selected: DiagnosticMode) -> bool {
    minimum == DiagnosticMode::Deep && selected == DiagnosticMode::Standard
}

fn failed_dependency<'a>(
    check: &'a dyn DiagnosticCheck,
    statuses: &HashMap<&str, DiagnosticCheckStatus>,
) -> Option<&'a str> {
    check.dependencies().iter().copied().find(|dependency| {
        statuses
            .get(dependency)
            .is_some_and(|status| status.is_terminal() && !status.satisfies_dependency())
    })
}

fn dependencies_satisfied(
    check: &dyn DiagnosticCheck,
    statuses: &HashMap<&str, DiagnosticCheckStatus>,
) -> bool {
    check.dependencies().iter().all(|dependency| {
        statuses
            .get(dependency)
            .is_some_and(|status| status.satisfies_dependency())
    })
}
