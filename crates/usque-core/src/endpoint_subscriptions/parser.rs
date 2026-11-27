use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use reqwest::Url;

use super::{
    MAX_ENDPOINTS_PER_SUBSCRIPTION, MAX_SUBSCRIPTION_NAME_CHARS, MAX_SUBSCRIPTION_NOTE_CHARS,
    MAX_SUBSCRIPTION_URL_BYTES, SubscriptionEndpoint, SubscriptionEndpointHost, SubscriptionError,
    canonical_endpoint_id,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedSubscription {
    pub endpoints: Vec<SubscriptionEndpoint>,
    pub skipped_line_count: u32,
}

/// Validates an external subscription URL (strictly HTTPS, bounded length,
/// no userinfo or fragment, and no private/loopback/reserved literal IP or local host).
pub fn validate_subscription_url(raw: &str) -> Result<String, SubscriptionError> {
    let trimmed = raw.trim();
    if trimmed.is_empty()
        || trimmed.len() > MAX_SUBSCRIPTION_URL_BYTES
        || trimmed.chars().any(char::is_control)
    {
        return Err(SubscriptionError::InvalidUrl);
    }
    let parsed = Url::parse(trimmed).map_err(|_| SubscriptionError::InvalidUrl)?;
    if parsed.scheme() != "https"
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.fragment().is_some()
        || parsed.port() == Some(0)
    {
        return Err(SubscriptionError::InvalidUrl);
    }
    let host = parsed.host_str().ok_or(SubscriptionError::InvalidUrl)?;
    validate_subscription_url_host(host)?;
    // Percent-encoding and IDNA can lengthen the input; the stored form is
    // the serialized URL, so bound that value too.
    if parsed.as_str().len() > MAX_SUBSCRIPTION_URL_BYTES {
        return Err(SubscriptionError::InvalidUrl);
    }
    Ok(parsed.into())
}

/// Resolves and re-validates an HTTP redirect `Location` target against a base
/// subscription URL, rejecting scheme downgrades, userinfo, fragments, or local hosts.
pub fn resolve_subscription_redirect_url(
    base_url: &str,
    location: &str,
) -> Result<String, SubscriptionError> {
    let base = Url::parse(base_url.trim()).map_err(|_| SubscriptionError::InvalidUrl)?;
    let trimmed_loc = location.trim();
    if trimmed_loc.is_empty() || trimmed_loc.chars().any(char::is_control) {
        return Err(SubscriptionError::InvalidUrl);
    }
    let joined = base
        .join(trimmed_loc)
        .map_err(|_| SubscriptionError::InvalidUrl)?;
    validate_subscription_url(joined.as_str())
}

fn validate_subscription_url_host(host: &str) -> Result<(), SubscriptionError> {
    if host.starts_with('[') && host.ends_with(']') && host.len() >= 2 {
        let ipv6: Ipv6Addr = host[1..host.len() - 1]
            .parse()
            .map_err(|_| SubscriptionError::InvalidUrl)?;
        return is_usable_public_endpoint_ip(ipv6.into())
            .then_some(())
            .ok_or(SubscriptionError::InvalidUrl);
    }
    if let Ok(ipv4) = host.parse::<Ipv4Addr>() {
        return is_usable_public_endpoint_ip(ipv4.into())
            .then_some(())
            .ok_or(SubscriptionError::InvalidUrl);
    }
    canonical_public_domain(host)
        .map(|_| ())
        .ok_or(SubscriptionError::InvalidUrl)
}

/// Validates and trims a subscription display name.
pub fn validate_subscription_name(raw: &str) -> Result<String, SubscriptionError> {
    let sanitized = strip_unsafe_text(raw);
    let count = sanitized.chars().count();
    if count == 0 || count > MAX_SUBSCRIPTION_NAME_CHARS {
        return Err(SubscriptionError::InvalidName);
    }
    Ok(sanitized)
}

/// Returns a redacted stand-in so subscription URLs never enter logs or diagnostics.
pub const fn redacted_url_summary() -> &'static str {
    "https://<redacted>"
}

/// Returns `true` only for globally routable public unicast IPv4/IPv6 addresses.
pub fn is_usable_public_endpoint_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let octets = v4.octets();
            !v4.is_unspecified()
                && octets[0] != 0
                && !v4.is_loopback()
                && !v4.is_private()
                && !v4.is_link_local()
                && !v4.is_multicast()
                && !v4.is_broadcast()
                && !v4.is_documentation()
                // CGNAT 100.64.0.0/10
                && !(octets[0] == 100 && (64..=127).contains(&octets[1]))
                // IETF protocol assignments 192.0.0.0/24
                && !(octets[0] == 192 && octets[1] == 0 && octets[2] == 0)
                // Benchmarking 198.18.0.0/15
                && !(octets[0] == 198 && (18..=19).contains(&octets[1]))
                // Reserved 240.0.0.0/4
                && octets[0] < 240
        }
        IpAddr::V6(v6) => {
            let seg = v6.segments();
            !v6.is_unspecified()
                && !v6.is_loopback()
                && !v6.is_multicast()
                && !v6.is_unicast_link_local()
                && !v6.is_unique_local()
                && v6.to_ipv4_mapped().is_none()
                // Require global unicast 2000::/3 and exclude 2001:db8::/32 documentation
                && (seg[0] & 0xe000) == 0x2000
                && !(seg[0] == 0x2001 && seg[1] == 0x0db8)
                // Teredo 2001::/32
                && !(seg[0] == 0x2001 && seg[1] == 0)
                // Benchmarking 2001:2::/48
                && !(seg[0] == 0x2001 && seg[1] == 0x0002 && seg[2] == 0)
                // ORCHIDv2 2001:20::/28
                && !(seg[0] == 0x2001 && (seg[1] & 0xfff0) == 0x0020)
                // Documentation 3fff::/20 (RFC 9637)
                && !(seg[0] == 0x3fff && (seg[1] & 0xf000) == 0)
        }
    }
}

/// Normalizes and validates a public FQDN (at least two labels, no local/internal TLDs).
pub fn canonical_public_domain(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    let trimmed = trimmed.strip_suffix('.').unwrap_or(trimmed);
    if trimmed.is_empty()
        || trimmed.len() > 253
        || trimmed.chars().any(char::is_whitespace)
        || !trimmed.is_ascii()
    {
        return None;
    }
    let normalized = trimmed.to_ascii_lowercase();
    let labels: Vec<&str> = normalized.split('.').collect();
    if labels.len() < 2 {
        return None;
    }
    for label in &labels {
        if label.is_empty()
            || label.len() > 63
            || label.starts_with('-')
            || label.ends_with('-')
            || !label
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return None;
        }
    }
    let tld = *labels.last()?;
    if tld.len() < 2 || tld.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    if matches!(
        tld,
        "localhost" | "local" | "internal" | "localdomain" | "home" | "lan" | "arpa" | "onion"
    ) {
        return None;
    }
    Some(normalized)
}

fn strip_unsafe_text(raw: &str) -> String {
    raw.chars()
        .filter(|&ch| {
            !ch.is_control()
                && !matches!(
                    ch,
                    '\u{200E}'
                        | '\u{200F}'
                        | '\u{061C}'
                        | '\u{202A}'..='\u{202E}'
                        | '\u{2066}'..='\u{2069}'
                )
        })
        .collect::<String>()
        .trim()
        .to_owned()
}

fn sanitize_note(raw: &str) -> String {
    let cleaned = strip_unsafe_text(raw);
    cleaned.chars().take(MAX_SUBSCRIPTION_NOTE_CHARS).collect()
}

fn parse_port(raw: &str) -> Option<u16> {
    if raw.is_empty() || !raw.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let port = raw.parse::<u16>().ok()?;
    (port != 0).then_some(port)
}

fn parse_target_and_port(target: &str) -> Option<(SubscriptionEndpointHost, Option<u16>)> {
    let target = target.trim();
    if target.is_empty() || target.contains("://") {
        return None;
    }
    // 1. Bracketed IPv6: [2606:4700::1]:443 or [2606:4700::1]
    if let Some(after_open) = target.strip_prefix('[') {
        let close = after_open.find(']')?;
        let ip_str = &after_open[..close];
        let rest = &after_open[close + 1..];
        let ipv6: Ipv6Addr = ip_str.parse().ok()?;
        if !is_usable_public_endpoint_ip(ipv6.into()) {
            return None;
        }
        let port = if rest.is_empty() {
            None
        } else {
            let port_str = rest.strip_prefix(':')?;
            Some(parse_port(port_str)?)
        };
        return Some((SubscriptionEndpointHost::Ipv6(ipv6), port));
    }
    // 2. Bare IPv6 (contains multiple colons, no port)
    if let Ok(ipv6) = target.parse::<Ipv6Addr>() {
        if !is_usable_public_endpoint_ip(ipv6.into()) {
            return None;
        }
        return Some((SubscriptionEndpointHost::Ipv6(ipv6), None));
    }
    // 3. IPv4 or domain, with optional single :port
    let (host_str, port) = match target.split_once(':') {
        Some((host_part, port_part)) => {
            if port_part.contains(':') {
                return None;
            }
            (host_part, Some(parse_port(port_part)?))
        }
        None => (target, None),
    };
    if let Ok(ipv4) = host_str.parse::<Ipv4Addr>() {
        if !is_usable_public_endpoint_ip(ipv4.into()) {
            return None;
        }
        return Some((SubscriptionEndpointHost::Ipv4(ipv4), port));
    }
    // Reject near-miss numeric IPv4s (e.g. 999.0.0.1 or 10.0.0.1 or 162.159.193)
    // because canonical_public_domain rejects all-digit TLDs.
    let domain = canonical_public_domain(host_str)?;
    Some((SubscriptionEndpointHost::Domain(domain), port))
}

/// Parses plain-text subscription content (`IP:Port@Note`, `[IPv6]:Port@Note`,
/// `Domain:Port@Note`, `#Note`, bare hosts, comment lines).
pub fn parse_subscription_body(body: &[u8]) -> Result<ParsedSubscription, SubscriptionError> {
    let text = std::str::from_utf8(body).map_err(|_| SubscriptionError::InvalidBody)?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut endpoints = Vec::new();
    let mut seen_ids = HashSet::new();
    let mut skipped_line_count: u32 = 0;

    for raw_line in text.lines() {
        let trimmed = raw_line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("//") {
            continue;
        }
        let (target_part, note_raw) = match trimmed.find(['@', '#']) {
            Some(pos) => (&trimmed[..pos], &trimmed[pos + 1..]),
            None => (trimmed, ""),
        };
        let Some((host, port)) = parse_target_and_port(target_part) else {
            skipped_line_count = skipped_line_count.saturating_add(1);
            continue;
        };
        let id = canonical_endpoint_id(&host, port);
        if !seen_ids.insert(id.clone()) {
            skipped_line_count = skipped_line_count.saturating_add(1);
            continue;
        }
        if endpoints.len() >= MAX_ENDPOINTS_PER_SUBSCRIPTION {
            skipped_line_count = skipped_line_count.saturating_add(1);
            continue;
        }
        endpoints.push(SubscriptionEndpoint {
            id,
            host,
            port,
            note: sanitize_note(note_raw),
            pinned: false,
        });
    }

    if endpoints.is_empty() {
        return Err(SubscriptionError::Empty);
    }

    Ok(ParsedSubscription {
        endpoints,
        skipped_line_count,
    })
}
