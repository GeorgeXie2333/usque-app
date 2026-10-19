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
        match manager.command_scoped(request, input, provider_lease, client, accounts) {
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
            command(
                &path,
                request,
                input,
                CommandContext {
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
