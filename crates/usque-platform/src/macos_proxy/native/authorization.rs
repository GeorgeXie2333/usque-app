//! Process-local Authorization Services ownership. Tests inject only the three
//! Security functions; they never touch authorizationd or network preferences.
use super::*;
use std::ffi::c_char;

const INTERACTION_ALLOWED: u32 = 1;
const EXTEND_RIGHTS: u32 = 2;
// This is the right checked by Apple's SCHelper for SCPreferences operations.
// Requesting only system.preferences can cause another prompt inside SCHelper.
const NETWORK_RIGHT: &std::ffi::CStr = c"system.services.systemconfiguration.network";

#[repr(C)]
struct Item {
    name: *const c_char,
    length: usize,
    value: *mut c_void,
    flags: u32,
}
#[repr(C)]
struct Items {
    count: u32,
    items: *mut Item,
}
type Create = unsafe extern "C" fn(*const Items, *const Items, u32, *mut *mut c_void) -> i32;
type CopyRights =
    unsafe extern "C" fn(*mut c_void, *const Items, *const Items, u32, *mut *mut Items) -> i32;
type Free = unsafe extern "C" fn(*mut c_void, u32) -> i32;

#[link(name = "Security", kind = "framework")]
unsafe extern "C" {
    fn AuthorizationCreate(
        rights: *const Items,
        environment: *const Items,
        flags: u32,
        output: *mut *mut c_void,
    ) -> i32;
    fn AuthorizationCopyRights(
        authorization: *mut c_void,
        rights: *const Items,
        environment: *const Items,
        flags: u32,
        output: *mut *mut Items,
    ) -> i32;
    fn AuthorizationFree(authorization: *mut c_void, flags: u32) -> i32;
}
#[derive(Clone, Copy)]
struct Api {
    create: Create,
    copy_rights: CopyRights,
    free: Free,
}
const SECURITY: Api = Api {
    create: AuthorizationCreate,
    copy_rights: AuthorizationCopyRights,
    free: AuthorizationFree,
};

pub(super) struct Authorization {
    handle: *mut c_void,
    api: Api,
}
// SAFETY: Authorization Services handles are process-wide opaque references.
// NativeBackend's mutex serializes their use and release; no client receives one.
unsafe impl Send for Authorization {}
impl Authorization {
    pub(super) fn create() -> Result<Self> {
        Self::with_api(SECURITY)
    }
    fn with_api(api: Api) -> Result<Self> {
        let mut handle = ptr::null_mut();
        // SAFETY: output is writable; null rights/environment create only an
        // empty reference. This call cannot acquire rights or display a prompt.
        status(unsafe { (api.create)(ptr::null(), ptr::null(), 0, &mut handle) })?;
        if handle.is_null() {
            return Err(Error::Unavailable);
        }
        Ok(Self { handle, api })
    }
    pub(super) fn require_network_right(&self, interactive: bool) -> Result<()> {
        let mut item = Item {
            name: NETWORK_RIGHT.as_ptr(),
            length: 0,
            value: ptr::null_mut(),
            flags: 0,
        };
        let rights = Items {
            count: 1,
            items: &mut item,
        };
        let flags = EXTEND_RIGHTS | if interactive { INTERACTION_ALLOWED } else { 0 };
        // SAFETY: this object owns the reference; the bounded right set remains
        // live through the call. Only explicit user actions permit interaction.
        // No password, credential environment or external form enters Usque.
        status(unsafe {
            (self.api.copy_rights)(self.handle, &rights, ptr::null(), flags, ptr::null_mut())
        })
    }
    pub(super) fn as_ptr(&self) -> *mut c_void {
        self.handle
    }
}
impl Drop for Authorization {
    fn drop(&mut self) {
        // SAFETY: the sole owner releases its handle once. Defaults do not
        // destroy shared login-session rights or modify system authorization rules.
        unsafe {
            (self.api.free)(self.handle, 0);
        }
    }
}
fn status(status: i32) -> Result<()> {
    match status {
        0 => Ok(()),
        -60006 => Err(Error::PermissionCancelled),
        -60005 | -60007 => Err(Error::PermissionRequired),
        _ => Err(Error::Unavailable),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, ffi::CStr};

    #[derive(Default)]
    struct FakeSecurity {
        creates: usize,
        frees: usize,
        prompts: usize,
        granted: bool,
        cancel_once: bool,
        requests: Vec<u32>,
    }
    thread_local! {
        static STATE: RefCell<FakeSecurity> = RefCell::default();
    }
    unsafe extern "C" fn create(
        rights: *const Items,
        environment: *const Items,
        flags: u32,
        output: *mut *mut c_void,
    ) -> i32 {
        assert!(rights.is_null() && environment.is_null());
        assert_eq!(flags, 0);
        STATE.with_borrow_mut(|state| state.creates += 1);
        // SAFETY: with_api supplies writable output storage; this synthetic
        // non-null handle is never dereferenced or passed to native functions.
        unsafe { *output = ptr::dangling_mut::<u8>().cast() };
        0
    }
    unsafe extern "C" fn copy_rights(
        handle: *mut c_void,
        rights: *const Items,
        environment: *const Items,
        flags: u32,
        output: *mut *mut Items,
    ) -> i32 {
        assert!(!handle.is_null());
        assert!(environment.is_null() && output.is_null());
        // SAFETY: require_network_right supplies this live, single-item set.
        let rights = unsafe { &*rights };
        assert_eq!(rights.count, 1);
        // SAFETY: the single item and its static C string remain live.
        let item = unsafe { &*rights.items };
        // SAFETY: the right name is the static, NUL-terminated NETWORK_RIGHT.
        let name = unsafe { CStr::from_ptr(item.name) };
        assert_eq!(
            name.to_bytes(),
            b"system.services.systemconfiguration.network"
        );
        assert_eq!(item.length, 0);
        assert!(item.value.is_null());
        assert_eq!(item.flags, 0);
        STATE.with_borrow_mut(|state| {
            state.requests.push(flags);
            if state.granted {
                return 0;
            }
            if flags & INTERACTION_ALLOWED == 0 {
                return -60007;
            }
            state.prompts += 1;
            if state.cancel_once {
                state.cancel_once = false;
                return -60006;
            }
            state.granted = true;
            0
        })
    }
    unsafe extern "C" fn free(handle: *mut c_void, flags: u32) -> i32 {
        assert!(!handle.is_null());
        assert_eq!(flags, 0);
        STATE.with_borrow_mut(|state| state.frees += 1);
        0
    }
    fn authorization() -> Authorization {
        STATE.set(FakeSecurity::default());
        Authorization::with_api(Api {
            create,
            copy_rights,
            free,
        })
        .unwrap()
    }

    #[test]
    fn connect_disconnect_reconnect_reuse_one_network_authorization() {
        let auth = authorization();
        auth.require_network_right(true).unwrap();
        auth.require_network_right(false).unwrap();
        auth.require_network_right(true).unwrap();
        auth.require_network_right(false).unwrap();
        drop(auth);
        STATE.with_borrow(|state| {
            assert_eq!((state.creates, state.prompts, state.frees), (1, 1, 1));
            assert_eq!(state.requests, [3, 2, 3, 2]);
        });
    }

    #[test]
    fn cancelled_connect_can_be_retried_without_a_second_handle() {
        let auth = authorization();
        STATE.with_borrow_mut(|state| state.cancel_once = true);
        assert_eq!(
            auth.require_network_right(true),
            Err(Error::PermissionCancelled)
        );
        assert_eq!(
            auth.require_network_right(false),
            Err(Error::PermissionRequired)
        );
        auth.require_network_right(true).unwrap();
        STATE.with_borrow(|state| {
            assert_eq!((state.creates, state.prompts), (1, 2));
            assert_eq!(state.requests, [3, 2, 3]);
        });
    }

    #[test]
    fn revoked_rights_fail_silently_until_explicit_retry() {
        let auth = authorization();
        auth.require_network_right(true).unwrap();
        STATE.with_borrow_mut(|state| state.granted = false);
        assert_eq!(
            auth.require_network_right(false),
            Err(Error::PermissionRequired)
        );
        STATE.with_borrow(|state| assert_eq!(state.prompts, 1));
        auth.require_network_right(true).unwrap();
        STATE.with_borrow(|state| assert_eq!(state.prompts, 2));
    }
}
