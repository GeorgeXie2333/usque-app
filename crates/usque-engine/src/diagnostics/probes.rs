use std::{sync::Arc, time::Duration};

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
    pub h3: Option<(usque_core::Profile, String, MasqueTlsIdentity)>,
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
                    Some((profile, sni, identity)) => {
                        tokio::time::timeout(usque_transport::NETWORK_PROBE_IO_TIMEOUT, async {
                            let endpoints = match usque_transport::h3_probe_endpoints_resolved(
                                profile,
                                identity,
                                Arc::clone(&probes.protector),
                                stop.clone(),
                            )
                            .await
                            {
                                Ok(endpoints) => endpoints,
                                Err(result) => return result,
                            };
                            endpoints
                                .probe(sni, identity, Arc::clone(&probes.protector), stop.clone())
                                .await
                        })
                        .await
                        .unwrap_or(NetworkProbeResult::TimedOut)
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
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

    struct PresetProbeProtector {
        generation: AtomicU64,
        protections: AtomicUsize,
        changed: bool,
        pending: bool,
    }

    #[async_trait]
    impl SocketProtector for PresetProbeProtector {
        fn protect(&self, _: usque_transport::SocketHandle) -> Result<(), String> {
            self.protections.fetch_add(1, Ordering::SeqCst);
            Err("unexpected_numeric_probe".into())
        }

        fn network_generation(&self) -> Option<u64> {
            Some(self.generation.load(Ordering::SeqCst))
        }

        async fn resolve_endpoint_preset(
            &self,
            host: &str,
            _: u16,
            _: u64,
        ) -> Result<Vec<std::net::SocketAddr>, String> {
            assert_eq!(host, "us.v6.free.orangesky.dev");
            if self.changed {
                self.generation.store(2, Ordering::SeqCst);
            }
            if self.pending {
                std::future::pending::<()>().await;
            }
            Err("fixture_dns_failed".into())
        }
    }

    fn preset_context(protector: Arc<PresetProbeProtector>) -> DiagnosticContext {
        let mut profile = usque_core::Profile::default();
        profile.endpoint.selection = usque_core::EndpointSelection::Custom;
        profile.endpoint.preset_region = Some(usque_core::EndpointPresetRegion::Us);
        profile.ip_policy = usque_core::IpPolicy::Ipv6Only;
        let identity = MasqueTlsIdentity::from_warp_identity(&crate::tests::test_identity(
            usque_core::IdentityProvider::Consumer,
            None,
        ))
        .unwrap();
        DiagnosticContext {
            connection: Default::default(),
            configuration_valid: true,
            secure_storage_available: true,
            kill_switch_expected: false,
            tunnel_dns_expected: false,
            system_proxy_expected: false,
            operating_system: "test".into(),
            timeline: Default::default(),
            platform_state: None,
            linux_platform: None,
            quality: crate::network_quality::disconnected_snapshot(),
            direct_dns: Default::default(),
            probes: Some(Arc::new(DiagnosticProbeContext {
                settings: Default::default(),
                protector,
                runtime_cancel: CancellationToken::new(),
                h3: Some((profile, "speed.cloudflare.com".into(), identity)),
                _lifecycle: None,
            })),
            captured_at: tokio::time::Instant::now(),
        }
    }

    #[tokio::test]
    async fn preset_dns_failure_and_network_change_never_probe_dormant_manual_addresses() {
        for changed in [false, true] {
            let protector = Arc::new(PresetProbeProtector {
                generation: AtomicU64::new(1),
                protections: AtomicUsize::new(0),
                changed,
                pending: false,
            });
            let context = preset_context(Arc::clone(&protector));
            let result = DeepCheck { h3: true }
                .run(&context, CancellationToken::new())
                .await;
            assert_eq!(
                result.status,
                if changed {
                    Status::Warning
                } else {
                    Status::Failed
                }
            );
            assert_eq!(
                result.summary_key,
                if changed {
                    "nq_finding_stale"
                } else {
                    "nq_finding_probe_failed"
                }
            );
            assert_eq!(protector.protections.load(Ordering::SeqCst), 0);
        }
    }

    #[tokio::test]
    async fn preset_discovery_is_cancelled_inside_the_deep_check() {
        let protector = Arc::new(PresetProbeProtector {
            generation: AtomicU64::new(1),
            protections: AtomicUsize::new(0),
            changed: false,
            pending: true,
        });
        let context = preset_context(Arc::clone(&protector));
        let cancellation = CancellationToken::new();
        let check = DeepCheck { h3: true };
        let run = check.run(&context, cancellation.clone());
        tokio::pin!(run);
        tokio::select! {
            _ = tokio::task::yield_now() => {},
            _ = &mut run => panic!("discovery should wait for cancellation"),
        }
        cancellation.cancel();
        assert_eq!(run.await.status, Status::Cancelled);
        assert_eq!(protector.protections.load(Ordering::SeqCst), 0);
    }

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
