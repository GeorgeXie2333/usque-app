//! Connection-local built-in endpoint discovery. No resolved address is saved.
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use usque_core::{
    EndpointPresetTier, IdentityProvider, IpPolicy, Profile, Transport, TransportPolicy,
};

use crate::h2::{MasqueTlsIdentity, TransportError};
use crate::socket::SocketProtector;

pub(crate) const RESOLUTION_TIMEOUT: Duration = Duration::from_secs(4);
pub(crate) const MAX_CANDIDATES: usize = 16;
pub(crate) const BATCH_SIZE: usize = 8;

pub(crate) fn tier(identity: &MasqueTlsIdentity) -> Result<EndpointPresetTier, TransportError> {
    match (&identity.provider, identity.entitlement) {
        (Some(IdentityProvider::ZeroTrust { .. }), _) => Ok(EndpointPresetTier::ZeroTrust),
        (Some(IdentityProvider::Consumer), Some(usque_core::ConsumerEntitlement::Free)) => {
            Ok(EndpointPresetTier::Free)
        }
        (Some(IdentityProvider::Consumer), Some(usque_core::ConsumerEntitlement::WarpPlus)) => {
            Ok(EndpointPresetTier::Plus)
        }
        _ => Err(TransportError::InvalidIdentity),
    }
}

pub(crate) fn usable_ipv6(address: &SocketAddr) -> bool {
    let std::net::IpAddr::V6(ip) = address.ip() else {
        return false;
    };
    !ip.is_unspecified()
        && !ip.is_multicast()
        && !ip.is_loopback()
        && ip.segments()[0] & 0xffc0 != 0xfe80
        && ip.segments()[0] & 0xfe00 != 0xfc00
        && ip.to_ipv4_mapped().is_none()
}

pub(crate) fn canonical_candidates(mut addresses: Vec<SocketAddr>, port: u16) -> Vec<SocketAddr> {
    addresses.retain(|address| address.port() == port && usable_ipv6(address));
    addresses.sort();
    addresses.dedup();
    addresses.truncate(MAX_CANDIDATES);
    addresses
}

pub(crate) struct ResolvedPreset {
    pub(crate) candidates: Vec<SocketAddr>,
    pub(crate) generation: u64,
}

fn protection_error(reason: String) -> TransportError {
    // The Agent can observe a physical change before the local network watch
    // catches up. Its exact-generation refusal is sufficient evidence to retry.
    let token = reason
        .strip_prefix("preset_dns_protection:")
        .unwrap_or(&reason);
    if token == crate::socket::STALE_GENERATION_REASON {
        TransportError::UnderlyingNetworkChanged
    } else {
        TransportError::SocketProtection(reason)
    }
}

/// Revocation runs before resolution as well as before fallback. A previous
/// connection's records can never become candidates for this attempt.
pub(crate) async fn resolve(
    profile: &Profile,
    identity: &MasqueTlsIdentity,
    protector: &Arc<dyn SocketProtector>,
) -> Result<ResolvedPreset, TransportError> {
    let region = profile
        .endpoint
        .preset_region
        .ok_or(TransportError::InvalidIdentity)?;
    if profile.endpoint.selection != usque_core::EndpointSelection::Custom
        || !matches!(profile.ip_policy, IpPolicy::Ipv6Only | IpPolicy::PreferIpv6)
    {
        return Err(TransportError::UnsupportedOperatingMode);
    }
    let host = region.hostname(tier(identity)?);
    let generation = protector.network_generation().unwrap_or_default();
    protector
        .authorize_endpoint_preset_candidates(host, &[], generation)
        .await
        .map_err(protection_error)?;
    let family_hint = SocketAddr::new(
        std::net::Ipv6Addr::UNSPECIFIED.into(),
        profile.endpoint.port,
    );
    if protector.endpoint_family_available(family_hint) == Some(false) {
        return Err(TransportError::EndpointFamilyUnavailable(
            usque_core::AddressFamily::Ipv6,
        ));
    }
    let result = tokio::time::timeout(
        RESOLUTION_TIMEOUT,
        protector.resolve_endpoint_preset(host, profile.endpoint.port, generation),
    )
    .await;
    if protector.network_generation().unwrap_or_default() != generation {
        return Err(TransportError::UnderlyingNetworkChanged);
    }
    let candidates = canonical_candidates(
        result
            .map_err(|_| TransportError::Dns("endpoint preset DNS timed out".into()))?
            .map_err(|reason| {
                if reason == crate::socket::STALE_GENERATION_REASON
                    || reason.starts_with("preset_dns_protection:")
                {
                    protection_error(reason)
                } else {
                    TransportError::Dns(reason)
                }
            })?,
        profile.endpoint.port,
    );
    protector
        .authorize_endpoint_preset_candidates(host, &candidates, generation)
        .await
        .map_err(protection_error)?;
    if candidates.is_empty() {
        return Err(TransportError::Dns(
            "endpoint preset returned no usable AAAA records".into(),
        ));
    }
    Ok(ResolvedPreset {
        candidates,
        generation,
    })
}

pub(crate) fn protocols(policy: TransportPolicy) -> &'static [Transport] {
    match policy {
        TransportPolicy::Auto => &[Transport::Http3, Transport::Http2],
        TransportPolicy::Http3 => &[Transport::Http3],
        TransportPolicy::Http2 => &[Transport::Http2],
    }
}

pub(crate) fn may_fallback(profile: &Profile, error: &TransportError) -> bool {
    profile.ip_policy == IpPolicy::PreferIpv6
        && !matches!(error, TransportError::UnderlyingNetworkChanged)
        && crate::recovery_policy::RecoveryDecision::for_failure(&error.failure(None, None))
            == crate::recovery_policy::RecoveryDecision::Retry
}

/// Both data planes use the same ordering and trust boundary. The callback
/// exhausts its permitted protocols for one family before IPv4 is admitted.
pub(crate) async fn connect<T, F, Fut>(
    profile: &Profile,
    identity: &MasqueTlsIdentity,
    protector: &Arc<dyn SocketProtector>,
    telemetry: &crate::ConnectionTelemetry,
    attempt: F,
) -> Result<T, TransportError>
where
    T: crate::endpoint_race::RaceConnection,
    F: FnMut(Vec<SocketAddr>, u64) -> Fut,
    Fut: std::future::Future<Output = Result<T, TransportError>>,
{
    connect_cancellable(
        profile,
        identity,
        protector,
        telemetry,
        &tokio_util::sync::CancellationToken::new(),
        attempt,
    )
    .await
}

pub(crate) async fn connect_cancellable<T, F, Fut>(
    profile: &Profile,
    identity: &MasqueTlsIdentity,
    protector: &Arc<dyn SocketProtector>,
    telemetry: &crate::ConnectionTelemetry,
    cancellation: &tokio_util::sync::CancellationToken,
    mut attempt: F,
) -> Result<T, TransportError>
where
    T: crate::endpoint_race::RaceConnection,
    F: FnMut(Vec<SocketAddr>, u64) -> Fut,
    Fut: std::future::Future<Output = Result<T, TransportError>>,
{
    let generation = protector.network_generation().unwrap_or_default();
    let resolved = tokio::select! {
        biased;
        _ = cancellation.cancelled() => return Err(TransportError::UnderlyingNetworkChanged),
        result = resolve(profile, identity, protector) => result,
    };
    let error = match resolved {
        Ok(resolved) => match attempt(resolved.candidates, resolved.generation).await {
            Ok(connection) => {
                if protector.network_generation().unwrap_or_default() != generation {
                    connection.shutdown().await;
                    return Err(TransportError::UnderlyingNetworkChanged);
                }
                return Ok(connection);
            }
            Err(error) => error,
        },
        Err(error) => error,
    };
    if protector.network_generation().unwrap_or_default() != generation {
        return Err(TransportError::UnderlyingNetworkChanged);
    }
    if !may_fallback(profile, &error) {
        return Err(error);
    }
    telemetry.increment_fallback();
    telemetry.record(
        crate::ConnectionEventType::FallbackStarted,
        Some(usque_core::TransportStage::EndpointResolution),
        crate::telemetry::ConnectionEventPath::new(None, Some(usque_core::AddressFamily::Ipv4)),
        None,
        Some(error.failure(None, Some(usque_core::AddressFamily::Ipv6))),
    );
    let connection = attempt(vec![profile.endpoint.ipv4_socket()], generation).await?;
    if protector.network_generation().unwrap_or_default() != generation {
        connection.shutdown().await;
        return Err(TransportError::UnderlyingNetworkChanged);
    }
    Ok(connection)
}

pub(crate) fn for_generation(
    inner: Arc<dyn SocketProtector>,
    generation: u64,
) -> Arc<dyn SocketProtector> {
    Arc::new(PresetGenerationProtector { inner, generation })
}

struct PresetGenerationProtector {
    inner: Arc<dyn SocketProtector>,
    generation: u64,
}

#[async_trait::async_trait]
impl SocketProtector for PresetGenerationProtector {
    fn protect(&self, socket: crate::socket::SocketHandle) -> Result<(), String> {
        if self.inner.network_generation().unwrap_or_default() != self.generation {
            return Err(crate::socket::STALE_GENERATION_REASON.into());
        }
        self.inner.protect(socket)
    }
    async fn protect_masque_endpoint_generation(
        &self,
        socket: crate::socket::SocketHandle,
        remote: SocketAddr,
        protocol: crate::socket::DirectProtocol,
        expected_generation: u64,
    ) -> Result<crate::socket::DirectEgressLease, String> {
        if expected_generation != self.generation {
            return Err(crate::socket::STALE_GENERATION_REASON.into());
        }
        self.inner
            .protect_masque_endpoint_generation(socket, remote, protocol, self.generation)
            .await
    }
    fn endpoint_family_available(&self, endpoint: SocketAddr) -> Option<bool> {
        self.inner.endpoint_family_available(endpoint)
    }
    fn network_generation(&self) -> Option<u64> {
        self.inner.network_generation()
    }
}

/// Proxy mode has no platform DNS-server inventory. A family-restricted OS
/// lookup issues AAAA only; its late result is never retained after timeout.
pub(crate) async fn resolve_system_ipv6(host: &str, port: u16) -> Result<Vec<SocketAddr>, String> {
    let host = host.to_owned();
    tokio::task::spawn_blocking(move || system_ipv6(&host, port))
        .await
        .map_err(|_| "endpoint preset resolver failed".to_owned())?
}

#[cfg(unix)]
fn system_ipv6(host: &str, port: u16) -> Result<Vec<SocketAddr>, String> {
    let host = std::ffi::CString::new(host).map_err(|_| "invalid endpoint hostname")?;
    // SAFETY: addrinfo is a C record whose all-zero state is valid; pointers
    // passed to getaddrinfo remain valid until it returns and are freed once.
    unsafe {
        let mut hints: libc::addrinfo = std::mem::zeroed();
        hints.ai_family = libc::AF_INET6;
        hints.ai_socktype = libc::SOCK_STREAM;
        let mut result = std::ptr::null_mut();
        if libc::getaddrinfo(host.as_ptr(), std::ptr::null(), &hints, &mut result) != 0 {
            return Err("endpoint preset AAAA lookup failed".into());
        }
        let mut addresses = Vec::new();
        let mut entry = result;
        while let Some(value) = entry.as_ref() {
            if value.ai_family == libc::AF_INET6
                && !value.ai_addr.is_null()
                && value.ai_addrlen as usize >= std::mem::size_of::<libc::sockaddr_in6>()
            {
                let ip = (*(value.ai_addr.cast::<libc::sockaddr_in6>()))
                    .sin6_addr
                    .s6_addr;
                addresses.push(SocketAddr::new(std::net::Ipv6Addr::from(ip).into(), port));
            }
            entry = value.ai_next;
        }
        libc::freeaddrinfo(result);
        Ok(canonical_candidates(addresses, port))
    }
}

#[cfg(test)]
mod tests;

#[cfg(windows)]
fn system_ipv6(host: &str, port: u16) -> Result<Vec<SocketAddr>, String> {
    use windows_sys::Win32::Networking::WinSock::{
        ADDRINFOW, AF_INET6, FreeAddrInfoW, GetAddrInfoW, SOCK_STREAM, SOCKADDR_IN6,
    };
    // std initializes Winsock before the direct family-restricted API call.
    let _winsock =
        std::net::UdpSocket::bind("127.0.0.1:0").map_err(|_| "initialize endpoint resolver")?;
    let host = host.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    // SAFETY: the zeroed hints are valid, the UTF-16 string is terminated, and
    // every returned record is checked for IPv6 layout before being read.
    unsafe {
        let mut hints: ADDRINFOW = std::mem::zeroed();
        hints.ai_family = i32::from(AF_INET6);
        hints.ai_socktype = SOCK_STREAM;
        let mut result = std::ptr::null_mut();
        if GetAddrInfoW(host.as_ptr(), std::ptr::null(), &hints, &mut result) != 0 {
            return Err("endpoint preset AAAA lookup failed".into());
        }
        let mut addresses = Vec::new();
        let mut entry = result;
        while let Some(value) = entry.as_ref() {
            if value.ai_family == i32::from(AF_INET6)
                && !value.ai_addr.is_null()
                && value.ai_addrlen >= std::mem::size_of::<SOCKADDR_IN6>()
            {
                let ip = (*(value.ai_addr.cast::<SOCKADDR_IN6>())).sin6_addr.u.Byte;
                addresses.push(SocketAddr::new(std::net::Ipv6Addr::from(ip).into(), port));
            }
            entry = value.ai_next;
        }
        FreeAddrInfoW(result);
        Ok(canonical_candidates(addresses, port))
    }
}
