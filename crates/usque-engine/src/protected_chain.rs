//! Failure retention for the applied HTTP/SOCKS VPN session. Saved drafts do
//! not authorize releasing an already-installed platform protection policy.
use crate::{ActiveDataPlane, ActiveRuntime, ControlService, ControlServiceError};
use std::time::Instant;
use usque_core::vpngate::{GateFailure, GateStage, GateStatus};
use usque_core::{KillSwitchState, LockdownState, Profile};
use usque_transport::ProxyPerformanceSnapshot;

pub(crate) fn requires_failure_retention(profile: &Profile) -> bool {
    profile.kill_switch && is_proxy_vpn(profile)
}

fn is_proxy_vpn(profile: &Profile) -> bool {
    profile.frontends.tunnel
        && profile
            .custom_chain()
            .is_some_and(|selection| selection.enabled && selection.source.is_proxy())
}

fn can_handoff(active: &ActiveDataPlane) -> bool {
    active.runtime.is_vpn()
        && (is_proxy_vpn(&active.profile)
            || active.runtime.replacement_pending()
            || active.profile.kill_switch && active.runtime.target_committed())
}

fn retains_protection(active: &ActiveDataPlane) -> bool {
    active.runtime.is_vpn()
        && (requires_failure_retention(&active.profile)
            || active.runtime.replacement_pending()
            || active.profile.kill_switch && active.runtime.target_committed())
}

impl ControlService {
    pub(crate) async fn protected_chain_present(&self) -> bool {
        self.data_plane
            .lock()
            .await
            .as_ref()
            .is_some_and(can_handoff)
    }

    #[cfg(windows)]
    pub(crate) async fn retry_protected_target(
        &self,
        profile_id: uuid::Uuid,
        cancellation: &tokio_util::sync::CancellationToken,
        retry_connected: bool,
    ) -> Result<Option<usque_core::ConnectionSnapshot>, ControlServiceError> {
        let eligible = self.data_plane.lock().await.as_ref().is_some_and(|active| {
            can_handoff(active)
                && (retry_connected
                    || active.runtime.failure_retained()
                    || active.profile_id != profile_id)
        });
        if !eligible {
            return Ok(None);
        }
        let mut target = self
            .config
            .read()
            .await
            .runtime_profile(profile_id)
            .ok_or(ControlServiceError::ProfileNotFound(profile_id))?;
        self.attach_proxy_auth(&mut target).await?;
        if !target.frontends.tunnel {
            // Selecting proxy-only explicitly removes VPN capture. Complete
            // the owned guard cleanup before admitting the selected frontend.
            target
                .validate()
                .map_err(ControlServiceError::profile_configuration)?;
            if cancellation.is_cancelled() {
                return Ok(Some(self.state.lock().await.snapshot().clone()));
            }
            self.disconnect_locked().await?;
            self.await_disconnect_cleanup().await?;
            *self.session_profile.lock().await = None;
            *self.session_congestion_control.lock().await = None;
            return Box::pin(self.connect_with_cancellation_locked(
                profile_id,
                cancellation.clone(),
                false,
            ))
            .await
            .map(Some);
        }
        Box::pin(self.reconnect_protected_chain(&target, cancellation)).await
    }

    /// Called with the lifecycle lock. Returns None only when this operation is
    /// outside the existing protected HTTP/SOCKS VPN boundary.
    #[cfg(windows)]
    pub(crate) async fn reconnect_protected_chain(
        &self,
        target: &Profile,
        cancellation: &tokio_util::sync::CancellationToken,
    ) -> Result<Option<usque_core::ConnectionSnapshot>, ControlServiceError> {
        let previous = self
            .data_plane
            .lock()
            .await
            .as_ref()
            .filter(|active| can_handoff(active) && target.frontends.tunnel)
            .map(|active| active.profile.clone());
        let Some(previous) = previous else {
            return Ok(None);
        };
        target
            .validate()
            .map_err(ControlServiceError::profile_configuration)?;
        if cancellation.is_cancelled() {
            return Err(ControlServiceError::Transport(
                usque_transport::TransportError::TunnelClosed,
            ));
        }
        #[cfg(test)]
        if self
            .data_plane
            .lock()
            .await
            .as_ref()
            .is_some_and(|active| matches!(active.runtime, ActiveRuntime::Harness(_)))
        {
            let mut active = self
                .data_plane
                .lock()
                .await
                .take()
                .expect("serialized lifecycle");
            let ActiveRuntime::Harness(runtime) = &mut active.runtime else {
                unreachable!()
            };
            let result = if runtime.fail_protected_reconnect {
                Err(ControlServiceError::PlatformVpn(
                    "injected protected replacement failure".into(),
                ))
            } else {
                runtime.replace_gate(target);
                runtime.reconnect_count += 1;
                Ok(())
            };
            return self
                .finish_protected_reconnect(active, target, result)
                .await
                .map(Some);
        }
        let warp_identity = self.load_warp_identity(target.id).await?;
        let identity = usque_transport::MasqueTlsIdentity::from_warp_identity(&warp_identity)?;
        let refresher = std::sync::Arc::new(crate::VaultEndpointPinRefresher {
            profile_id: target.id,
            vault: std::sync::Arc::clone(&self.vault),
            identity: tokio::sync::Mutex::new(warp_identity),
        });
        let selected = self.prepare_gate_selection(target)?;
        let policy = std::sync::Arc::new(crate::load_geo_direct_policy(target, &self.cache_dir)?);
        let ads_revision = policy.ads_revision().unwrap_or_default().to_owned();
        if !target.geo_direct_countries.is_empty() && !policy.is_enabled() {
            return Err(ControlServiceError::GeoRules(
                "the configured direct-rule cache is missing or invalid".into(),
            ));
        }
        self.abort_exit_probe().await;
        self.clear_network_quality_source().await;
        {
            let mut state = self.state.lock().await;
            if state.snapshot().phase != usque_core::ConnectionPhase::Reconnecting {
                state.transition(usque_core::ConnectionPhase::Reconnecting)?;
            }
        }
        let mut active = self
            .data_plane
            .lock()
            .await
            .take()
            .expect("serialized protected session");
        let ActiveRuntime::Vpn(runtime) = &mut active.runtime else {
            unreachable!("non-test Windows VPN")
        };
        let class = usque_core::classify_reconfigure(&previous, target);
        let same_operation = previous.id == target.id
            && !runtime.replacement_pending()
            && matches!(
                class,
                usque_core::ReconfigureClass::PersistOnly
                    | usque_core::ReconfigureClass::HotVpnGate
            );
        let result = if same_operation {
            runtime
                .retry_protected_chain(
                    target,
                    identity,
                    refresher,
                    policy,
                    selected,
                    self.gate_status.clone(),
                    cancellation,
                    &self.windows_device,
                )
                .await
        } else {
            runtime
                .replace_protected_connection(
                    target,
                    identity,
                    refresher,
                    policy,
                    selected,
                    self.gate_status.clone(),
                    cancellation,
                    &self.windows_device,
                )
                .await
        }
        .map_err(crate::map_windows_vpn_error);
        let mut snapshot = self
            .finish_protected_reconnect(active, target, result)
            .await?;
        self.state
            .lock()
            .await
            .update_ads_revision(ads_revision.clone());
        snapshot.ads_rule_revision = ads_revision;
        Ok(Some(snapshot))
    }

    #[cfg(windows)]
    async fn finish_protected_reconnect(
        &self,
        mut active: ActiveDataPlane,
        target: &Profile,
        result: Result<(), ControlServiceError>,
    ) -> Result<usque_core::ConnectionSnapshot, ControlServiceError> {
        if let Err(error) = result {
            if active.runtime.target_committed() {
                active.profile = target.clone();
                active.profile_id = target.id;
                active.frontends = target.frontends;
            }
            active.runtime.fail_gate(GateFailure::Transport).await;
            let mut status = active.runtime.gate_status();
            status.stage = GateStage::Error;
            status.failure.get_or_insert(GateFailure::Transport);
            status.network = None;
            *self.data_plane.lock().await = Some(active);
            self.stop_gate_connection_locked(status.clone(), &error)
                .await?;
            self.gate_status.send_replace(status);
            self.mark_connection_error(&error).await;
            return Err(error);
        }
        let generation = self.next_session_generation();
        active.profile_id = target.id;
        active.profile = target.clone();
        active.session_generation = generation;
        active.frontends = target.frontends;
        active.connected_at = Instant::now();
        active.last_sample_at = Instant::now();
        active.last_bytes_sent = 0;
        active.last_bytes_received = 0;
        active.last_proxy_performance = ProxyPerformanceSnapshot::default();
        let path = active.runtime.path();
        let status = active.runtime.gate_status();
        let quality = active.runtime.subscribe_network_quality();
        let frontends = active.runtime.frontend_statuses(target.frontends);
        let l4 = active.runtime.l4_snapshot();
        let reconnect_count = active.runtime.health().reconnect_count();
        let listeners = active
            .runtime
            .listeners()
            .iter()
            .map(ToString::to_string)
            .collect();
        *self.data_plane.lock().await = Some(active);
        let snapshot = {
            let mut state = self.state.lock().await;
            // Harness and retries that failed before an earlier phase change
            // still enter the same connected transition as the native path.
            if state.snapshot().phase != usque_core::ConnectionPhase::Reconnecting {
                state.transition(usque_core::ConnectionPhase::Reconnecting)?;
            }
            state.clear_exit_info();
            state.update_data_plane(target.data_plane, l4);
            state.mark_connected_with_gate(
                path.transport,
                path.endpoint_family,
                path.ipv4_available,
                path.ipv6_available,
                Some(&status),
            )?;
            state.update_frontends(frontends);
            let warnings = state.snapshot().warnings.clone();
            state.update_runtime_metadata(reconnect_count, listeners, warnings);
            state.update_safety_state(
                if target.kill_switch {
                    KillSwitchState::Active
                } else {
                    KillSwitchState::Inactive
                },
                LockdownState::NotSupported,
            );
            state.snapshot().clone()
        };
        self.gate_status.send_replace(status);
        *self.session_profile.lock().await = Some(target.clone());
        *self.session_congestion_control.lock().await =
            Some((target.id, target.congestion_control));
        self.install_network_quality_source(quality).await;
        self.publish_settings_runtime(Some(target.clone()), Some(generation))
            .await;
        self.spawn_gate_exit_probe(target.id, generation).await;
        Ok(snapshot)
    }

    pub(crate) async fn retain_failed_proxy_chain(
        &self,
        mut status: GateStatus,
        error: &ControlServiceError,
    ) -> bool {
        let protection = {
            let mut active = self.data_plane.lock().await;
            let Some(active) = active.as_mut().filter(|active| retains_protection(active)) else {
                return false;
            };
            active
                .runtime
                .retain_failed_gate(status.failure.unwrap_or(GateFailure::Transport))
                .await
        };
        self.abort_exit_probe().await;
        self.clear_network_quality_source().await;
        self.cancel_gate_refresh().await;
        #[cfg(windows)]
        self.clear_windows_connection_intent().await;
        status.stage = GateStage::Error;
        status.network = None;
        self.gate_status.send_replace(status);
        self.mark_connection_error(error).await;
        self.state.lock().await.update_safety_state(
            if protection.is_ok() {
                KillSwitchState::Active
            } else {
                KillSwitchState::Error
            },
            LockdownState::NotSupported,
        );
        true
    }

    pub(crate) async fn retain_failed_startup(
        &self,
        profile: &Profile,
        runtime: ActiveRuntime,
        error: &ControlServiceError,
    ) {
        let status = runtime.gate_status();
        let generation = self.next_session_generation();
        let now = Instant::now();
        self.state
            .lock()
            .await
            .update_frontends(runtime.frontend_statuses(profile.frontends));
        *self.session_profile.lock().await = Some(profile.clone());
        *self.data_plane.lock().await = Some(ActiveDataPlane {
            profile_id: profile.id,
            profile: profile.clone(),
            session_generation: generation,
            frontends: profile.frontends,
            connected_at: now,
            last_sample_at: now,
            last_bytes_sent: 0,
            last_bytes_received: 0,
            last_proxy_performance: ProxyPerformanceSnapshot::default(),
            runtime,
        });
        self.retain_failed_proxy_chain(status, error).await;
        // A retained bootstrap proves protection ownership, not that the final
        // addresses, DNS or listeners were successfully applied.
        self.publish_settings_runtime(None, None).await;
    }
}
