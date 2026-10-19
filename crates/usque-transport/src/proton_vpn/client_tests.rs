use super::client::ClientConfig;
use serde_json::{Value, json};

pub(super) const ANDROID_CONTEXT: &str = include_str!(
    "../../../../apps/usque_gui/android/app/src/test/resources/proton_android_client.json"
);

#[test]
fn android_payload_distinguishes_anonymous_and_sealed_login_frames() {
    let config = ClientConfig::android(ANDROID_CONTEXT).unwrap();
    let source: Value = serde_json::from_str(ANDROID_CONTEXT).unwrap();
    let mut device = source["device"].clone();
    device["v"] = json!("2.0.7");
    let anonymous = config.payload(false);
    assert_eq!(
        anonymous,
        json!({"Payload":{"vpn-android-v4-challenge-0":device}})
    );
    device["type"] = json!("me.proton.core.challenge.data.frame.ChallengeFrame.Device");
    assert_eq!(
        config.payload(true),
        json!({"Payload":{"vpn-android-v4-challenge-0":device}})
    );
    assert_eq!(config.identity.app_version, "android-vpn@5.20.74.0+apk");
    assert_eq!(
        config.identity.user_agent,
        "ProtonVPN/5.20.74.0 (Android 15; google Pixel 8)"
    );
    assert_eq!(config.identity.locale, "ja");
}

#[test]
fn tv_identity_and_non_ascii_user_agent_follow_android_format() {
    let mut source: Value = serde_json::from_str(ANDROID_CONTEXT).unwrap();
    source["is_tv"] = json!(true);
    source["model"] = json!("模型😀");
    let config = ClientConfig::android(&source.to_string()).unwrap();
    assert_eq!(config.identity.app_version, "android_tv-vpn@5.20.74.0+apk");
    assert_eq!(
        config.identity.user_agent,
        "ProtonVPN/5.20.74.0 (Android 15; google ????)"
    );
}

#[test]
fn malformed_platform_facts_cannot_enter_http_headers_or_challenge_frames() {
    for (path, value) in [
        ("/brand", json!("bad\r\nInjected: header")),
        ("/device/appLang", json!("ja\n")),
        ("/device/timezoneOffset", json!(2000)),
        ("/device/storageCapacity", json!(-1)),
        ("/device/preferredContentSize", json!("NaN")),
        ("/device/keyboards", json!(["bad\npackage"])),
    ] {
        let mut source: Value = serde_json::from_str(ANDROID_CONTEXT).unwrap();
        *source.pointer_mut(path).unwrap() = value;
        assert!(
            ClientConfig::android(&source.to_string()).is_err(),
            "{path}"
        );
    }
    assert!(ClientConfig::android("{}").is_err());
    assert!(ClientConfig::android(&" ".repeat(32 * 1024 + 1)).is_err());
}

#[test]
fn other_platforms_keep_their_independent_client_and_device_identity() {
    let config = ClientConfig::default();
    assert_eq!(
        config.identity.app_version,
        usque_core::proton_vpn::APP_VERSION
    );
    assert!(config.identity.user_agent.starts_with("Usque/"));
    let payload = config.payload(false);
    let key = format!("usque-{}-v4-challenge-0", std::env::consts::OS);
    assert!(payload["Payload"].get(key).is_some());
    assert!(!payload.to_string().contains("vpn-android"));
}

#[test]
fn device_hash_matches_android_signed_utf8_vectors() {
    use super::client::device_hash;
    assert_eq!(device_hash("Pixel 8/google shiba"), 73_362_205_250);
    assert_eq!(device_hash("é"), -670);
    assert_eq!(device_hash("模型😀/brand device"), 76_413_035_754);
}

#[cfg(not(target_os = "android"))]
#[test]
fn non_android_guest_has_a_complete_fixed_context_without_changing_the_default() {
    let original = ClientConfig::default();
    let config = original.for_guest().unwrap();
    assert_eq!(config.identity.app_version, "android-vpn@5.20.74.0+apk");
    assert_eq!(
        config.identity.user_agent,
        "ProtonVPN/5.20.74.0 (Android 15; google Pixel 8)"
    );
    assert_eq!(config.identity.locale, "en");
    let anonymous = json!({"Payload":{"vpn-android-v4-challenge-0":{
        "v":"2.0.7", "appLang":"en", "timezone":"UTC",
        "deviceName":73_362_205_250i64, "regionCode":"US", "timezoneOffset":0,
        "isJailbreak":false, "preferredContentSize":"1.0", "storageCapacity":128.0,
        "isDarkmodeOn":false, "keyboards":[]
    }}});
    assert_eq!(config.payload(false), anonymous);
    let mut credentialless = anonymous.clone();
    credentialless["Payload"]["vpn-android-v4-challenge-0"]["type"] =
        json!("me.proton.core.challenge.data.frame.ChallengeFrame.Device");
    assert_eq!(config.payload(true), credentialless);
    assert_eq!(config.for_guest().unwrap().payload(false), anonymous);
    assert_eq!(
        original.identity.app_version,
        usque_core::proton_vpn::APP_VERSION
    );
    assert!(!original.payload(false).to_string().contains("vpn-android"));
}

#[test]
fn guest_selection_preserves_real_android_and_tv_contexts() {
    for is_tv in [false, true] {
        let mut source: Value = serde_json::from_str(ANDROID_CONTEXT).unwrap();
        source["is_tv"] = json!(is_tv);
        let original = ClientConfig::android(&source.to_string()).unwrap();
        let selected = original.for_guest().unwrap();
        assert!(selected.identity == original.identity);
        assert_eq!(selected.payload(false), original.payload(false));
        assert_eq!(selected.payload(true), original.payload(true));
    }
}

#[cfg(target_os = "android")]
#[test]
fn android_does_not_replace_a_missing_platform_context_with_synthetic_facts() {
    let original = ClientConfig::default();
    let selected = original.for_guest().unwrap();
    assert!(selected.identity == original.identity);
    assert_eq!(selected.payload(false), original.payload(false));
}
