// Copyright (c) 2026 The Usque contributors.
// SPDX-License-Identifier: BSD-2-Clause

use std::ops::Range;

use rstest::rstest;

use super::*;
use crate::frame;
use crate::packet::Epoch;
use crate::ranges::RangeSet;
use crate::recovery::OnAckReceivedOutcome;
use crate::recovery::RecoveryConfig;
use crate::recovery::Sent;

const PACKET_BYTES: usize = 1200;

fn make_path(algorithm: &str) -> Path {
    let mut config = Config::new(crate::PROTOCOL_VERSION).unwrap();
    config.set_cc_algorithm_name(algorithm).unwrap();
    config.set_initial_rtt(Duration::from_millis(100));
    config.set_max_send_udp_payload_size(PACKET_BYTES);
    let recovery_config = RecoveryConfig::from_config(&config);
    Path::new(
        "127.0.0.1:1234".parse().unwrap(),
        "127.0.0.1:4321".parse().unwrap(),
        &recovery_config,
        config.path_challenge_recv_max_queue_len,
        true,
        None,
    )
}

fn send_packet(path: &mut Path, number: u64, now: Instant) {
    path.recovery.on_packet_sent(
        Sent {
            pkt_num: number,
            frames: Default::default(),
            time_sent: now,
            time_acked: None,
            time_lost: None,
            size: PACKET_BYTES,
            ack_eliciting: true,
            in_flight: true,
            delivered: 0,
            delivered_time: now,
            first_sent_time: now,
            is_app_limited: false,
            tx_in_flight: 0,
            lost: 0,
            has_data: false,
            is_pmtud_probe: false,
        },
        Epoch::Application,
        HandshakeStatus::default(),
        now,
        "",
    );
}

fn acknowledge(
    path: &mut Path, range: Range<u64>, now: Instant,
) -> OnAckReceivedOutcome {
    let mut ranges = RangeSet::default();
    ranges.insert(range);
    path.recovery
        .on_ack_received(
            &ranges,
            0,
            Epoch::Application,
            HandshakeStatus::default(),
            now,
            None,
            "",
        )
        .unwrap()
}

fn expire_timer(path: &mut Path) -> (Instant, OnLossDetectionTimeoutOutcome) {
    let deadline = path.recovery.loss_detection_timer().unwrap();
    let outcome = path.on_loss_detection_timeout(
        HandshakeStatus::default(),
        deadline,
        false,
        "",
    );
    (deadline, outcome)
}

#[rstest]
fn time_threshold_loss_timer_does_not_increment_path_pto_count(
    #[values("cubic", "reno", "bbr2_gcongestion", "bbr3")] algorithm: &str,
) {
    let mut path = make_path(algorithm);
    let sent_at = Instant::now();
    send_packet(&mut path, 0, sent_at);
    send_packet(&mut path, 1, sent_at);

    // Acknowledge only the later packet. The gap is below the packet-loss
    // threshold, and one RTT is below the time-loss threshold. Recovery must
    // arm a loss-time timer for packet zero, not a PTO probe timer.
    let acked = acknowledge(
        &mut path,
        1..2,
        sent_at + Duration::from_millis(100),
    );
    assert_eq!(acked.lost_packets, 0);
    assert_eq!(acked.acked_bytes, PACKET_BYTES);
    assert_eq!(path.recovery.pto_count(), 0);
    assert_eq!(path.total_pto_count, 0);
    assert_eq!(path.loss_detection_timeout_count, 0);
    assert_eq!(path.stats().loss_detection_timeout_count, 0);

    let (_, outcome) = expire_timer(&mut path);
    assert_eq!(outcome.lost_packets, 1);
    assert_eq!(outcome.lost_bytes, PACKET_BYTES);
    assert_eq!(path.recovery.bytes_in_flight(), 0);
    assert_eq!(path.recovery.pto_count(), 0);
    assert_eq!(path.recovery.loss_probes(Epoch::Application), 0);
    assert_eq!(path.recovery.lost_count(), 1);
    assert_eq!(path.total_pto_count, 0);
    assert_eq!(path.stats().total_pto_count, 0);
    assert_eq!(path.loss_detection_timeout_count, 1);
    assert_eq!(path.stats().loss_detection_timeout_count, 1);
}

#[rstest]
fn time_threshold_probe_loss_with_zero_reported_loss_is_not_a_pto(
    #[values("cubic", "reno", "bbr2_gcongestion", "bbr3")] algorithm: &str,
) {
    let mut path = make_path(algorithm);
    let sent_at = Instant::now();
    path.recovery.on_packet_sent(
        Sent {
            pkt_num: 0,
            frames: smallvec::smallvec![frame::Frame::Ping {
                mtu_probe: Some(PACKET_BYTES),
            }],
            time_sent: sent_at,
            time_acked: None,
            time_lost: None,
            size: PACKET_BYTES,
            ack_eliciting: true,
            in_flight: true,
            delivered: 0,
            delivered_time: sent_at,
            first_sent_time: sent_at,
            is_app_limited: false,
            tx_in_flight: 0,
            lost: 0,
            has_data: false,
            is_pmtud_probe: true,
        },
        Epoch::Application,
        HandshakeStatus::default(),
        sent_at,
        "",
    );
    send_packet(&mut path, 1, sent_at);
    let acked = acknowledge(
        &mut path,
        1..2,
        sent_at + Duration::from_millis(100),
    );
    assert_eq!(acked.lost_packets, 0);
    assert_eq!(path.recovery.bytes_in_flight(), PACKET_BYTES);

    // Only the probe is missing. Its real time-threshold loss removes its
    // bytes in flight but is excluded from ordinary congestion-loss totals.
    // Zero reported loss therefore cannot identify a PTO expiration.
    let (_, outcome) = expire_timer(&mut path);
    assert_eq!(outcome.lost_packets, 0);
    assert_eq!(outcome.lost_bytes, 0);
    assert!(!outcome.pto_expired);
    assert_eq!(path.recovery.bytes_in_flight(), 0);
    assert_eq!(path.recovery.lost_count(), 0);
    assert_eq!(path.recovery.pto_count(), 0);
    assert_eq!(path.recovery.loss_probes(Epoch::Application), 0);
    assert_eq!(path.total_pto_count, 0);
    assert_eq!(path.stats().total_pto_count, 0);
    assert_eq!(path.loss_detection_timeout_count, 1);
    assert_eq!(path.stats().loss_detection_timeout_count, 1);
}

#[rstest]
fn ack_driven_packet_loss_does_not_increment_path_timeout_counts(
    #[values("cubic", "reno", "bbr2_gcongestion", "bbr3")] algorithm: &str,
) {
    let mut path = make_path(algorithm);
    let sent_at = Instant::now();
    for number in 0..4 {
        send_packet(&mut path, number, sent_at);
    }

    // ACKing packet three declares packet zero lost by packet threshold,
    // without dispatching a recovery timeout callback.
    let acked = acknowledge(
        &mut path,
        3..4,
        sent_at + Duration::from_millis(100),
    );
    assert_eq!(acked.lost_packets, 1);
    assert_eq!(acked.acked_bytes, PACKET_BYTES);
    assert_eq!(path.recovery.lost_count(), 1);
    assert_eq!(path.recovery.pto_count(), 0);
    assert_eq!(path.total_pto_count, 0);
    assert_eq!(path.stats().total_pto_count, 0);
    assert_eq!(path.loss_detection_timeout_count, 0);
    assert_eq!(path.stats().loss_detection_timeout_count, 0);
}

#[rstest]
fn real_pto_events_count_and_ack_resets_only_consecutive_pto(
    #[values("cubic", "reno", "bbr2_gcongestion", "bbr3")] algorithm: &str,
) {
    let mut path = make_path(algorithm);
    send_packet(&mut path, 0, Instant::now());

    let (_, first) = expire_timer(&mut path);
    assert_eq!(first.lost_packets, 0);
    assert_eq!(first.lost_bytes, 0);
    assert_eq!(path.recovery.pto_count(), 1);
    assert_eq!(path.total_pto_count, 1);
    assert_eq!(path.loss_detection_timeout_count, 1);

    let (second_at, second) = expire_timer(&mut path);
    assert_eq!(second.lost_packets, 0);
    assert_eq!(path.recovery.pto_count(), 2);
    assert_eq!(path.total_pto_count, 2);
    assert_eq!(path.loss_detection_timeout_count, 2);

    let acked_at = second_at + Duration::from_millis(1);
    let acked = acknowledge(&mut path, 0..1, acked_at);
    assert_eq!(acked.acked_bytes, PACKET_BYTES);
    assert_eq!(path.recovery.pto_count(), 0);
    assert_eq!(path.total_pto_count, 2);
    assert_eq!(path.loss_detection_timeout_count, 2);
    assert_eq!(path.stats().loss_detection_timeout_count, 2);

    send_packet(&mut path, 1, acked_at + Duration::from_millis(1));
    let (_, third) = expire_timer(&mut path);
    assert_eq!(third.lost_packets, 0);
    assert_eq!(path.recovery.pto_count(), 1);
    assert_eq!(path.total_pto_count, 3);
    assert_eq!(path.stats().total_pto_count, 3);
    assert_eq!(path.loss_detection_timeout_count, 3);
    assert_eq!(path.stats().loss_detection_timeout_count, 3);
}

#[rstest]
fn loss_timer_between_real_ptos_does_not_add_or_erase_pto_events(
    #[values("cubic", "reno", "bbr2_gcongestion", "bbr3")] algorithm: &str,
) {
    let mut path = make_path(algorithm);
    send_packet(&mut path, 0, Instant::now());
    let (pto_at, _) = expire_timer(&mut path);
    assert_eq!(path.total_pto_count, 1);
    assert_eq!(path.loss_detection_timeout_count, 1);
    let acked_at = pto_at + Duration::from_millis(1);
    acknowledge(&mut path, 0..1, acked_at);
    assert_eq!(path.recovery.pto_count(), 0);
    assert_eq!(path.loss_detection_timeout_count, 1);

    let sent_at = acked_at + Duration::from_millis(1);
    send_packet(&mut path, 1, sent_at);
    send_packet(&mut path, 2, sent_at);
    let acked = acknowledge(
        &mut path,
        2..3,
        sent_at + Duration::from_millis(100),
    );
    assert_eq!(acked.lost_packets, 0);
    let (loss_at, loss) = expire_timer(&mut path);
    assert_eq!(loss.lost_packets, 1);
    assert_eq!(loss.lost_bytes, PACKET_BYTES);
    assert_eq!(path.recovery.pto_count(), 0);
    assert_eq!(path.total_pto_count, 1);
    assert_eq!(path.loss_detection_timeout_count, 2);

    send_packet(&mut path, 3, loss_at + Duration::from_millis(1));
    let (_, pto) = expire_timer(&mut path);
    assert_eq!(pto.lost_packets, 0);
    assert_eq!(path.recovery.pto_count(), 1);
    assert_eq!(path.total_pto_count, 2);
    assert_eq!(path.stats().total_pto_count, 2);
    assert_eq!(path.loss_detection_timeout_count, 3);
    assert_eq!(path.stats().loss_detection_timeout_count, 3);
}
