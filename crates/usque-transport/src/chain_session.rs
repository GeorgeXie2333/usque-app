//! Protocol-only session boundary shared by both chained protocols.
use bytes::Bytes;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::Mutex;
use usque_core::chain_exit::ValidatedProfile;
use usque_core::vpngate::PreparedProfile;
use usque_openvpn::{Error, Event};

pub(crate) enum Session {
    OpenVpn(usque_openvpn::Session),
    #[cfg(feature = "wireguard")]
    WireGuard(super::wireguard::Session),
}
#[derive(Clone)]
pub(crate) enum Input {
    OpenVpn(usque_openvpn::Input),
    #[cfg(feature = "wireguard")]
    WireGuard(super::wireguard::Input),
}
#[derive(Clone)]
pub(crate) enum EventStream {
    OpenVpn(Arc<Mutex<usque_openvpn::Output>>),
    #[cfg(feature = "wireguard")]
    WireGuard(Arc<Mutex<tokio::sync::mpsc::Receiver<Event>>>),
}
impl EventStream {
    pub(crate) async fn next(&self) -> Result<Event, Error> {
        match self {
            Self::OpenVpn(output) => output.lock().await.next_event().await,
            #[cfg(feature = "wireguard")]
            Self::WireGuard(output) => output.lock().await.recv().await.ok_or(Error::Closed),
        }
    }
    pub(crate) async fn try_next(&self) -> Result<Option<Event>, Error> {
        match self {
            Self::OpenVpn(output) => output.lock().await.try_next_event(),
            #[cfg(feature = "wireguard")]
            Self::WireGuard(output) => match output.lock().await.try_recv() {
                Ok(event) => Ok(Some(event)),
                Err(tokio::sync::mpsc::error::TryRecvError::Empty) => Ok(None),
                Err(_) => Err(Error::Closed),
            },
        }
    }
    pub(crate) async fn transport(&self, generation: u64) -> Result<Bytes, Error> {
        loop {
            match self.next().await? {
                Event::TransportPacket {
                    generation: current,
                    packet,
                } if current == generation => return Ok(packet),
                Event::Stopped => return Err(Error::Closed),
                _ => {}
            }
        }
    }
}
impl Session {
    pub(crate) fn split_packet_outputs(&mut self) -> Option<(EventStream, EventStream)> {
        match self {
            Self::OpenVpn(session) => session.split_packet_outputs().map(|(a, b)| {
                (
                    EventStream::OpenVpn(Arc::new(Mutex::new(a))),
                    EventStream::OpenVpn(Arc::new(Mutex::new(b))),
                )
            }),
            #[cfg(feature = "wireguard")]
            Self::WireGuard(session) => session.split_packet_outputs().map(|(a, b)| {
                (
                    EventStream::WireGuard(Arc::new(Mutex::new(a))),
                    EventStream::WireGuard(Arc::new(Mutex::new(b))),
                )
            }),
        }
    }
    pub(crate) fn start(profile: &PreparedProfile, remote: SocketAddr) -> Result<Self, Error> {
        if let Some(ValidatedProfile::WireGuard(config)) = profile.custom.as_deref() {
            #[cfg(feature = "wireguard")]
            {
                return Ok(Self::WireGuard(super::wireguard::Session::start(
                    config.clone(),
                )));
            }
            #[cfg(not(feature = "wireguard"))]
            {
                let _ = config;
                return Err(Error::InvalidConfig);
            }
        }
        let credentials = &profile.credentials;
        Ok(Self::OpenVpn(usque_openvpn::Session::start_with_options(
            profile.content(),
            remote,
            &credentials.username,
            &credentials.password,
            &credentials.private_key_password,
            matches!(profile.custom.as_deref(), Some(ValidatedProfile::OpenVpn(p)) if p.client_certificate == usque_core::chain_exit::ClientCertificateMode::Disabled),
        )?))
    }
    pub(crate) fn input(&self) -> Input {
        match self {
            Self::OpenVpn(session) => Input::OpenVpn(session.input()),
            #[cfg(feature = "wireguard")]
            Self::WireGuard(session) => Input::WireGuard(session.input()),
        }
    }
    pub(crate) async fn next_event(&mut self) -> Result<Event, Error> {
        match self {
            Self::OpenVpn(session) => session.next_event().await,
            #[cfg(feature = "wireguard")]
            Self::WireGuard(session) => session.next_event().await,
        }
    }
    pub(crate) async fn shutdown(&mut self) -> Result<(), Error> {
        match self {
            Self::OpenVpn(session) => session.shutdown().await,
            #[cfg(feature = "wireguard")]
            Self::WireGuard(session) => session.shutdown().await,
        }
    }
}
impl Input {
    pub(crate) async fn receive_transport_owned(
        &self,
        generation: u64,
        packet: Bytes,
    ) -> Result<(), Error> {
        match self {
            Self::OpenVpn(input) => input.receive_transport(generation, &packet).await,
            #[cfg(feature = "wireguard")]
            Self::WireGuard(input) => input.push_owned(1, generation, packet).await,
        }
    }
    pub(crate) async fn send_ip_owned(&self, generation: u64, packet: Bytes) -> Result<(), Error> {
        match self {
            Self::OpenVpn(input) => input.send_ip(generation, &packet).await,
            #[cfg(feature = "wireguard")]
            Self::WireGuard(input) => input.push_owned(2, generation, packet).await,
        }
    }
    pub(crate) async fn transport_connected(&self, generation: u64) -> Result<(), Error> {
        match self {
            Self::OpenVpn(input) => input.transport_connected(generation).await,
            #[cfg(feature = "wireguard")]
            Self::WireGuard(input) => input.push(3, generation, &[]).await,
        }
    }
    pub(crate) async fn transport_failed(&self, generation: u64) -> Result<(), Error> {
        match self {
            Self::OpenVpn(input) => input.transport_failed(generation).await,
            #[cfg(feature = "wireguard")]
            Self::WireGuard(input) => input.push(4, generation, &[]).await,
        }
    }
    pub(crate) async fn receive_transport(
        &self,
        generation: u64,
        packet: &[u8],
    ) -> Result<(), Error> {
        match self {
            Self::OpenVpn(input) => input.receive_transport(generation, packet).await,
            #[cfg(feature = "wireguard")]
            Self::WireGuard(input) => input.push(1, generation, packet).await,
        }
    }
    pub(crate) fn stop(&self) {
        match self {
            Self::OpenVpn(input) => input.stop(),
            #[cfg(feature = "wireguard")]
            Self::WireGuard(input) => input.stop(),
        }
    }
}
