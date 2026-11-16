//! Authenticated Darwin service. The connected Engine owns a single session;
//! disconnecting IPC retains a Kill Switch, including during service shutdown.
use super::{
    auth::CallerPolicy,
    backend::NativeBackend,
    journal::{JournalStore, SecureStore},
    system_proxy::{Receipt, SystemProxy},
    transaction::{Coordinator, Fence, Owner, Phase, Store},
};
use std::{
    fs, io,
    os::{
        fd::{AsRawFd, OwnedFd},
        unix::{
            fs::{FileTypeExt, MetadataExt, PermissionsExt},
            net::{UnixListener, UnixStream},
        },
    },
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
    time::Duration,
};
use usque_ipc::{
    agent_v1::{
        self as p, agent_request, agent_response, macos_agent_request::Operation as Op,
        macos_agent_response::Payload as Reply,
    },
    darwin_fd, decode_frame, encode_frame,
};
use uuid::Uuid;
type State = Coordinator<JournalStore, NativeBackend>;
struct Service {
    proxy: Option<SystemProxy>,
    state: Option<State>,
    failure: p::MacosFailure,
    owner_connection: Option<Uuid>,
    network_generation: u64,
}
fn denied() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "macOS Agent request rejected",
    )
}
fn require_root() -> io::Result<()> {
    // SAFETY: read-only process credential query.
    if unsafe { libc::geteuid() } != 0 {
        return Err(denied());
    }
    Ok(())
}
fn open_state() -> io::Result<State> {
    if !super::journal::SecureStore::<Initialization>::open_root(INITIALIZATION_PATH)?
        .load()?
        .is_some_and(|i| i.complete)
    {
        return Err(io::Error::other("Agent initialization incomplete"));
    }
    let mut store = JournalStore::open()?;
    let mut state = store.load()?;
    let backend = NativeBackend::open()?;
    if let Some(transaction) = state.as_mut()
        && promote_transition(transaction, backend.receipts())
    {
        store.save(Some(transaction))?;
    }
    validate_pair(state.as_ref(), backend.receipts())?;
    Coordinator::new(store, backend, state)
}
fn promote_transition(
    transaction: &mut super::transaction::Transaction,
    native: Option<&super::resources::NativeReceipts>,
) -> bool {
    let Some(next) = transaction.transition.clone() else {
        return false;
    };
    if native.is_none_or(|r| r.fence != next.fence) {
        return false;
    }
    transaction.fence = next.fence;
    transaction.owner = next.owner;
    transaction.kill_switch = next.kill_switch;
    transaction.transition = None;
    transaction.records.truncate(1);
    transaction.phase = Phase::RecoveryRequired;
    true
}
fn validate_pair(
    t: Option<&super::transaction::Transaction>,
    native: Option<&super::resources::NativeReceipts>,
) -> io::Result<()> {
    use super::transaction::{Step, StepState};
    match (t, native) {
        (None, None) => Ok(()),
        (Some(t), Some(n)) if t.fence == n.fence => Ok(()),
        (Some(t), None)
            if t.records.is_empty()
                || (t.records.len() == 1 && t.records[0].state == StepState::Intended)
                || (matches!(t.phase, Phase::Recovering | Phase::RecoveryRequired)
                    && t.records
                        .iter()
                        .all(|r| r.step == Step::Protection || r.state == StepState::Restored)) =>
        {
            Ok(())
        }
        _ => Err(io::Error::other("native and transaction journals disagree")),
    }
}
pub fn validate_only() -> io::Result<()> {
    require_root()?;
    let _single = super::journal::service_lock()?;
    CallerPolicy::load()?;
    SystemProxy::open()?;
    open_state().map(|_| ())
}
pub fn recover_for_maintenance() -> io::Result<()> {
    require_root()?;
    let marker = Path::new(PROXY_INITIALIZATION_PATH);
    let receipt = Path::new(super::system_proxy::PATH);
    // The incumbent's own validator checks all records its version requires.
    // Run before taking the exclusive lock, which its validator also needs.
    let legacy = validated_pre_proxy_install(marker, receipt, || {
        use super::command::{self, NativeRunner, Program};
        command::checked(
            &mut NativeRunner,
            Program::InstalledAgent,
            &command::arguments(&["--validate-only"]),
            None,
        )
        .map(|_| ())
    })?;
    let _single = super::journal::service_lock()?;
    if !legacy || !proxy_records_absent(marker, receipt)? {
        SystemProxy::open()?
            .restore()
            .map_err(|_| io::Error::other("system proxy recovery incomplete"))?;
    }
    open_state()?.recover_as_administrator()
}
fn proxy_records_absent(marker: &Path, receipt: &Path) -> io::Result<bool> {
    for file in [marker, receipt] {
        match fs::symlink_metadata(file) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
            Ok(_) => return Ok(false),
        }
    }
    Ok(true)
}
fn validated_pre_proxy_install(
    marker: &Path,
    receipt: &Path,
    validate: impl FnOnce() -> io::Result<()>,
) -> io::Result<bool> {
    if !proxy_records_absent(marker, receipt)? {
        return Ok(false);
    }
    validate()?;
    Ok(true)
}
/// Only the authorized installer may create the initial pair of clean records.
/// Existing records must be readable and clean; corrupt/missing recovery
/// evidence from an existing installation is never silently initialized.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Initialization {
    complete: bool,
}
impl super::journal::Evidence for Initialization {
    fn validate(&self) -> io::Result<()> {
        Ok(())
    }
}
const INITIALIZATION_PATH: &str = "/Library/Application Support/Usque/Agent/initialization.json";
const PROXY_INITIALIZATION_PATH: &str =
    "/Library/Application Support/Usque/Agent/system-proxy-initialization.json";
pub fn initialize_state() -> io::Result<()> {
    require_root()?;
    let _single = super::journal::service_lock()?;
    let mut intent = super::journal::SecureStore::<Initialization>::open_root(INITIALIZATION_PATH)?;
    let initialized = match intent.load() {
        Ok(Some(value)) => value.complete,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            if Path::new(super::JOURNAL_PATH).exists() || Path::new(super::RESOURCE_PATH).exists() {
                return Err(io::Error::other("existing evidence cannot be initialized"));
            }
            intent.save_value(Some(&Initialization { complete: false }))?;
            false
        }
        _ => return Err(io::Error::other("invalid initialization evidence")),
    };
    let mut state = JournalStore::open()?;
    let mut native = super::journal::SecureStore::<super::resources::NativeReceipts>::open_root(
        super::RESOURCE_PATH,
    )?;
    initialize_clean(&mut state, initialized)?;
    initialize_clean(&mut native, initialized)?;
    let mut marker = SecureStore::<Initialization>::open_root(PROXY_INITIALIZATION_PATH)?;
    let proxy_initialized = match marker.load() {
        Ok(Some(record)) if record.complete => true,
        Err(error) if error.kind() == io::ErrorKind::NotFound => false,
        _ => {
            return Err(io::Error::other(
                "invalid system proxy initialization evidence",
            ));
        }
    };
    initialize_clean(
        &mut SecureStore::<Receipt>::open_root(super::system_proxy::PATH)?,
        proxy_initialized,
    )?;
    marker.save_value(Some(&Initialization { complete: true }))?;
    intent.save_value(Some(&Initialization { complete: true }))
}
fn initialize_clean<T: super::journal::Evidence>(
    store: &mut super::journal::SecureStore<T>,
    initialized: bool,
) -> io::Result<()> {
    match store.load() {
        Ok(None) => Ok(()),
        Err(e) if !initialized && e.kind() == io::ErrorKind::NotFound => store.save_value(None),
        _ => Err(io::Error::other(
            "initialization requires complete clean evidence",
        )),
    }
}
/// Root installer probe; no network mutation and no ordinary-user bypass.
pub fn readiness() -> io::Result<()> {
    require_root()?;
    CallerPolicy::load()?;
    super::journal::verify_root_parents(Path::new(super::SOCKET_PATH))?;
    let stream = UnixStream::connect(super::SOCKET_PATH)?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    let peer = darwin_fd::peer_identity(&stream)?;
    if peer.audit_token[1] != 0 || peer.audit_token[3] != 0 {
        return Err(denied());
    }
    let request = p::AgentRequest {
        request_id: Uuid::new_v4().to_string(),
        protocol_version: crate::AGENT_PROTOCOL_VERSION,
        payload: Some(agent_request::Payload::Macos(p::MacosAgentRequest {
            sequence: 1,
            operation: Some(Op::Status(p::Empty {})),
        })),
    };
    darwin_fd::send_frame(
        &stream,
        &encode_frame(&request).map_err(io::Error::other)?,
        None::<&OwnedFd>,
    )?;
    let frame = darwin_fd::receive_frame(&stream, peer)?;
    if frame.descriptor.is_some() {
        return Err(denied());
    }
    let response: p::AgentResponse = decode_frame(frame.bytes.into()).map_err(io::Error::other)?;
    if response.request_id != request.request_id || response.error.is_some() {
        return Err(denied());
    }
    let Some(agent_response::Payload::Macos(response)) = response.payload else {
        return Err(denied());
    };
    let Some(Reply::Status(status)) = response.payload else {
        return Err(denied());
    };
    if response.failure != 0
        || status.failure != 0
        || status.system_proxy_active
        || !status.system_proxy_supported
        || status.system_proxy_recovery_pending
        || status.recovery_pending
        || status.phase != p::AgentPhase::Clean as i32
        || status.protocol_version != super::MACOS_PROTOCOL_VERSION
        || status.agent_version != env!("CARGO_PKG_VERSION")
        || status.architecture != std::env::consts::ARCH
    {
        return Err(io::Error::other("Agent not ready"));
    }
    Ok(())
}
fn validate_request(request: &p::AgentRequest, sequence: &mut u64) -> io::Result<()> {
    if request.protocol_version != crate::AGENT_PROTOCOL_VERSION || request.request_id.len() != 36 {
        return Err(denied());
    }
    let id = Uuid::parse_str(&request.request_id).map_err(|_| denied())?;
    if id.is_nil() || id.to_string() != request.request_id {
        return Err(denied());
    }
    let Some(agent_request::Payload::Macos(value)) = &request.payload else {
        return Err(denied());
    };
    if value.operation.is_none()
        || value.sequence == 0
        || sequence.checked_add(1) != Some(value.sequence)
    {
        return Err(denied());
    }
    *sequence = value.sequence;
    Ok(())
}
impl Service {
    fn status(&mut self, owner: Owner) -> p::MacosPlatformStatus {
        let mut status = p::MacosPlatformStatus {
            protocol_version: super::MACOS_PROTOCOL_VERSION,
            agent_version: env!("CARGO_PKG_VERSION").into(),
            architecture: std::env::consts::ARCH.into(),
            release_approved: super::RELEASE_APPROVED,
            test_tun_enabled: super::TEST_TUN_ENABLED,
            system_proxy_active: self.proxy.as_ref().is_some_and(|proxy| proxy.active()),
            system_proxy_supported: self.proxy.is_some() && self.state.is_some(),
            system_proxy_recovery_pending: self.proxy.as_mut().is_none_or(|proxy| proxy.pending()),
            failure: self.failure as i32,
            ..Default::default()
        };
        let Some(state) = self.state.as_mut() else {
            status.recovery_pending = true;
            status.phase = p::AgentPhase::RecoveryRequired as i32;
            return status;
        };
        let physical = if let Some(transaction) = state.state() {
            status.fence = Some(transaction.fence.into());
            status.caller_owns_transaction =
                self.owner_connection.is_some() && transaction.owner == owner;
            status.phase = match transaction.phase {
                Phase::Preparing => p::AgentPhase::Preparing,
                Phase::Active => p::AgentPhase::Active,
                Phase::Recovering => p::AgentPhase::Recovering,
                Phase::Retained | Phase::RecoveryRequired => p::AgentPhase::RecoveryRequired,
            } as i32;
            status.recovery_pending = matches!(
                transaction.phase,
                Phase::Retained | Phase::Recovering | Phase::RecoveryRequired
            ) || state.persistence_uncertain();
            let resumable = transaction.owner.uid == owner.uid
                && self.owner_connection.is_none()
                && matches!(transaction.phase, Phase::Retained | Phase::RecoveryRequired)
                && !state.persistence_uncertain();
            let expects_protection =
                transaction.kill_switch || transaction.transition.is_some() || resumable;
            let (retained, physical) = state.backend_mut().observe_status(expects_protection);
            status.protection_retained = retained;
            status.resumable = resumable && status.protection_retained;
            if resumable {
                status.control_api_candidates = state
                    .backend()
                    .receipts()
                    .map(|r| {
                        r.control_api_candidates
                            .iter()
                            .map(ToString::to_string)
                            .collect()
                    })
                    .unwrap_or_default();
            }
            physical
        } else {
            status.phase = p::AgentPhase::Clean as i32;
            state.backend_mut().preflight().ok()
        };
        status.preflight_passed = physical.is_some()
            && !status.recovery_pending
            && self.failure == p::MacosFailure::Unspecified;
        status.physical_network = physical.map(|network| {
            let mut interfaces = Vec::<p::PhysicalInterface>::new();
            for path in network.paths {
                if let Some(interface) = interfaces
                    .iter_mut()
                    .find(|i| i.interface_index == path.index)
                {
                    interface.address_family_mask |= if path.address.is_ipv4() { 1 } else { 2 };
                } else {
                    interfaces.push(p::PhysicalInterface {
                        interface_luid: 0,
                        interface_index: path.index,
                        dns_servers: network.dns.iter().map(ToString::to_string).collect(),
                        address_family_mask: if path.address.is_ipv4() { 1 } else { 2 },
                    });
                }
            }
            p::PhysicalNetworkInfo {
                interfaces,
                generation: self.network_generation,
            }
        });
        status
    }
    fn authorize(
        &self,
        owner: Owner,
        connection: Uuid,
        fence: Fence,
    ) -> Result<(), p::MacosFailure> {
        if self.owner_connection != Some(connection) {
            return Err(p::MacosFailure::Busy);
        }
        if self
            .state
            .as_ref()
            .and_then(|s| s.state())
            .is_none_or(|t| t.owner != owner || t.fence != fence)
        {
            return Err(p::MacosFailure::StaleFence);
        }
        Ok(())
    }
    fn execute(
        &mut self,
        owner: Owner,
        connection: Uuid,
        operation: Op,
    ) -> Result<(Reply, Option<OwnedFd>), p::MacosFailure> {
        if matches!(operation, Op::Status(_)) {
            return Ok((Reply::Status(self.status(owner)), None));
        }
        if self.state.is_none() {
            return Err(p::MacosFailure::RecoveryIncomplete);
        }
        if matches!(&operation, Op::Prepare(_) | Op::Resume(_)) {
            if !super::RELEASE_APPROVED && !super::TEST_TUN_ENABLED {
                return Err(p::MacosFailure::ReleaseGated);
            }
            if self.proxy.as_mut().is_none_or(|proxy| proxy.pending()) {
                return Err(p::MacosFailure::RecoveryIncomplete);
            }
        }
        let parse = |f: Option<p::MacosFence>| -> Result<Fence, p::MacosFailure> {
            f.ok_or(p::MacosFailure::Protocol)?
                .try_into()
                .map_err(|_| p::MacosFailure::Protocol)
        };
        match operation {
            Op::AcquireProxy(request) => self
                .proxy
                .as_mut()
                .ok_or(p::MacosFailure::Journal)?
                .acquire(owner, connection, request)?,
            Op::ReleaseProxy(request) => {
                let lease = super::system_proxy::lease_id(&request.lease)?;
                self.proxy
                    .as_mut()
                    .ok_or(p::MacosFailure::Journal)?
                    .release(owner, connection, Some(lease))?;
            }
            Op::RecoverProxy(_) => self
                .proxy
                .as_mut()
                .ok_or(p::MacosFailure::Journal)?
                .release(owner, connection, None)?,
            Op::Prepare(value) => {
                let fence = parse(value.fence)?;
                if fence.network != self.network_generation {
                    return Err(p::MacosFailure::StaleFence);
                }
                let plan = crate::plan::ValidatedTunnelPlan::from_macos_prepare(
                    value.plan.ok_or(p::MacosFailure::Protocol)?,
                )
                .map_err(|_| p::MacosFailure::Protocol)?;
                let state = self.state.as_mut().unwrap();
                if state.state().is_some() || self.owner_connection.is_some() {
                    return Err(p::MacosFailure::Busy);
                }
                let kill_switch = plan.kill_switch;
                state.backend_mut().set_plan(plan).map_err(map_error)?;
                self.owner_connection = Some(connection);
                state
                    .prepare(owner, fence, kill_switch)
                    .map_err(map_error)?;
            }
            Op::Configure(value) => {
                let fence = parse(value.fence)?;
                self.authorize(owner, connection, fence)?;
                let plan = value
                    .plan
                    .ok_or(p::MacosFailure::Protocol)?
                    .try_into()
                    .map_err(|_| p::MacosFailure::Protocol)?;
                let state = self.state.as_mut().unwrap();
                state.backend_mut().finalize_plan(plan).map_err(map_error)?;
                state.configure(owner, fence).map_err(map_error)?;
            }
            Op::Commit(value) => {
                let fence = parse(Some(value))?;
                self.authorize(owner, connection, fence)?;
                let state = self.state.as_mut().unwrap();
                state
                    .backend_mut()
                    .unchanged_network(fence)
                    .map_err(map_error)?;
                state.commit(owner, fence).map_err(map_error)?;
                state.backend_mut().set_active();
            }
            Op::Disconnect(value) => {
                let fence = parse(Some(value))?;
                if self.owner_connection.is_none() {
                    self.state
                        .as_mut()
                        .unwrap()
                        .disconnect_retained(owner, fence)
                        .map_err(map_error)?;
                } else {
                    self.authorize(owner, connection, fence)?;
                    self.state
                        .as_mut()
                        .unwrap()
                        .disconnect(owner, fence)
                        .map_err(map_error)?;
                }
                self.owner_connection = None;
                self.failure = p::MacosFailure::Unspecified;
            }
            Op::OpenTun(value) => {
                let fence = parse(Some(value))?;
                self.authorize(owner, connection, fence)?;
                let (descriptor, name, index) = self
                    .state
                    .as_ref()
                    .unwrap()
                    .backend()
                    .descriptor(fence)
                    .map_err(map_error)?;
                return Ok((
                    Reply::Descriptor(p::MacosDescriptor {
                        fence: Some(fence.into()),
                        interface_name: name,
                        interface_index: index,
                        ..Default::default()
                    }),
                    Some(descriptor),
                ));
            }
            Op::CreateSocket(value) => {
                let fence = parse(value.fence)?;
                self.authorize(owner, connection, fence)?;
                let remote = value
                    .remote_endpoint
                    .parse()
                    .map_err(|_| p::MacosFailure::Protocol)?;
                let protocol =
                    u8::try_from(value.protocol).map_err(|_| p::MacosFailure::Protocol)?;
                let purpose = p::MacosSocketPurpose::try_from(value.purpose)
                    .map_err(|_| p::MacosFailure::Protocol)?;
                let backend = self.state.as_mut().unwrap().backend_mut();
                let (id, descriptor, permit) = backend
                    .create_socket(fence, remote, protocol, purpose)
                    .map_err(map_error)?;
                let index = backend
                    .physical()
                    .ok_or(p::MacosFailure::NetworkChanged)?
                    .path(remote)
                    .map_err(map_error)?
                    .index;
                return Ok((
                    Reply::Descriptor(p::MacosDescriptor {
                        fence: Some(fence.into()),
                        lease_id: id,
                        interface_name: permit.interface,
                        interface_index: index,
                        local_endpoint: permit.local.to_string(),
                        remote_endpoint: permit.remote.to_string(),
                        protocol: u32::from(permit.protocol),
                    }),
                    Some(descriptor),
                ));
            }
            Op::ReleaseSocket(value) => {
                let fence = parse(value.fence)?;
                self.authorize(owner, connection, fence)?;
                self.state
                    .as_mut()
                    .unwrap()
                    .backend_mut()
                    .release_socket(fence, value.lease_id)
                    .map_err(map_error)?;
            }
            Op::SetEndpointCandidates(value) => {
                let fence = parse(value.fence)?;
                self.authorize(owner, connection, fence)?;
                if value.ipv6_candidates.len() > 16 {
                    return Err(p::MacosFailure::Protocol);
                }
                let candidates = value
                    .ipv6_candidates
                    .iter()
                    .map(|s| s.parse())
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|_| p::MacosFailure::Protocol)?;
                self.state
                    .as_mut()
                    .unwrap()
                    .backend_mut()
                    .endpoint_candidates(&value.hostname, candidates)
                    .map_err(map_error)?;
            }
            Op::Suspend(value) => {
                let fence = parse(Some(value))?;
                self.authorize(owner, connection, fence)?;
                self.state
                    .as_mut()
                    .unwrap()
                    .suspend(owner, fence)
                    .map_err(map_error)?;
                self.owner_connection = None;
            }
            Op::Resume(value) => {
                if self.owner_connection.is_some() {
                    return Err(p::MacosFailure::Busy);
                }
                let previous = parse(value.previous)?;
                let next = value.next.ok_or(p::MacosFailure::Protocol)?;
                let fence = parse(next.fence)?;
                if fence.network != self.network_generation {
                    return Err(p::MacosFailure::StaleFence);
                }
                let plan = crate::plan::ValidatedTunnelPlan::from_macos_prepare(
                    next.plan.ok_or(p::MacosFailure::Protocol)?,
                )
                .map_err(|_| p::MacosFailure::Protocol)?;
                let state = self.state.as_mut().unwrap();
                let kill_switch = plan.kill_switch;
                state.backend_mut().set_plan(plan).map_err(map_error)?;
                state
                    .resume(owner, previous, fence, kill_switch)
                    .map_err(map_error)?;
                self.owner_connection = Some(connection);
                self.failure = p::MacosFailure::Unspecified;
            }
            Op::Status(_) => unreachable!("handled above"),
        }
        Ok((Reply::Empty(p::Empty {}), None))
    }
    fn observe_network(&mut self) {
        let Some(state) = self.state.as_mut() else {
            return;
        };
        let Some(transaction) = state.state().cloned() else {
            return;
        };
        if !matches!(transaction.phase, Phase::Preparing | Phase::Active) {
            return;
        }
        if state
            .backend_mut()
            .unchanged_network(transaction.fence)
            .is_err()
        {
            self.network_generation = self.network_generation.saturating_add(1);
            self.failure = p::MacosFailure::NetworkChanged;
            self.owner_connection = None;
            if state
                .owner_lost(transaction.owner, transaction.fence)
                .is_err()
            {
                self.failure = p::MacosFailure::RecoveryIncomplete;
            }
        }
    }
    fn lost(&mut self, connection: Uuid) {
        if let Some(proxy) = self.proxy.as_mut() {
            proxy.lost(connection);
        }
        if self.owner_connection != Some(connection) {
            return;
        }
        self.owner_connection = None;
        if let Some(state) = self.state.as_mut()
            && let Some(transaction) = state.state().cloned()
            && state
                .owner_lost(transaction.owner, transaction.fence)
                .is_err()
        {
            self.failure = p::MacosFailure::RecoveryIncomplete;
        }
    }
}
fn map_error(error: io::Error) -> p::MacosFailure {
    if let Some(dial) = error
        .get_ref()
        .and_then(|e| e.downcast_ref::<super::socket::EndpointConnectFailure>())
    {
        tracing::debug!(
            os_error = dial.0.raw_os_error(),
            "physical endpoint connect failed"
        );
        return p::MacosFailure::EndpointUnreachable;
    }
    match error.kind() {
        io::ErrorKind::Unsupported => p::MacosFailure::PfIncompatible,
        io::ErrorKind::PermissionDenied => p::MacosFailure::OwnershipConflict,
        io::ErrorKind::InvalidData => p::MacosFailure::Journal,
        io::ErrorKind::NetworkUnreachable => p::MacosFailure::NetworkChanged,
        _ => p::MacosFailure::RecoveryIncomplete,
    }
}
fn readable(stream: &UnixStream, millis: i32) -> io::Result<bool> {
    let mut fd = libc::pollfd {
        fd: stream.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    // SAFETY: poll only observes one live stream and writes its revents.
    match unsafe { libc::poll(&mut fd, 1, millis) } {
        -1 if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted => Ok(false),
        -1 => Err(io::Error::last_os_error()),
        count => Ok(count != 0),
    }
}
fn serve_connection(
    stream: UnixStream,
    policy: &CallerPolicy,
    service: &Mutex<Service>,
    connection: Uuid,
    stopping: &AtomicBool,
) -> io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    let peer = darwin_fd::peer_identity(&stream)?;
    let administrator = peer.audit_token[1] == 0 && peer.audit_token[3] == 0;
    let owner = if administrator {
        Owner {
            uid: 0,
            pid: peer.audit_token[5],
            pid_version: peer.audit_token[7],
        }
    } else {
        policy.authenticate(peer)?
    };
    let mut sequence = 0;
    while !stopping.load(Ordering::Acquire) {
        // A quiet proxy connection may outlive the frame timeout (including
        // sleep). Poll for the first byte/EOF while still bounding partial frames.
        if service
            .lock()
            .map_err(|_| denied())?
            .proxy
            .as_ref()
            .is_some_and(|p| p.owns(connection))
            && !readable(&stream, 500)?
        {
            continue;
        }
        let frame = darwin_fd::receive_frame(&stream, peer)?;
        if frame.descriptor.is_some() {
            return Err(denied());
        }
        if !administrator && policy.authenticate(darwin_fd::peer_identity(&stream)?)? != owner {
            return Err(denied());
        }
        let request: p::AgentRequest =
            decode_frame(bytes::Bytes::from(frame.bytes)).map_err(|_| denied())?;
        validate_request(&request, &mut sequence)?;
        let Some(agent_request::Payload::Macos(value)) = request.payload else {
            return Err(denied());
        };
        if administrator && !matches!(value.operation, Some(Op::Status(_))) {
            return Err(denied());
        }
        let result = service.lock().map_err(|_| denied())?.execute(
            owner,
            connection,
            value.operation.ok_or_else(denied)?,
        );
        let (response, descriptor) = match result {
            Ok((payload, descriptor)) => (
                p::MacosAgentResponse {
                    failure: 0,
                    payload: Some(payload),
                },
                descriptor,
            ),
            Err(failure) => (
                p::MacosAgentResponse {
                    failure: failure as i32,
                    payload: None,
                },
                None,
            ),
        };
        let response = p::AgentResponse {
            request_id: request.request_id,
            error: None,
            payload: Some(agent_response::Payload::Macos(response)),
        };
        darwin_fd::send_frame(
            &stream,
            &encode_frame(&response).map_err(|_| denied())?,
            descriptor.as_ref(),
        )?;
    }
    Ok(())
}
pub fn serve(stopping: Arc<AtomicBool>) -> io::Result<()> {
    require_root()?;
    let _single = super::journal::service_lock()?;
    // Invalidate the previous clean-uninstall proof before accepting any lease.
    let marker = Path::new(super::INSTALL_DIRECTORY).with_added_extension("clean-uninstall");
    super::journal::verify_root_parents(&marker)?;
    match fs::remove_file(&marker) {
        Ok(()) => fs::File::open(marker.parent().ok_or_else(denied)?)?.sync_all()?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => (),
        Err(error) => return Err(error),
    }
    let policy = Arc::new(CallerPolicy::load()?);
    let state = open_state();
    let mut proxy = SystemProxy::open().ok();
    if let Some(proxy) = proxy.as_mut() {
        let _ = proxy.restore(); // Retain failed evidence for explicit recovery.
    }
    let mut service = match state {
        Ok(state) => Service {
            proxy,
            network_generation: state.state().map(|t| t.fence.network).unwrap_or(1),
            state: Some(state),
            failure: p::MacosFailure::Unspecified,
            owner_connection: None,
        },
        Err(_) => Service {
            proxy,
            network_generation: 1,
            state: None,
            failure: p::MacosFailure::Journal,
            owner_connection: None,
        },
    };
    if let Some(state) = service.state.as_mut()
        && let Some(transaction) = state.state().cloned()
    {
        // A restarted Agent cannot trust old egress leases or an old pump.
        if state
            .owner_lost(transaction.owner, transaction.fence)
            .is_err()
        {
            service.failure = p::MacosFailure::RecoveryIncomplete;
        }
    }
    super::journal::verify_root_parents(Path::new(super::SOCKET_PATH))?;
    match fs::symlink_metadata(super::SOCKET_PATH) {
        Ok(metadata) => {
            if !metadata.file_type().is_socket() || metadata.uid() != 0 {
                return Err(denied());
            }
            match UnixStream::connect(super::SOCKET_PATH) {
                Ok(_) => {
                    return Err(io::Error::new(
                        io::ErrorKind::AddrInUse,
                        "Agent already running",
                    ));
                }
                Err(error) if error.kind() == io::ErrorKind::ConnectionRefused => {
                    fs::remove_file(super::SOCKET_PATH)?
                }
                Err(error) => return Err(error),
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let listener = UnixListener::bind(super::SOCKET_PATH)?;
    let socket_identity = fs::symlink_metadata(super::SOCKET_PATH)?;
    fs::set_permissions(super::SOCKET_PATH, fs::Permissions::from_mode(0o666))?;
    listener.set_nonblocking(true)?;
    let service = Arc::new(Mutex::new(service));
    let count = Arc::new(AtomicUsize::new(0));
    let mut workers: Vec<thread::JoinHandle<()>> = Vec::new();
    let mut observed = std::time::Instant::now();
    while !stopping.load(Ordering::Acquire) {
        let mut index = 0;
        while index < workers.len() {
            if workers[index].is_finished() {
                let _ = workers.swap_remove(index).join();
            } else {
                index += 1;
            }
        }
        if observed.elapsed() >= Duration::from_secs(1) {
            service.lock().map_err(|_| denied())?.observe_network();
            observed = std::time::Instant::now();
        }
        match listener.accept() {
            Ok((stream, _)) => {
                if count.load(Ordering::Acquire) >= 8 {
                    continue;
                }
                count.fetch_add(1, Ordering::AcqRel);
                let (policy, service, count, stopping) = (
                    policy.clone(),
                    service.clone(),
                    count.clone(),
                    stopping.clone(),
                );
                workers.push(thread::spawn(move || {
                    let connection = Uuid::new_v4();
                    let _ = serve_connection(stream, &policy, &service, connection, &stopping);
                    if let Ok(mut service) = service.lock() {
                        service.lost(connection);
                    }
                    count.fetch_sub(1, Ordering::AcqRel);
                }));
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(50))
            }
            Err(error) => return Err(error),
        }
    }
    for worker in workers {
        let _ = worker.join();
    }
    if let Ok(mut service) = service.lock()
        && let Some(connection) = service.owner_connection
    {
        service.lost(connection);
    }
    // No automatic network restoration on daemon shutdown. PF remains in the
    // kernel; the durable journal determines explicit administrator recovery.
    let current = fs::symlink_metadata(super::SOCKET_PATH)?;
    if current.dev() != socket_identity.dev() || current.ino() != socket_identity.ino() {
        return Err(denied());
    }
    fs::remove_file(super::SOCKET_PATH)?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_endpoint_connect_errors_are_retryable_after_socket_setup() {
        use super::super::socket::endpoint_connect_error;
        for errno in [
            libc::EHOSTUNREACH,
            libc::EHOSTDOWN,
            libc::ENETUNREACH,
            libc::ECONNREFUSED,
            libc::ETIMEDOUT,
        ] {
            assert_eq!(
                map_error(endpoint_connect_error(io::Error::from_raw_os_error(errno))),
                p::MacosFailure::EndpointUnreachable,
            );
            // The same errno during an Agent check or permit rollback is fatal.
            assert_ne!(
                map_error(io::Error::from_raw_os_error(errno)),
                p::MacosFailure::EndpointUnreachable
            );
        }
        assert_eq!(
            map_error(endpoint_connect_error(io::Error::from_raw_os_error(
                libc::EPERM
            ))),
            p::MacosFailure::OwnershipConflict,
        );
    }
    #[test]
    fn request_ids_are_bounded_and_never_replay_a_mutation() {
        let mut seen = 0;
        let mut request = p::AgentRequest {
            request_id: Uuid::new_v4().to_string(),
            protocol_version: crate::AGENT_PROTOCOL_VERSION,
            payload: Some(agent_request::Payload::Macos(p::MacosAgentRequest {
                sequence: 1,
                operation: Some(Op::Status(p::Empty {})),
            })),
        };
        validate_request(&request, &mut seen).unwrap();
        assert!(validate_request(&request, &mut seen).is_err());
        request.request_id = Uuid::nil().to_string();
        assert!(validate_request(&request, &mut seen).is_err());
        request.request_id = Uuid::new_v4().to_string();
        request.protocol_version = 0;
        assert!(validate_request(&request, &mut seen).is_err());
    }
    fn transaction() -> super::super::transaction::Transaction {
        use super::super::transaction::*;
        Transaction {
            owner: Owner {
                uid: 501,
                pid: 42,
                pid_version: 5,
            },
            fence: Fence {
                session: Uuid::new_v4(),
                configuration: 1,
                network: 1,
            },
            kill_switch: false,
            phase: Phase::Active,
            transition: None,
            records: [
                Step::Protection,
                Step::Egress,
                Step::Utun,
                Step::Dns,
                Step::Routes,
            ]
            .into_iter()
            .map(|step| Record {
                step,
                state: StepState::Applied,
            })
            .collect(),
        }
    }
    fn native(fence: Fence) -> super::super::resources::NativeReceipts {
        super::super::resources::NativeReceipts {
            boot: Uuid::new_v4(),
            fence,
            control_api_candidates: vec![],
            pf: None,
            interface: None,
            dns: vec![],
            routes: vec![],
        }
    }
    #[test]
    fn missing_native_evidence_cannot_make_active_transaction_clean() {
        use super::super::transaction::*;
        let mut t = transaction();
        assert!(validate_pair(Some(&t), None).is_err());
        assert!(validate_pair(None, Some(&native(t.fence))).is_err());
        assert!(
            validate_pair(
                Some(&t),
                Some(&native(Fence {
                    configuration: 2,
                    ..t.fence
                }))
            )
            .is_err()
        );
        t.phase = Phase::Recovering;
        assert!(validate_pair(Some(&t), None).is_err());
        for r in &mut t.records[1..] {
            r.state = StepState::Restored;
        }
        validate_pair(Some(&t), None).unwrap();
        t.phase = Phase::Preparing;
        t.records.truncate(1);
        t.records[0].state = StepState::Intended;
        validate_pair(Some(&t), None).unwrap();
    }
    #[test]
    fn handoff_crash_after_native_commit_promotes_new_owner_and_kill_switch() {
        use super::super::transaction::Transition;
        let mut t = transaction();
        let next = Fence {
            configuration: 2,
            network: 2,
            ..t.fence
        };
        let owner = Owner {
            pid: 99,
            pid_version: 6,
            ..t.owner
        };
        t.transition = Some(Transition {
            owner,
            fence: next,
            kill_switch: true,
        });
        let previous = native(t.fence);
        assert!(!promote_transition(&mut t, Some(&previous)));
        assert!(promote_transition(&mut t, Some(&native(next))));
        assert_eq!(t.owner, owner);
        assert_eq!(t.fence, next);
        assert!(t.kill_switch);
        assert_eq!(t.phase, Phase::RecoveryRequired);
        assert!(t.transition.is_none());
        assert_eq!(t.records.len(), 1);
        t.validate().unwrap();
    }
    #[test]
    fn interrupted_initialization_only_repairs_missing_new_records() {
        use super::super::journal::{JournalStore, SecureStore};
        let directory = tempfile::tempdir().unwrap();
        fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let path = directory.path().join("network.json");
        let mut store: JournalStore = SecureStore::for_test(path.clone()).unwrap();
        assert!(initialize_clean(&mut store, true).is_err());
        initialize_clean(&mut store, false).unwrap();
        initialize_clean(&mut store, true).unwrap();
        fs::write(&path, b"corrupt").unwrap();
        assert!(initialize_clean(&mut store, false).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"corrupt");
    }
    #[test]
    fn only_a_validated_pre_proxy_install_can_recover_without_proxy_records() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("marker.json");
        let receipt = dir.path().join("proxy.json");
        let legacy = || Ok(());
        let damaged_install = || Err(io::Error::other("required proxy receipt missing"));
        assert!(validated_pre_proxy_install(&marker, &receipt, legacy).unwrap());
        assert!(validated_pre_proxy_install(&marker, &receipt, damaged_install).is_err());
        fs::write(&marker, b"existing").unwrap();
        assert!(
            !validated_pre_proxy_install(&marker, &receipt, || panic!("must restore proxy"))
                .unwrap()
        );
        fs::remove_file(&marker).unwrap();
        std::os::unix::fs::symlink(dir.path().join("missing"), &receipt).unwrap();
        assert!(
            !validated_pre_proxy_install(&marker, &receipt, || panic!("dangling receipt")).unwrap()
        );
    }
}
