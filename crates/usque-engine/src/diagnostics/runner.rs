use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::task::JoinSet;
use tokio::time::{timeout, timeout_at};
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
    let mode = manager
        .session_snapshot()
        .await
        .map(|session| session.mode)
        .unwrap_or(DiagnosticMode::Standard);
    let deadline = context.captured_at
        + match mode {
            DiagnosticMode::Standard => Duration::from_secs(2),
            DiagnosticMode::Deep => Duration::from_secs(15),
        };
    loop {
        if cancellation.is_cancelled() {
            drop(context);
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
        if tokio::time::Instant::now() >= deadline {
            for check in &checks {
                if statuses
                    .get(check.id())
                    .is_some_and(|status| !status.is_terminal())
                {
                    manager
                        .check_completed(timed_out_finding(check.as_ref()))
                        .await;
                }
            }
            drop(context);
            manager.finish(false).await;
            return;
        }
        let pending: Vec<_> = checks
            .iter()
            .filter(|check| statuses.get(check.id()) == Some(&DiagnosticCheckStatus::Pending))
            .cloned()
            .collect();
        if pending.is_empty() {
            drop(context);
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
            drop(context);
            manager.finish(false).await;
            return;
        }

        let mut tasks = JoinSet::new();
        let mut running = HashMap::new();
        for check in ready {
            manager.check_started(check.id()).await;
            let context = Arc::clone(&context);
            let cancellation = cancellation.clone();
            let running_check = Arc::clone(&check);
            let handle = tasks.spawn(async move {
                let started = Instant::now();
                let child = cancellation.child_token();
                let _cancel_on_drop = child.clone().drop_guard();
                let end = (tokio::time::Instant::now() + check.timeout()).min(deadline);
                let cleanup_budget = Duration::from_millis(100).min(check.timeout() / 10);
                let operation = check.run(context.as_ref(), child.clone());
                tokio::pin!(operation);
                let (result, interrupted) = tokio::select! {
                    biased;
                    _ = cancellation.cancelled() => (cancelled_finding(check.as_ref()), true),
                    result = timeout_at(end - cleanup_budget, &mut operation) => match result {
                        Ok(finding) => (finding, false),
                        Err(_) => (timed_out_finding(check.as_ref()), true),
                    },
                };
                if interrupted {
                    child.cancel();
                    let _ = timeout(cleanup_budget, &mut operation).await;
                }
                (result, started.elapsed())
            });
            running.insert(handle.id(), running_check);
        }

        while let Some(result) = tasks.join_next_with_id().await {
            match result {
                Ok((id, (mut finding, elapsed))) => {
                    running.remove(&id);
                    finding.duration_milliseconds =
                        Some(elapsed.as_millis().min(u128::from(u64::MAX)) as u64);
                    manager.check_completed(finding).await;
                }
                Err(error) => {
                    if let Some(check) = running.remove(&error.id()) {
                        manager
                            .check_completed(internal_finding(check.as_ref()))
                            .await;
                    }
                    tracing::warn!("diagnostic check task failed");
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
