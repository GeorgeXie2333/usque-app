use std::io;
use std::ptr::{self, NonNull};
use std::slice;

use async_trait::async_trait;
use uuid::Uuid;
use windows_sys::Win32::Foundation::{ERROR_NOT_FOUND, GetLastError};
use windows_sys::Win32::Security::Credentials::{
    CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC, CREDENTIALW, CredDeleteW, CredEnumerateW,
    CredFree, CredReadW, CredWriteW,
};
use zeroize::Zeroizing;

use crate::{SecretRecord, SecretVault, VaultError};

const MAX_CREDENTIAL_BLOB_BYTES: usize = 5 * 512;
const TARGET_PREFIX: &str = "io.github.georgexie2333.usque/identity";

#[derive(Debug, Clone, Copy, Default)]
pub struct WindowsCredentialVault;

impl WindowsCredentialVault {
    /// Removes every Usque identity owned by the current Windows user.
    ///
    /// This is intentionally namespace-scoped and is used by the optional
    /// uninstall cleanup path. Enumerating the namespace also removes orphaned
    /// transaction records that can no longer be reached from `config.json`.
    pub fn delete_all_namespaced() -> Result<(), VaultError> {
        let filter = wide_null(&enumeration_filter());
        let mut count = 0_u32;
        let mut raw_credentials = ptr::null_mut::<*mut CREDENTIALW>();
        // SAFETY: filter is null-terminated and both output pointers are valid
        // for the duration of CredEnumerateW.
        if unsafe { CredEnumerateW(filter.as_ptr(), 0, &mut count, &mut raw_credentials) } == 0 {
            // SAFETY: called immediately after the failing Win32 operation.
            let code = unsafe { GetLastError() };
            return if code == ERROR_NOT_FOUND {
                Ok(())
            } else {
                Err(platform_error(code))
            };
        }

        let credentials = CredentialListBuffer(raw_credentials);
        if count != 0 && credentials.0.is_null() {
            return Err(VaultError::Platform(
                "CredEnumerateW returned a null credential list".to_owned(),
            ));
        }

        let entries = if count == 0 {
            &[][..]
        } else {
            // SAFETY: CredEnumerateW returned `count` pointers in this buffer,
            // which remains owned by CredentialListBuffer for the whole loop.
            unsafe { slice::from_raw_parts(credentials.0, count as usize) }
        };
        let mut first_error = None;
        for &credential in entries {
            if credential.is_null() {
                if first_error.is_none() {
                    first_error = Some(VaultError::Platform(
                        "CredEnumerateW returned a null credential".to_owned(),
                    ));
                }
                continue;
            }
            // SAFETY: each non-null entry points to a CREDENTIALW owned by the
            // enumeration buffer.
            let target = unsafe { wide_null_to_string((*credential).TargetName) };
            let target = match target {
                Ok(target) if is_namespaced_target(&target) => target,
                Ok(_) => {
                    if first_error.is_none() {
                        first_error = Some(VaultError::Platform(
                            "Credential Manager returned a target outside the Usque namespace"
                                .to_owned(),
                        ));
                    }
                    continue;
                }
                Err(error) => {
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                    continue;
                }
            };
            let target = wide_null(&target);
            // Usque writes only generic credentials in this namespace. Using
            // the fixed type prevents an unexpected record type from widening
            // the cleanup operation.
            // SAFETY: target is a valid null-terminated UTF-16 string.
            if unsafe { CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0) } == 0 {
                // SAFETY: called immediately after the failing Win32 operation.
                let code = unsafe { GetLastError() };
                if code != ERROR_NOT_FOUND && first_error.is_none() {
                    first_error = Some(platform_error(code));
                }
            }
        }
        first_error.map_or(Ok(()), Err)
    }
}

#[async_trait]
impl SecretVault for WindowsCredentialVault {
    async fn put(
        &self,
        profile_id: Uuid,
        record: SecretRecord,
        value: &[u8],
    ) -> Result<(), VaultError> {
        if value.is_empty() || value.len() > MAX_CREDENTIAL_BLOB_BYTES {
            return Err(VaultError::InvalidSecretSize);
        }

        let mut target = wide_null(&target_name(profile_id, record));
        let credential = CREDENTIALW {
            Type: CRED_TYPE_GENERIC,
            TargetName: target.as_mut_ptr(),
            CredentialBlobSize: value.len() as u32,
            CredentialBlob: value.as_ptr().cast_mut(),
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            ..CREDENTIALW::default()
        };

        // SAFETY: all pointers remain valid for the duration of CredWriteW and
        // their sizes exactly describe the referenced buffers.
        if unsafe { CredWriteW(&credential, 0) } == 0 {
            return Err(last_platform_error());
        }
        Ok(())
    }

    async fn get(
        &self,
        profile_id: Uuid,
        record: SecretRecord,
    ) -> Result<Option<Zeroizing<Vec<u8>>>, VaultError> {
        let target = wide_null(&target_name(profile_id, record));
        let mut raw_credential = ptr::null_mut::<CREDENTIALW>();

        // SAFETY: target is null-terminated and the output pointer is valid.
        if unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut raw_credential) } == 0 {
            // SAFETY: GetLastError has no preconditions and is called
            // immediately after the failing Win32 operation.
            let code = unsafe { GetLastError() };
            return if code == ERROR_NOT_FOUND {
                Ok(None)
            } else {
                Err(platform_error(code))
            };
        }

        let credential = CredentialBuffer::new(raw_credential).ok_or_else(|| {
            VaultError::Platform("CredReadW returned a null credential".to_owned())
        })?;
        let record = credential.as_ref();
        let size = record.CredentialBlobSize as usize;
        let pointer = checked_blob_pointer(size, record.CredentialBlob)?;
        let value = match pointer {
            None => Zeroizing::new(Vec::new()),
            Some(pointer) => {
                // SAFETY: CredReadW guarantees that the credential allocation
                // owns at least `size` blob bytes until CredFree. The checked
                // pointer is non-null, and a byte slice has no alignment gap.
                Zeroizing::new(unsafe { slice::from_raw_parts(pointer.as_ptr(), size).to_vec() })
            }
        };
        Ok(Some(value))
    }

    async fn delete(&self, profile_id: Uuid, record: SecretRecord) -> Result<(), VaultError> {
        let target = wide_null(&target_name(profile_id, record));
        // SAFETY: target is a valid, null-terminated UTF-16 string.
        if unsafe { CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0) } == 0 {
            // SAFETY: called immediately after the failing Win32 operation.
            let code = unsafe { GetLastError() };
            if code != ERROR_NOT_FOUND {
                return Err(platform_error(code));
            }
        }
        Ok(())
    }
}

struct CredentialBuffer(NonNull<CREDENTIALW>);

impl CredentialBuffer {
    fn new(pointer: *mut CREDENTIALW) -> Option<Self> {
        NonNull::new(pointer).map(Self)
    }

    fn as_ref(&self) -> &CREDENTIALW {
        // SAFETY: CredReadW returned this non-null allocation and it remains
        // owned by this buffer until Drop runs.
        unsafe { self.0.as_ref() }
    }
}

impl Drop for CredentialBuffer {
    fn drop(&mut self) {
        // SAFETY: the pointer was allocated by CredReadW and is freed once.
        unsafe { CredFree(self.0.as_ptr().cast()) };
    }
}

struct CredentialListBuffer(*mut *mut CREDENTIALW);

impl Drop for CredentialListBuffer {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: the pointer was allocated by CredEnumerateW and is freed
            // exactly once after all nested credential pointers are unused.
            unsafe { CredFree(self.0.cast()) };
        }
    }
}

fn target_name(profile_id: Uuid, record: SecretRecord) -> String {
    format!("{TARGET_PREFIX}/{profile_id}/{}", record.key())
}

fn enumeration_filter() -> String {
    format!("{TARGET_PREFIX}/*")
}

fn is_namespaced_target(value: &str) -> bool {
    value
        .strip_prefix(TARGET_PREFIX)
        .is_some_and(|suffix| suffix.starts_with('/'))
}

fn checked_blob_pointer(size: usize, pointer: *mut u8) -> Result<Option<NonNull<u8>>, VaultError> {
    if size > MAX_CREDENTIAL_BLOB_BYTES {
        return Err(VaultError::Platform(
            "Credential Manager returned an invalid blob".to_owned(),
        ));
    }
    if size == 0 {
        return Ok(None);
    }
    NonNull::new(pointer).map(Some).ok_or_else(|| {
        VaultError::Platform("Credential Manager returned an invalid blob".to_owned())
    })
}

unsafe fn wide_null_to_string(value: *const u16) -> Result<String, VaultError> {
    const MAX_TARGET_UNITS: usize = 32_768;
    if value.is_null() {
        return Err(VaultError::Platform(
            "Credential Manager returned a null target name".to_owned(),
        ));
    }
    for length in 0..MAX_TARGET_UNITS {
        // SAFETY: the caller supplies a Windows-owned null-terminated buffer;
        // the defensive bound prevents an unbounded scan if it is malformed.
        if unsafe { *value.add(length) } == 0 {
            // SAFETY: every unit in this slice precedes the terminator.
            let units = unsafe { slice::from_raw_parts(value, length) };
            return String::from_utf16(units).map_err(|_| {
                VaultError::Platform(
                    "Credential Manager returned a non-UTF-16 target name".to_owned(),
                )
            });
        }
    }
    Err(VaultError::Platform(
        "Credential Manager returned an unterminated target name".to_owned(),
    ))
}

fn wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn last_platform_error() -> VaultError {
    // SAFETY: called immediately after a failing Win32 operation.
    platform_error(unsafe { GetLastError() })
}

fn platform_error(code: u32) -> VaultError {
    VaultError::Platform(io::Error::from_raw_os_error(code as i32).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_targets_are_namespaced_and_stable() {
        let profile = Uuid::parse_str("8c30b771-9ebd-457a-b67b-bbc74a1ddba6").unwrap();
        assert_eq!(
            target_name(profile, SecretRecord::EndpointPin),
            "io.github.georgexie2333.usque/identity/8c30b771-9ebd-457a-b67b-bbc74a1ddba6/endpoint-pin"
        );
        assert_eq!(
            enumeration_filter(),
            "io.github.georgexie2333.usque/identity/*"
        );
        assert!(is_namespaced_target(
            "io.github.georgexie2333.usque/identity/profile/warp-secret"
        ));
        assert!(!is_namespaced_target(
            "io.github.georgexie2333.usque/identity-other/profile/warp-secret"
        ));
    }

    #[test]
    fn credential_blob_pointer_validation_handles_empty_and_invalid_blobs() {
        assert!(checked_blob_pointer(0, ptr::null_mut()).unwrap().is_none());
        assert!(checked_blob_pointer(1, ptr::null_mut()).is_err());
        assert!(checked_blob_pointer(MAX_CREDENTIAL_BLOB_BYTES + 1, ptr::dangling_mut()).is_err());

        let mut byte = 42_u8;
        let expected = NonNull::from(&mut byte);
        let pointer = checked_blob_pointer(1, &mut byte).unwrap().unwrap();
        assert_eq!(pointer, expected);
    }

    #[tokio::test]
    async fn invalid_sizes_are_rejected_before_calling_windows() {
        let vault = WindowsCredentialVault;
        assert!(matches!(
            vault
                .put(Uuid::new_v4(), SecretRecord::AccessToken, &[])
                .await,
            Err(VaultError::InvalidSecretSize)
        ));
        assert!(matches!(
            vault
                .put(
                    Uuid::new_v4(),
                    SecretRecord::AccessToken,
                    &vec![0; MAX_CREDENTIAL_BLOB_BYTES + 1],
                )
                .await,
            Err(VaultError::InvalidSecretSize)
        ));
    }
}
