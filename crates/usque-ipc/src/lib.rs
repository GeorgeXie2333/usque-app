use bytes::{Buf, BufMut, Bytes, BytesMut};
use prost::Message;
use thiserror::Error;

pub mod v1 {
    include!(concat!(env!("OUT_DIR"), "/usque.v1.rs"));
}

pub mod agent_v1 {
    include!(concat!(env!("OUT_DIR"), "/usque.agent.v1.rs"));
}

pub const MAX_FRAME_SIZE: usize = 4 * 1024 * 1024;

pub fn encode_frame<M: Message>(message: &M) -> Result<Bytes, FrameError> {
    let encoded_len = message.encoded_len();
    if encoded_len > MAX_FRAME_SIZE {
        return Err(FrameError::TooLarge(encoded_len));
    }
    let mut output = BytesMut::with_capacity(4 + encoded_len);
    output.put_u32(encoded_len as u32);
    message.encode(&mut output)?;
    Ok(output.freeze())
}

pub fn decode_frame<M: Message + Default>(mut frame: Bytes) -> Result<M, FrameError> {
    if frame.len() < 4 {
        return Err(FrameError::TruncatedHeader);
    }
    let declared = frame.get_u32() as usize;
    if declared > MAX_FRAME_SIZE {
        return Err(FrameError::TooLarge(declared));
    }
    if frame.len() != declared {
        return Err(FrameError::LengthMismatch {
            declared,
            actual: frame.len(),
        });
    }
    Ok(M::decode(frame)?)
}

/// Extracts one complete length-prefixed protobuf frame from an incremental
/// stream buffer. Incomplete input is retained without being consumed.
pub fn split_frame(buffer: &mut BytesMut) -> Result<Option<Bytes>, FrameError> {
    if buffer.len() < 4 {
        return Ok(None);
    }
    let declared = u32::from_be_bytes(buffer[..4].try_into().expect("four-byte prefix")) as usize;
    if declared > MAX_FRAME_SIZE {
        return Err(FrameError::TooLarge(declared));
    }
    let frame_len = 4 + declared;
    if buffer.len() < frame_len {
        return Ok(None);
    }
    Ok(Some(buffer.split_to(frame_len).freeze()))
}

#[derive(Debug, Error)]
pub enum FrameError {
    #[error("protobuf frame header is truncated")]
    TruncatedHeader,
    #[error("protobuf frame exceeds {MAX_FRAME_SIZE} bytes: {0}")]
    TooLarge(usize),
    #[error("protobuf frame length mismatch: declared {declared}, actual {actual}")]
    LengthMismatch { declared: usize, actual: usize },
    #[error("protobuf encoding failed: {0}")]
    Encode(#[from] prost::EncodeError),
    #[error("protobuf decoding failed: {0}")]
    Decode(#[from] prost::DecodeError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_v1::{
        AcquireDirectEgressRequest, AcquireTunnelLeaseRequest, AgentCapabilities, AgentRequest,
        AgentState, GetCapabilitiesRequest, InspectPlatformStateRequest, PrepareTunnelRequest,
        ResumeTunnelRequest, TunnelPlan, agent_request,
    };
    use crate::v1::{
        Capabilities, CapabilitiesChanged, ControlRequest, CreateProfileWithIdentityRequest,
        DiagnosticMode, EventEnvelope, ExportWarpSecretRequest, FrontendKind, FrontendPhase,
        FrontendSettings, FrontendStatus, GetStatusRequest, IdentityProvisioning,
        IdentityProvisioningMethod, Profile, ProvisionIdentityRequest,
        ReconfigureActiveProfileRequest, StartDiagnosticsRequest, UpdateLicenseKeyRequest,
        UpdateProxyAuthRequest, WarningRaised, ZeroTrustEnrollment, control_request,
        event_envelope,
    };

    // Checked-in v1 wire snapshots. Changing an established field number or
    // envelope shape makes these tests fail even if generated Rust still
    // compiles, protecting older GUI/engine pairs during rolling upgrades.
    const GET_STATUS_V1_FRAME: &[u8] = &[0, 0, 0, 6, 0x0a, 2, b'r', b'1', 0x52, 0];
    const WARNING_V1_FRAME: &[u8] = &[
        0, 0, 0, 26, 0x08, 0x07, 0x6a, 0x16, 0x0a, 0x0b, b'L', b'A', b'N', b'_', b'E', b'X', b'P',
        b'O', b'S', b'E', b'D', 0x12, 0x07, b'w', b'a', b'r', b'n', b'i', b'n', b'g',
    ];
    const PROVISION_IDENTITY_V1_FRAME: &[u8] = &[
        0, 0, 0, 24, 0x0a, 2, b'p', b'1', 0xba, 0x01, 17, 0x0a, 2, b'i', b'd', 0x12, 1, b'x', 0x18,
        1, 0x22, 2, b'e', b'n', 0x2a, 2, b'p', b'c',
    ];
    const PROVISION_ZERO_TRUST_V2_FRAME: &[u8] = &[
        0, 0, 0, 22, 0x0a, 1, b'z', 0xba, 0x01, 16, 0x0a, 2, b'i', b'd', 0x18, 1, 0x38, 4, 0x42, 6,
        0x0a, 1, b't', 0x12, 1, b'c',
    ];
    const CAPABILITIES_V1_FRAME: &[u8] = &[
        0, 0, 0, 37, 0x08, 0x08, 0x72, 33, 0x0a, 31, 0x10, 1, 0x18, 1, 0x32, 7, b'w', b'i', b'n',
        b'd', b'o', b'w', b's', 0x3a, 6, b'x', b'8', b'6', b'_', b'6', b'4', 0x42, 2, b'h', b'3',
        0x42, 2, b'h', b'2', 0x48, 1,
    ];
    const IMPORT_LEGACY_PROFILES_V1_FRAME: &[u8] = &[
        0, 0, 0, 11, 0x0a, 2, b'm', b'1', 0xca, 0x01, 4, 0x12, 2, b'i', b'd',
    ];
    const CREATE_PROFILE_WITH_IDENTITY_V1_FRAME: &[u8] = &[
        0, 0, 0, 26, 0x0a, 2, b'c', b'1', 0xd2, 0x01, 19, 0x0a, 7, 0x0a, 2, b'i', b'd', 0x12, 1,
        b'n', 0x12, 8, 0x08, 1, 0x18, 1, 0x22, 2, b'e', b'n',
    ];
    const RECONFIGURE_V2_FRAME: &[u8] = &[
        0, 0, 0, 11, 0x0a, 1, b'x', 0xda, 0x01, 5, 0x0a, 3, 0x0a, 1, b'p',
    ];
    const UPDATE_LICENSE_V2_FRAME: &[u8] = &[
        0, 0, 0, 12, 0x0a, 1, b'x', 0xea, 0x01, 6, 0x0a, 1, b'p', 0x12, 1, b'k',
    ];
    const EXPORT_WARP_SECRET_V2_FRAME: &[u8] = &[
        0, 0, 0, 14, 0x0a, 1, b'x', 0xfa, 0x01, 8, 0x0a, 1, b'p', 0x12, 1, b'd', 0x18, 1,
    ];
    const UPDATE_PROXY_AUTH_V1_FRAME: &[u8] = &[
        0, 0, 0, 17, 0x0a, 1, b'x', 0x82, 0x02, 0x0b, 0x0a, 1, b'p', 0x12, 1, b'u', 0x1a, 1, b'k',
        0x20, 1,
    ];
    const START_DIAGNOSTICS_V1_FRAME: &[u8] =
        &[0, 0, 0, 9, 0x0a, 2, b'd', b'1', 0xa2, 0x02, 2, 0x08, 1];
    const AGENT_CAPABILITIES_V1_FRAME: &[u8] = &[0, 0, 0, 8, 0x0a, 2, b'a', b'1', 0x10, 1, 0x52, 0];
    const AGENT_RESUME_TUNNEL_V1_FRAME: &[u8] = &[
        0, 0, 0, 15, 0x0a, 2, b'r', b'1', 0x10, 1, 0xaa, 0x01, 6, 0x0a, 1, b'o', 0x12, 1, b'p',
    ];
    const AGENT_TUNNEL_LEASE_V1_FRAME: &[u8] = &[
        0, 0, 0, 12, 0x0a, 2, b'l', b'1', 0x10, 1, 0xb2, 0x01, 3, 0x0a, 1, b'o',
    ];
    const AGENT_CONTROL_API_V2_FRAME: &[u8] = &[
        0, 0, 0, 32, 0x0a, 2, b'p', b'2', 0x10, 2, 0x62, 24, 0x0a, 1, b'o', 0x12, 19, 0x5a, 17,
        b'1', b'9', b'8', b'.', b'5', b'1', b'.', b'1', b'0', b'0', b'.', b'1', b'0', b':', b'4',
        b'4', b'3',
    ];
    const AGENT_INSPECT_PLATFORM_V3_FRAME: &[u8] =
        &[0, 0, 0, 9, 0x0a, 2, b'i', b'1', 0x10, 3, 0xca, 0x01, 0];
    const AGENT_EXACT_EGRESS_V3_FRAME: &[u8] = &[
        0, 0, 0, 33, 0x0a, 2, b'g', b'1', 0x10, 3, 0xc2, 0x01, 24, 0x0a, 1, b'o', 0x12, 15, b'2',
        b'0', b'3', b'.', b'0', b'.', b'1', b'1', b'3', b'.', b'9', b':', b'4', b'4', b'3', 0x18,
        17, 0x20, 7,
    ];
    const NETWORK_QUALITY_V1_FRAME: &[u8] = &[
        0x00, 0x00, 0x00, 0x41, 0x0a, 0x02, b'n', b'1', 0xaa, 0x01, 0x3a, 0x08, 0xd2, 0x09, 0x12,
        0x02, b'c', b'1', 0x18, 0x01, 0x22, 0x0e, 0x20, 0x2a, 0x28, 0x01, 0x68, 0x15, 0x70, 0x01,
        0xc0, 0x03, 0x01, 0xc8, 0x03, 0x01, 0x2a, 0x1f, 0x08, 0x05, 0x10, 0x02, 0x18, 0x40, 0x20,
        0x64, 0x28, 0x80, 0xa3, 0x05, 0x30, 0x04, 0x38, 0xc8, 0x01, 0x40, 0x01, 0x48, 0x32, 0x50,
        0x07, 0x58, 0x01, 0x60, 0x01, 0x68, 0x0a, 0x70, 0x08,
    ];

    #[derive(Clone, PartialEq, Message)]
    struct LegacyControlResponse {
        #[prost(string, tag = "1")]
        request_id: String,
    }

    #[derive(Clone, PartialEq, Message)]
    struct LegacyAgentEnvelope {
        #[prost(string, tag = "1")]
        request_id: String,
        #[prost(uint32, tag = "2")]
        protocol_version: u32,
    }

    #[derive(Clone, PartialEq, Message)]
    struct LegacyAgentCapabilities {
        #[prost(uint32, tag = "9")]
        protocol_version: u32,
        #[prost(bool, tag = "13")]
        guarded_recovery: bool,
    }

    #[derive(Clone, PartialEq, Message)]
    struct LegacyAgentState {
        #[prost(enumeration = "agent_v1::AgentPhase", tag = "1")]
        phase: i32,
        #[prost(uint64, tag = "7")]
        journal_generation: u64,
    }

    #[test]
    fn control_request_round_trips_through_a_bounded_frame() {
        let request = ControlRequest {
            request_id: "request-1".to_owned(),
            payload: Some(control_request::Payload::GetStatus(GetStatusRequest {})),
        };
        let encoded = encode_frame(&request).unwrap();
        let decoded: ControlRequest = decode_frame(encoded).unwrap();
        assert_eq!(decoded, request);
    }

    #[test]
    fn privileged_agent_v1_wire_snapshot_is_stable() {
        let decoded: AgentRequest = decode_frame(Bytes::from_static(AGENT_CAPABILITIES_V1_FRAME))
            .expect("decode agent snapshot");
        assert_eq!(decoded.request_id, "a1");
        assert_eq!(decoded.protocol_version, 1);
        assert!(matches!(
            decoded.payload,
            Some(agent_request::Payload::GetCapabilities(
                GetCapabilitiesRequest {}
            ))
        ));
        assert_eq!(
            encode_frame(&decoded).expect("re-encode").as_ref(),
            AGENT_CAPABILITIES_V1_FRAME
        );
    }

    #[test]
    fn privileged_agent_resume_uses_a_new_append_only_field_number() {
        let decoded: AgentRequest = decode_frame(Bytes::from_static(AGENT_RESUME_TUNNEL_V1_FRAME))
            .expect("decode resume snapshot");
        assert!(matches!(
            decoded.payload.as_ref(),
            Some(agent_request::Payload::ResumeTunnel(ResumeTunnelRequest {
                operation_id,
                profile_id,
            })) if operation_id == "o" && profile_id == "p"
        ));
        assert_eq!(
            encode_frame(&decoded).expect("re-encode").as_ref(),
            AGENT_RESUME_TUNNEL_V1_FRAME
        );
    }

    #[test]
    fn privileged_agent_lease_uses_a_new_append_only_field_number() {
        let decoded: AgentRequest = decode_frame(Bytes::from_static(AGENT_TUNNEL_LEASE_V1_FRAME))
            .expect("decode lease snapshot");
        assert!(matches!(
            decoded.payload.as_ref(),
            Some(agent_request::Payload::AcquireTunnelLease(AcquireTunnelLeaseRequest {
                operation_id,
            })) if operation_id == "o"
        ));
        assert_eq!(
            encode_frame(&decoded).expect("re-encode").as_ref(),
            AGENT_TUNNEL_LEASE_V1_FRAME
        );
    }

    #[test]
    fn privileged_agent_v2_control_api_candidates_use_append_only_field_eleven() {
        let decoded: AgentRequest = decode_frame(Bytes::from_static(AGENT_CONTROL_API_V2_FRAME))
            .expect("decode control API snapshot");
        assert_eq!(decoded.protocol_version, 2);
        assert!(matches!(
            decoded.payload.as_ref(),
            Some(agent_request::Payload::PrepareTunnel(PrepareTunnelRequest {
                operation_id,
                plan: Some(TunnelPlan {
                    control_api_candidates,
                    ..
                }),
            })) if operation_id == "o"
                && control_api_candidates == &["198.51.100.10:443"]
        ));
        assert_eq!(
            encode_frame(&decoded).expect("re-encode").as_ref(),
            AGENT_CONTROL_API_V2_FRAME
        );
    }

    #[test]
    fn privileged_agent_v3_direct_egress_contract_round_trips() {
        let request = AgentRequest {
            request_id: "d3".to_owned(),
            protocol_version: 3,
            payload: Some(agent_request::Payload::AcquireDirectEgress(
                AcquireDirectEgressRequest {
                    operation_id: "operation".to_owned(),
                    remote_endpoint: "203.0.113.9:53".to_owned(),
                    protocol: 17,
                    expected_generation: 0,
                },
            )),
        };
        let decoded: AgentRequest = decode_frame(encode_frame(&request).unwrap()).unwrap();
        assert_eq!(decoded, request);
        assert!(matches!(
            decoded.payload,
            Some(agent_request::Payload::AcquireDirectEgress(
                AcquireDirectEgressRequest {
                    remote_endpoint,
                    protocol: 17,
                    ..
                }
            )) if remote_endpoint == "203.0.113.9:53"
        ));

        let capabilities = AgentCapabilities {
            protocol_version: 3,
            dynamic_direct_egress: true,
            physical_dns_snapshot: true,
            ..AgentCapabilities::default()
        };
        let decoded = AgentCapabilities::decode(&*capabilities.encode_to_vec()).unwrap();
        assert!(decoded.dynamic_direct_egress && decoded.physical_dns_snapshot);
    }

    #[test]
    fn privileged_exact_generation_fields_are_append_only_wire_snapshots() {
        let decoded: AgentRequest =
            decode_frame(Bytes::from_static(AGENT_EXACT_EGRESS_V3_FRAME)).unwrap();
        assert!(
            matches!(decoded.payload.as_ref(), Some(agent_request::Payload::AcquireDirectEgress(request))
            if request.expected_generation == 7 && request.protocol == 17 && request.remote_endpoint == "203.0.113.9:443")
        );
        assert_eq!(
            encode_frame(&decoded).unwrap().as_ref(),
            AGENT_EXACT_EGRESS_V3_FRAME
        );
        assert_eq!(
            agent_v1::DirectEgressLease {
                network_generation: 7,
                ..Default::default()
            }
            .encode_to_vec(),
            [0x28, 7]
        );
        assert_eq!(
            agent_v1::PhysicalInterface {
                address_family_mask: 3,
                ..Default::default()
            }
            .encode_to_vec(),
            [0x20, 3]
        );
        assert_eq!(
            AgentCapabilities {
                exact_generation_egress: true,
                ..Default::default()
            }
            .encode_to_vec(),
            [0x60, 1]
        );
    }

    #[test]
    fn privileged_agent_platform_inspection_uses_append_only_field_twenty_five() {
        let decoded: AgentRequest =
            decode_frame(Bytes::from_static(AGENT_INSPECT_PLATFORM_V3_FRAME))
                .expect("decode platform inspection snapshot");
        assert!(matches!(
            decoded.payload,
            Some(agent_request::Payload::InspectPlatformState(
                InspectPlatformStateRequest {}
            ))
        ));
        assert_eq!(
            encode_frame(&decoded).expect("re-encode").as_ref(),
            AGENT_INSPECT_PLATFORM_V3_FRAME
        );
    }

    #[test]
    fn guarded_recovery_uses_append_only_field_twenty_six_and_capability_thirteen() {
        let frame: &[u8] = &[
            0, 0, 0, 14, 0x0a, 2, b'g', b'1', 0x10, 3, 0xd2, 0x01, 5, 0x0a, 1, b'o', 0x10, 7,
        ];
        let request: AgentRequest = decode_frame(Bytes::copy_from_slice(frame)).unwrap();
        assert!(
            matches!(request.payload.as_ref(), Some(agent_request::Payload::RecoverOrphaned(value))
            if value.operation_id == "o" && value.expected_journal_generation == 7)
        );
        assert_eq!(encode_frame(&request).unwrap().as_ref(), frame);
        assert_eq!(
            AgentCapabilities {
                guarded_recovery: true,
                ..Default::default()
            }
            .encode_to_vec(),
            [0x68, 1]
        );
        assert!(
            !AgentCapabilities::decode(&[0x48, 3][..])
                .unwrap()
                .guarded_recovery
        );
        let legacy = AgentRequest {
            request_id: "r1".to_owned(),
            protocol_version: 3,
            payload: Some(agent_request::Payload::Recover(agent_v1::RecoverRequest {})),
        };
        assert_eq!(
            legacy.encode_to_vec(),
            [0x0a, 2, b'r', b'1', 0x10, 3, 0x92, 0x01, 0]
        );
    }

    #[test]
    fn automatic_recovery_uses_append_only_fields_without_bumping_v3() {
        let frame: &[u8] = &[
            0, 0, 0, 14, 0x0a, 2, b'a', b'1', 0x10, 3, 0xda, 0x01, 5, 0x0a, 1, b'o', 0x10, 7,
        ];
        let request: AgentRequest = decode_frame(Bytes::copy_from_slice(frame)).unwrap();
        assert!(
            matches!(request.payload.as_ref(), Some(agent_request::Payload::RestartAutomaticRecovery(value))
            if value.operation_id == "o" && value.expected_journal_generation == 7)
        );
        assert_eq!(encode_frame(&request).unwrap().as_ref(), frame);
        let legacy_request = LegacyAgentEnvelope::decode(&frame[4..]).unwrap();
        assert_eq!(legacy_request.request_id, "a1");
        assert_eq!(legacy_request.protocol_version, 3);
        assert_eq!(
            AgentCapabilities {
                automatic_recovery: true,
                ..Default::default()
            }
            .encode_to_vec(),
            [0x70, 1]
        );
        assert!(
            !AgentCapabilities::decode(&[0x68, 1][..])
                .unwrap()
                .automatic_recovery
        );
        let legacy_capabilities = LegacyAgentCapabilities::decode(
            AgentCapabilities {
                protocol_version: 3,
                guarded_recovery: true,
                automatic_recovery: true,
                ..Default::default()
            }
            .encode_to_vec()
            .as_slice(),
        )
        .unwrap();
        assert_eq!(legacy_capabilities.protocol_version, 3);
        assert!(legacy_capabilities.guarded_recovery);

        let status = agent_v1::AutomaticRecoveryStatus {
            phase: agent_v1::AutomaticRecoveryPhase::Exhausted as i32,
            attempts_completed: 3,
            attempt_limit: 3,
            terminal_error: None,
        };
        assert_eq!(
            AgentState {
                automatic_recovery: Some(status.clone()),
                ..Default::default()
            }
            .encode_to_vec(),
            [0x4a, 6, 0x08, 4, 0x10, 3, 0x18, 3]
        );
        let legacy_state = LegacyAgentState::decode(
            AgentState {
                phase: agent_v1::AgentPhase::RecoveryRequired as i32,
                journal_generation: 9,
                automatic_recovery: Some(status.clone()),
                ..Default::default()
            }
            .encode_to_vec()
            .as_slice(),
        )
        .unwrap();
        assert_eq!(
            legacy_state.phase,
            agent_v1::AgentPhase::RecoveryRequired as i32
        );
        assert_eq!(legacy_state.journal_generation, 9);
        assert_eq!(
            agent_v1::PlatformState {
                automatic_recovery: Some(status),
                ..Default::default()
            }
            .encode_to_vec(),
            [0x8a, 0x01, 6, 0x08, 4, 0x10, 3, 0x18, 3]
        );
    }

    #[test]
    fn v1_control_request_wire_snapshot_is_stable() {
        let decoded: ControlRequest =
            decode_frame(Bytes::from_static(GET_STATUS_V1_FRAME)).expect("decode snapshot");
        assert_eq!(decoded.request_id, "r1");
        assert!(matches!(
            decoded.payload,
            Some(control_request::Payload::GetStatus(_))
        ));
        assert_eq!(
            encode_frame(&decoded).expect("re-encode").as_ref(),
            GET_STATUS_V1_FRAME
        );
    }

    #[test]
    fn network_quality_append_only_wire_fixture_round_trips() {
        let decoded: crate::v1::ControlResponse =
            decode_frame(Bytes::from_static(NETWORK_QUALITY_V1_FRAME))
                .expect("decode network quality fixture");
        let snapshot = match decoded.payload.as_ref() {
            Some(crate::v1::control_response::Payload::NetworkQuality(snapshot)) => snapshot,
            payload => panic!("unexpected payload: {payload:?}"),
        };
        assert_eq!(snapshot.sampled_at_unix_ms, 1234);
        assert_eq!(snapshot.connection_instance_id, "c1");
        assert_eq!(snapshot.level, crate::v1::NetworkQualityLevel::Good as i32);
        let metrics = snapshot.metrics.as_ref().expect("metrics");
        assert_eq!(metrics.current_smoothed_rtt_milliseconds, 42);
        assert!(metrics.current_smoothed_rtt_known);
        assert!(!metrics.latest_rtt_known);
        assert_eq!(metrics.min_rtt_ms, 21);
        assert_eq!(
            metrics.smoothed_rtt_availability,
            crate::v1::MetricAvailability::Available as i32
        );
        assert_eq!(snapshot.queues.len(), 1);
        assert_eq!(snapshot.queues[0].current_bytes, 100);
        assert_eq!(snapshot.queues[0].drop_items, 1);
        assert_eq!(snapshot.queues[0].enqueue_count, 10);
        assert_eq!(
            encode_frame(&decoded).expect("re-encode").as_ref(),
            NETWORK_QUALITY_V1_FRAME
        );
    }

    #[test]
    fn latest_rtt_append_only_tags_are_independent_of_smoothed_rtt() {
        let metrics = crate::v1::ConnectionMetrics {
            current_smoothed_rtt_milliseconds: 42,
            current_smoothed_rtt_known: true,
            latest_rtt_ms: 7,
            latest_rtt_known: true,
            latest_rtt_availability: crate::v1::MetricAvailability::Available as i32,
            ..Default::default()
        };
        let wire = metrics.encode_to_vec();
        assert_eq!(
            wire,
            [0x20, 42, 0x28, 1, 0x80, 4, 7, 0x88, 4, 1, 0x90, 4, 1]
        );
        let decoded = crate::v1::ConnectionMetrics::decode(wire.as_slice()).unwrap();
        assert_eq!(decoded.latest_rtt_ms, 7);
        assert_eq!(decoded.current_smoothed_rtt_milliseconds, 42);
    }

    #[test]
    fn legacy_decoder_ignores_the_new_network_quality_response() {
        let legacy = LegacyControlResponse::decode(&NETWORK_QUALITY_V1_FRAME[4..])
            .expect("legacy decoder ignores append-only field 21");
        assert_eq!(legacy.request_id, "n1");
    }

    #[test]
    fn unknown_network_quality_enum_is_retained_as_an_unknown_wire_value() {
        let encoded = crate::v1::NetworkQualitySnapshot {
            level: 99,
            ..crate::v1::NetworkQualitySnapshot::default()
        }
        .encode_to_vec();
        let decoded = crate::v1::NetworkQualitySnapshot::decode(encoded.as_slice()).unwrap();
        assert_eq!(decoded.level, 99);
        assert!(crate::v1::NetworkQualityLevel::try_from(decoded.level).is_err());
    }

    #[test]
    fn diagnostics_request_uses_append_only_field_thirty_six() {
        let decoded: ControlRequest = decode_frame(Bytes::from_static(START_DIAGNOSTICS_V1_FRAME))
            .expect("decode diagnostics snapshot");
        assert!(matches!(
            decoded.payload.as_ref(),
            Some(control_request::Payload::StartDiagnostics(StartDiagnosticsRequest {
                mode,
            })) if *mode == DiagnosticMode::Standard as i32
        ));
        assert_eq!(
            encode_frame(&decoded).expect("re-encode").as_ref(),
            START_DIAGNOSTICS_V1_FRAME
        );
    }

    #[test]
    fn v1_event_wire_snapshot_is_stable() {
        let decoded: EventEnvelope =
            decode_frame(Bytes::from_static(WARNING_V1_FRAME)).expect("decode snapshot");
        assert_eq!(decoded.sequence, 7);
        assert!(matches!(
            decoded.payload.as_ref(),
            Some(event_envelope::Payload::WarningRaised(WarningRaised {
                code,
                message
            })) if code == "LAN_EXPOSED" && message == "warning"
        ));
        assert_eq!(
            encode_frame(&decoded).expect("re-encode").as_ref(),
            WARNING_V1_FRAME
        );
    }

    #[test]
    fn v1_identity_provisioning_wire_snapshot_is_stable() {
        let decoded: ControlRequest =
            decode_frame(Bytes::from_static(PROVISION_IDENTITY_V1_FRAME)).expect("decode snapshot");
        assert_eq!(decoded.request_id, "p1");
        assert!(matches!(
            decoded.payload.as_ref(),
            Some(control_request::Payload::ProvisionIdentity(request))
                if request.profile_id == "id"
                    && request.warp_secret == b"x"
                    && request.terms_accepted
                    && request.locale == "en"
                    && request.device_name == "pc"
        ));
        assert_eq!(
            encode_frame(&decoded).expect("re-encode").as_ref(),
            PROVISION_IDENTITY_V1_FRAME
        );
    }

    #[test]
    fn zero_trust_identity_fields_are_append_only_wire_snapshots() {
        let decoded: ControlRequest =
            decode_frame(Bytes::from_static(PROVISION_ZERO_TRUST_V2_FRAME))
                .expect("decode Zero Trust snapshot");
        assert!(matches!(
            decoded.payload.as_ref(),
            Some(control_request::Payload::ProvisionIdentity(ProvisionIdentityRequest {
                profile_id,
                method,
                zero_trust: Some(ZeroTrustEnrollment {
                    team_name,
                    callback_uri,
                }),
                ..
            })) if profile_id == "id"
                && *method == IdentityProvisioningMethod::RegisterZeroTrust as i32
                && team_name == "t"
                && callback_uri == b"c"
        ));
        assert_eq!(
            encode_frame(&decoded).expect("re-encode").as_ref(),
            PROVISION_ZERO_TRUST_V2_FRAME
        );

        let status = crate::v1::ProfileIdentityStatus {
            profile_id: "p".to_owned(),
            state: crate::v1::ProfileIdentityState::Ready as i32,
            license_state: crate::v1::LicenseState::NotApplicable as i32,
            account_type: "Zero Trust".to_owned(),
            provider: crate::v1::IdentityProvider::ZeroTrust as i32,
            organization: "t".to_owned(),
            ..Default::default()
        };
        assert_eq!(
            status.encode_to_vec(),
            [
                0x0a, 1, b'p', 0x10, 1, 0x18, 5, 0x22, 10, b'Z', b'e', b'r', b'o', b' ', b'T',
                b'r', b'u', b's', b't', 0x30, 2, 0x3a, 1, b't'
            ]
        );
    }

    #[test]
    fn v1_create_profile_with_identity_uses_append_only_field_twenty_six() {
        let decoded: ControlRequest =
            decode_frame(Bytes::from_static(CREATE_PROFILE_WITH_IDENTITY_V1_FRAME))
                .expect("decode create profile snapshot");
        assert_eq!(decoded.request_id, "c1");
        assert!(matches!(
            decoded.payload.as_ref(),
            Some(control_request::Payload::CreateProfileWithIdentity(request))
                if matches!(
                    request.as_ref(),
                    CreateProfileWithIdentityRequest {
                        profile: Some(Profile { id, name, .. }),
                        identity: Some(IdentityProvisioning {
                            method,
                            terms_accepted: true,
                            locale,
                            ..
                        }),
                    } if id == "id"
                        && name == "n"
                        && *method == IdentityProvisioningMethod::Register as i32
                        && locale == "en"
                )
        ));
        assert_eq!(
            encode_frame(&decoded).expect("re-encode").as_ref(),
            CREATE_PROFILE_WITH_IDENTITY_V1_FRAME
        );
    }

    #[test]
    fn composable_frontends_and_runtime_status_use_append_only_field_fifteen() {
        let profile = Profile {
            id: "p".to_owned(),
            frontends: Some(FrontendSettings {
                tunnel: true,
                socks5: true,
                http: false,
            }),
            ..Profile::default()
        };
        assert_eq!(
            profile.encode_to_vec(),
            [0x0a, 1, b'p', 0x7a, 4, 0x08, 1, 0x10, 1]
        );

        let legacy = Profile::decode(&*profile.encode_to_vec()).expect("decode without field 16");
        assert!(legacy.geo_direct_countries.is_empty());

        let with_geo = Profile {
            id: "p".to_owned(),
            geo_direct_countries: vec!["CN".to_owned()],
            ..Profile::default()
        };
        let decoded = Profile::decode(&*with_geo.encode_to_vec()).expect("decode field 16");
        assert_eq!(decoded.geo_direct_countries, ["CN"]);

        let snapshot = crate::v1::ConnectionSnapshot {
            frontends: vec![FrontendStatus {
                kind: FrontendKind::Socks5 as i32,
                phase: FrontendPhase::Active as i32,
                listeners: vec!["l".to_owned()],
                error: None,
            }],
            ..crate::v1::ConnectionSnapshot::default()
        };
        assert_eq!(
            snapshot.encode_to_vec(),
            [0x7a, 7, 0x08, 2, 0x10, 3, 0x1a, 1, b'l']
        );
    }

    #[test]
    fn reconfigure_license_and_secret_export_requests_are_append_only() {
        let reconfigure = ControlRequest {
            request_id: "x".to_owned(),
            payload: Some(control_request::Payload::ReconfigureActiveProfile(
                Box::new(ReconfigureActiveProfileRequest {
                    profile: Some(Profile {
                        id: "p".to_owned(),
                        ..Profile::default()
                    }),
                }),
            )),
        };
        assert_eq!(
            encode_frame(&reconfigure).unwrap().as_ref(),
            RECONFIGURE_V2_FRAME
        );

        let update = ControlRequest {
            request_id: "x".to_owned(),
            payload: Some(control_request::Payload::UpdateLicenseKey(
                UpdateLicenseKeyRequest {
                    profile_id: "p".to_owned(),
                    license_key: b"k".to_vec(),
                },
            )),
        };
        assert_eq!(
            encode_frame(&update).unwrap().as_ref(),
            UPDATE_LICENSE_V2_FRAME
        );

        let export = ControlRequest {
            request_id: "x".to_owned(),
            payload: Some(control_request::Payload::ExportWarpSecret(
                ExportWarpSecretRequest {
                    profile_id: "p".to_owned(),
                    destination: "d".to_owned(),
                    confirmed: true,
                },
            )),
        };
        assert_eq!(
            encode_frame(&export).unwrap().as_ref(),
            EXPORT_WARP_SECRET_V2_FRAME
        );

        let update_auth = ControlRequest {
            request_id: "x".to_owned(),
            payload: Some(control_request::Payload::UpdateProxyAuth(
                UpdateProxyAuthRequest {
                    profile_id: "p".to_owned(),
                    username: "u".to_owned(),
                    password: b"k".to_vec(),
                    confirmed: true,
                },
            )),
        };
        assert_eq!(
            encode_frame(&update_auth).unwrap().as_ref(),
            UPDATE_PROXY_AUTH_V1_FRAME
        );
    }

    #[test]
    fn v1_capabilities_event_wire_snapshot_is_stable() {
        let decoded: EventEnvelope =
            decode_frame(Bytes::from_static(CAPABILITIES_V1_FRAME)).expect("decode snapshot");
        assert!(matches!(
            decoded.payload.as_ref(),
            Some(event_envelope::Payload::CapabilitiesChanged(CapabilitiesChanged {
                capabilities: Some(Capabilities {
                    socks5: true,
                    http_proxy: true,
                    operating_system,
                    architecture,
                    transports,
                    secure_storage: true,
                    ..
                })
            })) if operating_system == "windows"
                && architecture == "x86_64"
                && transports == &["h3", "h2"]
        ));
        assert_eq!(
            encode_frame(&decoded).expect("re-encode").as_ref(),
            CAPABILITIES_V1_FRAME
        );
    }

    #[test]
    fn v1_legacy_profile_import_wire_snapshot_is_stable() {
        let decoded: ControlRequest =
            decode_frame(Bytes::from_static(IMPORT_LEGACY_PROFILES_V1_FRAME))
                .expect("decode snapshot");
        assert_eq!(decoded.request_id, "m1");
        assert!(matches!(
            decoded.payload.as_ref(),
            Some(control_request::Payload::ImportLegacyProfiles(request))
                if request.profiles.is_empty() && request.active_profile_id == "id"
        ));
        assert_eq!(
            encode_frame(&decoded).expect("re-encode").as_ref(),
            IMPORT_LEGACY_PROFILES_V1_FRAME
        );
    }

    #[test]
    fn stream_splitter_retains_partial_data_and_yields_multiple_frames() {
        let first = Bytes::from_static(GET_STATUS_V1_FRAME);
        let second = Bytes::from_static(WARNING_V1_FRAME);
        let mut stream = BytesMut::new();
        stream.extend_from_slice(&first[..3]);
        assert!(split_frame(&mut stream).expect("partial").is_none());
        assert_eq!(stream.as_ref(), &first[..3]);

        stream.extend_from_slice(&first[3..]);
        stream.extend_from_slice(&second);
        assert_eq!(
            split_frame(&mut stream).expect("first"),
            Some(first.clone())
        );
        assert_eq!(
            split_frame(&mut stream).expect("second"),
            Some(second.clone())
        );
        assert!(stream.is_empty());
    }

    #[test]
    fn stream_splitter_rejects_oversized_length_without_consuming() {
        let declared = (MAX_FRAME_SIZE as u32 + 1).to_be_bytes();
        let mut stream = BytesMut::from(declared.as_slice());
        let original = stream.clone();
        assert!(matches!(
            split_frame(&mut stream),
            Err(FrameError::TooLarge(_))
        ));
        assert_eq!(stream, original);
    }
}
