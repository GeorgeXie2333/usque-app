//! Native preferences operations. Tests replace this entire boundary.
use super::*;
use core_foundation::{
    array::{CFArrayGetCount, CFArrayGetValueAtIndex, CFArrayRef},
    base::{CFAllocatorRef, CFRelease, CFType, TCFType},
    boolean::CFBoolean,
    dictionary::{
        CFDictionary, CFDictionaryCreateMutableCopy, CFDictionaryRef, CFMutableDictionary,
    },
    number::CFNumber,
    string::{CFString, CFStringRef},
};
use sha2::{Digest, Sha256};
use std::{
    ffi::{c_char, c_void},
    ptr,
};
type Ref = *const c_void;
#[link(name = "SystemConfiguration", kind = "framework")]
unsafe extern "C" {
    fn SCPreferencesCreate(allocator: CFAllocatorRef, name: CFStringRef, prefs: CFStringRef)
    -> Ref;
    fn SCPreferencesCreateWithAuthorization(
        allocator: CFAllocatorRef,
        name: CFStringRef,
        prefs: CFStringRef,
        authorization: *mut c_void,
    ) -> Ref;
    fn SCPreferencesLock(prefs: Ref, wait: u8) -> u8;
    fn SCPreferencesUnlock(prefs: Ref) -> u8;
    fn SCPreferencesSynchronize(prefs: Ref);
    fn SCPreferencesCommitChanges(prefs: Ref) -> u8;
    fn SCPreferencesApplyChanges(prefs: Ref) -> u8;
    fn SCNetworkServiceCopyAll(prefs: Ref) -> CFArrayRef;
    fn SCNetworkServiceGetServiceID(service: Ref) -> CFStringRef;
    fn SCNetworkServiceGetEnabled(service: Ref) -> u8;
    fn SCNetworkServiceGetInterface(service: Ref) -> Ref;
    fn SCNetworkInterfaceGetInterfaceType(interface: Ref) -> CFStringRef;
    fn SCNetworkServiceCopyProtocol(service: Ref, protocol_type: CFStringRef) -> Ref;
    fn SCNetworkProtocolGetConfiguration(protocol: Ref) -> CFDictionaryRef;
    fn SCNetworkProtocolSetConfiguration(protocol: Ref, configuration: CFDictionaryRef) -> u8;
    fn SCNetworkProtocolGetEnabled(protocol: Ref) -> u8;
    fn SCNetworkSetCopyCurrent(prefs: Ref) -> Ref;
    fn SCNetworkSetCopyServices(set: Ref) -> CFArrayRef;
    fn SCError() -> i32;
}
#[repr(C)]
struct AuthorizationItem {
    name: *const c_char,
    length: usize,
    value: *mut c_void,
    flags: u32,
}
#[repr(C)]
struct AuthorizationItems {
    count: u32,
    items: *mut AuthorizationItem,
}
#[link(name = "Security", kind = "framework")]
unsafe extern "C" {
    fn AuthorizationCreate(
        rights: *const AuthorizationItems,
        environment: *const AuthorizationItems,
        flags: u32,
        output: *mut *mut c_void,
    ) -> i32;
    fn AuthorizationFree(authorization: *mut c_void, flags: u32) -> i32;
}
struct Authorization(*mut c_void);
// SAFETY: Authorization Services references are opaque process-wide handles.
// Every access and release is serialized by NativeBackend.authorization; no
// pointer is dereferenced by Rust or exposed to any client/provider process.
unsafe impl Send for Authorization {}
impl Authorization {
    fn create() -> Result<Self> {
        let mut right = AuthorizationItem {
            name: c"system.preferences".as_ptr(),
            length: 0,
            value: ptr::null_mut(),
            flags: 0,
        };
        let rights = AuthorizationItems {
            count: 1,
            items: &mut right,
        };
        let mut output = ptr::null_mut();
        // SAFETY: the item set and output storage remain live through this call;
        // only Authorization Services owns the permission dialog/credentials.
        let status = unsafe { AuthorizationCreate(&rights, ptr::null(), 1 | 2, &mut output) };
        match status {
            0 if !output.is_null() => Ok(Self(output)),
            -60006 => Err(Error::PermissionCancelled),
            -60005 | -60007 => Err(Error::PermissionRequired),
            _ => Err(Error::Unavailable),
        }
    }
}
impl Drop for Authorization {
    fn drop(&mut self) {
        // SAFETY: this sole owner releases the process-local authorization once.
        unsafe {
            AuthorizationFree(self.0, 0);
        }
    }
}
struct Owned(Ref);
impl Owned {
    fn new(value: Ref) -> Result<Self> {
        if value.is_null() {
            Err(sc_error())
        } else {
            Ok(Self(value))
        }
    }
}
impl Drop for Owned {
    fn drop(&mut self) {
        /* SAFETY: create/copy ownership is released exactly once. */
        unsafe {
            CFRelease(self.0);
        }
    }
}
struct Preferences(Owned);
impl Drop for Preferences {
    fn drop(&mut self) {
        /* SAFETY: successful lock stays owned until this scope exits. */
        unsafe {
            SCPreferencesUnlock(self.0.0);
        }
    }
}
#[derive(Default)]
pub(super) struct NativeBackend {
    authorization: Mutex<Option<Authorization>>,
}
struct NativeService {
    service: Service,
    protocol: Owned,
    dictionary: CFDictionary<CFString, CFType>,
}
impl Backend for NativeBackend {
    fn transaction(&self, interactive: bool, edit: &mut Edit<'_>) -> Result<()> {
        let mut authorization = self.authorization.lock().map_err(|_| Error::Unavailable)?;
        if interactive && authorization.is_none() {
            *authorization = Some(Authorization::create()?);
        }
        let name = CFString::new("Usque system proxy");
        // SAFETY: the CFString and optional authorization reference remain live
        // while the owned preferences session is created. Startup has no auth
        // reference and therefore cannot trigger an authorization dialog.
        let prefs = Owned::new(unsafe {
            match authorization.as_ref() {
                Some(auth) => SCPreferencesCreateWithAuthorization(
                    ptr::null(),
                    name.as_concrete_TypeRef(),
                    ptr::null(),
                    auth.0,
                ),
                None => SCPreferencesCreate(ptr::null(), name.as_concrete_TypeRef(), ptr::null()),
            }
        })?;
        // SAFETY: prefs owns a valid preferences object; never block on another writer.
        if unsafe { SCPreferencesLock(prefs.0, 0) } == 0 {
            return Err(sc_error());
        }
        let prefs = Preferences(prefs);
        // SAFETY: refresh the locked session before reading any configuration.
        unsafe {
            SCPreferencesSynchronize(prefs.0.0);
        }
        let services = read_services(prefs.0.0)?;
        let snapshots: Vec<_> = services
            .iter()
            .map(|service| service.service.clone())
            .collect();
        let patches = edit(&snapshots)?;
        // Recovery must re-apply committed preferences even when every owned
        // key is already restored: a previous commit may have succeeded while
        // ApplyChanges failed. The durable journal is not released until that
        // active apply is acknowledged.
        let mut edits = BTreeMap::<String, Vec<&Patch>>::new();
        for patch in &patches {
            if !KEYS.contains(&patch.key.as_str())
                || patch
                    .after
                    .as_ref()
                    .is_some_and(|value| !valid_value(&patch.key, value))
            {
                return Err(Error::Unavailable);
            }
            let current = services
                .iter()
                .find(|service| service.service.id == patch.service)
                .ok_or(Error::Conflict)?;
            if current.service.values.get(&patch.key) != patch.before.as_ref() {
                return Err(Error::Conflict);
            }
            edits.entry(patch.service.clone()).or_default().push(patch);
        }
        for (id, edits) in edits {
            let service = services
                .iter()
                .find(|service| service.service.id == id)
                .ok_or(Error::Conflict)?;
            // SAFETY: copy the immutable system dictionary into an independently
            // owned mutable dictionary; unrelated/PAC/SOCKS/secret keys retain
            // their exact native objects without serialization or projection.
            let copy = unsafe {
                CFDictionaryCreateMutableCopy(
                    ptr::null(),
                    0,
                    service.dictionary.as_concrete_TypeRef(),
                )
            };
            if copy.is_null() {
                return Err(Error::Unavailable);
            }
            // SAFETY: the successful Copy call transferred ownership of this dictionary.
            let mut copy: CFMutableDictionary<CFString, CFType> =
                unsafe { TCFType::wrap_under_create_rule(copy) };
            for patch in edits {
                let key = CFString::new(&patch.key);
                match &patch.after {
                    Some(value) => copy.set(key, to_cf(value)),
                    None => copy.remove(key),
                }
            }
            // SAFETY: service owns the protocol and copy owns a valid dictionary.
            if unsafe {
                SCNetworkProtocolSetConfiguration(
                    service.protocol.0,
                    copy.as_concrete_TypeRef().cast_const(),
                )
            } == 0
            {
                return Err(sc_error());
            }
        }
        // SAFETY: all changes belong to this exclusively locked session. A
        // failed commit/apply leaves the already durable recovery journal.
        let committed = unsafe {
            SCPreferencesCommitChanges(prefs.0.0) != 0 && SCPreferencesApplyChanges(prefs.0.0) != 0
        };
        if !committed {
            return Err(sc_error());
        }
        // SAFETY: reload committed preferences before verifying every owned key.
        unsafe {
            SCPreferencesSynchronize(prefs.0.0);
        }
        let verified = read_services(prefs.0.0)?;
        for patch in patches {
            if verified
                .iter()
                .find(|service| service.service.id == patch.service)
                .and_then(|service| service.service.values.get(&patch.key))
                != patch.after.as_ref()
            {
                return Err(Error::RecoveryPending);
            }
        }
        Ok(())
    }
}
fn read_services(prefs: Ref) -> Result<Vec<NativeService>> {
    // SAFETY: prefs is valid throughout the copied arrays and service references.
    let all = Owned::new(unsafe { SCNetworkServiceCopyAll(prefs).cast() })?;
    // SAFETY: a copied CFArray reports and retains all of its service elements.
    let count = unsafe { CFArrayGetCount(all.0.cast()) };
    if count < 0 || count as usize > MAX_SERVICES {
        return Err(Error::Unavailable);
    }
    // SAFETY: absence of an active network set is a normal no-service state.
    let current = unsafe { SCNetworkSetCopyCurrent(prefs) };
    let mut current_ids = BTreeSet::new();
    if !current.is_null() {
        let current = Owned(current);
        // SAFETY: current owns a live network set.
        let array = unsafe { SCNetworkSetCopyServices(current.0) };
        if !array.is_null() {
            let array = Owned(array.cast());
            // SAFETY: array is a copied array; each indexed item stays alive in it.
            let count = unsafe { CFArrayGetCount(array.0.cast()) };
            if count < 0 || count as usize > MAX_SERVICES {
                return Err(Error::Unavailable);
            }
            for index in 0..count {
                // SAFETY: index is bounded by the copied array's count.
                let service = unsafe { CFArrayGetValueAtIndex(array.0.cast(), index) };
                current_ids.insert(service_id(service)?);
            }
        }
    }
    let proxies = CFString::new("Proxies");
    let mut result = Vec::new();
    for index in 0..count {
        // SAFETY: index is bounded by all's copied array count.
        let service = unsafe { CFArrayGetValueAtIndex(all.0.cast(), index) };
        let id = service_id(service)?;
        // SAFETY: service is retained by all and protocol is returned with ownership.
        let protocol =
            unsafe { SCNetworkServiceCopyProtocol(service, proxies.as_concrete_TypeRef()) };
        if protocol.is_null() {
            continue;
        }
        let protocol = Owned(protocol);
        // SAFETY: protocol owns the returned immutable configuration during this read.
        let dictionary = unsafe { SCNetworkProtocolGetConfiguration(protocol.0) };
        let dictionary = if dictionary.is_null() {
            CFDictionary::from_CFType_pairs(&[])
        } else {
            // SAFETY: the non-null Get-rule dictionary is retained by this wrapper.
            unsafe { CFDictionary::<CFString, CFType>::wrap_under_get_rule(dictionary) }
        };
        let mut values = Values::new();
        for key in KEYS {
            if let Some(value) = dictionary.find(CFString::new(key)) {
                let value = from_cf(&value)?;
                if !valid_value(key, &value) {
                    return Err(Error::Unavailable);
                }
                values.insert(key.into(), value);
            }
        }
        // SAFETY: service/interface references are borrowed from a live copied array.
        let interface = unsafe { SCNetworkServiceGetInterface(service) };
        let kind = if interface.is_null() {
            String::new()
        } else {
            // SAFETY: interface remains live while its type is copied.
            let kind = unsafe { SCNetworkInterfaceGetInterfaceType(interface) };
            if kind.is_null() {
                String::new()
            } else {
                /* SAFETY: checked non-null CFString is retained. */
                unsafe { CFString::wrap_under_get_rule(kind) }.to_string()
            }
        };
        // SAFETY: service and protocol are live validated SystemConfiguration objects.
        let enabled = unsafe {
            SCNetworkServiceGetEnabled(service) != 0 && SCNetworkProtocolGetEnabled(protocol.0) != 0
        };
        let eligible = enabled
            && current_ids.contains(&id)
            && matches!(
                kind.as_str(),
                "Ethernet" | "IEEE80211" | "VLAN" | "Bond" | "Bridge" | "FireWire"
            );
        result.push(NativeService {
            service: Service {
                id,
                eligible,
                values,
                pac_fingerprint: pac_fingerprint(&dictionary)?,
            },
            protocol,
            dictionary,
        });
    }
    Ok(result)
}
/// Only this digest crosses the native boundary. PAC URLs and scripts remain
/// in their native dictionary; transient UTF-8 hashing buffers are zeroized.
fn pac_fingerprint(dictionary: &CFDictionary<CFString, CFType>) -> Result<String> {
    let mut hash = Sha256::new();
    hash.update(b"usque-macos-proxy-pac-v1\0");
    for key in ["ProxyAutoConfigURLString", "ProxyAutoConfigJavaScript"] {
        hash.update(key.as_bytes());
        match dictionary.find(CFString::new(key)) {
            None => hash.update([0]),
            Some(value) => {
                let value = value.downcast::<CFString>().ok_or(Error::Unavailable)?;
                if value.char_len() > 1024 * 1024 {
                    return Err(Error::Unavailable);
                }
                let text = Zeroizing::new(value.to_string());
                hash.update([1]);
                hash.update((text.len() as u64).to_be_bytes());
                hash.update(text.as_bytes());
            }
        }
    }
    Ok(hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}
fn service_id(service: Ref) -> Result<String> {
    // SAFETY: caller holds a copied service array throughout this query.
    let value = unsafe { SCNetworkServiceGetServiceID(service) };
    if value.is_null() {
        return Err(Error::Unavailable);
    }
    // SAFETY: the non-null borrowed CFString is retained before conversion.
    let value = unsafe { CFString::wrap_under_get_rule(value) }.to_string();
    if !valid_service_id(&value) {
        return Err(Error::Unavailable);
    }
    Ok(value)
}
fn from_cf(value: &CFType) -> Result<Value> {
    if let Some(value) = value.downcast::<CFString>() {
        return Ok(Value::Text(value.to_string()));
    }
    if let Some(value) = value.downcast::<CFBoolean>() {
        return Ok(Value::Boolean(value.into()));
    }
    if let Some(value) = value
        .downcast::<CFNumber>()
        .and_then(|value| value.to_i64())
    {
        return Ok(Value::Integer(value));
    }
    Err(Error::Unavailable)
}
fn to_cf(value: &Value) -> CFType {
    match value {
        Value::Text(value) => CFString::new(value).as_CFType(),
        Value::Integer(value) => CFNumber::from(*value).as_CFType(),
        Value::Boolean(value) => CFBoolean::from(*value).as_CFType(),
    }
}
fn sc_error() -> Error {
    // SAFETY: SCError only reads the calling thread's last SystemConfiguration status.
    match unsafe { SCError() } {
        1003 => Error::PermissionRequired,
        1006 | 3004 | 3005 => Error::Conflict,
        _ => Error::Unavailable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pac_fingerprint_tracks_only_native_pac_url_and_script() {
        let dictionary = |url: &str, script: &str, http: &str| {
            CFDictionary::from_CFType_pairs(&[
                (
                    CFString::new("ProxyAutoConfigURLString"),
                    CFString::new(url).as_CFType(),
                ),
                (
                    CFString::new("ProxyAutoConfigJavaScript"),
                    CFString::new(script).as_CFType(),
                ),
                (CFString::new("HTTPProxy"), CFString::new(http).as_CFType()),
            ])
        };
        let first = pac_fingerprint(&dictionary(
            "https://pac.example/one",
            "return DIRECT",
            "first.example",
        ))
        .unwrap();
        assert_eq!(first.len(), 64);
        assert_eq!(
            first,
            pac_fingerprint(&dictionary(
                "https://pac.example/one",
                "return DIRECT",
                "second.example"
            ))
            .unwrap()
        );
        assert_ne!(
            first,
            pac_fingerprint(&dictionary(
                "https://pac.example/two",
                "return DIRECT",
                "first.example"
            ))
            .unwrap()
        );
        assert_ne!(
            first,
            pac_fingerprint(&dictionary(
                "https://pac.example/one",
                "return PROXY",
                "first.example"
            ))
            .unwrap()
        );
    }
}
