//! Event-driven scheduling for established CONNECT-IP replacement attempts.
use super::*;
use crate::socket::PhysicalNetworkSnapshot;

const NETWORK_SETTLE: Duration = Duration::from_millis(250);
const NETWORK_ATTEMPT_SPACING: Duration = Duration::from_secs(1);

pub(super) struct ReconnectSchedule {
    network: Option<watch::Receiver<PhysicalNetworkSnapshot>>,
    observed: Option<PhysicalNetworkSnapshot>,
    last_attempt: Option<Instant>,
    fast_retry_at: Option<Instant>,
    policy: IpPolicy,
}

impl ReconnectSchedule {
    pub(super) fn new(protector: &dyn SocketProtector, policy: IpPolicy) -> Self {
        let network = protector.subscribe_physical_network();
        let observed = network.as_ref().map(|rx| *rx.borrow());
        Self {
            network,
            observed,
            last_attempt: None,
            fast_retry_at: None,
            policy,
        }
    }

    fn offline(&self) -> bool {
        self.observed
            .is_some_and(|state| state.usable_for(self.policy) == Some(false))
    }

    fn observe(&mut self, next: Option<PhysicalNetworkSnapshot>) {
        if let (Some(old), Some(next)) = (self.observed, next)
            && (next.generation < old.generation || next == old)
        {
            return;
        }
        let old = self.observed;
        self.observed = next;
        if next.is_some_and(|state| state.usable_for(self.policy) == Some(true)) && old != next {
            self.fast_retry_at = Some(
                (Instant::now() + NETWORK_SETTLE).max(
                    self.last_attempt
                        .map_or(Instant::now(), |at| at + NETWORK_ATTEMPT_SPACING),
                ),
            );
        } else {
            self.fast_retry_at = None;
        }
    }

    fn refresh(&mut self) {
        if let Some(rx) = self.network.as_mut() {
            let next = *rx.borrow_and_update();
            self.observe(Some(next));
        }
    }

    async fn changed(&mut self) {
        let Some(rx) = self.network.as_mut() else {
            std::future::pending::<()>().await;
            return;
        };
        if rx.changed().await.is_err() {
            self.network = None;
            self.observe(None);
        } else {
            self.refresh();
        }
    }

    /// Returns whether a physical-network event reset the backoff. Cancellation
    /// or closed packet input returns None. Packet queues stay bounded offline.
    pub(super) async fn wait(
        &mut self,
        delay: Duration,
        packet_io: &mut PacketIo,
        cancellation: &CancellationToken,
    ) -> Option<bool> {
        self.refresh();
        let retry_at = Instant::now() + delay;
        loop {
            let deadline = if self.offline() {
                None
            } else {
                Some(self.fast_retry_at.unwrap_or(retry_at))
            };
            tokio::select! {
                biased;
                _ = cancellation.cancelled() => return None,
                _ = self.changed() => {},
                _ = async {
                    match deadline {
                        Some(at) => tokio::time::sleep_until(at).await,
                        None => std::future::pending().await,
                    }
                } => {
                    self.last_attempt = Some(Instant::now());
                    return Some(self.fast_retry_at.take().is_some());
                },
                batch = packet_io.receive_outgoing_batch() => { batch?; },
            }
        }
    }

    /// Cancel an obsolete handshake even if its success is ready in the same
    /// poll. A duplicate or stale notification cannot cancel a current attempt.
    pub(super) async fn connect<F, T>(
        &mut self,
        connect: F,
        cancellation: &CancellationToken,
    ) -> Option<Result<T, TransportError>>
    where
        F: Future<Output = Option<Result<T, TransportError>>>,
    {
        let baseline = self.observed;
        tokio::pin!(connect);
        loop {
            tokio::select! {
                biased;
                _ = cancellation.cancelled() => return None,
                _ = self.changed() => {
                    if self.observed != baseline {
                        return Some(Err(TransportError::UnderlyingNetworkChanged));
                    }
                },
                result = &mut connect => return result,
            }
        }
    }
}
