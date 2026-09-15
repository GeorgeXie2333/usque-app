use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

use reqwest::Client;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{AddressFamily, ProxyAuthCredentials};

const IPV4_ENDPOINT: &str = "https://api-ipv4.ip.sb/ip";
const IPV6_ENDPOINT: &str = "https://api-ipv6.ip.sb/ip";
const GEO_ENDPOINT: &str = "https://api.ip.sb/geoip";
const LOOKUP_RETRY_DELAY: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExitInfo {
    pub ipv4: Option<IpAddr>,
    pub ipv6: Option<IpAddr>,
    pub ipv4_location: Option<GeoLocation>,
    pub ipv6_location: Option<GeoLocation>,
    pub checked_at: chrono::DateTime<chrono::Utc>,
}

impl ExitInfo {
    pub fn primary_location(&self) -> Option<&GeoLocation> {
        self.ipv4_location.as_ref().or(self.ipv6_location.as_ref())
    }
}

/// Shared exit discovery for both the ordinary WARP client and the VPN Gate
/// internal network. Each failed IP or GeoIP lookup gets one retry after a
/// short delay; successful lookups are retained. Callers must cancel/drop this
/// future when its session ends, and each supplied lookup must remain bounded
/// and use that same session's network.
pub async fn probe_exit_with_retry<FetchIp, IpFuture, FetchGeo, GeoFuture>(
    fetch_ip: FetchIp,
    fetch_geo: FetchGeo,
) -> Result<ExitInfo, ProbeError>
where
    FetchIp: Fn(AddressFamily) -> IpFuture,
    IpFuture: Future<Output = Option<IpAddr>>,
    FetchGeo: Fn(IpAddr) -> GeoFuture,
    GeoFuture: Future<Output = Option<GeoLocation>>,
{
    let (ipv4, ipv6) = tokio::join!(
        retry_lookup(|| fetch_ip(AddressFamily::Ipv4)),
        retry_lookup(|| fetch_ip(AddressFamily::Ipv6)),
    );
    if ipv4.is_none() && ipv6.is_none() {
        return Err(ProbeError::NoAddressFamily);
    }
    let (ipv4_location, ipv6_location) = tokio::join!(
        lookup_location(ipv4, &fetch_geo),
        lookup_location(ipv6, &fetch_geo),
    );
    Ok(ExitInfo {
        ipv4,
        ipv6,
        ipv4_location,
        ipv6_location,
        checked_at: chrono::Utc::now(),
    })
}

async fn lookup_location<FetchGeo, GeoFuture>(
    ip: Option<IpAddr>,
    fetch_geo: &FetchGeo,
) -> Option<GeoLocation>
where
    FetchGeo: Fn(IpAddr) -> GeoFuture,
    GeoFuture: Future<Output = Option<GeoLocation>>,
{
    let ip = ip?;
    retry_lookup(|| fetch_geo(ip)).await
}

async fn retry_lookup<T, Lookup, LookupFuture>(lookup: Lookup) -> Option<T>
where
    Lookup: Fn() -> LookupFuture,
    LookupFuture: Future<Output = Option<T>>,
{
    if let Some(value) = lookup().await {
        return Some(value);
    }
    tokio::time::sleep(LOOKUP_RETRY_DELAY).await;
    lookup().await
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GeoLocation {
    pub ip: IpAddr,
    pub country_code: Option<String>,
    pub country: Option<String>,
    pub region: Option<String>,
    pub city: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub organization: Option<String>,
    pub timezone: Option<String>,
    /// Legacy wire field. Clients render bundled flags using country_code.
    pub flag_svg: Option<String>,
}

impl GeoLocation {
    pub fn display_name(&self) -> String {
        match (self.city.as_deref(), self.country.as_deref()) {
            (Some(city), Some(country)) if !city.is_empty() => format!("{city}, {country}"),
            (_, Some(country)) => country.to_owned(),
            _ => "Unknown location".to_owned(),
        }
    }
}

fn authenticated_proxy(
    scheme: &str,
    proxy: SocketAddr,
    auth: Option<&ProxyAuthCredentials>,
) -> Result<reqwest::Proxy, ProbeError> {
    let mut proxy = reqwest::Proxy::all(format!("{scheme}://{proxy}"))?;
    if let Some(auth) = auth
        && let Ok(password) = std::str::from_utf8(auth.password_bytes())
    {
        proxy = proxy.basic_auth(auth.username(), password);
    }
    Ok(proxy)
}

#[derive(Clone)]
pub struct IpSbProbe {
    client: Client,
}

impl IpSbProbe {
    pub fn new() -> Result<Self, ProbeError> {
        Self::from_builder(Client::builder())
    }

    pub fn through_socks(proxy: SocketAddr) -> Result<Self, ProbeError> {
        Self::through_socks_with_auth(proxy, None)
    }

    pub fn through_http(proxy: SocketAddr) -> Result<Self, ProbeError> {
        Self::through_http_with_auth(proxy, None)
    }

    pub fn through_socks_with_auth(
        proxy: SocketAddr,
        auth: Option<&ProxyAuthCredentials>,
    ) -> Result<Self, ProbeError> {
        Self::from_builder(Client::builder().proxy(authenticated_proxy("socks5h", proxy, auth)?))
    }

    pub fn through_http_with_auth(
        proxy: SocketAddr,
        auth: Option<&ProxyAuthCredentials>,
    ) -> Result<Self, ProbeError> {
        Self::from_builder(Client::builder().proxy(authenticated_proxy("http", proxy, auth)?))
    }

    fn from_builder(builder: reqwest::ClientBuilder) -> Result<Self, ProbeError> {
        let client = builder
            .connect_timeout(Duration::from_secs(4))
            .timeout(Duration::from_secs(8))
            .user_agent("Usque/0.1 (+https://github.com/GeorgeXie2333/usque-app)")
            .build()?;
        Ok(Self { client })
    }

    /// The caller must arrange for this client's sockets to use the tunnel data
    /// plane. Probe failure is diagnostic and must not tear down a healthy VPN.
    pub async fn probe(&self) -> Result<ExitInfo, ProbeError> {
        probe_exit_with_retry(
            |family| async move {
                self.fetch_ip(match family {
                    AddressFamily::Ipv4 => IPV4_ENDPOINT,
                    AddressFamily::Ipv6 => IPV6_ENDPOINT,
                })
                .await
                .ok()
            },
            |ip| async move { self.fetch_geo(ip).await.ok() },
        )
        .await
    }

    async fn fetch_ip(&self, endpoint: &str) -> Result<IpAddr, ProbeError> {
        let response = self.client.get(endpoint).send().await?.error_for_status()?;
        let body = response.text().await?;
        body.trim()
            .parse()
            .map_err(|_| ProbeError::InvalidIp(body.trim().to_owned()))
    }

    async fn fetch_geo(&self, ip: IpAddr) -> Result<GeoLocation, ProbeError> {
        let response = self
            .client
            .get(format!("{GEO_ENDPOINT}/{ip}"))
            .send()
            .await?
            .error_for_status()?;
        let wire: GeoWire = response.json().await?;
        if wire.ip != ip {
            return Err(ProbeError::MismatchedGeoIp {
                expected: ip,
                received: wire.ip,
            });
        }
        Ok(wire.into())
    }
}

#[derive(Debug, Deserialize)]
struct GeoWire {
    ip: IpAddr,
    country_code: Option<String>,
    country: Option<String>,
    region: Option<String>,
    city: Option<String>,
    latitude: Option<f64>,
    longitude: Option<f64>,
    organization: Option<String>,
    timezone: Option<String>,
}

impl From<GeoWire> for GeoLocation {
    fn from(value: GeoWire) -> Self {
        Self {
            ip: value.ip,
            country_code: value.country_code,
            country: value.country,
            region: value.region,
            city: value.city,
            latitude: value.latitude,
            longitude: value.longitude,
            organization: value.organization,
            timezone: value.timezone,
            flag_svg: None,
        }
    }
}

#[derive(Debug, Error)]
pub enum ProbeError {
    #[error("IP.SB request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("IP.SB returned an invalid IP address: {0}")]
    InvalidIp(String),
    #[error("both IPv4 and IPv6 exit checks failed")]
    NoAddressFamily,
    #[error("GeoIP response IP mismatch: expected {expected}, received {received}")]
    MismatchedGeoIp { expected: IpAddr, received: IpAddr },
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn sample_ip(family: AddressFamily) -> IpAddr {
        match family {
            AddressFamily::Ipv4 => "203.0.113.7",
            AddressFamily::Ipv6 => "2001:db8::7",
        }
        .parse()
        .unwrap()
    }

    fn sample_location(ip: IpAddr) -> GeoLocation {
        GeoLocation {
            ip,
            country_code: Some("SG".into()),
            country: Some("Singapore".into()),
            region: None,
            city: None,
            latitude: None,
            longitude: None,
            organization: None,
            timezone: None,
            flag_svg: None,
        }
    }

    #[tokio::test(start_paused = true)]
    async fn successful_exit_lookups_are_not_repeated_or_delayed() {
        let ip_calls = Cell::new(0);
        let geo_calls = Cell::new(0);
        let started = tokio::time::Instant::now();
        let exit = probe_exit_with_retry(
            |family| {
                ip_calls.set(ip_calls.get() + 1);
                std::future::ready(Some(sample_ip(family)))
            },
            |ip| {
                geo_calls.set(geo_calls.get() + 1);
                std::future::ready(Some(sample_location(ip)))
            },
        )
        .await
        .unwrap();
        assert_eq!((ip_calls.get(), geo_calls.get()), (2, 2));
        assert_eq!(started.elapsed(), Duration::ZERO);
        assert_eq!(exit.ipv4_location.unwrap().ip, exit.ipv4.unwrap());
        assert_eq!(exit.ipv6_location.unwrap().ip, exit.ipv6.unwrap());
    }

    #[tokio::test(start_paused = true)]
    async fn retries_failed_ip_then_geo_once_and_keeps_successful_family() {
        let ip_calls = [Cell::new(0), Cell::new(0)];
        let geo_calls = [Cell::new(0), Cell::new(0)];
        let started = tokio::time::Instant::now();
        let exit = probe_exit_with_retry(
            |family| {
                let index = usize::from(family == AddressFamily::Ipv6);
                ip_calls[index].set(ip_calls[index].get() + 1);
                std::future::ready(
                    (index == 1 || ip_calls[index].get() == 2).then(|| sample_ip(family)),
                )
            },
            |ip| {
                let index = usize::from(ip.is_ipv6());
                geo_calls[index].set(geo_calls[index].get() + 1);
                std::future::ready(
                    (index == 1 || geo_calls[index].get() == 2).then(|| sample_location(ip)),
                )
            },
        )
        .await
        .unwrap();
        assert_eq!(ip_calls.map(|calls| calls.get()), [2, 1]);
        assert_eq!(geo_calls.map(|calls| calls.get()), [2, 1]);
        assert_eq!(started.elapsed(), LOOKUP_RETRY_DELAY * 2);
        assert_eq!(exit.ipv4_location.unwrap().ip, exit.ipv4.unwrap());
        assert_eq!(exit.ipv6_location.unwrap().ip, exit.ipv6.unwrap());
    }

    #[tokio::test(start_paused = true)]
    async fn two_failed_ip_attempts_stop_without_requesting_location() {
        let calls = [Cell::new(0), Cell::new(0)];
        let geo_calls = Cell::new(0);
        let result = probe_exit_with_retry(
            |family| {
                let index = usize::from(family == AddressFamily::Ipv6);
                calls[index].set(calls[index].get() + 1);
                std::future::ready(None)
            },
            |_| {
                geo_calls.set(geo_calls.get() + 1);
                std::future::ready(None)
            },
        )
        .await;
        assert!(matches!(result, Err(ProbeError::NoAddressFamily)));
        assert_eq!(calls.map(|calls| calls.get()), [2, 2]);
        assert_eq!(geo_calls.get(), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn two_failed_location_attempts_keep_measured_ips() {
        let calls = [Cell::new(0), Cell::new(0)];
        let exit = probe_exit_with_retry(
            |family| std::future::ready(Some(sample_ip(family))),
            |ip| {
                let index = usize::from(ip.is_ipv6());
                calls[index].set(calls[index].get() + 1);
                std::future::ready(None)
            },
        )
        .await
        .unwrap();
        assert_eq!(calls.map(|calls| calls.get()), [2, 2]);
        assert_eq!(exit.ipv4, Some(sample_ip(AddressFamily::Ipv4)));
        assert_eq!(exit.ipv6, Some(sample_ip(AddressFamily::Ipv6)));
        assert!(exit.ipv4_location.is_none());
        assert!(exit.ipv6_location.is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn failed_ipv6_lookups_do_not_discard_ipv4_location() {
        let calls = [Cell::new(0), Cell::new(0)];
        let exit = probe_exit_with_retry(
            |family| {
                let index = usize::from(family == AddressFamily::Ipv6);
                calls[index].set(calls[index].get() + 1);
                std::future::ready((index == 0).then(|| sample_ip(family)))
            },
            |ip| {
                assert!(ip.is_ipv4());
                std::future::ready(Some(sample_location(ip)))
            },
        )
        .await
        .unwrap();
        assert_eq!(calls.map(|calls| calls.get()), [1, 2]);
        assert_eq!(exit.ipv4_location.unwrap().ip, exit.ipv4.unwrap());
        assert!(exit.ipv6.is_none());
        assert!(exit.ipv6_location.is_none());
    }

    #[test]
    fn location_display_name_is_stable() {
        let location = GeoLocation {
            ip: "134.13.96.166".parse().unwrap(),
            country_code: Some("US".to_owned()),
            country: Some("United States".to_owned()),
            region: Some("California".to_owned()),
            city: Some("Los Angeles".to_owned()),
            latitude: None,
            longitude: None,
            organization: None,
            timezone: None,
            flag_svg: None,
        };
        assert_eq!(location.display_name(), "Los Angeles, United States");
    }

    #[test]
    fn missing_city_falls_back_to_country() {
        let location = GeoLocation {
            ip: "2606:4700:103::2".parse().unwrap(),
            country_code: Some("SG".to_owned()),
            country: Some("Singapore".to_owned()),
            region: None,
            city: None,
            latitude: None,
            longitude: None,
            organization: None,
            timezone: None,
            flag_svg: None,
        };
        assert_eq!(location.display_name(), "Singapore");
    }
}
