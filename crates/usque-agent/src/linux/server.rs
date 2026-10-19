use std::{
    collections::{HashMap, VecDeque},
    fs::{self, File, OpenOptions},
    io,
    os::{
        fd::{AsFd, AsRawFd, OwnedFd},
        unix::{
            fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt},
            net::{UnixListener, UnixStream},
        },
    },
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use bytes::Bytes;
use usque_ipc::{
    agent_v1::{
        self, AgentRequest, AgentResponse, agent_request::Payload as Request,
        agent_response::Payload as Response,
    },
    decode_frame, encode_frame,
};
use uuid::Uuid;

use super::{
    JOURNAL_PATH, SOCKET_PATH,
    auth::{AuthenticatedPeer, CallerPolicy},
    backend::NativeBackend,
    coordinator::Coordinator,
    journal::{JournalStore, Owner, Phase},
    transport,
};

const ORPHAN_GRACE: Duration = Duration::from_secs(30);
const MAX_CONNECTIONS: usize = 16;
type OwnerKey = (u32, u32, u64);

struct Service {
    coordinator: Coordinator<NativeBackend>,
    connections: HashMap<OwnerKey, usize>,
    stopped: bool,
    lease_epoch: u64,
}

pub fn validate_only() -> io::Result<()> {
    // This path reads installation policy/evidence only. In particular it does
    // not bind/unlink a socket, run nft, open /dev/net/tun or recover state.
    CallerPolicy::load()?;
    JournalStore::open(JOURNAL_PATH)?.load()?.validate()
}

pub fn recover_for_maintenance() -> io::Result<()> {
    require_root()?;
    let _lock = service_lock()?;
    let store = JournalStore::open(JOURNAL_PATH)?;
    Coordinator::open(store, NativeBackend::default())?.recover()
}

pub fn serve(stopping: Arc<AtomicBool>) -> io::Result<()> {
    require_root()?;
    let _lock = service_lock()?;
    let policy = Arc::new(CallerPolicy::load()?);
    let store = JournalStore::open(JOURNAL_PATH)?;
    let mut coordinator = Coordinator::open(store, NativeBackend::default())?;
    let retain = coordinator.state().phase == Phase::Active
        && coordinator.state().owner.as_ref().is_some_and(owner_alive);
    if !retain && let Err(error) = coordinator.recover() {
        tracing::error!(kind = ?error.kind(), "Linux startup recovery is incomplete; retaining protection and evidence");
    }
    let service = Arc::new(Mutex::new(Service {
        coordinator,
        connections: HashMap::new(),
        stopped: false,
        lease_epoch: 0,
    }));
    if retain {
        schedule_orphan_recovery(Arc::clone(&service));
    }
    let watcher = super::network::ChangeWatcher::open()?;
    let mut last_snapshot = Instant::now();
    let mut recovery_due: Option<(Uuid, u64, Instant)> = None;
    let listener = bind_listener()?;
    listener.set_nonblocking(true)?;
    let active = Arc::new(AtomicUsize::new(0));
    // Transient journal, netlink and accept failures are logged once and
    // retried. Only a failed listener leaves the loop, and still recovers.
    let mut recovery_arm_failed = false;
    let mut watcher_failed = false;
    let mut accept_failed = false;
    let mut fatal = None;
    while !stopping.load(Ordering::Acquire) {
        if let Ok(mut state) = service.lock() {
            let journal = state.coordinator.state();
            let pending_replacement =
                journal.replacement.is_some() || journal.replacement_origin.is_some();
            let connected = journal.owner.as_ref().is_some_and(|owner| {
                state.connections.contains_key(&(
                    owner.uid,
                    owner.process_id,
                    owner.process_start_time,
                ))
            });
            let armed = if !pending_replacement || !connected {
                match state.coordinator.arm_automatic_recovery() {
                    Ok(armed) => {
                        recovery_arm_failed = false;
                        armed
                    }
                    Err(error) => {
                        if !recovery_arm_failed {
                            tracing::warn!(kind = ?error.kind(), "Linux automatic recovery could not be armed; retrying");
                        }
                        recovery_arm_failed = true;
                        false
                    }
                }
            } else {
                false
            };
            if armed {
                let journal = state.coordinator.state();
                let operation = journal.operation_id.expect("recovery operation");
                let generation = journal.generation;
                if recovery_due.is_none_or(|(id, saved, _)| id != operation || saved != generation)
                {
                    let delay = [1, 5, 30][usize::from(journal.automatic_recovery.attempts.min(2))];
                    recovery_due = Some((
                        operation,
                        generation,
                        Instant::now() + Duration::from_secs(delay),
                    ));
                }
                if recovery_due.is_some_and(|(_, _, due)| Instant::now() >= due) {
                    if let Err(error) = state.coordinator.automatic_recover(operation, generation) {
                        tracing::warn!(kind = ?error.kind(), "Linux bounded automatic recovery retained unresolved resources");
                    }
                    recovery_due = None;
                }
            } else {
                recovery_due = None;
            }
        }
        let changed = match watcher.changed() {
            Ok(changed) => {
                watcher_failed = false;
                changed
            }
            // A failed read may have lost a notification: refresh once, then
            // rely on the periodic snapshot until the watcher recovers.
            Err(error) => {
                let first = !watcher_failed;
                if first {
                    tracing::warn!(kind = ?error.kind(), "Linux network change watcher failed; using periodic snapshots");
                }
                watcher_failed = true;
                first
            }
        };
        if changed || last_snapshot.elapsed() >= Duration::from_secs(5) {
            if let Ok(mut state) = service.lock()
                && matches!(
                    state.coordinator.state().phase,
                    Phase::Prepared | Phase::Active
                )
                && let Some(resources) = state.coordinator.state().resources.clone()
            {
                let snapshot =
                    super::backend::physical_snapshot(&resources.dns).unwrap_or_default();
                if let Err(error) = state.coordinator.update_physical_snapshot(snapshot) {
                    tracing::error!(kind = ?error.kind(), "Linux physical network changed but lease invalidation failed");
                }
            }
            last_snapshot = Instant::now();
        }
        let socket = match listener.accept() {
            Ok((socket, _)) => {
                accept_failed = false;
                socket
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                accept_failed = false;
                thread::sleep(Duration::from_millis(50));
                continue;
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) if transient_accept_error(&error) => {
                if !accept_failed {
                    tracing::warn!(kind = ?error.kind(), "Linux Agent connections cannot be accepted; retrying");
                }
                accept_failed = true;
                thread::sleep(Duration::from_millis(50));
                continue;
            }
            Err(error) => {
                fatal = Some(error);
                break;
            }
        };
        if active.load(Ordering::Acquire) >= MAX_CONNECTIONS {
            continue;
        }
        // A failure here affects only this connection, which is dropped.
        if socket
            .set_read_timeout(Some(Duration::from_secs(60)))
            .and_then(|()| socket.set_write_timeout(Some(Duration::from_secs(5))))
            .and_then(|()| transport::enable_credentials(&socket))
            .is_err()
        {
            continue;
        }
        let peer =
            match transport::peer_credentials(&socket).and_then(|peer| policy.authenticate(peer)) {
                Ok(peer) => peer,
                Err(_) => continue,
            };
        active.fetch_add(1, Ordering::AcqRel);
        let active = Arc::clone(&active);
        let policy = Arc::clone(&policy);
        let service = Arc::clone(&service);
        thread::spawn(move || {
            let key = owner_key(&peer);
            if let Ok(mut state) = service.lock() {
                *state.connections.entry(key).or_default() += 1;
                if state.coordinator.state().owner.as_ref() == Some(&owner(&peer))
                    && state.coordinator.backend_mut().has_tun()
                {
                    state.lease_epoch = state.lease_epoch.wrapping_add(1);
                }
            }
            let _ = serve_connection(&socket, &peer, &policy, &service);
            let mut orphaned = false;
            if let Ok(mut state) = service.lock() {
                if let Some(count) = state.connections.get_mut(&key) {
                    *count -= 1;
                    if *count == 0 {
                        state.connections.remove(&key);
                    }
                }
                if !state.connections.contains_key(&key)
                    && state.coordinator.state().owner.as_ref() == Some(&owner(&peer))
                    && state.coordinator.state().phase != Phase::Clean
                {
                    state.lease_epoch = state.lease_epoch.wrapping_add(1);
                    if let Some(resources) = state.coordinator.state().resources.clone()
                        && let Err(error) =
                            state.coordinator.backend_mut().revoke_egress(&resources)
                    {
                        tracing::error!(kind = ?error.kind(), "Linux physical egress lease cleanup failed");
                    }
                    orphaned = true;
                }
            }
            if orphaned {
                schedule_orphan_recovery(service);
            }
            active.fetch_sub(1, Ordering::AcqRel);
        });
    }
    let mut state = service
        .lock()
        .map_err(|_| io::Error::other("Linux Agent coordinator unavailable"))?;
    state.stopped = true;
    let result = state.coordinator.recover();
    drop(listener);
    remove_owned_socket()?;
    match fatal {
        Some(error) => {
            if let Err(recovery) = result {
                tracing::error!(kind = ?recovery.kind(), "Linux Agent listener failed and recovery is incomplete");
            }
            Err(error)
        }
        None => result,
    }
}

fn transient_accept_error(error: &io::Error) -> bool {
    matches!(
        error.raw_os_error(),
        Some(
            libc::ECONNABORTED
                | libc::EMFILE
                | libc::ENFILE
                | libc::ENOBUFS
                | libc::ENOMEM
                | libc::EPROTO
                | libc::EPERM
        )
    )
}

fn serve_connection(
    socket: &UnixStream,
    peer: &AuthenticatedPeer,
    policy: &CallerPolicy,
    service: &Mutex<Service>,
) -> io::Result<()> {
    let mut seen = VecDeque::new();
    loop {
        let frame = transport::receive_frame(socket, peer.credentials)?;
        policy.revalidate(peer)?;
        let request: AgentRequest = decode_frame(Bytes::from(frame.bytes))
            .map_err(|_| io::Error::other("invalid Agent protobuf"))?;
        if request.request_id.is_empty()
            || request.request_id.len() > 128
            || !request.request_id.is_ascii()
            || request.protocol_version != crate::AGENT_PROTOCOL_VERSION
            || seen.contains(&request.request_id)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid or replayed Agent request",
            ));
        }
        seen.push_back(request.request_id.clone());
        if seen.len() > 256 {
            seen.pop_front();
        }
        let mut state = service
            .lock()
            .map_err(|_| io::Error::other("Linux Agent coordinator unavailable"))?;
        if state.stopped {
            return Ok(());
        }
        let result = dispatch(
            &mut state.coordinator,
            peer,
            request.payload,
            frame.descriptor,
        );
        let (response, descriptor) = match result {
            Ok((payload, fd)) => (
                AgentResponse {
                    request_id: request.request_id,
                    error: None,
                    payload: Some(payload),
                },
                fd,
            ),
            Err(error) => (
                AgentResponse {
                    request_id: request.request_id,
                    error: Some(agent_error(error)),
                    payload: None,
                },
                None,
            ),
        };
        drop(state);
        let frame = encode_frame(&response).map_err(io::Error::other)?;
        transport::send_frame(socket, &frame, descriptor.as_ref())?;
    }
}

fn dispatch(
    coordinator: &mut Coordinator<NativeBackend>,
    peer: &AuthenticatedPeer,
    payload: Option<Request>,
    descriptor: Option<OwnedFd>,
) -> io::Result<(Response, Option<OwnedFd>)> {
    if descriptor.is_some()
        && !matches!(
            payload,
            Some(Request::AcquireDirectEgress(_) | Request::ResumeTunnel(_))
        )
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unexpected Agent file descriptor",
        ));
    }
    let caller = owner(peer);
    let response = match payload.ok_or_else(|| denied("Agent request payload missing"))? {
        Request::GetCapabilities(_) => Response::Capabilities(capabilities()),
        Request::GetState(_) => Response::State(state_proto(coordinator, caller.uid)),
        Request::ReplaceTunnel(request) => {
            let plan = request
                .plan
                .ok_or_else(|| denied("Linux replacement plan missing"))?
                .try_into()
                .map_err(|_| denied("invalid Linux replacement plan"))?;
            coordinator.replace(
                &caller,
                parse_operation(&request.source_operation_id)?,
                parse_operation(&request.operation_id)?,
                request.expected_journal_generation,
                plan,
            )?;
            Response::Empty(agent_v1::Empty {})
        }
        Request::AbortReplacement(request) => {
            coordinator.abort_replacement(
                &caller,
                parse_operation(&request.operation_id)?,
                request.expected_journal_generation,
            )?;
            Response::Empty(agent_v1::Empty {})
        }
        Request::PrepareTunnel(request) => {
            let plan = request
                .plan
                .ok_or_else(|| denied("Linux tunnel plan missing"))?
                .try_into()
                .map_err(|_| denied("invalid Linux tunnel plan"))?;
            coordinator.prepare(
                caller,
                parse_operation(&request.operation_id)?,
                request.expected_journal_generation,
                plan,
            )?;
            Response::Empty(agent_v1::Empty {})
        }
        Request::CommitTunnel(request) => {
            coordinator.commit(&caller, parse_operation(&request.operation_id)?)?;
            Response::Empty(agent_v1::Empty {})
        }
        Request::FinalizeTunnel(request) => {
            let plan = request
                .plan
                .ok_or_else(|| denied("Linux final network missing"))?
                .try_into()
                .map_err(|_| denied("invalid Linux final network"))?;
            coordinator.finalize(&caller, parse_operation(&request.operation_id)?, plan)?;
            Response::Empty(agent_v1::Empty {})
        }
        Request::BeginChainTransition(request) => {
            coordinator.begin_chain_transition(&caller, parse_operation(&request.operation_id)?)?;
            Response::Empty(agent_v1::Empty {})
        }
        Request::ResumeTunnel(request) => {
            coordinator.check_owner(&caller, parse_operation(&request.operation_id)?)?;
            if coordinator.state().phase != Phase::Active
                || coordinator
                    .state()
                    .plan
                    .as_ref()
                    .is_none_or(|plan| plan.profile_id.to_string() != request.profile_id)
            {
                return Err(denied("Linux resume identity or phase changed"));
            }
            let resources = coordinator
                .state()
                .resources
                .clone()
                .expect("active resources");
            coordinator.backend_mut().resume_tun(
                &resources,
                descriptor
                    .ok_or_else(|| denied("Linux resume requires the retained TUN descriptor"))?,
            )?;
            coordinator.invalidate_physical_network()?;
            Response::State(state_proto(coordinator, caller.uid))
        }
        Request::ClosePacketSession(request) => {
            // The Engine closes its received descriptor before this call. The
            // Agent retains the creator for reuse; Rollback deletes the exact
            // TUN and revokes every remaining descriptor's device.
            coordinator.check_owner(&caller, parse_operation(&request.operation_id)?)?;
            Response::Empty(agent_v1::Empty {})
        }
        Request::RollbackTunnel(request) => {
            coordinator.rollback(&caller, parse_operation(&request.operation_id)?)?;
            Response::Empty(agent_v1::Empty {})
        }
        Request::AcquireTunnelLease(request) => {
            coordinator.check_owner(&caller, parse_operation(&request.operation_id)?)?;
            Response::Empty(agent_v1::Empty {})
        }
        Request::OpenPacketSession(request) => {
            let id = parse_operation(&request.operation_id)?;
            let descriptor = coordinator.packet_descriptor(&caller, id)?;
            return Ok((
                Response::LinuxTunSession(agent_v1::LinuxTunSession {
                    operation_id: id.to_string(),
                    interface_index: coordinator
                        .state()
                        .resources
                        .as_ref()
                        .and_then(|resources| resources.interface_index)
                        .ok_or_else(|| denied("TUN index unavailable"))?,
                    journal_generation: coordinator.state().generation,
                }),
                Some(descriptor),
            ));
        }
        Request::AcquireDirectEgress(request) => {
            coordinator.check_owner(&caller, parse_operation(&request.operation_id)?)?;
            validate_network_generation(
                request.expected_generation,
                coordinator.state().resources.as_ref(),
            )?;
            let descriptor =
                descriptor.ok_or_else(|| denied("Linux egress requires one socket descriptor"))?;
            let remote = request
                .remote_endpoint
                .parse()
                .map_err(|_| denied("invalid Linux numeric egress endpoint"))?;
            let plan = coordinator.state().plan.clone().expect("active plan");
            let phase = coordinator.state().phase;
            let current = coordinator
                .state()
                .resources
                .clone()
                .expect("validated resources");
            coordinator.backend_mut().validate_egress_purpose(
                &plan,
                &current,
                phase,
                remote,
                request.protocol,
                request.purpose,
            )?;
            // Reserve only after request validation. grant_egress returns an
            // unused reservation to the free list on every other outcome.
            let (resources, reserved_mark) = coordinator.reserve_egress_mark(
                &caller,
                parse_operation(&request.operation_id)?,
                request.expected_generation,
            )?;
            let (physical_index, lease_id) = coordinator.backend_mut().grant_egress(
                &resources,
                peer,
                descriptor.as_fd(),
                remote,
                request.protocol,
                reserved_mark,
            )?;
            Response::DirectEgressLease(agent_v1::DirectEgressLease {
                remote_endpoint: request.remote_endpoint,
                protocol: request.protocol,
                network_generation: resources.network_generation,
                // Windows LUID fields have no Linux meaning; never synthesize one.
                interface_luid: 0,
                interface_index: physical_index,
                linux_lease_id: lease_id,
            })
        }
        Request::ReleaseLinuxDirectEgress(request) => {
            coordinator.check_owner(&caller, parse_operation(&request.operation_id)?)?;
            let resources = coordinator
                .state()
                .resources
                .clone()
                .expect("active resources");
            if request.network_generation > resources.network_generation {
                return Err(denied("Linux egress generation is from the future"));
            }
            coordinator
                .backend_mut()
                .release_egress(&resources, request.lease_id)?;
            Response::Empty(agent_v1::Empty {})
        }
        Request::SetPresetEndpointCandidates(request) => {
            coordinator.check_owner(&caller, parse_operation(&request.operation_id)?)?;
            validate_network_generation(
                request.expected_generation,
                coordinator.state().resources.as_ref(),
            )?;
            let candidates = request
                .ipv6_candidates
                .iter()
                .map(|address| {
                    address
                        .parse()
                        .map_err(|_| denied("invalid Linux preset candidate"))
                })
                .collect::<io::Result<Vec<_>>>()?;
            let plan = coordinator.state().plan.clone().expect("active plan");
            let resources = coordinator
                .state()
                .resources
                .clone()
                .expect("active resources");
            coordinator.backend_mut().set_preset_candidates(
                &plan,
                &resources,
                &request.hostname,
                request.expected_generation,
                candidates,
            )?;
            Response::Empty(agent_v1::Empty {})
        }
        Request::RestartAutomaticRecovery(request) => {
            let alive = coordinator.state().owner.as_ref().is_some_and(owner_alive);
            coordinator.restart_automatic_recovery(
                caller.uid,
                parse_operation(&request.operation_id)?,
                request.expected_journal_generation,
                alive,
            )?;
            Response::Empty(agent_v1::Empty {})
        }
        Request::RecoverOrphaned(request) => {
            let alive = coordinator.state().owner.as_ref().is_some_and(owner_alive);
            coordinator.recover_orphaned(
                caller.uid,
                parse_operation(&request.operation_id)?,
                request.expected_journal_generation,
                alive,
            )?;
            Response::Empty(agent_v1::Empty {})
        }
        Request::GetPhysicalNetworkInfo(request) => {
            coordinator.check_owner(&caller, parse_operation(&request.operation_id)?)?;
            let resources = coordinator
                .state()
                .resources
                .as_ref()
                .expect("active resources");
            Response::PhysicalNetworkInfo(agent_v1::PhysicalNetworkInfo {
                generation: resources.network_generation,
                interfaces: resources
                    .physical_interfaces
                    .iter()
                    .map(|link| agent_v1::PhysicalInterface {
                        interface_luid: 0,
                        interface_index: link.index,
                        address_family_mask: link.family_mask,
                        dns_servers: link.dns_servers.iter().map(ToString::to_string).collect(),
                    })
                    .collect(),
            })
        }
        // Unscoped Recover is maintenance-only; unsupported Windows/chain
        // messages never silently become a weaker Linux action.
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "Agent operation is unavailable on Linux",
            ));
        }
    };
    Ok((response, None))
}

pub fn capabilities() -> agent_v1::AgentCapabilities {
    agent_v1::AgentCapabilities {
        operating_system: "linux".into(),
        architecture: std::env::consts::ARCH.into(),
        protocol_version: crate::AGENT_PROTOCOL_VERSION,
        linux_tun: true,
        nftables_kill_switch: true,
        unix_fd_transfer: true,
        interface_addresses: true,
        interface_dns: true,
        dynamic_direct_egress: true,
        exact_generation_egress: true,
        guarded_recovery: true,
        automatic_recovery: true,
        physical_dns_snapshot: true,
        deferred_network_configuration: true,
        automatic_endpoint_leases: true,
        endpoint_preset_leases: true,
        protected_tunnel_replacement: true,
        ..Default::default()
    }
}

fn state_proto(coordinator: &Coordinator<NativeBackend>, uid: u32) -> agent_v1::AgentState {
    let journal = coordinator.state();
    let phase = match journal.phase {
        Phase::Clean => agent_v1::AgentPhase::Clean,
        Phase::Preparing => agent_v1::AgentPhase::Preparing,
        Phase::Prepared => agent_v1::AgentPhase::Prepared,
        Phase::Active => agent_v1::AgentPhase::Active,
        Phase::Recovering => agent_v1::AgentPhase::Recovering,
        Phase::RecoveryRequired => agent_v1::AgentPhase::RecoveryRequired,
    };
    let mut state = agent_v1::AgentState {
        phase: phase as i32,
        journal_generation: journal.generation,
        ..Default::default()
    };
    if journal.owner.as_ref().is_some_and(|owner| owner.uid != uid) {
        state.warnings.push("linux_owned_by_another_user".into());
        return state;
    }
    state.operation_id = journal
        .operation_id
        .map(|id| id.to_string())
        .unwrap_or_default();
    state.profile_id = journal
        .plan
        .as_ref()
        .map(|plan| plan.profile_id.to_string())
        .unwrap_or_default();
    state.plan = journal.plan.as_ref().map(|plan| Box::new(plan.to_proto()));
    let recovery = &journal.automatic_recovery;
    let automatic_phase = match recovery.phase {
        super::journal::AutomaticPhase::Inactive => agent_v1::AutomaticRecoveryPhase::Inactive,
        super::journal::AutomaticPhase::Waiting => agent_v1::AutomaticRecoveryPhase::Waiting,
        super::journal::AutomaticPhase::Running => agent_v1::AutomaticRecoveryPhase::Running,
        super::journal::AutomaticPhase::Exhausted => agent_v1::AutomaticRecoveryPhase::Exhausted,
        super::journal::AutomaticPhase::Blocked => agent_v1::AutomaticRecoveryPhase::Blocked,
    };
    state.automatic_recovery = Some(agent_v1::AutomaticRecoveryStatus {
        phase: automatic_phase as i32,
        attempts_completed: u32::from(recovery.attempts),
        attempt_limit: u32::from(super::journal::RECOVERY_ATTEMPT_LIMIT),
        terminal_error: matches!(recovery.phase, super::journal::AutomaticPhase::Exhausted | super::journal::AutomaticPhase::Blocked).then(|| agent_v1::AgentError {
            code: "linux_recovery_required".into(),
            message: "Linux network recovery requires attention; protection and recovery evidence were retained".into(),
            retryable: true,
        }),
    });
    state.kill_switch_active = coordinator.protection_active();
    state.linux_platform = Some(super::backend::inspect_platform(journal));
    if let Some(replacement) = &journal.replacement {
        state.replacement = Some(Box::new(agent_v1::TunnelReplacementStatus {
            source_operation_id: journal
                .operation_id
                .map(|id| id.to_string())
                .unwrap_or_default(),
            operation_id: replacement.operation_id.to_string(),
            phase: if replacement.state == super::journal::StepState::Applied {
                agent_v1::TunnelReplacementPhase::SourceCleanup
            } else {
                agent_v1::TunnelReplacementPhase::InstallingGuard
            } as i32,
            guard_active: state.kill_switch_active,
            source_journal_generation: journal.generation,
            target_plan: Some(Box::new(replacement.plan.to_proto())),
        }));
    } else if let Some(source) = journal.replacement_origin {
        state.replacement = Some(Box::new(agent_v1::TunnelReplacementStatus {
            source_operation_id: source.to_string(),
            operation_id: journal
                .operation_id
                .map(|id| id.to_string())
                .unwrap_or_default(),
            phase: if journal.phase == Phase::Active {
                agent_v1::TunnelReplacementPhase::Complete
            } else if journal.phase == Phase::Prepared {
                agent_v1::TunnelReplacementPhase::Prepared
            } else {
                agent_v1::TunnelReplacementPhase::PreparingTarget
            } as i32,
            guard_active: state.kill_switch_active,
            source_journal_generation: journal.generation,
            target_plan: journal
                .replacement_initial_plan
                .as_ref()
                .map(|plan| Box::new(plan.to_proto())),
        }));
    }
    state
}

fn schedule_orphan_recovery(service: Arc<Mutex<Service>>) {
    let snapshot = service.lock().ok().and_then(|state| {
        state
            .coordinator
            .state()
            .operation_id
            .map(|operation| (operation, state.lease_epoch))
    });
    thread::spawn(move || {
        thread::sleep(ORPHAN_GRACE);
        if let Ok(mut state) = service.lock() {
            if state.stopped
                || snapshot.is_none()
                || state
                    .coordinator
                    .state()
                    .operation_id
                    .zip(Some(state.lease_epoch))
                    != snapshot
            {
                return;
            }
            let key = state
                .coordinator
                .state()
                .owner
                .as_ref()
                .map(|owner| (owner.uid, owner.process_id, owner.process_start_time));
            if key.is_some_and(|key| state.connections.contains_key(&key))
                && state.coordinator.backend_mut().has_tun()
            {
                return;
            }
            if let Err(error) = state.coordinator.recover() {
                tracing::error!(kind = ?error.kind(), "Linux orphan recovery failed; protection and journal retained");
            }
        }
    });
}

fn owner(peer: &AuthenticatedPeer) -> Owner {
    Owner {
        uid: peer.credentials.uid,
        process_id: peer.credentials.process_id,
        process_start_time: peer.start_time,
    }
}
fn owner_key(peer: &AuthenticatedPeer) -> OwnerKey {
    (
        peer.credentials.uid,
        peer.credentials.process_id,
        peer.start_time,
    )
}
fn owner_alive(owner: &Owner) -> bool {
    super::auth::process_start_time(owner.process_id)
        .is_ok_and(|time| time == owner.process_start_time)
}
fn parse_operation(value: &str) -> io::Result<Uuid> {
    Uuid::parse_str(value).map_err(|_| denied("invalid Linux operation ID"))
}
fn denied(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, message)
}

#[derive(Debug)]
struct StaleNetworkGeneration;

impl std::fmt::Display for StaleNetworkGeneration {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Linux egress generation changed")
    }
}

impl std::error::Error for StaleNetworkGeneration {}

fn validate_network_generation(
    expected: u64,
    resources: Option<&super::journal::Resources>,
) -> io::Result<()> {
    let resources = resources.ok_or_else(|| denied("Linux network is absent"))?;
    if expected != resources.network_generation {
        return Err(io::Error::other(StaleNetworkGeneration));
    }
    Ok(())
}

fn agent_error(error: io::Error) -> agent_v1::AgentError {
    let stale_generation = error
        .get_ref()
        .is_some_and(|error| error.is::<StaleNetworkGeneration>());
    agent_v1::AgentError {
        code: if stale_generation {
            "linux_stale_generation"
        } else {
            match error.kind() {
                io::ErrorKind::PermissionDenied => "linux_permission_denied",
                io::ErrorKind::Unsupported => "linux_unavailable",
                _ => "linux_operation_failed",
            }
        }
        .into(),
        message: error.to_string(),
        retryable: stale_generation || error.kind() != io::ErrorKind::PermissionDenied,
    }
}

fn require_root() -> io::Result<()> {
    // SAFETY: geteuid returns a scalar and has no preconditions.
    if unsafe { libc::geteuid() } != 0 {
        return Err(denied("Linux Agent service requires root"));
    }
    Ok(())
}

fn service_lock() -> io::Result<File> {
    let path = Path::new("/run/usque/agent.lock");
    super::auth::verify_trusted_parents(path)?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?;
    let metadata = file.metadata()?;
    if metadata.uid() != 0
        || !metadata.is_file()
        || metadata.nlink() != 1
        || metadata.mode() & 0o077 != 0
    {
        return Err(denied("unsafe Linux Agent lock"));
    }
    // SAFETY: fd is retained for the entire service/maintenance lifetime.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(file)
}

fn bind_listener() -> io::Result<UnixListener> {
    super::auth::verify_trusted_parents(Path::new(SOCKET_PATH))?;
    remove_owned_socket()?;
    let listener = UnixListener::bind(SOCKET_PATH)?;
    transport::enable_credentials(&listener)?;
    fs::set_permissions(SOCKET_PATH, fs::Permissions::from_mode(0o666))?;
    Ok(listener)
}

fn remove_owned_socket() -> io::Result<()> {
    match fs::symlink_metadata(SOCKET_PATH) {
        Ok(metadata)
            if metadata.file_type().is_socket() && metadata.uid() == 0 && metadata.nlink() == 1 =>
        {
            fs::remove_file(SOCKET_PATH)
        }
        Ok(_) => Err(denied("foreign Linux Agent socket path")),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_typed_generation_mismatch_gets_the_retryable_network_code() {
        let mut resources = super::super::journal::Resources::for_operation(Uuid::new_v4());
        resources.network_generation = 7;
        assert!(validate_network_generation(7, Some(&resources)).is_ok());
        let stale = agent_error(validate_network_generation(6, Some(&resources)).unwrap_err());
        assert_eq!(stale.code, "linux_stale_generation");
        assert!(stale.retryable);
        for error in [
            denied("Linux egress generation changed"),
            validate_network_generation(6, None).unwrap_err(),
        ] {
            let denied = agent_error(error);
            assert_eq!(denied.code, "linux_permission_denied");
            assert!(!denied.retryable);
        }
    }

    #[test]
    fn linux_capabilities_never_advertise_windows_handles_or_unimplemented_lifecycle() {
        let value = capabilities();
        assert!(value.linux_tun && value.nftables_kill_switch && value.unix_fd_transfer);
        assert!(!value.wintun && !value.shared_packet_ring && !value.system_proxy);
        assert!(value.deferred_network_configuration && value.protected_tunnel_replacement);
    }
}
