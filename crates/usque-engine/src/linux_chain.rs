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
use usque_core::proton_vpn::store::{MAX_RECORD_CIPHERTEXT_BYTES, MAX_RECORD_PLAINTEXT_BYTES};
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
        // Shared encryption supports the largest record format. Each store
        // enforces its own format's limits before invoking the cipher.
        if value.len() > MAX_RECORD_PLAINTEXT_BYTES {
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
            || value.len() > MAX_RECORD_CIPHERTEXT_BYTES
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
        if length > MAX_RECORD_PLAINTEXT_BYTES {
            return Err(failure("record_too_large"));
        }
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
    use usque_core::{
        chain_exit::{ChainSource, Endpoint, ImportSecrets, store::ChainProfileStore},
        proton_vpn::{
            ApiIdentity, Node, Record, SelectedNode, SessionSecrets, store::SessionStore,
        },
    };
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

    #[test]
    fn complete_proton_catalogue_round_trips_beyond_chain_profile_capacity() {
        let parent = tempfile::tempdir().unwrap();
        let vault = Arc::new(TestVault {
            key: Mutex::new(None),
            locked: false,
        });
        let cipher = LinuxProfileCipher::new(vault.clone());
        let catalogue: Vec<_> = (0..2500)
            .map(|index| Node {
                id: format!("logical-{index}"),
                name: format!("Synthetic server {index}"),
                country: "IS".into(),
                city: "Synthetic city".into(),
                load: 50,
            })
            .collect();
        let record = Record {
            version: 2,
            id: Uuid::new_v4(),
            revision: Uuid::new_v4(),
            name: "Synthetic account".into(),
            identity: "login".into(),
            api_identity: ApiIdentity::default(),
            session: SessionSecrets {
                uid: "synthetic-uid".into(),
                access_token: "synthetic-access".into(),
                refresh_token: "synthetic-refresh".into(),
                user_id: "synthetic-user".into(),
            },
            ed25519_seed: Zeroizing::new(vec![7; 32]),
            certificate_chain: Zeroizing::new(String::new()),
            expires_at: 0,
            refresh_at: 0,
            max_tier: 2,
            node: SelectedNode {
                metadata: catalogue[0].clone(),
                domain: "agent.example.invalid".into(),
                endpoint: Endpoint {
                    host: "8.8.8.8".into(),
                    port: 443,
                },
                public_key: "CAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAg=".into(),
                label: None,
            },
            rotation_pending: false,
            server_id: Some("logical-2499".into()),
            server_catalog: catalogue,
            retired_sessions: Vec::new(),
            catalog_revision: Uuid::new_v4(),
            tunnel_seed: None,
            key_recovery_at: 0,
            connection_cache: None,
        };
        record.validate().unwrap();
        let plaintext = Zeroizing::new(serde_json::to_vec(&record).unwrap());
        assert!(plaintext.len() > usque_core::chain_exit::store::MAX_RECORD_PLAINTEXT_BYTES);
        let sessions = SessionStore::new(parent.path(), &cipher);
        sessions.save(&record).unwrap();
        let encrypted = std::fs::read(
            parent
                .path()
                .join("proton-vpn")
                .join(format!("{}.sealed", record.id)),
        )
        .unwrap();
        assert!(
            !encrypted
                .windows(plaintext.len())
                .any(|part| part == plaintext.as_slice())
        );
        let reopened = LinuxProfileCipher::new(vault);
        let restored = SessionStore::new(parent.path(), &reopened)
            .load(record.id)
            .unwrap();
        assert!(restored.server_catalog == record.server_catalog);
        assert_eq!(restored.server_id, record.server_id);
        assert_eq!(restored.session.uid, record.session.uid);
        assert_eq!(restored.session.access_token, record.session.access_token);
        assert_eq!(restored.session.refresh_token, record.session.refresh_token);
        assert_eq!(restored.session.user_id, record.session.user_id);
        assert_eq!(restored.ed25519_seed, record.ed25519_seed);
    }

    #[test]
    fn shared_cipher_preserves_independent_chain_profile_size_limits() {
        let parent = tempfile::tempdir().unwrap();
        let cipher = LinuxProfileCipher::new(Arc::new(TestVault {
            key: Mutex::new(None),
            locked: false,
        }));
        let store = ChainProfileStore::new(parent.path(), &cipher);
        let summary = store
            .import(
                ChainSource::WireguardCustom,
                "Synthetic profile",
                ImportSecrets::new("[Interface]\nPrivateKey = AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE=\nAddress = 10.8.0.2/32\n[Peer]\nPublicKey = AgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgI=\nEndpoint = 8.8.8.8:51820\nAllowedIPs = 0.0.0.0/0\n".into()),
            )
            .unwrap();
        let path = parent
            .path()
            .join("chain-profiles")
            .join(format!("{}.profile", summary.id));
        let encrypted = std::fs::read(&path).unwrap();
        let plaintext = cipher.open(summary.id, &encrypted).unwrap();
        for limit in [
            usque_core::chain_exit::store::MAX_RECORD_PLAINTEXT_BYTES,
            usque_core::chain_exit::store::MAX_RECORD_CIPHERTEXT_BYTES,
        ] {
            // Valid JSON whitespace keeps the original authenticated record
            // intact while independently exceeding each store-level bound.
            let mut oversized = plaintext.clone();
            oversized.resize(limit + 1, b' ');
            let envelope = cipher.seal(summary.id, &oversized).unwrap();
            assert_eq!(cipher.open(summary.id, &envelope).unwrap(), oversized);
            std::fs::write(&path, envelope).unwrap();
            assert_eq!(
                store.load(&summary.selection()).unwrap_err().reason,
                "secure_storage_failed"
            );
        }
    }

    #[test]
    fn shared_cipher_enforces_plaintext_and_ciphertext_upper_bounds() {
        let cipher = LinuxProfileCipher::new(Arc::new(TestVault {
            key: Mutex::new(Some(Zeroizing::new(vec![7; 32]))),
            locked: false,
        }));
        let id = Uuid::new_v4();
        let mut plaintext = Zeroizing::new(vec![42; MAX_RECORD_PLAINTEXT_BYTES]);
        let envelope = cipher.seal(id, &plaintext).unwrap();
        assert_eq!(cipher.open(id, &envelope).unwrap(), plaintext);
        plaintext.push(42);
        assert_eq!(
            cipher.seal(id, &plaintext).unwrap_err().reason,
            "record_too_large"
        );

        // Authenticate an oversized plaintext without the bounded seal method,
        // proving that open enforces its plaintext bound after authentication.
        let raw_cipher =
            aead::LessSafeKey::new(aead::UnboundKey::new(&aead::AES_256_GCM, &[7; 32]).unwrap());
        let nonce = [3; 12];
        raw_cipher
            .seal_in_place_append_tag(
                aead::Nonce::assume_unique_for_key(nonce),
                aead::Aad::from(id.as_bytes()),
                &mut *plaintext,
            )
            .unwrap();
        let mut oversized = MAGIC.to_vec();
        oversized.extend_from_slice(&nonce);
        oversized.extend_from_slice(&plaintext);
        assert_eq!(
            cipher.open(id, &oversized).unwrap_err().reason,
            "record_too_large"
        );

        let locked = LinuxProfileCipher::new(Arc::new(TestVault {
            key: Mutex::new(None),
            locked: true,
        }));
        oversized.resize(MAX_RECORD_CIPHERTEXT_BYTES + 1, 0);
        assert_eq!(
            locked.open(id, &oversized).unwrap_err().reason,
            "encrypted_record_invalid"
        );
        assert_eq!(
            locked
                .seal(id, &vec![0; MAX_RECORD_PLAINTEXT_BYTES + 1])
                .unwrap_err()
                .reason,
            "record_too_large"
        );
    }
}
