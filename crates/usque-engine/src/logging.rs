//! Bounded, privacy-filtered JSONL logging for the desktop engine.

use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    io::{self, BufWriter, Write},
    net::{IpAddr, SocketAddr},
    path::{Path, PathBuf},
    sync::{
        Arc, Condvar, Mutex, OnceLock, Weak,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        mpsc::{self, SyncSender, TrySendError},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use serde::Serialize;
use serde_json::Value;
use tracing_subscriber::fmt::MakeWriter;
use usque_core::diagnostics_contract_generated::LOG_EVENT_CODES;

const MAX_TOTAL_BYTES: u64 = 20 * 1024 * 1024;
const ROTATE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_EVENT_BYTES: usize = 256 * 1024;
const MAX_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);
const MAX_ACTIVE_SEGMENT_AGE: Duration = Duration::from_secs(24 * 60 * 60);
const ACTIVE_LOG_NAME: &str = "engine.jsonl";
const MAX_QUEUED_EVENTS: usize = 248;
const MAX_QUEUED_BYTES: usize = 2 * 1024 * 1024;
const COMMAND_CAPACITY: usize = 256;
const SYNC_INTERVAL: Duration = Duration::from_secs(1);
const PRUNE_INTERVAL: Duration = Duration::from_secs(60);
pub(crate) const MAX_LOG_DIRECTORY_ENTRIES: usize = 1024;

#[derive(Clone, Debug, Default, Serialize)]
pub struct LogHealthSnapshot {
    pub running: bool,
    pub writer_available: bool,
    pub queued_events: usize,
    pub queued_bytes: usize,
    pub written_events: u64,
    pub dropped_events: u64,
    pub queue_full_events: u64,
    pub write_failures: u64,
    pub oversized_events: u64,
    pub unconfirmed_events: u64,
    pub clear_epoch: u64,
}

#[derive(Default)]
struct LogHealth {
    running: AtomicBool,
    writer_available: AtomicBool,
    accepting: AtomicBool,
    stopping: AtomicBool,
    queued_events: AtomicUsize,
    queued_bytes: AtomicUsize,
    written_events: AtomicU64,
    dropped_events: AtomicU64,
    queue_full_events: AtomicU64,
    write_failures: AtomicU64,
    oversized_events: AtomicU64,
    unconfirmed_events: AtomicU64,
    clear_epoch: AtomicU64,
    stopped: Mutex<bool>,
    completed: Condvar,
}

impl LogHealth {
    fn snapshot(&self) -> LogHealthSnapshot {
        LogHealthSnapshot {
            running: self.running.load(Ordering::Acquire),
            writer_available: self.writer_available.load(Ordering::Acquire),
            queued_events: self.queued_events.load(Ordering::Acquire),
            queued_bytes: self.queued_bytes.load(Ordering::Acquire),
            written_events: self.written_events.load(Ordering::Acquire),
            dropped_events: self.dropped_events.load(Ordering::Acquire),
            queue_full_events: self.queue_full_events.load(Ordering::Acquire),
            write_failures: self.write_failures.load(Ordering::Acquire),
            oversized_events: self.oversized_events.load(Ordering::Acquire),
            unconfirmed_events: self.unconfirmed_events.load(Ordering::Acquire),
            clear_epoch: self.clear_epoch.load(Ordering::Acquire),
        }
    }
}

struct LogShared {
    sender: SyncSender<LogCommand>,
    health: Arc<LogHealth>,
}

impl Drop for LogShared {
    fn drop(&mut self) {
        self.health.accepting.store(false, Ordering::Release);
        self.health.stopping.store(true, Ordering::Release);
    }
}

enum LogCommand {
    Event(QueuedEvent),
    Operation(Box<dyn FnOnce(&mut LogState) + Send>),
}

struct QueuedEvent {
    bytes: Vec<u8>,
    health: Arc<LogHealth>,
    written: bool,
    epoch: u64,
}

impl Drop for QueuedEvent {
    fn drop(&mut self) {
        self.health.queued_events.fetch_sub(1, Ordering::AcqRel);
        self.health
            .queued_bytes
            .fetch_sub(self.bytes.len(), Ordering::AcqRel);
        if !self.written {
            self.health.dropped_events.fetch_add(1, Ordering::Relaxed);
        }
    }
}

static WRITERS: OnceLock<Mutex<HashMap<PathBuf, Weak<LogShared>>>> = OnceLock::new();

fn normalized_directory(directory: &Path) -> PathBuf {
    let absolute = if directory.is_absolute() {
        directory.to_owned()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(directory)
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            _ => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

fn live_writer(directory: &Path) -> Option<Arc<LogShared>> {
    let writers = WRITERS
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    writers
        .get(&normalized_directory(directory))
        .and_then(Weak::upgrade)
}

pub fn log_health(directory: &Path) -> Option<LogHealthSnapshot> {
    live_writer(directory).map(|shared| shared.health.snapshot())
}

#[derive(Clone)]
pub struct LogWriterFactory {
    shared: Arc<LogShared>,
}

struct LogState {
    directory: PathBuf,
    active_path: PathBuf,
    file: Option<BufWriter<File>>,
    bytes_written: u64,
    rotation_counter: u32,
    health: Arc<LogHealth>,
    retry_after: Instant,
    pending_events: u64,
    active_started: SystemTime,
}

impl LogWriterFactory {
    pub fn open(config_path: &Path) -> io::Result<Self> {
        let directory = normalized_directory(&log_directory(config_path));
        let mut writers = WRITERS
            .get_or_init(Mutex::default)
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(shared) = writers.get(&directory).and_then(Weak::upgrade)
            && shared.health.running.load(Ordering::Acquire)
        {
            return Ok(Self { shared });
        }
        let health = Arc::new(LogHealth::default());
        health.running.store(true, Ordering::Release);
        health.accepting.store(true, Ordering::Release);
        let (sender, receiver) = mpsc::sync_channel(COMMAND_CAPACITY);
        let shared = Arc::new(LogShared {
            sender,
            health: Arc::clone(&health),
        });
        let worker_health = Arc::clone(&health);
        if std::thread::Builder::new()
            .name("usque-log-writer".into())
            .spawn(move || run_writer(directory, receiver, worker_health))
            .is_err()
        {
            health.accepting.store(false, Ordering::Release);
            health.running.store(false, Ordering::Release);
            health.write_failures.fetch_add(1, Ordering::Relaxed);
            *health
                .stopped
                .lock()
                .unwrap_or_else(|error| error.into_inner()) = true;
        }
        writers.retain(|_, writer| writer.strong_count() != 0);
        writers.insert(
            normalized_directory(&log_directory(config_path)),
            Arc::downgrade(&shared),
        );
        // Disk and worker failures produce a disabled, observable sink. Logging
        // must not prevent the Engine from starting or performing cleanup.
        Ok(Self { shared })
    }

    pub fn health(&self) -> LogHealthSnapshot {
        self.shared.health.snapshot()
    }

    pub fn flush(&self, timeout: Duration) -> io::Result<()> {
        owner_operation(&self.shared, timeout, LogState::sync)
    }

    /// Stop accepting events, drain the bounded queue, and sync the active file.
    /// A timeout only bounds the caller; the worker retains its owned state.
    pub fn shutdown(&self, timeout: Duration) -> io::Result<()> {
        self.shared.health.accepting.store(false, Ordering::Release);
        self.shared.health.stopping.store(true, Ordering::Release);
        let stopped = self
            .shared
            .health
            .stopped
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let (stopped, _) = self
            .shared
            .health
            .completed
            .wait_timeout_while(stopped, timeout, |stopped| !*stopped)
            .unwrap_or_else(|error| error.into_inner());
        if !*stopped {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "log shutdown timed out",
            ));
        }
        if !self.shared.health.writer_available.load(Ordering::Acquire) {
            return Err(io::Error::other(
                "log shutdown completed with an unavailable writer",
            ));
        }
        Ok(())
    }
}

fn owner_operation<T: Send + 'static>(
    shared: &Arc<LogShared>,
    timeout: Duration,
    operation: impl FnOnce(&mut LogState) -> io::Result<T> + Send + 'static,
) -> io::Result<T> {
    if !shared.health.accepting.load(Ordering::Acquire) {
        return Err(io::Error::new(
            io::ErrorKind::BrokenPipe,
            "log writer is stopped",
        ));
    }
    let (reply, result) = mpsc::channel();
    shared
        .sender
        .try_send(LogCommand::Operation(Box::new(move |state| {
            let _ = reply.send(operation(state));
        })))
        .map_err(|error| match error {
            TrySendError::Full(_) => {
                io::Error::new(io::ErrorKind::WouldBlock, "log command queue is full")
            }
            TrySendError::Disconnected(_) => {
                io::Error::new(io::ErrorKind::BrokenPipe, "log writer is unavailable")
            }
        })?;
    result.recv_timeout(timeout).map_err(|error| match error {
        mpsc::RecvTimeoutError::Timeout => {
            io::Error::new(io::ErrorKind::TimedOut, "log operation timed out")
        }
        mpsc::RecvTimeoutError::Disconnected => {
            io::Error::new(io::ErrorKind::BrokenPipe, "log operation was abandoned")
        }
    })?
}

pub fn flush_logs(directory: &Path, timeout: Duration) -> io::Result<()> {
    let shared = live_writer(directory)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no live log writer"))?;
    owner_operation(&shared, timeout, LogState::sync)
}

pub(crate) fn capture_logs<T: Send + 'static>(
    directory: &Path,
    timeout: Duration,
    capture: impl FnOnce() -> T + Send + 'static,
) -> io::Result<T> {
    match live_writer(directory) {
        Some(shared) if shared.health.running.load(Ordering::Acquire) => {
            owner_operation(&shared, timeout, move |state| {
                state.sync()?;
                Ok(capture())
            })
        }
        _ => Ok(capture()),
    }
}

pub(crate) fn clear_logs(directory: &Path, timeout: Duration) -> io::Result<()> {
    match live_writer(directory) {
        Some(shared) if shared.health.running.load(Ordering::Acquire) => {
            owner_operation(&shared, timeout, LogState::clear)
        }
        _ => clear_log_files(directory),
    }
}

impl<'a> MakeWriter<'a> for LogWriterFactory {
    type Writer = BufferedLogEvent;

    fn make_writer(&'a self) -> Self::Writer {
        BufferedLogEvent {
            shared: Arc::clone(&self.shared),
            buffer: Vec::with_capacity(1024),
            overflowed: false,
            epoch: self.shared.health.clear_epoch.load(Ordering::Acquire),
        }
    }
}

pub struct BufferedLogEvent {
    shared: Arc<LogShared>,
    buffer: Vec<u8>,
    overflowed: bool,
    epoch: u64,
}

impl Write for BufferedLogEvent {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let remaining = MAX_EVENT_BYTES.saturating_sub(self.buffer.len());
        let copied = remaining.min(bytes.len());
        self.buffer.extend_from_slice(&bytes[..copied]);
        self.overflowed |= copied != bytes.len();
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Drop for BufferedLogEvent {
    fn drop(&mut self) {
        let mut event = if self.overflowed {
            self.shared
                .health
                .oversized_events
                .fetch_add(1, Ordering::Relaxed);
            br#"{"level":"ERROR","target":"usque_engine","message":"oversized log event omitted"}"#
                .to_vec()
        } else {
            sanitize_log_bytes(&self.buffer)
        };
        if event.len() > MAX_EVENT_BYTES {
            self.shared
                .health
                .oversized_events
                .fetch_add(1, Ordering::Relaxed);
            event = br#"{"level":"ERROR","target":"usque_engine","message":"oversized log event omitted"}"#.to_vec();
        }
        if event.is_empty() {
            return;
        }
        let health = &self.shared.health;
        if !health.accepting.load(Ordering::Acquire)
            || self.epoch != health.clear_epoch.load(Ordering::Acquire)
        {
            health.dropped_events.fetch_add(1, Ordering::Relaxed);
            return;
        }
        if health
            .queued_events
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |events| {
                (events < MAX_QUEUED_EVENTS).then_some(events + 1)
            })
            .is_err()
        {
            health.queue_full_events.fetch_add(1, Ordering::Relaxed);
            health.dropped_events.fetch_add(1, Ordering::Relaxed);
            return;
        }
        if health
            .queued_bytes
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |bytes| {
                bytes
                    .checked_add(event.len())
                    .filter(|total| *total <= MAX_QUEUED_BYTES)
            })
            .is_err()
        {
            health.queued_events.fetch_sub(1, Ordering::AcqRel);
            health.queue_full_events.fetch_add(1, Ordering::Relaxed);
            health.dropped_events.fetch_add(1, Ordering::Relaxed);
            return;
        }
        let event = QueuedEvent {
            bytes: event,
            health: Arc::clone(health),
            written: false,
            epoch: self.epoch,
        };
        if self
            .shared
            .sender
            .try_send(LogCommand::Event(event))
            .is_err()
        {
            health.queue_full_events.fetch_add(1, Ordering::Relaxed);
        }
    }
}

struct WorkerCompletion(Arc<LogHealth>);

impl Drop for WorkerCompletion {
    fn drop(&mut self) {
        self.0.accepting.store(false, Ordering::Release);
        self.0.running.store(false, Ordering::Release);
        *self
            .0
            .stopped
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = true;
        self.0.completed.notify_all();
    }
}

fn run_writer(directory: PathBuf, receiver: mpsc::Receiver<LogCommand>, health: Arc<LogHealth>) {
    let _completion = WorkerCompletion(Arc::clone(&health));
    let mut state = LogState {
        active_path: directory.join(ACTIVE_LOG_NAME),
        directory,
        file: None,
        bytes_written: 0,
        rotation_counter: 0,
        health: Arc::clone(&health),
        retry_after: Instant::now(),
        pending_events: 0,
        active_started: SystemTime::now(),
    };
    let _ = state.ensure_open(true);
    let _ = state.rotate_aged_segment(SystemTime::now());
    let mut last_sync = Instant::now();
    let mut last_prune = Instant::now();
    while !health.stopping.load(Ordering::Acquire) {
        match receiver.recv_timeout(Duration::from_millis(100)) {
            Ok(command) => process_command(&mut state, command),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        if last_sync.elapsed() >= SYNC_INTERVAL {
            let _ = state.sync();
            last_sync = Instant::now();
        }
        if last_prune.elapsed() >= PRUNE_INTERVAL {
            let _ = state.rotate_aged_segment(SystemTime::now());
            let limit = MAX_TOTAL_BYTES
                .saturating_sub(ROTATE_BYTES)
                .saturating_add(state.bytes_written.min(ROTATE_BYTES));
            if prune_logs(&state.directory, limit).is_err() {
                health.write_failures.fetch_add(1, Ordering::Relaxed);
            }
            last_prune = Instant::now();
        }
    }
    // Producers are barred first. Every accepted command owns bounded memory;
    // draining cannot turn shutdown into an unbounded queue of new work.
    for _ in 0..COMMAND_CAPACITY {
        let Ok(command) = receiver.try_recv() else {
            break;
        };
        process_command(&mut state, command);
    }
    let _ = state.sync();
}

fn process_command(state: &mut LogState, command: LogCommand) {
    match command {
        LogCommand::Event(mut event) => {
            // The producer can submit after its check but before clear changes
            // the epoch. Validate again on the owner, in FIFO command order.
            if event.epoch != state.health.clear_epoch.load(Ordering::Acquire) {
                return;
            }
            if state.write_event(&event.bytes).is_ok() {
                event.written = true;
            }
        }
        LogCommand::Operation(operation) => operation(state),
    }
}

impl LogState {
    fn ensure_open(&mut self, force: bool) -> io::Result<()> {
        if self.file.is_some() {
            return Ok(());
        }
        if !force && Instant::now() < self.retry_after {
            return Err(io::Error::other("log writer is awaiting retry"));
        }
        let result = (|| {
            fs::create_dir_all(&self.directory)?;
            let existing = fs::metadata(&self.active_path).ok();
            self.bytes_written = existing.as_ref().map_or(0, fs::Metadata::len);
            self.active_started = existing.as_ref().map_or_else(SystemTime::now, |metadata| {
                // Creation time gives a persistent segment boundary where the
                // filesystem supports it. An older mtime also identifies a
                // restored/quiet file; otherwise mtime is the fallback origin.
                [metadata.created().ok(), metadata.modified().ok()]
                    .into_iter()
                    .flatten()
                    .min()
                    .unwrap_or_else(SystemTime::now)
            });
            let limit = MAX_TOTAL_BYTES
                .saturating_sub(ROTATE_BYTES)
                .saturating_add(self.bytes_written.min(ROTATE_BYTES));
            prune_logs(&self.directory, limit)?;
            self.file = Some(open_active_log(&self.active_path)?);
            Ok(())
        })();
        if result.is_err() {
            self.failed();
        } else {
            self.health.writer_available.store(true, Ordering::Release);
        }
        result
    }

    fn failed(&mut self) {
        // Discard a failed buffered writer without retrying its raw buffer on
        // Drop. The data is already sanitized, but duplicate writes are not useful.
        if let Some(file) = self.file.take() {
            let _ = file.into_parts();
        }
        self.health.writer_available.store(false, Ordering::Release);
        self.health.write_failures.fetch_add(1, Ordering::Relaxed);
        self.health
            .unconfirmed_events
            .fetch_add(self.pending_events, Ordering::Relaxed);
        self.pending_events = 0;
        self.retry_after = Instant::now() + SYNC_INTERVAL;
    }

    fn sync(&mut self) -> io::Result<()> {
        self.ensure_open(true)?;
        let result = (|| {
            let file = self
                .file
                .as_mut()
                .ok_or_else(|| io::Error::other("log writer is unavailable"))?;
            file.flush()?;
            file.get_ref().sync_data()
        })();
        if result.is_err() {
            self.failed();
        } else {
            self.health
                .written_events
                .fetch_add(self.pending_events, Ordering::Relaxed);
            self.pending_events = 0;
        }
        result
    }

    fn clear(&mut self) -> io::Result<()> {
        // Formatters created before this command cannot resurrect old records,
        // even when they finish after clear, or enqueue behind the command.
        self.health.clear_epoch.fetch_add(1, Ordering::AcqRel);
        // The single owner closes/flushed its handle before clearing, then
        // resets the length used for rotation and reopens the current file.
        self.sync()?;
        self.file.take();
        let result = clear_log_files(&self.directory);
        self.bytes_written = 0;
        if result.is_err() {
            self.failed();
            return result;
        }
        self.ensure_open(true)?;
        self.active_started = SystemTime::now();
        Ok(())
    }

    fn write_event(&mut self, event: &[u8]) -> io::Result<()> {
        self.ensure_open(false)?;
        self.rotate_aged_segment(SystemTime::now())?;
        let result = self.write_open_event(event);
        if result.is_err() && self.health.writer_available.load(Ordering::Acquire) {
            self.failed();
        }
        result
    }

    fn write_open_event(&mut self, event: &[u8]) -> io::Result<()> {
        if self.bytes_written == 0 {
            self.active_started = SystemTime::now();
        }
        let event_length = u64::try_from(event.len())
            .unwrap_or(u64::MAX)
            .saturating_add(u64::from(!event.ends_with(b"\n")));
        if self.bytes_written > 0 && self.bytes_written.saturating_add(event_length) > ROTATE_BYTES
        {
            self.rotate()?;
        }
        let file = self
            .file
            .as_mut()
            .ok_or_else(|| io::Error::other("active log file is closed"))?;
        file.write_all(event)?;
        if !event.ends_with(b"\n") {
            file.write_all(b"\n")?;
        }
        self.bytes_written = self.bytes_written.saturating_add(event_length);
        self.pending_events = self.pending_events.saturating_add(1);
        Ok(())
    }

    fn rotate(&mut self) -> io::Result<()> {
        self.sync()?;
        self.file.take();
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let rotated_path = loop {
            let candidate = self.directory.join(format!(
                "engine-{timestamp}-{}.jsonl",
                self.rotation_counter
            ));
            self.rotation_counter = self.rotation_counter.wrapping_add(1);
            if !candidate.exists() {
                break candidate;
            }
        };
        if self.active_path.exists() {
            fs::rename(&self.active_path, rotated_path)?;
        }
        self.file = Some(open_active_log(&self.active_path)?);
        self.bytes_written = 0;
        self.active_started = SystemTime::now();
        prune_logs(
            &self.directory,
            MAX_TOTAL_BYTES.saturating_sub(ROTATE_BYTES),
        )?;
        Ok(())
    }

    /// Rotate an old nonempty segment once. Archives expire by their last
    /// modification time, so this is not a per-record seven-day guarantee.
    fn rotate_aged_segment(&mut self, now: SystemTime) -> io::Result<()> {
        if self.file.is_none()
            || self.bytes_written == 0
            || now.duration_since(self.active_started).unwrap_or_default() < MAX_ACTIVE_SEGMENT_AGE
        {
            return Ok(());
        }
        let result = self.rotate();
        if result.is_err() && self.health.writer_available.load(Ordering::Acquire) {
            self.failed();
        }
        result
    }
}

fn open_active_log(path: &Path) -> io::Result<BufWriter<File>> {
    let file = OpenOptions::new().create(true).append(true).open(path)?;
    Ok(BufWriter::with_capacity(64 * 1024, file))
}

fn clear_log_files(directory: &Path) -> io::Result<()> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    for (index, entry) in entries.enumerate() {
        if index >= MAX_LOG_DIRECTORY_ENTRIES {
            return Err(io::Error::other("log directory entry limit exceeded"));
        }
        let entry = entry?;
        if !fs::symlink_metadata(entry.path())?.file_type().is_file() {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == ACTIVE_LOG_NAME {
            OpenOptions::new()
                .write(true)
                .truncate(true)
                .open(entry.path())?;
        } else if is_engine_log_name(&name) || name == "windows-recovery-cache-v1.json" {
            fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}

pub fn log_directory(config_path: &Path) -> PathBuf {
    config_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("logs")
}

fn prune_logs(directory: &Path, byte_limit: u64) -> io::Result<()> {
    let now = SystemTime::now();
    let mut logs = Vec::new();
    for (index, entry) in match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    }
    .enumerate()
    {
        if index >= MAX_LOG_DIRECTORY_ENTRIES {
            return Err(io::Error::other("log directory entry limit exceeded"));
        }
        let entry = entry?;
        let path = entry.path();
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        if !is_engine_log_name(&file_name) {
            continue;
        }
        let metadata = fs::symlink_metadata(&path)?;
        if !metadata.file_type().is_file() {
            continue;
        }
        let modified = metadata.modified().unwrap_or(UNIX_EPOCH);
        if file_name != ACTIVE_LOG_NAME
            && now.duration_since(modified).unwrap_or_default() > MAX_AGE
        {
            fs::remove_file(path)?;
            continue;
        }
        logs.push((path, file_name == ACTIVE_LOG_NAME, modified, metadata.len()));
    }
    logs.sort_by_key(|(_, active, modified, _)| (*active, *modified));
    let mut total = logs.iter().map(|(_, _, _, length)| *length).sum::<u64>();
    for (path, active, _, length) in logs {
        if total <= byte_limit {
            break;
        }
        if active {
            continue;
        }
        fs::remove_file(path)?;
        total = total.saturating_sub(length);
    }
    Ok(())
}

fn is_engine_log_name(name: &str) -> bool {
    name == ACTIVE_LOG_NAME || (name.starts_with("engine-") && name.ends_with(".jsonl"))
}

pub fn sanitize_log_bytes(bytes: &[u8]) -> Vec<u8> {
    let text = String::from_utf8_lossy(bytes);
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let Ok(mut value) = serde_json::from_str::<Value>(trimmed) else {
        return br#"{"level":"WARN","target":"usque_engine","message":"non-JSON log event omitted"}"#
            .to_vec();
    };
    redact_log_value(&mut value, None);
    serde_json::to_vec(&value).unwrap_or_else(|_| {
        br#"{"level":"ERROR","target":"usque_engine","message":"log serialization failed"}"#
            .to_vec()
    })
}

fn redact_log_value(value: &mut Value, key: Option<&str>) {
    if key.is_some_and(is_public_numeric_key) && (value.is_number() || value.is_boolean()) {
        return;
    }
    if key.is_some_and(is_sensitive_log_key) {
        *value = Value::String("[REDACTED]".to_owned());
        return;
    }
    match value {
        Value::Object(map) => {
            for (key, value) in map {
                redact_log_value(value, Some(key));
            }
        }
        Value::Array(items) => {
            for item in items {
                redact_log_value(item, None);
            }
        }
        Value::String(text) => *text = scrub_network_tokens(text),
        _ => {}
    }
}

fn is_sensitive_log_key(key: &str) -> bool {
    let normalized = key.to_ascii_lowercase();
    [
        "access_token",
        "assertion",
        "authorization",
        "callback_uri",
        "certificate",
        "cf-access-jwt-assertion",
        "cookie",
        "credential",
        "device_id",
        "endpoint",
        "endpoint_pin",
        "ip",
        "ipv4",
        "ipv6",
        "license",
        "listener",
        "jwt",
        "name",
        "path",
        "directory",
        "hostname",
        "ssid",
        "username",
        "passwd",
        "password",
        "peer",
        "private_key",
        "proxy-authorization",
        "proxy_authorization",
        "proxy_password",
        "remote",
        "secret",
        "sni",
        "source",
        "token",
        "warp_secret",
        "zero_trust_callback",
    ]
    .iter()
    .any(|candidate| {
        normalized == *candidate
            || normalized.ends_with(&format!("_{candidate}"))
            || normalized.starts_with(&format!("{candidate}_"))
    })
}

fn scrub_network_tokens(input: &str) -> String {
    let normalized = input.to_ascii_lowercase();
    // An assignment may include quoted, whitespace-separated secret material.
    // Remove the whole free-text value instead of trying to guess its boundary.
    if [
        "password",
        "passwd",
        "token",
        "secret",
        "private_key",
        "authorization",
        "cookie",
        "assertion",
        "license",
        "username",
        "credential",
        "device_id",
        "certificate",
        "path",
        "directory",
    ]
    .iter()
    .any(|key| {
        normalized.match_indices(key).any(|(offset, _)| {
            normalized[offset + key.len()..]
                .trim_start_matches(|character: char| {
                    character.is_whitespace() || matches!(character, '"' | '\'')
                })
                .starts_with(['=', ':'])
        })
    }) {
        return "[REDACTED]".to_owned();
    }
    input
        .split_inclusive(char::is_whitespace)
        .map(|part| {
            let (token, whitespace) = part
                .strip_suffix(char::is_whitespace)
                .map_or((part, ""), |token| (token, &part[token.len()..]));
            let trimmed = token.trim_matches(|character: char| {
                matches!(character, '"' | '\'' | '(' | ')' | ',' | ';')
            });
            let identifier = trimmed.rsplit_once('=').map_or(trimmed, |(_, value)| value);
            let bare = identifier.trim_matches(['[', ']', '{', '}', ':', '.', '!']);
            // Parse the original first: trimming punctuation can otherwise
            // destroy a valid IPv6 address whose compressed suffix is `::`.
            if looks_like_network_identifier(identifier)
                || looks_like_network_identifier(identifier.trim_end_matches(':'))
                || looks_like_network_identifier(bare)
            {
                format!("[NETWORK_REDACTED]{whitespace}")
            } else if looks_like_file_path(identifier) || looks_like_file_path(bare) {
                format!("[PATH_REDACTED]{whitespace}")
            } else {
                part.to_owned()
            }
        })
        .collect()
}

fn looks_like_file_path(token: &str) -> bool {
    token.starts_with('/')
        || token.starts_with("./")
        || token.starts_with("../")
        || token.contains('\\')
        || token.contains(":/")
}

fn is_public_numeric_key(key: &str) -> bool {
    matches!(
        key,
        "sequence"
            | "elapsed_ms"
            | "duration_ms"
            | "received_frames"
            | "sent_frames"
            | "received_bytes"
            | "sent_bytes"
            | "attempt"
            | "attempts"
            | "attempt_limit"
            | "reconnect_count"
            | "fallback_count"
            | "listener_count"
            | "active_listener_count"
            | "endpoint_count"
            | "profiles"
            | "gate_driver_id"
            | "worker_pending"
            | "fatal"
            | "retryable"
            | "historical_terminal"
            | "ipv4_available"
            | "ipv6_available"
            | "endpoint_pin_valid"
            | "request_accepted"
            | "journal_generation"
            | "os_code"
            | "win32_code"
            | "panic_line"
            | "panic_column"
            | "queue_items"
            | "queue_bytes"
            | "drop_items"
            | "drop_bytes"
            | "queue_drops"
            | "network_generation"
            | "backlog"
            | "capacity"
            | "sent_packets"
            | "received_packets"
    ) || usque_core::diagnostics_contract_generated::EVIDENCE_KEYS.contains(&key)
}

/// Public bundles have a stronger boundary than local debug logs. Unknown text,
/// messages, errors, keys and nested objects have no path through this projection.
pub fn project_public_log(bytes: &[u8]) -> Vec<u8> {
    let Ok(Value::Object(input)) = serde_json::from_slice(bytes) else {
        return Vec::new();
    };
    let mut output = serde_json::Map::new();
    output.insert("schema_version".into(), Value::from(1));
    if let Some(level) = input
        .get("level")
        .and_then(Value::as_str)
        .filter(|level| matches!(*level, "ERROR" | "WARN" | "INFO" | "DEBUG" | "TRACE"))
    {
        output.insert("level".into(), Value::String(level.into()));
    }
    if let Some(timestamp) = input
        .get("timestamp")
        .and_then(Value::as_str)
        .and_then(|text| chrono::DateTime::parse_from_rfc3339(text).ok())
    {
        output.insert("timestamp".into(), Value::String(timestamp.to_rfc3339()));
    }
    if let Some(target) = input
        .get("target")
        .and_then(Value::as_str)
        .and_then(|target| target.split("::").next())
        .filter(|target| {
            matches!(
                *target,
                "usque_engine"
                    | "usque_transport"
                    | "usque_agent"
                    | "usque_android"
                    | "usque_openvpn"
            )
        })
    {
        output.insert("component".into(), Value::String(target.into()));
    }
    let project_fields = |fields: &serde_json::Map<String, Value>| {
        fields
            .iter()
            .filter_map(|(key, value)| {
                if (is_public_numeric_key(key)
                    && (value.is_boolean() || value.as_u64().is_some() || value.as_i64().is_some()))
                    || (key == "error" && value.is_boolean())
                {
                    return Some((key.clone(), value.clone()));
                }
                let text = value.as_str()?;
                let safe = match key.as_str() {
                    "gate_event" | "recovery_event" => LOG_EVENT_CODES.contains(&text),
                    "event_type" => {
                        usque_core::diagnostics_contract_generated::EVENT_TYPES.contains(&text)
                    }
                    "failure_code" | "error_code" => {
                        usque_core::diagnostics_contract_generated::FAILURE_CODES.contains(&text)
                            || LOG_EVENT_CODES.contains(&text)
                    }
                    "check_id" => {
                        usque_core::diagnostics_contract_generated::CHECK_IDS.contains(&text)
                    }
                    "io_error_kind" => matches!(
                        text,
                        "NotFound"
                            | "PermissionDenied"
                            | "ConnectionRefused"
                            | "ConnectionReset"
                            | "ConnectionAborted"
                            | "NotConnected"
                            | "AddrInUse"
                            | "AddrNotAvailable"
                            | "BrokenPipe"
                            | "AlreadyExists"
                            | "WouldBlock"
                            | "InvalidInput"
                            | "InvalidData"
                            | "TimedOut"
                            | "WriteZero"
                            | "Interrupted"
                            | "UnexpectedEof"
                            | "Unsupported"
                            | "OutOfMemory"
                            | "Other"
                    ),
                    "state" => matches!(
                        text,
                        "Pending" | "Running" | "Cancelling" | "Completed" | "Failed" | "Cancelled"
                    ),
                    "reason_code" => matches!(
                        text,
                        "direct_send_failed"
                            | "direct_resolution_failed"
                            | "direct_connect_failed"
                            | "timeout"
                            | "query_failed"
                    ),
                    "transport" => {
                        matches!(text, "h2" | "h3" | "Http2" | "Http3" | "HTTP/2" | "HTTP/3")
                    }
                    "phase" => matches!(
                        text,
                        "disconnected"
                            | "preparing"
                            | "connecting_h3"
                            | "connecting_h2"
                            | "connected"
                            | "degraded"
                            | "reconnecting"
                            | "disconnecting"
                            | "error"
                    ),
                    _ => false,
                };
                safe.then(|| (key.clone(), value.clone()))
            })
            .collect::<serde_json::Map<_, _>>()
    };
    let mut fields = project_fields(&input);
    if let Some(nested) = input.get("fields").and_then(Value::as_object) {
        fields.extend(project_fields(nested));
    }
    output.insert("fields".into(), Value::Object(fields));
    serde_json::to_vec(&Value::Object(output)).unwrap_or_default()
}

fn looks_like_network_identifier(token: &str) -> bool {
    if token.is_empty() {
        return false;
    }
    if token.parse::<IpAddr>().is_ok()
        || token.parse::<SocketAddr>().is_ok()
        || token.contains("://")
        || looks_like_jwt(token)
    {
        return true;
    }
    let authority = token
        .split(['/', '?', '#'])
        .next()
        .unwrap_or(token)
        .rsplit('@')
        .next()
        .unwrap_or(token);
    if authority.parse::<IpAddr>().is_ok() || authority.parse::<SocketAddr>().is_ok() {
        return true;
    }
    if let Some(bracketed) = authority
        .strip_prefix('[')
        .and_then(|value| value.split(']').next())
        && bracketed.parse::<IpAddr>().is_ok()
    {
        return true;
    }
    let host = authority
        .rsplit_once(':')
        .filter(|(host, port)| {
            !host.contains(':') && port.bytes().all(|byte| byte.is_ascii_digit())
        })
        .map_or(authority, |(host, _)| host)
        .trim_end_matches('.');
    host.eq_ignore_ascii_case("localhost")
        || host.contains('.')
            && !host.contains(['/', '\\'])
            && host.split('.').all(|label| {
                !label.is_empty()
                    && label.len() <= 63
                    && label
                        .chars()
                        .all(|character| character.is_ascii_alphanumeric() || character == '-')
            })
}

fn looks_like_jwt(token: &str) -> bool {
    let mut segments = token.split('.');
    let parts = [segments.next(), segments.next(), segments.next()];
    segments.next().is_none()
        && token.len() >= 32
        && parts.into_iter().all(|part| {
            part.is_some_and(|part| {
                !part.is_empty()
                    && part
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record_sequence(factory: &LogWriterFactory, sequence: u64) {
        let bytes = serde_json::to_vec(&serde_json::json!({"level": "INFO", "sequence": sequence}))
            .unwrap();
        factory.make_writer().write_all(&bytes).unwrap();
    }

    fn block_writer(factory: &LogWriterFactory) -> mpsc::Sender<()> {
        let (entered, waiting) = mpsc::channel();
        let (release, blocked) = mpsc::channel();
        assert!(
            factory
                .shared
                .sender
                .try_send(LogCommand::Operation(Box::new(move |_| {
                    let _ = entered.send(());
                    let _ = blocked.recv();
                })))
                .is_ok()
        );
        waiting.recv_timeout(Duration::from_secs(5)).unwrap();
        release
    }

    #[test]
    fn queue_overflow_is_bounded_and_flush_has_a_timeout() {
        let directory = tempfile::tempdir().unwrap();
        let factory = LogWriterFactory::open(&directory.path().join("config.json")).unwrap();
        let release = block_writer(&factory);
        for sequence in 0..(MAX_QUEUED_EVENTS + 17) {
            record_sequence(&factory, sequence as u64);
        }
        let health = factory.health();
        assert_eq!(health.queued_events, MAX_QUEUED_EVENTS);
        assert!(health.queued_bytes <= MAX_QUEUED_BYTES);
        assert_eq!(health.queue_full_events, 17);
        assert_eq!(health.dropped_events, 17);
        assert_eq!(
            factory.flush(Duration::ZERO).unwrap_err().kind(),
            io::ErrorKind::TimedOut
        );
        release.send(()).unwrap();
        factory.flush(Duration::from_secs(5)).unwrap();
        assert_eq!(factory.health().written_events, MAX_QUEUED_EVENTS as u64);
        assert_eq!(factory.health().queued_bytes, 0);
        factory.shutdown(Duration::from_secs(5)).unwrap();
        record_sequence(&factory, 999);
        assert_eq!(factory.health().dropped_events, 18);
    }

    #[test]
    fn queue_limits_total_payload_bytes_independently_of_event_count() {
        let directory = tempfile::tempdir().unwrap();
        let factory = LogWriterFactory::open(&directory.path().join("config.json")).unwrap();
        let release = block_writer(&factory);
        let bytes =
            serde_json::to_vec(&serde_json::json!({"padding": "x".repeat(MAX_EVENT_BYTES - 64)}))
                .unwrap();
        for _ in 0..9 {
            factory.make_writer().write_all(&bytes).unwrap();
        }
        assert!(factory.health().queued_bytes <= MAX_QUEUED_BYTES);
        assert!(factory.health().queued_events < MAX_QUEUED_EVENTS);
        assert!(factory.health().queue_full_events > 0);
        release.send(()).unwrap();
        factory.shutdown(Duration::from_secs(5)).unwrap();
    }

    #[test]
    fn disk_failure_degrades_startup_and_recovers_without_blocking_producers() {
        let directory = tempfile::tempdir().unwrap();
        let config = directory.path().join("config.json");
        let logs = log_directory(&config);
        fs::write(&logs, b"inert path blocker").unwrap();
        let factory = LogWriterFactory::open(&config).unwrap();
        record_sequence(&factory, 1);
        assert!(factory.flush(Duration::from_secs(5)).is_err());
        let health = factory.health();
        assert!(!health.writer_available);
        assert!(health.write_failures > 0);
        assert_eq!(health.dropped_events, 1);
        assert_eq!(health.written_events, 0);
        fs::remove_file(&logs).unwrap();
        factory.flush(Duration::from_secs(5)).unwrap();
        record_sequence(&factory, 2);
        factory.shutdown(Duration::from_secs(5)).unwrap();
        assert_eq!(factory.health().written_events, 1);
    }

    #[test]
    fn failed_flush_marks_buffered_records_unconfirmed_instead_of_synced() {
        let directory = tempfile::tempdir().unwrap();
        let factory = LogWriterFactory::open(&directory.path().join("config.json")).unwrap();
        owner_operation(&factory.shared, Duration::from_secs(5), |state| {
            state.sync()?;
            state.file = Some(BufWriter::new(File::open(&state.active_path)?));
            Ok(())
        })
        .unwrap();
        record_sequence(&factory, 1);
        assert!(factory.flush(Duration::from_secs(5)).is_err());
        let health = factory.health();
        assert_eq!(health.written_events, 0);
        assert_eq!(health.unconfirmed_events, 1);
        assert_eq!(health.queued_bytes, 0);
        assert!(!health.writer_available);
        factory.flush(Duration::from_secs(5)).unwrap();
        factory.shutdown(Duration::from_secs(5)).unwrap();
    }

    #[test]
    fn clearing_is_ordered_with_writes_and_resets_rotation_length() {
        let directory = tempfile::tempdir().unwrap();
        let config = directory.path().join("config.json");
        let logs = log_directory(&config);
        fs::create_dir(&logs).unwrap();
        File::create(logs.join(ACTIVE_LOG_NAME))
            .unwrap()
            .set_len(ROTATE_BYTES - 4)
            .unwrap();
        let factory = LogWriterFactory::open(&config).unwrap();
        record_sequence(&factory, 1);
        clear_logs(&logs, Duration::from_secs(5)).unwrap();
        record_sequence(&factory, 2);
        factory.shutdown(Duration::from_secs(5)).unwrap();
        assert_eq!(fs::read_dir(&logs).unwrap().count(), 1);
        let value: Value =
            serde_json::from_slice(&fs::read(logs.join(ACTIVE_LOG_NAME)).unwrap()).unwrap();
        assert_eq!(value["sequence"], 2);
    }

    #[test]
    fn clearing_rejects_an_old_formatter_that_finishes_after_clear() {
        let directory = tempfile::tempdir().unwrap();
        let config = directory.path().join("config.json");
        let factory = LogWriterFactory::open(&config).unwrap();
        let mut stale = factory.make_writer();
        stale.write_all(br#"{"sequence":1}"#).unwrap();
        clear_logs(&log_directory(&config), Duration::from_secs(5)).unwrap();
        drop(stale);
        record_sequence(&factory, 2);
        factory.shutdown(Duration::from_secs(5)).unwrap();
        let value: Value = serde_json::from_slice(
            &fs::read(log_directory(&config).join(ACTIVE_LOG_NAME)).unwrap(),
        )
        .unwrap();
        assert_eq!(value["sequence"], 2);
        assert_eq!(factory.health().clear_epoch, 1);
        assert_eq!(factory.health().dropped_events, 1);
        assert_eq!(factory.health().written_events, 1);
    }

    #[test]
    fn startup_rotates_and_prunes_an_expired_quiet_active_file() {
        let directory = tempfile::tempdir().unwrap();
        let config = directory.path().join("config.json");
        let logs = log_directory(&config);
        fs::create_dir(&logs).unwrap();
        let active = logs.join(ACTIVE_LOG_NAME);
        fs::write(&active, b"{\"sequence\":1}\n").unwrap();
        let old = SystemTime::now() - MAX_AGE - Duration::from_secs(60);
        File::options()
            .write(true)
            .open(&active)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(old))
            .unwrap();
        let factory = LogWriterFactory::open(&config).unwrap();
        factory.flush(Duration::from_secs(5)).unwrap();
        assert!(fs::read(&active).unwrap().is_empty());
        assert_eq!(fs::read_dir(&logs).unwrap().count(), 1);
        factory.shutdown(Duration::from_secs(5)).unwrap();
    }

    #[test]
    fn quiet_segments_rotate_by_age_while_fresh_archives_are_retained() {
        let directory = tempfile::tempdir().unwrap();
        let config = directory.path().join("config.json");
        let factory = LogWriterFactory::open(&config).unwrap();
        record_sequence(&factory, 1);
        owner_operation(&factory.shared, Duration::from_secs(5), |state| {
            let now = SystemTime::now();
            state.active_started = now - MAX_ACTIVE_SEGMENT_AGE;
            state.rotate_aged_segment(now)
        })
        .unwrap();
        record_sequence(&factory, 2);
        factory.shutdown(Duration::from_secs(5)).unwrap();
        let logs = log_directory(&config);
        let paths = fs::read_dir(&logs)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect::<Vec<_>>();
        assert_eq!(paths.len(), 2);
        let archived = paths
            .iter()
            .find(|path| path.file_name().unwrap() != ACTIVE_LOG_NAME)
            .unwrap();
        let prior: Value = serde_json::from_slice(&fs::read(archived).unwrap()).unwrap();
        let current: Value =
            serde_json::from_slice(&fs::read(logs.join(ACTIVE_LOG_NAME)).unwrap()).unwrap();
        assert_eq!(prior["sequence"], 1);
        assert_eq!(current["sequence"], 2);
    }

    #[test]
    fn owner_rejects_old_epoch_records_already_queued_behind_clear() {
        let directory = tempfile::tempdir().unwrap();
        let config = directory.path().join("config.json");
        let factory = LogWriterFactory::open(&config).unwrap();
        let release = block_writer(&factory);
        let mut stale = factory.make_writer();
        stale.write_all(br#"{"sequence":1}"#).unwrap();
        let (cleared, result) = mpsc::channel();
        assert!(
            factory
                .shared
                .sender
                .try_send(LogCommand::Operation(Box::new(move |state| {
                    let _ = cleared.send(state.clear());
                })))
                .is_ok()
        );
        // Clear is queued but cannot run yet: the producer's epoch check still
        // succeeds, so only the owner's second check can reject this record.
        drop(stale);
        release.send(()).unwrap();
        result
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap();
        factory.flush(Duration::from_secs(5)).unwrap();
        assert!(
            fs::read(log_directory(&config).join(ACTIVE_LOG_NAME))
                .unwrap()
                .is_empty()
        );
        assert_eq!(factory.health().dropped_events, 1);
        assert_eq!(factory.health().queued_bytes, 0);
        factory.shutdown(Duration::from_secs(5)).unwrap();
    }

    #[test]
    fn dropping_last_handle_stops_worker_without_a_sender_reference_cycle() {
        let directory = tempfile::tempdir().unwrap();
        let factory = LogWriterFactory::open(&directory.path().join("config.json")).unwrap();
        let health = Arc::clone(&factory.shared.health);
        record_sequence(&factory, 1);
        drop(factory);
        let stopped = health.stopped.lock().unwrap();
        let (stopped, _) = health
            .completed
            .wait_timeout_while(stopped, Duration::from_secs(5), |stopped| !*stopped)
            .unwrap();
        assert!(*stopped);
        assert!(!health.running.load(Ordering::Acquire));
        assert_eq!(health.snapshot().written_events, 1);
    }

    #[test]
    fn assignments_punctuation_and_paths_are_scrubbed_without_losing_typed_counts() {
        for message in [
            "password=super-secret with spaces",
            "password = \"super-secret\"",
            "username = super-secret",
            "Authorization: Bearer super-secret",
            "203.0.113.1:443: timeout",
            "endpoint=203.0.113.1:443",
            r"C:\Users\private\config.json",
            "/Users/private/config.json",
        ] {
            let input = serde_json::to_vec(&serde_json::json!({"message": message, "listener_count": 3, "ipv4_available": true, "endpoint_pin_valid": true})).unwrap();
            let sanitized = sanitize_log_bytes(&input);
            let text = String::from_utf8(sanitized.clone()).unwrap();
            for secret in ["super-secret", "203.0.113.1", "private"] {
                assert!(!text.contains(secret), "retained private fixture in {text}");
            }
            let value: Value = serde_json::from_slice(&sanitized).unwrap();
            assert_eq!(value["listener_count"], 3);
            assert_eq!(value["ipv4_available"], true);
            assert_eq!(value["endpoint_pin_valid"], true);
        }
    }

    #[test]
    fn bracketed_identifiers_and_unicode_assignment_whitespace_are_scrubbed() {
        for message in [
            "[example.com]",
            "endpoint=[203.0.113.1:443]",
            "{203.0.113.1}!",
            "[2001:db8::]",
            "2001:db8::",
            "::",
            "[/Users/private/config.json]",
            "password\u{2003}=\u{a0}super-secret",
            "password\r\n=\tsuper-secret",
            "authorization\u{202f}:\u{2003}super-secret",
        ] {
            let bytes =
                serde_json::to_vec(&serde_json::json!({"message": message, "listener_count": 3}))
                    .unwrap();
            let sanitized: Value = serde_json::from_slice(&sanitize_log_bytes(&bytes)).unwrap();
            let text = sanitized["message"].as_str().unwrap();
            assert!(text.contains("REDACTED"), "unfiltered fixture: {message}");
            for private in [
                "example.com",
                "203.0.113.1",
                "2001:db8",
                "private",
                "super-secret",
            ] {
                assert!(!text.contains(private));
            }
            assert_eq!(sanitized["listener_count"], 3);
        }
    }

    #[test]
    fn public_projection_excludes_unknown_text_and_preserves_typed_failure_evidence() {
        let input = serde_json::json!({
            "level": "WARN", "target": "usque_transport::vpngate",
            "message": "a raw secret", "arbitrary_secret_key": "another secret",
            "fields": {"gate_event": "TCP_READ_FAILED", "io_error_kind": "UnexpectedEof", "sent_frames": 1,
                "listener_count": 3, "ipv4_available": true, "error": "token=secret", "remote": "203.0.113.1:443",
                "sequence": "private", "state": "private", "elapsed_ms": {"secret": "private"}}
        });
        let projected = project_public_log(&serde_json::to_vec(&input).unwrap());
        let text = String::from_utf8(projected.clone()).unwrap();
        for private in ["secret", "private", "203.0.113.1", "arbitrary_secret_key"] {
            assert!(!text.contains(private));
        }
        let value: Value = serde_json::from_slice(&projected).unwrap();
        assert_eq!(value["component"], "usque_transport");
        assert_eq!(value["fields"]["gate_event"], "TCP_READ_FAILED");
        assert_eq!(value["fields"]["io_error_kind"], "UnexpectedEof");
        assert_eq!(value["fields"]["sent_frames"], 1);
        assert_eq!(value["fields"]["listener_count"], 3);
        assert_eq!(value["fields"]["ipv4_available"], true);
        assert!(project_public_log(br#"["secret"]"#).is_empty());
        let projected = project_public_log(br#"{"fields":{"gate_event":"AUTH_FAILED","error":true,"error_code":"AGENT_DIRECT_EGRESS_FAILED"}}"#);
        let value: Value = serde_json::from_slice(&projected).unwrap();
        assert_eq!(value["fields"]["gate_event"], "AUTH_FAILED");
        assert_eq!(value["fields"]["error"], true);
        assert_eq!(value["fields"]["error_code"], "AGENT_DIRECT_EGRESS_FAILED");
    }

    #[test]
    fn json_log_redaction_removes_secrets_and_network_identifiers() {
        let sanitized = sanitize_log_bytes(
            br#"{"level":"WARN","peer":"192.0.2.1:443","warp_secret":"secret","message":"failed to reach example.com at 2001:db8::1"}"#,
        );
        let text = String::from_utf8(sanitized).unwrap();
        assert!(!text.contains("192.0.2.1"));
        assert!(!text.contains(r#""warp_secret":"secret""#));
        assert!(!text.contains("example.com"));
        assert!(!text.contains("2001:db8"));
        assert!(text.contains("[REDACTED]"));
        assert!(text.contains("[NETWORK_REDACTED]"));
    }

    #[test]
    fn proxy_passwords_are_redacted_from_logs() {
        let sanitized = sanitize_log_bytes(
            br#"{"password":"listener-secret","proxy_password":"vault-secret","Proxy-Authorization":"Basic dXNlcjpwYXNz"}"#,
        );
        let text = String::from_utf8(sanitized).unwrap();
        for secret in ["listener-secret", "vault-secret", "Basic dXNlcjpwYXNz"] {
            assert!(!text.contains(secret), "log retained a sensitive fixture");
        }
        assert!(text.contains("[REDACTED]"));
    }

    #[test]
    fn gate_stage_diagnostics_survive_export_without_endpoint_or_credentials() {
        let sanitized = sanitize_log_bytes(
            br#"{"fields":{"gate_event":"TCP_READ_FAILED","io_error_kind":"UnexpectedEof","received_frames":0,"sent_frames":1,"remote":"203.0.113.1:443","private_key":"fixture-secret"}}"#,
        );
        let value: Value = serde_json::from_slice(&sanitized).unwrap();
        assert_eq!(value["fields"]["gate_event"], "TCP_READ_FAILED");
        assert_eq!(value["fields"]["io_error_kind"], "UnexpectedEof");
        assert_eq!(value["fields"]["sent_frames"], 1);
        assert_eq!(value["fields"]["remote"], "[REDACTED]");
        assert_eq!(value["fields"]["private_key"], "[REDACTED]");
    }

    #[test]
    fn hostname_ports_paths_and_bracketed_ipv6_are_redacted() {
        let sanitized = sanitize_log_bytes(
            br#"{"message":"example.com:443 example.net/path user@private.example:8443 [2001:db8::5]:443/path localhost"}"#,
        );
        let text = String::from_utf8(sanitized).unwrap();
        for private in [
            "example.com",
            "example.net",
            "private.example",
            "2001:db8",
            "localhost",
        ] {
            assert!(!text.contains(private), "log retained {private}");
        }
        assert_eq!(text.matches("[NETWORK_REDACTED]").count(), 5);
    }

    #[test]
    fn zero_trust_headers_callbacks_and_jwts_are_always_redacted() {
        let sanitized = sanitize_log_bytes(
            br#"{"CF-Access-Jwt-Assertion":"header-secret","jwt":"jwt-secret","assertion":"assertion-secret","zero_trust_callback":"callback-secret","message":"callback com.cloudflare.warp://example.cloudflareaccess.com/auth?token=secret JWT eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJ1c2VyIn0.signature"}"#,
        );
        let text = String::from_utf8(sanitized).unwrap();
        for secret in [
            "header-secret",
            "jwt-secret",
            "assertion-secret",
            "callback-secret",
            "com.cloudflare.warp",
            "eyJhbGciOiJIUzI1NiJ9",
        ] {
            assert!(!text.contains(secret), "log retained a sensitive fixture");
        }
    }

    #[test]
    fn writer_creates_a_bounded_jsonl_file() {
        let directory = tempfile::tempdir().unwrap();
        let config = directory.path().join("config.json");
        let factory = LogWriterFactory::open(&config).unwrap();
        {
            let mut writer = factory.make_writer();
            writer
                .write_all(br#"{"level":"INFO","message":"ready"}"#)
                .unwrap();
        }
        factory.flush(Duration::from_secs(5)).unwrap();
        let contents = fs::read_to_string(log_directory(&config).join(ACTIVE_LOG_NAME)).unwrap();
        assert!(contents.ends_with('\n'));
        assert!(contents.contains("\"ready\""));
        factory.shutdown(Duration::from_secs(5)).unwrap();
    }
}
