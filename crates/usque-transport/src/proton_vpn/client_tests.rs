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
