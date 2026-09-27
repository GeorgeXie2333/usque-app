//! Commands and process-local status for WARP WireGuard configuration generation.
use crate::chain_exit::ImportError;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub action: String,
    #[serde(default)]
    pub job_id: Option<Uuid>,
    #[serde(default)]
    pub name: String,
}
impl Request {
    pub fn parse(text: &str) -> Result<Self, ImportError> {
        if text.len() > 4096 {
            return Err(error("invalid_request"));
        }
        let value: Self = serde_json::from_str(text).map_err(|_| error("invalid_request"))?;
        value.validate()?;
        Ok(value)
    }
    pub fn validate(&self) -> Result<(), ImportError> {
        if !matches!(self.action.as_str(), "generate" | "cancel" | "get")
            || self.name.chars().count() > 64
            || self.name.chars().any(char::is_control)
            || (self.action == "cancel" && self.job_id.is_none())
        {
            return Err(error("invalid_request"));
        }
        Ok(())
    }
    pub fn needs_network(&self) -> bool {
        self.action == "generate"
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Job {
    pub id: Uuid,
    pub state: String,
    pub failure: Option<String>,
    pub profile_id: Option<Uuid>,
}
impl Job {
    pub fn generation() -> Self {
        Self {
            id: Uuid::new_v4(),
            state: "running".into(),
            failure: None,
            profile_id: None,
        }
    }
}
#[derive(Default, Serialize)]
pub struct Response {
    pub job: Option<Job>,
    pub error: Option<String>,
}

pub fn error(reason: &str) -> ImportError {
    ImportError {
        line: 0,
        field: "warp_wireguard".into(),
        reason: reason.into(),
    }
}
pub fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}
