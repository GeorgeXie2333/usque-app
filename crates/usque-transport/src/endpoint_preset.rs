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
        && crate::recovery_policy::RecoveryDecision::may_try_another_candidate(error)
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
    fn requires_owned_sockets(&self) -> bool {
        self.inner.requires_owned_sockets()
    }
    async fn prepare_tcp(
        &self,
        remote: SocketAddr,
        purpose: crate::socket::PhysicalSocketPurpose,
        generation: u64,
    ) -> Result<
        (
            crate::socket::PreparedTcpSocket,
            crate::socket::DirectEgressLease,
        ),
        String,
    > {
        if generation != self.generation {
            return Err(crate::socket::STALE_GENERATION_REASON.into());
        }
        self.inner.prepare_tcp(remote, purpose, generation).await
    }
    async fn prepare_udp(
        &self,
        remote: SocketAddr,
        purpose: crate::socket::PhysicalSocketPurpose,
        generation: u64,
    ) -> Result<(std::net::UdpSocket, crate::socket::DirectEgressLease), String> {
        if generation != self.generation {
            return Err(crate::socket::STALE_GENERATION_REASON.into());
        }
        self.inner.prepare_udp(remote, purpose, generation).await
    }
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
    async fn resolve_endpoint_hostname(
        &self,
        host: &str,
        port: u16,
        ip_policy: usque_core::IpPolicy,
        expected_generation: u64,
    ) -> Result<Vec<SocketAddr>, String> {
        if expected_generation != self.generation
            || self.inner.network_generation().unwrap_or_default() != self.generation
        {
            return Err(crate::socket::STALE_GENERATION_REASON.into());
        }
        let resolved = self
            .inner
            .resolve_endpoint_hostname(host, port, ip_policy, self.generation)
            .await?;
        // Answers from a network that changed during the lookup are discarded.
        if self.inner.network_generation().unwrap_or_default() != self.generation {
            return Err(crate::socket::STALE_GENERATION_REASON.into());
        }
        Ok(resolved)
    }
    async fn authorize_endpoint_hostname_candidates(
        &self,
        host: &str,
        candidates: &[SocketAddr],
        expected_generation: u64,
    ) -> Result<(), String> {
        if expected_generation != self.generation
            || self.inner.network_generation().unwrap_or_default() != self.generation
        {
            return Err(crate::socket::STALE_GENERATION_REASON.into());
        }
        self.inner
            .authorize_endpoint_hostname_candidates(host, candidates, self.generation)
            .await
    }
    async fn acquire_endpoint_probe_lease(
        &self,
        probe_session_id: &str,
        targets: &[crate::socket::ProbeLeaseTarget],
        dns_hostname: Option<&str>,
        expected_generation: u64,
    ) -> Result<crate::socket::EndpointProbeLeaseHandle, String> {
        if expected_generation != self.generation {
            return Err(crate::socket::STALE_GENERATION_REASON.into());
        }
        self.inner
            .acquire_endpoint_probe_lease(probe_session_id, targets, dns_hostname, self.generation)
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

/// Proxy mode has no platform DNS-server inventory. A family-restricted OS
/// lookup issues AAAA only; its late result is never retained after timeout.
pub(crate) async fn resolve_system_ipv6(host: &str, port: u16) -> Result<Vec<SocketAddr>, String> {
    let host = host.to_owned();
    tokio::task::spawn_blocking(move || system_ipv6(&host, port))
        .await
        .map_err(|_| "endpoint preset resolver failed".to_owned())?
}

// NI_MAXHOST on Darwin and Winsock, also large enough for a numeric host
// with an interface suffix on Linux and Android.
const NUMERIC_HOST_BYTES: usize = 1025;

fn parse_numeric_ipv6(host: &[u8]) -> Option<std::net::Ipv6Addr> {
    let host = std::ffi::CStr::from_bytes_until_nul(host)
        .ok()?
        .to_str()
        .ok()?;
    // Preset candidates carry only the address and caller's port. A numeric
    // host returned by the OS may also contain an interface scope suffix.
    host.split('%').next()?.parse().ok()
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
        let mut numeric_host = [0u8; NUMERIC_HOST_BYTES];
        let mut entry = result;
        while let Some(value) = entry.as_ref() {
            entry = value.ai_next;
            if value.ai_family != libc::AF_INET6
                || value.ai_addr.is_null()
                || (value.ai_addrlen as usize) < std::mem::size_of::<libc::sockaddr_in6>()
            {
                continue;
            }
            // Pass the original address and its length back to the resolver.
            // NI_NUMERICHOST formats the address without a reverse DNS lookup.
            let status = libc::getnameinfo(
                value.ai_addr,
                value.ai_addrlen,
                numeric_host.as_mut_ptr().cast(),
                numeric_host.len() as _,
                std::ptr::null_mut(),
                0,
                libc::NI_NUMERICHOST,
            );
            if status == 0
                && let Some(ip) = parse_numeric_ipv6(&numeric_host)
            {
                addresses.push(SocketAddr::new(ip.into(), port));
            }
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
        ADDRINFOW, AF_INET6, FreeAddrInfoW, GetAddrInfoW, NI_NUMERICHOST, SOCK_STREAM,
        SOCKADDR_IN6, getnameinfo,
    };
    // std initializes Winsock before the direct family-restricted API call.
    let _winsock =
        std::net::UdpSocket::bind("127.0.0.1:0").map_err(|_| "initialize endpoint resolver")?;
    let host = host.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    // SAFETY: the zeroed hints are valid, the UTF-16 string is terminated, and
    // Winsock's address and length remain valid until FreeAddrInfoW.
    unsafe {
        let mut hints: ADDRINFOW = std::mem::zeroed();
        hints.ai_family = i32::from(AF_INET6);
        hints.ai_socktype = SOCK_STREAM;
        let mut result = std::ptr::null_mut();
        if GetAddrInfoW(host.as_ptr(), std::ptr::null(), &hints, &mut result) != 0 {
            return Err("endpoint preset AAAA lookup failed".into());
        }
        let mut addresses = Vec::new();
        let mut numeric_host = [0u8; NUMERIC_HOST_BYTES];
        let mut entry = result;
        while let Some(value) = entry.as_ref() {
            entry = value.ai_next;
            if value.ai_family != i32::from(AF_INET6)
                || value.ai_addr.is_null()
                || value.ai_addrlen < std::mem::size_of::<SOCKADDR_IN6>()
            {
                continue;
            }
            let Ok(len) = value.ai_addrlen.try_into() else {
                continue;
            };
            let status = getnameinfo(
                value.ai_addr,
                len,
                numeric_host.as_mut_ptr(),
                numeric_host.len() as u32,
                std::ptr::null_mut(),
                0,
                NI_NUMERICHOST as i32,
            );
            if status == 0
                && let Some(ip) = parse_numeric_ipv6(&numeric_host)
            {
                addresses.push(SocketAddr::new(ip.into(), port));
            }
        }
        FreeAddrInfoW(result);
        Ok(canonical_candidates(addresses, port))
    }
}
