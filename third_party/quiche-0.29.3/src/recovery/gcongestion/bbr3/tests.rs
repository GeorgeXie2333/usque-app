// Copyright (c) 2026 The Usque contributors.
// SPDX-License-Identifier: BSD-3-Clause
use super::*;

const MSS: usize = 1200;

/// Deterministic FIFO bottleneck, real sender pacing and cwnd admission.
/// All times advance virtually; no sockets or wall-clock sleeps are used.
struct Link {
    cc: BBRv3,
    now: Instant,
    next_send: Instant,
    wire_free: Instant,
    bandwidth: Bandwidth,
    rtt: Duration,
    flight: VecDeque<(Instant, Acked, bool)>,
    inflight: usize,
    packet: u64,
    drop_every: Option<u64>,
    app_rate: Option<Bandwidth>,
    aggregation: Duration,
    origin: Instant,
    policer: Option<(Bandwidth, f64, f64, Instant)>,
}

impl Link {
    fn new() -> Self {
        let cc = BBRv3::new(10, 20_000, MSS, Duration::from_millis(50));
        let now = cc.min_rtt_stamp;
        Self {
            cc,
            now,
            next_send: now,
            wire_free: now,
            bandwidth: Bandwidth::from_mbits_per_second(10),
            rtt: Duration::from_millis(50),
            flight: VecDeque::new(),
            inflight: 0,
            packet: 0,
            drop_every: None,
            app_rate: None,
            aggregation: Duration::ZERO,
            origin: now,
            policer: None,
        }
    }

    fn step(&mut self) {
        let can_send = self.inflight + MSS <= self.cc.cwnd;
        if can_send
            && self
                .flight
                .front()
                .is_none_or(|(at, _, _)| self.next_send <= *at)
        {
            self.now = self.now.max(self.next_send);
            if self.app_rate.is_some() {
                self.cc.on_app_limited(self.inflight);
            }
            self.cc.on_packet_sent(
                self.now,
                self.inflight,
                self.packet,
                MSS,
                true,
            );
            self.inflight += MSS;
            let rate = self
                .app_rate
                .map_or(self.cc.pacing, |rate| rate.min(self.cc.pacing));
            self.next_send = self.now + rate.transfer_time(MSS as u64);
            self.wire_free = self.now.max(self.wire_free)
                + self.bandwidth.transfer_time(MSS as u64);
            let mut lost =
                self.drop_every.is_some_and(|n| self.packet % n == 0);
            if let Some((rate, tokens, burst, stamp)) = &mut self.policer {
                *tokens = (*tokens
                    + rate.to_bytes_per_period(
                        self.now.saturating_duration_since(*stamp),
                    ) as f64)
                    .min(*burst);
                *stamp = self.now;
                if *tokens >= MSS as f64 {
                    *tokens -= MSS as f64;
                } else {
                    lost = true;
                }
            }
            let mut arrival = self.wire_free + self.rtt;
            if !self.aggregation.is_zero() {
                let interval = self.aggregation.as_micros();
                let ticks = arrival
                    .saturating_duration_since(self.origin)
                    .as_micros()
                    .div_ceil(interval);
                arrival = self.origin
                    + Duration::from_micros((ticks * interval) as u64);
            }
            self.flight.push_back((
                arrival,
                Acked {
                    pkt_num: self.packet,
                    time_sent: self.now,
                },
                lost,
            ));
            self.packet += 1;
        } else {
            let (at, ack, lost) =
                self.flight.pop_front().expect("sender must not stall");
            self.now = self.now.max(at);
            let prior = self.inflight;
            self.inflight -= MSS;
            let lost_packet = Lost {
                packet_number: ack.pkt_num,
                bytes_lost: MSS,
            };
            let least = self
                .flight
                .front()
                .map_or(self.packet, |(_, ack, _)| ack.pkt_num);
            self.cc.on_congestion_event(
                true,
                prior,
                self.inflight,
                self.now,
                if lost {
                    &[]
                } else {
                    std::slice::from_ref(&ack)
                },
                if lost {
                    std::slice::from_ref(&lost_packet)
                } else {
                    &[]
                },
                least,
                &RttStats::new(self.rtt, Duration::ZERO),
                &mut RecoveryStats::default(),
            );
        }
        assert!(self.cc.cwnd >= 4 * MSS);
        assert!(self.cc.cwnd <= self.cc.max_cwnd);
        assert!(self.cc.pacing > Bandwidth::zero());
    }

    fn reach(&mut self, target: State) {
        for _ in 0..100_000 {
            if self.cc.state == target {
                return;
            }
            self.step();
        }
        panic!(
            "did not reach {target:?}; state {:?}, bw {:?}",
            self.cc.state,
            self.cc.max_bandwidth()
        );
    }

    fn run_for(&mut self, duration: Duration) {
        let end = self.now + duration;
        for _ in 0..1_000_000 {
            if self.now >= end {
                return;
            }
            self.step();
        }
        panic!("simulation work budget exceeded");
    }
}

#[test]
fn appendix_a1_a3_startup_plateau_and_drain() {
    let mut link = Link::new();
    link.reach(State::Drain);
    assert!(link.cc.full_bw_reached);
    assert_eq!(link.cc.full_bw_count, 3);
    let measured = link.cc.max_bandwidth().to_bytes_per_second() as f64;
    let expected = link.bandwidth.to_bytes_per_second() as f64;
    assert!((measured / expected - 1.0).abs() < 0.02);
    assert_eq!(link.cc.state.gains().0, 0.5);
    link.reach(State::Down);
}

#[test]
fn appendix_a5_a8_probe_up_plateau_and_drain_to_cruise() {
    let mut link = Link::new();
    link.reach(State::Up);
    link.reach(State::Down);
    assert!(link.cc.full_bw_now);
    assert_eq!(link.cc.state.gains().0, 0.9);
    link.reach(State::Cruise);
}

#[test]
fn low_bandwidth_has_a_four_packet_floor_and_bounded_quantum() {
    let mut link = Link::new();
    link.bandwidth = Bandwidth::from_kbits_per_second(64);
    link.rtt = Duration::from_millis(1);
    link.reach(State::Cruise);
    assert!(link.cc.cwnd >= 4 * MSS);
    assert!((2 * MSS..=64 * 1024).contains(&link.cc.send_quantum()));
}

#[test]
fn mss_change_does_not_invent_bandwidth_or_zero_the_window() {
    let mut link = Link::new();
    link.reach(State::Cruise);
    let rate = link.cc.max_bandwidth();
    link.cc.update_mss(1400);
    assert_eq!(link.cc.max_bandwidth(), rate);
    assert!(link.cc.cwnd >= 5600);
    link.cc.update_mss(1200);
    assert_eq!(link.cc.max_bandwidth(), rate);
}

#[test]
fn loss_changes_bbr3_model_without_changing_bbr2() {
    let mut link = Link::new();
    link.reach(State::Up);
    link.drop_every = Some(10);
    for _ in 0..20_000 {
        link.step();
        if link.cc.prev_probe_too_high {
            break;
        }
    }
    assert!(link.cc.inflight_longterm < usize::MAX);
    assert!(link.cc.prev_probe_too_high);
}

#[test]
fn appendix_a2_application_limited_startup_still_exits_on_high_loss() {
    let mut link = Link::new();
    link.cc = BBRv3::new(100, 20_000, MSS, link.rtt);
    link.now = link.cc.min_rtt_stamp;
    link.next_send = link.now;
    link.app_rate = Some(Bandwidth::from_mbits_per_second(5));
    link.drop_every = Some(4);
    link.reach(State::Drain);
    assert!(link.cc.full_bw_reached);
    assert!(link.cc.inflight_longterm < usize::MAX);
}

#[test]
fn appendix_a4_drain_has_a_three_round_bound() {
    let mut link = Link::new();
    link.reach(State::Drain);
    // A temporarily underestimated RTT keeps inflight above the drain target.
    link.cc.min_rtt = Duration::from_micros(1);
    let start = link.cc.drain_start_round;
    link.reach(State::Down);
    assert!(link.cc.round_count - start <= 3);
}

#[test]
fn appendix_a6_application_limited_probe_exits_on_loss() {
    let mut link = Link::new();
    link.reach(State::Up);
    link.app_rate = Some(Bandwidth::from_mbits_per_second(1));
    link.drop_every = Some(2);
    link.reach(State::Down);
    assert!(link.cc.prev_probe_too_high);
    assert!(!link.cc.is_probe_sample);
}

#[test]
fn appendix_a7_app_limited_samples_do_not_declare_a_plateau() {
    let mut link = Link::new();
    link.app_rate = Some(Bandwidth::from_kbits_per_second(64));
    link.run_for(Duration::from_secs(2));
    assert_eq!(link.cc.state, State::Startup);
    assert!(!link.cc.full_bw_reached);
    let mut link = Link::new();
    link.reach(State::Up);
    for _ in 0..10 {
        link.cc
            .check_full_bw(Bandwidth::from_kbits_per_second(64), true, true);
    }
    assert!(!link.cc.full_bw_now);
}

#[test]
fn appendix_a9_probe_down_times_out_to_refill_without_draining() {
    let mut link = Link::new();
    link.reach(State::Down);
    link.cc.update_probe_bw(
        link.now + link.cc.probe_wait + Duration::from_millis(1),
        false,
        false,
        link.cc.max_bandwidth(),
        SendTimeState::default(),
        0,
        usize::MAX,
    );
    assert_eq!(link.cc.state, State::Refill);
}

#[test]
fn appendix_a10_probe_rtt_waits_for_duration_and_round() {
    let mut link = Link::new();
    link.reach(State::ProbeRtt);
    assert_eq!(link.cc.state.gains().1, 0.5);
    while link.cc.probe_rtt_done_stamp.is_none() {
        link.step();
    }
    let done = link.cc.probe_rtt_done_stamp.unwrap();
    while link.cc.state == State::ProbeRtt {
        link.step();
    }
    assert!(link.now > done);
    assert!(link.cc.state.is_probe_bw());
}

#[test]
fn appendix_a11_idle_restart_avoids_an_unnecessary_probe_rtt() {
    let mut link = Link::new();
    link.reach(State::Cruise);
    link.cc.on_app_limited(0);
    let now = link.now + Duration::from_secs(6);
    link.cc.on_packet_sent(now, 0, link.packet, MSS, true);
    assert!(link.cc.idle_restart);
    link.cc.update_rtt(now + link.rtt, Some(link.rtt), 0, false);
    assert_ne!(link.cc.state, State::ProbeRtt);
}

#[test]
fn appendix_a12_a13_ack_aggregation_is_bounded_and_does_not_overestimate_rate()
{
    let mut link = Link::new();
    link.aggregation = Duration::from_millis(10);
    link.reach(State::Cruise);
    assert!(link.cc.max_bandwidth() <= link.bandwidth * 1.02f64);
    assert!(link.cc.max_bandwidth() >= link.bandwidth * 0.98f64);
    assert!(!link.cc.extra_acked.is_empty());
    assert!(link.cc.extra_acked.len() <= 10);
    assert!(link
        .cc
        .extra_acked
        .iter()
        .any(|(_, extra)| *extra > 2 * MSS));
}

#[test]
fn appendix_a15_bandwidth_increase_is_discovered() {
    let mut link = Link::new();
    link.reach(State::Cruise);
    link.bandwidth = Bandwidth::from_mbits_per_second(100);
    link.run_for(Duration::from_secs(6));
    assert!(link.cc.max_bandwidth() >= link.bandwidth * 0.98f64);
}

#[test]
fn appendix_a16_bandwidth_decrease_expires_the_old_maximum() {
    let mut link = Link::new();
    link.reach(State::Cruise);
    link.bandwidth = Bandwidth::from_mbits_per_second(1);
    link.drop_every = Some(10);
    link.run_for(Duration::from_secs(20));
    assert!(link.cc.max_bandwidth() <= link.bandwidth * 1.1f64);
}

#[test]
fn appendix_a17_token_bucket_loss_adapts_shortterm_model() {
    let mut link = Link::new();
    link.reach(State::Cruise);
    let rate = Bandwidth::from_mbits_per_second(1);
    let burst = (10 * MSS) as f64;
    link.policer = Some((rate, burst, burst, link.now));
    link.run_for(Duration::from_secs(15));
    assert!(link.cc.sampler.total_bytes_lost() > 0);
    assert!(link.cc.max_bandwidth() < link.bandwidth * 0.5f64);
    assert!(link.cc.sampler.total_bytes_acked() > 100 * MSS);
}

#[test]
fn appendix_a18_spurious_undo_requires_every_loss_in_the_episode() {
    let mut link = Link::new();
    link.reach(State::Up);
    let original_cwnd = link.cc.cwnd;
    // Unit fixture for transport-confirmed spurious recovery, separate from
    // congestion sampling. Both original packet numbers must be acknowledged.
    link.cc.undo = Some(Undo {
        state: State::Up,
        cwnd: original_cwnd,
        bw_shortterm: Bandwidth::infinite(),
        inflight_shortterm: usize::MAX,
        inflight_longterm: usize::MAX,
        lost_packets: [40, 42].into_iter().collect(),
    });
    link.cc.enter_down(link.now);
    link.cc.bw_shortterm = Bandwidth::from_kbits_per_second(64);
    let mut ranges = RangeSet::default();
    ranges.insert(40..41);
    link.cc.acknowledge_spurious_losses(&ranges, link.now);
    assert!(link.cc.undo.is_some());
    assert_eq!(link.cc.state, State::Down);
    ranges.insert(42..43);
    link.cc.acknowledge_spurious_losses(&ranges, link.now);
    assert!(link.cc.undo.is_none());
    assert_eq!(link.cc.state, State::Refill);
    assert_eq!(link.cc.bw_shortterm, Bandwidth::infinite());
}

#[test]
fn appendix_a19_quic_pto_is_not_tcp_rto_and_cannot_collapse_cwnd() {
    let mut link = Link::new();
    link.reach(State::Up);
    let before = (link.cc.cwnd, link.cc.pacing, link.cc.state);
    link.cc.on_retransmission_timeout(true);
    assert_eq!((link.cc.cwnd, link.cc.pacing, link.cc.state), before);
}

#[test]
fn appendix_a20_probe_rtt_during_startup_returns_to_startup() {
    let mut link = Link::new();
    link.cc.update_rtt(
        link.now + Duration::from_secs(6),
        Some(link.rtt),
        0,
        false,
    );
    assert_eq!(link.cc.state, State::ProbeRtt);
    link.cc.update_rtt(
        link.now + Duration::from_millis(6201),
        Some(link.rtt),
        0,
        true,
    );
    assert_eq!(link.cc.state, State::Startup);
    assert!(!link.cc.full_bw_reached);
}

#[test]
fn appendix_a21_a22_existing_longterm_bound_and_app_limited_refill() {
    let mut link = Link::new();
    link.reach(State::Up);
    link.drop_every = Some(10);
    link.reach(State::Down);
    assert!(link.cc.inflight_longterm < usize::MAX);
    link.drop_every = None;
    link.reach(State::Refill);
    let before = link.cc.max_bandwidth();
    link.cc.update_probe_bw(
        link.now,
        true,
        true,
        before * 0.1f64,
        SendTimeState::default(),
        MSS,
        link.inflight,
    );
    assert_eq!(link.cc.state, State::Up);
    assert_eq!(link.cc.max_bandwidth(), before);
}
