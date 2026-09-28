//! Android Keystore codec bound once in each native-owning process.
use super::*;
use std::path::Path;
use std::sync::OnceLock;
use usque_core::chain_exit::{ImportError, store::ProfileCipher};
use uuid::Uuid;

static CIPHER: OnceLock<AndroidCipher> = OnceLock::new();
#[cfg(feature = "wireguard")]
pub(crate) fn profile_cipher() -> Result<Arc<dyn ProfileCipher>, String> {
    struct Shared(&'static AndroidCipher);
    impl ProfileCipher for Shared {
        fn seal(&self, id: Uuid, value: &[u8]) -> Result<Vec<u8>, ImportError> {
            self.0.seal(id, value)
        }
        fn open(&self, id: Uuid, value: &[u8]) -> Result<Zeroizing<Vec<u8>>, ImportError> {
            self.0.open(id, value)
        }
    }
    Ok(Arc::new(Shared(
        CIPHER.get().ok_or("CHAIN_CRYPTO_UNAVAILABLE")?,
    )))
}
struct AndroidCipher {
    vm: JavaVM,
    object: Global<JObject<'static>>,
}
impl AndroidCipher {
    fn crypt(&self, id: Uuid, value: &[u8], seal: bool) -> Result<Zeroizing<Vec<u8>>, ImportError> {
        self.vm
            .attach_current_thread(|env| -> jni::errors::Result<_> {
                let id = env.new_string(id.to_string())?;
                let bytes = env.byte_array_from_slice(value)?;
                let method = if seal {
                    jni_str!("seal")
                } else {
                    jni_str!("open")
                };
                let result = env.call_method(
                    &self.object,
                    method,
                    jni_sig!("(Ljava/lang/String;[B)[B"),
                    &[JValue::Object(id.as_ref()), JValue::Object(bytes.as_ref())],
                );
                // Clear a provider/authentication exception before making any
                // further JNI call. CheckJNI rejects array writes while an
                // exception is pending, including our secret-buffer cleanup.
                if env.exception_check() {
                    env.exception_clear();
                }
                let zeros = vec![0i8; value.len()];
                bytes.set_region(env, 0, &zeros)?;
                let result = match result {
                    Ok(value) => value.l()?,
                    Err(error) => return Err(error),
                };
                let array = env.cast_local::<JByteArray>(result)?;
                let output = Zeroizing::new(env.convert_byte_array(&array)?);
                array.set_region(env, 0, &vec![0i8; output.len()])?;
                Ok(output)
            })
            .map_err(|_| ImportError::new(0, "storage", "secure_storage_failed"))
    }
}
impl ProfileCipher for AndroidCipher {
    fn seal(&self, id: Uuid, value: &[u8]) -> Result<Vec<u8>, ImportError> {
        self.crypt(id, value, true).map(|v| v.to_vec())
    }
    fn open(&self, id: Uuid, value: &[u8]) -> Result<Zeroizing<Vec<u8>>, ImportError> {
        self.crypt(id, value, false)
    }
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_georgexie2333_usque_NativeEngine_nativeInitializeChainCrypto<
    'local,
>(
    mut environment: EnvUnowned<'local>,
    _class: JClass<'local>,
    codec: JObject<'local>,
) -> jboolean {
    with_jni_env(&mut environment, |env| {
        if CIPHER.get().is_some() {
            return JNI_TRUE;
        }
        let Ok(vm) = env.get_java_vm() else {
            return JNI_FALSE;
        };
        let Ok(object) = env.new_global_ref(codec) else {
            return JNI_FALSE;
        };
        if CIPHER.set(AndroidCipher { vm, object }).is_ok() {
            JNI_TRUE
        } else {
            JNI_FALSE
        }
    })
}
pub(crate) fn prepare(
    parent: &Path,
    profile: &Profile,
) -> Result<
    Option<(
        usque_core::vpngate::ServerSummary,
        usque_core::vpngate::PreparedProfile,
    )>,
    String,
> {
    if !profile.chain_enabled() {
        return Ok(None);
    }
    if profile.custom_chain().is_none() {
        return profile
            .vpn_gate
            .selection
            .as_ref()
            .ok_or_else(|| "Missing chain selection".to_owned())
            .and_then(|s| {
                usque_core::vpngate::CatalogueStore::new(parent)
                    .load_selection(s)
                    .map(Some)
                    .map_err(|_| "Invalid chain selection".to_owned())
            });
    }
    let cipher = CIPHER.get().ok_or("Chain secure storage is unavailable")?;
    let result = usque_core::chain_exit::prepare_selection(parent, profile, cipher)
        .map_err(|e| e.to_string())?;
    if !cfg!(feature = "wireguard")
        && result.as_ref().is_some_and(|(_, p)| {
            p.summary
                .as_ref()
                .is_some_and(|s| s.protocol == usque_core::chain_exit::ChainProtocol::Wireguard)
        })
    {
        return Err("WireGuard is unavailable".into());
    }
    Ok(result)
}
pub(crate) fn command(
    path: &Path,
    request: usque_core::chain_exit::ChainProfileRequest,
    status: &usque_core::vpngate::GateStatus,
) -> Result<String, String> {
    let parent = path.parent().ok_or("Invalid configuration path")?;
    let config_store = usque_core::storage::ConfigStore::new(path);
    // Serialize retained-reference inspection with settings commits in both
    // Android processes. Lock ordering is always configuration then library.
    let _guard = config_store
        .lock_exclusive()
        .map_err(|_| "Configuration is busy")?;
    let config = config_store.load().map_err(|_| "Invalid configuration")?;
    let mut retained = Vec::new();
    if let Some(id) = config.network.chain_exit.and_then(|s| s.profile_id) {
        retained.push(id);
    }
    if let Some(profile) = &status.current_profile {
        retained.push(profile.id);
    }
    let cipher = CIPHER.get().ok_or("Chain secure storage is unavailable")?;
    let result = usque_core::chain_exit::profile_command(parent, cipher, request, &retained);
    serde_json::to_string(&result).map_err(|_| "Chain response failed".into())
}
