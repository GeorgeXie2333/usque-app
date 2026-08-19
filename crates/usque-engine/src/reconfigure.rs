use usque_core::{
    KillSwitchState, LockdownState, Profile, classify_reconfigure, reconfigure::ReconfigureClass,
};
use usque_ipc::v1;

use crate::{ControlService, ControlServiceError, profile_to_proto, snapshot_to_proto};

impl ControlService {
    pub(crate) async fn reconfigure_active_profile(
        &self,
        profile: Profile,
    ) -> Result<v1::ReconfigureResult, ControlServiceError> {
        profile
            .validate()
            .map_err(ControlServiceError::configuration)?;
        let _mutation = self.mutation_lock.lock().await;
        let active_profile_id = self
            .data_plane
            .lock()
            .await
            .as_ref()
            .map(|runtime| runtime.profile_id);
        if active_profile_id != Some(profile.id) {
            return Err(ControlServiceError::InvalidRequest(
                "only the connected Active Profile can be reconfigured".to_owned(),
            ));
        }
        let previous = self
            .config
            .read()
            .await
            .profiles
            .iter()
            .find(|candidate| candidate.id == profile.id)
            .cloned()
            .ok_or(ControlServiceError::ProfileNotFound(profile.id))?;

        let class = classify_reconfigure(&previous, &profile);
        match class {
            ReconfigureClass::Reject => {
                return Err(ControlServiceError::InvalidRequest(
                    "the connected Active Profile cannot be replaced by a different profile"
                        .to_owned(),
                ));
            }
            ReconfigureClass::HotFrontends
            | ReconfigureClass::HotSystemProxy
            | ReconfigureClass::HotTunnelAttach => {
                return self.commit_hot(profile, previous, class).await;
            }
            ReconfigureClass::ColdReconnect => {}
        }

        self.disconnect_locked().await?;
        let profile_id = profile.id;
        let applied = match self.upsert_profile_locked(profile).await {
            Ok(applied) => applied,
            Err(error) => {
                let _ = self.connect_locked(previous.id).await;
                return Err(error);
            }
        };
        let snapshot = match self.connect_locked(profile_id).await {
            Ok(snapshot) => snapshot,
            Err(error) => {
                self.upsert_profile_locked(previous.clone()).await?;
                if let Err(rollback_error) = self.connect_locked(previous.id).await {
                    tracing::error!(%rollback_error, "failed to restore the previous active Profile");
                }
                return Err(error);
            }
        };
        Ok(v1::ReconfigureResult {
            profile: Some(profile_to_proto(&applied)),
            snapshot: Some(snapshot_to_proto(&snapshot)),
        })
    }

    async fn commit_hot(
        &self,
        profile: Profile,
        previous: Profile,
        class: ReconfigureClass,
    ) -> Result<v1::ReconfigureResult, ControlServiceError> {
        let applied = self.upsert_profile_locked(profile).await?;
        let applied_result = match class {
            ReconfigureClass::HotFrontends => self.hot_reconfigure_frontends(&applied).await,
            ReconfigureClass::HotSystemProxy => self.hot_apply_system_proxy(&applied).await,
            ReconfigureClass::HotTunnelAttach => self.hot_tunnel_attach(&applied).await,
            ReconfigureClass::Reject | ReconfigureClass::ColdReconnect => {
                unreachable!("commit_hot is only for in-place classes")
            }
        };
        if let Err(error) = applied_result {
            let detach_committed = class == ReconfigureClass::HotTunnelAttach
                && !applied.frontends.tunnel
                && self
                    .data_plane
                    .lock()
                    .await
                    .as_ref()
                    .is_some_and(|active| !active.runtime.is_vpn());
            if detach_committed {
                self.apply_hot_profile_state(&applied).await;
            } else {
                let _ = self.upsert_profile_locked(previous).await;
            }
            return Err(error);
        }
        self.apply_hot_profile_state(&applied).await;
        let snapshot = self.status_snapshot().await;
        Ok(v1::ReconfigureResult {
            profile: Some(profile_to_proto(&applied)),
            snapshot: Some(snapshot_to_proto(&snapshot)),
        })
    }

    async fn hot_reconfigure_frontends(
        &self,
        profile: &Profile,
    ) -> Result<(), ControlServiceError> {
        let mut data_plane = self.data_plane.lock().await;
        let Some(active) = data_plane.as_mut() else {
            return Err(ControlServiceError::InvalidRequest(
                "a connected session is required".to_owned(),
            ));
        };
        active.runtime.reconfigure_frontends(profile).await?;
        active.frontends = profile.frontends;
        Ok(())
    }

    async fn hot_apply_system_proxy(&self, profile: &Profile) -> Result<(), ControlServiceError> {
        let mut data_plane = self.data_plane.lock().await;
        let Some(active) = data_plane.as_mut() else {
            return Err(ControlServiceError::InvalidRequest(
                "a connected session is required".to_owned(),
            ));
        };
        active.runtime.apply_system_proxy(profile).await
    }

    async fn hot_tunnel_attach(&self, profile: &Profile) -> Result<(), ControlServiceError> {
        let mut data_plane = self.data_plane.lock().await;
        let Some(mut active) = data_plane.take() else {
            return Err(ControlServiceError::InvalidRequest(
                "a connected session is required".to_owned(),
            ));
        };
        match active.runtime.with_tunnel(profile).await {
            Ok(runtime) => {
                active.runtime = runtime;
                active.frontends = profile.frontends;
                *data_plane = Some(active);
                Ok(())
            }
            Err((runtime, error)) => {
                let detached = !profile.frontends.tunnel && !runtime.is_vpn();
                active.runtime = runtime;
                if detached {
                    active.frontends.tunnel = false;
                }
                *data_plane = Some(active);
                Err(error)
            }
        }
    }

    pub(crate) async fn apply_hot_profile_state(&self, profile: &Profile) {
        let data_plane = self.data_plane.lock().await;
        let mut state = self.state.lock().await;
        let Some(active) = data_plane.as_ref() else {
            return;
        };
        let warnings = state.snapshot().warnings.clone();
        state.update_runtime_metadata(
            active.runtime.health().reconnect_count(),
            active
                .runtime
                .listeners()
                .iter()
                .map(ToString::to_string)
                .collect(),
            warnings,
        );
        state.update_frontends(active.runtime.frontend_statuses(active.frontends));
        state.update_safety_state(
            if profile.frontends.tunnel && active.runtime.is_vpn() {
                if profile.kill_switch {
                    KillSwitchState::Active
                } else {
                    KillSwitchState::Inactive
                }
            } else {
                KillSwitchState::NotApplicable
            },
            LockdownState::NotSupported,
        );
    }
}
