use std::{net::SocketAddr, sync::Arc, time::Duration};

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;
use usque_core::{
    DiagnosticCategory, DiagnosticCheckStatus as Status, DiagnosticFinding, DiagnosticMode,
    DirectDnsSettings,
};
use usque_transport::{MasqueTlsIdentity, NetworkProbeResult, SocketProtector};

use super::{
    checks::{DiagnosticCheck, DiagnosticContext},
    quality::finding,
};

/// Kept only for one Deep session. No Debug/Serialize: the profile and TLS
/// identity must never enter events or exports. A disconnected lifecycle guard
/// prevents a concurrent connect from changing the probe's protection contract.
pub(crate) struct DiagnosticProbeContext {
    pub settings: DirectDnsSettings,
    pub protector: Arc<dyn SocketProtector>,
    pub runtime_cancel: CancellationToken,
    pub h3: Option<(Vec<SocketAddr>, String, MasqueTlsIdentity)>,
    pub _lifecycle: Option<tokio::sync::OwnedMutexGuard<()>>,
}

pub(super) struct DeepCheck {
    pub h3: bool,
}

#[async_trait]
impl DiagnosticCheck for DeepCheck {
    fn id(&self) -> &'static str {
        if self.h3 {
            "transport.h3_path_validation_probe"
        } else {
            "dns.direct_encrypted_reachability"
        }
    }
    fn category(&self) -> DiagnosticCategory {
        if self.h3 {
            DiagnosticCategory::Transport
        } else {
            DiagnosticCategory::Protection
        }
    }
    fn dependencies(&self) -> &'static [&'static str] {
        if self.h3 {
            &["engine.configuration"]
        } else {
            &["dns.direct_encrypted_configuration"]
        }
    }
    fn minimum_mode(&self) -> DiagnosticMode {
        DiagnosticMode::Deep
    }
    fn resource_group(&self) -> &'static str {
        "network_probe"
    }
    fn timeout(&self) -> Duration {
        Duration::from_secs(4)
    }
    async fn run(
        &self,
        context: &DiagnosticContext,
        cancellation: CancellationToken,
    ) -> DiagnosticFinding {
        let Some(probes) = &context.probes else {
            return finding(
                self,
                Status::Skipped,
                "nq_finding_probe_unsafe",
                "none",
                vec![],
            );
        };
        let stop = probes.runtime_cancel.child_token();
        let _drop_guard = stop.clone().drop_guard();
        let operation = async {
            if self.h3 {
                match &probes.h3 {
                    Some((endpoints, sni, identity)) => {
                        usque_transport::probe_h3_handshake_candidates(
                            endpoints,
                            sni,
                            identity,
                            Arc::clone(&probes.protector),
                            stop.clone(),
                        )
                        .await
                    }
                    None => NetworkProbeResult::NotApplicable,
                }
            } else {
                usque_transport::probe_encrypted_dns(
                    &probes.settings,
                    Arc::clone(&probes.protector),
                    stop.clone(),
                )
                .await
            }
        };
        tokio::pin!(operation);
        let outcome = tokio::select! {
            biased;
            _ = cancellation.cancelled() => {
                stop.cancel();
                // Let the DNS driver/lease cleanup finish before reporting cancellation.
                let _ = tokio::time::timeout(Duration::from_millis(100), &mut operation).await;
                NetworkProbeResult::Cancelled
            },
            result = &mut operation => result,
        };
        match outcome {
            NetworkProbeResult::Passed { milliseconds } => finding(
                self,
                Status::Passed,
                "nq_finding_probe_success",
                "none",
                vec![format!("probe_ms={milliseconds}")],
            ),
            NetworkProbeResult::NotApplicable => finding(
                self,
                Status::Skipped,
                "nq_finding_probe_unsafe",
                "none",
                vec![],
            ),
            NetworkProbeResult::Cancelled => finding(
                self,
                Status::Cancelled,
                "nq_finding_probe_cancelled",
                "none",
                vec![],
            ),
            NetworkProbeResult::TimedOut => finding(
                self,
                Status::Warning,
                "nq_finding_probe_timeout",
                "nq_retry",
                vec![],
            ),
            NetworkProbeResult::NetworkChanged => finding(
                self,
                Status::Warning,
                "nq_finding_stale",
                "nq_retry",
                vec![],
            ),
            NetworkProbeResult::Failed => finding(
                self,
                Status::Failed,
                "nq_finding_probe_failed",
                "nq_network",
                vec![],
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deep_dns_uses_the_same_protection_category_as_android() {
        assert_eq!(
            DeepCheck { h3: false }.category(),
            DiagnosticCategory::Protection
        );
        assert_eq!(
            DeepCheck { h3: true }.category(),
            DiagnosticCategory::Transport
        );
    }
}
