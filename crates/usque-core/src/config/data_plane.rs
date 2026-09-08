use serde::{Deserialize, Serialize};

use crate::IdentityProvider;

/// Application data plane, independent of CONNECT-IP's H3/H2 selection.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum DataPlaneMode {
    #[default]
    ConnectIp,
    L4Proxy,
}

impl DataPlaneMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ConnectIp => "connect_ip",
            Self::L4Proxy => "l4_proxy",
        }
    }
}

pub const CONSUMER_L4_SNI: &str = "consumer-masque-proxy.cloudflareclient.com";
pub const ZERO_TRUST_L4_SNI: &str = "zt-masque-proxy.cloudflareclient.com";

/// Call with the provider loaded alongside the enrolled credential, never a
/// caller-supplied SNI, endpoint heuristic, or unverified UI account label.
pub const fn l4_server_name(provider: &IdentityProvider) -> &'static str {
    match provider {
        IdentityProvider::Consumer => CONSUMER_L4_SNI,
        IdentityProvider::ZeroTrust { .. } => ZERO_TRUST_L4_SNI,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_is_explicit_and_unknown_values_fail_closed() {
        for mode in [DataPlaneMode::ConnectIp, DataPlaneMode::L4Proxy] {
            let encoded = serde_json::to_string(&mode).unwrap();
            assert_eq!(
                serde_json::from_str::<DataPlaneMode>(&encoded).unwrap(),
                mode
            );
        }
        for invalid in ["\"auto\"", "\"h3\"", "\"l4\"", "null", "3"] {
            assert!(serde_json::from_str::<DataPlaneMode>(invalid).is_err());
        }
    }

    #[test]
    fn server_name_follows_the_credential_issuer() {
        assert_eq!(l4_server_name(&IdentityProvider::Consumer), CONSUMER_L4_SNI);
        assert_eq!(
            l4_server_name(&IdentityProvider::zero_trust("example-team").unwrap()),
            ZERO_TRUST_L4_SNI,
        );
    }
}
