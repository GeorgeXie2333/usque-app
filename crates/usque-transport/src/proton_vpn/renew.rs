//! Runtime account lease. Token rotation is journaled before its only request;
//! cancellation and ambiguous responses leave that journal pending for login.
use super::{
    Failure, Interaction,
    api::{Client, Http},
    crypto::KeyMaterial,
    nodes,
    selection::{Probe, Scope, Selector, WarpProbe},
    session::Access,
    warp_http::WarpHttp,
};
use async_trait::async_trait;
use base64::{Engine, engine::general_purpose::STANDARD};
use std::{fs::File, sync::Arc, time::Duration};
use tokio::sync::{mpsc, oneshot, watch};
use tokio_util::sync::CancellationToken;
use usque_core::{
    chain_exit::{ChainSource, ValidatedProfile},
    proton_vpn::{
        ConnectionFailure, FailureKind, LeasePersistence, Record, SelectedNode, TunnelCredentials,
    },
    vpngate::{GateFailure, PreparedProfile},
};
use zeroize::{Zeroize, Zeroizing};

const MONITOR_INTERVAL: Duration = Duration::from_secs(5);
const KEY_RECOVERY_COOLDOWN: i64 = 300;
// Cover the complete startup budget before accepting a refresh-due certificate.
const CERTIFICATE_STARTUP_MARGIN: i64 = 180;

pub(crate) struct RecoveryRequest {
    pub code: u32,
    pub reply: oneshot::Sender<Result<Arc<TunnelCredentials>, GateFailure>>,
}

enum MetadataOutcome {
    Finished(Result<(), Failure>),
    Recovery(RecoveryRequest),
    CertificateDue,
}

struct NoInteraction;
#[async_trait]
impl Interaction for NoInteraction {
    async fn verify(
        &self,
        _: Zeroizing<String>,
        _: Vec<String>,
        _: &CancellationToken,
    ) -> Result<(Zeroizing<String>, String), Failure> {
        Err(Failure::new("reauthentication_required"))
    }
    async fn totp(&self, _: &CancellationToken) -> Result<Zeroizing<String>, Failure> {
        Err(Failure::new("reauthentication_required"))
    }
    fn phase(&self, _: &str) {}
}

pub(crate) struct Lease {
    persistence: Arc<dyn LeasePersistence>,
    http: Arc<dyn Http>,
    generation: Option<u64>,
    current: Record,
    peer: Option<SelectedNode>,
    retry_at: Option<tokio::time::Instant>,
}

fn current(
    http: &dyn Http,
    generation: Option<u64>,
    cancel: &CancellationToken,
) -> Result<(), Failure> {
    if cancel.is_cancelled() {
        return Err(Failure::new("cancelled"));
    }
    if !http.current(generation) {
        return Err(Failure::new("underlay_changed"));
    }
    Ok(())
}

pub(super) fn gate_failure(failure: &Failure) -> GateFailure {
    let kind = match failure.reason.as_str() {
        "cancelled"
        | "underlay_changed"
        | "api_timeout"
        | "api_transport_failed"
        | "api_tls_failed" => FailureKind::Transport,
        "account_tier_changed" => FailureKind::Reconnect,
        "cryptographic_validation_failed" | "credential_expired" => FailureKind::Certificate,
        "profile_revision_mismatch"
        | "invalid_saved_key"
        | "secure_storage_failed"
        | "session_busy"
        | "selected_server_unavailable"
        | "no_free_wireguard_nodes"
        | "invalid_server_directory"
        | "invalid_client_configuration" => FailureKind::Protocol,
        "rate_limited" => FailureKind::RateLimited,
        "reauthentication_required"
        | "invalid_credentials"
        | "protected_user_mismatch"
        | "verification_required"
        | "device_verification_required" => FailureKind::Authentication,
        _ if failure.status == Some(401) => FailureKind::Authentication,
        _ => FailureKind::Api,
    };
    GateFailure::Proton(ConnectionFailure {
        kind,
        code: failure.code,
        http_status: failure.status,
    })
}

pub(crate) fn scoped(reason: GateFailure) -> GateFailure {
    let kind = match reason {
        GateFailure::Proton(_) => return reason,
        GateFailure::Authentication => FailureKind::Authentication,
        GateFailure::Certificate => FailureKind::Certificate,
        GateFailure::Transport | GateFailure::AddressChanged => FailureKind::Transport,
        GateFailure::Configuration | GateFailure::Protocol | GateFailure::Cleanup => {
            FailureKind::Protocol
        }
    };
    GateFailure::Proton(ConnectionFailure::new(kind))
}

async fn load(persistence: Arc<dyn LeasePersistence>) -> Result<Record, Failure> {
    tokio::task::spawn_blocking(move || persistence.load())
        .await
        .map_err(|_| Failure::new("secure_storage_failed"))?
        .map_err(Failure::from)
}

async fn lock(persistence: Arc<dyn LeasePersistence>) -> Result<File, Failure> {
    // The capability uses a nonblocking file lease. No detached task can wait
    // behind another account operation and then rotate after its owner stops.
    tokio::task::spawn_blocking(move || persistence.lock())
        .await
        .map_err(|_| Failure::new("secure_storage_failed"))?
        .map_err(Failure::from)
}

/// A failed peer must be resolved online on the next attempt. Keep unrelated
/// tokens, certificate and user selection from the latest locked record.
pub(crate) async fn invalidate_peer(
    persistence: Arc<dyn LeasePersistence>,
) -> Result<(), GateFailure> {
    tokio::task::spawn_blocking(move || {
        let _lock = persistence.lock()?;
        let mut record = persistence.load()?;
        if let Some(cache) = &mut record.connection_cache {
            // Nodes without a port override depend on the cached defaults too.
            cache.config_at = 0;
            cache.invalidate_directory();
            persistence.save(&record)?;
        }
        Ok::<_, usque_core::chain_exit::ImportError>(())
    })
    .await
    .map_err(|_| GateFailure::Cleanup)?
    .map_err(|_| GateFailure::Cleanup)
}

impl Lease {
    fn check(&self, cancel: &CancellationToken) -> Result<(), Failure> {
        current(&*self.http, self.generation, cancel)
    }

    async fn save(&self, record: &Record, cancel: &CancellationToken) -> Result<(), Failure> {
        self.check(cancel)?;
        let persistence = self.persistence.clone();
        let http = self.http.clone();
        let generation = self.generation;
        let task_cancel = cancel.clone();
        let record = record.clone();
        // Always await the write, including on cancellation. The owner awaits
        // this lease before replacing a selected tunnel or its platform vault.
        tokio::task::spawn_blocking(move || {
            current(&*http, generation, &task_cancel)?;
            persistence.save(&record).map_err(Failure::from)
        })
        .await
        .map_err(|_| Failure::new("secure_storage_failed"))??;
        self.check(cancel)
    }

    async fn rotate(
        &mut self,
        cancel: &CancellationToken,
    ) -> Result<Arc<TunnelCredentials>, Failure> {
        self.check(cancel)?;
        let _lock = lock(self.persistence.clone()).await?;
        self.check(cancel)?;
        let mut record = load(self.persistence.clone()).await?;
        record.validate()?;
        if record.rotation_pending || !unchanged(&self.current, &record) {
            return Err(Failure::new("reauthentication_required"));
        }
        let keys = KeyMaterial::from_seed(
            record
                .tunnel_seed()
                .try_into()
                .map_err(|_| Failure::new("invalid_saved_key"))?,
        )?;
        let mut client = Client::new(self.http.clone());
        client.config.identity = record.api_identity.clone();
        client.session = record.session.clone();
        let mut access = Access::new(self.persistence.clone(), &client);
        let result = async {
            access.cleanup(&mut client, &mut record, cancel).await?;
            access
                .permissions(&mut client, &mut record, &NoInteraction, cancel)
                .await?;
            if record.max_tier != self.current.max_tier
                || record.session.uid != self.current.session.uid
            {
                return Err(Failure::new("account_tier_changed"));
            }
            access
                .certificate(&mut client, &mut record, &keys, &NoInteraction, cancel)
                .await
        }
        .await;
        let certificate = match result {
            Ok(certificate) => certificate,
            Err(failure) => {
                // We still hold the account lease. Retain a known token update
                // durably recorded by Access, including when a later request
                // failed. A pending rotation or invalidated certificate cannot
                // authorize continued traffic.
                self.current = load(self.persistence.clone()).await?;
                return Err(failure);
            }
        };
        self.check(cancel)?;
        record.certificate_chain = certificate.pem;
        record.expires_at = certificate.expiration_time;
        record.refresh_at = certificate.refresh_time;
        self.save(&record, cancel).await?;
        let credentials = Arc::new(TunnelCredentials {
            certificate_chain: record.certificate_chain.clone(),
            private_key_pkcs8: keys.pkcs8_der(),
            server_name: self.peer.as_ref().unwrap_or(&record.node).domain.clone(),
            expires_at: record.expires_at,
            label: self.peer.as_ref().unwrap_or(&record.node).label.clone(),
        });
        self.current = record;
        Ok(credentials)
    }

    async fn recover(
        &mut self,
        code: u32,
        cancel: &CancellationToken,
    ) -> Result<Arc<TunnelCredentials>, GateFailure> {
        if matches!(code, 86102 | 86105) {
            let _lock = lock(self.persistence.clone())
                .await
                .map_err(|e| gate_failure(&e))?;
            let mut record = load(self.persistence.clone())
                .await
                .map_err(|e| gate_failure(&e))?;
            if record.rotation_pending || !unchanged(&self.current, &record) {
                return Err(gate_failure(&Failure::new("reauthentication_required")));
            }
            let now = super::interaction::now();
            if record.key_recovery_at > 0
                && now.saturating_sub(record.key_recovery_at) < KEY_RECOVERY_COOLDOWN
            {
                return Err(GateFailure::Proton(ConnectionFailure::agent(code)));
            }
            let keys = KeyMaterial::generate().map_err(|_| {
                GateFailure::Proton(ConnectionFailure::new(FailureKind::Certificate))
            })?;
            record.tunnel_seed = Some(Zeroizing::new(keys.ed25519_seed().to_vec()));
            record.connection_cache = None;
            record.key_recovery_at = now;
            record.promote();
            record.certificate_chain.zeroize();
            record.expires_at = 0;
            record.refresh_at = 0;
            // Save the new key before requesting its certificate; recovery from
            // a cancelled or ambiguous request never returns to a revoked key.
            self.save(&record, cancel)
                .await
                .map_err(|e| gate_failure(&e))?;
            self.current = record;
        }
        self.rotate(cancel).await.map_err(|e| gate_failure(&e))
    }

    #[cfg(test)]
    pub(crate) async fn run(
        self,
        sender: watch::Sender<Arc<TunnelCredentials>>,
        cancel: &CancellationToken,
    ) -> Result<(), GateFailure> {
        let (_requests, receiver) = mpsc::channel(1);
        self.run_with_recovery(sender, receiver, watch::channel(true).1, cancel)
            .await
    }

    pub(crate) async fn run_with_recovery(
        self,
        sender: watch::Sender<Arc<TunnelCredentials>>,
        requests: mpsc::Receiver<RecoveryRequest>,
        admitted: watch::Receiver<bool>,
        cancel: &CancellationToken,
    ) -> Result<(), GateFailure> {
        let http = self.http.clone();
        let result = self.run_inner(sender, requests, admitted, cancel).await;
        http.shutdown().await;
        result
    }

    async fn run_inner(
        mut self,
        sender: watch::Sender<Arc<TunnelCredentials>>,
        mut requests: mpsc::Receiver<RecoveryRequest>,
        mut admitted: watch::Receiver<bool>,
        cancel: &CancellationToken,
    ) -> Result<(), GateFailure> {
        let mut retry_at = self
            .retry_at
            .take()
            .unwrap_or_else(tokio::time::Instant::now);
        let mut retry_delay = Duration::from_secs(30);
        let mut metadata_retry_at = tokio::time::Instant::now();
        let mut metadata_retry_delay = Duration::from_secs(30);
        let mut admission_closed = false;
        loop {
            self.check(cancel)
                .map_err(|failure| gate_failure(&failure))?;
            let now = super::interaction::now();
            if now >= self.current.expires_at {
                return Err(GateFailure::Proton(ConnectionFailure::new(
                    FailureKind::Certificate,
                )));
            }
            let renewal_allowed = *admitted.borrow()
                || self.current.expires_at.saturating_sub(now) <= CERTIFICATE_STARTUP_MARGIN;
            if renewal_allowed
                && requests.is_empty()
                && now >= self.current.refresh_at
                && tokio::time::Instant::now() >= retry_at
            {
                let credentials = match self.rotate(cancel).await {
                    Ok(credentials) => credentials,
                    Err(failure) => {
                        if transient(&failure)
                            && !self.current.rotation_pending
                            && self.current.expires_at > super::interaction::now()
                        {
                            retry_at = tokio::time::Instant::now() + retry_delay;
                            retry_delay = (retry_delay * 2).min(Duration::from_secs(300));
                            continue;
                        }
                        return Err(gate_failure(&failure));
                    }
                };
                retry_delay = Duration::from_secs(30);
                self.check(cancel)
                    .map_err(|failure| gate_failure(&failure))?;
                let old = sender.borrow();
                if old.private_key_pkcs8 != credentials.private_key_pkcs8
                    || old.server_name != credentials.server_name
                {
                    return Err(GateFailure::Proton(ConnectionFailure::new(
                        FailureKind::Certificate,
                    )));
                }
                drop(old);
                sender
                    .send(credentials)
                    .map_err(|_| GateFailure::Transport)?;
            }
            if *admitted.borrow()
                && requests.is_empty()
                && now < self.current.refresh_at
                && tokio::time::Instant::now() >= metadata_retry_at
                && self.current.connection_cache.as_ref().is_none_or(|cache| {
                    cache.permissions_refresh_due(now)
                        || cache.directory_refresh_due(now)
                        || cache.config_refresh_due(now)
                })
            {
                let refresh_cancel = cancel.child_token();
                let certificate_due =
                    Duration::from_secs(self.current.refresh_at.saturating_sub(now).max(0) as u64);
                let outcome = {
                    let refresh = self.refresh_metadata(&refresh_cancel);
                    tokio::pin!(refresh);
                    tokio::select! {
                        biased;
                        _ = cancel.cancelled() => {
                            refresh_cancel.cancel();
                            let _ = refresh.await;
                            return Ok(());
                        }
                        Some(request) = requests.recv() => {
                            refresh_cancel.cancel();
                            let _ = refresh.await;
                            MetadataOutcome::Recovery(request)
                        }
                        _ = tokio::time::sleep(certificate_due) => {
                            refresh_cancel.cancel();
                            let _ = refresh.await;
                            MetadataOutcome::CertificateDue
                        }
                        result = &mut refresh => MetadataOutcome::Finished(result),
                    }
                };
                match outcome {
                    MetadataOutcome::Finished(Ok(())) => {
                        metadata_retry_delay = Duration::from_secs(30);
                    }
                    MetadataOutcome::Recovery(request) => {
                        let result = self.recover(request.code, cancel).await;
                        if let Ok(credentials) = &result {
                            sender.send_replace(credentials.clone());
                        }
                        let _ = request.reply.send(result);
                        continue;
                    }
                    MetadataOutcome::CertificateDue => continue,
                    MetadataOutcome::Finished(Err(failure))
                        if transient(&failure) || failure.reason == "session_busy" =>
                    {
                        metadata_retry_at = tokio::time::Instant::now() + metadata_retry_delay;
                        metadata_retry_delay =
                            (metadata_retry_delay * 2).min(Duration::from_secs(300));
                    }
                    MetadataOutcome::Finished(Err(failure)) => return Err(gate_failure(&failure)),
                }
            }
            let until_refresh = Duration::from_secs(
                u64::try_from(
                    self.current
                        .refresh_at
                        .saturating_sub(super::interaction::now()),
                )
                .unwrap_or(0),
            );
            let delay = if !renewal_allowed && until_refresh.is_zero() {
                MONITOR_INTERVAL
            } else if until_refresh.is_zero() {
                MONITOR_INTERVAL
                    .min(retry_at.saturating_duration_since(tokio::time::Instant::now()))
            } else {
                until_refresh.min(MONITOR_INTERVAL)
            }
            .max(Duration::from_millis(100));
            tokio::select! {
                biased;
                _ = cancel.cancelled() => return Ok(()),
                Some(request) = requests.recv() => {
                    let result = self.recover(request.code, cancel).await;
                    if let Ok(credentials) = &result {
                        // A changed key is published only for the next complete
                        // WireGuard session. The requesting agent closes its
                        // admission gate before issuing this request.
                        sender.send_replace(credentials.clone());
                    }
                    let _ = request.reply.send(result);
                }
                changed = admitted.changed(), if !admission_closed => {
                    admission_closed = changed.is_err();
                }
                _ = tokio::time::sleep(delay) => (),
            }
            let saved = load(self.persistence.clone())
                .await
                .map_err(|failure| gate_failure(&failure))?;
            if saved.rotation_pending || !unchanged(&self.current, &saved) {
                return Err(gate_failure(&Failure::new("reauthentication_required")));
            }
        }
    }

    async fn refresh_metadata(&mut self, cancel: &CancellationToken) -> Result<(), Failure> {
        self.check(cancel)?;
        let _lock = lock(self.persistence.clone()).await?;
        let mut record = load(self.persistence.clone()).await?;
        if record.rotation_pending || !unchanged(&self.current, &record) {
            return Err(Failure::new("reauthentication_required"));
        }
        let mut client = Client::new(self.http.clone());
        client.config.identity = record.api_identity.clone();
        client.session = record.session.clone();
        let mut access = Access::new(self.persistence.clone(), &client);
        let result = async {
            access.cleanup(&mut client, &mut record, cancel).await?;
            let now = super::interaction::now();
            if record
                .connection_cache
                .as_ref()
                .is_none_or(|cache| cache.permissions_refresh_due(now))
            {
                access
                    .permissions(&mut client, &mut record, &NoInteraction, cancel)
                    .await?;
            }
            // A tier/UID change invalidates the live identity. Access has saved
            // the known session and cleared its certificate before this check.
            if record.max_tier != self.current.max_tier
                || record.session.uid != self.current.session.uid
            {
                return Err(Failure::new("account_tier_changed"));
            }
            if record.connection_cache.as_ref().is_none_or(|cache| {
                cache.directory_refresh_due(now) || cache.config_refresh_due(now)
            }) {
                access
                    .directory(&mut client, &mut record, &NoInteraction, cancel)
                    .await?;
            }
            if record.max_tier != self.current.max_tier
                || record.session.uid != self.current.session.uid
            {
                return Err(Failure::new("account_tier_changed"));
            }
            access.save(&record, cancel).await
        }
        .await;
        if result.as_ref().is_err_and(|failure| {
            !transient(failure)
                && !matches!(
                    failure.reason.as_str(),
                    "cancelled" | "underlay_changed" | "session_busy"
                )
        }) && !cancel.is_cancelled()
        {
            record.connection_cache = None;
            access.save(&record, cancel).await?;
        }
        // Load the latest atomic state even on cancellation/known token update.
        // Holding the account lease prevents overwriting another account job.
        self.current = if result.is_ok() {
            record
        } else {
            load(self.persistence.clone()).await?
        };
        result
    }
}

fn transient(failure: &Failure) -> bool {
    matches!(
        failure.reason.as_str(),
        "api_timeout" | "api_transport_failed" | "rate_limited"
    ) || failure
        .status
        .is_some_and(|status| status == 429 || (500..600).contains(&status))
}

fn unchanged(left: &Record, right: &Record) -> bool {
    left.id == right.id
        && left.revision == right.revision
        && left.identity == right.identity
        && left.api_identity == right.api_identity
        && left.retired_sessions == right.retired_sessions
        && left.session.uid == right.session.uid
        && left.session.user_id == right.session.user_id
        && left.session.access_token == right.session.access_token
        && left.session.refresh_token == right.session.refresh_token
        && left.ed25519_seed == right.ed25519_seed
        && left.tunnel_seed == right.tunnel_seed
        && left.key_recovery_at == right.key_recovery_at
        && left.certificate_chain == right.certificate_chain
        && left.expires_at == right.expires_at
        && left.refresh_at == right.refresh_at
        && left.max_tier == right.max_tier
        && left.node.endpoint == right.node.endpoint
        && left.node.domain == right.node.domain
        && left.node.public_key == right.node.public_key
        && left.node.metadata.id == right.node.metadata.id
}

fn bound(profile: &PreparedProfile, record: &Record) -> Result<(), Failure> {
    record.validate()?;
    let summary = profile
        .summary
        .as_ref()
        .ok_or_else(|| Failure::new("profile_revision_mismatch"))?;
    let Some(ValidatedProfile::WireGuard(wg)) = profile.custom.as_deref() else {
        return Err(Failure::new("profile_revision_mismatch"));
    };
    let keys = KeyMaterial::from_seed(
        record
            .ed25519_seed
            .as_slice()
            .try_into()
            .map_err(|_| Failure::new("invalid_saved_key"))?,
    )?;
    let peer = STANDARD
        .decode(&record.node.public_key)
        .map_err(|_| Failure::new("profile_revision_mismatch"))?;
    if summary.source != ChainSource::ProtonVpn
        || summary.id != record.id
        || summary.revision != record.revision
        || summary.endpoint != record.node.endpoint
        || wg.endpoint != record.node.endpoint
        || wg.private_key.as_slice() != keys.wireguard_private()
        || wg.public_key.as_slice() != peer
        || wg.preshared_key.is_some()
        || profile.proton.as_ref().is_none_or(|credentials| {
            credentials.server_name != record.node.domain
                || credentials.private_key_pkcs8 != keys.pkcs8_der()
        })
    {
        return Err(Failure::new("profile_revision_mismatch"));
    }
    Ok(())
}

pub(crate) async fn prepare(
    profile: &mut PreparedProfile,
    warp: crate::InternalNetwork,
    cancel: &CancellationToken,
) -> Result<(Arc<TunnelCredentials>, Lease), GateFailure> {
    // The final traffic gate is still closed here (and may close again during
    // authorization recovery). Keep this lease's API and DNS on the existing
    // WARP stack throughout its lifetime; system sockets would depend on the
    // very Proton connection these requests are preparing or repairing.
    let selector = warp.proton_selector.clone();
    let probe = Arc::new(WarpProbe::new(warp.clone()));
    let http = Arc::new(WarpHttp::new(warp));
    let result = prepare_with_http(profile, http.clone(), &selector, probe, cancel).await;
    if result.is_err() {
        http.shutdown().await;
    }
    result.map_err(|failure| gate_failure(&failure))
}

async fn prepare_with_http(
    profile: &mut PreparedProfile,
    http: Arc<dyn Http>,
    selector: &Selector,
    probe: Arc<dyn Probe>,
    cancel: &CancellationToken,
) -> Result<(Arc<TunnelCredentials>, Lease), Failure> {
    let stage_started = tokio::time::Instant::now();
    let generation = http.generation();
    current(&*http, generation, cancel)?;
    let persistence = profile
        .proton_persistence
        .clone()
        .ok_or_else(|| Failure::new("secure_storage_failed"))?;
    let _lock = lock(persistence.clone()).await?;
    let mut record = load(persistence.clone()).await?;
    bound(profile, &record)?;
    if record.rotation_pending {
        return Err(Failure::new("reauthentication_required"));
    }
    let mut client = Client::new(http.clone());
    client.config.identity = record.api_identity.clone();
    client.session = record.session.clone();
    let mut access = Access::new(persistence.clone(), &client);
    access.cleanup(&mut client, &mut record, cancel).await?;
    let permissions_cached = record
        .connection_cache
        .as_ref()
        .is_some_and(|cache| cache.permissions_fresh(super::interaction::now()));
    if !permissions_cached {
        access
            .permissions(&mut client, &mut record, &NoInteraction, cancel)
            .await?;
    }
    let preference = match &profile.proton_server_id {
        Some(id) => (!id.is_empty()).then(|| id.clone()),
        None => record.selected_server().map(str::to_owned),
    };
    let pinned = profile
        .proton_selection
        .selected(record.id, preference.as_deref());
    let selected = preference.clone().or(pinned);
    // Affinity is volatile runtime state, never a saved manual preference.
    record.server_id = preference.clone();
    record.promote();
    // Automatic selection requires every known city. Legacy or size-trimmed
    // snapshots must fetch a full directory before selecting representatives.
    if selected.is_none()
        && let Some(cache) = record.connection_cache.as_mut()
        && (!cache.directory_complete || cache.peers.iter().any(|peer| peer.state.is_none()))
    {
        cache.invalidate_directory();
    }
    let cached = record.connection_cache.as_ref().and_then(|cache| {
        cache
            .peer(selected.as_deref(), super::interaction::now())
            .map(|peer| {
                if selected.is_some() {
                    vec![peer.clone()]
                } else {
                    cache.peers.clone()
                }
            })
    });
    let directory_cached = cached.is_some();
    let directory = if let Some(peers) = cached {
        peers
    } else {
        access
            .directory(&mut client, &mut record, &NoInteraction, cancel)
            .await?
    };
    let peer = if let Some(id) = selected.as_deref() {
        directory
            .into_iter()
            .find(|node| node.metadata.id == id)
            .ok_or_else(|| Failure::new("selected_server_unavailable"))?
    } else {
        selector
            .select(
                &directory,
                Scope {
                    account: record.id,
                    catalogue: record.catalog_revision,
                },
                probe,
                cancel,
            )
            .await?
    };
    current(&*http, generation, cancel)?;
    // Pin before certificate/startup work so an automatic retry after failure
    // repairs this logical node without repeating latency selection.
    profile
        .proton_selection
        .remember(record.id, preference.as_deref(), &peer.metadata.id);
    let keys = KeyMaterial::from_seed(
        record
            .tunnel_seed()
            .try_into()
            .map_err(|_| Failure::new("invalid_saved_key"))?,
    )?;
    let mut retry_at = None;
    let certificate_requested = record.certificate_chain.is_empty()
        || record.expires_at.saturating_sub(super::interaction::now())
            <= CERTIFICATE_STARTUP_MARGIN;
    if certificate_requested {
        let certificate = access
            .certificate(&mut client, &mut record, &keys, &NoInteraction, cancel)
            .await;
        match certificate {
            Ok(certificate) => {
                record.certificate_chain = certificate.pem;
                record.expires_at = certificate.expiration_time;
                record.refresh_at = certificate.refresh_time;
            }
            Err(failure)
                if transient(&failure)
                    && !record.rotation_pending
                    && !record.certificate_chain.is_empty()
                    && record.expires_at > super::interaction::now() =>
            {
                validate_cached(&record, &keys)?;
                retry_at = Some(tokio::time::Instant::now() + Duration::from_secs(30));
            }
            Err(failure) => return Err(failure),
        }
    } else {
        validate_cached(&record, &keys)?;
    }
    record.server_id = preference;
    record.promote();
    access.save(&record, cancel).await?;
    let credentials = Arc::new(TunnelCredentials {
        certificate_chain: record.certificate_chain.clone(),
        private_key_pkcs8: keys.pkcs8_der(),
        server_name: peer.domain.clone(),
        expires_at: record.expires_at,
        label: peer.label.clone(),
    });
    let secrets = nodes::configuration(&keys, &peer)?;
    let parsed = ValidatedProfile::parse(ChainSource::ProtonVpn, &secrets)?;
    let mut summary = parsed.summary(&record.name, record.id, record.revision)?;
    summary.source = ChainSource::ProtonVpn;
    let mut resolved = PreparedProfile::imported(summary, parsed, secrets);
    resolved.proton = Some(credentials.clone());
    resolved.proton_persistence = Some(persistence.clone());
    resolved.proton_server_id = profile.proton_server_id.clone();
    resolved.proton_selection = profile.proton_selection.clone();
    *profile = resolved;
    tracing::debug!(
        probe_stage = "proton_prepare",
        elapsed_us = stage_started.elapsed().as_micros(),
        permissions_cached,
        directory_cached,
        certificate_requested,
        "Proton connection preparation timing"
    );
    let lease = Lease {
        persistence,
        http,
        generation,
        current: record,
        peer: Some(peer),
        retry_at,
    };
    Ok((credentials, lease))
}

fn validate_cached(record: &Record, keys: &KeyMaterial) -> Result<(), Failure> {
    match super::crypto::validate_certificate(
        &record.certificate_chain,
        keys,
        record.expires_at,
        record.refresh_at,
        super::interaction::now(),
    ) {
        Ok(_) | Err(super::crypto::CryptoError::CertificateRefreshDue) => Ok(()),
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
#[path = "renew_tests.rs"]
mod tests;
