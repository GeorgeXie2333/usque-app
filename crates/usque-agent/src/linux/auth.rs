use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read},
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::fs::{MetadataExt, OpenOptionsExt},
    },
    path::{Component, Path},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{ENGINE_PATH, INSTALL_RECORD_PATH, transport::PeerCredentials};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct InstallRecord {
    pub schema_version: u32,
    pub version: String,
    pub protocol_version: u32,
    pub format: String,
    pub architecture: String,
    pub engine_sha256: String,
}

impl InstallRecord {
    pub fn validate(&self) -> io::Result<()> {
        let architecture = match std::env::consts::ARCH {
            "x86_64" => "amd64",
            "aarch64" => "arm64",
            _ => return Err(denied("unsupported Agent architecture")),
        };
        if self.schema_version != 1
            || self.protocol_version != crate::AGENT_PROTOCOL_VERSION
            || self.version != env!("CARGO_PKG_VERSION")
            || self.architecture != architecture
            || !matches!(self.format.as_str(), "deb" | "rpm" | "appimage")
            || self.engine_sha256.len() != 64
            || !self
                .engine_sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return Err(denied(
                "incompatible or malformed Linux installation record",
            ));
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct CallerPolicy {
    pub record: InstallRecord,
    engine: File,
}

impl CallerPolicy {
    /// Reads only fixed, root-owned installation paths. No caller-controlled
    /// executable allowlist or unsigned-debug escape hatch exists on Linux.
    pub fn load() -> io::Result<Self> {
        let mut file = open_trusted_file(Path::new(INSTALL_RECORD_PATH))?;
        if file.metadata()?.len() > 16 * 1024 {
            return Err(denied("installation record too large"));
        }
        let mut bytes = Vec::new();
        file.by_ref().take(16 * 1024 + 1).read_to_end(&mut bytes)?;
        let record: InstallRecord =
            serde_json::from_slice(&bytes).map_err(|_| denied("invalid installation record"))?;
        record.validate()?;
        let mut engine = open_trusted_file(Path::new(ENGINE_PATH))?;
        let digest = hash_file(&mut engine)?;
        if digest != record.engine_sha256 {
            return Err(denied(
                "installed Engine digest does not match installation",
            ));
        }
        Ok(Self { record, engine })
    }

    pub fn authenticate(&self, peer: PeerCredentials) -> io::Result<AuthenticatedPeer> {
        if peer.uid == 0 {
            return Err(denied("the Engine must run as a regular user"));
        }
        // A pidfd pins the process independently of PID reuse. It is not sent
        // over IPC or persisted as part of the recovery identity.
        // SAFETY: pidfd_open takes scalar PID/flags and returns a fresh fd.
        let raw = unsafe { libc::syscall(libc::SYS_pidfd_open, peer.process_id, 0) };
        if raw < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: successful pidfd_open transferred one new descriptor to us.
        let process = unsafe { OwnedFd::from_raw_fd(raw as i32) };
        let start_time = process_start_time(peer.process_id)?;
        let identity = AuthenticatedPeer {
            credentials: peer,
            start_time,
            process,
        };
        self.revalidate(&identity)?;
        Ok(identity)
    }

    pub fn revalidate(&self, identity: &AuthenticatedPeer) -> io::Result<()> {
        identity.ensure_alive()?;
        let pid = identity.credentials.process_id;
        if process_start_time(pid)? != identity.start_time {
            return Err(denied("Engine process identity changed"));
        }
        let executable = File::open(format!("/proc/{pid}/exe"))?.metadata()?;
        let installed = self.engine.metadata()?;
        if executable.dev() != installed.dev() || executable.ino() != installed.ino() {
            return Err(denied("Agent caller is not the installed Engine"));
        }
        let namespace = fs::metadata(format!("/proc/{pid}/ns/net"))?;
        let local = fs::metadata("/proc/self/ns/net")?;
        if namespace.ino() != local.ino() || namespace.dev() != local.dev() {
            return Err(denied("Engine network namespace differs from Agent"));
        }
        verify_process_loading(pid)?;
        let status = fs::read_to_string(format!("/proc/{pid}/status"))?;
        let ids = status
            .lines()
            .find_map(|line| line.strip_prefix("Uid:"))
            .ok_or_else(|| denied("Engine process credentials unavailable"))?;
        let ids = ids
            .split_whitespace()
            .map(str::parse::<u32>)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| denied("invalid process credentials"))?;
        if ids.len() != 4 || ids.iter().any(|uid| *uid != identity.credentials.uid) {
            return Err(denied("Engine process user changed"));
        }
        Ok(())
    }
}

/// An executable path alone is not a process-integrity boundary: an installed
/// Engine can otherwise be launched with an untrusted preloaded library.
fn verify_process_loading(pid: u32) -> io::Result<()> {
    let environment = fs::read(format!("/proc/{pid}/environ"))?;
    if environment.len() > 128 * 1024 {
        return Err(denied("Engine environment exceeds limit"));
    }
    for variable in environment.split(|byte| *byte == 0) {
        let key = variable
            .split(|byte| *byte == b'=')
            .next()
            .unwrap_or_default();
        if unsafe_loader_variable(key) {
            return Err(denied("Engine has an unsafe loader environment"));
        }
    }
    // PR_SET_DUMPABLE=0 makes proc symlink ownership root even for a normal
    // user. Require it before privileged IPC so peer users cannot attach a
    // debugger after authentication. The service needs CAP_SYS_PTRACE to read
    // this protected process's executable/maps.
    if fs::symlink_metadata(format!("/proc/{pid}/exe"))?.uid() != 0 {
        return Err(denied(
            "Engine must disable ptrace and core dumps before Agent IPC",
        ));
    }
    let mut mappings = String::new();
    File::open(format!("/proc/{pid}/smaps"))?
        .take(8 * 1024 * 1024 + 1)
        .read_to_string(&mut mappings)?;
    if mappings.len() > 8 * 1024 * 1024 {
        return Err(denied("Engine memory mappings exceed limit"));
    }
    for line in executable_mappings(&mappings)? {
        let parts: Vec<_> = line.split_whitespace().collect();
        if parts.len() < 5 {
            return Err(denied("invalid Engine memory mapping"));
        }
        if !parts[1].contains('x') {
            continue;
        }
        if parts[1].contains('w') {
            return Err(denied("Engine has writable executable memory"));
        }
        if parts
            .get(5)
            .is_some_and(|name| matches!(*name, "[vdso]" | "[vsyscall]"))
        {
            continue;
        }
        if parts.len() != 6 || !parts[5].starts_with('/') || parts[4] == "0" {
            return Err(denied("Engine has an untrusted executable mapping"));
        }
        let trusted = open_trusted_file(Path::new(parts[5]))?.metadata()?;
        let (major, minor) = parts[3]
            .split_once(':')
            .ok_or_else(|| denied("invalid Engine mapping device"))?;
        let major =
            u32::from_str_radix(major, 16).map_err(|_| denied("invalid Engine mapping device"))?;
        let minor =
            u32::from_str_radix(minor, 16).map_err(|_| denied("invalid Engine mapping device"))?;
        let inode = parts[4]
            .parse::<u64>()
            .map_err(|_| denied("invalid Engine mapping inode"))?;
        if major != libc::major(trusted.dev())
            || minor != libc::minor(trusted.dev())
            || inode != trusted.ino()
        {
            return Err(denied(
                "Engine loaded executable code outside its trusted installation",
            ));
        }
    }
    Ok(())
}

/// File-backed executable VMAs must still contain the installed file pages.
/// A preload that modifies text and then unmaps itself leaves anonymous COW
/// pages even if it restores rx permissions and erases its environment.
fn executable_mappings(smaps: &str) -> io::Result<Vec<&str>> {
    let mut mappings = Vec::new();
    let mut executable = false;
    let mut fields = 0u8;
    for line in smaps.lines() {
        let first = line.split_whitespace().next().unwrap_or_default();
        if first.split_once('-').is_some_and(|(start, end)| {
            !start.is_empty()
                && !end.is_empty()
                && start
                    .bytes()
                    .chain(end.bytes())
                    .all(|byte| byte.is_ascii_hexdigit())
        }) {
            if executable && fields != 15 {
                return Err(denied("incomplete Engine executable page accounting"));
            }
            executable = line
                .split_whitespace()
                .nth(1)
                .is_some_and(|permissions| permissions.contains('x'));
            fields = 0;
            if executable {
                mappings.push(line);
            }
        } else if executable {
            let (key, value) = line
                .split_once(':')
                .ok_or_else(|| denied("invalid Engine page accounting"))?;
            let flag = match key {
                "Anonymous" => 1,
                "Private_Dirty" => 2,
                "Shared_Dirty" => 4,
                "Swap" => 8,
                _ => continue,
            };
            let amount = value
                .trim()
                .strip_suffix(" kB")
                .and_then(|value| value.parse::<u64>().ok())
                .ok_or_else(|| denied("invalid Engine executable page accounting"))?;
            // Dirty file-cache pages also occur immediately after installing a
            // verified binary. Private text modifications are instead counted
            // as anonymous COW pages, including when swapped out.
            if fields & flag != 0 || (flag & (1 | 8) != 0 && amount != 0) {
                return Err(denied(
                    "Engine executable mapping contains modified or anonymous pages",
                ));
            }
            fields |= flag;
        }
    }
    if mappings.is_empty() || (executable && fields != 15) {
        return Err(denied("Engine executable page accounting unavailable"));
    }
    Ok(mappings)
}

fn unsafe_loader_variable(key: &[u8]) -> bool {
    key.starts_with(b"LD_")
        || matches!(
            key,
            b"GLIBC_TUNABLES"
                | b"GCONV_PATH"
                | b"LOCPATH"
                | b"GIO_EXTRA_MODULES"
                | b"GTK_PATH"
                | b"GI_TYPELIB_PATH"
        )
}

#[derive(Debug)]
pub struct AuthenticatedPeer {
    pub credentials: PeerCredentials,
    pub start_time: u64,
    process: OwnedFd,
}

impl AuthenticatedPeer {
    pub fn ensure_alive(&self) -> io::Result<()> {
        let mut descriptor = libc::pollfd {
            fd: self.process.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: poll receives one live, writable pollfd and does not own it.
        let result = unsafe { libc::poll(&mut descriptor, 1, 0) };
        if result < 0 {
            return Err(io::Error::last_os_error());
        }
        if descriptor.revents != 0 {
            return Err(denied("Engine process has exited"));
        }
        Ok(())
    }
}

pub fn process_start_time(pid: u32) -> io::Result<u64> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat"))?;
    // comm (field 2) may contain spaces and parentheses. The final ')' is the
    // delimiter; field 22 is offset 19 from the state (field 3).
    let tail = stat
        .rsplit_once(')')
        .ok_or_else(|| denied("invalid process stat"))?
        .1;
    tail.split_whitespace()
        .nth(19)
        .ok_or_else(|| denied("missing process start time"))?
        .parse()
        .map_err(|_| denied("invalid process start time"))
}

pub fn open_trusted_file(path: &Path) -> io::Result<File> {
    verify_trusted_parents(path)?;
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.uid() != 0
        || metadata.mode() & 0o022 != 0
        || metadata.nlink() != 1
    {
        return Err(denied("installation file is not exclusively root-owned"));
    }
    Ok(file)
}

/// Rejects writable/symlink parents before following a fixed system path.
/// Root-owned ancestors cannot be renamed by an unprivileged caller between
/// this check and open(O_NOFOLLOW).
pub fn verify_trusted_parents(path: &Path) -> io::Result<()> {
    if !path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, Component::CurDir | Component::ParentDir))
    {
        return Err(denied("trusted path must be absolute and normalized"));
    }
    let mut parent = path.parent();
    while let Some(directory) = parent {
        let metadata = fs::symlink_metadata(directory)?;
        if !metadata.is_dir() || metadata.uid() != 0 || metadata.mode() & 0o022 != 0 {
            return Err(denied("installation parent is not exclusively root-owned"));
        }
        parent = directory.parent();
    }
    Ok(())
}

fn hash_file(file: &mut File) -> io::Result<String> {
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

fn denied(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record() -> InstallRecord {
        InstallRecord {
            schema_version: 1,
            version: env!("CARGO_PKG_VERSION").into(),
            protocol_version: crate::AGENT_PROTOCOL_VERSION,
            format: "deb".into(),
            architecture: if std::env::consts::ARCH == "aarch64" {
                "arm64"
            } else {
                "amd64"
            }
            .into(),
            engine_sha256: "a".repeat(64),
        }
    }

    #[test]
    fn native_proc_accounting_parses_without_privileges_or_network_changes() {
        let mappings = fs::read_to_string("/proc/self/smaps").unwrap();
        assert!(!executable_mappings(&mappings).unwrap().is_empty());
    }

    #[test]
    fn executable_private_copy_and_missing_accounting_fail_closed() {
        let clean = "1000-2000 r-xp 0000 08:01 1 /usr/lib/usque/usque-engine\nAnonymous: 0 kB\nPrivate_Dirty: 0 kB\nShared_Dirty: 0 kB\nSwap: 0 kB\n";
        assert_eq!(executable_mappings(clean).unwrap().len(), 1);
        assert!(executable_mappings(&clean.replace("Anonymous: 0", "Anonymous: 4")).is_err());
        assert!(
            executable_mappings(&clean.replace("Private_Dirty: 0", "Private_Dirty: 4")).is_ok()
        );
        assert!(executable_mappings(&clean.replace("Swap: 0 kB\n", "")).is_err());
    }

    #[test]
    fn incompatible_and_unbounded_install_identities_fail_closed() {
        assert!(record().validate().is_ok());
        let mut changed = record();
        changed.protocol_version += 1;
        assert!(changed.validate().is_err());
        changed = record();
        changed.version.push('x');
        assert!(changed.validate().is_err());
        changed = record();
        changed.format = "unknown".into();
        assert!(changed.validate().is_err());
        changed = record();
        changed.engine_sha256 = "x".repeat(64);
        assert!(changed.validate().is_err());
    }

    #[test]
    fn own_process_start_time_is_stable() {
        assert_eq!(
            process_start_time(std::process::id()).unwrap(),
            process_start_time(std::process::id()).unwrap()
        );
    }

    #[test]
    fn temporary_executable_cannot_be_a_trusted_installation() {
        let file = tempfile::NamedTempFile::new().unwrap();
        assert!(open_trusted_file(file.path()).is_err());
    }

    #[test]
    fn loader_injection_environment_is_explicitly_rejected() {
        for key in [
            b"LD_PRELOAD".as_slice(),
            b"LD_LIBRARY_PATH",
            b"LD_AUDIT",
            b"GLIBC_TUNABLES",
            b"GCONV_PATH",
        ] {
            assert!(unsafe_loader_variable(key));
        }
        assert!(!unsafe_loader_variable(b"LANG"));
        assert!(!unsafe_loader_variable(b"XDG_RUNTIME_DIR"));
    }
}
