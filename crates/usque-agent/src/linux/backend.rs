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

    fn initialize_resources(&self, resources: &mut Resources) -> io::Result<()> {
        resources.dns = if active_resolver().is_ok() {
            super::dns::DnsReceipt::Resolved
        } else {
            super::dns::capture_network_manager()?
        };
        resources.physical_interfaces = physical_snapshot(&resources.dns)?;
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
                network::assert_route_ids_available(resources)?;
                firewall::install(resources, &exclusions(plan))?;
                self.leases.clear();
                self.reusable_marks.clear();
            }
            Step::Tun => {
                network::assert_route_ids_available(resources)?;
                self.tun = Some(network::create_tun(resources)?);
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
                self.clear_socket_marks()?;
                firewall::remove(resources)?;
                self.leases.clear();
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
        Ok(())
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
        if remote.port() == 0
            || remote.ip().is_unspecified()
            || remote.ip().is_multicast()
            || !matches!(protocol, 6 | 17)
        {
            return Err(denied("invalid Linux egress target"));
        }
        peer.ensure_alive()?;
        verify_socket_owner(descriptor, peer)?;
        let mask = if remote.is_ipv4() { 1 } else { 2 };
        let physical = resources
            .physical_interfaces
            .iter()
            .find(|link| link.family_mask & mask != 0)
            .ok_or_else(|| denied("no physical interface supports the egress family"))?;
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
                return Ok((physical.index, existing.lease_id));
            }
            return Err(denied("Linux egress socket already has a different permit"));
        }
        if current_mark != 0
            && (current_mark & 0xffff_0000 != resources.socket_mark
                || current_mark & 0xffff >= u32::from(resources.egress_mark_highwater))
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
        let interface = std::ffi::CString::new(network::interface_name(physical.index)?)
            .map_err(|_| denied("invalid physical interface"))?;
        // SAFETY: interface is a live NUL-terminated string; the kernel copies
        // it into this one authenticated socket, not a global route.
        if unsafe {
            libc::setsockopt(
                descriptor.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_BINDTODEVICE,
                interface.as_ptr().cast(),
                interface.as_bytes_with_nul().len() as _,
            )
        } != 0
        {
            return Err(io::Error::last_os_error());
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
        set_mark(descriptor, mark)?;
        if let Err(error) = firewall::update_egress(resources, &leases) {
            let _ = set_mark(descriptor, 0);
            return Err(error);
        }
        self.leases = leases;
        self.sockets.insert(mark, retained);
        Ok((physical.index, lease_id))
    }

    pub fn revoke_egress(&mut self, resources: &Resources) -> io::Result<()> {
        self.clear_socket_marks()?;
        firewall::update_egress(resources, &[])?;
        self.leases.clear();
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
        set_mark(socket.as_fd(), 0)?;
        let leases: Vec<_> = self
            .leases
            .iter()
            .filter(|lease| lease.lease_id != lease_id)
            .cloned()
            .collect();
        firewall::update_egress(resources, &leases)?;
        self.leases = leases;
        self.sockets.remove(&mark);
        // Reuse is allowed only after both the retained kernel socket mark and
        // its nft rule are gone. Restart drops this volatile free list; durable
        // highwater prevents reusing any unknown surviving socket's mark.
        self.reusable_marks.push((mark & 0xffff) as u16);
        Ok(())
    }

    fn clear_socket_marks(&mut self) -> io::Result<()> {
        for descriptor in self.sockets.values() {
            set_mark(descriptor.as_fd(), 0)?;
        }
        self.sockets.clear();
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
                    && resources
                        .physical_interfaces
                        .iter()
                        .any(|link| link.dns_servers.contains(&remote.ip()))
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

fn active_resolver() -> io::Result<()> {
    let value = property(RESOLVE_PATH, RESOLVE_MANAGER, "ResolvConfMode")?;
    if !matches!(value.as_str(), Some("stub" | "static")) {
        return Err(unavailable(
            "systemd-resolved is not the active stub resolver",
        ));
    }
    if property(RESOLVE_PATH, RESOLVE_MANAGER, "DNSStubListener")?.as_str() != Some("yes") {
        return Err(unavailable("systemd-resolved DNS stub is unavailable"));
    }
    let metadata = fs::metadata("/etc/resolv.conf")?;
    if metadata.len() > 64 * 1024 {
        return Err(unavailable("resolver configuration exceeds limit"));
    }
    let content = fs::read_to_string("/etc/resolv.conf")?;
    validate_resolv_conf(&content)
}

pub fn physical_snapshot(backend: &super::dns::DnsReceipt) -> io::Result<Vec<PhysicalInterface>> {
    let mut interfaces = network::physical_interfaces()?;
    match backend {
        super::dns::DnsReceipt::Resolved => {
            let dns = property(RESOLVE_PATH, RESOLVE_MANAGER, "DNS")?;
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
                    (Some(10), bytes) if bytes.len() == 16 => std::net::IpAddr::V6(
                        std::net::Ipv6Addr::from(<[u8; 16]>::try_from(bytes).expect("IPv6 length")),
                    ),
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
        return Err(unavailable(
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
        return Err(unavailable("Linux DNS configuration could not be verified"));
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
    fn mixed_foreign_or_missing_stub_dns_is_rejected() {
        assert!(validate_resolv_conf("nameserver 127.0.0.53\noptions edns0\n").is_ok());
        assert!(validate_resolv_conf("nameserver 127.0.0.53\nnameserver 1.1.1.1\n").is_err());
        assert!(validate_resolv_conf("nameserver 127.0.0.1\n").is_err());
        assert!(validate_resolv_conf("# nameserver 127.0.0.53\n").is_err());
    }
}
