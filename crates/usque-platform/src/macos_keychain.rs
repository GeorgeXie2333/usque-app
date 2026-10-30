//! Current-user macOS Keychain storage with bounded, zeroizing Engine-session
//! caching. Successfully unlocked records do not need another Keychain read on
//! every connect/disconnect. Writes and deletes invalidate before native access.

use async_trait::async_trait;
use security_framework::passwords::{
    delete_generic_password, get_generic_password, set_generic_password,
};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::{SecretRecord, SecretVault, VaultError};

const MAX_SECRET_BYTES: usize = 5 * 512;
const KEYCHAIN_SERVICE: &str = "io.github.georgexie2333.usque.identity";
const ERR_SEC_ITEM_NOT_FOUND: i32 = -25_300;
const MAX_CACHED_RECORDS: usize = 256;

#[derive(Clone)]
pub struct MacOsKeychainVault {
    state: Arc<Mutex<State>>,
}
impl Default for MacOsKeychainVault {
    fn default() -> Self {
        Self::with_store(Box::new(SystemKeychain))
    }
}
impl std::fmt::Debug for MacOsKeychainVault {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MacOsKeychainVault")
            .finish_non_exhaustive()
    }
}
struct State {
    store: Box<dyn KeychainStore>,
    records: BTreeMap<String, Zeroizing<Vec<u8>>>,
}
impl State {
    fn remember(&mut self, account: String, value: Zeroizing<Vec<u8>>) {
        if self.records.len() >= MAX_CACHED_RECORDS {
            self.records.pop_first();
        }
        self.records.insert(account, value);
    }
}
trait KeychainStore: Send {
    fn put(&self, account: &str, value: &[u8]) -> Result<(), VaultError>;
    fn get(&self, account: &str) -> Result<Option<Zeroizing<Vec<u8>>>, VaultError>;
    fn delete(&self, account: &str) -> Result<(), VaultError>;
}
struct SystemKeychain;
impl KeychainStore for SystemKeychain {
    fn put(&self, account: &str, value: &[u8]) -> Result<(), VaultError> {
        set_generic_password(KEYCHAIN_SERVICE, account, value).map_err(platform_error)
    }
    fn get(&self, account: &str) -> Result<Option<Zeroizing<Vec<u8>>>, VaultError> {
        match get_generic_password(KEYCHAIN_SERVICE, account) {
            Ok(value) => Ok(Some(Zeroizing::new(value))),
            Err(error) if error.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(None),
            Err(error) => Err(platform_error(error)),
        }
    }
    fn delete(&self, account: &str) -> Result<(), VaultError> {
        match delete_generic_password(KEYCHAIN_SERVICE, account) {
            Ok(()) => Ok(()),
            Err(error) if error.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(()),
            Err(error) => Err(platform_error(error)),
        }
    }
}
impl MacOsKeychainVault {
    fn with_store(store: Box<dyn KeychainStore>) -> Self {
        Self {
            state: Arc::new(Mutex::new(State {
                store,
                records: BTreeMap::new(),
            })),
        }
    }
    async fn operation<T: Send + 'static>(
        &self,
        operation: impl FnOnce(&mut State) -> Result<T, VaultError> + Send + 'static,
    ) -> Result<T, VaultError> {
        let state = Arc::clone(&self.state);
        tokio::task::spawn_blocking(move || {
            // The lock belongs to the blocking worker. Cancelling the async
            // caller cannot let a later read overtake an unfinished write.
            let mut state = state
                .lock()
                .map_err(|_| VaultError::Platform("Keychain session is unavailable".into()))?;
            operation(&mut state)
        })
        .await
        .map_err(worker_error)?
    }
}

#[async_trait]
impl SecretVault for MacOsKeychainVault {
    async fn put(
        &self,
        profile_id: Uuid,
        record: SecretRecord,
        value: &[u8],
    ) -> Result<(), VaultError> {
        validate_size(value)?;
        let account = account_name(profile_id, record);
        let value = Zeroizing::new(value.to_vec());
        self.operation(move |state| {
            state.records.remove(&account);
            // A later get must perform the real Keychain readback before the
            // new value is cached (the encryption-key initializer verifies it).
            state.store.put(&account, &value)
        })
        .await
    }

    async fn get(
        &self,
        profile_id: Uuid,
        record: SecretRecord,
    ) -> Result<Option<Zeroizing<Vec<u8>>>, VaultError> {
        let account = account_name(profile_id, record);
        self.operation(move |state| {
            if let Some(value) = state.records.get(&account) {
                return Ok(Some(value.clone()));
            }
            let value = state.store.get(&account)?;
            if let Some(value) = &value {
                validate_size(value)?;
                state.remember(account, value.clone());
            }
            // Missing records and cancelled/failed reads are never cached.
            Ok(value)
        })
        .await
    }

    async fn delete(&self, profile_id: Uuid, record: SecretRecord) -> Result<(), VaultError> {
        let account = account_name(profile_id, record);
        self.operation(move |state| {
            state.records.remove(&account);
            state.store.delete(&account)
        })
        .await
    }
}

fn validate_size(value: &[u8]) -> Result<(), VaultError> {
    if value.is_empty() || value.len() > MAX_SECRET_BYTES {
        Err(VaultError::InvalidSecretSize)
    } else {
        Ok(())
    }
}

fn account_name(profile_id: Uuid, record: SecretRecord) -> String {
    format!("{profile_id}/{}", record.key())
}

fn platform_error(error: security_framework::base::Error) -> VaultError {
    VaultError::Platform(format!("Security.framework OSStatus {}", error.code()))
}

fn worker_error(error: tokio::task::JoinError) -> VaultError {
    VaultError::Platform(format!("Keychain worker failed: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct FakeState {
        records: BTreeMap<String, Zeroizing<Vec<u8>>>,
        reads: usize,
        writes: usize,
        deletes: usize,
        fail_read: bool,
        fail_write: bool,
        fail_delete: bool,
    }
    struct FakeKeychain(Arc<Mutex<FakeState>>);
    impl KeychainStore for FakeKeychain {
        fn put(&self, account: &str, value: &[u8]) -> Result<(), VaultError> {
            let mut state = self.0.lock().unwrap();
            state.writes += 1;
            if std::mem::take(&mut state.fail_write) {
                return Err(VaultError::Platform("synthetic write cancellation".into()));
            }
            state
                .records
                .insert(account.into(), Zeroizing::new(value.to_vec()));
            Ok(())
        }
        fn get(&self, account: &str) -> Result<Option<Zeroizing<Vec<u8>>>, VaultError> {
            let mut state = self.0.lock().unwrap();
            state.reads += 1;
            if std::mem::take(&mut state.fail_read) {
                return Err(VaultError::Platform("synthetic read cancellation".into()));
            }
            Ok(state.records.get(account).cloned())
        }
        fn delete(&self, account: &str) -> Result<(), VaultError> {
            let mut state = self.0.lock().unwrap();
            state.deletes += 1;
            if std::mem::take(&mut state.fail_delete) {
                return Err(VaultError::Platform("synthetic delete cancellation".into()));
            }
            state.records.remove(account);
            Ok(())
        }
    }
    fn fixture() -> (MacOsKeychainVault, Arc<Mutex<FakeState>>, Uuid) {
        let state = Arc::new(Mutex::new(FakeState::default()));
        let profile = Uuid::new_v4();
        state.lock().unwrap().records.insert(
            account_name(profile, SecretRecord::AccessToken),
            Zeroizing::new(b"synthetic-old-token".to_vec()),
        );
        let vault = MacOsKeychainVault::with_store(Box::new(FakeKeychain(state.clone())));
        (vault, state, profile)
    }

    #[tokio::test]
    async fn concurrent_reconnect_reads_share_one_unlock_and_return_independent_secrets() {
        let (vault, state, profile) = fixture();
        let mut readers = Vec::new();
        for _ in 0..16 {
            let vault = vault.clone();
            readers.push(tokio::spawn(async move {
                vault
                    .get(profile, SecretRecord::AccessToken)
                    .await
                    .unwrap()
                    .unwrap()
            }));
        }
        for reader in readers {
            let mut token = reader.await.unwrap();
            assert_eq!(&*token, b"synthetic-old-token");
            token.fill(0);
        }
        assert_eq!(state.lock().unwrap().reads, 1);
        assert!(!format!("{vault:?}").contains("synthetic-old-token"));
        assert!(!format!("{vault:?}").contains(&profile.to_string()));
    }

    #[tokio::test]
    async fn writes_require_readback_and_deletion_removes_the_unlocked_copy() {
        let (vault, state, profile) = fixture();
        vault.get(profile, SecretRecord::AccessToken).await.unwrap();
        vault
            .put(profile, SecretRecord::AccessToken, b"synthetic-new-token")
            .await
            .unwrap();
        assert!(vault.state.lock().unwrap().records.is_empty());
        assert_eq!(
            &*vault
                .get(profile, SecretRecord::AccessToken)
                .await
                .unwrap()
                .unwrap(),
            b"synthetic-new-token"
        );
        // Even unchanged explicit writes reach durable storage, then require
        // native readback. A cached read cannot pretend persistence succeeded.
        vault
            .put(profile, SecretRecord::AccessToken, b"synthetic-new-token")
            .await
            .unwrap();
        assert_eq!(state.lock().unwrap().writes, 2);
        assert_eq!(state.lock().unwrap().reads, 2);
        assert!(vault.state.lock().unwrap().records.is_empty());
        vault
            .delete(profile, SecretRecord::AccessToken)
            .await
            .unwrap();
        assert!(vault.state.lock().unwrap().records.is_empty());
        assert!(
            vault
                .get(profile, SecretRecord::AccessToken)
                .await
                .unwrap()
                .is_none()
        );
        // Missing records are not cached: a subsequent restore is observable.
        state.lock().unwrap().records.insert(
            account_name(profile, SecretRecord::AccessToken),
            Zeroizing::new(b"synthetic-restored-token".to_vec()),
        );
        assert_eq!(
            &*vault
                .get(profile, SecretRecord::AccessToken)
                .await
                .unwrap()
                .unwrap(),
            b"synthetic-restored-token"
        );
    }

    #[tokio::test]
    async fn cancelled_reads_retry_and_failed_mutations_cannot_serve_stale_cache() {
        let (vault, state, profile) = fixture();
        state.lock().unwrap().fail_read = true;
        assert!(vault.get(profile, SecretRecord::AccessToken).await.is_err());
        assert!(vault.state.lock().unwrap().records.is_empty());
        vault.get(profile, SecretRecord::AccessToken).await.unwrap();
        assert_eq!(state.lock().unwrap().reads, 2);
        state.lock().unwrap().fail_write = true;
        assert!(
            vault
                .put(profile, SecretRecord::AccessToken, b"synthetic-replacement")
                .await
                .is_err()
        );
        assert!(vault.state.lock().unwrap().records.is_empty());
        vault.get(profile, SecretRecord::AccessToken).await.unwrap();
        assert_eq!(state.lock().unwrap().reads, 3);
        state.lock().unwrap().fail_delete = true;
        assert!(
            vault
                .delete(profile, SecretRecord::AccessToken)
                .await
                .is_err()
        );
        assert!(vault.state.lock().unwrap().records.is_empty());
        vault.get(profile, SecretRecord::AccessToken).await.unwrap();
        assert_eq!(state.lock().unwrap().reads, 4);
    }

    #[tokio::test]
    async fn a_new_engine_session_has_no_unlocked_records() {
        let (vault, state, profile) = fixture();
        vault.get(profile, SecretRecord::AccessToken).await.unwrap();
        drop(vault);
        let reopened = MacOsKeychainVault::with_store(Box::new(FakeKeychain(state.clone())));
        assert!(reopened.state.lock().unwrap().records.is_empty());
        reopened
            .get(profile, SecretRecord::AccessToken)
            .await
            .unwrap();
        assert_eq!(state.lock().unwrap().reads, 2);
    }

    #[tokio::test]
    async fn invalid_native_values_are_not_cached_and_cache_size_is_bounded() {
        let (vault, state, profile) = fixture();
        for bytes in [Vec::new(), vec![0; MAX_SECRET_BYTES + 1]] {
            state.lock().unwrap().records.insert(
                account_name(profile, SecretRecord::AccessToken),
                Zeroizing::new(bytes),
            );
            assert!(matches!(
                vault.get(profile, SecretRecord::AccessToken).await,
                Err(VaultError::InvalidSecretSize)
            ));
            assert!(vault.state.lock().unwrap().records.is_empty());
        }
        for _ in 0..MAX_CACHED_RECORDS + 1 {
            let profile = Uuid::new_v4();
            state.lock().unwrap().records.insert(
                account_name(profile, SecretRecord::AccessToken),
                Zeroizing::new(b"synthetic-token".to_vec()),
            );
            vault.get(profile, SecretRecord::AccessToken).await.unwrap();
        }
        assert_eq!(
            vault.state.lock().unwrap().records.len(),
            MAX_CACHED_RECORDS
        );
    }

    #[test]
    fn keychain_namespace_is_stable_and_has_no_secret_material() {
        let profile = Uuid::parse_str("8c30b771-9ebd-457a-b67b-bbc74a1ddba6").unwrap();
        assert_eq!(
            account_name(profile, SecretRecord::EndpointPin),
            "8c30b771-9ebd-457a-b67b-bbc74a1ddba6/endpoint-pin"
        );
        assert_eq!(KEYCHAIN_SERVICE, "io.github.georgexie2333.usque.identity");
    }

    #[tokio::test]
    async fn invalid_sizes_are_rejected_before_keychain_access() {
        let (vault, state, _profile) = fixture();
        assert!(matches!(
            vault
                .put(Uuid::new_v4(), SecretRecord::AccessToken, &[])
                .await,
            Err(VaultError::InvalidSecretSize)
        ));
        assert_eq!(state.lock().unwrap().writes, 0);
        assert!(matches!(
            vault
                .put(
                    Uuid::new_v4(),
                    SecretRecord::AccessToken,
                    &vec![0; MAX_SECRET_BYTES + 1],
                )
                .await,
            Err(VaultError::InvalidSecretSize)
        ));
    }
}
