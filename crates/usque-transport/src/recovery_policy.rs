//! Bounded, runtime-local preference after established H3 failures. This never
//! changes saved policy, authentication, endpoint protection, or egress rules.
use std::time::Duration;

use tokio::time::Instant;
use usque_core::{Transport, TransportFailure, TransportFailureCode, TransportPolicy};

const H3_FAILURE_THRESHOLD: u8 = 2;
const H2_COOLDOWN: Duration = Duration::from_secs(120);
const STABLE_H3_DURATION: Duration = Duration::from_secs(60);

#[derive(Default)]
pub(crate) struct AutoRecoveryPolicy {
    generation: Option<u64>,
    failures: u8,
    last_failure: Option<Instant>,
    prefer_h2_until: Option<Instant>,
}

impl AutoRecoveryPolicy {
    /// Only call for an established H3 tunnel under the user's Auto policy, not
    /// failed speculative probes or initial handshake attempts.
    pub(crate) fn record_failure(
        &mut self,
        failure: &TransportFailure,
        generation: Option<u64>,
        connection_lifetime: Duration,
        now: Instant,
    ) -> bool {
        self.observe_generation(generation);
        let allowed = failure.transport == Some(Transport::Http3)
            && failure.fallback_allowed
            && matches!(
                failure.code,
                TransportFailureCode::H3UdpUnreachable
                    | TransportFailureCode::H3ProtocolError
                    | TransportFailureCode::H3DatagramUnavailable
                    | TransportFailureCode::H3ConnectionClosed
                    | TransportFailureCode::PmtuRevalidationExhausted
            );
        if !allowed {
            self.failures = 0;
            self.last_failure = None;
            self.prefer_h2_until = None;
            return false;
        }
        if connection_lifetime >= STABLE_H3_DURATION
            || self
                .last_failure
                .is_some_and(|at| now.saturating_duration_since(at) >= H2_COOLDOWN)
        {
            self.failures = 0;
        }
        self.failures = self.failures.saturating_add(1);
        self.last_failure = Some(now);
        if self.failures < H3_FAILURE_THRESHOLD
            && failure.code != TransportFailureCode::PmtuRevalidationExhausted
        {
            return false;
        }
        self.prefer_h2_until = Some(now + H2_COOLDOWN);
        true
    }

    pub(crate) fn reconnect_transport(
        &mut self,
        configured: TransportPolicy,
        generation: Option<u64>,
        now: Instant,
    ) -> TransportPolicy {
        if configured == TransportPolicy::Auto && self.prefer_h2(generation, now) {
            TransportPolicy::Http2
        } else {
            configured
        }
    }

    fn prefer_h2(&mut self, generation: Option<u64>, now: Instant) -> bool {
        self.observe_generation(generation);
        if self.prefer_h2_until.is_some_and(|deadline| now >= deadline) {
            self.prefer_h2_until = None;
            self.last_failure = None;
            self.failures = 0;
        }
        self.prefer_h2_until.is_some()
    }

    fn observe_generation(&mut self, generation: Option<u64>) {
        if self.generation != generation {
            *self = Self {
                generation,
                ..Self::default()
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use usque_core::{AddressFamily, TransportStage};

    fn failure(code: TransportFailureCode) -> TransportFailure {
        TransportFailure::new(code, TransportStage::PacketReceive)
            .on_path(Transport::Http3, AddressFamily::Ipv4)
    }

    #[test]
    fn repeated_data_plane_failure_has_a_finite_generation_scoped_cooldown() {
        let mut policy = AutoRecoveryPolicy::default();
        let now = Instant::now();
        let error = failure(TransportFailureCode::H3ConnectionClosed);
        assert!(!policy.record_failure(&error, Some(1), Duration::from_secs(5), now));
        assert!(!policy.prefer_h2(Some(1), now));
        assert!(policy.record_failure(&error, Some(1), Duration::from_secs(5), now));
        assert!(policy.prefer_h2(Some(1), now + H2_COOLDOWN - Duration::from_secs(1)));
        assert!(!policy.prefer_h2(Some(1), now + H2_COOLDOWN));
        assert!(!policy.record_failure(&error, Some(1), Duration::ZERO, now + H2_COOLDOWN));
        assert!(policy.record_failure(&error, Some(1), Duration::ZERO, now + H2_COOLDOWN));
        assert!(!policy.prefer_h2(Some(2), now + H2_COOLDOWN));
    }

    #[test]
    fn stable_sessions_do_not_accumulate_a_failure_streak() {
        let mut policy = AutoRecoveryPolicy::default();
        let now = Instant::now();
        let error = failure(TransportFailureCode::H3ConnectionClosed);
        for _ in 0..10 {
            assert!(!policy.record_failure(&error, None, STABLE_H3_DURATION, now));
        }
        assert!(!policy.prefer_h2(None, now));
    }

    #[test]
    fn reconnect_selection_never_overrides_an_explicit_transport_policy() {
        let mut policy = AutoRecoveryPolicy::default();
        let now = Instant::now();
        policy.record_failure(
            &failure(TransportFailureCode::PmtuRevalidationExhausted),
            Some(1),
            Duration::ZERO,
            now,
        );
        assert_eq!(
            policy.reconnect_transport(TransportPolicy::Http3, Some(1), now),
            TransportPolicy::Http3
        );
        assert_eq!(
            policy.reconnect_transport(TransportPolicy::Http2, Some(1), now),
            TransportPolicy::Http2
        );
        assert_eq!(
            policy.reconnect_transport(TransportPolicy::Auto, Some(1), now),
            TransportPolicy::Http2
        );
        assert_eq!(
            policy.reconnect_transport(TransportPolicy::Auto, Some(2), now),
            TransportPolicy::Auto
        );
    }

    #[test]
    fn exhausted_pmtu_can_fallback_immediately_but_security_and_local_failures_cannot() {
        let now = Instant::now();
        for code in [
            TransportFailureCode::EndpointPinMismatch,
            TransportFailureCode::AuthenticationFailed,
            TransportFailureCode::IdentityInvalid,
            TransportFailureCode::SocketProtectionFailed,
            TransportFailureCode::AddressAssignmentInvalid,
            TransportFailureCode::PacketSendTimeout,
            TransportFailureCode::PacketSendFailed,
            TransportFailureCode::PhysicalNetworkChanged,
        ] {
            let mut policy = AutoRecoveryPolicy::default();
            assert!(policy.record_failure(
                &failure(TransportFailureCode::PmtuRevalidationExhausted),
                Some(1),
                Duration::ZERO,
                now
            ));
            for _ in 0..3 {
                assert!(!policy.record_failure(&failure(code), Some(1), Duration::ZERO, now));
            }
            assert!(!policy.prefer_h2(Some(1), now));
        }
        let mut policy = AutoRecoveryPolicy::default();
        let mut denied = failure(TransportFailureCode::H3ConnectionClosed);
        denied.fallback_allowed = false;
        for _ in 0..3 {
            assert!(!policy.record_failure(&denied, None, Duration::ZERO, now));
        }
    }
}
