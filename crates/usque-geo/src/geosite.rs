use std::collections::{BTreeMap, HashSet};

use prost::Message;

use crate::country::CountryCode;
use crate::error::GeoError;
use crate::proto::{DOMAIN_DOMAIN, DOMAIN_FULL, DOMAIN_PLAIN, DOMAIN_REGEX, GeoSiteList};

/// Suffix, exact, and keyword matcher for one country.
///
/// `include:` and `regexp:` are skipped (not DIRECT hits). A list with no
/// remaining domain/full/keyword rules is an error, not an empty allow-list.
#[derive(Clone, Debug)]
pub struct GeoSiteSet {
    country: CountryCode,
    full: HashSet<String>,
    suffixes: HashSet<String>,
    keywords: Vec<String>,
}

impl GeoSiteSet {
    fn empty(country: CountryCode) -> Self {
        Self {
            country,
            full: HashSet::new(),
            suffixes: HashSet::new(),
            keywords: Vec::new(),
        }
    }

    pub fn from_text(text: &str, country: &CountryCode) -> Result<Self, GeoError> {
        let mut set = Self::empty(country.clone());
        for line in text.lines() {
            set.push_text_line(line);
        }
        set.finish(country)
    }

    pub fn from_text_bytes(bytes: &[u8], country: &CountryCode) -> Result<Self, GeoError> {
        let text = std::str::from_utf8(bytes).map_err(|_| GeoError::InvalidGeoSite)?;
        Self::from_text(text, country)
    }

    pub fn from_v2ray_dat(bytes: &[u8], country: &CountryCode) -> Result<Self, GeoError> {
        Self::from_v2ray_dat_many(bytes, std::slice::from_ref(country))?
            .remove(country)
            .ok_or_else(|| GeoError::EmptyGeoSite(country.clone()))
    }

    /// Extracts every requested country from one protobuf decode. Each country
    /// is the union of `xx`, `geolocation-xx`, `category-xx`, and `tld-xx`.
    pub fn from_v2ray_dat_many(
        bytes: &[u8],
        countries: &[CountryCode],
    ) -> Result<BTreeMap<CountryCode, Self>, GeoError> {
        let list = GeoSiteList::decode(bytes).map_err(|_| GeoError::InvalidGeoSite)?;
        let mut sets = countries
            .iter()
            .cloned()
            .map(|country| (country.clone(), Self::empty(country)))
            .collect::<BTreeMap<_, _>>();
        for entry in list.entry {
            for (country, set) in &mut sets {
                if entry_matches_country(&entry.country_code, country) {
                    for domain in &entry.domain {
                        set.push_protobuf_domain(domain.r#type, &domain.value);
                    }
                }
            }
        }
        sets.retain(|_, set| set.has_rules());
        Ok(sets)
    }

    /// Validates the global v2fly catalog without selecting a country.
    pub fn validate_v2ray_dat(bytes: &[u8]) -> Result<(), GeoError> {
        let list = GeoSiteList::decode(bytes).map_err(|_| GeoError::InvalidGeoSite)?;
        if list.entry.iter().any(|entry| {
            !entry.country_code.trim().is_empty()
                && entry.domain.iter().any(|domain| {
                    matches!(domain.r#type, DOMAIN_FULL | DOMAIN_DOMAIN | DOMAIN_PLAIN)
                        && normalize_host(&domain.value).is_some()
                })
        }) {
            Ok(())
        } else {
            Err(GeoError::InvalidGeoSite)
        }
    }

    fn finish(self, country: &CountryCode) -> Result<Self, GeoError> {
        if self.has_rules() {
            Ok(self)
        } else {
            Err(GeoError::EmptyGeoSite(country.clone()))
        }
    }

    fn has_rules(&self) -> bool {
        !self.full.is_empty() || !self.suffixes.is_empty() || !self.keywords.is_empty()
    }

    pub fn country(&self) -> &CountryCode {
        &self.country
    }

    pub fn contains(&self, host: &str) -> bool {
        let Some(host) = normalize_host(host) else {
            return false;
        };
        if self.full.contains(&host) {
            return true;
        }
        if suffix_hit(&host, &self.suffixes) {
            return true;
        }
        self.keywords.iter().any(|keyword| host.contains(keyword))
    }

    fn push_text_line(&mut self, line: &str) {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            return;
        }
        let token = line
            .split_whitespace()
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        let token = token.split(":@").next().unwrap_or("").trim();
        if token.is_empty() || token.starts_with("include:") || token.starts_with("regexp:") {
            return;
        }
        if let Some(value) = token.strip_prefix("full:") {
            self.push_full(value);
        } else if let Some(value) = token.strip_prefix("keyword:") {
            self.push_keyword(value);
        } else if let Some(value) = token.strip_prefix("domain:") {
            self.push_suffix(value);
        } else {
            self.push_suffix(token);
        }
    }

    fn push_protobuf_domain(&mut self, kind: i32, value: &str) {
        match kind {
            DOMAIN_REGEX => {}
            DOMAIN_FULL => self.push_full(value),
            DOMAIN_DOMAIN => self.push_suffix(value),
            DOMAIN_PLAIN => self.push_keyword(value),
            _ => {}
        }
    }

    fn push_full(&mut self, value: &str) {
        if let Some(host) = normalize_host(value) {
            self.full.insert(host);
        }
    }

    fn push_suffix(&mut self, value: &str) {
        if let Some(host) = normalize_host(value) {
            self.suffixes.insert(host);
        }
    }

    fn push_keyword(&mut self, value: &str) {
        let value = value.trim().to_ascii_lowercase();
        if !value.is_empty() && !self.keywords.iter().any(|existing| existing == &value) {
            self.keywords.push(value);
        }
    }
}

fn entry_matches_country(entry: &str, country: &CountryCode) -> bool {
    let entry = entry.trim().to_ascii_lowercase();
    let country = country.as_lower();
    entry == country
        || entry == format!("geolocation-{country}")
        || entry == format!("category-{country}")
        || entry == format!("tld-{country}")
}

fn normalize_host(host: &str) -> Option<String> {
    let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
    if host.is_empty() || host.contains(['/', ':']) || host.contains(char::is_whitespace) {
        return None;
    }
    Some(host)
}

fn suffix_hit(host: &str, suffixes: &HashSet<String>) -> bool {
    if suffixes.contains(host) {
        return true;
    }
    let mut rest = host;
    while let Some((_, tail)) = rest.split_once('.') {
        if suffixes.contains(tail) {
            return true;
        }
        rest = tail;
    }
    false
}

#[cfg(test)]
mod tests {
    use prost::Message;

    use super::GeoSiteSet;
    use crate::country::CountryCode;
    use crate::proto::{
        DOMAIN_DOMAIN, DOMAIN_FULL, DOMAIN_PLAIN, DOMAIN_REGEX, Domain, GeoSite, GeoSiteList,
    };

    fn cn() -> CountryCode {
        CountryCode::parse("CN").unwrap()
    }

    #[test]
    fn text_suffix_full_keyword_and_skipped_regexp() {
        let set = GeoSiteSet::from_text(
            "# comment\n\
             domain:example.cn\n\
             domain:tagged.example.cn:@cn\n\
             domain:multi-tag.example.cn:@cn:@ads\n\
             full:baidu.com\n\
             full:tagged-full.test:@cn\n\
             keyword:weixin\n\
             regexp:.*\\.evil\\\\\n\
             include:alibaba\n\
             plain-as-domain.com\n",
            &cn(),
        )
        .unwrap();
        assert!(set.contains("example.cn"));
        assert!(set.contains("foo.example.cn"));
        assert!(!set.contains("example.cn.tld"));
        assert!(set.contains("tagged.example.cn"));
        assert!(set.contains("multi-tag.example.cn"));
        assert!(set.contains("baidu.com"));
        assert!(!set.contains("www.baidu.com"));
        assert!(set.contains("tagged-full.test"));
        assert!(!set.contains("www.tagged-full.test"));
        assert!(set.contains("service.weixin.qq.com"));
        assert!(set.contains("plain-as-domain.com"));
        assert!(set.contains("a.plain-as-domain.com"));
        assert!(!set.contains("google.com"));
    }

    #[test]
    fn protobuf_unions_all_country_candidate_lists() {
        let bytes = GeoSiteList {
            entry: vec![
                GeoSite {
                    country_code: "cn".to_owned(),
                    domain: vec![Domain {
                        r#type: DOMAIN_DOMAIN,
                        value: "example.cn".to_owned(),
                    }],
                },
                GeoSite {
                    country_code: "geolocation-cn".to_owned(),
                    domain: vec![
                        Domain {
                            r#type: DOMAIN_FULL,
                            value: "baidu.com".to_owned(),
                        },
                        Domain {
                            r#type: DOMAIN_PLAIN,
                            value: "weixin".to_owned(),
                        },
                        Domain {
                            r#type: DOMAIN_REGEX,
                            value: ".*".to_owned(),
                        },
                    ],
                },
                GeoSite {
                    country_code: "google".to_owned(),
                    domain: vec![Domain {
                        r#type: DOMAIN_DOMAIN,
                        value: "google.com".to_owned(),
                    }],
                },
                GeoSite {
                    country_code: "category-cn".to_owned(),
                    domain: vec![Domain {
                        r#type: DOMAIN_DOMAIN,
                        value: "category.example".to_owned(),
                    }],
                },
                GeoSite {
                    country_code: "tld-cn".to_owned(),
                    domain: vec![Domain {
                        r#type: DOMAIN_FULL,
                        value: "tld.example".to_owned(),
                    }],
                },
            ],
        }
        .encode_to_vec();
        let set = GeoSiteSet::from_v2ray_dat(&bytes, &cn()).unwrap();
        assert!(set.contains("foo.example.cn"));
        assert!(set.contains("baidu.com"));
        assert!(set.contains("weixin.qq.com"));
        assert!(set.contains("sub.category.example"));
        assert!(set.contains("tld.example"));
        assert!(!set.contains("google.com"));
        assert!(!set.contains("not-a-match.example"));
    }

    #[test]
    fn invalid_utf8_fails_closed() {
        assert!(GeoSiteSet::from_text_bytes(&[0xff, 0xfe], &cn()).is_err());
    }

    #[test]
    fn include_only_or_empty_text_is_not_a_valid_list() {
        assert!(matches!(
            GeoSiteSet::from_text("include:tld-cn\ninclude:geolocation-cn\n# comment\n", &cn()),
            Err(crate::error::GeoError::EmptyGeoSite(_))
        ));
        assert!(matches!(
            GeoSiteSet::from_text("regexp:.*\n", &cn()),
            Err(crate::error::GeoError::EmptyGeoSite(_))
        ));
        assert!(GeoSiteSet::from_text("", &cn()).is_err());
    }
}
