//! Linux's signed package and privileged service payload contracts.
//!
//! Verification consumes the original bytes before parsing. The trust anchor
//! is compiled into the application, never taken from a download or caller.
//! Local tests use a separate identity; an unconfigured release key fails closed.

use std::{collections::BTreeSet, io::Read};

use ring::signature::{ED25519, UnparsedPublicKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

pub const MAX_MANIFEST_BYTES: usize = 64 * 1024;
pub const MAX_PACKAGE_BYTES: u64 = 512 * 1024 * 1024;
pub const PROTOCOL_VERSION: u32 = 3;
pub const INSTALL_FORMATS: [&str; 3] = ["AppImage", "deb", "rpm"];
pub const SERVICE_FILES: [&str; 5] = [
    "usr/lib/usque/usque-agent",
    "usr/lib/usque/usque-engine",
    "usr/lib/usque/usque-update",
    "usr/lib/systemd/system/usque-agent.service",
    "usr/share/polkit-1/actions/io.github.georgexie2333.usque.policy",
];

const RELEASE_KEY: &str = include_str!("../../../packaging/linux/release-public-key.hex");

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub name: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema_version: u32,
    /// `packages` signs three packages; `service` signs fixed privileged files.
    pub kind: String,
    pub version: String,
    pub architecture: String,
    pub protocol_version: u32,
    pub entries: Vec<Entry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstallRecord {
    pub schema_version: u32,
    pub version: String,
    pub protocol_version: u32,
    pub format: String,
    pub architecture: String,
    pub engine_sha256: String,
}

impl InstallRecord {
    pub fn validate(&self) -> Result<(), ReleaseError> {
        if self.schema_version != 1
            || self.protocol_version != PROTOCOL_VERSION
            || !matches!(self.format.as_str(), "deb" | "rpm" | "appimage")
            || !matches!(self.architecture.as_str(), "amd64" | "arm64")
            || decode_hex::<32>(&self.engine_sha256).is_err()
        {
            return Err(ReleaseError::Metadata);
        }
        version(&self.version)?;
        Ok(())
    }

    pub fn require_source(&self, format: &str, architecture: &str) -> Result<(), ReleaseError> {
        self.validate()?;
        if !self.format.eq_ignore_ascii_case(format) || self.architecture != architecture {
            return Err(ReleaseError::MixedInstallation);
        }
        Ok(())
    }
}

pub fn host_architecture() -> Result<&'static str, ReleaseError> {
    match std::env::consts::ARCH {
        "x86_64" => Ok("amd64"),
        "aarch64" => Ok("arm64"),
        _ => Err(ReleaseError::Architecture),
    }
}

/// The source record is maintained only by root. A malformed or unsafe record
/// is an error, never an invitation to overwrite another installation.
#[cfg(target_os = "linux")]
pub fn read_install_record() -> Result<Option<InstallRecord>, ReleaseError> {
    use std::{
        fs::OpenOptions,
        os::unix::fs::{MetadataExt, OpenOptionsExt},
    };
    let parent = match std::fs::symlink_metadata("/etc/usque") {
        Ok(parent) => parent,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if !parent.is_dir() || parent.uid() != 0 || parent.mode() & 0o022 != 0 {
        return Err(ReleaseError::Metadata);
    }
    let file = match OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open("/etc/usque/install.json")
    {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.uid() != 0
        || metadata.mode() & 0o022 != 0
        || metadata.len() > 8192
    {
        return Err(ReleaseError::Metadata);
    }
    let mut bytes = Vec::new();
    file.take(8193).read_to_end(&mut bytes)?;
    if bytes.len() > 8192 {
        return Err(ReleaseError::Metadata);
    }
    let record: InstallRecord =
        serde_json::from_slice(&bytes).map_err(|_| ReleaseError::Metadata)?;
    record.validate()?;
    Ok(Some(record))
}

/// Verify using only the checked-in public key. There is no environment or CLI
/// override in production; changing the identity requires a reviewed rebuild.
pub fn verify_manifest(
    bytes: &[u8],
    signature: &[u8],
    kind: &str,
    architecture: &str,
    minimum_version: &str,
) -> Result<Manifest, ReleaseError> {
    let key = decode_hex::<32>(RELEASE_KEY.trim()).map_err(|_| ReleaseError::UnconfiguredKey)?;
    verify_with_key(bytes, signature, &key, kind, architecture, minimum_version)
}

fn verify_with_key(
    bytes: &[u8],
    signature: &[u8],
    key: &[u8; 32],
    kind: &str,
    architecture: &str,
    minimum_version: &str,
) -> Result<Manifest, ReleaseError> {
    if bytes.is_empty() || bytes.len() > MAX_MANIFEST_BYTES || signature.len() != 64 {
        return Err(ReleaseError::Signature);
    }
    UnparsedPublicKey::new(&ED25519, key)
        .verify(bytes, signature)
        .map_err(|_| ReleaseError::Signature)?;
    let manifest: Manifest = serde_json::from_slice(bytes).map_err(|_| ReleaseError::Metadata)?;
    manifest.validate(kind, architecture, minimum_version)?;
    Ok(manifest)
}

impl Manifest {
    fn validate(
        &self,
        kind: &str,
        architecture: &str,
        minimum_version: &str,
    ) -> Result<(), ReleaseError> {
        // A release replaces the Engine, Agent and helper together, so it may
        // advance their protocol. An installed helper must still be able to
        // verify and install that release; an older protocol is never accepted.
        if self.schema_version != 1 || self.protocol_version < PROTOCOL_VERSION || self.kind != kind
        {
            return Err(ReleaseError::Metadata);
        }
        if !matches!(architecture, "amd64" | "arm64") || self.architecture != architecture {
            return Err(ReleaseError::Architecture);
        }
        if version(&self.version)? < version(minimum_version)? {
            return Err(ReleaseError::Downgrade);
        }
        let expected: BTreeSet<String> = match kind {
            "packages" => INSTALL_FORMATS
                .iter()
                .map(|format| package_name(&self.version, architecture, format))
                .collect::<Result<_, _>>()?,
            "service" => SERVICE_FILES
                .iter()
                .map(|name| (*name).to_owned())
                .collect(),
            _ => return Err(ReleaseError::Metadata),
        };
        let mut actual = BTreeSet::new();
        for entry in &self.entries {
            if !actual.insert(entry.name.clone())
                || entry.size == 0
                || entry.size > MAX_PACKAGE_BYTES
                || decode_hex::<32>(&entry.sha256).is_err()
            {
                return Err(ReleaseError::Metadata);
            }
        }
        if actual != expected {
            return Err(ReleaseError::Metadata);
        }
        Ok(())
    }

    pub fn entry(&self, name: &str) -> Result<&Entry, ReleaseError> {
        self.entries
            .iter()
            .find(|entry| entry.name == name)
            .ok_or(ReleaseError::Metadata)
    }
}

/// Hash an already opened file and enforce the signed length while reading.
/// Callers must install this verified file, not reopen an attacker-controlled path.
pub fn verify_file(mut file: impl Read, entry: &Entry) -> Result<(), ReleaseError> {
    if entry.size == 0 || entry.size > MAX_PACKAGE_BYTES {
        return Err(ReleaseError::Metadata);
    }
    let mut digest = Sha256::new();
    let mut total = 0_u64;
    let mut buffer = [0; 64 * 1024];
    loop {
        let size = file.read(&mut buffer)?;
        if size == 0 {
            break;
        }
        total += size as u64;
        if total > entry.size {
            return Err(ReleaseError::Content);
        }
        digest.update(&buffer[..size]);
    }
    if total != entry.size || digest.finalize().as_slice() != decode_hex::<32>(&entry.sha256)? {
        return Err(ReleaseError::Content);
    }
    Ok(())
}

pub fn package_name(
    version_text: &str,
    architecture: &str,
    format: &str,
) -> Result<String, ReleaseError> {
    version(version_text)?;
    if !matches!(architecture, "amd64" | "arm64") || !INSTALL_FORMATS.contains(&format) {
        return Err(ReleaseError::Metadata);
    }
    Ok(format!(
        "usque-v{version_text}-linux-{architecture}.{format}"
    ))
}

/// Canonical stable/beta release ordering; stable always follows its betas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ReleaseVersion([u64; 3], ReleaseStage);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ReleaseStage {
    Beta(u64),
    Stable,
}

pub fn version(text: &str) -> Result<ReleaseVersion, ReleaseError> {
    let (core, stage) = if let Some((core, ordinal)) = text.split_once("-beta.") {
        if ordinal.is_empty()
            || ordinal.starts_with('0')
            || !ordinal.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(ReleaseError::Metadata);
        }
        let ordinal = ordinal.parse().map_err(|_| ReleaseError::Metadata)?;
        (core, ReleaseStage::Beta(ordinal))
    } else {
        (text, ReleaseStage::Stable)
    };
    let parts: Vec<_> = core.split('.').collect();
    if parts.len() != 3 {
        return Err(ReleaseError::Metadata);
    }
    let mut result = [0; 3];
    for (index, part) in parts.into_iter().enumerate() {
        if part.is_empty()
            || (part.len() > 1 && part.starts_with('0'))
            || !part.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(ReleaseError::Metadata);
        }
        result[index] = part.parse().map_err(|_| ReleaseError::Metadata)?;
    }
    Ok(ReleaseVersion(result, stage))
}

fn decode_hex<const N: usize>(value: &str) -> Result<[u8; N], ReleaseError> {
    if value.len() != N * 2
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(ReleaseError::Metadata);
    }
    let mut result = [0; N];
    for (index, byte) in result.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
            .map_err(|_| ReleaseError::Metadata)?;
    }
    Ok(result)
}

#[derive(Debug, Error)]
pub enum ReleaseError {
    #[error("Linux release verification key has not been configured")]
    UnconfiguredKey,
    #[error("Linux manifest signature is invalid")]
    Signature,
    #[error("Linux manifest metadata is invalid")]
    Metadata,
    #[error("Linux package architecture does not match this installation")]
    Architecture,
    #[error("Linux package version would roll back this installation")]
    Downgrade,
    #[error("Linux installation format does not match; remove the existing installation first")]
    MixedInstallation,
    #[error("Linux payload size or digest does not match")]
    Content,
    #[error("Linux package could not be read")]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::signature::{Ed25519KeyPair, KeyPair};

    #[test]
    fn beta_versions_sort_numerically_before_stable_and_reject_noncanonical_input() {
        let texts = [
            "0.3.2",
            "0.3.3-beta.1",
            "0.3.3-beta.2",
            "0.3.3-beta.10",
            "0.3.3",
        ];
        let values: Vec<_> = texts
            .into_iter()
            .map(|text| version(text).unwrap())
            .collect();
        assert!(values.windows(2).all(|pair| pair[0] < pair[1]));
        for invalid in [
            "0.3.3-beta.0",
            "0.3.3-beta.01",
            "0.3.3-rc.1",
            "0.3.3~beta.1",
            "0.3.3\n",
            "00.3.3",
            "0.3.3-beta.18446744073709551616",
        ] {
            assert!(version(invalid).is_err(), "{invalid}");
        }
        let (mut manifest, key) = fixture();
        manifest.version = "1.2.3-beta.1".into();
        for entry in &mut manifest.entries {
            entry.name = entry.name.replace("1.2.3", "1.2.3-beta.1");
        }
        assert!(verify(&manifest, &key, "amd64", "1.2.2").is_ok());
        assert!(verify(&manifest, &key, "amd64", "1.2.3").is_err());
        assert!(verify(&manifest, &key, "amd64", "1.2.3-beta.2").is_err());
    }

    fn fixture() -> (Manifest, Ed25519KeyPair) {
        let manifest = Manifest {
            schema_version: 1,
            kind: "packages".into(),
            version: "1.2.3".into(),
            architecture: "amd64".into(),
            protocol_version: PROTOCOL_VERSION,
            entries: INSTALL_FORMATS
                .iter()
                .map(|format| Entry {
                    name: package_name("1.2.3", "amd64", format).unwrap(),
                    size: 7,
                    sha256: Sha256::digest(b"payload")
                        .iter()
                        .map(|byte| format!("{byte:02x}"))
                        .collect(),
                })
                .collect(),
        };
        (
            manifest,
            Ed25519KeyPair::from_seed_unchecked(&[7; 32]).unwrap(),
        )
    }

    fn verify(
        manifest: &Manifest,
        key: &Ed25519KeyPair,
        arch: &str,
        minimum: &str,
    ) -> Result<Manifest, ReleaseError> {
        let bytes = serde_json::to_vec(manifest).unwrap();
        verify_with_key(
            &bytes,
            key.sign(&bytes).as_ref(),
            key.public_key().as_ref().try_into().unwrap(),
            "packages",
            arch,
            minimum,
        )
    }

    #[test]
    fn authentic_manifest_binds_all_formats_architecture_and_version() {
        let (manifest, key) = fixture();
        assert!(verify(&manifest, &key, "amd64", "1.2.2").is_ok());
        assert!(matches!(
            verify(&manifest, &key, "arm64", "1.2.2"),
            Err(ReleaseError::Architecture)
        ));
        assert!(matches!(
            verify(&manifest, &key, "amd64", "1.2.4"),
            Err(ReleaseError::Downgrade)
        ));
    }

    #[test]
    fn newer_releases_may_advance_but_never_regress_the_agent_protocol() {
        let (mut manifest, key) = fixture();
        manifest.protocol_version = PROTOCOL_VERSION + 1;
        assert_eq!(
            verify(&manifest, &key, "amd64", "1.2.2")
                .unwrap()
                .protocol_version,
            PROTOCOL_VERSION + 1
        );
        manifest.protocol_version = PROTOCOL_VERSION - 1;
        assert!(matches!(
            verify(&manifest, &key, "amd64", "1.2.2"),
            Err(ReleaseError::Metadata)
        ));
    }

    #[test]
    fn rejects_tampering_wrong_key_duplicate_missing_and_traversal_entries() {
        let (manifest, key) = fixture();
        let bytes = serde_json::to_vec(&manifest).unwrap();
        let signature = key.sign(&bytes);
        let other = Ed25519KeyPair::from_seed_unchecked(&[8; 32]).unwrap();
        assert!(
            verify_with_key(
                &bytes,
                signature.as_ref(),
                other.public_key().as_ref().try_into().unwrap(),
                "packages",
                "amd64",
                "1.0.0"
            )
            .is_err()
        );
        let mut tampered = bytes.clone();
        tampered.push(b' ');
        assert!(
            verify_with_key(
                &tampered,
                signature.as_ref(),
                key.public_key().as_ref().try_into().unwrap(),
                "packages",
                "amd64",
                "1.0.0"
            )
            .is_err()
        );
        for name in ["../usque.deb", "usque-v1.2.3-linux-amd64.rpm"] {
            let mut bad = manifest.clone();
            bad.entries[0].name = name.into();
            assert!(verify(&bad, &key, "amd64", "1.0.0").is_err());
        }
        let mut bad = manifest;
        bad.entries.pop();
        assert!(verify(&bad, &key, "amd64", "1.0.0").is_err());
    }

    #[test]
    fn verifies_open_bytes_and_rejects_truncation_and_growth() {
        let (manifest, _) = fixture();
        let entry = &manifest.entries[0];
        assert!(verify_file(&b"payload"[..], entry).is_ok());
        for bytes in [&b"payloa"[..], &b"payload!"[..], &b"PAYLOAD"[..]] {
            assert!(verify_file(bytes, entry).is_err());
        }
    }

    #[test]
    fn rejects_mixed_installations_and_ambiguous_versions() {
        let record = InstallRecord {
            schema_version: 1,
            version: "1.2.3".into(),
            protocol_version: PROTOCOL_VERSION,
            format: "appimage".into(),
            architecture: "amd64".into(),
            engine_sha256: "ab".repeat(32),
        };
        assert!(record.require_source("AppImage", "amd64").is_ok());
        assert!(matches!(
            record.require_source("deb", "amd64"),
            Err(ReleaseError::MixedInstallation)
        ));
        for text in ["01.2.3", "1.2", "1.2.3.4", "1.2.3-beta.0", "1.2.+3"] {
            assert!(version(text).is_err());
        }
    }
}
