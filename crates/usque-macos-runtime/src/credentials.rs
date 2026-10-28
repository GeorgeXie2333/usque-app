//! Dedicated ephemeral transport objects. Never serialize PreparedProfile:
//! its persistence callbacks and Proton material are intentionally not serde.
use crate::protocol::SessionKey;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::{fmt, sync::Arc};
use usque_core::{
    WarpIdentity,
    chain_exit::{ChainProfileSummary, ImportSecrets, ValidatedProfile},
    proton_vpn::{LeasePersistence, Record, TunnelCredentials},
    vpngate::{PreparedProfile, ServerSummary},
};
use uuid::Uuid;
use zeroize::Zeroizing;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionCredentials {
    /// The validated portable identity format preserves consumer/ZT metadata.
    pub warp_identity: Zeroizing<String>,
    pub chain: Option<ChainCredentials>,
}
impl fmt::Debug for SessionCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SessionCredentials([redacted])")
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ChainCredentials {
    VpnGate {
        server: ServerSummary,
        configuration: Zeroizing<String>,
    },
    Imported {
        server: ServerSummary,
        summary: ChainProfileSummary,
        secrets: ImportSecrets,
        proton: Option<ProtonCredentials>,
        proton_server_id: Option<String>,
    },
}
impl fmt::Debug for ChainCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ChainCredentials([redacted])")
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProtonCredentials {
    pub record: Record,
    pub certificate_chain: Zeroizing<String>,
    pub private_key_pkcs8: Zeroizing<Vec<u8>>,
    pub server_name: String,
    pub expires_at: i64,
    pub label: Option<String>,
}
impl fmt::Debug for ProtonCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ProtonCredentials([redacted])")
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PersistenceAck {
    pub session: SessionKey,
    pub profile_id: Uuid,
    pub revision: Uuid,
}

/// Authenticated Engine service. Implementations must verify the complete
/// session key and persist atomically before acknowledging. No extension vault.
#[async_trait]
pub trait CredentialBroker: Send + Sync {
    async fn persist_pin(
        &self,
        session: &SessionKey,
        profile_id: Uuid,
        previous: &WarpIdentity,
        next: &WarpIdentity,
    ) -> Result<PersistenceAck, String>;
    /// Returns an Engine-owned proxy, including load/save/lock semantics. A save
    /// must return only after durable Engine acknowledgement, including the
    /// rotation_pending record written before a token rotation request.
    fn proton_persistence(
        &self,
        session: &SessionKey,
        profile_id: Uuid,
        initial: &Record,
    ) -> Result<Arc<dyn LeasePersistence>, String>;
}

pub struct UnavailableCredentialBroker;
#[async_trait]
impl CredentialBroker for UnavailableCredentialBroker {
    async fn persist_pin(
        &self,
        _: &SessionKey,
        _: Uuid,
        _: &WarpIdentity,
        _: &WarpIdentity,
    ) -> Result<PersistenceAck, String> {
        Err("credential_broker_unavailable".into())
    }
    fn proton_persistence(
        &self,
        _: &SessionKey,
        _: Uuid,
        _: &Record,
    ) -> Result<Arc<dyn LeasePersistence>, String> {
        Err("credential_broker_unavailable".into())
    }
}

impl SessionCredentials {
    pub fn from_identity(identity: &WarpIdentity) -> Result<Self, String> {
        Ok(Self {
            warp_identity: identity
                .to_portable_secret_json()
                .map_err(|_| "invalid_identity")?,
            chain: None,
        })
    }
    pub fn identity(&self) -> Result<WarpIdentity, String> {
        usque_core::parse_manual_warp_secret(&self.warp_identity)
            .map_err(|_| "invalid_identity".into())
    }
}

impl ChainCredentials {
    pub fn from_prepared(
        server: &ServerSummary,
        prepared: &PreparedProfile,
    ) -> Result<Self, String> {
        match (&prepared.custom, &prepared.summary) {
            (Some(_), Some(summary)) => {
                let proton = match (&prepared.proton, &prepared.proton_persistence) {
                    (Some(value), Some(persistence)) => Some(ProtonCredentials {
                        record: persistence.load().map_err(|_| "proton_vault_unavailable")?,
                        certificate_chain: value.certificate_chain.clone(),
                        private_key_pkcs8: value.private_key_pkcs8.clone(),
                        server_name: value.server_name.clone(),
                        expires_at: value.expires_at,
                        label: value.label.clone(),
                    }),
                    (None, None) => None,
                    _ => return Err("incomplete_proton_handoff".into()),
                };
                Ok(Self::Imported {
                    server: server.clone(),
                    summary: *summary.clone(),
                    secrets: *prepared.credentials.clone(),
                    proton,
                    proton_server_id: prepared.proton_server_id.clone(),
                })
            }
            (None, None) => Ok(Self::VpnGate {
                server: server.clone(),
                configuration: Zeroizing::new(prepared.content().to_owned()),
            }),
            _ => Err("incomplete_chain_handoff".into()),
        }
    }
    pub fn prepare(
        &self,
        session: &SessionKey,
        broker: &dyn CredentialBroker,
    ) -> Result<(ServerSummary, PreparedProfile), String> {
        match self {
            Self::VpnGate {
                server,
                configuration,
            } => {
                let prepared = usque_core::vpngate::prepare_profile(configuration, server.ip)
                    .map_err(|_| "invalid_vpn_gate_profile")?;
                Ok((server.clone(), prepared))
            }
            Self::Imported {
                server,
                summary,
                secrets,
                proton,
                proton_server_id,
            } => {
                let parsed = ValidatedProfile::parse(summary.source, secrets)
                    .map_err(|_| "invalid_chain_profile")?;
                let expected = parsed
                    .summary(&summary.name, summary.id, summary.revision)
                    .map_err(|_| "invalid_chain_summary")?;
                if expected.protocol != summary.protocol || expected.endpoint != summary.endpoint {
                    return Err("chain_summary_mismatch".into());
                }
                let mut prepared =
                    PreparedProfile::imported(summary.clone(), parsed, secrets.clone());
                prepared.proton_server_id = proton_server_id.clone();
                if summary.source == usque_core::chain_exit::ChainSource::ProtonVpn {
                    let value = proton.as_ref().ok_or("missing_proton_credentials")?;
                    value
                        .record
                        .validate()
                        .map_err(|_| "invalid_proton_record")?;
                    if value.record.id != summary.id {
                        return Err("proton_account_mismatch".into());
                    }
                    prepared.proton_persistence =
                        Some(broker.proton_persistence(session, summary.id, &value.record)?);
                    prepared.proton = Some(Arc::new(TunnelCredentials {
                        certificate_chain: value.certificate_chain.clone(),
                        private_key_pkcs8: value.private_key_pkcs8.clone(),
                        server_name: value.server_name.clone(),
                        expires_at: value.expires_at,
                        label: value.label.clone(),
                    }));
                } else if proton.is_some() {
                    return Err("unexpected_proton_credentials".into());
                }
                Ok((server.clone(), prepared))
            }
        }
    }
}
