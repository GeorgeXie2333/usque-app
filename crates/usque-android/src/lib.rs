//! JNI boundary owned by the Android `:vpn` process.
//!
//! Android decrypts the selected profile's WARP identity immediately before
//! start and transfers it as a byte array. Rust validates and zeroizes that
//! buffer, owns the Tokio/MASQUE runtime, duplicates the TUN descriptor, and
//! routes every endpoint socket through Android's selected non-VPN network.
//! VPN mode additionally calls `VpnService.protect(fd)` before binding it;
//! proxy modes deliberately do not require VPN preparation permission.

#[cfg(any(test, target_os = "android"))]
use std::sync::atomic::AtomicBool;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
#[cfg(any(test, target_os = "android"))]
use std::time::Duration;
use std::{
    net::{IpAddr, SocketAddr},
    path::PathBuf,
};

use jni::{
    Env, EnvUnowned, JValue, JavaVM,
    errors::ThrowRuntimeExAndDefault,
    jni_sig, jni_str,
    objects::{JByteArray, JClass, JObject, JObjectArray, JString},
    refs::Global,
    strings::JNIString,
    sys::{JNI_FALSE, JNI_TRUE, jboolean, jbyteArray, jint, jlong, jstring},
};
use serde::{Deserialize, Serialize};
use usque_core::{
    AppConfig, ConsumerEntitlement, ConsumerRegistrationClient, DnsMode, EndpointSettings,
    FrontendSettings, IdentityProvider, IpPolicy, OperatingMode, Profile, ProxyDnsMode,
    ProxySettings, RegistrationError, RegistrationOptions, SharedNetworkSettings, TransportPolicy,
    WarpIdentity, parse_manual_warp_secret, storage::ConfigStore, update::UpdateChecker,
};
#[cfg(target_os = "android")]
use usque_transport::{
    EndpointPinRefresher, TransportError, refresh_endpoint_pin_over_protected_socket,
};
use usque_transport::{MasqueTlsIdentity, SocketHandle, SocketProtector};
use zeroize::Zeroizing;

pub const START_OK: i32 = 0;
pub const START_NOT_READY: i32 = -2;
pub const INVALID_WARP_SECRET: i32 = -3;
pub const START_ALREADY_RUNNING: i32 = -4;
pub const START_INVALID_PROFILE: i32 = -5;
pub const START_PLATFORM_FAILURE: i32 = -6;
pub const START_TRANSPORT_FAILURE: i32 = -7;
pub const START_TUN_FAILURE: i32 = -8;
pub const RECONFIGURE_OK: i32 = 0;
pub const RECONFIGURE_NEED_COLD: i32 = 1;
pub const RECONFIGURE_NEED_ATTACH: i32 = 2;
pub const RECONFIGURE_NOT_RUNNING: i32 = -10;

#[derive(Debug)]
struct JniCode(jint);

impl Default for JniCode {
    fn default() -> Self {
        Self(START_PLATFORM_FAILURE)
    }
}

fn with_jni_env<'local, T, F>(environment: &mut EnvUnowned<'local>, operation: F) -> T
where
    T: Default + 'local,
    F: FnOnce(&mut Env<'local>) -> T,
{
    environment
        .with_env(|environment| -> jni::errors::Result<T> { Ok(operation(environment)) })
        .resolve::<ThrowRuntimeExAndDefault>()
}

fn with_jni_code<'local, F>(environment: &mut EnvUnowned<'local>, operation: F) -> jint
where
    F: FnOnce(&mut Env<'local>) -> jint,
{
    with_jni_env(environment, |environment| JniCode(operation(environment))).0
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_georgexie2333_usque_NativeEngine_nativeIsReady<'local>(
    mut environment: EnvUnowned<'local>,
    _class: JClass<'local>,
) -> jboolean {
    with_jni_env(&mut environment, |_| {
        if engine_ready() { JNI_TRUE } else { JNI_FALSE }
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_georgexie2333_usque_NativeEngine_nativeStart<'local>(
    mut environment: EnvUnowned<'local>,
    _class: JClass<'local>,
    tun_file_descriptor: jint,
    profile_json: JString<'local>,
    warp_secret: JByteArray<'local>,
    proxy_password: JByteArray<'local>,
    vpn_service: JObject<'local>,
) -> jint {
    with_jni_code(&mut environment, |environment| {
        native_start(
            environment,
            tun_file_descriptor,
            profile_json,
            warp_secret,
            proxy_password,
            vpn_service,
        )
    })
}

fn native_start<'local>(
    environment: &mut Env<'local>,
    tun_file_descriptor: jint,
    profile_json: JString<'local>,
    warp_secret: JByteArray<'local>,
    proxy_password: JByteArray<'local>,
    vpn_service: JObject<'local>,
) -> jint {
    if tun_file_descriptor < 0 || !engine_ready() {
        return START_NOT_READY;
    }
    let profile_json = match profile_json.try_to_string(environment) {
        Ok(value) => value,
        Err(_) => return START_INVALID_PROFILE,
    };
    let secret = match environment.convert_byte_array(&warp_secret) {
        Ok(value) => Zeroizing::new(value),
        Err(_) => return INVALID_WARP_SECRET,
    };
    let profile = match parse_android_profile(&profile_json) {
        Ok(profile) if profile.frontends.tunnel => profile,
        _ => return START_INVALID_PROFILE,
    };
    let proxy_password = match environment.convert_byte_array(&proxy_password) {
        Ok(value) => Zeroizing::new(value),
        Err(_) => return START_INVALID_PROFILE,
    };
    let profile = match attach_android_proxy_password(profile, proxy_password) {
        Ok(profile) => profile,
        Err(code) => return code,
    };
    let identity = match warp_identity_from_secret(&secret) {
        Ok(identity) => identity,
        Err(_) => return INVALID_WARP_SECRET,
    };
    let java_vm = match environment.get_java_vm() {
        Ok(java_vm) => java_vm,
        Err(_) => return START_PLATFORM_FAILURE,
    };
    let network_generation = environment
        .call_method(
            &vpn_service,
            jni_str!("getUnderlyingNetworkGeneration"),
            jni_sig!("()J"),
            &[],
        )
        .and_then(|value| value.j())
        .unwrap_or_default()
        .max(0) as u64;
    let service = match environment.new_global_ref(vpn_service) {
        Ok(service) => service,
        Err(_) => return START_PLATFORM_FAILURE,
    };
    start_engine(
        tun_file_descriptor,
        profile,
        identity,
        Arc::new(AndroidSocketProtector {
            java_vm,
            service,
            policy: AndroidSocketRoutePolicy::Vpn,
            network_generation: AtomicU64::new(network_generation),
        }),
    )
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_georgexie2333_usque_NativeEngine_nativeStartProxy<'local>(
    mut environment: EnvUnowned<'local>,
    _class: JClass<'local>,
    profile_json: JString<'local>,
    warp_secret: JByteArray<'local>,
    proxy_password: JByteArray<'local>,
    vpn_service: JObject<'local>,
) -> jint {
    with_jni_code(&mut environment, |environment| {
        native_start_proxy(
            environment,
            profile_json,
            warp_secret,
            proxy_password,
            vpn_service,
        )
    })
}

fn native_start_proxy<'local>(
    environment: &mut Env<'local>,
    profile_json: JString<'local>,
    warp_secret: JByteArray<'local>,
    proxy_password: JByteArray<'local>,
    vpn_service: JObject<'local>,
) -> jint {
    if !engine_ready() {
        return START_NOT_READY;
    }
    let profile_json = match profile_json.try_to_string(environment) {
        Ok(value) => value,
        Err(_) => return START_INVALID_PROFILE,
    };
    let secret = match environment.convert_byte_array(&warp_secret) {
        Ok(value) => Zeroizing::new(value),
        Err(_) => return INVALID_WARP_SECRET,
    };
    let profile = match parse_android_profile(&profile_json) {
        Ok(profile) if !profile.frontends.tunnel => profile,
        _ => return START_INVALID_PROFILE,
    };
    let proxy_password = match environment.convert_byte_array(&proxy_password) {
        Ok(value) => Zeroizing::new(value),
        Err(_) => return START_INVALID_PROFILE,
    };
    let profile = match attach_android_proxy_password(profile, proxy_password) {
        Ok(profile) => profile,
        Err(code) => return code,
    };
    let identity = match warp_identity_from_secret(&secret) {
        Ok(identity) => identity,
        Err(_) => return INVALID_WARP_SECRET,
    };
    let java_vm = match environment.get_java_vm() {
        Ok(java_vm) => java_vm,
        Err(_) => return START_PLATFORM_FAILURE,
    };
    let network_generation = environment
        .call_method(
            &vpn_service,
            jni_str!("getUnderlyingNetworkGeneration"),
            jni_sig!("()J"),
            &[],
        )
        .and_then(|value| value.j())
        .unwrap_or_default()
        .max(0) as u64;
    let service = match environment.new_global_ref(vpn_service) {
        Ok(service) => service,
        Err(_) => return START_PLATFORM_FAILURE,
    };
    start_proxy_engine(
        profile,
        identity,
        Arc::new(AndroidSocketProtector {
            java_vm,
            service,
            policy: AndroidSocketRoutePolicy::Proxy,
            network_generation: AtomicU64::new(network_generation),
        }),
    )
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_georgexie2333_usque_NativeEngine_nativeCancel<'local>(
    mut environment: EnvUnowned<'local>,
    _class: JClass<'local>,
) {
    with_jni_env(&mut environment, |_| cancel_engine());
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_georgexie2333_usque_NativeEngine_nativeStop<'local>(
    mut environment: EnvUnowned<'local>,
    _class: JClass<'local>,
) {
    with_jni_env(&mut environment, |_| stop_engine());
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_georgexie2333_usque_NativeEngine_nativeNotifyNetworkChanged<
    'local,
>(
    mut environment: EnvUnowned<'local>,
    _class: JClass<'local>,
    generation: jlong,
) {
    with_jni_env(&mut environment, |_| {
        if generation >= 0 {
            notify_network_changed(generation as u64);
        }
    });
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_georgexie2333_usque_NativeEngine_nativeReconfigure<'local>(
    mut environment: EnvUnowned<'local>,
    _class: JClass<'local>,
    profile_json: JString<'local>,
) -> jint {
    with_jni_code(&mut environment, |environment| {
        let profile_json = match profile_json.try_to_string(environment) {
            Ok(value) => value,
            Err(_) => return START_INVALID_PROFILE,
        };
        let profile = match parse_android_profile(&profile_json) {
            Ok(profile) => profile,
            Err(_) => return START_INVALID_PROFILE,
        };
        reconfigure_engine(profile)
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_georgexie2333_usque_NativeEngine_nativeAttachTun<'local>(
    mut environment: EnvUnowned<'local>,
    _class: JClass<'local>,
    tun_file_descriptor: jint,
    profile_json: JString<'local>,
) -> jint {
    with_jni_code(&mut environment, |environment| {
        if tun_file_descriptor < 0 {
            return START_TUN_FAILURE;
        }
        let profile_json = match profile_json.try_to_string(environment) {
            Ok(value) => value,
            Err(_) => return START_INVALID_PROFILE,
        };
        let profile = match parse_android_profile(&profile_json) {
            Ok(profile) => profile,
            Err(_) => return START_INVALID_PROFILE,
        };
        attach_tun_engine(tun_file_descriptor, profile)
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_georgexie2333_usque_NativeEngine_nativeDetachTun<'local>(
    mut environment: EnvUnowned<'local>,
    _class: JClass<'local>,
) -> jint {
    with_jni_code(&mut environment, |_| detach_tun_engine())
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_georgexie2333_usque_NativeEngine_nativeValidateWarpSecret<
    'local,
>(
    mut environment: EnvUnowned<'local>,
    _class: JClass<'local>,
    secret: JByteArray<'local>,
) -> jint {
    with_jni_code(&mut environment, |environment| {
        native_validate_warp_secret(environment, secret)
    })
}

fn native_validate_warp_secret(environment: &mut Env<'_>, secret: JByteArray<'_>) -> jint {
    let bytes = match environment.convert_byte_array(&secret) {
        Ok(bytes) => Zeroizing::new(bytes),
        Err(_) => return INVALID_WARP_SECRET,
    };
    validate_warp_secret_bytes(&bytes)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_georgexie2333_usque_NativeEngine_nativeInspectWarpSecret<
    'local,
>(
    mut environment: EnvUnowned<'local>,
    _class: JClass<'local>,
    secret: JByteArray<'local>,
) -> jstring {
    with_jni_env(&mut environment, |environment| {
        native_inspect_warp_secret(environment, secret)
    })
}

fn native_inspect_warp_secret(environment: &mut Env<'_>, secret: JByteArray<'_>) -> jstring {
    let bytes = match environment.convert_byte_array(&secret) {
        Ok(bytes) => Zeroizing::new(bytes),
        Err(_) => return std::ptr::null_mut(),
    };
    let metadata = match identity_metadata(&bytes) {
        Ok(metadata) => metadata,
        Err(error) => {
            let error = JNIString::from(error);
            let _ = environment.throw_new(jni_str!("java/lang/IllegalArgumentException"), &error);
            return std::ptr::null_mut();
        }
    };
    let json = match serde_json::to_string(&metadata) {
        Ok(json) => json,
        Err(_) => return std::ptr::null_mut(),
    };
    match environment.new_string(json) {
        Ok(value) => value.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_georgexie2333_usque_NativeEngine_nativeSnapshot<'local>(
    mut environment: EnvUnowned<'local>,
    _class: JClass<'local>,
) -> jstring {
    with_jni_env(&mut environment, native_snapshot)
}

fn native_snapshot(environment: &mut Env<'_>) -> jstring {
    let json = serde_json::to_string(&engine_snapshot()).unwrap_or_else(|_| {
        r#"{"phase":"error","warning":"Native status serialization failed."}"#.to_owned()
    });
    match environment.new_string(json) {
        Ok(value) => value.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_georgexie2333_usque_NativeEngine_nativeRegisterConsumerWarp<
    'local,
>(
    mut environment: EnvUnowned<'local>,
    _class: JClass<'local>,
    locale: JString<'local>,
) -> jbyteArray {
    with_jni_env(&mut environment, |environment| {
        native_register_consumer_warp(environment, locale)
    })
}

fn native_register_consumer_warp(environment: &mut Env<'_>, locale: JString<'_>) -> jbyteArray {
    let locale = match locale.try_to_string(environment) {
        Ok(locale) => locale,
        Err(error) => {
            throw_io_error(environment, &error.to_string());
            return std::ptr::null_mut();
        }
    };
    let secret = match register_consumer_warp(&locale) {
        Ok(secret) => secret,
        Err(error) => {
            throw_io_error(environment, &error);
            return std::ptr::null_mut();
        }
    };
    match environment.byte_array_from_slice(secret.as_bytes()) {
        Ok(output) => output.into_raw(),
        Err(error) => {
            throw_io_error(environment, &error.to_string());
            std::ptr::null_mut()
        }
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_georgexie2333_usque_NativeEngine_nativeRegisterConsumerWarpWithLicense<
    'local,
>(
    mut environment: EnvUnowned<'local>,
    _class: JClass<'local>,
    locale: JString<'local>,
    license_key: JString<'local>,
) -> jbyteArray {
    with_jni_env(&mut environment, |environment| {
        native_register_consumer_warp_with_license(environment, locale, license_key)
    })
}

fn native_register_consumer_warp_with_license(
    environment: &mut Env<'_>,
    locale: JString<'_>,
    license_key: JString<'_>,
) -> jbyteArray {
    let locale = match locale.try_to_string(environment) {
        Ok(locale) => locale,
        Err(error) => {
            throw_io_error(environment, &error.to_string());
            return std::ptr::null_mut();
        }
    };
    let license_key = match license_key.try_to_string(environment) {
        Ok(value) => Zeroizing::new(value),
        Err(error) => {
            throw_io_error(environment, &error.to_string());
            return std::ptr::null_mut();
        }
    };
    let secret = match register_consumer_warp_with_license(&locale, license_key.as_str()) {
        Ok(secret) => secret,
        Err(error) => {
            throw_io_error(environment, &error);
            return std::ptr::null_mut();
        }
    };
    match environment.byte_array_from_slice(secret.as_bytes()) {
        Ok(output) => output.into_raw(),
        Err(error) => {
            throw_io_error(environment, &error.to_string());
            std::ptr::null_mut()
        }
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_georgexie2333_usque_NativeEngine_nativeRegisterZeroTrustWarp<
    'local,
>(
    mut environment: EnvUnowned<'local>,
    _class: JClass<'local>,
    locale: JString<'local>,
    team: JString<'local>,
    callback_uri: JString<'local>,
) -> jbyteArray {
    with_jni_env(&mut environment, |environment| {
        native_register_zero_trust_warp(environment, locale, team, callback_uri)
    })
}

fn native_register_zero_trust_warp(
    environment: &mut Env<'_>,
    locale: JString<'_>,
    team: JString<'_>,
    callback_uri: JString<'_>,
) -> jbyteArray {
    let locale = match locale.try_to_string(environment) {
        Ok(value) => value,
        Err(error) => {
            throw_io_error(environment, &error.to_string());
            return std::ptr::null_mut();
        }
    };
    let team = match team.try_to_string(environment) {
        Ok(value) => value,
        Err(error) => {
            throw_io_error(environment, &error.to_string());
            return std::ptr::null_mut();
        }
    };
    let callback_uri = match callback_uri.try_to_string(environment) {
        Ok(value) => Zeroizing::new(value),
        Err(error) => {
            throw_io_error(environment, &error.to_string());
            return std::ptr::null_mut();
        }
    };
    let envelope = match register_zero_trust_warp(&locale, &team, &callback_uri) {
        Ok(value) => value,
        Err(error) => {
            throw_io_error(environment, &error);
            return std::ptr::null_mut();
        }
    };
    match environment.byte_array_from_slice(envelope.as_bytes()) {
        Ok(output) => output.into_raw(),
        Err(error) => {
            throw_io_error(environment, &error.to_string());
            std::ptr::null_mut()
        }
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_georgexie2333_usque_NativeEngine_nativeUnbindConsumerWarp<
    'local,
>(
    mut environment: EnvUnowned<'local>,
    _class: JClass<'local>,
    warp_secret: JByteArray<'local>,
) -> jint {
    with_jni_code(&mut environment, |environment| {
        native_unbind_consumer_warp(environment, warp_secret)
    })
}

fn native_unbind_consumer_warp(environment: &mut Env<'_>, warp_secret: JByteArray<'_>) -> jint {
    let secret = match environment.convert_byte_array(&warp_secret) {
        Ok(value) => Zeroizing::new(value),
        Err(error) => {
            throw_io_error(environment, &error.to_string());
            return INVALID_WARP_SECRET;
        }
    };
    match unbind_consumer_warp(&secret) {
        Ok(()) => START_OK,
        Err(error) => {
            throw_io_error(environment, &error);
            START_PLATFORM_FAILURE
        }
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_georgexie2333_usque_NativeEngine_nativeCheckForUpdates<
    'local,
>(
    mut environment: EnvUnowned<'local>,
    _class: JClass<'local>,
) -> jstring {
    with_jni_env(&mut environment, native_check_for_updates)
}

fn native_check_for_updates(environment: &mut Env<'_>) -> jstring {
    let result = match check_for_updates() {
        Ok(result) => result,
        Err(error) => {
            throw_io_error(environment, &error);
            return std::ptr::null_mut();
        }
    };
    match environment.new_string(result) {
        Ok(output) => output.into_raw(),
        Err(error) => {
            throw_io_error(environment, &error.to_string());
            std::ptr::null_mut()
        }
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_georgexie2333_usque_NativeEngine_nativeApplyProfileCommand<
    'local,
>(
    mut environment: EnvUnowned<'local>,
    _class: JClass<'local>,
    config_path: JString<'local>,
    request_json: JString<'local>,
) -> jstring {
    with_jni_env(&mut environment, |environment| {
        native_apply_profile_command(environment, config_path, request_json)
    })
}

fn native_apply_profile_command(
    environment: &mut Env<'_>,
    config_path: JString<'_>,
    request_json: JString<'_>,
) -> jstring {
    let config_path = match config_path.try_to_string(environment) {
        Ok(value) => value,
        Err(error) => {
            throw_io_error(environment, &error.to_string());
            return std::ptr::null_mut();
        }
    };
    let request_json = match request_json.try_to_string(environment) {
        Ok(value) => value,
        Err(error) => {
            throw_io_error(environment, &error.to_string());
            return std::ptr::null_mut();
        }
    };
    let result = match apply_profile_command(&config_path, &request_json) {
        Ok(result) => result,
        Err(error) => {
            throw_io_error(environment, &error);
            return std::ptr::null_mut();
        }
    };
    match environment.new_string(result) {
        Ok(output) => output.into_raw(),
        Err(error) => {
            throw_io_error(environment, &error.to_string());
            std::ptr::null_mut()
        }
    }
}

fn engine_ready() -> bool {
    cfg!(target_os = "android")
}

fn warp_identity_from_secret(secret: &[u8]) -> Result<WarpIdentity, String> {
    let secret = std::str::from_utf8(secret).map_err(|_| "WARP Secret is not UTF-8")?;
    parse_manual_warp_secret(secret).map_err(|error| error.to_string())
}

fn identity_from_secret(secret: &[u8]) -> Result<MasqueTlsIdentity, String> {
    let identity = warp_identity_from_secret(secret)?;
    MasqueTlsIdentity::from_warp_identity(&identity).map_err(|error| error.to_string())
}

fn validate_warp_secret_bytes(secret: &[u8]) -> jint {
    match identity_from_secret(secret) {
        Ok(identity) => {
            drop(identity);
            START_OK
        }
        Err(_) => INVALID_WARP_SECRET,
    }
}

#[derive(Debug, Serialize)]
struct IdentityMetadata {
    ipv4: String,
    ipv6: String,
    provider: &'static str,
    organization: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    entitlement: Option<ConsumerEntitlement>,
}

fn identity_metadata(secret: &[u8]) -> Result<IdentityMetadata, String> {
    let secret = std::str::from_utf8(secret).map_err(|_| "WARP Secret is not UTF-8")?;
    let identity = parse_manual_warp_secret(secret).map_err(|error| error.to_string())?;
    Ok(IdentityMetadata {
        ipv4: identity.assigned_ipv4.to_string(),
        ipv6: identity.assigned_ipv6.to_string(),
        provider: match identity.provider() {
            IdentityProvider::Consumer => "consumer",
            IdentityProvider::ZeroTrust { .. } => "zeroTrust",
        },
        organization: identity.provider().organization().map(ToOwned::to_owned),
        entitlement: identity.entitlement(),
    })
}

#[derive(Debug, Deserialize)]
struct AndroidProfile {
    id: String,
    name: String,
    mode: String,
    #[serde(default)]
    frontends: Option<AndroidFrontends>,
    transport: String,
    ip_policy: String,
    endpoint_v4: String,
    endpoint_v6: String,
    endpoint_port: u16,
    sni: String,
    mtu: u16,
    dns_v4: String,
    dns_v6: String,
    dns_mode: String,
    kill_switch: bool,
    allow_lan: bool,
    auto_connect: bool,
    #[serde(default)]
    bypass_cidrs: Vec<String>,
    #[serde(default)]
    geo_direct_countries: Vec<String>,
    proxy: AndroidProxy,
}

#[derive(Debug, Deserialize)]
struct AndroidFrontends {
    tunnel: bool,
    socks5: bool,
    http: bool,
}

#[derive(Debug, Deserialize)]
struct AndroidProxy {
    socks_ipv4: String,
    socks_ipv6: String,
    socks_port: u16,
    http_ipv4: String,
    http_ipv6: String,
    http_port: u16,
    dns_mode: String,
    #[serde(default = "default_proxy_dns_v4")]
    dns_v4: String,
    #[serde(default = "default_proxy_dns_v6")]
    dns_v6: String,
    #[serde(default)]
    auth_username: Option<String>,
}

fn default_proxy_dns_v4() -> String {
    usque_core::config::DEFAULT_DNS_V4.to_string()
}

fn default_proxy_dns_v6() -> String {
    usque_core::config::DEFAULT_DNS_V6.to_string()
}

fn parse_android_profile(json: &str) -> Result<Profile, String> {
    if json.len() > 256 * 1024 {
        return Err("Android profile exceeds the safety limit".to_owned());
    }
    let source: AndroidProfile =
        serde_json::from_str(json).map_err(|error| format!("invalid Android profile: {error}"))?;
    android_profile_to_core(source)
}

fn android_profile_to_core(source: AndroidProfile) -> Result<Profile, String> {
    let mode = match source.mode.as_str() {
        "vpn" => OperatingMode::Vpn,
        "socks5" => OperatingMode::Socks5,
        "httpProxy" => OperatingMode::HttpProxy,
        _ => return Err("invalid Android operating mode".to_owned()),
    };
    let transport = match source.transport.as_str() {
        "automatic" => TransportPolicy::Auto,
        "http3" => TransportPolicy::Http3,
        "http2" => TransportPolicy::Http2,
        _ => return Err("invalid Android transport policy".to_owned()),
    };
    let ip_policy = match source.ip_policy.as_str() {
        "automatic" => IpPolicy::Auto,
        "preferIpv4" => IpPolicy::PreferIpv4,
        "preferIpv6" => IpPolicy::PreferIpv6,
        "ipv4Only" => IpPolicy::Ipv4Only,
        "ipv6Only" => IpPolicy::Ipv6Only,
        _ => return Err("invalid Android IP policy".to_owned()),
    };
    let dns_mode = match source.dns_mode.as_str() {
        "tunnel" => DnsMode::Tunnel,
        "localConfigured" => DnsMode::LocalConfigured,
        "system" => DnsMode::System,
        _ => return Err("invalid Android DNS mode".to_owned()),
    };
    let proxy_dns_mode = match source.proxy.dns_mode.as_str() {
        "remote" => ProxyDnsMode::Remote,
        "localConfigured" => ProxyDnsMode::LocalConfigured,
        "system" => ProxyDnsMode::System,
        _ => return Err("invalid Android proxy DNS mode".to_owned()),
    };
    let frontends = source
        .frontends
        .map(|frontends| FrontendSettings {
            tunnel: frontends.tunnel,
            socks5: frontends.socks5,
            http: frontends.http,
        })
        .unwrap_or_else(|| match mode {
            OperatingMode::Vpn => FrontendSettings::android_default(),
            OperatingMode::Socks5 => FrontendSettings {
                tunnel: false,
                socks5: true,
                http: true,
            },
            OperatingMode::HttpProxy => FrontendSettings {
                tunnel: false,
                socks5: false,
                http: true,
            },
        });

    let socks_ipv4: IpAddr = parse_value(&source.proxy.socks_ipv4, "SOCKS5 IPv4 listener")?;
    let socks_ipv6: IpAddr = parse_value(&source.proxy.socks_ipv6, "SOCKS5 IPv6 listener")?;
    let http_ipv4: IpAddr = parse_value(&source.proxy.http_ipv4, "HTTP IPv4 listener")?;
    let http_ipv6: IpAddr = parse_value(&source.proxy.http_ipv6, "HTTP IPv6 listener")?;
    let mut profile = Profile {
        id: parse_value(&source.id, "profile ID")?,
        name: source.name,
        mode,
        frontends,
        transport,
        endpoint: EndpointSettings {
            ipv4: parse_value(&source.endpoint_v4, "endpoint IPv4")?,
            ipv6: parse_value(&source.endpoint_v6, "endpoint IPv6")?,
            port: source.endpoint_port,
            sni: source.sni,
        },
        ip_policy,
        mtu: source.mtu,
        dns_mode,
        dns_servers: vec![
            parse_value(&source.dns_v4, "DNS IPv4")?,
            parse_value(&source.dns_v6, "DNS IPv6")?,
        ],
        allow_lan: source.allow_lan,
        split_exclusions: source
            .bypass_cidrs
            .iter()
            .map(|value| parse_value(value, "bypass CIDR"))
            .collect::<Result<Vec<_>, _>>()?,
        kill_switch: source.kill_switch,
        auto_connect: source.auto_connect,
        geo_direct_countries: source.geo_direct_countries,
        proxy: ProxySettings {
            socks5_listeners: vec![
                SocketAddr::new(socks_ipv4, source.proxy.socks_port),
                SocketAddr::new(socks_ipv6, source.proxy.socks_port),
            ],
            http_listeners: vec![
                SocketAddr::new(http_ipv4, source.proxy.http_port),
                SocketAddr::new(http_ipv6, source.proxy.http_port),
            ],
            system_proxy: false,
            udp_idle_timeout_seconds: 60,
            dns_mode: proxy_dns_mode,
            dns_servers: vec![
                parse_value(&source.proxy.dns_v4, "proxy DNS IPv4")?,
                parse_value(&source.proxy.dns_v6, "proxy DNS IPv6")?,
            ],
            auth_username: source.proxy.auth_username.filter(|value| !value.is_empty()),
            auth_password: None,
        },
    };
    profile.canonicalize_mode();
    profile
        .canonicalize_geo_direct()
        .map_err(|error| error.to_string())?;
    profile.validate().map_err(|error| error.to_string())?;
    Ok(profile)
}

fn attach_android_proxy_password(
    mut profile: Profile,
    password: Zeroizing<Vec<u8>>,
) -> Result<Profile, i32> {
    profile.proxy.normalize_auth();
    match profile.proxy.listener_auth_username() {
        None => {
            profile.proxy.auth_password = None;
            Ok(profile)
        }
        Some(_) => {
            if password.is_empty() {
                return Err(START_INVALID_PROFILE);
            }
            profile.proxy.auth_password = Some(password);
            if profile.proxy.listener_credentials().is_err() {
                return Err(START_INVALID_PROFILE);
            }
            Ok(profile)
        }
    }
}

fn parse_value<T>(value: &str, label: &str) -> Result<T, String>
where
    T: std::str::FromStr,
{
    value.parse().map_err(|_| format!("invalid {label}"))
}

#[derive(Debug, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
enum AndroidConfigCommand {
    ImportLegacyProfiles {
        profiles: Vec<AndroidProfile>,
        active_profile_id: String,
    },
    UpsertProfile {
        profile: Box<AndroidProfile>,
        identity_provider: Option<String>,
        organization: Option<String>,
    },
    DeleteProfile {
        profile_id: String,
    },
    SetActiveProfile {
        profile_id: String,
    },
    CompleteIdentityDeletions {
        profile_ids: Vec<String>,
    },
    BeginIdentityCreation {
        profile_id: String,
    },
    CommitProfileWithIdentity {
        profile: Box<AndroidProfile>,
        identity_provider: Option<String>,
        organization: Option<String>,
    },
    CompleteIdentityCreations {
        profile_ids: Vec<String>,
    },
    ClearAllData,
    ListProfiles,
    ReconfigureActiveProfile {
        profile: Box<AndroidProfile>,
    },
    ListGeoRules,
    DownloadGeoRules {
        country_code: String,
    },
    UpdateAllGeoRules,
}

fn apply_profile_command(config_path: &str, request_json: &str) -> Result<String, String> {
    if config_path.len() > 4096 || request_json.len() > 2 * 1024 * 1024 {
        return Err("profile-store request exceeds the safety limit".to_owned());
    }
    let config_path = PathBuf::from(config_path);
    if !config_path.is_absolute()
        || config_path.file_name().and_then(|name| name.to_str()) != Some("profiles-v2.json")
    {
        return Err("Android profile-store path is invalid".to_owned());
    }
    let command: AndroidConfigCommand = serde_json::from_str(request_json)
        .map_err(|error| format!("invalid profile-store command: {error}"))?;
    let clear_all_data = matches!(&command, AndroidConfigCommand::ClearAllData);
    let store = ConfigStore::new(config_path);
    let mut config = store.load_or_default().map_err(|error| error.to_string())?;
    let mut changed = false;

    match command {
        AndroidConfigCommand::ImportLegacyProfiles {
            profiles,
            active_profile_id,
        } => {
            if !config.preferences.profiles_migrated_from_flutter {
                let mut incoming = Vec::new();
                let mut incoming_ids = std::collections::HashSet::new();
                for source in profiles {
                    let profile = android_profile_to_core(source)?;
                    if !incoming_ids.insert(profile.id) {
                        return Err("legacy profile IDs must be unique".to_owned());
                    }
                    incoming.push(profile);
                }
                if !active_profile_id.trim().is_empty() {
                    let active_profile_id = parse_value(&active_profile_id, "active profile ID")?;
                    if !incoming_ids.contains(&active_profile_id)
                        && config.account(active_profile_id).is_none()
                    {
                        return Err("legacy active profile does not exist".to_owned());
                    }
                    config.active_profile_id = Some(active_profile_id);
                }
                if let Some(active) = incoming
                    .iter()
                    .find(|profile| Some(profile.id) == config.active_profile_id)
                {
                    config.network = SharedNetworkSettings::from_profile(active);
                }
                config.profiles.clear();
                for profile in incoming {
                    let managed_endpoint = profile
                        .endpoint
                        .is_zero_trust_managed()
                        .then_some(profile.endpoint.clone());
                    config
                        .insert_account(profile.id, profile.name, managed_endpoint)
                        .map_err(|error| error.to_string())?;
                }
                if config.active_profile().is_none() {
                    config.active_profile_id = config.profiles.first().map(|account| account.id);
                }
                config.preferences.profiles_migrated_from_flutter = true;
                changed = true;
            }
        }
        AndroidConfigCommand::UpsertProfile {
            profile,
            identity_provider,
            organization,
        } => {
            let binding = parse_identity_binding(identity_provider, organization)?;
            let profile = android_profile_to_core(*profile)?;
            let profile_id = profile.id;
            if let (Some(existing), Some(incoming)) =
                (config.identity_bindings.get(&profile.id), binding.as_ref())
                && existing != incoming
            {
                return Err("profile identity provider cannot be changed".to_owned());
            }
            if let Some(binding) = binding {
                config.identity_bindings.insert(profile_id, binding);
            }
            config
                .upsert_runtime_profile(profile)
                .map_err(|error| error.to_string())?;
            config.preferences.profiles_migrated_from_flutter = true;
            changed = true;
        }
        AndroidConfigCommand::DeleteProfile { profile_id } => {
            let profile_id = parse_value(&profile_id, "profile ID")?;
            if config.profiles.len() == 1 {
                return Err("at least one profile must remain".to_owned());
            }
            let index = config
                .profiles
                .iter()
                .position(|profile| profile.id == profile_id)
                .ok_or_else(|| "profile does not exist".to_owned())?;
            config.profiles.remove(index);
            config.identity_bindings.remove(&profile_id);
            if config.active_profile_id == Some(profile_id) {
                config.active_profile_id = config.profiles.first().map(|profile| profile.id);
            }
            if !config.pending_identity_deletions.contains(&profile_id) {
                config.pending_identity_deletions.push(profile_id);
            }
            changed = true;
        }
        AndroidConfigCommand::SetActiveProfile { profile_id } => {
            let profile_id = parse_value(&profile_id, "profile ID")?;
            if !config
                .profiles
                .iter()
                .any(|profile| profile.id == profile_id)
            {
                return Err("profile does not exist".to_owned());
            }
            config.active_profile_id = Some(profile_id);
            changed = true;
        }
        AndroidConfigCommand::CompleteIdentityDeletions { profile_ids } => {
            if profile_ids.len() > usque_core::config::MAX_PROFILES {
                return Err("too many completed identity deletions".to_owned());
            }
            let completed = profile_ids
                .into_iter()
                .map(|profile_id| parse_value::<uuid::Uuid>(&profile_id, "profile ID"))
                .collect::<Result<std::collections::HashSet<_>, _>>()?;
            config
                .pending_identity_deletions
                .retain(|profile_id| !completed.contains(profile_id));
            changed = true;
        }
        AndroidConfigCommand::BeginIdentityCreation { profile_id } => {
            let profile_id = parse_value(&profile_id, "profile ID")?;
            if config
                .profiles
                .iter()
                .any(|profile| profile.id == profile_id)
            {
                return Err("profile already exists".to_owned());
            }
            if !config.pending_identity_creations.contains(&profile_id) {
                config.pending_identity_creations.push(profile_id);
            }
            changed = true;
        }
        AndroidConfigCommand::CommitProfileWithIdentity {
            profile,
            identity_provider,
            organization,
        } => {
            let binding = parse_identity_binding(identity_provider, organization)?;
            let profile = android_profile_to_core(*profile)?;
            if !config.pending_identity_creations.contains(&profile.id) {
                return Err("profile identity creation was not prepared".to_owned());
            }
            if config
                .profiles
                .iter()
                .any(|existing| existing.id == profile.id)
            {
                return Err("profile already exists".to_owned());
            }
            config
                .pending_identity_creations
                .retain(|profile_id| *profile_id != profile.id);
            if let Some(binding) = binding {
                config.identity_bindings.insert(profile.id, binding);
            }
            let managed_endpoint = profile
                .endpoint
                .is_zero_trust_managed()
                .then_some(profile.endpoint.clone());
            config
                .insert_account(profile.id, profile.name, managed_endpoint)
                .map_err(|error| error.to_string())?;
            config.preferences.profiles_migrated_from_flutter = true;
            changed = true;
        }
        AndroidConfigCommand::CompleteIdentityCreations { profile_ids } => {
            if profile_ids.len() > usque_core::config::MAX_PROFILES {
                return Err("too many completed identity creations".to_owned());
            }
            let completed = profile_ids
                .into_iter()
                .map(|profile_id| parse_value::<uuid::Uuid>(&profile_id, "profile ID"))
                .collect::<Result<std::collections::HashSet<_>, _>>()?;
            config
                .pending_identity_creations
                .retain(|profile_id| !completed.contains(profile_id));
            changed = true;
        }
        AndroidConfigCommand::ClearAllData => {
            config = AppConfig::default();
            changed = true;
        }
        AndroidConfigCommand::ListProfiles => {}
        AndroidConfigCommand::ReconfigureActiveProfile { profile } => {
            let next = android_profile_to_core(*profile)?;
            if config.active_profile_id != Some(next.id) {
                return Err("only the Active Profile can be reconfigured".to_owned());
            }
            if !config
                .profiles
                .iter()
                .any(|candidate| candidate.id == next.id)
            {
                return Err("profile does not exist".to_owned());
            }
            config
                .upsert_runtime_profile(next)
                .map_err(|error| error.to_string())?;
            changed = true;
        }
        AndroidConfigCommand::ListGeoRules
        | AndroidConfigCommand::DownloadGeoRules { .. }
        | AndroidConfigCommand::UpdateAllGeoRules => {
            return apply_geo_command(store.path(), command);
        }
    }

    config.validate().map_err(|error| error.to_string())?;
    if changed {
        store.save(&config).map_err(|error| error.to_string())?;
        if clear_all_data {
            let _ = std::fs::remove_file(store.backup_path());
        }
    }
    serde_json::to_string(&android_profile_catalog(&config)).map_err(|error| error.to_string())
}

fn apply_geo_command(
    config_path: &std::path::Path,
    command: AndroidConfigCommand,
) -> Result<String, String> {
    let cache_dir = config_path
        .parent()
        .ok_or_else(|| "Android profile-store path is invalid".to_owned())?;
    match command {
        AndroidConfigCommand::ListGeoRules => {
            let (entries, last_successful_update_unix_milliseconds) =
                usque_core::list_geo_rules(cache_dir).map_err(|error| error.to_string())?;
            serde_json::to_string(&serde_json::json!({
                "entries": entries.iter().map(|entry| serde_json::json!({
                    "country_code": entry.country_code,
                    "has_geoip": entry.has_geoip,
                    "has_geosite": entry.has_geosite,
                    "last_updated_unix_milliseconds": entry.last_updated_unix_milliseconds,
                })).collect::<Vec<_>>(),
                "last_successful_update_unix_milliseconds": last_successful_update_unix_milliseconds,
            }))
            .map_err(|error| error.to_string())
        }
        AndroidConfigCommand::DownloadGeoRules { country_code } => {
            geo_update_json(cache_dir, Some(country_code))
        }
        AndroidConfigCommand::UpdateAllGeoRules => geo_update_json(cache_dir, None),
        _ => Err("unsupported geo command".to_owned()),
    }
}

fn geo_update_json(
    cache_dir: &std::path::Path,
    country_code: Option<String>,
) -> Result<String, String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("geo runtime failed: {error}"))?;
    let fetch = usque_geo::ReqwestFetch::new().map_err(|error| error.to_string())?;
    let downloader = usque_geo::GeoDownloader::new(fetch, cache_dir);
    let results = runtime
        .block_on(async {
            match country_code.as_deref() {
                Some(country) => usque_core::download_geo_rules(&downloader, country, |_| {}).await,
                None => usque_core::update_all_geo_rules(&downloader, |_| {}).await,
            }
        })
        .map_err(|error| error.to_string())?;
    serde_json::to_string(&serde_json::json!({
        "results": results.iter().map(|result| {
            let (status, reason) = match &result.status {
                usque_geo::UpdateStatus::UpToDate => ("up_to_date", String::new()),
                usque_geo::UpdateStatus::Updated => ("updated", String::new()),
                usque_geo::UpdateStatus::Failed { reason } => ("failed", reason.clone()),
            };
            serde_json::json!({
                "country_code": result.country_code,
                "status": status,
                "reason": reason,
                "artifact_kind": result.artifact_kind,
            })
        }).collect::<Vec<_>>(),
    }))
    .map_err(|error| error.to_string())
}

fn android_profile_catalog(config: &AppConfig) -> serde_json::Value {
    serde_json::json!({
        "profiles": config
            .runtime_profiles()
            .iter()
            .map(|profile| {
                android_profile_value(profile, config.identity_bindings.get(&profile.id))
            })
            .collect::<Vec<_>>(),
        "active_profile_id": config
            .active_profile_id
            .map(|id| id.to_string())
            .unwrap_or_default(),
        "pending_identity_deletions": config
            .pending_identity_deletions
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        "pending_identity_creations": config
            .pending_identity_creations
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
    })
}

fn android_profile_value(
    profile: &Profile,
    binding: Option<&IdentityProvider>,
) -> serde_json::Value {
    let dns_ipv4 = profile
        .dns_servers
        .iter()
        .find(|address| address.is_ipv4())
        .copied()
        .unwrap_or_else(|| usque_core::config::DEFAULT_DNS_V4.into());
    let dns_ipv6 = profile
        .dns_servers
        .iter()
        .find(|address| address.is_ipv6())
        .copied()
        .unwrap_or_else(|| usque_core::config::DEFAULT_DNS_V6.into());
    let socks_ipv4 = listener_for_family(&profile.proxy.socks5_listeners, true);
    let socks_ipv6 = listener_for_family(&profile.proxy.socks5_listeners, false);
    let http_ipv4 = listener_for_family(&profile.proxy.http_listeners, true);
    let http_ipv6 = listener_for_family(&profile.proxy.http_listeners, false);
    serde_json::json!({
        "id": profile.id.to_string(),
        "name": profile.name,
        "mode": match profile.mode {
            OperatingMode::Vpn => "vpn",
            OperatingMode::Socks5 => "socks5",
            OperatingMode::HttpProxy => "httpProxy",
        },
        "frontends": {
            "tunnel": profile.frontends.tunnel,
            "socks5": profile.frontends.socks5,
            "http": profile.frontends.http,
        },
        "transport": match profile.transport {
            TransportPolicy::Auto => "automatic",
            TransportPolicy::Http3 => "http3",
            TransportPolicy::Http2 => "http2",
        },
        "ip_policy": match profile.ip_policy {
            IpPolicy::Auto => "automatic",
            IpPolicy::PreferIpv4 => "preferIpv4",
            IpPolicy::PreferIpv6 => "preferIpv6",
            IpPolicy::Ipv4Only => "ipv4Only",
            IpPolicy::Ipv6Only => "ipv6Only",
        },
        "endpoint_v4": profile.endpoint.ipv4.to_string(),
        "endpoint_v6": profile.endpoint.ipv6.to_string(),
        "endpoint_port": profile.endpoint.port,
        "sni": profile.endpoint.sni,
        "identity_provider": match binding {
            Some(IdentityProvider::Consumer) => "consumer",
            Some(IdentityProvider::ZeroTrust { .. }) => "zero_trust",
            None => "",
        },
        "identity_organization": binding
            .and_then(IdentityProvider::organization)
            .unwrap_or_default(),
        "mtu": profile.mtu,
        "dns_v4": dns_ipv4.to_string(),
        "dns_v6": dns_ipv6.to_string(),
        "dns_mode": match profile.dns_mode {
            DnsMode::Tunnel => "tunnel",
            DnsMode::LocalConfigured => "localConfigured",
            DnsMode::System => "system",
        },
        "kill_switch": profile.kill_switch,
        "allow_lan": profile.allow_lan,
        "auto_connect": profile.auto_connect,
        "bypass_cidrs": profile
            .split_exclusions
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        "geo_direct_countries": profile.geo_direct_countries,
        "proxy": {
            "socks_ipv4": socks_ipv4.ip().to_string(),
            "socks_ipv6": socks_ipv6.ip().to_string(),
            "socks_port": socks_ipv4.port(),
            "http_ipv4": http_ipv4.ip().to_string(),
            "http_ipv6": http_ipv6.ip().to_string(),
            "http_port": http_ipv4.port(),
            "dns_mode": match profile.proxy.dns_mode {
                ProxyDnsMode::Remote => "remote",
                ProxyDnsMode::LocalConfigured => "localConfigured",
                ProxyDnsMode::System => "system",
            },
            "dns_v4": profile
                .proxy
                .dns_servers
                .iter()
                .find(|server| server.is_ipv4())
                .copied()
                .unwrap_or_else(|| usque_core::config::DEFAULT_DNS_V4.into())
                .to_string(),
            "dns_v6": profile
                .proxy
                .dns_servers
                .iter()
                .find(|server| server.is_ipv6())
                .copied()
                .unwrap_or_else(|| usque_core::config::DEFAULT_DNS_V6.into())
                .to_string(),
            "system_proxy": profile.proxy.system_proxy,
            "auth_username": profile
                .proxy
                .listener_auth_username()
                .unwrap_or_default(),
        }
    })
}

fn parse_identity_binding(
    provider: Option<String>,
    organization: Option<String>,
) -> Result<Option<IdentityProvider>, String> {
    match provider.as_deref() {
        None | Some("") if organization.as_deref().unwrap_or_default().is_empty() => Ok(None),
        Some("consumer") if organization.as_deref().unwrap_or_default().is_empty() => {
            Ok(Some(IdentityProvider::Consumer))
        }
        Some("zero_trust") => {
            let organization = organization.ok_or_else(|| {
                "Zero Trust identity binding is missing its organization".to_owned()
            })?;
            IdentityProvider::zero_trust(organization)
                .map(Some)
                .map_err(|_| "Zero Trust identity binding is invalid".to_owned())
        }
        _ => Err("profile identity binding is invalid".to_owned()),
    }
}

fn listener_for_family(listeners: &[SocketAddr], ipv4: bool) -> SocketAddr {
    listeners
        .iter()
        .find(|listener| listener.is_ipv4() == ipv4)
        .copied()
        .unwrap_or_else(|| {
            if ipv4 {
                "127.0.0.1:0".parse().expect("static IPv4 listener")
            } else {
                "[::1]:0".parse().expect("static IPv6 listener")
            }
        })
}

struct AndroidSocketProtector {
    java_vm: JavaVM,
    service: Global<JObject<'static>>,
    policy: AndroidSocketRoutePolicy,
    network_generation: AtomicU64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AndroidSocketRoutePolicy {
    Vpn,
    Proxy,
}

impl AndroidSocketRoutePolicy {
    fn requires_vpn_protection(self) -> bool {
        matches!(self, Self::Vpn)
    }
}

#[cfg(target_os = "android")]
#[link(name = "android")]
unsafe extern "C" {
    fn android_setsocknetwork(network: u64, fd: libc::c_int) -> libc::c_int;
}

impl SocketProtector for AndroidSocketProtector {
    fn protect(&self, socket: SocketHandle) -> Result<(), String> {
        let descriptor = jint::try_from(socket.value())
            .map_err(|_| "endpoint socket descriptor is out of range".to_owned())?;
        let (protected, network) = self
            .java_vm
            .attach_current_thread(|environment| -> jni::errors::Result<_> {
                let protected = if self.policy.requires_vpn_protection() {
                    environment
                        .call_method(
                            &self.service,
                            jni_str!("protect"),
                            jni_sig!("(I)Z"),
                            &[JValue::Int(descriptor)],
                        )
                        .and_then(|value| value.z())?
                } else {
                    true
                };

                #[cfg(target_os = "android")]
                let network = environment
                    .call_method(
                        &self.service,
                        jni_str!("getUnderlyingNetworkHandle"),
                        jni_sig!("()J"),
                        &[],
                    )
                    .and_then(|value| value.j())?;
                #[cfg(not(target_os = "android"))]
                let network = 0;

                Ok((protected, network))
            })
            .map_err(|error| format!("attach VpnService protector thread: {error}"))?;
        if !protected {
            return Err("VpnService.protect rejected the endpoint socket".to_owned());
        }

        #[cfg(target_os = "android")]
        {
            if network <= 0 {
                return Err("Android has no selected non-VPN physical network".to_owned());
            }
            // SAFETY: descriptor is a live socket FD; network is a valid
            // Android Network handle from getUnderlyingNetworkHandle.
            let result =
                unsafe { android_setsocknetwork(network as u64, descriptor as libc::c_int) };
            if result != 0 {
                return Err(format!(
                    "bind endpoint socket to Android network failed: errno {}",
                    result.saturating_neg()
                ));
            }
        }
        #[cfg(not(target_os = "android"))]
        let _ = network;
        Ok(())
    }

    fn endpoint_family_available(&self, endpoint: SocketAddr) -> Option<bool> {
        let mask = self
            .java_vm
            .attach_current_thread(|environment| -> jni::errors::Result<_> {
                environment
                    .call_method(
                        &self.service,
                        jni_str!("getUnderlyingFamilyMask"),
                        jni_sig!("()I"),
                        &[],
                    )
                    .and_then(|value| value.i())
            })
            .ok()?;
        if mask == 0 {
            return None;
        }
        Some(if endpoint.is_ipv4() {
            mask & 0x1 != 0
        } else {
            mask & 0x2 != 0
        })
    }

    fn network_generation(&self) -> Option<u64> {
        Some(self.network_generation.load(Ordering::Acquire))
    }

    fn resolve(&self, host: &str, port: u16) -> Result<Vec<SocketAddr>, String> {
        let resolved = self
            .java_vm
            .attach_current_thread(|environment| -> jni::errors::Result<Vec<String>> {
                let host = environment.new_string(host)?;
                let host_object = JObject::from(host);
                let resolved = environment
                    .call_method(
                        &self.service,
                        jni_str!("resolveUnderlyingHost"),
                        jni_sig!("(Ljava/lang/String;)[Ljava/lang/String;"),
                        &[JValue::Object(&host_object)],
                    )?
                    .l()?;
                let resolved =
                    environment.cast_local::<JObjectArray<JString<'static>>>(resolved)?;
                let length = resolved.len(environment)?;
                let mut values = Vec::with_capacity(length);
                for index in 0..length {
                    let value = resolved.get_element(environment, index)?;
                    values.push(value.try_to_string(environment)?);
                }
                Ok(values)
            })
            .map_err(|error| format!("resolve on Android underlying network: {error}"))?;
        let mut addresses = Vec::with_capacity(resolved.len());
        for value in resolved {
            if let Ok(ip) = value
                .split('%')
                .next()
                .unwrap_or_default()
                .parse::<IpAddr>()
                && !ip.is_unspecified()
                && !ip.is_multicast()
            {
                addresses.push(SocketAddr::new(ip, port));
            }
        }
        addresses.sort();
        addresses.dedup();
        addresses.truncate(16);
        if addresses.is_empty() {
            Err("Android underlying network returned no usable address".to_owned())
        } else {
            Ok(addresses)
        }
    }
}

#[cfg(target_os = "android")]
struct AndroidEndpointPinRefresher {
    profile_id: String,
    identity: tokio::sync::Mutex<WarpIdentity>,
    protector: Arc<AndroidSocketProtector>,
}

#[cfg(target_os = "android")]
impl AndroidEndpointPinRefresher {
    fn persist(&self, identity: &WarpIdentity) -> Result<(), TransportError> {
        let portable = identity
            .to_portable_secret_json()
            .map_err(|error| TransportError::EndpointPinRefresh(error.to_string()))?;
        let persisted = self
            .protector
            .java_vm
            .attach_current_thread(|environment| -> jni::errors::Result<_> {
                let profile_id = environment.new_string(&self.profile_id)?;
                let secret = environment.byte_array_from_slice(portable.as_bytes())?;
                let profile_object = JObject::from(profile_id);
                let secret_object = JObject::from(secret);
                environment
                    .call_method(
                        &self.protector.service,
                        jni_str!("persistRefreshedWarpIdentity"),
                        jni_sig!("(Ljava/lang/String;[B)Z"),
                        &[
                            JValue::Object(&profile_object),
                            JValue::Object(&secret_object),
                        ],
                    )
                    .and_then(|value| value.z())
            })
            .map_err(|error| TransportError::EndpointPinRefresh(error.to_string()))?;
        if persisted {
            Ok(())
        } else {
            Err(TransportError::EndpointPinRefresh(
                "Android Keystore rejected the refreshed enrollment".to_owned(),
            ))
        }
    }
}

#[cfg(target_os = "android")]
#[async_trait::async_trait]
impl EndpointPinRefresher for AndroidEndpointPinRefresher {
    async fn refresh(
        &self,
        protector: Arc<dyn SocketProtector>,
    ) -> Result<MasqueTlsIdentity, TransportError> {
        let mut identity = self.identity.lock().await;
        let refresh =
            refresh_endpoint_pin_over_protected_socket(&identity, None, protector).await?;
        let previous_pin = identity.endpoint_pin.clone();
        let previous_ipv4 = identity.assigned_ipv4;
        let previous_ipv6 = identity.assigned_ipv6;
        identity.endpoint_pin = refresh.endpoint_pin;
        identity.assigned_ipv4 = refresh.assigned_ipv4;
        identity.assigned_ipv6 = refresh.assigned_ipv6;
        let tls = match MasqueTlsIdentity::from_warp_identity(&identity) {
            Ok(tls) => tls,
            Err(error) => {
                identity.endpoint_pin = previous_pin;
                identity.assigned_ipv4 = previous_ipv4;
                identity.assigned_ipv6 = previous_ipv6;
                return Err(error);
            }
        };
        if let Err(error) = self.persist(&identity) {
            identity.endpoint_pin = previous_pin;
            identity.assigned_ipv4 = previous_ipv4;
            identity.assigned_ipv6 = previous_ipv6;
            return Err(error);
        }
        Ok(tls)
    }
}

#[derive(Debug, Clone, Serialize)]
struct NativeSnapshot {
    phase: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    warning: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    transport: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    address_family: Option<String>,
    download_bytes_per_second: u64,
    upload_bytes_per_second: u64,
    downloaded_bytes: u64,
    uploaded_bytes: u64,
    reconnect_count: u32,
    active_listeners: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    exit_ipv4: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    exit_ipv6: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    exit_city: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    exit_country: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    exit_country_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    exit_flag_svg: Option<String>,
}

impl NativeSnapshot {
    fn disconnected() -> Self {
        Self {
            phase: "disconnected".to_owned(),
            warning: None,
            error_code: None,
            transport: None,
            address_family: None,
            download_bytes_per_second: 0,
            upload_bytes_per_second: 0,
            downloaded_bytes: 0,
            uploaded_bytes: 0,
            reconnect_count: 0,
            active_listeners: Vec::new(),
            exit_ipv4: None,
            exit_ipv6: None,
            exit_city: None,
            exit_country: None,
            exit_country_code: None,
            exit_flag_svg: None,
        }
    }

    #[cfg(target_os = "android")]
    fn preparing() -> Self {
        Self {
            phase: "preparing".to_owned(),
            ..Self::disconnected()
        }
    }
}

#[cfg(target_os = "android")]
mod android_runtime;

fn start_engine(
    tun_file_descriptor: jint,
    profile: Profile,
    identity: WarpIdentity,
    protector: Arc<AndroidSocketProtector>,
) -> jint {
    #[cfg(target_os = "android")]
    {
        android_runtime::start(tun_file_descriptor, profile, identity, protector)
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (tun_file_descriptor, profile, identity, protector);
        START_NOT_READY
    }
}

fn start_proxy_engine(
    profile: Profile,
    identity: WarpIdentity,
    protector: Arc<AndroidSocketProtector>,
) -> jint {
    #[cfg(target_os = "android")]
    {
        android_runtime::start_proxy(profile, identity, protector)
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (profile, identity, protector);
        START_NOT_READY
    }
}

fn stop_engine() {
    #[cfg(target_os = "android")]
    android_runtime::stop();
}

fn cancel_engine() {
    #[cfg(target_os = "android")]
    android_runtime::cancel();
}

fn notify_network_changed(generation: u64) {
    #[cfg(target_os = "android")]
    android_runtime::notify_network_changed(generation);
    #[cfg(not(target_os = "android"))]
    let _ = generation;
}

fn engine_snapshot() -> NativeSnapshot {
    #[cfg(target_os = "android")]
    {
        android_runtime::snapshot()
    }
    #[cfg(not(target_os = "android"))]
    {
        NativeSnapshot::disconnected()
    }
}

fn reconfigure_engine(profile: Profile) -> jint {
    #[cfg(target_os = "android")]
    {
        android_runtime::reconfigure(profile)
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = profile;
        RECONFIGURE_NOT_RUNNING
    }
}

fn attach_tun_engine(tun_file_descriptor: jint, profile: Profile) -> jint {
    #[cfg(target_os = "android")]
    {
        android_runtime::attach_tun(tun_file_descriptor, profile)
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (tun_file_descriptor, profile);
        RECONFIGURE_NOT_RUNNING
    }
}

fn detach_tun_engine() -> jint {
    #[cfg(target_os = "android")]
    {
        android_runtime::detach_tun()
    }
    #[cfg(not(target_os = "android"))]
    {
        RECONFIGURE_NOT_RUNNING
    }
}

fn register_consumer_warp(locale: &str) -> Result<Zeroizing<String>, String> {
    if locale.trim().is_empty() || locale.chars().count() > 32 {
        return Err("USQUE_CONSUMER_INVALID_LOCALE".to_owned());
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| "USQUE_CONSUMER_RUNTIME_INITIALIZATION_FAILED".to_owned())?;
    let client = ConsumerRegistrationClient::new()
        .map_err(|_| "USQUE_CONSUMER_HTTP_CLIENT_INITIALIZATION_FAILED".to_owned())?;
    let identity = runtime
        .block_on(client.register(&RegistrationOptions {
            terms_accepted: true,
            model: "Android".to_owned(),
            device_name: None,
            locale: locale.to_owned(),
        }))
        .map_err(|error| safe_consumer_registration_error(&error))?;
    identity
        .to_portable_secret_json()
        .map_err(|_| "USQUE_CONSUMER_IDENTITY_SERIALIZATION_FAILED".to_owned())
}

fn register_consumer_warp_with_license(
    locale: &str,
    license_key: &str,
) -> Result<Zeroizing<String>, String> {
    if locale.trim().is_empty() || locale.chars().count() > 32 {
        return Err("USQUE_CONSUMER_INVALID_LOCALE".to_owned());
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| "USQUE_CONSUMER_RUNTIME_INITIALIZATION_FAILED".to_owned())?;
    let client = ConsumerRegistrationClient::new()
        .map_err(|_| "USQUE_CONSUMER_HTTP_CLIENT_INITIALIZATION_FAILED".to_owned())?;
    let identity = runtime
        .block_on(client.register_with_license(
            &RegistrationOptions {
                terms_accepted: true,
                model: "Android".to_owned(),
                device_name: None,
                locale: locale.to_owned(),
            },
            license_key,
        ))
        .map_err(|error| safe_consumer_registration_error(&error))?;
    identity
        .to_portable_secret_json()
        .map_err(|_| "USQUE_CONSUMER_IDENTITY_SERIALIZATION_FAILED".to_owned())
}

fn safe_consumer_registration_error(error: &RegistrationError) -> String {
    match error {
        RegistrationError::InvalidLicenseKey => "USQUE_CONSUMER_INVALID_LICENSE_KEY".to_owned(),
        RegistrationError::Http(_) => "USQUE_CONSUMER_NETWORK".to_owned(),
        RegistrationError::Api { status, .. } => {
            format!("USQUE_CONSUMER_HTTP:{}", status.as_u16())
        }
        _ => "USQUE_CONSUMER_REGISTRATION_FAILED".to_owned(),
    }
}

#[derive(Serialize)]
struct AndroidZeroTrustRegistration<'a> {
    warp_secret: &'a str,
    identity_metadata: &'a str,
    endpoint_v4: String,
    endpoint_v6: String,
    endpoint_port: u16,
    sni: &'a str,
    organization: &'a str,
}

fn register_zero_trust_warp(
    locale: &str,
    team: &str,
    callback_uri: &str,
) -> Result<Zeroizing<String>, String> {
    if locale.trim().is_empty() || locale.chars().count() > 32 {
        return Err("Android locale is invalid".to_owned());
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("registration runtime failed: {error}"))?;
    let client = ConsumerRegistrationClient::new().map_err(|error| error.to_string())?;
    let result = runtime
        .block_on(client.register_zero_trust(
            &RegistrationOptions {
                terms_accepted: true,
                model: "Android".to_owned(),
                device_name: None,
                locale: locale.to_owned(),
            },
            team,
            callback_uri,
        ))
        .map_err(|error| safe_zero_trust_registration_error(&error))?;
    let secret = result
        .identity
        .to_portable_secret_json()
        .map_err(|_| "USQUE_ZT_LOCAL_COMMIT_FAILED".to_owned())?;
    let metadata = result
        .identity
        .provider()
        .to_metadata_json()
        .map_err(|_| "USQUE_ZT_LOCAL_COMMIT_FAILED".to_owned())?;
    let metadata =
        std::str::from_utf8(&metadata).map_err(|_| "USQUE_ZT_LOCAL_COMMIT_FAILED".to_owned())?;
    let organization = result
        .identity
        .provider()
        .organization()
        .ok_or_else(|| "USQUE_ZT_CONTRACT_CHANGED".to_owned())?;
    serde_json::to_string(&AndroidZeroTrustRegistration {
        warp_secret: &secret,
        identity_metadata: metadata,
        endpoint_v4: result.endpoint.ipv4.to_string(),
        endpoint_v6: result.endpoint.ipv6.to_string(),
        endpoint_port: result.endpoint.port,
        sni: &result.endpoint.sni,
        organization,
    })
    .map(Zeroizing::new)
    .map_err(|_| "USQUE_ZT_LOCAL_COMMIT_FAILED".to_owned())
}

fn safe_zero_trust_registration_error(error: &RegistrationError) -> String {
    match error {
        RegistrationError::InvalidZeroTrustTeam => "USQUE_ZT_TEAM_INVALID".to_owned(),
        RegistrationError::InvalidZeroTrustCallback => "USQUE_ZT_CALLBACK_INVALID".to_owned(),
        RegistrationError::ZeroTrustLoginExpired => "USQUE_ZT_LOGIN_EXPIRED".to_owned(),
        RegistrationError::ZeroTrustLoginDenied => "USQUE_ZT_LOGIN_DENIED".to_owned(),
        RegistrationError::ZeroTrustRegistrationFailed { stage, status } => {
            format!("USQUE_ZT_HTTP:{}:{}", stage.as_code(), status.as_u16())
        }
        RegistrationError::ZeroTrustNetwork { stage } => {
            format!("USQUE_ZT_NETWORK:{}", stage.as_code())
        }
        RegistrationError::ZeroTrustContractChanged => "USQUE_ZT_CONTRACT_CHANGED".to_owned(),
        RegistrationError::TermsNotAccepted => "USQUE_ZT_DIAGNOSTIC:terms_not_accepted".to_owned(),
        RegistrationError::InvalidRegistrationOptions => {
            "USQUE_ZT_DIAGNOSTIC:invalid_registration_options".to_owned()
        }
        RegistrationError::InvalidApiUrl => "USQUE_ZT_DIAGNOSTIC:invalid_api_url".to_owned(),
        RegistrationError::InvalidDeviceId => "USQUE_ZT_DIAGNOSTIC:invalid_device_id".to_owned(),
        RegistrationError::InvalidLicenseKey => {
            "USQUE_ZT_DIAGNOSTIC:unexpected_license_error".to_owned()
        }
        RegistrationError::ApiResponseTooLarge => {
            "USQUE_ZT_DIAGNOSTIC:response_too_large".to_owned()
        }
        RegistrationError::InvalidApiResponse => {
            "USQUE_ZT_DIAGNOSTIC:invalid_api_response".to_owned()
        }
        RegistrationError::RequestSerialization => {
            "USQUE_ZT_DIAGNOSTIC:request_serialization".to_owned()
        }
        RegistrationError::Api { status, .. } => {
            format!("USQUE_ZT_HTTP:unknown:{}", status.as_u16())
        }
        RegistrationError::Http(_) => "USQUE_ZT_NETWORK:unknown".to_owned(),
        RegistrationError::Identity(_) => "USQUE_ZT_DIAGNOSTIC:identity_contract".to_owned(),
    }
}

fn unbind_consumer_warp(secret: &[u8]) -> Result<(), String> {
    let identity = warp_identity_from_secret(secret).map_err(|error| error.to_string())?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("license cleanup runtime failed: {error}"))?;
    let client = ConsumerRegistrationClient::new().map_err(|error| error.to_string())?;
    runtime
        .block_on(client.unbind_license(&identity))
        .map_err(|error| error.to_string())
}

fn check_for_updates() -> Result<String, String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("update runtime failed: {error}"))?;
    let checker = UpdateChecker::new().map_err(|error| error.to_string())?;
    let result = runtime
        .block_on(checker.check(env!("CARGO_PKG_VERSION")))
        .map_err(|error| error.to_string())?;
    serde_json::to_string(&result).map_err(|error| error.to_string())
}

fn throw_io_error(environment: &mut Env<'_>, message: &str) {
    let message: String = message.chars().take(512).collect();
    let message = JNIString::from(message);
    let _ = environment.throw_new(jni_str!("java/io/IOException"), &message);
}

#[cfg(any(test, target_os = "android"))]
fn wait_jni_command_reply(
    reply_rx: std::sync::mpsc::Receiver<i32>,
    cancelled: &AtomicBool,
    timeout: Duration,
) -> i32 {
    match reply_rx.recv_timeout(timeout) {
        Ok(code) => code,
        Err(_) => {
            cancelled.store(true, Ordering::Release);
            START_PLATFORM_FAILURE
        }
    }
}

#[cfg(any(test, target_os = "android"))]
fn jni_command_abandoned(cancelled: &AtomicBool) -> bool {
    cancelled.load(Ordering::Acquire)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jni_boundary_failure_default_never_reports_success() {
        assert_eq!(JniCode::default().0, START_PLATFORM_FAILURE);
    }

    #[test]
    fn jni_command_timeout_cancels_in_flight_command() {
        let (_tx, rx) = std::sync::mpsc::sync_channel::<i32>(1);
        let cancelled = AtomicBool::new(false);
        let code = wait_jni_command_reply(rx, &cancelled, Duration::from_millis(1));
        assert_eq!(code, START_PLATFORM_FAILURE);
        assert!(jni_command_abandoned(&cancelled));
    }

    #[test]
    fn jni_command_reply_success_leaves_command_active() {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let cancelled = AtomicBool::new(false);
        tx.send(RECONFIGURE_OK).unwrap();
        let code = wait_jni_command_reply(rx, &cancelled, Duration::from_secs(1));
        assert_eq!(code, RECONFIGURE_OK);
        assert!(!jni_command_abandoned(&cancelled));
    }

    #[test]
    fn jni_command_wait_does_not_hold_the_engine_slot() {
        let slot = std::sync::Mutex::new(Some(1u8));
        let (_reply_tx, reply_rx) = std::sync::mpsc::sync_channel::<i32>(1);
        let cancelled = AtomicBool::new(false);
        std::thread::scope(|scope| {
            scope.spawn(|| {
                let code = wait_jni_command_reply(reply_rx, &cancelled, Duration::from_millis(50));
                assert_eq!(code, START_PLATFORM_FAILURE);
            });
            let taken = slot.lock().unwrap().take();
            assert_eq!(taken, Some(1));
        });
        assert!(jni_command_abandoned(&cancelled));
    }

    #[test]
    fn proxy_socket_routing_does_not_require_vpn_permission() {
        assert!(AndroidSocketRoutePolicy::Vpn.requires_vpn_protection());
        assert!(!AndroidSocketRoutePolicy::Proxy.requires_vpn_protection());
    }

    #[test]
    fn zero_trust_native_errors_expose_only_safe_stage_and_status() {
        let network = safe_zero_trust_registration_error(&RegistrationError::ZeroTrustNetwork {
            stage: usque_core::ZeroTrustRegistrationStage::MasqueEnrollment,
        });
        assert_eq!(network, "USQUE_ZT_NETWORK:masque_enrollment");
        assert!(!network.contains("token"));
    }

    #[test]
    fn consumer_registration_errors_never_use_zero_trust_codes() {
        for error in [
            register_consumer_warp("").unwrap_err(),
            register_consumer_warp_with_license("", "unused").unwrap_err(),
        ] {
            assert!(!error.contains("USQUE_ZT_"), "{error}");
        }
    }

    fn valid_profile_json() -> String {
        serde_json::json!({
            "id": "8c30b771-9ebd-457a-b67b-bbc74a1ddba6",
            "name": "Default",
            "mode": "vpn",
            "transport": "automatic",
            "ip_policy": "automatic",
            "endpoint_v4": "162.159.198.2",
            "endpoint_v6": "2606:4700:103::2",
            "endpoint_port": 443,
            "sni": "www.visa.cn",
            "mtu": 1280,
            "dns_v4": "1.1.1.1",
            "dns_v6": "2606:4700:4700::1111",
            "dns_mode": "tunnel",
            "kill_switch": true,
            "allow_lan": false,
            "auto_connect": false,
            "bypass_cidrs": [],
            "proxy": {
                "socks_ipv4": "127.0.0.1",
                "socks_ipv6": "::1",
                "socks_port": 1080,
                "http_ipv4": "127.0.0.1",
                "http_ipv6": "::1",
                "http_port": 8080,
                "dns_mode": "remote",
                "dns_v4": "1.1.1.1",
                "dns_v6": "2606:4700:4700::1111",
                "system_proxy": false
            }
        })
        .to_string()
    }

    #[test]
    fn bootstrap_boundary_is_platform_explicit() {
        assert_eq!(engine_ready(), cfg!(target_os = "android"));
    }

    #[test]
    fn android_profile_maps_optional_listener_username_without_password() {
        let mut source: serde_json::Value = serde_json::from_str(&valid_profile_json()).unwrap();
        source["proxy"]["auth_username"] = serde_json::json!("lan-user");
        let profile = parse_android_profile(&source.to_string()).unwrap();
        assert_eq!(profile.proxy.auth_username.as_deref(), Some("lan-user"));
        assert!(profile.proxy.auth_password.is_none());
        let exported = android_profile_value(&profile, None);
        assert_eq!(exported["proxy"]["auth_username"], "lan-user");
        assert!(exported["proxy"].get("password").is_none());
        assert!(exported["proxy"].get("auth_password").is_none());
        let attached =
            attach_android_proxy_password(profile, Zeroizing::new(b"s3cret".to_vec())).unwrap();
        assert!(attached.proxy.listener_credentials().unwrap().is_some());
    }

    #[test]
    fn android_profile_maps_to_the_shared_validated_contract() {
        let profile = parse_android_profile(&valid_profile_json()).unwrap();
        assert_eq!(profile.mode, OperatingMode::Vpn);
        assert_eq!(profile.transport, TransportPolicy::Auto);
        assert_eq!(profile.endpoint.sni, "www.visa.cn");
        assert_eq!(
            profile.proxy.socks5_listeners[0].to_string(),
            "127.0.0.1:1080"
        );
        assert_eq!(
            profile.proxy.dns_servers,
            vec![
                "1.1.1.1".parse::<IpAddr>().unwrap(),
                "2606:4700:4700::1111".parse::<IpAddr>().unwrap(),
            ]
        );
        assert!(profile.geo_direct_countries.is_empty());
    }

    #[test]
    fn android_profile_accepts_geo_direct_countries_without_expanding_bypass() {
        let mut source: serde_json::Value = serde_json::from_str(&valid_profile_json()).unwrap();
        source["geo_direct_countries"] = serde_json::json!(["CN"]);
        source["bypass_cidrs"] = serde_json::json!(["192.0.2.0/24"]);
        let profile = parse_android_profile(&source.to_string()).unwrap();
        assert_eq!(profile.geo_direct_countries, vec!["CN".to_owned()]);
        assert_eq!(
            profile.split_exclusions,
            vec!["192.0.2.0/24".parse::<ipnet::IpNet>().unwrap()]
        );
        let exported = android_profile_value(&profile, None);
        assert_eq!(exported["geo_direct_countries"], serde_json::json!(["CN"]));
        assert_eq!(
            exported["bypass_cidrs"],
            serde_json::json!(["192.0.2.0/24"])
        );
    }

    #[test]
    fn android_profile_rejects_invalid_routes_and_modes() {
        let mut profile: serde_json::Value = serde_json::from_str(&valid_profile_json()).unwrap();
        profile["bypass_cidrs"] = serde_json::json!(["not-a-cidr"]);
        assert!(parse_android_profile(&profile.to_string()).is_err());
        profile["bypass_cidrs"] = serde_json::json!([]);
        profile["transport"] = serde_json::json!("insecure");
        assert!(parse_android_profile(&profile.to_string()).is_err());
    }

    #[test]
    fn android_proxy_modes_use_the_shared_validated_profile() {
        for (mode, expected) in [
            ("socks5", OperatingMode::Socks5),
            ("httpProxy", OperatingMode::HttpProxy),
        ] {
            let mut profile: serde_json::Value =
                serde_json::from_str(&valid_profile_json()).unwrap();
            profile["mode"] = serde_json::json!(mode);
            assert_eq!(
                parse_android_profile(&profile.to_string()).unwrap().mode,
                expected
            );
        }
    }

    #[test]
    fn rust_profile_store_imports_flutter_data_only_once() {
        let directory = tempfile::tempdir().unwrap();
        let config_path = directory.path().join("profiles-v2.json");
        let profile: serde_json::Value = serde_json::from_str(&valid_profile_json()).unwrap();
        let import = serde_json::json!({
            "command": "import_legacy_profiles",
            "profiles": [profile],
            "active_profile_id": "8c30b771-9ebd-457a-b67b-bbc74a1ddba6",
        });
        let first =
            apply_profile_command(config_path.to_str().unwrap(), &import.to_string()).unwrap();
        assert!(first.contains("\"name\":\"Default\""));

        let mut replacement: serde_json::Value =
            serde_json::from_str(&valid_profile_json()).unwrap();
        replacement["name"] = serde_json::json!("Must not replace");
        let second_import = serde_json::json!({
            "command": "import_legacy_profiles",
            "profiles": [replacement],
            "active_profile_id": "8c30b771-9ebd-457a-b67b-bbc74a1ddba6",
        });
        let second =
            apply_profile_command(config_path.to_str().unwrap(), &second_import.to_string())
                .unwrap();
        assert!(!second.contains("Must not replace"));

        let stored = ConfigStore::new(config_path).load().unwrap();
        assert!(stored.preferences.profiles_migrated_from_flutter);
        assert_eq!(stored.profiles[0].name, "Default");
    }

    #[test]
    fn rust_profile_store_imports_non_default_catalog_when_active_id_is_empty() {
        let directory = tempfile::tempdir().unwrap();
        let config_path = directory.path().join("profiles-v2.json");
        let imported_id = "7b60ea7c-03a5-455d-9914-2cdf0e268ac2";
        let mut profile: serde_json::Value = serde_json::from_str(&valid_profile_json()).unwrap();
        profile["id"] = serde_json::json!(imported_id);
        profile["name"] = serde_json::json!("Imported");
        let import = serde_json::json!({
            "command": "import_legacy_profiles",
            "profiles": [profile],
            "active_profile_id": "",
        });
        let response =
            apply_profile_command(config_path.to_str().unwrap(), &import.to_string()).unwrap();
        let response: serde_json::Value = serde_json::from_str(&response).unwrap();
        assert_eq!(response["active_profile_id"], imported_id);
        assert_eq!(response["profiles"].as_array().unwrap().len(), 1);
        assert_eq!(response["profiles"][0]["id"], imported_id);

        let stored = ConfigStore::new(config_path).load().unwrap();
        assert!(stored.preferences.profiles_migrated_from_flutter);
        assert_eq!(stored.profiles.len(), 1);
        assert_eq!(stored.profiles[0].id.to_string(), imported_id);
        assert_eq!(stored.active_profile().unwrap().id.to_string(), imported_id);
    }

    #[test]
    fn android_profile_store_persists_and_protects_zero_trust_binding() {
        let directory = tempfile::tempdir().unwrap();
        let config_path = directory.path().join("profiles-v2.json");
        let mut registered: serde_json::Value =
            serde_json::from_str(&valid_profile_json()).unwrap();
        registered["endpoint_v4"] = serde_json::json!("162.159.197.2");
        registered["endpoint_v6"] = serde_json::json!("2606:4700:102::2");
        registered["sni"] = serde_json::json!("zt-masque.cloudflareclient.com");
        let response = apply_profile_command(
            config_path.to_str().unwrap(),
            &serde_json::json!({
                "command": "upsert_profile",
                "profile": registered,
                "identity_provider": "zero_trust",
                "organization": "example-team",
            })
            .to_string(),
        )
        .unwrap();
        let response: serde_json::Value = serde_json::from_str(&response).unwrap();
        assert_eq!(response["profiles"][0]["identity_provider"], "zero_trust");
        assert_eq!(
            response["profiles"][0]["identity_organization"],
            "example-team"
        );

        let generic_edit: serde_json::Value = serde_json::from_str(&valid_profile_json()).unwrap();
        let response = apply_profile_command(
            config_path.to_str().unwrap(),
            &serde_json::json!({
                "command": "upsert_profile",
                "profile": generic_edit,
            })
            .to_string(),
        )
        .unwrap();
        let response: serde_json::Value = serde_json::from_str(&response).unwrap();
        assert_eq!(response["profiles"][0]["endpoint_v4"], "162.159.197.2");
        assert_eq!(
            response["profiles"][0]["sni"],
            "zt-masque.cloudflareclient.com"
        );

        let conversion = apply_profile_command(
            config_path.to_str().unwrap(),
            &serde_json::json!({
                "command": "upsert_profile",
                "profile": serde_json::from_str::<serde_json::Value>(&valid_profile_json()).unwrap(),
                "identity_provider": "consumer",
            })
            .to_string(),
        );
        assert!(conversion.is_err());
    }

    #[test]
    fn deleted_android_profiles_remain_tombstoned_until_keystore_cleanup_is_acknowledged() {
        let directory = tempfile::tempdir().unwrap();
        let config_path = directory.path().join("profiles-v2.json");
        let mut second: serde_json::Value = serde_json::from_str(&valid_profile_json()).unwrap();
        second["id"] = serde_json::json!("7b60ea7c-03a5-455d-9914-2cdf0e268ac2");
        second["name"] = serde_json::json!("Second");
        apply_profile_command(
            config_path.to_str().unwrap(),
            &serde_json::json!({
                "command": "upsert_profile",
                "profile": second,
            })
            .to_string(),
        )
        .unwrap();

        let deleted = apply_profile_command(
            config_path.to_str().unwrap(),
            &serde_json::json!({
                "command": "delete_profile",
                "profile_id": "8c30b771-9ebd-457a-b67b-bbc74a1ddba6",
            })
            .to_string(),
        )
        .unwrap();
        let deleted: serde_json::Value = serde_json::from_str(&deleted).unwrap();
        assert_eq!(
            deleted["pending_identity_deletions"]
                .as_array()
                .unwrap()
                .len(),
            1
        );

        let completed = apply_profile_command(
            config_path.to_str().unwrap(),
            &serde_json::json!({
                "command": "complete_identity_deletions",
                "profile_ids": ["8c30b771-9ebd-457a-b67b-bbc74a1ddba6"],
            })
            .to_string(),
        )
        .unwrap();
        let completed: serde_json::Value = serde_json::from_str(&completed).unwrap();
        assert!(
            completed["pending_identity_deletions"]
                .as_array()
                .unwrap()
                .is_empty()
        );

        let cleared = apply_profile_command(
            config_path.to_str().unwrap(),
            &serde_json::json!({"command": "clear_all_data"}).to_string(),
        )
        .unwrap();
        let cleared: serde_json::Value = serde_json::from_str(&cleared).unwrap();
        assert_eq!(cleared["profiles"].as_array().unwrap().len(), 1);
        assert_eq!(
            cleared["active_profile_id"],
            serde_json::json!("8c30b771-9ebd-457a-b67b-bbc74a1ddba6")
        );
    }

    #[test]
    fn reconfigure_active_profile_command_classifies_without_restarting() {
        let directory = tempfile::tempdir().unwrap();
        let config_path = directory.path().join("profiles-v2.json");
        let profile: serde_json::Value = serde_json::from_str(&valid_profile_json()).unwrap();
        apply_profile_command(
            config_path.to_str().unwrap(),
            &serde_json::json!({
                "command": "upsert_profile",
                "profile": profile,
            })
            .to_string(),
        )
        .unwrap();

        let mut socks: serde_json::Value = serde_json::from_str(&valid_profile_json()).unwrap();
        socks["proxy"]["socks_port"] = serde_json::json!(1081);
        let hot = apply_profile_command(
            config_path.to_str().unwrap(),
            &serde_json::json!({
                "command": "reconfigure_active_profile",
                "profile": socks,
            })
            .to_string(),
        )
        .unwrap();
        let hot: serde_json::Value = serde_json::from_str(&hot).unwrap();
        assert_eq!(hot["profiles"][0]["proxy"]["socks_port"], 1081);

        let mut detached =
            serde_json::from_str::<serde_json::Value>(&valid_profile_json()).unwrap();
        detached["proxy"]["socks_port"] = serde_json::json!(1081);
        detached["frontends"] = serde_json::json!({
            "tunnel": false,
            "socks5": true,
            "http": true,
        });
        let attach = apply_profile_command(
            config_path.to_str().unwrap(),
            &serde_json::json!({
                "command": "reconfigure_active_profile",
                "profile": detached,
            })
            .to_string(),
        )
        .unwrap();
        let attach: serde_json::Value = serde_json::from_str(&attach).unwrap();
        assert_eq!(attach["profiles"][0]["frontends"]["tunnel"], false);
        assert_eq!(attach["profiles"][0]["mode"], "socks5");

        let mut cold = serde_json::from_str::<serde_json::Value>(&valid_profile_json()).unwrap();
        cold["proxy"]["socks_port"] = serde_json::json!(1081);
        cold["frontends"] = serde_json::json!({
            "tunnel": false,
            "socks5": true,
            "http": true,
        });
        cold["mtu"] = serde_json::json!(1400);
        let cold = apply_profile_command(
            config_path.to_str().unwrap(),
            &serde_json::json!({
                "command": "reconfigure_active_profile",
                "profile": cold,
            })
            .to_string(),
        )
        .unwrap();
        let cold: serde_json::Value = serde_json::from_str(&cold).unwrap();
        assert_eq!(cold["profiles"][0]["mtu"], 1400);
    }

    #[test]
    fn malformed_manual_identity_is_rejected() {
        assert_eq!(
            validate_warp_secret_bytes(b"not a secret"),
            INVALID_WARP_SECRET
        );
        assert_eq!(validate_warp_secret_bytes(&[0xff]), INVALID_WARP_SECRET);
    }

    #[test]
    fn automatic_registration_rejects_invalid_locale_before_network_access() {
        assert!(register_consumer_warp("").is_err());
        assert!(register_consumer_warp(&"x".repeat(33)).is_err());
    }
}
