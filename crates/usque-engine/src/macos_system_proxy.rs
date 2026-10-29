//! Owned per-service macOS HTTP proxy settings with encrypted crash recovery.
//! Only bounded scalar ownership observations enter the encrypted journal. PAC
//! contents are hashed inside the native boundary; SOCKS settings, exclusion
//! lists and credential fields are retained as native objects without export.
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    net::SocketAddr,
    os::{
        fd::AsRawFd,
        unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    },
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use usque_core::chain_exit::store::ProfileCipher;
use uuid::Uuid;
use zeroize::Zeroizing;

mod native;
const JOURNAL_ID: Uuid = Uuid::from_bytes([
    0x61, 0x40, 0x1b, 0x21, 0x73, 0xc7, 0x4f, 0x64, 0x9a, 0xc3, 0x82, 0xb0, 0x17, 0x12, 0xd6, 0x6c,
]);
const MAX_JOURNAL_BYTES: u64 = 512 * 1024;
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
    #[error("macOS system proxy authorization was cancelled; retry to request permission again")]
    PermissionCancelled,
    #[error("macOS system proxy authorization is required; retry to request permission")]
    PermissionRequired,
    #[error("macOS system proxy preferences are busy or changed; retry")]
    Conflict,
    #[error("macOS system proxy recovery could not be verified; its encrypted journal is retained")]
    RecoveryPending,
    #[error("macOS system proxy requires an enabled physical network service")]
    NoServices,
    #[error("macOS system proxy requires a loopback HTTP listener")]
    InvalidListener,
    #[error("macOS system proxy configuration is unavailable")]
    Unavailable,
}
impl Error {
    pub(crate) fn code(self) -> &'static str {
        match self {
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
    pub(crate) fn retryable(self) -> bool {
        self != Self::InvalidListener
    }
}
type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum Value {
    Text(String),
    Integer(i64),
    Boolean(bool),
}
type Values = BTreeMap<String, Value>;
#[derive(Clone)]
struct Service {
    id: String,
    eligible: bool,
    values: Values,
    pac_fingerprint: String,
}
#[derive(Clone)]
struct Patch {
    service: String,
    key: String,
    before: Option<Value>,
    after: Option<Value>,
}
type Edit<'a> = dyn FnMut(&[Service]) -> Result<Vec<Patch>> + 'a;
trait Backend: Send + Sync {
    /// Hold the native preferences lock across observation, durable intent,
    /// compare-and-set, commit and apply. A cancelled prompt occurs before edit.
    fn transaction(&self, interactive: bool, edit: &mut Edit<'_>) -> Result<()>;
}

#[derive(Clone)]
pub(crate) struct Context {
    path: PathBuf,
    cipher: Arc<dyn ProfileCipher>,
}
impl Context {
    pub(crate) fn new(directory: &Path, cipher: Arc<dyn ProfileCipher>) -> Self {
        Self {
            path: directory.join("macos-system-proxy-v1.sealed"),
            cipher,
        }
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
        let context = self.clone();
        let transaction = tokio::task::spawn_blocking(move || {
            Transaction::start(
                context.path,
                context.cipher,
                Arc::new(native::NativeBackend::default()),
                listener,
            )
        })
        .await
        .map_err(|_| Error::Unavailable)??;
        Ok(MacSystemProxyGuard {
            transaction: Arc::new(Mutex::new(transaction)),
        })
    }
    pub(crate) async fn recover(&self, interactive: bool) -> Result<()> {
        if !self.path.try_exists().map_err(|_| Error::RecoveryPending)? {
            return Ok(());
        }
        let context = self.clone();
        tokio::task::spawn_blocking(move || {
            let mut transaction = Transaction::open(
                context.path,
                context.cipher,
                Arc::new(native::NativeBackend::default()),
            )?;
            // Startup passes false and never creates an authorization object.
            // Only an explicit retry may request the permission dialog.
            transaction.restore(interactive)
        })
        .await
        .map_err(|_| Error::RecoveryPending)?
    }
}
pub(crate) struct MacSystemProxyGuard {
    transaction: Arc<Mutex<Transaction>>,
}
impl MacSystemProxyGuard {
    pub(crate) async fn shutdown(&mut self) -> Result<()> {
        let transaction = Arc::clone(&self.transaction);
        tokio::task::spawn_blocking(move || {
            transaction
                .lock()
                .map_err(|_| Error::RecoveryPending)?
                .restore(false)
        })
        .await
        .map_err(|_| Error::RecoveryPending)?
    }
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Change {
    service: String,
    key: String,
    before: Option<Value>,
    applied: Value,
}
#[derive(Serialize, Deserialize)]
struct Journal {
    version: u32,
    changes: Vec<Change>,
    // All group members are observed, including members already equal to the
    // target. Such observations never cause no-op native mutations.
    observations: Vec<Change>,
    pac_fingerprints: BTreeMap<String, String>,
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
        let desired = desired(listener);
        backend.transaction(true, &mut |services| {
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
            result.journal = Some(Journal {
                version: 2,
                changes,
                observations,
                pac_fingerprints,
            });
            // Durable encrypted intent is completed before the native backend
            // receives any patch, including before its first staged mutation.
            result.persist()?;
            Ok(result
                .journal
                .as_ref()
                .unwrap()
                .changes
                .iter()
                .map(|change| Patch {
                    service: change.service.clone(),
                    key: change.key.clone(),
                    before: change.before.clone(),
                    after: Some(change.applied.clone()),
                })
                .collect())
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
        let mut identities = BTreeSet::new();
        let mut observed = BTreeSet::new();
        if journal.version != 2
            || journal.changes.len() > MAX_SERVICES * KEYS.len()
            || journal.observations.len() > MAX_SERVICES * KEYS.len()
            || journal.pac_fingerprints.len() > MAX_SERVICES
            || journal.observations.iter().any(|observation| {
                !valid_observation(observation)
                    || !observed.insert((observation.service.clone(), observation.key.clone()))
            })
            || journal.changes.iter().any(|change| {
                !valid_observation(change)
                    || change.before.as_ref() == Some(&change.applied)
                    || !identities.insert((change.service.clone(), change.key.clone()))
                    || !journal.observations.contains(change)
            })
            || journal
                .pac_fingerprints
                .iter()
                .any(|(service, fingerprint)| {
                    !valid_service_id(service)
                        || fingerprint.len() != 64
                        || !fingerprint.bytes().all(|byte| byte.is_ascii_hexdigit())
                        || KEYS
                            .iter()
                            .any(|key| !observed.contains(&(service.clone(), (*key).into())))
                })
            || journal
                .observations
                .iter()
                .any(|observation| !journal.pac_fingerprints.contains_key(&observation.service))
        {
            // Older incomplete per-key journals are retained: without the
            // complete observations they cannot prove ownership of a group.
            return Err(Error::RecoveryPending);
        }
        self.journal = Some(journal);
        Ok(())
    }
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
            Ok(journal
                .changes
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
                        _ => &[],
                    };
                    // A partial native apply may leave a mixture of our
                    // before/applied values. Any other group member indicates
                    // a later owner, whose endpoint AND enable flag must stay.
                    if group.iter().any(|key| {
                        let observation = journal.observations.iter().find(|observation| {
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
                        && journal.pac_fingerprints.get(&change.service)
                            != Some(&service.pac_fingerprint)
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
                .collect())
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
fn effective_uid() -> u32 {
    // SAFETY: geteuid has no preconditions.
    unsafe { libc::geteuid() }
}

#[cfg(test)]
mod tests;
