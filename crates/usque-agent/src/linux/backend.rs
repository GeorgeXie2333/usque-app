use std::{
    collections::HashMap,
    fs::{self, File},
    io, mem,
    net::SocketAddr,
    os::{
        fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd, OwnedFd},
        unix::fs::MetadataExt,
    },
    path::Path,
    ptr,
};

use ipnet::IpNet;
use serde_json::{Value, json};

use crate::plan::ValidatedTunnelPlan;
use usque_ipc::{
    agent_v1::{
        LinuxDnsBackend, LinuxFailureDetail, LinuxFailureReason as FailureReason,
        LinuxFailureStage as FailureStage,
    },
    linux_failure,
};

use super::{
    auth::AuthenticatedPeer,
    command,
    coordinator::Backend,
    firewall::{self, EgressLease},
    journal::{PhysicalInterface, Resources, Step},
    network,
};

const BUSCTL: &str = "/usr/bin/busctl";
const RESOLVE_SERVICE: &str = "org.freedesktop.resolve1";
const RESOLVE_PATH: &str = "/org/freedesktop/resolve1";
const RESOLVE_MANAGER: &str = "org.freedesktop.resolve1.Manager";

#[derive(Default)]
pub struct NativeBackend {
    tun: Option<File>,
    leases: Vec<EgressLease>,
    preset_candidates: Vec<SocketAddr>,
    preset_generation: u64,
    sockets: HashMap<u32, OwnedFd>,
    reusable_marks: Vec<u16>,
    binding_indices: HashMap<u32, u32>,
    owned_resources: Vec<Resources>,
    pub(crate) observation: super::coordinator::PhysicalObservation,
    pub(crate) egress_cleanup_uncertain: bool,
}

impl Backend for NativeBackend {
    fn take_reusable_egress_mark(&mut self) -> Option<u16> {
        self.reusable_marks.pop()
    }
    fn protection_active(&self, resources: &Resources, plan: &ValidatedTunnelPlan) -> bool {
        firewall::inspect(resources, &exclusions(plan), &self.leases).is_ok()
    }
    fn install_bridge(
        &mut self,
        resources: &mut Resources,
        plan: &ValidatedTunnelPlan,
    ) -> io::Result<()> {
        self.owned_resources
            .retain(|r| r.route_table != resources.route_table);
        self.owned_resources.push(resources.clone());
        self.observation.routing_baseline =
            Some(network::routing_fingerprint(&self.owned_resources)?);
        firewall::install_bridge(resources, &exclusions(plan))
    }
    fn invalidate_network(&mut self, resources: &Resources) -> io::Result<()> {
        self.revoke_egress(resources)
    }
    fn preflight(&self, plan: &ValidatedTunnelPlan) -> io::Result<()> {
        plan.validate()
            .map_err(|_| unavailable("invalid Linux tunnel plan"))?;
        super::auth::open_trusted_file(Path::new("/usr/sbin/nft"))?;
        super::auth::open_trusted_file(Path::new(BUSCTL))?;
        Ok(())
    }

    fn initialize_guard(&self, resources: &mut Resources) -> io::Result<()> {
        // Freeze this before installing or journaling the guard. Later DNS and
        // physical snapshots must retain its exact policy, including legacy
        // guards whose absent receipt allowed only root's DHCP traffic.
        if resources.dhcp_client_uids.is_none() {
            resources.dhcp_client_uids = Some(dhcp_client_uids());
        }
        Ok(())
    }

    fn initialize_resources(&self, resources: &mut Resources) -> io::Result<()> {
        resources.dns = select_dns_backend(active_resolver(), super::dns::capture_network_manager)?;
        resources.physical_interfaces = physical_snapshot(&resources.dns)
            .map_err(|e| linux_failure::wrap(FailureStage::PhysicalSnapshot, e))?;
        Ok(())
    }

    fn apply(
        &mut self,
        step: Step,
        resources: &mut Resources,
        plan: &ValidatedTunnelPlan,
    ) -> io::Result<()> {
        match step {
            Step::Protection => {
                self.owned_resources
                    .retain(|r| r.route_table != resources.route_table);
                self.owned_resources.push(resources.clone());
                self.observation.routing_baseline =
                    Some(network::routing_fingerprint(&self.owned_resources)?);
                self.observation.unavailable = false;
                self.observation.invalidated = false;
                network::assert_route_ids_available(resources)?;
                firewall::install(resources, &exclusions(plan))?;
                self.leases.clear();
                self.egress_cleanup_uncertain = false;
                self.reusable_marks.clear();
            }
            Step::Tun => {
                network::assert_route_ids_available(resources)?;
                self.tun = Some(network::create_tun(resources)?);
                self.owned_resources
                    .retain(|r| r.route_table != resources.route_table);
                self.owned_resources.push(resources.clone());
            }
            Step::Addresses => network::configure(
                resources,
                plan.assigned_ipv4.into_iter().chain(plan.assigned_ipv6),
                plan.mtu,
            )?,
            Step::Dns => {
                match &resources.dns {
                    super::dns::DnsReceipt::Resolved => configure_dns(resources, plan)?,
                    super::dns::DnsReceipt::NetworkManager { previous } => {
                        super::dns::apply_network_manager(previous, &plan.dns_servers)?
                    }
                }
                network::set_up(resources)?;
            }
            Step::Routes => network::install_routes(
                resources,
                plan.assigned_ipv4.is_some(),
                plan.assigned_ipv6.is_some(),
                &exclusions(plan),
                &plan.dns_servers,
            )?,
        }
        Ok(())
    }

    fn restore(
        &mut self,
        step: Step,
        resources: &Resources,
        plan: &ValidatedTunnelPlan,
    ) -> io::Result<()> {
        match step {
            Step::Routes => network::remove_routes(
                resources,
                plan.assigned_ipv4.is_some(),
                plan.assigned_ipv6.is_some(),
                &exclusions(plan),
                &plan.dns_servers,
            )?,
            Step::Dns => match &resources.dns {
                super::dns::DnsReceipt::Resolved => restore_dns(resources, plan)?,
                super::dns::DnsReceipt::NetworkManager { previous } => {
                    super::dns::restore_network_manager(previous, &plan.dns_servers)?
                }
            },
            Step::Addresses => {
                network::remove_addresses(
                    resources,
                    plan.assigned_ipv4.into_iter().chain(plan.assigned_ipv6),
                )?;
            }
            Step::Tun => {
                network::remove_tun(resources)?;
                self.tun = None;
            }
            Step::Protection => {
                // Never remove the host guard after uncertain socket cleanup.
                // An Intended protection step may have created no table at all.
                if !self.sockets.is_empty() || !self.leases.is_empty() {
                    self.revoke_egress(resources)?;
                }
                firewall::remove(resources)?;
                self.sockets.clear();
                self.binding_indices.clear();
                self.leases.clear();
                self.egress_cleanup_uncertain = false;
                self.owned_resources
                    .retain(|r| r.route_table != resources.route_table);
            }
        }
        Ok(())
    }

    fn duplicate_tun(&self, resources: &Resources) -> io::Result<OwnedFd> {
        network::verify_interface(resources)?.ok_or_else(|| unavailable("Linux TUN is absent"))?;
        self.tun
            .as_ref()
            .ok_or_else(|| unavailable("Linux TUN descriptor is not retained"))?
            .try_clone()
            .map(Into::into)
    }
}

impl NativeBackend {
    pub fn has_tun(&self) -> bool {
        self.tun.is_some()
    }

    pub fn resume_tun(&mut self, resources: &Resources, descriptor: OwnedFd) -> io::Result<()> {
        network::verify_tun_descriptor(resources, &descriptor)?;
        self.tun = Some(descriptor.into());
        self.owned_resources
            .retain(|r| r.route_table != resources.route_table);
        self.owned_resources.push(resources.clone());
        Ok(())
    }

    pub fn routing_snapshot(&self) -> io::Result<Vec<Vec<u8>>> {
        network::routing_fingerprint(&self.owned_resources)
    }

    /// The descriptor must be in this authenticated process's fd table and
    /// owned by its UID. A mark is assigned only to that kernel socket, while
    /// nftables restricts the mark to the exact numeric destination/protocol.
    #[expect(
        clippy::too_many_arguments,
        reason = "socket permit includes its durably reserved mark and authenticated owner"
    )]
    pub fn grant_egress(
        &mut self,
        resources: &Resources,
        peer: &AuthenticatedPeer,
        descriptor: BorrowedFd<'_>,
        remote: SocketAddr,
        protocol: u32,
        mark: u32,
    ) -> io::Result<(u32, u64)> {
        let result =
            self.grant_reserved_egress(resources, peer, descriptor, remote, protocol, mark);
        let granted = matches!(&result, Ok((_, lease_id))
            if self.leases.iter().any(|lease| lease.lease_id == *lease_id && lease.mark == mark));
        // A failed or reused-lease request leaves its reservation unused. Read
        // the kernel's mark: a socket still carrying it keeps the slot retired.
        if !granted
            && socket_option::<u32>(descriptor, libc::SO_MARK).is_ok_and(|current| current != mark)
        {
            self.recycle_mark(resources, mark);
        }
        result
    }

    fn recycle_mark(&mut self, resources: &Resources, mark: u32) {
        let slot = mark & 0xffff;
        if mark & 0xffff_0000 == resources.socket_mark
            && slot != 0
            && slot <= u32::from(resources.egress_mark_highwater)
            && !self.sockets.contains_key(&mark)
            && self.leases.iter().all(|lease| lease.mark != mark)
            && !self.reusable_marks.contains(&(slot as u16))
        {
            self.reusable_marks.push(slot as u16);
        }
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "socket permit includes its durably reserved mark and authenticated owner"
    )]
    fn grant_reserved_egress(
        &mut self,
        resources: &Resources,
        peer: &AuthenticatedPeer,
        descriptor: BorrowedFd<'_>,
        remote: SocketAddr,
        protocol: u32,
        mark: u32,
    ) -> io::Result<(u32, u64)> {
        if remote.port() == 0
            || remote.ip().is_unspecified()
            || remote.ip().is_multicast()
            || !matches!(protocol, 6 | 17)
        {
            return Err(denied("invalid Linux egress target"));
        }
        peer.ensure_alive()?;
        verify_socket_owner(descriptor, peer)?;
        if self.observation.unavailable {
            return Err(linux_failure::wrap(
                FailureStage::PhysicalSnapshot,
                unavailable("physical snapshot unavailable"),
            ));
        }
        let domain: i32 = socket_option(descriptor, libc::SO_DOMAIN)?;
        let socket_protocol: i32 = socket_option(descriptor, libc::SO_PROTOCOL)?;
        let socket_type: i32 = socket_option(descriptor, libc::SO_TYPE)?;
        if domain
            != if remote.is_ipv4() {
                libc::AF_INET
            } else {
                libc::AF_INET6
            }
            || socket_protocol != protocol as i32
            || socket_type
                != if protocol == 6 {
                    libc::SOCK_STREAM
                } else {
                    libc::SOCK_DGRAM
                }
        {
            return Err(denied("Linux egress socket type does not match target"));
        }
        let cookie: u64 = socket_option(descriptor, libc::SO_COOKIE)?;
        let current_mark: u32 = socket_option(descriptor, libc::SO_MARK)?;
        if let Some(existing) = self
            .leases
            .iter()
            .find(|lease| lease.socket_cookie == cookie)
        {
            if existing.remote == remote
                && existing.protocol == protocol
                && existing.mark == current_mark
            {
                let index = *self
                    .binding_indices
                    .get(&existing.mark)
                    .ok_or_else(|| denied("retained socket binding unavailable"))?;
                if bound_device(descriptor)? != network::interface_name(index)? {
                    return Err(denied("retained socket binding changed"));
                }
                let query = socket_route_query(
                    descriptor,
                    remote,
                    protocol,
                    existing.mark,
                    peer.credentials.uid,
                    Some(index),
                )?;
                if network::lookup_physical_route(resources, &query)? != index {
                    return Err(denied("retained socket route changed"));
                }
                return Ok((index, existing.lease_id));
            }
            return Err(denied("Linux egress socket already has a different permit"));
        }
        if current_mark != 0
            && (current_mark & 0xffff_0000 != resources.socket_mark
                || current_mark & 0xffff == 0
                || current_mark & 0xffff > u32::from(resources.egress_mark_highwater))
        {
            return Err(denied("Linux egress socket already has an external mark"));
        }
        if self.leases.len() >= 1024 {
            return Err(denied("Linux egress lease limit reached"));
        }
        if resources.egress_mark_highwater == 0 {
            return Err(denied("Linux egress mark was not reserved"));
        }
        let retained = descriptor.try_clone_to_owned()?;
        if mark & 0xffff_0000 != resources.socket_mark
            || mark & 0xffff == 0
            || mark & 0xffff > u32::from(resources.egress_mark_highwater)
        {
            return Err(denied("Linux egress mark was not allocated"));
        }
        if !bound_device(descriptor)?.is_empty() {
            return Err(denied("fresh socket has an external device binding"));
        }
        // DNS keeps its explicit physical owner/scope constraint. Ordinary
        // destinations use the kernel FIB, not the main-default candidate list.
        let owner = if remote.port() == 53 {
            Some(egress_interface(resources, remote)?.index)
        } else {
            None
        };
        let query = socket_route_query(
            descriptor,
            remote,
            protocol,
            mark,
            peer.credentials.uid,
            owner,
        )?;
        let physical_index = network::lookup_physical_route(resources, &query)
            .map_err(|e| linux_failure::wrap(FailureStage::EgressPermission, e))?;
        if self
            .owned_resources
            .iter()
            .any(|r| r.interface_index == Some(physical_index))
        {
            return Err(denied("egress selected an owned TUN"));
        }
        // Retain before any socket mutation. Failed cleanup keeps the descriptor
        // and retires the mark until recovery can confirm both fields cleared.
        self.sockets.insert(mark, retained);
        self.binding_indices.insert(mark, physical_index);
        let activation = (|| {
            set_device(descriptor, Some(physical_index))?;
            let constrained = network::RouteQuery {
                interface: Some(physical_index),
                ..query
            };
            network::lookup_physical_route(resources, &constrained)?;
            set_mark(descriptor, mark)
        })();
        if let Err(error) = activation {
            if self.clear_retained_socket(mark).is_err() {
                self.egress_cleanup_uncertain = true;
            }
            return Err(error);
        }
        let lease_id = loop {
            let candidate = uuid::Uuid::new_v4().as_u64_pair().1;
            if candidate != 0 && self.leases.iter().all(|lease| lease.lease_id != candidate) {
                break candidate;
            }
        };
        let lease = EgressLease {
            mark,
            remote,
            protocol,
            socket_cookie: cookie,
            lease_id,
        };
        let mut leases = self.leases.clone();
        leases.push(lease);
        // The mark alone cannot grant egress before the exact nft rule exists.
        if let Err(error) = firewall::update_egress(resources, &leases) {
            // The command may have committed before its reply was lost.
            if let Some(socket) = self.sockets.get(&mark) {
                let _ = clear_socket(socket.as_fd());
            }
            self.egress_cleanup_uncertain = true;
            return Err(error);
        }
        self.leases = leases;
        Ok((physical_index, lease_id))
    }

    pub fn revoke_egress(&mut self, resources: &Resources) -> io::Result<()> {
        // Remove authorization even if one socket refuses field cleanup.
        let rules = firewall::update_egress(resources, &[]);
        let sockets = self.clear_socket_marks();
        if rules.is_err() || sockets.is_err() {
            self.egress_cleanup_uncertain = true;
        }
        rules?;
        sockets?;
        self.sockets.clear();
        self.binding_indices.clear();
        for lease in mem::take(&mut self.leases) {
            self.recycle_mark(resources, lease.mark);
        }
        self.preset_candidates.clear();
        self.preset_generation = 0;
        Ok(())
    }

    pub fn release_egress(&mut self, resources: &Resources, lease_id: u64) -> io::Result<()> {
        let Some(lease) = self.leases.iter().find(|lease| lease.lease_id == lease_id) else {
            return Ok(());
        };
        let mark = lease.mark;
        let Some(socket) = self.sockets.get(&mark) else {
            return Err(denied("Linux permit socket is not retained"));
        };
        let leases: Vec<_> = self
            .leases
            .iter()
            .filter(|lease| lease.lease_id != lease_id)
            .cloned()
            .collect();
        let rules = firewall::update_egress(resources, &leases);
        let socket = clear_socket(socket.as_fd());
        if rules.is_err() || socket.is_err() {
            self.egress_cleanup_uncertain = true;
        }
        rules?;
        socket?;
        self.leases = leases;
        self.sockets.remove(&mark);
        self.binding_indices.remove(&mark);
        // Reuse is allowed only after both the retained kernel socket mark and
        // its nft rule are gone. Restart drops this volatile free list; durable
        // highwater prevents reusing any unknown surviving socket's mark.
        self.reusable_marks.push((mark & 0xffff) as u16);
        Ok(())
    }

    fn clear_retained_socket(&mut self, mark: u32) -> io::Result<()> {
        self.clear_retained_socket_using(mark, clear_socket)
    }
    fn clear_retained_socket_using(
        &mut self,
        mark: u32,
        clear: impl FnOnce(BorrowedFd<'_>) -> io::Result<()>,
    ) -> io::Result<()> {
        if let Some(descriptor) = self.sockets.get(&mark) {
            clear(descriptor.as_fd())?;
        }
        self.sockets.remove(&mark);
        self.binding_indices.remove(&mark);
        Ok(())
    }
    fn clear_socket_marks(&mut self) -> io::Result<()> {
        let mut failure = None;
        for socket in self.sockets.values() {
            if let Err(error) = clear_socket(socket.as_fd()) {
                failure.get_or_insert(error);
            }
        }
        if let Some(error) = failure {
            self.egress_cleanup_uncertain = true;
            return Err(error);
        }
        Ok(())
    }

    pub fn set_preset_candidates(
        &mut self,
        plan: &ValidatedTunnelPlan,
        resources: &Resources,
        hostname: &str,
        generation: u64,
        mut candidates: Vec<SocketAddr>,
    ) -> io::Result<()> {
        let policy = plan
            .preset_endpoint_policy
            .as_ref()
            .ok_or_else(|| denied("Linux preset policy unavailable"))?;
        if generation != resources.network_generation || hostname != policy.hostname {
            return Err(denied("Linux preset generation or hostname changed"));
        }
        policy
            .validate_candidates(&candidates)
            .map_err(|_| denied("invalid Linux preset candidates"))?;
        candidates.sort();
        candidates.dedup();
        self.preset_candidates = candidates;
        self.preset_generation = generation;
        Ok(())
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "authorization checks the complete typed request against its session"
    )]
    pub fn validate_egress_purpose(
        &self,
        plan: &ValidatedTunnelPlan,
        resources: &Resources,
        phase: super::journal::Phase,
        remote: SocketAddr,
        protocol: u32,
        purpose: i32,
    ) -> io::Result<()> {
        if !matches!(protocol, 6 | 17) {
            return Err(denied("invalid Linux egress protocol"));
        }
        let allowed = match purpose {
            0 => {
                phase == super::journal::Phase::Active
                    || protocol == 6 && plan.control_api_candidates.contains(&remote)
                    || plan.preset_endpoint_policy.is_none()
                        && plan.automatic_endpoint_policy.is_none()
                        && plan.endpoint_candidates.contains(&remote)
            }
            1 => plan.automatic_endpoint_policy.is_some_and(|policy| {
                policy.permits(
                    remote,
                    if protocol == 6 {
                        usque_core::Transport::Http2
                    } else {
                        usque_core::Transport::Http3
                    },
                )
            }),
            2 => {
                plan.preset_endpoint_policy.is_some()
                    && remote.port() == 53
                    && egress_interface(resources, remote)
                        .is_ok_and(|link| link.dns_servers.contains(&remote.ip()))
            }
            3 => {
                self.preset_generation == resources.network_generation
                    && plan.preset_endpoint_policy.as_ref().is_some_and(|policy| {
                        policy.permits_protocol(protocol as u8)
                            && (policy.ipv4_fallback == Some(remote)
                                || self.preset_candidates.contains(&remote))
                    })
            }
            _ => false,
        };
        if !allowed {
            return Err(denied(
                "Linux egress target does not match its authorized purpose",
            ));
        }
        Ok(())
    }
}

fn select_dns_backend(
    resolved: io::Result<()>,
    manager: impl FnOnce() -> io::Result<super::dns::DnsReceipt>,
) -> io::Result<super::dns::DnsReceipt> {
    Ok(match resolved {
        Ok(()) => super::dns::DnsReceipt::Resolved,
        Err(resolved) => match manager() {
            Ok(receipt) => receipt,
            Err(manager) => {
                return Err(linux_failure::with_detail(
                    LinuxFailureDetail {
                        stage: FailureStage::DnsProbe as i32,
                        reason: FailureReason::NoDnsBackend as i32,
                        dns_probe_failures: vec![
                            linux_failure::probe(LinuxDnsBackend::Resolved, &resolved),
                            linux_failure::probe(LinuxDnsBackend::NetworkManager, &manager),
                        ],
                        ..Default::default()
                    },
                    manager,
                ));
            }
        },
    })
}

fn egress_interface(resources: &Resources, remote: SocketAddr) -> io::Result<&PhysicalInterface> {
    let mask = if remote.is_ipv4() { 1 } else { 2 };
    let scope = match remote {
        SocketAddr::V6(address) => address.scope_id(),
        SocketAddr::V4(_) => 0,
    };
    let supports_target = |link: &PhysicalInterface| {
        link.family_mask & mask != 0 && (scope == 0 || link.index == scope)
    };
    // A link-local DNS address can exist on several links. Honor its explicit
    // scope before choosing the device, and prefer the snapshot's DNS owner
    // for unscoped resolver addresses reachable only on that physical link.
    resources
        .physical_interfaces
        .iter()
        .find(|link| {
            supports_target(link) && remote.port() == 53 && link.dns_servers.contains(&remote.ip())
        })
        .or_else(|| {
            resources
                .physical_interfaces
                .iter()
                .find(|link| supports_target(link))
        })
        .ok_or_else(|| denied("no physical interface supports the egress family"))
}

fn verify_socket_owner(descriptor: BorrowedFd<'_>, peer: &AuthenticatedPeer) -> io::Result<()> {
    // SAFETY: zeroed stat is output-only storage for fstat.
    let mut metadata: libc::stat = unsafe { mem::zeroed() };
    // SAFETY: descriptor is borrowed and metadata points to complete storage.
    if unsafe { libc::fstat(descriptor.as_raw_fd(), &mut metadata) } != 0 {
        return Err(io::Error::last_os_error());
    }
    if metadata.st_mode & libc::S_IFMT != libc::S_IFSOCK || metadata.st_uid != peer.credentials.uid
    {
        return Err(denied(
            "Linux egress descriptor is not a socket owned by the caller",
        ));
    }
    let namespace = fs::metadata(format!("/proc/{}/ns/net", peer.credentials.process_id))?;
    let local = fs::metadata("/proc/self/ns/net")?;
    if namespace.ino() != local.ino() || namespace.dev() != local.dev() {
        return Err(denied("Engine network namespace differs from Agent"));
    }
    // A process can receive a socket from another network namespace while
    // remaining in ours; checking only /proc/pid/ns/net would miss that case.
    // SAFETY: socket takes scalar arguments and returns a new owned descriptor.
    let raw = unsafe { libc::socket(libc::AF_INET, libc::SOCK_DGRAM | libc::SOCK_CLOEXEC, 0) };
    if raw < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful socket transferred this descriptor to us.
    let local_socket = unsafe { OwnedFd::from_raw_fd(raw) };
    if socket_option::<u64>(descriptor, libc::SO_NETNS_COOKIE)?
        != socket_option::<u64>(local_socket.as_fd(), libc::SO_NETNS_COOKIE)?
    {
        return Err(denied(
            "Linux egress socket belongs to another network namespace",
        ));
    }
    let mut owned = false;
    for entry in fs::read_dir(format!("/proc/{}/fd", peer.credentials.process_id))?.take(8193) {
        let entry = entry?;
        if let Ok(candidate) = fs::metadata(entry.path())
            && candidate.dev() == metadata.st_dev
            && candidate.ino() == metadata.st_ino
        {
            owned = true;
            break;
        }
    }
    if !owned {
        return Err(denied(
            "Linux egress descriptor is not held by the authenticated Engine",
        ));
    }
    Ok(())
}

fn socket_option<T: Copy>(descriptor: BorrowedFd<'_>, option: i32) -> io::Result<T> {
    let mut value = mem::MaybeUninit::<T>::uninit();
    let mut length = mem::size_of::<T>() as libc::socklen_t;
    // SAFETY: value points to output space of exactly the size supplied.
    if unsafe {
        libc::getsockopt(
            descriptor.as_raw_fd(),
            libc::SOL_SOCKET,
            option,
            value.as_mut_ptr().cast(),
            &mut length,
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    if length as usize != mem::size_of::<T>() {
        return Err(denied("invalid Linux socket option size"));
    }
    // SAFETY: getsockopt initialized the complete T output above. All callers
    // use integer T types, for which every bit representation is valid.
    Ok(unsafe { value.assume_init() })
}

fn bound_device(descriptor: BorrowedFd<'_>) -> io::Result<String> {
    let mut bytes = [0u8; libc::IF_NAMESIZE];
    let mut length = bytes.len() as libc::socklen_t;
    // SAFETY: bytes is writable for length bytes and the FD remains borrowed.
    if unsafe {
        libc::getsockopt(
            descriptor.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_BINDTODEVICE,
            bytes.as_mut_ptr().cast(),
            &mut length,
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    if length as usize > bytes.len() {
        return Err(denied("invalid device binding length"));
    }
    let end = bytes[..length as usize]
        .iter()
        .position(|b| *b == 0)
        .unwrap_or(length as usize);
    std::str::from_utf8(&bytes[..end])
        .map(str::to_owned)
        .map_err(|_| denied("invalid device binding"))
}

fn set_device(descriptor: BorrowedFd<'_>, index: Option<u32>) -> io::Result<()> {
    let name = index
        .map(network::interface_name)
        .transpose()?
        .unwrap_or_default();
    let name = std::ffi::CString::new(name).map_err(|_| denied("invalid interface name"))?;
    // SAFETY: name is NUL terminated and the kernel copies its bytes during the call.
    if unsafe {
        libc::setsockopt(
            descriptor.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_BINDTODEVICE,
            name.as_ptr().cast(),
            name.as_bytes_with_nul().len() as _,
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn clear_socket(descriptor: BorrowedFd<'_>) -> io::Result<()> {
    let mark = set_mark(descriptor, 0);
    let device = set_device(descriptor, None);
    mark?;
    device?;
    if socket_option::<u32>(descriptor, libc::SO_MARK)? != 0
        || !bound_device(descriptor)?.is_empty()
    {
        return Err(denied("socket protection cleanup could not be verified"));
    }
    Ok(())
}

fn socket_route_query(
    descriptor: BorrowedFd<'_>,
    remote: SocketAddr,
    protocol: u32,
    mark: u32,
    uid: u32,
    interface: Option<u32>,
) -> io::Result<network::RouteQuery> {
    // SAFETY: sockaddr_storage is output storage for getsockname and large
    // enough for either family. Casts below check the returned family/length.
    let mut address: libc::sockaddr_storage = unsafe { mem::zeroed() };
    let mut length = mem::size_of_val(&address) as libc::socklen_t;
    // SAFETY: address and length point to valid writable output storage.
    if unsafe {
        libc::getsockname(
            descriptor.as_raw_fd(),
            ptr::from_mut(&mut address).cast(),
            &mut length,
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    let source = match address.ss_family as i32 {
        libc::AF_INET
            if length as usize >= mem::size_of::<libc::sockaddr_in>() && remote.is_ipv4() =>
        {
            // SAFETY: storage alignment, length and family were checked above.
            let address = unsafe { &*ptr::from_ref(&address).cast::<libc::sockaddr_in>() };
            SocketAddr::new(
                std::net::Ipv4Addr::from(address.sin_addr.s_addr.to_ne_bytes()).into(),
                u16::from_be(address.sin_port),
            )
        }
        libc::AF_INET6
            if length as usize >= mem::size_of::<libc::sockaddr_in6>() && remote.is_ipv6() =>
        {
            // SAFETY: storage alignment, length and family were checked above.
            let address = unsafe { &*ptr::from_ref(&address).cast::<libc::sockaddr_in6>() };
            SocketAddr::V6(std::net::SocketAddrV6::new(
                address.sin6_addr.s6_addr.into(),
                u16::from_be(address.sin6_port),
                0,
                address.sin6_scope_id,
            ))
        }
        _ => return Err(denied("egress socket source family mismatch")),
    };
    let mut tos = 0_i32;
    let mut size = mem::size_of_val(&tos) as libc::socklen_t;
    let (level, option) = if remote.is_ipv4() {
        (libc::IPPROTO_IP, libc::IP_TOS)
    } else {
        (libc::IPPROTO_IPV6, libc::IPV6_TCLASS)
    };
    // SAFETY: tos is a complete integer output and size describes it.
    if unsafe {
        libc::getsockopt(
            descriptor.as_raw_fd(),
            level,
            option,
            ptr::from_mut(&mut tos).cast(),
            &mut size,
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    if size as usize != mem::size_of_val(&tos) {
        return Err(denied("invalid traffic class option length"));
    }
    let source_scope = match source {
        SocketAddr::V6(a) if a.scope_id() != 0 => Some(a.scope_id()),
        _ => None,
    };
    let destination_scope = match remote {
        SocketAddr::V6(a) if a.scope_id() != 0 => Some(a.scope_id()),
        _ => None,
    };
    if source_scope
        .zip(destination_scope)
        .is_some_and(|(a, b)| a != b)
    {
        return Err(denied("socket source and destination scopes differ"));
    }
    let scope = destination_scope.or(source_scope);
    if interface.zip(scope).is_some_and(|(a, b)| a != b) {
        return Err(denied("DNS owner and scope differ"));
    }
    Ok(network::RouteQuery {
        remote,
        source: Some(source),
        mark,
        uid,
        protocol: u8::try_from(protocol).map_err(|_| denied("invalid egress protocol"))?,
        tos: u8::try_from(tos).map_err(|_| denied("invalid traffic class"))?,
        interface: interface.or(scope),
    })
}

fn set_mark(descriptor: BorrowedFd<'_>, mark: u32) -> io::Result<()> {
    // SAFETY: mark is a live u32 option and descriptor remains borrowed.
    if unsafe {
        libc::setsockopt(
            descriptor.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_MARK,
            ptr::from_ref(&mark).cast(),
            mem::size_of_val(&mark) as _,
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn exclusions(plan: &ValidatedTunnelPlan) -> Vec<IpNet> {
    let mut networks = plan.split_exclusions.clone();
    if plan.allow_lan {
        networks.extend(
            [
                "10.0.0.0/8",
                "172.16.0.0/12",
                "192.168.0.0/16",
                "169.254.0.0/16",
                "fc00::/7",
                "fe80::/10",
            ]
            .into_iter()
            .map(|network| network.parse::<IpNet>().expect("fixed LAN subnet")),
        );
    }
    networks.sort();
    networks.dedup();
    networks
}

/// System users of DHCP clients that drop root (systemd-networkd and
/// privilege-separated dhcpcd). Read from the root-owned passwd file without
/// NSS modules; a missing entry merely leaves only root's exception.
fn dhcp_client_uids() -> Vec<u32> {
    let mut passwd = String::new();
    let read = File::open("/etc/passwd")
        .and_then(|file| io::Read::read_to_string(&mut io::Read::take(file, 4 << 20), &mut passwd));
    if let Err(error) = read {
        tracing::warn!(kind = ?error.kind(), "Linux DHCP client users are unavailable");
        return Vec::new();
    }
    parse_dhcp_client_uids(&passwd)
}

fn parse_dhcp_client_uids(passwd: &str) -> Vec<u32> {
    let mut uids: Vec<u32> = passwd
        .lines()
        .filter_map(|line| {
            let mut fields = line.split(':');
            let name = fields.next()?;
            let uid = fields.nth(1)?.parse::<u32>().ok()?;
            (matches!(name, "systemd-network" | "dhcpcd" | "_dhcpcd") && uid != 0).then_some(uid)
        })
        .collect();
    uids.sort_unstable();
    uids.dedup();
    uids
}

fn active_resolver() -> io::Result<()> {
    let value = property(RESOLVE_PATH, RESOLVE_MANAGER, "ResolvConfMode")?;
    if !matches!(value.as_str(), Some("stub" | "static")) {
        return Err(linux_failure::classified(
            FailureReason::ResolverMode,
            "systemd-resolved is not the active stub resolver",
        ));
    }
    if property(RESOLVE_PATH, RESOLVE_MANAGER, "DNSStubListener")?.as_str() != Some("yes") {
        return Err(linux_failure::classified(
            FailureReason::StubListener,
            "systemd-resolved DNS stub is unavailable",
        ));
    }
    let metadata = fs::metadata("/etc/resolv.conf")?;
    if metadata.len() > 64 * 1024 {
        return Err(unavailable("resolver configuration exceeds limit"));
    }
    let content = fs::read_to_string("/etc/resolv.conf")?;
    validate_resolv_conf(&content)
}

pub fn physical_snapshot(backend: &super::dns::DnsReceipt) -> io::Result<Vec<PhysicalInterface>> {
    physical_snapshot_inner(backend).map_err(|error| {
        linux_failure::dns_snapshot(
            match backend {
                super::dns::DnsReceipt::Resolved => LinuxDnsBackend::Resolved,
                super::dns::DnsReceipt::NetworkManager { .. } => LinuxDnsBackend::NetworkManager,
            },
            usque_ipc::agent_v1::LinuxDnsSnapshotOperation::Unspecified,
            error,
        )
    })
}

fn physical_snapshot_inner(backend: &super::dns::DnsReceipt) -> io::Result<Vec<PhysicalInterface>> {
    let mut interfaces = network::physical_interfaces()?;
    match backend {
        super::dns::DnsReceipt::Resolved => {
            (|| -> io::Result<()> {
                let dns = property(RESOLVE_PATH, RESOLVE_MANAGER, "DNS").map_err(|error| {
                    linux_failure::dns_snapshot(
                        LinuxDnsBackend::Resolved,
                        usque_ipc::agent_v1::LinuxDnsSnapshotOperation::ResolvedDns,
                        error,
                    )
                })?;
                for entry in dns
                    .as_array()
                    .ok_or_else(|| unavailable("invalid resolved physical DNS snapshot"))?
                {
                    let fields = entry
                        .as_array()
                        .filter(|fields| fields.len() == 3)
                        .ok_or_else(|| unavailable("invalid resolved DNS record"))?;
                    let index = fields[0]
                        .as_u64()
                        .and_then(|value| u32::try_from(value).ok())
                        .ok_or_else(|| unavailable("invalid resolved DNS interface"))?;
                    let bytes = fields[2]
                        .as_array()
                        .ok_or_else(|| unavailable("invalid resolved DNS address"))?
                        .iter()
                        .map(|value| {
                            value
                                .as_u64()
                                .and_then(|value| u8::try_from(value).ok())
                                .ok_or_else(|| unavailable("invalid resolved DNS byte"))
                        })
                        .collect::<io::Result<Vec<_>>>()?;
                    let address = match (fields[1].as_i64(), bytes.as_slice()) {
                        (Some(2), [a, b, c, d]) => {
                            std::net::IpAddr::V4(std::net::Ipv4Addr::new(*a, *b, *c, *d))
                        }
                        (Some(10), bytes) if bytes.len() == 16 => {
                            std::net::IpAddr::V6(std::net::Ipv6Addr::from(
                                <[u8; 16]>::try_from(bytes).expect("IPv6 length"),
                            ))
                        }
                        _ => return Err(unavailable("invalid resolved DNS family")),
                    };
                    if address.is_loopback() || address.is_unspecified() || address.is_multicast() {
                        continue;
                    }
                    for link in interfaces
                        .iter_mut()
                        .filter(|link| index == 0 || link.index == index)
                    {
                        link.dns_servers.push(address);
                    }
                }
                Ok(())
            })()
            .map_err(|error| {
                linux_failure::dns_snapshot(
                    LinuxDnsBackend::Resolved,
                    usque_ipc::agent_v1::LinuxDnsSnapshotOperation::ResolvedDns,
                    error,
                )
            })?;
        }
        super::dns::DnsReceipt::NetworkManager { .. } => {
            for link in &mut interfaces {
                link.dns_servers = super::dns::physical_dns(link.index)?;
            }
        }
    }
    for link in &mut interfaces {
        link.dns_servers.sort();
        link.dns_servers.dedup();
        link.dns_servers.truncate(8);
    }
    Ok(interfaces)
}

fn validate_resolv_conf(content: &str) -> io::Result<()> {
    let servers: Vec<_> = content
        .lines()
        .filter_map(|line| {
            let mut words = line
                .split(['#', ';'])
                .next()
                .unwrap_or_default()
                .split_whitespace();
            (words.next() == Some("nameserver"))
                .then(|| words.next())
                .flatten()
        })
        .collect();
    if servers.is_empty() || servers.iter().any(|server| *server != "127.0.0.53") {
        return Err(linux_failure::classified(
            FailureReason::ResolverConfig,
            "system DNS does not exclusively use the resolved stub",
        ));
    }
    Ok(())
}

pub fn inspect_platform(
    journal: &super::journal::Journal,
) -> usque_ipc::agent_v1::LinuxPlatformState {
    let mut observation = usque_ipc::agent_v1::LinuxPlatformState::default();
    let Some((resources, plan)) = journal.resources.as_ref().zip(journal.plan.as_ref()) else {
        return observation;
    };
    if let Ok(present) = network::verify_interface(resources) {
        observation.tun_known = true;
        observation.tun_present = present.is_some();
    }
    let expected = |step| {
        journal
            .records
            .iter()
            .any(|record| record.step == step && record.state == super::journal::StepState::Applied)
    };
    if let Ok(consistent) = network::inspect_routes(
        resources,
        plan.assigned_ipv4.is_some(),
        plan.assigned_ipv6.is_some(),
        &exclusions(plan),
        &plan.dns_servers,
        expected(Step::Routes),
    ) {
        observation.routes_known = true;
        observation.routes_consistent = consistent;
    }
    if expected(Step::Dns) {
        let dns = match &resources.dns {
            super::dns::DnsReceipt::Resolved => (|| {
                active_resolver()?;
                let index = network::verify_interface(resources)?
                    .ok_or_else(|| unavailable("DNS interface missing"))?;
                let path = format!("/org/freedesktop/resolve1/link/_{index}");
                Ok(
                    property(&path, "org.freedesktop.resolve1.Link", "DNS")? == expected_dns(plan)
                        && property(&path, "org.freedesktop.resolve1.Link", "Domains")?
                            == json!([[".", true]]),
                )
            })(),
            super::dns::DnsReceipt::NetworkManager { .. } => {
                super::dns::inspect_network_manager(&plan.dns_servers)
            }
        };
        if let Ok(consistent) = dns {
            observation.dns_known = true;
            observation.dns_consistent = consistent;
        }
    }
    observation
}

fn configure_dns(resources: &Resources, plan: &ValidatedTunnelPlan) -> io::Result<()> {
    active_resolver()?;
    let index =
        network::verify_interface(resources)?.ok_or_else(|| unavailable("DNS TUN is absent"))?;
    let mut args = vec![index.to_string(), plan.dns_servers.len().to_string()];
    for server in &plan.dns_servers {
        let (family, bytes) = match server {
            std::net::IpAddr::V4(ip) => (libc::AF_INET, ip.octets().to_vec()),
            std::net::IpAddr::V6(ip) => (libc::AF_INET6, ip.octets().to_vec()),
        };
        args.push(family.to_string());
        args.push(bytes.len().to_string());
        args.extend(bytes.into_iter().map(|byte| byte.to_string()));
    }
    method("SetLinkDNS", "ia(iay)", args)?;
    method(
        "SetLinkDomains",
        "ia(sb)",
        vec![index.to_string(), "1".into(), ".".into(), "true".into()],
    )?;
    let path = format!("/org/freedesktop/resolve1/link/_{index}");
    if property(&path, "org.freedesktop.resolve1.Link", "DNS")? != expected_dns(plan)
        || property(&path, "org.freedesktop.resolve1.Link", "Domains")? != json!([[".", true]])
    {
        return Err(linux_failure::classified(
            FailureReason::Verification,
            "Linux DNS configuration could not be verified",
        ));
    }
    Ok(())
}

fn restore_dns(resources: &Resources, plan: &ValidatedTunnelPlan) -> io::Result<()> {
    let Some(index) = network::verify_interface(resources)? else {
        return Ok(());
    };
    let path = format!("/org/freedesktop/resolve1/link/_{index}");
    // Restore only fields still holding our value. All settings were empty on
    // this newly created TUN; never reset another interface or global DNS.
    if property(&path, "org.freedesktop.resolve1.Link", "DNS")? == expected_dns(plan) {
        method("SetLinkDNS", "ia(iay)", vec![index.to_string(), "0".into()])?;
    }
    if property(&path, "org.freedesktop.resolve1.Link", "Domains")? == json!([[".", true]]) {
        method(
            "SetLinkDomains",
            "ia(sb)",
            vec![index.to_string(), "0".into()],
        )?;
    }
    Ok(())
}

fn expected_dns(plan: &ValidatedTunnelPlan) -> Value {
    Value::Array(
        plan.dns_servers
            .iter()
            .map(|server| match server {
                std::net::IpAddr::V4(ip) => json!([libc::AF_INET, ip.octets()]),
                std::net::IpAddr::V6(ip) => json!([libc::AF_INET6, ip.octets()]),
            })
            .collect(),
    )
}

fn property(path: &str, interface: &str, name: &str) -> io::Result<Value> {
    let args = [
        "--system",
        "--json=short",
        "--timeout=5",
        "get-property",
        RESOLVE_SERVICE,
        path,
        interface,
        name,
    ]
    .map(str::to_owned);
    let response: Value = serde_json::from_slice(&command::run(BUSCTL, &args, None)?)
        .map_err(|_| unavailable("invalid resolver response"))?;
    response
        .get("data")
        .cloned()
        .ok_or_else(|| unavailable("resolver property is unavailable"))
}

fn method(name: &str, signature: &str, arguments: Vec<String>) -> io::Result<()> {
    let mut args = vec![
        "--system".into(),
        "--timeout=5".into(),
        "call".into(),
        RESOLVE_SERVICE.into(),
        RESOLVE_PATH.into(),
        RESOLVE_MANAGER.into(),
        name.into(),
        signature.into(),
    ];
    args.extend(arguments);
    command::run(BUSCTL, &args, None).map(|_| ())
}

fn denied(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, message)
}
fn unavailable(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::Unsupported, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dns_fallback_is_lazy_and_preserves_both_rejections() {
        assert_eq!(
            select_dns_backend(Ok(()), || panic!("resolved succeeded")).unwrap(),
            super::super::dns::DnsReceipt::Resolved
        );
        let receipt = super::super::dns::DnsReceipt::NetworkManager {
            previous: Default::default(),
        };
        assert_eq!(
            select_dns_backend(
                Err(io::Error::from_raw_os_error(13)),
                || Ok(receipt.clone())
            )
            .unwrap(),
            receipt
        );
        let error = select_dns_backend(
            Err(linux_failure::classified(
                FailureReason::StubListener,
                "secret 192.0.2.1 /private",
            )),
            || Err(io::Error::from_raw_os_error(2)),
        )
        .unwrap_err();
        let detail = linux_failure::detail(&error).unwrap();
        assert!(linux_failure::valid(&detail));
        assert_eq!(
            detail.dns_probe_failures[0].backend,
            LinuxDnsBackend::Resolved as i32
        );
        assert_eq!(
            detail.dns_probe_failures[0].reason,
            FailureReason::StubListener as i32
        );
        assert_eq!(detail.dns_probe_failures[1].os_errno, Some(2));
        assert!(!serde_json::to_string(&detail).unwrap().contains("secret"));
    }

    #[test]
    fn uncertain_socket_cleanup_retains_descriptor_binding_and_mark_slot() {
        let mut backend = NativeBackend::default();
        let mut resources = Resources::for_operation(uuid::Uuid::new_v4());
        resources.egress_mark_highwater = 1;
        let mark = resources.socket_mark | 1;
        let (socket, _) = std::os::unix::net::UnixStream::pair().unwrap();
        backend.sockets.insert(mark, socket.into());
        backend.binding_indices.insert(mark, 42);
        assert!(
            backend
                .clear_retained_socket_using(mark, |_| Err(io::Error::from_raw_os_error(1)))
                .is_err()
        );
        backend.recycle_mark(&resources, mark);
        assert!(backend.reusable_marks.is_empty());
        assert_eq!(backend.binding_indices[&mark], 42);
        assert!(backend.sockets.contains_key(&mark));
        backend
            .clear_retained_socket_using(mark, |_| Ok(()))
            .unwrap();
        backend.recycle_mark(&resources, mark);
        assert_eq!(backend.reusable_marks, [1]);
        assert!(!backend.binding_indices.contains_key(&mark));
    }

    #[test]
    fn socket_query_keeps_actual_bound_source_and_caller_context() {
        let socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let query = socket_route_query(
            socket.as_fd(),
            "192.0.2.1:443".parse().unwrap(),
            17,
            0x12340008,
            1001,
            None,
        )
        .unwrap();
        assert_eq!(query.source, Some(socket.local_addr().unwrap()));
        assert_eq!(query.uid, 1001);
        assert_eq!(query.mark, 0x12340008);
        assert_eq!(query.protocol, 17);
        assert_eq!(query.tos, 0);
    }

    fn multiple_physical_interfaces() -> Resources {
        let mut resources = Resources::for_operation(uuid::Uuid::new_v4());
        resources.physical_interfaces = vec![
            PhysicalInterface {
                index: 2,
                family_mask: 3,
                dns_servers: vec!["192.0.2.53".parse().unwrap(), "fe80::1".parse().unwrap()],
            },
            PhysicalInterface {
                index: 3,
                family_mask: 3,
                dns_servers: vec!["192.168.2.1".parse().unwrap(), "fe80::1".parse().unwrap()],
            },
        ];
        resources
    }

    #[test]
    fn scoped_dns_uses_its_physical_interface_and_rejects_missing_scope() {
        let mut resources = multiple_physical_interfaces();
        let remote = "[fe80::1%3]:53".parse().unwrap();
        assert_eq!(egress_interface(&resources, remote).unwrap().index, 3);
        assert!(egress_interface(&resources, "[fe80::1%4]:53".parse().unwrap()).is_err());
        resources.physical_interfaces[1].family_mask = 1;
        assert!(egress_interface(&resources, remote).is_err());
    }

    #[test]
    fn physical_dns_uses_its_owner_without_rebinding_other_destinations() {
        let resources = multiple_physical_interfaces();
        assert_eq!(
            egress_interface(&resources, "192.168.2.1:53".parse().unwrap())
                .unwrap()
                .index,
            3,
        );
        assert_eq!(
            egress_interface(&resources, "192.168.2.1:443".parse().unwrap())
                .unwrap()
                .index,
            2,
        );
    }

    #[test]
    fn preset_dns_authorization_requires_the_scoped_interface_to_own_the_resolver() {
        let mut resources = multiple_physical_interfaces();
        let plan = ValidatedTunnelPlan {
            profile_id: uuid::Uuid::new_v4(),
            endpoint: "162.159.192.1:443".parse().unwrap(),
            endpoint_candidates: vec!["162.159.192.1:443".parse().unwrap()],
            control_api_candidates: Vec::new(),
            mtu: 1280,
            dns_servers: vec!["1.1.1.1".parse().unwrap()],
            split_exclusions: Vec::new(),
            allow_lan: false,
            kill_switch: true,
            split_dns: false,
            assigned_ipv4: Some("172.16.0.2/32".parse().unwrap()),
            assigned_ipv6: None,
            vpn_chain: false,
            defer_network_configuration: false,
            automatic_endpoint_policy: None,
            preset_endpoint_policy: Some(crate::plan::PresetEndpointPolicy {
                hostname: usque_core::EndpointPresetRegion::ALL[0]
                    .hostname(usque_core::EndpointPresetTier::Free)
                    .into(),
                port: 443,
                ipv4_fallback: None,
                tcp: true,
                udp: true,
            }),
        };
        let backend = NativeBackend::default();
        let remote = "[fe80::1%3]:53".parse().unwrap();
        assert!(
            backend
                .validate_egress_purpose(
                    &plan,
                    &resources,
                    super::super::journal::Phase::Prepared,
                    remote,
                    17,
                    2,
                )
                .is_ok()
        );
        // The same address on a different link cannot authorize this scope.
        resources.physical_interfaces[1].dns_servers.clear();
        assert!(
            backend
                .validate_egress_purpose(
                    &plan,
                    &resources,
                    super::super::journal::Phase::Prepared,
                    remote,
                    17,
                    2,
                )
                .is_err()
        );
    }

    #[test]
    fn only_unattached_allocated_marks_return_to_the_free_list() {
        let mut resources = Resources::for_operation(uuid::Uuid::new_v4());
        resources.egress_mark_highwater = 3;
        let prefix = resources.socket_mark;
        let mut backend = NativeBackend::default();
        backend.leases.push(EgressLease {
            mark: prefix | 2,
            remote: "192.0.2.1:443".parse().unwrap(),
            protocol: 17,
            socket_cookie: 1,
            lease_id: 1,
        });
        for mark in [prefix, prefix | 2, prefix | 4, (prefix ^ 0x0001_0000) | 1] {
            backend.recycle_mark(&resources, mark);
        }
        assert!(backend.reusable_marks.is_empty());
        backend.recycle_mark(&resources, prefix | 3);
        backend.recycle_mark(&resources, prefix | 3);
        backend.recycle_mark(&resources, prefix | 1);
        assert_eq!(backend.reusable_marks, [3, 1]);
    }

    #[test]
    fn only_named_system_dhcp_clients_receive_the_renewal_exception() {
        let passwd = "root:x:0:0:root:/root:/bin/bash\n\
                      systemd-network:x:192:192:systemd Network Management:/:/usr/sbin/nologin\n\
                      dhcpcd:x:977:977::/var/lib/dhcpcd:/usr/bin/nologin\n\
                      _dhcpcd:x:977:977::/var/lib/dhcpcd:/usr/bin/nologin\n\
                      alice:x:1000:1000::/home/alice:/bin/bash\n\
                      dhcpcd-root:x:0:0::/:/bin/sh\n\
                      systemd-network:x:bad:0::/:/bin/sh\n";
        assert_eq!(parse_dhcp_client_uids(passwd), vec![192, 977]);
        assert!(parse_dhcp_client_uids("dhcpcd:x:0:0::/:/bin/sh\n").is_empty());
    }

    #[test]
    fn mixed_foreign_or_missing_stub_dns_is_rejected() {
        assert!(validate_resolv_conf("nameserver 127.0.0.53\noptions edns0\n").is_ok());
        assert!(validate_resolv_conf("nameserver 127.0.0.53\nnameserver 1.1.1.1\n").is_err());
        assert!(validate_resolv_conf("nameserver 127.0.0.1\n").is_err());
        assert!(validate_resolv_conf("# nameserver 127.0.0.53\n").is_err());
    }
}
