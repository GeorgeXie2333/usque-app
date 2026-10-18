//! Current-session GNOME/KDE proxy ownership and encrypted recovery journal.
//! Every mutation has a durable prior intent. Restore only values that still
//! equal ours; subsequent user or application changes always win.

use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    net::SocketAddr,
    os::{
        fd::AsRawFd,
        unix::fs::{MetadataExt, OpenOptionsExt},
    },
    path::PathBuf,
    sync::{Arc, Mutex},
};
use usque_core::chain_exit::store::ProfileCipher;
use usque_platform::linux_paths::{LinuxPaths, effective_uid, private_directory};
use uuid::Uuid;
use zeroize::Zeroizing;

const JOURNAL_ID: Uuid = Uuid::from_bytes([
    0x99, 0x51, 0x35, 0x02, 0x2b, 0x75, 0x44, 0x11, 0x83, 0x69, 0x46, 0xd0, 0x10, 0xb2, 0x01, 0xe7,
]);

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
enum Desktop {
    Gnome,
    Kde,
}
#[derive(Clone, Serialize, Deserialize)]
struct Change {
    key: String,
    before: Option<String>,
    applied: String,
}
#[derive(Serialize, Deserialize)]
struct Journal {
    version: u32,
    desktop: Desktop,
    changes: Vec<Change>,
}

trait Backend: Send + Sync {
    fn read(&self, desktop: Desktop, key: &str, user_only: bool) -> io::Result<Option<String>>;
    fn write(&self, desktop: Desktop, key: &str, value: Option<&str>) -> io::Result<()>;
    fn notify(&self, desktop: Desktop) -> io::Result<()>;
}

pub(crate) struct LinuxSystemProxyGuard {
    transaction: Arc<Mutex<Transaction>>,
}
struct Transaction {
    path: PathBuf,
    cipher: Arc<dyn ProfileCipher>,
    backend: Arc<dyn Backend>,
    journal: Option<Journal>,
    _lease: File,
}

impl LinuxSystemProxyGuard {
    pub(crate) async fn start(
        listener: SocketAddr,
        cipher: Arc<dyn ProfileCipher>,
    ) -> io::Result<Self> {
        if !listener.ip().is_loopback() || listener.port() == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "system proxy requires a loopback HTTP listener",
            ));
        }
        let paths = LinuxPaths::from_environment()?;
        let backend = Arc::new(NativeBackend {
            config: paths.config.parent().ok_or_else(failed)?.to_path_buf(),
        });
        let desktop = current_desktop()?;
        let transaction = tokio::task::spawn_blocking(move || {
            private_directory(&paths.state)?;
            Transaction::start(
                paths.state.join("system-proxy-v1.sealed"),
                cipher,
                backend,
                desktop,
                listener,
            )
        })
        .await
        .map_err(|_| failed())??;
        Ok(Self {
            transaction: Arc::new(Mutex::new(transaction)),
        })
    }

    pub(crate) async fn shutdown(&mut self) -> io::Result<()> {
        let transaction = Arc::clone(&self.transaction);
        tokio::task::spawn_blocking(move || transaction.lock().map_err(|_| failed())?.restore())
            .await
            .map_err(|_| failed())?
    }

    pub(crate) async fn recover(cipher: Arc<dyn ProfileCipher>) -> io::Result<()> {
        let paths = LinuxPaths::from_environment()?;
        let path = paths.state.join("system-proxy-v1.sealed");
        if !path.try_exists()? {
            return Ok(());
        }
        let backend = Arc::new(NativeBackend {
            config: paths.config.parent().ok_or_else(failed)?.to_path_buf(),
        });
        tokio::task::spawn_blocking(move || {
            private_directory(&paths.state)?;
            let lease = proxy_lease(&path)?;
            let mut transaction = Transaction {
                _lease: lease,
                path,
                cipher,
                backend,
                journal: None,
            };
            transaction.load()?;
            transaction.restore()
        })
        .await
        .map_err(|_| failed())?
    }
}

fn proxy_lease(path: &std::path::Path) -> io::Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path.with_extension("lock"))?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.uid() != effective_uid() || metadata.mode() & 0o077 != 0 {
        return Err(failed());
    }
    // SAFETY: flock borrows an owned, open file descriptor without transferring ownership.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            "another Engine owns system proxy recovery",
        ));
    }
    Ok(file)
}

impl Transaction {
    fn start(
        path: PathBuf,
        cipher: Arc<dyn ProfileCipher>,
        backend: Arc<dyn Backend>,
        desktop: Desktop,
        listener: SocketAddr,
    ) -> io::Result<Self> {
        let lease = proxy_lease(&path)?;
        let mut transaction = Self {
            _lease: lease,
            path,
            cipher,
            backend,
            journal: None,
        };
        transaction.load()?;
        transaction.restore()?;
        let changes = desired(desktop, listener)
            .into_iter()
            .map(|(key, applied)| {
                let before = transaction.backend.read(desktop, &key, true)?;
                if before.as_ref().is_some_and(|value| value.len() > 4096) {
                    return Err(failed());
                }
                Ok(Change {
                    key,
                    before,
                    applied,
                })
            })
            .collect::<io::Result<Vec<_>>>()?;
        transaction.journal = Some(Journal {
            version: 1,
            desktop,
            changes,
        });
        transaction.persist()?;
        let journal = transaction.journal.as_ref().ok_or_else(failed)?;
        for change in &journal.changes {
            transaction
                .backend
                .write(desktop, &change.key, Some(&change.applied))?;
            if transaction
                .backend
                .read(desktop, &change.key, false)?
                .as_deref()
                != Some(&change.applied)
            {
                return Err(failed());
            }
        }
        transaction.backend.notify(desktop)?;
        Ok(transaction)
    }

    fn load(&mut self) -> io::Result<()> {
        let file = match OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&self.path)
        {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        };
        let metadata = file.metadata()?;
        if !metadata.is_file()
            || metadata.uid() != effective_uid()
            || metadata.mode() & 0o077 != 0
            || metadata.len() > 64 * 1024
        {
            return Err(failed());
        }
        let mut bytes = Vec::new();
        file.take(64 * 1024 + 1).read_to_end(&mut bytes)?;
        if bytes.len() > 64 * 1024 {
            return Err(failed());
        }
        let plaintext = self.cipher.open(JOURNAL_ID, &bytes).map_err(|_| failed())?;
        let journal: Journal = serde_json::from_slice(&plaintext).map_err(|_| failed())?;
        if journal.version != 1
            || journal.changes.is_empty()
            || journal.changes.len() > 8
            || journal.changes.iter().any(|change| {
                !allowed_key(journal.desktop, &change.key)
                    || change.applied.len() > 4096
                    || change
                        .before
                        .as_ref()
                        .is_some_and(|value| value.len() > 4096)
            })
        {
            return Err(failed());
        }
        self.journal = Some(journal);
        Ok(())
    }

    fn persist(&self) -> io::Result<()> {
        let plaintext = Zeroizing::new(
            serde_json::to_vec(self.journal.as_ref().ok_or_else(failed)?).map_err(|_| failed())?,
        );
        let encrypted = self
            .cipher
            .seal(JOURNAL_ID, &plaintext)
            .map_err(|_| failed())?;
        let parent = self.path.parent().ok_or_else(failed)?;
        let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
        temporary.write_all(&encrypted)?;
        temporary.as_file().sync_all()?;
        temporary.persist(&self.path).map_err(|error| error.error)?;
        File::open(parent)?.sync_all()
    }

    fn restore(&mut self) -> io::Result<()> {
        let Some(journal) = self.journal.as_ref() else {
            return Ok(());
        };
        let mut failure = None;
        // Mode was applied last and is restored first, so readers cannot see an
        // enabled proxy after its endpoint has already been removed.
        for change in journal.changes.iter().rev() {
            let restore = || {
                if self
                    .backend
                    .read(journal.desktop, &change.key, false)?
                    .as_deref()
                    == Some(&change.applied)
                {
                    self.backend
                        .write(journal.desktop, &change.key, change.before.as_deref())?;
                    if self.backend.read(journal.desktop, &change.key, true)? != change.before {
                        return Err(failed());
                    }
                }
                Ok::<_, io::Error>(())
            };
            if let Err(error) = restore() {
                failure.get_or_insert(error);
            }
        }
        if let Err(error) = self.backend.notify(journal.desktop) {
            failure.get_or_insert(error);
        }
        if let Some(error) = failure {
            return Err(error);
        }
        fs::remove_file(&self.path)?;
        File::open(self.path.parent().ok_or_else(failed)?)?.sync_all()?;
        self.journal = None;
        Ok(())
    }
}
impl Drop for Transaction {
    fn drop(&mut self) {
        // An abandoned start future still restores changes. Failed cleanup
        // keeps the encrypted durable journal for the next Engine session.
        let _ = self.restore();
    }
}

fn desired(desktop: Desktop, listener: SocketAddr) -> Vec<(String, String)> {
    let host = listener.ip().to_string();
    let port = listener.port().to_string();
    let values = match desktop {
        Desktop::Gnome => vec![
            ("http.host", format!("'{host}'")),
            ("http.port", port.clone()),
            ("http.use-authentication", "false".into()),
            ("https.host", format!("'{host}'")),
            ("https.port", port),
            ("mode", "'manual'".into()),
        ],
        Desktop::Kde => vec![
            ("httpProxy", format!("http://{listener}")),
            ("httpsProxy", format!("http://{listener}")),
            ("ProxyType", "1".into()),
        ],
    };
    values
        .into_iter()
        .map(|(key, value)| (key.into(), value))
        .collect()
}
fn allowed_key(desktop: Desktop, key: &str) -> bool {
    match desktop {
        Desktop::Gnome => matches!(
            key,
            "http.host"
                | "http.port"
                | "http.use-authentication"
                | "https.host"
                | "https.port"
                | "mode"
        ),
        Desktop::Kde => matches!(key, "httpProxy" | "httpsProxy" | "ProxyType"),
    }
}
fn current_desktop() -> io::Result<Desktop> {
    let desktop = std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .to_ascii_lowercase();
    if desktop.split(':').any(|name| name == "kde") {
        Ok(Desktop::Kde)
    } else if desktop
        .split(':')
        .any(|name| matches!(name, "gnome" | "ubuntu" | "unity"))
    {
        Ok(Desktop::Gnome)
    } else {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "system proxy requires a GNOME or KDE user session",
        ))
    }
}
fn failed() -> io::Error {
    io::Error::other(
        "session proxy change or recovery could not be verified; retry after unlocking the keyring and desktop session",
    )
}

struct NativeBackend {
    config: PathBuf,
}
impl Backend for NativeBackend {
    fn read(&self, desktop: Desktop, key: &str, user_only: bool) -> io::Result<Option<String>> {
        if !allowed_key(desktop, key) {
            return Err(failed());
        }
        match desktop {
            Desktop::Gnome => {
                let (schema, key) = gnome_key(key);
                usque_platform::linux_gsettings::read(&schema, key, user_only)
            }
            Desktop::Kde => Ok(kde_entry(&self.read_kde()?, key)?.map(str::to_owned)),
        }
    }
    fn write(&self, desktop: Desktop, key: &str, value: Option<&str>) -> io::Result<()> {
        if !allowed_key(desktop, key) {
            return Err(failed());
        }
        match desktop {
            Desktop::Gnome => {
                let (schema, key) = gnome_key(key);
                usque_platform::linux_gsettings::write(&schema, key, value)
            }
            Desktop::Kde => {
                let current = self.read_kde()?;
                let updated = update_kde(&current, key, value)?;
                let mut temporary = tempfile::NamedTempFile::new_in(&self.config)?;
                temporary.write_all(updated.as_bytes())?;
                temporary.as_file().sync_all()?;
                // Compare the whole file immediately before replacement; an
                // intervening writer makes this operation retryable.
                if self.read_kde()? != current {
                    return Err(failed());
                }
                temporary
                    .persist(self.config.join("kioslaverc"))
                    .map_err(|error| error.error)?;
                File::open(&self.config)?.sync_all()
            }
        }
    }
    fn notify(&self, desktop: Desktop) -> io::Result<()> {
        if desktop == Desktop::Gnome {
            return Ok(());
        }
        // KIO listens to this session-bus signal; it carries no settings or secrets.
        let mut child = std::process::Command::new("/usr/bin/gdbus")
            .args([
                "emit",
                "--session",
                "--object-path",
                "/KIO/Scheduler",
                "--signal",
                "org.kde.KIO.Scheduler.reparseSlaveConfiguration",
                "''",
            ])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if let Some(status) = child.try_wait()? {
                return if status.success() {
                    Ok(())
                } else {
                    Err(failed())
                };
            }
            if std::time::Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                return Err(failed());
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
}
impl NativeBackend {
    fn read_kde(&self) -> io::Result<String> {
        let file = match OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(self.config.join("kioslaverc"))
        {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(String::new()),
            Err(error) => return Err(error),
        };
        let metadata = file.metadata()?;
        if !metadata.is_file()
            || metadata.uid() != effective_uid()
            || metadata.mode() & 0o022 != 0
            || metadata.len() > 1024 * 1024
        {
            return Err(failed());
        }
        let mut result = String::new();
        file.take(1024 * 1024 + 1).read_to_string(&mut result)?;
        if result.len() > 1024 * 1024 {
            return Err(failed());
        }
        Ok(result)
    }
}
fn gnome_key(key: &str) -> (String, &str) {
    match key.split_once('.') {
        Some((child, key)) => (format!("org.gnome.system.proxy.{child}"), key),
        None => ("org.gnome.system.proxy".into(), key),
    }
}
fn kde_entry<'a>(document: &'a str, key: &str) -> io::Result<Option<&'a str>> {
    let mut proxy = false;
    let mut result = None;
    for line in document.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            proxy = trimmed == "[Proxy Settings]";
            if trimmed.starts_with("[Proxy Settings][") {
                return Err(failed());
            }
        } else if proxy && let Some((name, value)) = line.split_once('=') {
            if name == key {
                if result.is_some() {
                    return Err(failed());
                }
                result = Some(value);
            } else if name.starts_with(&format!("{key}[")) {
                return Err(failed());
            }
        }
    }
    Ok(result)
}
fn update_kde(document: &str, key: &str, value: Option<&str>) -> io::Result<String> {
    kde_entry(document, key)?;
    if value.is_some_and(|value| value.contains(['\r', '\n'])) {
        return Err(failed());
    }
    let mut output = String::new();
    let mut proxy = false;
    let mut found_group = false;
    let mut written = false;
    for line in document.lines() {
        if line.trim().starts_with('[') {
            if proxy && !written {
                if let Some(value) = value {
                    output.push_str(&format!("{key}={value}\n"));
                }
                written = true;
            }
            proxy = line.trim() == "[Proxy Settings]";
            found_group |= proxy;
        }
        if proxy && line.split_once('=').is_some_and(|(name, _)| name == key) {
            if let Some(value) = value {
                output.push_str(&format!("{key}={value}\n"));
            }
            written = true;
        } else {
            output.push_str(line);
            output.push('\n');
        }
    }
    if !written && let Some(value) = value {
        if !found_group {
            output.push_str("[Proxy Settings]\n");
        }
        output.push_str(&format!("{key}={value}\n"));
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    struct FakeBackend {
        values: Mutex<BTreeMap<String, String>>,
        fail_key: Option<String>,
    }
    impl Backend for FakeBackend {
        fn read(&self, _: Desktop, key: &str, _: bool) -> io::Result<Option<String>> {
            Ok(self.values.lock().unwrap().get(key).cloned())
        }
        fn write(&self, _: Desktop, key: &str, value: Option<&str>) -> io::Result<()> {
            if self.fail_key.as_deref() == Some(key) {
                return Err(failed());
            }
            let mut values = self.values.lock().unwrap();
            if let Some(value) = value {
                values.insert(key.into(), value.into());
            } else {
                values.remove(key);
            }
            Ok(())
        }
        fn notify(&self, _: Desktop) -> io::Result<()> {
            Ok(())
        }
    }
    struct TestCipher;
    impl ProfileCipher for TestCipher {
        fn seal(
            &self,
            _: Uuid,
            value: &[u8],
        ) -> Result<Vec<u8>, usque_core::chain_exit::ImportError> {
            Ok(value.to_vec())
        }
        fn open(
            &self,
            _: Uuid,
            value: &[u8],
        ) -> Result<Zeroizing<Vec<u8>>, usque_core::chain_exit::ImportError> {
            Ok(Zeroizing::new(value.to_vec()))
        }
    }
    #[test]
    fn restores_only_owned_settings_and_preserves_external_edits() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("journal");
        let backend = Arc::new(FakeBackend {
            values: Mutex::new(BTreeMap::from([("http.host".into(), "'prior'".into())])),
            fail_key: None,
        });
        let mut transaction = Transaction::start(
            path.clone(),
            Arc::new(TestCipher),
            backend.clone(),
            Desktop::Gnome,
            "127.0.0.1:8080".parse().unwrap(),
        )
        .unwrap();
        backend
            .values
            .lock()
            .unwrap()
            .insert("https.host".into(), "'external'".into());
        transaction.restore().unwrap();
        let values = backend.values.lock().unwrap();
        assert_eq!(values.get("http.host").unwrap(), "'prior'");
        assert_eq!(values.get("https.host").unwrap(), "'external'");
        assert!(!values.contains_key("mode"));
        assert!(!path.exists());
    }
    #[test]
    fn every_apply_failure_restores_the_successful_prefix() {
        for (key, _) in desired(Desktop::Gnome, "127.0.0.1:8080".parse().unwrap()) {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("journal");
            let backend = Arc::new(FakeBackend {
                values: Mutex::new(BTreeMap::new()),
                fail_key: Some(key),
            });
            assert!(
                Transaction::start(
                    path.clone(),
                    Arc::new(TestCipher),
                    backend.clone(),
                    Desktop::Gnome,
                    "127.0.0.1:8080".parse().unwrap()
                )
                .is_err()
            );
            assert!(backend.values.lock().unwrap().is_empty());
            assert!(!path.exists());
        }
    }
    #[test]
    fn a_second_engine_cannot_take_over_a_live_proxy_journal() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("journal");
        let backend = Arc::new(FakeBackend {
            values: Mutex::new(BTreeMap::new()),
            fail_key: None,
        });
        let transaction = Transaction::start(
            path.clone(),
            Arc::new(TestCipher),
            backend.clone(),
            Desktop::Gnome,
            "127.0.0.1:8080".parse().unwrap(),
        )
        .unwrap();
        assert!(
            Transaction::start(
                path.clone(),
                Arc::new(TestCipher),
                backend.clone(),
                Desktop::Gnome,
                "127.0.0.1:9090".parse().unwrap()
            )
            .is_err()
        );
        assert_eq!(
            backend.values.lock().unwrap().get("http.port").unwrap(),
            "8080"
        );
        drop(transaction);
        assert!(backend.values.lock().unwrap().is_empty());
        assert!(!path.exists());
    }
    #[test]
    fn kde_edits_preserve_other_groups_and_reject_immutable_overrides() {
        let text =
            "[Other]\nkeep=unchanged\n[Proxy Settings]\nhttpProxy=http://old\nNoProxyFor=local\n";
        let changed = update_kde(text, "httpProxy", Some("http://127.0.0.1:8080")).unwrap();
        assert!(changed.contains("keep=unchanged"));
        assert!(changed.contains("NoProxyFor=local"));
        assert_eq!(
            kde_entry(&changed, "httpProxy").unwrap(),
            Some("http://127.0.0.1:8080")
        );
        assert!(update_kde("[Proxy Settings][$i]\nhttpProxy=old\n", "httpProxy", None).is_err());
        assert!(update_kde("[Proxy Settings]\nhttpProxy[$i]=old\n", "httpProxy", None).is_err());
    }
}
