// Copyright (c) 2026 The Usque contributors.
// SPDX-License-Identifier: BSD-3-Clause
//
// Independent implementation of draft-ietf-ccwg-bbr-06 (6 July 2026),
// sections 4 and 5. This is NOT a parameter profile for BBRv2.
// Code components derived from the draft are subject to the Revised BSD
// license in COPYING-BBR3. See USQUE-PATCH.md for provenance and adaptations.

use std::collections::{BTreeSet, VecDeque};
use std::time::{Duration, Instant};

use super::bbr::{BandwidthSampler, SendTimeState};
use super::{Acked, Bandwidth, CongestionControl, Lost, RttStats};
use crate::recovery::{RangeSet, RecoveryStats};

const STARTUP_GAIN: f64 = 2.77;
const BETA: f64 = 0.7;
const LOSS_THRESHOLD: f64 = 0.02;
const PROBE_RTT_INTERVAL: Duration = Duration::from_secs(5);
const MIN_RTT_WINDOW: Duration = Duration::from_secs(10);
const PROBE_RTT_DURATION: Duration = Duration::from_millis(200);
// Bounded, conservative undo: if evidence is discarded, never infer that a
// recovery was wholly spurious. Loss records contain packet numbers only.
const MAX_UNDO_PACKETS: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Startup,
    Drain,
    Down,
    Cruise,
    Refill,
    Up,
    ProbeRtt,
}

impl State {
    fn is_probe_bw(self) -> bool {
        matches!(self, Self::Down | Self::Cruise | Self::Refill | Self::Up)
    }

    fn is_probing(self) -> bool {
        matches!(self, Self::Startup | Self::Refill | Self::Up)
    }

    fn gains(self) -> (f64, f64) {
        match self {
            Self::Startup => (STARTUP_GAIN, 2.0),
            Self::Drain => (0.5, 2.0),
            Self::Down => (0.9, 2.0),
            Self::Up => (1.25, 2.25),
            Self::Cruise | Self::Refill => (1.0, 2.0),
            Self::ProbeRtt => (1.0, 0.5),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AckPhase {
    Init,
    Refilling,
    Starting,
    Feedback,
    Stopping,
}

#[derive(Debug)]
struct Undo {
    state: State,
    cwnd: usize,
    bw_shortterm: Bandwidth,
    inflight_shortterm: usize,
    inflight_longterm: usize,
    lost_packets: BTreeSet<u64>,
}

#[derive(Debug)]
pub(crate) struct BBRv3 {
    sampler: BandwidthSampler,
    state: State,
    ack_phase: AckPhase,
    mss: usize,
    initial_cwnd: usize,
    max_cwnd: usize,
    cwnd: usize,
    prior_cwnd: usize,
    pacing: Bandwidth,
    max_bw: [Bandwidth; 2],
    bw_shortterm: Bandwidth,
    inflight_shortterm: usize,
    inflight_longterm: usize,
    min_rtt: Duration,
    min_rtt_stamp: Instant,
    probe_rtt_min_delay: Duration,
    probe_rtt_min_stamp: Instant,
    probe_rtt_done_stamp: Option<Instant>,
    probe_rtt_round_done: bool,
    idle_restart: bool,
    next_round_delivered: usize,
    round_count: u64,
    drain_start_round: u64,
    rounds_since_probe: u64,
    cycle_stamp: Instant,
    probe_wait: Duration,
    full_bw: Bandwidth,
    full_bw_count: u8,
    full_bw_now: bool,
    full_bw_reached: bool,
    is_probe_sample: bool,
    prev_probe_too_high: bool,
    prev_probe_precautionary: bool,
    probe_up_rounds: u32,
    probe_up_acked: usize,
    probe_up_acked_per_inc: usize,
    cwnd_limited_in_round: bool,
    loss_round_delivered: usize,
    loss_in_round: bool,
    bw_latest: Bandwidth,
    inflight_latest: usize,
    startup_loss_ranges: usize,
    last_lost_packet: Option<u64>,
    startup_lost: usize,
    startup_delivered: usize,
    recovery_end_packet: Option<u64>,
    last_sent_packet: u64,
    undo: Option<Undo>,
    ack_epoch: Instant,
    ack_epoch_delivered: usize,
    extra_acked: VecDeque<(u64, usize)>,
    #[cfg(feature = "qlog")]
    send_rate: Option<Bandwidth>,
    #[cfg(feature = "qlog")]
    ack_rate: Option<Bandwidth>,
}

impl BBRv3 {
    pub(crate) fn new(
        initial_packets: usize,
        max_packets: usize,
        mss: usize,
        initial_rtt: Duration,
    ) -> Self {
        let now = Instant::now();
        let initial_cwnd = initial_packets.saturating_mul(mss).max(4 * mss);
        let initial_rtt = initial_rtt.max(Duration::from_millis(1));
        Self {
            sampler: BandwidthSampler::new(10, true, true),
            state: State::Startup,
            ack_phase: AckPhase::Init,
            mss,
            initial_cwnd,
            max_cwnd: max_packets.saturating_mul(mss).max(initial_cwnd),
            cwnd: initial_cwnd,
            prior_cwnd: initial_cwnd,
            pacing: Bandwidth::from_bytes_and_time_delta(
                initial_cwnd,
                initial_rtt,
            ) * STARTUP_GAIN,
            max_bw: [Bandwidth::zero(); 2],
            bw_shortterm: Bandwidth::infinite(),
            inflight_shortterm: usize::MAX,
            inflight_longterm: usize::MAX,
            min_rtt: initial_rtt,
            min_rtt_stamp: now,
            probe_rtt_min_delay: initial_rtt,
            probe_rtt_min_stamp: now,
            probe_rtt_done_stamp: None,
            probe_rtt_round_done: false,
            idle_restart: false,
            next_round_delivered: 0,
            round_count: 0,
            drain_start_round: 0,
            rounds_since_probe: 0,
            cycle_stamp: now,
            probe_wait: Duration::from_secs(2),
            full_bw: Bandwidth::zero(),
            full_bw_count: 0,
            full_bw_now: false,
            full_bw_reached: false,
            is_probe_sample: false,
            prev_probe_too_high: false,
            prev_probe_precautionary: false,
            probe_up_rounds: 0,
            probe_up_acked: 0,
            probe_up_acked_per_inc: usize::MAX,
            cwnd_limited_in_round: false,
            loss_round_delivered: 0,
            loss_in_round: false,
            bw_latest: Bandwidth::zero(),
            inflight_latest: 0,
            startup_loss_ranges: 0,
            last_lost_packet: None,
            startup_lost: 0,
            startup_delivered: 0,
            recovery_end_packet: None,
            last_sent_packet: 0,
            undo: None,
            ack_epoch: now,
            ack_epoch_delivered: 0,
            extra_acked: VecDeque::new(),
            #[cfg(feature = "qlog")]
            send_rate: None,
            #[cfg(feature = "qlog")]
            ack_rate: None,
        }
    }

    fn bw(&self) -> Bandwidth {
        self.max_bandwidth().min(self.bw_shortterm)
    }

    fn bdp(&self, gain: f64) -> usize {
        (self.bw() * gain)
            .to_bytes_per_period(self.min_rtt)
            .min(usize::MAX as u64) as usize
    }

    pub(crate) fn send_quantum(&self) -> usize {
        self.pacing
            .to_bytes_per_period(Duration::from_millis(1))
            .min(64 * 1024)
            .max((2 * self.mss) as u64) as usize
    }

    fn inflight(&self, gain: f64) -> usize {
        self.bdp(gain)
            .max(self.send_quantum())
            .max(4 * self.mss)
            .saturating_add(if self.state == State::Up {
                2 * self.mss
            } else {
                0
            })
    }

    fn headroom(&self) -> usize {
        if self.inflight_longterm == usize::MAX {
            return usize::MAX;
        }
        self.inflight_longterm
            .saturating_sub(
                ((self.inflight_longterm as f64 * 0.15) as usize).max(self.mss),
            )
            .max(4 * self.mss)
    }

    fn start_round(&mut self) {
        self.next_round_delivered = self.sampler.total_bytes_acked();
    }

    fn reset_full_bw(&mut self) {
        self.full_bw = Bandwidth::zero();
        self.full_bw_count = 0;
        self.full_bw_now = false;
    }

    fn reset_shortterm(&mut self) {
        self.bw_shortterm = Bandwidth::infinite();
        self.inflight_shortterm = usize::MAX;
    }

    fn enter_down(&mut self, now: Instant) {
        self.loss_in_round = false;
        self.bw_latest = Bandwidth::zero();
        self.inflight_latest = 0;
        self.probe_up_acked_per_inc = usize::MAX;
        self.rounds_since_probe = crate::rand::rand_u64_uniform(2);
        self.probe_wait = Duration::from_micros(
            2_000_000 + crate::rand::rand_u64_uniform(1_000_001),
        );
        self.cycle_stamp = now;
        self.ack_phase = AckPhase::Stopping;
        self.start_round();
        self.state = State::Down;
    }

    fn enter_refill(&mut self) {
        self.reset_shortterm();
        self.probe_up_rounds = 0;
        self.probe_up_acked = 0;
        self.prev_probe_precautionary = false;
        self.ack_phase = AckPhase::Refilling;
        self.start_round();
        self.state = State::Refill;
    }

    fn raise_slope(&mut self) {
        self.probe_up_acked_per_inc =
            (self.cwnd / (1usize << self.probe_up_rounds)).max(self.mss);
        self.probe_up_rounds = (self.probe_up_rounds + 1).min(30);
    }

    fn check_full_bw(
        &mut self,
        rate: Bandwidth,
        app_limited: bool,
        round: bool,
    ) {
        if self.full_bw_now
            || !round
            || app_limited
            || rate == Bandwidth::zero()
        {
            return;
        }
        if rate >= self.full_bw * 1.25f64 {
            self.reset_full_bw();
            self.full_bw = rate;
        } else {
            self.full_bw_count = self.full_bw_count.saturating_add(1);
            self.full_bw_now = self.full_bw_count >= 3;
            self.full_bw_reached |= self.full_bw_now;
        }
    }

    // Sections 5.3.1.3 and 5.5.10: process each lost packet's own send-time
    // state, not the smaller inflight of the ACK which revealed the loss.
    fn on_loss(&mut self, lost: &Lost, now: Instant) {
        if self.recovery_end_packet.is_none() {
            self.recovery_end_packet = Some(self.last_sent_packet);
            self.undo = Some(Undo {
                state: self.state,
                cwnd: self.cwnd,
                bw_shortterm: self.bw_shortterm,
                inflight_shortterm: self.inflight_shortterm,
                inflight_longterm: self.inflight_longterm,
                lost_packets: BTreeSet::new(),
            });
        }
        if let Some(undo) = &mut self.undo {
            if undo.lost_packets.len() < MAX_UNDO_PACKETS {
                undo.lost_packets.insert(lost.packet_number);
            } else {
                self.undo = None;
            }
        }
        if !self.loss_in_round {
            self.loss_round_delivered = self.sampler.total_bytes_acked();
        }
        self.loss_in_round = true;
        self.startup_lost = self.startup_lost.saturating_add(lost.bytes_lost);
        if self.last_lost_packet.and_then(|n| n.checked_add(1))
            != Some(lost.packet_number)
        {
            self.startup_loss_ranges += 1;
        }
        self.last_lost_packet = Some(lost.packet_number);
        let sample = self.sampler.on_congestion_event(
            now,
            &[],
            std::slice::from_ref(lost),
            Some(self.max_bandwidth()),
            self.bw_shortterm,
            self.round_count as usize,
        );
        let sent = sample.last_packet_send_state;
        let lost_since_send = self
            .sampler
            .total_bytes_lost()
            .saturating_sub(sent.total_bytes_lost);
        if !self.is_probe_sample
            || !sent.is_valid
            || lost_since_send as f64
                <= sent.bytes_in_flight as f64 * LOSS_THRESHOLD
        {
            return;
        }
        self.prev_probe_too_high = true;
        self.is_probe_sample = false;
        if !sent.is_app_limited {
            let prior_inflight =
                sent.bytes_in_flight.saturating_sub(lost.bytes_lost);
            let prior_lost = lost_since_send.saturating_sub(lost.bytes_lost);
            let prefix = ((LOSS_THRESHOLD * prior_inflight as f64
                - prior_lost as f64)
                / (1.0 - LOSS_THRESHOLD))
                .max(0.0) as usize;
            self.inflight_longterm = prior_inflight
                .saturating_add(prefix)
                .max((self.bdp(1.0).min(self.cwnd) as f64 * BETA) as usize)
                .max(4 * self.mss);
        }
        if self.state == State::Up {
            self.enter_down(now);
        }
    }

    /// Only undo when every loss in the saved episode was acknowledged.
    /// ACK ranges are already validated by QUIC before reaching this method.
    pub(crate) fn acknowledge_spurious_losses(
        &mut self,
        ranges: &RangeSet,
        now: Instant,
    ) {
        let Some(undo) = &mut self.undo else {
            return;
        };
        if undo.lost_packets.is_empty() {
            return;
        }
        undo.lost_packets.retain(|packet| {
            !ranges.iter().any(|range| range.contains(packet))
        });
        if !undo.lost_packets.is_empty() {
            return;
        }
        let undo = self.undo.take().unwrap();
        self.cwnd = self.cwnd.max(undo.cwnd).min(self.max_cwnd);
        self.bw_shortterm = self.bw_shortterm.max(undo.bw_shortterm);
        self.inflight_shortterm =
            self.inflight_shortterm.max(undo.inflight_shortterm);
        self.inflight_longterm =
            self.inflight_longterm.max(undo.inflight_longterm);
        self.loss_in_round = false;
        self.startup_lost = 0;
        self.startup_loss_ranges = 0;
        self.recovery_end_packet = None;
        self.reset_full_bw();
        if undo.state == State::Startup {
            self.full_bw_reached = false;
            if self.state != State::ProbeRtt {
                self.state = State::Startup;
            }
        } else if undo.state == State::Up && self.state != State::ProbeRtt {
            self.enter_refill();
        }
        self.update_control(0, now);
    }

    fn update_probe_bw(
        &mut self,
        now: Instant,
        round: bool,
        app_limited: bool,
        rate: Bandwidth,
        sent: SendTimeState,
        acked: usize,
        inflight: usize,
    ) {
        if !self.full_bw_reached {
            return;
        }
        if self.ack_phase == AckPhase::Starting && round {
            self.ack_phase = AckPhase::Feedback;
        }
        if self.ack_phase == AckPhase::Stopping && round {
            self.is_probe_sample = false;
            self.ack_phase = AckPhase::Init;
            if self.state.is_probe_bw() && !app_limited {
                self.max_bw[0] = self.max_bw[1];
                self.max_bw[1] = Bandwidth::zero();
            }
            if self.state.is_probe_bw()
                && self.prev_probe_precautionary
                && !self.prev_probe_too_high
            {
                self.enter_refill();
                return;
            }
        }
        let losses = self
            .sampler
            .total_bytes_lost()
            .saturating_sub(sent.total_bytes_lost);
        if sent.is_valid
            && losses as f64 <= sent.bytes_in_flight as f64 * LOSS_THRESHOLD
            && self.inflight_longterm != usize::MAX
        {
            self.inflight_longterm =
                self.inflight_longterm.max(sent.bytes_in_flight);
            if self.state == State::Up
                && self.cwnd_limited_in_round
                && self.cwnd >= self.inflight_longterm
            {
                self.probe_up_acked = self.probe_up_acked.saturating_add(acked);
                let delta = self.probe_up_acked / self.probe_up_acked_per_inc;
                self.probe_up_acked %= self.probe_up_acked_per_inc;
                self.inflight_longterm = self
                    .inflight_longterm
                    .saturating_add(delta.saturating_mul(self.mss));
                if round {
                    self.raise_slope();
                }
            }
        }
        match self.state {
            State::Down | State::Cruise => {
                // Reno coexistence counts packets, not bytes (section 5.3.3.8).
                let reno_rounds = (self.bdp(1.0).min(self.cwnd) / self.mss)
                    .clamp(1, 63) as u64;
                if now.saturating_duration_since(self.cycle_stamp)
                    > self.probe_wait
                    || self.rounds_since_probe >= reno_rounds
                {
                    self.enter_refill();
                } else if self.state == State::Down
                    && inflight <= self.headroom()
                    && inflight <= self.inflight(1.0)
                {
                    self.state = State::Cruise;
                }
            }
            State::Refill if round => {
                self.is_probe_sample = true;
                self.ack_phase = AckPhase::Starting;
                self.start_round();
                self.reset_full_bw();
                self.full_bw = rate;
                self.state = State::Up;
                self.raise_slope();
            }
            State::Up => {
                let precautionary = self.prev_probe_too_high
                    && inflight >= self.inflight_longterm;
                if !precautionary
                    && self.cwnd_limited_in_round
                    && self.cwnd >= self.inflight_longterm
                {
                    self.reset_full_bw();
                    self.full_bw = rate;
                } else if precautionary || self.full_bw_now {
                    self.prev_probe_precautionary = precautionary;
                    self.prev_probe_too_high = false;
                    self.enter_down(now);
                }
            }
            _ => {}
        }
    }

    fn update_rtt(
        &mut self,
        now: Instant,
        rtt: Option<Duration>,
        inflight: usize,
        round: bool,
    ) {
        let expired = now.saturating_duration_since(self.probe_rtt_min_stamp)
            > PROBE_RTT_INTERVAL;
        if let Some(rtt) = rtt.filter(|rtt| !rtt.is_zero()) {
            if rtt < self.probe_rtt_min_delay || expired {
                self.probe_rtt_min_delay = rtt;
                self.probe_rtt_min_stamp = now;
            }
            if self.probe_rtt_min_delay < self.min_rtt
                || now.saturating_duration_since(self.min_rtt_stamp)
                    > MIN_RTT_WINDOW
            {
                self.min_rtt = self.probe_rtt_min_delay;
                self.min_rtt_stamp = self.probe_rtt_min_stamp;
            }
        }
        if expired && !self.idle_restart && self.state != State::ProbeRtt {
            self.prior_cwnd = self.cwnd;
            self.state = State::ProbeRtt;
            self.probe_rtt_done_stamp = None;
            self.probe_rtt_round_done = false;
            self.ack_phase = AckPhase::Stopping;
            self.start_round();
        }
        if self.state == State::ProbeRtt {
            self.sampler.on_app_limited();
            if self.probe_rtt_done_stamp.is_none()
                && inflight <= self.bdp(0.5).max(4 * self.mss)
            {
                self.probe_rtt_done_stamp = Some(now + PROBE_RTT_DURATION);
                self.probe_rtt_round_done = false;
                self.start_round();
            } else if self.probe_rtt_done_stamp.is_some() {
                self.probe_rtt_round_done |= round;
                if self.probe_rtt_round_done {
                    self.check_probe_rtt_done(now);
                }
            }
        }
    }

    fn check_probe_rtt_done(&mut self, now: Instant) {
        if self.probe_rtt_done_stamp.is_some_and(|done| now > done) {
            self.probe_rtt_min_stamp = now;
            self.cwnd = self.cwnd.max(self.prior_cwnd);
            self.reset_shortterm();
            if self.full_bw_reached {
                self.enter_down(now);
                self.state = State::Cruise;
            } else {
                self.state = State::Startup;
            }
            self.probe_rtt_done_stamp = None;
        }
    }

    fn update_aggregation(&mut self, now: Instant, acked: usize) {
        let expected = self
            .bw()
            .to_bytes_per_period(now.saturating_duration_since(self.ack_epoch))
            .min(usize::MAX as u64) as usize;
        let expected = if self.ack_epoch_delivered <= expected {
            self.ack_epoch_delivered = 0;
            self.ack_epoch = now;
            0
        } else {
            expected
        };
        self.ack_epoch_delivered =
            self.ack_epoch_delivered.saturating_add(acked);
        let extra = self
            .ack_epoch_delivered
            .saturating_sub(expected)
            .min(self.cwnd);
        let window = if self.full_bw_reached { 10 } else { 1 };
        while self.extra_acked.front().is_some_and(|(round, _)| {
            self.round_count.saturating_sub(*round) >= window
        }) {
            self.extra_acked.pop_front();
        }
        if let Some((round, value)) = self
            .extra_acked
            .back_mut()
            .filter(|(round, _)| *round == self.round_count)
        {
            let _ = round;
            *value = (*value).max(extra);
        } else {
            self.extra_acked.push_back((self.round_count, extra));
        }
    }

    fn update_control(&mut self, acked: usize, _now: Instant) {
        let (pacing_gain, cwnd_gain) = self.state.gains();
        let rate = self.bw() * (pacing_gain * 0.99);
        if self.bw() != Bandwidth::zero()
            && (self.full_bw_reached || rate > self.pacing)
        {
            self.pacing = rate;
        }
        let extra = self
            .extra_acked
            .iter()
            .map(|(_, value)| *value)
            .max()
            .unwrap_or(0);
        let target = self
            .bdp(cwnd_gain)
            .saturating_add(extra)
            .max(self.send_quantum())
            .max(4 * self.mss)
            .saturating_add(if self.state == State::Up {
                2 * self.mss
            } else {
                0
            });
        if self.full_bw_reached {
            self.cwnd = self.cwnd.saturating_add(acked).min(target);
        } else if self.cwnd < target
            || self.sampler.total_bytes_acked() < self.initial_cwnd
        {
            self.cwnd = self.cwnd.saturating_add(acked);
        }
        self.cwnd = self.cwnd.max(4 * self.mss);
        if self.state == State::ProbeRtt {
            self.cwnd = self.cwnd.min(self.bdp(0.5).max(4 * self.mss));
        }
        let cap = match self.state {
            State::Down | State::Refill | State::Up => self.inflight_longterm,
            State::Cruise | State::ProbeRtt => self.headroom(),
            _ => usize::MAX,
        };
        self.cwnd = self
            .cwnd
            .min(cap.min(self.inflight_shortterm).max(4 * self.mss))
            .min(self.max_cwnd);
    }

    #[cfg(feature = "qlog")]
    pub(crate) fn send_rate(&self) -> Option<Bandwidth> {
        self.send_rate
    }
    #[cfg(feature = "qlog")]
    pub(crate) fn ack_rate(&self) -> Option<Bandwidth> {
        self.ack_rate
    }
}

impl CongestionControl for BBRv3 {
    #[cfg(feature = "qlog")]
    fn state_str(&self) -> &'static str {
        match self.state {
            State::Startup => "bbr3_startup",
            State::Drain => "bbr3_drain",
            State::Down => "bbr3_probe_down",
            State::Cruise => "bbr3_probe_cruise",
            State::Refill => "bbr3_probe_refill",
            State::Up => "bbr3_probe_up",
            State::ProbeRtt => "bbr3_probe_rtt",
        }
    }
    fn get_congestion_window(&self) -> usize {
        self.cwnd
    }
    fn get_congestion_window_in_packets(&self) -> usize {
        self.cwnd / self.mss
    }
    fn can_send(&self, inflight: usize) -> bool {
        inflight < self.cwnd
    }

    fn on_packet_sent(
        &mut self,
        now: Instant,
        inflight: usize,
        packet: u64,
        bytes: usize,
        retransmittable: bool,
    ) {
        if inflight == 0 && self.sampler.is_app_limited() {
            self.idle_restart = true;
            self.ack_epoch = now;
            self.ack_epoch_delivered = 0;
            if self.state.is_probe_bw() && self.bw() != Bandwidth::zero() {
                self.pacing = self.bw() * 0.99f64;
            }
            if self.state == State::ProbeRtt {
                self.check_probe_rtt_done(now);
            }
        }
        self.last_sent_packet = packet;
        self.cwnd_limited_in_round |=
            inflight.saturating_add(bytes) >= self.cwnd;
        self.sampler.on_packet_sent(
            now,
            packet,
            bytes,
            inflight,
            retransmittable,
        );
    }

    fn on_congestion_event(
        &mut self,
        _rtt_updated: bool,
        _prior_inflight: usize,
        inflight: usize,
        now: Instant,
        acked: &[Acked],
        lost: &[Lost],
        least_unacked: u64,
        _rtt: &RttStats,
        _stats: &mut RecoveryStats,
    ) {
        for packet in lost {
            self.on_loss(packet, now);
        }
        let prior_delivered = self.sampler.total_bytes_acked();
        let sample = self.sampler.on_congestion_event(
            now,
            acked,
            &[],
            Some(self.max_bandwidth()),
            self.bw_shortterm,
            self.round_count as usize,
        );
        let delivered = self.sampler.total_bytes_acked();
        let newly_acked = delivered.saturating_sub(prior_delivered);
        self.sampler.remove_obsolete_packets(least_unacked);
        if newly_acked == 0 {
            self.update_control(0, now);
            return;
        }
        let sent = sample.last_packet_send_state;
        let rate = sample.sample_max_bandwidth.unwrap_or(Bandwidth::zero());
        let round = sent.is_valid
            && sent.total_bytes_acked >= self.next_round_delivered;
        if round {
            self.start_round();
            self.round_count += 1;
            self.rounds_since_probe += 1;
        }
        if !sample.sample_is_app_limited || rate >= self.max_bandwidth() {
            self.max_bw[1] = self.max_bw[1].max(rate);
        }
        self.bw_latest = self.bw_latest.max(rate);
        self.inflight_latest =
            self.inflight_latest.max(sample.sample_max_inflight);
        let loss_round = sent.is_valid
            && sent.total_bytes_acked >= self.loss_round_delivered;
        if loss_round {
            self.loss_round_delivered = delivered;
            if !self.state.is_probing() && self.loss_in_round {
                if self.bw_shortterm == Bandwidth::infinite() {
                    self.bw_shortterm = self.max_bandwidth();
                }
                if self.inflight_shortterm == usize::MAX {
                    self.inflight_shortterm = self.cwnd;
                }
                self.bw_shortterm =
                    (self.bw_shortterm * BETA).max(self.bw_latest);
                self.inflight_shortterm =
                    ((self.inflight_shortterm as f64 * BETA) as usize)
                        .max(self.inflight_latest);
            }
            self.loss_in_round = false;
        }
        self.startup_delivered =
            self.startup_delivered.saturating_add(newly_acked);
        self.update_aggregation(now, newly_acked);
        self.check_full_bw(rate, sample.sample_is_app_limited, round);
        if self.state == State::Startup
            && round
            && self.startup_loss_ranges >= 6
            && self.startup_lost as f64
                > (self.startup_delivered + self.startup_lost) as f64
                    * LOSS_THRESHOLD
        {
            self.full_bw_now = true;
            self.full_bw_reached = true;
            self.inflight_longterm =
                self.bdp(1.0).max(self.inflight_latest).max(4 * self.mss);
        }
        if self.state == State::Startup && self.full_bw_reached {
            self.state = State::Drain;
            self.drain_start_round = self.round_count;
        } else if self.state == State::Drain
            && (inflight <= self.inflight(1.0)
                || self.round_count.saturating_sub(self.drain_start_round) >= 3)
        {
            self.enter_down(now);
        } else {
            self.update_probe_bw(
                now,
                round,
                sample.sample_is_app_limited,
                rate,
                sent,
                newly_acked,
                inflight,
            );
        }
        self.update_rtt(now, sample.sample_rtt, inflight, round);
        self.idle_restart = false;
        if round {
            self.startup_lost = 0;
            self.startup_delivered = 0;
            self.startup_loss_ranges = 0;
            self.last_lost_packet = None;
            self.cwnd_limited_in_round = false;
        }
        if loss_round {
            self.bw_latest = rate;
            self.inflight_latest = sample.sample_max_inflight;
        }
        if self.recovery_end_packet.is_some_and(|end| {
            acked.last().is_some_and(|ack| ack.pkt_num > end)
        }) {
            self.recovery_end_packet = None;
        }
        #[cfg(feature = "qlog")]
        {
            self.send_rate = sample.sample_max_send_rate;
            self.ack_rate = sample.sample_max_ack_rate;
        }
        self.update_control(newly_acked, now);
    }

    fn on_packet_neutered(&mut self, packet: u64) {
        self.sampler.on_packet_neutered(packet);
    }
    // QUIC PTO schedules probes; it is not TCP RTO or proof of congestion.
    fn on_retransmission_timeout(&mut self, _retransmitted: bool) {}
    fn on_connection_migration(&mut self) {
        *self = Self::new(
            self.initial_cwnd / self.mss,
            self.max_cwnd / self.mss,
            self.mss,
            self.min_rtt,
        );
    }
    fn limit_cwnd(&mut self, max_cwnd: usize) {
        self.max_cwnd = max_cwnd.max(4 * self.mss);
        self.cwnd = self.cwnd.min(self.max_cwnd);
    }
    fn is_in_recovery(&self) -> bool {
        self.recovery_end_packet.is_some()
    }
    fn is_cwnd_limited(&self, inflight: usize) -> bool {
        inflight >= self.cwnd
    }
    fn pacing_rate(&self, _inflight: usize, _rtt: &RttStats) -> Bandwidth {
        self.pacing
    }
    fn bandwidth_estimate(&self, _rtt: &RttStats) -> Bandwidth {
        self.bw()
    }
    fn max_bandwidth(&self) -> Bandwidth {
        self.max_bw[0].max(self.max_bw[1])
    }
    fn update_mss(&mut self, mss: usize) {
        // Model bandwidth/inflight are bytes, not packet counts. Only window
        // limits scale with MSS; PMTU discovery is not new bandwidth evidence.
        let scale = |bytes: usize| {
            ((bytes as u128 * mss as u128) / self.mss as u128)
                .min(usize::MAX as u128) as usize
        };
        self.initial_cwnd = scale(self.initial_cwnd);
        self.max_cwnd = scale(self.max_cwnd);
        self.mss = mss;
        self.cwnd = self.cwnd.max(4 * mss).min(self.max_cwnd);
    }
    fn on_app_limited(&mut self, inflight: usize) {
        if inflight < self.cwnd {
            self.sampler.on_app_limited();
        }
    }
}

#[cfg(test)]
mod tests;
