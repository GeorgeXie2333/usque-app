//! Typed failure policy shared by initial connection and continuous recovery.
use usque_core::{TransportFailure, TransportFailureCode};

/// The same decision applies to an established driver's exit and to a failed
/// replacement. Platform availability is considered only after this check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RecoveryDecision {
    Retry,
    RefreshPin,
    Stop,
}

impl RecoveryDecision {
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
