//! The command's confirmation follows successful publication, including on
//! hosts where JNI and Android TUN descriptors are unavailable to unit tests.
use std::sync::atomic::{AtomicBool, Ordering};
use usque_core::Profile;
use usque_transport::{DataPlaneRuntime, PreparedTrafficPolicyUpdate, TransportError};

pub(crate) trait RoutingRuntime {
    type Prepared;
    fn prepare(&self, profile: &Profile) -> Result<Self::Prepared, TransportError>;
    fn publish(&mut self, prepared: Self::Prepared) -> Result<(), TransportError>;
    fn update_knobs(&mut self, profile: &Profile);
}

impl RoutingRuntime for DataPlaneRuntime {
    type Prepared = PreparedTrafficPolicyUpdate;
    fn prepare(&self, profile: &Profile) -> Result<Self::Prepared, TransportError> {
        self.prepare_routing_update(profile)
    }
    fn publish(&mut self, prepared: Self::Prepared) -> Result<(), TransportError> {
        self.publish_routing_update(prepared)
    }
    fn update_knobs(&mut self, profile: &Profile) {
        self.update_ads_enabled(profile.routing.ads_enabled);
        self.update_traffic_policy(profile.disable_quic);
    }
}

pub(crate) fn apply<R: RoutingRuntime>(
    runtime: &mut R,
    confirmed: &mut Profile,
    next: Profile,
    cancelled: &AtomicBool,
) -> Result<(), TransportError> {
    if cancelled.load(Ordering::Acquire) {
        return Err(TransportError::TunnelClosed);
    }
    let prepared = runtime.prepare(&next)?;
    if cancelled.load(Ordering::Acquire) {
        return Err(TransportError::TunnelClosed);
    }
    runtime.publish(prepared)?;
    runtime.update_knobs(&next);
    *confirmed = next;
    Ok(())
}

pub(crate) fn attachment_rules_match(current: &Profile, next: &Profile) -> bool {
    current.routing.rules == next.routing.rules
}

pub(crate) fn record_rejection(snapshot: &mut crate::NativeSnapshot) {
    // This is an operation failure, not a transport failure. Preserve the
    // observed phase, final-network state and listeners of the old session.
    if matches!(snapshot.phase.as_str(), "connected" | "degraded") {
        snapshot.warning = Some("The routing update was not applied.".into());
        snapshot.error_code = Some("ROUTING_UPDATE_FAILED".into());
        snapshot.failure = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use usque_transport::{GeoDirectPolicy, GeoRoute, PreparedRoutingUpdate};

    struct Runtime<'a> {
        policy: GeoDirectPolicy,
        prepare_fails: bool,
        publish_fails: bool,
        cancel_during_prepare: Option<&'a AtomicBool>,
        publications: u32,
        disable_quic: bool,
    }
    impl RoutingRuntime for Runtime<'_> {
        type Prepared = PreparedRoutingUpdate;
        fn prepare(&self, profile: &Profile) -> Result<Self::Prepared, TransportError> {
            if self.prepare_fails {
                return Err(TransportError::Netstack("unavailable resource".into()));
            }
            let prepared = self
                .policy
                .prepare_routing_update(profile)
                .map_err(|error| TransportError::Netstack(error.to_string()))?;
            if let Some(cancel) = self.cancel_during_prepare {
                cancel.store(true, Ordering::Release);
            }
            Ok(prepared)
        }
        fn publish(&mut self, prepared: Self::Prepared) -> Result<(), TransportError> {
            self.publications += 1;
            if self.publish_fails {
                return Err(TransportError::Netstack("retired resource".into()));
            }
            self.policy
                .publish_routing_update(prepared)
                .map_err(|error| TransportError::Netstack(error.to_string()))
        }
        fn update_knobs(&mut self, profile: &Profile) {
            self.disable_quic = profile.disable_quic;
        }
    }

    fn target() -> Profile {
        let mut profile = Profile {
            disable_quic: true,
            ..Profile::default()
        };
        profile.routing.rules.push(usque_core::RoutingRule {
            id: uuid::Uuid::new_v4(),
            kind: usque_core::RoutingMatch::Domain,
            target: "blocked.test".into(),
            action: usque_core::RoutingAction::Reject,
        });
        profile
    }

    #[test]
    fn hot_command_only_confirms_successfully_published_rules_and_knobs() {
        for failure in [
            "none",
            "prepare",
            "publish",
            "cancel_before",
            "cancel_during",
        ] {
            let cancelled = AtomicBool::new(failure == "cancel_before");
            let mut runtime = Runtime {
                policy: GeoDirectPolicy::disabled(),
                prepare_fails: failure == "prepare",
                publish_fails: failure == "publish",
                publications: 0,
                disable_quic: false,
                cancel_during_prepare: (failure == "cancel_during").then_some(&cancelled),
            };
            let mut confirmed = Profile::default();
            let previous = confirmed.clone();
            let mut next = target();
            next.id = confirmed.id;
            let expected = next.clone();
            let result = apply(&mut runtime, &mut confirmed, next, &cancelled);
            if failure == "none" {
                result.unwrap();
                assert_eq!(confirmed, expected);
                assert_eq!(runtime.policy.route_host("blocked.test"), GeoRoute::Reject);
                assert!(runtime.disable_quic);
            } else {
                assert!(result.is_err());
                assert_eq!(confirmed, previous);
                assert_eq!(runtime.policy.route_host("blocked.test"), GeoRoute::Tunnel);
                assert!(!runtime.disable_quic);
            }
            assert_eq!(
                runtime.publications,
                u32::from(matches!(failure, "none" | "publish"))
            );
        }
    }

    #[test]
    fn attach_cannot_apply_a_policy_that_was_never_reconfigured() {
        let current = Profile::default();
        let mut attached = current.clone();
        attached.frontends.tunnel = !current.frontends.tunnel;
        assert!(attachment_rules_match(&current, &attached));
        attached.routing = target().routing;
        assert!(!attachment_rules_match(&current, &attached));
    }

    #[test]
    fn rejected_command_preserves_connected_gate_health_and_runtime_metadata() {
        let mut snapshot = crate::NativeSnapshot::disconnected();
        snapshot.phase = "connected".into();
        snapshot.reconnect_count = 7;
        snapshot.active_listeners = vec!["127.0.0.1:1080".into()];
        snapshot.vpn_gate = Some(usque_core::vpngate::GateStatus {
            stage: usque_core::vpngate::GateStage::Connected,
            ..Default::default()
        });
        record_rejection(&mut snapshot);
        assert_eq!(snapshot.phase, "connected");
        assert_eq!(snapshot.reconnect_count, 7);
        assert_eq!(snapshot.active_listeners, ["127.0.0.1:1080"]);
        assert_eq!(
            snapshot.vpn_gate.unwrap().stage,
            usque_core::vpngate::GateStage::Connected
        );
        assert_eq!(
            snapshot.error_code.as_deref(),
            Some("ROUTING_UPDATE_FAILED")
        );
        assert!(snapshot.failure.is_none());
        assert_eq!(crate::RECONFIGURE_REJECTED, -11);
    }
}
