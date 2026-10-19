//! Bounded encrypted Proton session sidecars. No password or raw server response
//! is persisted, and a pending token rotation is never silently replayed.
use super::{AccountSummary, Record, error};
use crate::chain_exit::{ImportError, store::ProfileCipher};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};
use uuid::Uuid;
use zeroize::Zeroizing;

const MAX_PLAINTEXT: usize = 128 * 1024;
const MAX_CIPHERTEXT: usize = 192 * 1024;
const MAX_RECORDS: usize = 128;

pub struct SessionStore<'a> {
    directory: PathBuf,
    cipher: &'a dyn ProfileCipher,
}
impl<'a> SessionStore<'a> {
    pub fn new(parent: &Path, cipher: &'a dyn ProfileCipher) -> Self {
        Self {
            directory: parent.join("proton-vpn"),
            cipher,
        }
    }
    fn prepare_directory(&self) -> Result<(), ImportError> {
        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        match builder.create(&self.directory) {
            Ok(()) => (),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
            Err(_) => return Err(error("secure_storage_failed")),
        }
        let metadata =
            fs::symlink_metadata(&self.directory).map_err(|_| error("secure_storage_failed"))?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(error("secure_storage_failed"));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if metadata.permissions().mode() & 0o077 != 0 {
                return Err(error("secure_storage_failed"));
            }
        }
        Ok(())
    }
    fn lock(&self) -> Result<fs::File, ImportError> {
        self.prepare_directory()?;
        let store = crate::storage::ConfigStore::new(self.directory.join("catalog.json"));
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(200);
        loop {
            if let Some(lock) = store
                .try_lock_exclusive()
                .map_err(|_| error("secure_storage_failed"))?
            {
                return Ok(lock);
            }
            if std::time::Instant::now() >= deadline {
                return Err(error("session_busy"));
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
    /// Hold this lease across refresh and its journaled response. The separate
    /// catalogue lock protects disk operations, while this prevents concurrent
    /// workers from rotating the same remote session.
    pub fn lease(&self, id: Uuid) -> Result<fs::File, ImportError> {
        self.prepare_directory()?;
        crate::storage::ConfigStore::new(self.directory.join(format!("{id}.lease")))
            .try_lock_exclusive()
            .map_err(|_| error("secure_storage_failed"))?
            .ok_or_else(|| error("session_busy"))
    }
    fn path(&self, id: Uuid) -> PathBuf {
        self.directory.join(format!("{id}.sealed"))
    }
    fn read(&self, id: Uuid) -> Result<Record, ImportError> {
        let path = self.path(id);
        let metadata = fs::symlink_metadata(&path).map_err(|_| error("session_not_found"))?;
        if !metadata.is_file()
            || metadata.file_type().is_symlink()
            || metadata.len() > MAX_CIPHERTEXT as u64
        {
            return Err(error("secure_storage_failed"));
        }
        let mut encrypted = Vec::new();
        fs::File::open(path)
            .map_err(|_| error("secure_storage_failed"))?
            .take(MAX_CIPHERTEXT as u64 + 1)
            .read_to_end(&mut encrypted)
            .map_err(|_| error("secure_storage_failed"))?;
        if encrypted.len() > MAX_CIPHERTEXT {
            return Err(error("secure_storage_failed"));
        }
        let plaintext = self.cipher.open(id, &encrypted)?;
        if plaintext.len() > MAX_PLAINTEXT {
            return Err(error("secure_storage_failed"));
        }
        let record: Record =
            serde_json::from_slice(&plaintext).map_err(|_| error("secure_storage_failed"))?;
        record.validate()?;
        if record.id != id {
            return Err(error("secure_storage_failed"));
        }
        Ok(record)
    }
    fn ids(&self) -> Result<Vec<Uuid>, ImportError> {
        let mut ids = Vec::new();
        for entry in fs::read_dir(&self.directory).map_err(|_| error("secure_storage_failed"))? {
            let entry = entry.map_err(|_| error("secure_storage_failed"))?;
            let name = entry.file_name();
            if let Some(id) = name
                .to_str()
                .and_then(|name| name.strip_suffix(".sealed"))
                .and_then(|name| Uuid::parse_str(name).ok())
            {
                if ids.len() >= MAX_RECORDS {
                    return Err(error("profile_limit"));
                }
                ids.push(id);
            }
        }
        ids.sort_unstable();
        Ok(ids)
    }
    pub fn load(&self, id: Uuid) -> Result<Record, ImportError> {
        let _guard = self.lock()?;
        self.read(id)
    }
    pub fn list(&self) -> Result<Vec<AccountSummary>, ImportError> {
        let _guard = self.lock()?;
        self.ids()?
            .into_iter()
            .map(|id| self.read(id).map(|r| r.summary()))
            .collect()
    }
    pub fn save(&self, record: &Record) -> Result<(), ImportError> {
        record.validate()?;
        let _guard = self.lock()?;
        let ids = self.ids()?;
        if !ids.contains(&record.id) && ids.len() >= MAX_RECORDS {
            return Err(error("profile_limit"));
        }
        let path = self.path(record.id);
        match fs::symlink_metadata(&path) {
            Ok(meta) if meta.file_type().is_symlink() || !meta.is_file() => {
                return Err(error("secure_storage_failed"));
            }
            Ok(_) => (),
            Err(failure) if failure.kind() == std::io::ErrorKind::NotFound => (),
            Err(_) => return Err(error("secure_storage_failed")),
        }
        let plaintext =
            Zeroizing::new(serde_json::to_vec(record).map_err(|_| error("secure_storage_failed"))?);
        if plaintext.len() > MAX_PLAINTEXT {
            return Err(error("secure_storage_failed"));
        }
        let encrypted = self.cipher.seal(record.id, &plaintext)?;
        if encrypted.is_empty() || encrypted.len() > MAX_CIPHERTEXT {
            return Err(error("secure_storage_failed"));
        }
        let mut file = tempfile::Builder::new()
            .prefix(".proton-")
            .tempfile_in(&self.directory)
            .map_err(|_| error("secure_storage_failed"))?;
        file.write_all(&encrypted)
            .map_err(|_| error("secure_storage_failed"))?;
        file.as_file()
            .sync_all()
            .map_err(|_| error("secure_storage_failed"))?;
        #[cfg(windows)]
        persist_windows(file, &path)?;
        #[cfg(not(windows))]
        {
            file.persist(&path)
                .map_err(|_| error("secure_storage_failed"))?;
            #[cfg(unix)]
            {
                // The rotation journal is not committed merely because its
                // encrypted file contents were flushed. Persist the containing
                // directory entry before permitting a remote token rotation.
                // If this fails the new file may already be visible: report an
                // error and let the caller stop, never roll back to stale tokens.
                fs::File::open(&self.directory)
                    .and_then(|directory| directory.sync_all())
                    .map_err(|_| error("secure_storage_failed"))?;
            }
        }
        Ok(())
    }
    pub fn delete(&self, id: Uuid) -> Result<(), ImportError> {
        let _guard = self.lock()?;
        let path = self.path(id);
        let metadata = fs::symlink_metadata(&path).map_err(|_| error("session_not_found"))?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(error("secure_storage_failed"));
        }
        fs::remove_file(path).map_err(|_| error("secure_storage_failed"))
    }
}

#[cfg(windows)]
fn persist_windows(file: tempfile::NamedTempFile, destination: &Path) -> Result<(), ImportError> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_NORMAL, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
        SetFileAttributesW,
    };

    let (file, mut temporary) = file.into_parts();
    let mut source: Vec<u16> = temporary.as_os_str().encode_wide().collect();
    let mut destination: Vec<u16> = destination.as_os_str().encode_wide().collect();
    if source.contains(&0) || destination.contains(&0) {
        return Err(error("secure_storage_failed"));
    }
    source.push(0);
    destination.push(0);
    // NamedTempFile uses FILE_ATTRIBUTE_TEMPORARY on Windows. Remove that
    // caching hint and flush the file again before its same-directory rename.
    // Keep TempPath ownership throughout: every failure removes the temporary
    // file without attempting to replace or restore the previous sealed object.
    // SAFETY: source is a null-terminated path to our exclusively created file
    // and remains alive throughout the synchronous Windows API call.
    if unsafe { SetFileAttributesW(source.as_ptr(), FILE_ATTRIBUTE_NORMAL) } == 0 {
        return Err(error("secure_storage_failed"));
    }
    file.sync_all()
        .map_err(|_| error("secure_storage_failed"))?;
    drop(file);
    // No cross-volume copy flag is allowed. Both paths are in the same private
    // directory, so this is an atomic replacement with write-through requested.
    // Success is the durability boundary before a subsequent refresh request;
    // failure is deliberately opaque and must prevent that request. Actual
    // power-loss behavior remains a platform validation task, not a unit test.
    // SAFETY: both null-terminated wide paths outlive this synchronous call.
    let moved = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if moved == 0 {
        return Err(error("secure_storage_failed"));
    }
    // The original temporary name no longer identifies our file. Avoid trying
    // to remove that name during Drop after the successful replacement.
    temporary.disable_cleanup(true);
    Ok(())
}
