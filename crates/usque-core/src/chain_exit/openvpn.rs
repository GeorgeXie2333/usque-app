use super::{
    ChainProtocol, ClientCertificateMode, Endpoint, ImportError, MAX_CONFIG_BYTES, MssModifier,
    MssPolicy, OpenVpnEndpoint, OpenVpnProfile,
};
use std::collections::BTreeSet;
use zeroize::Zeroizing;

pub(super) fn parse(text: &str) -> Result<OpenVpnProfile, ImportError> {
    parse_record(text, false)
}
pub(super) fn parse_record(text: &str, legacy: bool) -> Result<OpenVpnProfile, ImportError> {
    let mut output = Zeroizing::new(String::new());
    let mut blocks = BTreeSet::new();
    let mut directives = BTreeSet::new();
    let mut block: Option<&str> = None;
    let mut remotes = Vec::new();
    let mut remote_random = false;
    let mut client_cert = None;
    let mut mss = MssPolicy::Default;
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
        if !directives.insert(name) && name != "remote" {
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
                remotes.push((Endpoint::parse(host, "1194", line)?, None, line));
                true
            }
            ("remote", [host, port]) => {
                remotes.push((Endpoint::parse(host, port, line)?, None, line));
                true
            }
            ("remote", [host, port, proto]) => {
                remotes.push((
                    Endpoint::parse(host, port, line)?,
                    Some(parse_protocol(proto, line)?),
                    line,
                ));
                true
            }
            ("proto", [proto]) => {
                protocol = Some(parse_protocol(proto, line)?);
                true
            }
            ("remote-random", []) => {
                remote_random = true;
                true
            }
            ("setenv", ["CLIENT_CERT", value @ ("0" | "1")]) => {
                client_cert = Some((*value == "1", line));
                true
            }
            ("mssfix", ["0"]) => {
                mss = MssPolicy::Disabled;
                true
            }
            ("mssfix", [value]) if number(value, 576, 65535) => {
                mss = MssPolicy::Value {
                    value: value.parse().expect("validated"),
                    modifier: MssModifier::None,
                };
                true
            }
            ("mssfix", [value, modifier @ ("mtu" | "fixed")]) if number(value, 576, 65535) => {
                mss = MssPolicy::Value {
                    value: value.parse().expect("validated"),
                    modifier: if *modifier == "mtu" {
                        MssModifier::Mtu
                    } else {
                        MssModifier::Fixed
                    },
                };
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
        if !matches!(name, "remote" | "proto" | "remote-random" | "setenv") {
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
    let has_cert = blocks.contains("cert");
    if client_cert.is_some_and(|(required, _)| required != has_cert)
        || !legacy && !has_cert && !auth
    {
        return Err(ImportError::new(
            client_cert.map_or(0, |(_, line)| line),
            "CLIENT_CERT",
            "conflicting_authentication",
        ));
    }
    let client_certificate = if has_cert {
        ClientCertificateMode::Required
    } else {
        ClientCertificateMode::Disabled
    };
    let default_protocol = protocol.unwrap_or((ChainProtocol::OpenvpnUdp, None));
    let mut candidates = Vec::new();
    let mut selected_protocol = None;
    for (endpoint, override_protocol, line) in remotes {
        if protocol.is_some() && override_protocol.is_some_and(|p| p.0 != default_protocol.0) {
            return Err(ImportError::new(line, "proto", "conflicting_protocol"));
        }
        let (protocol, ipv6) = override_protocol.unwrap_or(default_protocol);
        if selected_protocol.is_some_and(|selected| selected != protocol) {
            return Err(ImportError::new(line, "remote", "mixed_protocols"));
        }
        selected_protocol = Some(protocol);
        if endpoint
            .address()
            .is_some_and(|address| ipv6.is_some_and(|v6| address.is_ipv6() != v6))
        {
            return Err(ImportError::new(
                line,
                "remote",
                "conflicting_address_family",
            ));
        }
        let ipv6 = endpoint.address().map(|address| address.is_ipv6()).or(ipv6);
        let candidate = OpenVpnEndpoint { endpoint, ipv6 };
        if !candidates.contains(&candidate) {
            candidates.push(candidate);
        }
        if candidates.len() > 16 {
            return Err(ImportError::new(line, "remote", "too_many_endpoints"));
        }
    }
    let first = candidates
        .first()
        .ok_or_else(|| ImportError::new(0, "remote", "missing_endpoint"))?;
    let endpoint = first.endpoint.clone();
    let endpoint_ipv6 = first.ipv6;
    let protocol = selected_protocol.expect("nonempty candidates");
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
        candidates,
        remote_random,
        client_certificate,
        mss,
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
