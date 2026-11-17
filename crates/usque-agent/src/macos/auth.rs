//! The installation pins both file contents and the live code directory.
use super::transaction::Owner;
use core_foundation::{
    base::{CFType, TCFType},
    data::CFData,
    dictionary::{CFDictionary, CFDictionaryRef},
    number::CFNumber,
    string::{CFString, CFStringRef},
};
use security_framework::os::macos::code_signing::{
    Flags, GuestAttributes, SecCode, SecRequirement,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{File, OpenOptions},
    io::{self, Read},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::Path,
    str::FromStr,
};
use usque_ipc::darwin_fd::PeerIdentity;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct InstallRecord {
    pub schema: u32,
    pub version: String,
    pub protocol: u32,
    pub architecture: String,
    pub engine_sha256: String,
    pub engine_cdhash: String,
}
impl InstallRecord {
    pub fn validate(&self) -> io::Result<()> {
        if self.schema != 1
            || self.version != env!("CARGO_PKG_VERSION")
            || self.protocol != crate::AGENT_PROTOCOL_VERSION
            || self.architecture != std::env::consts::ARCH
            || !matches!(self.architecture.as_str(), "aarch64" | "x86_64")
            || !hex(&self.engine_sha256, 64)
            || !hex(&self.engine_cdhash, 40)
        {
            return Err(denied("incompatible macOS Agent installation"));
        }
        Ok(())
    }
}
fn owner(peer: PeerIdentity) -> io::Result<Owner> {
    let token = peer.audit_token;
    if token[1] == 0
        || token[1] != token[3]
        || token[5] == 0
        || token[5] > i32::MAX as u32
        || token[7] == 0
    {
        return Err(denied("Engine must have a stable ordinary-user identity"));
    }
    Ok(Owner {
        uid: token[1],
        pid: token[5],
        pid_version: token[7],
    })
}
fn hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn denied(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, message)
}

/// No debug escape hatch. Copies of the same Engine at user-owned paths do not
/// qualify, even if they have a matching ad-hoc signature.
pub struct CallerPolicy {
    pub record: InstallRecord,
    engine: File,
}
impl CallerPolicy {
    pub fn load() -> io::Result<Self> {
        let mut bytes = Vec::new();
        trusted_file(Path::new(super::INSTALL_RECORD_PATH))?
            .take(16385)
            .read_to_end(&mut bytes)?;
        if bytes.len() > 16384 {
            return Err(denied("installation record too large"));
        }
        let record: InstallRecord =
            serde_json::from_slice(&bytes).map_err(|_| denied("invalid installation record"))?;
        record.validate()?;
        let mut engine = trusted_file(Path::new(super::ENGINE_PATH))?;
        if engine.metadata()?.len() > 512 * 1024 * 1024 {
            return Err(denied("Engine exceeds size bound"));
        }
        let mut hash = Sha256::new();
        let mut buffer = [0_u8; 65536];
        loop {
            let n = engine.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            hash.update(&buffer[..n]);
        }
        let digest: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
        if digest != record.engine_sha256 {
            return Err(denied("Engine file digest mismatch"));
        }
        Ok(Self { record, engine })
    }
    /// The server calls this again for every command, using the socket's audit
    /// token. Security.framework asks the kernel for that exact incarnation.
    pub fn authenticate(&self, peer: PeerIdentity) -> io::Result<Owner> {
        let owner = owner(peer)?;
        let token: Vec<u8> = peer
            .audit_token
            .iter()
            .flat_map(|n| n.to_ne_bytes())
            .collect();
        let token = CFData::from_buffer(&token);
        let mut attrs = GuestAttributes::new();
        attrs.set_audit_token(token.as_concrete_TypeRef());
        let code = SecCode::copy_guest_with_attribues(None, &attrs, Flags::NONE)
            .map_err(|_| denied("Engine code identity unavailable"))?;
        let requirement =
            SecRequirement::from_str(&format!("cdhash H\"{}\"", self.record.engine_cdhash))
                .map_err(|_| denied("invalid Engine code requirement"))?;
        code.check_validity(Flags::STRICT_VALIDATE, &requirement)
            .map_err(|_| denied("Engine live signature rejected"))?;
        let path = code
            .path(Flags::NONE)
            .map_err(|_| denied("Engine path unavailable"))?
            .to_path()
            .ok_or_else(|| denied("Engine path unavailable"))?;
        if path != Path::new(super::ENGINE_PATH) {
            return Err(denied("caller is not the root-installed Engine"));
        }
        let installed = trusted_file(&path)?.metadata()?;
        let pinned = self.engine.metadata()?;
        if installed.dev() != pinned.dev()
            || installed.ino() != pinned.ino()
            || installed.len() != pinned.len()
        {
            return Err(denied(
                "Engine changed; restart Agent after authorized update",
            ));
        }
        verify_hardening(&code)?;
        // Recheck after inspecting static signing information. Kernel dynamic
        // validity must still agree with the audit-token-selected guest.
        code.check_validity(Flags::NONE, &requirement)
            .map_err(|_| denied("Engine integrity changed"))?;
        Ok(owner)
    }
}
pub(super) fn trusted_file(path: &Path) -> io::Result<File> {
    super::journal::verify_root_parents(path)?;
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(path)?;
    let m = file.metadata()?;
    if !m.is_file() || m.uid() != 0 || m.mode() & 0o022 != 0 || m.nlink() != 1 {
        return Err(denied("untrusted installation file"));
    }
    super::journal::no_extended_acl(&file)?;
    Ok(file)
}
#[link(name = "Security", kind = "framework")]
unsafe extern "C" {
    fn SecCodeCopySigningInformation(
        code: *const std::ffi::c_void,
        flags: u32,
        information: *mut CFDictionaryRef,
    ) -> i32;
    static kSecCodeInfoFlags: CFStringRef;
    static kSecCodeInfoStatus: CFStringRef;
    static kSecCodeInfoEntitlementsDict: CFStringRef;
    static kSecCodeInfoEntitlements: CFStringRef;
}
fn verify_hardening(code: &SecCode) -> io::Result<()> {
    let mut raw = std::ptr::null();
    // SAFETY: live code and writable CF out-pointer; signing + dynamic info.
    let result = unsafe {
        SecCodeCopySigningInformation(code.as_CFTypeRef(), (1 << 1) | (1 << 3), &mut raw)
    };
    if result != 0 || raw.is_null() {
        return Err(denied("Engine hardening information unavailable"));
    }
    // SAFETY: successful Copy transferred an owned dictionary.
    let info: CFDictionary<CFString, CFType> = unsafe { CFDictionary::wrap_under_create_rule(raw) };
    // SAFETY: exported framework keys live for the process lifetime.
    let (flags, status, entitlements) = unsafe {
        (
            CFString::wrap_under_get_rule(kSecCodeInfoFlags),
            CFString::wrap_under_get_rule(kSecCodeInfoStatus),
            CFString::wrap_under_get_rule(kSecCodeInfoEntitlementsDict),
        )
    };
    let number = |key: &CFString| {
        info.find(key)
            .and_then(|v| v.downcast::<CFNumber>())
            .and_then(|n| n.to_i64())
            .map(|n| n as u32)
            .ok_or_else(|| denied("missing Engine code flags"))
    };
    let flags = number(&flags)?;
    let status = number(&status)?;
    let entitlements_empty = match info.find(&entitlements) {
        None => true,
        Some(value) => value
            .downcast::<CFDictionary>()
            .is_some_and(|d| d.is_empty()),
    };
    // A missing EntitlementsDict can also mean malformed non-dictionary
    // entitlement data. No entitlement blob is needed by the installed Engine.
    // SAFETY: this exported framework key lives for the process lifetime.
    let raw_entitlements = unsafe { CFString::wrap_under_get_rule(kSecCodeInfoEntitlements) };
    let no_blob = info
        .find(&raw_entitlements)
        .is_none_or(|value| value.downcast::<CFData>().is_some_and(|d| d.is_empty()));
    validate_hardening(flags, status, entitlements_empty && no_blob)
}
fn validate_hardening(flags: u32, status: u32, entitlements_empty: bool) -> io::Result<()> {
    const RUNTIME: u32 = 0x10000;
    const VALID: u32 = 1;
    const GET_TASK_ALLOW: u32 = 4;
    const DEBUGGED: u32 = 0x10000000;
    if flags & RUNTIME == 0
        || status & (RUNTIME | VALID) != (RUNTIME | VALID)
        || status & (GET_TASK_ALLOW | DEBUGGED) != 0
        || !entitlements_empty
    {
        return Err(denied(
            "Engine requires hardened runtime without entitlements or debugging",
        ));
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn record() -> InstallRecord {
        InstallRecord {
            schema: 1,
            version: env!("CARGO_PKG_VERSION").into(),
            protocol: crate::AGENT_PROTOCOL_VERSION,
            architecture: std::env::consts::ARCH.into(),
            engine_sha256: "ab".repeat(32),
            engine_cdhash: "cd".repeat(20),
        }
    }
    #[test]
    fn pins_exact_version_architecture_protocol_and_hashes() {
        let valid = record();
        valid.validate().unwrap();
        for bad in [
            InstallRecord {
                schema: 2,
                ..valid.clone()
            },
            InstallRecord {
                protocol: 0,
                ..valid.clone()
            },
            InstallRecord {
                architecture: "other".into(),
                ..valid.clone()
            },
            InstallRecord {
                version: "0".into(),
                ..valid.clone()
            },
            InstallRecord {
                engine_sha256: "AB".repeat(32),
                ..valid.clone()
            },
            InstallRecord {
                engine_cdhash: "x".repeat(40),
                ..valid
            },
        ] {
            assert!(bad.validate().is_err());
        }
    }
    #[test]
    fn audit_owner_requires_regular_unchanged_user_and_process_incarnation() {
        let token = [501, 501, 20, 501, 20, 123, 9, 3];
        assert_eq!(
            owner(PeerIdentity { audit_token: token }).unwrap(),
            Owner {
                uid: 501,
                pid: 123,
                pid_version: 3
            }
        );
        for (index, value) in [(1, 0), (3, 502), (5, 0), (7, 0)] {
            let mut audit_token = token;
            audit_token[index] = value;
            assert!(owner(PeerIdentity { audit_token }).is_err());
        }
    }
    #[test]
    fn rejects_debugging_entitlements_and_missing_live_hardening() {
        validate_hardening(0x10000, 0x10001, true).unwrap();
        for (flags, status, empty) in [
            (0, 0x10001, true),
            (0x10000, 0x10000, true),
            (0x10000, 1, true),
            (0x10000, 0x10005, true),
            (0x10000, 0x10010001, true),
            (0x10000, 0x10001, false),
        ] {
            assert!(validate_hardening(flags, status, empty).is_err());
        }
        let code = SecCode::for_self(Flags::NONE).unwrap();
        assert!(
            verify_hardening(&code).is_err(),
            "test binary must not satisfy the installed hardened Engine policy"
        );
    }
}
