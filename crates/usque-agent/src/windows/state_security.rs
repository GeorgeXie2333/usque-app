//! ACL hardening for the privileged Agent recovery journal.
//!
//! The journal is security-sensitive because it is the only durable map from
//! persistent WFP/routes/DNS mutations to their exact rollback receipts. The
//! containing directory and any existing journal are therefore restricted to
//! LocalSystem and Administrators before the journal is read.

use std::{
    fs, io,
    os::windows::{ffi::OsStrExt, fs::MetadataExt},
    path::{Path, PathBuf},
    ptr,
};

use thiserror::Error;
use windows_sys::Win32::{
    Foundation::{LocalFree, WIN32_ERROR},
    Security::{
        ACL,
        Authorization::{
            ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1, SE_FILE_OBJECT,
            SetNamedSecurityInfoW,
        },
        DACL_SECURITY_INFORMATION, GetSecurityDescriptorDacl, PROTECTED_DACL_SECURITY_INFORMATION,
        PSECURITY_DESCRIPTOR,
    },
};

const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
const AGENT_STATE_SDDL: &str = "D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)";

pub fn secure_agent_state_path(journal_path: &Path) -> Result<(), StateSecurityError> {
    if !journal_path.is_absolute() || journal_path.file_name().is_none() {
        return Err(StateSecurityError::InvalidPath(journal_path.to_path_buf()));
    }
    let directory = journal_path
        .parent()
        .ok_or_else(|| StateSecurityError::InvalidPath(journal_path.to_path_buf()))?;

    reject_existing_reparse_points(directory)?;
    fs::create_dir_all(directory)?;
    reject_existing_reparse_points(directory)?;
    apply_protected_dacl(directory)?;

    if journal_path.exists() {
        let metadata = fs::symlink_metadata(journal_path)?;
        if !metadata.is_file() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(StateSecurityError::UnsafeEntry(journal_path.to_path_buf()));
        }
        apply_protected_dacl(journal_path)?;
    }
    Ok(())
}

fn reject_existing_reparse_points(path: &Path) -> Result<(), StateSecurityError> {
    let mut current = Some(path);
    while let Some(candidate) = current {
        if candidate.exists() {
            let metadata = fs::symlink_metadata(candidate)?;
            if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                return Err(StateSecurityError::UnsafeEntry(candidate.to_path_buf()));
            }
        }
        current = candidate.parent();
    }
    Ok(())
}

fn apply_protected_dacl(path: &Path) -> Result<(), StateSecurityError> {
    let descriptor = SecurityDescriptor::from_sddl(AGENT_STATE_SDDL)?;
    let mut present = 0;
    let mut defaulted = 0;
    let mut dacl: *mut ACL = ptr::null_mut();
    // SAFETY: descriptor remains live and all output pointers are valid.
    if unsafe { GetSecurityDescriptorDacl(descriptor.0, &mut present, &mut dacl, &mut defaulted) }
        == 0
        || present == 0
        || dacl.is_null()
    {
        return Err(StateSecurityError::Windows(
            "GetSecurityDescriptorDacl",
            io::Error::last_os_error(),
        ));
    }
    let wide = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    // SAFETY: the path and descriptor-owned DACL remain live for the call.
    let status = unsafe {
        SetNamedSecurityInfoW(
            wide.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            ptr::null_mut(),
            ptr::null_mut(),
            dacl,
            ptr::null(),
        )
    };
    if status == WIN32_ERROR::default() {
        Ok(())
    } else {
        Err(StateSecurityError::Windows(
            "SetNamedSecurityInfoW",
            io::Error::from_raw_os_error(status as i32),
        ))
    }
}

struct SecurityDescriptor(PSECURITY_DESCRIPTOR);

impl SecurityDescriptor {
    fn from_sddl(sddl: &str) -> Result<Self, StateSecurityError> {
        let wide = sddl
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
        // SAFETY: wide is null-terminated and descriptor is writable.
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                wide.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                ptr::null_mut(),
            )
        } == 0
        {
            return Err(StateSecurityError::Windows(
                "ConvertStringSecurityDescriptorToSecurityDescriptorW",
                io::Error::last_os_error(),
            ));
        }
        Ok(Self(descriptor))
    }
}

impl Drop for SecurityDescriptor {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: SDDL conversion allocates this descriptor with LocalAlloc.
            unsafe {
                LocalFree(self.0);
            }
        }
    }
}

#[derive(Debug, Error)]
pub enum StateSecurityError {
    #[error("Agent recovery journal path must be an absolute file path: {0}")]
    InvalidPath(PathBuf),
    #[error("Agent recovery state path contains a reparse point or non-file entry: {0}")]
    UnsafeEntry(PathBuf),
    #[error("Agent recovery state filesystem failed: {0}")]
    Io(#[from] io::Error),
    #[error("Windows {0} failed while securing Agent recovery state: {1}")]
    Windows(&'static str, io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_acl_has_no_regular_user_or_everyone_ace() {
        assert!(AGENT_STATE_SDDL.contains(";;;SY)"));
        assert!(AGENT_STATE_SDDL.contains(";;;BA)"));
        assert!(!AGENT_STATE_SDDL.contains(";;;WD)"));
        assert!(!AGENT_STATE_SDDL.contains(";;;AU)"));
        assert!(!AGENT_STATE_SDDL.contains(";;;BU)"));
    }

    #[test]
    fn relative_journal_path_is_rejected_before_filesystem_mutation() {
        assert!(matches!(
            secure_agent_state_path(Path::new("recovery.json")),
            Err(StateSecurityError::InvalidPath(_))
        ));
    }
}
