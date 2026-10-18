//! XDG locations and private directories for the production Linux desktop.
//!
//! Resolving paths never creates them. Callers using a custom configuration
//! (including tests) keep their caches and logs beside that configuration.

use std::{
    ffi::{CString, OsString},
    fs::{self, File},
    io,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::MetadataExt,
    },
    path::{Component, Path, PathBuf},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinuxPaths {
    pub config: PathBuf,
    pub data: PathBuf,
    pub cache: PathBuf,
    pub state: PathBuf,
    pub runtime: PathBuf,
}

impl LinuxPaths {
    pub fn from_environment() -> io::Result<Self> {
        Self::resolve(|name| std::env::var_os(name), effective_uid())
    }

    fn resolve(mut get: impl FnMut(&str) -> Option<OsString>, uid: u32) -> io::Result<Self> {
        let home = get("HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute());
        let mut directory = |name: &str, fallback: &str| -> io::Result<PathBuf> {
            let base = get(name)
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())
                .or_else(|| home.as_ref().map(|home| home.join(fallback)))
                .ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::NotFound,
                        "absolute HOME or XDG directory is required",
                    )
                })?;
            Ok(base.join("usque"))
        };
        let config = directory("XDG_CONFIG_HOME", ".config")?;
        let data = directory("XDG_DATA_HOME", ".local/share")?;
        let cache = directory("XDG_CACHE_HOME", ".cache")?;
        let state = directory("XDG_STATE_HOME", ".local/state")?;
        let runtime = get("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .unwrap_or_else(|| PathBuf::from(format!("/run/user/{uid}")))
            .join("usque");
        Ok(Self {
            config,
            data,
            cache,
            state,
            runtime,
        })
    }

    pub fn config_file(&self) -> PathBuf {
        self.config.join("config.json")
    }

    pub fn control_socket(&self) -> PathBuf {
        self.runtime.join("engine-v1.sock")
    }

    /// Only the production default is mapped to the user's XDG directories.
    pub fn for_config(config: &Path) -> Option<Self> {
        Self::from_environment()
            .ok()
            .filter(|paths| paths.config_file() == config)
    }
}

pub fn effective_uid() -> u32 {
    // SAFETY: geteuid has no preconditions or output pointers.
    unsafe { libc::geteuid() }
}

/// Open or create a user-owned 0700 directory without following any symlink.
/// Parent directories are traversed with directory FDs, so replacing a path
/// component cannot redirect creation into another directory.
pub fn private_directory(path: &Path) -> io::Result<File> {
    if !path.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "private directory must be absolute",
        ));
    }
    let mut parent = File::open("/")?;
    for component in path.components() {
        let Component::Normal(name) = component else {
            if matches!(component, Component::RootDir) {
                continue;
            }
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "private directory has an invalid component",
            ));
        };
        let name = CString::new(name.as_encoded_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "directory contains NUL"))?;
        let open = || {
            // SAFETY: parent owns a directory FD and name is a live NUL-terminated component.
            unsafe {
                libc::openat(
                    parent.as_raw_fd(),
                    name.as_ptr(),
                    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
                )
            }
        };
        let mut descriptor = open();
        if descriptor < 0 && io::Error::last_os_error().kind() == io::ErrorKind::NotFound {
            // SAFETY: parent and name obey the openat invariants above. mkdirat does not follow the last component.
            if unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o700) } != 0
                && io::Error::last_os_error().kind() != io::ErrorKind::AlreadyExists
            {
                return Err(io::Error::last_os_error());
            }
            descriptor = open();
        }
        if descriptor < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: openat returned a fresh descriptor, now exclusively owned by File.
        parent = unsafe { File::from_raw_fd(descriptor) };
    }
    verify_private_directory(&parent)?;
    Ok(parent)
}

pub fn verify_private_directory(directory: &File) -> io::Result<()> {
    let metadata = directory.metadata()?;
    if !metadata.is_dir() || metadata.uid() != effective_uid() || metadata.mode() & 0o777 != 0o700 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "directory must be owned by the current user with mode 0700",
        ));
    }
    Ok(())
}

pub fn prepare_config_file(path: &Path) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "configuration has no parent")
    })?;
    private_directory(parent)?;
    match fs::symlink_metadata(path) {
        Ok(metadata)
            if !metadata.is_file()
                || metadata.uid() != effective_uid()
                || metadata.mode() & 0o077 != 0 =>
        {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "configuration must be a private current-user regular file",
            ))
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};

    #[test]
    fn xdg_overrides_and_relative_values_follow_specification() {
        let paths = LinuxPaths::resolve(
            |key| match key {
                "HOME" => Some("/home/example".into()),
                "XDG_CONFIG_HOME" => Some("/custom/config".into()),
                "XDG_CACHE_HOME" => Some("relative/ignored".into()),
                "XDG_RUNTIME_DIR" => Some("/run/user/42".into()),
                _ => None,
            },
            42,
        )
        .unwrap();
        assert_eq!(
            paths.config_file(),
            Path::new("/custom/config/usque/config.json")
        );
        assert_eq!(paths.cache, Path::new("/home/example/.cache/usque"));
        assert_eq!(paths.state, Path::new("/home/example/.local/state/usque"));
        assert_eq!(
            paths.control_socket(),
            Path::new("/run/user/42/usque/engine-v1.sock")
        );
    }

    #[test]
    fn private_directories_reject_links_and_insecure_existing_permissions() {
        let root = tempfile::tempdir().unwrap();
        let private = root.path().join("private");
        private_directory(&private).unwrap();
        assert_eq!(fs::metadata(&private).unwrap().mode() & 0o777, 0o700);
        symlink(&private, root.path().join("link")).unwrap();
        assert!(private_directory(&root.path().join("link/child")).is_err());
        assert!(!private.join("child").exists());
        fs::set_permissions(&private, fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(
            private_directory(&private).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        assert_eq!(fs::metadata(&private).unwrap().mode() & 0o777, 0o755);
    }
}
