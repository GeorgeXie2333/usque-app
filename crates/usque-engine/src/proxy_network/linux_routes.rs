//! Private route fingerprints for the unprivileged proxy observer.
use std::hash::{Hash, Hasher};

#[derive(Hash)]
enum Ipv6Routes<'a> {
    Unavailable,
    Parsed(Vec<[&'a str; 8]>),
    Unrecognized(&'a [u8]),
}

pub(super) fn hash_ipv6_routes(contents: Option<&[u8]>, state: &mut impl Hasher) {
    let snapshot = match contents {
        None => Ipv6Routes::Unavailable,
        Some(bytes) => match parse_ipv6_routes(bytes) {
            Some(routes) => Ipv6Routes::Parsed(routes),
            // Preserve changes in unknown schemas or malformed reads instead
            // of silently treating them as an empty or unchanged route table.
            None => Ipv6Routes::Unrecognized(bytes),
        },
    };
    snapshot.hash(state);
}

fn parse_ipv6_routes(contents: &[u8]) -> Option<Vec<[&str; 8]>> {
    let text = std::str::from_utf8(contents).ok()?;
    let mut routes = Vec::new();
    for line in text.lines().filter(|line| !line.trim_ascii().is_empty()) {
        // /proc/net/ipv6_route: destination/prefix, source/prefix, gateway,
        // metric, reference count, use count, flags, interface. The counters
        // change with ordinary socket activity, without any routing change.
        let fields: [&str; 10] = line
            .split_ascii_whitespace()
            .collect::<Vec<_>>()
            .try_into()
            .ok()?;
        if [0, 2, 4].iter().any(|&i| !hex(fields[i], 32))
            || [1, 3].iter().any(|&i| {
                !hex(fields[i], 2)
                    || !u8::from_str_radix(fields[i], 16).is_ok_and(|prefix| prefix <= 128)
            })
            || (5..=8).any(|i| !hex(fields[i], 8))
        {
            return None;
        }
        routes.push([
            fields[0], fields[1], fields[2], fields[3], fields[4], fields[5], fields[8], fields[9],
        ]);
    }
    // Enumeration order is not a route change. Keep duplicate rows so route
    // additions/removals still change the fingerprint, even for equal rows.
    routes.sort_unstable();
    Some(routes)
}

fn hex(value: &str, width: usize) -> bool {
    value.len() == width && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}
