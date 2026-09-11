//! Once-per-second, bounded directional samples; no packet-level logs.
use std::{collections::VecDeque, time::Instant};
use usque_core::{L4ReceiveInterval, L4ReceiveSnapshot};
pub(super) const LIMIT: usize = 120;

struct Last {
    at: Instant,
    key: (u64, u64),
    layers: [u64; 3],
    packets: u64,
    calls: u64,
    drops: Option<u64>,
}
#[derive(Default)]
pub(super) struct ReceiveHistory {
    origin: Option<Instant>,
    last: Option<Last>,
    entries: VecDeque<L4ReceiveInterval>,
    evicted: u64,
}
impl ReceiveHistory {
    pub(super) fn record(
        &mut self,
        now: Instant,
        key: (u64, u64),
        layers: [u64; 3],
        receive: &mut L4ReceiveSnapshot,
    ) {
        let origin = *self.origin.get_or_insert(now);
        let last = self.last.as_ref();
        let same_path = last.is_some_and(|last| last.key == key);
        let delta =
            |index: usize| last.map_or(0, |last| layers[index].saturating_sub(last.layers[index]));
        let item = L4ReceiveInterval {
            elapsed_ms: millis(now.saturating_duration_since(origin)),
            interval_ms: last.map_or(0, |last| millis(now.saturating_duration_since(last.at))),
            path_reset: !same_path,
            h3_read_bytes: delta(0),
            tcp_accepted_bytes: delta(1),
            tun_ingress_bytes: delta(2),
            received_datagrams: last
                .filter(|_| same_path)
                .map(|last| receive.received_datagrams.saturating_sub(last.packets)),
            recv_syscalls: last
                .filter(|_| same_path)
                .map(|last| receive.recv_syscalls.saturating_sub(last.calls)),
            socket_drops: last
                .filter(|_| same_path)
                .and_then(|last| receive.socket_drops_reported.zip(last.drops))
                .and_then(|(now, last)| now.checked_sub(last)),
            socket_drops_reported: receive.socket_drops_reported,
        };
        self.last = Some(Last {
            at: now,
            key,
            layers,
            packets: receive.received_datagrams,
            calls: receive.recv_syscalls,
            drops: receive.socket_drops_reported,
        });
        if self.entries.len() == LIMIT {
            self.entries.pop_front();
            self.evicted = self.evicted.saturating_add(1);
        }
        self.entries.push_back(item);
        receive.history = self.entries.iter().cloned().collect();
        receive.history_dropped = self.evicted;
    }
}
fn millis(value: std::time::Duration) -> u64 {
    value.as_millis().min(u128::from(u64::MAX)) as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn direction_deltas_are_bounded_and_do_not_cross_socket_generations() {
        let mut history = ReceiveHistory::default();
        let start = Instant::now();
        let mut receive = L4ReceiveSnapshot::default();
        for i in 0..125 {
            receive.received_datagrams = i * 10;
            history.record(
                start + std::time::Duration::from_secs(i),
                (1, 10),
                [i * 100, i * 99, i * 5],
                &mut receive,
            );
        }
        assert_eq!(receive.history.len(), LIMIT);
        assert_eq!(receive.history_dropped, 5);
        let last = receive.history.last().unwrap();
        assert_eq!((last.h3_read_bytes, last.tun_ingress_bytes), (100, 5));
        assert_eq!(last.received_datagrams, Some(10));
        assert!(last.socket_drops.is_none());
        receive.received_datagrams = 3;
        receive.socket_drops_reported = Some(2);
        history.record(
            start + std::time::Duration::from_secs(125),
            (1, 11),
            [12500, 12375, 625],
            &mut receive,
        );
        let last = receive.history.last().unwrap();
        assert!(last.path_reset);
        assert!(last.received_datagrams.is_none());
        assert!(last.socket_drops.is_none());
        assert_eq!(last.socket_drops_reported, Some(2));
    }
}
