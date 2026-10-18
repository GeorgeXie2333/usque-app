//! Linux maintenance boundary. Only authenticated signed bytes reach a package
//! manager or a fixed service path; user-controlled paths are never executed as
//! root. Package downloads are copied into private staging before verification.

use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Seek, SeekFrom, Write},
    os::{
        fd::AsRawFd,
        unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    },
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use clap::{ArgGroup, Parser};
use tempfile::{Builder, TempDir};
use thiserror::Error;
use usque_core::linux_release::{
    self, Entry, InstallRecord, MAX_MANIFEST_BYTES, Manifest, PROTOCOL_VERSION, ReleaseError,
    SERVICE_FILES,
};

const HELPER: &str = "/usr/lib/usque/usque-update";
const RECORD: &str = "/etc/usque/install.json";
const STAGING: &str = "/var/lib/usque/maintenance";

mod transaction;

#[derive(Debug, Parser)]
#[command(group(ArgGroup::new("operation").required(true).multiple(false).args([
    "verify_only", "install", "run_install", "install_service", "prepare_remove", "remove_service", "repair_service"
])))]
struct Arguments {
    #[arg(long)]
    verify_only: bool,
    #[arg(long)]
    install: bool,
    #[arg(long, hide = true)]
    run_install: bool,
    #[arg(long)]
    install_service: bool,
    #[arg(long)]
    prepare_remove: bool,
    #[arg(long, requires = "prepare_remove", conflicts_with_all = ["verify_only", "install", "run_install", "install_service", "remove_service", "repair_service"])]
    disable_service: bool,
    #[arg(long)]
    remove_service: bool,
    #[arg(long)]
    repair_service: bool,
    #[arg(long)]
    package: Option<PathBuf>,
    #[arg(long)]
    manifest: Option<PathBuf>,
    #[arg(long)]
    signature: Option<PathBuf>,
    #[arg(long)]
    bundle: Option<PathBuf>,
    #[arg(long)]
    appimage: Option<PathBuf>,
}

pub fn run() -> Result<(), Error> {
    let args = Arguments::parse();
    if args.disable_service && !args.prepare_remove {
        return Err(Error::Arguments);
    }
    // Verification is read-only. Every privileged mutation first serializes
    // against other helpers and resolves an interrupted file transaction.
    let mut maintenance_lock = if is_root() && !args.verify_only {
        let lock = lock_maintenance()?;
        transaction::Store::production()
            .recover(&mut NativeLifecycle)
            .map_err(|_| Error::RecoveryRequired)?;
        Some(lock)
    } else {
        None
    };
    if args.repair_service {
        require_root()?;
        let record = linux_release::read_install_record()?.ok_or(Error::Installation)?;
        record.validate()?;
        root_command("/usr/lib/usque/usque-agent", &["--validate-only"])?;
        return maintain_existing(true, false, false);
    }
    if args.prepare_remove || args.remove_service {
        require_root()?;
        let record = linux_release::read_install_record()?.ok_or(Error::Installation)?;
        if args.remove_service {
            record.require_source("appimage", linux_release::host_architecture()?)?;
        }
        return maintain_existing(false, args.remove_service, args.disable_service);
    }
    if args.install_service {
        require_root()?;
        return install_service(required(&args.bundle)?);
    }
    let manifest_bytes = bounded_read(required(&args.manifest)?, MAX_MANIFEST_BYTES)?;
    let signature = bounded_read(required(&args.signature)?, 64)?;
    let architecture = linux_release::host_architecture()?;
    let record = linux_release::read_install_record()?;
    let minimum = minimum_version(record.as_ref())?;
    let manifest = linux_release::verify_manifest(
        &manifest_bytes,
        &signature,
        "packages",
        architecture,
        minimum,
    )?;
    let package_path = required(&args.package)?;
    let name = package_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(Error::Arguments)?;
    let entry = manifest.entry(name)?;
    let format = name
        .rsplit_once('.')
        .map(|(_, value)| value)
        .ok_or(Error::Arguments)?;
    if let Some(record) = &record {
        record.require_source(format, architecture)?;
    }
    if args.verify_only {
        linux_release::verify_file(open_regular(package_path)?, entry)?;
        return Ok(());
    }
    if format == "AppImage" {
        if args.run_install || is_root() {
            return Err(Error::Arguments);
        }
        return replace_appimage(
            package_path,
            required(&args.appimage)?,
            entry,
            record.is_some(),
        );
    }
    if args.install {
        // pkexec is the only authorization boundary. No setuid bits, service
        // socket operation, or per-user configuration can grant maintenance.
        let status = Command::new("/usr/bin/pkexec")
            .arg(HELPER)
            .arg("--run-install")
            .arg("--package")
            .arg(package_path)
            .arg("--manifest")
            .arg(required(&args.manifest)?)
            .arg("--signature")
            .arg(required(&args.signature)?)
            .status()?;
        return status.success().then_some(()).ok_or(Error::Authorization);
    }
    require_root()?;
    if !args.run_install || !matches!(format, "deb" | "rpm") {
        return Err(Error::Arguments);
    }
    private_root_directory(Path::new(STAGING))?;
    let staging = Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir_in(STAGING)?;
    let staged = staging.path().join(name);
    copy_verified(package_path, &staged, entry, 0o600)?;
    // The native package-manager hooks stop and recover the old service before
    // replacing its executable. Do not stop it before acquiring their lock.
    // Native package managers own their transactions. Their hooks invoke this
    // helper, so retaining our lock while waiting for apt/dnf would deadlock.
    drop(maintenance_lock.take());
    let package = staged.to_str().ok_or(Error::Arguments)?;
    if format == "deb" {
        root_command(
            "/usr/bin/apt-get",
            &["--yes", "install", "--no-remove", "--", package],
        )
    } else {
        root_command("/usr/bin/dnf", &["--assumeyes", "install", "--", package])
    }
}

fn required(value: &Option<PathBuf>) -> Result<&Path, Error> {
    value.as_deref().ok_or(Error::Arguments)
}

fn minimum_version(record: Option<&InstallRecord>) -> Result<&str, Error> {
    let built = env!("CARGO_PKG_VERSION");
    if let Some(record) = record
        && linux_release::version(&record.version)? > linux_release::version(built)?
    {
        return Ok(&record.version);
    }
    Ok(built)
}

fn is_root() -> bool {
    // SAFETY: geteuid has no arguments or memory requirements.
    unsafe { libc::geteuid() == 0 }
}

fn require_root() -> Result<(), Error> {
    is_root().then_some(()).ok_or(Error::Authorization)
}

fn open_regular(path: &Path) -> Result<File, Error> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    if !file.metadata()?.is_file() {
        return Err(Error::Arguments);
    }
    Ok(file)
}

fn bounded_read(path: &Path, maximum: usize) -> Result<Vec<u8>, Error> {
    let mut bytes = Vec::new();
    open_regular(path)?
        .take(maximum as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > maximum {
        return Err(Error::Arguments);
    }
    Ok(bytes)
}

fn copy_verified(source: &Path, destination: &Path, entry: &Entry, mode: u32) -> Result<(), Error> {
    let source = open_regular(source)?;
    let mut output = OpenOptions::new()
        .create_new(true)
        .read(true)
        .write(true)
        .mode(mode)
        .open(destination)?;
    io::copy(&mut source.take(entry.size + 1), &mut output)?;
    output.seek(SeekFrom::Start(0))?;
    linux_release::verify_file(&mut output, entry)?;
    output.sync_all()?;
    Ok(())
}

fn private_root_directory(path: &Path) -> Result<(), Error> {
    // Walk each component and reject symlinks and non-root writable parents.
    // Once a parent is trusted, an ordinary user cannot replace its child.
    let mut current = PathBuf::from("/");
    for component in path.components() {
        match component {
            std::path::Component::RootDir => continue,
            std::path::Component::Normal(name) => current.push(name),
            _ => return Err(Error::Arguments),
        }
        match fs::symlink_metadata(&current) {
            Ok(metadata) => {
                if !metadata.is_dir() || metadata.uid() != 0 || metadata.mode() & 0o022 != 0 {
                    return Err(Error::Installation);
                }
                if private_state_directory(&current) && metadata.mode() & 0o777 != 0o700 {
                    fs::set_permissions(&current, fs::Permissions::from_mode(0o700))?;
                    File::open(&current)?.sync_all()?;
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                fs::create_dir(&current)?;
                let mode = if private_state_directory(&current) {
                    0o700
                } else {
                    0o755
                };
                fs::set_permissions(&current, fs::Permissions::from_mode(mode))?;
                File::open(current.parent().ok_or(Error::Installation)?)?.sync_all()?;
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn private_state_directory(path: &Path) -> bool {
    path == Path::new("/var/lib/usque") || path == Path::new(STAGING)
}

fn root_command(executable: &str, arguments: &[&str]) -> Result<(), Error> {
    let status = Command::new(executable)
        .args(arguments)
        .env_clear()
        .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
        .env("LANG", "C.UTF-8")
        .stdin(Stdio::null())
        .status()?;
    status.success().then_some(()).ok_or(Error::Maintenance)
}

fn prepare_remove() -> Result<(), Error> {
    root_command("/usr/bin/systemctl", &["stop", "usque-agent.service"])?;
    root_command("/usr/lib/usque/usque-agent", &["--recover-state"])
}

fn lock_maintenance() -> Result<File, Error> {
    private_root_directory(Path::new(STAGING))?;
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(Path::new(STAGING).join("maintenance.lock"))?;
    let metadata = lock.metadata()?;
    if !metadata.is_file() || metadata.uid() != 0 || metadata.mode() & 0o777 != 0o600 {
        return Err(Error::Installation);
    }
    // SAFETY: lock owns a live file descriptor; flock does not dereference memory.
    if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err(Error::Maintenance);
    }
    Ok(lock)
}

struct NativeLifecycle;

impl transaction::Lifecycle for NativeLifecycle {
    fn stop_candidate(&mut self) -> Result<(), Error> {
        prepare_remove()
    }
    fn reload(&mut self) -> Result<(), Error> {
        root_command("/usr/bin/systemctl", &["daemon-reload"])
    }
    fn enable(&mut self, enabled: bool) -> Result<(), Error> {
        root_command(
            "/usr/bin/systemctl",
            &[
                if enabled { "enable" } else { "disable" },
                "usque-agent.service",
            ],
        )
    }
    fn start_previous(&mut self) -> Result<(), Error> {
        // An unsuccessful pre-stop recovery must never be hidden by restarting
        // the service over an unresolved privileged networking journal.
        root_command("/usr/lib/usque/usque-agent", &["--recover-state"])?;
        root_command("/usr/bin/systemctl", &["start", "usque-agent.service"])
    }
}

fn service_state(operation: &str) -> Result<bool, Error> {
    let status = Command::new("/usr/bin/systemctl")
        .args([operation, "--quiet", "usque-agent.service"])
        .env_clear()
        .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
        .env("LANG", "C.UTF-8")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    match status.code() {
        Some(0) => Ok(true),
        Some(1 | 3 | 4) => Ok(false),
        _ => Err(Error::Maintenance),
    }
}

fn begin_transaction(staging: TempDir, replacing: bool) -> Result<transaction::Transaction, Error> {
    let active = replacing && service_state("is-active")?;
    let enabled = replacing && service_state("is-enabled")?;
    // Retain staging before publishing intent: a directory-fsync error can
    // return after the journal rename has already succeeded.
    // Only commit or successful recovery removes this protected staging folder.
    let staging = staging.keep();
    transaction::Store::production().begin(&staging, replacing, active, enabled)
}

fn finish_transaction(result: Result<(), Error>) -> Result<(), Error> {
    transaction::Store::production().finish_operation(result, &mut NativeLifecycle)
}

fn new_transaction_stage() -> Result<TempDir, Error> {
    private_root_directory(Path::new(STAGING))?;
    Ok(Builder::new()
        .prefix("transaction-")
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir_in(STAGING)?)
}

fn maintain_existing(repair: bool, remove: bool, disable: bool) -> Result<(), Error> {
    let mut transaction = begin_transaction(new_transaction_stage()?, true)?;
    let result = (|| {
        prepare_remove()?;
        if remove || disable {
            transaction.before_enablement()?;
            root_command("/usr/bin/systemctl", &["disable", "usque-agent.service"])?;
        }
        if remove {
            for name in SERVICE_FILES {
                transaction.remove(name)?;
            }
            transaction.remove(RECORD.trim_start_matches('/'))?;
        }
        if repair || remove {
            root_command("/usr/bin/systemctl", &["daemon-reload"])?;
        }
        if repair {
            transaction.before_start()?;
            root_command(
                "/usr/bin/systemctl",
                &["enable", "--now", "usque-agent.service"],
            )?;
        }
        transaction.commit()
    })();
    finish_transaction(result)
}

fn install_service(bundle: &Path) -> Result<(), Error> {
    let bytes = bounded_read(
        &bundle.join("linux-service-manifest.json"),
        MAX_MANIFEST_BYTES,
    )?;
    let signature = bounded_read(&bundle.join("linux-service-manifest.json.sig"), 64)?;
    let architecture = linux_release::host_architecture()?;
    let previous = linux_release::read_install_record()?;
    if let Some(record) = &previous {
        record.require_source("appimage", architecture)?;
    }
    let minimum = minimum_version(previous.as_ref())?;
    let manifest =
        linux_release::verify_manifest(&bytes, &signature, "service", architecture, minimum)?;
    let staging = new_transaction_stage()?;
    // No privileged child is started until every service input has been copied
    // and independently verified. Extraction never happens with root privileges.
    for (index, entry) in manifest.entries.iter().enumerate() {
        copy_verified(
            &bundle.join(&entry.name),
            &staging.path().join(index.to_string()),
            entry,
            0o600,
        )?;
    }
    verify_host_dependencies(architecture)?;
    let mut transaction = begin_transaction(staging, previous.is_some())?;
    let result = (|| {
        if previous.is_some() {
            prepare_remove()?;
        }
        publish_service(&manifest, &mut transaction)?;
        root_command("/usr/bin/systemctl", &["daemon-reload"])?;
        transaction.before_start()?;
        root_command(
            "/usr/bin/systemctl",
            &["enable", "--now", "usque-agent.service"],
        )?;
        transaction.commit()
    })();
    finish_transaction(result)
}

fn trusted_host_file(path: &Path, executable: bool) -> Result<File, Error> {
    // Standard system locations may contain distribution-owned symlinks (for
    // example /lib -> /usr/lib). Resolve first, then validate every owner/mode.
    let path = fs::canonicalize(path)?;
    let mut current = PathBuf::from("/");
    for component in path.components() {
        match component {
            std::path::Component::RootDir => continue,
            std::path::Component::Normal(name) => current.push(name),
            _ => return Err(Error::Dependencies),
        }
        let metadata = fs::symlink_metadata(&current)?;
        if metadata.uid() != 0 || metadata.mode() & 0o022 != 0 || metadata.is_symlink() {
            return Err(Error::Dependencies);
        }
        if current != path && !metadata.is_dir() {
            return Err(Error::Dependencies);
        }
    }
    let file = open_regular(&path)?;
    if executable && file.metadata()?.mode() & 0o111 == 0 {
        return Err(Error::Dependencies);
    }
    Ok(file)
}

fn matching_elf(header: &[u8; 20], machine: u16) -> bool {
    header[..4] == *b"\x7fELF"
        && header[4] == 2
        && header[5] == 1
        && u16::from_le_bytes([header[18], header[19]]) == machine
}

fn verify_host_dependencies(architecture: &str) -> Result<(), Error> {
    for path in ["/usr/sbin/nft", "/usr/bin/busctl", "/usr/bin/systemctl"] {
        trusted_host_file(Path::new(path), true).map_err(|_| Error::Dependencies)?;
    }
    let (multiarch, machine) = match architecture {
        "amd64" => ("x86_64-linux-gnu", 62),
        "arm64" => ("aarch64-linux-gnu", 183),
        _ => return Err(Error::Dependencies),
    };
    // The installed Engine must use host-owned libsecret, never the mounted
    // AppImage copy. This checks its presence/architecture without loading a
    // library or opening the root user's keyring. Session availability is
    // checked by the ordinary user's Engine and remains retryable there.
    for directory in [
        format!("/usr/lib/{multiarch}"),
        format!("/lib/{multiarch}"),
        "/usr/lib64".into(),
        "/lib64".into(),
        "/usr/lib".into(),
        "/lib".into(),
    ] {
        let Ok(mut library) =
            trusted_host_file(&Path::new(&directory).join("libsecret-1.so.0"), false)
        else {
            continue;
        };
        let mut header = [0; 20];
        if library.read_exact(&mut header).is_ok() && matching_elf(&header, machine) {
            return Ok(());
        }
    }
    Err(Error::Dependencies)
}

fn publish_service(
    manifest: &Manifest,
    transaction: &mut transaction::Transaction,
) -> Result<(), Error> {
    let staging = transaction.stage_path();
    for (index, entry) in manifest.entries.iter().enumerate() {
        let mode = if entry.name.starts_with("usr/lib/usque/") {
            0o755
        } else {
            0o644
        };
        transaction.replace(&entry.name, &staging.join(index.to_string()), mode)?;
    }
    let record = InstallRecord {
        schema_version: 1,
        version: manifest.version.clone(),
        protocol_version: PROTOCOL_VERSION,
        format: "appimage".into(),
        architecture: manifest.architecture.clone(),
        engine_sha256: manifest.entry("usr/lib/usque/usque-engine")?.sha256.clone(),
    };
    let path = staging.join("candidate-record");
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)?;
    file.write_all(&serde_json::to_vec(&record).map_err(|_| Error::Installation)?)?;
    file.sync_all()?;
    // Record and executables are all covered by the same durable transaction.
    transaction.replace(RECORD.trim_start_matches('/'), &path, 0o644)
}

fn replace_appimage(
    source: &Path,
    destination: &Path,
    entry: &Entry,
    update_service: bool,
) -> Result<(), Error> {
    // SAFETY: geteuid has no arguments or memory requirements.
    let uid = unsafe { libc::geteuid() };
    let parent = destination
        .parent()
        .filter(|path| path.is_absolute())
        .ok_or(Error::Arguments)?;
    let metadata = fs::symlink_metadata(parent)?;
    let old = fs::symlink_metadata(destination)?;
    if !metadata.is_dir()
        || metadata.uid() != uid
        || metadata.mode() & 0o022 != 0
        || !old.is_file()
        || old.uid() != uid
    {
        return Err(Error::Installation);
    }
    let staging = Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir_in(parent)?;
    let staged_path = staging.path().join(&entry.name);
    copy_verified(source, &staged_path, entry, 0o700)?;
    if update_service {
        let status = Command::new(&staged_path)
            .arg("--appimage-extract")
            .current_dir(staging.path())
            .stdout(Stdio::null())
            .status()?;
        if !status.success() {
            return Err(Error::Maintenance);
        }
    }
    fs::set_permissions(&staged_path, fs::Permissions::from_mode(0o755))?;
    let backup = staging.path().join("previous.AppImage");
    fs::hard_link(destination, &backup)?;
    fs::rename(&staged_path, destination)?;
    let result = (|| {
        // Confirm the user-writable replacement before touching the service.
        // A rename or fsync failure cannot leave a newer root service paired
        // with the older GUI. Cancellation restores the original AppImage.
        File::open(parent)?.sync_all()?;
        if update_service {
            let mut helper = Command::new("/usr/bin/pkexec")
                .arg(HELPER)
                .arg("--install-service")
                .arg("--bundle")
                .arg(staging.path().join("squashfs-root"))
                .spawn()?;
            let status = helper.wait().map_err(|_| Error::RecoveryRequired)?;
            service_update_result(status.code())?;
        }
        Ok(())
    })();
    finish_appimage_replacement(result, destination, &backup, parent, staging)
}

fn service_update_result(code: Option<i32>) -> Result<(), Error> {
    match code {
        Some(0) => Ok(()),
        // The helper uses 1 only before mutation or after confirmed rollback;
        // pkexec uses 126/127 when authorization prevents helper execution.
        Some(1) => Err(Error::Maintenance),
        Some(126 | 127) => Err(Error::Authorization),
        // Signals, interrupted waits, and unknown helpers are ambiguous. Keep
        // both AppImages until privileged recovery determines the service state.
        _ => Err(Error::RecoveryRequired),
    }
}

fn finish_appimage_replacement(
    result: Result<(), Error>,
    destination: &Path,
    backup: &Path,
    parent: &Path,
    staging: TempDir,
) -> Result<(), Error> {
    if matches!(result, Err(Error::RecoveryRequired)) {
        let _retained_staging = staging.keep();
        return result;
    }
    if result.is_err() {
        if fs::rename(backup, destination).is_err() || File::open(parent)?.sync_all().is_err() {
            let _retained_staging = staging.keep();
            return Err(Error::RecoveryRequired);
        }
        return result;
    }
    Ok(())
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("Linux maintenance arguments are invalid")]
    Arguments,
    #[error("Linux maintenance authorization was cancelled or denied; retry when ready")]
    Authorization,
    #[error("Linux installation state is unsafe or incompatible")]
    Installation,
    #[error("Linux maintenance did not finish; retained data permits retry")]
    Maintenance,
    #[error("Linux maintenance recovery is incomplete; protected recovery copies were retained")]
    RecoveryRequired,
    #[error(
        "USQUE_LINUX_DEPENDENCIES_MISSING: install systemd, nftables and the host libsecret-1 library with your system package manager, then retry; a Secret Service keyring is required in your login session"
    )]
    Dependencies,
    #[error(transparent)]
    Release(#[from] ReleaseError),
    #[error("Linux maintenance filesystem operation failed: {0}")]
    Io(#[from] io::Error),
}

impl Error {
    pub fn exit_code(&self) -> i32 {
        if matches!(self, Self::RecoveryRequired) {
            2
        } else {
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    #[test]
    fn staging_rejects_symlinks_and_tampering_without_touching_destination() {
        let root = TempDir::new().unwrap();
        let source = root.path().join("download");
        fs::write(&source, b"signed package").unwrap();
        let entry = Entry {
            name: "package.deb".into(),
            size: 14,
            sha256: Sha256::digest(b"signed package")
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
        };
        let staged = root.path().join("staged");
        copy_verified(&source, &staged, &entry, 0o600).unwrap();
        fs::write(&source, b"changed bytes!").unwrap();
        assert_eq!(fs::read(&staged).unwrap(), b"signed package");
        assert!(copy_verified(&source, &root.path().join("bad"), &entry, 0o600).is_err());
        let symlink = root.path().join("symlink");
        std::os::unix::fs::symlink(&source, &symlink).unwrap();
        assert!(open_regular(&symlink).is_err());
        assert!(open_regular(root.path()).is_err());
    }

    #[test]
    fn bounded_documents_and_required_operation_reject_invalid_inputs() {
        let root = TempDir::new().unwrap();
        let path = root.path().join("manifest");
        fs::write(&path, [0; 65]).unwrap();
        assert!(bounded_read(&path, 64).is_err());
        assert!(Arguments::try_parse_from(["usque-update"]).is_err());
        assert!(
            Arguments::try_parse_from(["usque-update", "--repair-service", "--disable-service"])
                .is_err()
        );
        assert!(
            Arguments::try_parse_from(["usque-update", "--prepare-remove", "--disable-service"])
                .is_ok()
        );
        assert!(
            Arguments::try_parse_from(["usque-update", "--install", "--remove-service"]).is_err()
        );
    }
    #[test]
    fn host_library_requires_64_bit_little_endian_elf_for_exact_architecture() {
        let mut header = [0; 20];
        header[..4].copy_from_slice(b"\x7fELF");
        header[4] = 2;
        header[5] = 1;
        header[18..20].copy_from_slice(&62u16.to_le_bytes());
        assert!(matching_elf(&header, 62));
        assert!(!matching_elf(&header, 183));
        header[4] = 1;
        assert!(!matching_elf(&header, 62));
        header[4] = 2;
        header[5] = 2;
        assert!(!matching_elf(&header, 62));
    }

    #[test]
    fn appimage_tracks_confirmed_commit_rollback_and_uncertain_service_results() {
        for code in [
            Some(0),
            Some(1),
            Some(126),
            Some(127),
            Some(2),
            Some(42),
            None,
        ] {
            let root = TempDir::new().unwrap();
            let destination = root.path().join("usque.AppImage");
            fs::write(&destination, b"new").unwrap();
            let staging = TempDir::new_in(root.path()).unwrap();
            let staging_path = staging.path().to_owned();
            let backup = staging.path().join("previous.AppImage");
            fs::write(&backup, b"old").unwrap();
            let result = finish_appimage_replacement(
                service_update_result(code),
                &destination,
                &backup,
                root.path(),
                staging,
            );
            match code {
                Some(0) => {
                    result.unwrap();
                    assert_eq!(fs::read(&destination).unwrap(), b"new");
                    assert!(!staging_path.exists());
                }
                Some(1 | 126 | 127) => {
                    assert_eq!(result.unwrap_err().exit_code(), 1);
                    assert_eq!(fs::read(&destination).unwrap(), b"old");
                    assert!(!staging_path.exists());
                }
                _ => {
                    assert_eq!(result.unwrap_err().exit_code(), 2);
                    assert_eq!(fs::read(&destination).unwrap(), b"new");
                    assert_eq!(fs::read(&backup).unwrap(), b"old");
                    assert!(staging_path.exists());
                }
            }
        }
    }
}
