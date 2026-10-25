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
            let network = Arc::new(AccountNetworkProvider {
                service: self.clone(),
                selected: self.config.read().await.active_profile().map(|p| p.id),
            });
            let manager = self.proton_manager.clone();
            let result = tokio::task::spawn_blocking(move || {
                manager.command_with_network(
                    request,
                    input,
                    provider_lease,
                    usque_transport::proton_vpn::ClientConfig::default(),
                    (accounts, Some(network)),
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

#[cfg(all(any(windows, target_os = "linux"), feature = "wireguard"))]
struct AccountNetworkProvider {
    service: ControlService,
    selected: Option<uuid::Uuid>,
}
#[cfg(all(any(windows, target_os = "linux"), feature = "wireguard"))]
#[async_trait::async_trait]
impl usque_transport::proton_vpn::AccountWarpProvider for AccountNetworkProvider {
    async fn context(
        &self,
        cancel: &tokio_util::sync::CancellationToken,
    ) -> Result<usque_transport::warp_wireguard::Context, usque_core::chain_exit::ImportError> {
        use usque_core::proton_vpn::error;
        let service = &self.service;
        if cancel.is_cancelled() {
            return Err(error("cancelled"));
        }
        let profile = service
            .config
            .read()
            .await
            .active_profile()
            .ok_or_else(|| error("warp_identity_required"))?;
        if Some(profile.id) != self.selected {
            return Err(error("underlay_changed"));
        }
        let existing = service
            .data_plane
            .lock()
            .await
            .as_ref()
            .and_then(|r| r.runtime.internal_networks())
            .map(|(_, warp)| warp)
            .filter(|network| {
                matches!(
                    network.health_snapshot(),
                    usque_transport::RuntimeHealth::Connected { .. }
                )
            });
        #[cfg(windows)]
        let allow_physical = existing.is_none()
            && crate::windows_agent::catalogue_physical_network_permitted().await;
        #[cfg(all(target_os = "linux", not(test)))]
        let allow_physical =
            existing.is_none() && crate::linux_observation::disconnected_probe_safe().await;
        #[cfg(all(target_os = "linux", test))]
        let allow_physical = existing.is_none();
        let identity = if existing.is_none() && allow_physical {
            Some(
                service
                    .load_warp_identity(profile.id)
                    .await
                    .map_err(|_| error("warp_identity_required"))?,
            )
        } else {
            None
        };
        let refresh_identity = if identity.is_some() {
            Some(
                service
                    .load_warp_identity(profile.id)
                    .await
                    .map_err(|_| error("warp_identity_required"))?,
            )
        } else {
            None
        };
        let refresher = refresh_identity.map(|identity| {
            Arc::new(crate::VaultEndpointPinRefresher {
                profile_id: profile.id,
                vault: service.vault.clone(),
                identity: tokio::sync::Mutex::new(identity),
            }) as Arc<dyn usque_transport::EndpointPinRefresher>
        });
        Ok(usque_transport::warp_wireguard::Context {
            profile,
            existing,
            identity,
            allow_physical,
            refresher,
            lifecycle_lease: None,
            protector: Arc::new(usque_transport::NoopSocketProtector),
        })
    }
}
