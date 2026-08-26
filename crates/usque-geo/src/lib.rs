//! GeoIP and GeoSite classifiers plus a jsDelivr-only rule downloader.
//!
//! Unknown, truncated, or unverified data is never a DIRECT hit.
//! CN GeoSite downloads use v2fly's flattened release list, while any unknown
//! rule type remains fail-closed.

mod cache;
mod country;
mod error;
mod fetch;
mod geoip;
mod geosite;
mod proto;
mod update;

pub use cache::{
    CachedCountry, GeoClassifier, geoip_cache_path, geosite_cache_path, list_cached_countries,
};
pub use country::CountryCode;
pub use error::{ArtifactKind, GeoError};
pub use fetch::{
    ALLOWED_HOSTS, FetchedBody, HttpFetch, MAX_GEOIP_BYTES, MAX_GEOSITE_BYTES, ReqwestFetch,
};
pub use geoip::GeoIpSet;
pub use geosite::GeoSiteSet;
pub use update::{ArtifactResult, GeoDownloader, UpdateStatus};
