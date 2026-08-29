//! GitHub release update discovery.
//!
//! Discovery is deliberately separate from download and installation. A
//! release package is exposed only after its GitHub asset metadata and the
//! release manifest agree on its exact name, platform, variant, size, and
//! SHA-256 digest.

use std::time::Duration;

use futures::StreamExt;
use reqwest::{
    Client, StatusCode,
    header::{ACCEPT, HeaderMap, HeaderValue, USER_AGENT},
    redirect::Policy,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

const RELEASES_ENDPOINT: &str =
    "https://api.github.com/repos/GeorgeXie2333/usque-app/releases?per_page=20";
const RELEASE_URL_PREFIX: &str = "https://github.com/GeorgeXie2333/usque-app/releases/";
const RELEASE_DOWNLOAD_PREFIX: &str =
    "https://github.com/GeorgeXie2333/usque-app/releases/download/";
const MAX_RELEASE_RESPONSE_BYTES: u64 = 512 * 1024;
const MAX_RELEASE_MANIFEST_BYTES: u64 = 64 * 1024;
const MAX_UPDATE_PACKAGE_BYTES: u64 = 512 * 1024 * 1024;
const RELEASE_MANIFEST_NAME: &str = "release-manifest.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UpdateInfo {
    pub available: bool,
    pub version: String,
    pub release_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub package: Option<UpdatePackage>,
}

impl UpdateInfo {
    pub fn current() -> Self {
        Self {
            available: false,
            version: String::new(),
            release_url: String::new(),
            package: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UpdatePackage {
    pub name: String,
    pub download_url: String,
    pub size: u64,
    pub sha256: String,
    pub platform: String,
    pub variant: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct UpdateTarget {
    platform: &'static str,
    variant: &'static str,
    extension: &'static str,
}

fn current_update_target() -> Option<UpdateTarget> {
    if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        return Some(UpdateTarget {
            platform: "windows",
            variant: "x64-v2",
            extension: "msi",
        });
    }
    if cfg!(all(target_os = "windows", target_arch = "aarch64")) {
        return Some(UpdateTarget {
            platform: "windows",
            variant: "arm64",
            extension: "msi",
        });
    }
    if cfg!(all(target_os = "android", target_arch = "aarch64")) {
        return Some(UpdateTarget {
            platform: "android",
            variant: "arm64-v8a",
            extension: "apk",
        });
    }
    if cfg!(all(target_os = "android", target_arch = "x86_64")) {
        return Some(UpdateTarget {
            platform: "android",
            variant: "x86_64",
            extension: "apk",
        });
    }
    if cfg!(all(target_os = "android", target_arch = "arm")) {
        return Some(UpdateTarget {
            platform: "android",
            variant: "armeabi-v7a",
            extension: "apk",
        });
    }
    None
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
            .redirect(Policy::custom(|attempt| {
                if attempt.previous().len() >= 5 {
                    return attempt.error("the update request exceeded the redirect limit");
                }
                if approved_github_url(attempt.url()) {
                    attempt.follow()
                } else {
                    attempt.error("the update request redirected outside approved GitHub hosts")
                }
            }))
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
        let bytes = fetch_bounded(&self.client, &self.endpoint, MAX_RELEASE_RESPONSE_BYTES).await?;
        let releases: Vec<GitHubRelease> = serde_json::from_slice(&bytes)?;
        let Some((_, release)) = select_newest_release(&current, releases) else {
            return Ok(UpdateInfo::current());
        };
        let package = if let Some(target) = current_update_target() {
            resolve_release_package(&self.client, &release, target)
                .await
                .ok()
        } else {
            None
        };
        Ok(UpdateInfo {
            available: true,
            version: release.tag_name,
            release_url: release.html_url,
            package,
        })
    }
}

fn approved_github_url(url: &reqwest::Url) -> bool {
    if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some() {
        return false;
    }
    let Some(host) = url.host_str() else {
        return false;
    };
    matches!(
        host.to_ascii_lowercase().as_str(),
        "api.github.com"
            | "github.com"
            | "release-assets.githubusercontent.com"
            | "objects.githubusercontent.com"
    ) || host
        .to_ascii_lowercase()
        .ends_with(".githubusercontent.com")
}

async fn fetch_bounded(client: &Client, url: &str, maximum: u64) -> Result<Vec<u8>, UpdateError> {
    let response = client.get(url).send().await?;
    if response.status() != StatusCode::OK {
        return Err(UpdateError::HttpStatus(response.status()));
    }
    let content_length = response.content_length();
    if content_length.unwrap_or_default() > maximum {
        return Err(UpdateError::ResponseTooLarge);
    }
    let capacity = usize::try_from(content_length.unwrap_or_default().min(maximum))
        .map_err(|_| UpdateError::ResponseTooLarge)?;
    let mut bytes = Vec::with_capacity(capacity);
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        let remaining = maximum.saturating_sub(bytes.len() as u64);
        if chunk.len() as u64 > remaining {
            return Err(UpdateError::ResponseTooLarge);
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    html_url: String,
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<GitHubAsset>,
}

#[derive(Debug, Clone, Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
    size: u64,
    #[serde(default)]
    digest: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ReleaseManifest {
    schema_version: u32,
    tag: String,
    artifacts: Vec<ManifestArtifact>,
}

#[derive(Debug, Deserialize)]
struct ManifestArtifact {
    name: String,
    platform: String,
    variant: String,
    sha256: String,
    size: u64,
}

fn select_newest_release(
    current: &ComparableVersion,
    releases: Vec<GitHubRelease>,
) -> Option<(ComparableVersion, GitHubRelease)> {
    releases
        .into_iter()
        .filter(|release| {
            !release.draft
                && !release.prerelease
                && release.html_url.starts_with(RELEASE_URL_PREFIX)
        })
        .filter_map(|release| {
            let version = parse_version(&release.tag_name).ok()?;
            (version.prerelease.is_empty() && version > *current).then_some((version, release))
        })
        .max_by(|left, right| left.0.cmp(&right.0))
}

async fn resolve_release_package(
    client: &Client,
    release: &GitHubRelease,
    target: UpdateTarget,
) -> Result<UpdatePackage, UpdateError> {
    let expected_name = format!(
        "usque-{}-{}-{}.{}",
        release.tag_name, target.platform, target.variant, target.extension
    );
    let package_asset = unique_asset(&release.assets, &expected_name)?;
    validate_asset(package_asset, &release.tag_name, &expected_name)?;
    if package_asset.size == 0 || package_asset.size > MAX_UPDATE_PACKAGE_BYTES {
        return Err(UpdateError::InvalidManifest(
            "the selected update package has an invalid size".to_owned(),
        ));
    }

    let manifest_asset = unique_asset(&release.assets, RELEASE_MANIFEST_NAME)?;
    validate_asset(manifest_asset, &release.tag_name, RELEASE_MANIFEST_NAME)?;
    if manifest_asset.size == 0 || manifest_asset.size > MAX_RELEASE_MANIFEST_BYTES {
        return Err(UpdateError::InvalidManifest(
            "the release manifest has an invalid size".to_owned(),
        ));
    }
    let manifest_bytes = fetch_bounded(
        client,
        &manifest_asset.browser_download_url,
        MAX_RELEASE_MANIFEST_BYTES,
    )
    .await?;
    if manifest_bytes.len() as u64 != manifest_asset.size {
        return Err(UpdateError::InvalidManifest(
            "the release manifest size did not match GitHub metadata".to_owned(),
        ));
    }
    if let Some(digest) = &manifest_asset.digest {
        validate_github_digest(digest, &sha256_hex(&manifest_bytes))?;
    }
    let manifest: ReleaseManifest = serde_json::from_slice(&manifest_bytes)?;
    if manifest.schema_version != 1 || manifest.tag != release.tag_name {
        return Err(UpdateError::InvalidManifest(
            "the release manifest schema or tag did not match the release".to_owned(),
        ));
    }
    package_from_manifest(target, &expected_name, package_asset, &manifest)
}

fn package_from_manifest(
    target: UpdateTarget,
    expected_name: &str,
    package_asset: &GitHubAsset,
    manifest: &ReleaseManifest,
) -> Result<UpdatePackage, UpdateError> {
    let matches = manifest
        .artifacts
        .iter()
        .filter(|artifact| artifact.name == expected_name)
        .collect::<Vec<_>>();
    let [artifact] = matches.as_slice() else {
        return Err(UpdateError::InvalidManifest(
            "the release manifest did not contain one selected package".to_owned(),
        ));
    };
    if artifact.platform != target.platform
        || artifact.variant != target.variant
        || artifact.size != package_asset.size
        || artifact.size == 0
        || artifact.size > MAX_UPDATE_PACKAGE_BYTES
        || !valid_sha256(&artifact.sha256)
    {
        return Err(UpdateError::InvalidManifest(
            "the selected package metadata did not match the release manifest".to_owned(),
        ));
    }
    if let Some(digest) = &package_asset.digest {
        validate_github_digest(digest, &artifact.sha256)?;
    }
    Ok(UpdatePackage {
        name: expected_name.to_owned(),
        download_url: package_asset.browser_download_url.clone(),
        size: artifact.size,
        sha256: artifact.sha256.to_ascii_lowercase(),
        platform: target.platform.to_owned(),
        variant: target.variant.to_owned(),
    })
}

fn unique_asset<'a>(assets: &'a [GitHubAsset], name: &str) -> Result<&'a GitHubAsset, UpdateError> {
    let matches = assets
        .iter()
        .filter(|asset| asset.name == name)
        .collect::<Vec<_>>();
    let [asset] = matches.as_slice() else {
        return Err(UpdateError::InvalidManifest(format!(
            "the release did not contain one {name} asset"
        )));
    };
    Ok(asset)
}

fn validate_asset(asset: &GitHubAsset, tag: &str, name: &str) -> Result<(), UpdateError> {
    let expected_url = format!("{RELEASE_DOWNLOAD_PREFIX}{tag}/{name}");
    if asset.browser_download_url != expected_url {
        return Err(UpdateError::InvalidManifest(format!(
            "the {name} download URL was not the expected repository release URL"
        )));
    }
    Ok(())
}

fn validate_github_digest(value: &str, expected: &str) -> Result<(), UpdateError> {
    if value != format!("sha256:{}", expected.to_ascii_lowercase()) {
        return Err(UpdateError::InvalidManifest(
            "GitHub's asset digest did not match the release manifest".to_owned(),
        ));
    }
    Ok(())
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};

    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
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
    #[error("the GitHub release manifest was invalid: {0}")]
    InvalidManifest(String),
    #[error("an application or release version was not valid SemVer: {0}")]
    InvalidVersion(String),
}

#[cfg(test)]
mod tests {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::*;

    #[test]
    fn selects_newest_stable_release() {
        let releases = vec![
            GitHubRelease {
                tag_name: "v0.1.0-beta.2".to_owned(),
                html_url: "https://github.com/GeorgeXie2333/usque-app/releases/tag/v0.1.0-beta.2"
                    .to_owned(),
                draft: false,
                prerelease: true,
                assets: Vec::new(),
            },
            GitHubRelease {
                tag_name: "v9.0.0".to_owned(),
                html_url: "https://attacker.invalid/release".to_owned(),
                draft: false,
                prerelease: false,
                assets: Vec::new(),
            },
            GitHubRelease {
                tag_name: "v1.0.0".to_owned(),
                html_url: "https://github.com/GeorgeXie2333/usque-app/releases/tag/v1.0.0"
                    .to_owned(),
                draft: true,
                prerelease: false,
                assets: Vec::new(),
            },
            GitHubRelease {
                tag_name: "v0.2.0".to_owned(),
                html_url: "https://github.com/GeorgeXie2333/usque-app/releases/tag/v0.2.0"
                    .to_owned(),
                draft: false,
                prerelease: false,
                assets: Vec::new(),
            },
        ];
        let selected = select_newest_release(&parse_version("0.1.0-beta.1").unwrap(), releases);
        assert_eq!(selected.unwrap().1.tag_name, "v0.2.0");
    }

    #[test]
    fn ignores_current_and_older_versions() {
        let releases = vec![GitHubRelease {
            tag_name: "v0.1.0-beta.1".to_owned(),
            html_url: "https://github.com/GeorgeXie2333/usque-app/releases/tag/v0.1.0-beta.1"
                .to_owned(),
            draft: false,
            prerelease: true,
            assets: Vec::new(),
        }];
        assert!(select_newest_release(&parse_version("0.1.0-beta.1").unwrap(), releases).is_none());
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

    #[test]
    fn package_manifest_must_match_size_digest_platform_and_variant() {
        let target = UpdateTarget {
            platform: "windows",
            variant: "x64-v2",
            extension: "msi",
        };
        let name = "usque-v0.2.2-windows-x64-v2.msi";
        let digest = "a5".repeat(32);
        let asset = GitHubAsset {
            name: name.to_owned(),
            browser_download_url: format!("{RELEASE_DOWNLOAD_PREFIX}v0.2.2/{name}"),
            size: 4096,
            digest: Some(format!("sha256:{digest}")),
        };
        let manifest = ReleaseManifest {
            schema_version: 1,
            tag: "v0.2.2".to_owned(),
            artifacts: vec![ManifestArtifact {
                name: name.to_owned(),
                platform: "windows".to_owned(),
                variant: "x64-v2".to_owned(),
                sha256: digest.clone(),
                size: 4096,
            }],
        };
        let package = package_from_manifest(target, name, &asset, &manifest).unwrap();
        assert_eq!(package.variant, "x64-v2");
        assert_eq!(package.sha256, digest);

        let mut wrong_size = manifest;
        wrong_size.artifacts[0].size = 4095;
        assert!(package_from_manifest(target, name, &asset, &wrong_size).is_err());
        wrong_size.artifacts[0].size = 4096;
        wrong_size.artifacts[0].variant = "arm64".to_owned();
        assert!(package_from_manifest(target, name, &asset, &wrong_size).is_err());
    }

    #[test]
    fn asset_selection_rejects_wrong_domains_duplicates_and_universal_fallbacks() {
        let selected = "usque-v0.2.2-android-arm64-v8a.apk";
        let universal = GitHubAsset {
            name: "usque-v0.2.2-android-universal.apk".to_owned(),
            browser_download_url: format!(
                "{RELEASE_DOWNLOAD_PREFIX}v0.2.2/usque-v0.2.2-android-universal.apk"
            ),
            size: 1,
            digest: None,
        };
        assert!(unique_asset(&[universal], selected).is_err());

        let attacker = GitHubAsset {
            name: selected.to_owned(),
            browser_download_url: format!("https://attacker.invalid/{selected}"),
            size: 1,
            digest: None,
        };
        assert!(validate_asset(&attacker, "v0.2.2", selected).is_err());

        let duplicate = GitHubAsset {
            name: selected.to_owned(),
            browser_download_url: format!("{RELEASE_DOWNLOAD_PREFIX}v0.2.2/{selected}"),
            size: 1,
            digest: None,
        };
        assert!(unique_asset(&[duplicate.clone(), duplicate], selected).is_err());
    }

    #[test]
    fn redirects_are_limited_to_https_github_release_hosts() {
        assert!(approved_github_url(
            &reqwest::Url::parse("https://release-assets.githubusercontent.com/object").unwrap()
        ));
        assert!(approved_github_url(
            &reqwest::Url::parse("https://api.github.com/repos/example/releases").unwrap()
        ));
        assert!(!approved_github_url(
            &reqwest::Url::parse("http://github.com/release").unwrap()
        ));
        assert!(!approved_github_url(
            &reqwest::Url::parse("https://github.com.attacker.invalid/release").unwrap()
        ));
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

    #[tokio::test]
    async fn rejects_streamed_responses_without_content_length_at_the_limit() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/releases", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0_u8; 2048];
            let _ = stream.read(&mut request).await.unwrap();
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n")
                .await
                .unwrap();
            let oversized = vec![b'x'; MAX_RELEASE_RESPONSE_BYTES as usize + 1];
            let _ = stream.write_all(&oversized).await;
        });
        let checker = UpdateChecker::with_endpoint(endpoint).unwrap();
        assert!(matches!(
            checker.check("0.1.0-beta.1").await,
            Err(UpdateError::ResponseTooLarge)
        ));
        server.await.unwrap();
    }
}
