//! Root-only write-ahead file transactions. Recovery never executes a journal
//! path and never overwrites bytes which differ from both recorded versions.

use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Component, Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;
use usque_core::linux_release::{MAX_PACKAGE_BYTES, SERVICE_FILES};

use super::{Error, RECORD, STAGING, open_regular};

const JOURNAL: &str = "service-transaction.json";
const MAX_JOURNAL: u64 = 64 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Identity {
    size: u64,
    sha256: String,
    mode: u32,
}

impl Identity {
    fn validate(&self) -> bool {
        self.size <= MAX_PACKAGE_BYTES
            && self.mode & !0o777 == 0
            && self.mode & 0o022 == 0
            && self.sha256.len() == 64
            && self
                .sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    }

    fn same_bytes(&self, other: &Self) -> bool {
        self.size == other.size && self.sha256 == other.sha256
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Change {
    name: String,
    prior: Option<Identity>,
    candidate: Option<Identity>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    schema_version: u32,
    stage: String,
    replacing: bool,
    previous_active: bool,
    previous_enabled: bool,
    start_attempted: bool,
    enablement_attempted: bool,
    candidate_stopped: bool,
    enablement_reverted: bool,
    complete: bool,
    #[serde(default)]
    rolled_back: bool,
    changes: Vec<Change>,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Recovery {
    NoTransaction,
    Committed,
    RolledBack,
}

/// Service state is changed only through fixed operations supplied by the
/// maintenance executable. Deterministic tests substitute an inert recorder.
pub(super) trait Lifecycle {
    fn stop_candidate(&mut self) -> Result<(), Error>;
    fn reload(&mut self) -> Result<(), Error>;
    fn enable(&mut self, enabled: bool) -> Result<(), Error>;
    fn start_previous(&mut self) -> Result<(), Error>;
}

pub(super) struct Store {
    root: PathBuf,
    owner: u32,
}

pub(super) struct Transaction {
    store: Store,
    journal: Journal,
}

impl Store {
    pub(super) fn production() -> Self {
        Self {
            root: PathBuf::from("/"),
            owner: 0,
        }
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.root.join(relative.trim_start_matches('/'))
    }

    fn base(&self) -> PathBuf {
        self.path(STAGING)
    }

    fn secure_directory(&self, path: &Path, create: bool) -> Result<(), Error> {
        let relative = path
            .strip_prefix(&self.root)
            .map_err(|_| Error::Installation)?;
        let mut current = self.root.clone();
        for component in std::iter::once(None).chain(relative.components().map(Some)) {
            if let Some(component) = component {
                if let Component::Normal(value) = component {
                    current.push(value);
                } else {
                    return Err(Error::Installation);
                }
            }
            let private = current == self.path("/var/lib/usque") || current == self.base();
            match fs::symlink_metadata(&current) {
                Ok(metadata)
                    if metadata.is_dir()
                        && metadata.uid() == self.owner
                        && metadata.mode() & 0o022 == 0 =>
                {
                    if private && metadata.mode() & 0o777 != 0o700 {
                        if !create {
                            return Err(Error::Installation);
                        }
                        fs::set_permissions(&current, fs::Permissions::from_mode(0o700))?;
                        File::open(&current)?.sync_all()?;
                    }
                }
                Err(error) if create && error.kind() == io::ErrorKind::NotFound => {
                    fs::create_dir(&current)?;
                    fs::set_permissions(
                        &current,
                        fs::Permissions::from_mode(if private { 0o700 } else { 0o755 }),
                    )?;
                    File::open(current.parent().ok_or(Error::Installation)?)?.sync_all()?;
                }
                _ => return Err(Error::Installation),
            }
        }
        Ok(())
    }

    fn trusted_file(&self, path: &Path) -> Result<File, Error> {
        self.secure_directory(path.parent().ok_or(Error::Installation)?, false)?;
        let file = open_regular(path)?;
        let metadata = file.metadata()?;
        if metadata.uid() != self.owner || metadata.mode() & 0o022 != 0 {
            return Err(Error::Installation);
        }
        Ok(file)
    }

    fn identity(&self, path: &Path) -> Result<Identity, Error> {
        let mut file = self.trusted_file(path)?;
        let metadata = file.metadata()?;
        if metadata.len() > MAX_PACKAGE_BYTES {
            return Err(Error::Installation);
        }
        let mut digest = Sha256::new();
        let mut size = 0;
        let mut buffer = [0; 64 * 1024];
        loop {
            let count = file.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            size += count as u64;
            if size > MAX_PACKAGE_BYTES {
                return Err(Error::Installation);
            }
            digest.update(&buffer[..count]);
        }
        if size != metadata.len() {
            return Err(Error::Installation);
        }
        Ok(Identity {
            size,
            sha256: digest
                .finalize()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
            mode: metadata.mode() & 0o777,
        })
    }

    fn existing_identity(&self, path: &Path) -> Result<Option<Identity>, Error> {
        match fs::symlink_metadata(path) {
            Ok(_) => self.identity(path).map(Some),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    fn restored_parent(&self, path: &Path) -> Result<PathBuf, Error> {
        let mut parent = path.parent().ok_or(Error::Installation)?;
        loop {
            match fs::symlink_metadata(parent) {
                Ok(_) => {
                    self.secure_directory(parent, false)?;
                    return Ok(parent.to_owned());
                }
                // An absent original can also have absent parent directories.
                // Persist their absence at the nearest existing trusted ancestor
                // instead of creating directories just to finish recovery.
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    parent = parent.parent().ok_or(Error::Installation)?;
                }
                Err(error) => return Err(error.into()),
            }
        }
    }

    fn save(&self, journal: &Journal) -> Result<(), Error> {
        self.secure_directory(&self.base(), true)?;
        let bytes = serde_json::to_vec(journal).map_err(|_| Error::Installation)?;
        if bytes.len() > MAX_JOURNAL as usize {
            return Err(Error::Installation);
        }
        let mut next = NamedTempFile::new_in(self.base())?;
        next.as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))?;
        next.write_all(&bytes)?;
        next.as_file().sync_all()?;
        next.persist(self.base().join(JOURNAL))
            .map_err(|error| Error::Io(error.error))?;
        File::open(self.base())?.sync_all()?;
        Ok(())
    }

    fn load(&self) -> Result<Option<Journal>, Error> {
        let path = self.base().join(JOURNAL);
        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
            Ok(_) => {}
        }
        let file = self.trusted_file(&path)?;
        if file.metadata()?.mode() & 0o777 != 0o600 {
            return Err(Error::RecoveryRequired);
        }
        let mut bytes = Vec::new();
        file.take(MAX_JOURNAL + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_JOURNAL {
            return Err(Error::RecoveryRequired);
        }
        let journal: Journal =
            serde_json::from_slice(&bytes).map_err(|_| Error::RecoveryRequired)?;
        let mut names = BTreeSet::new();
        if journal.schema_version != 1
            || !safe_stage_name(&journal.stage)
            || journal.changes.len() > SERVICE_FILES.len() + 1
            || journal.changes.iter().any(|change| {
                !allowed(&change.name)
                    || !names.insert(&change.name)
                    || change
                        .prior
                        .iter()
                        .chain(change.candidate.iter())
                        .any(|value| !value.validate())
            })
        {
            return Err(Error::RecoveryRequired);
        }
        Ok(Some(journal))
    }

    fn atomic_copy(
        &self,
        source: &Path,
        destination: &Path,
        expected: &Identity,
    ) -> Result<(), Error> {
        self.secure_directory(destination.parent().ok_or(Error::Installation)?, true)?;
        let source = self.trusted_file(source)?;
        let mut next = NamedTempFile::new_in(destination.parent().ok_or(Error::Installation)?)?;
        io::copy(&mut source.take(expected.size + 1), next.as_file_mut())?;
        if !self.identity(next.path())?.same_bytes(expected) {
            return Err(Error::RecoveryRequired);
        }
        next.as_file()
            .set_permissions(fs::Permissions::from_mode(expected.mode))?;
        next.as_file().sync_all()?;
        next.persist(destination)
            .map_err(|error| Error::Io(error.error))?;
        File::open(destination.parent().ok_or(Error::Installation)?)?.sync_all()?;
        Ok(())
    }

    pub(super) fn begin(
        self,
        stage: &Path,
        replacing: bool,
        previous_active: bool,
        previous_enabled: bool,
    ) -> Result<Transaction, Error> {
        if self.load()?.is_some() || stage.parent() != Some(self.base().as_path()) {
            return Err(Error::RecoveryRequired);
        }
        self.secure_directory(stage, false)?;
        let stage_name = stage
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or(Error::Installation)?;
        if !safe_stage_name(stage_name) || fs::metadata(stage)?.mode() & 0o777 != 0o700 {
            return Err(Error::Installation);
        }
        let journal = Journal {
            schema_version: 1,
            stage: stage_name.to_owned(),
            replacing,
            previous_active,
            previous_enabled,
            start_attempted: false,
            enablement_attempted: false,
            candidate_stopped: false,
            enablement_reverted: false,
            complete: false,
            rolled_back: false,
            changes: Vec::new(),
        };
        File::open(stage)?.sync_all()?;
        self.save(&journal)?;
        Ok(Transaction {
            store: self,
            journal,
        })
    }

    pub(super) fn finish_operation(
        self,
        result: Result<(), Error>,
        lifecycle: &mut impl Lifecycle,
    ) -> Result<(), Error> {
        match result {
            Ok(()) => Ok(()),
            Err(error) => match self.recover(lifecycle) {
                Ok(Recovery::Committed) => Ok(()),
                Ok(Recovery::RolledBack) => Err(match error {
                    // Recovery has resolved any earlier uncertainty. Tell the
                    // caller it can now restore its previous AppImage too.
                    Error::RecoveryRequired => Error::Maintenance,
                    error => error,
                }),
                // Without a journal we cannot establish which version survived.
                Ok(Recovery::NoTransaction) | Err(_) => Err(Error::RecoveryRequired),
            },
        }
    }

    pub(super) fn recover(self, lifecycle: &mut impl Lifecycle) -> Result<Recovery, Error> {
        self.recover_with_retry_sync(lifecycle, &mut |directory| {
            File::open(directory)?.sync_all()
        })
    }

    fn recover_with_retry_sync(
        self,
        lifecycle: &mut impl Lifecycle,
        retry_sync: &mut impl FnMut(&Path) -> io::Result<()>,
    ) -> Result<Recovery, Error> {
        let Some(journal) = self.load()? else {
            return Ok(Recovery::NoTransaction);
        };
        let mut transaction = Transaction {
            store: self,
            journal,
        };
        if transaction.journal.complete {
            // A prior save may have returned after rename but before its
            // directory fsync. Confirm the terminal decision is durable before
            // telling the AppImage caller which version to keep.
            File::open(transaction.store.base())?.sync_all()?;
            transaction.finish_committed();
            return Ok(if transaction.journal.rolled_back {
                Recovery::RolledBack
            } else {
                Recovery::Committed
            });
        }
        let stage = transaction.stage();
        transaction.store.secure_directory(&stage, false)?;
        if fs::metadata(&stage)?.mode() & 0o777 != 0o700 {
            return Err(Error::RecoveryRequired);
        }
        if transaction.journal.start_attempted && !transaction.journal.candidate_stopped {
            lifecycle
                .stop_candidate()
                .map_err(|_| Error::RecoveryRequired)?;
            transaction.journal.candidate_stopped = true;
            transaction.store.save(&transaction.journal)?;
        }
        // Disable an initial installation before removing its unit file. A
        // failed disable keeps that file available for a later recovery retry.
        if transaction.journal.enablement_attempted
            && !transaction.journal.previous_enabled
            && !transaction.journal.enablement_reverted
        {
            lifecycle
                .enable(false)
                .map_err(|_| Error::RecoveryRequired)?;
            transaction.journal.enablement_reverted = true;
            transaction.store.save(&transaction.journal)?;
        }
        let mut failed = false;
        for (index, change) in transaction.journal.changes.iter().enumerate().rev() {
            let restored = (|| {
                let destination = transaction.store.path(&change.name);
                let current = transaction.store.existing_identity(&destination)?;
                if current == change.prior {
                    // A previous recovery may have restored the name but failed
                    // its directory fsync. Reconfirm durability before recording
                    // rollback and letting the caller restore its old AppImage.
                    retry_sync(&transaction.store.restored_parent(&destination)?)?;
                    return Ok(());
                }
                if current != change.candidate {
                    return Err(Error::RecoveryRequired);
                }
                match &change.prior {
                    Some(prior) => transaction.store.atomic_copy(
                        &stage.join(format!("backup-{index}")),
                        &destination,
                        prior,
                    ),
                    None => {
                        fs::remove_file(&destination)?;
                        File::open(destination.parent().ok_or(Error::Installation)?)?.sync_all()?;
                        Ok(())
                    }
                }
            })();
            failed |= restored.is_err();
        }
        failed |= lifecycle.reload().is_err();
        if transaction.journal.enablement_attempted && transaction.journal.previous_enabled {
            failed |= lifecycle.enable(true).is_err();
        }
        // Never restart mixed or externally edited executable state.
        if !failed && transaction.journal.previous_active {
            failed |= lifecycle.start_previous().is_err();
        }
        if failed {
            return Err(Error::RecoveryRequired);
        }
        transaction.journal.rolled_back = true;
        transaction.commit()?;
        Ok(Recovery::RolledBack)
    }
}

impl Transaction {
    fn stage(&self) -> PathBuf {
        self.store.base().join(&self.journal.stage)
    }

    pub(super) fn stage_path(&self) -> PathBuf {
        self.stage()
    }

    pub(super) fn replace(&mut self, name: &str, source: &Path, mode: u32) -> Result<(), Error> {
        let mut candidate = self.store.identity(source)?;
        candidate.mode = mode;
        self.intent(name, Some(candidate.clone()))?;
        self.store
            .atomic_copy(source, &self.store.path(name), &candidate)
    }

    pub(super) fn remove(&mut self, name: &str) -> Result<(), Error> {
        self.intent(name, None)?;
        let destination = self.store.path(name);
        if destination.exists() {
            fs::remove_file(&destination)?;
            File::open(destination.parent().ok_or(Error::Installation)?)?.sync_all()?;
        }
        Ok(())
    }

    fn intent(&mut self, name: &str, candidate: Option<Identity>) -> Result<(), Error> {
        if !allowed(name)
            || self
                .journal
                .changes
                .iter()
                .any(|change| change.name == name)
            || candidate.iter().any(|value| !value.validate())
        {
            return Err(Error::Installation);
        }
        let destination = self.store.path(name);
        self.store
            .secure_directory(destination.parent().ok_or(Error::Installation)?, true)?;
        let prior = self.store.existing_identity(&destination)?;
        if prior.is_some() && !self.journal.replacing {
            return Err(Error::Installation);
        }
        if let Some(prior) = &prior {
            let backup = self
                .stage()
                .join(format!("backup-{}", self.journal.changes.len()));
            let input = self.store.trusted_file(&destination)?;
            let mut output = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&backup)?;
            io::copy(&mut input.take(prior.size + 1), &mut output)?;
            output.sync_all()?;
            if !self.store.identity(&backup)?.same_bytes(prior) {
                return Err(Error::RecoveryRequired);
            }
            File::open(self.stage())?.sync_all()?;
        }
        self.journal.changes.push(Change {
            name: name.to_owned(),
            prior,
            candidate,
        });
        // This durable intent precedes the target replacement or removal.
        self.store.save(&self.journal)
    }

    pub(super) fn before_start(&mut self) -> Result<(), Error> {
        self.journal.start_attempted = true;
        self.journal.enablement_attempted = true;
        self.store.save(&self.journal)
    }

    pub(super) fn before_enablement(&mut self) -> Result<(), Error> {
        self.journal.enablement_attempted = true;
        self.store.save(&self.journal)
    }

    pub(super) fn commit(&mut self) -> Result<(), Error> {
        self.journal.complete = true;
        self.store.save(&self.journal)?;
        self.finish_committed();
        Ok(())
    }

    fn finish_committed(&self) {
        // The durable terminal decision is authoritative. Cleanup failure must
        // not make the unprivileged caller roll back only its AppImage while
        // leaving the new service installed. The journal retains retry intent.
        if self.finish().is_err() {
            eprintln!("Linux maintenance completed; retained transaction cleanup will be retried");
        }
    }

    fn finish(&self) -> Result<(), Error> {
        let stage = self.stage();
        if fs::symlink_metadata(&stage).is_ok() {
            self.store.secure_directory(&stage, false)?;
            if fs::metadata(&stage)?.mode() & 0o777 != 0o700 {
                return Err(Error::RecoveryRequired);
            }
            fs::remove_dir_all(stage)?;
        }
        fs::remove_file(self.store.base().join(JOURNAL))?;
        File::open(self.store.base())?.sync_all()?;
        Ok(())
    }
}

fn allowed(name: &str) -> bool {
    SERVICE_FILES.contains(&name) || name == RECORD.trim_start_matches('/')
}

fn safe_stage_name(name: &str) -> bool {
    name.starts_with("transaction-")
        && name.len() <= 128
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::{Builder, TempDir};

    #[derive(Default)]
    struct FakeLifecycle {
        stops: usize,
        starts: usize,
        enables: Vec<bool>,
        fail_stop: bool,
        fail_reload: bool,
    }
    impl Lifecycle for FakeLifecycle {
        fn stop_candidate(&mut self) -> Result<(), Error> {
            self.stops += 1;
            if self.fail_stop {
                Err(Error::Maintenance)
            } else {
                Ok(())
            }
        }
        fn reload(&mut self) -> Result<(), Error> {
            if self.fail_reload {
                Err(Error::Maintenance)
            } else {
                Ok(())
            }
        }
        fn enable(&mut self, enabled: bool) -> Result<(), Error> {
            self.enables.push(enabled);
            Ok(())
        }
        fn start_previous(&mut self) -> Result<(), Error> {
            self.starts += 1;
            Ok(())
        }
    }

    fn fixture() -> (TempDir, Store, PathBuf) {
        let root = TempDir::new().unwrap();
        let store = Store {
            root: root.path().to_owned(),
            owner: root.path().metadata().unwrap().uid(),
        };
        store.secure_directory(&store.base(), true).unwrap();
        let stage = Builder::new()
            .prefix("transaction-")
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir_in(store.base())
            .unwrap()
            .keep();
        (root, store, stage)
    }
    fn reopen(root: &TempDir) -> Store {
        Store {
            root: root.path().to_owned(),
            owner: root.path().metadata().unwrap().uid(),
        }
    }
    fn write_file(path: &Path, bytes: &[u8], mode: u32) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
    }

    #[test]
    fn restart_rolls_back_every_published_prefix_and_preserves_prior_mode() {
        for count in 0..=SERVICE_FILES.len() {
            let (root, store, stage) = fixture();
            for name in SERVICE_FILES {
                write_file(&store.path(name), b"old", 0o755);
            }
            write_file(&stage.join("candidate"), b"new", 0o600);
            let mut transaction = store.begin(&stage, true, true, true).unwrap();
            for name in &SERVICE_FILES[..count] {
                transaction
                    .replace(name, &stage.join("candidate"), 0o755)
                    .unwrap();
            }
            drop(transaction); // Simulated process death; journal and backups survive.
            let mut lifecycle = FakeLifecycle::default();
            reopen(&root).recover(&mut lifecycle).unwrap();
            assert_eq!(lifecycle.stops, 0, "never stop an unstarted candidate");
            assert_eq!(lifecycle.starts, 1);
            for name in SERVICE_FILES {
                let path = reopen(&root).path(name);
                assert_eq!(fs::read(&path).unwrap(), b"old");
                assert_eq!(fs::metadata(path).unwrap().mode() & 0o777, 0o755);
            }
            assert!(!stage.exists());
        }
    }

    #[test]
    fn external_changes_are_preserved_other_resources_recover_and_retry_is_idempotent() {
        let (root, store, stage) = fixture();
        for name in &SERVICE_FILES[..2] {
            write_file(&store.path(name), b"old", 0o755);
        }
        write_file(&stage.join("candidate"), b"new", 0o600);
        let mut transaction = store.begin(&stage, true, true, false).unwrap();
        for name in &SERVICE_FILES[..2] {
            transaction
                .replace(name, &stage.join("candidate"), 0o755)
                .unwrap();
        }
        let external = reopen(&root).path(SERVICE_FILES[0]);
        write_file(&external, b"external admin change", 0o755);
        assert!(
            reopen(&root)
                .recover(&mut FakeLifecycle::default())
                .is_err()
        );
        assert_eq!(fs::read(&external).unwrap(), b"external admin change");
        assert_eq!(
            fs::read(reopen(&root).path(SERVICE_FILES[1])).unwrap(),
            b"old"
        );
        assert!(stage.exists());
        write_file(&external, b"new", 0o755);
        reopen(&root)
            .recover(&mut FakeLifecycle::default())
            .unwrap();
        assert_eq!(fs::read(external).unwrap(), b"old");
    }

    #[test]
    fn missing_original_and_removal_recover_only_owned_candidate() {
        let (root, store, stage) = fixture();
        write_file(&stage.join("candidate"), b"new", 0o600);
        let mut transaction = store.begin(&stage, false, false, false).unwrap();
        transaction
            .replace(SERVICE_FILES[0], &stage.join("candidate"), 0o755)
            .unwrap();
        transaction.before_start().unwrap();
        let mut lifecycle = FakeLifecycle {
            fail_stop: true,
            ..Default::default()
        };
        assert!(reopen(&root).recover(&mut lifecycle).is_err());
        assert!(reopen(&root).path(SERVICE_FILES[0]).exists());
        lifecycle.fail_stop = false;
        reopen(&root).recover(&mut lifecycle).unwrap();
        assert!(!reopen(&root).path(SERVICE_FILES[0]).exists());
        assert_eq!(lifecycle.enables, vec![false]);

        let (root, store, stage) = fixture();
        write_file(&store.path(SERVICE_FILES[0]), b"old", 0o755);
        let mut transaction = store.begin(&stage, true, false, false).unwrap();
        transaction.remove(SERVICE_FILES[0]).unwrap();
        reopen(&root)
            .recover(&mut FakeLifecycle::default())
            .unwrap();
        assert_eq!(
            fs::read(reopen(&root).path(SERVICE_FILES[0])).unwrap(),
            b"old"
        );
    }

    #[test]
    fn restored_name_requires_directory_sync_before_rollback_is_confirmed() {
        for original_exists in [false, true] {
            let (root, store, stage) = fixture();
            let destination = store.path(SERVICE_FILES[0]);
            if original_exists {
                write_file(&destination, b"old", 0o755);
            }
            write_file(&stage.join("candidate"), b"new", 0o600);
            let mut transaction = store.begin(&stage, true, true, false).unwrap();
            transaction
                .replace(SERVICE_FILES[0], &stage.join("candidate"), 0o755)
                .unwrap();
            let parent = destination.parent().unwrap();
            // Construct the exact retry state after restoring the name, before
            // its directory fsync succeeds. Payload bytes themselves are synced.
            if original_exists {
                let mut restored = NamedTempFile::new_in(parent).unwrap();
                restored.write_all(b"old").unwrap();
                restored
                    .as_file()
                    .set_permissions(fs::Permissions::from_mode(0o755))
                    .unwrap();
                restored.as_file().sync_all().unwrap();
                restored.persist(&destination).unwrap();
            } else {
                fs::remove_file(&destination).unwrap();
            }
            drop(transaction);
            let mut attempts = 0;
            let mut lifecycle = FakeLifecycle::default();
            let result = reopen(&root).recover_with_retry_sync(&mut lifecycle, &mut |directory| {
                assert_eq!(directory, parent);
                attempts += 1;
                Err(io::Error::other("injected directory fsync failure"))
            });
            assert!(matches!(result, Err(Error::RecoveryRequired)));
            assert_eq!(attempts, 1);
            assert_eq!(lifecycle.starts, 0);
            let journal = reopen(&root).load().unwrap().unwrap();
            assert!(!journal.complete && !journal.rolled_back);
            assert!(stage.exists());
            assert_eq!(
                reopen(&root)
                    .recover_with_retry_sync(&mut lifecycle, &mut |directory| {
                        assert_eq!(directory, parent);
                        attempts += 1;
                        File::open(directory)?.sync_all()
                    })
                    .unwrap(),
                Recovery::RolledBack
            );
            assert_eq!(attempts, 2);
            assert_eq!(lifecycle.starts, 1);
            if original_exists {
                assert_eq!(fs::read(destination).unwrap(), b"old");
            } else {
                assert!(!destination.exists());
            }
            assert!(!stage.exists());
        }
    }

    #[test]
    fn absent_original_with_missing_parents_syncs_existing_ancestor() {
        let (root, store, stage) = fixture();
        let destination = store.path(SERVICE_FILES[0]);
        assert!(!destination.parent().unwrap().exists());
        let mut transaction = store.begin(&stage, false, false, false).unwrap();
        transaction.remove(SERVICE_FILES[0]).unwrap();
        // No original or replacement ever existed. Missing parents must not
        // make this valid no-op recovery permanently fail or recreate paths.
        fs::remove_dir_all(root.path().join("usr")).unwrap();
        let mut attempts = 0;
        assert_eq!(
            reopen(&root)
                .recover_with_retry_sync(&mut FakeLifecycle::default(), &mut |directory| {
                    assert_eq!(directory, root.path());
                    attempts += 1;
                    File::open(directory)?.sync_all()
                })
                .unwrap(),
            Recovery::RolledBack
        );
        assert_eq!(attempts, 1);
        assert!(!root.path().join("usr").exists());
        assert!(!stage.exists());
    }

    #[test]
    fn absent_original_never_syncs_an_untrusted_parent() {
        for symlink in [false, true] {
            let (root, store, stage) = fixture();
            let destination = store.path(SERVICE_FILES[0]);
            let parent = destination.parent().unwrap();
            let mut transaction = store.begin(&stage, false, false, false).unwrap();
            transaction.remove(SERVICE_FILES[0]).unwrap();
            let saved = parent.with_file_name("trusted-original");
            if symlink {
                fs::rename(parent, &saved).unwrap();
                std::os::unix::fs::symlink(&saved, parent).unwrap();
            } else {
                fs::set_permissions(parent, fs::Permissions::from_mode(0o777)).unwrap();
            }
            assert!(
                reopen(&root)
                    .recover_with_retry_sync(&mut FakeLifecycle::default(), &mut |_| {
                        panic!("untrusted directory must be rejected before fsync")
                    })
                    .is_err()
            );
            assert!(!reopen(&root).load().unwrap().unwrap().complete);
            assert!(stage.exists());
            if symlink {
                fs::remove_file(parent).unwrap();
                fs::rename(saved, parent).unwrap();
            } else {
                fs::set_permissions(parent, fs::Permissions::from_mode(0o755)).unwrap();
            }
            assert_eq!(
                reopen(&root)
                    .recover(&mut FakeLifecycle::default())
                    .unwrap(),
                Recovery::RolledBack
            );
        }
    }

    #[test]
    fn corrupt_journal_and_tampered_backup_never_restore_untrusted_bytes() {
        let (root, store, stage) = fixture();
        write_file(&store.path(SERVICE_FILES[0]), b"old", 0o755);
        write_file(&stage.join("candidate"), b"new", 0o600);
        let mut transaction = store.begin(&stage, true, false, false).unwrap();
        assert!(
            transaction
                .replace("../escape", &stage.join("candidate"), 0o755)
                .is_err()
        );
        transaction
            .replace(SERVICE_FILES[0], &stage.join("candidate"), 0o755)
            .unwrap();
        write_file(&stage.join("backup-0"), b"tampered", 0o600);
        assert!(
            reopen(&root)
                .recover(&mut FakeLifecycle::default())
                .is_err()
        );
        assert_eq!(
            fs::read(reopen(&root).path(SERVICE_FILES[0])).unwrap(),
            b"new"
        );
        let journal = reopen(&root).base().join(JOURNAL);
        fs::write(&journal, b"{malformed}").unwrap();
        assert!(
            reopen(&root)
                .recover(&mut FakeLifecycle::default())
                .is_err()
        );
        fs::write(&journal, vec![b'x'; MAX_JOURNAL as usize + 1]).unwrap();
        assert!(
            reopen(&root)
                .recover(&mut FakeLifecycle::default())
                .is_err()
        );
    }
    #[test]
    fn recovery_retry_does_not_execute_a_removed_candidate() {
        let (root, store, stage) = fixture();
        write_file(&stage.join("candidate"), b"new", 0o600);
        let mut transaction = store.begin(&stage, false, false, false).unwrap();
        for name in SERVICE_FILES {
            transaction
                .replace(name, &stage.join("candidate"), 0o755)
                .unwrap();
        }
        transaction.before_start().unwrap();
        let mut first = FakeLifecycle {
            fail_reload: true,
            ..Default::default()
        };
        assert!(reopen(&root).recover(&mut first).is_err());
        assert_eq!(first.stops, 1);
        assert_eq!(first.enables, vec![false]);
        for name in SERVICE_FILES {
            assert!(!reopen(&root).path(name).exists());
        }
        let mut retry = FakeLifecycle {
            fail_stop: true,
            ..Default::default()
        };
        reopen(&root).recover(&mut retry).unwrap();
        assert_eq!(retry.stops, 0);
        assert!(retry.enables.is_empty());
        assert!(!stage.exists());
    }

    #[test]
    fn intent_without_replacement_and_completed_cleanup_survive_restart() {
        let (root, store, stage) = fixture();
        let name = SERVICE_FILES[0];
        write_file(&store.path(name), b"old", 0o755);
        write_file(&stage.join("candidate"), b"new", 0o600);
        let candidate = store.identity(&stage.join("candidate")).unwrap();
        let mut transaction = store.begin(&stage, true, false, false).unwrap();
        transaction.intent(name, Some(candidate)).unwrap();
        reopen(&root)
            .recover(&mut FakeLifecycle::default())
            .unwrap();
        assert_eq!(fs::read(reopen(&root).path(name)).unwrap(), b"old");

        let (root, store, stage) = fixture();
        let mut transaction = store.begin(&stage, false, false, false).unwrap();
        transaction.journal.complete = true;
        transaction.store.save(&transaction.journal).unwrap();
        fs::remove_dir_all(&stage).unwrap(); // Crash during committed cleanup.
        let mut lifecycle = FakeLifecycle {
            fail_stop: true,
            fail_reload: true,
            ..Default::default()
        };
        reopen(&root).recover(&mut lifecycle).unwrap();
        assert_eq!(lifecycle.stops, 0);
        assert!(reopen(&root).load().unwrap().is_none());
    }

    #[test]
    fn durable_commit_boundary_determines_the_result_after_an_io_error() {
        for committed in [false, true] {
            let (root, store, stage) = fixture();
            let destination = store.path(SERVICE_FILES[0]);
            write_file(&destination, b"old", 0o755);
            write_file(&stage.join("candidate"), b"new", 0o600);
            let mut transaction = store.begin(&stage, true, true, true).unwrap();
            transaction
                .replace(SERVICE_FILES[0], &stage.join("candidate"), 0o755)
                .unwrap();
            // The same in-memory state can accompany a failed save either
            // before or after rename. Only the published journal decides.
            transaction.journal.complete = true;
            if committed {
                transaction.store.save(&transaction.journal).unwrap();
            }
            drop(transaction);
            let mut lifecycle = FakeLifecycle::default();
            let result = reopen(&root).finish_operation(
                Err(io::Error::other("injected commit save failure").into()),
                &mut lifecycle,
            );
            assert_eq!(result.is_ok(), committed);
            assert_eq!(
                fs::read(&destination).unwrap(),
                if committed { b"new" } else { b"old" }
            );
            assert_eq!(lifecycle.starts, usize::from(!committed));
            assert!(!stage.exists());
            assert!(reopen(&root).load().unwrap().is_none());
        }
    }

    #[test]
    fn committed_cleanup_failure_keeps_new_service_and_retry_never_rolls_back() {
        let (root, store, stage) = fixture();
        let destination = store.path(SERVICE_FILES[0]);
        write_file(&destination, b"old", 0o755);
        write_file(&stage.join("candidate"), b"new", 0o600);
        let mut transaction = store.begin(&stage, true, true, true).unwrap();
        transaction
            .replace(SERVICE_FILES[0], &stage.join("candidate"), 0o755)
            .unwrap();
        // Cleanup rejects this altered staging mode even when tests run as root.
        fs::set_permissions(&stage, fs::Permissions::from_mode(0o755)).unwrap();
        transaction.commit().unwrap();
        assert_eq!(fs::read(&destination).unwrap(), b"new");
        assert!(reopen(&root).load().unwrap().unwrap().complete);
        let mut lifecycle = FakeLifecycle {
            fail_stop: true,
            fail_reload: true,
            ..Default::default()
        };
        assert_eq!(
            reopen(&root).recover(&mut lifecycle).unwrap(),
            Recovery::Committed
        );
        assert!(stage.exists());
        assert!(matches!(
            reopen(&root).begin(&stage, true, true, true),
            Err(Error::RecoveryRequired)
        ));
        fs::set_permissions(&stage, fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(
            reopen(&root).recover(&mut lifecycle).unwrap(),
            Recovery::Committed
        );
        assert_eq!(fs::read(destination).unwrap(), b"new");
        assert_eq!(lifecycle.stops, 0);
        assert_eq!(lifecycle.starts, 0);
        assert!(!stage.exists());
    }

    #[test]
    fn rollback_terminal_journal_never_becomes_a_successful_installation() {
        let (root, store, stage) = fixture();
        let destination = store.path(SERVICE_FILES[0]);
        write_file(&destination, b"old", 0o755);
        let mut transaction = store.begin(&stage, true, false, false).unwrap();
        // Crash after rollback reached its terminal decision but before cleanup.
        transaction.journal.complete = true;
        transaction.journal.rolled_back = true;
        transaction.store.save(&transaction.journal).unwrap();
        let result =
            reopen(&root).finish_operation(Err(Error::Maintenance), &mut FakeLifecycle::default());
        assert!(matches!(result, Err(Error::Maintenance)));
        assert_eq!(fs::read(destination).unwrap(), b"old");
        assert!(!stage.exists());
    }

    #[test]
    fn successful_recovery_resolves_an_initially_uncertain_failure() {
        let (root, store, stage) = fixture();
        let destination = store.path(SERVICE_FILES[0]);
        write_file(&destination, b"old", 0o755);
        write_file(&stage.join("candidate"), b"new", 0o600);
        let mut transaction = store.begin(&stage, true, false, false).unwrap();
        transaction
            .replace(SERVICE_FILES[0], &stage.join("candidate"), 0o755)
            .unwrap();
        let error = reopen(&root)
            .finish_operation(Err(Error::RecoveryRequired), &mut FakeLifecycle::default())
            .unwrap_err();
        assert_eq!(error.exit_code(), 1);
        assert_eq!(fs::read(destination).unwrap(), b"old");
        assert!(!stage.exists());
    }

    #[test]
    fn missing_or_corrupt_transaction_cannot_prove_safe_appimage_rollback() {
        let (root, store, stage) = fixture();
        assert!(matches!(
            store.finish_operation(Err(Error::Maintenance), &mut FakeLifecycle::default()),
            Err(Error::RecoveryRequired)
        ));
        let transaction = reopen(&root).begin(&stage, false, false, false).unwrap();
        fs::write(transaction.store.base().join(JOURNAL), b"corrupt").unwrap();
        assert!(matches!(
            reopen(&root).finish_operation(Err(Error::Maintenance), &mut FakeLifecycle::default()),
            Err(Error::RecoveryRequired)
        ));
        assert!(stage.exists());
    }

    #[test]
    fn fresh_state_directories_are_private_without_changing_system_ancestors() {
        let (_root, store, stage) = fixture();
        for path in [store.path("/var/lib/usque"), store.base(), stage] {
            assert_eq!(fs::metadata(path).unwrap().mode() & 0o777, 0o700);
        }
        assert_eq!(
            fs::metadata(store.path("/var/lib")).unwrap().mode() & 0o777,
            0o755
        );
        fs::set_permissions(store.base(), fs::Permissions::from_mode(0o755)).unwrap();
        assert!(store.secure_directory(&store.base(), false).is_err());
        store.secure_directory(&store.base(), true).unwrap();
        assert_eq!(fs::metadata(store.base()).unwrap().mode() & 0o777, 0o700);
    }
}
