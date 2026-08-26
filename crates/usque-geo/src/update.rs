use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::cache::{
    atomic_write, cached_geoip_countries, cached_geosite_countries, geoip_cache_path,
    geosite_cache_path,
};
use crate::country::CountryCode;
use crate::error::{ArtifactKind, GeoError};
use crate::fetch::{
    ALLOWED_HOSTS, HttpFetch, MAX_CHECKSUM_BYTES, MAX_GEOIP_BYTES, MAX_GEOSITE_BYTES,
    fetch_first_ok, geoip_dat_url, geoip_sha256_url, geosite_cn_url, geosite_geolocation_cn_url,
};
use crate::geoip::GeoIpSet;
use crate::geosite::GeoSiteSet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateStatus {
    UpToDate,
    Updated,
    Failed { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactResult {
    pub country: CountryCode,
    pub kind: ArtifactKind,
    pub status: UpdateStatus,
}

pub struct GeoDownloader<F> {
    fetch: F,
    cache_dir: PathBuf,
}

impl<F: HttpFetch> GeoDownloader<F> {
    pub fn new(fetch: F, cache_dir: impl Into<PathBuf>) -> Self {
        Self {
            fetch,
            cache_dir: cache_dir.into(),
        }
    }

    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }

    pub async fn download_geoip(&self, country: &CountryCode) -> Result<(), GeoError> {
        let bytes = self.fetch_verified_geoip(country).await?;
        atomic_write(&geoip_cache_path(&self.cache_dir, country), &bytes)
    }

    pub async fn download_geosite(&self, country: &CountryCode) -> Result<(), GeoError> {
        let text = self.fetch_validated_geosite(country).await?;
        atomic_write(
            &geosite_cache_path(&self.cache_dir, country),
            text.as_bytes(),
        )
    }

    pub async fn update_cached(&self) -> Vec<ArtifactResult> {
        let mut jobs = Vec::new();
        if let Ok(countries) = cached_geoip_countries(&self.cache_dir) {
            for country in countries {
                jobs.push((country, ArtifactKind::GeoIp));
            }
        }
        if let Ok(countries) = cached_geosite_countries(&self.cache_dir) {
            for country in countries {
                jobs.push((country, ArtifactKind::GeoSite));
            }
        }

        let mut results = Vec::with_capacity(jobs.len());
        for chunk in jobs.chunks(2) {
            match chunk {
                [(country, ArtifactKind::GeoIp)] => {
                    results.push(self.update_geoip(country).await);
                }
                [(country, ArtifactKind::GeoSite)] => {
                    results.push(self.update_geosite(country).await);
                }
                [(left_country, left_kind), (right_country, right_kind)] => {
                    let (left, right) = tokio::join!(
                        self.update_one(left_country, *left_kind),
                        self.update_one(right_country, *right_kind)
                    );
                    results.push(left);
                    results.push(right);
                }
                _ => {}
            }
        }
        results
    }

    async fn update_one(&self, country: &CountryCode, kind: ArtifactKind) -> ArtifactResult {
        match kind {
            ArtifactKind::GeoIp => self.update_geoip(country).await,
            ArtifactKind::GeoSite => self.update_geosite(country).await,
        }
    }

    async fn update_geoip(&self, country: &CountryCode) -> ArtifactResult {
        match self.update_geoip_inner(country).await {
            Ok(status) => ArtifactResult {
                country: country.clone(),
                kind: ArtifactKind::GeoIp,
                status,
            },
            Err(error) => ArtifactResult {
                country: country.clone(),
                kind: ArtifactKind::GeoIp,
                status: UpdateStatus::Failed {
                    reason: error.to_string(),
                },
            },
        }
    }

    async fn update_geosite(&self, country: &CountryCode) -> ArtifactResult {
        match self.update_geosite_inner(country).await {
            Ok(status) => ArtifactResult {
                country: country.clone(),
                kind: ArtifactKind::GeoSite,
                status,
            },
            Err(error) => ArtifactResult {
                country: country.clone(),
                kind: ArtifactKind::GeoSite,
                status: UpdateStatus::Failed {
                    reason: error.to_string(),
                },
            },
        }
    }

    async fn update_geoip_inner(&self, country: &CountryCode) -> Result<UpdateStatus, GeoError> {
        let expected = self.fetch_geoip_digest(country).await?;
        let path = geoip_cache_path(&self.cache_dir, country);
        if let Ok(existing) = std::fs::read(&path)
            && sha256_digest(&existing) == expected
        {
            GeoIpSet::from_v2ray_dat(&existing, country)?;
            return Ok(UpdateStatus::UpToDate);
        }
        let bytes = self.fetch_geoip_dat(country).await?;
        if sha256_digest(&bytes) != expected {
            return Err(GeoError::ChecksumMismatch);
        }
        GeoIpSet::from_v2ray_dat(&bytes, country)?;
        atomic_write(&path, &bytes)?;
        Ok(UpdateStatus::Updated)
    }

    async fn update_geosite_inner(&self, country: &CountryCode) -> Result<UpdateStatus, GeoError> {
        let text = self.fetch_validated_geosite(country).await?;
        let path = geosite_cache_path(&self.cache_dir, country);
        if let Ok(existing) = std::fs::read(&path)
            && existing == text.as_bytes()
        {
            return Ok(UpdateStatus::UpToDate);
        }
        atomic_write(&path, text.as_bytes())?;
        Ok(UpdateStatus::Updated)
    }

    async fn fetch_verified_geoip(&self, country: &CountryCode) -> Result<Vec<u8>, GeoError> {
        let expected = self.fetch_geoip_digest(country).await?;
        let bytes = self.fetch_geoip_dat(country).await?;
        if sha256_digest(&bytes) != expected {
            return Err(GeoError::ChecksumMismatch);
        }
        GeoIpSet::from_v2ray_dat(&bytes, country)?;
        Ok(bytes)
    }

    async fn fetch_geoip_digest(&self, country: &CountryCode) -> Result<[u8; 32], GeoError> {
        let urls = url_fallbacks(|host| geoip_sha256_url(host, country))?;
        let body = fetch_first_ok(&self.fetch, urls, MAX_CHECKSUM_BYTES).await?;
        let text = std::str::from_utf8(&body).map_err(|_| GeoError::InvalidChecksum)?;
        parse_sha256sum(text)
    }

    async fn fetch_geoip_dat(&self, country: &CountryCode) -> Result<Vec<u8>, GeoError> {
        let urls = url_fallbacks(|host| geoip_dat_url(host, country))?;
        let body = fetch_first_ok(&self.fetch, urls, MAX_GEOIP_BYTES).await?;
        Ok(body.to_vec())
    }

    async fn fetch_validated_geosite(&self, country: &CountryCode) -> Result<String, GeoError> {
        if country.as_str() != "CN" {
            return Err(GeoError::UnsupportedGeoSite(country.clone()));
        }
        let cn = fetch_first_ok(
            &self.fetch,
            url_fallbacks(geosite_cn_url)?,
            MAX_GEOSITE_BYTES,
        )
        .await?;
        let geo = fetch_first_ok(
            &self.fetch,
            url_fallbacks(geosite_geolocation_cn_url)?,
            MAX_GEOSITE_BYTES,
        )
        .await?;
        let mut combined = Vec::new();
        crate::fetch::extend_capped(&mut combined, &cn, MAX_GEOSITE_BYTES)?;
        crate::fetch::extend_capped(&mut combined, b"\n", MAX_GEOSITE_BYTES)?;
        crate::fetch::extend_capped(&mut combined, &geo, MAX_GEOSITE_BYTES)?;
        let text = String::from_utf8(combined).map_err(|_| GeoError::InvalidGeoSite)?;
        GeoSiteSet::from_text(&text, country)?;
        Ok(text)
    }
}

fn url_fallbacks(
    build: impl Fn(&str) -> Result<String, GeoError>,
) -> Result<Vec<String>, GeoError> {
    ALLOWED_HOSTS.iter().copied().map(build).collect()
}

fn sha256_digest(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

pub(crate) fn parse_sha256sum(text: &str) -> Result<[u8; 32], GeoError> {
    let token = text
        .split_whitespace()
        .next()
        .ok_or(GeoError::InvalidChecksum)?;
    if token.len() != 64 || !token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(GeoError::InvalidChecksum);
    }
    let mut digest = [0_u8; 32];
    for (index, chunk) in token.as_bytes().chunks_exact(2).enumerate() {
        let hex = std::str::from_utf8(chunk).map_err(|_| GeoError::InvalidChecksum)?;
        digest[index] = u8::from_str_radix(hex, 16).map_err(|_| GeoError::InvalidChecksum)?;
    }
    Ok(digest)
}

#[cfg(test)]
mod tests {
    use super::parse_sha256sum;
    use crate::error::GeoError;

    #[test]
    fn parses_gnu_sha256sum_lines() {
        let digest = parse_sha256sum(
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  cn.dat\n",
        )
        .unwrap();
        assert_eq!(
            digest,
            [
                0xe3, 0xb0, 0xc4, 0x42, 0x98, 0xfc, 0x1c, 0x14, 0x9a, 0xfb, 0xf4, 0xc8, 0x99, 0x6f,
                0xb9, 0x24, 0x27, 0xae, 0x41, 0xe4, 0x64, 0x9b, 0x93, 0x4c, 0xa4, 0x95, 0x99, 0x1b,
                0x78, 0x52, 0xb8, 0x55
            ]
        );
    }

    #[test]
    fn rejects_truncated_checksums() {
        assert!(matches!(
            parse_sha256sum("deadbeef"),
            Err(GeoError::InvalidChecksum)
        ));
    }
}
