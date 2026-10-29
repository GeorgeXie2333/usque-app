//! Bounded admission and serialized provider lifecycle. Admission never reports
//! Connected; only a healthy backend plus an acknowledged allow transition can.
use crate::{
    backend::{SessionBackend, SessionHealth},
    credentials::SessionCredentials,
    protocol::*,
};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
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
    sessions: HashSet<Uuid>,
    stop: Option<StopFence>,
}
/// One convergent, terminal Stop per session. Repeated Stop requests observe or
/// retry this operation instead of consuming replay capacity. The canonical
/// identity remains available even when the ordinary journal is exhausted.
struct StopFence {
    envelope: Envelope,
    fingerprint: [u8; 32],
    pending: bool,
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
    stops: mpsc::Sender<Work>,
}

impl Controller {
    /// Must run within a Tokio runtime. Dropping control clients intentionally
    /// does not stop the provider or release protection.
    pub fn spawn(backend: Box<dyn SessionBackend>, queue_capacity: usize) -> Self {
        let (commands, receiver) = mpsc::channel(queue_capacity.max(1));
        let (stops, stop_receiver) = mpsc::channel(1);
        let shared = Arc::new(Shared {
            admission: Mutex::new(Admission {
                fingerprints: HashMap::new(),
                cancellations: HashMap::new(),
                head: None,
                runtime_reserved: false,
                sessions: HashSet::new(),
                stop: None,
            }),
            snapshot: Mutex::new(Snapshot::default()),
        });
        tokio::spawn(run(backend, receiver, stop_receiver, shared.clone()));
        Self {
            shared,
            commands,
            stops,
        }
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
            if !matches!(request.command, Command::Stop) {
                return Ok(Reply {
                    operation_id: request.operation_id,
                    acceptance: Acceptance::Duplicate,
                    snapshot: self.snapshot(),
                });
            }
        }
        if let Some(stop) = &admission.stop
            && stop.envelope.operation_id == request.operation_id
            && stop.fingerprint != fingerprint
        {
            return Err(ControlError::ReplayMismatch);
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
                if admission.stop.is_some() && same
                    || (admission.runtime_reserved && !matches!(&request.command, Command::Protect))
                    || !same && !previous_stopped
                {
                    return Err(ControlError::StaleSession);
                }
                if !same && admission.sessions.contains(&request.session.session_id) {
                    return Err(ControlError::StaleSession);
                }
            }
            Command::Replace { .. } => {
                let Some(head) = &admission.head else {
                    return Err(ControlError::StaleSession);
                };
                if admission.stop.is_some()
                    || !admission.runtime_reserved
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
        if matches!(request.command, Command::Stop) {
            let duplicate = admission
                .stop
                .as_ref()
                .is_some_and(|stop| stop.envelope.operation_id == operation_id);
            if admission.stop.as_ref().is_some_and(|stop| stop.pending)
                || snapshot.phase == Phase::Stopped
                    && snapshot.session.as_ref() == Some(&request.session)
            {
                return Ok(Reply {
                    operation_id,
                    acceptance: if duplicate {
                        Acceptance::Duplicate
                    } else {
                        Acceptance::Accepted
                    },
                    snapshot,
                });
            }
            // A separate bounded lane makes Stop available when ordinary work
            // or its replay journal is full. Nothing is mutated before reserve.
            let permit = self
                .stops
                .clone()
                .try_reserve_owned()
                .map_err(|_| ControlError::Unavailable)?;
            for token in admission.cancellations.values() {
                token.cancel();
            }
            if admission.stop.is_none() {
                if admission.fingerprints.len() < REPLAY_WINDOW {
                    admission.fingerprints.insert(operation_id, fingerprint);
                }
                admission.stop = Some(StopFence {
                    envelope: request,
                    fingerprint,
                    pending: false,
                });
            }
            let stop = admission.stop.as_mut().expect("Stop fence established");
            stop.pending = true;
            permit.send(Work {
                envelope: stop.envelope.clone(),
                cancellation: CancellationToken::new(),
            });
            return Ok(Reply {
                operation_id,
                acceptance: if duplicate {
                    Acceptance::Duplicate
                } else {
                    Acceptance::Accepted
                },
                snapshot,
            });
        }
        if admission.fingerprints.len() == REPLAY_WINDOW {
            return Err(ControlError::ReplayWindowFull);
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
        if matches!(
            request.command,
            Command::Start { .. } | Command::Replace { .. }
        ) {
            admission.runtime_reserved = true;
        }
        if admission.head.as_ref() != Some(&request.session) {
            admission.sessions.insert(request.session.session_id);
            admission.stop = None;
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
    mut stop_receiver: mpsc::Receiver<Work>,
    shared: Arc<Shared>,
) {
    let mut tick = tokio::time::interval(std::time::Duration::from_millis(50));
    let mut clients_connected = true;
    let mut stop_clients_connected = true;
    loop {
        let work = tokio::select! {
            biased;
            work = stop_receiver.recv(), if stop_clients_connected => {
                if work.is_none() { stop_clients_connected = false; }
                work
            }
            work = receiver.recv(), if clients_connected => {
                if work.is_none() {
                    clients_connected = false;
                    // Do not tear down the VPN when its UI/Engine disappears.
                }
                work
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
                None
            }
        };
        let Some(Work {
            envelope,
            cancellation,
        }) = work
        else {
            continue;
        };
        let operation_id = envelope.operation_id;
        // Stop cancels queued operations as well as the in-flight connection.
        // They must never re-arm protection or restart the runtime afterwards.
        if cancellation.is_cancelled() {
            shared
                .admission
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .cancellations
                .remove(&operation_id);
            continue;
        }
        let key = envelope.session;
        let is_stop = matches!(envelope.command, Command::Stop);
        let result = match envelope.command {
            Command::Protect => protect(backend.as_mut(), &shared, &key).await,
            Command::Start {
                configuration,
                credentials,
            } => {
                start(
                    backend.as_mut(),
                    &shared,
                    &key,
                    (configuration, credentials),
                    cancellation,
                    false,
                )
                .await
            }
            Command::Replace {
                configuration,
                credentials,
            } => {
                start(
                    backend.as_mut(),
                    &shared,
                    &key,
                    (configuration, credentials),
                    cancellation,
                    true,
                )
                .await
            }
            Command::Stop => {
                update(&shared, |s| {
                    s.session = Some(key.clone());
                    s.phase = Phase::Stopping;
                    s.selected_path = None;
                });
                async {
                    backend.block(&key).await?;
                    backend.disconnect().await?;
                    backend.clear_dns_routes(&key).await?;
                    backend.release_protection(&key).await
                }
                .await
            }
            Command::Snapshot | Command::Cancel { .. } => Err("invalid_queued_command".into()),
        };
        let stopped = is_stop && result.is_ok();
        let mut admission = shared.admission.lock().unwrap_or_else(|e| e.into_inner());
        admission.cancellations.remove(&operation_id);
        if let Err(code) = result {
            failure(&shared, &code);
        }
        if stopped {
            // Publish terminal state and release the reservation atomically
            // with admission; a new session must not lose its reservation to
            // completion of the preceding Stop.
            admission.runtime_reserved = false;
            update(&shared, |s| {
                s.phase = Phase::Stopped;
                s.protection_active = false;
                s.selected_path = None;
                s.error_code = None;
            });
        }
        if is_stop
            && let Some(stop) = &mut admission.stop
            && stop.envelope.operation_id == operation_id
        {
            stop.pending = false;
        }
    }
}
