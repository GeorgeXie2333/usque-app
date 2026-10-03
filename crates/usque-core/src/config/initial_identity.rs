use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{AppConfig, ConfigError};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InitialIdentityPhase {
    Pending,
    Interrupted,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct InitialIdentityOperation {
    pub operation_id: Uuid,
    pub profile_id: Uuid,
    pub method: String,
    pub organization: Option<String>,
    pub owner_epoch: Uuid,
    pub phase: InitialIdentityPhase,
    pub error_code: Option<String>,
}

impl InitialIdentityOperation {
    pub fn new(
        operation_id: Uuid,
        profile_id: Uuid,
        method: &str,
        organization: Option<String>,
        owner_epoch: Uuid,
    ) -> Result<Self, ConfigError> {
        let operation = Self {
            operation_id,
            profile_id,
            method: method.to_owned(),
            organization,
            owner_epoch,
            phase: InitialIdentityPhase::Pending,
            error_code: None,
        };
        operation.validate()?;
        Ok(operation)
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.operation_id.is_nil()
            || self.profile_id.is_nil()
            || self.owner_epoch.is_nil()
            || !matches!(
                self.method.as_str(),
                "register" | "registerWithLicense" | "zeroTrust"
            )
            || (self.method == "zeroTrust") != self.organization.is_some()
            || self.organization.as_ref().is_some_and(|team| {
                crate::registration::normalize_zero_trust_team(team)
                    .ok()
                    .as_ref()
                    != Some(team)
            })
            || self
                .error_code
                .as_deref()
                .is_some_and(|code| !initial_error_code_allowed(code))
        {
            return Err(ConfigError::InvalidInitialIdentityOperation);
        }
        Ok(())
    }
}

pub fn initial_error_code_allowed(code: &str) -> bool {
    matches!(
        code,
        "REGISTRATION_FAILED"
            | "INITIAL_IDENTITY_INTERRUPTED"
            | "INITIAL_IDENTITY_REPAIR_REQUIRED"
            | "INITIAL_IDENTITY_SUPERSEDED"
            | "INVALID_LICENSE_KEY"
            | "ZERO_TRUST_LOGIN_REQUIRED"
            | "ZERO_TRUST_REGISTRATION_FAILED"
            | "INITIAL_IDENTITY_COMMIT_FAILED"
    )
}

impl AppConfig {
    /// Caller owns the initial-execution lease before reserving an operation.
    pub fn begin_initial_identity(
        &mut self,
        operation: InitialIdentityOperation,
    ) -> Result<bool, ConfigError> {
        operation.validate()?;
        if self.active_profile_id != Some(operation.profile_id)
            || self.account(operation.profile_id).is_none()
            || self.identity_bindings.contains_key(&operation.profile_id)
            || self
                .pending_identity_replacements
                .contains_key(&operation.profile_id)
        {
            return Err(ConfigError::InvalidInitialIdentityOperation);
        }
        if let Some(existing) = &self.initial_identity_operation {
            if existing.operation_id == operation.operation_id {
                return Ok(false);
            }
            if existing.phase == InitialIdentityPhase::Pending {
                return Err(ConfigError::InvalidInitialIdentityOperation);
            }
        }
        self.initial_identity_operation = Some(operation);
        Ok(true)
    }

    pub fn finish_initial_identity(
        &mut self,
        operation_id: Uuid,
        profile_id: Uuid,
        phase: InitialIdentityPhase,
        error_code: Option<String>,
    ) -> Result<(), ConfigError> {
        if phase == InitialIdentityPhase::Pending || self.active_profile_id != Some(profile_id) {
            return Err(ConfigError::InvalidInitialIdentityOperation);
        }
        let operation = self
            .initial_identity_operation
            .as_mut()
            .filter(|operation| {
                operation.operation_id == operation_id
                    && operation.profile_id == profile_id
                    && operation.phase == InitialIdentityPhase::Pending
            })
            .ok_or(ConfigError::InvalidInitialIdentityOperation)?;
        operation.phase = phase;
        operation.error_code = error_code;
        operation.validate()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_intent_is_fixed_and_never_replayed_while_pending() {
        let mut config = AppConfig::default();
        let profile = config.profiles[0].id;
        let operation = InitialIdentityOperation::new(
            Uuid::new_v4(),
            profile,
            "register",
            None,
            Uuid::new_v4(),
        )
        .unwrap();
        assert!(config.begin_initial_identity(operation.clone()).unwrap());
        assert!(!config.begin_initial_identity(operation.clone()).unwrap());
        let next = InitialIdentityOperation::new(
            Uuid::new_v4(),
            profile,
            "register",
            None,
            Uuid::new_v4(),
        )
        .unwrap();
        assert!(config.begin_initial_identity(next.clone()).is_err());
        assert!(
            config
                .finish_initial_identity(
                    next.operation_id,
                    profile,
                    InitialIdentityPhase::Completed,
                    None
                )
                .is_err()
        );
        config
            .finish_initial_identity(
                operation.operation_id,
                profile,
                InitialIdentityPhase::Interrupted,
                Some("INITIAL_IDENTITY_INTERRUPTED".into()),
            )
            .unwrap();
        assert!(config.begin_initial_identity(next).unwrap());
        assert!(
            !serde_json::to_string(&config)
                .unwrap()
                .contains("license_key")
        );
    }
}
