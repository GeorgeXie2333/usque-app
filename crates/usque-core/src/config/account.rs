use std::net::{Ipv4Addr, Ipv6Addr};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{DEFAULT_PROFILE_ID, EndpointSettings};

/// Registration-owned Zero Trust endpoint addresses. Port and SNI remain in
/// the device-wide network settings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManagedEndpointIps {
    pub ipv4: Ipv4Addr,
    pub ipv6: Ipv6Addr,
}

impl ManagedEndpointIps {
    pub fn from_endpoint(endpoint: &EndpointSettings) -> Self {
        Self {
            ipv4: endpoint.ipv4,
            ipv6: endpoint.ipv6,
        }
    }

    /// Registration currently accepts only the documented Zero Trust ingress
    /// address families. Keep the same contract when recovering an older
    /// schema from its migration backup.
    pub fn matches_zero_trust_contract(&self) -> bool {
        self.ipv4.octets()[..3] == [162, 159, 197]
            && self.ipv6.segments()[..3] == [0x2606, 0x4700, 0x0102]
    }
}

/// Persisted WARP account. Network settings live on [`super::AppConfig::network`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Account {
    pub id: Uuid,
    pub name: String,
    /// Zero Trust IPv4/IPv6 returned by registration. Schema 10 stored the
    /// same addresses inside a full `managed_endpoint` object.
    #[serde(
        default,
        alias = "managed_endpoint",
        skip_serializing_if = "Option::is_none"
    )]
    pub managed_endpoint_ips: Option<ManagedEndpointIps>,
}

impl Account {
    pub fn new(id: Uuid, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            managed_endpoint_ips: None,
        }
    }

    pub fn default_account() -> Self {
        Self::new(DEFAULT_PROFILE_ID, "Default")
    }
}
