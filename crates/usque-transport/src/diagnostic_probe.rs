//! Explicit Deep Doctor probes; no CONNECT-IP, resolver discovery, or OS mutation.
use std::{net::SocketAddr, sync::Arc, time::Duration};

use bytes::Bytes;
use tokio::time::{Instant, timeout_at};
use tokio_util::sync::CancellationToken;
use usque_core::{DirectDnsMode, DirectDnsSettings};

use crate::{
    DirectDnsError, DirectDnsQueryContext, DirectDnsResolver, MasqueTlsIdentity,
    NetworkQualityTelemetry, SocketProtector,
};

pub(crate) const PROBE_IO_TIMEOUT: Duration = Duration::from_millis(3_800);

/// Allowlisted results only, never remote errors or identifying data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkProbeResult {
    Passed { milliseconds: u64 },
    NotApplicable,
    Failed,
    TimedOut,
    Cancelled,
    NetworkChanged,
}

pub async fn probe_encrypted_dns(
    settings: &DirectDnsSettings,
    protector: Arc<dyn SocketProtector>,
    cancellation: CancellationToken,
) -> NetworkProbeResult {
    if settings.mode == DirectDnsMode::PhysicalSystem {
        return NetworkProbeResult::NotApplicable;
    }
    if cancellation.is_cancelled() {
        return NetworkProbeResult::Cancelled;
    }
    let started = Instant::now();
    let lifetime = cancellation.child_token();
    let _cancel_on_drop = lifetime.clone().drop_guard();
    let resolver = match DirectDnsResolver::new(
        settings,
        Arc::clone(&protector),
        NetworkQualityTelemetry::default(),
        &lifetime,
    ) {
        Ok(resolver) => resolver,
        Err(_) => return NetworkProbeResult::Failed,
    };
    run_dns_probe(resolver, protector, cancellation, lifetime, started).await
}

pub(crate) async fn run_dns_probe(
    resolver: Arc<DirectDnsResolver>,
    protector: Arc<dyn SocketProtector>,
    cancellation: CancellationToken,
    lifetime: CancellationToken,
    started: Instant,
) -> NetworkProbeResult {
    let _cancel_on_drop = lifetime.clone().drop_guard();
    let generation = protector.network_generation().unwrap_or_default();
    // A reserved, fixed name, never a user's query. NXDOMAIN is a valid reply.
    let query = Bytes::from_static(b"\x12\x34\x01\x00\x00\x01\x00\x00\x00\x00\x00\x00\x07example\x07invalid\x00\x00\x01\x00\x01");
    let deadline = started + PROBE_IO_TIMEOUT;
    let outcome = tokio::select! {
        biased;
        _ = cancellation.cancelled() => Err(DirectDnsError::Cancelled),
        reply = timeout_at(deadline, resolver.query(query, DirectDnsQueryContext { network_generation: generation, deadline })) =>
            reply.unwrap_or(Err(DirectDnsError::Timeout)),
    };
    lifetime.cancel();
    // A dedicated pool avoids changing or clearing any business DNS pool.
    // Wait for actual I/O destruction, not only for a driver abort request.
    let cleaned = resolver.close_probe_pool().await;
    if cancellation.is_cancelled() {
        return NetworkProbeResult::Cancelled;
    }
    if protector.network_generation().unwrap_or_default() != generation {
        return NetworkProbeResult::NetworkChanged;
    }
    if !cleaned {
        return NetworkProbeResult::Failed;
    }
    match outcome {
        Ok(_) => NetworkProbeResult::Passed {
            milliseconds: elapsed_ms(started),
        },
        Err(DirectDnsError::Timeout | DirectDnsError::Busy) => NetworkProbeResult::TimedOut,
        Err(DirectDnsError::Cancelled) => NetworkProbeResult::Cancelled,
        Err(DirectDnsError::NetworkChanged) => NetworkProbeResult::NetworkChanged,
        Err(_) => NetworkProbeResult::Failed,
    }
}

/// Caller must hold its connection-lifecycle exclusion while disconnected.
/// This completes only the authenticated QUIC handshake, with no HTTP/3 or
/// CONNECT-IP stream ever constructed, and closes the socket before its lease.
pub async fn probe_h3_handshake(
    endpoint: SocketAddr,
    sni: &str,
    identity: &MasqueTlsIdentity,
    protector: Arc<dyn SocketProtector>,
    cancellation: CancellationToken,
) -> NetworkProbeResult {
    crate::h3::diagnostic::handshake(endpoint, sni, identity, protector, cancellation).await
}

pub(crate) fn elapsed_ms(started: Instant) -> u64 {
    started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
}
