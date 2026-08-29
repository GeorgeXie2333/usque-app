//! Shared offline Authenticode verification for installed Usque executables.
//!
//! Public releases use a fixed self-signed signing identity. WinVerifyTrust
//! validates the embedded digest and signature, while the caller compares the
//! embedded certificate SHA-256 fingerprint with a trusted Usque executable.

use std::{ffi::c_void, io, mem, os::windows::ffi::OsStrExt, path::Path, ptr};

use thiserror::Error;
use windows_sys::Win32::Security::WinTrust::{
    WINTRUST_ACTION_GENERIC_VERIFY_V2, WINTRUST_DATA, WINTRUST_FILE_INFO,
    WTD_CACHE_ONLY_URL_RETRIEVAL, WTD_CHOICE_FILE, WTD_REVOCATION_CHECK_NONE,
    WTD_STATEACTION_CLOSE, WTD_STATEACTION_VERIFY, WTD_UI_NONE, WTD_UICONTEXT_EXECUTE,
    WinVerifyTrust,
};
use windows_sys::Win32::{
    Foundation::CERT_E_UNTRUSTEDROOT,
    Security::Cryptography::{
        CERT_FIND_SUBJECT_CERT, CERT_INFO, CERT_QUERY_CONTENT_FLAG_PKCS7_SIGNED_EMBED,
        CERT_QUERY_FORMAT_FLAG_BINARY, CERT_QUERY_OBJECT_FILE, CERT_SHA256_HASH_PROP_ID,
        CMSG_SIGNER_INFO, CMSG_SIGNER_INFO_PARAM, CertCloseStore, CertFindCertificateInStore,
        CertFreeCertificateContext, CertGetCertificateContextProperty, CryptMsgClose,
        CryptMsgGetParam, CryptQueryObject, HCERTSTORE, PKCS_7_ASN_ENCODING, X509_ASN_ENCODING,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignerFingerprint([u8; 32]);

impl SignerFingerprint {
    pub fn parse(value: &str) -> Result<Self, AuthenticodeError> {
        let compact = value
            .bytes()
            .filter(|byte| !matches!(byte, b':' | b'-' | b' '))
            .collect::<Vec<_>>();
        if compact.len() != 64 {
            return Err(AuthenticodeError::InvalidFingerprint);
        }
        let mut output = [0_u8; 32];
        for (index, pair) in compact.chunks_exact(2).enumerate() {
            let high = hex_nibble(pair[0]).ok_or(AuthenticodeError::InvalidFingerprint)?;
            let low = hex_nibble(pair[1]).ok_or(AuthenticodeError::InvalidFingerprint)?;
            output[index] = high << 4 | low;
        }
        Ok(Self(output))
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

pub fn verify_same_signer(reference: &Path, candidate: &Path) -> Result<(), AuthenticodeError> {
    verify_authenticode(reference)?;
    verify_authenticode(candidate)?;
    if signer_fingerprint(reference)? != signer_fingerprint(candidate)? {
        return Err(AuthenticodeError::SignerMismatch);
    }
    Ok(())
}

pub fn verify_authenticode(path: &Path) -> Result<(), AuthenticodeError> {
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
    // SAFETY: both WinTrust structures have their documented sizes and remain
    // alive through verification and state cleanup.
    let status = unsafe {
        WinVerifyTrust(
            ptr::null_mut(),
            &mut action,
            (&mut data as *mut WINTRUST_DATA).cast(),
        )
    };
    data.dwStateAction = WTD_STATEACTION_CLOSE;
    // SAFETY: this closes only state allocated by the preceding verification.
    unsafe {
        WinVerifyTrust(
            ptr::null_mut(),
            &mut action,
            (&mut data as *mut WINTRUST_DATA).cast(),
        );
    }
    if !authenticode_status_is_acceptable(status) {
        return Err(AuthenticodeError::Verification(status));
    }
    Ok(())
}

pub fn signer_fingerprint(path: &Path) -> Result<SignerFingerprint, AuthenticodeError> {
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
    // SAFETY: this first call obtains the exact variable-size signer buffer.
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
    // SAFETY: the buffer is exactly the size requested by Crypt32.
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
    // SAFETY: CryptMsgGetParam populated CMSG_SIGNER_INFO at the buffer head.
    let signer = unsafe { &*signer_buffer.as_ptr().cast::<CMSG_SIGNER_INFO>() };
    let certificate_info = CERT_INFO {
        Issuer: signer.Issuer,
        SerialNumber: signer.SerialNumber,
        ..CERT_INFO::default()
    };
    // SAFETY: certificate_info references the live signer buffer; the returned
    // context is independently reference counted.
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

pub(crate) fn authenticode_status_is_acceptable(status: i32) -> bool {
    // Public releases deliberately use a fixed self-signed identity without
    // adding it to the machine root store. Every other chain/digest result is
    // rejected, and callers pin the extracted certificate fingerprint.
    status == 0 || status == CERT_E_UNTRUSTEDROOT
}

fn wide_path(path: &Path) -> Vec<u16> {
    path.as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn hex_nibble(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn last_error(operation: &'static str) -> AuthenticodeError {
    AuthenticodeError::Windows(operation, io::Error::last_os_error())
}

struct CertificateStore(HCERTSTORE);

impl Drop for CertificateStore {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: CryptQueryObject transferred this store handle.
            unsafe { CertCloseStore(self.0, 0) };
        }
    }
}

struct CryptographicMessage(*mut c_void);

impl Drop for CryptographicMessage {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: CryptQueryObject transferred this message handle.
            unsafe { CryptMsgClose(self.0) };
        }
    }
}

struct CertificateContext(*mut windows_sys::Win32::Security::Cryptography::CERT_CONTEXT);

impl Drop for CertificateContext {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: CertFindCertificateInStore returned this context.
            unsafe { CertFreeCertificateContext(self.0) };
        }
    }
}

#[derive(Debug, Error)]
pub enum AuthenticodeError {
    #[error("Windows {0} failed: {1}")]
    Windows(&'static str, io::Error),
    #[error("Authenticode verification failed with HRESULT 0x{0:08x}")]
    Verification(i32),
    #[error("the embedded signer certificate does not match the trusted Usque identity")]
    SignerMismatch,
    #[error("signer fingerprint must contain exactly 64 hexadecimal digits")]
    InvalidFingerprint,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_accepts_common_display_formats() {
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
    fn self_signed_policy_accepts_only_the_untrusted_root_chain_result() {
        assert!(authenticode_status_is_acceptable(0));
        assert!(authenticode_status_is_acceptable(CERT_E_UNTRUSTEDROOT));
        assert!(!authenticode_status_is_acceptable(0x8009_6010_u32 as i32));
        assert!(!authenticode_status_is_acceptable(0x800b_0101_u32 as i32));
    }
}
