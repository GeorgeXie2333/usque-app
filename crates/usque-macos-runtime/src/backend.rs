//! Concrete adapter to the existing shared MASQUE/OpenVPN/WireGuard runtime.
//! Platform operations are acknowledged by the native providers; this crate
//! never substitutes a noop socket protector or packet-filter fallback.
use crate::{
    credentials::{CredentialBroker, SessionCredentials},
    protocol::*,
};
use async_trait::async_trait;
use bytes::Bytes;
use serde::Serialize;
use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::Arc,
};
use tokio::sync::{Mutex, mpsc};
use tokio_util::sync::CancellationToken;
use usque_core::WarpIdentity;
use usque_transport::{
    DataPlaneRuntime, DirectEgressLease, DirectProtocol, EndpointPinRefresher, MasqueTlsIdentity,
    RuntimeHealth, SocketHandle, SocketProtector, TransportError, TunPacketIo,
};

pub enum SessionHealth {
    Starting,
    Connected(ProviderPath),
    Failed,
}

#[async_trait]
pub trait SessionBackend: Send + 'static {
    async fn protect(&mut self, key: &SessionKey) -> Result<(), String>;
    async fn configure_dns_routes(
        &mut self,
        key: &SessionKey,
        configuration: &SessionConfiguration,
        credentials: &SessionCredentials,
    ) -> Result<(), String>;
    async fn connect(
        &mut self,
        key: &SessionKey,
        configuration: SessionConfiguration,
        credentials: SessionCredentials,
        cancellation: CancellationToken,
    ) -> Result<(), String>;
    fn health(&self) -> SessionHealth;
    async fn allow(&mut self, key: &SessionKey) -> Result<(), String>;
    async fn block(&mut self, key: &SessionKey) -> Result<(), String>;
    async fn disconnect(&mut self) -> Result<(), String>;
    async fn clear_dns_routes(&mut self, key: &SessionKey) -> Result<(), String>;
    async fn release_protection(&mut self, key: &SessionKey) -> Result<(), String>;
}

#[derive(Clone, Debug, Serialize)]
pub struct NetworkSettings {
    pub assigned_ipv4: Option<Ipv4Addr>,
    pub assigned_ipv6: Option<Ipv6Addr>,
    pub dns_servers: Vec<IpAddr>,
    pub mtu: u16,
    pub enforce_routes: bool,
}
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PlatformRequest {
    Protect {
        session: SessionKey,
    },
    ConfigureDnsRoutes {
        session: SessionKey,
        settings: NetworkSettings,
    },
    Allow {
        session: SessionKey,
    },
    Block {
        session: SessionKey,
    },
    ClearDnsRoutes {
        session: SessionKey,
    },
    Release {
        session: SessionKey,
    },
    ProtectSocket {
        session: SessionKey,
        socket: u64,
        remote: String,
        protocol: u8,
        generation: u64,
    },
}
/// Called on a blocking worker. Return success only after the native OS action
/// completes, including filter admission. Errors must be stable nonsecret codes.
pub trait PlatformAdapter: Send + Sync + 'static {
    fn request(&self, request: &PlatformRequest) -> Result<(), String>;
}
async fn platform(
    adapter: Arc<dyn PlatformAdapter>,
    request: PlatformRequest,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || adapter.request(&request))
        .await
        .map_err(|_| "platform_worker_failed".to_owned())?
}

pub const PACKET_QUEUE_CAPACITY: usize = 128;
pub const MAX_PACKET_BYTES: usize = 65535;
/// Packet Tunnel is only a remote VPN. Flow routing, DNS policy and content
/// filtering are owned by separate native providers, never its TUN stack.
pub fn packet_tunnel_profile(source: &usque_core::Profile) -> usque_core::Profile {
    let mut profile = source.clone();
    profile.frontends = usque_core::FrontendSettings {
        tunnel: true,
        socks5: false,
        http: false,
    };
    profile.proxy.system_proxy = false;
    profile.proxy.auth_username = None;
    profile.proxy.auth_password = None;
    profile.allow_lan = false;
    profile.geo_direct_countries.clear();
    profile.bypass_domains.clear();
    profile.split_exclusions.clear();
    profile.routing = Default::default();
    profile.direct_dns = Default::default();
    profile.disable_quic = false;
    profile.canonicalize_mode();
    profile
}
/// Engine frontends never receive this endpoint. Only the Packet Tunnel's
/// NEPacketTunnelFlow can send/receive these bounded packet queues.
pub struct PacketEndpoint {
    pub outgoing: mpsc::Sender<SessionPacket>,
    pub incoming: mpsc::Receiver<SessionPacket>,
}
pub struct SessionPacket {
    pub session: SessionKey,
    pub bytes: Bytes,
}
pub struct DataPlaneBackend {
    adapter: Arc<dyn PlatformAdapter>,
    broker: Arc<dyn CredentialBroker>,
    runtime: Option<DataPlaneRuntime>,
    packet_pump: Option<tokio::task::JoinHandle<()>>,
    packet_cancel: CancellationToken,
    outgoing: Arc<Mutex<mpsc::Receiver<SessionPacket>>>,
    incoming: mpsc::Sender<SessionPacket>,
    generation: Arc<std::sync::atomic::AtomicU64>,
    configured: Option<NetworkSettings>,
}
impl DataPlaneBackend {
    pub fn new(
        adapter: Arc<dyn PlatformAdapter>,
        broker: Arc<dyn CredentialBroker>,
    ) -> (Self, PacketEndpoint) {
        let (outgoing_tx, outgoing_rx) = mpsc::channel(PACKET_QUEUE_CAPACITY);
        let (incoming_tx, incoming_rx) = mpsc::channel(PACKET_QUEUE_CAPACITY);
        (
            Self {
                adapter,
                broker,
                runtime: None,
                packet_pump: None,
                packet_cancel: CancellationToken::new(),
                outgoing: Arc::new(Mutex::new(outgoing_rx)),
                incoming: incoming_tx,
                generation: Arc::new(std::sync::atomic::AtomicU64::new(0)),
                configured: None,
            },
            PacketEndpoint {
                outgoing: outgoing_tx,
                incoming: incoming_rx,
            },
        )
    }
}
#[async_trait]
impl SessionBackend for DataPlaneBackend {
    async fn protect(&mut self, key: &SessionKey) -> Result<(), String> {
        platform(
            self.adapter.clone(),
            PlatformRequest::Protect {
                session: key.clone(),
            },
        )
        .await?;
        self.generation
            .store(key.network_generation, std::sync::atomic::Ordering::Release);
        Ok(())
    }
    async fn configure_dns_routes(
        &mut self,
        key: &SessionKey,
        configuration: &SessionConfiguration,
        credentials: &SessionCredentials,
    ) -> Result<(), String> {
        configuration
            .profile
            .validate()
            .map_err(|_| "invalid_profile")?;
        crate::policy::PolicyEvaluator::load(configuration)?;
        let identity = credentials.identity()?;
        let settings = NetworkSettings {
            assigned_ipv4: Some(identity.assigned_ipv4),
            assigned_ipv6: Some(identity.assigned_ipv6),
            dns_servers: configuration.profile.dns_servers.clone(),
            mtu: configuration.profile.mtu,
            enforce_routes: true,
        };
        platform(
            self.adapter.clone(),
            PlatformRequest::ConfigureDnsRoutes {
                session: key.clone(),
                settings: settings.clone(),
            },
        )
        .await?;
        self.configured = Some(settings);
        Ok(())
    }
    async fn connect(
        &mut self,
        key: &SessionKey,
        configuration: SessionConfiguration,
        credentials: SessionCredentials,
        cancellation: CancellationToken,
    ) -> Result<(), String> {
        if self.runtime.is_some() {
            return Err("remote_session_already_owned".into());
        }
        // An authenticated physical endpoint resolver is not yet implemented.
        // The signed experiment requires numeric custom endpoint addresses;
        // never let automatic discovery recurse through the not-yet-up VPN.
        if configuration.profile.endpoint.selection != usque_core::EndpointSelection::Custom
            || configuration.profile.endpoint.preset_region.is_some()
        {
            return Err("physical_endpoint_dns_unverified".into());
        }
        let identity = credentials.identity()?;
        let tls =
            MasqueTlsIdentity::from_warp_identity(&identity).map_err(|_| "invalid_identity")?;
        let profile = packet_tunnel_profile(&configuration.profile);
        // All HTTP/SOCKS sockets remain in Engine. They enter this sole session
        // via normal system networking, never by creating another MASQUE peer.
        profile.validate().map_err(|_| "invalid_provider_profile")?;
        let selected = credentials
            .chain
            .as_ref()
            .map(|chain| chain.prepare(key, self.broker.as_ref()))
            .transpose()?;
        if profile.chain_enabled() != selected.is_some() {
            return Err("chain_credentials_mismatch".into());
        }
        let protector: Arc<dyn SocketProtector> = Arc::new(ProviderProtector {
            adapter: self.adapter.clone(),
            key: key.clone(),
            generation: self.generation.clone(),
        });
        let refresher: Arc<dyn EndpointPinRefresher> = Arc::new(BrokerPinRefresher {
            key: key.clone(),
            profile_id: profile.id,
            identity: Mutex::new(identity),
            broker: self.broker.clone(),
        });
        let mut runtime = DataPlaneRuntime::start_with_vpngate(
            &profile,
            tls,
            protector,
            Some(refresher),
            Arc::new(usque_transport::GeoDirectPolicy::disabled()),
            usque_transport::ChainExitStart {
                continuous: true,
                selected,
                cancellation: cancellation.clone(),
                ..Default::default()
            },
        )
        .await
        .map_err(|_| "data_plane_start_failed")?;
        if cancellation.is_cancelled() {
            runtime.shutdown().await;
            return Err("operation_cancelled".into());
        }
        let final_network = runtime.network_parameters();
        let settings = NetworkSettings {
            assigned_ipv4: final_network.ipv4,
            assigned_ipv6: final_network.ipv6,
            dns_servers: final_network.dns_servers,
            mtu: final_network.mtu,
            enforce_routes: true,
        };
        if let Err(code) = platform(
            self.adapter.clone(),
            PlatformRequest::ConfigureDnsRoutes {
                session: key.clone(),
                settings: settings.clone(),
            },
        )
        .await
        {
            runtime.shutdown().await;
            return Err(code);
        }
        let io = match runtime.attach_tun() {
            Ok(io) => io,
            Err(_) => {
                runtime.shutdown().await;
                return Err("tun_attach_failed".into());
            }
        };
        self.packet_cancel = CancellationToken::new();
        // Final chain admission happens after final OS routes exist and while
        // the independent filter still blocks ordinary physical egress.
        if runtime.activate_final().await.is_err() {
            runtime.shutdown().await;
            return Err("final_activation_failed".into());
        }
        self.packet_pump = Some(tokio::spawn(pump_packets(
            io,
            key.clone(),
            self.outgoing.clone(),
            self.incoming.clone(),
            self.packet_cancel.clone(),
        )));
        self.configured = Some(settings);
        self.runtime = Some(runtime);
        Ok(())
    }
    fn health(&self) -> SessionHealth {
        if self
            .packet_pump
            .as_ref()
            .is_some_and(|task| task.is_finished())
        {
            return SessionHealth::Failed;
        }
        match self.runtime.as_ref().map(DataPlaneRuntime::health) {
            Some(RuntimeHealth::Connected { path, .. }) => SessionHealth::Connected(path.into()),
            Some(RuntimeHealth::Connecting { .. } | RuntimeHealth::Reconnecting { .. }) => {
                SessionHealth::Starting
            }
            _ => SessionHealth::Failed,
        }
    }
    async fn allow(&mut self, key: &SessionKey) -> Result<(), String> {
        if let Some(runtime) = &mut self.runtime {
            runtime
                .activate_final()
                .await
                .map_err(|_| "final_activation_failed")?;
        }
        platform(
            self.adapter.clone(),
            PlatformRequest::Allow {
                session: key.clone(),
            },
        )
        .await
    }
    async fn block(&mut self, key: &SessionKey) -> Result<(), String> {
        platform(
            self.adapter.clone(),
            PlatformRequest::Block {
                session: key.clone(),
            },
        )
        .await
    }
    async fn disconnect(&mut self) -> Result<(), String> {
        self.packet_cancel.cancel();
        if let Some(task) = self.packet_pump.take() {
            let _ = task.await;
        }
        if let Some(mut runtime) = self.runtime.take() {
            runtime.shutdown().await;
        }
        // A new runtime must not consume packets queued for a previous session.
        let mut outgoing = self.outgoing.lock().await;
        while outgoing.try_recv().is_ok() {}
        Ok(())
    }
    async fn clear_dns_routes(&mut self, key: &SessionKey) -> Result<(), String> {
        platform(
            self.adapter.clone(),
            PlatformRequest::ClearDnsRoutes {
                session: key.clone(),
            },
        )
        .await?;
        self.configured = None;
        Ok(())
    }
    async fn release_protection(&mut self, key: &SessionKey) -> Result<(), String> {
        if self.runtime.is_some() {
            return Err("remote_session_still_active".into());
        }
        platform(
            self.adapter.clone(),
            PlatformRequest::Release {
                session: key.clone(),
            },
        )
        .await
    }
}

async fn pump_packets(
    mut io: TunPacketIo,
    key: SessionKey,
    outgoing: Arc<Mutex<mpsc::Receiver<SessionPacket>>>,
    incoming: mpsc::Sender<SessionPacket>,
    cancellation: CancellationToken,
) {
    let mut outgoing = outgoing.lock().await;
    loop {
        tokio::select! {
            biased;
            _ = cancellation.cancelled() => break,
            packet = outgoing.recv() => {
                let Some(packet) = packet else { break; };
                if packet.session != key { continue; }
                let send = io.start_send_owned_packet(packet.bytes);
                tokio::select! { _ = cancellation.cancelled() => break, result = send => if result.is_err() { break; } }
            }
            packet = io.receive_packet() => {
                let Ok(packet) = packet else { break; };
                tokio::select! { _ = cancellation.cancelled() => break, result = incoming.send(SessionPacket { session: key.clone(), bytes: packet }) => if result.is_err() { break; } }
            }
        }
    }
}

struct ProviderProtector {
    adapter: Arc<dyn PlatformAdapter>,
    key: SessionKey,
    generation: Arc<std::sync::atomic::AtomicU64>,
}
#[async_trait]
impl SocketProtector for ProviderProtector {
    fn protect(&self, _: SocketHandle) -> Result<(), String> {
        Err("exact_target_required".into())
    }
    async fn protect_for_target_generation(
        &self,
        socket: SocketHandle,
        remote: SocketAddr,
        protocol: DirectProtocol,
        generation: u64,
    ) -> Result<DirectEgressLease, String> {
        if self.network_generation() != Some(generation) {
            return Err("stale_generation".into());
        }
        platform(
            self.adapter.clone(),
            PlatformRequest::ProtectSocket {
                session: self.key.clone(),
                socket: socket.value(),
                remote: remote.to_string(),
                protocol: protocol.iana_number(),
                generation,
            },
        )
        .await?;
        if self.network_generation() != Some(generation) {
            return Err("stale_generation".into());
        }
        // The native adapter owns the finite target permission. It must revoke
        // the permission when the socket closes and bind the exact generation.
        Ok(DirectEgressLease::for_generation(generation))
    }
    fn network_generation(&self) -> Option<u64> {
        Some(self.generation.load(std::sync::atomic::Ordering::Acquire))
    }
    // Deliberately false until signed DIRECT routing tests prove it safe.
    fn tun_direct_available(&self) -> bool {
        false
    }
    fn resolve(&self, _: &str, _: u16) -> Result<Vec<SocketAddr>, String> {
        Err("physical_dns_broker_unavailable".into())
    }
}

pub struct BrokerPinRefresher {
    pub key: SessionKey,
    pub profile_id: uuid::Uuid,
    pub identity: Mutex<WarpIdentity>,
    pub broker: Arc<dyn CredentialBroker>,
}
impl BrokerPinRefresher {
    /// Applies a verified refresh only after a matching durable Engine ACK.
    pub async fn commit(
        &self,
        refresh: usque_core::EndpointPinRefresh,
    ) -> Result<MasqueTlsIdentity, TransportError> {
        let mut current = self.identity.lock().await;
        let mut next = duplicate_identity(&current)?;
        next.endpoint_pin = refresh.endpoint_pin;
        next.assigned_ipv4 = refresh.assigned_ipv4;
        next.assigned_ipv6 = refresh.assigned_ipv6;
        let ack = self
            .broker
            .persist_pin(&self.key, self.profile_id, &current, &next)
            .await
            .map_err(|_| {
                TransportError::EndpointPinRefresh("persistence_not_acknowledged".into())
            })?;
        if ack.session != self.key || ack.profile_id != self.profile_id || ack.revision.is_nil() {
            return Err(TransportError::EndpointPinRefresh(
                "persistence_ack_mismatch".into(),
            ));
        }
        let tls = MasqueTlsIdentity::from_warp_identity(&next)?;
        *current = next;
        Ok(tls)
    }
}
#[async_trait]
impl EndpointPinRefresher for BrokerPinRefresher {
    async fn refresh(
        &self,
        protector: Arc<dyn SocketProtector>,
    ) -> Result<MasqueTlsIdentity, TransportError> {
        let identity = {
            let current = self.identity.lock().await;
            duplicate_identity(&current)?
        };
        let refresh =
            usque_transport::refresh_endpoint_pin_over_protected_socket(&identity, None, protector)
                .await?;
        self.commit(refresh).await
    }
}

fn duplicate_identity(identity: &WarpIdentity) -> Result<WarpIdentity, TransportError> {
    let portable = identity
        .to_portable_secret_json()
        .map_err(|_| TransportError::InvalidIdentity)?;
    usque_core::parse_manual_warp_secret(&portable).map_err(|_| TransportError::InvalidIdentity)
}
