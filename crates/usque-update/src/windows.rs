use std::{
    ffi::OsStr,
    fs::{self, File},
    io::{self, Read},
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    ptr,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use clap::{Parser, ValueEnum};
use sha2::{Digest, Sha256};
use thiserror::Error;
use usque_platform::windows_authenticode::{AuthenticodeError, verify_same_signer};
use windows_sys::Win32::{
    Foundation::{
        CloseHandle, ERROR_MORE_DATA, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS,
        ERROR_SUCCESS_REBOOT_INITIATED, ERROR_SUCCESS_REBOOT_REQUIRED, HANDLE, WAIT_FAILED,
    },
    Storage::FileSystem::SYNCHRONIZE,
    System::{
        ApplicationInstallationAndServicing::{
            MSIDBOPEN_READONLY, MSIHANDLE, MsiCloseHandle, MsiDatabaseOpenViewW,
            MsiGetSummaryInformationW, MsiOpenDatabaseW, MsiRecordGetStringW,
            MsiSummaryInfoGetPropertyW, MsiViewExecute, MsiViewFetch, PID_TEMPLATE,
        },
        SystemInformation::GetSystemDirectoryW,
        Threading::{
            INFINITE, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, WaitForSingleObject,
        },
    },
    UI::WindowsAndMessaging::{MB_ICONERROR, MB_ICONINFORMATION, MB_OK, MessageBoxW},
};

const UPGRADE_CODE: &str = "{076CF387-E447-4666-9153-2DA16049A390}";
const MAX_PACKAGE_SIZE: u64 = 512 * 1024 * 1024;
const STALE_TEMP_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Variant {
    #[value(name = "x64-v2")]
    X64V2,
    #[value(name = "arm64")]
    Arm64,
}

impl Variant {
    fn as_str(self) -> &'static str {
        match self {
            Self::X64V2 => "x64-v2",
            Self::Arm64 => "arm64",
        }
    }

    fn msi_template(self) -> &'static str {
        match self {
            Self::X64V2 => "x64",
            Self::Arm64 => "Arm64",
        }
    }
}

#[derive(Debug, Parser)]
#[command(disable_help_subcommand = true)]
struct Arguments {
    #[arg(long, conflicts_with_all = ["install", "run_install"])]
    verify_only: bool,
    #[arg(long, conflicts_with_all = ["verify_only", "run_install"])]
    install: bool,
    #[arg(long, hide = true, conflicts_with_all = ["verify_only", "install"])]
    run_install: bool,
    #[arg(long)]
    package: PathBuf,
    #[arg(long)]
    version: String,
    #[arg(long)]
    expected_name: String,
    #[arg(long)]
    expected_size: u64,
    #[arg(long)]
    expected_sha256: String,
    #[arg(long, value_enum)]
    variant: Variant,
    #[arg(long)]
    parent_pid: Option<u32>,
    #[arg(long, hide = true, requires = "run_install")]
    gui_path: Option<PathBuf>,
}

pub fn run() -> Result<i32, UpdateError> {
    let arguments = Arguments::parse();
    if !arguments.verify_only && !arguments.install && !arguments.run_install {
        return Err(UpdateError::InvalidArgument(
            "one update operation must be selected".to_owned(),
        ));
    }
    let helper = fs::canonicalize(std::env::current_exe()?)?;
    let metadata = ExpectedPackage::from_arguments(&arguments)?;
    if arguments.verify_only {
        verify_package(&helper, &arguments.package, &metadata, true)?;
        return Ok(0);
    }
    if arguments.install {
        let result = begin_install(&helper, &arguments, &metadata);
        if result.is_err() {
            remove_package(&arguments.package);
        }
        return result.map(|()| 0);
    }
    let result = complete_install(&helper, &arguments, &metadata);
    if result.is_err() {
        remove_package(&arguments.package);
        show_message(
            "Usque update failed",
            "The update could not be completed. The previous Usque version will be opened again.",
            true,
        );
        if let Some(gui_path) = arguments.gui_path.as_deref() {
            let _ = restart_gui(&helper, gui_path);
        }
    }
    result
}

#[derive(Debug)]
struct ExpectedPackage {
    product_version: String,
    name: String,
    size: u64,
    sha256: String,
    variant: Variant,
}

impl ExpectedPackage {
    fn from_arguments(arguments: &Arguments) -> Result<Self, UpdateError> {
        let (version, version_parts) = stable_version(&arguments.version).ok_or_else(|| {
            UpdateError::InvalidArgument(
                "the update version must be a stable three-part numeric version".to_owned(),
            )
        })?;
        let (_, current_parts) = stable_version(env!("CARGO_PKG_VERSION")).ok_or_else(|| {
            UpdateError::InvalidArgument("the installed Usque version is invalid".to_owned())
        })?;
        if version_parts <= current_parts {
            return Err(UpdateError::InvalidArgument(
                "the update version must be newer than the installed Usque version".to_owned(),
            ));
        }
        let product_version = msi_product_version(version_parts)?;
        let expected_name = format!(
            "usque-v{version}-windows-{}.msi",
            arguments.variant.as_str()
        );
        if arguments.expected_name != expected_name
            || arguments.expected_size == 0
            || arguments.expected_size > MAX_PACKAGE_SIZE
            || !is_sha256(&arguments.expected_sha256)
        {
            return Err(UpdateError::InvalidArgument(
                "the expected update package metadata is invalid".to_owned(),
            ));
        }
        Ok(Self {
            product_version,
            name: expected_name,
            size: arguments.expected_size,
            sha256: arguments.expected_sha256.to_ascii_lowercase(),
            variant: arguments.variant,
        })
    }
}

fn begin_install(
    helper: &Path,
    arguments: &Arguments,
    expected: &ExpectedPackage,
) -> Result<(), UpdateError> {
    let parent_pid = arguments.parent_pid.ok_or_else(|| {
        UpdateError::InvalidArgument("--parent-pid is required for --install".to_owned())
    })?;
    verify_package(helper, &arguments.package, expected, false)?;
    cleanup_stale_helpers();
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let directory = std::env::temp_dir().join(format!("UsqueUpdate-{parent_pid}-{unique}"));
    fs::create_dir(&directory)?;
    let copied_helper = directory.join("usque-update.exe");
    fs::copy(helper, &copied_helper)?;
    verify_same_signer(helper, &copied_helper)?;
    let gui_path = helper
        .parent()
        .ok_or_else(|| {
            UpdateError::InvalidArgument("the installed helper has no parent".to_owned())
        })?
        .join("usque.exe");
    let mut command = Command::new(&copied_helper);
    command
        .arg("--run-install")
        .arg("--parent-pid")
        .arg(parent_pid.to_string())
        .arg("--gui-path")
        .arg(gui_path)
        .args(package_arguments(arguments))
        // The installed helper is launched by Dart with captured stdout/stderr.
        // Do not let the long-running handoff inherit those pipe write handles,
        // otherwise Process.run waits for EOF while this child waits for the GUI.
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command.spawn()?;
    Ok(())
}

fn complete_install(
    helper: &Path,
    arguments: &Arguments,
    expected: &ExpectedPackage,
) -> Result<i32, UpdateError> {
    let parent_pid = arguments.parent_pid.ok_or_else(|| {
        UpdateError::InvalidArgument("--parent-pid is required for installation".to_owned())
    })?;
    let gui_path = arguments.gui_path.as_deref().ok_or_else(|| {
        UpdateError::InvalidArgument("--gui-path is required for installation".to_owned())
    })?;
    wait_for_process(parent_pid)?;
    verify_package(helper, &arguments.package, expected, false)?;
    let msiexec = system_msiexec_path()?;
    let status = Command::new(msiexec)
        .arg("/i")
        .arg(&arguments.package)
        .arg("/passive")
        .arg("/norestart")
        .status();
    remove_package(&arguments.package);
    let code = status?.code().unwrap_or(1) as u32;
    match classify_install_exit(code) {
        InstallOutcome::Success => {
            restart_gui(helper, gui_path)?;
            Ok(0)
        }
        InstallOutcome::RebootRequired => {
            show_message(
                "Usque update",
                "The update was installed. Windows must restart before Usque can be opened again.",
                false,
            );
            Ok(code as i32)
        }
        InstallOutcome::Cancelled => {
            show_message(
                "Usque update cancelled",
                "The update was cancelled. The previous Usque version will be opened again.",
                true,
            );
            restart_gui(helper, gui_path)?;
            Ok(code as i32)
        }
        InstallOutcome::Failed(failure) => {
            show_message(
                "Usque update failed",
                &format!(
                    "Windows Installer returned exit code {failure}. The previous Usque version will be opened again."
                ),
                true,
            );
            restart_gui(helper, gui_path)?;
            Ok(failure as i32)
        }
    }
}

fn system_msiexec_path() -> Result<PathBuf, UpdateError> {
    let mut buffer = vec![0_u16; 32_768];
    // SAFETY: the buffer is writable for the exact capacity passed to Kernel32.
    let length = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) };
    if length == 0 {
        return Err(UpdateError::Windows(
            "GetSystemDirectoryW",
            io::Error::last_os_error(),
        ));
    }
    if length as usize >= buffer.len() {
        return Err(UpdateError::PackageIdentity(
            "the Windows system directory exceeded its safe bound".to_owned(),
        ));
    }
    buffer.truncate(length as usize);
    let directory =
        PathBuf::from(String::from_utf16(&buffer).map_err(|_| UpdateError::InvalidWindowsPath)?);
    let canonical_directory = directory.canonicalize()?;
    let candidate = canonical_directory.join("msiexec.exe");
    let canonical_candidate = candidate.canonicalize()?;
    if !canonical_candidate.is_file()
        || canonical_candidate.parent() != Some(canonical_directory.as_path())
        || !canonical_candidate
            .file_name()
            .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case("msiexec.exe"))
    {
        return Err(UpdateError::PackageIdentity(
            "Windows returned an invalid system msiexec path".to_owned(),
        ));
    }
    Ok(canonical_candidate)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InstallOutcome {
    Success,
    RebootRequired,
    Cancelled,
    Failed(u32),
}

fn classify_install_exit(code: u32) -> InstallOutcome {
    match code {
        ERROR_SUCCESS => InstallOutcome::Success,
        ERROR_SUCCESS_REBOOT_REQUIRED | ERROR_SUCCESS_REBOOT_INITIATED => {
            InstallOutcome::RebootRequired
        }
        1602 => InstallOutcome::Cancelled,
        failure => InstallOutcome::Failed(failure),
    }
}

fn verify_package(
    helper: &Path,
    package: &Path,
    expected: &ExpectedPackage,
    allow_partial: bool,
) -> Result<(), UpdateError> {
    let root = update_cache_directory()?.canonicalize()?;
    let package = package.canonicalize()?;
    let expected_file_name = if allow_partial {
        format!("{}.part", expected.name)
    } else {
        expected.name.clone()
    };
    if package.parent() != Some(root.as_path())
        || package.file_name() != Some(OsStr::new(&expected_file_name))
    {
        return Err(UpdateError::PackageIdentity(
            "the MSI is outside Usque's private update cache".to_owned(),
        ));
    }
    if package.metadata()?.len() != expected.size {
        return Err(UpdateError::PackageIdentity(
            "the MSI size does not match the release manifest".to_owned(),
        ));
    }
    if file_sha256(&package)? != expected.sha256 {
        return Err(UpdateError::PackageIdentity(
            "the MSI SHA-256 digest does not match the release manifest".to_owned(),
        ));
    }
    verify_same_signer(helper, &package)?;
    let identity = MsiIdentity::read(&package)?;
    if !identity.upgrade_code.eq_ignore_ascii_case(UPGRADE_CODE)
        || identity.product_version != expected.product_version
        || identity.variant != expected.variant.as_str()
        || !identity
            .template
            .eq_ignore_ascii_case(expected.variant.msi_template())
    {
        return Err(UpdateError::PackageIdentity(
            "the MSI product identity, version, architecture, or variant is invalid".to_owned(),
        ));
    }
    Ok(())
}

#[derive(Debug)]
struct MsiIdentity {
    upgrade_code: String,
    product_version: String,
    variant: String,
    template: String,
}

impl MsiIdentity {
    fn read(path: &Path) -> Result<Self, UpdateError> {
        let path = wide(path.as_os_str());
        let mut database = 0;
        // SAFETY: the path is null-terminated and database is writable.
        let status = unsafe { MsiOpenDatabaseW(path.as_ptr(), MSIDBOPEN_READONLY, &mut database) };
        check_msi(status, "MsiOpenDatabaseW")?;
        let database = MsiHandle(database);
        let upgrade_code = msi_property(database.0, "UpgradeCode")?;
        let product_version = msi_property(database.0, "ProductVersion")?;
        let variant = msi_property(database.0, "USQUE_UPDATE_VARIANT")?;
        let template = msi_template(database.0)?;
        Ok(Self {
            upgrade_code,
            product_version,
            variant,
            template: template.split(';').next().unwrap_or_default().to_owned(),
        })
    }
}

fn msi_property(database: MSIHANDLE, property: &str) -> Result<String, UpdateError> {
    let query = wide(OsStr::new(&format!(
        "SELECT `Value` FROM `Property` WHERE `Property`='{property}'"
    )));
    let mut view = 0;
    // SAFETY: database is open, query is null-terminated, and view is writable.
    let status = unsafe { MsiDatabaseOpenViewW(database, query.as_ptr(), &mut view) };
    check_msi(status, "MsiDatabaseOpenViewW")?;
    let view = MsiHandle(view);
    // SAFETY: the query has no parameter record.
    check_msi(unsafe { MsiViewExecute(view.0, 0) }, "MsiViewExecute")?;
    let mut record = 0;
    // SAFETY: record is writable and view has executed.
    let status = unsafe { MsiViewFetch(view.0, &mut record) };
    if status == ERROR_NO_MORE_ITEMS {
        return Err(UpdateError::PackageIdentity(format!(
            "the MSI property {property} is missing"
        )));
    }
    check_msi(status, "MsiViewFetch")?;
    let record = MsiHandle(record);
    msi_record_string(record.0, 1)
}

fn msi_record_string(record: MSIHANDLE, field: u32) -> Result<String, UpdateError> {
    let mut buffer = vec![0_u16; 1024];
    let mut length = (buffer.len() - 1) as u32;
    // SAFETY: buffer and length are writable and record is open.
    let status = unsafe { MsiRecordGetStringW(record, field, buffer.as_mut_ptr(), &mut length) };
    check_msi(status, "MsiRecordGetStringW")?;
    buffer.truncate(length as usize);
    String::from_utf16(&buffer).map_err(|_| UpdateError::InvalidMsiText)
}

fn msi_template(database: MSIHANDLE) -> Result<String, UpdateError> {
    let mut summary = 0;
    // SAFETY: database is open and summary is writable.
    let status = unsafe { MsiGetSummaryInformationW(database, ptr::null(), 0, &mut summary) };
    check_msi(status, "MsiGetSummaryInformationW")?;
    let summary = MsiHandle(summary);
    let mut data_type = 0;
    let mut integer = 0;
    let mut file_time = windows_sys::Win32::Foundation::FILETIME::default();
    let mut buffer = vec![0_u16; 128];
    let mut length = (buffer.len() - 1) as u32;
    // SAFETY: all output values and the text buffer are writable.
    let status = unsafe {
        MsiSummaryInfoGetPropertyW(
            summary.0,
            PID_TEMPLATE,
            &mut data_type,
            &mut integer,
            &mut file_time,
            buffer.as_mut_ptr(),
            &mut length,
        )
    };
    check_msi(status, "MsiSummaryInfoGetPropertyW")?;
    buffer.truncate(length as usize);
    String::from_utf16(&buffer).map_err(|_| UpdateError::InvalidMsiText)
}

fn check_msi(status: u32, operation: &'static str) -> Result<(), UpdateError> {
    if status == ERROR_SUCCESS {
        return Ok(());
    }
    if status == ERROR_MORE_DATA {
        return Err(UpdateError::PackageIdentity(
            "an MSI metadata value exceeded its safe bound".to_owned(),
        ));
    }
    Err(UpdateError::WindowsInstaller(operation, status))
}

fn wait_for_process(pid: u32) -> Result<(), UpdateError> {
    // SAFETY: requested access is read/wait-only and the returned handle is owned.
    let handle = unsafe { OpenProcess(SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        return Err(UpdateError::Windows(
            "OpenProcess",
            io::Error::last_os_error(),
        ));
    }
    let handle = OwnedHandle(handle);
    // SAFETY: handle remains live and INFINITE has the documented meaning.
    let status = unsafe { WaitForSingleObject(handle.0, INFINITE) };
    if status == WAIT_FAILED {
        return Err(UpdateError::Windows(
            "WaitForSingleObject",
            io::Error::last_os_error(),
        ));
    }
    Ok(())
}

fn restart_gui(helper: &Path, gui_path: &Path) -> Result<(), UpdateError> {
    verify_same_signer(helper, gui_path)?;
    Command::new(gui_path).spawn()?;
    Ok(())
}

fn update_cache_directory() -> Result<PathBuf, UpdateError> {
    let local = std::env::var_os("LOCALAPPDATA").ok_or(UpdateError::MissingLocalAppData)?;
    Ok(PathBuf::from(local).join("Usque").join("updates"))
}

fn package_arguments(arguments: &Arguments) -> Vec<String> {
    vec![
        "--package".to_owned(),
        arguments.package.to_string_lossy().into_owned(),
        "--version".to_owned(),
        arguments.version.clone(),
        "--expected-name".to_owned(),
        arguments.expected_name.clone(),
        "--expected-size".to_owned(),
        arguments.expected_size.to_string(),
        "--expected-sha256".to_owned(),
        arguments.expected_sha256.clone(),
        "--variant".to_owned(),
        arguments.variant.as_str().to_owned(),
    ]
}

fn file_sha256(path: &Path) -> Result<String, UpdateError> {
    let mut file = File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn stable_version(value: &str) -> Option<(&str, (u64, u64, u64))> {
    let value = value.strip_prefix('v').unwrap_or(value);
    let parts = value.split('.').collect::<Vec<_>>();
    if parts.len() != 3
        || !parts.iter().all(|part| {
            !part.is_empty()
                && part.bytes().all(|byte| byte.is_ascii_digit())
                && (part == &"0" || !part.starts_with('0'))
        })
    {
        return None;
    }
    Some((
        value,
        (
            parts[0].parse().ok()?,
            parts[1].parse().ok()?,
            parts[2].parse().ok()?,
        ),
    ))
}

fn msi_product_version(parts: (u64, u64, u64)) -> Result<String, UpdateError> {
    let (major, minor, patch) = parts;
    let build = patch
        .checked_mul(100)
        .and_then(|value| value.checked_add(99))
        .filter(|value| *value <= 65_535)
        .ok_or_else(|| {
            UpdateError::InvalidArgument("the update version exceeds MSI limits".to_owned())
        })?;
    if major > 255 || minor > 255 {
        return Err(UpdateError::InvalidArgument(
            "the update version exceeds MSI limits".to_owned(),
        ));
    }
    Ok(format!("{major}.{minor}.{build}"))
}

fn remove_package(path: &Path) {
    if let Ok(root) =
        update_cache_directory().and_then(|path| path.canonicalize().map_err(Into::into))
        && let Ok(package) = path.canonicalize()
        && package.parent() == Some(root.as_path())
    {
        let _ = fs::remove_file(package);
    }
}

fn cleanup_stale_helpers() {
    let cutoff = SystemTime::now()
        .checked_sub(STALE_TEMP_AGE)
        .unwrap_or(UNIX_EPOCH);
    let Ok(entries) = fs::read_dir(std::env::temp_dir()) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let stale = entry
            .file_name()
            .to_string_lossy()
            .starts_with("UsqueUpdate-")
            && entry
                .metadata()
                .and_then(|metadata| metadata.modified())
                .is_ok_and(|modified| modified < cutoff);
        if stale {
            let _ = fs::remove_dir_all(path);
        }
    }
}

fn show_message(title: &str, message: &str, error: bool) {
    let title = wide(OsStr::new(title));
    let message = wide(OsStr::new(message));
    let icon = if error {
        MB_ICONERROR
    } else {
        MB_ICONINFORMATION
    };
    // SAFETY: both strings are null-terminated and no owner window is required.
    unsafe {
        MessageBoxW(
            ptr::null_mut(),
            message.as_ptr(),
            title.as_ptr(),
            MB_OK | icon,
        )
    };
}

fn wide(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(std::iter::once(0)).collect()
}

struct MsiHandle(MSIHANDLE);

impl Drop for MsiHandle {
    fn drop(&mut self) {
        if self.0 != 0 {
            // SAFETY: this wrapper uniquely owns a Windows Installer handle.
            unsafe { MsiCloseHandle(self.0) };
        }
    }
}

struct OwnedHandle(HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: this wrapper uniquely owns the process handle.
            unsafe { CloseHandle(self.0) };
        }
    }
}

#[derive(Debug, Error)]
pub enum UpdateError {
    #[error("invalid update command: {0}")]
    InvalidArgument(String),
    #[error("update package verification failed: {0}")]
    PackageIdentity(String),
    #[error("LOCALAPPDATA is unavailable")]
    MissingLocalAppData,
    #[error("MSI metadata is not valid UTF-16")]
    InvalidMsiText,
    #[error("Windows returned a non-UTF-16 system path")]
    InvalidWindowsPath,
    #[error("Windows Installer {0} failed with code {1}")]
    WindowsInstaller(&'static str, u32),
    #[error("Windows {0} failed: {1}")]
    Windows(&'static str, io::Error),
    #[error(transparent)]
    Authenticode(#[from] AuthenticodeError),
    #[error(transparent)]
    Io(#[from] io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_stable_three_part_versions() {
        assert_eq!(stable_version("0.2.2"), Some(("0.2.2", (0, 2, 2))));
        assert_eq!(
            stable_version("v10.20.30"),
            Some(("10.20.30", (10, 20, 30)))
        );
        assert!(stable_version("0.2.2-beta.1").is_none());
        assert!(stable_version("0.2").is_none());
        assert!(stable_version("00.2.2").is_none());
        assert_eq!(msi_product_version((0, 2, 2)).unwrap(), "0.2.299");
    }

    #[test]
    fn digest_validation_is_exact() {
        assert!(is_sha256(&"a5".repeat(32)));
        assert!(!is_sha256(&"a5".repeat(31)));
        assert!(!is_sha256(&"zz".repeat(32)));
    }

    #[test]
    fn installer_exit_codes_preserve_restart_and_reboot_contract() {
        assert_eq!(classify_install_exit(0), InstallOutcome::Success);
        assert_eq!(classify_install_exit(1602), InstallOutcome::Cancelled);
        assert_eq!(classify_install_exit(3010), InstallOutcome::RebootRequired);
        assert_eq!(classify_install_exit(1641), InstallOutcome::RebootRequired);
        assert_eq!(classify_install_exit(1603), InstallOutcome::Failed(1603));
    }

    #[test]
    fn installer_path_is_the_absolute_windows_system_binary() {
        let path = system_msiexec_path().expect("system msiexec");
        assert!(path.is_absolute());
        assert!(
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case("msiexec.exe"))
        );
    }
}
