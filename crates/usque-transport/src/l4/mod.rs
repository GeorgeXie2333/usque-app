//! TCP CONNECT data plane. Never starts CONNECT-IP or a general UDP relay.
mod actor;
#[cfg(test)]
mod actor_tests;
mod budget_wait;
mod client;
#[cfg(test)]
mod client_tests;
#[cfg(test)]
mod parallel_tests;
pub(crate) mod performance;
mod pool;
#[cfg(test)]
mod quic_fallback_tests;
mod receive_history;
mod relay;
mod runtime;
pub(crate) mod stream;
#[cfg(test)]
pub(crate) mod test_options;
mod tun;
mod tun_reject;
#[cfg(test)]
mod tun_tests;
mod tun_udp;
pub(crate) mod tun_wire;

pub(crate) use actor::{L4Actor, SessionHandle};
pub(crate) use client::L4Client;
pub(crate) use runtime::L4Runtime;
pub(crate) use stream::{BufferBudget, L4Metrics, OpenRequest};
pub(crate) use tun::L4TunIo;

use std::time::Duration;

pub(crate) const DRAIN_TIMEOUT: Duration = Duration::from_secs(30);
pub(crate) const FLOW_BUFFER: usize = 256 * 1024;
pub(crate) const CHUNK_SIZE: usize = 32 * 1024;
pub(crate) const DNS_OPERATIONS: usize = 16;

pub(crate) fn failure(error: &crate::h2::TransportError) -> usque_core::TransportFailure {
    use usque_core::{Transport, TransportFailure, TransportFailureCode as Code};
    let original = error.failure(Some(Transport::Http3), None);
    let mut result = match original.code {
        Code::H3ProtocolError | Code::H3DatagramUnavailable => {
            TransportFailure::new(Code::L4ProtocolError, original.stage)
        }
        Code::H3UdpUnreachable
        | Code::H3HandshakeTimeout
        | Code::H3ConnectionClosed
        | Code::PmtuRevalidationExhausted => {
            TransportFailure::new(Code::L4SessionUnavailable, original.stage)
        }
        _ => original,
    };
    result.transport = Some(Transport::Http3);
    result.fallback_allowed = false;
    result
}

pub(crate) fn transport_error(error: crate::h2::TransportError) -> crate::h2::TransportError {
    crate::h2::TransportError::L4(Box::new(failure(&error)))
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Limits {
    pub(crate) active: usize,
    pub(crate) pending: usize,
    pub(crate) relay: usize,
    pub(crate) initial_connection: u64,
    pub(crate) maximum_connection: u64,
    pub(crate) initial_stream: u64,
    pub(crate) maximum_stream: u64,
    pub(crate) buffers: usize,
}

impl Limits {
    pub(crate) const fn platform() -> Self {
        Self::for_platform(
            cfg!(target_os = "android"),
            cfg!(target_pointer_width = "32"),
        )
    }

    const fn for_platform(android: bool, bits32: bool) -> Self {
        const MIB: usize = 1024 * 1024;
        if android && bits32 {
            Self {
                active: 64,
                pending: 32,
                relay: 64 * 1024,
                initial_connection: 2 << 20,
                maximum_connection: 8 << 20,
                initial_stream: 256 << 10,
                maximum_stream: 4 << 20,
                buffers: 24 * MIB,
            }
        } else if android {
            Self {
                active: 128,
                pending: 64,
                relay: 128 * 1024,
                initial_connection: 4 << 20,
                maximum_connection: 32 << 20,
                initial_stream: 512 << 10,
                maximum_stream: 16 << 20,
                buffers: 64 * MIB,
            }
        } else {
            Self {
                active: 256,
                pending: 128,
                relay: 128 * 1024,
                initial_connection: 8 << 20,
                maximum_connection: 64 << 20,
                initial_stream: 1 << 20,
                maximum_stream: 32 << 20,
                buffers: 128 * MIB,
            }
        }
    }

    pub(crate) fn connection_share(self, connections: u8) -> Self {
        assert!((1..=usque_core::MAX_MASQUE_CONNECTIONS).contains(&connections));
        // Keep one extra slot for GOAWAY draining/replacement. Credit already
        // advertised on a healthy connection cannot be revoked later.
        let maximum_connection = self.maximum_connection / (u64::from(connections) + 1);
        Self {
            initial_connection: self.initial_connection.min(maximum_connection),
            maximum_connection,
            initial_stream: self.initial_stream.min(maximum_connection),
            maximum_stream: self.maximum_stream.min(maximum_connection),
            ..self
        }
    }

    #[cfg(test)]
    pub(crate) fn configure(self, config: &mut quiche::Config) {
        self.configure_connections(config, 1);
    }

    pub(crate) fn configure_connections(self, config: &mut quiche::Config, connections: u8) {
        let share = self.connection_share(connections);
        config.set_initial_max_data(share.initial_connection);
        config.set_max_connection_window(share.maximum_connection);
        config.set_initial_max_stream_data_bidi_local(share.initial_stream);
        config.set_max_stream_window(share.maximum_stream);
        // Client-created CONNECT streams are limited by the SERVER, not this.
        config.set_initial_max_streams_bidi(0);
        config.enable_dgram(false, 0, 0);
    }
}

#[cfg(test)]
mod contracts {
    use super::*;
    #[test]
    fn platform_limits_and_failure_actions_are_bounded_and_never_fallback() {
        for (android, bits32, active, pending, buffers) in [
            (false, false, 256, 128, 128),
            (true, false, 128, 64, 64),
            (true, true, 64, 32, 24),
        ] {
            let limits = Limits::for_platform(android, bits32);
            assert_eq!(
                (limits.active, limits.pending, limits.buffers),
                (active, pending, buffers << 20)
            );
            assert!(limits.initial_connection * 2 <= limits.maximum_connection);
            for connections in 1..=usque_core::MAX_MASQUE_CONNECTIONS {
                let share = limits.connection_share(connections);
                let slots = u64::from(connections) + 1;
                assert!(share.maximum_connection * slots <= limits.maximum_connection);
                assert!(share.initial_connection <= share.maximum_connection);
                assert!(share.initial_stream <= share.maximum_stream);
                assert!(share.maximum_stream <= share.maximum_connection);
                assert_eq!(share.buffers, limits.buffers);
                assert_eq!(share.active, limits.active);
                assert_eq!(share.pending, limits.pending);
            }
        }
        for error in [
            crate::TransportError::TunnelClosed,
            crate::TransportError::EndpointPinMismatch,
            crate::TransportError::PmtuRevalidationExhausted,
        ] {
            let result = failure(&error);
            assert!(!result.fallback_allowed);
            assert_ne!(result.action(), usque_core::FailureAction::FallbackToH2);
        }
    }
}
