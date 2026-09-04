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
        coordinator::{AgentCoordinator, ORPHANED_TUNNEL_RECOVERY_GRACE, TunnelInspection},
        journal::{JournalStore, OperationKind, RecoveryPhase},
        windows::{
            auth::{CallerPolicy, SignerFingerprint},
            backend::WindowsBackend,
            server::{
                AGENT_PIPE_NAME, AgentService, ServeExit, ShutdownReason, serve_until_ready,
                validate_pipe_creation,
            },
            service_config::{
                NoopServiceStartModeController, PRESHUTDOWN_TIMEOUT_MS, ServiceStartModeController,
                WindowsServiceStartModeController,
            },
            state_security::{finalize_uninstall_state, secure_agent_state_path},
            wfp,
        },
    };
    use windows_sys::Win32::{
        Foundation::ERROR_GEN_FAILURE,
        System::Services::{
            RegisterServiceCtrlHandlerExW, SERVICE_ACCEPT_PRESHUTDOWN, SERVICE_ACCEPT_SHUTDOWN,
            SERVICE_ACCEPT_STOP, SERVICE_CONTROL_INTERROGATE, SERVICE_CONTROL_PRESHUTDOWN,
            SERVICE_CONTROL_SHUTDOWN, SERVICE_CONTROL_STOP, SERVICE_RUNNING, SERVICE_START_PENDING,
            SERVICE_STATUS, SERVICE_STATUS_HANDLE, SERVICE_STOP_PENDING, SERVICE_STOPPED,
            SERVICE_TABLE_ENTRYW, SERVICE_WIN32_OWN_PROCESS, SetServiceStatus,
            StartServiceCtrlDispatcherW,
        },
    };

    const SERVICE_NAME: &str = "UsqueAgent";
    // Leave time to publish STOPPED before SCM's 30-second preshutdown limit.
    const SHUTDOWN_RECOVERY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(25);

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum StartupRecoveryAction {
        None,
        RecoverOnce,
        RetainActiveTunnel,
        QuarantineRecoveryRequired,
    }

    fn startup_recovery_action(
        phase: RecoveryPhase,
        operation_kind: Option<OperationKind>,
    ) -> StartupRecoveryAction {
        if phase == RecoveryPhase::Clean {
            StartupRecoveryAction::None
        } else if operation_kind == Some(OperationKind::SystemProxy) {
            StartupRecoveryAction::RecoverOnce
        } else {
            match phase {
                RecoveryPhase::Active => StartupRecoveryAction::RetainActiveTunnel,
                RecoveryPhase::RecoveryRequired => {
                    StartupRecoveryAction::QuarantineRecoveryRequired
                }
                _ => StartupRecoveryAction::RecoverOnce,
            }
        }
    }

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
        /// Remove a proven-clean recovery journal and empty Agent state
        /// directories. Reserved for true MSI uninstall, never major upgrade.
        #[arg(
            long,
            conflicts_with_all = [
                "service",
                "validate_only",
                "recover_state",
                "emergency_remove_kill_switch"
            ]
        )]
        finalize_uninstall: bool,
        /// Remove every persistent WFP object owned by current Usque builds
        /// without consulting the recovery journal. Reserved for MSI recovery.
        #[arg(
            long,
            conflicts_with_all = [
                "service",
                "validate_only",
                "recover_state",
                "finalize_uninstall"
            ]
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
        shutdown: Option<watch::Sender<Option<ShutdownReason>>>,
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
            runtime.block_on(run_agent(
                arguments,
                async {
                    if let Err(error) = tokio::signal::ctrl_c().await {
                        error!(%error, "failed to install Ctrl+C handler");
                    }
                    ShutdownReason::ServiceStop
                },
                || Ok(()),
            ))
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

    async fn run_agent<Ready>(
        arguments: Arguments,
        shutdown: impl Future<Output = ShutdownReason>,
        ready: Ready,
    ) -> Result<(), Box<dyn std::error::Error>>
    where
        Ready: FnOnce() -> io::Result<()>,
    {
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
        if arguments.finalize_uninstall {
            finalize_uninstall_state(journal_path)?;
            info!(
                path = %journal_path.display(),
                "removed clean Agent recovery state for uninstall"
            );
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
        if arguments.recover_state {
            let state = coordinator.state().await;
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

        let manages_service_mode = arguments.service;
        let pipe_name = arguments
            .pipe
            .clone()
            .unwrap_or_else(|| AGENT_PIPE_NAME.to_owned());
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
        let state = coordinator.state().await;
        if arguments.validate_only {
            validate_pipe_creation(&pipe_name)?;
            info!(
                phase = ?state.phase,
                %pipe_name,
                "Agent configuration, pinned Wintun library, recovery journal, and pipe ACL are valid"
            );
            return Ok(());
        }

        let start_mode: Arc<dyn ServiceStartModeController> = if manages_service_mode {
            Arc::new(WindowsServiceStartModeController::new(SERVICE_NAME))
        } else {
            Arc::new(NoopServiceStartModeController)
        };
        start_mode.ensure_shutdown_timeout().await?;
        let service = Arc::new(AgentService::with_start_mode_controller(
            Arc::clone(&coordinator),
            capabilities,
            start_mode,
        ));
        match service.reconcile_removed_adapter_dependencies().await {
            Ok(true) => {
                let reconciled = service.state().await;
                info!(
                    phase = ?reconciled.phase,
                    generation = reconciled.generation,
                    "reconciled adapter-dependent recovery receipts after the exact Wintun adapter was already removed"
                );
            }
            Ok(false) => {}
            Err(error) => error!(
                %error,
                "could not persist journal-only startup reconciliation; retaining recovery-required state"
            ),
        }
        let state = service.state().await;
        let action = match startup_recovery_action(state.phase, state.operation_kind) {
            StartupRecoveryAction::RetainActiveTunnel => {
                match service.inspect_startup_tunnel().await {
                    Ok(TunnelInspection::Reattachable) => StartupRecoveryAction::RetainActiveTunnel,
                    Ok(TunnelInspection::NeedsRecovery) => StartupRecoveryAction::RecoverOnce,
                    Err(error) => {
                        error!(%error, "startup resource inspection failed; retaining blocked recovery state");
                        StartupRecoveryAction::QuarantineRecoveryRequired
                    }
                }
            }
            action => action,
        };
        match action {
            StartupRecoveryAction::None => {}
            StartupRecoveryAction::RecoverOnce => {
                // A dead local proxy would otherwise strand WinINet clients.
                // Incomplete tunnel setup also gets one bounded recovery pass.
                // A failure is quarantined instead of terminating the service:
                // the authenticated Engine must still be able to inspect state
                // and request an explicit retry, and MSI must not loop on SCM
                // startup while the journal remains RecoveryRequired.
                match service.recover_stale().await {
                    Ok(()) => info!(
                        phase = ?state.phase,
                        generation = state.generation,
                        "recovered incomplete Agent transaction during startup"
                    ),
                    Err(recovery_error) => {
                        error!(
                            phase = ?state.phase,
                            generation = state.generation,
                            %recovery_error,
                            "startup recovery failed; keeping Agent available in recovery-required mode"
                        );
                    }
                }
            }
            StartupRecoveryAction::RetainActiveTunnel => {
                warn!(
                    phase = ?state.phase,
                    generation = state.generation,
                    grace_seconds = ORPHANED_TUNNEL_RECOVERY_GRACE.as_secs(),
                    "active tunnel retained briefly for authenticated Engine reattachment"
                );
            }
            StartupRecoveryAction::QuarantineRecoveryRequired => {
                // Startup itself never issues an unguarded recovery against a
                // previously failed journal. The service-owned supervisor is
                // armed before the pipe becomes ready and revalidates the exact
                // operation/generation after each bounded backoff delay.
                warn!(
                    phase = ?state.phase,
                    generation = state.generation,
                    "Agent recovery remains required; bounded automatic recovery will be scheduled"
                );
            }
        }

        // Repair any stale service configuration left by an interrupted mode
        // transition or an upgrade. In particular, a clean journal must not
        // leave the Agent configured to start again at the next boot.
        service.synchronize_start_mode().await;

        let state = service.state().await;
        let startup_orphan = (action == StartupRecoveryAction::RetainActiveTunnel
            && state.phase == RecoveryPhase::Active
            && state.operation_kind == Some(OperationKind::Tunnel))
        .then_some(state.operation_id)
        .flatten();
        if let Some(operation_id) = startup_orphan {
            let watchdog = Arc::clone(&service);
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

        info!(%pipe_name, "starting privileged Agent Named Pipe");
        match serve_until_ready(Arc::clone(&service), policy, pipe_name, shutdown, ready).await? {
            ServeExit::Shutdown(ShutdownReason::ServiceStop) => {
                info!("Agent stop requested; persistent recovery state was retained")
            }
            ServeExit::Shutdown(ShutdownReason::SystemShutdown) => {
                // Detach, do not abort, on timeout: a native spawn_blocking call
                // cannot be cancelled. Its task retains the mutation gate until
                // it completes or the service process exits. New work is barred.
                let mut recovery =
                    tokio::spawn(async move { service.recover_for_shutdown().await });
                match tokio::time::timeout(SHUTDOWN_RECOVERY_TIMEOUT, &mut recovery).await {
                    Ok(Ok(Ok(()))) => info!("system shutdown restored Agent network state"),
                    Ok(Ok(Err(error))) => {
                        error!(%error, "shutdown recovery incomplete; retained recovery journal");
                        return Err(error.into());
                    }
                    Ok(Err(_)) => {
                        return Err(io::Error::other(
                            "shutdown recovery worker failed; retained recovery journal",
                        )
                        .into());
                    }
                    Err(_) => {
                        return Err(io::Error::new(
                            io::ErrorKind::TimedOut,
                            "shutdown recovery deadline expired; retained recovery journal",
                        )
                        .into());
                    }
                }
            }
            ServeExit::Idle => info!("Agent exited after the clean idle grace period"),
        }
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
        let (shutdown_sender, mut shutdown_receiver) = watch::channel(None);
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
        let result = runtime.block_on(async move {
            let heartbeat = tokio::spawn(async {
                let mut checkpoint = 1_u32;
                let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));
                interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                loop {
                    interval.tick().await;
                    checkpoint = checkpoint.saturating_add(1);
                    if let Err(error) = advance_pending_checkpoint(checkpoint, 15_000) {
                        error!(%error, "could not refresh the Agent service pending checkpoint");
                    }
                }
            });
            let result = run_agent(
                arguments,
                async move {
                    loop {
                        if let Some(reason) = *shutdown_receiver.borrow() {
                            return reason;
                        }
                        if shutdown_receiver.changed().await.is_err() {
                            return ShutdownReason::ServiceStop;
                        }
                    }
                },
                report_service_running_if_starting,
            )
            .await;
            heartbeat.abort();
            let _ = heartbeat.await;
            result
        });
        // In-flight native cleanup retains its write-ahead evidence. Do not
        // let Runtime::drop wait indefinitely beyond the SCM shutdown budget.
        runtime.shutdown_background();
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
            SERVICE_CONTROL_STOP | SERVICE_CONTROL_SHUTDOWN | SERVICE_CONTROL_PRESHUTDOWN => {
                let _ = report_service_status(SERVICE_STOP_PENDING, 0, 1, PRESHUTDOWN_TIMEOUT_MS);
                if let Some(runtime) = SERVICE_RUNTIME.get()
                    && let Ok(state) = runtime.lock()
                    && let Some(shutdown) = &state.shutdown
                {
                    let _ = shutdown.send(Some(shutdown_reason(control)));
                }
            }
            SERVICE_CONTROL_INTERROGATE => {
                let _ = repeat_service_status();
            }
            _ => {}
        }
        0
    }

    fn shutdown_reason(control: u32) -> ShutdownReason {
        match control {
            SERVICE_CONTROL_PRESHUTDOWN | SERVICE_CONTROL_SHUTDOWN => {
                ShutdownReason::SystemShutdown
            }
            _ => ShutdownReason::ServiceStop,
        }
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
            SERVICE_ACCEPT_STOP | SERVICE_ACCEPT_SHUTDOWN | SERVICE_ACCEPT_PRESHUTDOWN
        } else {
            0
        };
        state.status.dwWin32ExitCode = exit_code;
        state.status.dwCheckPoint = checkpoint;
        state.status.dwWaitHint = wait_hint;
        set_status(state.status_handle, &state.status)
    }

    fn advance_pending_checkpoint(checkpoint: u32, wait_hint: u32) -> io::Result<()> {
        let runtime = SERVICE_RUNTIME
            .get()
            .ok_or_else(|| io::Error::other("service runtime is unavailable"))?;
        let mut state = runtime
            .lock()
            .map_err(|_| io::Error::other("service status lock was poisoned"))?;
        if !matches!(
            state.status.dwCurrentState,
            SERVICE_START_PENDING | SERVICE_STOP_PENDING
        ) {
            return Ok(());
        }
        state.status.dwCheckPoint = checkpoint;
        state.status.dwWaitHint = if state.status.dwCurrentState == SERVICE_STOP_PENDING {
            PRESHUTDOWN_TIMEOUT_MS
        } else {
            wait_hint
        };
        set_status(state.status_handle, &state.status)
    }

    fn report_service_running_if_starting() -> io::Result<()> {
        let runtime = SERVICE_RUNTIME
            .get()
            .ok_or_else(|| io::Error::other("service runtime is unavailable"))?;
        let mut state = runtime
            .lock()
            .map_err(|_| io::Error::other("service status lock was poisoned"))?;
        if state.status.dwCurrentState == SERVICE_STOP_PENDING {
            return Ok(());
        }
        if state.status.dwCurrentState != SERVICE_START_PENDING {
            return Err(io::Error::other("service left start-pending unexpectedly"));
        }
        state.status.dwCurrentState = SERVICE_RUNNING;
        state.status.dwControlsAccepted =
            SERVICE_ACCEPT_STOP | SERVICE_ACCEPT_SHUTDOWN | SERVICE_ACCEPT_PRESHUTDOWN;
        state.status.dwWin32ExitCode = 0;
        state.status.dwCheckPoint = 0;
        state.status.dwWaitHint = 0;
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

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn system_shutdown_is_distinct_from_maintenance_service_stop() {
            assert_eq!(
                shutdown_reason(SERVICE_CONTROL_STOP),
                ShutdownReason::ServiceStop
            );
            assert_eq!(
                shutdown_reason(SERVICE_CONTROL_SHUTDOWN),
                ShutdownReason::SystemShutdown
            );
            assert_eq!(
                shutdown_reason(SERVICE_CONTROL_PRESHUTDOWN),
                ShutdownReason::SystemShutdown
            );
            assert!(SHUTDOWN_RECOVERY_TIMEOUT.as_millis() < u128::from(PRESHUTDOWN_TIMEOUT_MS));
        }

        #[test]
        fn recovery_required_tunnel_is_quarantined_for_the_bounded_supervisor() {
            assert_eq!(
                startup_recovery_action(
                    RecoveryPhase::RecoveryRequired,
                    Some(OperationKind::Tunnel)
                ),
                StartupRecoveryAction::QuarantineRecoveryRequired
            );
        }

        #[test]
        fn stale_system_proxy_gets_one_recovery_attempt() {
            assert_eq!(
                startup_recovery_action(
                    RecoveryPhase::RecoveryRequired,
                    Some(OperationKind::SystemProxy)
                ),
                StartupRecoveryAction::RecoverOnce
            );
        }

        #[test]
        fn active_tunnel_is_retained_for_reattachment() {
            assert_eq!(
                startup_recovery_action(RecoveryPhase::Active, Some(OperationKind::Tunnel)),
                StartupRecoveryAction::RetainActiveTunnel
            );
        }
    }
}

#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    windows_main::main()
}
