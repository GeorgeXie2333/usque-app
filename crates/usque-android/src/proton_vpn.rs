//! Dedicated bounded byte-array JNI channel: secrets never enter catalogue JSON.
use super::*;
use std::path::Path;
#[cfg(feature = "wireguard")]
use std::sync::Mutex;
#[cfg(feature = "wireguard")]
use usque_core::proton_vpn::Response;
use usque_core::proton_vpn::{MAX_INPUT_BYTES, Request};
#[cfg(feature = "wireguard")]
type Slot = Option<(PathBuf, Arc<usque_transport::proton_vpn::Manager>)>;
#[cfg(feature = "wireguard")]
static MANAGER: OnceLock<Mutex<Slot>> = OnceLock::new();

pub(crate) fn signal_stop() {
    #[cfg(feature = "wireguard")]
    if let Some(slot) = MANAGER.get()
        && let Ok(slot) = slot.lock()
        && let Some((_, manager)) = &*slot
    {
        manager.signal_stop();
    }
}
#[cfg(feature = "wireguard")]
pub(crate) fn stop() -> bool {
    #[cfg(feature = "wireguard")]
    {
        MANAGER
            .get()
            .and_then(|slot| slot.lock().ok())
            .and_then(|slot| slot.as_ref().map(|(_, manager)| manager.clone()))
            .is_none_or(|manager| manager.stop_blocking())
    }
}
#[cfg(feature = "wireguard")]
fn manager(path: &Path) -> Result<Arc<usque_transport::proton_vpn::Manager>, String> {
    let mut slot = MANAGER
        .get_or_init(|| Mutex::new(None))
        .lock()
        .map_err(|_| "PROTON_UNAVAILABLE")?;
    if slot.is_none() {
        *slot = Some((
            path.into(),
            Arc::new(usque_transport::proton_vpn::Manager::new(
                path.into(),
                crate::chain_exit::profile_cipher()?,
            )),
        ));
    }
    let (current, manager) = slot.as_ref().ok_or("PROTON_UNAVAILABLE")?;
    if current != path {
        return Err("PROTON_UNAVAILABLE".into());
    }
    Ok(manager.clone())
}
struct CommandContext<'a> {
    provider_lease: Option<Arc<std::fs::File>>,
    client_context: &'a str,
    #[cfg(feature = "wireguard")]
    network: Option<Arc<dyn usque_transport::proton_vpn::AccountWarpProvider>>,
}
fn command(
    path: &Path,
    request: Request,
    input: Zeroizing<Vec<u8>>,
    context: CommandContext<'_>,
) -> Result<(Vec<u8>, Zeroizing<Vec<u8>>), String> {
    let CommandContext {
        provider_lease,
        client_context,
        #[cfg(feature = "wireguard")]
        network,
    } = context;
    #[cfg(feature = "wireguard")]
    {
        let manager = manager(path)?;
        let client = if matches!(request.action.as_str(), "guest" | "login") {
            usque_transport::proton_vpn::ClientConfig::android(client_context)
                .map_err(|_| "PROTON_CLIENT_CONTEXT_INVALID")?
        } else {
            usque_transport::proton_vpn::ClientConfig::default()
        };
        let config = if request.mutates_account() {
            Some(
                ConfigStore::new(path)
                    .load()
                    .map_err(|_| "PROTON_UNAVAILABLE")?,
            )
        } else {
            ConfigStore::new(path).load().ok()
        };
        let accounts = usque_transport::proton_vpn::AccountContext {
            active: engine_snapshot()
                .vpn_gate
                .unwrap_or_default()
                .current_profile
                .map(|profile| profile.id)
                .into_iter()
                .collect(),
            selected: config.as_ref().and_then(|config| {
                config
                    .network
                    .chain_exit
                    .as_ref()
                    .and_then(|settings| settings.profile_id)
            }),
        };
        if let Some(config) = &config {
            let active = engine_snapshot()
                .vpn_gate
                .unwrap_or_default()
                .current_profile
                .map(|p| p.id);
            let selected = config
                .network
                .chain_exit
                .as_ref()
                .and_then(|s| s.profile_id);
            if matches!(request.action.as_str(), "logout" | "refresh" | "forget")
                && request.profile_id.is_some()
                && (active == request.profile_id
                    || matches!(request.action.as_str(), "logout" | "forget")
                        && selected == request.profile_id)
            {
                return Ok((
                    b"{\"error\":\"profile_in_use\",\"profiles\":[]}".to_vec(),
                    Zeroizing::new(Vec::new()),
                ));
            }
        }
        match manager.command_with_network(
            request,
            input,
            provider_lease,
            client,
            (accounts, network),
        ) {
            Ok(output) => Ok((
                serde_json::to_vec(&output.metadata).map_err(|_| "PROTON_UNAVAILABLE")?,
                output.sensitive_handoff,
            )),
            Err(failure) => Ok((
                serde_json::to_vec(&Response {
                    error: Some(failure.reason),
                    ..Default::default()
                })
                .map_err(|_| "PROTON_UNAVAILABLE")?,
                Zeroizing::new(Vec::new()),
            )),
        }
    }
    #[cfg(not(feature = "wireguard"))]
    {
        let _ = (path, request, input, provider_lease, client_context);
        Err("PROTON_UNAVAILABLE".into())
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_georgexie2333_usque_NativeEngine_nativeProtonVpn<'local>(
    mut environment: EnvUnowned<'local>,
    _class: JClass<'local>,
    config_path: JString<'local>,
    command_json: JString<'local>,
    sensitive_input: JByteArray<'local>,
    client_context: JString<'local>,
    vpn_service: JObject<'local>,
) -> jni::sys::jobjectArray {
    with_jni_env(&mut environment, |env| {
        let result = (|| -> Result<_, String> {
            let length = sensitive_input
                .len(env)
                .map_err(|_| "PROTON_INPUT_INVALID")?;
            if length > MAX_INPUT_BYTES {
                return Err("PROTON_INPUT_INVALID".into());
            }
            let input = Zeroizing::new(
                env.convert_byte_array(&sensitive_input)
                    .map_err(|_| "PROTON_INPUT_INVALID")?,
            );
            sensitive_input
                .set_region(env, 0, &vec![0i8; length])
                .map_err(|_| "PROTON_INPUT_INVALID")?;
            let path = PathBuf::from(
                config_path
                    .try_to_string(env)
                    .map_err(|_| "PROTON_REQUEST_INVALID")?,
            );
            if !path.is_absolute()
                || path.file_name().and_then(|s| s.to_str()) != Some("profiles-v2.json")
            {
                return Err("PROTON_REQUEST_INVALID".into());
            }
            let request = Request::parse(
                &command_json
                    .try_to_string(env)
                    .map_err(|_| "PROTON_REQUEST_INVALID")?,
            )
            .map_err(|_| "PROTON_REQUEST_INVALID")?;
            let client_context = client_context
                .try_to_string(env)
                .map_err(|_| "PROTON_CLIENT_CONTEXT_INVALID")?;
            if client_context.len() > 32 * 1024
                || matches!(request.action.as_str(), "guest" | "login") == client_context.is_empty()
            {
                return Err("PROTON_CLIENT_CONTEXT_INVALID".into());
            }
            // Retain the cross-process account-mutation fence through the
            // entire worker, independently of WARP identity or connection state.
            let provider_lease = if request.mutates_account() {
                Some(Arc::new(
                    usque_core::proton_vpn::lifecycle_lease(
                        path.parent().ok_or("PROTON_UNAVAILABLE")?,
                    )
                    .map_err(|_| "PROTON_OPERATION_BUSY")?,
                ))
            } else {
                None
            };
            #[cfg(feature = "wireguard")]
            let network = if request.network_policy_version == 1 {
                Some(Arc::new(AccountNetworkProvider {
                    path: path.clone(),
                    selected: ConfigStore::new(&path)
                        .load()
                        .ok()
                        .and_then(|c| c.active_profile().map(|p| p.id)),
                    java_vm: env.get_java_vm().map_err(|_| "PROTON_UNAVAILABLE")?,
                    service: env
                        .new_global_ref(vpn_service)
                        .map_err(|_| "PROTON_UNAVAILABLE")?,
                })
                    as Arc<dyn usque_transport::proton_vpn::AccountWarpProvider>)
            } else {
                None
            };
            #[cfg(not(feature = "wireguard"))]
            let _ = vpn_service;
            command(
                &path,
                request,
                input,
                CommandContext {
                    provider_lease,
                    client_context: &client_context,
                    #[cfg(feature = "wireguard")]
                    network,
                },
            )
        })();
        match result {
            Ok((metadata, handoff)) => {
                let encoded = (|| -> jni::errors::Result<_> {
                    let first = env.byte_array_from_slice(&metadata)?;
                    let second = env.byte_array_from_slice(&handoff)?;
                    let array = JObjectArray::<JByteArray>::new(env, 2, &first)?;
                    array.set_element(env, 1, &second)?;
                    Ok(array.into_raw())
                })();
                encoded.unwrap_or(std::ptr::null_mut())
            }
            Err(_) => {
                if env.exception_check() {
                    env.exception_clear();
                }
                throw_io_error(env, "PROTON_UNAVAILABLE");
                std::ptr::null_mut()
            }
        }
    })
}

#[cfg(feature = "wireguard")]
struct AccountNetworkProvider {
    path: PathBuf,
    selected: Option<uuid::Uuid>,
    java_vm: JavaVM,
    service: Global<JObject<'static>>,
}
#[cfg(feature = "wireguard")]
#[async_trait::async_trait]
impl usque_transport::proton_vpn::AccountWarpProvider for AccountNetworkProvider {
    async fn context(
        &self,
        cancel: &tokio_util::sync::CancellationToken,
    ) -> Result<usque_transport::warp_wireguard::Context, usque_core::chain_exit::ImportError> {
        use usque_core::proton_vpn::error;
        if cancel.is_cancelled() {
            return Err(error("cancelled"));
        }
        let profile = ConfigStore::new(&self.path)
            .load()
            .ok()
            .and_then(|c| c.active_profile())
            .ok_or_else(|| error("warp_identity_required"))?;
        if Some(profile.id) != self.selected {
            return Err(error("underlay_changed"));
        }
        #[cfg(target_os = "android")]
        let existing = android_runtime::internal_networks()
            .map(|(_, warp)| warp)
            .filter(|network| {
                matches!(
                    network.health_snapshot(),
                    usque_transport::RuntimeHealth::Connected { .. }
                )
            });
        #[cfg(not(target_os = "android"))]
        let existing = None;
        let (allow_physical, identity) = if existing.is_none() {
            self.java_vm
                .attach_current_thread(|env| -> jni::errors::Result<_> {
                    let allowed = env
                        .call_method(
                            &self.service,
                            jni_str!("cataloguePhysicalAllowed"),
                            jni_sig!("()Z"),
                            &[],
                        )?
                        .z()?;
                    if !allowed {
                        return Ok((false, None));
                    }
                    let id = JObject::from(env.new_string(profile.id.to_string())?);
                    let json = JObject::from(
                        env.new_string(serde_json::to_string(&profile).unwrap_or_default())?,
                    );
                    let raw = env
                        .call_method(
                            &self.service,
                            jni_str!("protonWarpSecret"),
                            jni_sig!("(Ljava/lang/String;Ljava/lang/String;)[B"),
                            &[JValue::Object(&id), JValue::Object(&json)],
                        )?
                        .l()?;
                    let array = env.cast_local::<JByteArray>(raw)?;
                    let bytes = Zeroizing::new(env.convert_byte_array(&array)?);
                    array.set_region(env, 0, &vec![0i8; bytes.len()])?;
                    Ok((true, warp_identity_from_secret(&bytes).ok()))
                })
                .map_err(|_| error("warp_identity_required"))?
        } else {
            (false, None)
        };
        // The endpoint refresher receives the Context's NoopSocketProtector.
        // This Java holder is only used to persist the existing identity.
        #[cfg(target_os = "android")]
        let refresher = if identity.is_some() {
            let protector = self
                .java_vm
                .attach_current_thread(|env| -> jni::errors::Result<_> {
                    Ok(Arc::new(AndroidSocketProtector {
                        java_vm: env.get_java_vm()?,
                        service: env.new_global_ref(&self.service)?,
                        policy: AndroidSocketRoutePolicy::Proxy,
                        network_generation: AtomicU64::new(0),
                        physical_watch: OnceLock::new(),
                    }))
                })
                .map_err(|_| error("warp_identity_required"))?;
            identity
                .as_ref()
                .and_then(|identity| identity.to_portable_secret_json().ok())
                .and_then(|json| warp_identity_from_secret(json.as_bytes()).ok())
                .map(|identity| {
                    Arc::new(AndroidEndpointPinRefresher {
                        profile_id: profile.id.to_string(),
                        identity: tokio::sync::Mutex::new(identity),
                        protector,
                    }) as Arc<dyn EndpointPinRefresher>
                })
        } else {
            None
        };
        #[cfg(not(target_os = "android"))]
        let refresher = None;
        Ok(usque_transport::warp_wireguard::Context {
            profile,
            existing,
            identity,
            refresher,
            allow_physical,
            lifecycle_lease: None,
            protector: Arc::new(usque_transport::NoopSocketProtector),
        })
    }
}
