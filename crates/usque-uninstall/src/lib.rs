//! Confirmation helper launched from the Windows Apps uninstall entry.
//!
//! Settings does not show the MSI wizard. This binary owns the interactive
//! confirm-and-optional-data-deletion prompt, runs `msiexec`, and then removes
//! any hidden Burn registration that delivered the MSI.

use thiserror::Error;

mod l10n;

#[cfg(windows)]
mod windows;

/// MSI `ERROR_INSTALL_USEREXIT`. Settings should keep the app listed.
pub const ERROR_INSTALL_USEREXIT: i32 = 1602;

const TEMP_COPY_PREFIX: &str = "UsqueUninstall-";
const HELPER_FILE_NAME: &str = "usque-uninstall.exe";

#[derive(Debug, Error)]
pub enum UninstallError {
    #[error("usque-uninstall can only show the confirmation dialog on Windows")]
    WindowsOnly,
    #[error("the Usque product code is missing; pass --product-code or install the MSI")]
    MissingProductCode,
    #[error("invalid product code {0:?}")]
    InvalidProductCode(String),
    #[error("invalid process id {0:?}")]
    InvalidProcessId(String),
    #[error("uninstall, dry-run, and quiet-copy staging/verification modes cannot be combined")]
    ConflictingArguments,
    #[error(
        "quiet uninstall must run from the verified temporary helper; use the registered QuietUninstallString"
    )]
    InvalidExecutionContext,
    #[error("unknown argument {0:?}")]
    UnknownArgument(String),
    #[error("{0}")]
    Detail(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Interactive,
    Quiet,
    DryRun,
    StageQuiet(u32),
    VerifyQuiet(u32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cli {
    pub mode: Mode,
    pub product_code: Option<String>,
    pub remove_user_data: bool,
    pub wait_for_pid: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UninstallRequest {
    pub product_code: String,
    pub remove_user_data: bool,
}

impl Cli {
    pub fn parse<I, S>(arguments: I) -> Result<Self, UninstallError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut dry_run = false;
        let mut quiet = false;
        let mut product_code = None;
        let mut remove_user_data = false;
        let mut waiting_for_product_code = false;
        let mut wait_for_pid = None;
        let mut waiting_for_pid = false;
        let mut quiet_copy_mode = None;

        for argument in arguments {
            let argument = argument.as_ref();
            if waiting_for_product_code {
                product_code = Some(normalize_product_code(argument)?);
                waiting_for_product_code = false;
                continue;
            }
            if waiting_for_pid {
                wait_for_pid = Some(normalize_process_id(argument)?);
                waiting_for_pid = false;
                continue;
            }
            match argument {
                "--dry-run" => dry_run = true,
                "--quiet" => quiet = true,
                "--remove-user-data" => remove_user_data = true,
                "--product-code" => waiting_for_product_code = true,
                "--wait-for-pid" => waiting_for_pid = true,
                value if let Some(pid) = value.strip_prefix("--stage-quiet=") => {
                    if quiet_copy_mode
                        .replace(Mode::StageQuiet(normalize_process_id(pid)?))
                        .is_some()
                    {
                        return Err(UninstallError::ConflictingArguments);
                    }
                }
                value if let Some(pid) = value.strip_prefix("--verify-quiet=") => {
                    if quiet_copy_mode
                        .replace(Mode::VerifyQuiet(normalize_process_id(pid)?))
                        .is_some()
                    {
                        return Err(UninstallError::ConflictingArguments);
                    }
                }
                value if let Some(code) = value.strip_prefix("--product-code=") => {
                    product_code = Some(normalize_product_code(code)?);
                }
                value if let Some(pid) = value.strip_prefix("--wait-for-pid=") => {
                    wait_for_pid = Some(normalize_process_id(pid)?);
                }
                value => return Err(UninstallError::UnknownArgument(value.to_owned())),
            }
        }
        if waiting_for_product_code {
            return Err(UninstallError::InvalidProductCode(String::new()));
        }
        if waiting_for_pid {
            return Err(UninstallError::InvalidProcessId(String::new()));
        }
        if dry_run && (quiet || wait_for_pid.is_some()) {
            return Err(UninstallError::ConflictingArguments);
        }
        if quiet_copy_mode.is_some()
            && (dry_run
                || quiet
                || product_code.is_some()
                || remove_user_data
                || wait_for_pid.is_some())
        {
            return Err(UninstallError::ConflictingArguments);
        }
        let mode = quiet_copy_mode.unwrap_or(match (dry_run, quiet) {
            (true, _) => Mode::DryRun,
            (false, true) => Mode::Quiet,
            (false, false) => Mode::Interactive,
        });
        Ok(Self {
            mode,
            product_code,
            remove_user_data,
            wait_for_pid,
        })
    }
}

impl UninstallRequest {
    pub fn command_line(&self) -> String {
        format!(
            "msiexec /x {} USQUE_REMOVE_USER_DATA={} /qb /norestart",
            self.product_code,
            if self.remove_user_data { "1" } else { "0" }
        )
    }

    pub fn arguments(&self, quiet: bool) -> Vec<String> {
        vec![
            "/x".to_owned(),
            self.product_code.clone(),
            format!(
                "USQUE_REMOVE_USER_DATA={}",
                if self.remove_user_data { "1" } else { "0" }
            ),
            if quiet { "/qn" } else { "/qb" }.to_owned(),
            "/norestart".to_owned(),
        ]
    }
}

pub fn normalize_product_code(value: &str) -> Result<String, UninstallError> {
    let trimmed = value.trim();
    let body = trimmed
        .strip_prefix('{')
        .and_then(|item| item.strip_suffix('}'))
        .unwrap_or(trimmed);
    if !is_guid_body(body) {
        return Err(UninstallError::InvalidProductCode(trimmed.to_owned()));
    }
    Ok(format!("{{{}}}", body.to_ascii_uppercase()))
}

fn normalize_process_id(value: &str) -> Result<u32, UninstallError> {
    let value = value.trim();
    let process_id = value
        .parse::<u32>()
        .map_err(|_| UninstallError::InvalidProcessId(value.to_owned()))?;
    if process_id == 0 {
        return Err(UninstallError::InvalidProcessId(value.to_owned()));
    }
    Ok(process_id)
}

pub fn is_temp_relaunch_path(current_exe: &std::path::Path, temp_root: &std::path::Path) -> bool {
    let parent = current_exe.parent();
    let file_name = current_exe.file_name().and_then(|name| name.to_str());
    let folder = parent
        .and_then(|path| path.file_name())
        .and_then(|name| name.to_str());
    let Some(parent) = parent else {
        return false;
    };
    file_name == Some(HELPER_FILE_NAME)
        && folder.is_some_and(|name| {
            name.strip_prefix(TEMP_COPY_PREFIX).is_some_and(|rest| {
                !rest.is_empty() && rest.bytes().all(|byte| byte.is_ascii_digit())
            })
        })
        && current_exe.starts_with(temp_root)
        && parent.starts_with(temp_root)
}

pub fn temp_relaunch_path(temp_root: &std::path::Path, pid: u32) -> std::path::PathBuf {
    temp_root
        .join(format!("{TEMP_COPY_PREFIX}{pid}"))
        .join(HELPER_FILE_NAME)
}

pub fn resolve_product_code(
    explicit: Option<String>,
    installed: impl FnOnce() -> Result<String, UninstallError>,
) -> Result<String, UninstallError> {
    if let Some(code) = explicit {
        return normalize_product_code(&code);
    }
    normalize_product_code(&installed()?)
}

pub fn run<I, S>(arguments: I) -> Result<i32, UninstallError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let cli = Cli::parse(arguments)?;
    match cli.mode {
        Mode::DryRun => {
            attach_parent_console();
            let product_code = resolve_product_code(cli.product_code, read_installed_product_code)?;
            let request = UninstallRequest {
                product_code,
                remove_user_data: cli.remove_user_data,
            };
            println!("{}", request.command_line());
            Ok(0)
        }
        Mode::Interactive => run_interactive(cli.product_code, cli.wait_for_pid),
        Mode::Quiet => run_quiet(cli.product_code, cli.remove_user_data, cli.wait_for_pid),
        Mode::StageQuiet(pid) => prepare_quiet_copy(pid, false),
        Mode::VerifyQuiet(pid) => prepare_quiet_copy(pid, true),
    }
}

pub fn emit_error(error: &UninstallError, show_dialog: bool) {
    attach_parent_console();
    eprintln!("{error}");
    #[cfg(windows)]
    if show_dialog {
        windows::show_error_message(error);
    }
    #[cfg(not(windows))]
    {
        let _ = show_dialog;
    }
}

fn run_interactive(
    product_code: Option<String>,
    wait_for_pid: Option<u32>,
) -> Result<i32, UninstallError> {
    #[cfg(windows)]
    {
        windows::run_interactive(product_code, wait_for_pid)
    }
    #[cfg(not(windows))]
    {
        let _ = (product_code, wait_for_pid);
        Err(UninstallError::WindowsOnly)
    }
}

fn run_quiet(
    product_code: Option<String>,
    remove_user_data: bool,
    wait_for_pid: Option<u32>,
) -> Result<i32, UninstallError> {
    #[cfg(windows)]
    {
        windows::run_quiet(product_code, remove_user_data, wait_for_pid)
    }
    #[cfg(not(windows))]
    {
        let _ = (product_code, remove_user_data, wait_for_pid);
        Err(UninstallError::WindowsOnly)
    }
}

fn prepare_quiet_copy(pid: u32, verify_only: bool) -> Result<i32, UninstallError> {
    #[cfg(windows)]
    {
        windows::prepare_quiet_copy(pid, verify_only)
    }
    #[cfg(not(windows))]
    {
        let _ = (pid, verify_only);
        Err(UninstallError::WindowsOnly)
    }
}

fn read_installed_product_code() -> Result<String, UninstallError> {
    #[cfg(windows)]
    {
        windows::read_installed_product_code()
    }
    #[cfg(not(windows))]
    {
        Err(UninstallError::MissingProductCode)
    }
}

fn attach_parent_console() {
    #[cfg(windows)]
    {
        windows::attach_parent_console();
    }
}

fn is_guid_body(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 36
        && bytes[8] == b'-'
        && bytes[13] == b'-'
        && bytes[18] == b'-'
        && bytes[23] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| matches!(index, 8 | 13 | 18 | 23) || byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn dry_run_command_preserves_user_data_by_default() {
        let cli = Cli::parse([
            "--dry-run",
            "--product-code",
            "{076cf387-e447-4666-9153-2da16049a390}",
        ])
        .expect("parse");
        assert_eq!(cli.mode, Mode::DryRun);
        assert!(!cli.remove_user_data);
        let request = UninstallRequest {
            product_code: cli.product_code.expect("code"),
            remove_user_data: cli.remove_user_data,
        };
        assert_eq!(
            request.command_line(),
            "msiexec /x {076CF387-E447-4666-9153-2DA16049A390} USQUE_REMOVE_USER_DATA=0 /qb /norestart"
        );
        assert!(request.command_line().contains("/qb"));
        assert!(!request.command_line().contains("USQUE_REMOVE_USER_DATA=1"));
    }

    #[test]
    fn dry_run_command_can_request_user_data_removal() {
        let cli = Cli::parse([
            "--dry-run",
            "--remove-user-data",
            "--product-code=076cf387-e447-4666-9153-2da16049a390",
        ])
        .expect("parse");
        let request = UninstallRequest {
            product_code: cli.product_code.expect("code"),
            remove_user_data: cli.remove_user_data,
        };
        assert_eq!(
            request.command_line(),
            "msiexec /x {076CF387-E447-4666-9153-2DA16049A390} USQUE_REMOVE_USER_DATA=1 /qb /norestart"
        );
    }

    #[test]
    fn missing_product_code_fails_resolution() {
        let error = resolve_product_code(None, || Err(UninstallError::MissingProductCode))
            .expect_err("missing");
        assert!(matches!(error, UninstallError::MissingProductCode));
    }

    #[test]
    fn invalid_product_code_is_rejected() {
        assert!(Cli::parse(["--product-code", "not-a-guid"]).is_err());
    }

    #[test]
    fn quiet_mode_and_parent_wait_are_explicit() {
        let quiet = Cli::parse(["--quiet"]).expect("quiet");
        assert_eq!(quiet.mode, Mode::Quiet);
        let child = Cli::parse([
            "--quiet",
            "--wait-for-pid=42",
            "--product-code={076CF387-E447-4666-9153-2DA16049A390}",
        ])
        .expect("child");
        assert_eq!(child.mode, Mode::Quiet);
        assert_eq!(child.wait_for_pid, Some(42));
    }

    #[test]
    fn dry_run_rejects_child_flags_and_invalid_process_ids() {
        assert!(matches!(
            Cli::parse(["--dry-run", "--quiet"]),
            Err(UninstallError::ConflictingArguments)
        ));
        assert!(matches!(
            Cli::parse(["--wait-for-pid", "0"]),
            Err(UninstallError::InvalidProcessId(_))
        ));
        assert!(Cli::parse(["--unknown"]).is_err());
    }

    #[test]
    fn quiet_copy_modes_cannot_start_an_uninstall() {
        assert_eq!(
            Cli::parse(["--stage-quiet=42"]).expect("stage").mode,
            Mode::StageQuiet(42)
        );
        assert_eq!(
            Cli::parse(["--verify-quiet=42"]).expect("verify").mode,
            Mode::VerifyQuiet(42)
        );
        for flag in ["--stage-quiet=42", "--verify-quiet=42"] {
            for conflicting in [
                "--quiet",
                "--dry-run",
                "--remove-user-data",
                "--wait-for-pid=43",
                "--stage-quiet=43",
            ] {
                assert!(matches!(
                    Cli::parse([flag, conflicting]),
                    Err(UninstallError::ConflictingArguments)
                ));
            }
        }
        assert!(Cli::parse(["--stage-quiet=0"]).is_err());
        assert!(Cli::parse(["--verify-quiet=invalid"]).is_err());
    }

    #[test]
    fn temp_copy_paths_are_detected() {
        let temp = Path::new(r"C:\Users\Public\AppData\Local\Temp");
        let copy = temp.join("UsqueUninstall-4242").join("usque-uninstall.exe");
        assert!(is_temp_relaunch_path(&copy, temp));
        assert!(!is_temp_relaunch_path(
            Path::new(r"C:\Program Files\Usque\usque-uninstall.exe"),
            temp
        ));
        assert!(!is_temp_relaunch_path(
            &temp.join("other").join("usque-uninstall.exe"),
            temp
        ));
    }

    #[test]
    fn process_arguments_avoid_shell_parsing() {
        let request = UninstallRequest {
            product_code: "{076CF387-E447-4666-9153-2DA16049A390}".to_owned(),
            remove_user_data: false,
        };
        assert_eq!(
            request.arguments(false),
            [
                "/x",
                "{076CF387-E447-4666-9153-2DA16049A390}",
                "USQUE_REMOVE_USER_DATA=0",
                "/qb",
                "/norestart",
            ]
        );
        assert_eq!(request.arguments(true)[3], "/qn");
    }

    #[test]
    fn interactive_mode_is_windows_only_on_other_targets() {
        if cfg!(windows) {
            return;
        }
        let error = run(["--product-code", "{076CF387-E447-4666-9153-2DA16049A390}"])
            .expect_err("non-windows interactive");
        assert!(matches!(error, UninstallError::WindowsOnly));
    }
}
