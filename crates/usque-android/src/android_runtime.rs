use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::sync::{
    Arc, Mutex, OnceLock,
    atomic::{AtomicBool, Ordering},
};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use tokio::io::unix::AsyncFd;
use tokio::time::{MissedTickBehavior, interval};
use tokio_util::sync::CancellationToken;
use usque_core::{AddressFamily, IpSbProbe, Transport, WarpIdentity};
use usque_core::{ReconfigureClass, classify_reconfigure};
use usque_transport::{
    EndpointPinRefresher, MasqueRuntime, MasqueTunIo, RuntimeHealth, TrafficSnapshot,
    TransportError,
};

use super::{
    AndroidEndpointPinRefresher, AndroidSocketProtector, MasqueTlsIdentity, NativeSnapshot,
    Profile, RECONFIGURE_NEED_ATTACH, RECONFIGURE_NEED_COLD, RECONFIGURE_NOT_RUNNING,
    RECONFIGURE_OK, START_ALREADY_RUNNING, START_INVALID_PROFILE, START_OK, START_PLATFORM_FAILURE,
    START_TRANSPORT_FAILURE, START_TUN_FAILURE, SocketProtector,
};

static ENGINE: OnceLock<Mutex<Option<EngineHandle>>> = OnceLock::new();
static LAST_START_ERROR: OnceLock<Mutex<Option<NativeSnapshot>>> = OnceLock::new();

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
}

struct EngineHandle {
    cancellation: CancellationToken,
    status: Arc<Mutex<NativeSnapshot>>,
    protector: Arc<AndroidSocketProtector>,
    commands: tokio::sync::mpsc::UnboundedSender<RuntimeCommand>,
    thread: JoinHandle<()>,
}

pub(super) fn start(
    tun_file_descriptor: i32,
    profile: Profile,
    identity: WarpIdentity,
    protector: Arc<AndroidSocketProtector>,
) -> i32 {
    spawn_runtime(
        "usque-vpn",
        Some(tun_file_descriptor),
        profile,
        identity,
        protector,
    )
}

pub(super) fn start_proxy(
    profile: Profile,
    identity: WarpIdentity,
    protector: Arc<AndroidSocketProtector>,
) -> i32 {
    spawn_runtime("usque-proxy", None, profile, identity, protector)
}

fn spawn_runtime(
    thread_name: &str,
    tun_file_descriptor: Option<i32>,
    profile: Profile,
    identity: WarpIdentity,
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

    let cancellation = CancellationToken::new();
    let status = Arc::new(Mutex::new(NativeSnapshot::preparing()));
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
        thread,
    });
    drop(slot);

    let result = match started_rx.recv_timeout(Duration::from_secs(30)) {
        Ok(result) => result,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            set_error_with_code(
                &status,
                "ANDROID_START_TIMEOUT",
                "The Android native runtime did not start within 30 seconds.".to_owned(),
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

pub(super) fn stop() {
    clear_last_start_error();
    let Some(engine) = ENGINE.get() else {
        return;
    };
    let handle = engine.lock().ok().and_then(|mut slot| slot.take());
    if let Some(handle) = handle {
        handle.cancellation.cancel();
        let _ = handle.thread.join();
    }
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
        handle
            .protector
            .network_generation
            .store(generation, Ordering::Release);
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
    super::wait_jni_command_reply(reply_rx, &cancelled, Duration::from_secs(30))
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
    let mut tunnel = {
        let startup =
            MasqueRuntime::start_with_refresh(&profile, identity, protector, Some(pin_refresher));
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
                set_transport_error(&status, &error);
                let _ = started.send(START_TRANSPORT_FAILURE);
                return;
            }
        }
    };
    update_health(&status, tunnel.health());
    if let Ok(mut snapshot) = status.lock() {
        snapshot.active_listeners = tunnel.listeners().iter().map(ToString::to_string).collect();
    }
    let _ = started.send(START_OK);
    spawn_exit_probe(&status, &tunnel, &profile, tun.is_some());

    let tun_io = if tun.is_some() {
        match tunnel.attach_tun() {
            Ok(tun_io) => Some(tun_io),
            Err(error) => {
                set_transport_error(&status, &error);
                tunnel.shutdown().await;
                return;
            }
        }
    } else {
        None
    };
    run_session(tun, tun_io, tunnel, profile, cancellation, status, commands).await;
}

fn spawn_exit_probe(
    status: &Arc<Mutex<NativeSnapshot>>,
    tunnel: &MasqueRuntime,
    profile: &Profile,
    has_tun: bool,
) {
    let probe = if has_tun {
        IpSbProbe::new().ok()
    } else {
        let listener = tunnel
            .listeners()
            .iter()
            .copied()
            .find(|address| address.ip().is_loopback());
        let listener_auth = profile.proxy.listener_credentials().ok().flatten();
        listener.and_then(|listener| {
            if profile.frontends.socks5 {
                IpSbProbe::through_socks_with_auth(listener, listener_auth.as_ref()).ok()
            } else if profile.frontends.http {
                IpSbProbe::through_http_with_auth(listener, listener_auth.as_ref()).ok()
            } else {
                None
            }
        })
    };
    if let Some(probe) = probe {
        tokio::spawn(populate_exit(Arc::clone(status), probe));
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "session loop owns optional TUN I/O, MASQUE, profile, cancellation, status, and reconfigure commands"
)]
async fn run_session(
    mut tun: Option<AsyncFd<TunFd>>,
    mut tun_io: Option<MasqueTunIo>,
    mut tunnel: MasqueRuntime,
    mut profile: Profile,
    cancellation: CancellationToken,
    status: Arc<Mutex<NativeSnapshot>>,
    mut commands: tokio::sync::mpsc::UnboundedReceiver<RuntimeCommand>,
) {
    let mut packet = vec![0u8; 65_535];
    let mut ticker = interval(Duration::from_secs(1));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut last_sample = Instant::now();
    let mut last_traffic = TrafficSnapshot::default();

    loop {
        tokio::select! {
            biased;
            _ = cancellation.cancelled() => break,
            command = commands.recv() => {
                let Some(command) = command else { break; };
                handle_runtime_command(
                    command,
                    &mut tunnel,
                    &mut profile,
                    &mut tun,
                    &mut tun_io,
                    &status,
                )
                .await;
            }
            read = async {
                match tun.as_ref() {
                    Some(tun) => Some(read_packet(tun, &mut packet).await),
                    None => {
                        std::future::pending::<()>().await;
                        None
                    }
                }
            } => {
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
                if let Err(error) = io.send_packet(&packet[..length]).await {
                    set_error(&status, error.to_string());
                    break;
                }
            }
            received = async {
                match tun_io.as_mut() {
                    Some(io) => Some(io.receive_packet().await),
                    None => {
                        std::future::pending::<()>().await;
                        None
                    }
                }
            } => {
                let Some(received) = received else { continue; };
                let Some(tun) = tun.as_ref() else { continue; };
                match received {
                    Ok(packet) => {
                        if let Err(error) = write_packet(tun, &packet).await {
                            set_error(&status, format!("write Android TUN: {error}"));
                            break;
                        }
                    }
                    Err(error) => {
                        set_error(&status, error.to_string());
                        break;
                    }
                }
            }
            _ = ticker.tick() => {
                update_health(&status, tunnel.health());
                if let Ok(mut snapshot) = status.lock() {
                    snapshot.active_listeners =
                        tunnel.listeners().iter().map(ToString::to_string).collect();
                }
                let now = Instant::now();
                let current = tunnel.statistics();
                let seconds = now.duration_since(last_sample).as_secs_f64().max(0.001);
                if let Ok(mut snapshot) = status.lock() {
                    snapshot.upload_bytes_per_second =
                        rate(current.bytes_sent, last_traffic.bytes_sent, seconds);
                    snapshot.download_bytes_per_second =
                        rate(current.bytes_received, last_traffic.bytes_received, seconds);
                    snapshot.uploaded_bytes = current.bytes_sent;
                    snapshot.downloaded_bytes = current.bytes_received;
                }
                last_sample = now;
                last_traffic = current;
            }
        }
    }
    tunnel.shutdown().await;
}

async fn handle_runtime_command(
    command: RuntimeCommand,
    tunnel: &mut MasqueRuntime,
    profile: &mut Profile,
    tun: &mut Option<AsyncFd<TunFd>>,
    tun_io: &mut Option<MasqueTunIo>,
    status: &Arc<Mutex<NativeSnapshot>>,
) {
    match command {
        RuntimeCommand::Reconfigure {
            profile: mut next,
            reply,
            cancelled,
        } => {
            if super::jni_command_abandoned(&cancelled) {
                let _ = reply.send(START_PLATFORM_FAILURE);
                return;
            }
            if next.proxy.listener_auth_username().is_some() && next.proxy.auth_password.is_none() {
                next.proxy.auth_password = profile.proxy.auth_password.clone();
            }
            let code = match classify_reconfigure(profile, &next) {
                ReconfigureClass::Reject => START_INVALID_PROFILE,
                ReconfigureClass::ColdReconnect => RECONFIGURE_NEED_COLD,
                ReconfigureClass::HotSystemProxy => {
                    *profile = next;
                    RECONFIGURE_OK
                }
                ReconfigureClass::HotFrontends => match tunnel.reconfigure_frontends(&next).await {
                    Ok(()) => {
                        *profile = next;
                        if let Ok(mut snapshot) = status.lock() {
                            snapshot.active_listeners =
                                tunnel.listeners().iter().map(ToString::to_string).collect();
                        }
                        RECONFIGURE_OK
                    }
                    Err(error) => {
                        set_transport_error(status, &error);
                        START_TRANSPORT_FAILURE
                    }
                },
                ReconfigureClass::HotTunnelAttach => {
                    if next.frontends.tunnel && tun.is_none() {
                        RECONFIGURE_NEED_ATTACH
                    } else if !next.frontends.tunnel && tun.is_some() {
                        detach_tun_locked(tunnel, tun, tun_io);
                        *profile = next;
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
            if super::jni_command_abandoned(&cancelled) {
                let _ = reply.send(START_PLATFORM_FAILURE);
                return;
            }
            if tun.is_some() {
                let _ = reply.send(START_ALREADY_RUNNING);
                return;
            }
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
                    set_transport_error(status, &error);
                    let _ = reply.send(START_TRANSPORT_FAILURE);
                    return;
                }
            };
            if next.proxy.listener_auth_username().is_some() && next.proxy.auth_password.is_none() {
                next.proxy.auth_password = profile.proxy.auth_password.clone();
            }
            if let Err(error) = tunnel.reconfigure_frontends(&next).await {
                tunnel.detach_tun();
                set_transport_error(status, &error);
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
            if super::jni_command_abandoned(&cancelled) || reply.send(RECONFIGURE_OK).is_err() {
                detach_tun_locked(tunnel, tun, tun_io);
                return;
            }
            *profile = next;
            if let Ok(mut snapshot) = status.lock() {
                snapshot.active_listeners =
                    tunnel.listeners().iter().map(ToString::to_string).collect();
            }
        }
        RuntimeCommand::DetachTun { reply, cancelled } => {
            if super::jni_command_abandoned(&cancelled) {
                let _ = reply.send(START_PLATFORM_FAILURE);
                return;
            }
            detach_tun_locked(tunnel, tun, tun_io);
            profile.frontends.tunnel = false;
            let _ = reply.send(RECONFIGURE_OK);
        }
    }
}

fn detach_tun_locked(
    tunnel: &mut MasqueRuntime,
    tun: &mut Option<AsyncFd<TunFd>>,
    tun_io: &mut Option<MasqueTunIo>,
) {
    *tun_io = None;
    tunnel.detach_tun();
    *tun = None;
}

async fn populate_exit(status: Arc<Mutex<NativeSnapshot>>, probe: IpSbProbe) {
    let Ok(exit) = probe.probe().await else {
        return;
    };
    let location = exit.primary_location().cloned();
    let flag_svg = location.as_ref().and_then(|value| value.flag_svg.clone());
    if let Ok(mut snapshot) = status.lock() {
        snapshot.exit_ipv4 = exit.ipv4.map(|address| address.to_string());
        snapshot.exit_ipv6 = exit.ipv6.map(|address| address.to_string());
        snapshot.exit_city = location.as_ref().and_then(|value| value.city.clone());
        snapshot.exit_country = location.as_ref().and_then(|value| value.country.clone());
        snapshot.exit_country_code = location
            .as_ref()
            .and_then(|value| value.country_code.clone());
        snapshot.exit_flag_svg = flag_svg;
    }
}

fn update_health(status: &Arc<Mutex<NativeSnapshot>>, health: RuntimeHealth) {
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
            let dual_stack = path.ipv4_available && path.ipv6_available;
            snapshot.phase = if dual_stack { "connected" } else { "degraded" }.to_owned();
            snapshot.warning = (!dual_stack).then(|| {
                if path.ipv4_available {
                    "The CONNECT-IP peer is not currently routing IPv6; IPv4 remains protected."
                } else {
                    "The CONNECT-IP peer is not currently routing IPv4; IPv6 remains protected."
                }
                .to_owned()
            });
            snapshot.error_code = None;
        }
        RuntimeHealth::Reconnecting { reason, .. } => {
            snapshot.phase = "reconnecting".to_owned();
            snapshot.warning = Some(reason);
            snapshot.error_code = None;
        }
        RuntimeHealth::Failed { message, .. } => {
            snapshot.phase = "error".to_owned();
            snapshot.warning = Some(message);
            snapshot.error_code = Some("MASQUE_CONNECT_FAILED".to_owned());
        }
    }
}

fn set_error(status: &Arc<Mutex<NativeSnapshot>>, message: String) {
    set_error_with_code(status, "ANDROID_RUNTIME_FAILED", message);
}

fn set_transport_error(status: &Arc<Mutex<NativeSnapshot>>, error: &TransportError) {
    let message = error.to_string();
    let code = match error {
        TransportError::SocksListener { .. } | TransportError::HttpProxyListener { .. } => {
            "PROXY_LISTEN_FAILED"
        }
        TransportError::SocketProtection(_) => "ANDROID_SOCKET_ROUTE_FAILED",
        TransportError::EndpointFamilyUnavailable(_) => "ENDPOINT_FAMILY_UNAVAILABLE",
        TransportError::EndpointTimeout(_) => "MASQUE_ENDPOINT_TIMEOUT",
        TransportError::AllEndpointsFailed(_) => "MASQUE_ALL_ENDPOINTS_FAILED",
        _ if message.contains("Android network")
            || message.contains("endpoint socket")
            || message.contains("VpnService.protect") =>
        {
            "ANDROID_SOCKET_ROUTE_FAILED"
        }
        _ => "MASQUE_CONNECT_FAILED",
    };
    set_error_with_code(status, code, message);
}

fn set_error_with_code(status: &Arc<Mutex<NativeSnapshot>>, code: &str, message: String) {
    if let Ok(mut snapshot) = status.lock() {
        snapshot.phase = "error".to_owned();
        snapshot.warning = Some(message.chars().take(512).collect());
        snapshot.error_code = Some(code.to_owned());
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

async fn write_packet(tun: &AsyncFd<TunFd>, packet: &[u8]) -> io::Result<()> {
    loop {
        let mut ready = tun.writable().await?;
        match ready.try_io(|inner| {
            // SAFETY: fd is writable (AsyncFd); packet is a valid readable
            // buffer for its full length and outlives the write call.
            let written = unsafe {
                libc::write(
                    inner.get_ref().as_raw_fd(),
                    packet.as_ptr().cast(),
                    packet.len(),
                )
            };
            if written < 0 {
                Err(io::Error::last_os_error())
            } else if written as usize != packet.len() {
                Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "Android TUN accepted a partial packet",
                ))
            } else {
                Ok(())
            }
        }) {
            Ok(result) => return result,
            Err(_) => continue,
        }
    }
}
