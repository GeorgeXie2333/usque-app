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
    DirectEgressLease, PhysicalNetworkAvailability, PhysicalNetworkSnapshot, PhysicalSocketPurpose,
    STALE_GENERATION_REASON, SocketHandle, SocketProtector,
};

pub struct MacSocketProtector {
    agent: MacAgentClient,
    fence: p::MacosFence,
    physical: watch::Sender<PhysicalNetworkSnapshot>,
    dns: Vec<SocketAddr>,
    control: Vec<SocketAddr>,
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
            fence,
            physical,
            dns,
            control,
        }))
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
        if generation != self.fence.network || self.physical.borrow().generation != generation {
            return Err(STALE_GENERATION_REASON.into());
        }
        let purpose = match purpose {
            PhysicalSocketPurpose::Masque => p::MacosSocketPurpose::Masque,
            PhysicalSocketPurpose::BootstrapDns => p::MacosSocketPurpose::BootstrapDns,
            PhysicalSocketPurpose::Direct | PhysicalSocketPurpose::DirectDns => {
                p::MacosSocketPurpose::Direct
            }
            PhysicalSocketPurpose::Control => p::MacosSocketPurpose::Control,
        };
        let agent = self.agent.clone();
        let fence = self.fence.clone();
        // Cancelling a candidate cannot cancel a request already sent to the Agent.
        // Keep receiving its reply and own the lease before returning it to the caller.
        let (info, fd, lease) = tokio::spawn(async move {
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
