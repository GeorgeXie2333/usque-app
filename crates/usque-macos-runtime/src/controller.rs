//! Bounded admission and serialized provider lifecycle. Admission never reports
//! Connected; only a healthy backend plus an acknowledged allow transition can.
use crate::{
    backend::{SessionBackend, SessionHealth},
    credentials::SessionCredentials,
    protocol::*,
};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use zeroize::Zeroizing;

const REPLAY_WINDOW: usize = 4096;
struct Admission {
    fingerprints: HashMap<Uuid, [u8; 32]>,
    cancellations: HashMap<Uuid, CancellationToken>,
    head: Option<SessionKey>,
    runtime_reserved: bool,
}
struct Shared {
    admission: Mutex<Admission>,
    snapshot: Mutex<Snapshot>,
}
struct Work {
    envelope: Envelope,
    cancellation: CancellationToken,
}
/// Admission may lead the worker. The new key is pending until protection is
/// acknowledged, never already connected/stopped as its predecessor was.
fn snapshot_for_key(mut snapshot: Snapshot, key: &SessionKey) -> Snapshot {
    if snapshot.session.as_ref() != Some(key) {
        snapshot.session = Some(key.clone());
        snapshot.phase = Phase::Starting;
        snapshot.protection_active = false;
        snapshot.selected_path = None;
        snapshot.error_code = None;
    }
    snapshot
}
#[derive(Clone)]
pub struct Controller {
    shared: Arc<Shared>,
    commands: mpsc::Sender<Work>,
}

impl Controller {
    /// Must run within a Tokio runtime. Dropping control clients intentionally
    /// does not stop the provider or release protection.
    pub fn spawn(backend: Box<dyn SessionBackend>, queue_capacity: usize) -> Self {
        let (commands, receiver) = mpsc::channel(queue_capacity.max(1));
        let shared = Arc::new(Shared {
            admission: Mutex::new(Admission {
                fingerprints: HashMap::new(),
                cancellations: HashMap::new(),
                head: None,
                runtime_reserved: false,
            }),
            snapshot: Mutex::new(Snapshot::default()),
        });
        tokio::spawn(run(backend, receiver, shared.clone()));
        Self { shared, commands }
    }
    pub fn snapshot(&self) -> Snapshot {
        self.shared
            .snapshot
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
    pub fn submit_json(&self, bytes: &[u8]) -> Result<Reply, ControlError> {
        if bytes.len() > MAX_CONTROL_BYTES {
            return Err(ControlError::InvalidRequest);
        }
        let request = serde_json::from_slice(bytes).map_err(|_| ControlError::InvalidRequest)?;
        self.submit(request)
    }
    pub fn submit(&self, request: Envelope) -> Result<Reply, ControlError> {
        if request.version != PROTOCOL_VERSION {
            return Err(ControlError::UnsupportedVersion);
        }
        if request.operation_id.is_nil()
            || request.session.session_id.is_nil()
            || request.session.config_revision == 0
        {
            return Err(ControlError::InvalidRequest);
        }
        let encoded =
            Zeroizing::new(serde_json::to_vec(&request).map_err(|_| ControlError::InvalidRequest)?);
        if encoded.len() > MAX_CONTROL_BYTES {
            return Err(ControlError::InvalidRequest);
        }
        let fingerprint: [u8; 32] = Sha256::digest(encoded.as_slice()).into();
        let mut admission = self
            .shared
            .admission
            .lock()
            .map_err(|_| ControlError::Unavailable)?;
        if let Some(previous) = admission.fingerprints.get(&request.operation_id) {
            if previous != &fingerprint {
                return Err(ControlError::ReplayMismatch);
            }
            return Ok(Reply {
                operation_id: request.operation_id,
                acceptance: Acceptance::Duplicate,
                snapshot: self.snapshot(),
            });
        }
        if admission.fingerprints.len() == REPLAY_WINDOW {
            return Err(ControlError::ReplayWindowFull);
        }
        let snapshot = self.snapshot();
        match &request.command {
            Command::Snapshot => {
                if admission
                    .head
                    .as_ref()
                    .is_some_and(|head| head != &request.session)
                {
                    return Err(ControlError::StaleSession);
                }
            }
            Command::Protect | Command::Start { .. } => {
                let same = admission.head.as_ref() == Some(&request.session);
                let previous_stopped = snapshot.phase == Phase::Stopped
                    && snapshot.session.as_ref() == admission.head.as_ref();
                if (admission.runtime_reserved && !matches!(&request.command, Command::Protect))
                    || !same && !previous_stopped
                {
                    return Err(ControlError::StaleSession);
                }
                if !same
                    && admission
                        .head
                        .as_ref()
                        .is_some_and(|head| head.session_id == request.session.session_id)
                {
                    return Err(ControlError::StaleSession);
                }
            }
            Command::Replace { .. } => {
                let Some(head) = &admission.head else {
                    return Err(ControlError::StaleSession);
                };
                if !admission.runtime_reserved
                    || request.session.session_id != head.session_id
                    || request.session.config_revision < head.config_revision
                    || request.session.network_generation < head.network_generation
                    || request.session == *head
                {
                    return Err(ControlError::StaleSession);
                }
            }
            Command::Stop | Command::Cancel { .. } => {
                if admission.head.as_ref() != Some(&request.session) {
                    return Err(ControlError::StaleSession);
                }
            }
        }
        let operation_id = request.operation_id;
        if matches!(request.command, Command::Snapshot) {
            // Observations cannot cause effects and are polled indefinitely;
            // reserve the bounded replay journal for mutating operations.
            return Ok(Reply {
                operation_id,
                acceptance: Acceptance::Accepted,
                snapshot: snapshot_for_key(snapshot, &request.session),
            });
        }
        if let Command::Cancel {
            operation_id: target,
        } = request.command
        {
            let token = admission
                .cancellations
                .get(&target)
                .ok_or(ControlError::InvalidRequest)?;
            token.cancel();
            admission.fingerprints.insert(operation_id, fingerprint);
            return Ok(Reply {
                operation_id,
                acceptance: Acceptance::Accepted,
                snapshot: self.snapshot(),
            });
        }
        // Reserve before journal or head mutation. Failed admission cannot
        // silently supersede a previously admitted session or retain secrets.
        let permit = self
            .commands
            .clone()
            .try_reserve_owned()
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => ControlError::Backpressure,
                mpsc::error::TrySendError::Closed(_) => ControlError::Unavailable,
            })?;
        let cancellation = CancellationToken::new();
        if matches!(request.command, Command::Stop) {
            // Stop interrupts long startup even if the bounded queue is full
            // of other accepted work; each queued operation then observes it.
            for token in admission.cancellations.values() {
                token.cancel();
            }
        }
        if matches!(
            request.command,
            Command::Start { .. } | Command::Replace { .. }
        ) {
            admission.runtime_reserved = true;
        }
        admission.head = Some(request.session.clone());
        admission
            .cancellations
            .insert(operation_id, cancellation.clone());
        admission.fingerprints.insert(operation_id, fingerprint);
        let admitted = request.session.clone();
        permit.send(Work {
            envelope: request,
            cancellation,
        });
        Ok(Reply {
            operation_id,
            acceptance: Acceptance::Accepted,
            snapshot: snapshot_for_key(self.snapshot(), &admitted),
        })
    }
}

fn update(shared: &Shared, operation: impl FnOnce(&mut Snapshot)) {
    let mut snapshot = shared.snapshot.lock().unwrap_or_else(|e| e.into_inner());
    operation(&mut snapshot);
    snapshot.production_capable = false;
}
fn failure(shared: &Shared, code: &str) {
    update(shared, |snapshot| {
        snapshot.phase = Phase::Failed;
        snapshot.selected_path = None;
        snapshot.error_code = Some(code.into());
    });
}
async fn protect(
    backend: &mut dyn SessionBackend,
    shared: &Shared,
    key: &SessionKey,
) -> Result<(), String> {
    backend.protect(key).await?;
    update(shared, |s| {
        s.session = Some(key.clone());
        s.phase = Phase::Protected;
        s.protection_active = true;
        s.selected_path = None;
        s.error_code = None;
    });
    Ok(())
}
async fn start(
    backend: &mut dyn SessionBackend,
    shared: &Shared,
    key: &SessionKey,
    handoff: (SessionConfiguration, SessionCredentials),
    cancellation: CancellationToken,
    replacement: bool,
) -> Result<(), String> {
    let (configuration, credentials) = handoff;
    protect(backend, shared, key).await?;
    if replacement {
        backend.disconnect().await?;
        backend.clear_dns_routes(key).await?;
    }
    if cancellation.is_cancelled() {
        return Err("operation_cancelled".into());
    }
    backend
        .configure_dns_routes(key, &configuration, &credentials)
        .await?;
    update(shared, |s| s.phase = Phase::Starting);
    let result = backend
        .connect(key, configuration, credentials, cancellation.clone())
        .await;
    if result.is_err() || cancellation.is_cancelled() {
        backend.block(key).await?;
        backend.disconnect().await?;
        return Err(if cancellation.is_cancelled() {
            "operation_cancelled".into()
        } else {
            result.err().unwrap_or_else(|| "startup_failed".into())
        });
    }
    Ok(())
}
async fn run(
    mut backend: Box<dyn SessionBackend>,
    mut receiver: mpsc::Receiver<Work>,
    shared: Arc<Shared>,
) {
    let mut tick = tokio::time::interval(std::time::Duration::from_millis(50));
    let mut clients_connected = true;
    loop {
        tokio::select! {
            work = receiver.recv(), if clients_connected => {
                let Some(Work { envelope, cancellation }) = work else {
                    clients_connected = false;
                    // Do not tear down the VPN when its UI/Engine disappears.
                    continue;
                };
                let operation_id = envelope.operation_id;
                let key = envelope.session;
                let result = match envelope.command {
                    Command::Protect => protect(backend.as_mut(), &shared, &key).await,
                    Command::Start { configuration, credentials } => start(backend.as_mut(), &shared, &key, (configuration, credentials), cancellation, false).await,
                    Command::Replace { configuration, credentials } => start(backend.as_mut(), &shared, &key, (configuration, credentials), cancellation, true).await,
                    Command::Stop => {
                        update(&shared, |s| { s.phase = Phase::Stopping; s.selected_path = None; });
                        let result = async {
                            backend.block(&key).await?;
                            backend.disconnect().await?;
                            backend.clear_dns_routes(&key).await?;
                            backend.release_protection(&key).await
                        }.await;
                        if result.is_ok() {
                            update(&shared, |s| { s.phase = Phase::Stopped; s.protection_active = false; s.selected_path = None; s.error_code = None; });
                            let mut admission = shared.admission.lock().unwrap_or_else(|e| e.into_inner());
                            admission.runtime_reserved = false;
                        }
                        result
                    }
                    Command::Snapshot | Command::Cancel { .. } => Err("invalid_queued_command".into()),
                };
                if let Err(code) = result { failure(&shared, &code); }
                shared.admission.lock().unwrap_or_else(|e| e.into_inner()).cancellations.remove(&operation_id);
            }
            _ = tick.tick() => {
                let snapshot = shared.snapshot.lock().unwrap_or_else(|e| e.into_inner()).clone();
                if matches!(snapshot.phase, Phase::Starting | Phase::Connected) {
                    let Some(key) = snapshot.session else { continue; };
                    match backend.health() {
                        SessionHealth::Connected(path) => {
                            if snapshot.phase != Phase::Connected {
                                match backend.allow(&key).await {
                                    Ok(()) => update(&shared, |s| { s.phase = Phase::Connected; s.selected_path = Some(path); }),
                                    Err(code) => failure(&shared, &code),
                                }
                            }
                        }
                        SessionHealth::Starting => {
                            if snapshot.phase == Phase::Connected {
                                match backend.block(&key).await {
                                    Ok(()) => update(&shared, |s| { s.phase = Phase::Starting; s.selected_path = None; }),
                                    Err(code) => failure(&shared, &code),
                                }
                            }
                        }
                        SessionHealth::Failed => {
                            let code = backend.block(&key).await.err().unwrap_or_else(|| "transport_failed".into());
                            failure(&shared, &code);
                        }
                    }
                }
            }
        }
    }
}
