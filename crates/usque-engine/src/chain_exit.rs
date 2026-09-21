use crate::{ControlService, ControlServiceError};
use usque_core::chain_exit::{ChainExitSettings, ChainSource};
#[cfg(windows)]
use usque_core::chain_exit::{ChainProfileRequest, ImportSecrets};
use usque_ipc::v1;

pub(crate) fn settings_from_proto(
    value: v1::ChainExitSettings,
) -> Result<ChainExitSettings, ControlServiceError> {
    let source = source(&value.source)?;
    let settings = ChainExitSettings {
        enabled: value.enabled,
        source,
        profile_id: uuid(&value.profile_id)?,
        revision: uuid(&value.revision)?,
    };
    settings
        .validate()
        .map_err(ControlServiceError::configuration)?;
    Ok(settings)
}
pub(crate) fn settings_to_proto(value: &ChainExitSettings) -> v1::ChainExitSettings {
    v1::ChainExitSettings {
        enabled: value.enabled,
        source: source_name(value.source).into(),
        profile_id: value
            .profile_id
            .map(|id| id.to_string())
            .unwrap_or_default(),
        revision: value.revision.map(|id| id.to_string()).unwrap_or_default(),
    }
}
fn source(value: &str) -> Result<ChainSource, ControlServiceError> {
    match value {
        "openvpn_custom" | "" => Ok(ChainSource::OpenvpnCustom),
        "wireguard_custom" => Ok(ChainSource::WireguardCustom),
        "vpn_gate" => Ok(ChainSource::VpnGate),
        _ => Err(ControlServiceError::InvalidRequest(
            "Unknown chain source".into(),
        )),
    }
}
fn source_name(value: ChainSource) -> &'static str {
    match value {
        ChainSource::OpenvpnCustom => "openvpn_custom",
        ChainSource::WireguardCustom => "wireguard_custom",
        ChainSource::VpnGate => "vpn_gate",
    }
}
fn uuid(value: &str) -> Result<Option<uuid::Uuid>, ControlServiceError> {
    if value.is_empty() {
        Ok(None)
    } else {
        value
            .parse()
            .map(Some)
            .map_err(|_| ControlServiceError::InvalidRequest("Invalid chain reference".into()))
    }
}
impl ControlService {
    pub(crate) async fn chain_profile_command(
        &self,
        request: v1::ChainProfileRequest,
    ) -> Result<v1::ChainProfileResponse, ControlServiceError> {
        #[cfg(windows)]
        {
            let _mutation = self.mutation_lock.lock().await;
            let request = ChainProfileRequest {
                action: request.action,
                source: source(&request.source)?,
                name: request.name,
                profile_id: uuid(&request.profile_id)?,
                revision: uuid(&request.revision)?,
                secrets: ImportSecrets {
                    configuration: String::from_utf8(request.configuration).map_err(|_| {
                        ControlServiceError::InvalidRequest("Invalid configuration encoding".into())
                    })?,
                    username: request.username,
                    password: request.password,
                    private_key_password: request.private_key_password,
                },
            };
            let mut retained = Vec::new();
            if let Some(id) = self
                .config
                .read()
                .await
                .network
                .chain_exit
                .as_ref()
                .and_then(|s| s.profile_id)
            {
                retained.push(id);
            }
            if let Some(profile) = &self.gate_status.borrow().current_profile {
                retained.push(profile.id);
            }
            let parent = self.cache_dir.clone();
            let result = tokio::task::spawn_blocking(move || {
                usque_core::chain_exit::profile_command(
                    &parent,
                    &usque_core::chain_exit::store::WindowsProfileCipher,
                    request,
                    &retained,
                )
            })
            .await
            .map_err(|_| {
                ControlServiceError::InvalidRequest("Chain storage worker failed".into())
            })?;
            Ok(v1::ChainProfileResponse {
                metadata_json: serde_json::to_string(&result)
                    .map_err(ControlServiceError::configuration)?,
            })
        }
        #[cfg(not(windows))]
        {
            let _ = request;
            Err(ControlServiceError::InvalidRequest(
                "Chain import is unavailable on this host".into(),
            ))
        }
    }
    pub(crate) fn validate_chain_selection(
        &self,
        profile: &usque_core::Profile,
    ) -> Result<(), ControlServiceError> {
        Self::validate_chain_selection_at(&self.cache_dir, profile)
    }
    pub(crate) fn validate_chain_selection_at(
        parent: &std::path::Path,
        profile: &usque_core::Profile,
    ) -> Result<(), ControlServiceError> {
        #[cfg(not(windows))]
        let _ = parent;
        if profile
            .custom_chain()
            .is_some_and(|s| s.profile_id.is_some())
        {
            #[cfg(windows)]
            {
                let (summary, _, _) = usque_core::chain_exit::store::ChainProfileStore::new(
                    parent,
                    &usque_core::chain_exit::store::WindowsProfileCipher,
                )
                .load(profile.custom_chain().expect("checked"))
                .map_err(ControlServiceError::configuration)?;
                if profile.chain_enabled()
                    && (summary.protocol.requires_udp()
                        && profile.data_plane == usque_core::DataPlaneMode::L4Proxy
                        || summary.protocol == usque_core::chain_exit::ChainProtocol::Wireguard
                            && !cfg!(feature = "wireguard"))
                {
                    return Err(ControlServiceError::InvalidRequest(
                        "Selected chain protocol is unavailable in this mode".into(),
                    ));
                }
            }
            #[cfg(not(windows))]
            return Err(ControlServiceError::InvalidRequest(
                "Chain import is unavailable on this host".into(),
            ));
        }
        Ok(())
    }
}
