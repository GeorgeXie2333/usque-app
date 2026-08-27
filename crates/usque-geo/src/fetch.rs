use std::future::Future;
use std::time::Duration;

use bytes::Bytes;
use futures::StreamExt;
use reqwest::Client;
use reqwest::redirect::{Attempt, Policy};

use crate::country::CountryCode;
use crate::error::GeoError;

pub const ALLOWED_HOSTS: [&str; 3] = [
    "cdn.jsdelivr.net",
    "testingcf.jsdelivr.net",
    "fastly.jsdelivr.net",
];

const GEOIP_PATH_PREFIX: &str = "/gh/v2fly/geoip@";
const GEOSITE_PATH_PREFIX: &str = "/gh/v2fly/domain-list-community@";

pub const MAX_GEOIP_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_GEOSITE_BYTES: usize = 16 * 1024 * 1024;
pub(crate) const MAX_CHECKSUM_BYTES: usize = 8 * 1024;

#[derive(Debug, Clone)]
pub struct FetchedBody {
    pub status: u16,
    pub url: String,
    pub body: Bytes,
}

pub trait HttpFetch: Send + Sync {
    fn get_capped(
        &self,
        url: &str,
        max_bytes: usize,
    ) -> impl Future<Output = Result<FetchedBody, GeoError>> + Send;
}

#[derive(Clone)]
pub struct ReqwestFetch {
    client: Client,
}

impl ReqwestFetch {
    pub fn new() -> Result<Self, GeoError> {
        Self::from_builder(
            Client::builder()
                .connect_timeout(Duration::from_secs(8))
                .timeout(Duration::from_secs(30)),
        )
    }

    pub fn from_builder(builder: reqwest::ClientBuilder) -> Result<Self, GeoError> {
        Ok(Self {
            client: builder.redirect(jsdelivr_redirect_policy()).build()?,
        })
    }
}

impl HttpFetch for ReqwestFetch {
    async fn get_capped(&self, url: &str, max_bytes: usize) -> Result<FetchedBody, GeoError> {
        let parsed = parse_allowed_url(url)?;
        let response = self.client.get(parsed).send().await?;
        let final_url = response.url().to_string();
        parse_allowed_url(&final_url)?;
        let status = response.status().as_u16();
        if response
            .content_length()
            .is_some_and(|length| length > max_bytes as u64)
        {
            return Err(GeoError::PayloadTooLarge(max_bytes));
        }
        let mut body = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            extend_capped(&mut body, &chunk?, max_bytes)?;
        }
        Ok(FetchedBody {
            status,
            url: final_url,
            body: Bytes::from(body),
        })
    }
}

pub(crate) fn jsdelivr_redirect_policy() -> Policy {
    Policy::custom(|attempt: Attempt| {
        let next = attempt.url().clone();
        if attempt.previous().len() >= 4 {
            return attempt.error(GeoError::DisallowedUrl(next.to_string()));
        }
        match parse_allowed_url(next.as_str()) {
            Ok(_) => attempt.follow(),
            Err(error) => attempt.error(error),
        }
    })
}

pub(crate) fn parse_allowed_url(url: &str) -> Result<reqwest::Url, GeoError> {
    let parsed = reqwest::Url::parse(url).map_err(|_| GeoError::DisallowedUrl(url.to_owned()))?;
    if parsed.scheme() != "https" {
        return Err(GeoError::DisallowedUrl(url.to_owned()));
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(GeoError::DisallowedUrl(url.to_owned()));
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| GeoError::DisallowedUrl(url.to_owned()))?;
    if !ALLOWED_HOSTS.contains(&host) {
        return Err(GeoError::DisallowedHost(host.to_owned()));
    }
    if parsed
        .path()
        .split('/')
        .any(|segment| segment == ".." || segment.eq_ignore_ascii_case("%2e%2e"))
    {
        return Err(GeoError::DisallowedUrl(url.to_owned()));
    }
    if !jsdelivr_path_allowed(parsed.path()) {
        return Err(GeoError::DisallowedUrl(url.to_owned()));
    }
    Ok(parsed)
}

fn jsdelivr_path_allowed(path: &str) -> bool {
    path.starts_with(GEOIP_PATH_PREFIX) || path.starts_with(GEOSITE_PATH_PREFIX)
}

pub(crate) fn geoip_release_object(country: &CountryCode) -> String {
    let cc = country.as_lower();
    if cc == "cn" {
        format!("{cc}.dat")
    } else {
        format!("dat/{cc}.dat")
    }
}

pub(crate) fn geoip_dat_url(host: &str, country: &CountryCode) -> Result<String, GeoError> {
    allowlisted_host(host)?;
    Ok(format!(
        "https://{host}/gh/v2fly/geoip@release/{}",
        geoip_release_object(country)
    ))
}

pub(crate) fn geoip_sha256_url(host: &str, country: &CountryCode) -> Result<String, GeoError> {
    allowlisted_host(host)?;
    Ok(format!(
        "https://{host}/gh/v2fly/geoip@release/{}.sha256sum",
        geoip_release_object(country)
    ))
}

pub(crate) fn geosite_dat_url(host: &str) -> Result<String, GeoError> {
    allowlisted_host(host)?;
    Ok(format!(
        "https://{host}/gh/v2fly/domain-list-community@release/dlc.dat"
    ))
}

pub(crate) fn geosite_sha256_url(host: &str) -> Result<String, GeoError> {
    allowlisted_host(host)?;
    Ok(format!(
        "https://{host}/gh/v2fly/domain-list-community@release/dlc.dat.sha256sum"
    ))
}

fn allowlisted_host(host: &str) -> Result<(), GeoError> {
    if ALLOWED_HOSTS.contains(&host) {
        Ok(())
    } else {
        Err(GeoError::DisallowedHost(host.to_owned()))
    }
}

pub(crate) fn extend_capped(
    body: &mut Vec<u8>,
    chunk: &[u8],
    max_bytes: usize,
) -> Result<(), GeoError> {
    if body.len().saturating_add(chunk.len()) > max_bytes {
        return Err(GeoError::PayloadTooLarge(max_bytes));
    }
    body.extend_from_slice(chunk);
    Ok(())
}

pub(crate) async fn fetch_first_ok<F: HttpFetch>(
    fetch: &F,
    urls: impl IntoIterator<Item = String>,
    max_bytes: usize,
) -> Result<Bytes, GeoError> {
    let mut last_error = None;
    for url in urls {
        match try_fetch_ok(fetch, &url, max_bytes).await {
            Ok(body) => return Ok(body),
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.unwrap_or(GeoError::HttpStatus(404)))
}

async fn try_fetch_ok<F: HttpFetch>(
    fetch: &F,
    url: &str,
    max_bytes: usize,
) -> Result<Bytes, GeoError> {
    parse_allowed_url(url)?;
    let response = fetch.get_capped(url, max_bytes).await?;
    parse_allowed_url(&response.url)?;
    if response.status != 200 {
        return Err(GeoError::HttpStatus(response.status));
    }
    Ok(response.body)
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;

    use super::{
        ALLOWED_HOSTS, FetchedBody, HttpFetch, MAX_GEOIP_BYTES, extend_capped, fetch_first_ok,
        geoip_dat_url, parse_allowed_url,
    };
    use crate::country::CountryCode;
    use crate::error::GeoError;

    #[test]
    fn rejects_disallowed_hosts_and_http() {
        assert!(matches!(
            parse_allowed_url("https://evil.example/gh/v2fly/geoip@release/cn.dat"),
            Err(GeoError::DisallowedHost(_))
        ));
        assert!(matches!(
            parse_allowed_url("http://cdn.jsdelivr.net/gh/v2fly/geoip@release/cn.dat"),
            Err(GeoError::DisallowedUrl(_))
        ));
        assert!(
            parse_allowed_url("https://cdn.jsdelivr.net/gh/v2fly/geoip@release/cn.dat").is_ok()
        );
        assert!(
            parse_allowed_url(
                "https://cdn.jsdelivr.net/gh/v2fly/domain-list-community@release/dlc.dat"
            )
            .is_ok()
        );
        assert!(matches!(
            parse_allowed_url("https://cdn.jsdelivr.net/gh/other/repo@release/cn.dat"),
            Err(GeoError::DisallowedUrl(_))
        ));
        assert!(matches!(
            parse_allowed_url("https://cdn.jsdelivr.net/gh/v2fly/other@release/cn.dat"),
            Err(GeoError::DisallowedUrl(_))
        ));
    }

    #[test]
    fn builds_geoip_urls_from_country_code_only() {
        let cn = CountryCode::parse("CN").unwrap();
        let us = CountryCode::parse("US").unwrap();
        assert_eq!(
            geoip_dat_url(ALLOWED_HOSTS[0], &cn).unwrap(),
            "https://cdn.jsdelivr.net/gh/v2fly/geoip@release/cn.dat"
        );
        assert_eq!(
            geoip_dat_url(ALLOWED_HOSTS[0], &us).unwrap(),
            "https://cdn.jsdelivr.net/gh/v2fly/geoip@release/dat/us.dat"
        );
        assert!(geoip_dat_url("evil.example", &cn).is_err());
    }

    #[test]
    fn aborts_when_the_body_exceeds_the_cap() {
        let mut body = vec![0; MAX_GEOIP_BYTES];
        assert!(matches!(
            extend_capped(&mut body, &[0], MAX_GEOIP_BYTES),
            Err(GeoError::PayloadTooLarge(MAX_GEOIP_BYTES))
        ));
    }

    struct PoisonedFetch;

    impl HttpFetch for PoisonedFetch {
        async fn get_capped(&self, _url: &str, _max_bytes: usize) -> Result<FetchedBody, GeoError> {
            Ok(FetchedBody {
                status: 200,
                url: "https://evil.example/stolen".to_owned(),
                body: Bytes::from_static(b"nope"),
            })
        }
    }

    #[tokio::test]
    async fn rejects_redirects_off_allowlisted_hosts() {
        let error = fetch_first_ok(
            &PoisonedFetch,
            [format!(
                "https://{}/gh/v2fly/geoip@release/cn.dat",
                ALLOWED_HOSTS[0]
            )],
            16,
        )
        .await
        .unwrap_err();
        assert!(matches!(error, GeoError::DisallowedHost(_)));
    }

    struct SameHostWrongRepoFetch;

    impl HttpFetch for SameHostWrongRepoFetch {
        async fn get_capped(&self, _url: &str, _max_bytes: usize) -> Result<FetchedBody, GeoError> {
            Ok(FetchedBody {
                status: 200,
                url: format!("https://{}/gh/other/repo@main/cn.dat", ALLOWED_HOSTS[0]),
                body: Bytes::from_static(b"nope"),
            })
        }
    }

    #[tokio::test]
    async fn rejects_same_host_redirects_off_v2fly_path() {
        let error = fetch_first_ok(
            &SameHostWrongRepoFetch,
            [format!(
                "https://{}/gh/v2fly/geoip@release/cn.dat",
                ALLOWED_HOSTS[0]
            )],
            16,
        )
        .await
        .unwrap_err();
        assert!(matches!(error, GeoError::DisallowedUrl(_)));
    }
}
