//! Current-user macOS Keychain storage for WARP identity records.

use async_trait::async_trait;
use security_framework::passwords::{
    delete_generic_password, get_generic_password, set_generic_password,
};
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::{SecretRecord, SecretVault, VaultError};

const MAX_SECRET_BYTES: usize = 5 * 512;
const KEYCHAIN_SERVICE: &str = "io.github.georgexie2333.usque.identity";
const ERR_SEC_ITEM_NOT_FOUND: i32 = -25_300;

#[derive(Debug, Clone, Copy, Default)]
pub struct MacOsKeychainVault;

#[async_trait]
impl SecretVault for MacOsKeychainVault {
    async fn put(
        &self,
        profile_id: Uuid,
        record: SecretRecord,
        value: &[u8],
    ) -> Result<(), VaultError> {
        validate_size(value)?;
        let account = account_name(profile_id, record);
        let value = Zeroizing::new(value.to_vec());
        tokio::task::spawn_blocking(move || {
            set_generic_password(KEYCHAIN_SERVICE, &account, &value).map_err(platform_error)
        })
        .await
        .map_err(worker_error)?
    }

    async fn get(
        &self,
        profile_id: Uuid,
        record: SecretRecord,
    ) -> Result<Option<Zeroizing<Vec<u8>>>, VaultError> {
        let account = account_name(profile_id, record);
        tokio::task::spawn_blocking(move || {
            match get_generic_password(KEYCHAIN_SERVICE, &account) {
                Ok(value) if value.is_empty() || value.len() > MAX_SECRET_BYTES => Err(
                    VaultError::Platform("Keychain returned an invalid secret size".to_owned()),
                ),
                Ok(value) => Ok(Some(Zeroizing::new(value))),
                Err(error) if error.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(None),
                Err(error) => Err(platform_error(error)),
            }
        })
        .await
        .map_err(worker_error)?
    }

    async fn delete(&self, profile_id: Uuid, record: SecretRecord) -> Result<(), VaultError> {
        let account = account_name(profile_id, record);
        tokio::task::spawn_blocking(move || {
            match delete_generic_password(KEYCHAIN_SERVICE, &account) {
                Ok(()) => Ok(()),
                Err(error) if error.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(()),
                Err(error) => Err(platform_error(error)),
            }
        })
        .await
        .map_err(worker_error)?
    }
}

fn validate_size(value: &[u8]) -> Result<(), VaultError> {
    if value.is_empty() || value.len() > MAX_SECRET_BYTES {
        Err(VaultError::InvalidSecretSize)
    } else {
        Ok(())
    }
}

fn account_name(profile_id: Uuid, record: SecretRecord) -> String {
    format!("{profile_id}/{}", record.key())
}

fn platform_error(error: security_framework::base::Error) -> VaultError {
    VaultError::Platform(format!("Security.framework OSStatus {}", error.code()))
}

fn worker_error(error: tokio::task::JoinError) -> VaultError {
    VaultError::Platform(format!("Keychain worker failed: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keychain_namespace_is_stable_and_has_no_secret_material() {
        let profile = Uuid::parse_str("8c30b771-9ebd-457a-b67b-bbc74a1ddba6").unwrap();
        assert_eq!(
            account_name(profile, SecretRecord::EndpointPin),
            "8c30b771-9ebd-457a-b67b-bbc74a1ddba6/endpoint-pin"
        );
        assert_eq!(KEYCHAIN_SERVICE, "io.github.georgexie2333.usque.identity");
    }

    #[tokio::test]
    async fn invalid_sizes_are_rejected_before_keychain_access() {
        let vault = MacOsKeychainVault;
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
                    &vec![0; MAX_SECRET_BYTES + 1],
                )
                .await,
            Err(VaultError::InvalidSecretSize)
        ));
    }
}
