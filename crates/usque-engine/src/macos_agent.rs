//! Root-owned Darwin Agent connection. Credentials never cross this boundary.
use std::{
    fs, io,
    net::Shutdown,
    os::{
        fd::{AsRawFd, OwnedFd},
        unix::{
            fs::{FileTypeExt, MetadataExt, OpenOptionsExt},
            net::UnixStream,
        },
    },
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};
use usque_ipc::{
    agent_v1::{
        self as p, agent_request, agent_response, macos_agent_request::Operation,
        macos_agent_response::Payload,
    },
    darwin_fd, decode_frame, encode_frame,
};
use uuid::Uuid;

pub const SOCKET_PATH: &str = "/Library/PrivilegedHelperTools/com.usque.agent/control.sock";
pub const ENGINE_PATH: &str = "/Library/PrivilegedHelperTools/com.usque.agent/usque-engine";
pub(super) const INSTALL_DIRECTORY: &str = "/Library/PrivilegedHelperTools/com.usque.agent";
pub const PROTOCOL_VERSION: u32 = 1;
pub const RELEASE_APPROVED: bool = false;

/// Also recorded in each signed Mach-O so cross-built packages can verify
/// both helpers without executing them or relying on a mutable bundle plist.
#[used]
// SAFETY: this dedicated read-only Mach-O section contains one boolean byte.
#[unsafe(link_section = "__TEXT,__usque_tun")]
pub static TEST_TUN_ENABLED: bool = cfg!(feature = "macos-test-tun");

#[derive(Debug, thiserror::Error)]
#[error("macOS Agent: {0:?}")]
pub(super) struct AgentFailure(pub p::MacosFailure);
struct Connection {
    socket: UnixStream,
    peer: darwin_fd::PeerIdentity,
    sequence: u64,
}
#[derive(Clone)]
pub struct MacAgentClient {
    connection: Arc<Mutex<Option<Connection>>>,
}
fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "macOS Agent protocol or identity rejected",
    )
}

unsafe extern "C" {
    fn acl_get_fd_np(fd: libc::c_int, kind: libc::c_int) -> *mut std::ffi::c_void;
    fn acl_free(value: *mut std::ffi::c_void) -> libc::c_int;
}
fn no_acl(file: &fs::File) -> io::Result<()> {
    // SAFETY: live FD and Darwin ACL_TYPE_EXTENDED.
    let acl = unsafe { acl_get_fd_np(file.as_raw_fd(), 0x100) };
    if acl.is_null() {
        let error = io::Error::last_os_error();
        return if error.raw_os_error() == Some(libc::ENOENT) {
            Ok(())
        } else {
            Err(error)
        };
    }
    // SAFETY: releases the returned ACL exactly once.
    unsafe {
        acl_free(acl);
    }
    Err(invalid())
}
fn trusted_parents(path: &Path) -> io::Result<()> {
    for parent in path.ancestors().skip(1) {
        let directory = fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(parent)?;
        no_acl(&directory)?;
        let metadata = directory.metadata()?;
        if !metadata.is_dir() || metadata.uid() != 0 || metadata.mode() & 0o022 != 0 {
            return Err(invalid());
        }
    }
    Ok(())
}
fn trusted_socket() -> io::Result<()> {
    trusted_parents(Path::new(SOCKET_PATH))?;
    let metadata = fs::symlink_metadata(SOCKET_PATH)?;
    if !metadata.file_type().is_socket() || metadata.uid() != 0 {
        return Err(invalid());
    }
    Ok(())
}
fn idle_proxy_stream(stream: &UnixStream) -> bool {
    let mut fd = libc::pollfd {
        fd: stream.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    loop {
        // SAFETY: read-only, zero-timeout observation of the owned stream.
        match unsafe { libc::poll(&mut fd, 1, 0) } {
            -1 if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted => continue,
            n => return n == 0,
        }
    }
}
impl MacAgentClient {
    /// The dedicated proxy stream is silent between requests. Any readable
    /// byte or EOF on an idle stream means that its lease can no longer be trusted.
    pub(super) fn proxy_stream_alive(&self) -> bool {
        let guard = match self.connection.try_lock() {
            Ok(guard) => guard,
            Err(std::sync::TryLockError::WouldBlock) => return true,
            Err(_) => return false,
        };
        guard
            .as_ref()
            .is_some_and(|connection| idle_proxy_stream(&connection.socket))
    }
    pub async fn connect() -> io::Result<Self> {
        tokio::task::spawn_blocking(|| {
            trusted_socket()?;
            let socket = UnixStream::connect(SOCKET_PATH)?;
            socket.set_read_timeout(Some(Duration::from_secs(10)))?;
            socket.set_write_timeout(Some(Duration::from_secs(5)))?;
            let peer = darwin_fd::peer_identity(&socket)?;
            if peer.audit_token[1] != 0 || peer.audit_token[3] != 0 {
                return Err(invalid());
            }
            Ok(Self {
                connection: Arc::new(Mutex::new(Some(Connection {
                    socket,
                    peer,
                    sequence: 0,
                }))),
            })
        })
        .await
        .map_err(io::Error::other)?
    }
    pub async fn request(&self, operation: Operation) -> io::Result<(Payload, Option<OwnedFd>)> {
        let connection = self.connection.clone();
        tokio::task::spawn_blocking(move || {
            let mut guard = connection.lock().map_err(|_| invalid())?;
            let result = (|| {
                let connection = guard.as_mut().ok_or_else(invalid)?;
                connection.sequence = connection.sequence.checked_add(1).ok_or_else(invalid)?;
                let dialing = matches!(operation, Operation::CreateSocket(_));
                let descriptor_fence = match &operation {
                    Operation::CreateSocket(request) => {
                        Some(request.fence.clone().ok_or_else(invalid)?)
                    }
                    Operation::OpenTun(fence) => Some(fence.clone()),
                    _ => None,
                };
                let request = p::AgentRequest {
                    request_id: Uuid::new_v4().to_string(),
                    protocol_version: 3,
                    payload: Some(agent_request::Payload::Macos(p::MacosAgentRequest {
                        sequence: connection.sequence,
                        operation: Some(operation),
                    })),
                };
                darwin_fd::send_frame(
                    &connection.socket,
                    &encode_frame(&request).map_err(io::Error::other)?,
                    None::<&OwnedFd>,
                )?;
                let frame = darwin_fd::receive_frame(&connection.socket, connection.peer)?;
                let response: p::AgentResponse =
                    decode_frame(frame.bytes.into()).map_err(io::Error::other)?;
                if response.request_id != request.request_id || response.error.is_some() {
                    return Err(invalid());
                }
                let Some(agent_response::Payload::Macos(response)) = response.payload else {
                    return Err(invalid());
                };
                if response.failure != 0 {
                    if response.payload.is_some() || frame.descriptor.is_some() {
                        return Err(invalid());
                    }
                    let failure =
                        p::MacosFailure::try_from(response.failure).map_err(|_| invalid())?;
                    if dialing && failure == p::MacosFailure::EndpointUnreachable {
                        // The Agent confirmed that this attempt's permit was revoked.
                        // The owner still needs this stream for the next endpoint or Disconnect.
                        return Ok(Err(AgentFailure(failure)));
                    }
                    return Err(io::Error::other(AgentFailure(failure)));
                }
                let payload = response.payload.ok_or_else(invalid)?;
                match (
                    descriptor_fence.as_ref(),
                    &payload,
                    frame.descriptor.is_some(),
                ) {
                    (Some(fence), Payload::Descriptor(info), true)
                        if info.fence.as_ref() == Some(fence)
                            && info.interface_index != 0
                            && (info.lease_id != 0) == dialing => {}
                    (None, Payload::Status(_) | Payload::Empty(_), false) => {}
                    _ => return Err(invalid()),
                }
                Ok(Ok((payload, frame.descriptor)))
            })();
            if result.is_err()
                && let Some(old) = guard.take()
            {
                let _ = old.socket.shutdown(Shutdown::Both);
            }
            result?.map_err(io::Error::other)
        })
        .await
        .map_err(io::Error::other)?
    }
    pub async fn status(&self) -> io::Result<p::MacosPlatformStatus> {
        let (payload, _) = self.request(Operation::Status(p::Empty {})).await?;
        match payload {
            Payload::Status(status)
                if status.protocol_version == PROTOCOL_VERSION
                    && status.agent_version == env!("CARGO_PKG_VERSION")
                    && status.architecture == std::env::consts::ARCH =>
            {
                Ok(status)
            }
            _ => Err(invalid()),
        }
    }
    pub async fn empty(&self, operation: Operation) -> io::Result<()> {
        match self.request(operation).await? {
            (Payload::Empty(_), None) => Ok(()),
            _ => Err(invalid()),
        }
    }
    pub async fn descriptor(
        &self,
        operation: Operation,
    ) -> io::Result<(p::MacosDescriptor, OwnedFd)> {
        match self.request(operation).await? {
            (Payload::Descriptor(info), Some(fd)) => Ok((info, fd)),
            _ => Err(invalid()),
        }
    }
    /// Closing ownership retains protection. Only an explicit Disconnect RPC
    /// asks the Agent to restore the user's system network.
    pub async fn abandon(&self) {
        let connection = self.connection.clone();
        let _ = tokio::task::spawn_blocking(move || {
            if let Ok(mut guard) = connection.lock()
                && let Some(old) = guard.take()
            {
                let _ = old.socket.shutdown(Shutdown::Both);
            }
        })
        .await;
    }
}

fn builds_allow_tun(status: &p::MacosPlatformStatus) -> bool {
    (RELEASE_APPROVED && status.release_approved) || (TEST_TUN_ENABLED && status.test_tun_enabled)
}
pub fn capability(status: &p::MacosPlatformStatus) -> bool {
    builds_allow_tun(status)
        && status.protocol_version == PROTOCOL_VERSION
        && status.agent_version == env!("CARGO_PKG_VERSION")
        && status.architecture == std::env::consts::ARCH
        && status.preflight_passed
        && !status.recovery_pending
        && !status.system_proxy_recovery_pending
        && status.failure == 0
}

/// A retained transaction has a distinct guarded-resume path. It never makes
/// the ordinary fresh-connect capability true while recovery is outstanding.
pub fn resumable(status: &p::MacosPlatformStatus) -> bool {
    builds_allow_tun(status)
        && !status.system_proxy_recovery_pending
        && status.resumable
        && status.protection_retained
        && status.fence.is_some()
        && status.physical_network.is_some()
        && !status.control_api_candidates.is_empty()
        && status.protocol_version == PROTOCOL_VERSION
        && status.agent_version == env!("CARGO_PKG_VERSION")
        && status.architecture == std::env::consts::ARCH
}
pub async fn connection_available() -> bool {
    match MacAgentClient::connect().await {
        Ok(client) => client
            .status()
            .await
            .is_ok_and(|s| capability(&s) || resumable(&s)),
        Err(_) => false,
    }
}
#[async_trait::async_trait]
trait RecoveryClient: Sync {
    async fn recovery_status(&self) -> io::Result<p::MacosPlatformStatus>;
    async fn disconnect_fence(&self, fence: p::MacosFence) -> io::Result<()>;
    async fn recover_proxy(&self) -> io::Result<()>;
}
#[async_trait::async_trait]
impl RecoveryClient for MacAgentClient {
    async fn recover_proxy(&self) -> io::Result<()> {
        self.empty(Operation::RecoverProxy(p::Empty {})).await
    }
    async fn recovery_status(&self) -> io::Result<p::MacosPlatformStatus> {
        self.status().await
    }
    async fn disconnect_fence(&self, fence: p::MacosFence) -> io::Result<()> {
        self.empty(Operation::Disconnect(fence)).await
    }
}
async fn recover_explicit(
    client: &impl RecoveryClient,
    expected: Option<&p::MacosFence>,
) -> io::Result<()> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    loop {
        let status = client.recovery_status().await?;
        if status.system_proxy_recovery_pending {
            client.recover_proxy().await?;
            if tokio::time::Instant::now() >= deadline {
                return Err(invalid());
            }
            continue;
        }
        if clean(&status) {
            return Ok(());
        }
        if status.system_proxy_active && status.fence.is_none() {
            if tokio::time::Instant::now() >= deadline {
                return Err(invalid());
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
            continue;
        }
        let fence = status.fence.as_ref().ok_or_else(invalid)?;
        // An interrupted Resume can commit its successor before the runtime
        // is published. Only the same session's monotonic successor is ours.
        if expected.is_some_and(|old| {
            fence.session_id != old.session_id
                || fence.configuration < old.configuration
                || fence.network < old.network
        }) {
            return Err(invalid());
        }
        if status.recovery_pending
            && matches!(
                p::AgentPhase::try_from(status.phase),
                Ok(p::AgentPhase::RecoveryRequired | p::AgentPhase::Recovering)
            )
        {
            // Agent authenticates this Engine and enforces the retained UID;
            // neither a new PID nor an observed fence permits another user.
            return client.disconnect_fence(fence.clone()).await;
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(invalid());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}
fn retained_guard(status: &p::MacosPlatformStatus, expected: &p::MacosFence) -> bool {
    status.protection_retained
        && status.recovery_pending
        && status.resumable
        && status.fence.as_ref().is_some_and(|f| {
            f.session_id == expected.session_id
                && f.configuration >= expected.configuration
                && f.network >= expected.network
        })
}
pub async fn confirm_retained_protection(expected: &p::MacosFence) -> io::Result<()> {
    let client = MacAgentClient::connect().await?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    loop {
        if retained_guard(&client.status().await?, expected) {
            return Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(io::Error::other(
                "network protection could not be confirmed",
            ));
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}
pub async fn disconnect_retained(fence: p::MacosFence) -> io::Result<()> {
    recover_explicit(&MacAgentClient::connect().await?, Some(&fence)).await
}
fn clean(status: &p::MacosPlatformStatus) -> bool {
    !status.system_proxy_recovery_pending
        && !status.system_proxy_active
        && status.phase == p::AgentPhase::Clean as i32
        && !status.recovery_pending
        && !status.protection_retained
        && status.failure == 0
        && status.fence.is_none()
}
/// The bundled Engine can outlive installation of the pinned Engine. Only
/// the latter is allowed to ask the Agent whether there is anything to clean.
pub async fn check_clean() -> io::Result<()> {
    if clean(&MacAgentClient::connect().await?.status().await?) {
        Ok(())
    } else {
        Err(invalid())
    }
}
pub(super) async fn recover_on_explicit_disconnect(install: &Path) -> io::Result<()> {
    recover_from(install, &std::env::current_exe()?).await
}
pub(super) async fn recover_from(install: &Path, running: &Path) -> io::Result<()> {
    match fs::symlink_metadata(install) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
        Ok(m) if !m.is_dir() => return Err(invalid()),
        Ok(_) => (),
    }
    let engine = install.join("usque-engine");
    if running == engine
        && let Ok(client) = MacAgentClient::connect().await
        && recover_explicit(&client, None).await.is_ok()
    {
        return Ok(());
    }
    // A still-running bundled Engine has no Agent privileges. Delegate a
    // read-only clean check, never someone else's Disconnect. The child's
    // exit code is the acknowledgement; missing, incompatible or retained
    // service state cannot succeed. Production always uses fixed root paths.
    verify_probe(&engine)?;
    run_clean_probe(&engine).await
}
/// A stale Engine cannot authenticate after replacement. For a known lease,
/// a missing socket alone cannot prove that the proxy was restored.
pub(super) async fn confirm_proxy_cleanup(install: &Path) -> io::Result<()> {
    match fs::symlink_metadata(install) {
        Ok(directory) if directory.is_dir() => {
            let pinned = install.join("usque-engine");
            verify_probe(&pinned)?;
            run_clean_probe(&pinned).await
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            use std::io::Read;
            let marker = trusted_file(&install.with_added_extension("clean-uninstall"))?;
            let mut contents = Vec::new();
            marker.take(64).read_to_end(&mut contents)?;
            if contents == b"usque-clean-uninstall-v1\n" {
                Ok(())
            } else {
                Err(invalid())
            }
        }
        Err(error) => Err(error),
        Ok(_) => Err(invalid()),
    }
}
fn verify_probe(engine: &Path) -> io::Result<()> {
    trusted_file(engine).map(drop)
}
fn trusted_file(engine: &Path) -> io::Result<fs::File> {
    trusted_parents(engine)?;
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(engine)?;
    let m = file.metadata()?;
    if !m.is_file() || m.uid() != 0 || m.nlink() != 1 || m.mode() & 0o022 != 0 {
        return Err(invalid());
    }
    no_acl(&file)?;
    Ok(file)
}
async fn run_clean_probe(engine: &Path) -> io::Result<()> {
    use std::process::Stdio;
    let mut child = {
        let _serial = darwin_fd::DESCRIPTOR_EXEC_LOCK
            .lock()
            .map_err(|_| invalid())?;
        tokio::process::Command::new(engine)
            .arg("--macos-agent-check-clean")
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()?
    };
    let status = tokio::time::timeout(Duration::from_secs(12), child.wait()).await??;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(
            "background service cleanup could not be confirmed",
        ))
    }
}

pub fn verify_utun(fd: &OwnedFd, info: &p::MacosDescriptor) -> io::Result<()> {
    if info.lease_id != 0
        || info.protocol != 0
        || !info.local_endpoint.is_empty()
        || !info.remote_endpoint.is_empty()
    {
        return Err(invalid());
    }
    let mut name = [0u8; libc::IFNAMSIZ];
    let mut length = name.len() as libc::socklen_t;
    // SAFETY: live owned control FD and bounded output buffer.
    if unsafe {
        libc::getsockopt(
            fd.as_raw_fd(),
            libc::SYSPROTO_CONTROL,
            2,
            name.as_mut_ptr().cast(),
            &mut length,
        )
    } != 0
    {
        return Err(invalid());
    }
    let bytes = name
        .get(..length as usize)
        .and_then(|s| s.strip_suffix(&[0]))
        .ok_or_else(invalid)?;
    if bytes != info.interface_name.as_bytes()
        || !info
            .interface_name
            .strip_prefix("utun")
            .is_some_and(|s| s.parse::<u32>().is_ok_and(|n| n.to_string() == s))
    {
        return Err(invalid());
    }
    let cname = std::ffi::CString::new(bytes).map_err(|_| invalid())?;
    // SAFETY: terminated kernel name and live descriptor; only read-only queries.
    let index = unsafe { libc::if_nametoindex(cname.as_ptr()) };
    // SAFETY: fcntl only reads flags on this live descriptor.
    let flags = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETFL) };
    if index != info.interface_index
        || info.interface_index == 0
        || flags < 0
        || flags & libc::O_NONBLOCK == 0
    {
        return Err(invalid());
    }
    Ok(())
}

pub fn verify_socket(
    fd: &OwnedFd,
    info: &p::MacosDescriptor,
    remote: std::net::SocketAddr,
    protocol: u8,
) -> io::Result<()> {
    if info.lease_id == 0
        || info.remote_endpoint != remote.to_string()
        || info.protocol != u32::from(protocol)
    {
        return Err(invalid());
    }
    let mut kind = 0i32;
    let mut length = std::mem::size_of_val(&kind) as libc::socklen_t;
    // SAFETY: live owned FD and exact writable getsockopt buffer.
    if unsafe {
        libc::getsockopt(
            fd.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_TYPE,
            (&mut kind as *mut i32).cast(),
            &mut length,
        )
    } != 0
        || kind
            != if protocol == 6 {
                libc::SOCK_STREAM
            } else {
                libc::SOCK_DGRAM
            }
    {
        return Err(invalid());
    }
    let mut index = 0u32;
    length = std::mem::size_of_val(&index) as _;
    let (level, option) = if remote.is_ipv4() {
        (libc::IPPROTO_IP, libc::IP_BOUND_IF)
    } else {
        (libc::IPPROTO_IPV6, libc::IPV6_BOUND_IF)
    };
    // SAFETY: reads the interface binding from the received socket.
    if unsafe {
        libc::getsockopt(
            fd.as_raw_fd(),
            level,
            option,
            (&mut index as *mut u32).cast(),
            &mut length,
        )
    } != 0
        || index != info.interface_index
    {
        return Err(invalid());
    }
    // SAFETY: inspecting flags of a live descriptor does not transfer ownership.
    let flags = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETFL) };
    if flags < 0 || flags & libc::O_NONBLOCK == 0 {
        return Err(invalid());
    }
    Ok(())
}

/// Read-only projection for an Engine restarted without an in-memory owner.
/// The installed service remains authoritative until explicit recovery.
#[derive(Default)]
pub(super) struct ProbeCache(tokio::sync::Mutex<Option<ProbeSample>>);

struct ProbeSample {
    engine: (u64, u64),
    socket: (u64, u64),
    checked: tokio::time::Instant,
    clean: bool,
}
impl ProbeCache {
    // Event subscribers may poll once a second. Share only their displayed
    // observation, never the acknowledgement of an explicit Disconnect/Quit.
    async fn observe(&self, engine: &Path) -> bool {
        let mut last = self.0.lock().await;
        let identity = |path: &Path| fs::symlink_metadata(path).map(|m| (m.dev(), m.ino()));
        let (Ok(bin), Ok(sock)) = (
            identity(engine),
            identity(&engine.with_file_name("control.sock")),
        ) else {
            *last = None;
            return false;
        };
        if let Some(s) = last.as_ref()
            && s.engine == bin
            && s.socket == sock
            && s.checked.elapsed() < Duration::from_secs(5)
        {
            return s.clean;
        }
        let clean = verify_probe(engine).is_ok() && run_clean_probe(engine).await.is_ok();
        *last = Some(ProbeSample {
            engine: bin,
            socket: sock,
            checked: tokio::time::Instant::now(),
            clean,
        });
        clean
    }
    pub(super) async fn invalidate(&self) {
        *self.0.lock().await = None;
    }
}
pub(super) async fn project_recovery(
    snapshot: &mut usque_core::ConnectionSnapshot,
    install: &Path,
    cache: &ProbeCache,
) {
    match fs::symlink_metadata(install) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return,
        Err(_) => {
            project_observation(snapshot, None);
            return;
        }
        Ok(_) => (),
    }
    let engine = install.join("usque-engine");
    if std::env::current_exe().ok().as_deref() == Some(engine.as_path())
        && let Ok(Ok(status)) = tokio::time::timeout(Duration::from_secs(2), async {
            MacAgentClient::connect().await?.status().await
        })
        .await
    {
        project_observation(snapshot, Some(&status));
    } else if !cache.observe(&engine).await {
        project_observation(snapshot, None);
    }
}
fn project_observation(
    snapshot: &mut usque_core::ConnectionSnapshot,
    status: Option<&p::MacosPlatformStatus>,
) {
    if status.is_some_and(clean) {
        return;
    }
    // Startup protects the host before it can publish its data plane. A separate
    // status socket from the same authenticated process can observe this interval.
    if let Some(status) = status
        && status.caller_owns_transaction
        && status.fence.is_some()
        && matches!(
            status.phase(),
            p::AgentPhase::Preparing | p::AgentPhase::Active
        )
        && !status.recovery_pending
        && !status.system_proxy_recovery_pending
        && status.failure == 0
        && matches!(
            snapshot.phase,
            usque_core::ConnectionPhase::Preparing
                | usque_core::ConnectionPhase::ConnectingHttp3
                | usque_core::ConnectionPhase::ConnectingHttp2
                | usque_core::ConnectionPhase::Connected
                | usque_core::ConnectionPhase::Degraded
                | usque_core::ConnectionPhase::Reconnecting
        )
    {
        if status.protection_retained {
            snapshot.kill_switch_state = usque_core::KillSwitchState::Active;
        }
        return;
    }
    snapshot.phase = usque_core::ConnectionPhase::Error;
    snapshot.kill_switch_state = if status.is_some_and(|s| s.protection_retained) {
        usque_core::KillSwitchState::Active
    } else {
        usque_core::KillSwitchState::Error
    };
    snapshot.error = Some(usque_core::ConnectionError {
        code: usque_core::ErrorCode::MacosAgentRecoveryUnconfirmed,
        message:
            "macOS network recovery is incomplete; disconnect or restore the background service"
                .into(),
        retryable: true,
    });
}

/// Non-secret status used by native Settings without opening a user profile.
pub async fn service_status() -> serde_json::Value {
    let result = async { MacAgentClient::connect().await?.status().await }.await;
    match result {
        Ok(status) => {
            serde_json::json!({"installed": true, "compatible": status.system_proxy_supported && status.test_tun_enabled == TEST_TUN_ENABLED, "systemProxySupported": status.system_proxy_supported, "version": status.agent_version,
            "architecture": status.architecture, "preflightPassed": status.preflight_passed,
            "recoveryPending": status.recovery_pending || status.system_proxy_recovery_pending, "protectionRetained": status.protection_retained,
            "testTunEnabled": TEST_TUN_ENABLED && status.test_tun_enabled,
            "tunAvailable": capability(&status), "releaseApproved": status.release_approved && RELEASE_APPROVED,
            "phase": status.phase, "failure": status.failure})
        }
        Err(_) => {
            let present = !matches!(fs::symlink_metadata(INSTALL_DIRECTORY), Err(e) if e.kind() == io::ErrorKind::NotFound);
            serde_json::json!({"installed": present, "compatible": false,
            "recoveryPending": present, "tunAvailable": false, "failure": "service_unavailable"})
        }
    }
}

#[cfg(test)]
mod request_tests;
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quiet_proxy_stream_distinguishes_an_idle_owner_from_agent_exit() {
        let (socket, daemon) = UnixStream::pair().unwrap();
        assert!(idle_proxy_stream(&socket));
        drop(daemon);
        assert!(!idle_proxy_stream(&socket));
        assert!(!clean(&p::MacosPlatformStatus {
            phase: p::AgentPhase::Clean as i32,
            system_proxy_active: true,
            ..Default::default()
        }));
    }
    #[tokio::test]
    async fn a_known_proxy_lease_cannot_accept_an_absent_socket_or_user_written_uninstall_proof() {
        let dir = tempfile::tempdir().unwrap();
        let install = dir.path().join("com.usque.agent");
        assert!(confirm_proxy_cleanup(&install).await.is_err());
        fs::write(
            install.with_added_extension("clean-uninstall"),
            b"usque-clean-uninstall-v1\n",
        )
        .unwrap();
        assert!(confirm_proxy_cleanup(&install).await.is_err());
        fs::create_dir(&install).unwrap();
        assert!(confirm_proxy_cleanup(&install).await.is_err());
    }
    #[tokio::test]
    async fn quit_after_service_install_requires_clean_proof_from_the_pinned_engine() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let install = tmp.path().join("agent");
        let pinned = install.join("usque-engine");
        let running = tmp.path().join("bundle-engine");
        assert!(
            recover_from(&install, &running).await.is_ok(),
            "absent service"
        );
        fs::create_dir(&install).unwrap();
        assert!(
            recover_from(&install, &running).await.is_err(),
            "partial install is not clean"
        );
        for (code, expected) in [(1, false), (0, true)] {
            fs::write(&pinned, format!("#!/bin/sh\n[ \"$#\" = 1 ] && [ \"$1\" = --macos-agent-check-clean ] || exit 42\nexit {code}\n")).unwrap();
            fs::set_permissions(&pinned, fs::Permissions::from_mode(0o700)).unwrap();
            assert!(
                recover_from(&install, &running).await.is_err(),
                "user-writable executable is not a trusted probe"
            );
            assert_eq!(run_clean_probe(&pinned).await.is_ok(), expected);
        }
        fs::set_permissions(&pinned, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(
            run_clean_probe(&pinned).await.is_err(),
            "unexecutable Engine"
        );
    }
    struct FakeRecovery {
        status: p::MacosPlatformStatus,
        disconnected: Mutex<Vec<p::MacosFence>>,
    }
    #[async_trait::async_trait]
    impl RecoveryClient for FakeRecovery {
        async fn recover_proxy(&self) -> io::Result<()> {
            Err(invalid())
        }
        async fn recovery_status(&self) -> io::Result<p::MacosPlatformStatus> {
            Ok(self.status.clone())
        }
        async fn disconnect_fence(&self, fence: p::MacosFence) -> io::Result<()> {
            self.disconnected.lock().unwrap().push(fence);
            Ok(())
        }
    }
    #[tokio::test]
    async fn failed_startup_and_failed_resume_disconnect_the_observed_retained_fence() {
        let old = p::MacosFence {
            session_id: Uuid::new_v4().to_string(),
            configuration: 1,
            network: 1,
        };
        let next = p::MacosFence {
            configuration: 2,
            network: 2,
            ..old.clone()
        };
        let fake = FakeRecovery {
            status: p::MacosPlatformStatus {
                fence: Some(next.clone()),
                recovery_pending: true,
                phase: p::AgentPhase::RecoveryRequired as i32,
                ..Default::default()
            },
            disconnected: Mutex::new(vec![]),
        };
        recover_explicit(&fake, Some(&old)).await.unwrap();
        recover_explicit(&fake, None).await.unwrap();
        assert_eq!(*fake.disconnected.lock().unwrap(), [next.clone(), next]);
        let unrelated = p::MacosFence {
            session_id: Uuid::new_v4().to_string(),
            ..old
        };
        assert!(recover_explicit(&fake, Some(&unrelated)).await.is_err());
        assert_eq!(fake.disconnected.lock().unwrap().len(), 2);
    }
    #[tokio::test]
    async fn missing_recovery_evidence_is_never_a_clean_disconnect() {
        let fake = FakeRecovery {
            status: p::MacosPlatformStatus {
                recovery_pending: true,
                phase: p::AgentPhase::RecoveryRequired as i32,
                ..Default::default()
            },
            disconnected: Mutex::new(vec![]),
        };
        assert!(recover_explicit(&fake, None).await.is_err());
        assert!(fake.disconnected.lock().unwrap().is_empty());
    }
    #[tokio::test]
    async fn lost_disconnect_reply_retries_as_authenticated_clean() {
        let old = p::MacosFence {
            session_id: Uuid::new_v4().to_string(),
            configuration: 1,
            network: 1,
        };
        let fake = FakeRecovery {
            status: p::MacosPlatformStatus {
                phase: p::AgentPhase::Clean as i32,
                ..Default::default()
            },
            disconnected: Mutex::new(vec![]),
        };
        recover_explicit(&fake, Some(&old)).await.unwrap();
        assert!(fake.disconnected.lock().unwrap().is_empty());
    }
    #[test]
    fn protection_requires_actual_guard_confirmation_not_just_retained_phase() {
        let fence = p::MacosFence {
            session_id: Uuid::new_v4().to_string(),
            configuration: 1,
            network: 1,
        };
        let mut status = p::MacosPlatformStatus {
            fence: Some(fence.clone()),
            recovery_pending: true,
            resumable: true,
            ..Default::default()
        };
        assert!(!retained_guard(&status, &fence));
        status.protection_retained = true;
        assert!(retained_guard(&status, &fence));
        status.resumable = false;
        assert!(!retained_guard(&status, &fence));
    }
    #[test]
    fn dialing_engine_projects_its_owned_transaction_as_startup() {
        use usque_core::{ConnectionPhase, ConnectionSnapshot, KillSwitchState};
        let status = p::MacosPlatformStatus {
            caller_owns_transaction: true,
            phase: p::AgentPhase::Preparing as i32,
            protection_retained: true,
            fence: Some(p::MacosFence::default()),
            ..Default::default()
        };
        let starting = ConnectionSnapshot {
            phase: ConnectionPhase::ConnectingHttp3,
            ..Default::default()
        };
        for agent_phase in [p::AgentPhase::Preparing, p::AgentPhase::Active] {
            let mut snapshot = starting.clone();
            project_observation(
                &mut snapshot,
                Some(&p::MacosPlatformStatus {
                    phase: agent_phase as i32,
                    ..status.clone()
                }),
            );
            assert_eq!(snapshot.phase, starting.phase);
            assert!(snapshot.error.is_none());
            assert_eq!(snapshot.kill_switch_state, KillSwitchState::Active);
        }
        for unhealthy in [
            p::MacosPlatformStatus {
                caller_owns_transaction: false,
                ..status.clone()
            },
            p::MacosPlatformStatus {
                recovery_pending: true,
                ..status.clone()
            },
            p::MacosPlatformStatus {
                system_proxy_recovery_pending: true,
                ..status.clone()
            },
            p::MacosPlatformStatus {
                phase: p::AgentPhase::RecoveryRequired as i32,
                ..status.clone()
            },
            p::MacosPlatformStatus {
                failure: p::MacosFailure::OwnershipConflict as i32,
                ..status
            },
        ] {
            let mut snapshot = starting.clone();
            project_observation(&mut snapshot, Some(&unhealthy));
            assert_eq!(snapshot.phase, ConnectionPhase::Error);
        }
    }

    #[test]
    fn restarted_engine_projects_pending_or_unknown_recovery_as_error() {
        let mut snapshot = usque_core::ConnectionSnapshot::default();
        project_observation(&mut snapshot, None);
        assert_eq!(snapshot.phase, usque_core::ConnectionPhase::Error);
        assert_eq!(
            snapshot.kill_switch_state,
            usque_core::KillSwitchState::Error
        );
        project_observation(
            &mut snapshot,
            Some(&p::MacosPlatformStatus {
                recovery_pending: true,
                protection_retained: true,
                ..Default::default()
            }),
        );
        assert_eq!(
            snapshot.kill_switch_state,
            usque_core::KillSwitchState::Active
        );
    }
    #[test]
    fn an_installed_agent_cannot_override_release_acceptance() {
        let status = p::MacosPlatformStatus {
            release_approved: true,
            preflight_passed: true,
            protocol_version: PROTOCOL_VERSION,
            agent_version: env!("CARGO_PKG_VERSION").into(),
            architecture: std::env::consts::ARCH.into(),
            ..Default::default()
        };
        assert!(!capability(&status));
    }
    #[test]
    fn test_tun_requires_matching_builds_and_a_healthy_agent() {
        let mut status = p::MacosPlatformStatus {
            test_tun_enabled: true,
            preflight_passed: true,
            protocol_version: PROTOCOL_VERSION,
            agent_version: env!("CARGO_PKG_VERSION").into(),
            architecture: std::env::consts::ARCH.into(),
            ..Default::default()
        };
        assert_eq!(capability(&status), cfg!(feature = "macos-test-tun"));
        status.preflight_passed = false;
        assert!(!capability(&status));
        status.preflight_passed = true;
        status.system_proxy_recovery_pending = true;
        assert!(!capability(&status));
        status.system_proxy_recovery_pending = false;
        status.recovery_pending = true;
        assert!(!capability(&status));
        status.recovery_pending = false;
        status.test_tun_enabled = false;
        assert!(!capability(&status));
        status.test_tun_enabled = true;
        status.protocol_version += 1;
        assert!(!capability(&status));
    }
    #[test]
    fn ordinary_descriptor_cannot_impersonate_utun() {
        let (socket, _peer) = std::os::unix::net::UnixDatagram::pair().unwrap();
        socket.set_nonblocking(true).unwrap();
        let info = p::MacosDescriptor {
            interface_name: "utun4".into(),
            interface_index: 4,
            ..Default::default()
        };
        assert!(verify_utun(&socket.into(), &info).is_err());
    }
    #[test]
    fn ordinary_unbound_socket_cannot_impersonate_an_agent_descriptor() {
        let socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let fd: OwnedFd = socket.into();
        let remote: std::net::SocketAddr = "127.0.0.1:53".parse().unwrap();
        let info = p::MacosDescriptor {
            lease_id: 1,
            interface_index: 1,
            remote_endpoint: remote.to_string(),
            protocol: 17,
            ..Default::default()
        };
        assert!(verify_socket(&fd, &info, remote, 17).is_err());
    }
}
