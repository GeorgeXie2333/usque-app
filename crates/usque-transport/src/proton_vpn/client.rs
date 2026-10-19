//! Android wire compatibility pinned to ProtonVPN/android-app 316a3d499e12
//! (5.20.74.0, direct APK) and its published Proton Core 36.8.0 sources.
//! Android platform facts enter through JNI. New non-Android guests use a
//! separate fixed synthetic context without observing the host device.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use usque_core::{chain_exit::ImportError, proton_vpn::ApiIdentity};

pub const ANDROID_COMPAT_VERSION: &str = "5.20.74.0";
const ANDROID_FRAME_KEY: &str = "vpn-android-v4-challenge-0";
const DEVICE_TYPE: &str = "me.proton.core.challenge.data.frame.ChallengeFrame.Device";

#[derive(Clone, Default)]
pub struct ClientConfig {
    pub(super) identity: ApiIdentity,
    device: Option<AndroidContext>,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct AndroidContext {
    os_release: String,
    brand: String,
    model: String,
    locale: String,
    is_tv: bool,
    device: AndroidDevice,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct AndroidDevice {
    #[serde(rename = "appLang")]
    app_language: String,
    timezone: String,
    #[serde(rename = "deviceName")]
    device_name: i64,
    #[serde(rename = "regionCode")]
    region_code: String,
    #[serde(rename = "timezoneOffset")]
    timezone_offset: i32,
    #[serde(rename = "isJailbreak")]
    rooted: bool,
    #[serde(rename = "preferredContentSize")]
    font_size: String,
    #[serde(rename = "storageCapacity")]
    storage: f64,
    #[serde(rename = "isDarkmodeOn")]
    dark_mode: bool,
    keyboards: Vec<String>,
}

impl ClientConfig {
    pub fn android(raw: &str) -> Result<Self, ImportError> {
        let invalid = || usque_core::proton_vpn::error("invalid_client_context");
        if raw.len() > 32 * 1024 {
            return Err(invalid());
        }
        let context: AndroidContext = serde_json::from_str(raw).map_err(|_| invalid())?;
        Self::from_android(context)
    }

    fn from_android(context: AndroidContext) -> Result<Self, ImportError> {
        let invalid = || usque_core::proton_vpn::error("invalid_client_context");
        let device = &context.device;
        for value in [
            &context.os_release,
            &context.brand,
            &context.model,
            &context.locale,
            &device.app_language,
            &device.timezone,
            &device.region_code,
            &device.font_size,
        ] {
            if value.len() > 256 || value.chars().any(char::is_control) {
                return Err(invalid());
            }
        }
        if context.os_release.is_empty()
            || device.timezone.is_empty()
            || !(-24 * 60..=24 * 60).contains(&device.timezone_offset)
            || !device.storage.is_finite()
            || !(0.0..=1_000_000_000.0).contains(&device.storage)
            || device
                .font_size
                .parse::<f32>()
                .ok()
                .is_none_or(|size| !size.is_finite() || size <= 0.0 || size > 100.0)
            || device.keyboards.len() > 128
            || device.keyboards.iter().any(|name| {
                name.is_empty() || name.len() > 256 || name.chars().any(char::is_control)
            })
        {
            return Err(invalid());
        }
        let user_agent = format!(
            "ProtonVPN/{ANDROID_COMPAT_VERSION} (Android {}; {} {})",
            context.os_release, context.brand, context.model
        )
        .encode_utf16()
        .map(|c| if c < 128 { c as u8 as char } else { '?' })
        .collect();
        let client = if context.is_tv {
            "android_tv-vpn"
        } else {
            "android-vpn"
        };
        let identity = ApiIdentity {
            app_version: format!("{client}@{ANDROID_COMPAT_VERSION}+apk"),
            user_agent,
            locale: context.locale.clone(),
        };
        identity.validate()?;
        Ok(Self {
            identity,
            device: Some(context),
        })
    }

    /// Select before creating any session; saved sessions retain their identity.
    pub(super) fn for_guest(&self) -> Result<Self, ImportError> {
        if self.device.is_some() || cfg!(target_os = "android") {
            return Ok(self.clone());
        }
        Self::from_android(AndroidContext {
            os_release: "15".into(),
            brand: "google".into(),
            model: "Pixel 8".into(),
            locale: "en".into(),
            is_tv: false,
            device: AndroidDevice {
                app_language: "en".into(),
                timezone: "UTC".into(),
                device_name: device_hash("Pixel 8/google shiba"),
                region_code: "US".into(),
                timezone_offset: 0,
                rooted: false,
                font_size: "1.0".into(),
                storage: 128.0,
                dark_mode: false,
                keyboards: Vec::new(),
            },
        })
    }

    pub(super) fn payload(&self, login: bool) -> Value {
        if let Some(context) = &self.device {
            let mut device = serde_json::to_value(&context.device).expect("finite Android device");
            device["v"] = json!("2.0.7");
            // Core uses a concrete Device map for requestSession, and a sealed
            // ChallengeFrame map for login/credentialless. The latter includes
            // the Kotlin serializer's default class discriminator.
            if login {
                device["type"] = json!(DEVICE_TYPE);
            }
            return json!({"Payload": {ANDROID_FRAME_KEY: device}});
        }
        let platform = std::env::consts::OS;
        let description = format!("{platform}/usque {}", std::env::consts::ARCH);
        let device = device_hash(&description);
        json!({"Payload": {format!("usque-{platform}-v4-challenge-0"): {"v":"2.0.7","deviceName":device}}})
    }
}

pub(super) fn device_hash(description: &str) -> i64 {
    let mut weight = 1i64;
    let mut device = 0i64;
    for byte in description.bytes() {
        device += (byte as i8 as i64) * weight;
        weight = weight * 7 % 100000007;
    }
    device
}
