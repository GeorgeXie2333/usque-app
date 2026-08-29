use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::DEFAULT_PROFILE_ID;

/// Persisted WARP account. Network settings live on [`super::AppConfig::network`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Account {
    pub id: Uuid,
    pub name: String,
}

impl Account {
    pub fn new(id: Uuid, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
        }
    }

    pub fn default_account() -> Self {
        Self::new(DEFAULT_PROFILE_ID, "Default")
    }
}
