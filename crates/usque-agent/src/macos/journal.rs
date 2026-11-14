//! Root-owned Darwin journal. Missing evidence is not a clean journal.

use super::transaction::{Store, Transaction};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    os::{
        fd::AsRawFd,
        unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    },
    path::{Path, PathBuf},
};

const MAX_BYTES: u64 = 256 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope<T> {
    schema: u32,
    revision: u64,
    transaction: Option<T>,
}

pub type JournalStore = SecureStore<Transaction>;

/// Held even in quarantine, when neither network journal could be loaded.
pub(super) fn service_lock() -> io::Result<File> {
    let path = Path::new(super::STATE_DIRECTORY).join("service.lock");
    verify_root_parents(&path)?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(path)?;
    verify_file(&file, 0)?;
    // SAFETY: the owned lock file is held for the complete service lifetime.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(file)
}

pub trait Evidence: Clone + Serialize + DeserializeOwned {
    fn validate(&self) -> io::Result<()>;
}
impl Evidence for Transaction {
    fn validate(&self) -> io::Result<()> {
        Transaction::validate(self)
    }
}

pub struct SecureStore<T: Evidence> {
    _value: std::marker::PhantomData<T>,
    path: PathBuf,
    uid: u32,
    revision: u64,
    _lock: File,
}

impl JournalStore {
    pub fn open() -> io::Result<Self> {
        let path = PathBuf::from(super::JOURNAL_PATH);
        verify_root_parents(&path)?;
        Self::at(path, 0)
    }
}
impl<T: Evidence> SecureStore<T> {
    #[cfg(test)]
    pub(super) fn for_test(path: PathBuf) -> io::Result<Self> {
        // SAFETY: read-only process credential query.
        Self::at(path, unsafe { libc::geteuid() })
    }
    pub(super) fn open_root(path: &'static str) -> io::Result<Self> {
        let path = PathBuf::from(path);
        verify_root_parents(&path)?;
        Self::at(path, 0)
    }
    fn at(path: PathBuf, uid: u32) -> io::Result<Self> {
        verify_parent(&path, uid)?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
            .open(path.with_extension("lock"))?;
        verify_file(&lock, uid)?;
        // SAFETY: flock acquires a nonblocking exclusive lock on the owned fd.
        if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self {
            _value: std::marker::PhantomData,
            path,
            uid,
            revision: 0,
            _lock: lock,
        })
    }
    pub fn load(&mut self) -> io::Result<Option<T>> {
        verify_parent(&self.path, self.uid)?;
        let file = open_record(&self.path)?;
        verify_file(&file, self.uid)?;
        let mut bytes = Vec::new();
        file.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err(invalid());
        }
        let envelope: Envelope<T> = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
        if envelope.schema != 1 || envelope.revision == 0 {
            return Err(invalid());
        }
        if let Some(value) = &envelope.transaction {
            value.validate()?;
        }
        self.revision = envelope.revision;
        Ok(envelope.transaction)
    }
    pub fn save_value(&mut self, value: Option<&T>) -> io::Result<()> {
        if let Some(value) = value {
            value.validate()?;
        }
        verify_parent(&self.path, self.uid)?;
        match open_record(&self.path) {
            Ok(file) => {
                verify_file(&file, self.uid)?;
                if self.revision == 0 {
                    return Err(invalid());
                }
                let mut bytes = Vec::new();
                file.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
                if bytes.len() as u64 > MAX_BYTES {
                    return Err(invalid());
                }
                let previous: Envelope<T> =
                    serde_json::from_slice(&bytes).map_err(|_| invalid())?;
                if previous.schema != 1 || previous.revision != self.revision {
                    return Err(invalid());
                }
                if let Some(value) = previous.transaction {
                    value.validate()?;
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound && self.revision == 0 => {}
            Err(error) => return Err(error),
        }
        let next = self.revision.checked_add(1).ok_or_else(invalid)?;
        let envelope = Envelope {
            schema: 1,
            revision: next,
            transaction: value.cloned(),
        };
        let bytes = serde_json::to_vec(&envelope).map_err(io::Error::other)?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err(invalid());
        }
        let parent = self.path.parent().ok_or_else(invalid)?;
        let mut temp = tempfile::NamedTempFile::new_in(parent)?;
        temp.as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))?;
        temp.write_all(&bytes)?;
        temp.flush()?;
        temp.as_file().sync_all()?;
        // SAFETY: F_FULLFSYNC requests the Darwin disk-cache flush required for
        // a durable pre-mutation intent; failure must stop the transaction.
        if unsafe { libc::fcntl(temp.as_file().as_raw_fd(), libc::F_FULLFSYNC) } != 0 {
            return Err(io::Error::last_os_error());
        }
        let persisted = temp.persist(&self.path).map_err(|error| error.error)?;
        File::open(parent)?.sync_all()?;
        // fsync(directory) submits the rename metadata; a final full disk-cache
        // flush is required *after* it, before a caller may mutate networking.
        // SAFETY: persisted is a live file on the same filesystem as parent.
        if unsafe { libc::fcntl(persisted.as_raw_fd(), libc::F_FULLFSYNC) } != 0 {
            return Err(io::Error::last_os_error());
        }
        self.revision = next;
        Ok(())
    }
}

impl Store for JournalStore {
    fn save(&mut self, value: Option<&Transaction>) -> io::Result<()> {
        self.save_value(value)
    }
}

fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "unsafe or corrupt macOS recovery journal",
    )
}
fn open_record(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(path)
}
fn verify_file(file: &File, uid: u32) -> io::Result<()> {
    let m = file.metadata()?;
    if !m.is_file()
        || m.uid() != uid
        || m.mode() & 0o077 != 0
        || m.nlink() != 1
        || m.len() > MAX_BYTES
    {
        return Err(invalid());
    }
    no_extended_acl(file)
}
fn verify_parent(path: &Path, uid: u32) -> io::Result<()> {
    let parent = path.parent().ok_or_else(invalid)?;
    let directory = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(parent)?;
    let m = directory.metadata()?;
    if m.uid() != uid || m.mode() & 0o077 != 0 {
        return Err(invalid());
    }
    no_extended_acl(&directory)
}
pub(super) fn verify_root_parents(path: &Path) -> io::Result<()> {
    for parent in path.ancestors().skip(1) {
        let directory = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(parent)?;
        let m = directory.metadata()?;
        if m.uid() != 0 || m.mode() & 0o022 != 0 {
            return Err(invalid());
        }
        no_extended_acl(&directory)?;
    }
    Ok(())
}
// libc does not expose Darwin's extended ACL API. Reject *all* extended ACLs
// on the private store (including restrictive ones) rather than overlooking
// an ACL grant that bypasses Unix mode bits.
unsafe extern "C" {
    fn acl_get_fd_np(fd: libc::c_int, kind: libc::c_int) -> *mut std::ffi::c_void;
    fn acl_free(value: *mut std::ffi::c_void) -> libc::c_int;
}
pub(super) fn no_extended_acl(file: &File) -> io::Result<()> {
    // SAFETY: the fd is live, ACL_TYPE_EXTENDED is the SDK's 0x100 value.
    let acl = unsafe { acl_get_fd_np(file.as_raw_fd(), 0x100) };
    if acl.is_null() {
        let error = io::Error::last_os_error();
        // A valid open fd with no FILESEC_ACL property returns ENOENT in
        // Darwin's acl_get_fd_np; this is distinct from an unreadable ACL.
        return if error.raw_os_error() == Some(libc::ENOENT) {
            Ok(())
        } else {
            Err(error)
        };
    }
    // SAFETY: release the ACL allocation exactly once. Even an explicitly
    // empty ACL is refused; the installer must create plain private files.
    unsafe {
        acl_free(acl);
    }
    Err(invalid())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::PermissionsExt};
    fn fixture() -> (tempfile::TempDir, PathBuf, u32) {
        let dir = tempfile::tempdir().unwrap();
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let path = dir.path().join("network.json");
        // SAFETY: reading our own UID has no preconditions.
        (dir, path, unsafe { libc::geteuid() })
    }
    #[test]
    fn round_trips_durable_clean_record_and_locks_out_a_second_agent() {
        let (_dir, path, uid) = fixture();
        let mut store = JournalStore::at(path.clone(), uid).unwrap();
        assert_eq!(store.load().unwrap_err().kind(), io::ErrorKind::NotFound);
        store.save(None).unwrap();
        assert_eq!(store.load().unwrap(), None);
        assert!(JournalStore::at(path.clone(), uid).is_err());
        drop(store);
        assert_eq!(JournalStore::at(path, uid).unwrap().load().unwrap(), None);
    }
    #[test]
    fn corrupt_unknown_and_oversize_records_never_become_clean() {
        let (_dir, path, uid) = fixture();
        let mut store = JournalStore::at(path.clone(), uid).unwrap();
        store.save(None).unwrap();
        for bytes in [
            b"{broken".to_vec(),
            br#"{"schema":2,"revision":1,"transaction":null}"#.to_vec(),
            br#"{"schema":1,"revision":1,"transaction":null,"other":1}"#.to_vec(),
            vec![b' '; MAX_BYTES as usize + 1],
        ] {
            fs::write(&path, bytes).unwrap();
            assert!(store.load().is_err());
        }
    }
    #[test]
    fn refuses_symlinks_hardlinks_and_world_readable_records() {
        let (dir, path, uid) = fixture();
        let mut store = JournalStore::at(path.clone(), uid).unwrap();
        store.save(None).unwrap();
        let other = dir.path().join("other");
        fs::hard_link(&path, &other).unwrap();
        assert!(store.load().is_err());
        fs::remove_file(&other).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(store.load().is_err());
        fs::rename(&path, &other).unwrap();
        std::os::unix::fs::symlink(&other, &path).unwrap();
        assert!(store.load().is_err());
        assert!(store.save(None).is_err());
    }
    #[test]
    fn refuses_unsafe_parent_and_unsafe_lock() {
        let (dir, path, uid) = fixture();
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o755)).unwrap();
        assert!(JournalStore::at(path.clone(), uid).is_err());
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
        std::os::unix::fs::symlink("/dev/null", path.with_extension("lock")).unwrap();
        assert!(JournalStore::at(path, uid).is_err());
    }
    #[test]
    fn existing_evidence_must_be_loaded_before_any_replacement() {
        let (_dir, path, uid) = fixture();
        let mut store = JournalStore::at(path.clone(), uid).unwrap();
        store.save(None).unwrap();
        drop(store);
        let mut store = JournalStore::at(path.clone(), uid).unwrap();
        assert!(store.save(None).is_err());
        store.load().unwrap();
        fs::write(&path, br#"{"schema":1,"revision":999,"transaction":null}"#).unwrap();
        assert!(store.save(None).is_err());
        fs::write(&path, b"{broken").unwrap();
        assert!(store.save(None).is_err());
    }
}
