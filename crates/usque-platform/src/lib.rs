use async_trait::async_trait;
use thiserror::Error;
use uuid::Uuid;
use zeroize::Zeroizing;

pub mod packet_ring;

#[cfg(target_os = "linux")]
pub mod linux_paths;

#[cfg(target_os = "linux")]
pub mod linux_gsettings;

#[cfg(target_os = "linux")]
pub mod linux_secret_service;

#[cfg(target_os = "linux")]
pub use linux_secret_service::LinuxSecretServiceVault;

#[cfg(windows)]
pub mod windows_authenticode;

#[cfg(windows)]
pub mod windows_vault;

#[cfg(target_os = "macos")]
pub mod macos_keychain;

#[cfg(target_os = "macos")]
pub use macos_keychain::MacOsKeychainVault;

#[cfg(windows)]
pub use windows_vault::WindowsCredentialVault;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SecretRecord {
    WarpSecret,
    MasquePrivateKey,
    AccessToken,
    DeviceId,
    License,
    EndpointPin,
    AssignedIpv4,
    AssignedIpv6,
    IdentityMetadata,
    ProxyPassword,
    ChainEncryptionKey,
}

impl SecretRecord {
    pub const ALL: [Self; 11] = [
        Self::WarpSecret,
        Self::MasquePrivateKey,
        Self::AccessToken,
        Self::DeviceId,
        Self::License,
        Self::EndpointPin,
        Self::AssignedIpv4,
        Self::AssignedIpv6,
        Self::IdentityMetadata,
        Self::ProxyPassword,
        Self::ChainEncryptionKey,
    ];

    pub const fn key(self) -> &'static str {
        match self {
            Self::WarpSecret => "warp-secret",
            Self::MasquePrivateKey => "masque-private-key",
            Self::AccessToken => "access-token",
            Self::DeviceId => "device-id",
            Self::License => "license",
            Self::EndpointPin => "endpoint-pin",
            Self::AssignedIpv4 => "assigned-ipv4",
            Self::AssignedIpv6 => "assigned-ipv6",
            Self::IdentityMetadata => "identity-metadata",
            Self::ProxyPassword => "proxy-password",
            Self::ChainEncryptionKey => "chain-encryption-key",
        }
    }
}

#[async_trait]
pub trait SecretVault: Send + Sync {
    async fn put(
        &self,
        profile_id: Uuid,
        record: SecretRecord,
        value: &[u8],
    ) -> Result<(), VaultError>;

    async fn get(
        &self,
        profile_id: Uuid,
        record: SecretRecord,
    ) -> Result<Option<Zeroizing<Vec<u8>>>, VaultError>;

    async fn delete(&self, profile_id: Uuid, record: SecretRecord) -> Result<(), VaultError>;

    async fn delete_identity(&self, profile_id: Uuid) -> Result<(), VaultError> {
        let mut first_error = None;
        for record in SecretRecord::ALL {
            if matches!(
                record,
                SecretRecord::ProxyPassword | SecretRecord::ChainEncryptionKey
            ) {
                continue;
            }
            if let Err(error) = self.delete(profile_id, record).await
                && first_error.is_none()
            {
                first_error = Some(error);
            }
        }
        first_error.map_or(Ok(()), Err)
    }
}

#[derive(Debug, Error)]
pub enum VaultError {
    #[error("secret values must contain between 1 and 2560 bytes")]
    InvalidSecretSize,
    #[error("the credential service needs attention: {0}")]
    Unavailable(String),
    #[error("the platform credential operation failed: {0}")]
    Platform(String),
}
