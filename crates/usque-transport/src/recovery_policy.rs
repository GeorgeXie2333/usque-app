//! Typed failure policy shared by initial connection and continuous recovery.
use usque_core::{
    Transport, TransportFailure, TransportFailureCode, TransportPolicy, TransportStage,
};

/// The same decision applies to an established driver's exit and to a failed
/// replacement. Platform availability is considered only after this check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RecoveryDecision {
    Retry,
    RefreshPin,
    Stop,
}

impl RecoveryDecision {
    /// A terminal H3 link-capacity rejection does not revoke an Auto intent's
    /// permission to choose another authenticated, protected transport.
    pub(crate) fn for_intent(policy: TransportPolicy, failure: &TransportFailure) -> Self {
        if policy == TransportPolicy::Auto
            && failure.transport == Some(Transport::Http3)
            && failure.code == TransportFailureCode::H3DatagramUnavailable
            && failure.stage == TransportStage::PacketSend
            && failure.fallback_allowed
        {
            Self::Retry
        } else {
            Self::for_failure(failure)
        }
    }

    pub(crate) fn may_try_another_candidate(error: &crate::TransportError) -> bool {
        matches!(error, crate::TransportError::Ipv6MinimumMtuUnavailable(_))
            || Self::for_failure(&error.failure(None, None)) == Self::Retry
    }

    pub(crate) fn for_failure(failure: &TransportFailure) -> Self {
        if failure.code == TransportFailureCode::EndpointPinMismatch {
            Self::RefreshPin
        } else if !failure.retryable || failure.action() == usque_core::FailureAction::Stop {
            Self::Stop
        } else {
            Self::Retry
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use usque_core::{AddressFamily, Transport, TransportStage};

    #[test]
    fn insufficient_ipv6_link_stops_explicit_h3_but_auto_can_select_another_carrier() {
        let mtu_failure = crate::TransportError::Ipv6MinimumMtuUnavailable(1270)
            .failure(Some(Transport::Http3), Some(AddressFamily::Ipv4));
        assert!(!mtu_failure.retryable);
        assert!(mtu_failure.fallback_allowed);
        assert_eq!(
            RecoveryDecision::for_failure(&mtu_failure),
            RecoveryDecision::Stop
        );
        assert_eq!(
            RecoveryDecision::for_intent(TransportPolicy::Http3, &mtu_failure),
            RecoveryDecision::Stop
        );
        assert_eq!(
            RecoveryDecision::for_intent(TransportPolicy::Http2, &mtu_failure),
            RecoveryDecision::Stop
        );
        assert_eq!(
            RecoveryDecision::for_intent(TransportPolicy::Auto, &mtu_failure),
            RecoveryDecision::Retry
        );
        let exhausted = crate::TransportError::Ipv6MinimumMtuUnavailable(1270).failure(None, None);
        assert_eq!(exhausted.transport, Some(Transport::Http3));
        assert_eq!(
            RecoveryDecision::for_intent(TransportPolicy::Http3, &exhausted),
            RecoveryDecision::Stop
        );
        assert_eq!(
            RecoveryDecision::for_intent(TransportPolicy::Auto, &exhausted),
            RecoveryDecision::Retry
        );
        for code in [
            TransportFailureCode::AuthenticationFailed,
            TransportFailureCode::IdentityInvalid,
            TransportFailureCode::SocketProtectionFailed,
            TransportFailureCode::AddressAssignmentInvalid,
        ] {
            assert_eq!(
                RecoveryDecision::for_intent(TransportPolicy::Auto, &failure(code)),
                RecoveryDecision::Stop
            );
        }
    }

    #[test]
    fn terminal_failures_are_never_retried_as_network_failures() {
        for code in [
            TransportFailureCode::AuthenticationFailed,
            TransportFailureCode::IdentityInvalid,
            TransportFailureCode::ConfigurationInvalid,
            TransportFailureCode::SocketProtectionFailed,
            TransportFailureCode::AddressAssignmentInvalid,
        ] {
            assert_eq!(
                RecoveryDecision::for_failure(&failure(code)),
                RecoveryDecision::Stop
            );
        }
        assert_eq!(
            RecoveryDecision::for_failure(&failure(TransportFailureCode::EndpointPinMismatch)),
            RecoveryDecision::RefreshPin
        );
        assert_eq!(
            RecoveryDecision::for_failure(&failure(TransportFailureCode::H3ConnectionClosed)),
            RecoveryDecision::Retry
        );
        let mut overridden = failure(TransportFailureCode::H3ConnectionClosed);
        overridden.retryable = false;
        assert_eq!(
            RecoveryDecision::for_failure(&overridden),
            RecoveryDecision::Stop
        );
    }

    fn failure(code: TransportFailureCode) -> TransportFailure {
        TransportFailure::new(code, TransportStage::PacketReceive)
            .on_path(Transport::Http3, AddressFamily::Ipv4)
    }
}
