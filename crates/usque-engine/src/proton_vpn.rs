use crate::{ControlService, ControlServiceError};
#[cfg(all(any(windows, target_os = "linux"), feature = "wireguard"))]
use std::sync::Arc;
use usque_ipc::v1;

impl ControlService {
    pub(crate) async fn cancel_proton_jobs(&self) -> Result<(), ControlServiceError> {
        #[cfg(all(any(windows, target_os = "linux"), feature = "wireguard"))]
        {
            let manager = self.proton_manager.clone();
            if !tokio::task::spawn_blocking(move || manager.stop_blocking())
                .await
                .unwrap_or(false)
            {
                return Err(ControlServiceError::InvalidRequest(
                    "Proton operation cleanup pending".into(),
                ));
            }
        }
        Ok(())
    }
    pub(crate) async fn proton_vpn_command(
        &self,
        mut request: v1::ProtonVpnRequest,
    ) -> Result<v1::ProtonVpnResponse, ControlServiceError> {
        let input = zeroize::Zeroizing::new(std::mem::take(&mut request.sensitive_input));
        #[cfg(all(any(windows, target_os = "linux"), feature = "wireguard"))]
        {
            let request = usque_core::proton_vpn::Request::parse(&request.command_json)
                .map_err(ControlServiceError::configuration)?;
            let provider_lease = if request.mutates_account() {
                Some(Arc::new(
                    usque_core::proton_vpn::lifecycle_lease(&self.profile_dir)
                        .map_err(ControlServiceError::configuration)?,
                ))
            } else {
                None
            };
            let _lifecycle = if request.mutates_account() {
                Some(self.mutation_lock.clone().try_lock_owned().map_err(|_| {
                    ControlServiceError::InvalidRequest(
                        "Proton operation unavailable during a connection change".into(),
                    )
                })?)
            } else {
                None
            };
            let accounts = usque_transport::proton_vpn::AccountContext {
                selected: self
                    .config
                    .read()
                    .await
                    .network
                    .chain_exit
                    .as_ref()
                    .and_then(|settings| settings.profile_id),
                active: self
                    .gate_status
                    .borrow()
                    .current_profile
                    .as_ref()
                    .map(|profile| profile.id)
                    .into_iter()
                    .collect(),
            };
            if matches!(request.action.as_str(), "logout" | "refresh" | "forget") {
                let selected = self
                    .config
                    .read()
                    .await
                    .network
                    .chain_exit
                    .as_ref()
                    .and_then(|s| s.profile_id);
                let active = self
                    .gate_status
                    .borrow()
                    .current_profile
                    .as_ref()
                    .map(|p| p.id);
                if request.profile_id.is_some()
                    && (active == request.profile_id
                        || matches!(request.action.as_str(), "logout" | "forget")
                            && selected == request.profile_id)
                {
                    return Ok(v1::ProtonVpnResponse {
                        metadata_json: "{\"error\":\"profile_in_use\",\"profiles\":[]}".into(),
                        sensitive_handoff: Vec::new(),
                    });
                }
            }
            let context = if request.needs_network() {
                let warp = self.warp_generator.clone();
                if !tokio::task::spawn_blocking(move || warp.stop_blocking())
                    .await
                    .unwrap_or(false)
                {
                    return Err(ControlServiceError::InvalidRequest(
                        "WARP operation cleanup pending".into(),
                    ));
                }
                let profile = self.config.read().await.active_profile().ok_or_else(|| {
                    ControlServiceError::InvalidRequest("WARP account required".into())
                })?;
                let existing = {
                    let active = self.data_plane.lock().await;
                    if let Some(active) = active.as_ref() {
                        if active.profile_id != profile.id {
                            return Err(ControlServiceError::InvalidRequest(
                                "WARP underlay account changed; reconnect before continuing".into(),
                            ));
                        }
                        active.runtime.internal_networks().map(|(_, warp)| warp)
                    } else {
                        None
                    }
                };
                #[cfg(windows)]
                let allow_physical = existing.is_none()
                    && crate::windows_agent::catalogue_physical_network_permitted().await;
                #[cfg(all(target_os = "linux", not(test)))]
                let allow_physical =
                    existing.is_none() && crate::linux_observation::disconnected_probe_safe().await;
                #[cfg(all(target_os = "linux", test))]
                let allow_physical = existing.is_none();
                let identity = if existing.is_none() && allow_physical {
                    Some(self.load_warp_identity(profile.id).await?)
                } else {
                    None
                };
                let refresher = if identity.is_some() {
                    Some(Arc::new(crate::VaultEndpointPinRefresher {
                        profile_id: profile.id,
                        vault: self.vault.clone(),
                        identity: tokio::sync::Mutex::new(
                            self.load_warp_identity(profile.id).await?,
                        ),
                    })
                        as Arc<dyn usque_transport::EndpointPinRefresher>)
                } else {
                    None
                };
                Some(usque_transport::proton_vpn::Context {
                    lifecycle_lease: provider_lease.clone(),
                    profile,
                    existing,
                    identity,
                    allow_physical,
                    refresher,
                    protector: Arc::new(usque_transport::NoopSocketProtector),
                })
            } else {
                None
            };
            let manager = self.proton_manager.clone();
            let result = tokio::task::spawn_blocking(move || {
                let _provider_lease = provider_lease;
                manager.command_scoped(
                    request,
                    input,
                    context,
                    usque_transport::proton_vpn::ClientConfig::default(),
                    accounts,
                )
            })
            .await
            .map_err(|_| ControlServiceError::InvalidRequest("Proton worker failed".into()))?;
            match result {
                Ok(value) => Ok(v1::ProtonVpnResponse {
                    metadata_json: serde_json::to_string(&value.metadata)
                        .map_err(ControlServiceError::configuration)?,
                    sensitive_handoff: value.sensitive_handoff.to_vec(),
                }),
                Err(failure) => Ok(v1::ProtonVpnResponse {
                    metadata_json: serde_json::to_string(&usque_core::proton_vpn::Response {
                        error: Some(failure.reason),
                        ..Default::default()
                    })
                    .map_err(ControlServiceError::configuration)?,
                    sensitive_handoff: Vec::new(),
                }),
            }
        }
        #[cfg(not(all(any(windows, target_os = "linux"), feature = "wireguard")))]
        {
            let _ = input;
            Err(ControlServiceError::FeatureUnavailable(
                "Proton VPN unavailable",
            ))
        }
    }
}
