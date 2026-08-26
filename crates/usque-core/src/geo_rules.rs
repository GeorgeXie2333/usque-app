//! GeoIP/GeoSite cache listing, download, and profile-enablement checks.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use usque_geo::{
    ArtifactKind, ArtifactResult, CachedCountry, CountryCode, GeoClassifier, GeoDownloader,
    GeoError, HttpFetch, UpdateStatus, geoip_cache_path, geosite_cache_path, list_cached_countries,
};

use crate::config::{ConfigError, Profile, normalize_geo_direct_countries};

pub use crate::config::MAX_GEO_DIRECT_COUNTRIES;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeoRulesEntry {
    pub country_code: String,
    pub has_geoip: bool,
    pub has_geosite: bool,
    pub last_updated_unix_milliseconds: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeoProgress {
    pub current_file: String,
    pub completed: u32,
    pub total: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeoRulesUpdate {
    pub country_code: String,
    pub artifact_kind: String,
    pub status: UpdateStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
struct LastUpdateState {
    last_successful_update_unix_milliseconds: i64,
}

pub fn list_geo_rules(cache_dir: &Path) -> Result<(Vec<GeoRulesEntry>, i64), GeoError> {
    let cached = list_cached_countries(cache_dir)?;
    let entries = cached
        .into_iter()
        .map(|country| entry_from_cached(cache_dir, country))
        .collect();
    Ok((entries, last_successful_update(cache_dir)))
}

pub fn last_successful_update(cache_dir: &Path) -> i64 {
    let path = last_update_path(cache_dir);
    std::fs::read(&path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<LastUpdateState>(&bytes).ok())
        .map(|state| state.last_successful_update_unix_milliseconds)
        .unwrap_or(0)
}

pub fn record_successful_geo_update(cache_dir: &Path) -> Result<(), GeoError> {
    let path = last_update_path(cache_dir);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let state = LastUpdateState {
        last_successful_update_unix_milliseconds: unix_millis(SystemTime::now()),
    };
    let bytes = serde_json::to_vec_pretty(&state).unwrap_or_else(|_| b"{}\n".to_vec());
    std::fs::write(path, bytes)?;
    Ok(())
}

pub fn validate_geo_direct_cache(profile: &Profile, cache_dir: &Path) -> Result<(), ConfigError> {
    let countries = normalize_geo_direct_countries(&profile.geo_direct_countries)?;
    if countries.is_empty() {
        return Ok(());
    }
    let codes = countries
        .iter()
        .map(|code| CountryCode::parse(code))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| ConfigError::InvalidGeoDirectCountry(error.to_string()))?;
    let classifier = GeoClassifier::load(cache_dir, &codes).map_err(|error| match error {
        GeoError::MissingArtifact { country, .. } => {
            ConfigError::GeoDirectCountryNotDownloaded(country.to_string())
        }
        GeoError::InvalidGeoIp => codes
            .first()
            .map(|country| ConfigError::GeoDirectCountryNotDownloaded(country.to_string()))
            .unwrap_or_else(|| ConfigError::GeoDirectCountryNotDownloaded(String::new())),
        other => ConfigError::GeoDirectCountryNotDownloaded(other.to_string()),
    })?;
    if profile.frontends.tunnel {
        for server in &profile.dns_servers {
            if classifier.lookup_ip(*server).is_some() {
                return Err(ConfigError::VpnDnsServerBypassed(*server));
            }
        }
    }
    Ok(())
}

pub async fn download_geo_rules<F, P>(
    downloader: &GeoDownloader<F>,
    country: &str,
    mut progress: P,
) -> Result<Vec<GeoRulesUpdate>, GeoError>
where
    F: HttpFetch,
    P: FnMut(GeoProgress),
{
    let country = CountryCode::parse(country)?;
    let include_geosite = country.as_str() == "CN";
    let total = if include_geosite { 2 } else { 1 };
    let mut results = Vec::new();

    progress(GeoProgress {
        current_file: format!("{country} geoip"),
        completed: 0,
        total,
    });
    match downloader.download_geoip(&country).await {
        Ok(()) => results.push(GeoRulesUpdate {
            country_code: country.to_string(),
            artifact_kind: ArtifactKind::GeoIp.to_string(),
            status: UpdateStatus::Updated,
        }),
        Err(error) => {
            results.push(GeoRulesUpdate {
                country_code: country.to_string(),
                artifact_kind: ArtifactKind::GeoIp.to_string(),
                status: UpdateStatus::Failed {
                    reason: error.to_string(),
                },
            });
            return Ok(results);
        }
    }
    progress(GeoProgress {
        current_file: format!("{country} geoip"),
        completed: 1,
        total,
    });

    if include_geosite {
        progress(GeoProgress {
            current_file: format!("{country} geosite"),
            completed: 1,
            total,
        });
        match downloader.download_geosite(&country).await {
            Ok(()) => results.push(GeoRulesUpdate {
                country_code: country.to_string(),
                artifact_kind: ArtifactKind::GeoSite.to_string(),
                status: UpdateStatus::Updated,
            }),
            Err(error) => results.push(GeoRulesUpdate {
                country_code: country.to_string(),
                artifact_kind: ArtifactKind::GeoSite.to_string(),
                status: UpdateStatus::Failed {
                    reason: error.to_string(),
                },
            }),
        }
        progress(GeoProgress {
            current_file: format!("{country} geosite"),
            completed: 2,
            total,
        });
    }

    if results
        .iter()
        .any(|result| matches!(result.status, UpdateStatus::Updated))
    {
        record_successful_geo_update(downloader.cache_dir())?;
    }
    Ok(results)
}

pub async fn update_all_geo_rules<F, P>(
    downloader: &GeoDownloader<F>,
    mut progress: P,
) -> Result<Vec<GeoRulesUpdate>, GeoError>
where
    F: HttpFetch,
    P: FnMut(GeoProgress),
{
    let cached = list_cached_countries(downloader.cache_dir())?;
    let mut jobs = Vec::new();
    for country in &cached {
        if country.geoip {
            jobs.push((country.country.clone(), ArtifactKind::GeoIp));
        }
        if country.geosite {
            jobs.push((country.country.clone(), ArtifactKind::GeoSite));
        }
    }
    let total = u32::try_from(jobs.len()).unwrap_or(u32::MAX);
    if total == 0 {
        return Ok(Vec::new());
    }
    let mut results = Vec::with_capacity(jobs.len());
    for (index, (country, kind)) in jobs.into_iter().enumerate() {
        let completed = u32::try_from(index).unwrap_or(u32::MAX);
        progress(GeoProgress {
            current_file: format!("{country} {kind}"),
            completed,
            total,
        });
        let artifact = match kind {
            ArtifactKind::GeoIp => downloader.update_geoip(&country).await,
            ArtifactKind::GeoSite => downloader.update_geosite(&country).await,
        };
        results.push(from_artifact(artifact));
        progress(GeoProgress {
            current_file: format!("{country} {kind}"),
            completed: completed.saturating_add(1),
            total,
        });
    }
    if results.iter().any(|result| {
        matches!(
            result.status,
            UpdateStatus::Updated | UpdateStatus::UpToDate
        )
    }) {
        record_successful_geo_update(downloader.cache_dir())?;
    }
    Ok(results)
}

fn from_artifact(artifact: ArtifactResult) -> GeoRulesUpdate {
    GeoRulesUpdate {
        country_code: artifact.country.to_string(),
        artifact_kind: artifact.kind.to_string(),
        status: artifact.status,
    }
}

fn entry_from_cached(cache_dir: &Path, country: CachedCountry) -> GeoRulesEntry {
    let mut last_updated = 0;
    if country.geoip {
        last_updated = last_updated.max(file_mtime_millis(&geoip_cache_path(
            cache_dir,
            &country.country,
        )));
    }
    if country.geosite {
        last_updated = last_updated.max(file_mtime_millis(&geosite_cache_path(
            cache_dir,
            &country.country,
        )));
    }
    GeoRulesEntry {
        country_code: country.country.to_string(),
        has_geoip: country.geoip,
        has_geosite: country.geosite,
        last_updated_unix_milliseconds: last_updated,
    }
}

fn last_update_path(cache_dir: &Path) -> std::path::PathBuf {
    cache_dir.join("geo").join("last-update.json")
}

fn file_mtime_millis(path: &Path) -> i64 {
    std::fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .map(unix_millis)
        .unwrap_or(0)
}

fn unix_millis(time: SystemTime) -> i64 {
    i64::try_from(
        time.duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
    )
    .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::{list_geo_rules, validate_geo_direct_cache};
    use crate::config::{ConfigError, FrontendSettings, Profile, ProxySettings};
    use usque_geo::{CountryCode, geoip_cache_path};

    const GEOIP_CN: &[u8] = include_bytes!("../../usque-geo/tests/fixtures/geoip-cn.dat");

    fn vpn_profile(countries: &[&str]) -> Profile {
        Profile {
            geo_direct_countries: countries.iter().map(|code| (*code).to_owned()).collect(),
            frontends: FrontendSettings {
                tunnel: true,
                socks5: false,
                http: false,
            },
            proxy: ProxySettings {
                system_proxy: false,
                ..ProxySettings::default()
            },
            ..Profile::default()
        }
    }

    #[test]
    fn lists_empty_cache() {
        let directory = tempfile::tempdir().unwrap();
        let (entries, last) = list_geo_rules(directory.path()).unwrap();
        assert!(entries.is_empty());
        assert_eq!(last, 0);
    }

    #[test]
    fn enable_without_cache_is_rejected() {
        let directory = tempfile::tempdir().unwrap();
        let error = validate_geo_direct_cache(&vpn_profile(&["CN"]), directory.path()).unwrap_err();
        assert!(matches!(
            error,
            ConfigError::GeoDirectCountryNotDownloaded(_)
        ));
    }

    #[tokio::test]
    async fn download_uses_injected_http_and_does_not_touch_jsdelivr() {
        use std::collections::HashMap;
        use std::sync::Mutex;

        use bytes::Bytes;
        use usque_geo::{ALLOWED_HOSTS, FetchedBody, GeoDownloader, HttpFetch, geoip_cache_path};

        struct MockFetch {
            routes: HashMap<String, Vec<u8>>,
            hits: Mutex<Vec<String>>,
        }

        impl HttpFetch for MockFetch {
            async fn get_capped(
                &self,
                url: &str,
                _max_bytes: usize,
            ) -> Result<FetchedBody, usque_geo::GeoError> {
                self.hits.lock().expect("hits").push(url.to_owned());
                let body = self.routes.get(url).cloned().unwrap_or_default();
                Ok(FetchedBody {
                    status: if body.is_empty() { 404 } else { 200 },
                    url: url.to_owned(),
                    body: Bytes::from(body),
                })
            }
        }

        let mut routes = HashMap::new();
        for host in ALLOWED_HOSTS {
            routes.insert(
                format!("https://{host}/gh/v2fly/geoip@release/cn.dat"),
                GEOIP_CN.to_vec(),
            );
            routes.insert(
                format!("https://{host}/gh/v2fly/geoip@release/cn.dat.sha256sum"),
                include_str!("../../usque-geo/tests/fixtures/geoip-cn.dat.sha256sum")
                    .as_bytes()
                    .to_vec(),
            );
        }
        let fetch = MockFetch {
            routes,
            hits: Mutex::new(Vec::new()),
        };
        let directory = tempfile::tempdir().unwrap();
        let downloader = GeoDownloader::new(fetch, directory.path());
        let results = super::download_geo_rules(&downloader, "CN", |_| {})
            .await
            .unwrap();
        assert!(results.iter().any(|result| result.artifact_kind == "geoip"));
        assert!(geoip_cache_path(directory.path(), &CountryCode::parse("CN").unwrap()).exists());
    }

    #[test]
    fn tunnel_dns_inside_enabled_country_is_rejected() {
        let directory = tempfile::tempdir().unwrap();
        let cn = CountryCode::parse("CN").unwrap();
        let path = geoip_cache_path(directory.path(), &cn);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, GEOIP_CN).unwrap();

        let mut profile = vpn_profile(&["CN"]);
        profile.dns_servers = vec!["1.2.3.4".parse().unwrap()];
        assert_eq!(
            validate_geo_direct_cache(&profile, directory.path()),
            Err(ConfigError::VpnDnsServerBypassed(
                "1.2.3.4".parse().unwrap()
            ))
        );

        profile.dns_servers = vec!["1.1.1.1".parse().unwrap()];
        assert_eq!(
            validate_geo_direct_cache(&profile, directory.path()),
            Ok(())
        );
    }
}
