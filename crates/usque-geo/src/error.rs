use std::io;
use std::path::PathBuf;

use thiserror::Error;

use crate::country::CountryCode;

#[derive(Debug, Error)]
pub enum GeoError {
    #[error("invalid country code {0:?}")]
    InvalidCountryCode(String),
    #[error("truncated or invalid GeoIP data")]
    InvalidGeoIp,
    #[error("truncated or invalid GeoSite data")]
    InvalidGeoSite,
    #[error("payload exceeds the {0} byte safety limit")]
    PayloadTooLarge(usize),
    #[error("host is not an allowed jsDelivr CDN: {0}")]
    DisallowedHost(String),
    #[error("refusing non-HTTPS geo URL: {0}")]
    DisallowedUrl(String),
    #[error("HTTP status {0}")]
    HttpStatus(u16),
    #[error("HTTP request failed: {0}")]
    Http(String),
    #[error("checksum mismatch")]
    ChecksumMismatch,
    #[error("invalid checksum file")]
    InvalidChecksum,
    #[error("missing cached {kind} artifact for {country}")]
    MissingArtifact {
        country: CountryCode,
        kind: ArtifactKind,
    },
    #[error("GeoSite lists are only published for CN, not {0}")]
    UnsupportedGeoSite(CountryCode),
    #[error("cache path is missing a parent directory: {0}")]
    MissingParent(PathBuf),
    #[error(transparent)]
    Io(#[from] io::Error),
}

impl From<reqwest::Error> for GeoError {
    fn from(error: reqwest::Error) -> Self {
        Self::Http(error.to_string())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactKind {
    GeoIp,
    GeoSite,
}

impl std::fmt::Display for ArtifactKind {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::GeoIp => formatter.write_str("geoip"),
            Self::GeoSite => formatter.write_str("geosite"),
        }
    }
}
