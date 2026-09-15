//! User-local, bounded historical evidence. Never an input to connection/recovery.
use std::{
    fs,
    io::{self, Read, Write},
    path::Path,
};

use super::{PlatformState, Value, agent_v1, json};
use serde::{Deserialize, Serialize};

pub(crate) const FILE_NAME: &str = "windows-recovery-cache-v1.json";
const MAX_BYTES: u64 = 192 * 1024;
type WriteGate = std::sync::Mutex<()>;
static WRITE_GATES: std::sync::Mutex<Vec<(std::path::PathBuf, std::sync::Weak<WriteGate>)>> =
    std::sync::Mutex::new(Vec::new());

fn write_gate(directory: &Path) -> io::Result<std::sync::Arc<WriteGate>> {
    let mut gates = WRITE_GATES
        .lock()
        .map_err(|_| io::Error::other("evidence gates unavailable"))?;
    gates.retain(|(_, gate)| gate.strong_count() != 0);
    if let Some(gate) = gates
        .iter()
        .find_map(|(path, gate)| (path == directory).then(|| gate.upgrade()).flatten())
    {
        return Ok(gate);
    }
    let gate = std::sync::Arc::new(std::sync::Mutex::new(()));
    gates.push((directory.to_owned(), std::sync::Arc::downgrade(&gate)));
    Ok(gate)
}

#[derive(Serialize, Deserialize)]
struct Record {
    schema_version: u32,
    captured_at_unix_ms: u64,
    agent_phase: i32,
    pending_cleanup: bool,
    automatic_phase: Option<i32>,
    attempts_completed: Option<u32>,
    attempt_limit: Option<u32>,
    // A closed enum, never the Agent's code/message string.
    terminal: Option<Terminal>,
    diagnostics: agent_v1::RecoveryDiagnostics,
}

#[derive(Serialize, Deserialize)]
enum Terminal {
    Exhausted,
    Blocked,
}

fn useful(value: &Value) -> bool {
    [&value["history"], &value["trace"]["events"]]
        .iter()
        .any(|rows| rows.as_array().is_some_and(|rows| !rows.is_empty()))
}

/// Rebuild every persisted field. No PlatformState/journal/receipt strings are
/// written; the public export independently rechecks all fields when reading.
fn record(state: &PlatformState, captured_at_unix_ms: u64) -> Option<Record> {
    if captured_at_unix_ms == 0 || !useful(&super::summary(Some(state))) {
        return None;
    }
    let diagnostics = state.recovery_diagnostics.as_ref()?;
    let mut history: Vec<_> = diagnostics
        .history
        .iter()
        .rev()
        .filter(|e| {
            e.occurred_at_unix_ms != 0
                && e.journal_generation != 0
                && agent_v1::RecoveryStep::try_from(e.step)
                    .is_ok_and(|s| s != agent_v1::RecoveryStep::Unspecified)
        })
        .take(32)
        .map(|e| agent_v1::RecoveryHistoryEvent {
            occurred_at_unix_ms: e.occurred_at_unix_ms,
            journal_generation: e.journal_generation,
            step: e.step,
            restored: e.restored,
            elapsed_ms: e.elapsed_ms,
            api: e.api,
            win32_code: e.win32_code,
            adapter: e.adapter.as_ref().map(|a| agent_v1::RecoveryRemovalResult {
                stage: a.stage,
                failure: a.failure,
                interface: a.interface,
                pnp_device: a.pnp_device,
                request_accepted: a.request_accepted,
                elapsed_ms: a.elapsed_ms,
                api: a.api,
                win32_code: a.win32_code,
            }),
        })
        .collect();
    history.reverse();
    let trace = diagnostics.trace.as_ref().map(|trace| {
        let readable = matches!(
            agent_v1::RecoveryHistoryStatus::try_from(trace.status),
            Ok(agent_v1::RecoveryHistoryStatus::Complete
                | agent_v1::RecoveryHistoryStatus::Partial)
        );
        let mut events: Vec<_> = trace
            .events
            .iter()
            .rev()
            .filter(|_| readable)
            .filter_map(super::trace::cache_event)
            .take(128)
            .collect();
        events.reverse();
        agent_v1::RecoveryTrace {
            status: if events.len() != trace.events.len() && readable {
                agent_v1::RecoveryHistoryStatus::Partial as i32
            } else {
                trace.status
            },
            events,
            dropped_events: trace.dropped_events,
            write_failures: trace.write_failures,
        }
    });
    let automatic = state.automatic_recovery.as_ref();
    Some(Record {
        schema_version: 1,
        captured_at_unix_ms,
        agent_phase: state.agent_phase,
        pending_cleanup: state.pending_cleanup,
        automatic_phase: automatic.map(|s| s.phase),
        attempts_completed: automatic.map(|s| s.attempts_completed),
        attempt_limit: automatic.map(|s| s.attempt_limit),
        terminal: automatic
            .and_then(|s| s.terminal_error.as_ref())
            .and_then(|e| match e.code.as_str() {
                "AGENT_AUTOMATIC_RECOVERY_EXHAUSTED" => Some(Terminal::Exhausted),
                "AGENT_AUTOMATIC_RECOVERY_BLOCKED" => Some(Terminal::Blocked),
                _ => None,
            }),
        diagnostics: agent_v1::RecoveryDiagnostics {
            current: diagnostics.current.as_ref().map(super::cache_observation),
            history_status: diagnostics.history_status,
            history,
            trace,
        },
    })
}

pub(crate) fn store(
    directory: &Path,
    state: &PlatformState,
    captured_at_unix_ms: u64,
) -> io::Result<bool> {
    let Some(mut record) = record(state, captured_at_unix_ms) else {
        return Ok(false);
    };
    // Export and the background recorder can finish concurrently. Atomic file
    // replacement prevents torn reads; this lock also prevents an older capture
    // from overwriting a later one or discarding a temporarily unreadable trace.
    let gate = write_gate(directory)?;
    let _guard = gate
        .try_lock()
        .map_err(|_| io::Error::other("evidence lock unavailable"))?;
    if let Ok(previous) = read(directory) {
        if previous.captured_at_unix_ms > record.captured_at_unix_ms {
            return Ok(false);
        }
        merge_history(&mut record.diagnostics, previous.diagnostics);
    }
    let bytes = serde_json::to_vec(&record)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(io::Error::other("recovery evidence exceeds limit"));
    }
    fs::create_dir_all(directory)?;
    let path = directory.join(FILE_NAME);
    if fs::symlink_metadata(&path).is_ok_and(|m| !safe_file(&m)) {
        return Err(io::Error::other("unsafe recovery evidence entry"));
    }
    let mut temporary = tempfile::NamedTempFile::new_in(directory)?;
    temporary.write_all(&bytes)?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|e| e.error)?;
    Ok(true)
}

fn merge_history(
    current: &mut agent_v1::RecoveryDiagnostics,
    previous: agent_v1::RecoveryDiagnostics,
) {
    let mut history = previous.history;
    for event in current.history.drain(..) {
        if !history.contains(&event) {
            history.push(event);
        }
    }
    if history.len() > 32 {
        history.drain(..history.len() - 32);
    }
    current.history = history;
    if let Some(previous) = previous.trace {
        let mut events: Vec<_> = previous
            .events
            .iter()
            .filter_map(super::trace::cache_event)
            .collect();
        if let Some(current) = current.trace.as_mut() {
            for event in current.events.drain(..) {
                if let Some(old) = events.iter_mut().find(|old| {
                    old.agent_run_id == event.agent_run_id
                        && old.event_sequence == event.event_sequence
                }) {
                    *old = event;
                } else {
                    events.push(event);
                }
            }
            if events.len() > 128 {
                events.drain(..events.len() - 128);
            }
            current.events = events;
            if !current.events.is_empty()
                && !matches!(
                    agent_v1::RecoveryHistoryStatus::try_from(current.status),
                    Ok(agent_v1::RecoveryHistoryStatus::Complete
                        | agent_v1::RecoveryHistoryStatus::Partial)
                )
            {
                current.status = agent_v1::RecoveryHistoryStatus::Partial as i32;
            }
        } else {
            if events.len() > 128 {
                events.drain(..events.len() - 128);
            }
            current.trace = Some(agent_v1::RecoveryTrace {
                status: agent_v1::RecoveryHistoryStatus::Partial as i32,
                events,
                dropped_events: previous.dropped_events,
                write_failures: previous.write_failures,
            });
        }
    }
}

fn safe_file(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.is_file() && metadata.file_attributes() & 0x400 == 0
    }
    #[cfg(not(windows))]
    {
        metadata.is_file() && !metadata.file_type().is_symlink()
    }
}

fn read(directory: &Path) -> io::Result<Record> {
    let path = directory.join(FILE_NAME);
    if !safe_file(&fs::symlink_metadata(&path)?) {
        return Err(io::Error::other("unsafe evidence"));
    }
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x0020_0000); // OPEN_REPARSE_POINT
    }
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    if !safe_file(&metadata) || metadata.len() > MAX_BYTES {
        return Err(io::Error::other("invalid evidence size/type"));
    }
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(io::Error::other("oversized evidence"));
    }
    let record: Record = serde_json::from_slice(&bytes)?;
    if record.schema_version != 1 || record.captured_at_unix_ms == 0 {
        return Err(io::Error::other("unsupported recovery evidence"));
    }
    Ok(record)
}

pub(crate) fn summary(platform: Option<&PlatformState>, directory: &Path) -> Value {
    let mut live = super::summary(platform);
    // A second full trace would exceed the bundle's evidence budget. Keep live
    // evidence when present; cache is exclusively a labelled historical fallback.
    live["cached_evidence"] = if useful(&live) {
        json!({"availability": "not_needed"})
    } else {
        match read(directory) {
            Ok(record) => {
                let state = PlatformState {
                    agent_phase: record.agent_phase,
                    pending_cleanup: record.pending_cleanup,
                    automatic_recovery: record.automatic_phase.map(|phase| {
                        agent_v1::AutomaticRecoveryStatus {
                            phase,
                            attempts_completed: record.attempts_completed.unwrap_or(u32::MAX),
                            attempt_limit: record.attempt_limit.unwrap_or(0),
                            terminal_error: record.terminal.map(|terminal| agent_v1::AgentError {
                                code: match terminal {
                                    Terminal::Exhausted => "AGENT_AUTOMATIC_RECOVERY_EXHAUSTED",
                                    Terminal::Blocked => "AGENT_AUTOMATIC_RECOVERY_BLOCKED",
                                }
                                .into(),
                                message: String::new(),
                                retryable: false,
                            }),
                        }
                    }),
                    recovery_diagnostics: Some(Box::new(record.diagnostics)),
                    ..Default::default()
                };
                let mut snapshot = super::summary(Some(&state));
                let fields = snapshot.as_object_mut().expect("summary object");
                let observation = fields.remove("current_observation").unwrap_or(Value::Null);
                fields.insert("observation_at_capture".into(), observation);
                json!({"availability": "available", "source": "previous_agent_response",
                    "captured_at_unix_ms": record.captured_at_unix_ms, "snapshot": snapshot})
            }
            Err(error) => {
                json!({"availability": if error.kind() == io::ErrorKind::NotFound {"missing"} else {"unavailable"}})
            }
        }
    };
    live
}

#[cfg(test)]
mod tests;
