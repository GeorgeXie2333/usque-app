//! Agent-owned system proxy leases and migration of older user-encrypted recovery records.
use crate::macos_agent::{AgentFailure, MacAgentClient};
#[cfg(test)]
use std::{collections::BTreeMap, io::Write};
use std::{
    fs::{self, File, OpenOptions},
    io::Read,
    net::SocketAddr,
    os::{
        fd::AsRawFd,
        unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    },
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use usque_core::chain_exit::store::ProfileCipher;
use usque_ipc::agent_v1::{self as p, macos_agent_request::Operation};
use uuid::Uuid;
#[cfg(test)]
use zeroize::Zeroizing;

pub use usque_platform::macos_proxy::Error;
use usque_platform::macos_proxy::{Backend, Journal, NativeBackend, Result};
#[cfg(test)]
use usque_platform::macos_proxy::{Change, Edit, Service, Value, Values};
const JOURNAL_ID: Uuid = Uuid::from_bytes([
    0x61, 0x40, 0x1b, 0x21, 0x73, 0xc7, 0x4f, 0x64, 0x9a, 0xc3, 0x82, 0xb0, 0x17, 0x12, 0xd6, 0x6c,
]);
const MAX_JOURNAL_BYTES: u64 = 512 * 1024;
#[derive(Clone)]
pub(crate) struct Context {
    path: PathBuf,
    cipher: Arc<dyn ProfileCipher>,
    backend: Arc<dyn Backend>,
    pending_lease: Arc<Mutex<Option<Uuid>>>,
}
impl Context {
    pub(crate) fn new(directory: &Path, cipher: Arc<dyn ProfileCipher>) -> Self {
        Self {
            path: directory.join("macos-system-proxy-v1.sealed"),
            cipher,
            backend: Arc::new(NativeBackend::default()),
            pending_lease: Default::default(),
        }
    }
    /// A failed start can have committed preferences before native apply or
    /// rollback failed. The durable intent is then a cleanup owner even though
    /// no live guard was returned. Treat an unreadable path as pending too.
    pub(crate) fn recovery_pending(&self) -> bool {
        self.path.try_exists().unwrap_or(true)
            || self
                .pending_lease
                .lock()
                .map_or(true, |lease| lease.is_some())
    }
    pub(crate) async fn start(&self, listener: SocketAddr) -> Result<MacSystemProxyGuard> {
        if !listener.ip().is_loopback() || listener.port() == 0 {
            return Err(Error::InvalidListener);
        }
        let executable = std::env::current_exe().map_err(|_| Error::Unavailable)?;
        // Read-only images may be translocated or mounted outside /Volumes.
        // Determine the backing filesystem directly for every launch path.
        let path = std::ffi::CString::new(executable.as_os_str().as_encoded_bytes())
            .map_err(|_| Error::Unavailable)?;
        let mut volume = std::mem::MaybeUninit::<libc::statfs>::uninit();
        // SAFETY: the C path and writable statfs storage remain live for this
        // read-only metadata query. No preferences have been opened.
        if unsafe { libc::statfs(path.as_ptr(), volume.as_mut_ptr()) } != 0 {
            return Err(Error::Unavailable);
        }
        // SAFETY: statfs succeeded and initialized the complete structure.
        let volume = unsafe { volume.assume_init() };
        if volume.f_flags & libc::MNT_RDONLY as u32 != 0 {
            return Err(Error::InstallRequired);
        }
        // Drain an older Engine's user-encrypted intent before handing ownership
        // to the root service. No new user-authorized transaction is created.
        self.recover(false).await?;
        let client = MacAgentClient::connect()
            .await
            .map_err(|_| Error::AgentRequired)?;
        let status = client.status().await.map_err(|_| Error::AgentRequired)?;
        if status.system_proxy_recovery_pending {
            client
                .empty(Operation::RecoverProxy(p::Empty {}))
                .await
                .map_err(|_| Error::RecoveryPending)?;
        }
        if !status.system_proxy_supported {
            return Err(Error::AgentRequired);
        }
        let lease = Uuid::new_v4();
        let request = Operation::AcquireProxy(p::MacosProxyListener {
            lease: lease.to_string(),
            loopback_ip: listener.ip().to_string(),
            port: listener.port().into(),
        });
        if let Err(error) = client.empty(request).await {
            if let Some(failure) = error
                .get_ref()
                .and_then(|e| e.downcast_ref::<AgentFailure>())
            {
                return Err(match failure.0 {
                    p::MacosFailure::Busy | p::MacosFailure::OwnershipConflict => Error::Conflict,
                    p::MacosFailure::ProxyNoService => Error::NoServices,
                    p::MacosFailure::Journal | p::MacosFailure::RecoveryIncomplete => {
                        *self
                            .pending_lease
                            .lock()
                            .map_err(|_| Error::RecoveryPending)? = Some(lease);
                        Error::RecoveryPending
                    }
                    _ => Error::Unavailable,
                });
            }
            *self
                .pending_lease
                .lock()
                .map_err(|_| Error::RecoveryPending)? = Some(lease);
            self.recover(false).await?;
            return Err(Error::Unavailable);
        }
        Ok(MacSystemProxyGuard { client, lease })
    }
    pub(crate) async fn recover(&self, interactive: bool) -> Result<()> {
        let lease = *self
            .pending_lease
            .lock()
            .map_err(|_| Error::RecoveryPending)?;
        if let Some(lease) = lease {
            release_on_new_connection(lease).await?;
            let mut pending = self
                .pending_lease
                .lock()
                .map_err(|_| Error::RecoveryPending)?;
            if *pending == Some(lease) {
                *pending = None;
            }
        }
        if !self.path.try_exists().map_err(|_| Error::RecoveryPending)? {
            return Ok(());
        }
        let context = self.clone();
        tokio::task::spawn_blocking(move || {
            usque_platform::macos_keychain::INTERACTIVE.sync_scope(interactive, || {
                let mut transaction =
                    Transaction::open(context.path, context.cipher, context.backend)?;
                transaction.restore(interactive)
            })
        })
        .await
        .map_err(|_| Error::RecoveryPending)?
    }
}
async fn release_on_new_connection(lease: Uuid) -> Result<()> {
    if let Ok(client) = MacAgentClient::connect().await
        && client.empty(release_request(lease)).await.is_ok()
    {
        return Ok(());
    }
    crate::macos_agent::confirm_proxy_cleanup(Path::new(crate::macos_agent::INSTALL_DIRECTORY))
        .await
        .map_err(|_| Error::RecoveryPending)
}
fn release_request(lease: Uuid) -> Operation {
    Operation::ReleaseProxy(p::MacosProxyRelease {
        lease: lease.to_string(),
    })
}
pub(crate) struct MacSystemProxyGuard {
    client: MacAgentClient,
    lease: Uuid,
}
impl MacSystemProxyGuard {
    pub fn active(&self) -> bool {
        self.client.proxy_stream_alive()
    }
    pub fn failure(&self) -> Option<String> {
        (!self.active()).then(|| "macOS system proxy service disconnected".into())
    }
    pub(crate) async fn shutdown(&mut self, _interactive: bool) -> Result<()> {
        if self
            .client
            .empty(release_request(self.lease))
            .await
            .is_err()
        {
            release_on_new_connection(self.lease).await?;
        }
        Ok(())
    }
}
struct Transaction {
    path: PathBuf,
    cipher: Arc<dyn ProfileCipher>,
    backend: Arc<dyn Backend>,
    journal: Option<Journal>,
    _lease: File,
}
impl Transaction {
    fn open(
        path: PathBuf,
        cipher: Arc<dyn ProfileCipher>,
        backend: Arc<dyn Backend>,
    ) -> Result<Self> {
        let parent = path.parent().ok_or(Error::Unavailable)?;
        fs::create_dir_all(parent).map_err(|_| Error::Unavailable)?;
        let directory = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY | libc::O_CLOEXEC)
            .open(parent)
            .map_err(|_| Error::Unavailable)?;
        let metadata = directory.metadata().map_err(|_| Error::Unavailable)?;
        if !metadata.is_dir() || metadata.uid() != effective_uid() {
            return Err(Error::Unavailable);
        }
        directory
            .set_permissions(fs::Permissions::from_mode(0o700))
            .map_err(|_| Error::Unavailable)?;
        let lease = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path.with_extension("lock"))
            .map_err(|_| Error::Unavailable)?;
        let metadata = lease.metadata().map_err(|_| Error::Unavailable)?;
        if !metadata.is_file() || metadata.uid() != effective_uid() || metadata.mode() & 0o077 != 0
        {
            return Err(Error::Unavailable);
        }
        // SAFETY: lease is an owned regular file; this nonblocking call only locks it.
        if unsafe { libc::flock(lease.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err(Error::Conflict);
        }
        let mut result = Self {
            path,
            cipher,
            backend,
            journal: None,
            _lease: lease,
        };
        result.load()?;
        Ok(result)
    }
    #[cfg(test)]
    fn start(
        path: PathBuf,
        cipher: Arc<dyn ProfileCipher>,
        backend: Arc<dyn Backend>,
        listener: SocketAddr,
    ) -> Result<Self> {
        if !listener.ip().is_loopback() || listener.port() == 0 {
            return Err(Error::InvalidListener);
        }
        let mut result = Self::open(path, cipher, backend)?;
        result.restore(true)?;
        let backend = Arc::clone(&result.backend);
        backend.transaction(true, &mut |services| {
            result.journal = Some(Journal::capture(services, listener)?);
            result.persist()?;
            Ok(result.journal.as_ref().unwrap().apply_patches())
        })?;
        Ok(result)
    }
    fn load(&mut self) -> Result<()> {
        let file = match OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&self.path)
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(_) => return Err(Error::RecoveryPending),
        };
        let metadata = file.metadata().map_err(|_| Error::RecoveryPending)?;
        if !metadata.is_file()
            || metadata.uid() != effective_uid()
            || metadata.mode() & 0o077 != 0
            || metadata.len() > MAX_JOURNAL_BYTES
        {
            return Err(Error::RecoveryPending);
        }
        let mut encrypted = Vec::new();
        file.take(MAX_JOURNAL_BYTES + 1)
            .read_to_end(&mut encrypted)
            .map_err(|_| Error::RecoveryPending)?;
        if encrypted.len() as u64 > MAX_JOURNAL_BYTES {
            return Err(Error::RecoveryPending);
        }
        let plaintext = self
            .cipher
            .open(JOURNAL_ID, &encrypted)
            .map_err(|_| Error::RecoveryPending)?;
        let journal: Journal =
            serde_json::from_slice(&plaintext).map_err(|_| Error::RecoveryPending)?;
        journal.validate()?;
        self.journal = Some(journal);
        Ok(())
    }
    #[cfg(test)]
    fn persist(&self) -> Result<()> {
        let plaintext = Zeroizing::new(
            serde_json::to_vec(self.journal.as_ref().ok_or(Error::RecoveryPending)?)
                .map_err(|_| Error::RecoveryPending)?,
        );
        let encrypted = self
            .cipher
            .seal(JOURNAL_ID, &plaintext)
            .map_err(|_| Error::RecoveryPending)?;
        if encrypted.len() as u64 > MAX_JOURNAL_BYTES {
            return Err(Error::RecoveryPending);
        }
        let parent = self.path.parent().ok_or(Error::RecoveryPending)?;
        let mut temporary =
            tempfile::NamedTempFile::new_in(parent).map_err(|_| Error::RecoveryPending)?;
        temporary
            .as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|_| Error::RecoveryPending)?;
        temporary
            .write_all(&encrypted)
            .map_err(|_| Error::RecoveryPending)?;
        temporary
            .as_file()
            .sync_all()
            .map_err(|_| Error::RecoveryPending)?;
        temporary
            .persist(&self.path)
            .map_err(|_| Error::RecoveryPending)?;
        File::open(parent)
            .and_then(|file| file.sync_all())
            .map_err(|_| Error::RecoveryPending)
    }
    fn restore(&mut self, interactive: bool) -> Result<()> {
        let Some(journal) = self.journal.as_ref() else {
            return Ok(());
        };
        self.backend.transaction(interactive, &mut |services| {
            Ok(journal.restore_patches(services))
        })?;
        fs::remove_file(&self.path).map_err(|_| Error::RecoveryPending)?;
        File::open(self.path.parent().ok_or(Error::RecoveryPending)?)
            .and_then(|file| file.sync_all())
            .map_err(|_| Error::RecoveryPending)?;
        self.journal = None;
        Ok(())
    }
}
impl Drop for Transaction {
    fn drop(&mut self) {
        let _ = self.restore(false);
    }
}
fn effective_uid() -> u32 {
    // SAFETY: geteuid has no preconditions.
    unsafe { libc::geteuid() }
}

#[cfg(test)]
mod tests;
