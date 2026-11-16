//! Bidirectional authenticated XPC credential messages. Digests bind ACKs to
//! the complete request, not just to a session or a reused callback identifier.
use crate::{
    credentials::{CredentialBroker, PersistenceAck},
    protocol::{PROTOCOL_VERSION, SessionKey},
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
#[cfg(unix)]
use std::io::Read;
use std::{
    fmt,
    fs::File,
    sync::{Arc, Mutex},
};
use usque_core::{
    WarpIdentity,
    proton_vpn::{LeasePersistence, Record},
};
use uuid::Uuid;
use zeroize::Zeroizing;

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum BrokerRequest {
    PersistPin {
        session: SessionKey,
        profile_id: Uuid,
        previous_identity: Zeroizing<String>,
        next_identity: Zeroizing<String>,
    },
    ProtonLoad {
        session: SessionKey,
        profile_id: Uuid,
        initial: Record,
    },
    ProtonSave {
        session: SessionKey,
        profile_id: Uuid,
        initial: Record,
        record: Box<Record>,
    },
    ProtonLock {
        session: SessionKey,
        profile_id: Uuid,
        initial: Record,
    },
    ProtonUnlock {
        session: SessionKey,
        profile_id: Uuid,
        initial: Record,
        lease_id: Uuid,
    },
}
impl fmt::Debug for BrokerRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("BrokerRequest([redacted])")
    }
}
impl BrokerRequest {
    pub fn session(&self) -> &SessionKey {
        match self {
            Self::PersistPin { session, .. }
            | Self::ProtonLoad { session, .. }
            | Self::ProtonSave { session, .. }
            | Self::ProtonLock { session, .. }
            | Self::ProtonUnlock { session, .. } => session,
        }
    }
    pub fn profile_id(&self) -> Uuid {
        match self {
            Self::PersistPin { profile_id, .. }
            | Self::ProtonLoad { profile_id, .. }
            | Self::ProtonSave { profile_id, .. }
            | Self::ProtonLock { profile_id, .. }
            | Self::ProtonUnlock { profile_id, .. } => *profile_id,
        }
    }
    pub fn digest(&self) -> Result<String, String> {
        let encoded =
            Zeroizing::new(serde_json::to_vec(self).map_err(|_| "invalid_broker_request")?);
        Ok(Sha256::digest(encoded.as_slice())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect())
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CredentialRequest {
    pub callback_id: Uuid,
    pub request_digest: String,
    pub broker: BrokerRequest,
}
impl fmt::Debug for CredentialRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CredentialRequest([redacted])")
    }
}
impl CredentialRequest {
    pub fn new(broker: BrokerRequest) -> Result<Self, String> {
        Ok(Self {
            callback_id: Uuid::new_v4(),
            request_digest: broker.digest()?,
            broker,
        })
    }
    pub fn validate_digest(&self) -> Result<(), String> {
        if self.callback_id.is_nil() || self.request_digest != self.broker.digest()? {
            Err("broker_request_digest_mismatch".into())
        } else {
            Ok(())
        }
    }
}
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct BrokerResponse {
    pub accepted: bool,
    #[serde(default)]
    pub session: Option<SessionKey>,
    #[serde(default)]
    pub profile_id: Option<Uuid>,
    #[serde(default)]
    pub revision: Option<Uuid>,
    #[serde(default)]
    pub request_digest: Option<String>,
    #[serde(default)]
    pub record: Option<Record>,
    #[serde(default)]
    pub lease_id: Option<Uuid>,
    #[serde(default, alias = "error")]
    pub error_code: Option<String>,
}
impl fmt::Debug for BrokerResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("BrokerResponse([redacted])")
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CredentialResponse {
    pub callback_id: Uuid,
    pub broker: BrokerResponse,
}

/// Native transport must enforce a deadline and authenticate the Engine peer.
/// `exchange` is only invoked on blocking workers, never a Tokio reactor.
pub trait CredentialTransport: Send + Sync + 'static {
    fn exchange(&self, request: &CredentialRequest) -> Result<CredentialResponse, String>;
}
pub struct RpcCredentialBroker {
    transport: Arc<dyn CredentialTransport>,
}
impl RpcCredentialBroker {
    pub fn new(transport: Arc<dyn CredentialTransport>) -> Self {
        Self { transport }
    }
}
fn exchange(
    transport: &dyn CredentialTransport,
    broker: BrokerRequest,
) -> Result<BrokerResponse, String> {
    let request = CredentialRequest::new(broker)?;
    let response = transport.exchange(&request)?;
    let payload = response.broker;
    if response.callback_id != request.callback_id
        || !payload.accepted
        || payload.session.as_ref() != Some(request.broker.session())
        || payload.profile_id != Some(request.broker.profile_id())
        || payload.request_digest.as_deref() != Some(request.request_digest.as_str())
    {
        return Err("credential_ack_mismatch".into());
    }
    Ok(payload)
}
#[async_trait]
impl CredentialBroker for RpcCredentialBroker {
    async fn persist_pin(
        &self,
        session: &SessionKey,
        profile_id: Uuid,
        previous: &WarpIdentity,
        next: &WarpIdentity,
    ) -> Result<PersistenceAck, String> {
        let request = BrokerRequest::PersistPin {
            session: session.clone(),
            profile_id,
            previous_identity: previous
                .to_portable_secret_json()
                .map_err(|_| "invalid_identity")?,
            next_identity: next
                .to_portable_secret_json()
                .map_err(|_| "invalid_identity")?,
        };
        let transport = self.transport.clone();
        let response = tokio::task::spawn_blocking(move || exchange(transport.as_ref(), request))
            .await
            .map_err(|_| "credential_worker_failed")??;
        let revision = response
            .revision
            .filter(|id| !id.is_nil())
            .ok_or("missing_persistence_revision")?;
        Ok(PersistenceAck {
            session: session.clone(),
            profile_id,
            revision,
        })
    }
    fn proton_persistence(
        &self,
        session: &SessionKey,
        profile_id: Uuid,
        initial: &Record,
    ) -> Result<Arc<dyn LeasePersistence>, String> {
        initial.validate().map_err(|_| "invalid_proton_record")?;
        if initial.id != profile_id {
            return Err("proton_account_mismatch".into());
        }
        Ok(Arc::new(RpcProtonPersistence {
            session: session.clone(),
            profile_id,
            initial: Mutex::new(initial.clone()),
            transport: self.transport.clone(),
        }))
    }
}
struct RpcProtonPersistence {
    session: SessionKey,
    profile_id: Uuid,
    initial: Mutex<Record>,
    transport: Arc<dyn CredentialTransport>,
}
fn import_error(_: impl fmt::Display) -> usque_core::chain_exit::ImportError {
    usque_core::chain_exit::ImportError::new(0, "proton", "credential_broker_unavailable")
}
impl LeasePersistence for RpcProtonPersistence {
    fn load(&self) -> Result<Record, usque_core::chain_exit::ImportError> {
        let initial = self.initial.lock().map_err(import_error)?.clone();
        let response = exchange(
            self.transport.as_ref(),
            BrokerRequest::ProtonLoad {
                session: self.session.clone(),
                profile_id: self.profile_id,
                initial,
            },
        )
        .map_err(import_error)?;
        let record = response
            .record
            .ok_or_else(|| import_error("missing_record"))?;
        record.validate()?;
        if record.id != self.profile_id || response.revision != Some(record.revision) {
            return Err(import_error("proton_record_mismatch"));
        }
        *self.initial.lock().map_err(import_error)? = record.clone();
        Ok(record)
    }
    fn save(&self, record: &Record) -> Result<(), usque_core::chain_exit::ImportError> {
        record.validate()?;
        if record.id != self.profile_id {
            return Err(import_error("proton_record_mismatch"));
        }
        let initial = self.initial.lock().map_err(import_error)?.clone();
        let response = exchange(
            self.transport.as_ref(),
            BrokerRequest::ProtonSave {
                session: self.session.clone(),
                profile_id: self.profile_id,
                initial,
                record: Box::new(record.clone()),
            },
        )
        .map_err(import_error)?;
        if response.revision != Some(record.revision) {
            return Err(import_error("proton_revision_mismatch"));
        }
        *self.initial.lock().map_err(import_error)? = record.clone();
        Ok(())
    }
    fn lock(&self) -> Result<File, usque_core::chain_exit::ImportError> {
        #[cfg(unix)]
        {
            use std::os::fd::OwnedFd;
            let initial = self.initial.lock().map_err(import_error)?.clone();
            let response = exchange(
                self.transport.as_ref(),
                BrokerRequest::ProtonLock {
                    session: self.session.clone(),
                    profile_id: self.profile_id,
                    initial: initial.clone(),
                },
            )
            .map_err(import_error)?;
            let lease_id = response
                .lease_id
                .filter(|id| !id.is_nil())
                .ok_or_else(|| import_error("missing_lock_lease"))?;
            let (owned, mut observed) =
                std::os::unix::net::UnixStream::pair().map_err(import_error)?;
            let transport = self.transport.clone();
            let session = self.session.clone();
            let profile_id = self.profile_id;
            // Returning a socket-backed File preserves the existing drop-based
            // lock lifetime without creating a file or duplicating the vault.
            std::thread::Builder::new()
                .name("usque-proton-lock".into())
                .spawn(move || {
                    let mut byte = [0u8; 1];
                    loop {
                        match observed.read(&mut byte) {
                            Ok(0) => break,
                            Ok(_) => {}
                            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                            Err(_) => break,
                        }
                    }
                    let _ = exchange(
                        transport.as_ref(),
                        BrokerRequest::ProtonUnlock {
                            session,
                            profile_id,
                            initial,
                            lease_id,
                        },
                    );
                })
                .map_err(import_error)?;
            let descriptor: OwnedFd = owned.into();
            Ok(File::from(descriptor))
        }
        #[cfg(not(unix))]
        {
            Err(import_error("unsupported_platform"))
        }
    }
}

/// Kept explicit so a future incompatible callback protocol is a separate ABI.
pub const CREDENTIAL_PROTOCOL_VERSION: u32 = PROTOCOL_VERSION;
