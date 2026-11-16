//! Current-user macOS Keychain storage with bounded, zeroizing Engine-session
//! caching. Successfully unlocked records do not need another Keychain read on
//! every connect/disconnect. Writes and deletes invalidate before native access.

mod account;

use async_trait::async_trait;
use security_framework::os::macos::keychain::SecKeychain;
use security_framework::passwords::{
    delete_generic_password, get_generic_password, set_generic_password,
};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, TryLockError},
};
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::{SecretRecord, SecretVault, VaultError};

const MAX_SECRET_BYTES: usize = 5 * 512;
const KEYCHAIN_SERVICE: &str = "io.github.georgexie2333.usque.identity";
const ERR_SEC_ITEM_NOT_FOUND: i32 = -25_300;
const MAX_CACHED_RECORDS: usize = 256;

tokio::task_local! {
    /// Propagate the request's intent across awaits, including synchronous
    /// storage workers that explicitly inherit it.
    pub static INTERACTIVE: bool;
}

pub fn interaction_allowed() -> bool {
    INTERACTIVE.try_with(|allowed| *allowed).unwrap_or(true)
}

pub async fn without_interaction<F: Future>(future: F) -> F::Output {
    INTERACTIVE.scope(false, future).await
}

#[derive(Clone)]
pub struct MacOsKeychainVault {
    state: Arc<Mutex<State>>,
}
impl Default for MacOsKeychainVault {
    fn default() -> Self {
        let directory =
            std::env::home_dir().map(|home| home.join("Library/Application Support/Usque"));
        Self::with_store(Box::new(account::AccountKeychain::new(
            Box::new(SystemKeychain),
            directory,
        )))
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
    fn put(&mut self, account: &str, value: &[u8]) -> Result<(), VaultError>;
    fn get(&mut self, account: &str) -> Result<Option<Zeroizing<Vec<u8>>>, VaultError>;
    fn delete(&mut self, account: &str) -> Result<(), VaultError>;

    fn delete_identity(&mut self, id: Uuid) -> Result<(), VaultError> {
        for field in account::FIELDS {
            self.delete(&account_name(id, field))?;
        }
        Ok(())
    }

    fn replace_identity(
        &mut self,
        id: Uuid,
        records: Vec<(SecretRecord, Zeroizing<Vec<u8>>)>,
    ) -> Result<(), VaultError> {
        for field in account::FIELDS {
            let name = account_name(id, field);
            if let Some((_, value)) = records.iter().find(|(record, _)| *record == field) {
                self.put(&name, value)?;
            } else {
                self.delete(&name)?;
            }
        }
        Ok(())
    }
}
// SecKeychainSetUserInteractionAllowed changes process-wide state. Hold this
// lock across each native call and restore the previous setting before release.
fn native_operation<T>(operation: impl FnOnce() -> Result<T, VaultError>) -> Result<T, VaultError> {
    static NATIVE: Mutex<()> = Mutex::new(());
    let interactive = interaction_allowed();
    let unavailable = || VaultError::Platform("Keychain UI policy is unavailable".into());
    let _lock = if interactive {
        NATIVE.lock().map_err(|_| unavailable())?
    } else {
        NATIVE.try_lock().map_err(|error| match error {
            TryLockError::WouldBlock => VaultError::InteractionRequired,
            TryLockError::Poisoned(_) => unavailable(),
        })?
    };
    let _guard =
        if !interactive && SecKeychain::user_interaction_allowed().map_err(platform_error)? {
            Some(SecKeychain::disable_user_interaction().map_err(platform_error)?)
        } else {
            None
        };
    operation()
}
struct SystemKeychain;
impl KeychainStore for SystemKeychain {
    fn put(&mut self, account: &str, value: &[u8]) -> Result<(), VaultError> {
        native_operation(|| {
            set_generic_password(KEYCHAIN_SERVICE, account, value).map_err(platform_error)
        })
    }
    fn get(&mut self, account: &str) -> Result<Option<Zeroizing<Vec<u8>>>, VaultError> {
        native_operation(|| match get_generic_password(KEYCHAIN_SERVICE, account) {
            Ok(value) => Ok(Some(Zeroizing::new(value))),
            Err(error) if error.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(None),
            Err(error) => Err(platform_error(error)),
        })
    }
    fn delete(&mut self, account: &str) -> Result<(), VaultError> {
        native_operation(
            || match delete_generic_password(KEYCHAIN_SERVICE, account) {
                Ok(()) => Ok(()),
                Err(error) if error.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(()),
                Err(error) => Err(platform_error(error)),
            },
        )
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
        let interactive = interaction_allowed();
        tokio::task::spawn_blocking(move || {
            // The lock belongs to the blocking worker. Cancelling the async
            // caller cannot let a later read overtake an unfinished write.
            let unavailable = || VaultError::Platform("Keychain session is unavailable".into());
            let mut state = if interactive {
                state.lock().map_err(|_| unavailable())?
            } else {
                state.try_lock().map_err(|error| match error {
                    TryLockError::WouldBlock => VaultError::InteractionRequired,
                    TryLockError::Poisoned(_) => unavailable(),
                })?
            };
            INTERACTIVE.sync_scope(interactive, || operation(&mut state))
        })
        .await
        .map_err(worker_error)?
    }
}

#[async_trait]
impl SecretVault for MacOsKeychainVault {
    async fn delete_identity(&self, id: Uuid) -> Result<(), VaultError> {
        self.operation(move |state| {
            for field in account::FIELDS {
                state.records.remove(&account_name(id, field));
            }
            state.store.delete_identity(id)
        })
        .await
    }

    async fn replace_identity(
        &self,
        id: Uuid,
        records: Vec<(SecretRecord, Zeroizing<Vec<u8>>)>,
    ) -> Result<(), VaultError> {
        for (_, value) in &records {
            validate_size(value)?;
        }
        self.operation(move |state| {
            for field in account::FIELDS {
                state.records.remove(&account_name(id, field));
            }
            state.store.replace_identity(id, records)
        })
        .await
    }
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
    match error.code() {
        // userCanceledErr, errSecAuthFailed, errSecInteractionNotAllowed, errSecInteractionRequired.
        -128 | -25293 | -25308 | -25315 => VaultError::InteractionRequired,
        code => VaultError::Platform(format!("Security.framework OSStatus {code}")),
    }
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
        needs_permission: bool,
        prompts: usize,
        denied_account: Option<String>,
    }
    struct FakeKeychain(Arc<Mutex<FakeState>>);
    impl KeychainStore for FakeKeychain {
        fn put(&mut self, account: &str, value: &[u8]) -> Result<(), VaultError> {
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
        fn get(&mut self, account: &str) -> Result<Option<Zeroizing<Vec<u8>>>, VaultError> {
            let mut state = self.0.lock().unwrap();
            state.reads += 1;
            if state.denied_account.as_deref() == Some(account) {
                return Err(VaultError::InteractionRequired);
            }
            if state.needs_permission && state.records.contains_key(account) {
                if !interaction_allowed() {
                    return Err(VaultError::InteractionRequired);
                }
                state.prompts += 1;
            }
            if std::mem::take(&mut state.fail_read) {
                return Err(VaultError::Platform("synthetic read cancellation".into()));
            }
            Ok(state.records.get(account).cloned())
        }
        fn delete(&mut self, account: &str) -> Result<(), VaultError> {
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
    async fn background_reads_never_prompt_and_manual_unlock_warms_the_cache() {
        let (vault, state, profile) = fixture();
        state.lock().unwrap().needs_permission = true;
        for _ in 0..3 {
            assert!(matches!(
                without_interaction(vault.get(profile, SecretRecord::AccessToken)).await,
                Err(VaultError::InteractionRequired)
            ));
        }
        assert_eq!(state.lock().unwrap().prompts, 0);
        vault.get(profile, SecretRecord::AccessToken).await.unwrap();
        without_interaction(vault.get(profile, SecretRecord::AccessToken))
            .await
            .unwrap();
        assert_eq!(state.lock().unwrap().prompts, 1);
    }

    #[tokio::test]
    async fn one_authorization_opens_all_fields_after_an_engine_update() {
        let raw = Arc::new(Mutex::new(FakeState::default()));
        let dir = tempfile::tempdir().unwrap();
        let open = || {
            MacOsKeychainVault::with_store(Box::new(account::AccountKeychain::new(
                Box::new(FakeKeychain(raw.clone())),
                Some(dir.path().into()),
            )))
        };
        let id = Uuid::new_v4();
        let fields = [
            SecretRecord::MasquePrivateKey,
            SecretRecord::AccessToken,
            SecretRecord::EndpointPin,
        ];
        let first = open();
        for field in fields {
            first.put(id, field, field.key().as_bytes()).await.unwrap();
        }
        drop(first);
        assert_eq!(raw.lock().unwrap().records.len(), 1);
        raw.lock().unwrap().needs_permission = true;
        let updated = open();
        for field in fields {
            assert_eq!(
                updated.get(id, field).await.unwrap().unwrap().as_slice(),
                field.key().as_bytes()
            );
        }
        assert_eq!(raw.lock().unwrap().prompts, 1);
    }

    #[tokio::test]
    async fn denied_legacy_migration_keeps_old_items_and_resumes_after_approved_fields() {
        let raw = Arc::new(Mutex::new(FakeState::default()));
        let dir = tempfile::tempdir().unwrap();
        let id = Uuid::new_v4();
        let key = account_name(id, SecretRecord::MasquePrivateKey);
        let token = account_name(id, SecretRecord::AccessToken);
        {
            let mut state = raw.lock().unwrap();
            state.records.insert(key.clone(), Zeroizing::new(vec![1]));
            state.records.insert(token.clone(), Zeroizing::new(vec![2]));
            state.needs_permission = true;
            state.denied_account = Some(token.clone());
        }
        let vault = MacOsKeychainVault::with_store(Box::new(account::AccountKeychain::new(
            Box::new(FakeKeychain(raw.clone())),
            Some(dir.path().into()),
        )));
        let read = || vault.get(id, SecretRecord::MasquePrivateKey);
        assert!(matches!(
            without_interaction(read()).await,
            Err(VaultError::InteractionRequired)
        ));
        assert_eq!(raw.lock().unwrap().prompts, 0);
        assert!(matches!(read().await, Err(VaultError::InteractionRequired)));
        {
            let mut state = raw.lock().unwrap();
            assert_eq!(state.prompts, 1);
            assert_eq!(state.writes, 0);
            assert_eq!(state.deletes, 0);
            state.denied_account = None;
        }
        assert_eq!(read().await.unwrap().unwrap().as_slice(), &[1]);
        let state = raw.lock().unwrap();
        assert_eq!(
            state.prompts, 3,
            "only the remaining token and synthetic readback need access"
        );
        assert_eq!(state.records.len(), 1);
        assert!(state.records.contains_key(&format!("{id}/identity-v1")));
    }

    #[tokio::test]
    async fn quiet_native_operation_restores_the_system_ui_policy_without_opening_a_keychain() {
        let allowed = || SecKeychain::user_interaction_allowed().map_err(platform_error);
        let original = native_operation(allowed).unwrap();
        assert!(
            !without_interaction(async { native_operation(allowed) })
                .await
                .unwrap()
        );
        assert_eq!(native_operation(allowed).unwrap(), original);
    }

    #[tokio::test]
    async fn switching_packed_accounts_keeps_absent_optional_fields_unlocked() {
        let raw = Arc::new(Mutex::new(FakeState::default()));
        let dir = tempfile::tempdir().unwrap();
        let open = || {
            MacOsKeychainVault::with_store(Box::new(account::AccountKeychain::new(
                Box::new(FakeKeychain(raw.clone())),
                Some(dir.path().into()),
            )))
        };
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let first = open();
        for id in [a, b] {
            first
                .replace_identity(
                    id,
                    vec![(SecretRecord::AccessToken, Zeroizing::new(vec![7]))],
                )
                .await
                .unwrap();
        }
        let updated = open();
        raw.lock().unwrap().needs_permission = true;
        for id in [a, b, a, b] {
            assert!(
                updated
                    .get(id, SecretRecord::License)
                    .await
                    .unwrap()
                    .is_none()
            );
        }
        assert_eq!(raw.lock().unwrap().prompts, 2);
    }

    #[tokio::test]
    async fn failed_packed_replacement_preserves_old_bytes_and_bulk_delete_stops_on_denial() {
        let raw = Arc::new(Mutex::new(FakeState::default()));
        let dir = tempfile::tempdir().unwrap();
        let vault = MacOsKeychainVault::with_store(Box::new(account::AccountKeychain::new(
            Box::new(FakeKeychain(raw.clone())),
            Some(dir.path().into()),
        )));
        let id = Uuid::new_v4();
        let fields = |n| vec![(SecretRecord::AccessToken, Zeroizing::new(vec![n]))];
        vault.replace_identity(id, fields(1)).await.unwrap();
        raw.lock().unwrap().fail_write = true;
        assert!(vault.replace_identity(id, fields(2)).await.is_err());
        assert_eq!(
            vault
                .get(id, SecretRecord::AccessToken)
                .await
                .unwrap()
                .unwrap()
                .as_slice(),
            &[1]
        );
        raw.lock().unwrap().fail_delete = true;
        assert!(vault.delete_identity(id).await.is_err());
        assert_eq!(raw.lock().unwrap().deletes, 1);
        vault.delete_identity(id).await.unwrap();
        assert!(raw.lock().unwrap().records.is_empty());
    }

    #[tokio::test]
    async fn background_read_does_not_queue_behind_an_open_dialog() {
        let (vault, state, profile) = fixture();
        let shared = Arc::clone(&vault.state);
        let (started, ready) = tokio::sync::oneshot::channel();
        let (release, until_released) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            let _held = shared.lock().unwrap();
            started.send(()).unwrap();
            until_released.recv().unwrap();
        });
        ready.await.unwrap();
        let read = without_interaction(vault.get(profile, SecretRecord::AccessToken));
        let result = tokio::time::timeout(std::time::Duration::from_secs(1), read).await;
        release.send(()).unwrap();
        worker.join().unwrap();
        assert!(matches!(result, Ok(Err(VaultError::InteractionRequired))));
        assert_eq!(state.lock().unwrap().reads, 0);
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
