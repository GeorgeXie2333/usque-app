//! One authenticated Engine connection owns the machine-wide proxy. The
//! journal contains only owned proxy fields and PAC fingerprints.
use super::{
    journal::{Evidence, SecureStore},
    transaction::Owner,
};
use std::{io, net::SocketAddr};
use usque_ipc::agent_v1::{MacosFailure as Failure, MacosProxyListener};
use usque_platform::macos_proxy::{Backend, Error, Journal, NativeBackend};
use uuid::Uuid;

pub(super) const PATH: &str = "/Library/Application Support/Usque/Agent/system-proxy.json";
#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Receipt {
    uid: u32,
    lease: Uuid,
    journal: Journal,
}
impl Evidence for Receipt {
    fn validate(&self) -> io::Result<()> {
        if self.uid == 0 || self.lease.is_nil() {
            return Err(io::Error::other("invalid system proxy owner"));
        }
        // A misconfigured proxy field can contain a URL with userinfo.
        // Never copy such values from SystemConfiguration to the root journal.
        for change in &self.journal.observations {
            if let Some(usque_platform::macos_proxy::Value::Text(host)) = &change.before
                && host.contains(['@', '/', '?', '#', '\\'])
            {
                return Err(io::Error::other("unsupported proxy server field"));
            }
        }
        self.journal.validate().map_err(io::Error::other)
    }
}
pub(super) struct SystemProxy<B = NativeBackend> {
    store: SecureStore<Receipt>,
    backend: B,
    connection: Option<Uuid>,
}
impl SystemProxy {
    pub fn open() -> io::Result<Self> {
        let mut store = SecureStore::open_root(PATH)?;
        store.load()?;
        Ok(Self {
            store,
            backend: NativeBackend::default(),
            connection: None,
        })
    }
}
pub(super) fn lease_id(text: &str) -> Result<Uuid, Failure> {
    let id = Uuid::parse_str(text).map_err(|_| Failure::Protocol)?;
    if id.is_nil() || id.to_string() != text {
        return Err(Failure::Protocol);
    }
    Ok(id)
}
impl<B: Backend> SystemProxy<B> {
    pub fn owns(&self, connection: Uuid) -> bool {
        self.connection == Some(connection)
    }

    pub fn active(&self) -> bool {
        self.connection.is_some()
    }
    pub fn pending(&mut self) -> bool {
        self.connection.is_none() && !matches!(self.store.load(), Ok(None))
    }

    pub fn acquire(
        &mut self,
        owner: Owner,
        connection: Uuid,
        request: MacosProxyListener,
    ) -> Result<(), Failure> {
        let lease = lease_id(&request.lease)?;
        let addr = request.loopback_ip.parse().map_err(|_| Failure::Protocol)?;
        let port = u16::try_from(request.port).map_err(|_| Failure::Protocol)?;
        let listener = SocketAddr::new(addr, port);
        if !listener.ip().is_loopback() || port == 0 {
            return Err(Failure::Protocol);
        }
        if self.connection.is_some() {
            return Err(Failure::Busy);
        }
        if self.store.load().map_err(|_| Failure::Journal)?.is_some() {
            return Err(Failure::RecoveryIncomplete);
        }
        self.connection = Some(connection);
        let result = self.backend.transaction(false, &mut |services| {
            let journal = Journal::capture(services, listener)?;
            let patches = journal.apply_patches();
            let receipt = Receipt {
                uid: owner.uid,
                lease,
                journal,
            };
            receipt.validate().map_err(|_| Error::Unavailable)?;
            self.store
                .save_value(Some(&receipt))
                .map_err(|_| Error::RecoveryPending)?;
            Ok(patches)
        });
        if let Err(error) = result {
            self.connection = None;
            self.restore()?;
            return Err(match error {
                Error::NoServices => Failure::ProxyNoService,
                Error::Conflict => Failure::Busy,
                Error::RecoveryPending => Failure::Journal,
                _ => Failure::Io,
            });
        }
        Ok(())
    }

    pub fn release(
        &mut self,
        owner: Owner,
        connection: Uuid,
        lease: Option<Uuid>,
    ) -> Result<(), Failure> {
        let Some(receipt) = self.store.load().map_err(|_| Failure::Journal)? else {
            return Ok(());
        };
        // A successor could only be granted after restoring the previous
        // receipt. A retry for that previous token has nothing left to do.
        if lease.is_some_and(|id| id != receipt.lease) {
            return Ok(());
        }
        if receipt.uid != owner.uid {
            return Err(Failure::OwnershipConflict);
        }
        if self.connection.is_some() && (!self.owns(connection) || lease.is_none()) {
            return Err(Failure::Busy);
        }
        self.restore()?;
        self.connection = None;
        Ok(())
    }
    pub fn lost(&mut self, connection: Uuid) {
        if self.owns(connection) {
            self.connection = None;
            // A failed restore stays in the root journal for status and retry.
            let _ = self.restore();
        }
    }
    pub fn restore(&mut self) -> Result<(), Failure> {
        if let Some(receipt) = self.store.load().map_err(|_| Failure::Journal)? {
            self.backend
                .transaction(false, &mut |services| {
                    Ok(receipt.journal.restore_patches(services))
                })
                .map_err(|_| Failure::RecoveryIncomplete)?;
            self.store.save_value(None).map_err(|_| Failure::Journal)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
