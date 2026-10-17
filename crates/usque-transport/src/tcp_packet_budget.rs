//! Local CONNECT-IP capacity control; never sourced from diagnostic samples or ICMP.
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use ts_netstack_smoltcp::netcore::{Channel, HasChannel, stack_control};

use crate::h2::TransportError;

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct H3PacketBudget {
    pub(crate) limit: usize,
    pub(crate) confirmed: bool,
}

pub(crate) async fn wait_for_h3(
    source: &mut Option<watch::Receiver<Option<H3PacketBudget>>>,
) -> Result<Option<H3PacketBudget>, ()> {
    let source = source.as_mut().expect("selected source exists");
    source.changed().await.map_err(|_| ())?;
    Ok(*source.borrow_and_update())
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct TcpPacketBudget {
    pub(crate) revision: u64,
    pub(crate) ipv4: Option<usize>,
    pub(crate) ipv6: Option<usize>,
}

/// One supervisor owns this publisher across sequential active sessions. Actors
/// never get this sender: dropping an old actor's receiver fences late updates.
pub(crate) struct BudgetPublisher(watch::Sender<TcpPacketBudget>);

impl BudgetPublisher {
    pub(crate) fn new() -> (Self, watch::Receiver<TcpPacketBudget>) {
        let (tx, rx) = watch::channel(TcpPacketBudget::default());
        (Self(tx), rx)
    }

    pub(crate) fn publish(&self, budget: Option<H3PacketBudget>) {
        let (ipv4, ipv6) = match budget {
            Some(budget) => (Some(budget.limit.max(128)), Some(budget.limit.max(1280))),
            None => (None, None), // H2 has no QUIC DATAGRAM restriction.
        };
        self.0.send_if_modified(|current| {
            if current.revision != 0 && (current.ipv4, current.ipv6) == (ipv4, ipv6) {
                return false;
            }
            current.revision = current
                .revision
                .checked_add(1)
                .expect("budget revision exhausted");
            current.ipv4 = ipv4;
            current.ipv6 = ipv6;
            true
        });
    }
}

async fn apply(channel: &Channel, budget: TcpPacketBudget) -> Result<(), TransportError> {
    channel
        .request(
            None,
            stack_control::Command::SetTcpPacketBudget {
                revision: budget.revision,
                ipv4: budget.ipv4,
                ipv6: budget.ipv6,
            },
        )
        .await
        .map_err(|error| TransportError::Netstack(error.to_string()))?
        .to_ok()
        .map_err(|error| TransportError::Netstack(error.to_string()))
}

/// Apply before publishing frontends, then retain exactly one cancellable command
/// and one latest-value watch. No per-packet task and no unbounded update queue.
pub(crate) async fn attach(
    channel: &Channel,
    updates: Option<watch::Receiver<TcpPacketBudget>>,
    cancellation: &CancellationToken,
) -> Result<Option<JoinHandle<()>>, TransportError> {
    let Some(mut updates) = updates else {
        return Ok(None);
    };
    let initial = *updates.borrow_and_update();
    tokio::select! {
        biased;
        _ = cancellation.cancelled() => return Err(TransportError::TunnelClosed),
        result = apply(channel, initial) => result?,
    }
    let channel = channel.clone();
    let cancellation = cancellation.clone();
    Ok(Some(tokio::spawn(async move {
        loop {
            tokio::select! {
                biased;
                _ = cancellation.cancelled() => break,
                changed = updates.changed() => if changed.is_err() { break; },
            }
            let budget = *updates.borrow_and_update();
            tokio::select! {
                biased;
                _ = cancellation.cancelled() => break,
                result = apply(&channel, budget) => {
                    if result.is_err() {
                        tracing::error!("local TCP packet budget update failed; stopping its stack");
                        cancellation.cancel();
                        break;
                    }
                }
            }
        }
    })))
}
