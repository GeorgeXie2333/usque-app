//! Atomic encrypted objects. The platform owns encryption; settings contain
//! only opaque IDs/revisions, and failed writes cannot replace a valid object.
use super::*;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

pub trait ProfileCipher: Send + Sync {
    fn seal(&self, id: Uuid, value: &[u8]) -> Result<Vec<u8>, ImportError>;
    fn open(&self, id: Uuid, value: &[u8]) -> Result<Zeroizing<Vec<u8>>, ImportError>;
}

/// Called only by the explicit clear-all-data flow after connection cleanup.
/// Enumerates only this library's regular files; never follows linked paths.
pub fn clear_library(parent: &Path) -> Result<(), ImportError> {
    let directory = parent.join("chain-profiles");
    if !directory.exists() {
        return Ok(());
    }
    let meta = fs::symlink_metadata(&directory).map_err(|_| storage_error())?;
    if !meta.is_dir() || meta.file_type().is_symlink() {
        return Err(storage_error());
    }
    let _guard = crate::storage::ConfigStore::new(directory.join("catalog.json"))
        .lock_exclusive()
        .map_err(|_| storage_error())?;
    for entry in fs::read_dir(directory).map_err(|_| storage_error())? {
        let entry = entry.map_err(|_| storage_error())?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if (name.starts_with(".chain-")
            || name
                .strip_suffix(".profile")
                .is_some_and(|s| Uuid::parse_str(s).is_ok()))
            && entry.file_type().map_err(|_| storage_error())?.is_file()
        {
            fs::remove_file(entry.path()).map_err(|_| storage_error())?;
        }
    }
    Ok(())
}
#[derive(Serialize, Deserialize)]
struct Record {
    version: u32,
    summary: ChainProfileSummary,
    secrets: ImportSecrets,
}

pub struct ChainProfileStore<'a> {
    directory: PathBuf,
    cipher: &'a dyn ProfileCipher,
}
impl<'a> ChainProfileStore<'a> {
    pub fn new(parent: &Path, cipher: &'a dyn ProfileCipher) -> Self {
        Self {
            directory: parent.join("chain-profiles"),
            cipher,
        }
    }
    fn path(&self, id: Uuid) -> PathBuf {
        self.directory.join(format!("{id}.profile"))
    }
    fn lock(&self) -> Result<fs::File, ImportError> {
        if self.directory.exists() {
            let meta = fs::symlink_metadata(&self.directory).map_err(|_| storage_error())?;
            if !meta.is_dir() || meta.file_type().is_symlink() {
                return Err(storage_error());
            }
        }
        crate::storage::ConfigStore::new(self.directory.join("catalog.json"))
            .lock_exclusive()
            .map_err(|_| storage_error())
    }
    fn read(&self, id: Uuid) -> Result<Record, ImportError> {
        let path = self.path(id);
        let meta = fs::symlink_metadata(&path).map_err(|_| storage_error())?;
        if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > 256 * 1024 {
            return Err(storage_error());
        }
        let mut encrypted = Vec::new();
        fs::File::open(path)
            .map_err(|_| storage_error())?
            .take(256 * 1024 + 1)
            .read_to_end(&mut encrypted)
            .map_err(|_| storage_error())?;
        if encrypted.len() > 256 * 1024 {
            return Err(storage_error());
        }
        let plaintext = self.cipher.open(id, &encrypted)?;
        if plaintext.len() > 192 * 1024 {
            return Err(storage_error());
        }
        let record: Record = serde_json::from_slice(&plaintext).map_err(|_| storage_error())?;
        if record.version != 1 || record.summary.id != id {
            return Err(storage_error());
        }
        let validated = ValidatedProfile::parse(record.summary.protocol.source(), &record.secrets)?;
        let mut expected = validated.summary(&record.summary.name, id, record.summary.revision)?;
        expected.edit_revision = record.summary.edit_revision;
        if expected != record.summary {
            return Err(storage_error());
        }
        Ok(record)
    }
    fn ids(&self) -> Result<Vec<Uuid>, ImportError> {
        if !self.directory.exists() {
            return Ok(vec![]);
        }
        let mut ids = Vec::new();
        for entry in fs::read_dir(&self.directory).map_err(|_| storage_error())? {
            let entry = entry.map_err(|_| storage_error())?;
            let name = entry.file_name();
            let Some(name) = name.to_str().and_then(|s| s.strip_suffix(".profile")) else {
                continue;
            };
            let id = Uuid::parse_str(name).map_err(|_| storage_error())?;
            if name != id.to_string() || ids.len() >= MAX_IMPORTED_PROFILES {
                return Err(storage_error());
            }
            ids.push(id);
        }
        Ok(ids)
    }
    pub fn list(&self) -> Result<Vec<ChainProfileSummary>, ImportError> {
        let _guard = self.lock()?;
        for entry in fs::read_dir(&self.directory).map_err(|_| storage_error())? {
            let entry = entry.map_err(|_| storage_error())?;
            if entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with(".chain-"))
                && entry.file_type().map_err(|_| storage_error())?.is_file()
            {
                fs::remove_file(entry.path()).map_err(|_| storage_error())?;
            }
        }
        let mut entries = self
            .ids()?
            .into_iter()
            .map(|id| self.read(id).map(|r| r.summary))
            .collect::<Result<Vec<_>, _>>()?;
        entries.sort_by(|a, b| {
            (a.protocol.source() as u8, &a.name, a.id).cmp(&(
                b.protocol.source() as u8,
                &b.name,
                b.id,
            ))
        });
        Ok(entries)
    }
    pub fn import(
        &self,
        source: ChainSource,
        name: &str,
        secrets: ImportSecrets,
    ) -> Result<ChainProfileSummary, ImportError> {
        let parsed = ValidatedProfile::parse(source, &secrets)?;
        let summary = parsed.summary(name, Uuid::new_v4(), Uuid::new_v4())?;
        let _guard = self.lock()?;
        if self.ids()?.len() >= MAX_IMPORTED_PROFILES {
            return Err(ImportError::new(0, "profiles", "profile_limit"));
        }
        self.write(&Record {
            version: 1,
            summary: summary.clone(),
            secrets,
        })?;
        Ok(summary)
    }
    fn write(&self, record: &Record) -> Result<(), ImportError> {
        // Binder carries UTF-16 strings. Bound the complete metadata catalogue
        // below its transaction budget, including each profile's AllowedIPs.
        let mut metadata_bytes = serde_json::to_vec(&record.summary)
            .map_err(|_| storage_error())?
            .len();
        for id in self.ids()? {
            if id != record.summary.id {
                metadata_bytes += serde_json::to_vec(&self.read(id)?.summary)
                    .map_err(|_| storage_error())?
                    .len();
            }
        }
        if metadata_bytes > 256 * 1024 {
            return Err(ImportError::new(0, "profiles", "metadata_limit"));
        }
        let plaintext = Zeroizing::new(serde_json::to_vec(record).map_err(|_| storage_error())?);
        let encrypted = self.cipher.seal(record.summary.id, &plaintext)?;
        if encrypted.is_empty() || encrypted.len() > 256 * 1024 {
            return Err(storage_error());
        }
        let mut file = tempfile::Builder::new()
            .prefix(".chain-")
            .tempfile_in(&self.directory)
            .map_err(|_| storage_error())?;
        file.write_all(&encrypted).map_err(|_| storage_error())?;
        file.as_file().sync_all().map_err(|_| storage_error())?;
        file.persist(self.path(record.summary.id))
            .map_err(|_| storage_error())?;
        Ok(())
    }
    pub fn load(
        &self,
        settings: &ChainExitSettings,
    ) -> Result<(ChainProfileSummary, ValidatedProfile, ImportSecrets), ImportError> {
        settings.validate()?;
        let id = settings
            .profile_id
            .ok_or_else(|| ImportError::new(0, "selection", "missing_profile"))?;
        let record = self.read(id)?;
        if Some(record.summary.revision) != settings.revision
            || record.summary.protocol.source() != settings.source
        {
            return Err(ImportError::new(0, "selection", "stale_revision"));
        }
        let parsed = ValidatedProfile::parse(settings.source, &record.secrets)?;
        Ok((record.summary, parsed, record.secrets))
    }
    pub fn rename(
        &self,
        id: Uuid,
        revision: Uuid,
        name: &str,
    ) -> Result<ChainProfileSummary, ImportError> {
        let _guard = self.lock()?;
        let mut record = self.read(id)?;
        if record.summary.edit_revision != revision {
            return Err(ImportError::new(0, "revision", "stale_revision"));
        }
        let parsed = ValidatedProfile::parse(record.summary.protocol.source(), &record.secrets)?;
        // Display-only edits do not invalidate a selected immutable protocol revision.
        record.summary = parsed.summary(name, id, record.summary.revision)?;
        record.summary.edit_revision = Uuid::new_v4();
        self.write(&record)?;
        Ok(record.summary)
    }
    pub fn remove(&self, id: Uuid, revision: Uuid, retained: &[Uuid]) -> Result<(), ImportError> {
        let _guard = self.lock()?;
        if retained.contains(&id) {
            return Err(ImportError::new(0, "profile", "profile_in_use"));
        }
        if self.read(id)?.summary.edit_revision != revision {
            return Err(ImportError::new(0, "revision", "stale_revision"));
        }
        fs::remove_file(self.path(id)).map_err(|_| storage_error())
    }
    pub fn update_credentials(
        &self,
        id: Uuid,
        revision: Uuid,
        credentials: ImportSecrets,
    ) -> Result<ChainProfileSummary, ImportError> {
        let _guard = self.lock()?;
        let mut record = self.read(id)?;
        if record.summary.edit_revision != revision {
            return Err(ImportError::new(0, "revision", "stale_revision"));
        }
        record.secrets.username.clone_from(&credentials.username);
        record.secrets.password.clone_from(&credentials.password);
        record
            .secrets
            .private_key_password
            .clone_from(&credentials.private_key_password);
        record.secrets.validate()?;
        record.summary.edit_revision = Uuid::new_v4();
        // Running sessions retain their own zeroizing credential snapshot.
        self.write(&record)?;
        Ok(record.summary)
    }
}
fn storage_error() -> ImportError {
    ImportError::new(0, "storage", "secure_storage_failed")
}

#[cfg(windows)]
pub struct WindowsProfileCipher;
#[cfg(windows)]
impl ProfileCipher for WindowsProfileCipher {
    fn seal(&self, id: Uuid, value: &[u8]) -> Result<Vec<u8>, ImportError> {
        windows_crypt(id, value, true).map(|v| v.to_vec())
    }
    fn open(&self, id: Uuid, value: &[u8]) -> Result<Zeroizing<Vec<u8>>, ImportError> {
        windows_crypt(id, value, false)
    }
}
#[cfg(windows)]
fn windows_crypt(id: Uuid, value: &[u8], encrypt: bool) -> Result<Zeroizing<Vec<u8>>, ImportError> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
    };
    let input = CRYPT_INTEGER_BLOB {
        cbData: value.len().try_into().map_err(|_| storage_error())?,
        pbData: value.as_ptr().cast_mut(),
    };
    let entropy = format!("Usque/chain-profiles/v1/{id}");
    let entropy = CRYPT_INTEGER_BLOB {
        cbData: entropy.len() as u32,
        pbData: entropy.as_ptr().cast_mut(),
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };
    // SAFETY: inputs remain alive and are not modified by DPAPI. Output is
    // allocated by DPAPI; we copy, wipe, and free it before returning.
    let ok = unsafe {
        if encrypt {
            CryptProtectData(
                &input,
                std::ptr::null(),
                &entropy,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        } else {
            CryptUnprotectData(
                &input,
                std::ptr::null_mut(),
                &entropy,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        }
    };
    if ok == 0 || output.pbData.is_null() {
        return Err(storage_error());
    }
    // SAFETY: successful DPAPI owns a readable allocation of cbData bytes.
    let result = unsafe {
        let bytes = std::slice::from_raw_parts_mut(output.pbData, output.cbData as usize);
        let result = Zeroizing::new(bytes.to_vec());
        bytes.zeroize();
        LocalFree(output.pbData.cast());
        result
    };
    Ok(result)
}
