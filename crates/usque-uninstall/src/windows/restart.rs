//! Restart only after the completed-uninstall UI's separate final confirmation.
use std::{mem::size_of, ptr};
use windows_sys::Win32::{
    Foundation::{CloseHandle, ERROR_SUCCESS, GetLastError, HANDLE, LUID, SetLastError},
    Security::{
        AdjustTokenPrivileges, LUID_AND_ATTRIBUTES, LookupPrivilegeValueW, SE_PRIVILEGE_ENABLED,
        SE_SHUTDOWN_NAME, TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY,
    },
    System::{
        Shutdown::{
            EWX_REBOOT, ExitWindowsEx, SHTDN_REASON_FLAG_PLANNED, SHTDN_REASON_MAJOR_APPLICATION,
            SHTDN_REASON_MINOR_INSTALLATION,
        },
        Threading::{GetCurrentProcess, OpenProcessToken},
    },
};

struct ShutdownPrivilege {
    token: HANDLE,
    previous: TOKEN_PRIVILEGES,
}

impl ShutdownPrivilege {
    fn enable() -> Result<Self, u32> {
        let mut token = ptr::null_mut();
        // SAFETY: only this process token is opened; this neither changes the
        // user identity nor requests elevation through another executable.
        if unsafe {
            OpenProcessToken(
                GetCurrentProcess(),
                TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY,
                &mut token,
            )
        } == 0
        {
            // SAFETY: read immediately after the failed token call.
            return Err(unsafe { GetLastError() });
        }
        let mut guard = Self {
            token,
            previous: TOKEN_PRIVILEGES::default(),
        };
        let mut luid = LUID::default();
        // SAFETY: the well-known privilege name is a static nul-terminated
        // string, and luid is a writable output value.
        if unsafe { LookupPrivilegeValueW(ptr::null(), SE_SHUTDOWN_NAME, &mut luid) } == 0 {
            // SAFETY: read immediately after the failed lookup.
            return Err(unsafe { GetLastError() });
        }
        let requested = TOKEN_PRIVILEGES {
            PrivilegeCount: 1,
            Privileges: [LUID_AND_ATTRIBUTES {
                Luid: luid,
                Attributes: SE_PRIVILEGE_ENABLED,
            }],
        };
        let mut returned_length = 0;
        // SAFETY: both privilege structures describe at most one privilege.
        // Capture its previous state for restoration on every return path.
        let (changed, error) = unsafe {
            SetLastError(ERROR_SUCCESS);
            let changed = AdjustTokenPrivileges(
                token,
                0,
                &requested,
                size_of::<TOKEN_PRIVILEGES>() as u32,
                &mut guard.previous,
                &mut returned_length,
            );
            (changed, GetLastError())
        };
        // BOOL success can still mean ERROR_NOT_ALL_ASSIGNED. Never attempt a
        // reboot when this user was not granted the required privilege.
        if changed == 0 || error != ERROR_SUCCESS {
            return Err(error.max(1));
        }
        Ok(guard)
    }
}

impl Drop for ShutdownPrivilege {
    fn drop(&mut self) {
        // SAFETY: the guard owns this token. Restore only the privilege changed
        // above, then release the handle. No forceful shutdown flags are used.
        unsafe {
            if self.previous.PrivilegeCount != 0 {
                AdjustTokenPrivileges(
                    self.token,
                    0,
                    &self.previous,
                    0,
                    ptr::null_mut(),
                    ptr::null_mut(),
                );
            }
            CloseHandle(self.token);
        }
    }
}

pub(super) fn request() -> Result<(), u32> {
    let _privilege = ShutdownPrivilege::enable()?;
    // SAFETY: reachable only from RestartFlow's explicit live confirmation.
    // EWX_REBOOT preserves Windows' unsaved-work prompts: neither EWX_FORCE nor
    // EWX_FORCEIFHUNG is allowed. Success means requested, not completed.
    if unsafe {
        ExitWindowsEx(
            EWX_REBOOT,
            SHTDN_REASON_MAJOR_APPLICATION
                | SHTDN_REASON_MINOR_INSTALLATION
                | SHTDN_REASON_FLAG_PLANNED,
        )
    } == 0
    {
        // SAFETY: read immediately after the failed reboot request.
        Err(unsafe { GetLastError() })
    } else {
        Ok(())
    }
}
