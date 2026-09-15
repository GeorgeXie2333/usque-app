//! One event-driven reader of an already-running Agent. Never starts services.
use super::PlatformState;
use std::{
    path::{Path, PathBuf},
    sync::OnceLock,
    time::Duration,
};
use tokio::{sync::mpsc, task::JoinHandle};
use tokio_util::sync::CancellationToken;

static REQUESTS: OnceLock<mpsc::Sender<()>> = OnceLock::new();

pub struct Recorder {
    stop: CancellationToken,
    task: JoinHandle<()>,
}

impl Recorder {
    pub fn start(config_path: &Path) -> Self {
        let (sender, receiver) = mpsc::channel(1);
        let _ = REQUESTS.set(sender.clone());
        let stop = CancellationToken::new();
        let task = tokio::spawn(run(
            crate::logging::log_directory(config_path),
            receiver,
            stop.clone(),
            super::capture,
        ));
        // Also recover evidence from a still-running Agent after an Engine restart.
        let _ = sender.try_send(());
        Self { stop, task }
    }

    pub async fn finish(mut self) {
        self.stop.cancel();
        // Includes any current read plus a final read after disconnect cleanup.
        // Evidence failure cannot prevent the Engine from exiting.
        if tokio::time::timeout(Duration::from_secs(5), &mut self.task)
            .await
            .is_err()
        {
            self.task.abort();
        }
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

pub(crate) fn request() {
    if let Some(sender) = REQUESTS.get() {
        let _ = sender.try_send(());
    }
}

pub(crate) struct Boundary(pub bool);
impl Drop for Boundary {
    fn drop(&mut self) {
        if self.0 {
            request();
        }
    }
}

async fn run<F, Fut>(
    directory: PathBuf,
    mut requests: mpsc::Receiver<()>,
    stop: CancellationToken,
    mut sample: F,
) where
    F: FnMut() -> Fut,
    Fut: Future<Output = PlatformState>,
{
    loop {
        tokio::select! {
            biased;
            () = stop.cancelled() => {
                save(&directory, sample().await).await;
                return;
            }
            request = requests.recv() => { if request.is_none() { return; } }
        }
        // Coalesce lifecycle bursts and let the Agent's nonblocking trace writer
        // flush. No periodic reader remains active while the application is idle.
        tokio::select! {
            () = stop.cancelled() => continue,
            () = tokio::time::sleep(Duration::from_millis(250)) => {}
        }
        while requests.try_recv().is_ok() {}
        save(&directory, sample().await).await;
        // A single follow-up preserves late trace writes, including final Clean.
        tokio::select! {
            () = stop.cancelled() => continue,
            () = tokio::time::sleep(Duration::from_millis(750)) => {}
        }
        save(&directory, sample().await).await;
    }
}

async fn save(directory: &Path, state: PlatformState) {
    let directory = directory.to_owned();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis().min(u128::from(u64::MAX)) as u64);
    // Cache writes, like reads, are independent of all cleanup/connection gates.
    let result = write_evidence(move || super::cache::store(&directory, &state, now)).await;
    if result.is_err() {
        tracing::warn!("Windows recovery evidence cache could not be written");
    }
}

async fn write_evidence(
    write: impl FnOnce() -> std::io::Result<bool> + Send + 'static,
) -> std::io::Result<bool> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    // Exactly one write is awaited by the recorder. Unlike spawn_blocking,
    // this I/O-only worker cannot pin Tokio runtime shutdown after the five-
    // second evidence budget. It owns no network resources or cleanup work.
    std::thread::Builder::new()
        .name("usque-evidence-cache".into())
        .spawn(move || {
            let _ = sender.send(write());
        })?;
    receiver
        .await
        .map_err(|_| std::io::Error::other("evidence writer unavailable"))?
}

#[cfg(test)]
mod tests;
