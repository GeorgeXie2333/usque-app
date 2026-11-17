//! Ownership of macOS HTTP/HTTPS proxy settings and the native preferences transaction.
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    net::SocketAddr,
    sync::Mutex,
};
use zeroize::Zeroizing;
mod native;
pub use native::NativeBackend;
const MAX_SERVICES: usize = 256;
const KEYS: [&str; 8] = [
    "HTTPEnable",
    "HTTPProxy",
    "HTTPPort",
    "HTTPSEnable",
    "HTTPSProxy",
    "HTTPSPort",
    "ProxyAutoConfigEnable",
    "ProxyAutoDiscoveryEnable",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("copy Usque to Applications and open that copy before enabling system integration")]
    InstallRequired,
    #[error("install or repair the macOS background service, then reopen Usque")]
    AgentRequired,
    #[error("macOS system proxy authorization was cancelled; retry to request permission again")]
    PermissionCancelled,
    #[error("macOS system proxy authorization is required; retry to request permission")]
    PermissionRequired,
    #[error("macOS system proxy preferences are busy or changed; retry")]
    Conflict,
    #[error("macOS system proxy recovery could not be verified; its recovery record is retained")]
    RecoveryPending,
    #[error("macOS system proxy requires an enabled physical network service")]
    NoServices,
    #[error("macOS system proxy requires a loopback HTTP listener")]
    InvalidListener,
    #[error("macOS system proxy configuration is unavailable")]
    Unavailable,
}
impl Error {
    pub fn code(self) -> &'static str {
        match self {
            Self::AgentRequired => "MACOS_PROXY_AGENT_REQUIRED",
            Self::InstallRequired => "MACOS_INSTALL_REQUIRED",
            Self::PermissionCancelled => "SYSTEM_PROXY_PERMISSION_CANCELLED",
            Self::PermissionRequired => "SYSTEM_PROXY_PERMISSION_REQUIRED",
            Self::Conflict => "SYSTEM_PROXY_CONFLICT",
            Self::RecoveryPending => "SYSTEM_PROXY_RECOVERY_PENDING",
            Self::NoServices => "SYSTEM_PROXY_NO_NETWORK_SERVICE",
            Self::InvalidListener => "SYSTEM_PROXY_INVALID_LISTENER",
            Self::Unavailable => "SYSTEM_PROXY_UNAVAILABLE",
        }
    }
    pub fn retryable(self) -> bool {
        self != Self::InvalidListener
    }
}
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Value {
    Text(String),
    Integer(i64),
    Boolean(bool),
}
pub type Values = BTreeMap<String, Value>;
#[derive(Clone)]
pub struct Service {
    pub id: String,
    pub eligible: bool,
    pub values: Values,
    pub pac_fingerprint: String,
}
#[derive(Clone)]
pub struct Patch {
    pub service: String,
    pub key: String,
    pub before: Option<Value>,
    pub after: Option<Value>,
}
pub type Edit<'a> = dyn FnMut(&[Service]) -> Result<Vec<Patch>> + 'a;
pub trait Backend: Send + Sync {
    /// Hold the native preferences lock across observation, durable intent,
    /// compare-and-set, commit and apply. A cancelled prompt occurs before edit.
    fn transaction(&self, interactive: bool, edit: &mut Edit<'_>) -> Result<()>;
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Change {
    pub service: String,
    pub key: String,
    pub before: Option<Value>,
    pub applied: Value,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Journal {
    pub version: u32,
    pub changes: Vec<Change>,
    // All group members are observed, including members already equal to the
    // target. Such observations never cause no-op native mutations.
    pub observations: Vec<Change>,
    pub pac_fingerprints: BTreeMap<String, String>,
}
impl Journal {
    pub fn capture(services: &[Service], listener: SocketAddr) -> Result<Self> {
        if !listener.ip().is_loopback() || listener.port() == 0 {
            return Err(Error::InvalidListener);
        }
        let desired = desired(listener);
        let eligible: Vec<_> = services.iter().filter(|service| service.eligible).collect();
        if eligible.is_empty() {
            return Err(Error::NoServices);
        }
        if services.len() > MAX_SERVICES {
            return Err(Error::Unavailable);
        }
        let observations: Vec<_> = eligible
            .iter()
            .flat_map(|service| {
                desired.iter().map(move |(key, value)| Change {
                    service: service.id.clone(),
                    key: key.clone(),
                    before: service.values.get(key).cloned(),
                    applied: value.clone(),
                })
            })
            .collect();
        let changes = observations
            .iter()
            .filter(|observation| observation.before.as_ref() != Some(&observation.applied))
            .cloned()
            .collect();
        let pac_fingerprints = eligible
            .iter()
            .map(|service| (service.id.clone(), service.pac_fingerprint.clone()))
            .collect();
        Ok(Self {
            version: 2,
            changes,
            observations,
            pac_fingerprints,
        })
    }
    pub fn apply_patches(&self) -> Vec<Patch> {
        self.changes
            .iter()
            .map(|change| Patch {
                service: change.service.clone(),
                key: change.key.clone(),
                before: change.before.clone(),
                after: Some(change.applied.clone()),
            })
            .collect()
    }
    pub fn validate(&self) -> Result<()> {
        let mut identities = BTreeSet::new();
        let mut observed = BTreeSet::new();
        if self.version != 2
            || self.changes.len() > MAX_SERVICES * KEYS.len()
            || self.observations.len() > MAX_SERVICES * KEYS.len()
            || self.pac_fingerprints.len() > MAX_SERVICES
            || self.observations.iter().any(|observation| {
                !valid_observation(observation)
                    || !observed.insert((observation.service.clone(), observation.key.clone()))
            })
            || self.changes.iter().any(|change| {
                !valid_observation(change)
                    || change.before.as_ref() == Some(&change.applied)
                    || !identities.insert((change.service.clone(), change.key.clone()))
                    || !self.observations.contains(change)
            })
            || self.pac_fingerprints.iter().any(|(service, fingerprint)| {
                !valid_service_id(service)
                    || fingerprint.len() != 64
                    || !fingerprint.bytes().all(|byte| byte.is_ascii_hexdigit())
                    || KEYS
                        .iter()
                        .any(|key| !observed.contains(&(service.clone(), (*key).into())))
            })
            || self
                .observations
                .iter()
                .any(|observation| !self.pac_fingerprints.contains_key(&observation.service))
        {
            // Older incomplete per-key journals are retained: without the
            // complete observations they cannot prove ownership of a group.
            return Err(Error::RecoveryPending);
        }
        Ok(())
    }
    pub fn restore_patches(&self, services: &[Service]) -> Vec<Patch> {
        self.changes
            .iter()
            .filter_map(|change| {
                let service = services
                    .iter()
                    .find(|service| service.id == change.service)?;
                let current = service.values.get(&change.key);
                let group: &[&str] = match change.key.as_str() {
                    "HTTPEnable" | "HTTPProxy" | "HTTPPort" => {
                        &["HTTPEnable", "HTTPProxy", "HTTPPort"]
                    }
                    "HTTPSEnable" | "HTTPSProxy" | "HTTPSPort" => {
                        &["HTTPSEnable", "HTTPSProxy", "HTTPSPort"]
                    }
                    // Restoring PAC or WPAD can supersede a manual endpoint
                    // selected by another application after our acquisition.
                    "ProxyAutoConfigEnable" | "ProxyAutoDiscoveryEnable" => &KEYS[..6],
                    _ => &[],
                };
                // A partial native apply may leave a mixture of our
                // before/applied values. Any other group member indicates
                // a later owner, whose endpoint AND enable flag must stay.
                if group.iter().any(|key| {
                    let observation = self.observations.iter().find(|observation| {
                        observation.service == change.service && observation.key == *key
                    });
                    observation.is_none_or(|observation| {
                        let current = service.values.get(*key);
                        current != observation.before.as_ref()
                            && current != Some(&observation.applied)
                    })
                }) {
                    return None;
                }
                if change.key == "ProxyAutoConfigEnable"
                    && self.pac_fingerprints.get(&change.service) != Some(&service.pac_fingerprint)
                {
                    return None;
                }
                (current == Some(&change.applied)).then(|| Patch {
                    service: change.service.clone(),
                    key: change.key.clone(),
                    before: current.cloned(),
                    after: change.before.clone(),
                })
            })
            .collect()
    }
}
fn valid_observation(change: &Change) -> bool {
    valid_service_id(&change.service)
        && KEYS.contains(&change.key.as_str())
        && valid_value(&change.key, &change.applied)
        && change
            .before
            .as_ref()
            .is_none_or(|value| valid_value(&change.key, value))
}
fn desired(listener: SocketAddr) -> Values {
    let host = Value::Text(listener.ip().to_string());
    let port = Value::Integer(i64::from(listener.port()));
    [
        ("HTTPEnable", Value::Integer(1)),
        ("HTTPProxy", host.clone()),
        ("HTTPPort", port.clone()),
        ("HTTPSEnable", Value::Integer(1)),
        ("HTTPSProxy", host),
        ("HTTPSPort", port),
        ("ProxyAutoConfigEnable", Value::Integer(0)),
        ("ProxyAutoDiscoveryEnable", Value::Integer(0)),
    ]
    .into_iter()
    .map(|(key, value)| (key.into(), value))
    .collect()
}
fn valid_service_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value.is_ascii()
        && !value.chars().any(char::is_control)
}
fn valid_value(key: &str, value: &Value) -> bool {
    match (key, value) {
        ("HTTPProxy" | "HTTPSProxy", Value::Text(text)) => {
            text.len() <= 4096 && !text.chars().any(char::is_control)
        }
        ("HTTPPort" | "HTTPSPort", Value::Integer(value)) => (0..=65535).contains(value),
        (
            "HTTPEnable" | "HTTPSEnable" | "ProxyAutoConfigEnable" | "ProxyAutoDiscoveryEnable",
            Value::Integer(0 | 1) | Value::Boolean(_),
        ) => true,
        _ => false,
    }
}
