use serde::{Deserialize, Serialize};

/// TLS ClientHello preset for authenticated WARP MASQUE, adapted to H2 or QUIC.
/// This does not emulate Chrome's HTTP or QUIC packet behavior.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum TlsFingerprint {
    /// Original handshake, displayed as `unsafe`; retain its persisted wire name.
    Default,
    #[default]
    Chrome155,
}

impl TlsFingerprint {
    pub const ALL: [Self; 2] = [Self::Default, Self::Chrome155];
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Account, Profile, SharedNetworkSettings};

    #[test]
    fn new_and_legacy_profiles_and_shared_settings_default_to_chrome155() {
        assert_eq!(TlsFingerprint::default(), TlsFingerprint::Chrome155);
        assert_eq!(
            Profile::default().tls_fingerprint,
            TlsFingerprint::Chrome155
        );
        assert_eq!(
            SharedNetworkSettings::default().tls_fingerprint,
            TlsFingerprint::Chrome155
        );
        let mut profile = serde_json::to_value(Profile::default()).unwrap();
        profile.as_object_mut().unwrap().remove("tls_fingerprint");
        assert_eq!(
            serde_json::from_value::<Profile>(profile)
                .unwrap()
                .tls_fingerprint,
            TlsFingerprint::Chrome155
        );
        let mut shared = serde_json::to_value(SharedNetworkSettings::default()).unwrap();
        shared.as_object_mut().unwrap().remove("tls_fingerprint");
        assert_eq!(
            serde_json::from_value::<SharedNetworkSettings>(shared)
                .unwrap()
                .tls_fingerprint,
            TlsFingerprint::Chrome155
        );
    }

    #[test]
    fn preset_survives_shared_persistence_and_account_hydration() {
        for (preset, name) in [
            (TlsFingerprint::Default, "default"),
            (TlsFingerprint::Chrome155, "chrome155"),
        ] {
            assert_eq!(serde_json::to_value(preset).unwrap(), name);
            let profile = Profile {
                tls_fingerprint: preset,
                ..Default::default()
            };
            let shared = SharedNetworkSettings::from_profile(&profile);
            let shared: SharedNetworkSettings =
                serde_json::from_slice(&serde_json::to_vec(&shared).unwrap()).unwrap();
            assert_eq!(
                shared.hydrate(&Account::default_account()).tls_fingerprint,
                preset
            );
        }
        for value in ["chrome", "chrome131", "Chrome155", "unknown"] {
            assert!(serde_json::from_value::<TlsFingerprint>(serde_json::json!(value)).is_err());
        }
    }
}
