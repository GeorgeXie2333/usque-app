use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use super::*;
use usque_transport::{ConnectionInstanceId, MetricValue};

#[tokio::test]
async fn standard_quality_checks_are_read_only_and_export_only_codes_and_numbers() {
    let mut context = context();
    context.quality.connection_id = Some(ConnectionInstanceId(uuid::Uuid::new_v4()));
    context.quality.transport = Some(usque_core::Transport::Http3);
    context.quality.rtt.smoothed = MetricValue::available(Duration::from_millis(160));
    context.quality.loss.interval_basis_points = MetricValue::available(220);
    context.quality.pmtu.current_bytes = MetricValue::available(1350);
    context.quality.pmtu.phase = usque_transport::PmtuPhase::Degraded;
    context.direct_dns = usque_core::DirectDnsSettings {
        mode: usque_core::DirectDnsMode::Doh,
        server_name: "private-resolver.example".to_owned(),
        doh_path: "/private-path".to_owned(),
        bootstrap_ips: vec!["192.0.2.53".parse().unwrap()],
        port: 443,
    };
    let before_quality = context.quality.clone();
    let before_settings = context.direct_dns.clone();
    let checks = diagnostic_catalog();
    let mut findings = Vec::new();
    for check in &checks {
        if check.id().starts_with("quality.")
            || check.id().starts_with("dns.direct_encrypted_")
                && check.minimum_mode() == DiagnosticMode::Standard
            || check.id() == "transport.migration_capability"
        {
            findings.push(check.run(&context, CancellationToken::new()).await);
        }
    }
    assert_eq!(findings.len(), 7);
    assert_eq!(
        findings
            .iter()
            .find(|finding| finding.check_id == "quality.rtt")
            .unwrap()
            .status,
        DiagnosticCheckStatus::Warning
    );
    assert_eq!(
        findings
            .iter()
            .find(|finding| finding.check_id == "quality.pmtu")
            .unwrap()
            .status,
        DiagnosticCheckStatus::Warning
    );
    assert_eq!(context.quality, before_quality);
    assert_eq!(context.direct_dns, before_settings);
    let serialized = serde_json::to_string(&findings).unwrap();
    for private in ["private-resolver.example", "/private-path", "192.0.2.53"] {
        assert!(!serialized.contains(private));
    }
    for evidence in findings
        .iter()
        .flat_map(|finding| &finding.sanitized_evidence)
    {
        let (_, value) = evidence.split_once('=').unwrap();
        assert!(value.parse::<u64>().is_ok());
    }
    context.quality.transport = Some(usque_core::Transport::Http2);
    for id in [
        "quality.packet_loss",
        "quality.pmtu",
        "transport.migration_capability",
    ] {
        let check = checks.iter().find(|check| check.id() == id).unwrap();
        assert_eq!(
            check.run(&context, CancellationToken::new()).await.status,
            DiagnosticCheckStatus::Skipped
        );
    }
    context.quality.sampled_at = tokio::time::Instant::now() - Duration::from_secs(4);
    let rtt = checks
        .iter()
        .find(|check| check.id() == "quality.rtt")
        .unwrap();
    assert_eq!(
        rtt.run(&context, CancellationToken::new())
            .await
            .summary_key,
        "nq_finding_stale"
    );
}

struct ResourceCheck {
    id: &'static str,
    group: &'static str,
    live: Arc<AtomicUsize>,
    peak: Arc<AtomicUsize>,
    cancelled: Arc<AtomicBool>,
    delay: Duration,
}
struct Live(Arc<AtomicUsize>);
impl Drop for Live {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}
#[async_trait]
impl DiagnosticCheck for ResourceCheck {
    fn id(&self) -> &'static str {
        self.id
    }
    fn category(&self) -> DiagnosticCategory {
        DiagnosticCategory::Transport
    }
    fn dependencies(&self) -> &'static [&'static str] {
        &[]
    }
    fn minimum_mode(&self) -> DiagnosticMode {
        DiagnosticMode::Standard
    }
    fn resource_group(&self) -> &'static str {
        self.group
    }
    fn timeout(&self) -> Duration {
        Duration::from_secs(4)
    }
    async fn run(
        &self,
        _: &DiagnosticContext,
        cancellation: CancellationToken,
    ) -> DiagnosticFinding {
        let active = self.live.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak.fetch_max(active, Ordering::SeqCst);
        let _live = Live(self.live.clone());
        let mut finding = DiagnosticFinding::pending(self.id, self.category());
        finding.status = tokio::select! {
            _ = cancellation.cancelled() => { self.cancelled.store(true, Ordering::SeqCst); DiagnosticCheckStatus::Cancelled },
            _ = tokio::time::sleep(self.delay) => DiagnosticCheckStatus::Passed,
        };
        finding
    }
}

#[tokio::test(start_paused = true)]
async fn deep_session_budget_and_resource_group_limit_include_cleanup() {
    let manager = DiagnosticsManager::new();
    let live = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let cancelled = Arc::new(AtomicBool::new(false));
    let checks = ["a", "b", "c", "d", "e"]
        .into_iter()
        .map(|id| {
            Arc::new(ResourceCheck {
                id,
                group: "network_probe",
                live: live.clone(),
                peak: peak.clone(),
                cancelled: cancelled.clone(),
                delay: Duration::from_millis(3500),
            }) as Arc<dyn DiagnosticCheck>
        })
        .collect();
    let start = tokio::time::Instant::now();
    manager
        .start_with_checks(DiagnosticMode::Deep, context(), checks)
        .await
        .unwrap();
    let session = tokio::time::timeout(Duration::from_secs(16), async {
        loop {
            let session = manager.get().await.unwrap();
            if !session.state.is_active() {
                return session;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(start.elapsed() <= Duration::from_secs(15));
    assert_eq!(peak.load(Ordering::SeqCst), 1);
    assert_eq!(live.load(Ordering::SeqCst), 0);
    assert!(cancelled.load(Ordering::SeqCst));
    assert_eq!(session.summary.failed, 1);
}

#[tokio::test]
async fn cancellation_is_terminal_only_after_a_check_releases_its_resources() {
    let manager = DiagnosticsManager::new();
    let live = Arc::new(AtomicUsize::new(0));
    let cancelled = Arc::new(AtomicBool::new(false));
    let check = ResourceCheck {
        id: "probe",
        group: "network_probe",
        live: live.clone(),
        peak: Arc::new(AtomicUsize::new(0)),
        cancelled: cancelled.clone(),
        delay: Duration::from_secs(60),
    };
    let session = manager
        .start_with_checks(DiagnosticMode::Deep, context(), vec![Arc::new(check)])
        .await
        .unwrap();
    while live.load(Ordering::SeqCst) == 0 {
        tokio::task::yield_now().await;
    }
    manager.cancel(Some(session.session_id)).await.unwrap();
    assert_eq!(
        wait_terminal(&manager).await.state,
        DiagnosticSessionState::Cancelled
    );
    assert_eq!(live.load(Ordering::SeqCst), 0);
    assert!(cancelled.load(Ordering::SeqCst));
}
