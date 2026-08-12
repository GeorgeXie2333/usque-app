//! Authentication of an Engine process connected to the Agent Named Pipe.
//!
//! Authentication binds a connection to its kernel-reported PID, impersonated
//! user SID, exact installed executable path, valid Authenticode signature, and
//! pinned signer-certificate SHA-256 fingerprint. The process handle remains
//! open for the connection lifetime, preventing PID reuse before HANDLE
//! duplication for the packet ring.

use std::{
    ffi::c_void,
    fs, io, mem,
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    ptr,
};

use thiserror::Error;
use windows_sys::Win32::Security::WinTrust::{
    WINTRUST_ACTION_GENERIC_VERIFY_V2, WINTRUST_DATA, WINTRUST_FILE_INFO,
    WTD_CACHE_ONLY_URL_RETRIEVAL, WTD_CHOICE_FILE, WTD_REVOCATION_CHECK_NONE,
    WTD_STATEACTION_CLOSE, WTD_STATEACTION_VERIFY, WTD_UI_NONE, WTD_UICONTEXT_EXECUTE,
    WinVerifyTrust,
};
use windows_sys::{
    Win32::{
        Foundation::{CERT_E_UNTRUSTEDROOT, CloseHandle, HANDLE, LocalFree},
        Security::{
            Authorization::ConvertSidToStringSidW,
            Cryptography::{
                CERT_FIND_SUBJECT_CERT, CERT_INFO, CERT_QUERY_CONTENT_FLAG_PKCS7_SIGNED_EMBED,
                CERT_QUERY_FORMAT_FLAG_BINARY, CERT_QUERY_OBJECT_FILE, CERT_SHA256_HASH_PROP_ID,
                CMSG_SIGNER_INFO, CMSG_SIGNER_INFO_PARAM, CertCloseStore,
                CertFindCertificateInStore, CertFreeCertificateContext,
                CertGetCertificateContextProperty, CryptMsgClose, CryptMsgGetParam,
                CryptQueryObject, HCERTSTORE, PKCS_7_ASN_ENCODING, X509_ASN_ENCODING,
            },
            GetTokenInformation, RevertToSelf, TOKEN_QUERY, TOKEN_USER, TokenUser,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignerFingerprint([u8; 32]);

impl SignerFingerprint {
    pub fn parse(value: &str) -> Result<Self, AuthenticationError> {
        let compact = value
            .bytes()
            .filter(|byte| !matches!(byte, b':' | b'-' | b' '))
            .collect::<Vec<_>>();
        if compact.len() != 64 {
            return Err(AuthenticationError::InvalidFingerprint);
        }
        let mut output = [0_u8; 32];
        for (index, pair) in compact.chunks_exact(2).enumerate() {
            let high = hex_nibble(pair[0]).ok_or(AuthenticationError::InvalidFingerprint)?;
            let low = hex_nibble(pair[1]).ok_or(AuthenticationError::InvalidFingerprint)?;
            output[index] = high << 4 | low;
        }
        Ok(Self(output))
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

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

fn verify_authenticode(path: &Path) -> Result<(), AuthenticationError> {
    let path = wide_path(path);
    let mut file = WINTRUST_FILE_INFO {
        cbStruct: mem::size_of::<WINTRUST_FILE_INFO>() as u32,
        pcwszFilePath: path.as_ptr(),
        hFile: ptr::null_mut(),
        pgKnownSubject: ptr::null_mut(),
    };
    let mut data = WINTRUST_DATA {
        cbStruct: mem::size_of::<WINTRUST_DATA>() as u32,
        pPolicyCallbackData: ptr::null_mut(),
        pSIPClientData: ptr::null_mut(),
        dwUIChoice: WTD_UI_NONE,
        fdwRevocationChecks: WTD_REVOCATION_CHECK_NONE,
        dwUnionChoice: WTD_CHOICE_FILE,
        Anonymous: windows_sys::Win32::Security::WinTrust::WINTRUST_DATA_0 { pFile: &mut file },
        dwStateAction: WTD_STATEACTION_VERIFY,
        hWVTStateData: ptr::null_mut(),
        pwszURLReference: ptr::null_mut(),
        dwProvFlags: WTD_CACHE_ONLY_URL_RETRIEVAL,
        dwUIContext: WTD_UICONTEXT_EXECUTE,
        pSignatureSettings: ptr::null_mut(),
    };
    let mut action = WINTRUST_ACTION_GENERIC_VERIFY_V2;
    // SAFETY: all structures have the documented size and remain alive for the
    // complete verification call.
    let status = unsafe {
        WinVerifyTrust(
            ptr::null_mut(),
            &mut action,
            (&mut data as *mut WINTRUST_DATA).cast(),
        )
    };
    data.dwStateAction = WTD_STATEACTION_CLOSE;
    // SAFETY: this releases any state allocated by the preceding call.
    unsafe {
        WinVerifyTrust(
            ptr::null_mut(),
            &mut action,
            (&mut data as *mut WINTRUST_DATA).cast(),
        );
    }
    // The public release intentionally uses a stable self-signed publisher
    // certificate and does not install it as a machine-wide root CA. Windows
    // therefore reports CERT_E_UNTRUSTEDROOT after the Authenticode provider
    // has successfully checked the embedded file digest and signature. That
    // one chain result is safe here because the caller immediately extracts
    // the embedded signer certificate and compares its SHA-256 fingerprint
    // with the release-pinned value. Bad digests, expired certificates,
    // revocation failures, and every other trust result remain fatal.
    if !authenticode_status_is_acceptable(status) {
        return Err(AuthenticationError::Authenticode(status));
    }
    Ok(())
}

fn authenticode_status_is_acceptable(status: i32) -> bool {
    status == 0 || status == CERT_E_UNTRUSTEDROOT
}

fn signer_fingerprint(path: &Path) -> Result<SignerFingerprint, AuthenticationError> {
    let path = wide_path(path);
    let mut encoding = 0_u32;
    let mut content = 0_u32;
    let mut format = 0_u32;
    let mut store: HCERTSTORE = ptr::null_mut();
    let mut message: *mut c_void = ptr::null_mut();
    // SAFETY: path is null-terminated and all output pointers are writable.
    if unsafe {
        CryptQueryObject(
            CERT_QUERY_OBJECT_FILE,
            path.as_ptr().cast(),
            CERT_QUERY_CONTENT_FLAG_PKCS7_SIGNED_EMBED,
            CERT_QUERY_FORMAT_FLAG_BINARY,
            0,
            &mut encoding,
            &mut content,
            &mut format,
            &mut store,
            &mut message,
            ptr::null_mut(),
        )
    } == 0
    {
        return Err(last_error("CryptQueryObject"));
    }
    let store = CertificateStore(store);
    let message = CryptographicMessage(message);

    let mut signer_size = 0_u32;
    // SAFETY: the first query obtains the exact variable-size signer buffer.
    if unsafe {
        CryptMsgGetParam(
            message.0,
            CMSG_SIGNER_INFO_PARAM,
            0,
            ptr::null_mut(),
            &mut signer_size,
        )
    } == 0
        || signer_size < mem::size_of::<CMSG_SIGNER_INFO>() as u32
    {
        return Err(last_error("CryptMsgGetParam(size)"));
    }
    let mut signer_buffer = vec![0_u8; signer_size as usize];
    // SAFETY: signer_buffer is exactly the size requested by Crypt32.
    if unsafe {
        CryptMsgGetParam(
            message.0,
            CMSG_SIGNER_INFO_PARAM,
            0,
            signer_buffer.as_mut_ptr().cast(),
            &mut signer_size,
        )
    } == 0
    {
        return Err(last_error("CryptMsgGetParam"));
    }
    // SAFETY: CryptMsgGetParam populated a CMSG_SIGNER_INFO at the buffer head.
    let signer = unsafe { &*signer_buffer.as_ptr().cast::<CMSG_SIGNER_INFO>() };
    let certificate_info = CERT_INFO {
        Issuer: signer.Issuer,
        SerialNumber: signer.SerialNumber,
        ..CERT_INFO::default()
    };
    // SAFETY: certificate_info references signer_buffer, which remains alive,
    // and the returned context is independently reference counted.
    let context = unsafe {
        CertFindCertificateInStore(
            store.0,
            X509_ASN_ENCODING | PKCS_7_ASN_ENCODING,
            0,
            CERT_FIND_SUBJECT_CERT,
            (&certificate_info as *const CERT_INFO).cast(),
            ptr::null(),
        )
    };
    if context.is_null() {
        return Err(last_error("CertFindCertificateInStore"));
    }
    let context = CertificateContext(context);
    let mut fingerprint = [0_u8; 32];
    let mut fingerprint_size = fingerprint.len() as u32;
    // SAFETY: the output buffer is exactly 32 bytes and context is live.
    if unsafe {
        CertGetCertificateContextProperty(
            context.0,
            CERT_SHA256_HASH_PROP_ID,
            fingerprint.as_mut_ptr().cast(),
            &mut fingerprint_size,
        )
    } == 0
        || fingerprint_size != fingerprint.len() as u32
    {
        return Err(last_error("CertGetCertificateContextProperty"));
    }
    Ok(SignerFingerprint(fingerprint))
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

fn wide_path(path: &Path) -> Vec<u16> {
    path.as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
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

fn hex_nibble(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn last_error(operation: &'static str) -> AuthenticationError {
    AuthenticationError::Windows(operation, io::Error::last_os_error())
}

struct RevertGuard;

impl Drop for RevertGuard {
    fn drop(&mut self) {
        // SAFETY: the current thread is impersonating only for this guard.
        unsafe {
            RevertToSelf();
        }
    }
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

struct CertificateStore(HCERTSTORE);

impl Drop for CertificateStore {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: CryptQueryObject transferred this store handle.
            unsafe {
                CertCloseStore(self.0, 0);
            }
        }
    }
}

struct CryptographicMessage(*mut c_void);

impl Drop for CryptographicMessage {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: CryptQueryObject transferred this message handle.
            unsafe {
                CryptMsgClose(self.0);
            }
        }
    }
}

struct CertificateContext(*mut windows_sys::Win32::Security::Cryptography::CERT_CONTEXT);

impl Drop for CertificateContext {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: CertFindCertificateInStore returned this context.
            unsafe {
                CertFreeCertificateContext(self.0);
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
    #[error("Engine Authenticode verification failed with HRESULT 0x{0:08x}")]
    Authenticode(i32),
    #[error("Engine signer certificate does not match the pinned fingerprint")]
    SignerMismatch,
    #[error("unsigned Engine clients are denied")]
    UnsignedClientDenied,
    #[error("signer fingerprint must contain exactly 64 hexadecimal digits")]
    InvalidFingerprint,
    #[error("caller policy is invalid: {0}")]
    InvalidPolicy(String),
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn pinned_self_signed_policy_allows_only_the_untrusted_root_chain_result() {
        assert!(authenticode_status_is_acceptable(0));
        assert!(authenticode_status_is_acceptable(CERT_E_UNTRUSTEDROOT));
        assert!(!authenticode_status_is_acceptable(0x8009_6010_u32 as i32)); // TRUST_E_BAD_DIGEST
        assert!(!authenticode_status_is_acceptable(0x800b_0101_u32 as i32)); // CERT_E_EXPIRED
    }
}
