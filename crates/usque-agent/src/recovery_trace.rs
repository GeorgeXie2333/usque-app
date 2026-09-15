//! Best-effort, bounded lifecycle evidence. This never authorizes recovery.
use std::{
    cell::RefCell,
    collections::VecDeque,
    fs,
    io::{self, Read, Seek, SeekFrom, Write},
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    time::{Duration, Instant},
};

use usque_ipc::agent_v1::{
    self, RecoveryHistoryStatus as HistoryStatus, RecoveryTraceEvent as Event,
    RecoveryTraceStage as Stage,
};

pub const TRACE_LOG_NAME: &str = "recovery-trace-v1.jsonl";
const MAX_FILE_BYTES: u64 = 1024 * 1024;
const MAX_RECORD_BYTES: usize = 4096;
const MAX_RECORDS: usize = 128;
const QUEUE_CAPACITY: usize = 128;

#[derive(Clone, Default)]
pub struct TraceSink(Option<Arc<Producer>>);

struct Producer {
    sender: SyncSender<Event>,
    metadata: Arc<Metadata>,
    resource_sequence: AtomicU64,
}

struct Metadata {
    run_id: u64,
    started: Instant,
    sequence: AtomicU64,
    dropped: AtomicU64,
    failures: AtomicU64,
}

#[derive(Clone, Default)]
pub struct TraceResource(Option<Arc<Resource>>);

struct Resource {
    sink: TraceSink,
    id: u64,
    generation: AtomicU64,
}

thread_local! {
    static CURRENT_RESOURCE: RefCell<Option<TraceResource>> = const { RefCell::new(None) };
}

impl TraceSink {
    /// The caller secures the journal directory first. No cleanup depends on
    /// this writer being available, and producers never perform filesystem I/O.
    pub fn open(journal_path: &Path) -> Self {
        let (sink, receiver) = Self::channel(QUEUE_CAPACITY);
        let Some(parent) = journal_path.parent() else {
            return Self::default();
        };
        let path = parent.join(TRACE_LOG_NAME);
        let metadata = Arc::clone(&sink.0.as_ref().expect("enabled sink").metadata);
        let failure_metadata = Arc::clone(&metadata);
        if std::thread::Builder::new()
            .name("usque-recovery-evidence".into())
            .spawn(move || {
                for mut event in receiver {
                    event.dropped_events = metadata.dropped.load(Ordering::Relaxed);
                    event.write_failures = metadata.failures.load(Ordering::Relaxed);
                    if append(&path, &event).is_err() {
                        metadata.failures.fetch_add(1, Ordering::Relaxed);
                    }
                }
            })
            .is_err()
        {
            failure_metadata.failures.fetch_add(1, Ordering::Relaxed);
        }
        sink
    }

    pub(crate) fn channel(capacity: usize) -> (Self, Receiver<Event>) {
        let (sender, receiver) = mpsc::sync_channel(capacity);
        let run_id = (uuid::Uuid::new_v4().as_u128() as u64).max(1);
        let metadata = Arc::new(Metadata {
            run_id,
            started: Instant::now(),
            sequence: AtomicU64::new(0),
            dropped: AtomicU64::new(0),
            failures: AtomicU64::new(0),
        });
        (
            Self(Some(Arc::new(Producer {
                sender,
                metadata,
                resource_sequence: AtomicU64::new(0),
            }))),
            receiver,
        )
    }

    pub fn enabled(&self) -> bool {
        self.0.is_some()
    }

    pub fn resource(&self, generation: u64) -> TraceResource {
        TraceResource(self.0.as_ref().map(|producer| {
            Arc::new(Resource {
                sink: self.clone(),
                id: producer.resource_sequence.fetch_add(1, Ordering::Relaxed) + 1,
                generation: AtomicU64::new(generation),
            })
        }))
    }

    pub fn losses(&self) -> (u64, u64) {
        self.0.as_ref().map_or((0, 0), |producer| {
            (
                producer.metadata.dropped.load(Ordering::Relaxed),
                producer.metadata.failures.load(Ordering::Relaxed),
            )
        })
    }

    pub fn record(&self, mut event: Event) {
        let Some(producer) = &self.0 else { return };
        let metadata = &producer.metadata;
        // Correlation belongs to the writer, never to input/native strings.
        event.schema_version = 1;
        event.agent_run_id = metadata.run_id;
        event.event_sequence = metadata.sequence.fetch_add(1, Ordering::Relaxed) + 1;
        if event.occurred_at_unix_ms == 0 {
            event.occurred_at_unix_ms = crate::recovery_diagnostics::unix_ms();
        }
        event.monotonic_ms = milliseconds(metadata.started.elapsed());
        (event.dropped_events, event.write_failures) = self.losses();
        if producer.sender.try_send(event).is_err() {
            metadata.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// An asynchronous native callback has no resource/transaction context.
    /// Only callbacks occurring inside a scoped foreign call inherit its ID.
    pub fn native_message(&self, level: u32, timestamp_ms: u64, message: &str) {
        let stage = if message.starts_with("Failed to remove adapter when closing") {
            Stage::NativeRemoveFailed
        } else if message.starts_with("Failed to remove orphaned adapter") {
            Stage::NativeOrphanRemoveFailed
        } else if message.starts_with("Removed orphaned adapter") {
            Stage::NativeOrphanRemoved
        } else {
            Stage::NativeOther
        };
        let context = CURRENT_RESOURCE.with(|slot| slot.try_borrow().ok().and_then(|v| v.clone()));
        let mut event = context
            .filter(|resource| resource.same_sink(self))
            .map(|resource| resource.event(stage))
            .unwrap_or(Event {
                stage: stage as i32,
                ..Default::default()
            });
        event.native_level = (1..=3).contains(&level).then_some(level);
        event.occurred_at_unix_ms = timestamp_ms;
        self.record(event);
    }
}

impl TraceResource {
    pub fn generation(&self) -> u64 {
        self.0
            .as_ref()
            .map_or(0, |r| r.generation.load(Ordering::Relaxed))
    }

    pub fn child(&self) -> Self {
        self.0
            .as_ref()
            .map_or_else(Self::default, |r| r.sink.resource(self.generation()))
    }

    pub fn set_generation(&self, generation: u64) {
        if let Some(resource) = &self.0 {
            resource.generation.store(generation, Ordering::Relaxed);
        }
    }

    fn same_sink(&self, sink: &TraceSink) -> bool {
        self.0
            .as_ref()
            .is_some_and(|resource| match (&resource.sink.0, &sink.0) {
                (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                _ => false,
            })
    }

    pub fn event(&self, stage: Stage) -> Event {
        Event {
            journal_generation: self
                .0
                .as_ref()
                .map_or(0, |r| r.generation.load(Ordering::Relaxed)),
            resource_id: self.0.as_ref().map_or(0, |r| r.id),
            stage: stage as i32,
            ..Default::default()
        }
    }

    pub fn emit(&self, event: Event) {
        if let Some(resource) = &self.0 {
            resource.sink.record(event);
        }
    }

    pub fn record(&self, stage: Stage) {
        self.emit(self.event(stage));
    }

    pub fn call<T>(&self, start: Stage, returned: Stage, call: impl FnOnce() -> T) -> T {
        struct RestoreContext(Option<TraceResource>);
        impl Drop for RestoreContext {
            fn drop(&mut self) {
                CURRENT_RESOURCE.with(|slot| *slot.borrow_mut() = self.0.take());
            }
        }
        let old = CURRENT_RESOURCE.with(|slot| slot.replace(Some(self.clone())));
        let _guard = RestoreContext(old);
        self.record(start);
        let began = Instant::now();
        let result = call();
        let mut event = self.event(returned);
        event.elapsed_ms = Some(milliseconds(began.elapsed()));
        self.emit(event);
        result
    }
}

pub fn milliseconds(duration: Duration) -> u64 {
    duration.as_millis().min(u128::from(u64::MAX)) as u64
}

fn append(path: &Path, event: &Event) -> io::Result<()> {
    let mut bytes = serde_json::to_vec(event)?;
    if bytes.len() > MAX_RECORD_BYTES {
        return Err(io::Error::other("oversized lifecycle event"));
    }
    bytes.push(b'\n');
    let mut options = fs::OpenOptions::new();
    options.create(true).write(true).truncate(false);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x0020_0000).share_mode(1); // OPEN_REPARSE_POINT; share read only
    }
    if fs::symlink_metadata(path).is_ok_and(|m| !m.is_file() || is_reparse(&m)) {
        return Err(io::Error::other("unsafe lifecycle evidence entry"));
    }
    let mut file = options.open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || is_reparse(&metadata) {
        return Err(io::Error::other("unsafe lifecycle evidence handle"));
    }
    if metadata.len().saturating_add(bytes.len() as u64) > MAX_FILE_BYTES {
        file.set_len(0)?;
    }
    file.seek(SeekFrom::End(0))?;
    file.write_all(&bytes)?;
    file.sync_data()
}

fn is_reparse(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

/// Read without following reparse points. Invalid/oversized records are never
/// returned verbatim, and one bad line does not hide other valid evidence.
pub fn read(journal_path: &Path) -> agent_v1::RecoveryTrace {
    let mut trace = agent_v1::RecoveryTrace::default();
    let result = (|| -> io::Result<HistoryStatus> {
        let path = journal_path
            .parent()
            .ok_or_else(|| io::Error::other("no parent"))?
            .join(TRACE_LOG_NAME);
        let metadata = match fs::symlink_metadata(&path) {
            Ok(value) => value,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(HistoryStatus::Missing);
            }
            Err(error) => return Err(error),
        };
        if !metadata.is_file() || is_reparse(&metadata) {
            return Ok(HistoryStatus::Unavailable);
        }
        if metadata.len() > MAX_FILE_BYTES {
            return Ok(HistoryStatus::TooLarge);
        }
        let mut options = fs::OpenOptions::new();
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.custom_flags(0x0020_0000);
        }
        let file = options.open(path)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() || is_reparse(&metadata) {
            return Ok(HistoryStatus::Unavailable);
        }
        let mut bytes = Vec::new();
        file.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return Ok(HistoryStatus::TooLarge);
        }
        let mut status = HistoryStatus::Complete;
        let mut events = VecDeque::new();
        for line in bytes.split(|b| *b == b'\n').filter(|line| !line.is_empty()) {
            let event = (line.len() <= MAX_RECORD_BYTES)
                .then(|| serde_json::from_slice::<Event>(line).ok())
                .flatten()
                .and_then(sanitize_event);
            if let Some(event) = event {
                if events.len() == MAX_RECORDS {
                    events.pop_front();
                }
                events.push_back(event);
            } else {
                status = HistoryStatus::Partial;
            }
        }
        trace.events = events.into_iter().collect();
        Ok(status)
    })();
    trace.status = result.unwrap_or(HistoryStatus::Unavailable) as i32;
    trace
}

pub fn sanitize_resource(
    mut value: agent_v1::RecoveryResourceObservation,
) -> agent_v1::RecoveryResourceObservation {
    use agent_v1::{
        RecoveryDiagnosticApi as Api, RecoveryIdentityCheck as Identity, RecoveryPresence,
    };
    if Api::try_from(value.api).is_err() {
        value.api = 0;
    }
    if Identity::try_from(value.identity_check).is_err() {
        value.identity_check = 0;
    }
    if RecoveryPresence::try_from(value.presence).is_err()
        || value.identity_check != Identity::Verified as i32
        || value.win32_code.is_some()
        || value.configret_code.is_some()
    {
        value.presence = 0;
    }
    value.interface_oper_status = value.interface_oper_status.filter(|n| (1..=7).contains(n));
    value.interface_admin_status = value.interface_admin_status.filter(|n| (1..=3).contains(n));
    value.media_connect_state = value.media_connect_state.filter(|n| *n <= 2);
    if value.presence != RecoveryPresence::Present as i32 {
        value.interface_oper_status = None;
        value.interface_admin_status = None;
        value.media_connect_state = None;
        value.devnode_status = None;
        value.problem_code = None;
    }
    value
}

pub fn sanitize_event(mut event: Event) -> Option<Event> {
    let stage = Stage::try_from(event.stage).ok()?;
    if event.schema_version != 1
        || event.agent_run_id == 0
        || event.event_sequence == 0
        || event.occurred_at_unix_ms == 0
        || stage == Stage::Unspecified
    {
        return None;
    }
    let native = matches!(
        stage,
        Stage::NativeRemoveFailed
            | Stage::NativeOrphanRemoveFailed
            | Stage::NativeOrphanRemoved
            | Stage::NativeOther
    );
    if !native && (event.journal_generation == 0 || event.resource_id == 0) {
        return None;
    }
    event.native_level = event.native_level.filter(|n| native && (1..=3).contains(n));
    event.reference_count = event
        .reference_count
        .filter(|n| stage == Stage::AdapterReleaseRequested && (1..=1_000_000).contains(n));
    if !matches!(
        stage,
        Stage::PumpJoinReturned
            | Stage::EndSessionReturned
            | Stage::CloseAdapterReturned
            | Stage::RemovalAttemptReturned
    ) {
        event.elapsed_ms = None;
    }
    if !matches!(
        stage,
        Stage::PumpJoinReturned | Stage::RemovalAttemptReturned
    ) {
        event.succeeded = None;
    }
    if matches!(
        stage,
        Stage::ObservationChanged | Stage::ObservationAbsent | Stage::RemovalAttemptReturned
    ) {
        let sample = sanitize_observation(event.observation.take()?, event.journal_generation);
        let absent = !(sample.status != agent_v1::RecoverySampleStatus::Complete as i32
            || [&sample.interface, &sample.pnp_device].iter().any(|value| {
                value
                    .as_ref()
                    .is_none_or(|r| r.presence != agent_v1::RecoveryPresence::Absent as i32)
            }));
        if stage == Stage::ObservationAbsent && !absent {
            return None;
        }
        if stage == Stage::RemovalAttemptReturned && event.succeeded == Some(true) && !absent {
            event.succeeded = None;
        }
        event.observation = Some(sample);
    } else {
        event.observation = None;
    }
    Some(event)
}

pub fn sanitize_observation(
    mut sample: agent_v1::RecoveryObservation,
    generation: u64,
) -> agent_v1::RecoveryObservation {
    use agent_v1::RecoverySampleStatus as Status;
    if Status::try_from(sample.status).is_err() {
        sample.status = Status::Unspecified as i32;
    }
    if sample.status == Status::Complete as i32 {
        if generation == 0 || sample.journal_generation != generation {
            sample.status = Status::GenerationChanged as i32;
        } else if sample.sampled_at_unix_ms == 0 {
            sample.status = Status::Unavailable as i32;
        }
    }
    if sample.status == Status::Complete as i32 {
        sample.interface = sample.interface.take().map(sanitize_resource);
        sample.pnp_device = sample.pnp_device.take().map(sanitize_resource);
    } else {
        sample.interface = None;
        sample.pnp_device = None;
    }
    sample
}

#[cfg(test)]
mod tests;
