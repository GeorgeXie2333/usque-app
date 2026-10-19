//! Runtime account lease. Token rotation is journaled before its only request;
//! cancellation and ambiguous responses leave that journal pending for login.
use super::{
    Failure, Interaction,
    api::{Client, Http},
    crypto::KeyMaterial,
    workflow,
};
use crate::InternalNetwork;
use async_trait::async_trait;
use base64::{Engine, engine::general_purpose::STANDARD};
use std::{fs::File, sync::Arc, time::Duration};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
use usque_core::{
    chain_exit::{ChainSource, ValidatedProfile},
    proton_vpn::{LeasePersistence, Node, Record, TunnelCredentials},
    vpngate::{GateFailure, PreparedProfile},
};
use zeroize::Zeroizing;

const MONITOR_INTERVAL: Duration = Duration::from_secs(5);

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
    async fn node(&self, _: Vec<Node>, _: &CancellationToken) -> Result<String, Failure> {
        Err(Failure::new("reauthentication_required"))
    }
    fn phase(&self, _: &str) {}
}

pub(crate) struct Lease {
    persistence: Arc<dyn LeasePersistence>,
    http: Arc<dyn Http>,
    generation: Option<u64>,
    current: Record,
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

fn gate_failure(failure: &Failure) -> GateFailure {
    match failure.reason.as_str() {
        "cancelled"
        | "underlay_changed"
        | "api_timeout"
        | "api_transport_failed"
        | "api_tls_failed" => GateFailure::Transport,
        "cryptographic_validation_failed" | "credential_expired" => GateFailure::Certificate,
        "profile_revision_mismatch"
        | "invalid_saved_key"
        | "secure_storage_failed"
        | "session_busy" => GateFailure::Configuration,
        _ => GateFailure::Authentication,
    }
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
                .ed25519_seed
                .as_slice()
                .try_into()
                .map_err(|_| Failure::new("invalid_saved_key"))?,
        )?;
        let mut client = Client::new(self.http.clone());
        client.session = record.session.clone();
        record.rotation_pending = true;
        self.save(&record, cancel).await?;
        workflow::refresh_session(&mut client, &NoInteraction, cancel).await?;
        self.check(cancel)?;
        record.max_tier = workflow::permissions(
            &mut client,
            &NoInteraction,
            cancel,
            Some(&record.session.user_id),
        )
        .await?;
        self.check(cancel)?;
        record.session = client.session.clone();
        record.rotation_pending = false;
        // Persist the known replacement before asking for its certificate. A
        // later certificate failure never replays an already rotated token.
        self.save(&record, cancel).await?;
        if !client.revoke_retired(cancel).await {
            return Err(Failure::new("cleanup_failed_remote_state_unknown"));
        }
        let certificate = workflow::certificate(&mut client, &keys, &NoInteraction, cancel).await?;
        self.check(cancel)?;
        record.certificate_chain = certificate.pem;
        record.expires_at = certificate.expiration_time;
        record.refresh_at = certificate.refresh_time;
        self.save(&record, cancel).await?;
        let credentials = Arc::new(TunnelCredentials {
            certificate_chain: record.certificate_chain.clone(),
            private_key_pkcs8: keys.pkcs8_der(),
            server_name: record.node.domain.clone(),
            expires_at: record.expires_at,
        });
        self.current = record;
        Ok(credentials)
    }

    pub(crate) async fn run(
        mut self,
        sender: watch::Sender<Arc<TunnelCredentials>>,
        cancel: &CancellationToken,
    ) -> Result<(), GateFailure> {
        loop {
            self.check(cancel)
                .map_err(|failure| gate_failure(&failure))?;
            let now = super::interaction::now();
            if now >= self.current.expires_at {
                return Err(GateFailure::Certificate);
            }
            if now >= self.current.refresh_at {
                let credentials = self
                    .rotate(cancel)
                    .await
                    .map_err(|failure| gate_failure(&failure))?;
                self.check(cancel)
                    .map_err(|failure| gate_failure(&failure))?;
                let old = sender.borrow();
                if old.private_key_pkcs8 != credentials.private_key_pkcs8
                    || old.server_name != credentials.server_name
                {
                    return Err(GateFailure::Certificate);
                }
                drop(old);
                sender
                    .send(credentials)
                    .map_err(|_| GateFailure::Transport)?;
            }
            let delay = Duration::from_secs(
                u64::try_from(
                    self.current
                        .refresh_at
                        .saturating_sub(super::interaction::now()),
                )
                .unwrap_or(0),
            )
            .min(MONITOR_INTERVAL);
            tokio::select! {
                biased;
                _ = cancel.cancelled() => return Ok(()),
                _ = tokio::time::sleep(delay) => (),
            }
            let saved = load(self.persistence.clone())
                .await
                .map_err(|failure| gate_failure(&failure))?;
            if saved.rotation_pending || !unchanged(&self.current, &saved) {
                return Err(GateFailure::Authentication);
            }
        }
    }
}

fn unchanged(left: &Record, right: &Record) -> bool {
    left.id == right.id
        && left.revision == right.revision
        && left.identity == right.identity
        && left.session.uid == right.session.uid
        && left.session.user_id == right.session.user_id
        && left.session.access_token == right.session.access_token
        && left.session.refresh_token == right.session.refresh_token
        && left.ed25519_seed == right.ed25519_seed
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
    profile: &PreparedProfile,
    network: InternalNetwork,
    cancel: &CancellationToken,
) -> Result<(Arc<TunnelCredentials>, Lease), GateFailure> {
    prepare_with_http(profile, Arc::new(network), cancel)
        .await
        .map_err(|failure| gate_failure(&failure))
}

async fn prepare_with_http(
    profile: &PreparedProfile,
    http: Arc<dyn Http>,
    cancel: &CancellationToken,
) -> Result<(Arc<TunnelCredentials>, Lease), Failure> {
    let generation = http.generation();
    current(&*http, generation, cancel)?;
    let persistence = profile
        .proton_persistence
        .clone()
        .ok_or_else(|| Failure::new("secure_storage_failed"))?;
    let record = load(persistence.clone()).await?;
    bound(profile, &record)?;
    let mut lease = Lease {
        persistence,
        http,
        generation,
        current: record,
    };
    let credentials = lease.rotate(cancel).await?;
    Ok((credentials, lease))
}

#[cfg(test)]
#[path = "renew_tests.rs"]
mod tests;
