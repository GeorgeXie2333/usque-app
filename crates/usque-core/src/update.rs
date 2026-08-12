//! GitHub release update discovery.
//!
//! The checker only discovers a newer release and returns its GitHub URL. It
//! never downloads or installs application binaries.

use std::time::Duration;

use reqwest::{
    Client, StatusCode,
    header::{ACCEPT, HeaderMap, HeaderValue, USER_AGENT},
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

const RELEASES_ENDPOINT: &str =
    "https://api.github.com/repos/GeorgeXie2333/usque-app/releases?per_page=20";
const RELEASE_URL_PREFIX: &str = "https://github.com/GeorgeXie2333/usque-app/releases/";
const MAX_RELEASE_RESPONSE_BYTES: u64 = 512 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UpdateInfo {
    pub available: bool,
    pub version: String,
    pub release_url: String,
}

impl UpdateInfo {
    pub fn current() -> Self {
        Self {
            available: false,
            version: String::new(),
            release_url: String::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct UpdateChecker {
    client: Client,
    endpoint: String,
}

impl UpdateChecker {
    pub fn new() -> Result<Self, UpdateError> {
        let mut headers = HeaderMap::new();
        headers.insert(
            USER_AGENT,
            HeaderValue::from_static(concat!(
                "Usque/",
                env!("CARGO_PKG_VERSION"),
                " update-check"
            )),
        );
        headers.insert(
            ACCEPT,
            HeaderValue::from_static("application/vnd.github+json"),
        );
        let client = Client::builder()
            .default_headers(headers)
            .connect_timeout(Duration::from_secs(8))
            .timeout(Duration::from_secs(15))
            .build()?;
        Ok(Self {
            client,
            endpoint: RELEASES_ENDPOINT.to_owned(),
        })
    }

    #[cfg(test)]
    fn with_endpoint(endpoint: String) -> Result<Self, UpdateError> {
        let mut checker = Self::new()?;
        checker.endpoint = endpoint;
        Ok(checker)
    }

    pub async fn check(&self, current_version: &str) -> Result<UpdateInfo, UpdateError> {
        let current = parse_version(current_version)?;
        let response = self.client.get(&self.endpoint).send().await?;
        if response.status() != StatusCode::OK {
            return Err(UpdateError::HttpStatus(response.status()));
        }
        if response.content_length().unwrap_or_default() > MAX_RELEASE_RESPONSE_BYTES {
            return Err(UpdateError::ResponseTooLarge);
        }
        let bytes = response.bytes().await?;
        if bytes.len() as u64 > MAX_RELEASE_RESPONSE_BYTES {
            return Err(UpdateError::ResponseTooLarge);
        }
        let releases: Vec<GitHubRelease> = serde_json::from_slice(&bytes)?;
        Ok(select_newest_release(&current, releases))
    }
}

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    html_url: String,
    draft: bool,
}

fn select_newest_release(current: &ComparableVersion, releases: Vec<GitHubRelease>) -> UpdateInfo {
    releases
        .into_iter()
        .filter(|release| !release.draft && release.html_url.starts_with(RELEASE_URL_PREFIX))
        .filter_map(|release| {
            let version = parse_version(&release.tag_name).ok()?;
            (version > *current).then_some((version, release))
        })
        .max_by(|left, right| left.0.cmp(&right.0))
        .map_or_else(UpdateInfo::current, |(_, release)| UpdateInfo {
            available: true,
            version: release.tag_name,
            release_url: release.html_url,
        })
}

fn parse_version(value: &str) -> Result<ComparableVersion, UpdateError> {
    ComparableVersion::parse(value).ok_or_else(|| UpdateError::InvalidVersion(value.to_owned()))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ComparableVersion {
    major: u64,
    minor: u64,
    patch: u64,
    prerelease: Vec<PrereleaseIdentifier>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PrereleaseIdentifier {
    Numeric(u64),
    Text(String),
}

impl ComparableVersion {
    fn parse(value: &str) -> Option<Self> {
        let value = value.trim().strip_prefix('v').unwrap_or(value.trim());
        let without_build = value.split_once('+').map_or(value, |(version, _)| version);
        let (core, prerelease) = without_build
            .split_once('-')
            .map_or((without_build, ""), |(core, prerelease)| (core, prerelease));
        let mut core = core.split('.');
        let major = parse_core_number(core.next()?)?;
        let minor = parse_core_number(core.next()?)?;
        let patch = parse_core_number(core.next()?)?;
        if core.next().is_some() {
            return None;
        }
        let prerelease = if prerelease.is_empty() {
            Vec::new()
        } else {
            prerelease
                .split('.')
                .map(|identifier| {
                    if identifier.is_empty()
                        || !identifier
                            .chars()
                            .all(|character| character.is_ascii_alphanumeric() || character == '-')
                    {
                        return None;
                    }
                    if identifier
                        .chars()
                        .all(|character| character.is_ascii_digit())
                    {
                        if identifier.len() > 1 && identifier.starts_with('0') {
                            return None;
                        }
                        Some(PrereleaseIdentifier::Numeric(identifier.parse().ok()?))
                    } else {
                        Some(PrereleaseIdentifier::Text(identifier.to_owned()))
                    }
                })
                .collect::<Option<Vec<_>>>()?
        };
        Some(Self {
            major,
            minor,
            patch,
            prerelease,
        })
    }
}

fn parse_core_number(value: &str) -> Option<u64> {
    if value.is_empty()
        || !value.chars().all(|character| character.is_ascii_digit())
        || (value.len() > 1 && value.starts_with('0'))
    {
        return None;
    }
    value.parse().ok()
}

impl PartialOrd for ComparableVersion {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ComparableVersion {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.major
            .cmp(&other.major)
            .then_with(|| self.minor.cmp(&other.minor))
            .then_with(|| self.patch.cmp(&other.patch))
            .then_with(
                || match (self.prerelease.is_empty(), other.prerelease.is_empty()) {
                    (true, true) | (false, false) => {
                        compare_prerelease(&self.prerelease, &other.prerelease)
                    }
                    (true, false) => std::cmp::Ordering::Greater,
                    (false, true) => std::cmp::Ordering::Less,
                },
            )
    }
}

fn compare_prerelease(
    left: &[PrereleaseIdentifier],
    right: &[PrereleaseIdentifier],
) -> std::cmp::Ordering {
    for (left, right) in left.iter().zip(right) {
        let ordering = match (left, right) {
            (PrereleaseIdentifier::Numeric(left), PrereleaseIdentifier::Numeric(right)) => {
                left.cmp(right)
            }
            (PrereleaseIdentifier::Numeric(_), PrereleaseIdentifier::Text(_)) => {
                std::cmp::Ordering::Less
            }
            (PrereleaseIdentifier::Text(_), PrereleaseIdentifier::Numeric(_)) => {
                std::cmp::Ordering::Greater
            }
            (PrereleaseIdentifier::Text(left), PrereleaseIdentifier::Text(right)) => {
                left.cmp(right)
            }
        };
        if ordering != std::cmp::Ordering::Equal {
            return ordering;
        }
    }
    left.len().cmp(&right.len())
}

#[derive(Debug, Error)]
pub enum UpdateError {
    #[error("could not initialize or contact the GitHub update endpoint: {0}")]
    Request(#[from] reqwest::Error),
    #[error("the GitHub update endpoint returned HTTP {0}")]
    HttpStatus(StatusCode),
    #[error("the GitHub update response exceeded 512 KiB")]
    ResponseTooLarge,
    #[error("the GitHub update response was invalid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("an application or release version was not valid SemVer: {0}")]
    InvalidVersion(String),
}

#[cfg(test)]
mod tests {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::*;

    #[test]
    fn selects_newest_non_draft_release() {
        let releases = vec![
            GitHubRelease {
                tag_name: "v0.1.0-beta.2".to_owned(),
                html_url: "https://github.com/GeorgeXie2333/usque-app/releases/tag/v0.1.0-beta.2"
                    .to_owned(),
                draft: false,
            },
            GitHubRelease {
                tag_name: "v9.0.0".to_owned(),
                html_url: "https://attacker.invalid/release".to_owned(),
                draft: false,
            },
            GitHubRelease {
                tag_name: "v1.0.0".to_owned(),
                html_url: "https://github.com/GeorgeXie2333/usque-app/releases/tag/v1.0.0"
                    .to_owned(),
                draft: true,
            },
        ];
        let selected = select_newest_release(&parse_version("0.1.0-beta.1").unwrap(), releases);
        assert!(selected.available);
        assert_eq!(selected.version, "v0.1.0-beta.2");
    }

    #[test]
    fn ignores_current_and_older_versions() {
        let releases = vec![GitHubRelease {
            tag_name: "v0.1.0-beta.1".to_owned(),
            html_url: "https://github.com/GeorgeXie2333/usque-app/releases/tag/v0.1.0-beta.1"
                .to_owned(),
            draft: false,
        }];
        assert_eq!(
            select_newest_release(&parse_version("0.1.0-beta.1").unwrap(), releases),
            UpdateInfo::current()
        );
    }

    #[test]
    fn semver_prerelease_order_matches_the_release_contract() {
        let beta_1 = parse_version("v0.1.0-beta.1").unwrap();
        let beta_2 = parse_version("0.1.0-beta.2+build.9").unwrap();
        let stable = parse_version("0.1.0").unwrap();
        assert!(beta_1 < beta_2);
        assert!(beta_2 < stable);
        assert!(parse_version("0.01.0").is_err());
        assert!(parse_version("0.1").is_err());
    }

    #[tokio::test]
    async fn rejects_oversized_responses_before_json_parsing() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/releases", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0_u8; 2048];
            let _ = stream.read(&mut request).await.unwrap();
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 524289\r\nConnection: close\r\n\r\n",
                )
                .await
                .unwrap();
        });
        let checker = UpdateChecker::with_endpoint(endpoint).unwrap();
        assert!(matches!(
            checker.check("0.1.0-beta.1").await,
            Err(UpdateError::ResponseTooLarge)
        ));
        server.await.unwrap();
    }
}
