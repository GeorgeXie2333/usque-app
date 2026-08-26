use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::DEFAULT_PROFILE_ID;
use super::EndpointSettings;

/// Persisted WARP account. Network settings live on [`super::AppConfig::network`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Account {
    pub id: Uuid,
    pub name: String,
    /// Zero Trust ingress only. Consumer accounts always use shared network endpoint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub managed_endpoint: Option<EndpointSettings>,
}

impl Account {
    pub fn new(id: Uuid, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            managed_endpoint: None,
        }
    }

    pub fn default_account() -> Self {
        Self::new(DEFAULT_PROFILE_ID, "Default")
    }
}
