use super::{Failure, crypto::KeyMaterial};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::Value;
use std::net::Ipv4Addr;
use usque_core::{
    chain_exit::{ChainSource, Endpoint, ImportSecrets, ValidatedProfile},
    proton_vpn::{Node, SelectedNode},
};

pub(super) fn collect(directory: &Value, config: &Value) -> Result<Vec<SelectedNode>, Failure> {
    let fallback = config
        .pointer("/DefaultPorts/WireGuard/UDP")
        .and_then(Value::as_array)
        .filter(|v| !v.is_empty() && v.len() <= 32)
        .ok_or_else(|| Failure::new("invalid_client_configuration"))?;
    let port = |value: &Value| {
        value
            .as_u64()
            .and_then(|v| u16::try_from(v).ok())
            .filter(|v| *v > 0)
    };
    if fallback.iter().any(|v| port(v).is_none()) {
        return Err(Failure::new("invalid_client_configuration"));
    }
    let logicals = directory
        .get("LogicalServers")
        .and_then(Value::as_array)
        .filter(|v| v.len() <= 20000)
        .ok_or_else(|| Failure::new("invalid_server_directory"))?;
    let mut output = Vec::new();
    for logical in logicals {
        if logical.get("Tier").and_then(Value::as_u64) != Some(0)
            || logical.get("Status").and_then(Value::as_u64).unwrap_or(1) != 1
            || logical
                .get("Features")
                .and_then(Value::as_u64)
                .is_none_or(|v| v & 33 != 0)
        {
            continue;
        }
        let Some(logical_id) = text(logical.get("ID"), 100) else {
            continue;
        };
        let Some(name) = text(logical.get("Name"), 64) else {
            continue;
        };
        let Some(country) = logical
            .get("ExitCountry")
            .and_then(Value::as_str)
            .filter(|v| v.len() == 2 && v.bytes().all(|v| v.is_ascii_uppercase()))
        else {
            continue;
        };
        let Some(physicals) = logical.get("Servers").and_then(Value::as_array) else {
            continue;
        };
        for physical in physicals {
            if physical.get("Status").and_then(Value::as_u64) != Some(1) {
                continue;
            }
            let Some(id) = text(physical.get("ID"), 100) else {
                continue;
            };
            let entry = match physical.get("EntryPerProtocol") {
                None | Some(Value::Null) => None,
                Some(Value::Object(protocols)) => match protocols.get("WireGuardUDP") {
                    None => None,
                    Some(Value::Object(entry)) => Some(entry),
                    _ => continue,
                },
                _ => continue,
            };
            // Official ConnectingDomain falls back to EntryIP when this
            // protocol has no override. Current v2 directories use that shape.
            // A malformed explicit override must not silently select another IP.
            let Some(address) = entry
                .map_or_else(|| physical.get("EntryIP"), |entry| entry.get("IPv4"))
                .and_then(Value::as_str)
                .and_then(|v| v.parse::<Ipv4Addr>().ok())
                .filter(public)
            else {
                continue;
            };
            let ports = match entry.and_then(|entry| entry.get("Ports")) {
                None | Some(Value::Null) => fallback,
                Some(Value::Array(ports)) => ports,
                _ => continue,
            };
            if ports.is_empty() || ports.len() > 32 || ports.iter().any(|v| port(v).is_none()) {
                continue;
            }
            let Some(port) = ports.first().and_then(port) else {
                continue;
            };
            let Some(public_key) = physical
                .get("X25519PublicKey")
                .and_then(Value::as_str)
                .filter(|v| v.len() == 44)
            else {
                continue;
            };
            let Ok(raw) = STANDARD.decode(public_key) else {
                continue;
            };
            if raw.len() != 32 || raw.iter().all(|v| *v == 0) || STANDARD.encode(raw) != public_key
            {
                continue;
            }
            let Some(domain) = text(physical.get("Domain"), 253) else {
                continue;
            };
            let node = SelectedNode {
                metadata: Node {
                    id: format!("{logical_id}:{id}"),
                    name: name.into(),
                    country: country.into(),
                    city: text(logical.get("City"), 128).unwrap_or("").into(),
                    load: logical
                        .get("Load")
                        .and_then(Value::as_u64)
                        .unwrap_or(100)
                        .min(100) as u32,
                },
                domain: domain.into(),
                endpoint: Endpoint {
                    host: address.to_string(),
                    port,
                },
                public_key: public_key.into(),
            };
            if node.validate().is_ok() {
                output.push(node);
            }
            if output.len() >= 1024 {
                break;
            }
        }
        if output.len() >= 1024 {
            break;
        }
    }
    output.sort_by(|a, b| {
        (a.metadata.load, &a.metadata.name, &a.metadata.id).cmp(&(
            b.metadata.load,
            &b.metadata.name,
            &b.metadata.id,
        ))
    });
    output.dedup_by(|a, b| a.metadata.id == b.metadata.id);
    output.truncate(200);
    if output.is_empty() {
        return Err(Failure::new("no_free_wireguard_nodes"));
    }
    Ok(output)
}
fn text(value: Option<&Value>, max: usize) -> Option<&str> {
    value
        .and_then(Value::as_str)
        .filter(|v| !v.is_empty() && v.len() <= max && !v.chars().any(char::is_control))
}
fn public(ip: &Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !(a == 0
        || a == 10
        || a == 127
        || a >= 224
        || a == 100 && (64..=127).contains(&b)
        || a == 169 && b == 254
        || a == 172 && (16..=31).contains(&b)
        || a == 192 && (b == 168 || b == 0 && (c == 0 || c == 2))
        || a == 192 && b == 88 && c == 99
        || a == 198 && (b == 18 || b == 19 || b == 51 && c == 100)
        || a == 203 && b == 0 && c == 113)
}
pub(super) fn configuration(
    keys: &KeyMaterial,
    node: &SelectedNode,
) -> Result<ImportSecrets, Failure> {
    node.validate()?;
    let result = ImportSecrets::new(format!(
        "[Interface]\nPrivateKey = {}\nAddress = 10.2.0.2/32\nDNS = 10.2.0.1\n\n[Peer]\nPublicKey = {}\nEndpoint = {}:{}\nAllowedIPs = 0.0.0.0/0, ::/0\nPersistentKeepalive = 60\n",
        STANDARD.encode(keys.wireguard_private()),
        node.public_key,
        node.endpoint.host,
        node.endpoint.port
    ));
    ValidatedProfile::parse(ChainSource::ProtonVpn, &result)?;
    Ok(result)
}
