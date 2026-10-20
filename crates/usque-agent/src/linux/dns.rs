//! DNS backend selection and NetworkManager compare-and-restore receipts.
//! GlobalDnsConfiguration is the manager's typed D-Bus interface; physical
//! connection profiles and /etc/resolv.conf are never edited directly.

use std::{collections::BTreeMap, fs, io, net::IpAddr};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::command;
use usque_ipc::{agent_v1::LinuxFailureReason as FailureReason, linux_failure};

const SERVICE: &str = "org.freedesktop.NetworkManager";
const PATH: &str = "/org/freedesktop/NetworkManager";
const DNS_PATH: &str = "/org/freedesktop/NetworkManager/DnsManager";
const DNS_INTERFACE: &str = "org.freedesktop.NetworkManager.DnsManager";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "backend", rename_all = "snake_case", deny_unknown_fields)]
pub enum DnsReceipt {
    #[default]
    Resolved,
    NetworkManager {
        previous: GlobalDns,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct GlobalDns {
    #[serde(skip_serializing_if = "Option::is_none")]
    searches: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    options: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    domains: Option<BTreeMap<String, Domain>>,
    #[serde(
        rename = "certification-authority",
        skip_serializing_if = "Option::is_none"
    )]
    certification_authority: Option<String>,
    #[serde(rename = "resolve-mode", skip_serializing_if = "Option::is_none")]
    resolve_mode: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Domain {
    #[serde(skip_serializing_if = "Option::is_none")]
    servers: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    options: Option<Vec<String>>,
}

impl GlobalDns {
    pub fn validate(&self) -> io::Result<()> {
        for value in [&self.certification_authority, &self.resolve_mode]
            .into_iter()
            .flatten()
        {
            valid_string(value)?;
        }
        for list in [&self.searches, &self.options].into_iter().flatten() {
            valid_list(list)?;
        }
        if let Some(domains) = &self.domains {
            if domains.len() > 64 {
                return Err(invalid("NetworkManager DNS domain limit exceeded"));
            }
            for (name, domain) in domains {
                valid_string(name)?;
                if domain.servers.as_ref().is_none_or(Vec::is_empty) {
                    return Err(invalid("NetworkManager DNS domain has no servers"));
                }
                for list in [&domain.servers, &domain.options].into_iter().flatten() {
                    valid_list(list)?;
                }
            }
        }
        Ok(())
    }

    fn for_servers(servers: &[IpAddr]) -> Self {
        Self {
            certification_authority: None,
            resolve_mode: None,
            searches: Some(Vec::new()),
            options: Some(Vec::new()),
            domains: Some(BTreeMap::from([(
                "*".into(),
                Domain {
                    servers: Some(servers.iter().map(ToString::to_string).collect()),
                    options: None,
                },
            )])),
        }
    }

    fn from_property(value: Value) -> io::Result<Self> {
        let decoded = unvariant(value)?;
        let config: Self = serde_json::from_value(decoded).map_err(|_| {
            linux_failure::classified(
                FailureReason::Schema,
                "unsupported NetworkManager global DNS schema",
            )
        })?;
        config.validate()?;
        Ok(config)
    }

    fn arguments(&self) -> Vec<String> {
        let mut entries = Vec::new();
        let mut count = 0;
        for (name, values) in [("searches", &self.searches), ("options", &self.options)] {
            if let Some(values) = values {
                count += 1;
                array_argument(&mut entries, name, values);
            }
        }
        for (name, value) in [
            ("certification-authority", &self.certification_authority),
            ("resolve-mode", &self.resolve_mode),
        ] {
            if let Some(value) = value {
                count += 1;
                entries.extend([name.into(), "s".into(), value.clone()]);
            }
        }
        if let Some(domains) = &self.domains {
            count += 1;
            entries.extend(["domains".into(), "a{sv}".into(), domains.len().to_string()]);
            for (name, domain) in domains {
                entries.extend([
                    name.clone(),
                    "a{sv}".into(),
                    (usize::from(domain.servers.is_some()) + usize::from(domain.options.is_some()))
                        .to_string(),
                ]);
                if let Some(servers) = &domain.servers {
                    array_argument(&mut entries, "servers", servers);
                }
                if let Some(options) = &domain.options {
                    array_argument(&mut entries, "options", options);
                }
            }
        }
        let mut args = vec![count.to_string()];
        args.extend(entries);
        args
    }
}

pub fn capture_network_manager() -> io::Result<DnsReceipt> {
    let mode = property(DNS_PATH, DNS_INTERFACE, "Mode")?;
    let manager = property(DNS_PATH, DNS_INTERFACE, "RcManager")?;
    if !matches!(mode.as_str(), Some("default" | "dnsmasq" | "dnsconfd"))
        || !matches!(
            manager.as_str(),
            Some("symlink" | "file" | "resolvconf" | "netconfig")
        )
    {
        return Err(linux_failure::classified(
            FailureReason::ResolverMode,
            "NetworkManager does not manage system DNS",
        ));
    }
    let canonical = fs::canonicalize("/etc/resolv.conf")?;
    if canonical != std::path::Path::new("/run/NetworkManager/resolv.conf") {
        let content = fs::read_to_string("/etc/resolv.conf")?;
        if content.len() > 64 * 1024
            || !content
                .lines()
                .take(3)
                .any(|line| line.starts_with("# Generated by NetworkManager"))
        {
            return Err(linux_failure::classified(
                FailureReason::DnsOwnership,
                "NetworkManager ownership of system resolver is unproven",
            ));
        }
    }
    Ok(DnsReceipt::NetworkManager {
        previous: read_global()?,
    })
}

pub fn apply_network_manager(previous: &GlobalDns, servers: &[IpAddr]) -> io::Result<()> {
    if read_global()? != *previous {
        return Err(invalid("NetworkManager DNS changed before apply"));
    }
    let applied = GlobalDns::for_servers(servers);
    write_global(&applied)?;
    if read_global()? != applied {
        return Err(linux_failure::classified(
            FailureReason::Verification,
            "NetworkManager DNS apply verification failed",
        ));
    }
    Ok(())
}

pub fn inspect_network_manager(servers: &[IpAddr]) -> io::Result<bool> {
    Ok(read_global()? == GlobalDns::for_servers(servers))
}

pub fn restore_network_manager(previous: &GlobalDns, servers: &[IpAddr]) -> io::Result<()> {
    let applied = GlobalDns::for_servers(servers);
    let current = read_global()?;
    let restored = merge_restore(previous, &applied, &current);
    if restored == current {
        return Ok(());
    }
    write_global(&restored)?;
    if read_global()? != restored {
        return Err(linux_failure::classified(
            FailureReason::Verification,
            "NetworkManager DNS restore verification failed",
        ));
    }
    Ok(())
}

/// Restore only fields still equal to this operation's written value. A later
/// search/option change must not strand our wildcard server, or be overwritten.
fn merge_restore(previous: &GlobalDns, applied: &GlobalDns, current: &GlobalDns) -> GlobalDns {
    let mut restored = current.clone();
    if current.searches == applied.searches {
        restored.searches.clone_from(&previous.searches);
    }
    if current.options == applied.options {
        restored.options.clone_from(&previous.options);
    }
    if current.certification_authority == applied.certification_authority {
        restored
            .certification_authority
            .clone_from(&previous.certification_authority);
    }
    if current.resolve_mode == applied.resolve_mode {
        restored.resolve_mode.clone_from(&previous.resolve_mode);
    }
    if current.domains == applied.domains {
        restored.domains.clone_from(&previous.domains);
    } else if let Some(domains) = &mut restored.domains {
        let empty = BTreeMap::new();
        let old = previous.domains.as_ref().unwrap_or(&empty);
        let written = applied.domains.as_ref().unwrap_or(&empty);
        for key in old.keys().chain(written.keys()) {
            let before = old.get(key);
            let ours = written.get(key);
            if domains.get(key) == ours {
                if let Some(before) = before {
                    domains.insert(key.clone(), before.clone());
                } else {
                    domains.remove(key);
                }
            } else if let (Some(now), Some(ours)) = (domains.get_mut(key), ours) {
                if now.servers == ours.servers {
                    now.servers = before.and_then(|value| value.servers.clone());
                }
                if now.options == ours.options {
                    now.options = before.and_then(|value| value.options.clone());
                }
            }
        }
    }
    restored
}
fn read_global() -> io::Result<GlobalDns> {
    GlobalDns::from_property(property(PATH, SERVICE, "GlobalDnsConfiguration")?)
}
fn write_global(value: &GlobalDns) -> io::Result<()> {
    value.validate()?;
    let mut args = [
        "--system",
        "--timeout=5",
        "set-property",
        SERVICE,
        PATH,
        SERVICE,
        "GlobalDnsConfiguration",
        "a{sv}",
    ]
    .map(str::to_owned)
    .to_vec();
    args.extend(value.arguments());
    command::run("/usr/bin/busctl", &args, None).map(|_| ())
}

pub fn property(path: &str, interface: &str, name: &str) -> io::Result<Value> {
    let args = [
        "--system",
        "--json=short",
        "--timeout=5",
        "get-property",
        SERVICE,
        path,
        interface,
        name,
    ]
    .map(str::to_owned);
    let value: Value = serde_json::from_slice(&command::run("/usr/bin/busctl", &args, None)?)
        .map_err(|_| invalid("invalid NetworkManager response"))?;
    value
        .get("data")
        .cloned()
        .ok_or_else(|| invalid("NetworkManager property unavailable"))
}

pub fn physical_dns(index: u32) -> io::Result<Vec<IpAddr>> {
    let name = super::network::interface_name(index)?;
    let args = [
        "--system",
        "--json=short",
        "--timeout=5",
        "call",
        SERVICE,
        PATH,
        SERVICE,
        "GetDeviceByIpIface",
        "s",
        &name,
    ]
    .map(str::to_owned);
    let response: Value = serde_json::from_slice(&command::run("/usr/bin/busctl", &args, None)?)
        .map_err(|_| invalid("invalid NetworkManager device response"))?;
    let data = &response["data"];
    let device = data
        .as_str()
        .or_else(|| data.get(0).and_then(Value::as_str))
        .ok_or_else(|| invalid("NetworkManager device unavailable"))?;
    let mut servers = Vec::new();
    for (property_name, interface) in [
        ("Ip4Config", "org.freedesktop.NetworkManager.IP4Config"),
        ("Ip6Config", "org.freedesktop.NetworkManager.IP6Config"),
    ] {
        let path = property(
            device,
            "org.freedesktop.NetworkManager.Device",
            property_name,
        )?;
        let path = path
            .as_str()
            .ok_or_else(|| invalid("invalid NetworkManager IP configuration"))?;
        if path == "/" {
            continue;
        }
        let values = unvariant(property(path, interface, "NameserverData")?)?;
        for entry in values
            .as_array()
            .ok_or_else(|| invalid("invalid NetworkManager nameservers"))?
        {
            if let Some(address) = entry
                .get("address")
                .and_then(Value::as_str)
                .and_then(|address| address.parse::<IpAddr>().ok())
                && !address.is_unspecified()
                && !address.is_loopback()
                && !address.is_multicast()
            {
                servers.push(address);
            }
        }
    }
    servers.sort();
    servers.dedup();
    servers.truncate(8);
    Ok(servers)
}

pub fn unvariant(value: Value) -> io::Result<Value> {
    fn walk(value: Value, depth: usize) -> io::Result<Value> {
        if depth > 8 {
            return Err(invalid("NetworkManager variant nesting exceeded"));
        }
        match value {
            Value::Object(mut object)
                if object.len() == 2
                    && object.contains_key("type")
                    && object.contains_key("data") =>
            {
                if !matches!(
                    object["type"].as_str(),
                    Some("a{sv}" | "as" | "s" | "b" | "i" | "u")
                ) {
                    return Err(invalid("unsupported NetworkManager variant"));
                }
                walk(object.remove("data").expect("checked variant"), depth + 1)
            }
            Value::Object(object) => object
                .into_iter()
                .map(|(key, value)| Ok((key, walk(value, depth + 1)?)))
                .collect::<io::Result<serde_json::Map<_, _>>>()
                .map(Value::Object),
            Value::Array(array) => array
                .into_iter()
                .map(|value| walk(value, depth + 1))
                .collect::<io::Result<Vec<_>>>()
                .map(Value::Array),
            value => Ok(value),
        }
    }
    walk(value, 0)
}

fn array_argument(arguments: &mut Vec<String>, name: &str, values: &[String]) {
    arguments.extend([name.to_owned(), "as".into(), values.len().to_string()]);
    arguments.extend_from_slice(values);
}
fn valid_string(value: &str) -> io::Result<()> {
    if value.len() > 512 || value.chars().any(char::is_control) {
        return Err(invalid("invalid NetworkManager DNS string"));
    }
    Ok(())
}
fn valid_list(values: &[String]) -> io::Result<()> {
    if values.len() > 64 {
        return Err(invalid("NetworkManager DNS array limit exceeded"));
    }
    values.iter().try_for_each(|value| valid_string(value))
}
fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn network_manager_snapshots_round_trip_known_variants() {
        let value = json!({"searches":{"type":"as","data":[]}, "options":{"type":"as","data":[]},
            "domains":{"type":"a{sv}","data":{"*":{"type":"a{sv}","data":{"servers":{"type":"as","data":["1.1.1.1"]}}}}}});
        let config = GlobalDns::from_property(value).unwrap();
        assert_eq!(
            config,
            GlobalDns::for_servers(&["1.1.1.1".parse().unwrap()])
        );
        assert_eq!(
            config.arguments(),
            [
                "3", "searches", "as", "0", "options", "as", "0", "domains", "a{sv}", "1", "*",
                "a{sv}", "1", "servers", "as", "1", "1.1.1.1"
            ]
        );
    }

    #[test]
    fn outside_dns_modifications_are_preserved() {
        let applied = GlobalDns::for_servers(&["1.1.1.1".parse().unwrap()]);
        let external = GlobalDns::for_servers(&["9.9.9.9".parse().unwrap()]);
        let previous = GlobalDns::default();
        assert_eq!(merge_restore(&previous, &applied, &applied), previous);
        assert_eq!(
            merge_restore(&previous, &applied, &external).domains,
            external.domains
        );
        let mut changed_search = applied.clone();
        changed_search.searches = Some(vec!["example.test".into()]);
        let result = merge_restore(&previous, &applied, &changed_search);
        assert_eq!(result.searches, changed_search.searches);
        assert_eq!(result.domains, previous.domains);
        let mut changed_option = applied.clone();
        changed_option
            .domains
            .as_mut()
            .unwrap()
            .get_mut("*")
            .unwrap()
            .options = Some(vec!["timeout:2".into()]);
        let result = merge_restore(&previous, &applied, &changed_option);
        assert_eq!(result.domains.as_ref().unwrap()["*"].servers, None);
        assert_eq!(
            result.domains.as_ref().unwrap()["*"].options,
            Some(vec!["timeout:2".into()])
        );
    }

    #[test]
    fn unknown_dns_schema_is_rejected_before_mutation() {
        assert!(GlobalDns::from_property(json!({"unknown":"value"})).is_err());
        assert!(GlobalDns::from_property(json!({"options":{"type":"h","data":42}})).is_err());
    }
}
