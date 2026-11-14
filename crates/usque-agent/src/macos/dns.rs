//! SystemConfiguration DNS with semantic compare-and-restore receipts.
use super::resources::DnsReceipt;
use core_foundation::{
    array::CFArray,
    base::{CFType, TCFType},
    data::CFData,
    dictionary::{CFDictionary, CFMutableDictionary},
    propertylist,
    string::CFString,
};
use std::{io, net::IpAddr};
type Dictionary = CFDictionary<CFString, CFType>;
fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid DNS preferences")
}
pub(super) fn decode(bytes: &[u8]) -> io::Result<Dictionary> {
    if bytes.is_empty() || bytes.len() > 32768 {
        return Err(invalid());
    }
    let (raw, _) =
        propertylist::create_with_data(CFData::from_buffer(bytes), 0).map_err(|_| invalid())?;
    // SAFETY: the property-list parser transferred ownership of this CF value.
    let value = unsafe { CFType::wrap_under_create_rule(raw) };
    let dict = value.downcast::<CFDictionary>().ok_or_else(invalid)?;
    // SAFETY: only string keys are accepted below before typing the dictionary.
    let (keys, _) = dict.get_keys_and_values();
    for key in keys {
        // SAFETY: dict retains every key throughout this inspection.
        if unsafe { CFType::wrap_under_get_rule(key) }
            .downcast::<CFString>()
            .is_none()
        {
            return Err(invalid());
        }
    }
    // SAFETY: validated string keys, CFType accepts every retained value.
    Ok(unsafe { Dictionary::wrap_under_get_rule(dict.as_concrete_TypeRef()) })
}
fn encode(dict: &Dictionary) -> io::Result<Vec<u8>> {
    let data = propertylist::create_data(
        dict.as_CFTypeRef(),
        propertylist::kCFPropertyListBinaryFormat_v1_0,
    )
    .map_err(|_| invalid())?;
    if data.len() > 32768 {
        return Err(invalid());
    }
    Ok(data.bytes().to_vec())
}
pub fn equivalent(a: Option<&[u8]>, b: Option<&[u8]>) -> io::Result<bool> {
    match (a, b) {
        (None, None) => Ok(true),
        (Some(a), Some(b)) => Ok(decode(a)? == decode(b)?),
        _ => Ok(false),
    }
}
fn patch(original: Option<&[u8]>, servers: &[IpAddr]) -> io::Result<Vec<u8>> {
    if servers.is_empty()
        || servers.len() > 8
        || servers
            .iter()
            .any(|ip| !super::resources::usable_address(*ip))
    {
        return Err(invalid());
    }
    let before = original
        .map(decode)
        .transpose()?
        .unwrap_or_else(|| Dictionary::from_CFType_pairs(&[]));
    // SAFETY: before is a valid immutable dictionary; Copy retains its keys
    // and values, and the new mutable dictionary is exclusively owned here.
    let raw = unsafe {
        core_foundation::dictionary::CFDictionaryCreateMutableCopy(
            std::ptr::null(),
            0,
            before.as_concrete_TypeRef(),
        )
    };
    if raw.is_null() {
        return Err(invalid());
    }
    // SAFETY: Copy transferred ownership and preserved the validated types.
    let mut after = unsafe { CFMutableDictionary::<CFString, CFType>::wrap_under_create_rule(raw) };
    let addresses: Vec<_> = servers
        .iter()
        .map(|ip| CFString::new(&ip.to_string()))
        .collect();
    after.set(
        CFString::new("ServerAddresses"),
        CFArray::from_CFTypes(&addresses).as_CFType(),
    );
    encode(&after.to_immutable())
}

type Ref = *const std::ffi::c_void;
#[link(name = "SystemConfiguration", kind = "framework")]
unsafe extern "C" {
    fn SCPreferencesCreate(
        allocator: Ref,
        name: core_foundation::string::CFStringRef,
        prefs: core_foundation::string::CFStringRef,
    ) -> Ref;
    fn SCPreferencesLock(prefs: Ref, wait: u8) -> u8;
    fn SCPreferencesUnlock(prefs: Ref) -> u8;
    fn SCPreferencesSynchronize(prefs: Ref);
    fn SCPreferencesCommitChanges(prefs: Ref) -> u8;
    fn SCPreferencesApplyChanges(prefs: Ref) -> u8;
    fn SCPreferencesPathGetValue(prefs: Ref, path: core_foundation::string::CFStringRef) -> Ref;
    fn SCPreferencesPathSetValue(
        prefs: Ref,
        path: core_foundation::string::CFStringRef,
        value: Ref,
    ) -> u8;
    fn SCPreferencesPathRemoveValue(prefs: Ref, path: core_foundation::string::CFStringRef) -> u8;
    fn SCNetworkServiceCopyAll(prefs: Ref) -> core_foundation::array::CFArrayRef;
    fn SCNetworkServiceGetServiceID(service: Ref) -> core_foundation::string::CFStringRef;
    fn SCNetworkServiceGetEnabled(service: Ref) -> u8;
    fn SCNetworkServiceGetInterface(service: Ref) -> Ref;
    fn SCNetworkInterfaceGetBSDName(interface: Ref) -> core_foundation::string::CFStringRef;
}
struct Preferences(CFType);
impl Preferences {
    fn open() -> io::Result<Self> {
        let name = CFString::new("Usque Agent DNS");
        // SAFETY: live name; null default allocator and preferences identifier.
        let raw = unsafe {
            SCPreferencesCreate(
                std::ptr::null(),
                name.as_concrete_TypeRef(),
                std::ptr::null(),
            )
        };
        if raw.is_null() {
            return Err(invalid());
        }
        // SAFETY: Create transfers one owned preferences reference.
        let value = unsafe { CFType::wrap_under_create_rule(raw) };
        // SAFETY: preferences is valid; a nonblocking lock avoids deadlocking
        // the recovery service behind another preferences writer.
        if unsafe { SCPreferencesLock(raw, 0) } == 0 {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "DNS preferences busy",
            ));
        }
        // SAFETY: reload under the acquired lock.
        unsafe { SCPreferencesSynchronize(raw) };
        Ok(Self(value))
    }
    fn read(&self, path: &CFString) -> io::Result<Option<Vec<u8>>> {
        // SAFETY: preferences and path live for this call.
        let raw =
            unsafe { SCPreferencesPathGetValue(self.0.as_CFTypeRef(), path.as_concrete_TypeRef()) };
        if raw.is_null() {
            return Ok(None);
        }
        // SAFETY: retain the borrowed preferences dictionary before encoding it.
        let value = unsafe { CFType::wrap_under_get_rule(raw) };
        let dict = value.downcast::<CFDictionary>().ok_or_else(invalid)?;
        let data = propertylist::create_data(
            dict.as_CFTypeRef(),
            propertylist::kCFPropertyListBinaryFormat_v1_0,
        )
        .map_err(|_| invalid())?;
        if data.len() > 32768 {
            return Err(invalid());
        }
        // Validate keys and types before persisting any original state.
        decode(data.bytes())?;
        Ok(Some(data.bytes().to_vec()))
    }
}
impl Drop for Preferences {
    fn drop(&mut self) {
        // SAFETY: this instance owns the successfully acquired lock.
        unsafe {
            SCPreferencesUnlock(self.0.as_CFTypeRef());
        }
    }
}
fn path(service: uuid::Uuid) -> CFString {
    CFString::new(&format!(
        "/NetworkServices/{}/DNS",
        service.to_string().to_ascii_uppercase()
    ))
}

/// Snapshot all enabled physical services before writing any preferences.
/// A newly enabled/added service must trigger a protected reconfiguration.
pub fn snapshot(servers: &[IpAddr]) -> io::Result<Vec<DnsReceipt>> {
    let prefs = Preferences::open()?;
    // SAFETY: prefs lives throughout enumeration; Copy returns ownership.
    let raw = unsafe { SCNetworkServiceCopyAll(prefs.0.as_CFTypeRef()) };
    if raw.is_null() {
        return Err(invalid());
    }
    // SAFETY: every service is retained by this array.
    let services: CFArray<CFType> = unsafe { CFArray::wrap_under_create_rule(raw) };
    if services.len() > 64 {
        return Err(invalid());
    }
    let mut receipts = Vec::new();
    for service in services.iter() {
        let raw = service.as_CFTypeRef();
        // SAFETY: service is retained by the copied array.
        if unsafe { SCNetworkServiceGetEnabled(raw) } == 0 {
            continue;
        }
        // SAFETY: interface is borrowed from the live service.
        let interface = unsafe { SCNetworkServiceGetInterface(raw) };
        if interface.is_null() {
            continue;
        }
        // SAFETY: the interface remains retained by service.
        let bsd = unsafe { SCNetworkInterfaceGetBSDName(interface) };
        if bsd.is_null() {
            continue;
        }
        // SAFETY: nonnull Get result retained by wrapper.
        let name = unsafe { CFString::wrap_under_get_rule(bsd) }.to_string();
        if name.starts_with("utun") || name == "lo0" {
            continue;
        }
        if !super::resources::interface_name(&name) {
            return Err(invalid());
        }
        // SAFETY: live service holds this identifier.
        let id = unsafe { SCNetworkServiceGetServiceID(raw) };
        if id.is_null() {
            return Err(invalid());
        }
        // SAFETY: identifier is a nonnull retained Get result.
        let service = unsafe { CFString::wrap_under_get_rule(id) }
            .to_string()
            .parse::<uuid::Uuid>()
            .map_err(|_| invalid())?;
        let before = prefs.read(&path(service))?;
        let applied = patch(before.as_deref(), servers)?;
        receipts.push(DnsReceipt {
            service,
            before,
            applied,
            restored: false,
        });
    }
    if receipts.is_empty() {
        return Err(invalid());
    }
    Ok(receipts)
}
pub fn apply(receipt: &DnsReceipt, restore: bool) -> io::Result<()> {
    let prefs = Preferences::open()?;
    let path = path(receipt.service);
    let observed = prefs.read(&path)?;
    let (expected, target) = if restore {
        (Some(receipt.applied.as_slice()), receipt.before.as_deref())
    } else {
        (receipt.before.as_deref(), Some(receipt.applied.as_slice()))
    };
    let already_written = equivalent(observed.as_deref(), target)?;
    if !already_written && !equivalent(observed.as_deref(), expected)? {
        return Err(io::Error::other(
            "DNS ownership conflict; recovery incomplete",
        ));
    }
    let value = target.map(decode).transpose()?;
    // SAFETY: all CF values remain live; the caller persisted this exact intent
    // before acquiring the preferences lock. No arbitrary path is accepted.
    let changed = if already_written {
        1
    } else {
        // SAFETY: the locked preferences and all CF values remain live.
        unsafe {
            match &value {
                Some(value) => SCPreferencesPathSetValue(
                    prefs.0.as_CFTypeRef(),
                    path.as_concrete_TypeRef(),
                    value.as_CFTypeRef(),
                ),
                None => {
                    SCPreferencesPathRemoveValue(prefs.0.as_CFTypeRef(), path.as_concrete_TypeRef())
                }
            }
        }
    };
    // SAFETY: commit/apply use the same locked preferences instance.
    let applied = unsafe {
        changed != 0
            && SCPreferencesCommitChanges(prefs.0.as_CFTypeRef()) != 0
            && SCPreferencesApplyChanges(prefs.0.as_CFTypeRef()) != 0
    };
    if !applied {
        return Err(io::Error::other(
            "DNS commit/apply failed; recovery incomplete",
        ));
    }
    // SAFETY: refresh from disk under the lock before comparing actual values.
    unsafe { SCPreferencesSynchronize(prefs.0.as_CFTypeRef()) };
    if !equivalent(prefs.read(&path)?.as_deref(), target)? {
        return Err(io::Error::other("DNS readback failed; recovery incomplete"));
    }
    // Commit and Apply are distinct operations. Retry Apply even if disk was
    // already at target after a crash; then wait for configd's active Setup.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        if equivalent(
            super::network::setup_dns(receipt.service)?.as_deref(),
            target,
        )? {
            break;
        }
        if std::time::Instant::now() >= deadline {
            return Err(io::Error::other(
                "active DNS readback failed; recovery incomplete",
            ));
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn replacing_servers_preserves_unrelated_dns_settings_and_original_evidence() {
        let before = Dictionary::from_CFType_pairs(&[(
            CFString::new("SearchDomains"),
            CFArray::from_CFTypes(&[CFString::new("example.test")]).as_CFType(),
        )]);
        let saved = encode(&before).unwrap();
        let output = patch(
            Some(&saved),
            &["198.18.0.1".parse().unwrap(), "fd00::1".parse().unwrap()],
        )
        .unwrap();
        let after = decode(&output).unwrap();
        assert_eq!(
            &*after.find(CFString::new("SearchDomains")).unwrap(),
            &*before.find(CFString::new("SearchDomains")).unwrap()
        );
        assert!(after.find(CFString::new("ServerAddresses")).is_some());
        assert!(equivalent(Some(&saved), Some(&encode(&before).unwrap())).unwrap());
        assert!(!equivalent(Some(&saved), Some(&output)).unwrap());
        assert!(before.find(CFString::new("ServerAddresses")).is_none());
    }
    #[test]
    fn rejects_unbounded_invalid_or_empty_dns_settings() {
        assert!(patch(None, &[]).is_err());
        assert!(patch(None, &["0.0.0.0".parse().unwrap()]).is_err());
        assert!(patch(Some(b"not a plist"), &["198.18.0.1".parse().unwrap()]).is_err());
        assert!(patch(None, &["198.18.0.1".parse().unwrap(); 9]).is_err());
        assert!(equivalent(Some(b"broken"), Some(b"broken")).is_err());
    }
}
