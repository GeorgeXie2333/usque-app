use std::collections::HashSet;

use prost::Message;

use crate::country::CountryCode;
use crate::error::GeoError;
use crate::proto::{DOMAIN_DOMAIN, DOMAIN_FULL, DOMAIN_PLAIN, DOMAIN_REGEX, GeoSiteList};

/// Suffix, exact, and keyword matcher for one country.
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
        Ok(set)
    }

    pub fn from_text_bytes(bytes: &[u8], country: &CountryCode) -> Result<Self, GeoError> {
        let text = std::str::from_utf8(bytes).map_err(|_| GeoError::InvalidGeoSite)?;
        Self::from_text(text, country)
    }

    pub fn from_v2ray_dat(bytes: &[u8], country: &CountryCode) -> Result<Self, GeoError> {
        let list = GeoSiteList::decode(bytes).map_err(|_| GeoError::InvalidGeoSite)?;
        let mut set = Self::empty(country.clone());
        let mut found = false;
        for entry in list.entry {
            if !entry_matches_country(&entry.country_code, country) {
                continue;
            }
            found = true;
            for domain in entry.domain {
                set.push_protobuf_domain(domain.r#type, &domain.value);
            }
        }
        if !found {
            return Err(GeoError::InvalidGeoSite);
        }
        Ok(set)
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
            self.push_suffix(&token);
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
    if CountryCode::parse(entry).is_ok_and(|parsed| parsed == *country) {
        return true;
    }
    country.as_str() == "CN" && entry.trim().eq_ignore_ascii_case("geolocation-cn")
}

fn normalize_host(host: &str) -> Option<String> {
    let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
    if host.is_empty() || host.contains('/') || host.contains(char::is_whitespace) {
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
             full:baidu.com\n\
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
        assert!(set.contains("baidu.com"));
        assert!(!set.contains("www.baidu.com"));
        assert!(set.contains("service.weixin.qq.com"));
        assert!(set.contains("plain-as-domain.com"));
        assert!(set.contains("a.plain-as-domain.com"));
        assert!(!set.contains("google.com"));
    }

    #[test]
    fn protobuf_extracts_cn_and_geolocation_cn() {
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
            ],
        }
        .encode_to_vec();
        let set = GeoSiteSet::from_v2ray_dat(&bytes, &cn()).unwrap();
        assert!(set.contains("foo.example.cn"));
        assert!(set.contains("baidu.com"));
        assert!(set.contains("weixin.qq.com"));
        assert!(!set.contains("google.com"));
        assert!(!set.contains("not-a-match.example"));
    }

    #[test]
    fn invalid_utf8_fails_closed() {
        assert!(GeoSiteSet::from_text_bytes(&[0xff, 0xfe], &cn()).is_err());
    }
}
