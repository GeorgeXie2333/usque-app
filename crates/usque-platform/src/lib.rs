use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::Mutex;
use uuid::Uuid;
use zeroize::Zeroizing;

pub mod packet_ring;

#[cfg(windows)]
pub mod windows_vault;

#[cfg(target_os = "macos")]
pub mod macos_keychain;

#[cfg(target_os = "macos")]
pub use macos_keychain::MacOsKeychainVault;

#[cfg(windows)]
pub use windows_vault::WindowsCredentialVault;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlatformCapabilities {
    pub tun: bool,
    pub kill_switch: bool,
    pub system_proxy: bool,
    pub secure_storage: bool,
    pub captive_portal_pause: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TunnelPlan {
    pub profile_id: Uuid,
    pub endpoint: SocketAddr,
    pub mtu: u16,
    pub dns_servers: Vec<IpAddr>,
    pub split_exclusions: Vec<String>,
    pub allow_lan: bool,
    pub kill_switch: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct AppliedPlatformState {
    pub plan: Option<TunnelPlan>,
    pub captive_portal_pause_deadline_unix_seconds: Option<i64>,
}

#[async_trait]
pub trait PlatformAgent: Send + Sync {
    fn capabilities(&self) -> PlatformCapabilities;
    async fn prepare(&self, plan: TunnelPlan) -> Result<(), AgentError>;
    async fn commit(&self) -> Result<(), AgentError>;
    async fn rollback(&self) -> Result<(), AgentError>;
    async fn pause_for_captive_portal(&self, seconds: u32) -> Result<(), AgentError>;
    async fn state(&self) -> Result<AppliedPlatformState, AgentError>;
}

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
}

impl SecretRecord {
    pub const ALL: [Self; 8] = [
        Self::WarpSecret,
        Self::MasquePrivateKey,
        Self::AccessToken,
        Self::DeviceId,
        Self::License,
        Self::EndpointPin,
        Self::AssignedIpv4,
        Self::AssignedIpv6,
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
            if let Err(error) = self.delete(profile_id, record).await
                && first_error.is_none()
            {
                first_error = Some(error);
            }
        }
        first_error.map_or(Ok(()), Err)
    }
}

/// Deterministic agent used by core integration tests. Production platform
/// crates must replace it before a release build is accepted.
#[derive(Debug, Clone, Default)]
pub struct MockPlatformAgent {
    state: Arc<Mutex<AppliedPlatformState>>,
}

#[async_trait]
impl PlatformAgent for MockPlatformAgent {
    fn capabilities(&self) -> PlatformCapabilities {
        PlatformCapabilities {
            tun: true,
            kill_switch: true,
            system_proxy: true,
            secure_storage: false,
            captive_portal_pause: true,
        }
    }

    async fn prepare(&self, plan: TunnelPlan) -> Result<(), AgentError> {
        self.state.lock().await.plan = Some(plan);
        Ok(())
    }

    async fn commit(&self) -> Result<(), AgentError> {
        if self.state.lock().await.plan.is_none() {
            return Err(AgentError::InvalidOrder(
                "prepare must run before commit".to_owned(),
            ));
        }
        Ok(())
    }

    async fn rollback(&self) -> Result<(), AgentError> {
        *self.state.lock().await = AppliedPlatformState::default();
        Ok(())
    }

    async fn pause_for_captive_portal(&self, seconds: u32) -> Result<(), AgentError> {
        if seconds == 0 || seconds > 600 {
            return Err(AgentError::InvalidPause(seconds));
        }
        self.state
            .lock()
            .await
            .captive_portal_pause_deadline_unix_seconds =
            Some(chrono_like_unix_now() + i64::from(seconds));
        Ok(())
    }

    async fn state(&self) -> Result<AppliedPlatformState, AgentError> {
        Ok(self.state.lock().await.clone())
    }
}

fn chrono_like_unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[derive(Debug, Error)]
pub enum AgentError {
    #[error("platform operation was called in an invalid order: {0}")]
    InvalidOrder(String),
    #[error("captive portal pause must be between 1 and 600 seconds, got {0}")]
    InvalidPause(u32),
    #[error("platform permission was denied: {0}")]
    PermissionDenied(String),
    #[error("platform operation failed: {0}")]
    Operation(String),
}

#[derive(Debug, Error)]
pub enum VaultError {
    #[error("secret values must contain between 1 and 2560 bytes")]
    InvalidSecretSize,
    #[error("the platform credential operation failed: {0}")]
    Platform(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn captive_portal_pause_is_capped_at_ten_minutes() {
        let agent = MockPlatformAgent::default();
        assert!(agent.pause_for_captive_portal(600).await.is_ok());
        assert!(matches!(
            agent.pause_for_captive_portal(601).await,
            Err(AgentError::InvalidPause(601))
        ));
    }
}
