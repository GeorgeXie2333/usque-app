//! City coverage and short-lived RTT hints on the existing WARP network only.
//! A ping response is a ranking hint, never WireGuard or local-agent admission.
use super::Failure;
use crate::InternalNetwork;
use async_trait::async_trait;
use std::{
    collections::{BTreeMap, VecDeque},
    sync::Arc,
    time::Duration,
};
use tokio::{sync::Mutex, task::JoinSet, time::Instant};
use tokio_util::sync::CancellationToken;
use usque_core::proton_vpn::SelectedNode;
use uuid::Uuid;

mod ping;
pub(super) use ping::WarpProbe;
#[cfg(test)]
mod tests;

const DEFAULT_CONCURRENCY: usize = 8;
const MAX_CONCURRENCY: usize = 16;
const CITY_ATTEMPTS: usize = 3;
const FINALISTS: usize = 5;
const RESULT_TTL: Duration = Duration::from_secs(30);
const HISTORY_TTL: Duration = Duration::from_secs(300);
const PROBE_TIMEOUT: Duration = Duration::from_secs(1);

type Generation = (u64, Option<u64>);
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct Scope {
    pub account: Uuid,
    pub catalogue: Uuid,
}

#[async_trait]
pub(super) trait Probe: Send + Sync {
    fn generation(&self) -> Option<Generation>;
    async fn measure(
        &self,
        peer: &SelectedNode,
        cancel: &CancellationToken,
    ) -> Result<Option<Duration>, Failure>;
}

// Include connection identity so changed endpoints/keys cannot inherit an RTT.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct PeerKey(String, String, u16, String);
impl From<&SelectedNode> for PeerKey {
    fn from(peer: &SelectedNode) -> Self {
        Self(
            peer.metadata.id.clone(),
            peer.endpoint.host.clone(),
            peer.endpoint.port,
            peer.public_key.clone(),
        )
    }
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct City(String, String, String, Option<String>);
fn normalize(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}
fn city(peer: &SelectedNode) -> City {
    let name = normalize(&peer.metadata.city);
    City(
        peer.metadata.country.clone(),
        normalize(peer.state.as_deref().unwrap_or("")),
        name.clone(),
        name.is_empty().then(|| peer.metadata.id.clone()),
    )
}

struct Observation {
    at: Instant,
    rtt: Duration,
}
#[derive(Default)]
struct Cache {
    scope: Option<(Scope, Generation)>,
    observations: BTreeMap<PeerKey, Observation>,
    winner: Option<(Instant, PeerKey)>,
}

/// Shared only by clones of one WARP handle; never persisted or sent to the UI.
pub(crate) struct Selector {
    cache: Mutex<Cache>,
    concurrency: usize,
}
impl Default for Selector {
    fn default() -> Self {
        Self {
            cache: Mutex::default(),
            concurrency: DEFAULT_CONCURRENCY,
        }
    }
}

fn check(
    probe: &dyn Probe,
    generation: Generation,
    cancel: &CancellationToken,
) -> Result<(), Failure> {
    if cancel.is_cancelled() {
        return Err(Failure::new("cancelled"));
    }
    if probe.generation() != Some(generation) {
        return Err(Failure::new("underlay_changed"));
    }
    Ok(())
}

impl Selector {
    pub(super) async fn select(
        &self,
        nodes: &[SelectedNode],
        scope: Scope,
        probe: Arc<dyn Probe>,
        cancel: &CancellationToken,
    ) -> Result<SelectedNode, Failure> {
        let generation = probe
            .generation()
            .ok_or_else(|| Failure::new("underlay_changed"))?;
        let mut cache = tokio::select! { biased;
            _ = cancel.cancelled() => return Err(Failure::new("cancelled")),
            cache = self.cache.lock() => cache,
        };
        check(&*probe, generation, cancel)?;
        if nodes.is_empty() || nodes.len() > usque_core::proton_vpn::MAX_CATALOG_NODES {
            return Err(Failure::new("invalid_server_directory"));
        }
        if cache.scope != Some((scope, generation)) {
            *cache = Cache {
                scope: Some((scope, generation)),
                ..Default::default()
            };
        }
        if let Some((at, key)) = &cache.winner
            && at.elapsed() < RESULT_TTL
            && let Some(peer) = nodes.iter().find(|node| PeerKey::from(*node) == *key)
        {
            return Ok(peer.clone());
        }
        cache.winner = None;
        cache
            .observations
            .retain(|_, value| value.at.elapsed() < HISTORY_TTL);
        let groups = groups(nodes, &cache);
        let city_count = groups.len();
        let child = cancel.child_token();
        let started = Instant::now();
        // Work uses a private snapshot. Cancellation/network changes never publish
        // partial rankings; every spawned probe is cancelled and joined below.
        let work = scan(groups, probe.clone(), self.concurrency, &child);
        tokio::pin!(work);
        let mut monitor = tokio::time::interval(Duration::from_millis(50));
        let result = loop {
            tokio::select! { biased;
                _ = cancel.cancelled() => {
                    child.cancel();
                    let _ = work.await;
                    break Err(Failure::new("cancelled"));
                }
                _ = monitor.tick() => {
                    if let Err(error) = check(&*probe, generation, cancel) {
                        child.cancel();
                        let _ = work.await;
                        break Err(error);
                    }
                }
                result = &mut work => break result,
            }
        }?;
        check(&*probe, generation, cancel)?;
        // Keep only observations from this complete scan, bounding the cache by
        // the current catalogue even if endpoints change without a new ID.
        cache.observations.clear();
        for (peer, rtt) in &result.observations {
            cache.observations.insert(
                PeerKey::from(peer),
                Observation {
                    at: Instant::now(),
                    rtt: *rtt,
                },
            );
        }
        let peer = result.winner.as_ref().unwrap_or(&nodes[0]).clone();
        // Probe filtering must not make an otherwise usable VPN impossible to
        // connect. All-timeout scans use the directory order without caching a
        // fabricated RTT; final WireGuard/mTLS admission is still mandatory.
        if result.winner.is_some() {
            cache.winner = Some((Instant::now(), PeerKey::from(&peer)));
        }
        tracing::debug!(
            probe_stage = "proton_city_selection",
            cities = city_count,
            responding = result.observations.len(),
            elapsed_us = started.elapsed().as_micros(),
            "Proton city selection timing"
        );
        Ok(peer)
    }
}

fn groups(nodes: &[SelectedNode], cache: &Cache) -> Vec<VecDeque<SelectedNode>> {
    let mut cities: BTreeMap<City, Vec<SelectedNode>> = BTreeMap::new();
    for peer in nodes {
        cities.entry(city(peer)).or_default().push(peer.clone());
    }
    cities
        .into_values()
        .map(|mut nodes| {
            nodes.sort_by_key(|node| {
                let historical = cache
                    .observations
                    .get(&PeerKey::from(node))
                    .map(|value| value.rtt);
                (
                    historical.is_none(),
                    historical,
                    node.metadata.load,
                    node.metadata.name.clone(),
                    node.metadata.id.clone(),
                )
            });
            nodes.truncate(CITY_ATTEMPTS);
            nodes.into()
        })
        .collect()
}

type Measurement = (usize, SelectedNode, Result<Option<Duration>, Failure>);
struct Scan {
    observations: Vec<(SelectedNode, Duration)>,
    winner: Option<SelectedNode>,
}
async fn scan(
    mut groups: Vec<VecDeque<SelectedNode>>,
    probe: Arc<dyn Probe>,
    concurrency: usize,
    cancel: &CancellationToken,
) -> Result<Scan, Failure> {
    let mut tasks = JoinSet::new();
    let result = scan_inner(
        &mut groups,
        probe,
        concurrency.clamp(1, MAX_CONCURRENCY),
        cancel,
        &mut tasks,
    )
    .await;
    tasks.shutdown().await;
    result
}

async fn scan_inner(
    groups: &mut [VecDeque<SelectedNode>],
    probe: Arc<dyn Probe>,
    concurrency: usize,
    cancel: &CancellationToken,
    tasks: &mut JoinSet<Measurement>,
) -> Result<Scan, Failure> {
    let mut pending: VecDeque<usize> = (0..groups.len()).collect();
    let mut found = Vec::new();
    while !pending.is_empty() || !tasks.is_empty() {
        while tasks.len() < concurrency {
            let Some(index) = pending.pop_front() else {
                break;
            };
            let Some(peer) = groups[index].pop_front() else {
                continue;
            };
            let probe = probe.clone();
            let cancel = cancel.clone();
            tasks.spawn(async move {
                let result = measure(&*probe, &peer, &cancel).await;
                (index, peer, result)
            });
        }
        if tasks.is_empty() {
            break;
        }
        let (index, peer, rtt) = next(tasks, cancel).await?;
        if let Some(rtt) = rtt? {
            found.push((peer, rtt));
        } else if !groups[index].is_empty() {
            // Finish first representatives of other cities before retries.
            pending.push_back(index);
        }
    }
    sort(&mut found);
    let finalists: Vec<_> = found.iter().take(FINALISTS).cloned().collect();
    let finalist_count = finalists.len();
    // At most five finalists, still respecting a smaller configured concurrency.
    for (index, (peer, initial)) in finalists.into_iter().enumerate() {
        let probe = probe.clone();
        let task_cancel = cancel.clone();
        tasks.spawn(async move {
            let result = async {
                let first = measure(&*probe, &peer, &task_cancel).await?;
                let second = measure(&*probe, &peer, &task_cancel).await?;
                let mut samples = [
                    initial,
                    first.unwrap_or(PROBE_TIMEOUT),
                    second.unwrap_or(PROBE_TIMEOUT),
                ];
                samples.sort();
                Ok((first.is_some() || second.is_some()).then_some(samples[1]))
            }
            .await;
            (index, peer, result)
        });
        if tasks.len() >= concurrency {
            let (index, peer, rtt) = next(tasks, cancel).await?;
            found[index] = (peer, rtt?.unwrap_or(PROBE_TIMEOUT));
        }
    }
    while !tasks.is_empty() {
        let (index, peer, rtt) = next(tasks, cancel).await?;
        found[index] = (peer, rtt?.unwrap_or(PROBE_TIMEOUT));
    }
    // Only confirmed candidates compete for the winner. An unconfirmed sixth
    // node must not beat their medians using a single lucky initial sample.
    let mut confirmed = found[..finalist_count].to_vec();
    confirmed.retain(|(_, rtt)| *rtt < PROBE_TIMEOUT);
    sort(&mut confirmed);
    let winner = confirmed.first().map(|(peer, _)| peer.clone());
    found.retain(|(_, rtt)| *rtt < PROBE_TIMEOUT);
    Ok(Scan {
        observations: found,
        winner,
    })
}

fn sort(found: &mut [(SelectedNode, Duration)]) {
    found.sort_by(|(a, ar), (b, br)| {
        (*ar, a.metadata.load, &a.metadata.id).cmp(&(*br, b.metadata.load, &b.metadata.id))
    });
}

async fn next(
    tasks: &mut JoinSet<Measurement>,
    cancel: &CancellationToken,
) -> Result<Measurement, Failure> {
    tokio::select! { biased;
        _ = cancel.cancelled() => Err(Failure::new("cancelled")),
        result = tasks.join_next() => result
            .ok_or_else(|| Failure::new("api_transport_failed"))?
            .map_err(|_| Failure::new("api_transport_failed")),
    }
}

async fn measure(
    probe: &dyn Probe,
    peer: &SelectedNode,
    cancel: &CancellationToken,
) -> Result<Option<Duration>, Failure> {
    tokio::select! { biased;
        _ = cancel.cancelled() => Err(Failure::new("cancelled")),
        result = tokio::time::timeout(PROBE_TIMEOUT, probe.measure(peer, cancel)) => result.unwrap_or(Ok(None)),
    }
}

impl InternalNetwork {
    pub(crate) async fn invalidate_proton_selection(&self) {
        *self.proton_selector.cache.lock().await = Cache::default();
    }
}
