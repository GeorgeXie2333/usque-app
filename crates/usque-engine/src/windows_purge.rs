//! Current-user data cleanup used only when explicitly selected during MSI
//! uninstall. Network state remains the privileged Agent's responsibility.

use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use thiserror::Error;
use usque_platform::{VaultError, WindowsCredentialVault};

const ENGINE_DATA_DIRECTORY: &str = "Usque";
const PREFERENCES_COMPANY_DIRECTORY: &str = "io.github.georgexie2333";
const PREFERENCES_PRODUCT_DIRECTORY: &str = "Usque";

/// Deletes all current-user Usque state after validating that the MSI supplied
/// paths exactly match this user's standard application-data directories.
pub fn purge_current_user_data(
    config_path: &Path,
    preferences_directory: &Path,
) -> Result<(), UserDataPurgeError> {
    let local_app_data = env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .ok_or(UserDataPurgeError::MissingEnvironment("LOCALAPPDATA"))?;
    let roaming_app_data = env::var_os("APPDATA")
        .map(PathBuf::from)
        .ok_or(UserDataPurgeError::MissingEnvironment("APPDATA"))?;
    let paths = validate_user_data_paths(
        config_path,
        preferences_directory,
        &local_app_data,
        &roaming_app_data,
    )?;

    WindowsCredentialVault::delete_all_namespaced()?;
    remove_directory_if_present(&paths.engine_data_directory)?;
    remove_directory_if_present(&paths.preferences_directory)?;
    remove_empty_directory(&paths.preferences_company_directory)?;
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
struct ValidatedUserDataPaths {
    engine_data_directory: PathBuf,
    preferences_directory: PathBuf,
    preferences_company_directory: PathBuf,
}

fn validate_user_data_paths(
    config_path: &Path,
    preferences_directory: &Path,
    local_app_data: &Path,
    roaming_app_data: &Path,
) -> Result<ValidatedUserDataPaths, UserDataPurgeError> {
    let expected_engine_directory = local_app_data.join(ENGINE_DATA_DIRECTORY);
    let expected_config = expected_engine_directory.join("config.json");
    let expected_company_directory = roaming_app_data.join(PREFERENCES_COMPANY_DIRECTORY);
    let expected_preferences_directory =
        expected_company_directory.join(PREFERENCES_PRODUCT_DIRECTORY);

    ensure_exact_windows_path("configuration", config_path, &expected_config)?;
    ensure_exact_windows_path(
        "preferences",
        preferences_directory,
        &expected_preferences_directory,
    )?;

    Ok(ValidatedUserDataPaths {
        engine_data_directory: expected_engine_directory,
        preferences_directory: expected_preferences_directory,
        preferences_company_directory: expected_company_directory,
    })
}

fn ensure_exact_windows_path(
    kind: &'static str,
    actual: &Path,
    expected: &Path,
) -> Result<(), UserDataPurgeError> {
    if !actual.is_absolute() || !expected.is_absolute() {
        return Err(UserDataPurgeError::UnsafePath {
            kind,
            actual: actual.to_path_buf(),
        });
    }
    let normalize = |path: &Path| {
        path.to_string_lossy()
            .replace('/', "\\")
            .trim_end_matches('\\')
            .to_ascii_lowercase()
    };
    if normalize(actual) != normalize(expected) {
        return Err(UserDataPurgeError::UnsafePath {
            kind,
            actual: actual.to_path_buf(),
        });
    }
    Ok(())
}

fn remove_directory_if_present(path: &Path) -> Result<(), UserDataPurgeError> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(UserDataPurgeError::FileSystem(path.to_path_buf(), error)),
    }
}

fn remove_empty_directory(path: &Path) -> Result<(), UserDataPurgeError> {
    match fs::remove_dir(path) {
        Ok(()) => Ok(()),
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::DirectoryNotEmpty
            ) =>
        {
            Ok(())
        }
        Err(error) => Err(UserDataPurgeError::FileSystem(path.to_path_buf(), error)),
    }
}

#[derive(Debug, Error)]
pub enum UserDataPurgeError {
    #[error("{0} is unavailable while deleting current-user Usque data")]
    MissingEnvironment(&'static str),
    #[error("refusing to delete an unexpected {kind} path: {actual}")]
    UnsafePath { kind: &'static str, actual: PathBuf },
    #[error("secure identity cleanup failed: {0}")]
    Vault(#[from] VaultError),
    #[error("failed to remove {0}: {1}")]
    FileSystem(PathBuf, io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_the_two_exact_current_user_locations() {
        let paths = validate_user_data_paths(
            Path::new(r"C:\Users\Alice\AppData\Local\Usque\config.json"),
            Path::new(r"C:\Users\Alice\AppData\Roaming\io.github.georgexie2333\Usque"),
            Path::new(r"C:\Users\Alice\AppData\Local"),
            Path::new(r"C:\Users\Alice\AppData\Roaming"),
        )
        .expect("standard paths");
        assert_eq!(
            paths.engine_data_directory,
            Path::new(r"C:\Users\Alice\AppData\Local\Usque")
        );
        assert_eq!(
            paths.preferences_company_directory,
            Path::new(r"C:\Users\Alice\AppData\Roaming\io.github.georgexie2333")
        );
    }

    #[test]
    fn rejects_parent_traversal_and_unrelated_directories() {
        let error = validate_user_data_paths(
            Path::new(r"C:\Users\Alice\AppData\Local\Usque\..\Other\config.json"),
            Path::new(r"C:\Users\Alice\AppData\Roaming\io.github.georgexie2333\Usque"),
            Path::new(r"C:\Users\Alice\AppData\Local"),
            Path::new(r"C:\Users\Alice\AppData\Roaming"),
        )
        .expect_err("traversal must be rejected");
        assert!(matches!(
            error,
            UserDataPurgeError::UnsafePath {
                kind: "configuration",
                ..
            }
        ));
    }
}
