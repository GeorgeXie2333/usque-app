//! Authenticated chain profile encryption with a per-user Secret Service key.
//!
//! Only encrypted records are written to disk. Unlock failures never generate
//! a replacement for an existing key. A loaded key remains in zeroizing memory
//! for this Engine lifetime, like the active account and transport credentials.

use ring::{
    aead,
    rand::{SecureRandom, SystemRandom},
};
use std::sync::{Arc, Mutex};
use usque_core::chain_exit::{ImportError, store::ProfileCipher};
use usque_platform::{SecretRecord, SecretVault};
use uuid::Uuid;
use zeroize::Zeroizing;

const MAGIC: &[u8] = b"USQUE-LINUX-CHAIN\0\x01";
pub(crate) const KEY_ID: Uuid = Uuid::from_bytes([
    0x2a, 0x31, 0x17, 0x93, 0x15, 0x52, 0x48, 0x51, 0x90, 0x42, 0xa0, 0x2b, 0x3e, 0x1c, 0x6f, 0x8d,
]);

pub(crate) struct LinuxProfileCipher {
    vault: Arc<dyn SecretVault>,
    key: Mutex<Option<Zeroizing<Vec<u8>>>>,
}

impl LinuxProfileCipher {
    pub(crate) fn new(vault: Arc<dyn SecretVault>) -> Self {
        Self {
            vault,
            key: Mutex::new(None),
        }
    }

    fn key(&self, create: bool) -> Result<Zeroizing<Vec<u8>>, ImportError> {
        let mut cached = self
            .key
            .lock()
            .map_err(|_| failure("keyring_unavailable"))?;
        if let Some(key) = cached.as_ref() {
            return Ok(key.clone());
        }
        let vault = Arc::clone(&self.vault);
        // ProfileCipher is synchronous and used by the existing atomic storage
        // transaction. A dedicated thread owns the async keyring runtime so it
        // never nests block_on inside the Engine's Tokio runtime.
        let key = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|_| failure("keyring_unavailable"))?;
            let result = runtime.block_on(async move {
                if let Some(key) = vault
                    .get(KEY_ID, SecretRecord::ChainEncryptionKey)
                    .await
                    .map_err(|_| failure("keyring_locked_or_unavailable"))?
                {
                    if key.len() != 32 {
                        return Err(failure("keyring_key_invalid"));
                    }
                    return Ok(key);
                }
                if !create {
                    return Err(failure("keyring_key_missing"));
                }
                let mut key = Zeroizing::new(vec![0; 32]);
                SystemRandom::new()
                    .fill(&mut key)
                    .map_err(|_| failure("random_unavailable"))?;
                vault
                    .put(KEY_ID, SecretRecord::ChainEncryptionKey, &key)
                    .await
                    .map_err(|_| failure("keyring_locked_or_unavailable"))?;
                let stored = vault
                    .get(KEY_ID, SecretRecord::ChainEncryptionKey)
                    .await
                    .map_err(|_| failure("keyring_locked_or_unavailable"))?
                    .ok_or_else(|| failure("keyring_key_missing"))?;
                if stored.as_slice() != key.as_slice() {
                    return Err(failure("keyring_key_changed"));
                }
                Ok(key)
            });
            runtime.shutdown_timeout(std::time::Duration::from_secs(1));
            result
        })
        .join()
        .map_err(|_| failure("keyring_worker_failed"))??;
        *cached = Some(key.clone());
        Ok(key)
    }
}

impl ProfileCipher for LinuxProfileCipher {
    fn reset_session_key(&self) {
        if let Ok(mut key) = self.key.lock() {
            *key = None;
        }
    }

    fn seal(&self, id: Uuid, value: &[u8]) -> Result<Vec<u8>, ImportError> {
        if value.len() > usque_core::chain_exit::store::MAX_RECORD_PLAINTEXT_BYTES {
            return Err(failure("record_too_large"));
        }
        let key = self.key(true)?;
        let cipher = aead::LessSafeKey::new(
            aead::UnboundKey::new(&aead::AES_256_GCM, &key)
                .map_err(|_| failure("keyring_key_invalid"))?,
        );
        let mut nonce = [0; 12];
        SystemRandom::new()
            .fill(&mut nonce)
            .map_err(|_| failure("random_unavailable"))?;
        let mut ciphertext = Zeroizing::new(value.to_vec());
        cipher
            .seal_in_place_append_tag(
                aead::Nonce::assume_unique_for_key(nonce),
                aead::Aad::from(id.as_bytes()),
                &mut *ciphertext,
            )
            .map_err(|_| failure("encryption_failed"))?;
        let mut envelope = Vec::with_capacity(MAGIC.len() + nonce.len() + ciphertext.len());
        envelope.extend_from_slice(MAGIC);
        envelope.extend_from_slice(&nonce);
        envelope.append(&mut ciphertext);
        Ok(envelope)
    }

    fn open(&self, id: Uuid, value: &[u8]) -> Result<Zeroizing<Vec<u8>>, ImportError> {
        let header = MAGIC.len() + 12;
        if !value.starts_with(MAGIC)
            || value.len() < header + aead::AES_256_GCM.tag_len()
            || value.len() > usque_core::chain_exit::store::MAX_RECORD_CIPHERTEXT_BYTES
        {
            return Err(failure("encrypted_record_invalid"));
        }
        let key = self.key(false)?;
        let cipher = aead::LessSafeKey::new(
            aead::UnboundKey::new(&aead::AES_256_GCM, &key)
                .map_err(|_| failure("keyring_key_invalid"))?,
        );
        let nonce: [u8; 12] = value[MAGIC.len()..header]
            .try_into()
            .map_err(|_| failure("encrypted_record_invalid"))?;
        let mut plaintext = Zeroizing::new(value[header..].to_vec());
        let length = cipher
            .open_in_place(
                aead::Nonce::assume_unique_for_key(nonce),
                aead::Aad::from(id.as_bytes()),
                &mut plaintext,
            )
            .map_err(|_| failure("encrypted_record_invalid"))?
            .len();
        plaintext.truncate(length);
        Ok(plaintext)
    }
}

fn failure(reason: &'static str) -> ImportError {
    ImportError::new(0, "secret_service", reason)
}

#[cfg(test)]
mod tests {
    use super::*;
    use usque_platform::VaultError;
    struct TestVault {
        key: Mutex<Option<Zeroizing<Vec<u8>>>>,
        locked: bool,
    }
    #[async_trait::async_trait]
    impl SecretVault for TestVault {
        async fn put(&self, _: Uuid, _: SecretRecord, value: &[u8]) -> Result<(), VaultError> {
            assert!(!self.locked, "a locked keyring must never be overwritten");
            *self.key.lock().unwrap() = Some(Zeroizing::new(value.to_vec()));
            Ok(())
        }
        async fn get(
            &self,
            _: Uuid,
            _: SecretRecord,
        ) -> Result<Option<Zeroizing<Vec<u8>>>, VaultError> {
            if self.locked {
                return Err(VaultError::Unavailable("locked".into()));
            }
            Ok(self.key.lock().unwrap().clone())
        }
        async fn delete(&self, _: Uuid, _: SecretRecord) -> Result<(), VaultError> {
            unreachable!()
        }
    }
    #[test]
    fn encrypted_records_bind_identity_key_nonce_and_ciphertext() {
        let vault = Arc::new(TestVault {
            key: Mutex::new(None),
            locked: false,
        });
        let cipher = LinuxProfileCipher::new(vault.clone());
        let id = Uuid::new_v4();
        let first = cipher.seal(id, b"private\0binary\xffvalue").unwrap();
        let second = cipher.seal(id, b"private\0binary\xffvalue").unwrap();
        assert_ne!(first, second);
        assert_eq!(
            cipher.open(id, &first).unwrap().as_slice(),
            b"private\0binary\xffvalue"
        );
        assert!(cipher.open(Uuid::new_v4(), &first).is_err());
        let reopened = LinuxProfileCipher::new(vault);
        assert!(reopened.open(id, &first).is_ok());
        let mut corrupted = first.clone();
        *corrupted.last_mut().unwrap() ^= 1;
        assert!(cipher.open(id, &corrupted).is_err());
        for length in 0..MAGIC.len() + 28 {
            assert!(cipher.open(id, &first[..length.min(first.len())]).is_err());
        }
    }
    #[test]
    fn locked_keyring_never_falls_back_or_replaces_its_key() {
        let cipher = LinuxProfileCipher::new(Arc::new(TestVault {
            key: Mutex::new(Some(Zeroizing::new(vec![7; 32]))),
            locked: true,
        }));
        assert!(cipher.seal(Uuid::nil(), b"secret").is_err());
    }
}
