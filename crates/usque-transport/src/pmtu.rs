use std::collections::VecDeque;
use std::net::{IpAddr, SocketAddr};
use std::time::{Duration, Instant};

use crate::network_quality::PmtuPhase;

pub(crate) const INITIAL_SAFE_UDP_PAYLOAD: usize = 1_350;
pub(crate) const IPV4_MAX_UDP_PAYLOAD: usize = 1_472;
pub(crate) const IPV6_MAX_UDP_PAYLOAD: usize = 1_452;
pub(crate) const PMTUD_MAX_PROBES: u8 = 3;

const MIN_QUIC_UDP_PAYLOAD: usize = 1_200;
pub(crate) const IPV6_MINIMUM_INNER_MTU: usize = 1_280;
const MAX_TRACKED_PATHS: usize = 3;
const REVALIDATION_WINDOW: Duration = Duration::from_secs(10);
const SEND_ERROR_SUPPRESSION: Duration = Duration::from_secs(1);
// The locked quiche binary search needs at most ten sizes (including 1200)
// between 1200 and 1472, with three loss attempts per size. Keep discovery
// bounded without exhausting the separate completed-PMTU revalidation budget.
const MAX_DISCOVERY_SEND_ERRORS: usize = PMTUD_MAX_PROBES as usize
    * ((IPV4_MAX_UDP_PAYLOAD - MIN_QUIC_UDP_PAYLOAD).ilog2() as usize + 2);

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct PmtuPathKey {
    pub(crate) local: SocketAddr,
    pub(crate) peer: SocketAddr,
}

impl PmtuPathKey {
    pub(crate) const fn new(local: SocketAddr, peer: SocketAddr) -> Self {
        Self { local, peer }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PmtuObservation {
    pub(crate) phase: PmtuPhase,
    pub(crate) outer_payload_bytes: Option<usize>,
    pub(crate) effective_connect_ip_payload_bytes: Option<usize>,
    pub(crate) numeric_changed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PmtuRevalidationAction {
    ContinueDiscovery(PmtuObservation),
    Revalidate(PmtuObservation),
    Exhausted(PmtuObservation),
}

struct PathPmtuState {
    key: PmtuPathKey,
    phase: PmtuPhase,
    stable_outer_payload: Option<usize>,
    published_outer_payload: Option<usize>,
    effective_connect_ip_payload: Option<usize>,
    send_too_large_count: u64,
    revalidation_triggers: VecDeque<Instant>,
    discovery_send_errors: usize,
    send_suppressed_until: Option<Instant>,
}

impl PathPmtuState {
    fn new(key: PmtuPathKey) -> Self {
        Self {
            key,
            phase: PmtuPhase::Unknown,
            stable_outer_payload: None,
            published_outer_payload: None,
            effective_connect_ip_payload: None,
            send_too_large_count: 0,
            revalidation_triggers: VecDeque::with_capacity(PMTUD_MAX_PROBES as usize),
            discovery_send_errors: 0,
            send_suppressed_until: None,
        }
    }

    fn observation(&self, previous_outer_payload: Option<usize>) -> PmtuObservation {
        PmtuObservation {
            phase: self.phase,
            outer_payload_bytes: self.published_outer_payload,
            effective_connect_ip_payload_bytes: self.effective_connect_ip_payload,
            numeric_changed: previous_outer_payload.is_some()
                && self.published_outer_payload.is_some()
                && previous_outer_payload != self.published_outer_payload,
        }
    }
}

/// Tracks application-visible PMTU state independently for every bounded H3
/// path. quiche owns probe generation and loss inference; this controller owns
/// publication semantics and the send-error circuit breaker.
pub(crate) struct PmtuController {
    paths: Vec<PathPmtuState>,
    active: Option<PmtuPathKey>,
    automatic: bool,
}

impl Default for PmtuController {
    fn default() -> Self {
        Self {
            paths: Vec::new(),
            active: None,
            automatic: crate::PRODUCTION_NETWORK_FEATURES.automatic_pmtu,
        }
    }
}

impl PmtuController {
    #[cfg(test)]
    pub(crate) fn new(initial_path: PmtuPathKey) -> Self {
        Self::with_automatic(
            initial_path,
            crate::PRODUCTION_NETWORK_FEATURES.automatic_pmtu,
        )
    }

    pub(crate) fn with_automatic(initial_path: PmtuPathKey, automatic: bool) -> Self {
        Self {
            paths: vec![PathPmtuState::new(initial_path)],
            active: Some(initial_path),
            automatic,
        }
    }

    pub(crate) fn observe_active_path(
        &mut self,
        key: PmtuPathKey,
        completed_outer_payload: Option<usize>,
        _estimated_outer_payload: usize,
        effective_connect_ip_payload: Option<usize>,
    ) -> PmtuObservation {
        self.activate_path(key);
        let automatic = self.automatic;
        let state = self.active_state_mut();
        let previous = state.stable_outer_payload;
        if !automatic {
            state.published_outer_payload = Some(INITIAL_SAFE_UDP_PAYLOAD);
            state.effective_connect_ip_payload = effective_connect_ip_payload;
            state.phase = if effective_connect_ip_payload
                .is_some_and(|payload| payload < IPV6_MINIMUM_INNER_MTU)
            {
                PmtuPhase::Degraded
            } else {
                PmtuPhase::Stable
            };
            return state.observation(previous);
        }

        // Publish one explicit probing observation for every fresh path even
        // when a very fast probe completed before the one-hertz sampler ran.
        // This preserves Unknown -> Probing -> Stable/Degraded and prevents a
        // newly active path from appearing to inherit an old stable value.
        if state.phase == PmtuPhase::Unknown {
            state.phase = PmtuPhase::Probing;
            state.published_outer_payload = None;
            state.effective_connect_ip_payload = None;
            return state.observation(previous);
        }

        if let Some(completed) = completed_outer_payload {
            state.stable_outer_payload = Some(completed);
            state.published_outer_payload = Some(completed);
            state.effective_connect_ip_payload = effective_connect_ip_payload;
            state.phase = if effective_connect_ip_payload
                .is_some_and(|payload| payload < IPV6_MINIMUM_INNER_MTU)
            {
                PmtuPhase::Degraded
            } else {
                PmtuPhase::Stable
            };
            state.send_suppressed_until = None;
            state.discovery_send_errors = 0;
        } else {
            // quiche still owns a conservative data-send cap while probing.
            // Neither that estimate nor a stale completed value is a current
            // measurement, so both numeric publications remain NotReady.
            state.published_outer_payload = None;
            state.effective_connect_ip_payload = None;
            match state.phase {
                PmtuPhase::Stable | PmtuPhase::Degraded => {
                    state.phase = PmtuPhase::Revalidating;
                }
                PmtuPhase::Revalidating | PmtuPhase::Probing => {}
                PmtuPhase::Unsupported => {
                    debug_assert!(false, "H3 PMTU state cannot become unsupported");
                    state.phase = PmtuPhase::Unknown;
                }
                PmtuPhase::Unknown => unreachable!("fresh PMTU paths return above"),
            }
        }

        state.observation(previous)
    }

    pub(crate) fn on_send_too_large(
        &mut self,
        key: PmtuPathKey,
        completed_outer_payload: Option<usize>,
        now: Instant,
    ) -> PmtuRevalidationAction {
        self.activate_path(key);
        let automatic = self.automatic;
        let state = self.active_state_mut();
        let previous = state.stable_outer_payload;
        state.send_too_large_count = state.send_too_large_count.saturating_add(1);
        if !automatic {
            state.phase = PmtuPhase::Degraded;
            state.published_outer_payload = Some(INITIAL_SAFE_UDP_PAYLOAD);
            state.effective_connect_ip_payload = None;
            return PmtuRevalidationAction::Exhausted(state.observation(previous));
        }
        state.published_outer_payload = None;
        state.effective_connect_ip_payload = None;
        state.send_suppressed_until = Some(now + SEND_ERROR_SUPPRESSION);
        if completed_outer_payload.is_none() {
            state.discovery_send_errors = state.discovery_send_errors.saturating_add(1);
            if state.discovery_send_errors >= MAX_DISCOVERY_SEND_ERRORS {
                state.phase = PmtuPhase::Degraded;
                return PmtuRevalidationAction::Exhausted(state.observation(previous));
            }
            if state.stable_outer_payload.is_some() || state.phase == PmtuPhase::Revalidating {
                state.phase = PmtuPhase::Revalidating;
            } else {
                state.phase = PmtuPhase::Probing;
            }
            // A failed size is one probe in the existing discovery round.
            // Let quiche's loss inference advance/down-search it; do not
            // restart discovery or count this as another full revalidation.
            return PmtuRevalidationAction::ContinueDiscovery(state.observation(previous));
        }
        state.discovery_send_errors = 0;
        while state
            .revalidation_triggers
            .front()
            .is_some_and(|trigger| now.saturating_duration_since(*trigger) >= REVALIDATION_WINDOW)
        {
            state.revalidation_triggers.pop_front();
        }
        state.revalidation_triggers.push_back(now);

        if state.revalidation_triggers.len() >= PMTUD_MAX_PROBES as usize {
            state.phase = PmtuPhase::Degraded;
            state.send_suppressed_until = None;
            return PmtuRevalidationAction::Exhausted(state.observation(previous));
        }

        state.phase = PmtuPhase::Revalidating;
        PmtuRevalidationAction::Revalidate(state.observation(previous))
    }

    /// A promoted path gets fresh state and never inherits the old path's
    /// stable PMTU.
    pub(crate) fn on_path_promoted(&mut self, key: PmtuPathKey) -> PmtuRevalidationAction {
        self.activate_path(key);
        let automatic = self.automatic;
        let state = self.active_state_mut();
        let previous = state.published_outer_payload;
        state.phase = if automatic {
            PmtuPhase::Revalidating
        } else {
            PmtuPhase::Stable
        };
        state.stable_outer_payload = None;
        state.published_outer_payload = if automatic {
            None
        } else {
            Some(INITIAL_SAFE_UDP_PAYLOAD)
        };
        state.effective_connect_ip_payload = None;
        state.send_suppressed_until = None;
        state.discovery_send_errors = 0;
        state.revalidation_triggers.clear();
        PmtuRevalidationAction::Revalidate(state.observation(previous))
    }

    pub(crate) fn send_suppressed_until(&self, now: Instant) -> Option<Instant> {
        self.active_state().and_then(|state| {
            state
                .send_suppressed_until
                .filter(|deadline| *deadline > now)
        })
    }

    fn activate_path(&mut self, key: PmtuPathKey) {
        if self.paths.iter().any(|path| path.key == key) {
            self.active = Some(key);
            return;
        }
        if self.paths.len() == MAX_TRACKED_PATHS {
            let removable = self
                .paths
                .iter()
                .position(|path| Some(path.key) != self.active)
                .unwrap_or(0);
            self.paths.remove(removable);
        }
        self.paths.push(PathPmtuState::new(key));
        self.active = Some(key);
    }

    fn active_state(&self) -> Option<&PathPmtuState> {
        let active = self.active?;
        self.paths.iter().find(|path| path.key == active)
    }

    fn active_state_mut(&mut self) -> &mut PathPmtuState {
        let active = self
            .active
            .expect("PMTU controller always has an active path");
        self.paths
            .iter_mut()
            .find(|path| path.key == active)
            .expect("active PMTU path remains tracked")
    }

    #[cfg(test)]
    fn path_state(&self, key: PmtuPathKey) -> Option<&PathPmtuState> {
        self.paths.iter().find(|path| path.key == key)
    }
}

pub(crate) const fn family_udp_payload_ceiling(endpoint: SocketAddr) -> usize {
    match endpoint.ip() {
        IpAddr::V4(_) => IPV4_MAX_UDP_PAYLOAD,
        IpAddr::V6(_) => IPV6_MAX_UDP_PAYLOAD,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(local: &str, peer: &str) -> PmtuPathKey {
        PmtuPathKey::new(local.parse().unwrap(), peer.parse().unwrap())
    }

    #[test]
    fn rollback_is_fixed_1350_across_paths_and_fails_closed_on_emsgsize() {
        let first = path("192.0.2.10:1000", "192.0.2.20:443");
        let second = path("192.0.2.11:1001", "192.0.2.20:443");
        let mut controller = PmtuController::with_automatic(first, false);
        let observation = controller.observe_active_path(first, Some(1_472), 1_350, Some(1_280));
        assert_eq!(observation.phase, PmtuPhase::Stable);
        assert_eq!(observation.outer_payload_bytes, Some(1_350));
        let PmtuRevalidationAction::Revalidate(promoted) = controller.on_path_promoted(second)
        else {
            panic!("role swap must reset per-path state");
        };
        assert_eq!(promoted.phase, PmtuPhase::Stable);
        assert_eq!(promoted.outer_payload_bytes, Some(1_350));
        assert!(matches!(
            controller.on_send_too_large(second, Some(1_350), Instant::now()),
            PmtuRevalidationAction::Exhausted(_)
        ));
    }

    #[test]
    fn family_ceiling_matches_a_1500_byte_outer_link() {
        assert_eq!(
            family_udp_payload_ceiling("192.0.2.1:443".parse().unwrap()),
            IPV4_MAX_UDP_PAYLOAD
        );
        assert_eq!(
            family_udp_payload_ceiling("[2001:db8::1]:443".parse().unwrap()),
            IPV6_MAX_UDP_PAYLOAD
        );
    }

    #[test]
    fn h3_path_moves_from_unknown_through_probing_to_stable() {
        let key = path("192.0.2.10:1000", "192.0.2.20:443");
        let mut controller = PmtuController::new(key);

        let probing = controller.observe_active_path(key, None, 1_200, Some(1_190));
        assert_eq!(probing.phase, PmtuPhase::Probing);
        assert_eq!(probing.outer_payload_bytes, None);
        assert_eq!(probing.effective_connect_ip_payload_bytes, None);

        let stable = controller.observe_active_path(key, Some(1_472), 1_472, Some(1_400));
        assert_eq!(stable.phase, PmtuPhase::Stable);
        assert_eq!(stable.outer_payload_bytes, Some(1_472));
        assert_eq!(stable.effective_connect_ip_payload_bytes, Some(1_400));
        assert!(!stable.numeric_changed);
    }

    #[test]
    fn fast_probe_completion_still_publishes_not_ready_once() {
        let key = path("192.0.2.10:1000", "192.0.2.20:443");
        let mut controller = PmtuController::new(key);

        let first = controller.observe_active_path(key, Some(1_472), 1_472, Some(1_400));
        assert_eq!(first.phase, PmtuPhase::Probing);
        assert_eq!(first.outer_payload_bytes, None);

        let second = controller.observe_active_path(key, Some(1_472), 1_472, Some(1_400));
        assert_eq!(second.phase, PmtuPhase::Stable);
        assert_eq!(second.outer_payload_bytes, Some(1_472));
    }

    #[test]
    fn link_mtu_drop_from_1500_to_1280_revalidates_to_the_lower_payload() {
        let key = path("192.0.2.10:1000", "192.0.2.20:443");
        let mut controller = PmtuController::new(key);
        controller.observe_active_path(key, Some(1_472), 1_472, Some(1_400));
        controller.observe_active_path(key, Some(1_472), 1_472, Some(1_400));
        let now = Instant::now();

        let PmtuRevalidationAction::Revalidate(revalidating) =
            controller.on_send_too_large(key, Some(1_472), now)
        else {
            panic!("first send-too-large must revalidate");
        };
        assert_eq!(revalidating.phase, PmtuPhase::Revalidating);
        assert_eq!(revalidating.outer_payload_bytes, None);
        assert!(!revalidating.numeric_changed);
        let pending = controller.observe_active_path(key, None, 1_200, Some(1_190));
        assert_eq!(pending.phase, PmtuPhase::Revalidating);
        assert_eq!(pending.outer_payload_bytes, None);
        assert_eq!(pending.effective_connect_ip_payload_bytes, None);

        // IPv4 UDP payload ceilings for 1500- and 1280-byte outer links are
        // respectively 1472 and 1252 bytes.
        let stable = controller.observe_active_path(key, Some(1_252), 1_252, Some(1_180));
        assert_eq!(stable.phase, PmtuPhase::Degraded);
        assert_eq!(stable.outer_payload_bytes, Some(1_252));
        assert!(stable.numeric_changed);
    }

    #[test]
    fn blackholed_probe_does_not_publish_an_unvalidated_ceiling() {
        let key = path("[2001:db8::10]:1000", "[2001:db8::20]:443");
        let mut controller = PmtuController::new(key);

        for _ in 0..32 {
            let observation = controller.observe_active_path(key, None, 1_200, Some(1_190));
            assert_eq!(observation.phase, PmtuPhase::Probing);
            assert_eq!(observation.outer_payload_bytes, None);
        }

        let conservative = controller.observe_active_path(key, Some(1_200), 1_200, Some(1_150));
        assert_eq!(conservative.phase, PmtuPhase::Degraded);
        assert_eq!(conservative.outer_payload_bytes, Some(1_200));
    }

    #[test]
    fn three_invalidated_completed_results_exhaust_the_revalidation_window() {
        let key = path("192.0.2.10:1000", "192.0.2.20:443");
        let mut controller = PmtuController::new(key);
        controller.observe_active_path(key, Some(1_472), 1_472, Some(1_400));
        controller.observe_active_path(key, Some(1_472), 1_472, Some(1_400));
        let start = Instant::now();

        assert!(matches!(
            controller.on_send_too_large(key, Some(1_472), start),
            PmtuRevalidationAction::Revalidate(_)
        ));
        assert_eq!(
            controller.send_suppressed_until(start),
            Some(start + SEND_ERROR_SUPPRESSION)
        );
        assert!(matches!(
            controller.on_send_too_large(key, Some(1_350), start + Duration::from_secs(1)),
            PmtuRevalidationAction::Revalidate(_)
        ));
        assert!(matches!(
            controller.on_send_too_large(key, Some(1_300), start + Duration::from_secs(2)),
            PmtuRevalidationAction::Exhausted(_)
        ));
        assert_eq!(controller.path_state(key).unwrap().send_too_large_count, 3);
    }

    #[test]
    fn emsgsize_window_discards_expired_revalidation_triggers() {
        let key = path("192.0.2.10:1000", "192.0.2.20:443");
        let mut controller = PmtuController::new(key);
        controller.observe_active_path(key, Some(1_472), 1_472, Some(1_400));
        controller.observe_active_path(key, Some(1_472), 1_472, Some(1_400));
        let start = Instant::now();

        assert!(matches!(
            controller.on_send_too_large(key, Some(1_472), start),
            PmtuRevalidationAction::Revalidate(_)
        ));
        assert!(matches!(
            controller.on_send_too_large(key, Some(1_350), start + REVALIDATION_WINDOW),
            PmtuRevalidationAction::Revalidate(_)
        ));
    }

    #[test]
    fn promoted_path_does_not_inherit_the_previous_stable_pmtu() {
        let first = path("192.0.2.10:1000", "192.0.2.20:443");
        let second = path("192.0.2.11:1001", "192.0.2.20:443");
        let mut controller = PmtuController::new(first);
        controller.observe_active_path(first, Some(1_472), 1_472, Some(1_400));
        controller.observe_active_path(first, Some(1_472), 1_472, Some(1_400));

        let PmtuRevalidationAction::Revalidate(promoted) = controller.on_path_promoted(second)
        else {
            panic!("path promotion must request revalidation");
        };
        assert_eq!(promoted.phase, PmtuPhase::Revalidating);
        assert_eq!(promoted.outer_payload_bytes, None);
        let pending = controller.observe_active_path(second, None, 1_200, Some(1_190));
        assert_eq!(pending.outer_payload_bytes, None);
        assert_eq!(pending.effective_connect_ip_payload_bytes, None);
        assert_eq!(
            controller.path_state(first).unwrap().stable_outer_payload,
            Some(1_472)
        );
        assert_eq!(
            controller.path_state(second).unwrap().stable_outer_payload,
            None
        );
    }

    #[test]
    fn three_failed_size_probes_allow_downsearch_without_restarting_discovery() {
        let key = path("192.0.2.10:1000", "192.0.2.20:443");
        let mut controller = PmtuController::new(key);
        let start = Instant::now();
        for index in 0..PMTUD_MAX_PROBES {
            let now = start + Duration::from_secs(u64::from(index));
            let PmtuRevalidationAction::ContinueDiscovery(observation) =
                controller.on_send_too_large(key, None, now)
            else {
                panic!("one failed probe is not a full failed revalidation");
            };
            assert_eq!(observation.phase, PmtuPhase::Probing);
            assert_eq!(observation.outer_payload_bytes, None);
            assert_eq!(
                controller.send_suppressed_until(now),
                Some(now + SEND_ERROR_SUPPRESSION)
            );
        }
        assert!(
            controller
                .path_state(key)
                .unwrap()
                .revalidation_triggers
                .is_empty()
        );
        // The locked search moves from 1472 to midpoint(1200,1472) after
        // three losses. Its later completed result can now be published.
        let completed = controller.observe_active_path(key, Some(1_336), 1_336, Some(1_280));
        assert_eq!(completed.outer_payload_bytes, Some(1_336));
        assert_eq!(completed.phase, PmtuPhase::Stable);
        assert_eq!(controller.path_state(key).unwrap().discovery_send_errors, 0);
    }

    #[test]
    fn unfinished_discovery_has_a_separate_finite_send_error_budget() {
        let key = path("192.0.2.10:1000", "192.0.2.20:443");
        let mut controller = PmtuController::new(key);
        let start = Instant::now();
        assert_eq!(MAX_DISCOVERY_SEND_ERRORS, 30);
        for index in 1..MAX_DISCOVERY_SEND_ERRORS {
            assert!(matches!(
                controller.on_send_too_large(key, None, start + Duration::from_secs(index as u64)),
                PmtuRevalidationAction::ContinueDiscovery(_)
            ));
        }
        assert!(matches!(
            controller.on_send_too_large(key, None, start + Duration::from_secs(30)),
            PmtuRevalidationAction::Exhausted(_)
        ));
        assert_eq!(controller.path_state(key).unwrap().send_too_large_count, 30);
    }
}
