//! Shared CONNECT-IP control-capsule apply path for HTTP/2 and HTTP/3.
//!
//! ADDRESS_ASSIGN and ROUTE_ADVERTISEMENT replace `PeerNetworkState`.
//! ADDRESS_REQUEST is rejected with an unspecified ADDRESS_ASSIGN. Unknown
//! types stay framed and are ignored. This module never mutates OS routes.

use std::collections::VecDeque;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use bytes::{Buf, Bytes, BytesMut};
use tokio::sync::watch;
use usque_protocol::{AddressAssign, CapsuleEffect, ConnectIpCapsule, IpPrefix, PeerNetworkState};

use crate::h2::TransportError;

pub(crate) const MAX_PENDING_CONTROL_CAPSULES: usize = 64;
pub(crate) const MAX_PENDING_CONTROL_BYTES: usize = 256 * 1024;

pub(crate) struct PendingControlCapsule {
    pub(crate) bytes: Bytes,
    pub(crate) offset: usize,
}

pub(crate) struct ConnectIpControlPlane {
    pub(crate) buffer: BytesMut,
    pub(crate) state: PeerNetworkState,
    pub(crate) state_tx: watch::Sender<PeerNetworkState>,
    pub(crate) pending: VecDeque<PendingControlCapsule>,
}

impl ConnectIpControlPlane {
    pub(crate) fn new(state_tx: watch::Sender<PeerNetworkState>) -> Self {
        Self {
            buffer: BytesMut::with_capacity(4_096),
            state: PeerNetworkState::default(),
            state_tx,
            pending: VecDeque::new(),
        }
    }

    pub(crate) fn drain(&mut self) -> Result<(), TransportError> {
        drain_control_capsules(
            &mut self.buffer,
            &mut self.state,
            &self.state_tx,
            &mut self.pending,
        )
    }

    pub(crate) fn apply(&mut self, capsule: &ConnectIpCapsule) -> Result<(), TransportError> {
        apply_connect_ip_capsule(&mut self.state, &self.state_tx, &mut self.pending, capsule)
    }
}

pub(crate) fn drain_control_capsules(
    buffer: &mut BytesMut,
    state: &mut PeerNetworkState,
    state_tx: &watch::Sender<PeerNetworkState>,
    pending: &mut VecDeque<PendingControlCapsule>,
) -> Result<(), TransportError> {
    loop {
        let mut cursor = buffer.clone().freeze();
        let Some(capsule) = ConnectIpCapsule::decode_if_complete(&mut cursor)? else {
            return Ok(());
        };
        let consumed = buffer.len() - cursor.len();
        buffer.advance(consumed);
        apply_connect_ip_capsule(state, state_tx, pending, &capsule)?;
    }
}

pub(crate) fn apply_connect_ip_capsule(
    state: &mut PeerNetworkState,
    state_tx: &watch::Sender<PeerNetworkState>,
    pending: &mut VecDeque<PendingControlCapsule>,
    capsule: &ConnectIpCapsule,
) -> Result<(), TransportError> {
    match state.apply(capsule) {
        CapsuleEffect::AssignmentsReplaced | CapsuleEffect::RoutesReplaced => {
            state_tx.send_replace(state.clone());
        }
        CapsuleEffect::AddressRequested(request) => {
            let addresses = request
                .addresses
                .into_iter()
                .map(|requested| IpPrefix {
                    request_id: requested.request_id,
                    address: match requested.address {
                        IpAddr::V4(_) => IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                        IpAddr::V6(_) => IpAddr::V6(Ipv6Addr::UNSPECIFIED),
                    },
                    prefix_len: match requested.address {
                        IpAddr::V4(_) => 32,
                        IpAddr::V6(_) => 128,
                    },
                })
                .collect();
            let rejection =
                ConnectIpCapsule::AddressAssign(AddressAssign { addresses }).encode()?;
            let pending_bytes = pending.iter().fold(0_usize, |total, item| {
                total.saturating_add(item.bytes.len().saturating_sub(item.offset))
            });
            if pending.len() >= MAX_PENDING_CONTROL_CAPSULES
                || pending_bytes.saturating_add(rejection.len()) > MAX_PENDING_CONTROL_BYTES
            {
                return Err(TransportError::SendQueueFull);
            }
            pending.push_back(PendingControlCapsule {
                bytes: rejection,
                offset: 0,
            });
        }
        CapsuleEffect::UnknownIgnored(capsule_type) => {
            tracing::debug!(
                capsule_type,
                "ignored an unknown CONNECT-IP control capsule"
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use usque_protocol::{AddressRequest, IpAddressRange, RouteAdvertisement};

    #[test]
    fn control_capsules_replace_peer_state_across_fragmented_reads() {
        let assignment = ConnectIpCapsule::AddressAssign(AddressAssign {
            addresses: vec![IpPrefix {
                request_id: 0,
                address: "172.16.0.2".parse().unwrap(),
                prefix_len: 32,
            }],
        })
        .encode()
        .unwrap();
        let routes = ConnectIpCapsule::RouteAdvertisement(RouteAdvertisement {
            ranges: vec![IpAddressRange {
                start: "0.0.0.0".parse().unwrap(),
                end: "255.255.255.255".parse().unwrap(),
                protocol: 0,
            }],
        })
        .encode()
        .unwrap();
        let mut wire = BytesMut::new();
        wire.extend_from_slice(&assignment);
        wire.extend_from_slice(&routes);

        let (state_tx, state_rx) = watch::channel(PeerNetworkState::default());
        let mut state = PeerNetworkState::default();
        let mut pending = VecDeque::new();
        let mut buffer = BytesMut::new();
        for byte in wire {
            buffer.extend_from_slice(&[byte]);
            drain_control_capsules(&mut buffer, &mut state, &state_tx, &mut pending).unwrap();
        }

        assert!(buffer.is_empty());
        assert!(pending.is_empty());
        assert_eq!(state_rx.borrow().assigned_addresses.len(), 1);
        assert_eq!(state_rx.borrow().available_routes.len(), 1);
    }

    #[test]
    fn address_requests_are_rejected_without_assigning_peer_addresses() {
        let request = ConnectIpCapsule::AddressRequest(AddressRequest {
            addresses: vec![
                IpPrefix {
                    request_id: 7,
                    address: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                    prefix_len: 32,
                },
                IpPrefix {
                    request_id: 8,
                    address: IpAddr::V6(Ipv6Addr::UNSPECIFIED),
                    prefix_len: 128,
                },
            ],
        })
        .encode()
        .unwrap();
        let (state_tx, _state_rx) = watch::channel(PeerNetworkState::default());
        let mut state = PeerNetworkState::default();
        let mut pending = VecDeque::new();
        let mut buffer = BytesMut::from(request.as_ref());

        drain_control_capsules(&mut buffer, &mut state, &state_tx, &mut pending).unwrap();
        let mut rejection = pending.pop_front().unwrap().bytes;
        let ConnectIpCapsule::AddressAssign(rejection) =
            ConnectIpCapsule::decode(&mut rejection).unwrap()
        else {
            panic!("expected ADDRESS_ASSIGN rejection");
        };
        assert_eq!(rejection.addresses[0].request_id, 7);
        assert_eq!(
            rejection.addresses[0].address,
            IpAddr::V4(Ipv4Addr::UNSPECIFIED)
        );
        assert_eq!(rejection.addresses[0].prefix_len, 32);
        assert_eq!(rejection.addresses[1].request_id, 8);
        assert_eq!(
            rejection.addresses[1].address,
            IpAddr::V6(Ipv6Addr::UNSPECIFIED)
        );
        assert_eq!(rejection.addresses[1].prefix_len, 128);
        assert!(!state.assignments_advertised);
        assert!(state.assigned_addresses.is_empty());
    }

    #[test]
    fn address_request_rejections_have_item_and_byte_bounds() {
        let request = ConnectIpCapsule::AddressRequest(AddressRequest {
            addresses: vec![IpPrefix {
                request_id: 1,
                address: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                prefix_len: 32,
            }],
        });
        let (state_tx, _state_rx) = watch::channel(PeerNetworkState::default());
        let mut state = PeerNetworkState::default();
        let mut pending = VecDeque::new();
        for _ in 0..MAX_PENDING_CONTROL_CAPSULES {
            apply_connect_ip_capsule(&mut state, &state_tx, &mut pending, &request).unwrap();
        }
        assert_eq!(pending.len(), MAX_PENDING_CONTROL_CAPSULES);
        assert!(matches!(
            apply_connect_ip_capsule(&mut state, &state_tx, &mut pending, &request),
            Err(TransportError::SendQueueFull)
        ));
        assert!(
            pending
                .iter()
                .map(|item| item.bytes.len().saturating_sub(item.offset))
                .sum::<usize>()
                <= MAX_PENDING_CONTROL_BYTES
        );
    }
}
