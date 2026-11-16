//! Read-only SystemConfiguration observations for ordinary proxy sessions.
//!
//! Source addresses alone miss a new router or interface that reuses an IP.
//! Observe route identity and selected DNS/link policy without reading counters,
//! modifying preferences, spawning utilities, or claiming a protected VPN path.
use core_foundation::{
    array::{CFArray, CFArrayRef},
    base::{CFType, TCFType},
    boolean::CFBoolean,
    dictionary::{CFDictionary, CFDictionaryRef},
    number::CFNumber,
    string::CFString,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::{CString, c_void},
    ptr,
};

const ROUTE_FIELDS: &[&str] = &["Router", "PrimaryInterface", "PrimaryService"];
const LINK_FIELDS: &[&str] = &["Active", "Detaching"];
const DNS_FIELDS: &[&str] = &[
    "ServerAddresses",
    "ServerPort",
    "SearchDomains",
    "DomainName",
    "SupplementalMatchDomains",
    "SupplementalMatchOrders",
    "SearchOrder",
    "ServerTimeout",
    "InterfaceName",
    "Options",
];

#[derive(Debug, PartialEq, Eq, Hash)]
pub(super) enum Observation {
    /// A read failure is stable, distinct from an empty store, and does not
    /// override the source-address observer's online/offline determination.
    Unavailable,
    Available(BTreeMap<String, Value>),
}

#[derive(Debug, PartialEq, Eq, Hash)]
pub(super) enum Value {
    Missing,
    Invalid,
    Text(String),
    Boolean(bool),
    Number(i64),
    List(Vec<Value>),
}

#[link(name = "SystemConfiguration", kind = "framework")]
unsafe extern "C" {
    fn SCDynamicStoreCopyMultiple(
        store: *const c_void,
        keys: CFArrayRef,
        patterns: CFArrayRef,
    ) -> CFDictionaryRef;
}

pub(super) fn observe() -> Observation {
    let keys = CFArray::from_CFTypes(&[
        CFString::new("State:/Network/Global/IPv4"),
        CFString::new("State:/Network/Global/IPv6"),
        CFString::new("State:/Network/Global/DNS"),
    ]);
    // Read one store snapshot, then retain only the primary families' service
    // and interface entries. Unrelated VPN/service/interface churn is ignored.
    let patterns = CFArray::from_CFTypes(&[
        CFString::new("^State:/Network/Interface/[^/]+/Link$"),
        CFString::new("^State:/Network/Service/[^/]+/DNS$"),
    ]);
    // SAFETY: CopyMultiple accepts a null session; live CF arrays contain valid
    // strings. The returned create-rule dictionary is owned exactly once below.
    let values = unsafe {
        SCDynamicStoreCopyMultiple(
            ptr::null(),
            keys.as_concrete_TypeRef(),
            patterns.as_concrete_TypeRef(),
        )
    };
    if values.is_null() {
        return Observation::Unavailable;
    }
    // SAFETY: a non-null successful CopyMultiple result is a CFDictionary.
    let values = unsafe { CFDictionary::wrap_under_create_rule(values) };
    snapshot(Some(&values), |name| {
        let Ok(name) = CString::new(name) else {
            return 0;
        };
        // SAFETY: if_nametoindex only reads a terminated interface name and
        // returns a scalar identity; it cannot modify routes or link settings.
        unsafe { libc::if_nametoindex(name.as_ptr()) }
    })
}

fn snapshot(values: Option<&CFDictionary>, identity: impl Fn(&str) -> u32) -> Observation {
    let Some(values) = values else {
        return Observation::Unavailable;
    };
    let mut result = BTreeMap::new();
    let mut interfaces = BTreeSet::new();
    let mut services = BTreeSet::new();
    for family in ["IPv4", "IPv6"] {
        let key = format!("State:/Network/Global/{family}");
        if let Some(route) = append(&mut result, values, &key, ROUTE_FIELDS) {
            if let Some(name) = text(&route, "PrimaryInterface")
                && name.len() < libc::IFNAMSIZ
            {
                interfaces.insert(name);
            }
            if let Some(service) = text(&route, "PrimaryService")
                && service.len() <= 256
            {
                services.insert(service);
            }
        }
    }
    append(&mut result, values, "State:/Network/Global/DNS", DNS_FIELDS);
    for name in interfaces {
        result.insert(
            format!("InterfaceIndex/{name}"),
            Value::Number(i64::from(identity(&name))),
        );
        append(
            &mut result,
            values,
            &format!("State:/Network/Interface/{name}/Link"),
            LINK_FIELDS,
        );
    }
    for service in services {
        append(
            &mut result,
            values,
            &format!("State:/Network/Service/{service}/DNS"),
            DNS_FIELDS,
        );
    }
    Observation::Available(result)
}

fn entry(dictionary: &CFDictionary, key: &str) -> Option<CFType> {
    let key = CFString::new(key);
    dictionary.find(key.as_CFTypeRef()).map(|value| {
        // SAFETY: the untyped CF dictionary retains its valid CFType value;
        // wrap-under-get-rule retains it before the borrowed entry expires.
        unsafe { CFType::wrap_under_get_rule(*value) }
    })
}

fn text(dictionary: &CFDictionary, key: &str) -> Option<String> {
    match entry(dictionary, key).map(|value| value_of(&value, 0)) {
        Some(Value::Text(text)) if !text.is_empty() && !text.contains(['/', '\0']) => Some(text),
        _ => None,
    }
}

fn append(
    result: &mut BTreeMap<String, Value>,
    values: &CFDictionary,
    key: &str,
    fields: &[&str],
) -> Option<CFDictionary> {
    let Some(value) = entry(values, key) else {
        result.insert(key.into(), Value::Missing);
        return None;
    };
    let Some(dictionary) = value.downcast::<CFDictionary>() else {
        result.insert(key.into(), Value::Invalid);
        return None;
    };
    result.insert(key.into(), Value::Boolean(true));
    for field in fields {
        result.insert(
            format!("{key}/{field}"),
            entry(&dictionary, field).map_or(Value::Missing, |value| value_of(&value, 0)),
        );
    }
    Some(dictionary)
}

fn value_of(value: &CFType, depth: usize) -> Value {
    if let Some(string) = value.downcast::<CFString>() {
        return if string.char_len() <= 4096 {
            Value::Text(string.to_string())
        } else {
            Value::Invalid
        };
    }
    if let Some(boolean) = value.downcast::<CFBoolean>() {
        return Value::Boolean(boolean.into());
    }
    if let Some(number) = value.downcast::<CFNumber>() {
        return number.to_i64().map_or(Value::Invalid, Value::Number);
    }
    if let Some(array) = value.downcast::<CFArray>()
        && depth < 2
        && array.len() <= 128
    {
        return Value::List(
            array
                .iter()
                .map(|item| {
                    // SAFETY: CF arrays from the dynamic store retain CFType
                    // elements; retain the value before the entry expires.
                    let value = unsafe { CFType::wrap_under_get_rule(*item) };
                    value_of(&value, depth + 1)
                })
                .collect(),
        );
    }
    Value::Invalid
}

#[cfg(test)]
mod tests;
