//! Native resource intents are durable independently of coordinator phases.
//! Both journals must agree on the session; neither missing file means clean.
use super::{
    command::{self, NativeRunner, Program, Runner},
    dns, firewall,
    journal::SecureStore,
    network::{self, Network},
    resources::*,
    routes,
    socket::PhysicalSocket,
    transaction::{Backend, Fence, Step},
    utun::Utun,
};
use crate::plan::ValidatedTunnelPlan;
use std::{collections::BTreeMap, io, net::SocketAddr, os::fd::OwnedFd};
use usque_ipc::agent_v1::MacosSocketPurpose;

pub struct NativeBackend<R: Runner = NativeRunner> {
    store: SecureStore<NativeReceipts>,
    receipts: Option<NativeReceipts>,
    runner: R,
    network: Option<Network>,
    plan: Option<ValidatedTunnelPlan>,
    device: Option<Utun>,
    leases: BTreeMap<u64, PhysicalSocket>,
    next_lease: u64,
    active: bool,
    uncertain: bool,
    preset_candidates: Vec<SocketAddr>,
    snapshot: fn(&[DnsReceipt]) -> io::Result<Network>,
    boot: fn() -> io::Result<uuid::Uuid>,
}
impl NativeBackend {
    pub fn open() -> io::Result<Self> {
        let mut store = SecureStore::open_root(super::RESOURCE_PATH)?;
        let receipts = store.load()?;
        Ok(Self {
            store,
            receipts,
            runner: NativeRunner,
            network: None,
            plan: None,
            device: None,
            leases: BTreeMap::new(),
            next_lease: 0,
            active: false,
            uncertain: false,
            preset_candidates: vec![],
            snapshot: network::snapshot_with_original,
            boot: super::boot::identity,
        })
    }
}
impl<R: Runner> NativeBackend<R> {
    pub fn receipts(&self) -> Option<&NativeReceipts> {
        self.receipts.as_ref()
    }
    pub fn physical(&self) -> Option<&Network> {
        self.network.as_ref()
    }
    pub fn preflight(&mut self) -> io::Result<Network> {
        firewall::preflight(
            &mut self.runner,
            self.receipts
                .as_ref()
                .and_then(|r| r.pf.as_ref())
                .map(|p| p.anchor.as_str()),
        )?;
        (self.snapshot)(
            self.receipts
                .as_ref()
                .map(|r| r.dns.as_slice())
                .unwrap_or_default(),
        )
    }
    pub fn set_plan(&mut self, plan: ValidatedTunnelPlan) -> io::Result<()> {
        self.require_certain()?;
        plan.validate_network_fields().map_err(io::Error::other)?;
        // Rule-selected DIRECT flows use protected sockets. A global bypass
        // route cannot implement domain policy and is deliberately refused.
        if !plan.split_exclusions.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "macOS TUN requires data-plane split routing",
            ));
        }
        self.plan = Some(plan);
        Ok(())
    }
    pub fn set_active(&mut self) {
        self.active = true;
    }
    pub fn finalize_plan(&mut self, plan: ValidatedTunnelPlan) -> io::Result<()> {
        let mut allowed = self.plan.clone().ok_or_else(unavailable)?;
        if !allowed.defer_network_configuration || plan.defer_network_configuration {
            return Err(unavailable());
        }
        allowed.assigned_ipv4 = plan.assigned_ipv4;
        allowed.assigned_ipv6 = plan.assigned_ipv6;
        allowed.dns_servers.clone_from(&plan.dns_servers);
        allowed.mtu = plan.mtu;
        allowed.defer_network_configuration = false;
        if allowed != plan {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "bootstrap policy cannot change during finalization",
            ));
        }
        self.set_plan(plan)
    }
    pub fn descriptor(&self, fence: Fence) -> io::Result<(OwnedFd, String, u32)> {
        self.check(fence)?;
        let device = self
            .device
            .as_ref()
            .ok_or_else(|| io::Error::other("utun unavailable"))?;
        Ok((device.duplicate()?, device.name().into(), device.index()))
    }
    pub fn unchanged_network(&mut self, fence: Fence) -> io::Result<()> {
        self.check(fence)?;
        let observed = (self.snapshot)(&self.receipts.as_ref().ok_or_else(unavailable)?.dns);
        if observed.as_ref().ok() != self.network.as_ref() {
            self.quiesce_and_revoke_egress(fence)?;
            return Err(io::Error::new(
                io::ErrorKind::NetworkUnreachable,
                "physical network generation changed",
            ));
        }
        if let Err(error) = self.verify_protection() {
            // A privileged external PF change invalidated the guard. Stop the
            // shared file descriptions even when rule ownership prevents edit.
            for socket in self.leases.values() {
                let _ = socket.shutdown();
            }
            self.active = false;
            return Err(error);
        }
        Ok(())
    }
    pub fn verify_protection(&mut self) -> io::Result<()> {
        let receipts = self.receipts.as_ref().ok_or_else(unavailable)?;
        let pf = receipts.pf.as_ref().ok_or_else(unavailable)?;
        if receipts.boot != (self.boot)()?
            || pf.removal_intended
            || pf.restored
            || pf.enable_token.is_none()
            || pf.previous_rules.is_some()
        {
            return Err(unavailable());
        }
        firewall::verify_active(&mut self.runner, &pf.anchor, &pf.intended_rules)
    }

    pub fn create_socket(
        &mut self,
        fence: Fence,
        remote: SocketAddr,
        protocol: u8,
        purpose: MacosSocketPurpose,
    ) -> io::Result<(u64, OwnedFd, firewall::Permit)> {
        self.unchanged_network(fence)?;
        if self.leases.len() >= 128 {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "physical socket lease limit reached",
            ));
        }
        self.authorize_destination(remote, protocol, purpose)?;
        let socket = PhysicalSocket::create(
            self.network.as_ref().ok_or_else(unavailable)?,
            remote,
            protocol,
        )?;
        let permit = socket.permit.clone();
        let id = self.next_lease.checked_add(1).ok_or_else(unavailable)?;
        self.next_lease = id;
        self.leases.insert(id, socket);
        if let Err(error) = self.update_permits() {
            // Keep the FD pinned if persistence/application is uncertain. An
            // ephemeral port must not be reused while its permit may survive.
            self.uncertain = true;
            return Err(error);
        }
        if let Err(error) = self.leases[&id].connect() {
            // No descriptor was published. First revoke its tuple; only then
            // close the private FD, even when connect itself never completed.
            let permits = self
                .leases
                .iter()
                .filter(|(key, _)| **key != id)
                .map(|(_, s)| s.permit.clone())
                .collect::<Vec<_>>();
            self.replace_permits(&permits)?;
            self.leases.remove(&id);
            return Err(error);
        }
        match self.leases[&id].duplicate() {
            Ok(descriptor) => Ok((id, descriptor, permit)),
            Err(error) => {
                self.release_socket(fence, id)?;
                Err(error)
            }
        }
    }
    pub fn release_socket(&mut self, fence: Fence, id: u64) -> io::Result<()> {
        self.check(fence)?;
        if !self.leases.contains_key(&id) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "unknown physical socket lease",
            ));
        }
        let permits = self
            .leases
            .iter()
            .filter(|(key, _)| **key != id)
            .map(|(_, s)| s.permit.clone())
            .collect::<Vec<_>>();
        self.replace_permits(&permits)?;
        // The exact permit is durably absent. An already closed TCP peer or
        // an unpublished socket must not prevent completing this cleanup.
        let _ = self.leases[&id].shutdown();
        self.leases.remove(&id);
        Ok(())
    }
    pub fn endpoint_candidates(
        &mut self,
        hostname: &str,
        candidates: Vec<SocketAddr>,
    ) -> io::Result<()> {
        let policy = self
            .plan
            .as_ref()
            .and_then(|p| p.preset_endpoint_policy.as_ref())
            .ok_or_else(unavailable)?;
        if hostname != policy.hostname {
            return Err(unavailable());
        }
        policy
            .validate_candidates(&candidates)
            .map_err(io::Error::other)?;
        self.preset_candidates = candidates;
        Ok(())
    }
    fn authorize_destination(
        &self,
        remote: SocketAddr,
        protocol: u8,
        purpose: MacosSocketPurpose,
    ) -> io::Result<()> {
        let plan = self.plan.as_ref().ok_or_else(unavailable)?;
        let network = self.network.as_ref().ok_or_else(unavailable)?;
        if !matches!(protocol, 6 | 17) || remote.port() == 0 || !usable_address(remote.ip()) {
            return Err(unavailable());
        }
        let allowed = match purpose {
            MacosSocketPurpose::Masque => {
                plan.endpoint_candidates.contains(&remote)
                    || plan.endpoint == remote
                    || plan.automatic_endpoint_policy.is_some_and(|p| {
                        p.permits(
                            remote,
                            if protocol == 6 {
                                usque_core::Transport::Http2
                            } else {
                                usque_core::Transport::Http3
                            },
                        )
                    })
                    || plan.preset_endpoint_policy.as_ref().is_some_and(|p| {
                        p.permits_protocol(protocol)
                            && (p.ipv4_fallback == Some(remote)
                                || self.preset_candidates.contains(&remote))
                    })
            }
            MacosSocketPurpose::BootstrapDns => network.dns.contains(&remote),
            MacosSocketPurpose::Direct => self.active,
            MacosSocketPurpose::Control => {
                protocol == 6 && plan.control_api_candidates.contains(&remote)
            }
            _ => false,
        };
        if !allowed {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "physical destination is outside the session policy",
            ));
        }
        Ok(())
    }
    fn check(&self, fence: Fence) -> io::Result<()> {
        self.require_certain()?;
        if self.receipts.as_ref().is_none_or(|r| r.fence != fence) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "stale native resource fence",
            ));
        }
        Ok(())
    }
    fn require_certain(&self) -> io::Result<()> {
        if self.uncertain {
            Err(io::Error::other(
                "native journal uncertain; restart and recover",
            ))
        } else {
            Ok(())
        }
    }
    fn save(&mut self) -> io::Result<()> {
        self.require_certain()?;
        if let Err(error) = self.store.save_value(self.receipts.as_ref()) {
            self.uncertain = true;
            return Err(error);
        }
        Ok(())
    }
    fn restore_dns_with(
        &mut self,
        mut restore: impl FnMut(&DnsReceipt) -> io::Result<()>,
    ) -> io::Result<()> {
        let count = self.receipts.as_ref().ok_or_else(unavailable)?.dns.len();
        let mut first_error = None;
        for index in (0..count).rev() {
            let receipt = &self.receipts.as_ref().unwrap().dns[index];
            if receipt.restored {
                continue;
            }
            if let Err(error) = restore(receipt) {
                first_error.get_or_insert(error);
                continue;
            }
            self.receipts.as_mut().unwrap().dns[index].restored = true;
            self.save()?;
        }
        first_error.map_or(Ok(()), Err)
    }
    fn update_permits(&mut self) -> io::Result<()> {
        let permits = self
            .leases
            .values()
            .map(|s| s.permit.clone())
            .collect::<Vec<_>>();
        self.replace_permits(&permits)
    }
    fn replace_permits(&mut self, permits: &[firewall::Permit]) -> io::Result<()> {
        let receipts = self.receipts.as_ref().ok_or_else(unavailable)?;
        let pf = receipts.pf.as_ref().ok_or_else(unavailable)?;
        let current = firewall::read(&mut self.runner, &pf.anchor)?;
        if current != pf.intended_rules && pf.previous_rules.as_deref() != Some(current.as_str()) {
            return Err(io::Error::other("PF ownership conflict"));
        }
        let text=match self.network.as_ref() {
            Some(network)=>firewall::rules(network,receipts.interface.as_ref().filter(|i|!i.restored).map(|i|i.name.as_str()),permits)?,
            None if permits.is_empty()=>"pass quick on lo0 all no state\nblock drop quick inet all\nblock drop quick inet6 all\n".into(),
            None=>return Err(unavailable()),
        };
        let intended = firewall::canonical(&mut self.runner, &pf.anchor, &text)?;
        let anchor = pf.anchor.clone();
        let pf = self.receipts.as_mut().unwrap().pf.as_mut().unwrap();
        pf.previous_rules = Some(current);
        pf.intended_rules = intended.clone();
        self.save()?;
        firewall::replace(&mut self.runner, &anchor, &intended)?;
        self.receipts
            .as_mut()
            .unwrap()
            .pf
            .as_mut()
            .unwrap()
            .previous_rules = None;
        self.save()
    }
}
fn unavailable() -> io::Error {
    io::Error::other("macOS native session unavailable")
}

impl<R: Runner> Backend for NativeBackend<R> {
    fn stop_io(&mut self) -> io::Result<()> {
        self.active = false;
        let mut error = None;
        for socket in self.leases.values() {
            if let Err(e) = socket.shutdown() {
                error = Some(e);
            }
        }
        error.map_or(Ok(()), Err)
    }
    fn rebase(&mut self, previous: Fence, next: Fence) -> io::Result<()> {
        self.check(previous)?;
        self.verify_protection()?;
        let receipts = self.receipts.as_ref().unwrap();
        if receipts.boot != (self.boot)()?
            || receipts.routes.iter().any(|r| !r.restored)
            || receipts.dns.iter().any(|r| !r.restored)
            || receipts.interface.as_ref().is_some_and(|r| !r.restored)
            || receipts
                .pf
                .as_ref()
                .is_none_or(|p| p.removal_intended || p.restored)
            || !self.leases.is_empty()
        {
            return Err(unavailable());
        }
        let network = self.preflight()?;
        let receipts = self.receipts.as_mut().unwrap();
        receipts.fence = next;
        receipts.control_api_candidates = self
            .plan
            .as_ref()
            .ok_or_else(unavailable)?
            .control_api_candidates
            .clone();
        receipts.interface = None;
        receipts.dns.clear();
        receipts.routes.clear();
        self.save()?;
        self.network = Some(network);
        self.replace_permits(&[])
    }
    fn apply_and_verify(&mut self, fence: Fence, step: Step) -> io::Result<()> {
        self.require_certain()?;
        match step {
            Step::Protection => {
                if self.receipts.is_some() {
                    return Err(io::Error::other("native resources require recovery"));
                }
                let network = self.preflight()?;
                let anchor = firewall::anchor(fence.session);
                if !firewall::read(&mut self.runner, &anchor)?.is_empty() {
                    return Err(io::Error::other("PF anchor already exists"));
                }
                let text = firewall::rules(&network, None, &[])?;
                let intended = firewall::canonical(&mut self.runner, &anchor, &text)?;
                self.receipts = Some(NativeReceipts {
                    boot: (self.boot)()?,
                    control_api_candidates: self
                        .plan
                        .as_ref()
                        .map(|p| p.control_api_candidates.clone())
                        .unwrap_or_default(),
                    fence,
                    pf: Some(PfReceipt {
                        removal_intended: false,
                        anchor: anchor.clone(),
                        enable_intended: false,
                        enable_token: None,
                        reference: None,
                        previous_rules: Some(String::new()),
                        intended_rules: intended.clone(),
                        restored: false,
                    }),
                    interface: None,
                    dns: vec![],
                    routes: vec![],
                });
                self.network = Some(network);
                self.save()?;
                firewall::replace(&mut self.runner, &anchor, &intended)?;
                let pf = self.receipts.as_mut().unwrap().pf.as_mut().unwrap();
                pf.previous_rules = None;
                pf.enable_intended = true;
                self.save()?;
                let reference = super::pf_reference::acquire(&mut self.runner)?;
                let token = reference.token;
                self.receipts
                    .as_mut()
                    .unwrap()
                    .pf
                    .as_mut()
                    .unwrap()
                    .enable_token = Some(token);
                self.receipts
                    .as_mut()
                    .unwrap()
                    .pf
                    .as_mut()
                    .unwrap()
                    .reference = Some(reference);
                self.save()?;
                firewall::preflight(&mut self.runner, Some(&anchor))?;
            }
            Step::Egress => {
                self.unchanged_network(fence)?;
            }
            Step::Utun => {
                self.check(fence)?;
                let plan = self.plan.as_ref().ok_or_else(unavailable)?.clone();
                if plan.defer_network_configuration {
                    return Err(io::Error::other("final network configuration not supplied"));
                }
                let device = Utun::create()?;
                let name = device.name().to_owned();
                let index = device.index();
                let addresses = plan
                    .assigned_ipv4
                    .into_iter()
                    .chain(plan.assigned_ipv6)
                    .collect::<Vec<_>>();
                if addresses.is_empty() {
                    return Err(unavailable());
                }
                self.device = Some(device);
                self.receipts.as_mut().unwrap().interface = Some(InterfaceReceipt {
                    name: name.clone(),
                    index,
                    addresses: addresses.clone(),
                    restored: false,
                });
                self.save()?;
                command::checked(
                    &mut self.runner,
                    Program::Ifconfig,
                    &command::arguments(&[&name, "mtu", &plan.mtu.to_string()]),
                    None,
                )?;
                for address in &addresses {
                    let args = match address {
                        ipnet::IpNet::V4(ip) => command::arguments(&[
                            &name,
                            "inet",
                            &ip.addr().to_string(),
                            &ip.addr().to_string(),
                            "netmask",
                            &ip.netmask().to_string(),
                            "alias",
                        ]),
                        ipnet::IpNet::V6(ip) => {
                            command::arguments(&[&name, "inet6", &ip.to_string(), "alias"])
                        }
                    };
                    command::checked(&mut self.runner, Program::Ifconfig, &args, None)?;
                }
                command::checked(
                    &mut self.runner,
                    Program::Ifconfig,
                    &command::arguments(&[&name, "up"]),
                    None,
                )?;
                let observed = command::checked(
                    &mut self.runner,
                    Program::Ifconfig,
                    &command::arguments(&[&name]),
                    None,
                )?
                .stdout;
                if !observed
                    .lines()
                    .next()
                    .is_some_and(|s| s.contains(&format!("mtu {}", plan.mtu)))
                    || addresses.iter().any(|a| {
                        !observed
                            .split_whitespace()
                            .any(|s| s == a.addr().to_string())
                    })
                {
                    return Err(io::Error::other("utun configuration readback failed"));
                }
                self.update_permits()?;
            }
            Step::Dns => {
                self.check(fence)?;
                let plan = self.plan.as_ref().ok_or_else(unavailable)?;
                let receipts = dns::snapshot(&plan.dns_servers)?;
                self.receipts.as_mut().unwrap().dns = receipts;
                self.save()?;
                for receipt in &self.receipts.as_ref().unwrap().dns {
                    dns::apply(receipt, false)?;
                }
            }
            Step::Routes => {
                self.check(fence)?;
                let plan = self.plan.as_ref().ok_or_else(unavailable)?;
                let interface = self
                    .receipts
                    .as_ref()
                    .unwrap()
                    .interface
                    .as_ref()
                    .ok_or_else(unavailable)?;
                let receipts = destinations_for(
                    plan,
                    interface,
                    self.network.as_ref().ok_or_else(unavailable)?,
                )?;
                for receipt in &receipts {
                    routes::require_absent(&mut self.runner, receipt)?;
                }
                self.receipts.as_mut().unwrap().routes = receipts;
                self.save()?;
                for receipt in &self.receipts.as_ref().unwrap().routes {
                    routes::apply(&mut self.runner, receipt, false)?;
                }
            }
        }
        Ok(())
    }
    fn quiesce_and_revoke_egress(&mut self, fence: Fence) -> io::Result<()> {
        if self.receipts.as_ref().is_some_and(|r| r.fence != fence) {
            return Err(unavailable());
        }
        // Best-effort kernel shutdown precedes disk/PF work. Failure is not
        // proof of revocation: retain every original until PF removal has
        // succeeded and been read back, including never-published sockets.
        let _ = self.stop_io();
        self.require_certain()?;
        if self.receipts.is_none() {
            return Ok(());
        }
        self.check(fence)?;
        let receipts = self.receipts.as_ref().unwrap();
        if receipts.boot != (self.boot)()? {
            if receipts.pf.as_ref().is_some_and(|pf| !pf.restored) {
                let anchor = &receipts.pf.as_ref().unwrap().anchor;
                if !firewall::read(&mut self.runner, anchor)?.is_empty() {
                    return Err(io::Error::other("PF identity changed across boot"));
                }
            }
            return Ok(());
        }
        if receipts.pf.as_ref().is_some_and(|pf| pf.removal_intended) {
            if receipts.routes.iter().any(|r| !r.restored)
                || receipts.dns.iter().any(|r| !r.restored)
                || receipts.interface.as_ref().is_some_and(|r| !r.restored)
                || !self.leases.is_empty()
            {
                return Err(io::Error::other("invalid guard removal intent"));
            }
            return Ok(());
        }
        // An initial load rejected by pfctl leaves only its write-ahead
        // intent. No egress was published. Confirm the original empty anchor
        // before treating revocation as complete, including after a restart.
        if let Some(pf) = &receipts.pf
            && !pf.enable_intended
            && pf.previous_rules.as_deref() == Some("")
            && receipts.interface.is_none()
            && receipts.dns.is_empty()
            && receipts.routes.is_empty()
            && self.device.is_none()
            && self.leases.is_empty()
            && firewall::read(&mut self.runner, &pf.anchor)?.is_empty()
        {
            return Ok(());
        }
        self.replace_permits(&[])?;
        self.leases.clear();
        self.active = false;
        self.preset_candidates.clear();
        Ok(())
    }
    fn restore_and_verify(&mut self, fence: Fence, step: Step) -> io::Result<()> {
        self.require_certain()?;
        let Some(receipts) = self.receipts.as_ref() else {
            return Ok(());
        };
        if receipts.fence != fence {
            return Err(io::Error::other("native recovery fence conflict"));
        }
        let same_boot = receipts.boot == (self.boot)()?;
        match step {
            Step::Routes => {
                let mut first_error = None;
                for index in (0..receipts.routes.len()).rev() {
                    let receipt = &self.receipts.as_ref().unwrap().routes[index];
                    if receipt.restored {
                        continue;
                    }
                    if same_boot && let Err(error) = routes::apply(&mut self.runner, receipt, true)
                    {
                        // Each prefix has its own ownership proof. Retain this receipt
                        // and the guard while restoring the remaining prefixes.
                        first_error.get_or_insert(error);
                        continue;
                    }
                    self.receipts.as_mut().unwrap().routes[index].restored = true;
                    self.save()?;
                }
                if let Some(error) = first_error {
                    return Err(error);
                }
            }
            Step::Dns => {
                self.restore_dns_with(|r| dns::apply(r, true))?;
            }
            Step::Utun => {
                let interface = receipts.interface.clone();
                if let Some(interface) = interface {
                    if interface.restored {
                        return Ok(());
                    }
                    if same_boot {
                        let name = std::ffi::CString::new(interface.name.as_str())
                            .map_err(io::Error::other)?;
                        // SAFETY: NUL-terminated name points to valid storage.
                        let index = unsafe { libc::if_nametoindex(name.as_ptr()) };
                        if index != 0 && index != interface.index {
                            return Err(io::Error::other(
                                "utun identity changed; recovery incomplete",
                            ));
                        }
                        if index != 0 {
                            command::checked(
                                &mut self.runner,
                                Program::Ifconfig,
                                &command::arguments(&[&interface.name, "down"]),
                                None,
                            )?;
                        }
                        self.device.take();
                        // SAFETY: closing the last descriptor destroys utun. An
                        // Engine-held duplicate must be closed before Disconnect.
                        if unsafe { libc::if_nametoindex(name.as_ptr()) } != 0 {
                            return Err(io::Error::other(
                                "utun still held by Engine; recovery incomplete",
                            ));
                        }
                    }
                    self.receipts
                        .as_mut()
                        .unwrap()
                        .interface
                        .as_mut()
                        .unwrap()
                        .restored = true;
                    self.save()?;
                }
            }
            Step::Egress => {
                self.quiesce_and_revoke_egress(fence)?;
            }
            Step::Protection => {
                if receipts.routes.iter().any(|r| !r.restored)
                    || receipts.dns.iter().any(|d| !d.restored)
                    || receipts.interface.as_ref().is_some_and(|i| !i.restored)
                    || !self.leases.is_empty()
                {
                    return Err(io::Error::other(
                        "network cleanup incomplete; retaining guard",
                    ));
                }
                if let Some(pf) = receipts.pf.clone() {
                    let observed = firewall::read(&mut self.runner, &pf.anchor)?;
                    if !observed.is_empty()
                        && observed != pf.intended_rules
                        && pf.previous_rules.as_deref() != Some(&observed)
                    {
                        return Err(io::Error::other(
                            "PF ownership conflict; recovery incomplete",
                        ));
                    }
                    if same_boot && pf.enable_intended && pf.enable_token.is_none() {
                        return Err(io::Error::other(
                            "PF reference token uncertain; administrator recovery required",
                        ));
                    }
                    // Persist empty as the intended ruleset before removing it.
                    let saved = self.receipts.as_mut().unwrap().pf.as_mut().unwrap();
                    saved.previous_rules = Some(observed.clone());
                    saved.intended_rules = String::new();
                    saved.removal_intended = true;
                    self.save()?;
                    let current = firewall::read(&mut self.runner, &pf.anchor)?;
                    if !current.is_empty() {
                        if current != observed {
                            return Err(io::Error::other("PF ownership changed during recovery"));
                        }
                        firewall::replace(&mut self.runner, &pf.anchor, "")?;
                    }
                    if same_boot && let Some(reference) = &pf.reference {
                        super::pf_reference::release(&mut self.runner, reference)?;
                    }
                }
                self.receipts = None;
                self.save()?;
                self.network = None;
                self.plan = None;
            }
        }
        Ok(())
    }
}
fn destinations_for(
    plan: &ValidatedTunnelPlan,
    interface: &InterfaceReceipt,
    network: &Network,
) -> io::Result<Vec<RouteReceipt>> {
    let mut destinations = Vec::<ipnet::IpNet>::new();
    // XNU's fallback for IP_BOUND_IF looks up the all-zero destination
    // without a netmask. A utun 0/1 wins that lookup over the physical
    // default. Cover routable destinations with nonzero keys instead.
    if plan.assigned_ipv4.is_some() {
        destinations.extend(
            ipnet::Ipv4Subnets::new(
                std::net::Ipv4Addr::new(1, 0, 0, 0),
                std::net::Ipv4Addr::BROADCAST,
                0,
            )
            .map(ipnet::IpNet::V4),
        );
    }
    if plan.assigned_ipv6.is_some() {
        // Keep the reserved zero /16 with the system, including loopback and
        // IPv4-mapped addresses. Both NAT64 prefixes still go through utun.
        destinations.extend(
            ipnet::Ipv6Subnets::new(
                std::net::Ipv6Addr::new(1, 0, 0, 0, 0, 0, 0, 0),
                std::net::Ipv6Addr::from(u128::MAX),
                0,
            )
            .map(ipnet::IpNet::V6),
        );
    }
    // More-specific capture routes beat physical connected prefixes. LAN
    // permission remains a shared data-plane rule; no PF/global route bypass.
    let connected = network
        .paths
        .iter()
        .flat_map(|p| &p.addresses)
        .map(|prefix| prefix.trunc())
        .collect::<std::collections::BTreeSet<_>>();
    for path in &network.paths {
        if (path.address.is_ipv4() && plan.assigned_ipv4.is_none())
            || (path.address.is_ipv6() && plan.assigned_ipv6.is_none())
        {
            continue;
        }
        let max = if path.address.is_ipv4() { 32 } else { 128 };
        for address in &path.addresses {
            if address.prefix_len() < max {
                destinations.extend(
                    address
                        .trunc()
                        .subnets(address.prefix_len() + 1)
                        .map_err(io::Error::other)?,
                );
            }
        }
    }
    // A connected prefix is already present. Its more-specific capture
    // routes steer traffic without replacing the physical network's entry.
    destinations.retain(|prefix| !prefix.network().is_unspecified() && !connected.contains(prefix));
    for dns in &plan.dns_servers {
        destinations.push(
            ipnet::IpNet::new(*dns, if dns.is_ipv4() { 32 } else { 128 })
                .map_err(io::Error::other)?,
        );
    }
    destinations.sort();
    destinations.dedup();
    Ok(destinations
        .into_iter()
        .map(|destination| RouteReceipt {
            destination,
            interface: interface.name.clone(),
            interface_index: interface.index,
            restored: false,
        })
        .collect())
}

#[cfg(test)]
mod disposable;
#[cfg(test)]
mod tests;
