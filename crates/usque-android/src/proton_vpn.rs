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
fn fetch<'local>(
    env: &mut Env<'local>,
    path: &Path,
    secret: &[u8],
    service: JObject<'local>,
) -> Result<vpngate::FetchContext, String> {
    #[cfg(target_os = "android")]
    let networks = android_runtime::internal_networks();
    #[cfg(not(target_os = "android"))]
    let networks = None;
    let identity = if secret.is_empty() {
        None
    } else {
        Some(warp_identity_from_secret(secret).map_err(|_| "PROTON_WARP_IDENTITY_INVALID")?)
    };
    let allow = env
        .call_method(
            &service,
            jni_str!("cataloguePhysicalAllowed"),
            jni_sig!("()Z"),
            &[],
        )
        .and_then(|v| v.z());
    if env.exception_check() {
        env.exception_clear();
        return Err("PROTON_UNAVAILABLE".into());
    }
    let allow_physical = allow.unwrap_or(false) && networks.is_none();
    let generation = env
        .call_method(
            &service,
            jni_str!("getUnderlyingNetworkGeneration"),
            jni_sig!("()J"),
            &[],
        )
        .and_then(|v| v.j());
    if env.exception_check() {
        env.exception_clear();
        return Err("PROTON_UNAVAILABLE".into());
    }
    let protector = Arc::new(AndroidSocketProtector {
        java_vm: env.get_java_vm().map_err(|_| "PROTON_UNAVAILABLE")?,
        service: env
            .new_global_ref(service)
            .map_err(|_| "PROTON_UNAVAILABLE")?,
        policy: AndroidSocketRoutePolicy::Proxy,
        network_generation: AtomicU64::new(generation.unwrap_or_default().max(0) as u64),
        physical_watch: OnceLock::new(),
    });
    #[cfg(target_os = "android")]
    let refresher = identity.as_ref().and_then(|_| {
        let profile = ConfigStore::new(path).load().ok()?.active_profile()?;
        Some(Arc::new(AndroidEndpointPinRefresher {
            profile_id: profile.id.to_string(),
            identity: tokio::sync::Mutex::new(warp_identity_from_secret(secret).ok()?),
            protector: protector.clone(),
        }) as Arc<dyn EndpointPinRefresher>)
    });
    #[cfg(not(target_os = "android"))]
    let refresher = {
        let _ = path;
        None
    };
    Ok(vpngate::FetchContext {
        identity,
        protector,
        networks,
        allow_physical,
        refresher,
    })
}
struct CommandContext<'a> {
    fetch: Option<vpngate::FetchContext>,
    expected_profile: Option<uuid::Uuid>,
    provider_lease: Option<Arc<std::fs::File>>,
    client_context: &'a str,
}
fn command(
    path: &Path,
    request: Request,
    input: Zeroizing<Vec<u8>>,
    context: CommandContext<'_>,
) -> Result<(Vec<u8>, Zeroizing<Vec<u8>>), String> {
    let CommandContext {
        fetch,
        expected_profile,
        provider_lease,
        client_context,
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
        let config = if request.needs_network() || request.action == "forget" {
            Some(
                ConfigStore::new(path)
                    .load()
                    .map_err(|_| "PROTON_UNAVAILABLE")?,
            )
        } else {
            None
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
        let context = if request.needs_network() {
            if !crate::warp_wireguard::stop() {
                return Err("PROTON_UNDERLAY_CLEANUP_PENDING".into());
            }
            let fetch = fetch.ok_or("PROTON_UNAVAILABLE")?;
            let config = config.ok_or("PROTON_UNAVAILABLE")?;
            validate_underlay_snapshot(config.active_profile_id, expected_profile)?;
            #[cfg(target_os = "android")]
            if fetch.networks.is_some() {
                validate_underlay_snapshot(android_runtime::profile_id(), expected_profile)?;
            }
            Some(usque_transport::proton_vpn::Context {
                lifecycle_lease: provider_lease.clone(),
                profile: config
                    .active_profile()
                    .ok_or("PROTON_WARP_IDENTITY_REQUIRED")?,
                existing: fetch.networks.map(|(_, warp)| warp),
                identity: fetch.identity,
                protector: fetch.protector,
                refresher: fetch.refresher,
                allow_physical: fetch.allow_physical,
            })
        } else {
            None
        };
        match manager.command_with_client(request, input, context, client) {
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
        let _ = (
            path,
            request,
            input,
            fetch,
            expected_profile,
            provider_lease,
            client_context,
        );
        Err("PROTON_UNAVAILABLE".into())
    }
}

#[cfg(any(feature = "wireguard", test))]
fn validate_underlay_snapshot(
    current: Option<uuid::Uuid>,
    expected: Option<uuid::Uuid>,
) -> Result<(), String> {
    if expected.is_none() || current != expected {
        return Err("PROTON_UNDERLAY_CHANGED".into());
    }
    Ok(())
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_georgexie2333_usque_NativeEngine_nativeProtonVpn<'local>(
    mut environment: EnvUnowned<'local>,
    _class: JClass<'local>,
    config_path: JString<'local>,
    command_json: JString<'local>,
    sensitive_input: JByteArray<'local>,
    warp_secret: JByteArray<'local>,
    expected_warp_profile_id: JString<'local>,
    client_context: JString<'local>,
    service: JObject<'local>,
) -> jni::sys::jobjectArray {
    with_jni_env(&mut environment, |env| {
        let result = (|| -> Result<_, String> {
            let length = sensitive_input
                .len(env)
                .map_err(|_| "PROTON_INPUT_INVALID")?;
            let warp_length = warp_secret.len(env).map_err(|_| "PROTON_INPUT_INVALID")?;
            if length > MAX_INPUT_BYTES || warp_length > 64 * 1024 {
                return Err("PROTON_INPUT_INVALID".into());
            }
            let input = Zeroizing::new(
                env.convert_byte_array(&sensitive_input)
                    .map_err(|_| "PROTON_INPUT_INVALID")?,
            );
            sensitive_input
                .set_region(env, 0, &vec![0i8; length])
                .map_err(|_| "PROTON_INPUT_INVALID")?;
            let secret = Zeroizing::new(
                env.convert_byte_array(&warp_secret)
                    .map_err(|_| "PROTON_INPUT_INVALID")?,
            );
            warp_secret
                .set_region(env, 0, &vec![0i8; warp_length])
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
            let expected = expected_warp_profile_id
                .try_to_string(env)
                .map_err(|_| "PROTON_REQUEST_INVALID")?;
            let expected_profile = if request.needs_network() {
                if expected.len() != 36 {
                    return Err("PROTON_REQUEST_INVALID".into());
                }
                Some(uuid::Uuid::parse_str(&expected).map_err(|_| "PROTON_REQUEST_INVALID")?)
            } else {
                if !expected.is_empty() {
                    return Err("PROTON_REQUEST_INVALID".into());
                }
                None
            };
            // Fence configuration before reading its identity/network pair;
            // Kotlin's earlier secret snapshot is checked again under this lease.
            let provider_lease = if request.needs_network() || request.action == "forget" {
                Some(Arc::new(
                    usque_core::proton_vpn::lifecycle_lease(
                        path.parent().ok_or("PROTON_UNAVAILABLE")?,
                    )
                    .map_err(|_| "PROTON_OPERATION_BUSY")?,
                ))
            } else {
                None
            };
            let fetch = if request.needs_network() {
                Some(fetch(env, &path, &secret, service)?)
            } else {
                None
            };
            command(
                &path,
                request,
                input,
                CommandContext {
                    fetch,
                    expected_profile,
                    provider_lease,
                    client_context: &client_context,
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

#[cfg(test)]
mod tests {
    use super::validate_underlay_snapshot;

    #[test]
    fn a_secret_or_borrowed_network_from_another_account_is_rejected() {
        let original = uuid::Uuid::from_u128(1);
        let replacement = uuid::Uuid::from_u128(2);
        assert!(validate_underlay_snapshot(Some(original), Some(original)).is_ok());
        assert!(validate_underlay_snapshot(Some(replacement), Some(original)).is_err());
        assert!(validate_underlay_snapshot(None, Some(original)).is_err());
        assert!(validate_underlay_snapshot(Some(original), None).is_err());
    }
}
