//! Shared timing and admission limits for a user's continuous connection intent.
use std::time::Duration;

pub const RETRY_INTERVAL: Duration = Duration::from_secs(1);
pub const NETWORK_SETTLE: Duration = Duration::from_millis(100);
pub const AUTOMATIC_CANDIDATE_LIMIT: usize = 14;
pub const H3_CANDIDATE_LIMIT: usize = 8;
pub const H2_CANDIDATE_LIMIT: usize = 10;
pub const TRANSPORT_RACE_DELAY: Duration = Duration::from_millis(250);

pub fn liveness_timeout(rtt: Option<Duration>) -> Duration {
    rtt.map_or(Duration::from_secs(3), |rtt| rtt.saturating_mul(4))
        .clamp(Duration::from_secs(3), Duration::from_secs(10))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn android_service_uses_the_same_recovery_timing() {
        let android = include_str!(
            "../../../apps/usque_gui/android/app/src/main/kotlin/io/github/georgexie2333/usque/SessionNetworkRecovery.kt"
        );
        assert!(android.contains("RETRY_INTERVAL_MILLIS = 1_000L"));
        assert!(android.contains("NETWORK_SETTLE_MILLIS = 100L"));
        assert_eq!(RETRY_INTERVAL.as_millis(), 1_000);
        assert_eq!(NETWORK_SETTLE.as_millis(), 100);
    }

    #[test]
    fn liveness_tracks_rtt_with_a_bounded_deadline() {
        assert_eq!(liveness_timeout(None), Duration::from_secs(3));
        assert_eq!(
            liveness_timeout(Some(Duration::from_millis(100))),
            Duration::from_secs(3)
        );
        assert_eq!(
            liveness_timeout(Some(Duration::from_secs(1))),
            Duration::from_secs(4)
        );
        assert_eq!(
            liveness_timeout(Some(Duration::from_secs(4))),
            Duration::from_secs(10)
        );
    }
}
