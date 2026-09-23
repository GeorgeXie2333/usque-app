#[cfg(all(windows, feature = "wireguard"))]
use crate::VaultEndpointPinRefresher;
use crate::{ControlService, ControlServiceError};
#[cfg(all(windows, feature = "wireguard"))]
use std::sync::Arc;
use usque_ipc::v1;

impl ControlService {
    pub(crate) async fn cancel_warp_jobs(&self) -> Result<(), ControlServiceError> {
        #[cfg(all(windows, feature = "wireguard"))]
        {
            let manager = self.warp_scanner.clone();
            if !tokio::task::spawn_blocking(move || manager.stop_blocking())
                .await
                .unwrap_or(false)
            {
                return Err(ControlServiceError::InvalidRequest(
                    "WARP scanner cleanup pending".into(),
                ));
            }
        }
        Ok(())
    }
    pub(crate) async fn warp_wireguard_command(
        &self,
        request: v1::WarpWireguardRequest,
    ) -> Result<v1::WarpWireguardResponse, ControlServiceError> {
        #[cfg(all(windows, feature = "wireguard"))]
        {
            let request = usque_core::warp_wireguard::Request::parse(&request.command_json)
                .map_err(ControlServiceError::configuration)?;
            let _lifecycle = if request.needs_network() {
                Some(self.mutation_lock.clone().try_lock_owned().map_err(|_| {
                    ControlServiceError::InvalidRequest(
                        "WARP scanner unavailable during a connection change".into(),
                    )
                })?)
            } else {
                None
            };
            let context = if request.needs_network() {
                let profile = self.config.read().await.active_profile().ok_or_else(|| {
                    ControlServiceError::InvalidRequest("WARP account required".into())
                })?;
                let existing = self
                    .data_plane
                    .lock()
                    .await
                    .as_ref()
                    .and_then(|r| r.runtime.internal_networks())
                    .map(|(_, warp)| warp);
                let allow_physical = existing.is_none()
                    && crate::windows_agent::catalogue_physical_network_permitted().await;
                let identity = if existing.is_none() && allow_physical {
                    Some(self.load_warp_identity(profile.id).await?)
                } else {
                    None
                };
                let refresher = if identity.is_some() {
                    Some(Arc::new(VaultEndpointPinRefresher {
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
                Some(usque_transport::warp_wireguard::Context {
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
            let manager = self.warp_scanner.clone();
            let response = tokio::task::spawn_blocking(move || manager.command(request, context))
                .await
                .map_err(|_| {
                    ControlServiceError::InvalidRequest("WARP scanner worker failed".into())
                })?;
            let response = response.unwrap_or_else(|e| usque_core::warp_wireguard::Response {
                error: Some(e.reason),
                ..Default::default()
            });
            Ok(v1::WarpWireguardResponse {
                metadata_json: serde_json::to_string(&response)
                    .map_err(ControlServiceError::configuration)?,
            })
        }
        #[cfg(not(all(windows, feature = "wireguard")))]
        {
            let _ = request;
            Err(ControlServiceError::InvalidRequest(
                "WARP WireGuard unavailable".into(),
            ))
        }
    }
}
