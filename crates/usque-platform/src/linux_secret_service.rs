//! Binary Secret Service credentials through the system libsecret library.
//!
//! No secrets are placed in files, environment variables or process arguments.
//! A missing service, locked collection or dismissed prompt remains retryable.
//! The library is loaded at use time so a missing keyring does not prevent the
//! GUI from opening its repair guidance. Operations are cancellable and bounded.

use std::{
    ffi::{CString, c_char, c_int, c_void},
    ptr,
    sync::{Arc, OnceLock},
    time::Duration,
};

use async_trait::async_trait;
use libloading::Library;
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::{SecretRecord, SecretVault, VaultError};

const MAX_SECRET_BYTES: usize = 2560;
const OPERATION_TIMEOUT: Duration = Duration::from_secs(120);
const APPLICATION: &std::ffi::CStr = c"io.github.georgexie2333.usque.identity";

#[derive(Debug, Default, Clone, Copy)]
pub struct LinuxSecretServiceVault;

#[async_trait]
impl SecretVault for LinuxSecretServiceVault {
    async fn put(
        &self,
        profile: Uuid,
        record: SecretRecord,
        value: &[u8],
    ) -> Result<(), VaultError> {
        validate_size(value.len())?;
        let value = Zeroizing::new(value.to_vec());
        operation(move |api, cancel| {
            let attributes = Attributes::new(api, profile, record)?;
            let _existing = api.search(&attributes, cancel, false)?;
            // SAFETY: value is valid for its explicit length; the content type is static.
            let secret = unsafe {
                (api.value_new)(
                    value.as_ptr().cast(),
                    value.len() as isize,
                    c"application/octet-stream".as_ptr(),
                )
            };
            if secret.is_null() {
                return Err(unavailable("value allocation failed"));
            }
            let secret = SecretValue {
                api,
                pointer: secret,
            };
            let mut error = ptr::null_mut();
            // SAFETY: live owned attributes, SecretValue and cancellable outlive this synchronous call.
            let stored = unsafe {
                (api.store)(
                    ptr::null(),
                    attributes.pointer,
                    ptr::null(),
                    c"Usque account credential".as_ptr(),
                    secret.pointer,
                    cancel,
                    &mut error,
                )
            };
            api.check_error(error)?;
            if stored == 0 {
                return Err(unavailable(
                    "storage was cancelled or the collection is locked",
                ));
            }
            Ok(())
        })
        .await
    }

    async fn get(
        &self,
        profile: Uuid,
        record: SecretRecord,
    ) -> Result<Option<Zeroizing<Vec<u8>>>, VaultError> {
        operation(move |api, cancel| {
            let attributes = Attributes::new(api, profile, record)?;
            let items = api.search(&attributes, cancel, true)?;
            let Some(item) = items.single()? else {
                return Ok(None);
            };
            // SAFETY: item is a live SecretItem owned by items; this takes a new SecretValue reference.
            let value = unsafe { (api.item_secret)(item) };
            if value.is_null() {
                return Err(unavailable(
                    "credential remained locked or could not be loaded",
                ));
            }
            let secret = SecretValue {
                api,
                pointer: value,
            };
            let mut size = 0;
            // SAFETY: secret owns the live value, and size is writable for this call.
            let bytes = unsafe { (api.value_get)(secret.pointer, &mut size) };
            validate_size(size)?;
            if bytes.is_null() {
                return Err(unavailable("keyring returned an invalid value"));
            }
            // SAFETY: libsecret returned a readable buffer of size bytes owned by secret.
            let value = unsafe { std::slice::from_raw_parts(bytes.cast::<u8>(), size) };
            Ok(Some(Zeroizing::new(value.to_vec())))
        })
        .await
    }

    async fn delete(&self, profile: Uuid, record: SecretRecord) -> Result<(), VaultError> {
        operation(move |api, cancel| {
            let attributes = Attributes::new(api, profile, record)?;
            let items = api.search(&attributes, cancel, false)?;
            for item in items.pointers()? {
                let mut error = ptr::null_mut();
                // SAFETY: each SecretItem is owned by items; the cancellable and error slot remain live.
                let deleted = unsafe { (api.item_delete)(item, cancel, &mut error) };
                api.check_error(error)?;
                if deleted == 0 {
                    return Err(unavailable("credential deletion was cancelled"));
                }
            }
            if !api
                .search(&attributes, cancel, false)?
                .pointers()?
                .is_empty()
            {
                return Err(unavailable("credential deletion could not be verified"));
            }
            Ok(())
        })
        .await
    }
}

fn validate_size(size: usize) -> Result<(), VaultError> {
    if (1..=MAX_SECRET_BYTES).contains(&size) {
        Ok(())
    } else {
        Err(VaultError::InvalidSecretSize)
    }
}

fn unavailable(reason: &str) -> VaultError {
    VaultError::Unavailable(format!(
        "Secret Service: {reason}; install or unlock the session keyring and retry"
    ))
}

async fn operation<T: Send + 'static>(
    work: impl FnOnce(&Api, *mut c_void) -> Result<T, VaultError> + Send + 'static,
) -> Result<T, VaultError> {
    // GObject registers process-global type callbacks. Keep libsecret loaded
    // after the first successful call so no callback points into an unloaded DSO.
    static API: OnceLock<Arc<Api>> = OnceLock::new();
    let api = match API.get() {
        Some(api) => Arc::clone(api),
        None => {
            let candidate = Arc::new(Api::load()?);
            let _ = API.set(Arc::clone(&candidate));
            Arc::clone(API.get().unwrap_or(&candidate))
        }
    };
    // SAFETY: g_cancellable_new has no input and returns one owned GObject reference.
    let pointer = unsafe { (api.cancellable_new)() };
    if pointer.is_null() {
        return Err(unavailable("cancellation allocation failed"));
    }
    let cancellation = Arc::new(Cancellation {
        api,
        pointer: pointer as usize,
    });
    let guard = CancelOnDrop(Arc::clone(&cancellation));
    let worker = tokio::task::spawn_blocking(move || {
        work(&cancellation.api, cancellation.pointer as *mut c_void)
    });
    let result = tokio::time::timeout(OPERATION_TIMEOUT, worker).await;
    drop(guard); // Also cancels a native prompt if the Rust caller is dropped.
    result
        .map_err(|_| unavailable("operation timed out"))?
        .map_err(|_| unavailable("credential worker failed"))?
}

// GCancellable is explicitly thread safe. The pointer is never dereferenced by
// Rust and its owned GObject reference remains live until the final Arc drops.
struct Cancellation {
    api: Arc<Api>,
    pointer: usize,
}
impl Drop for Cancellation {
    fn drop(&mut self) {
        // SAFETY: this is the final owner of the reference from g_cancellable_new.
        unsafe { (self.api.object_unref)(self.pointer as *mut c_void) };
    }
}
struct CancelOnDrop(Arc<Cancellation>);
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        // SAFETY: GCancellable supports concurrent cancellation; the Arc retains its reference.
        unsafe { (self.0.api.cancellable_cancel)(self.0.pointer as *mut c_void) };
    }
}

struct Attributes<'a> {
    api: &'a Api,
    pointer: *mut c_void,
    // Hash table borrows these strings without freeing them.
    _profile: CString,
    _record: CString,
}
impl<'a> Attributes<'a> {
    fn new(api: &'a Api, profile: Uuid, record: SecretRecord) -> Result<Self, VaultError> {
        let profile = CString::new(profile.to_string()).expect("UUID has no NUL");
        let record = CString::new(record.key()).expect("record keys have no NUL");
        // SAFETY: GLib receives matching hash/equality functions for NUL-terminated UTF-8 strings.
        let pointer = unsafe { (api.hash_new)(Some(api.str_hash), Some(api.str_equal)) };
        if pointer.is_null() {
            return Err(unavailable("attribute allocation failed"));
        }
        // SAFETY: all keys are static and values outlive the table; no destroy function is registered.
        unsafe {
            (api.hash_insert)(
                pointer,
                c"application".as_ptr().cast_mut().cast(),
                APPLICATION.as_ptr().cast_mut().cast(),
            );
            (api.hash_insert)(
                pointer,
                c"profile".as_ptr().cast_mut().cast(),
                profile.as_ptr().cast_mut().cast(),
            );
            (api.hash_insert)(
                pointer,
                c"record".as_ptr().cast_mut().cast(),
                record.as_ptr().cast_mut().cast(),
            );
        }
        Ok(Self {
            api,
            pointer,
            _profile: profile,
            _record: record,
        })
    }
}
impl Drop for Attributes<'_> {
    fn drop(&mut self) {
        // SAFETY: pointer is the table owned by self, with no externally owned references.
        unsafe { (self.api.hash_unref)(self.pointer) };
    }
}
struct SecretValue<'a> {
    api: &'a Api,
    pointer: *mut c_void,
}
impl Drop for SecretValue<'_> {
    fn drop(&mut self) {
        if !self.pointer.is_null() {
            // SAFETY: this owns a reference returned by libsecret; unref securely frees the value.
            unsafe { (self.api.value_unref)(self.pointer) };
        }
    }
}

#[repr(C)]
struct GError {
    domain: u32,
    code: c_int,
    message: *mut c_char,
}
type Hash = unsafe extern "C" fn(*const c_void) -> u32;
type Equal = unsafe extern "C" fn(*const c_void, *const c_void) -> c_int;
type Store = unsafe extern "C" fn(
    *const c_void,
    *mut c_void,
    *const c_char,
    *const c_char,
    *mut c_void,
    *mut c_void,
    *mut *mut GError,
) -> c_int;
type Search = unsafe extern "C" fn(
    *mut c_void,
    *const c_void,
    *mut c_void,
    c_int,
    *mut c_void,
    *mut *mut GError,
) -> *mut GList;
#[repr(C)]
struct GList {
    data: *mut c_void,
    next: *mut GList,
    previous: *mut GList,
}
struct Items<'a> {
    api: &'a Api,
    pointer: *mut GList,
}
impl Items<'_> {
    fn pointers(&self) -> Result<Vec<*mut c_void>, VaultError> {
        let mut result = Vec::new();
        let mut node = self.pointer;
        while !node.is_null() {
            if result.len() >= 32 {
                return Err(unavailable("too many matching credential records"));
            }
            // SAFETY: the list and every item are owned by self until Drop; libsecret returns a valid GLib list.
            unsafe {
                result.push((*node).data);
                node = (*node).next;
            }
        }
        Ok(result)
    }
    fn single(&self) -> Result<Option<*mut c_void>, VaultError> {
        let items = self.pointers()?;
        if items.len() > 1 {
            return Err(unavailable("ambiguous duplicate credential records"));
        }
        Ok(items.first().copied())
    }
}
impl Drop for Items<'_> {
    fn drop(&mut self) {
        // SAFETY: list ownership (including every SecretItem reference) was transferred by search_sync.
        unsafe { (self.api.list_free_full)(self.pointer, Some(self.api.object_unref)) };
    }
}

struct Api {
    _library: Library,
    store: Store,
    search_items: Search,
    item_locked: unsafe extern "C" fn(*mut c_void) -> c_int,
    item_secret: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
    item_delete: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut *mut GError) -> c_int,
    list_free_full: unsafe extern "C" fn(*mut GList, Option<unsafe extern "C" fn(*mut c_void)>),
    value_new: unsafe extern "C" fn(*const c_char, isize, *const c_char) -> *mut c_void,
    value_get: unsafe extern "C" fn(*mut c_void, *mut usize) -> *const c_char,
    value_unref: unsafe extern "C" fn(*mut c_void),
    hash_new: unsafe extern "C" fn(Option<Hash>, Option<Equal>) -> *mut c_void,
    hash_insert: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> c_int,
    hash_unref: unsafe extern "C" fn(*mut c_void),
    str_hash: Hash,
    str_equal: Equal,
    error_free: unsafe extern "C" fn(*mut GError),
    cancellable_new: unsafe extern "C" fn() -> *mut c_void,
    cancellable_cancel: unsafe extern "C" fn(*mut c_void),
    object_unref: unsafe extern "C" fn(*mut c_void),
}

impl Api {
    fn load() -> Result<Self, VaultError> {
        // SAFETY: libsecret is the system library; its stable C ABI symbols below
        // use the documented libsecret >= 0.19 and GLib types. It is retained for
        // the entire lifetime of every function pointer and native object.
        unsafe {
            let library = Library::new("libsecret-1.so.0")
                .map_err(|_| unavailable("libsecret-1 is not installed"))?;
            macro_rules! symbol {
                ($name:literal) => {
                    *library
                        .get(concat!($name, "\0").as_bytes())
                        .map_err(|_| unavailable("libsecret-1 is incompatible"))?
                };
            }
            Ok(Self {
                store: symbol!("secret_password_storev_binary_sync"),
                search_items: symbol!("secret_service_search_sync"),
                item_locked: symbol!("secret_item_get_locked"),
                item_secret: symbol!("secret_item_get_secret"),
                item_delete: symbol!("secret_item_delete_sync"),
                list_free_full: symbol!("g_list_free_full"),
                value_new: symbol!("secret_value_new"),
                value_get: symbol!("secret_value_get"),
                value_unref: symbol!("secret_value_unref"),
                hash_new: symbol!("g_hash_table_new"),
                hash_insert: symbol!("g_hash_table_insert"),
                hash_unref: symbol!("g_hash_table_unref"),
                str_hash: symbol!("g_str_hash"),
                str_equal: symbol!("g_str_equal"),
                error_free: symbol!("g_error_free"),
                cancellable_new: symbol!("g_cancellable_new"),
                cancellable_cancel: symbol!("g_cancellable_cancel"),
                object_unref: symbol!("g_object_unref"),
                _library: library,
            })
        }
    }

    fn search(
        &self,
        attributes: &Attributes<'_>,
        cancel: *mut c_void,
        load: bool,
    ) -> Result<Items<'_>, VaultError> {
        let mut error = ptr::null_mut();
        // SECRET_SEARCH_ALL | SECRET_SEARCH_UNLOCK; loading is unnecessary for deletion.
        let flags = 2 | 4 | if load { 8 } else { 0 };
        // SAFETY: attributes/cancellable are live; NULL service asks libsecret for the session service.
        let pointer = unsafe {
            (self.search_items)(
                ptr::null_mut(),
                ptr::null(),
                attributes.pointer,
                flags,
                cancel,
                &mut error,
            )
        };
        let items = Items { api: self, pointer };
        self.check_error(error)?;
        for item in items.pointers()? {
            // SAFETY: search_sync returns referenced SecretItem instances owned by items.
            if item.is_null() || unsafe { (self.item_locked)(item) } != 0 {
                return Err(unavailable(
                    "collection is locked or the unlock prompt was cancelled",
                ));
            }
        }
        Ok(items)
    }

    fn check_error(&self, error: *mut GError) -> Result<(), VaultError> {
        if error.is_null() {
            return Ok(());
        }
        // SAFETY: the libsecret call returned this owned GError; copy only the
        // numeric code, then free exactly once. Do not expose D-Bus error text,
        // which may contain attributes, addresses or credential values.
        let code = unsafe {
            let code = (*error).code;
            (self.error_free)(error);
            code
        };
        Err(unavailable(&format!(
            "credential operation failed ({code})"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn invalid_secret_sizes_never_access_the_session_keyring() {
        for value in [Vec::new(), vec![0; MAX_SECRET_BYTES + 1]] {
            assert!(matches!(
                LinuxSecretServiceVault
                    .put(Uuid::nil(), SecretRecord::AccessToken, &value)
                    .await,
                Err(VaultError::InvalidSecretSize)
            ));
        }
    }

    #[test]
    fn binary_values_and_namespace_cover_the_existing_vault_contract() {
        assert!(validate_size(MAX_SECRET_BYTES).is_ok());
        assert!(validate_size(1).is_ok());
        assert_eq!(
            APPLICATION.to_str().unwrap(),
            "io.github.georgexie2333.usque.identity"
        );
        for record in SecretRecord::ALL {
            assert!(!record.key().contains('\0'));
        }
    }
}
