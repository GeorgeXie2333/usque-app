//! Restricted VPN Gate profiles. No downloaded directive can execute a command,
//! read another file, change a remote endpoint, or disable peer verification.
use std::collections::BTreeSet;
use std::fmt;
use std::net::{IpAddr, SocketAddr};
use zeroize::Zeroizing;

pub const MAX_CONFIG_BYTES: usize = 128 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnsupportedReason {
    UdpTransport,
    InvalidEndpoint,
    UnsupportedDirective,
    InvalidProfile,
}

#[derive(Clone)]
pub struct PreparedProfile {
    pub remote: SocketAddr,
    content: Zeroizing<String>,
    pub custom: Option<Box<crate::chain_exit::ValidatedProfile>>,
    pub summary: Option<Box<crate::chain_exit::ChainProfileSummary>>,
    pub credentials: Box<crate::chain_exit::ImportSecrets>,
}
impl fmt::Debug for PreparedProfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PreparedProfile")
            .field("content", &"[redacted]")
            .finish()
    }
}
impl PreparedProfile {
    pub fn imported(
        summary: crate::chain_exit::ChainProfileSummary,
        custom: crate::chain_exit::ValidatedProfile,
        credentials: crate::chain_exit::ImportSecrets,
    ) -> Self {
        let remote = summary.endpoint.address().unwrap_or_else(|| {
            SocketAddr::new(
                std::net::Ipv4Addr::UNSPECIFIED.into(),
                summary.endpoint.port,
            )
        });
        let content = match &custom {
            crate::chain_exit::ValidatedProfile::OpenVpn(p) => p.content.clone(),
            crate::chain_exit::ValidatedProfile::WireGuard(_) => Zeroizing::new(String::new()),
        };
        Self {
            remote,
            content,
            custom: Some(Box::new(custom)),
            summary: Some(Box::new(summary)),
            credentials: Box::new(credentials),
        }
    }
    pub fn content(&self) -> &str {
        &self.content
    }
}

pub fn prepare_profile(
    content: &str,
    node_ip: IpAddr,
) -> Result<PreparedProfile, UnsupportedReason> {
    use UnsupportedReason::{InvalidEndpoint, InvalidProfile, UdpTransport, UnsupportedDirective};
    if content.is_empty()
        || content.len() > MAX_CONFIG_BYTES
        || content
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\r' | '\n' | '\t'))
    {
        return Err(InvalidProfile);
    }
    if !public_endpoint(node_ip) {
        return Err(InvalidEndpoint);
    }
    let mut normalized = Zeroizing::new(String::new());
    let mut block: Option<&str> = None;
    let mut blocks = BTreeSet::new();
    let mut directives = BTreeSet::new();
    let mut remote = None;
    let mut udp = false;
    for line in content.lines() {
        let line = line.trim();
        if let Some(active) = block {
            if line == format!("</{active}>") {
                block = None;
            } else if line.contains('<') || line.contains('>') || !line.is_ascii() {
                return Err(InvalidProfile);
            }
            normalized.push_str(line);
            normalized.push('\n');
            continue;
        }
        if line.is_empty() || line.starts_with(['#', ';']) {
            continue;
        }
        if line.starts_with('<') {
            let name = line
                .strip_prefix('<')
                .and_then(|s| s.strip_suffix('>'))
                .ok_or(InvalidProfile)?;
            if !matches!(name, "ca" | "cert" | "key" | "tls-auth" | "tls-crypt") {
                return Err(UnsupportedDirective);
            }
            if !blocks.insert(name) {
                return Err(InvalidProfile);
            }
            block = Some(name);
            normalized.push_str(line);
            normalized.push('\n');
            continue;
        }
        if line.contains(['"', '\'', '\\']) {
            return Err(UnsupportedDirective);
        }
        let parts: Vec<_> = line
            .split_ascii_whitespace()
            .take_while(|v| !v.starts_with(['#', ';']))
            .collect();
        let Some(&name) = parts.first() else {
            continue;
        };
        let args = &parts[1..];
        if !directives.insert(name) {
            return Err(InvalidProfile);
        }
        let allowed = match (name, args) {
            ("client" | "tls-client" | "nobind" | "persist-key" | "persist-tun", []) => true,
            ("dev" | "dev-type", ["tun"]) => true,
            ("proto", [protocol]) => {
                udp = matches!(*protocol, "udp" | "udp4" | "udp6");
                udp || matches!(
                    *protocol,
                    "tcp" | "tcp-client" | "tcp4" | "tcp6" | "tcp4-client" | "tcp6-client"
                )
            }
            ("remote", [ip, port]) => {
                let ip: IpAddr = ip.parse().map_err(|_| InvalidEndpoint)?;
                let port: u16 = port.parse().map_err(|_| InvalidEndpoint)?;
                if ip != node_ip || port == 0 {
                    return Err(InvalidEndpoint);
                }
                remote = Some(SocketAddr::new(ip, port));
                true
            }
            ("cipher", [cipher]) => allowed_cipher(cipher),
            ("data-ciphers", [ciphers]) => ciphers.split(':').all(allowed_cipher),
            ("auth", ["SHA1" | "SHA256" | "SHA384" | "SHA512"]) => true,
            ("remote-cert-tls", ["server"]) => true,
            ("tls-version-min", ["1.2" | "1.3"]) => true,
            ("tls-version-max", ["1.2" | "1.3"]) => true,
            ("key-direction", ["0" | "1"]) => true,
            ("resolv-retry", ["infinite"]) => true,
            ("verb", [v]) => v.parse::<u8>().is_ok_and(|n| n <= 3),
            ("ping" | "ping-restart" | "connect-timeout" | "connect-retry", [v]) => {
                v.parse::<u16>().is_ok_and(|n| (1..=600).contains(&n))
            }
            ("keepalive", [a, b]) => [a, b]
                .iter()
                .all(|v| v.parse::<u16>().is_ok_and(|n| (1..=600).contains(&n))),
            ("tun-mtu", [v]) => v.parse::<u16>().is_ok_and(|n| (1280..=9000).contains(&n)),
            _ => false,
        };
        if !allowed {
            return Err(UnsupportedDirective);
        }
        normalized.push_str(&parts.join(" "));
        normalized.push('\n');
    }
    if block.is_some()
        || !directives.contains("client")
        || !directives.contains("dev")
        || !directives.contains("proto")
        || !blocks.contains("ca")
        || blocks.contains("cert") != blocks.contains("key")
    {
        return Err(InvalidProfile);
    }
    if udp {
        return Err(UdpTransport);
    }
    if !directives.contains("tls-version-min") {
        normalized.push_str("tls-version-min 1.2\n");
    }
    if !directives.contains("remote-cert-tls") {
        normalized.push_str("remote-cert-tls server\n");
    }
    // Enforced defaults also count toward the native configuration budget.
    if normalized.len() > MAX_CONFIG_BYTES {
        return Err(InvalidProfile);
    }
    Ok(PreparedProfile {
        remote: remote.ok_or(InvalidEndpoint)?,
        content: normalized,
        custom: None,
        summary: None,
        credentials: Default::default(),
    })
}

fn allowed_cipher(cipher: &str) -> bool {
    matches!(
        cipher,
        "AES-128-CBC" | "AES-256-CBC" | "AES-128-GCM" | "AES-256-GCM" | "CHACHA20-POLY1305"
    )
}

fn public_endpoint(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let [a, b, c, _] = ip.octets();
            !(a == 0
                || a == 10
                || a == 127
                || a >= 224
                || a == 100 && (64..=127).contains(&b)
                || a == 169 && b == 254
                || a == 172 && (16..=31).contains(&b)
                || a == 192 && (b == 168 || b == 0 && (c == 0 || c == 2))
                // Deprecated relay range, including the non-global 6a44 address.
                || a == 192 && b == 88 && c == 99
                || a == 198 && (b == 18 || b == 19 || b == 51 && c == 100)
                || a == 203 && b == 0 && c == 113)
        }
        IpAddr::V6(ip) => {
            let segments = ip.segments();
            (0x2000..=0x3fff).contains(&segments[0])
                // IANA special-purpose blocks: benchmarking (2001:2::/48)
                // and documentation (2001:db8::/32 and 3fff::/20).
                && !matches!(
                    segments,
                    [0x2001, 0x0002, 0, ..]
                        | [0x2001, 0x0db8, ..]
                        | [0x3fff, 0x0000..=0x0fff, ..]
                )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(extra: &str) -> String {
        format!(
            "client\ndev tun\nproto tcp\nremote 8.8.4.4 443\ncipher AES-128-CBC\nauth SHA1\n<ca>\nTEST\n</ca>\n{extra}"
        )
    }
    #[test]
    fn restricts_downloaded_profiles_and_preserves_endpoint() {
        let prepared = prepare_profile(&fixture(""), "8.8.4.4".parse().unwrap()).unwrap();
        assert_eq!(prepared.remote.to_string(), "8.8.4.4:443");
        assert!(prepared.content().contains("tls-version-min 1.2"));
        for directive in [
            "up bad.exe",
            "plugin module",
            "ca other.pem",
            "script-security 2",
            "http-proxy 1.1.1.1 80",
            "tls-cert-profile insecure",
            "remote-cert-tls client",
            "compress lz4",
            "auth-user-pass",
        ] {
            assert!(
                prepare_profile(&fixture(directive), "8.8.4.4".parse().unwrap()).is_err(),
                "{directive}"
            );
        }
        assert!(prepare_profile(&fixture(""), "1.1.1.1".parse().unwrap()).is_err());
    }
    #[test]
    fn never_converts_udp_and_rejects_malformed_blocks() {
        let text = fixture("").replace("proto tcp", "proto udp");
        assert!(matches!(
            prepare_profile(&text, "8.8.4.4".parse().unwrap()),
            Err(UnsupportedReason::UdpTransport)
        ));
        for text in [
            fixture("").replace("</ca>", ""),
            fixture("").replace("TEST", "<key>"),
            fixture("proto tcp"),
        ] {
            assert!(prepare_profile(&text, "8.8.4.4".parse().unwrap()).is_err());
        }
    }
    #[test]
    fn enforced_defaults_cannot_exceed_the_native_configuration_budget() {
        let base = fixture("");
        let enforced = "tls-version-min 1.2\nremote-cert-tls server\n".len();
        let padding = MAX_CONFIG_BYTES - base.len() - enforced;
        let exact = base.replace("TEST", &format!("TEST{}", "A".repeat(padding)));
        let prepared = prepare_profile(&exact, "8.8.4.4".parse().unwrap()).unwrap();
        assert_eq!(prepared.content().len(), MAX_CONFIG_BYTES);
        let oversized = exact.replace("TEST", "TESTA");
        assert!(oversized.len() < MAX_CONFIG_BYTES);
        assert!(matches!(
            prepare_profile(&oversized, "8.8.4.4".parse().unwrap()),
            Err(UnsupportedReason::InvalidProfile)
        ));
    }
    #[test]
    fn rejects_nonpublic_remote_addresses() {
        for address in [
            "127.0.0.1",
            "10.0.0.1",
            "169.254.1.1",
            "100.64.0.1",
            "::1",
            "fc00::1",
            "2001:db8::1",
            "::ffff:127.0.0.1",
        ] {
            assert!(!public_endpoint(address.parse().unwrap()));
        }
    }
    #[test]
    fn rejects_special_use_remote_prefixes_including_their_boundaries() {
        for address in [
            "2001:2::",
            "2001:2::1",
            "2001:2:0:ffff:ffff:ffff:ffff:ffff",
            "2001:db8::",
            "2001:db8:ffff:ffff:ffff:ffff:ffff:ffff",
            "3fff::",
            "3fff::1",
            "3fff:fff:ffff:ffff:ffff:ffff:ffff:ffff",
            "192.88.99.0",
            "192.88.99.2",
            "192.88.99.255",
        ] {
            let content = fixture("").replace("8.8.4.4", address);
            assert!(
                matches!(
                    prepare_profile(&content, address.parse().unwrap()),
                    Err(UnsupportedReason::InvalidEndpoint)
                ),
                "{address}"
            );
        }
    }
    #[test]
    fn preserves_public_remotes_outside_rejected_prefixes() {
        for address in [
            "8.8.4.4",
            "192.88.98.255",
            "192.88.100.0",
            "2001:4860:4860::8888",
            "2606:4700:4700::1111",
            "3ffe:ffff:ffff:ffff:ffff:ffff:ffff:ffff",
            "3fff:1000::",
        ] {
            let ip = address.parse().unwrap();
            let content = fixture("").replace("8.8.4.4", address);
            let prepared = prepare_profile(&content, ip).unwrap();
            assert_eq!(prepared.remote, SocketAddr::new(ip, 443));
        }
    }
}
