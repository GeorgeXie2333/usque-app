//! Bounded target-address racing shared by the HTTP and SOCKS frontends.
use std::collections::{HashSet, VecDeque};
use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use tokio::time::{Instant, sleep_until};
use tokio_util::sync::CancellationToken;

use crate::dns::CandidateResolution;
use crate::h2::TransportError;
use crate::tcp::{DialError, FlowClass, TcpDialer, TcpStream, TcpTarget};

const STAGGER: Duration = Duration::from_millis(250);
const MAX_CANDIDATES: usize = 16;

#[derive(Debug)]
pub(crate) enum CandidateDialError {
    Resolve(TransportError),
    Dial(DialError),
}

type DialFuture = Pin<Box<dyn Future<Output = Result<TcpStream, DialError>> + Send>>;
struct Attempt {
    family: usize,
    ordinal: usize,
    cancellation: CancellationToken,
    armed: bool,
    future: DialFuture,
}

impl Attempt {
    fn new(
        dialer: Arc<dyn TcpDialer>,
        address: IpAddr,
        ordinal: usize,
        port: u16,
        deadline: Instant,
        parent: &CancellationToken,
    ) -> Self {
        let cancellation = parent.child_token();
        let token = cancellation.clone();
        Self {
            family: family(address),
            ordinal,
            cancellation,
            armed: true,
            future: Box::pin(async move {
                dialer
                    .connect(
                        TcpTarget::address(SocketAddr::new(address, port)),
                        deadline,
                        &token,
                        FlowClass::Business,
                    )
                    .await
            }),
        }
    }
}

impl Drop for Attempt {
    fn drop(&mut self) {
        if self.armed {
            self.cancellation.cancel();
        }
    }
}

#[derive(Default)]
struct Candidates {
    queues: [VecDeque<(IpAddr, usize)>; 2],
    // Includes launched addresses, bounding the total attempts as well as queues.
    seen: HashSet<IpAddr>,
    preferred: Option<usize>,
    ordinal: usize,
}

fn family(address: IpAddr) -> usize {
    usize::from(address.is_ipv6())
}

impl Candidates {
    fn add(&mut self, addresses: Vec<IpAddr>, pending: [bool; 2]) {
        for address in addresses {
            if self.seen.contains(&address) {
                continue;
            }
            let family = family(address);
            let other_present = self
                .seen
                .iter()
                .any(|ip| crate::tcp_candidates::family(*ip) != family);
            let limit = MAX_CANDIDATES - usize::from(pending[1 - family] && !other_present);
            if self.seen.len() >= limit {
                // A static/system result can put many addresses of one family
                // first. Retain a late alternative by replacing only an untried
                // tail, never an active or previously attempted address.
                if !self
                    .seen
                    .iter()
                    .any(|ip| crate::tcp_candidates::family(*ip) == family)
                {
                    if let Some((removed, _)) = self.queues[1 - family].pop_back() {
                        self.seen.remove(&removed);
                    } else {
                        continue;
                    }
                } else {
                    continue;
                }
            }
            self.seen.insert(address);
            self.queues[family].push_back((address, self.ordinal));
            self.ordinal += 1;
            self.preferred.get_or_insert(family);
        }
    }

    fn next_family(&self, attempts: &[Option<Attempt>; 2], pending: [bool; 2]) -> Option<usize> {
        if attempts.iter().all(Option::is_some) {
            return None;
        }
        if let Some(active) = attempts.iter().flatten().next() {
            let other = 1 - active.family;
            if !self.queues[other].is_empty() {
                return Some(other);
            }
            return (!pending[other] && !self.queues[active.family].is_empty())
                .then_some(active.family);
        }
        let preferred = self.preferred.unwrap_or(0);
        [preferred, 1 - preferred]
            .into_iter()
            .find(|&family| !self.queues[family].is_empty())
    }

    fn is_empty(&self) -> bool {
        self.queues.iter().all(VecDeque::is_empty)
    }
}

fn pending_families(resolution: &Option<CandidateResolution>) -> [bool; 2] {
    [true, false].map(|ipv4| resolution.as_ref().is_some_and(|r| r.pending_family(ipv4)))
}

async fn wait_attempt(attempt: &mut Option<Attempt>) -> Result<TcpStream, DialError> {
    match attempt {
        Some(attempt) => attempt.future.as_mut().await,
        None => std::future::pending().await,
    }
}

async fn next_resolution(
    resolution: &mut Option<CandidateResolution>,
) -> Option<Result<Vec<IpAddr>, TransportError>> {
    match resolution {
        Some(resolution) => resolution.next().await,
        None => std::future::pending().await,
    }
}

enum Event {
    Dial(usize, Result<TcpStream, DialError>),
    Addresses(Option<Result<Vec<IpAddr>, TransportError>>),
}

pub(crate) async fn connect_candidates(
    dialer: Arc<dyn TcpDialer>,
    resolution: CandidateResolution,
    port: u16,
    deadline: Instant,
    cancellation: &CancellationToken,
) -> Result<TcpStream, CandidateDialError> {
    let mut resolution = Some(resolution);
    let mut candidates = Candidates::default();
    let mut attempts: [Option<Attempt>; 2] = [None, None];
    let mut next_launch = Instant::now();
    let mut last_dial: Option<(usize, DialError)> = None;
    let mut last_dns = None;
    let mut budget_exhausted = false;
    loop {
        if cancellation.is_cancelled() {
            return Err(CandidateDialError::Dial(DialError::Cancelled));
        }
        if Instant::now() >= deadline {
            return Err(CandidateDialError::Dial(DialError::Timeout));
        }
        let launch = if budget_exhausted {
            None
        } else {
            candidates.next_family(&attempts, pending_families(&resolution))
        };
        if let Some(family) = launch
            && Instant::now() >= next_launch
        {
            let (address, ordinal) = candidates.queues[family]
                .pop_front()
                .expect("eligible candidate");
            let slot = attempts
                .iter_mut()
                .find(|slot| slot.is_none())
                .expect("bounded race slot");
            *slot = Some(Attempt::new(
                dialer.clone(),
                address,
                ordinal,
                port,
                deadline,
                cancellation,
            ));
            next_launch = Instant::now() + STAGGER;
            continue;
        }
        if attempts.iter().all(Option::is_none) && resolution.is_none() && candidates.is_empty() {
            return Err(if budget_exhausted {
                CandidateDialError::Dial(DialError::Budget)
            } else if let Some((_, error)) = last_dial {
                CandidateDialError::Dial(error)
            } else {
                CandidateDialError::Resolve(last_dns.unwrap_or_else(|| {
                    TransportError::Dns("no usable A or AAAA records".to_owned())
                }))
            });
        }
        let [first, second] = &mut attempts;
        let event = tokio::select! {
            biased;
            _ = cancellation.cancelled() => return Err(CandidateDialError::Dial(DialError::Cancelled)),
            _ = sleep_until(deadline) => return Err(CandidateDialError::Dial(DialError::Timeout)),
            result = wait_attempt(first), if first.is_some() => Event::Dial(0, result),
            result = wait_attempt(second), if second.is_some() => Event::Dial(1, result),
            result = next_resolution(&mut resolution), if resolution.is_some() => Event::Addresses(result),
            _ = sleep_until(next_launch), if launch.is_some() => continue,
        };
        match event {
            Event::Addresses(Some(Ok(addresses))) => {
                candidates.add(addresses, pending_families(&resolution))
            }
            Event::Addresses(Some(Err(error))) => last_dns = Some(error),
            Event::Addresses(None) => {
                resolution.take();
            }
            Event::Dial(slot, result) => {
                let mut finished = attempts[slot].take().expect("completed race slot");
                match result {
                    Ok(stream) => {
                        // L4 retains descendants of this token in the returned
                        // flow. Only the winner loses its cancellation guard.
                        finished.armed = false;
                        return Ok(stream);
                    }
                    Err(error @ (DialError::Cancelled | DialError::Rejected(401 | 403))) => {
                        return Err(CandidateDialError::Dial(error));
                    }
                    Err(DialError::Budget) => {
                        budget_exhausted = true;
                        candidates.queues.iter_mut().for_each(VecDeque::clear);
                        resolution.take();
                    }
                    Err(error) => {
                        if last_dial.is_none_or(|(ordinal, _)| finished.ordinal >= ordinal) {
                            last_dial = Some((finished.ordinal, error));
                        }
                        candidates.preferred = Some(1 - finished.family);
                        next_launch = Instant::now();
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
