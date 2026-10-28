use std::{path::PathBuf, sync::Arc, time::Duration};

use clap::Parser;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use usque_core::{LogLevel, storage::ConfigStore};
use usque_engine::ControlService;

#[derive(Debug, Parser)]
#[command(name = "usque-engine", hide = true)]
struct Arguments {
    /// Non-secret, versioned configuration supplied by the native host.
    #[arg(long)]
    config: PathBuf,
    /// Validate configuration and exit. Intended for CI and installers.
    #[arg(long)]
    validate_only: bool,
    /// Permanently remove current-user profiles, preferences, logs, caches,
    /// and namespaced Windows Credential Manager records during MSI uninstall.
    #[cfg(windows)]
    #[arg(
        long,
        conflicts_with = "validate_only",
        requires = "preferences_directory"
    )]
    purge_user_data: bool,
    /// Exact SharedPreferences directory supplied by the MSI uninstall action.
    #[cfg(windows)]
    #[arg(long, requires = "purge_user_data")]
    preferences_directory: Option<PathBuf>,
    /// Override the per-user Windows Named Pipe name (development only).
    #[cfg(windows)]
    #[arg(long, hide = true)]
    pipe: Option<String>,
    /// Exit when the desktop UI process exits (development and sidecar use).
    #[cfg(any(windows, target_os = "linux"))]
    #[arg(long, hide = true)]
    parent_pid: Option<u32>,
    /// Override the current-user desktop Unix Socket path (development only).
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[arg(long, hide = true)]
    socket: Option<PathBuf>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(target_os = "linux")]
    {
        // The privileged Agent authenticates executable mappings and rejects
        // processes which permit same-user ptrace injection. Set this before
        // starting any worker or accessing credentials.
        // SAFETY: PR_SET_DUMPABLE takes one integer flag and no pointers.
        if unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0) } != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
    }
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let mut log_shutdown = None;
    let result = runtime.block_on(run(&mut log_shutdown));
    // run() finishes privileged cleanup (or records its failure) first. A
    // pending in-memory native worker must not hold process exit indefinitely.
    finish_runtime(runtime, std::time::Duration::from_secs(5));
    drop(log_shutdown);
    result
}

fn finish_runtime(runtime: tokio::runtime::Runtime, grace: std::time::Duration) {
    let started = std::time::Instant::now();
    info!(
        recovery_event = "ENGINE_RUNTIME_WAIT_STARTED",
        "Waiting for Engine runtime shutdown"
    );
    // This bounds waiting; it does not abort native threads or free their Arc-
    // owned memory. Any remaining threads end when this process exits.
    runtime.shutdown_timeout(grace);
    info!(
        recovery_event = "ENGINE_RUNTIME_WAIT_FINISHED",
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Engine runtime shutdown wait finished"
    );
}

async fn run(
    log_shutdown: &mut Option<LogShutdownGuard>,
) -> Result<(), Box<dyn std::error::Error>> {
    let arguments = Arguments::parse();
    let config_path = arguments.config.clone();

    #[cfg(windows)]
    if arguments.purge_user_data {
        let preferences_directory = arguments
            .preferences_directory
            .as_deref()
            .ok_or("--preferences-directory is required with --purge-user-data")?;
        usque_engine::windows_purge::purge_current_user_data(&config_path, preferences_directory)?;
        return Ok(());
    }

    let store = ConfigStore::new(config_path.clone());
    if arguments.validate_only {
        store.load_or_default()?.validate()?;
        return Ok(());
    }
    #[cfg(target_os = "linux")]
    {
        usque_platform::linux_paths::prepare_config_file(&config_path)?;
        if let Some(paths) = usque_platform::linux_paths::LinuxPaths::for_config(&config_path) {
            for directory in [&paths.cache, &paths.state, &paths.data] {
                usque_platform::linux_paths::private_directory(directory)?;
            }
        }
    }
    // Bootstrap diagnostics before reading user configuration. Validate-only
    // stays side-effect free, and invalid configuration still has a local cause.
    let log_writer = usque_engine::logging::LogWriterFactory::open(&config_path)?;
    *log_shutdown = Some(LogShutdownGuard(log_writer.clone()));
    let (filter, filter_handle) = tracing_subscriber::reload::Layer::new(
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
    );
    tracing_subscriber::registry()
        .with(filter)
        .with(
            tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_writer(log_writer.clone())
                .json(),
        )
        .init();
    install_panic_logging(log_writer);
    let config = match store.load_or_default() {
        Ok(config) => config,
        Err(error) => {
            tracing::error!(
                error_code = "ENGINE_CONFIGURATION_LOAD_FAILED",
                "Engine configuration could not be read"
            );
            return Err(error.into());
        }
    };
    if let Err(error) = config.validate() {
        tracing::error!(
            error_code = "ENGINE_CONFIGURATION_INVALID",
            "Engine configuration failed validation"
        );
        return Err(error.into());
    }
    let default_filter = match config.preferences.log_level {
        LogLevel::Error => "error",
        LogLevel::Warn => "warn",
        LogLevel::Info => "info",
        LogLevel::Debug => "debug",
    };
    if filter_handle
        .reload(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_filter)),
        )
        .is_err()
    {
        tracing::warn!(
            error_code = "ENGINE_LOG_FILTER_RELOAD_FAILED",
            "Engine log filter could not be updated"
        );
    }

    #[cfg(target_os = "linux")]
    let linux_server = {
        let socket = match arguments.socket {
            Some(socket) => socket,
            None => usque_platform::linux_paths::LinuxPaths::from_environment()?.control_socket(),
        };
        usque_engine::linux_ipc::LinuxIpcServer::bind(socket).await?
    };
    let service = ControlService::open(store)?;
    #[cfg(target_os = "macos")]
    if let Ok(provider) = usque_engine::macos_runtime::BundledProviderControl::from_current_bundle()
    {
        service
            .set_macos_provider_control(Arc::new(provider))
            .await?;
    }

    #[cfg(not(target_os = "linux"))]
    if let Err(error) = service.migrate_shared_proxy_password().await {
        warn!(%error, "shared proxy-password migration will be retried later");
    }
    #[cfg(not(target_os = "linux"))]
    if let Err(error) = service.reap_pending_identity_deletions().await {
        warn!(%error, "deferred secure identity cleanup will be retried later");
    }
    let config = service.config_snapshot().await;
    info!(
        profiles = config.profiles.len(),
        active_profile = ?config.active_profile_id,
        "Usque control service initialized"
    );

    #[cfg(windows)]
    {
        let parent_pid = arguments.parent_pid;
        let pipe_name = arguments
            .pipe
            .unwrap_or(usque_engine::windows_ipc::current_user_pipe_name()?);
        let event_pipe_name = usque_engine::windows_ipc::event_pipe_name(&pipe_name)?;
        info!(%pipe_name, "starting current-user Named Pipe control service");
        info!(%event_pipe_name, "starting current-user Named Pipe event service");
        let service = Arc::new(service);
        let recovery_monitor = tokio::spawn(Arc::clone(&service).run_windows_recovery_monitor());
        tokio::select! {
            result = usque_engine::windows_ipc::serve(Arc::clone(&service), pipe_name) => result?,
            result = usque_engine::windows_ipc::serve_events(
                Arc::clone(&service),
                event_pipe_name,
            ) => result?,
            result = tokio::signal::ctrl_c() => result?,
            result = wait_for_parent_exit(parent_pid) => {
                result?;
                info!("desktop UI process exited");
            },
        }
        if let Err(error) = service.shutdown().await {
            warn!(%error, "engine shutdown could not fully restore platform state");
        }
        service.stop_windows_recovery_monitor();
        if let Err(error) = recovery_monitor.await {
            warn!(%error, "Windows recovery monitor stopped unexpectedly");
        }
    }

    #[cfg(target_os = "macos")]
    {
        let socket_path = arguments.socket.unwrap_or_else(|| {
            config_path
                .parent()
                .unwrap_or_else(|| std::path::Path::new("."))
                .join("engine-v1.sock")
        });
        info!(path = %socket_path.display(), "starting current-user Unix Socket control service");
        let service = Arc::new(service);
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        let result = tokio::select! {
            result = usque_engine::macos_ipc::serve(Arc::clone(&service), socket_path) => result,
            result = tokio::signal::ctrl_c() => result,
            _ = terminate.recv() => Ok(()),
        };
        // UI/Engine process lifetime is distinct from explicit user Stop. The
        // extension retains its session and protection after losing the UI.
        if let Err(error) = service.detach_macos_process().await {
            warn!(%error, "engine local frontend detach failed");
        }
        result?;
    }

    #[cfg(target_os = "linux")]
    {
        let service = Arc::new(service);
        // A locked Secret Service can wait for an interactive unlock. Keep
        // authenticated IPC available while serialized cleanup is waiting.
        let startup = tokio_util::task::AbortOnDropHandle::new(tokio::spawn({
            let service = Arc::clone(&service);
            async move { service.initialize_linux_user_state().await }
        }));
        let observations = tokio_util::task::AbortOnDropHandle::new(tokio::spawn(
            Arc::clone(&service).run_linux_observation_monitor(),
        ));
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        let result = tokio::select! {
            result = linux_server.serve(Arc::clone(&service)) => result,
            result = tokio::signal::ctrl_c() => result,
            _ = terminate.recv() => Ok(()),
            result = usque_engine::linux_ipc::wait_for_parent_exit(arguments.parent_pid) => result,
        };
        drop(startup);
        drop(observations);
        // A failed listener or parent monitor must still finish frontend and
        // platform cleanup before propagating its error to the launcher.
        if let Err(error) = service.shutdown().await {
            warn!(%error, "engine shutdown could not fully restore platform state");
        }
        result?;
    }

    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    {
        let service = Arc::new(service);
        info!("this platform has no engine IPC transport");
        tokio::signal::ctrl_c().await?;
        if let Err(error) = service.shutdown().await {
            warn!(%error, "engine shutdown could not fully restore platform state");
        }
    }
    info!("shutdown requested");
    Ok(())
}

/// Preserve the source of release-profile aborts in the same bounded,
/// privacy-filtered log as the rest of the Engine diagnostics. The default
/// hook is retained so an attached debugger or stderr collector still sees
/// Rust's normal panic report.
fn install_panic_logging(log_writer: usque_engine::logging::LogWriterFactory) {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let location = info.location();
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| info.payload().downcast_ref::<String>().map(String::as_str))
            .unwrap_or("non-string panic payload");
        tracing::error!(
            panic_message = message,
            panic_file = location.map(|location| location.file()),
            panic_line = location.map(|location| location.line()),
            panic_column = location.map(|location| location.column()),
            "unhandled Engine panic"
        );
        if std::thread::current().name() != Some("usque-log-writer") {
            let _ = log_writer.flush(Duration::from_millis(250));
        }
        default_hook(info);
    }));
}

struct LogShutdownGuard(usque_engine::logging::LogWriterFactory);

impl Drop for LogShutdownGuard {
    fn drop(&mut self) {
        // Cover ordinary and early-error exits without making diagnostic I/O
        // an unbounded prerequisite for platform shutdown.
        let _ = self.0.shutdown(Duration::from_secs(2));
    }
}

#[cfg(windows)]
async fn wait_for_parent_exit(parent_pid: Option<u32>) -> std::io::Result<()> {
    let Some(parent_pid) = parent_pid else {
        std::future::pending::<()>().await;
        unreachable!();
    };
    let cancellation = tokio_util::sync::CancellationToken::new();
    let _guard = cancellation.clone().drop_guard();
    tokio::task::spawn_blocking(move || {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{
            OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject,
        };

        // SAFETY: OpenProcess may be called with any PID; on success the HANDLE
        // is exclusively owned here until CloseHandle (null is checked next).
        let process = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, parent_pid) };
        if process.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        let result = wait_for_parent_signal(&cancellation, || {
            // SAFETY: process is a live handle owned by this task. A bounded
            // wait lets cancellation release the handle when another exit
            // trigger wins the select in run().
            unsafe { WaitForSingleObject(process, 100) }
        });
        // SAFETY: process is still owned here and is closed exactly once.
        unsafe {
            CloseHandle(process);
        }
        result
    })
    .await
    .map_err(|error| std::io::Error::other(error.to_string()))?
}

#[cfg(windows)]
fn wait_for_parent_signal(
    cancellation: &tokio_util::sync::CancellationToken,
    mut wait: impl FnMut() -> u32,
) -> std::io::Result<()> {
    use windows_sys::Win32::Foundation::{WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT};
    while !cancellation.is_cancelled() {
        match wait() {
            WAIT_OBJECT_0 => return Ok(()),
            WAIT_TIMEOUT => {}
            WAIT_FAILED => return Err(std::io::Error::last_os_error()),
            status => {
                return Err(std::io::Error::other(format!(
                    "unexpected process wait status {status}"
                )));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_shutdown_returns_with_a_pending_worker_without_destroying_its_state() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let (release, waiting) = std::sync::mpsc::channel();
        let (started, entered) = std::sync::mpsc::channel();
        let (exited, finished) = std::sync::mpsc::channel();
        let state = Arc::new(());
        let worker_state = Arc::clone(&state);
        runtime.spawn_blocking(move || {
            let _ = started.send(());
            let _ = waiting.recv();
            drop(worker_state);
            let _ = exited.send(());
        });
        entered
            .recv_timeout(std::time::Duration::from_secs(3))
            .unwrap();
        finish_runtime(runtime, std::time::Duration::ZERO);
        assert_eq!(Arc::strong_count(&state), 2);
        release.send(()).unwrap();
        finished
            .recv_timeout(std::time::Duration::from_secs(3))
            .unwrap();
        assert_eq!(Arc::strong_count(&state), 1);
    }

    #[cfg(windows)]
    #[test]
    fn cancelled_parent_monitor_does_not_wait_for_the_parent_to_exit() {
        use windows_sys::Win32::Foundation::WAIT_TIMEOUT;
        let cancel = tokio_util::sync::CancellationToken::new();
        let mut waits = 0;
        wait_for_parent_signal(&cancel, || {
            waits += 1;
            cancel.cancel();
            WAIT_TIMEOUT
        })
        .unwrap();
        assert_eq!(waits, 1);
        wait_for_parent_signal(&cancel, || {
            panic!("already cancelled monitor must not wait")
        })
        .unwrap();
    }
}
