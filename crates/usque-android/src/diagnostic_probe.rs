//! One authenticated local Deep Doctor request at a time. No remote control,
//! TUN creation, route mutation or second CONNECT-IP path exists here.
use std::sync::{Mutex, OnceLock};

use super::*;
use tokio_util::sync::CancellationToken;

struct ProbeSlot {
    id: jint,
    cancellation: CancellationToken,
    claimed: bool,
}
static PROBE: OnceLock<Mutex<Option<ProbeSlot>>> = OnceLock::new();

fn prepare(id: jint) -> bool {
    if id <= 0 {
        return false;
    }
    let mut slot = PROBE
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if slot.is_some() {
        return false;
    }
    *slot = Some(ProbeSlot {
        id,
        cancellation: CancellationToken::new(),
        claimed: false,
    });
    true
}

fn cancel(id: jint) {
    let mut slot = PROBE
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if let Some(probe) = slot.as_ref()
        && (id == 0 || probe.id == id)
    {
        probe.cancellation.cancel();
        if !probe.claimed {
            slot.take();
        }
    }
}

struct ProbeGuard(jint);
impl Drop for ProbeGuard {
    fn drop(&mut self) {
        let mut slot = PROBE
            .get_or_init(|| Mutex::new(None))
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if slot.as_ref().is_some_and(|probe| probe.id == self.0)
            && let Some(probe) = slot.take()
        {
            probe.cancellation.cancel();
        }
    }
}

fn claim(id: jint) -> Option<(ProbeGuard, CancellationToken)> {
    let mut slot = PROBE
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let probe = slot.as_mut()?;
    if probe.id != id || probe.claimed || probe.cancellation.is_cancelled() {
        return None;
    }
    probe.claimed = true;
    Some((ProbeGuard(id), probe.cancellation.clone()))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_georgexie2333_usque_NativeEngine_nativePrepareDiagnosticProbe<
    'local,
>(
    mut environment: EnvUnowned<'local>,
    _class: JClass<'local>,
    id: jint,
) -> jboolean {
    with_jni_env(&mut environment, |_| {
        if prepare(id) { JNI_TRUE } else { JNI_FALSE }
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_georgexie2333_usque_NativeEngine_nativeCancelDiagnosticProbe<
    'local,
>(
    mut environment: EnvUnowned<'local>,
    _class: JClass<'local>,
    id: jint,
) {
    with_jni_env(&mut environment, |_| cancel(id));
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_georgexie2333_usque_NativeEngine_nativeValidateDiagnosticProfile<
    'local,
>(
    mut environment: EnvUnowned<'local>,
    _class: JClass<'local>,
    profile: JString<'local>,
) -> jboolean {
    with_jni_env(&mut environment, |environment| {
        if profile
            .try_to_string(environment)
            .ok()
            .is_some_and(|json| parse_android_profile(&json).is_ok())
        {
            JNI_TRUE
        } else {
            JNI_FALSE
        }
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_georgexie2333_usque_NativeEngine_nativeDiagnosticProbe<
    'local,
>(
    mut environment: EnvUnowned<'local>,
    _class: JClass<'local>,
    id: jint,
    kind: JString<'local>,
    profile: JString<'local>,
    secret: JByteArray<'local>,
    vpn_service: JObject<'local>,
    vpn_protected: jboolean,
) -> jstring {
    with_jni_env(&mut environment, |environment| {
        let outcome = (|| {
            use usque_transport::NetworkProbeResult as ResultCode;
            let Some((_guard, cancellation)) = claim(id) else {
                return ResultCode::Cancelled;
            };
            let Ok(kind) = kind.try_to_string(environment) else {
                return ResultCode::Failed;
            };
            let Some(profile) = profile
                .try_to_string(environment)
                .ok()
                .and_then(|json| parse_android_profile(&json).ok())
            else {
                return ResultCode::Failed;
            };
            let Ok(secret) = environment.convert_byte_array(&secret) else {
                return ResultCode::Failed;
            };
            let secret = Zeroizing::new(secret);
            run_probe(
                environment,
                &kind,
                profile,
                &secret,
                (vpn_service, vpn_protected == JNI_TRUE),
                cancellation,
            )
            .unwrap_or(ResultCode::Failed)
        })();
        let (code, milliseconds) = match outcome {
            usque_transport::NetworkProbeResult::Passed { milliseconds } => {
                ("passed", Some(milliseconds))
            }
            usque_transport::NetworkProbeResult::NotApplicable => ("not_applicable", None),
            usque_transport::NetworkProbeResult::Failed => ("failed", None),
            usque_transport::NetworkProbeResult::TimedOut => ("timeout", None),
            usque_transport::NetworkProbeResult::Cancelled => ("cancelled", None),
            usque_transport::NetworkProbeResult::NetworkChanged => ("network_changed", None),
        };
        environment
            .new_string(serde_json::json!({"code": code, "milliseconds": milliseconds}).to_string())
            .map(JString::into_raw)
            .unwrap_or_default()
    })
}

#[cfg(not(target_os = "android"))]
fn run_probe(
    _environment: &mut Env<'_>,
    _kind: &str,
    _profile: Profile,
    _secret: &[u8],
    _platform: (JObject<'_>, bool),
    _cancellation: CancellationToken,
) -> Option<usque_transport::NetworkProbeResult> {
    Some(usque_transport::NetworkProbeResult::NotApplicable)
}

#[cfg(target_os = "android")]
fn run_probe(
    environment: &mut Env<'_>,
    kind: &str,
    profile: Profile,
    secret: &[u8],
    platform: (JObject<'_>, bool),
    cancellation: CancellationToken,
) -> Option<usque_transport::NetworkProbeResult> {
    use usque_transport::NetworkProbeResult as ResultCode;
    let (service, vpn) = platform;
    if kind != "dns" && kind != "h3" {
        return Some(ResultCode::NotApplicable);
    }
    if kind == "h3"
        && (android_runtime::is_running() || vpn || profile.transport == TransportPolicy::Http2)
    {
        return Some(ResultCode::NotApplicable);
    }
    let java_vm = environment.get_java_vm().ok()?;
    let generation = environment
        .call_method(
            &service,
            jni_str!("getUnderlyingNetworkGeneration"),
            jni_sig!("()J"),
            &[],
        )
        .and_then(|value| value.j())
        .ok()?
        .max(0) as u64;
    let service = environment.new_global_ref(service).ok()?;
    let protector: Arc<dyn SocketProtector> = Arc::new(ProbeProtector(AndroidSocketProtector {
        java_vm,
        service,
        policy: if vpn {
            AndroidSocketRoutePolicy::Vpn
        } else {
            AndroidSocketRoutePolicy::Proxy
        },
        network_generation: AtomicU64::new(generation),
    }));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .ok()?;
    let result = runtime.block_on(async {
        if kind == "dns" {
            usque_transport::probe_encrypted_dns(&profile.direct_dns, protector, cancellation).await
        } else {
            let identity = match warp_identity_from_secret(secret)
                .ok()
                .and_then(|identity| MasqueTlsIdentity::from_warp_identity(&identity).ok())
            {
                Some(identity) => identity,
                None => return ResultCode::NotApplicable,
            };
            let endpoints = usque_transport::h3_probe_endpoints(&profile);
            usque_transport::probe_h3_handshake_candidates(
                &endpoints,
                &profile.endpoint.sni,
                &identity,
                protector,
                cancellation,
            )
            .await
        }
    });
    // Current-thread tasks (including aborted DNS drivers) are dropped before
    // returning across JNI; no background runtime survives a probe.
    drop(runtime);
    Some(result)
}

#[cfg(target_os = "android")]
struct ProbeProtector(AndroidSocketProtector);

#[cfg(target_os = "android")]
#[async_trait::async_trait]
impl SocketProtector for ProbeProtector {
    fn protect(&self, _socket: SocketHandle) -> Result<(), String> {
        Err("exact_generation_required".to_owned())
    }
    async fn protect_for_target_generation(
        &self,
        socket: SocketHandle,
        _target: SocketAddr,
        _protocol: DirectProtocol,
        generation: u64,
    ) -> Result<DirectEgressLease, String> {
        if self.network_generation() != Some(generation) {
            return Err(STALE_GENERATION_REASON.to_owned());
        }
        self.0.bind_socket_for_generation(socket, generation)?;
        if self.network_generation() != Some(generation) {
            return Err(STALE_GENERATION_REASON.to_owned());
        }
        Ok(DirectEgressLease::for_generation(generation))
    }
    fn network_generation(&self) -> Option<u64> {
        self.0
            .java_vm
            .attach_current_thread(|environment| -> jni::errors::Result<_> {
                environment
                    .call_method(
                        &self.0.service,
                        jni_str!("getUnderlyingNetworkGeneration"),
                        jni_sig!("()J"),
                        &[],
                    )
                    .and_then(|value| value.j())
            })
            .ok()
            .and_then(|value| u64::try_from(value).ok())
    }
    fn endpoint_family_available(&self, endpoint: SocketAddr) -> Option<bool> {
        self.0.endpoint_family_available(endpoint)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn one_probe_and_exact_cancellation_leave_no_slot() {
        assert!(prepare(41));
        assert!(!prepare(42));
        let (guard, cancellation) = claim(41).unwrap();
        assert!(claim(41).is_none());
        cancel(42);
        assert!(!cancellation.is_cancelled());
        cancel(41);
        assert!(cancellation.is_cancelled());
        assert!(!prepare(42));
        drop(guard);
        assert!(prepare(42));
        cancel(42);
        assert!(claim(42).is_none());
        assert!(PROBE.get().unwrap().lock().unwrap().is_none());
    }
}
