use std::io;
use std::ptr;
use std::slice;

use async_trait::async_trait;
use uuid::Uuid;
use windows_sys::Win32::Foundation::{ERROR_NOT_FOUND, GetLastError};
use windows_sys::Win32::Security::Credentials::{
    CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC, CREDENTIALW, CredDeleteW, CredFree, CredReadW,
    CredWriteW,
};
use zeroize::Zeroizing;

use crate::{SecretRecord, SecretVault, VaultError};

const MAX_CREDENTIAL_BLOB_BYTES: usize = 5 * 512;
const TARGET_PREFIX: &str = "io.github.georgexie2333.usque/identity";

#[derive(Debug, Clone, Copy, Default)]
pub struct WindowsCredentialVault;

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

        let credential = CredentialBuffer(raw_credential);
        if credential.0.is_null() {
            return Err(VaultError::Platform(
                "CredReadW returned a null credential".to_owned(),
            ));
        }
        // SAFETY: CredReadW returned a valid CREDENTIALW allocation owned by
        // CredentialBuffer until after this copy completes.
        let value = unsafe {
            let size = (*credential.0).CredentialBlobSize as usize;
            let pointer = (*credential.0).CredentialBlob;
            if size > MAX_CREDENTIAL_BLOB_BYTES || (size != 0 && pointer.is_null()) {
                return Err(VaultError::Platform(
                    "Credential Manager returned an invalid blob".to_owned(),
                ));
            }
            Zeroizing::new(slice::from_raw_parts(pointer, size).to_vec())
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

struct CredentialBuffer(*mut CREDENTIALW);

impl Drop for CredentialBuffer {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: the pointer was allocated by CredReadW and is freed once.
            unsafe { CredFree(self.0.cast()) };
        }
    }
}

fn target_name(profile_id: Uuid, record: SecretRecord) -> String {
    format!("{TARGET_PREFIX}/{profile_id}/{}", record.key())
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
