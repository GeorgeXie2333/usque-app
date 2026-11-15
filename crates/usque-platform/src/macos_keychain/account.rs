//! One login Keychain item per WARP identity. Explicit proxy credentials and
//! the desktop encryption key keep their independent lifetimes and namespaces.
use super::{KeychainStore, SecretRecord, VaultError, account_name, validate_size};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    os::unix::fs::OpenOptionsExt,
    path::PathBuf,
};
use uuid::Uuid;
use zeroize::Zeroizing;

// This order is the on-disk v1 format. Each field has a big-endian u16 length;
// zero denotes an absent optional field.
pub(super) const FIELDS: [SecretRecord; 9] = [
    SecretRecord::WarpSecret,
    SecretRecord::MasquePrivateKey,
    SecretRecord::AccessToken,
    SecretRecord::DeviceId,
    SecretRecord::License,
    SecretRecord::EndpointPin,
    SecretRecord::AssignedIpv4,
    SecretRecord::AssignedIpv6,
    SecretRecord::IdentityMetadata,
];
const HEADER: &[u8] = b"UsqueIdentity\x01";
type Bytes = Zeroizing<Vec<u8>>;

#[derive(Default)]
struct Identity([Option<Bytes>; FIELDS.len()]);

impl Identity {
    fn decode(mut data: &[u8]) -> Result<Self, VaultError> {
        let invalid = || VaultError::Platform("invalid Keychain identity item".into());
        data = data.strip_prefix(HEADER).ok_or_else(invalid)?;
        let mut record = Self::default();
        for field in &mut record.0 {
            let (len, rest) = data.split_at_checked(2).ok_or_else(invalid)?;
            let len = u16::from_be_bytes([len[0], len[1]]) as usize;
            let (value, rest) = rest.split_at_checked(len).ok_or_else(invalid)?;
            if len > 0 {
                validate_size(value)?;
                *field = Some(Zeroizing::new(value.to_vec()));
            }
            data = rest;
        }
        if !data.is_empty() {
            return Err(invalid());
        }
        Ok(record)
    }

    fn encode(&self) -> Bytes {
        let len = HEADER.len()
            + FIELDS.len() * 2
            + self
                .0
                .iter()
                .flatten()
                .map(|value| value.len())
                .sum::<usize>();
        let mut bytes = Zeroizing::new(Vec::with_capacity(len));
        bytes.extend_from_slice(HEADER);
        for field in &self.0 {
            let value = field.as_ref().map(|v| v.as_slice()).unwrap_or_default();
            bytes.extend_from_slice(&(value.len() as u16).to_be_bytes());
            bytes.extend_from_slice(value);
        }
        bytes
    }
}

pub(super) struct AccountKeychain {
    raw: Box<dyn KeychainStore>,
    directory: Option<PathBuf>,
    cached: BTreeMap<Uuid, Identity>,
    approved_legacy_fields: BTreeMap<(Uuid, usize), Bytes>,
}

impl AccountKeychain {
    pub(super) fn new(raw: Box<dyn KeychainStore>, directory: Option<PathBuf>) -> Self {
        Self {
            raw,
            directory,
            cached: BTreeMap::new(),
            approved_legacy_fields: BTreeMap::new(),
        }
    }

    fn field(account: &str) -> Option<(Uuid, usize)> {
        let (id, name) = account.split_once('/')?;
        Some((
            id.parse().ok()?,
            FIELDS.iter().position(|f| f.key() == name)?,
        ))
    }

    // The file contains no secrets. Serialize read/modify/write and legacy
    // promotion across Engine processes that share the login keychain.
    fn mutation_lock(&self) -> Result<File, VaultError> {
        let dir = self
            .directory
            .as_ref()
            .ok_or_else(|| VaultError::Platform("user home unavailable".into()))?;
        let storage_error =
            |e: std::io::Error| VaultError::Platform(format!("Keychain identity lock: {e}"));
        fs::create_dir_all(dir).map_err(storage_error)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(dir.join("keychain-identity-v1.lock"))
            .map_err(storage_error)?;
        match file.try_lock() {
            Ok(()) => Ok(file),
            Err(std::fs::TryLockError::WouldBlock) => Err(VaultError::InteractionRequired),
            Err(std::fs::TryLockError::Error(e)) => Err(storage_error(e)),
        }
    }

    fn packed(&mut self, name: &str) -> Result<Option<Identity>, VaultError> {
        self.raw
            .get(name)?
            .map(|data| Identity::decode(&data))
            .transpose()
    }

    fn legacy(&mut self, id: Uuid) -> Result<Identity, VaultError> {
        let mut record = Identity::default();
        for (index, (slot, field)) in record.0.iter_mut().zip(FIELDS).enumerate() {
            if let Some(approved) = self.approved_legacy_fields.get(&(id, index)) {
                *slot = Some(approved.clone());
                continue;
            }
            if let Some(value) = self.raw.get(&account_name(id, field))? {
                validate_size(&value)?;
                if self.approved_legacy_fields.len() >= super::MAX_CACHED_RECORDS {
                    self.approved_legacy_fields.pop_first();
                }
                self.approved_legacy_fields
                    .insert((id, index), value.clone());
                *slot = Some(value);
            }
        }
        Ok(record)
    }

    fn persist(&mut self, name: &str, record: &Identity) -> Result<(), VaultError> {
        self.raw.put(name, &record.encode())
    }

    fn finish_migration(
        &mut self,
        id: Uuid,
        name: &str,
        legacy: &Identity,
    ) -> Result<(), VaultError> {
        // Never retire a legacy item until every field has been read and
        // the complete replacement has been read back from durable storage.
        let expected = legacy.encode();
        self.raw.put(name, &expected)?;
        if self.raw.get(name)?.as_deref() != Some(&*expected) {
            return Err(VaultError::Platform(
                "Keychain identity readback failed".into(),
            ));
        }
        // Old items may have distinct delete ACLs. Cleanup must not generate
        // a second round of prompts. Explicit account deletion covers both layouts.
        super::INTERACTIVE.sync_scope(false, || {
            for (field, value) in FIELDS.iter().zip(&legacy.0) {
                if value.is_some() {
                    let _ = self.raw.delete(&account_name(id, *field));
                }
            }
        });
        Ok(())
    }

    fn load(&mut self, id: Uuid, name: &str) -> Result<Identity, VaultError> {
        if let Some(record) = self.packed(name)? {
            return Ok(record);
        }
        let legacy = self.legacy(id)?;
        if legacy.0.iter().any(Option::is_some) {
            self.finish_migration(id, name, &legacy)?;
            self.approved_legacy_fields
                .retain(|(owner, _), _| *owner != id);
        }
        Ok(legacy)
    }
}

impl KeychainStore for AccountKeychain {
    fn delete_identity(&mut self, id: Uuid) -> Result<(), VaultError> {
        self.cached.remove(&id);
        self.approved_legacy_fields
            .retain(|(owner, _), _| *owner != id);
        let _lock = self.mutation_lock()?;
        // Keep the packed item authoritative while any legacy deletions are pending.
        self.raw.delete_identity(id)?;
        self.raw.delete(&format!("{id}/identity-v1"))
    }

    fn replace_identity(
        &mut self,
        id: Uuid,
        records: Vec<(SecretRecord, Bytes)>,
    ) -> Result<(), VaultError> {
        let mut identity = Identity::default();
        for (field, value) in records {
            let slot = FIELDS
                .iter()
                .position(|f| *f == field)
                .ok_or_else(|| VaultError::Platform("unexpected identity field".into()))?;
            identity.0[slot] = Some(value);
        }
        self.cached.remove(&id);
        self.approved_legacy_fields
            .retain(|(owner, _), _| *owner != id);
        let _lock = self.mutation_lock()?;
        // One Keychain write atomically replaces the complete account. A denied
        // update leaves the previous item intact.
        self.persist(&format!("{id}/identity-v1"), &identity)
    }

    fn get(&mut self, account: &str) -> Result<Option<Bytes>, VaultError> {
        let Some((id, index)) = Self::field(account) else {
            return self.raw.get(account);
        };
        if let Some(record) = self.cached.get(&id) {
            return Ok(record.0[index].clone());
        }
        let _lock = self.mutation_lock()?;
        let record = self.load(id, &format!("{id}/identity-v1"))?;
        let value = record.0[index].clone();
        if record.0.iter().any(Option::is_some) {
            if self.cached.len() >= super::MAX_CACHED_RECORDS / FIELDS.len() {
                self.cached.pop_first();
            }
            self.cached.insert(id, record);
        }
        Ok(value)
    }

    fn put(&mut self, account: &str, value: &[u8]) -> Result<(), VaultError> {
        let Some((id, index)) = Self::field(account) else {
            return self.raw.put(account, value);
        };
        self.cached.remove(&id);
        let _lock = self.mutation_lock()?;
        let name = format!("{id}/identity-v1");
        let mut record = self.load(id, &name)?;
        record.0[index] = Some(Zeroizing::new(value.to_vec()));
        self.persist(&name, &record)
    }

    fn delete(&mut self, account: &str) -> Result<(), VaultError> {
        let Some((id, index)) = Self::field(account) else {
            return self.raw.delete(account);
        };
        self.cached.remove(&id);
        let _lock = self.mutation_lock()?;
        let name = format!("{id}/identity-v1");
        let packed = self.packed(&name)?;
        self.approved_legacy_fields.remove(&(id, index));
        self.raw.delete(account)?;
        if let Some(mut record) = packed
            && record.0[index].take().is_some()
        {
            if record.0.iter().all(Option::is_none) {
                self.raw.delete(&name)?;
            } else {
                self.persist(&name, &record)?;
            }
        }
        Ok(())
    }
}
