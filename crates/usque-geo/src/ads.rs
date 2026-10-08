//! Strict, bounded matching for the complete GeoSite advertising category.
use std::collections::HashSet;
use std::path::Path;

use prost::Message;
use regex::{RegexSet, RegexSetBuilder};
use sha2::{Digest, Sha256};

use crate::cache::{atomic_write, global_geosite_cache_path};
use crate::proto::{DOMAIN_DOMAIN, DOMAIN_FULL, DOMAIN_PLAIN, DOMAIN_REGEX, GeoSiteList};
use crate::{GeoError, MAX_GEOSITE_BYTES};

const CATEGORY: &str = "category-ads-all";
const MAX_RULES: usize = 100_000;
const MAX_REGEX_BYTES: usize = 1024 * 1024;

#[derive(Clone, Debug)]
pub struct AdsRules {
    exact: HashSet<String>,
    suffixes: HashSet<String>,
    patterns: RegexSet,
    revision: String,
}

impl AdsRules {
    pub fn parse(bytes: &[u8]) -> Result<Self, GeoError> {
        if bytes.len() > MAX_GEOSITE_BYTES {
            return Err(GeoError::InvalidGeoSite);
        }
        let list = GeoSiteList::decode(bytes).map_err(|_| GeoError::InvalidGeoSite)?;
        let entries = list
            .entry
            .into_iter()
            .filter(|entry| entry.country_code.eq_ignore_ascii_case(CATEGORY));
        let mut exact = HashSet::new();
        let mut suffixes = HashSet::new();
        let mut patterns = Vec::new();
        let mut regex_bytes = 0_usize;
        let mut count = 0_usize;
        for entry in entries {
            for domain in entry.domain {
                count += 1;
                if count > MAX_RULES || domain.value.is_empty() {
                    return Err(GeoError::InvalidGeoSite);
                }
                match domain.r#type {
                    DOMAIN_FULL | DOMAIN_DOMAIN => {
                        let value = domain.value.trim_end_matches('.').to_ascii_lowercase();
                        if value.len() > 253
                            || !value.split('.').all(|label| {
                                !label.is_empty()
                                    && label.len() <= 63
                                    && label
                                        .bytes()
                                        .all(|b| b.is_ascii_alphanumeric() || b == b'-')
                            })
                        {
                            return Err(GeoError::InvalidGeoSite);
                        }
                        if domain.r#type == DOMAIN_FULL {
                            exact.insert(value);
                        } else {
                            suffixes.insert(value);
                        }
                    }
                    DOMAIN_PLAIN | DOMAIN_REGEX => {
                        let pattern = if domain.r#type == DOMAIN_PLAIN {
                            regex::escape(&domain.value.to_ascii_lowercase())
                        } else {
                            domain.value
                        };
                        regex_bytes = regex_bytes.saturating_add(pattern.len());
                        if regex_bytes > MAX_REGEX_BYTES {
                            return Err(GeoError::InvalidGeoSite);
                        }
                        patterns.push(pattern);
                    }
                    _ => return Err(GeoError::InvalidGeoSite),
                }
            }
        }
        if count == 0 {
            return Err(GeoError::InvalidGeoSite);
        }
        let patterns = RegexSetBuilder::new(patterns)
            .case_insensitive(true)
            .size_limit(8 * 1024 * 1024)
            .dfa_size_limit(2 * 1024 * 1024)
            .build()
            .map_err(|_| GeoError::InvalidGeoSite)?;
        Ok(Self {
            exact,
            suffixes,
            patterns,
            revision: Sha256::digest(bytes)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
        })
    }

    pub fn contains(&self, host: &str) -> bool {
        if host.is_empty() || host.len() > 253 {
            return false;
        }
        let host = host.trim_end_matches('.').to_ascii_lowercase();
        if self.exact.contains(&host) || self.patterns.is_match(&host) {
            return true;
        }
        let mut tail = host.as_str();
        loop {
            if self.suffixes.contains(tail) {
                return true;
            }
            match tail.split_once('.') {
                Some((_, next)) => tail = next,
                None => return false,
            }
        }
    }

    pub fn revision(&self) -> &str {
        &self.revision
    }

    pub fn load(cache_dir: &Path) -> Result<Self, GeoError> {
        for path in [
            global_geosite_cache_path(cache_dir),
            cache_dir.join("geo/geosite/ads-last-good.dat"),
        ] {
            if let Ok(metadata) = std::fs::metadata(&path)
                && metadata.len() <= MAX_GEOSITE_BYTES as u64
                && let Ok(bytes) = std::fs::read(path)
                && let Ok(rules) = Self::parse(&bytes)
            {
                return Ok(rules);
            }
        }
        Err(GeoError::InvalidGeoSite)
    }
}

/// A new catalog cannot destroy the last complete Ads category. The global
/// country catalog can still update when its publisher omits the Ads category.
pub(crate) fn retain_valid_ads(cache_dir: &Path, bytes: &[u8]) -> Result<(), GeoError> {
    if AdsRules::parse(bytes).is_ok() {
        atomic_write(&cache_dir.join("geo/geosite/ads-last-good.dat"), bytes)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::{Domain, GeoSite};

    fn fixture(rules: &[(i32, &str)]) -> Vec<u8> {
        GeoSiteList {
            entry: vec![GeoSite {
                country_code: CATEGORY.into(),
                domain: rules
                    .iter()
                    .map(|(kind, value)| Domain {
                        r#type: *kind,
                        value: (*value).into(),
                    })
                    .collect(),
            }],
        }
        .encode_to_vec()
    }

    proptest::proptest! {
        #[test]
        fn malformed_catalogues_are_bounded_and_never_panic(bytes in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..4096)) {
            let _ = AdsRules::parse(&bytes);
        }
    }

    #[test]
    fn complete_category_and_boundaries() {
        let rules = AdsRules::parse(&fixture(&[
            (DOMAIN_DOMAIN, "ads.test"),
            (DOMAIN_FULL, "exact.test"),
            (DOMAIN_PLAIN, "tracking"),
            (DOMAIN_REGEX, "^ad[0-9]+\\.test$"),
        ]))
        .unwrap();
        for host in [
            "ads.test",
            "a.ads.test",
            "exact.test",
            "tracking.test",
            "ad123.test",
        ] {
            assert!(rules.contains(host));
        }
        for host in ["notads.test", "a.exact.test", "example.test"] {
            assert!(!rules.contains(host));
        }
        assert!(
            AdsRules::parse(&fixture(&[(DOMAIN_DOMAIN, "good.test"), (99, "unknown")])).is_err()
        );
        assert!(AdsRules::parse(&fixture(&[(DOMAIN_REGEX, "[")])).is_err());
        assert!(AdsRules::parse(&fixture(&[])).is_err());
    }

    #[test]
    fn invalid_update_retains_last_complete_category() {
        let dir = tempfile::tempdir().unwrap();
        let good = fixture(&[(DOMAIN_DOMAIN, "ads.test")]);
        retain_valid_ads(dir.path(), &good).unwrap();
        retain_valid_ads(dir.path(), &fixture(&[(DOMAIN_REGEX, "[")])).unwrap();
        assert!(AdsRules::load(dir.path()).unwrap().contains("ads.test"));
    }
}
