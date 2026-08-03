use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::time::Duration;

use futures::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

const IPV4_ENDPOINT: &str = "https://api-ipv4.ip.sb/ip";
const IPV6_ENDPOINT: &str = "https://api-ipv6.ip.sb/ip";
const GEO_ENDPOINT: &str = "https://api.ip.sb/geoip";
const FLAG_CDN_BASE: &str = "https://cdn.jsdelivr.net/gh/lipis/flag-icons@7.5.0/flags/4x3";
const MAX_FLAG_SVG_BYTES: usize = 64 * 1024;

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

    pub fn flag_url(&self) -> Option<String> {
        let code = self.country_code.as_deref()?.to_ascii_lowercase();
        if code.len() == 2 && code.bytes().all(|byte| byte.is_ascii_lowercase()) {
            Some(format!("{FLAG_CDN_BASE}/{code}.svg"))
        } else {
            None
        }
    }
}

#[derive(Clone)]
pub struct IpSbProbe {
    client: Client,
    flag_cache_directory: Option<PathBuf>,
}

impl IpSbProbe {
    pub fn new() -> Result<Self, ProbeError> {
        Self::from_builder(Client::builder())
    }

    pub fn through_socks(proxy: SocketAddr) -> Result<Self, ProbeError> {
        let proxy = reqwest::Proxy::all(format!("socks5h://{proxy}"))?;
        Self::from_builder(Client::builder().proxy(proxy))
    }

    pub fn through_http(proxy: SocketAddr) -> Result<Self, ProbeError> {
        let proxy = reqwest::Proxy::all(format!("http://{proxy}"))?;
        Self::from_builder(Client::builder().proxy(proxy))
    }

    fn from_builder(builder: reqwest::ClientBuilder) -> Result<Self, ProbeError> {
        let client = builder
            .connect_timeout(Duration::from_secs(4))
            .timeout(Duration::from_secs(8))
            .user_agent("Usque/0.1 (+https://github.com/GeorgeXie2333/usque-app)")
            .build()?;
        Ok(Self {
            client,
            flag_cache_directory: None,
        })
    }

    /// Uses a version-scoped local cache for already validated flag SVGs.
    /// Network misses still use this probe's configured tunneled HTTP client.
    pub fn with_flag_cache(mut self, directory: impl Into<PathBuf>) -> Self {
        self.flag_cache_directory = Some(directory.into());
        self
    }

    /// The caller must arrange for this client's sockets to use the tunnel data
    /// plane. Probe failure is diagnostic and must not tear down a healthy VPN.
    pub async fn probe(&self) -> Result<ExitInfo, ProbeError> {
        let (ipv4_result, ipv6_result) =
            tokio::join!(self.fetch_ip(IPV4_ENDPOINT), self.fetch_ip(IPV6_ENDPOINT));
        let ipv4 = ipv4_result.ok();
        let ipv6 = ipv6_result.ok();

        if ipv4.is_none() && ipv6.is_none() {
            return Err(ProbeError::NoAddressFamily);
        }

        let (mut ipv4_location, mut ipv6_location) =
            tokio::join!(self.fetch_optional_geo(ipv4), self.fetch_optional_geo(ipv6));
        if let Some(location) = ipv4_location.as_mut().or(ipv6_location.as_mut())
            && let Ok(flag_svg) = self.fetch_flag_svg(location).await
        {
            location.flag_svg = Some(flag_svg);
        }

        Ok(ExitInfo {
            ipv4,
            ipv6,
            ipv4_location,
            ipv6_location,
            checked_at: chrono::Utc::now(),
        })
    }

    pub async fn fetch_flag_svg(&self, location: &GeoLocation) -> Result<String, ProbeError> {
        if let Some(cached) = self.load_cached_flag_svg(location).await {
            return Ok(cached);
        }
        let url = location.flag_url().ok_or(ProbeError::MissingCountryCode)?;
        let response = self.client.get(url).send().await?.error_for_status()?;
        if response
            .content_length()
            .is_some_and(|length| length > MAX_FLAG_SVG_BYTES as u64)
        {
            return Err(ProbeError::FlagTooLarge);
        }
        let mut body = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            if body.len().saturating_add(chunk.len()) > MAX_FLAG_SVG_BYTES {
                return Err(ProbeError::FlagTooLarge);
            }
            body.extend_from_slice(&chunk);
        }
        let svg = validate_flag_svg(body)?;
        self.store_cached_flag_svg(location, svg.as_bytes()).await;
        Ok(svg)
    }

    async fn load_cached_flag_svg(&self, location: &GeoLocation) -> Option<String> {
        let path = self.flag_cache_path(location)?;
        let bytes = tokio::fs::read(&path).await.ok()?;
        if bytes.is_empty() || bytes.len() > MAX_FLAG_SVG_BYTES {
            let _ = tokio::fs::remove_file(path).await;
            return None;
        }
        match validate_flag_svg(bytes) {
            Ok(svg) => Some(svg),
            Err(_) => {
                let _ = tokio::fs::remove_file(path).await;
                None
            }
        }
    }

    async fn store_cached_flag_svg(&self, location: &GeoLocation, svg: &[u8]) {
        let Some(path) = self.flag_cache_path(location) else {
            return;
        };
        let Some(directory) = path.parent() else {
            return;
        };
        if tokio::fs::create_dir_all(directory).await.is_err() {
            return;
        }
        let temporary = directory.join(format!(".{}.tmp", Uuid::new_v4()));
        let stored = async {
            let mut file = tokio::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&temporary)
                .await?;
            file.write_all(svg).await?;
            file.flush().await?;
            file.sync_all().await?;
            drop(file);
            if tokio::fs::rename(&temporary, &path).await.is_err() {
                let _ = tokio::fs::remove_file(&path).await;
                tokio::fs::rename(&temporary, &path).await?;
            }
            Ok::<(), std::io::Error>(())
        }
        .await;
        if stored.is_err() {
            let _ = tokio::fs::remove_file(temporary).await;
        }
    }

    fn flag_cache_path(&self, location: &GeoLocation) -> Option<PathBuf> {
        let code = normalized_country_code(location.country_code.as_deref()?)?;
        Some(
            self.flag_cache_directory
                .as_ref()?
                .join(format!("{code}.svg")),
        )
    }

    async fn fetch_ip(&self, endpoint: &str) -> Result<IpAddr, ProbeError> {
        let response = self.client.get(endpoint).send().await?.error_for_status()?;
        let body = response.text().await?;
        body.trim()
            .parse()
            .map_err(|_| ProbeError::InvalidIp(body.trim().to_owned()))
    }

    async fn fetch_optional_geo(&self, ip: Option<IpAddr>) -> Option<GeoLocation> {
        let ip = ip?;
        self.fetch_geo(ip).await.ok()
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

fn normalized_country_code(value: &str) -> Option<String> {
    let code = value.to_ascii_lowercase();
    (code.len() == 2 && code.bytes().all(|byte| byte.is_ascii_lowercase())).then_some(code)
}

fn validate_flag_svg(body: Vec<u8>) -> Result<String, ProbeError> {
    let svg = String::from_utf8(body).map_err(|_| ProbeError::InvalidFlagSvg)?;
    let lower = svg.to_ascii_lowercase();
    if !lower.trim_start().starts_with("<svg")
        || lower.contains("<script")
        || lower.contains("<foreignobject")
        || lower.contains("<!entity")
        || lower.contains("onload=")
        || lower.contains("javascript:")
        || lower.contains("xlink:href")
        || lower.contains("href=\"http")
        || lower.contains("href='http")
    {
        return Err(ProbeError::InvalidFlagSvg);
    }
    Ok(svg)
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
    #[error("GeoIP response does not contain a valid country code")]
    MissingCountryCode,
    #[error("flag SVG exceeds the safety limit")]
    FlagTooLarge,
    #[error("flag CDN returned unsafe or invalid SVG")]
    InvalidFlagSvg,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn location_and_versioned_flag_url_are_stable() {
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
        assert_eq!(
            location.flag_url().as_deref(),
            Some("https://cdn.jsdelivr.net/gh/lipis/flag-icons@7.5.0/flags/4x3/us.svg")
        );
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

    #[test]
    fn flag_svg_rejects_active_content_and_external_references() {
        assert!(validate_flag_svg(b"<svg><path d=\"M0 0\"/></svg>".to_vec()).is_ok());
        assert!(validate_flag_svg(b"<svg><script>alert(1)</script></svg>".to_vec()).is_err());
        assert!(
            validate_flag_svg(b"<svg><image href=\"https://example.com/a\"/></svg>".to_vec())
                .is_err()
        );
        assert!(validate_flag_svg(b"not svg".to_vec()).is_err());
    }

    #[tokio::test]
    async fn validated_flag_cache_is_version_scoped_and_rejects_corruption() {
        let directory = tempfile::tempdir().unwrap();
        let cache = directory.path().join("flag-icons-7.5.0");
        let probe = IpSbProbe::new().unwrap().with_flag_cache(&cache);
        let location = GeoLocation {
            ip: "134.13.96.166".parse().unwrap(),
            country_code: Some("US".to_owned()),
            country: Some("United States".to_owned()),
            region: None,
            city: None,
            latitude: None,
            longitude: None,
            organization: None,
            timezone: None,
            flag_svg: None,
        };
        probe
            .store_cached_flag_svg(&location, b"<svg><path d=\"M0 0\"/></svg>")
            .await;
        assert!(
            probe
                .load_cached_flag_svg(&location)
                .await
                .unwrap()
                .starts_with("<svg")
        );

        tokio::fs::write(cache.join("us.svg"), b"<svg><script/></svg>")
            .await
            .unwrap();
        assert!(probe.load_cached_flag_svg(&location).await.is_none());
        assert!(!cache.join("us.svg").exists());
    }
}
