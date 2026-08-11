use std::{path::PathBuf, sync::Arc};

use clap::Parser;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;
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
    #[cfg(windows)]
    #[arg(long, hide = true)]
    parent_pid: Option<u32>,
    /// Override the current-user macOS Unix Socket path (development only).
    #[cfg(target_os = "macos")]
    #[arg(long, hide = true)]
    socket: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
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
    let config = store.load_or_default()?;
    config.validate()?;

    if arguments.validate_only {
        return Ok(());
    }

    let default_filter = match config.preferences.log_level {
        LogLevel::Error => "error",
        LogLevel::Warn => "warn",
        LogLevel::Info => "info",
        LogLevel::Debug => "debug",
    };
    let log_writer = usque_engine::logging::LogWriterFactory::open(&config_path)?;
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_filter)),
        )
        .with_ansi(false)
        .with_writer(log_writer)
        .json()
        .init();

    let service = ControlService::open(store)?;
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
        tokio::select! {
            result = usque_engine::macos_ipc::serve(Arc::clone(&service), socket_path) => result?,
            result = tokio::signal::ctrl_c() => result?,
        }
        if let Err(error) = service.shutdown().await {
            warn!(%error, "engine shutdown could not fully restore platform state");
        }
    }

    #[cfg(not(any(windows, target_os = "macos")))]
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

#[cfg(windows)]
async fn wait_for_parent_exit(parent_pid: Option<u32>) -> std::io::Result<()> {
    let Some(parent_pid) = parent_pid else {
        std::future::pending::<()>().await;
        unreachable!();
    };
    tokio::task::spawn_blocking(move || {
        use windows_sys::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
        use windows_sys::Win32::System::Threading::{
            INFINITE, OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject,
        };

        // SAFETY: OpenProcess may be called with any PID; on success the HANDLE
        // is exclusively owned here until CloseHandle (null is checked next).
        let process = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, parent_pid) };
        if process.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: process is a live handle owned by this task.
        let wait_result = unsafe { WaitForSingleObject(process, INFINITE) };
        // SAFETY: process is still owned here and is closed exactly once.
        unsafe {
            CloseHandle(process);
        }
        if wait_result == WAIT_OBJECT_0 {
            Ok(())
        } else {
            Err(std::io::Error::other(format!(
                "WaitForSingleObject returned {wait_result}"
            )))
        }
    })
    .await
    .map_err(|error| std::io::Error::other(error.to_string()))?
}
