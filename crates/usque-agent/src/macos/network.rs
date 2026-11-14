//! Physical network observations are obtained from SystemConfiguration, never
//! from a system DNS lookup that might recurse through the tunnel.
use core_foundation::{
    array::CFArray,
    base::{CFType, TCFType},
    dictionary::CFDictionary,
    string::{CFString, CFStringRef},
};
use serde::{Deserialize, Serialize};
use std::{
    ffi::CString,
    io,
    net::{IpAddr, SocketAddr, SocketAddrV6},
};
use uuid::Uuid;
type Dictionary = CFDictionary<CFString, CFType>;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhysicalPath {
    pub name: String,
    pub index: u32,
    pub address: IpAddr,
    pub addresses: Vec<ipnet::IpNet>,
    pub gateway: IpAddr,
    pub service: Uuid,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Network {
    pub paths: Vec<PhysicalPath>,
    pub dns: Vec<SocketAddr>,
}
impl PhysicalPath {
    pub fn source(&self, remote: SocketAddr) -> io::Result<IpAddr> {
        let matching = self
            .addresses
            .iter()
            .filter(|a| a.contains(&remote.ip()))
            .max_by_key(|a| a.prefix_len())
            .map(|a| a.addr());
        if matches!(remote.ip(), IpAddr::V6(ip) if ip.is_unicast_link_local()) {
            return matching
                .filter(|a| matches!(a, IpAddr::V6(ip) if ip.is_unicast_link_local()))
                .ok_or_else(invalid);
        }
        Ok(matching.unwrap_or(self.address))
    }
}
impl Network {
    pub fn path(&self, remote: SocketAddr) -> io::Result<&PhysicalPath> {
        self.paths
            .iter()
            .find(|p| p.address.is_ipv4() == remote.is_ipv4())
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NetworkUnreachable,
                    "physical address family unavailable",
                )
            })
    }
    pub fn validate(&self) -> io::Result<()> {
        if self.paths.is_empty()
            || self.paths.len() > 2
            || self.dns.is_empty()
            || self.dns.len() > 8
        {
            return Err(invalid());
        }
        for path in &self.paths {
            if !super::resources::interface_name(&path.name)
                || path.name.starts_with("utun")
                || path.name == "lo0"
                || path.index == 0
                || path.service.is_nil()
                || !super::resources::usable_address(path.address)
                || !super::resources::usable_address(path.gateway)
                || path.address.is_ipv4() != path.gateway.is_ipv4()
                || path.addresses.is_empty()
                || path.addresses.len() > 16
                || !path.addresses.iter().any(|a| a.addr() == path.address)
                || path.addresses.iter().any(|a| {
                    a.addr().is_ipv4() != path.address.is_ipv4()
                        || a.prefix_len() == 0
                        || !super::resources::usable_address(a.addr())
                })
            {
                return Err(invalid());
            }
        }
        // Multi-homed DNS needs a service-scoped egress model. Refuse it until
        // every resolver can be bound to its actual service, never pick by AF.
        if self.paths.len() == 2
            && (self.paths[0].address.is_ipv4() == self.paths[1].address.is_ipv4()
                || self.paths[0].name != self.paths[1].name
                || self.paths[0].index != self.paths[1].index
                || self.paths[0].service != self.paths[1].service)
        {
            return Err(invalid());
        }
        for server in &self.dns {
            if server.port() != 53
                || !super::resources::usable_address(server.ip())
                || self.path(*server).is_err()
            {
                return Err(invalid());
            }
        }
        Ok(())
    }
}
fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "physical network snapshot unavailable",
    )
}
fn string(dict: &Dictionary, key: &str) -> io::Result<String> {
    dict.find(CFString::new(key))
        .and_then(|v| v.downcast::<CFString>())
        .map(|v| v.to_string())
        .ok_or_else(invalid)
}
fn strings(dict: &Dictionary, key: &str) -> io::Result<Vec<String>> {
    let value = dict
        .find(CFString::new(key))
        .and_then(|v| v.downcast::<CFArray>())
        .ok_or_else(invalid)?;
    if value.len() > 64 {
        return Err(invalid());
    }
    value
        .iter()
        .map(|raw| {
            // SAFETY: CFArray owns each value during this inspection.
            unsafe { CFType::wrap_under_get_rule(*raw) }
                .downcast::<CFString>()
                .map(|s| s.to_string())
                .ok_or_else(invalid)
        })
        .collect()
}
fn parse_path(
    primary: &Dictionary,
    addresses: &Dictionary,
    index: u32,
    v6: bool,
) -> io::Result<PhysicalPath> {
    let name = string(primary, "PrimaryInterface")?;
    let service = string(primary, "PrimaryService")?
        .parse::<Uuid>()
        .map_err(|_| invalid())?;
    let gateway = string(primary, "Router")?
        .split('%')
        .next()
        .ok_or_else(invalid)?
        .parse::<IpAddr>()
        .map_err(|_| invalid())?;
    let all_addresses = strings(addresses, "Addresses")?;
    let prefixes: Vec<u8> = if v6 {
        let values = addresses
            .find(CFString::new("PrefixLength"))
            .and_then(|v| v.downcast::<CFArray>())
            .ok_or_else(invalid)?;
        if values.len() as usize != all_addresses.len() {
            return Err(invalid());
        }
        values
            .iter()
            .map(|raw| {
                // SAFETY: the array owns the value for the duration of this read.
                let value = unsafe { CFType::wrap_under_get_rule(*raw) };
                u8::try_from(
                    value
                        .downcast::<core_foundation::number::CFNumber>()
                        .and_then(|v| v.to_i64())
                        .ok_or_else(invalid)?,
                )
                .map_err(|_| invalid())
            })
            .collect::<io::Result<_>>()?
    } else {
        let masks = strings(addresses, "SubnetMasks")?;
        if masks.len() != all_addresses.len() {
            return Err(invalid());
        }
        masks
            .iter()
            .map(|s| {
                let bits = u32::from(s.parse::<std::net::Ipv4Addr>().map_err(|_| invalid())?);
                let n = bits.leading_ones();
                if bits != u32::MAX.checked_shl(32 - n).unwrap_or(0) {
                    return Err(invalid());
                }
                Ok(n as u8)
            })
            .collect::<io::Result<_>>()?
    };
    let mut observed = all_addresses
        .iter()
        .zip(prefixes)
        .map(|(s, prefix)| {
            let ip = s
                .split('%')
                .next()
                .ok_or_else(invalid)?
                .parse::<IpAddr>()
                .map_err(|_| invalid())?;
            if ip.is_ipv6() != v6 || !super::resources::usable_address(ip) || prefix == 0 {
                return Err(invalid());
            }
            ipnet::IpNet::new(ip, prefix).map_err(|_| invalid())
        })
        .collect::<io::Result<Vec<_>>>()?;
    observed.sort();
    observed.dedup();
    if observed.len() > 16 {
        return Err(invalid());
    }
    let address = observed
        .iter()
        .map(|a| a.addr())
        .filter(|a| !matches!(a, IpAddr::V6(ip) if ip.is_unicast_link_local()))
        .min_by_key(|a| u8::from(matches!(a, IpAddr::V6(ip) if ip.is_unique_local())))
        .ok_or_else(invalid)?;
    if index == 0
        || service.is_nil()
        || !super::resources::interface_name(&name)
        || name.starts_with("utun")
        || name == "lo0"
        || gateway.is_ipv6() != v6
        || !super::resources::usable_address(gateway)
    {
        return Err(invalid());
    }
    Ok(PhysicalPath {
        name,
        index,
        address,
        addresses: observed,
        gateway,
        service,
    })
}
#[link(name = "SystemConfiguration", kind = "framework")]
unsafe extern "C" {
    fn SCDynamicStoreCreate(
        allocator: *const std::ffi::c_void,
        name: CFStringRef,
        callback: *const std::ffi::c_void,
        context: *mut std::ffi::c_void,
    ) -> *const std::ffi::c_void;
    fn SCDynamicStoreCopyValue(
        store: *const std::ffi::c_void,
        key: CFStringRef,
    ) -> *const std::ffi::c_void;
}
struct DynamicStore(CFType);
impl DynamicStore {
    fn open() -> io::Result<Self> {
        let name = CFString::new("Usque physical network");
        // SAFETY: no callback is registered, name is live during Create.
        let raw = unsafe {
            SCDynamicStoreCreate(
                std::ptr::null(),
                name.as_concrete_TypeRef(),
                std::ptr::null(),
                std::ptr::null_mut(),
            )
        };
        if raw.is_null() {
            return Err(invalid());
        }
        // SAFETY: Create transfers ownership of the dynamic store.
        Ok(Self(unsafe { CFType::wrap_under_create_rule(raw) }))
    }
    fn dictionary(&self, key: &str) -> io::Result<Option<Dictionary>> {
        let key = CFString::new(key);
        // SAFETY: store and key are valid; Copy returns an owned CF value.
        let raw =
            unsafe { SCDynamicStoreCopyValue(self.0.as_CFTypeRef(), key.as_concrete_TypeRef()) };
        if raw.is_null() {
            return Ok(None);
        }
        // SAFETY: successful Copy transferred one reference.
        let value = unsafe { CFType::wrap_under_create_rule(raw) };
        let dict = value.downcast::<CFDictionary>().ok_or_else(invalid)?;
        // SAFETY: SystemConfiguration dictionaries have string keys and CF values.
        Ok(Some(unsafe {
            Dictionary::wrap_under_get_rule(dict.as_concrete_TypeRef())
        }))
    }
}
pub(super) fn setup_dns(service: Uuid) -> io::Result<Option<Vec<u8>>> {
    let value = DynamicStore::open()?.dictionary(&format!(
        "Setup:/Network/Service/{}/DNS",
        service.to_string().to_ascii_uppercase()
    ))?;
    value
        .map(|d| {
            core_foundation::propertylist::create_data(
                d.as_CFTypeRef(),
                core_foundation::propertylist::kCFPropertyListBinaryFormat_v1_0,
            )
            .map(|data| data.bytes().to_vec())
            .map_err(|_| invalid())
        })
        .transpose()
}
pub fn snapshot() -> io::Result<Network> {
    snapshot_with_original(&[])
}
pub fn snapshot_with_original(original: &[super::resources::DnsReceipt]) -> io::Result<Network> {
    let store = DynamicStore::open()?;
    let mut network = Network {
        paths: Vec::new(),
        dns: Vec::new(),
    };
    for (family, v6) in [("IPv4", false), ("IPv6", true)] {
        let Some(primary) = store.dictionary(&format!("State:/Network/Global/{family}"))? else {
            continue;
        };
        let service = string(&primary, "PrimaryService")?;
        service.parse::<Uuid>().map_err(|_| invalid())?;
        let name = string(&primary, "PrimaryInterface")?;
        let c_name = CString::new(name).map_err(|_| invalid())?;
        // SAFETY: c_name is a valid NUL-terminated interface name.
        let index = unsafe { libc::if_nametoindex(c_name.as_ptr()) };
        let addresses = store
            .dictionary(&format!("State:/Network/Service/{service}/{family}"))?
            .ok_or_else(invalid)?;
        let path = parse_path(&primary, &addresses, index, v6)?;
        let original = match original.iter().find(|r| r.service == path.service) {
            Some(receipt) => receipt
                .before
                .as_deref()
                .map(super::dns::decode)
                .transpose()?,
            None => store.dictionary(&format!("Setup:/Network/Service/{service}/DNS"))?,
        };
        let state = store.dictionary(&format!("State:/Network/Service/{service}/DNS"))?;
        // configd gives manually configured Setup servers precedence over DHCP
        // State servers. During TUN use the receipt's original Setup value.
        let configured = match original {
            Some(d) if d.contains_key(&CFString::new("ServerAddresses")) => {
                if strings(&d, "ServerAddresses")?.is_empty() {
                    state
                } else {
                    Some(d)
                }
            }
            _ => state,
        };
        if let Some(dns) = configured {
            for server in strings(&dns, "ServerAddresses")? {
                let ip = server
                    .split('%')
                    .next()
                    .ok_or_else(invalid)?
                    .parse::<IpAddr>()
                    .map_err(|_| invalid())?;
                let address = match ip {
                    IpAddr::V6(ip) if ip.is_unicast_link_local() => {
                        SocketAddr::V6(SocketAddrV6::new(ip, 53, 0, index))
                    }
                    _ => SocketAddr::new(ip, 53),
                };
                if !network.dns.contains(&address) {
                    network.dns.push(address);
                }
            }
        }
        network.paths.push(path);
    }
    network.dns.retain(|s| {
        network
            .paths
            .iter()
            .any(|p| p.address.is_ipv4() == s.is_ipv4())
    });
    network.validate()?;
    Ok(network)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn dict(pairs: &[(&str, &str)]) -> Dictionary {
        Dictionary::from_CFType_pairs(
            &pairs
                .iter()
                .map(|(k, v)| (CFString::new(k), CFString::new(v).as_CFType()))
                .collect::<Vec<_>>(),
        )
    }
    #[test]
    fn snapshot_keeps_gateway_and_service_identity_even_when_local_ip_is_unchanged() {
        let primary = dict(&[
            ("PrimaryInterface", "en0"),
            ("PrimaryService", "12345678-1234-1234-1234-123456789abc"),
            ("Router", "192.0.2.1"),
        ]);
        let addresses = Dictionary::from_CFType_pairs(&[
            (
                CFString::new("Addresses"),
                CFArray::from_CFTypes(&[CFString::new("192.0.2.7")]).as_CFType(),
            ),
            (
                CFString::new("SubnetMasks"),
                CFArray::from_CFTypes(&[CFString::new("255.255.255.0")]).as_CFType(),
            ),
        ]);
        let path = parse_path(&primary, &addresses, 5, false).unwrap();
        assert_eq!(path.address, "192.0.2.7".parse::<IpAddr>().unwrap());
        let next = dict(&[
            ("PrimaryInterface", "en0"),
            ("PrimaryService", "12345678-1234-1234-1234-123456789abc"),
            ("Router", "192.0.2.254"),
        ]);
        assert_ne!(path, parse_path(&next, &addresses, 5, false).unwrap());
        assert!(parse_path(&primary, &addresses, 0, false).is_err());
        assert!(parse_path(&primary, &addresses, 5, true).is_err());
    }
    #[test]
    fn dual_scope_ipv6_snapshot_captures_all_aliases_and_chooses_on_link_source() {
        let primary = dict(&[
            ("PrimaryInterface", "en0"),
            ("PrimaryService", "12345678-1234-1234-1234-123456789abc"),
            ("Router", "fe80::1%en0"),
        ]);
        let addresses = Dictionary::from_CFType_pairs(&[
            (
                CFString::new("Addresses"),
                CFArray::from_CFTypes(&[
                    CFString::new("2001:db8::2"),
                    CFString::new("fd00::2"),
                    CFString::new("fe80::2%en0"),
                ])
                .as_CFType(),
            ),
            (
                CFString::new("PrefixLength"),
                CFArray::from_CFTypes(&[
                    core_foundation::number::CFNumber::from(64i32),
                    core_foundation::number::CFNumber::from(64i32),
                    core_foundation::number::CFNumber::from(64i32),
                ])
                .as_CFType(),
            ),
        ]);
        let path = parse_path(&primary, &addresses, 4, true).unwrap();
        assert_eq!(path.addresses.len(), 3);
        assert_eq!(path.address, "2001:db8::2".parse::<IpAddr>().unwrap());
        for (remote, source) in [
            ("[fd00::1]:443", "fd00::2"),
            ("[fe80::1]:53", "fe80::2"),
            ("[2001:db8:1::1]:443", "2001:db8::2"),
        ] {
            assert_eq!(
                path.source(remote.parse().unwrap()).unwrap(),
                source.parse::<IpAddr>().unwrap()
            );
        }
    }
}
