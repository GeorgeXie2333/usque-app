//! Protocol-only session boundary shared by both chained protocols.
use std::net::SocketAddr;
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
impl Session {
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
        Ok(Self::OpenVpn(
            usque_openvpn::Session::start_with_credentials(
                profile.content(),
                remote,
                &credentials.username,
                &credentials.password,
                &credentials.private_key_password,
            )?,
        ))
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
    pub(crate) async fn send_ip(&self, generation: u64, packet: &[u8]) -> Result<(), Error> {
        match self {
            Self::OpenVpn(input) => input.send_ip(generation, packet).await,
            #[cfg(feature = "wireguard")]
            Self::WireGuard(input) => input.push(2, generation, packet).await,
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
