//! Exact, authenticated Linux socket egress. No system resolver is used after
//! the Agent has installed protection and taken ownership of interface DNS.
use crate::linux_agent::LinuxAgentClient;
use async_trait::async_trait;
use std::{
    net::SocketAddr,
    os::fd::{FromRawFd, OwnedFd},
    sync::{Arc, RwLock},
    time::Duration,
};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
use usque_ipc::agent_v1::{self, agent_request, agent_response};
use usque_transport::{
    DirectEgressLease, DirectProtocol, PhysicalNetworkAvailability, PhysicalNetworkSnapshot,
    STALE_GENERATION_REASON, SocketHandle, SocketProtector,
};
use uuid::Uuid;

pub(crate) struct LinuxSocketProtector {
    agent: LinuxAgentClient,
    operation: Uuid,
    physical: RwLock<agent_v1::PhysicalNetworkInfo>,
    updates: watch::Sender<PhysicalNetworkSnapshot>,
    registration: Vec<SocketAddr>,
    automatic: bool,
    preset: Option<String>,
    profile: String,
    retained_device: std::sync::Mutex<Option<OwnedFd>>,
}
impl LinuxSocketProtector {
    pub(crate) async fn new(
        agent: LinuxAgentClient,
        operation: Uuid,
        registration: Vec<SocketAddr>,
        plan: &agent_v1::TunnelPlan,
    ) -> std::io::Result<Arc<Self>> {
        let physical = agent.physical_network(operation).await?;
        let (updates, _) = watch::channel(observation(&physical));
        Ok(Arc::new(Self {
            agent,
            operation,
            physical: RwLock::new(physical),
            updates,
            registration,
            automatic: plan.automatic_endpoint_policy.is_some(),
            profile: plan.profile_id.clone(),
            retained_device: std::sync::Mutex::new(None),
            preset: plan
                .preset_endpoint_policy
                .as_ref()
                .map(|p| p.hostname.clone()),
        }))
    }
    pub(crate) fn set_device(&self, descriptor: &OwnedFd) -> std::io::Result<()> {
        *self
            .retained_device
            .lock()
            .map_err(|_| std::io::Error::other("retained device lock unavailable"))? =
            Some(descriptor.try_clone()?);
        Ok(())
    }
    pub(crate) fn clear_device(&self) {
        if let Ok(mut device) = self.retained_device.lock() {
            *device = None;
        }
    }
    async fn resume_after_disconnect(
        &self,
        cancel: &CancellationToken,
    ) -> Option<agent_v1::PhysicalNetworkInfo> {
        let retry = async {
            loop {
                if cancel.is_cancelled() {
                    return None;
                }
                let descriptor = self
                    .retained_device
                    .lock()
                    .ok()?
                    .as_ref()?
                    .try_clone()
                    .ok()?;
                if self.agent.reconnect().await.is_ok()
                    && self
                        .agent
                        .resume(self.operation, &self.profile, descriptor)
                        .await
                        .is_ok()
                    && let Ok(network) = self.agent.physical_network(self.operation).await
                {
                    return Some(network);
                }
                tokio::select! {()=cancel.cancelled()=>return None,_=tokio::time::sleep(Duration::from_millis(500))=>{}}
            }
        };
        tokio::time::timeout(Duration::from_secs(20), retry)
            .await
            .ok()
            .flatten()
    }
    pub(crate) fn monitor(
        self: &Arc<Self>,
        cancel: CancellationToken,
        failure: watch::Sender<Option<String>>,
    ) -> tokio_util::task::AbortOnDropHandle<()> {
        let protector = Arc::clone(self);
        tokio_util::task::AbortOnDropHandle::new(tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(2));
            loop {
                tokio::select! {
                    biased;
                    ()=cancel.cancelled()=>break,
                    _=interval.tick()=>{},
                }
                let observed = tokio::select! {
                    biased;
                    ()=cancel.cancelled()=>break,
                    value=protector.agent.physical_network(protector.operation)=>value,
                };
                let observed = match observed {
                    Ok(value) => Some(value),
                    Err(_) => protector.resume_after_disconnect(&cancel).await,
                };
                match observed {
                    Some(value) => {
                        let snapshot = observation(&value);
                        if let Ok(mut physical) = protector.physical.write() {
                            *physical = value;
                        }
                        protector.updates.send_if_modified(|previous| {
                            if *previous == snapshot {
                                false
                            } else {
                                *previous = snapshot;
                                true
                            }
                        });
                    }
                    None => {
                        failure.send_replace(Some(
                            "Linux Agent connection lost; traffic remains blocked until recovery"
                                .into(),
                        ));
                        cancel.cancel();
                        break;
                    }
                }
            }
        }))
    }
    async fn grant(
        &self,
        socket: SocketHandle,
        remote: SocketAddr,
        protocol: DirectProtocol,
        generation: u64,
        purpose: agent_v1::DirectEgressPurpose,
    ) -> Result<DirectEgressLease, String> {
        if self.network_generation() != Some(generation) {
            return Err(STALE_GENERATION_REASON.into());
        }
        let raw = i32::try_from(socket.value()).map_err(|_| "invalid Linux socket descriptor")?;
        // SAFETY: SocketProtector is called while the transport owns the socket.
        // Duplication pins that kernel socket until the Agent has authenticated it.
        let duplicate = unsafe { libc::fcntl(raw, libc::F_DUPFD_CLOEXEC, 3) };
        if duplicate < 0 {
            return Err("Linux socket duplication failed".into());
        }
        // SAFETY: fcntl returned a new, uniquely owned descriptor.
        let descriptor = unsafe { OwnedFd::from_raw_fd(duplicate) };
        let response = self
            .agent
            .request(
                agent_request::Payload::AcquireDirectEgress(agent_v1::AcquireDirectEgressRequest {
                    operation_id: self.operation.to_string(),
                    remote_endpoint: remote.to_string(),
                    protocol: u32::from(protocol.iana_number()),
                    expected_generation: generation,
                    purpose: purpose as i32,
                }),
                Some(descriptor),
            )
            .await
            .map_err(|error| authorization_error(error, "Linux exact-egress authorization failed"))?
            .0;
        match response {
            agent_response::Payload::DirectEgressLease(lease)
                if lease.network_generation == generation
                    && lease.remote_endpoint == remote.to_string()
                    && lease.protocol == u32::from(protocol.iana_number())
                    && lease.linux_lease_id != 0
                    && self.network_generation() == Some(generation) =>
            {
                Ok(DirectEgressLease::hold_for_generation(
                    LinuxEgressLease {
                        agent: self.agent.clone(),
                        operation: self.operation,
                        generation,
                        id: lease.linux_lease_id,
                    },
                    generation,
                ))
            }
            _ => Err(STALE_GENERATION_REASON.into()),
        }
    }
}
struct LinuxEgressLease {
    agent: LinuxAgentClient,
    operation: Uuid,
    generation: u64,
    id: u64,
}
impl Drop for LinuxEgressLease {
    fn drop(&mut self) {
        let agent = self.agent.clone();
        let request = agent_v1::ReleaseLinuxDirectEgressRequest {
            operation_id: self.operation.to_string(),
            network_generation: self.generation,
            lease_id: self.id,
        };
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                let _ = agent
                    .request(
                        agent_request::Payload::ReleaseLinuxDirectEgress(request),
                        None,
                    )
                    .await;
            });
        }
    }
}
fn observation(network: &agent_v1::PhysicalNetworkInfo) -> PhysicalNetworkSnapshot {
    let mask = network
        .interfaces
        .iter()
        .fold(0, |mask, i| mask | i.address_family_mask);
    PhysicalNetworkSnapshot {
        generation: network.generation,
        availability: if mask & 3 == 0 {
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
impl SocketProtector for LinuxSocketProtector {
    fn protect(&self, _socket: SocketHandle) -> Result<(), String> {
        Err("Linux TUN sockets require an exact destination authorization".into())
    }
    fn subscribe_physical_network(&self) -> Option<watch::Receiver<PhysicalNetworkSnapshot>> {
        Some(self.updates.subscribe())
    }
    fn tun_direct_available(&self) -> bool {
        true
    }
    fn network_generation(&self) -> Option<u64> {
        self.physical.read().ok().map(|p| p.generation)
    }
    fn endpoint_family_available(&self, endpoint: SocketAddr) -> Option<bool> {
        self.physical.read().ok().map(|p| {
            p.interfaces
                .iter()
                .any(|i| i.address_family_mask & if endpoint.is_ipv4() { 1 } else { 2 } != 0)
        })
    }
    fn physical_dns_servers(&self) -> Vec<SocketAddr> {
        let Ok(physical) = self.physical.read() else {
            return Vec::new();
        };
        let mut servers = physical
            .interfaces
            .iter()
            .flat_map(|interface| {
                interface.dns_servers.iter().filter_map(move |server| {
                    let ip = server.parse::<std::net::IpAddr>().ok()?;
                    if ip.is_unspecified() || ip.is_multicast() || ip.is_loopback() {
                        return None;
                    }
                    Some(match ip {
                        std::net::IpAddr::V6(ip) if ip.is_unicast_link_local() => SocketAddr::V6(
                            std::net::SocketAddrV6::new(ip, 53, 0, interface.interface_index),
                        ),
                        _ => SocketAddr::new(ip, 53),
                    })
                })
            })
            .collect::<Vec<_>>();
        servers.sort();
        servers.dedup();
        servers.truncate(16);
        servers
    }
    fn resolve(&self, host: &str, port: u16) -> Result<Vec<SocketAddr>, String> {
        if host == usque_core::REGISTRATION_API_HOST && port == usque_core::REGISTRATION_API_PORT {
            Ok(self.registration.clone())
        } else {
            Err("Linux protected resolver requires the pinned registration hostname".into())
        }
    }
    async fn resolve_direct(&self, host: &str, port: u16) -> Result<Vec<SocketAddr>, String> {
        usque_transport::resolve_physical_host(self, host, port).await
    }
    async fn protect_for_target(
        &self,
        socket: SocketHandle,
        remote: SocketAddr,
        protocol: DirectProtocol,
    ) -> Result<DirectEgressLease, String> {
        self.grant(
            socket,
            remote,
            protocol,
            self.network_generation().ok_or(STALE_GENERATION_REASON)?,
            agent_v1::DirectEgressPurpose::Unspecified,
        )
        .await
    }
    async fn protect_for_target_generation(
        &self,
        socket: SocketHandle,
        remote: SocketAddr,
        protocol: DirectProtocol,
        generation: u64,
    ) -> Result<DirectEgressLease, String> {
        self.grant(
            socket,
            remote,
            protocol,
            generation,
            agent_v1::DirectEgressPurpose::Unspecified,
        )
        .await
    }
    async fn protect_masque_endpoint_generation(
        &self,
        socket: SocketHandle,
        remote: SocketAddr,
        protocol: DirectProtocol,
        generation: u64,
    ) -> Result<DirectEgressLease, String> {
        self.grant(
            socket,
            remote,
            protocol,
            generation,
            if self.preset.is_some() {
                agent_v1::DirectEgressPurpose::PresetMasque
            } else if self.automatic {
                agent_v1::DirectEgressPurpose::AutomaticMasque
            } else {
                agent_v1::DirectEgressPurpose::Unspecified
            },
        )
        .await
    }
    async fn protect_endpoint_preset_dns_generation(
        &self,
        socket: SocketHandle,
        remote: SocketAddr,
        protocol: DirectProtocol,
        generation: u64,
    ) -> Result<DirectEgressLease, String> {
        if self.preset.is_none() || remote.port() != 53 {
            return Err("preset DNS policy unavailable".into());
        }
        self.grant(
            socket,
            remote,
            protocol,
            generation,
            agent_v1::DirectEgressPurpose::PresetDns,
        )
        .await
    }
    async fn authorize_endpoint_preset_candidates(
        &self,
        host: &str,
        candidates: &[SocketAddr],
        generation: u64,
    ) -> Result<(), String> {
        if self.preset.as_deref() != Some(host) {
            return Err("preset hostname does not match the active policy".into());
        }
        if self.network_generation() != Some(generation) {
            return Err(STALE_GENERATION_REASON.into());
        }
        let result = self
            .agent
            .request(
                agent_request::Payload::SetPresetEndpointCandidates(
                    agent_v1::SetPresetEndpointCandidatesRequest {
                        operation_id: self.operation.to_string(),
                        hostname: host.into(),
                        expected_generation: generation,
                        ipv6_candidates: candidates.iter().map(ToString::to_string).collect(),
                    },
                ),
                None,
            )
            .await
            .map_err(|error| authorization_error(error, "preset endpoint authorization failed"))?;
        if !matches!(result.0, agent_response::Payload::Empty(_)) {
            return Err("preset endpoint acknowledgement mismatch".into());
        }
        if self.network_generation() != Some(generation) {
            return Err(STALE_GENERATION_REASON.into());
        }
        Ok(())
    }
}
fn authorization_error(error: std::io::Error, denied: &'static str) -> String {
    if crate::linux_agent::is_stale_generation(&error) {
        STALE_GENERATION_REASON.into()
    } else {
        denied.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn agent_generation_refusals_retry_but_other_refusals_stay_terminal() {
        use usque_ipc::{
            agent_v1::{AgentRequest, AgentResponse},
            decode_frame, encode_frame, unix_fd,
        };

        for code in [
            "linux_stale_generation",
            "linux_permission_denied",
            "linux_operation_failed",
        ] {
            let (agent, server) = LinuxAgentClient::fake_pair();
            let worker = std::thread::spawn(move || {
                let peer = unix_fd::peer_credentials(&server).unwrap();
                let frame = unix_fd::receive_frame(&server, peer).unwrap();
                let request: AgentRequest = decode_frame(frame.bytes.into()).unwrap();
                assert!(matches!(
                    request.payload,
                    Some(agent_request::Payload::SetPresetEndpointCandidates(_))
                ));
                let response = AgentResponse {
                    request_id: request.request_id,
                    error: Some(agent_v1::AgentError {
                        code: code.into(),
                        // Identical text and retry hints cannot turn a permission
                        // refusal into evidence of a network-generation change.
                        message: "Linux egress generation changed".into(),
                        retryable: true,
                    }),
                    payload: None,
                };
                unix_fd::send_frame(&server, &encode_frame(&response).unwrap(), None::<&OwnedFd>)
                    .unwrap();
            });
            let physical = agent_v1::PhysicalNetworkInfo {
                generation: 7,
                ..Default::default()
            };
            let protector = LinuxSocketProtector {
                agent,
                operation: Uuid::new_v4(),
                updates: watch::channel(observation(&physical)).0,
                physical: RwLock::new(physical),
                registration: Vec::new(),
                automatic: false,
                preset: Some("us.v6.free.orangesky.dev".into()),
                profile: Uuid::new_v4().to_string(),
                retained_device: std::sync::Mutex::new(None),
            };
            let error = protector
                .authorize_endpoint_preset_candidates("us.v6.free.orangesky.dev", &[], 7)
                .await
                .unwrap_err();
            assert_eq!(
                error == STALE_GENERATION_REASON,
                code == "linux_stale_generation"
            );
            worker.join().unwrap();
        }
    }

    #[test]
    fn physical_observations_keep_generation_and_both_families() {
        let network = agent_v1::PhysicalNetworkInfo {
            generation: 42,
            interfaces: vec![
                agent_v1::PhysicalInterface {
                    address_family_mask: 1,
                    ..Default::default()
                },
                agent_v1::PhysicalInterface {
                    address_family_mask: 2,
                    ..Default::default()
                },
            ],
        };
        assert_eq!(
            observation(&network),
            PhysicalNetworkSnapshot {
                generation: 42,
                availability: PhysicalNetworkAvailability::Online {
                    ipv4: true,
                    ipv6: true
                }
            }
        );
        assert_eq!(
            observation(&Default::default()).availability,
            PhysicalNetworkAvailability::Offline
        );
    }
}
