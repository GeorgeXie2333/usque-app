use super::{ChainProtocol, Endpoint, ImportError, MAX_CONFIG_BYTES, OpenVpnProfile};
use std::collections::BTreeSet;
use zeroize::Zeroizing;

pub(super) fn parse(text: &str) -> Result<OpenVpnProfile, ImportError> {
    let mut output = Zeroizing::new(String::new());
    let mut blocks = BTreeSet::new();
    let mut directives = BTreeSet::new();
    let mut block: Option<&str> = None;
    let mut endpoint = None;
    let mut protocol = None;
    let mut auth = false;
    let mut key_password = false;
    for (index, raw) in text.trim_start_matches('\u{feff}').lines().enumerate() {
        let line = index + 1;
        let raw = raw.trim();
        if let Some(name) = block {
            if raw == format!("</{name}>") {
                block = None;
            } else if raw.contains(['<', '>']) || !raw.is_ascii() {
                return Err(ImportError::new(line, "inline_block", "invalid_block"));
            }
            if name == "key" && raw.contains("ENCRYPTED") {
                key_password = true;
            }
            output.push_str(raw);
            output.push('\n');
            continue;
        }
        if raw.is_empty() || raw.starts_with(['#', ';']) {
            continue;
        }
        if raw.starts_with('<') {
            let name = raw
                .strip_prefix('<')
                .and_then(|s| s.strip_suffix('>'))
                .ok_or_else(|| ImportError::new(line, "inline_block", "invalid_block"))?;
            if !matches!(name, "ca" | "cert" | "key" | "tls-auth" | "tls-crypt")
                || !blocks.insert(name)
            {
                return Err(ImportError::new(
                    line,
                    "inline_block",
                    "unsupported_or_duplicate_block",
                ));
            }
            block = Some(name);
            output.push_str(raw);
            output.push('\n');
            continue;
        }
        let parts =
            words(raw).ok_or_else(|| ImportError::new(line, "directive", "invalid_syntax"))?;
        let Some(name) = parts.first().copied() else {
            continue;
        };
        let args = &parts[1..];
        if !directives.insert(name) {
            return Err(ImportError::new(line, "directive", "duplicate_directive"));
        }
        let allowed = match (name, args) {
            (
                "client" | "tls-client" | "nobind" | "persist-key" | "persist-tun" | "auth-nocache"
                | "pull",
                [],
            ) => true,
            ("dev" | "dev-type", ["tun"]) => true,
            ("remote", [host]) => {
                endpoint = Some(Endpoint::parse(host, "1194", line)?);
                true
            }
            ("remote", [host, port]) => {
                endpoint = Some(Endpoint::parse(host, port, line)?);
                true
            }
            ("remote", [host, port, proto]) => {
                endpoint = Some(Endpoint::parse(host, port, line)?);
                let selected = parse_protocol(proto, line)?;
                if protocol.is_some_and(|p| p != selected) {
                    return Err(ImportError::new(line, "proto", "conflicting_protocol"));
                }
                protocol = Some(selected);
                true
            }
            ("proto", [proto]) => {
                let selected = parse_protocol(proto, line)?;
                if protocol.is_some_and(|p| p != selected) {
                    return Err(ImportError::new(line, "proto", "conflicting_protocol"));
                }
                protocol = Some(selected);
                true
            }
            ("auth-user-pass", []) => {
                auth = true;
                true
            }
            ("cipher" | "data-ciphers-fallback", [cipher]) => cipher_allowed(cipher),
            ("data-ciphers", [ciphers]) => {
                !ciphers.is_empty() && ciphers.split(':').all(cipher_allowed)
            }
            ("auth", ["SHA1" | "SHA256" | "SHA384" | "SHA512"]) => true,
            ("remote-cert-tls", ["server"]) => true,
            ("verify-x509-name", [value, "name" | "name-prefix" | "subject"]) => {
                !value.is_empty() && value.len() <= 256
            }
            ("tls-version-min" | "tls-version-max", ["1.2" | "1.3"]) => true,
            ("key-direction", ["0" | "1"]) => true,
            ("resolv-retry", ["infinite"]) => true,
            ("verb", [value]) => number(value, 0, 3),
            (
                "ping"
                | "ping-restart"
                | "connect-timeout"
                | "connect-retry"
                | "server-poll-timeout",
                [value],
            ) => number(value, 1, 600),
            ("reneg-sec", [value]) => number(value, 0, 86400),
            ("keepalive", [a, b]) => number(a, 1, 600) && number(b, 1, 600),
            ("tun-mtu", [value]) => number(value, 1280, 9000),
            ("explicit-exit-notify", []) => true,
            ("explicit-exit-notify", [value]) => number(value, 0, 10),
            ("redirect-gateway", args) => args
                .iter()
                .all(|v| matches!(*v, "def1" | "ipv6" | "!ipv4" | "bypass-dhcp" | "bypass-dns")),
            _ => false,
        };
        if !allowed {
            return Err(ImportError::new(line, "directive", "unsupported_directive"));
        }
        // Endpoint/protocol are re-emitted once; every connection uses the WARP
        // resolver and the native bridge's exact numeric endpoint override.
        if !matches!(name, "remote" | "proto") {
            output.push_str(raw);
            output.push('\n');
        }
    }
    if block.is_some()
        || !directives.contains("client")
        || !directives.contains("dev")
        || !blocks.contains("ca")
        || blocks.contains("cert") != blocks.contains("key")
    {
        return Err(ImportError::new(0, "configuration", "incomplete_profile"));
    }
    let endpoint = endpoint.ok_or_else(|| ImportError::new(0, "remote", "missing_endpoint"))?;
    let (protocol, endpoint_ipv6) = protocol.unwrap_or((ChainProtocol::OpenvpnUdp, None));
    if let Some(address) = endpoint.address()
        && endpoint_ipv6.is_some_and(|v6| address.is_ipv6() != v6)
    {
        return Err(ImportError::new(0, "remote", "conflicting_address_family"));
    }
    output.push_str(&format!(
        "remote {} {}\nproto {}\n",
        endpoint.host,
        endpoint.port,
        if protocol == ChainProtocol::OpenvpnTcp {
            "tcp-client"
        } else {
            "udp"
        }
    ));
    if !directives.contains("remote-cert-tls") {
        output.push_str("remote-cert-tls server\n");
    }
    if !directives.contains("tls-version-min") {
        output.push_str("tls-version-min 1.2\n");
    }
    if output.len() > MAX_CONFIG_BYTES {
        return Err(ImportError::new(0, "configuration", "size_limit"));
    }
    Ok(OpenVpnProfile {
        endpoint,
        endpoint_ipv6,
        protocol,
        content: output,
        requires_auth: auth,
        requires_key_password: key_password,
    })
}
fn parse_protocol(value: &str, line: usize) -> Result<(ChainProtocol, Option<bool>), ImportError> {
    let protocol = match value {
        "tcp" | "tcp-client" | "tcp4" | "tcp6" | "tcp4-client" | "tcp6-client" => {
            ChainProtocol::OpenvpnTcp
        }
        "udp" | "udp4" | "udp6" => ChainProtocol::OpenvpnUdp,
        _ => return Err(ImportError::new(line, "proto", "unsupported_protocol")),
    };
    Ok((
        protocol,
        if value.contains('4') {
            Some(false)
        } else if value.contains('6') {
            Some(true)
        } else {
            None
        },
    ))
}
fn number(value: &str, min: u32, max: u32) -> bool {
    value.parse::<u32>().is_ok_and(|n| (min..=max).contains(&n))
}
fn cipher_allowed(value: &str) -> bool {
    matches!(
        value,
        "AES-128-CBC" | "AES-256-CBC" | "AES-128-GCM" | "AES-256-GCM" | "CHACHA20-POLY1305"
    )
}
fn words(mut line: &str) -> Option<Vec<&str>> {
    let mut result = Vec::new();
    while !line.is_empty() {
        line = line.trim_start();
        if line.is_empty() || line.starts_with(['#', ';']) {
            break;
        }
        if line.contains('\\') {
            return None;
        }
        if let Some(quote) = line.chars().next().filter(|c| matches!(c, '\'' | '"')) {
            let end = line[1..].find(quote)? + 1;
            result.push(&line[1..end]);
            line = &line[end + 1..];
            if !line.is_empty() && !line.starts_with(char::is_whitespace) {
                return None;
            }
        } else {
            let end = line.find(char::is_whitespace).unwrap_or(line.len());
            let token = &line[..end];
            if token.contains(['\'', '"']) {
                return None;
            }
            result.push(token);
            line = &line[end..];
        }
    }
    Some(result)
}
