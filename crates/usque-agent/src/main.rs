#[cfg(not(windows))]
fn main() {
    eprintln!("usque-agent is available only on Windows");
}

#[cfg(windows)]
mod windows_main {
    use std::{
        ffi::c_void,
        future::Future,
        io,
        path::PathBuf,
        ptr,
        sync::{Arc, Mutex, OnceLock},
    };

    use clap::Parser;
    use tokio::sync::watch;
    use tracing::{error, info, warn};
    use tracing_subscriber::EnvFilter;
    use usque_agent::{
        coordinator::{AgentCoordinator, ORPHANED_TUNNEL_RECOVERY_GRACE},
        journal::{JournalStore, OperationKind, RecoveryPhase},
        windows::{
            auth::{CallerPolicy, SignerFingerprint},
            backend::WindowsBackend,
            server::{AGENT_PIPE_NAME, AgentService, serve_until},
            state_security::secure_agent_state_path,
            wfp,
        },
    };
    use windows_sys::Win32::{
        Foundation::ERROR_GEN_FAILURE,
        System::Services::{
            RegisterServiceCtrlHandlerExW, SERVICE_ACCEPT_SHUTDOWN, SERVICE_ACCEPT_STOP,
            SERVICE_CONTROL_INTERROGATE, SERVICE_CONTROL_SHUTDOWN, SERVICE_CONTROL_STOP,
            SERVICE_RUNNING, SERVICE_START_PENDING, SERVICE_STATUS, SERVICE_STATUS_HANDLE,
            SERVICE_STOP_PENDING, SERVICE_STOPPED, SERVICE_TABLE_ENTRYW, SERVICE_WIN32_OWN_PROCESS,
            SetServiceStatus, StartServiceCtrlDispatcherW,
        },
    };

    const SERVICE_NAME: &str = "UsqueAgent";

    #[derive(Debug, Clone, Parser)]
    #[command(name = "usque-agent", hide = true)]
    struct Arguments {
        /// Run under the Windows Service Control Manager.
        #[arg(long)]
        service: bool,
        /// Validate paths, Wintun, policy, and recovery journal, then exit.
        #[arg(long)]
        validate_only: bool,
        /// Restore every journaled OS mutation, then exit. Reserved for the
        /// elevated MSI uninstall/upgrade sequence after the service stops.
        #[arg(long, conflicts_with_all = ["service", "validate_only"])]
        recover_state: bool,
        /// Remove every persistent WFP object owned by current Usque builds
        /// without consulting the recovery journal. Reserved for MSI recovery.
        #[arg(
            long,
            conflicts_with_all = ["service", "validate_only", "recover_state"]
        )]
        emergency_remove_kill_switch: bool,
        /// Exact signed Engine path accepted by the privileged Named Pipe.
        #[arg(long = "engine-path")]
        engine_paths: Vec<PathBuf>,
        /// SHA-256 fingerprint of the Authenticode signer certificate.
        #[arg(long)]
        signer_sha256: Option<String>,
        /// Development-only escape hatch; release binaries always reject it.
        #[arg(long, hide = true)]
        allow_unsigned_debug_client: bool,
        /// Override the official pinned Wintun DLL location.
        #[arg(long)]
        wintun: Option<PathBuf>,
        /// Override the LocalSystem recovery journal location.
        #[arg(long)]
        journal: Option<PathBuf>,
        /// Override the fixed Agent pipe in development.
        #[arg(long, hide = true)]
        pipe: Option<String>,
    }

    static SERVICE_ARGUMENTS: OnceLock<Arguments> = OnceLock::new();
    static SERVICE_RUNTIME: OnceLock<Mutex<ServiceRuntimeState>> = OnceLock::new();

    struct ServiceRuntimeState {
        status_handle: usize,
        status: SERVICE_STATUS,
        shutdown: Option<watch::Sender<bool>>,
    }

    pub fn main() -> Result<(), Box<dyn std::error::Error>> {
        tracing_subscriber::fmt()
            .with_env_filter(
                EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
            )
            .with_ansi(false)
            .json()
            .init();

        let arguments = normalize_arguments(Arguments::parse())?;
        if arguments.service {
            SERVICE_ARGUMENTS
                .set(arguments)
                .map_err(|_| "service arguments were initialized twice")?;
            run_service_dispatcher()?;
            Ok(())
        } else {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .thread_name("usque-agent")
                .build()?;
            runtime.block_on(run_agent(arguments, async {
                if let Err(error) = tokio::signal::ctrl_c().await {
                    error!(%error, "failed to install Ctrl+C handler");
                }
            }))
        }
    }

    fn normalize_arguments(mut arguments: Arguments) -> io::Result<Arguments> {
        let executable = std::env::current_exe()?;
        let directory = executable
            .parent()
            .ok_or_else(|| io::Error::other("Agent executable has no parent directory"))?;
        if arguments.engine_paths.is_empty() {
            arguments
                .engine_paths
                .push(directory.join("usque-engine.exe"));
        }
        if arguments.wintun.is_none() {
            arguments.wintun = Some(directory.join("wintun.dll"));
        }
        if arguments.journal.is_none() {
            let program_data = std::env::var_os("ProgramData")
                .ok_or_else(|| io::Error::other("ProgramData is unavailable"))?;
            arguments.journal = Some(
                PathBuf::from(program_data)
                    .join("Usque")
                    .join("agent")
                    .join("recovery-v1.json"),
            );
        }
        Ok(arguments)
    }

    async fn run_agent(
        arguments: Arguments,
        shutdown: impl Future<Output = ()>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let wintun_path = arguments.wintun.as_deref().expect("normalized Wintun path");
        let journal_path = arguments
            .journal
            .as_deref()
            .expect("normalized journal path");
        if arguments.emergency_remove_kill_switch {
            wfp::emergency_remove_kill_switch()?;
            info!("removed all stable Usque WFP Kill Switch resources");
            return Ok(());
        }
        if arguments.recover_state {
            // Always restore basic connectivity first. This path is independent
            // of journal parsing and therefore still works if an interrupted
            // write or disk failure made the detailed recovery state unusable.
            wfp::emergency_remove_kill_switch()?;
            info!("completed journal-independent WFP emergency cleanup");
        }
        secure_agent_state_path(journal_path)?;
        let backend = Arc::new(WindowsBackend::open(wintun_path)?);
        let capabilities = backend.capabilities();
        let coordinator = match AgentCoordinator::open(JournalStore::new(journal_path), backend) {
            Ok(coordinator) => Arc::new(coordinator),
            Err(error) => {
                // A corrupt journal must fail closed with respect to arbitrary
                // mutations, but it must not leave a known Usque block-all WFP
                // policy permanently attached to the host.
                if let Err(cleanup_error) = wfp::emergency_remove_kill_switch() {
                    error!(%cleanup_error, "emergency WFP cleanup after journal failure also failed");
                }
                return Err(error.into());
            }
        };
        let state = coordinator.state().await;
        if arguments.recover_state {
            if state.phase != RecoveryPhase::Clean {
                coordinator.recover_stale().await?;
                info!(
                    generation = state.generation,
                    "restored Agent recovery journal for uninstall or upgrade"
                );
            } else {
                info!("Agent recovery journal is already clean");
            }
            return Ok(());
        }

        let signer = arguments
            .signer_sha256
            .as_deref()
            .map(SignerFingerprint::parse)
            .transpose()?;
        let policy = Arc::new(CallerPolicy::new(
            arguments.engine_paths,
            signer,
            arguments.allow_unsigned_debug_client,
        )?);
        if arguments.validate_only {
            info!("Agent configuration and pinned Wintun library are valid");
            return Ok(());
        }
        if state.phase != RecoveryPhase::Clean {
            if state.operation_kind == Some(OperationKind::SystemProxy) {
                // A dead local proxy would otherwise strand WinINet clients.
                // Its write-ahead receipt is per-user and safe to restore
                // automatically.
                coordinator.recover_stale().await?;
                info!("recovered stale per-user system proxy transaction");
            } else if state.phase != RecoveryPhase::Active {
                // Preparing/Prepared/Recovering/RecoveryRequired cannot carry
                // traffic and must never retain a persistent block-all policy
                // across an Agent or machine restart.
                coordinator.recover_stale().await?;
                info!(
                    phase = ?state.phase,
                    generation = state.generation,
                    "recovered incomplete tunnel transaction during Agent startup"
                );
            } else {
                warn!(
                    phase = ?state.phase,
                    generation = state.generation,
                    grace_seconds = ORPHANED_TUNNEL_RECOVERY_GRACE.as_secs(),
                    "active tunnel retained briefly for authenticated Engine reattachment"
                );
            }
        }

        let startup_orphan = (state.phase == RecoveryPhase::Active
            && state.operation_kind == Some(OperationKind::Tunnel))
        .then_some(state.operation_id)
        .flatten();
        if let Some(operation_id) = startup_orphan {
            let watchdog = Arc::clone(&coordinator);
            tokio::spawn(async move {
                tokio::time::sleep(ORPHANED_TUNNEL_RECOVERY_GRACE).await;
                match watchdog.recover_orphaned_tunnel(operation_id, 0).await {
                    Ok(true) => warn!(
                        %operation_id,
                        grace_seconds = ORPHANED_TUNNEL_RECOVERY_GRACE.as_secs(),
                        "recovered an active tunnel that was not reattached after Agent restart"
                    ),
                    Ok(false) => {}
                    Err(error) => error!(
                        %operation_id,
                        %error,
                        "failed to recover an orphaned tunnel after Agent restart"
                    ),
                }
            });
        }

        let pipe_name = arguments.pipe.unwrap_or_else(|| AGENT_PIPE_NAME.to_owned());
        let service = Arc::new(AgentService::new(Arc::clone(&coordinator), capabilities));
        info!(%pipe_name, "starting privileged Agent Named Pipe");
        serve_until(service, policy, pipe_name, shutdown).await?;
        info!("Agent stop requested; persistent recovery state was retained");
        Ok(())
    }

    fn run_service_dispatcher() -> io::Result<()> {
        let mut service_name = wide(SERVICE_NAME);
        let table = [
            SERVICE_TABLE_ENTRYW {
                lpServiceName: service_name.as_mut_ptr(),
                lpServiceProc: Some(service_main),
            },
            SERVICE_TABLE_ENTRYW::default(),
        ];
        // SAFETY: the table remains live until the blocking dispatcher returns
        // and contains the required null terminator entry.
        if unsafe { StartServiceCtrlDispatcherW(table.as_ptr()) } == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    unsafe extern "system" fn service_main(_count: u32, _arguments: *mut *mut u16) {
        if let Err(error) = run_service_main() {
            error!(%error, "Usque Agent service failed");
            let _ = report_service_status(SERVICE_STOPPED, ERROR_GEN_FAILURE, 0, 0);
        }
    }

    fn run_service_main() -> Result<(), Box<dyn std::error::Error>> {
        let mut service_name = wide(SERVICE_NAME);
        // SAFETY: service_name is null-terminated and the callback has the
        // documented lifetime.
        let status_handle = unsafe {
            RegisterServiceCtrlHandlerExW(
                service_name.as_mut_ptr(),
                Some(service_control_handler),
                ptr::null(),
            )
        };
        if status_handle.is_null() {
            return Err(io::Error::last_os_error().into());
        }
        let (shutdown_sender, mut shutdown_receiver) = watch::channel(false);
        SERVICE_RUNTIME
            .set(Mutex::new(ServiceRuntimeState {
                status_handle: status_handle as usize,
                status: SERVICE_STATUS {
                    dwServiceType: SERVICE_WIN32_OWN_PROCESS,
                    dwCurrentState: SERVICE_START_PENDING,
                    dwControlsAccepted: 0,
                    dwWin32ExitCode: 0,
                    dwServiceSpecificExitCode: 0,
                    dwCheckPoint: 1,
                    dwWaitHint: 15_000,
                },
                shutdown: Some(shutdown_sender),
            }))
            .map_err(|_| "service runtime was initialized twice")?;
        report_service_status(SERVICE_START_PENDING, 0, 1, 15_000)?;

        let arguments = SERVICE_ARGUMENTS
            .get()
            .ok_or("service arguments are unavailable")?
            .clone();
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("usque-agent")
            .build()?;
        report_service_status(SERVICE_RUNNING, 0, 0, 0)?;
        let result = runtime.block_on(run_agent(arguments, async move {
            while !*shutdown_receiver.borrow() {
                if shutdown_receiver.changed().await.is_err() {
                    break;
                }
            }
        }));
        report_service_status(
            SERVICE_STOPPED,
            if result.is_ok() { 0 } else { ERROR_GEN_FAILURE },
            0,
            0,
        )?;
        result
    }

    unsafe extern "system" fn service_control_handler(
        control: u32,
        _event_type: u32,
        _event_data: *mut c_void,
        _context: *mut c_void,
    ) -> u32 {
        match control {
            SERVICE_CONTROL_STOP | SERVICE_CONTROL_SHUTDOWN => {
                let _ = report_service_status(SERVICE_STOP_PENDING, 0, 1, 15_000);
                if let Some(runtime) = SERVICE_RUNTIME.get()
                    && let Ok(state) = runtime.lock()
                    && let Some(shutdown) = &state.shutdown
                {
                    let _ = shutdown.send(true);
                }
            }
            SERVICE_CONTROL_INTERROGATE => {
                let _ = repeat_service_status();
            }
            _ => {}
        }
        0
    }

    fn report_service_status(
        current_state: u32,
        exit_code: u32,
        checkpoint: u32,
        wait_hint: u32,
    ) -> io::Result<()> {
        let runtime = SERVICE_RUNTIME
            .get()
            .ok_or_else(|| io::Error::other("service runtime is unavailable"))?;
        let mut state = runtime
            .lock()
            .map_err(|_| io::Error::other("service status lock was poisoned"))?;
        state.status.dwCurrentState = current_state;
        state.status.dwControlsAccepted = if current_state == SERVICE_RUNNING {
            SERVICE_ACCEPT_STOP | SERVICE_ACCEPT_SHUTDOWN
        } else {
            0
        };
        state.status.dwWin32ExitCode = exit_code;
        state.status.dwCheckPoint = checkpoint;
        state.status.dwWaitHint = wait_hint;
        set_status(state.status_handle, &state.status)
    }

    fn repeat_service_status() -> io::Result<()> {
        let runtime = SERVICE_RUNTIME
            .get()
            .ok_or_else(|| io::Error::other("service runtime is unavailable"))?;
        let state = runtime
            .lock()
            .map_err(|_| io::Error::other("service status lock was poisoned"))?;
        set_status(state.status_handle, &state.status)
    }

    fn set_status(status_handle: usize, status: &SERVICE_STATUS) -> io::Result<()> {
        // SAFETY: SCM supplied this handle and status is valid for the call.
        if unsafe { SetServiceStatus(status_handle as SERVICE_STATUS_HANDLE, status) } == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }
}

#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    windows_main::main()
}
