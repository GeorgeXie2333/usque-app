#![cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "PR-08 freezes the validation barrier before PR-09 enables migration cycles"
    )
)]

use std::time::{Duration, Instant};

pub(crate) const MIGRATION_DRAIN_BUDGET: Duration = Duration::from_millis(50);
pub(crate) const MIGRATION_DRAIN_PACKET_BUDGET: usize = 64;

/// Prevents application DATAGRAM injection only during the short candidate
/// validation send cycle. The active path continues to carry already queued
/// output and normal injection resumes immediately when the cycle ends.
#[derive(Debug, Default)]
pub(crate) struct MigrationTxBarrier {
    started_at: Option<Instant>,
    active_output_drained: bool,
}

impl MigrationTxBarrier {
    pub(crate) fn begin(&mut self, now: Instant) -> bool {
        if self.started_at.is_some() {
            return false;
        }
        self.started_at = Some(now);
        self.active_output_drained = false;
        true
    }

    pub(crate) const fn allows_application_injection(&self) -> bool {
        self.started_at.is_none()
    }

    pub(crate) fn complete_active_drain(
        &mut self,
        now: Instant,
        packets_generated: usize,
        quiche_reported_done: bool,
    ) -> bool {
        let Some(started_at) = self.started_at else {
            return false;
        };
        if now.saturating_duration_since(started_at) > MIGRATION_DRAIN_BUDGET
            || packets_generated > MIGRATION_DRAIN_PACKET_BUDGET
            || !quiche_reported_done
        {
            self.finish();
            return false;
        }
        self.active_output_drained = true;
        true
    }

    pub(crate) const fn candidate_send_allowed(&self) -> bool {
        self.started_at.is_some() && self.active_output_drained
    }

    pub(crate) fn finish(&mut self) {
        self.started_at = None;
        self.active_output_drained = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn barrier_is_short_bounded_and_never_allows_probe_before_active_drain() {
        let now = Instant::now();
        let mut barrier = MigrationTxBarrier::default();
        assert!(barrier.allows_application_injection());
        assert!(barrier.begin(now));
        assert!(!barrier.allows_application_injection());
        assert!(!barrier.candidate_send_allowed());
        assert!(barrier.complete_active_drain(now, 1, true));
        assert!(barrier.candidate_send_allowed());
        barrier.finish();
        assert!(barrier.allows_application_injection());

        assert!(barrier.begin(now));
        assert!(!barrier.complete_active_drain(
            now + MIGRATION_DRAIN_BUDGET + Duration::from_nanos(1),
            0,
            true,
        ));
        assert!(barrier.allows_application_injection());
    }
}
