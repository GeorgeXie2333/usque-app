//! Authentication of an Engine process connected to the Agent Named Pipe.
//!
//! Authentication binds a connection to its kernel-reported PID, impersonated
//! user SID, exact installed executable path, valid Authenticode signature, and
//! pinned signer-certificate SHA-256 fingerprint. The process handle remains
//! open for the connection lifetime, preventing PID reuse before HANDLE
//! duplication for the packet ring.

use std::{
    ffi::c_void,
    fs, io,
    path::{Path, PathBuf},
    ptr,
};

use thiserror::Error;
use tracing::error;
pub use usque_platform::windows_authenticode::SignerFingerprint;
use usque_platform::windows_authenticode::{
    AuthenticodeError, signer_fingerprint, verify_authenticode,
};
use windows_sys::{
    Win32::{
        Foundation::{CloseHandle, HANDLE, LocalFree},
        Security::{
            Authorization::ConvertSidToStringSidW, GetTokenInformation, RevertToSelf, TOKEN_QUERY,
            TOKEN_USER, TokenUser,
        },
        System::{
            Pipes::{GetNamedPipeClientProcessId, ImpersonateNamedPipeClient},
            Threading::{
                GetCurrentThread, GetProcessId, OpenProcess, OpenProcessToken, OpenThreadToken,
                PROCESS_DUP_HANDLE, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
            },
        },
    },
    core::PWSTR,
};

use crate::AuthenticatedCaller;

const MAX_IMAGE_PATH_UNITS: usize = 32_768;

#[derive(Debug, Clone)]
pub struct CallerPolicy {
    allowed_engine_paths: Vec<PathBuf>,
    signer: Option<SignerFingerprint>,
    allow_unsigned_debug_client: bool,
}

impl CallerPolicy {
    pub fn new(
        allowed_engine_paths: Vec<PathBuf>,
        signer: Option<SignerFingerprint>,
        allow_unsigned_debug_client: bool,
    ) -> Result<Self, AuthenticationError> {
        if allowed_engine_paths.is_empty()
            || allowed_engine_paths.iter().any(|path| !path.is_absolute())
        {
            return Err(AuthenticationError::InvalidPolicy(
                "at least one absolute Engine path is required".to_owned(),
            ));
        }
        if allow_unsigned_debug_client && !cfg!(debug_assertions) {
            return Err(AuthenticationError::InvalidPolicy(
                "unsigned clients can never be enabled in a release build".to_owned(),
            ));
        }
        if signer.is_none() && !allow_unsigned_debug_client {
            return Err(AuthenticationError::InvalidPolicy(
                "a pinned signer fingerprint is required".to_owned(),
            ));
        }
        Ok(Self {
            allowed_engine_paths,
            signer,
            allow_unsigned_debug_client,
        })
    }

    fn path_is_allowed(&self, path: &Path) -> Result<bool, AuthenticationError> {
        let actual = normalized_path(path)?;
        for allowed in &self.allowed_engine_paths {
            if normalized_path(allowed)? == actual {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

pub struct AuthenticatedProcess {
    caller: AuthenticatedCaller,
    process: OwnedHandle,
}

impl AuthenticatedProcess {
    pub fn caller(&self) -> &AuthenticatedCaller {
        &self.caller
    }

    pub fn process_handle(&self) -> HANDLE {
        self.process.0
    }
}

/// Authenticates the currently connected client on a server-side Named Pipe.
pub(crate) fn authenticate_named_pipe(
    pipe: HANDLE,
    policy: &CallerPolicy,
) -> Result<AuthenticatedProcess, AuthenticationError> {
    let mut process_id = 0_u32;
    // SAFETY: `pipe` is owned by the live Named Pipe server and process_id is
    // writable for the duration of the call.
    if unsafe { GetNamedPipeClientProcessId(pipe, &mut process_id) } == 0 || process_id == 0 {
        return Err(last_error("GetNamedPipeClientProcessId"));
    }

    // SAFETY: OpenProcess validates the PID and returns an owned kernel handle.
    let process = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_DUP_HANDLE,
            0,
            process_id,
        )
    };
    if process.is_null() {
        return Err(last_error("OpenProcess"));
    }
    let process = OwnedHandle(process);
    // Re-read the pipe owner after opening the process to close the PID-reuse
    // race. Holding this process handle prevents subsequent reuse.
    let mut confirmed_process_id = 0_u32;
    // SAFETY: `pipe` is still the live Named Pipe server handle and
    // `&mut confirmed_process_id` is a valid out-pointer for the call duration.
    if unsafe { GetNamedPipeClientProcessId(pipe, &mut confirmed_process_id) } == 0
        || confirmed_process_id != process_id
        // SAFETY: process.0 is a live process handle owned by this function.
        || unsafe { GetProcessId(process.0) } != process_id
    {
        return Err(AuthenticationError::ProcessChanged);
    }

    let impersonated_sid = sid_from_pipe_client(pipe)?;
    let process_sid = sid_from_process(process.0)?;
    if impersonated_sid != process_sid {
        return Err(AuthenticationError::SidMismatch);
    }
    let executable_path = process_image_path(process.0)?;
    if !policy.path_is_allowed(&executable_path)? {
        return Err(AuthenticationError::UnexpectedExecutable(executable_path));
    }

    if let Some(expected) = policy.signer {
        verify_authenticode(&executable_path)?;
        let actual = signer_fingerprint(&executable_path)?;
        if actual != expected {
            return Err(AuthenticationError::SignerMismatch);
        }
    } else if !policy.allow_unsigned_debug_client {
        return Err(AuthenticationError::UnsignedClientDenied);
    }

    Ok(AuthenticatedProcess {
        caller: AuthenticatedCaller {
            process_id,
            user_sid: impersonated_sid,
            executable_path,
            process_handle: Some(process.0 as usize),
        },
        process,
    })
}

fn sid_from_pipe_client(pipe: HANDLE) -> Result<String, AuthenticationError> {
    // SAFETY: the pipe has an active client connection. RevertGuard guarantees
    // the service thread returns to LocalSystem even on every error path.
    if unsafe { ImpersonateNamedPipeClient(pipe) } == 0 {
        return Err(last_error("ImpersonateNamedPipeClient"));
    }
    let _revert = RevertGuard;
    let mut token: HANDLE = ptr::null_mut();
    // SAFETY: token points to writable storage and GetCurrentThread returns a
    // valid pseudo-handle.
    if unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, 1, &mut token) } == 0 {
        return Err(last_error("OpenThreadToken"));
    }
    sid_from_token(OwnedHandle(token))
}

fn sid_from_process(process: HANDLE) -> Result<String, AuthenticationError> {
    let mut token: HANDLE = ptr::null_mut();
    // SAFETY: process remains open and token points to writable storage.
    if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) } == 0 {
        return Err(last_error("OpenProcessToken"));
    }
    sid_from_token(OwnedHandle(token))
}

fn sid_from_token(token: OwnedHandle) -> Result<String, AuthenticationError> {
    let mut required = 0_u32;
    // SAFETY: the first call intentionally supplies no buffer to get its size.
    unsafe {
        GetTokenInformation(token.0, TokenUser, ptr::null_mut(), 0, &mut required);
    }
    if required == 0 {
        return Err(last_error("GetTokenInformation(size)"));
    }
    let mut buffer = vec![0_u8; required as usize];
    // SAFETY: the buffer has the exact size returned by Windows.
    if unsafe {
        GetTokenInformation(
            token.0,
            TokenUser,
            buffer.as_mut_ptr().cast(),
            required,
            &mut required,
        )
    } == 0
    {
        return Err(last_error("GetTokenInformation"));
    }
    // SAFETY: a successful TokenUser query begins with a valid TOKEN_USER.
    let user = unsafe { &*buffer.as_ptr().cast::<TOKEN_USER>() };
    let mut text: PWSTR = ptr::null_mut();
    // SAFETY: the SID is valid while buffer lives, and LocalFree owns the
    // returned string.
    if unsafe { ConvertSidToStringSidW(user.User.Sid, &mut text) } == 0 {
        return Err(last_error("ConvertSidToStringSidW"));
    }
    let allocation = LocalAllocation(text.cast());
    wide_null_to_string(allocation.0.cast(), 256)
}

fn process_image_path(process: HANDLE) -> Result<PathBuf, AuthenticationError> {
    let mut buffer = vec![0_u16; MAX_IMAGE_PATH_UNITS];
    let mut length = u32::try_from(buffer.len()).expect("path bound fits u32");
    // SAFETY: the process handle has QUERY_LIMITED_INFORMATION and the UTF-16
    // buffer is writable for `length` units.
    if unsafe { QueryFullProcessImageNameW(process, 0, buffer.as_mut_ptr(), &mut length) } == 0 {
        return Err(last_error("QueryFullProcessImageNameW"));
    }
    buffer.truncate(length as usize);
    Ok(PathBuf::from(
        String::from_utf16(&buffer).map_err(|_| AuthenticationError::InvalidProcessPath)?,
    ))
}

fn normalized_path(path: &Path) -> Result<String, AuthenticationError> {
    let canonical = fs::canonicalize(path)
        .map_err(|error| AuthenticationError::Path(path.to_path_buf(), error))?;
    Ok(canonical
        .to_string_lossy()
        .trim_start_matches(r"\\?\")
        .replace('/', r"\")
        .to_ascii_lowercase())
}

fn wide_null_to_string(value: *const u16, maximum: usize) -> Result<String, AuthenticationError> {
    if value.is_null() {
        return Err(AuthenticationError::InvalidSid);
    }
    for length in 0..maximum {
        // SAFETY: callers provide a Windows-owned null-terminated buffer and a
        // defensive maximum bound.
        if unsafe { *value.add(length) } == 0 {
            // SAFETY: every unit in this slice precedes the terminator.
            let units = unsafe { std::slice::from_raw_parts(value, length) };
            return String::from_utf16(units).map_err(|_| AuthenticationError::InvalidSid);
        }
    }
    Err(AuthenticationError::InvalidSid)
}

fn last_error(operation: &'static str) -> AuthenticationError {
    AuthenticationError::Windows(operation, io::Error::last_os_error())
}

struct RevertGuard;

impl Drop for RevertGuard {
    fn drop(&mut self) {
        // SAFETY: the current thread is impersonating only for this guard.
        let reverted = unsafe { RevertToSelf() };
        abort_if_impersonation_revert_failed(reverted);
    }
}

fn revert_status_requires_abort(reverted: i32) -> bool {
    reverted == 0
}

fn abort_if_impersonation_revert_failed(reverted: i32) {
    if !revert_status_requires_abort(reverted) {
        return;
    }
    let error = io::Error::last_os_error();
    error!(
        error = %error,
        "RevertToSelf failed after named-pipe impersonation; aborting so this worker is not reused for privileged work"
    );
    std::process::abort();
}

struct OwnedHandle(HANDLE);

// SAFETY: Windows kernel handles may be used and closed from any process
// thread. This wrapper has unique ownership and closes the handle exactly once.
unsafe impl Send for OwnedHandle {}
// SAFETY: `&OwnedHandle` is safe to share: the HANDLE value is immutable after
// construction, kernel object ops are thread-safe, and Drop still closes once.
unsafe impl Sync for OwnedHandle {}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: this wrapper uniquely owns a valid kernel handle.
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}

struct LocalAllocation(*mut c_void);

impl Drop for LocalAllocation {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: ConvertSidToStringSidW allocates with LocalAlloc.
            unsafe {
                LocalFree(self.0);
            }
        }
    }
}

#[derive(Debug, Error)]
pub enum AuthenticationError {
    #[error("Windows {0} failed: {1}")]
    Windows(&'static str, io::Error),
    #[error("caller process changed while it was being authenticated")]
    ProcessChanged,
    #[error("Named Pipe impersonation SID does not match the caller process SID")]
    SidMismatch,
    #[error("Windows returned an invalid caller SID")]
    InvalidSid,
    #[error("Windows returned a non-UTF-16 caller executable path")]
    InvalidProcessPath,
    #[error("caller executable is not an allowed installed Engine: {0}")]
    UnexpectedExecutable(PathBuf),
    #[error("failed to normalize path {0}: {1}")]
    Path(PathBuf, io::Error),
    #[error(transparent)]
    Authenticode(#[from] AuthenticodeError),
    #[error("Engine signer certificate does not match the pinned fingerprint")]
    SignerMismatch,
    #[error("unsigned Engine clients are denied")]
    UnsignedClientDenied,
    #[error("caller policy is invalid: {0}")]
    InvalidPolicy(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn impersonation_revert_failure_is_process_fatal() {
        // RevertGuard::drop logs the Win32 error and calls abort() when
        // RevertToSelf fails. This test does not invoke RevertToSelf or abort:
        // abort cannot be caught, and authentication runs on the blocking
        // thread pool. Returning a still-impersonating worker would reuse the
        // Engine user's token for WFP, route, DNS, and system-proxy work.
        assert!(std::mem::needs_drop::<RevertGuard>());
        assert!(revert_status_requires_abort(0));
        assert!(!revert_status_requires_abort(1));
    }

    #[test]
    fn signer_fingerprint_accepts_common_display_formats() {
        let compact = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";
        let separated = compact
            .as_bytes()
            .chunks(2)
            .map(|pair| std::str::from_utf8(pair).expect("hex"))
            .collect::<Vec<_>>()
            .join(":");
        assert_eq!(
            SignerFingerprint::parse(compact).expect("compact"),
            SignerFingerprint::parse(&separated).expect("separated")
        );
    }

    #[test]
    fn release_policy_never_accepts_an_unpinned_client() {
        if cfg!(debug_assertions) {
            let policy = CallerPolicy::new(
                vec![PathBuf::from(r"C:\Program Files\Usque\usque-engine.exe")],
                None,
                true,
            );
            assert!(policy.is_ok());
        }
        assert!(
            CallerPolicy::new(
                vec![PathBuf::from(r"C:\Program Files\Usque\usque-engine.exe")],
                None,
                false,
            )
            .is_err()
        );
    }
}
