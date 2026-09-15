use std::future::Future;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, atomic::AtomicBool};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use tokio::io::unix::AsyncFd;
use tokio::time::{MissedTickBehavior, interval};
use tokio_util::sync::CancellationToken;
use usque_core::{AddressFamily, Transport, WarpIdentity};
use usque_core::{ReconfigureClass, classify_reconfigure};
use usque_geo::CountryCode;
use usque_transport::{
    DataPlaneRuntime, EndpointPinRefresher, GeoDirectPolicy, RuntimeHealth, RuntimePath,
    TrafficSnapshot, TransportError, TunPacketIo,
};

use crate::exit_probe_task::{ExitProbeTask, run_probe};
use crate::session_pump::{SessionDataEvent, next_session_data, wait_pending};
use crate::tun_read_slab::TunReadSlab;

use super::{
    AndroidEndpointPinRefresher, AndroidSocketProtector, MasqueTlsIdentity, NativeFailure,
    NativeSnapshot, Profile, RECONFIGURE_NEED_ATTACH, RECONFIGURE_NEED_COLD,
    RECONFIGURE_NOT_RUNNING, RECONFIGURE_OK, START_ALREADY_RUNNING, START_INVALID_PROFILE,
    START_OK, START_PLATFORM_FAILURE, START_TRANSPORT_FAILURE, START_TUN_FAILURE, SocketProtector,
    android_transport_failure,
};

static ENGINE: OnceLock<Mutex<Option<EngineHandle>>> = OnceLock::new();
static LAST_START_ERROR: OnceLock<Mutex<Option<NativeSnapshot>>> = OnceLock::new();
type InternalNetworks = (
    usque_transport::InternalNetwork,
    usque_transport::InternalNetwork,
);
static NETWORKS: Mutex<Option<InternalNetworks>> = Mutex::new(None);
pub(super) fn internal_networks() -> Option<InternalNetworks> {
    NETWORKS.lock().ok().and_then(|v| v.clone())
}

pub(super) fn is_running() -> bool {
    ENGINE
        .get()
        .is_some_and(|engine| engine.lock().map_or(true, |slot| slot.is_some()))
}

enum RuntimeCommand {
    Reconfigure {
        profile: Profile,
        reply: std::sync::mpsc::SyncSender<i32>,
        cancelled: Arc<AtomicBool>,
    },
    AttachTun {
        tun: OwnedFd,
        profile: Profile,
        reply: std::sync::mpsc::SyncSender<i32>,
        cancelled: Arc<AtomicBool>,
    },
    DetachTun {
        reply: std::sync::mpsc::SyncSender<i32>,
        cancelled: Arc<AtomicBool>,
    },
    RejectFinalNetwork {
        reply: std::sync::mpsc::SyncSender<i32>,
        cancelled: Arc<AtomicBool>,
    },
}

struct EngineHandle {
    cancellation: CancellationToken,
    status: Arc<Mutex<NativeSnapshot>>,
    protector: Arc<AndroidSocketProtector>,
    commands: tokio::sync::mpsc::UnboundedSender<RuntimeCommand>,
    // A retained, shared join owner keeps ENGINE occupied while stopping.
    // Starts remain fail-closed, without holding ENGINE across a blocking wait.
    thread: Arc<Mutex<Option<JoinHandle<()>>>>,
}

pub(super) fn start(
    tun_file_descriptor: i32,
    profile: Profile,
    identity: WarpIdentity,
    geo_cache_dir: PathBuf,
    protector: Arc<AndroidSocketProtector>,
) -> i32 {
    spawn_runtime(
        "usque-vpn",
        Some(tun_file_descriptor),
        profile,
        identity,
        geo_cache_dir,
        protector,
    )
}

pub(super) fn start_proxy(
    profile: Profile,
    identity: WarpIdentity,
    geo_cache_dir: PathBuf,
    protector: Arc<AndroidSocketProtector>,
) -> i32 {
    spawn_runtime(
        "usque-proxy",
        None,
        profile,
        identity,
        geo_cache_dir,
        protector,
    )
}

fn spawn_runtime(
    thread_name: &str,
    tun_file_descriptor: Option<i32>,
    profile: Profile,
    identity: WarpIdentity,
    geo_cache_dir: PathBuf,
    protector: Arc<AndroidSocketProtector>,
) -> i32 {
    clear_last_start_error();
    let engine = ENGINE.get_or_init(|| Mutex::new(None));
    let mut slot = match engine.lock() {
        Ok(slot) => slot,
        Err(_) => return START_PLATFORM_FAILURE,
    };
    if slot.is_some() {
        return START_ALREADY_RUNNING;
    }
    super::connection_timeline::publish(Default::default());
    let tun = match tun_file_descriptor {
        Some(fd) => match duplicate_tun(fd) {
            Ok(tun) => Some(tun),
            Err(code) => return code,
        },
        None => None,
    };
    let tls_identity = match MasqueTlsIdentity::from_warp_identity(&identity) {
        Ok(identity) => identity,
        Err(_) => return START_TRANSPORT_FAILURE,
    };
    let pin_refresher: Arc<dyn EndpointPinRefresher> = Arc::new(AndroidEndpointPinRefresher {
        profile_id: profile.id.to_string(),
        identity: tokio::sync::Mutex::new(identity),
        protector: Arc::clone(&protector),
    });
    let geo_policy = match load_geo_direct_policy(&profile, &geo_cache_dir) {
        Ok(policy) => Arc::new(policy),
        Err(message) => {
            let mut snapshot = NativeSnapshot::disconnected();
            snapshot.phase = "error".to_owned();
            snapshot.error_code = Some("ANDROID_GEO_RULES_UNAVAILABLE".to_owned());
            snapshot.warning = Some(message);
            remember_last_start_error(snapshot);
            return START_TRANSPORT_FAILURE;
        }
    };
    let selected_gate = if profile.vpn_gate.enabled {
        let selected = profile.vpn_gate.selection.as_ref().and_then(|selection| {
            usque_core::vpngate::CatalogueStore::new(&geo_cache_dir)
                .load_selection(selection)
                .ok()
        });
        let Some(selected) = selected else {
            return START_INVALID_PROFILE;
        };
        // The provisional blocking TUN must never carry final packets.
        if tun.is_some() {
            return START_INVALID_PROFILE;
        }
        Some(selected)
    } else {
        None
    };

    // JNI captured an earlier generation before taking ENGINE's lock. Re-read
    // the authoritative atomic Java generation before any worker can bind;
    // notifications after this point wait for this same lock and see the handle.
    if protector.refresh_network_generation().is_err() {
        return START_PLATFORM_FAILURE;
    }
    let cancellation = CancellationToken::new();
    let mut initial_status = NativeSnapshot::preparing();
    initial_status.session_congestion_control = Some(profile.congestion_control);
    initial_status.data_plane = Some(profile.data_plane);
    let status = Arc::new(Mutex::new(initial_status));
    let (started_tx, started_rx) = std::sync::mpsc::sync_channel(1);
    let (command_tx, command_rx) = tokio::sync::mpsc::unbounded_channel();
    let thread_cancel = cancellation.clone();
    let thread_status = Arc::clone(&status);
    let handle_protector = Arc::clone(&protector);
    let thread = std::thread::Builder::new()
        .name(thread_name.to_owned())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    set_error_with_code(
                        &thread_status,
                        "ANDROID_RUNTIME_FAILED",
                        format!("Tokio runtime failed: {error}"),
                    );
                    let _ = started_tx.send(START_PLATFORM_FAILURE);
                    return;
                }
            };
            runtime.block_on(run(
                tun,
                profile,
                tls_identity,
                protector,
                geo_policy,
                selected_gate,
                geo_cache_dir,
                pin_refresher,
                thread_cancel,
                thread_status,
                started_tx,
                command_rx,
            ));
        });
    let thread = match thread {
        Ok(thread) => thread,
        Err(_) => return START_PLATFORM_FAILURE,
    };
    *slot = Some(EngineHandle {
        cancellation,
        status: Arc::clone(&status),
        protector: handle_protector,
        commands: command_tx,
        thread: Arc::new(Mutex::new(Some(thread))),
    });
    drop(slot);

    let result = match started_rx.recv_timeout(Duration::from_secs(90)) {
        Ok(result) => result,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            set_error_with_code(
                &status,
                "ANDROID_START_TIMEOUT",
                "The Android native runtime did not start within 90 seconds.".to_owned(),
            );
            START_TRANSPORT_FAILURE
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            set_error_with_code(
                &status,
                "ANDROID_RUNTIME_FAILED",
                "The Android native runtime exited before reporting startup status.".to_owned(),
            );
            START_PLATFORM_FAILURE
        }
    };
    if result != START_OK {
        let failure = snapshot();
        stop();
        remember_last_start_error(failure);
    }
    result
}

fn load_geo_direct_policy(profile: &Profile, cache_dir: &Path) -> Result<GeoDirectPolicy, String> {
    if profile.geo_direct_countries.is_empty() {
        return Ok(GeoDirectPolicy::disabled());
    }
    let countries = match profile
        .geo_direct_countries
        .iter()
        .map(|country| CountryCode::parse(country))
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(countries) => countries,
        Err(error) => {
            return Err(format!("invalid Android GEO direct policy: {error}"));
        }
    };
    match GeoDirectPolicy::load(cache_dir, countries) {
        Ok(policy) => Ok(policy),
        Err(error) => Err(format!("Android GEO cache could not be loaded: {error}")),
    }
}

fn duplicate_tun(tun_file_descriptor: i32) -> Result<OwnedFd, i32> {
    if tun_file_descriptor < 0 {
        return Err(START_TUN_FAILURE);
    }
    // SAFETY: tun_file_descriptor is the VpnService TUN FD passed from Java.
    let duplicated = unsafe { libc::dup(tun_file_descriptor) };
    if duplicated < 0 {
        return Err(START_TUN_FAILURE);
    }
    // SAFETY: duplicated is a freshly owned FD from dup; ownership transfers
    // to OwnedFd.
    let owned = unsafe { OwnedFd::from_raw_fd(duplicated) };
    if let Err(error) = set_nonblocking(&owned) {
        tracing::error!(%error, "could not make Android TUN nonblocking");
        return Err(START_TUN_FAILURE);
    }
    Ok(owned)
}

pub(super) fn stop() -> bool {
    clear_last_start_error();
    let Some(engine) = ENGINE.get() else {
        return true;
    };
    let owner = {
        let Ok(slot) = engine.try_lock() else {
            return false;
        };
        let Some(handle) = slot.as_ref() else {
            return true;
        };
        handle.cancellation.cancel();
        Arc::clone(&handle.thread)
    };
    // Only stop callers serialize on this lock. Snapshots, cancellation and
    // start admission never wait for a worker while holding ENGINE.
    let Ok(mut joining) = owner.try_lock() else {
        return false;
    };
    if let Some(thread) = joining.as_ref()
        && !crate::runtime_stop::wait_finished(thread, Duration::from_secs(5))
    {
        return false;
    }
    if let Some(thread) = joining.take() {
        let _ = thread.join();
    }
    let Ok(mut slot) = engine.try_lock() else {
        return false;
    };
    if slot
        .as_ref()
        .is_some_and(|handle| Arc::ptr_eq(&handle.thread, &owner))
    {
        slot.take();
    }
    true
}

pub(super) fn cancel() {
    let Some(engine) = ENGINE.get() else {
        return;
    };
    let Ok(slot) = engine.lock() else {
        return;
    };
    if let Some(handle) = slot.as_ref() {
        handle.cancellation.cancel();
    }
}

pub(super) fn notify_network_changed(generation: u64) {
    let Some(engine) = ENGINE.get() else {
        return;
    };
    let Ok(slot) = engine.lock() else {
        return;
    };
    if let Some(handle) = slot.as_ref() {
        super::publish_network_generation(&handle.protector.network_generation, generation);
    }
}

pub(super) fn snapshot() -> NativeSnapshot {
    ENGINE
        .get()
        .and_then(|engine| engine.lock().ok())
        .and_then(|slot| {
            slot.as_ref()
                .and_then(|handle| handle.status.lock().ok())
                .map(|status| status.clone())
        })
        .or_else(last_start_error)
        .unwrap_or_else(NativeSnapshot::disconnected)
}

pub(super) fn reconfigure(profile: Profile) -> i32 {
    send_command(|reply, cancelled| RuntimeCommand::Reconfigure {
        profile,
        reply,
        cancelled,
    })
}

pub(super) fn attach_tun(tun_file_descriptor: i32, profile: Profile) -> i32 {
    if tun_file_descriptor < 0 {
        return START_TUN_FAILURE;
    }
    // SAFETY: tun_file_descriptor is the VpnService TUN FD passed from Java.
    let duplicated = unsafe { libc::dup(tun_file_descriptor) };
    if duplicated < 0 {
        return START_TUN_FAILURE;
    }
    // SAFETY: duplicated is a freshly owned FD from dup.
    let tun = unsafe { OwnedFd::from_raw_fd(duplicated) };
    send_command(|reply, cancelled| RuntimeCommand::AttachTun {
        tun,
        profile,
        reply,
        cancelled,
    })
}

pub(super) fn detach_tun() -> i32 {
    send_command(|reply, cancelled| RuntimeCommand::DetachTun { reply, cancelled })
}

pub(super) fn reject_final_network() -> i32 {
    send_command(|reply, cancelled| RuntimeCommand::RejectFinalNetwork { reply, cancelled })
}

fn send_command(
    build: impl FnOnce(std::sync::mpsc::SyncSender<i32>, Arc<AtomicBool>) -> RuntimeCommand,
) -> i32 {
    let Some(engine) = ENGINE.get() else {
        return RECONFIGURE_NOT_RUNNING;
    };
    let commands = {
        let Ok(slot) = engine.lock() else {
            return START_PLATFORM_FAILURE;
        };
        let Some(handle) = slot.as_ref() else {
            return RECONFIGURE_NOT_RUNNING;
        };
        handle.commands.clone()
    };
    let (reply_tx, reply_rx) = std::sync::mpsc::sync_channel(1);
    let cancelled = Arc::new(AtomicBool::new(false));
    if commands
        .send(build(reply_tx, Arc::clone(&cancelled)))
        .is_err()
    {
        return RECONFIGURE_NOT_RUNNING;
    }
    super::wait_jni_command_reply(reply_rx, &cancelled, Duration::from_secs(90))
}

fn clear_last_start_error() {
    if let Ok(mut error) = LAST_START_ERROR.get_or_init(|| Mutex::new(None)).lock() {
        *error = None;
    }
}

fn remember_last_start_error(snapshot: NativeSnapshot) {
    if let Ok(mut error) = LAST_START_ERROR.get_or_init(|| Mutex::new(None)).lock() {
        *error = Some(snapshot);
    }
}

fn last_start_error() -> Option<NativeSnapshot> {
    LAST_START_ERROR
        .get()
        .and_then(|error| error.lock().ok())
        .and_then(|error| error.clone())
}

#[expect(
    clippy::too_many_arguments,
    reason = "session startup owns optional TUN, profile, identity, protector, pin refresh, cancellation, status, start handshake, and reconfigure commands"
)]
async fn run(
    tun: Option<OwnedFd>,
    profile: Profile,
    identity: MasqueTlsIdentity,
    protector: Arc<dyn SocketProtector>,
    geo_policy: Arc<GeoDirectPolicy>,
    selected_gate: Option<(
        usque_core::vpngate::ServerSummary,
        usque_core::vpngate::PreparedProfile,
    )>,
    cache_dir: PathBuf,
    pin_refresher: Arc<dyn EndpointPinRefresher>,
    cancellation: CancellationToken,
    status: Arc<Mutex<NativeSnapshot>>,
    started: std::sync::mpsc::SyncSender<i32>,
    commands: tokio::sync::mpsc::UnboundedReceiver<RuntimeCommand>,
) {
    let tun = match tun {
        Some(fd) => match AsyncFd::new(TunFd(fd)) {
            Ok(tun) => Some(tun),
            Err(error) => {
                set_error_with_code(
                    &status,
                    "ANDROID_RUNTIME_FAILED",
                    format!("register TUN descriptor: {error}"),
                );
                let _ = started.send(START_TUN_FAILURE);
                return;
            }
        },
        None => None,
    };
    let (gate_tx, mut gate_rx) =
        tokio::sync::watch::channel(usque_core::vpngate::GateStatus::default());
    struct ClearNetworks;
    impl Drop for ClearNetworks {
        fn drop(&mut self) {
            if let Ok(mut networks) = NETWORKS.lock() {
                *networks = None;
            }
        }
    }
    let _clear_networks = ClearNetworks;
    let gate_status = Arc::clone(&status);
    let gate_cancel = cancellation.clone();
    let _gate_watch = tokio_util::task::AbortOnDropHandle::new(tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = gate_cancel.cancelled() => break,
                changed = gate_rx.changed() => {
                    if changed.is_err() { break; }
                    if let Ok(mut snapshot) = gate_status.lock() {
                        if gate_cancel.is_cancelled() { break; }
                        snapshot.vpn_gate = Some(gate_rx.borrow_and_update().clone());
                    }
                }
            }
        }
    }));
    let mut tunnel = {
        let startup = Box::pin(DataPlaneRuntime::start_with_vpngate(
            &profile,
            identity,
            protector,
            Some(pin_refresher),
            geo_policy,
            usque_transport::VpnGateStart {
                selected: selected_gate,
                status: Some(gate_tx.clone()),
                cancellation: cancellation.clone(),
            },
        ));
        tokio::pin!(startup);
        let started_tunnel = tokio::select! {
            biased;
            _ = cancellation.cancelled() => {
                let _ = started.send(START_TRANSPORT_FAILURE);
                return;
            }
            result = &mut startup => result,
        };
        match started_tunnel {
            Ok(tunnel) => tunnel,
            Err(error) => {
                if let Ok(mut snapshot) = status.lock() {
                    snapshot.vpn_gate = Some(gate_tx.borrow().clone());
                }
                set_transport_error(&status, &error);
                let _ = started.send(START_TRANSPORT_FAILURE);
                return;
            }
        }
    };
    if !profile.frontends.tunnel
        && let Err(error) = tunnel.activate_final().await
    {
        set_transport_error(&status, &error);
        tunnel.shutdown().await;
        let _ = started.send(START_TRANSPORT_FAILURE);
        return;
    }
    update_health(&status, &tunnel);
    update_frontends(&status, &tunnel);
    let _ = started.send(START_OK);
    let gate_context = GateContext {
        cache_dir,
        status: gate_tx,
        snapshot: status.clone(),
        cancellation: cancellation.clone(),
        exit_probe: ExitProbeTask::default(),
    };
    spawn_exit_probe(&gate_context, &tunnel, &profile);

    let tun_io = if tun.is_some() {
        match tunnel.attach_tun() {
            Ok(tun_io) => Some(tun_io),
            Err(error) => {
                set_transport_error_on_path(&status, &error, tunnel.path());
                tunnel.shutdown().await;
                return;
            }
        }
    } else {
        None
    };
    run_session(
        tun,
        tun_io,
        tunnel,
        profile,
        cancellation.clone(),
        status.clone(),
        commands,
        gate_context,
    )
    .await;
}

fn spawn_exit_probe(context: &GateContext, tunnel: &DataPlaneRuntime, profile: &Profile) {
    let cancellation = context.exit_probe.begin(&context.cancellation);
    if profile.vpn_gate.enabled && !matches!(tunnel.health(), RuntimeHealth::Connected { .. }) {
        return;
    }
    // Keep both WARP and Gate diagnostics inside the selected final session.
    // OS-routed sockets can observe the physical exit during a TUN handoff;
    // local frontend listeners can also apply explicit direct-routing rules.
    let network = tunnel.internal_network();
    let status = Arc::clone(&context.snapshot);
    let gate_generation = profile
        .vpn_gate
        .enabled
        .then(|| tunnel.gate_status().generation);
    tokio::spawn(async move {
        if let Some(Ok(exit)) = run_probe(&cancellation, network.probe_exit()).await
            && let Ok(mut snapshot) = status.lock()
            && !cancellation.is_cancelled()
            && gate_generation.is_none_or(|generation| {
                snapshot.vpn_gate.as_ref().is_some_and(|gate| {
                    gate.generation == generation
                        && gate.stage == usque_core::vpngate::GateStage::Connected
                })
            })
        {
            apply_exit(&mut snapshot, exit);
        }
    });
}

type OwnedSessionDataEvent = SessionDataEvent<
    Option<io::Result<usize>>,
    Option<Result<bytes::Bytes, TransportError>>,
    Result<(), TransportError>,
    io::Result<()>,
>;

struct GateContext {
    cache_dir: PathBuf,
    status: tokio::sync::watch::Sender<usque_core::vpngate::GateStatus>,
    snapshot: Arc<Mutex<NativeSnapshot>>,
    cancellation: CancellationToken,
    exit_probe: ExitProbeTask,
}

struct PendingPacketIo<'a, F> {
    send: std::pin::Pin<&'a mut Option<F>>,
    write: Option<&'a [u8]>,
    observer: Option<&'a usque_transport::TunWriteObserver>,
}

async fn next_owned_session_data<F: Future<Output = Result<(), TransportError>>>(
    packet_slab: &mut TunReadSlab,
    slot_size: usize,
    tun: Option<&AsyncFd<TunFd>>,
    mut tun_io: Option<&mut TunPacketIo>,
    tick: impl Future,
    pending: PendingPacketIo<'_, F>,
) -> OwnedSessionDataEvent {
    let PendingPacketIo {
        send: pending_send,
        write: pending_write,
        observer,
    } = pending;
    let can_read = pending_send.as_ref().get_ref().is_none();
    let allocated = match packet_slab.prepare(slot_size) {
        Ok(allocated) => allocated,
        Err(error) => return SessionDataEvent::PreparationError(error),
    };
    if allocated && let Some(io) = tun_io.as_deref() {
        io.record_platform_packet_buffer_allocation();
    }
    let tun_read_buffer = packet_slab.read_buffer();
    next_session_data(
        async {
            match tun {
                Some(tun) if can_read => Some(read_packet(tun, tun_read_buffer).await),
                _ => {
                    std::future::pending::<()>().await;
                    None
                }
            }
        },
        async {
            match tun_io.as_deref_mut() {
                Some(io) if pending_write.is_none() => Some(io.receive_packet().await),
                _ => {
                    std::future::pending::<()>().await;
                    None
                }
            }
        },
        wait_pending(pending_send),
        async {
            match (tun, pending_write) {
                (Some(tun), Some(packet)) => write_packet(tun, packet, observer).await,
                _ => std::future::pending().await,
            }
        },
        tick,
    )
    .await
}

#[expect(
    clippy::too_many_arguments,
    reason = "session loop owns optional TUN I/O, MASQUE, profile, cancellation, status, and reconfigure commands"
)]
async fn run_session(
    mut tun: Option<AsyncFd<TunFd>>,
    mut tun_io: Option<TunPacketIo>,
    mut tunnel: DataPlaneRuntime,
    mut profile: Profile,
    cancellation: CancellationToken,
    status: Arc<Mutex<NativeSnapshot>>,
    mut commands: tokio::sync::mpsc::UnboundedReceiver<RuntimeCommand>,
    gate_context: GateContext,
) {
    let mut packet_slab = TunReadSlab::new();
    let mut ticker = interval(Duration::from_secs(1));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut last_sample = Instant::now();
    let mut last_traffic = TrafficSnapshot::default();
    // These slots hold at most one packet per direction. The send future stays
    // pinned in-place across ticks; it is neither re-enqueued nor heap-boxed.
    let mut pending_send = std::pin::pin!(None);
    let mut pending_write: Option<bytes::Bytes> = None;
    let mut write_observer = tun_io.as_ref().and_then(TunPacketIo::write_observer);
    let mut write_sample = None;

    loop {
        let mut completed_write = None;
        tokio::select! {
            biased;
            _ = cancellation.cancelled() => break,
            command = commands.recv() => {
                let Some(command) = command else { break; };
                if matches!(&command, RuntimeCommand::AttachTun { .. } | RuntimeCommand::DetachTun { .. } | RuntimeCommand::RejectFinalNetwork { .. })
                    || matches!(&command, RuntimeCommand::Reconfigure { profile: next, .. } if next.frontends.tunnel != profile.frontends.tunnel || next.vpn_gate != profile.vpn_gate)
                {
                    pending_send.set(None);
                    pending_write = None;
                    write_sample = None;
                }
                handle_runtime_command(
                    command,
                    &mut tunnel,
                    &mut profile,
                    &mut tun,
                    &mut tun_io,
                    &gate_context,
                )
                .await;
                if tunnel.gate_status().stage == usque_core::vpngate::GateStage::Error
                    || (profile.vpn_gate.enabled && status.lock().is_ok_and(|s| s.phase == "error"))
                {
                    break;
                }
                write_observer = tun_io.as_ref().and_then(TunPacketIo::write_observer);
            }
            event = next_owned_session_data(
                &mut packet_slab,
                usize::from(profile.mtu),
                tun.as_ref(),
                tun_io.as_mut(),
                ticker.tick(),
                PendingPacketIo { send: pending_send.as_mut(), write: pending_write.as_deref(), observer: write_observer.as_ref() },
            ) => match event {
                SessionDataEvent::Sent(result) => {
                    pending_send.set(None);
                    if let Err(error) = result {
                        set_transport_error_on_path(&status, &error, tunnel.path());
                        break;
                    }
                }
                SessionDataEvent::Written(result) => {
                    if result.is_ok() {
                        completed_write = pending_write.as_ref().map(bytes::Bytes::len);
                        if let (Some(observer), Some(sample)) = (&write_observer, write_sample.take()) { observer.finish(sample); }
                    }
                    pending_write = None;
                    if let Err(error) = result {
                        set_error(&status, format!("write Android TUN: {error}"));
                        break;
                    }
                }
                SessionDataEvent::PreparationError(error) => {
                    set_error(&status, format!("prepare Android TUN read: {error}"));
                    break;
                }
                SessionDataEvent::TunRead(read) => {
                    let Some(read) = read else { continue; };
                    let Some(io) = tun_io.as_ref() else { continue; };
                    let length = match read {
                        Ok(0) => break,
                        Ok(length) => length,
                        Err(error) => {
                            set_error(&status, format!("read Android TUN: {error}"));
                            break;
                        }
                    };
                    let packet = match packet_slab.take_packet(length) {
                        Ok(packet) => packet,
                        Err(error) => {
                            set_error(&status, format!("own Android TUN packet: {error}"));
                            break;
                        }
                    };
                    pending_send.set(Some(io.start_send_owned_packet(packet)));
                }
                SessionDataEvent::TunnelReceive(received) => {
                    let Some(received) = received else { continue; };
                    if tun.is_none() { continue; }
                    match received {
                        Ok(packet) => {
                            pending_write = Some(packet);
                            write_sample = write_observer.as_ref().map(|o| o.begin());
                        }
                        Err(error) => {
                            set_transport_error_on_path(&status, &error, tunnel.path());
                            break;
                        }
                    }
                }
                SessionDataEvent::Tick => {
                    super::connection_timeline::publish(tunnel.connection_timeline());
                    update_health(&status, &tunnel);
                    update_frontends(&status, &tunnel);
                    if matches!(tunnel.health(), RuntimeHealth::Failed { .. }) {
                        break;
                    }
                    let now = Instant::now();
                    let current = tunnel.statistics();
                    let seconds = now.duration_since(last_sample).as_secs_f64().max(0.001);
                    if let Ok(mut snapshot) = status.lock() {
                        snapshot.data_plane = Some(tunnel.mode());
                        snapshot.l4 = tunnel.l4_snapshot();
                        snapshot.upload_bytes_per_second =
                            rate(current.bytes_sent, last_traffic.bytes_sent, seconds);
                        snapshot.download_bytes_per_second =
                            rate(current.bytes_received, last_traffic.bytes_received, seconds);
                        snapshot.uploaded_bytes = current.bytes_sent;
                        snapshot.downloaded_bytes = current.bytes_received;
                        snapshot.network_quality = usque_transport::PRODUCTION_NETWORK_FEATURES.network_quality_metrics
                            .then(|| super::network_quality_value(&tunnel.network_quality()));
                    }
                    last_sample = now;
                    last_traffic = current;
                }
            }
        }
        // L4 download only: drain already-ready packets without awaiting one
        // direction. One existing pending slot owns any WouldBlock remainder.
        if let (Some(tun), Some(io), Some(observer)) =
            (tun.as_ref(), tun_io.as_mut(), write_observer.as_ref())
        {
            let mut batch = crate::session_pump::ReadyBatch::new();
            if let Some(bytes) = completed_write {
                batch.completed(bytes);
            }
            loop {
                if cancellation.is_cancelled() {
                    break;
                }
                if pending_write.is_none() {
                    match io.try_receive_packet() {
                        Ok(Some(packet)) => {
                            pending_write = Some(packet);
                            write_sample = Some(observer.begin());
                        }
                        Ok(None) => break,
                        Err(error) => {
                            set_transport_error_on_path(&status, &error, tunnel.path());
                            cancellation.cancel();
                            break;
                        }
                    }
                }
                let packet = pending_write.as_ref().expect("pending packet");
                if !batch.allows(packet.len()) {
                    tokio::task::yield_now().await;
                    break;
                }
                let result = tun.try_io(tokio::io::Interest::WRITABLE, |inner| {
                    write_packet_once(inner, packet, Some(observer))
                });
                match result {
                    Ok(()) => {
                        batch.completed(packet.len());
                        pending_write = None;
                        if let Some(sample) = write_sample.take() {
                            observer.finish(sample);
                        }
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                    Err(error) => {
                        set_error(&status, format!("write Android TUN: {error}"));
                        cancellation.cancel();
                        break;
                    }
                }
            }
        }
    }
    super::connection_timeline::publish(tunnel.connection_timeline());
    gate_context.exit_probe.cancel();
    cancellation.cancel();
    tunnel.cancel_immediately();
    if let Ok(mut snapshot) = status.lock() {
        snapshot.finish_runtime(tunnel.gate_status());
    }
    pending_send.set(None);
    drop(pending_write.take());
    drop(tun_io.take());
    // Release the native dup before awaiting backend cleanup. Java retains its
    // own FD for fail-closed recovery unless the user explicitly disconnected.
    drop(tun.take());
    tunnel.shutdown().await;
}

async fn handle_runtime_command(
    command: RuntimeCommand,
    tunnel: &mut DataPlaneRuntime,
    profile: &mut Profile,
    tun: &mut Option<AsyncFd<TunFd>>,
    tun_io: &mut Option<TunPacketIo>,
    gate_context: &GateContext,
) {
    let status = &gate_context.snapshot;
    match command {
        RuntimeCommand::RejectFinalNetwork { reply, cancelled } => {
            if !super::jni_command_abandoned(&cancelled) && profile.vpn_gate.enabled {
                gate_context.exit_probe.cancel();
                detach_tun_locked(tunnel, tun, tun_io);
                tunnel
                    .fail_gate(usque_core::vpngate::GateFailure::Configuration)
                    .await;
                update_health(status, tunnel);
                update_frontends(status, tunnel);
            }
            let _ = reply.send(RECONFIGURE_OK);
        }
        RuntimeCommand::Reconfigure {
            profile: mut next,
            reply,
            cancelled,
        } => {
            // Store updates are next-session preferences, not live CC changes.
            next.congestion_control = profile.congestion_control;
            if super::jni_command_abandoned(&cancelled) {
                let _ = reply.send(START_PLATFORM_FAILURE);
                return;
            }
            if next.proxy.listener_auth_username().is_some() && next.proxy.auth_password.is_none() {
                next.proxy.auth_password = profile.proxy.auth_password.clone();
            }
            let code = match classify_reconfigure(profile, &next) {
                ReconfigureClass::PersistOnly => RECONFIGURE_OK,
                ReconfigureClass::Reject => START_INVALID_PROFILE,
                ReconfigureClass::ColdReconnect => RECONFIGURE_NEED_COLD,
                ReconfigureClass::HotVpnGate => {
                    gate_context.exit_probe.cancel();
                    tunnel.quiesce_final();
                    detach_tun_locked(tunnel, tun, tun_io);
                    if let Ok(mut snapshot) = status.lock() {
                        snapshot.phase = "reconnecting".into();
                        snapshot.exit_ipv4 = None;
                        snapshot.exit_ipv6 = None;
                        snapshot.exit_city = None;
                        snapshot.exit_country = None;
                        snapshot.exit_country_code = None;
                        snapshot.exit_flag_svg = None;
                    }
                    let selected = next.vpn_gate.selection.as_ref().and_then(|selection| {
                        usque_core::vpngate::CatalogueStore::new(&gate_context.cache_dir)
                            .load_selection(selection)
                            .ok()
                    });
                    let policy = load_geo_direct_policy(&next, &gate_context.cache_dir);
                    let result = match (selected, policy) {
                        (selected, Ok(policy)) if selected.is_some() || !next.vpn_gate.enabled => {
                            tunnel
                                .replace_gate(
                                    &next,
                                    selected,
                                    Arc::new(policy),
                                    gate_context.status.clone(),
                                    &gate_context.cancellation,
                                )
                                .await
                        }
                        _ => Err(TransportError::VpnGate(
                            usque_core::vpngate::GateFailure::Configuration,
                        )),
                    };
                    *profile = next;
                    if super::jni_command_abandoned(&cancelled) {
                        tunnel.cancel_immediately();
                        START_PLATFORM_FAILURE
                    } else if let Err(error) = result {
                        let reason = match &error {
                            TransportError::VpnGate(reason) => *reason,
                            _ => usque_core::vpngate::GateFailure::Transport,
                        };
                        tunnel.fail_gate(reason).await;
                        set_transport_error_on_path(status, &error, tunnel.path());
                        START_TRANSPORT_FAILURE
                    } else {
                        update_frontends(status, tunnel);
                        if profile.frontends.tunnel {
                            RECONFIGURE_NEED_ATTACH
                        } else {
                            match tunnel.activate_final().await {
                                Ok(()) => {
                                    update_frontends(status, tunnel);
                                    spawn_exit_probe(gate_context, tunnel, profile);
                                    RECONFIGURE_OK
                                }
                                Err(error) => {
                                    set_transport_error_on_path(status, &error, tunnel.path());
                                    START_TRANSPORT_FAILURE
                                }
                            }
                        }
                    }
                }
                ReconfigureClass::HotSystemProxy => {
                    *profile = next;
                    RECONFIGURE_OK
                }
                ReconfigureClass::HotFrontends => {
                    gate_context.exit_probe.cancel();
                    match tunnel.reconfigure_frontends(&next).await {
                        Ok(()) => {
                            *profile = next;
                            update_frontends(status, tunnel);
                            spawn_exit_probe(gate_context, tunnel, profile);
                            RECONFIGURE_OK
                        }
                        Err(error) => {
                            set_transport_error_on_path(status, &error, tunnel.path());
                            START_TRANSPORT_FAILURE
                        }
                    }
                }
                ReconfigureClass::HotTunnelAttach => {
                    if next.frontends.tunnel && tun.is_none() {
                        RECONFIGURE_NEED_ATTACH
                    } else if !next.frontends.tunnel && tun.is_some() {
                        gate_context.exit_probe.cancel();
                        detach_tun_locked(tunnel, tun, tun_io);
                        *profile = next;
                        spawn_exit_probe(gate_context, tunnel, profile);
                        RECONFIGURE_OK
                    } else {
                        *profile = next;
                        RECONFIGURE_OK
                    }
                }
            };
            let _ = reply.send(code);
        }
        RuntimeCommand::AttachTun {
            tun: owned,
            profile: mut next,
            reply,
            cancelled,
        } => {
            next.congestion_control = profile.congestion_control;
            if super::jni_command_abandoned(&cancelled) {
                let _ = reply.send(START_PLATFORM_FAILURE);
                return;
            }
            if tun.is_some() {
                let _ = reply.send(START_ALREADY_RUNNING);
                return;
            }
            gate_context.exit_probe.cancel();
            if let Err(error) = set_nonblocking(&owned) {
                set_error(
                    status,
                    format!("could not make Android TUN nonblocking: {error}"),
                );
                let _ = reply.send(START_TUN_FAILURE);
                return;
            }
            let attached = match AsyncFd::new(TunFd(owned)) {
                Ok(attached) => attached,
                Err(error) => {
                    set_error(status, format!("register TUN descriptor: {error}"));
                    let _ = reply.send(START_TUN_FAILURE);
                    return;
                }
            };
            let io = match tunnel.attach_tun() {
                Ok(io) => io,
                Err(error) => {
                    set_transport_error_on_path(status, &error, tunnel.path());
                    let _ = reply.send(START_TRANSPORT_FAILURE);
                    return;
                }
            };
            if next.proxy.listener_auth_username().is_some() && next.proxy.auth_password.is_none() {
                next.proxy.auth_password = profile.proxy.auth_password.clone();
            }
            if let Err(error) = tunnel.reconfigure_frontends(&next).await {
                tunnel.detach_tun();
                set_transport_error_on_path(status, &error, tunnel.path());
                let _ = reply.send(START_TRANSPORT_FAILURE);
                return;
            }
            if super::jni_command_abandoned(&cancelled) {
                tunnel.detach_tun();
                let _ = reply.send(START_PLATFORM_FAILURE);
                return;
            }
            *tun = Some(attached);
            *tun_io = Some(io);
            if let Err(error) = tunnel.activate_final().await {
                detach_tun_locked(tunnel, tun, tun_io);
                set_transport_error_on_path(status, &error, tunnel.path());
                let _ = reply.send(START_TRANSPORT_FAILURE);
                return;
            }
            if super::jni_command_abandoned(&cancelled) || reply.send(RECONFIGURE_OK).is_err() {
                detach_tun_locked(tunnel, tun, tun_io);
                return;
            }
            *profile = next;
            update_frontends(status, tunnel);
            spawn_exit_probe(gate_context, tunnel, profile);
        }
        RuntimeCommand::DetachTun { reply, cancelled } => {
            if super::jni_command_abandoned(&cancelled) {
                let _ = reply.send(START_PLATFORM_FAILURE);
                return;
            }
            gate_context.exit_probe.cancel();
            detach_tun_locked(tunnel, tun, tun_io);
            profile.frontends.tunnel = false;
            let _ = reply.send(RECONFIGURE_OK);
        }
    }
}

fn detach_tun_locked(
    tunnel: &mut DataPlaneRuntime,
    tun: &mut Option<AsyncFd<TunFd>>,
    tun_io: &mut Option<TunPacketIo>,
) {
    *tun_io = None;
    tunnel.detach_tun();
    *tun = None;
}

fn apply_exit(snapshot: &mut NativeSnapshot, exit: usque_core::ExitInfo) {
    let location = exit.primary_location().cloned();
    let flag_svg = location.as_ref().and_then(|value| value.flag_svg.clone());
    snapshot.exit_ipv4 = exit.ipv4.map(|address| address.to_string());
    snapshot.exit_ipv6 = exit.ipv6.map(|address| address.to_string());
    snapshot.exit_city = location.as_ref().and_then(|value| value.city.clone());
    snapshot.exit_country = location.as_ref().and_then(|value| value.country.clone());
    snapshot.exit_country_code = location
        .as_ref()
        .and_then(|value| value.country_code.clone());
    snapshot.exit_flag_svg = flag_svg;
}

fn update_health(status: &Arc<Mutex<NativeSnapshot>>, tunnel: &DataPlaneRuntime) {
    let health = tunnel.health();
    let gate = tunnel.gate_status();
    let Ok(mut snapshot) = status.lock() else {
        return;
    };
    let path = health.path();
    snapshot.transport = Some(
        match path.transport {
            Transport::Http3 => "h3",
            Transport::Http2 => "h2",
        }
        .to_owned(),
    );
    snapshot.address_family = Some(
        match path.endpoint_family {
            AddressFamily::Ipv4 => "ipv4",
            AddressFamily::Ipv6 => "ipv6",
        }
        .to_owned(),
    );
    snapshot.reconnect_count = health.reconnect_count();
    match health {
        RuntimeHealth::Connected { path, .. } => {
            snapshot.tunnel_ipv4_available = path.ipv4_available;
            snapshot.tunnel_ipv6_available = path.ipv6_available;
            let connected = usque_core::ConnectionPhase::connected_tunnel(
                path.ipv4_available,
                path.ipv6_available,
                Some(&gate),
            ) == usque_core::ConnectionPhase::Connected;
            snapshot.phase = if connected { "connected" } else { "degraded" }.to_owned();
            snapshot.warning = (!connected).then(|| {
                if path.ipv4_available {
                    "The CONNECT-IP peer is not currently routing IPv6; IPv4 remains protected."
                } else {
                    "The CONNECT-IP peer is not currently routing IPv4; IPv6 remains protected."
                }
                .to_owned()
            });
            snapshot.error_code = None;
            snapshot.failure = None;
        }
        RuntimeHealth::Reconnecting {
            reason, failure, ..
        } => {
            snapshot.tunnel_ipv4_available = false;
            snapshot.tunnel_ipv6_available = false;
            snapshot.phase = "reconnecting".to_owned();
            snapshot.warning = Some(reason);
            snapshot.error_code = Some(failure.code.as_str().to_owned());
            snapshot.failure = Some(NativeFailure::from_failure(&failure));
        }
        RuntimeHealth::Failed {
            message, failure, ..
        } => {
            snapshot.tunnel_ipv4_available = false;
            snapshot.tunnel_ipv6_available = false;
            snapshot.phase = "error".to_owned();
            snapshot.warning = Some(message);
            snapshot.error_code = Some(failure.code.as_str().to_owned());
            snapshot.failure = Some(NativeFailure::from_failure(&failure));
        }
    }
}

fn update_frontends(status: &Arc<Mutex<NativeSnapshot>>, tunnel: &DataPlaneRuntime) {
    if let Ok(mut networks) = NETWORKS.lock() {
        *networks = Some((tunnel.internal_network(), tunnel.warp_internal_network()));
    }
    let Ok(mut snapshot) = status.lock() else {
        return;
    };
    snapshot.active_listeners = tunnel.listeners().iter().map(ToString::to_string).collect();
    snapshot.data_plane = Some(tunnel.mode());
    snapshot.l4 = tunnel.l4_snapshot();
    let gate = tunnel.gate_status();
    snapshot.final_network = gate.network.clone();
    snapshot.vpn_gate = Some(gate);
    snapshot.active_frontends.clear();
    if !tunnel.socks5_listeners().is_empty() {
        snapshot.active_frontends.push("socks5".to_owned());
    }
    if !tunnel.http_listeners().is_empty() {
        snapshot.active_frontends.push("http".to_owned());
    }
}

fn set_error(status: &Arc<Mutex<NativeSnapshot>>, message: String) {
    set_error_with_code(status, "ANDROID_RUNTIME_FAILED", message);
}

fn set_transport_error(status: &Arc<Mutex<NativeSnapshot>>, error: &TransportError) {
    set_transport_failure(status, error, android_transport_failure(error, None));
}

fn set_transport_error_on_path(
    status: &Arc<Mutex<NativeSnapshot>>,
    error: &TransportError,
    path: RuntimePath,
) {
    set_transport_failure(status, error, android_transport_failure(error, Some(path)));
}

fn set_transport_failure(
    status: &Arc<Mutex<NativeSnapshot>>,
    error: &TransportError,
    failure: usque_core::TransportFailure,
) {
    let message = error.to_string();
    if let Ok(mut snapshot) = status.lock() {
        snapshot.phase = "error".to_owned();
        snapshot.warning = Some(message.chars().take(512).collect());
        snapshot.error_code = Some(failure.code.as_str().to_owned());
        snapshot.failure = Some(NativeFailure::from_failure(&failure));
    }
}

fn set_error_with_code(status: &Arc<Mutex<NativeSnapshot>>, code: &str, message: String) {
    if let Ok(mut snapshot) = status.lock() {
        snapshot.phase = "error".to_owned();
        snapshot.warning = Some(message.chars().take(512).collect());
        snapshot.error_code = Some(code.to_owned());
        snapshot.failure = None;
    }
}

fn rate(current: u64, previous: u64, seconds: f64) -> u64 {
    ((current.saturating_sub(previous) as f64) / seconds).clamp(0.0, u64::MAX as f64) as u64
}

struct TunFd(OwnedFd);

impl AsRawFd for TunFd {
    fn as_raw_fd(&self) -> i32 {
        self.0.as_raw_fd()
    }
}

fn set_nonblocking(fd: &OwnedFd) -> io::Result<()> {
    // SAFETY: fd is a live OwnedFd; F_GETFL takes no extra pointer args.
    let flags = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETFL) };
    if flags < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: fd is live; flags is the previous F_GETFL result.
    if unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

async fn read_packet(tun: &AsyncFd<TunFd>, packet: &mut [u8]) -> io::Result<usize> {
    loop {
        let mut ready = tun.readable().await?;
        match ready.try_io(|inner| {
            // SAFETY: fd is readable (AsyncFd); packet buffer is writable for
            // its full length and outlives the read call.
            let read = unsafe {
                libc::read(
                    inner.get_ref().as_raw_fd(),
                    packet.as_mut_ptr().cast(),
                    packet.len(),
                )
            };
            if read < 0 {
                Err(io::Error::last_os_error())
            } else {
                Ok(read as usize)
            }
        }) {
            Ok(result) => return result,
            Err(_) => continue,
        }
    }
}

fn write_packet_once(
    tun: &TunFd,
    packet: &[u8],
    observer: Option<&usque_transport::TunWriteObserver>,
) -> io::Result<()> {
    // SAFETY: the owned nonblocking TUN descriptor and readable packet outlive
    // this synchronous call. IP packet boundaries require exactly one write.
    let written = unsafe { libc::write(tun.as_raw_fd(), packet.as_ptr().cast(), packet.len()) };
    let result = if written < 0 {
        Err(io::Error::last_os_error())
    } else if written as usize != packet.len() {
        Err(io::Error::new(
            io::ErrorKind::WriteZero,
            "Android TUN accepted a partial packet",
        ))
    } else {
        Ok(())
    };
    if let Some(observer) = observer {
        observer.syscall(
            result
                .as_ref()
                .is_err_and(|e| e.kind() == io::ErrorKind::WouldBlock),
        );
    }
    result
}

async fn write_packet(
    tun: &AsyncFd<TunFd>,
    packet: &[u8],
    observer: Option<&usque_transport::TunWriteObserver>,
) -> io::Result<()> {
    loop {
        let mut ready = tun.writable().await?;
        match ready.try_io(|inner| write_packet_once(inner.get_ref(), packet, observer)) {
            Ok(result) => return result,
            Err(_) => continue,
        }
    }
}
