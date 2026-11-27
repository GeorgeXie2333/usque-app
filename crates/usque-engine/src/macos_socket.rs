//! Physical socket ownership and network epochs for the unprivileged Engine.
use crate::macos_agent::{AgentFailure, MacAgentClient, verify_socket};
use async_trait::async_trait;
use std::{
    io,
    net::{SocketAddr, UdpSocket},
    sync::Arc,
};
use tokio::sync::watch;
use usque_ipc::agent_v1::{self as p, macos_agent_request::Operation};
use usque_transport::{
    DirectEgressLease, EndpointProbeLeaseHandle, PhysicalNetworkAvailability,
    PhysicalNetworkSnapshot, PhysicalSocketPurpose, ProbeLeaseTarget, ProbeTransportProtocol,
    STALE_GENERATION_REASON, SocketHandle, SocketProtector,
};

pub struct MacSocketProtector {
    agent: MacAgentClient,
    dial_queue: Arc<tokio::sync::Mutex<()>>,
    fence: p::MacosFence,
    physical: watch::Sender<PhysicalNetworkSnapshot>,
    dns: Vec<SocketAddr>,
    control: Vec<SocketAddr>,
    hostname_policy_host: Option<String>,
}
struct Lease {
    agent: MacAgentClient,
    fence: p::MacosFence,
    id: u64,
    runtime: tokio::runtime::Handle,
}
impl Drop for Lease {
    fn drop(&mut self) {
        let (agent, fence, id) = (self.agent.clone(), self.fence.clone(), self.id);
        self.runtime.spawn(async move {
            let _ = agent
                .empty(Operation::ReleaseSocket(p::MacosReleaseSocket {
                    fence: Some(fence),
                    lease_id: id,
                }))
                .await;
        });
    }
}
impl MacSocketProtector {
    pub fn invalidate(&self) {
        self.physical.send_replace(PhysicalNetworkSnapshot {
            generation: self.fence.network.saturating_add(1),
            availability: PhysicalNetworkAvailability::Offline,
        });
    }
    pub fn new(
        agent: MacAgentClient,
        fence: p::MacosFence,
        network: &p::PhysicalNetworkInfo,
        control: Vec<SocketAddr>,
        hostname_policy_host: Option<String>,
    ) -> io::Result<Arc<Self>> {
        if network.generation != fence.network || network.interfaces.is_empty() {
            return Err(io::Error::other("physical network epoch unavailable"));
        }
        let mut dns = Vec::new();
        for interface in &network.interfaces {
            for value in &interface.dns_servers {
                let address = value.parse::<SocketAddr>().map_err(io::Error::other)?;
                if !dns.contains(&address) {
                    dns.push(address);
                }
            }
        }
        if dns.is_empty() || dns.len() > 8 {
            return Err(io::Error::other("physical DNS snapshot unavailable"));
        }
        let physical = watch::channel(snapshot(network)).0;
        Ok(Arc::new(Self {
            agent,
            dial_queue: Arc::default(),
            fence,
            physical,
            dns,
            control,
            hostname_policy_host: hostname_policy_host
                .as_deref()
                .and_then(usque_core::endpoint_subscriptions::canonical_public_domain),
        }))
    }
    pub fn probe_protector(
        self: &Arc<Self>,
        dns_hostname: Option<String>,
    ) -> Arc<dyn SocketProtector> {
        Arc::new(MacProbeSocketProtector {
            inner: Arc::clone(self),
            dns_hostname: dns_hostname
                .as_deref()
                .and_then(usque_core::endpoint_subscriptions::canonical_public_domain),
        })
    }
    pub fn observe(&self, status: &p::MacosPlatformStatus) -> bool {
        let current = status.fence.as_ref() == Some(&self.fence)
            && !status.recovery_pending
            && status.failure == 0;
        let value =
            status
                .physical_network
                .as_ref()
                .map(snapshot)
                .unwrap_or(PhysicalNetworkSnapshot {
                    generation: self.fence.network.saturating_add(1),
                    availability: PhysicalNetworkAvailability::Offline,
                });
        let unchanged = current && value.generation == self.fence.network;
        self.physical.send_replace(value);
        unchanged
    }
    async fn open(
        &self,
        remote: SocketAddr,
        protocol: u8,
        purpose: PhysicalSocketPurpose,
        generation: u64,
    ) -> Result<(std::os::fd::OwnedFd, DirectEgressLease, SocketAddr), String> {
        let purpose = match purpose {
            PhysicalSocketPurpose::Masque => p::MacosSocketPurpose::Masque,
            PhysicalSocketPurpose::BootstrapDns => p::MacosSocketPurpose::BootstrapDns,
            PhysicalSocketPurpose::Direct | PhysicalSocketPurpose::DirectDns => {
                p::MacosSocketPurpose::Direct
            }
            PhysicalSocketPurpose::Control => p::MacosSocketPurpose::Control,
        };
        self.open_with_purpose(remote, protocol, purpose, generation)
            .await
    }
    async fn open_with_purpose(
        &self,
        remote: SocketAddr,
        protocol: u8,
        purpose: p::MacosSocketPurpose,
        generation: u64,
    ) -> Result<(std::os::fd::OwnedFd, DirectEgressLease, SocketAddr), String> {
        // The Agent serializes permit updates. Keep waiting candidates in the
        // caller's future so a lost race cannot enqueue more privileged work.
        let admission = self.dial_queue.clone().lock_owned().await;
        if generation != self.fence.network || self.physical.borrow().generation != generation {
            return Err(STALE_GENERATION_REASON.into());
        }
        let agent = self.agent.clone();
        let fence = self.fence.clone();
        // Cancelling a candidate cannot cancel a request already sent to the Agent.
        // Keep receiving its reply and own the lease before returning it to the caller.
        let (info, fd, lease) = tokio::spawn(async move {
            let _admission = admission;
            let (info, fd) = agent
                .descriptor(Operation::CreateSocket(p::MacosSocketRequest {
                    fence: Some(fence.clone()),
                    remote_endpoint: remote.to_string(),
                    protocol: protocol.into(),
                    purpose: purpose as i32,
                }))
                .await
                .map_err(socket_setup_error)?;
            let lease = Lease {
                agent,
                fence,
                id: info.lease_id,
                runtime: tokio::runtime::Handle::current(),
            };
            Ok::<_, String>((info, fd, lease))
        })
        .await
        .map_err(|error| error.to_string())??;
        let lease = DirectEgressLease::hold_for_generation(lease, generation);
        verify_socket(&fd, &info, remote, protocol).map_err(|e| e.to_string())?;
        let local = info
            .local_endpoint
            .parse()
            .map_err(|_| "invalid local socket address")?;
        if self.physical.borrow().generation != generation {
            return Err(STALE_GENERATION_REASON.into());
        }
        Ok((fd, lease, local))
    }
}
fn socket_setup_error(error: io::Error) -> String {
    match error
        .get_ref()
        .and_then(|e| e.downcast_ref::<AgentFailure>())
    {
        Some(AgentFailure(p::MacosFailure::EndpointUnreachable)) => {
            usque_transport::ENDPOINT_UNREACHABLE_REASON.into()
        }
        _ => format!("macOS socket setup failed: {error}"),
    }
}

fn snapshot(network: &p::PhysicalNetworkInfo) -> PhysicalNetworkSnapshot {
    let mask = network
        .interfaces
        .iter()
        .fold(0, |mask, interface| mask | interface.address_family_mask);
    PhysicalNetworkSnapshot {
        generation: network.generation,
        availability: if mask == 0 {
            PhysicalNetworkAvailability::Offline
        } else {
            PhysicalNetworkAvailability::Online {
                ipv4: mask & 1 != 0,
                ipv6: mask & 2 != 0,
            }
        },
    }
}
#[async_trait]
impl SocketProtector for MacSocketProtector {
    fn protect(&self, _: SocketHandle) -> Result<(), String> {
        Err("macOS requires Agent-created physical sockets".into())
    }
    fn requires_owned_sockets(&self) -> bool {
        true
    }
    async fn prepare_tcp(
        &self,
        remote: SocketAddr,
        purpose: PhysicalSocketPurpose,
        generation: u64,
    ) -> Result<(usque_transport::PreparedTcpSocket, DirectEgressLease), String> {
        let (fd, lease, local) = self.open(remote, 6, purpose, generation).await?;
        let stream = std::net::TcpStream::from(fd);
        if stream.local_addr().map_err(|e| e.to_string())? != local {
            return Err("physical socket address mismatch".into());
        }
        Ok((
            usque_transport::PreparedTcpSocket::Connecting {
                stream: tokio::net::TcpStream::from_std(stream).map_err(|e| e.to_string())?,
                remote,
            },
            lease,
        ))
    }
    async fn prepare_udp(
        &self,
        remote: SocketAddr,
        purpose: PhysicalSocketPurpose,
        generation: u64,
    ) -> Result<(UdpSocket, DirectEgressLease), String> {
        let (fd, lease, local) = self.open(remote, 17, purpose, generation).await?;
        let socket = UdpSocket::from(fd);
        if socket.local_addr().map_err(|e| e.to_string())? != local {
            return Err("physical socket address mismatch".into());
        }
        Ok((socket, lease))
    }
    fn network_generation(&self) -> Option<u64> {
        Some(self.physical.borrow().generation)
    }
    fn subscribe_physical_network(&self) -> Option<watch::Receiver<PhysicalNetworkSnapshot>> {
        Some(self.physical.subscribe())
    }
    fn physical_dns_servers(&self) -> Vec<SocketAddr> {
        self.dns.clone()
    }
    fn tun_direct_available(&self) -> bool {
        true
    }
    fn endpoint_family_available(&self, remote: SocketAddr) -> Option<bool> {
        Some(match self.physical.borrow().availability {
            PhysicalNetworkAvailability::Online { ipv4, ipv6 } => {
                if remote.is_ipv4() {
                    ipv4
                } else {
                    ipv6
                }
            }
            _ => false,
        })
    }
    fn resolve(&self, host: &str, port: u16) -> Result<Vec<SocketAddr>, String> {
        if host == usque_core::REGISTRATION_API_HOST && port == 443 {
            return Ok(self.control.clone());
        }
        Err("synchronous system DNS is unavailable while macOS TUN owns DNS".into())
    }
    async fn resolve_direct(&self, host: &str, port: u16) -> Result<Vec<SocketAddr>, String> {
        usque_transport::resolve_physical_host(self, host, port).await
    }
    async fn authorize_endpoint_preset_candidates(
        &self,
        host: &str,
        candidates: &[SocketAddr],
        generation: u64,
    ) -> Result<(), String> {
        if generation != self.fence.network {
            return Err(STALE_GENERATION_REASON.into());
        }
        self.agent
            .empty(Operation::SetEndpointCandidates(
                p::MacosEndpointCandidates {
                    fence: Some(self.fence.clone()),
                    hostname: host.into(),
                    ipv6_candidates: candidates.iter().map(ToString::to_string).collect(),
                },
            ))
            .await
            .map_err(|e| e.to_string())
    }
    async fn resolve_endpoint_hostname(
        &self,
        host: &str,
        port: u16,
        ip_policy: usque_core::IpPolicy,
        generation: u64,
    ) -> Result<Vec<SocketAddr>, String> {
        let Some(expected) = self.hostname_policy_host.as_deref() else {
            return Err("hostname endpoint policy is unavailable".into());
        };
        let Some(canonical) = usque_core::endpoint_subscriptions::canonical_public_domain(host)
        else {
            return Err("invalid endpoint subscription hostname".into());
        };
        if canonical != expected {
            return Err("endpoint hostname does not match the active policy".into());
        }
        usque_transport::resolve_physical_endpoint_hostname(self, host, port, ip_policy, generation)
            .await
    }
    async fn authorize_endpoint_hostname_candidates(
        &self,
        host: &str,
        candidates: &[SocketAddr],
        generation: u64,
    ) -> Result<(), String> {
        let Some(expected) = self.hostname_policy_host.as_deref() else {
            return Err("hostname endpoint policy is unavailable".into());
        };
        let Some(canonical) = usque_core::endpoint_subscriptions::canonical_public_domain(host)
        else {
            return Err("invalid endpoint subscription hostname".into());
        };
        if canonical != expected {
            return Err("endpoint hostname does not match the active policy".into());
        }
        if generation != self.fence.network {
            return Err(STALE_GENERATION_REASON.into());
        }
        self.agent
            .empty(Operation::SetHostnameEndpointCandidates(
                p::MacosHostnameEndpointCandidates {
                    fence: Some(self.fence.clone()),
                    hostname: canonical,
                    candidates: candidates.iter().map(ToString::to_string).collect(),
                },
            ))
            .await
            .map_err(|e| e.to_string())?;
        if self.physical.borrow().generation != generation {
            return Err(STALE_GENERATION_REASON.into());
        }
        Ok(())
    }
    async fn acquire_endpoint_probe_lease(
        &self,
        probe_session_id: &str,
        targets: &[ProbeLeaseTarget],
        dns_hostname: Option<&str>,
        generation: u64,
    ) -> Result<EndpointProbeLeaseHandle, String> {
        if generation != self.fence.network || self.physical.borrow().generation != generation {
            return Err(STALE_GENERATION_REASON.into());
        }
        let targets = targets
            .iter()
            .map(|target| p::ProbeTarget {
                remote_endpoint: target.addr.to_string(),
                protocol: match target.protocol {
                    ProbeTransportProtocol::Tcp => 6,
                    ProbeTransportProtocol::Udp => 17,
                },
            })
            .collect();
        let lease = self
            .agent
            .probe_lease(Operation::AcquireProbeLease(p::MacosAcquireProbeLease {
                fence: Some(self.fence.clone()),
                probe_session_id: probe_session_id.into(),
                targets,
                dns_hostname: dns_hostname.unwrap_or_default().into(),
            }))
            .await
            .map_err(|e| e.to_string())?;
        if lease.lease_id == 0
            || lease.network_generation != generation
            || self.physical.borrow().generation != generation
        {
            return Err(STALE_GENERATION_REASON.into());
        }
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        Ok(EndpointProbeLeaseHandle {
            lease_id: lease.lease_id.to_string(),
            expires_at_unix_ms: now_ms.saturating_add(u64::from(lease.ttl_seconds) * 1000),
        })
    }
    async fn release_endpoint_probe_lease(
        &self,
        probe_session_id: &str,
        lease_id: &str,
    ) -> Result<(), String> {
        let Ok(parsed_id) = lease_id.parse::<u64>() else {
            return Err("invalid macOS probe lease identifier".into());
        };
        self.agent
            .empty(Operation::ReleaseProbeLease(p::MacosReleaseProbeLease {
                fence: Some(self.fence.clone()),
                probe_session_id: probe_session_id.into(),
                lease_id: parsed_id,
            }))
            .await
            .map_err(|e| e.to_string())
    }
}

struct MacProbeSocketProtector {
    inner: Arc<MacSocketProtector>,
    dns_hostname: Option<String>,
}

#[async_trait]
impl SocketProtector for MacProbeSocketProtector {
    fn protect(&self, socket: SocketHandle) -> Result<(), String> {
        self.inner.protect(socket)
    }
    fn requires_owned_sockets(&self) -> bool {
        self.inner.requires_owned_sockets()
    }
    async fn prepare_tcp(
        &self,
        remote: SocketAddr,
        purpose: PhysicalSocketPurpose,
        generation: u64,
    ) -> Result<(usque_transport::PreparedTcpSocket, DirectEgressLease), String> {
        if matches!(purpose, PhysicalSocketPurpose::BootstrapDns) && self.dns_hostname.is_none() {
            return Err("probe DNS lease is unavailable".into());
        }
        let (fd, lease, local) = self
            .inner
            .open_with_purpose(remote, 6, p::MacosSocketPurpose::EndpointProbe, generation)
            .await?;
        let stream = std::net::TcpStream::from(fd);
        if stream.local_addr().map_err(|e| e.to_string())? != local {
            return Err("physical socket address mismatch".into());
        }
        Ok((
            usque_transport::PreparedTcpSocket::Connecting {
                stream: tokio::net::TcpStream::from_std(stream).map_err(|e| e.to_string())?,
                remote,
            },
            lease,
        ))
    }
    async fn prepare_udp(
        &self,
        remote: SocketAddr,
        purpose: PhysicalSocketPurpose,
        generation: u64,
    ) -> Result<(UdpSocket, DirectEgressLease), String> {
        if matches!(purpose, PhysicalSocketPurpose::BootstrapDns) && self.dns_hostname.is_none() {
            return Err("probe DNS lease is unavailable".into());
        }
        let (fd, lease, local) = self
            .inner
            .open_with_purpose(remote, 17, p::MacosSocketPurpose::EndpointProbe, generation)
            .await?;
        let socket = UdpSocket::from(fd);
        if socket.local_addr().map_err(|e| e.to_string())? != local {
            return Err("physical socket address mismatch".into());
        }
        Ok((socket, lease))
    }
    fn network_generation(&self) -> Option<u64> {
        self.inner.network_generation()
    }
    fn subscribe_physical_network(&self) -> Option<watch::Receiver<PhysicalNetworkSnapshot>> {
        self.inner.subscribe_physical_network()
    }
    fn physical_dns_servers(&self) -> Vec<SocketAddr> {
        self.inner.physical_dns_servers()
    }
    fn tun_direct_available(&self) -> bool {
        self.inner.tun_direct_available()
    }
    fn endpoint_family_available(&self, remote: SocketAddr) -> Option<bool> {
        self.inner.endpoint_family_available(remote)
    }
    fn resolve(&self, host: &str, port: u16) -> Result<Vec<SocketAddr>, String> {
        self.inner.resolve(host, port)
    }
    async fn resolve_endpoint_hostname(
        &self,
        host: &str,
        port: u16,
        ip_policy: usque_core::IpPolicy,
        generation: u64,
    ) -> Result<Vec<SocketAddr>, String> {
        let Some(expected) = self.dns_hostname.as_deref() else {
            return Err("probe DNS lease is unavailable".into());
        };
        let Some(canonical) = usque_core::endpoint_subscriptions::canonical_public_domain(host)
        else {
            return Err("invalid endpoint subscription hostname".into());
        };
        if canonical != expected {
            return Err("probe hostname does not match active lease".into());
        }
        usque_transport::resolve_physical_endpoint_hostname(self, host, port, ip_policy, generation)
            .await
    }
    async fn acquire_endpoint_probe_lease(
        &self,
        probe_session_id: &str,
        targets: &[ProbeLeaseTarget],
        dns_hostname: Option<&str>,
        generation: u64,
    ) -> Result<EndpointProbeLeaseHandle, String> {
        self.inner
            .acquire_endpoint_probe_lease(probe_session_id, targets, dns_hostname, generation)
            .await
    }
    async fn release_endpoint_probe_lease(
        &self,
        probe_session_id: &str,
        lease_id: &str,
    ) -> Result<(), String> {
        self.inner
            .release_endpoint_probe_lease(probe_session_id, lease_id)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_typed_endpoint_failure_allows_another_candidate() {
        let retry = usque_transport::ENDPOINT_UNREACHABLE_REASON;
        assert_eq!(
            socket_setup_error(io::Error::other(AgentFailure(
                p::MacosFailure::EndpointUnreachable
            ))),
            retry
        );
        for error in [
            io::Error::other(AgentFailure(p::MacosFailure::RecoveryIncomplete)),
            io::Error::other(AgentFailure(p::MacosFailure::OwnershipConflict)),
            io::Error::new(io::ErrorKind::PermissionDenied, retry),
        ] {
            assert_ne!(socket_setup_error(error), retry);
        }
    }
}
