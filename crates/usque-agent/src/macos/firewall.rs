//! Only a session-owned anchor and its enable reference are mutated. Foreign
//! states/anchors are never flushed; an unprovable configuration is refused.
use super::{
    command::{self, Program, Runner},
    network::Network,
};
use std::{io, net::SocketAddr};
use uuid::Uuid;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Permit {
    pub local: SocketAddr,
    pub remote: SocketAddr,
    pub protocol: u8,
    pub interface: String,
}
pub fn anchor(session: Uuid) -> String {
    format!("com.apple/zzzz.usque.{}", session.simple())
}
fn incompatible() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "PF configuration is incompatible with verified protection",
    )
}
pub fn rules(network: &Network, tun: Option<&str>, permits: &[Permit]) -> io::Result<String> {
    network.validate()?;
    if permits.len() > 128
        || tun.is_some_and(|n| {
            !super::resources::interface_name(n)
                || !n
                    .strip_prefix("utun")
                    .is_some_and(|n| n.parse::<u32>().is_ok())
        })
    {
        return Err(incompatible());
    }
    let mut result = "pass quick on lo0 all no state\n".to_owned();
    if let Some(tun) = tun {
        result.push_str(&format!("pass quick on {tun} all no state\n"));
    }
    // Link bootstrap must work on the next physical interface before it has
    // an address/default route and can appear in a new network generation.
    // These are fixed infrastructure protocols, never general egress permits.
    result.push_str(
        "pass out quick inet proto udp from any port = 68 to any port = 67 user = 0 no state\n",
    );
    result.push_str(
        "pass in quick inet proto udp from any port = 67 to any port = 68 user = 0 no state\n",
    );
    result.push_str("pass out quick inet6 proto udp from fe80::/10 port = 546 to { ff02::1:2, fe80::/10 } port = 547 user = 0 no state\n");
    result.push_str("pass in quick inet6 proto udp from fe80::/10 port = 547 to { fe80::/10, ff02::/16 } port = 546 user = 0 no state\n");
    result.push_str("pass out quick inet6 proto ipv6-icmp from { ::, fe80::/10 } to { ff02::/16, fe80::/10 } icmp6-type { routersol, neighbrsol, neighbradv } no state\n");
    result.push_str("pass in quick inet6 proto ipv6-icmp from { ::, fe80::/10 } to { ff02::/16, fe80::/10 } icmp6-type { routeradv, neighbrsol, neighbradv } no state\n");
    for path in &network.paths {
        let interface = &path.name;
        let local = path
            .addresses
            .iter()
            .map(|a| a.addr().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        if path.address.is_ipv4() {
            // DHCP is infrastructure, not an arbitrary UDP bypass. Its ports,
            // source family, interface and root-created socket are constrained.
            result.push_str(&format!("pass out quick on {interface} inet proto udp from {{ 0.0.0.0, {local} }} port = 68 to {{ 255.255.255.255, {} }} port = 67 user = 0 no state\n",path.gateway));
            result.push_str(&format!("pass in quick on {interface} inet proto udp from any port = 67 to {{ 255.255.255.255, {local} }} port = 68 user = 0 no state\n"));
            result.push_str(&format!("pass in quick on {interface} inet proto icmp from any to {{ {local} }} icmp-type {{ unreach, timex, paramprob }} no state\n"));
        } else {
            let prefixes = path
                .addresses
                .iter()
                .map(|a| a.trunc().to_string())
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>()
                .join(", ");
            result.push_str(&format!("pass out quick on {interface} inet6 proto ipv6-icmp from {{ ::, fe80::/10 }} to {{ ff02::2, {} }} icmp6-type routersol no state\n",path.gateway));
            result.push_str(&format!("pass in quick on {interface} inet6 proto ipv6-icmp from fe80::/10 to {{ ff02::1, {local} }} icmp6-type routeradv no state\n"));
            // NS/NA may use a GUA/ULA source and a unicast neighbour target
            // (RFC 4861 7.2). Keep them interface/prefix/type constrained.
            result.push_str(&format!("pass out quick on {interface} inet6 proto ipv6-icmp from {{ ::, {local} }} to {{ ff02::1, ff02::1:ff00:0/104, {prefixes} }} icmp6-type {{ neighbrsol, neighbradv }} no state\n"));
            result.push_str(&format!("pass in quick on {interface} inet6 proto ipv6-icmp from {{ ::, {prefixes} }} to {{ ff02::1, ff02::1:ff00:0/104, {local} }} icmp6-type {{ neighbrsol, neighbradv }} no state\n"));
            result.push_str(&format!("pass in quick on {interface} inet6 proto ipv6-icmp from any to {{ {local} }} icmp6-type {{ unreach, toobig, timex, paramprob }} no state\n"));
            result.push_str(&format!("pass out quick on {interface} inet6 proto udp from fe80::/10 port = 546 to {{ ff02::1:2, fe80::/10 }} port = 547 user = 0 no state\n"));
            result.push_str(&format!("pass in quick on {interface} inet6 proto udp from fe80::/10 port = 547 to {{ fe80::/10, {local} }} port = 546 user = 0 no state\n"));
        }
    }
    for permit in permits {
        let path = network.path(permit.remote)?;
        if permit.interface != path.name
            || permit.local.ip() != path.source(permit.remote)?
            || permit.local.port() == 0
            || permit.remote.port() == 0
            || !super::resources::usable_address(permit.remote.ip())
        {
            return Err(incompatible());
        }
        let family = if permit.remote.is_ipv4() {
            "inet"
        } else {
            "inet6"
        };
        let protocol = match permit.protocol {
            6 => "tcp",
            17 => "udp",
            _ => return Err(incompatible()),
        };
        let flags = if permit.protocol == 6 {
            " flags any"
        } else {
            ""
        };
        for (direction, source, destination) in [
            ("out", permit.local, permit.remote),
            ("in", permit.remote, permit.local),
        ] {
            result.push_str(&format!("pass {direction} quick on {} {family} proto {protocol} from {} port = {} to {} port = {} user = 0{flags} no state\n",permit.interface,source.ip(),source.port(),destination.ip(),destination.port()));
        }
    }
    result.push_str("block drop quick inet all\nblock drop quick inet6 all\n");
    Ok(result)
}
fn validate_environment(
    main: &str,
    foreign_rules: &[String],
    states: &str,
    interfaces: &str,
) -> io::Result<()> {
    let filter = "anchor \"com.apple/*\" all";
    let supported = main.trim() == filter
        || main.trim() == format!("scrub-anchor \"com.apple/*\" all fragment reassemble\n{filter}");
    if !supported
        || foreign_rules.iter().any(|s| !s.trim().is_empty())
        || !states.trim().is_empty()
        || interfaces.contains("skip")
    {
        return Err(incompatible());
    }
    Ok(())
}
fn pf(runner: &mut impl Runner, args: &[&str], input: Option<&str>) -> io::Result<command::Output> {
    command::checked(runner, Program::Pfctl, &command::arguments(args), input)
}
/// A conservative supported subset: the standard Apple hook, no competing
/// filtering/NAT or skipped interfaces, and no pre-existing PF states. Existing
/// TCP connections without PF state are filtered when the guard is activated.
pub fn preflight(runner: &mut impl Runner, owned_anchor: Option<&str>) -> io::Result<()> {
    let main = pf(runner, &["-sr"], None)?.stdout;
    let interfaces = pf(runner, &["-v", "-s", "Interfaces"], None)?.stdout;
    let states = pf(runner, &["-ss"], None)?.stdout;
    if !pf(runner, &["-sn"], None)?.stdout.lines().all(|line| {
        line.trim().is_empty()
            || line.trim() == "nat-anchor \"com.apple/*\" all"
            || line.trim() == "rdr-anchor \"com.apple/*\" all"
    }) {
        return Err(incompatible());
    }
    let children = pf(runner, &["-a", "com.apple", "-s", "Anchors"], None)?.stdout;
    let mut foreign = Vec::new();
    for child in children.lines().map(str::trim).filter(|s| !s.is_empty()) {
        // pfctl prints either the direct name or its full path depending on
        // system version. Never send a parsed arbitrary path back to pfctl.
        let name = child.strip_prefix("com.apple/").unwrap_or(child);
        if name.len() > 128
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        {
            return Err(incompatible());
        }
        let path = format!("com.apple/{name}");
        if Some(path.as_str()) != owned_anchor {
            foreign.push(pf(runner, &["-a", &path, "-sr"], None)?.stdout);
            foreign.push(pf(runner, &["-a", &path, "-sn"], None)?.stdout);
            foreign.push(pf(runner, &["-a", &path, "-s", "Anchors"], None)?.stdout);
        }
    }
    validate_environment(&main, &foreign, &states, &interfaces)
}
/// Compile through the installed PF parser before persisting the intended
/// canonical rules. This is parse-only and never loads the ruleset.
pub fn canonical(runner: &mut impl Runner, anchor: &str, rules: &str) -> io::Result<String> {
    let output = pf(
        runner,
        &["-n", "-v", "-o", "none", "-R", "-a", anchor, "-f", "-"],
        Some(rules),
    )?;
    if output.stdout.trim().is_empty() {
        return Err(incompatible());
    }
    Ok(output.stdout.trim().to_owned())
}
pub fn read(runner: &mut impl Runner, anchor: &str) -> io::Result<String> {
    Ok(pf(runner, &["-a", anchor, "-sr"], None)?
        .stdout
        .trim()
        .to_owned())
}
pub fn verify_active(runner: &mut impl Runner, anchor: &str, intended: &str) -> io::Result<()> {
    preflight(runner, Some(anchor))?;
    let info = pf(runner, &["-s", "info"], None)?.stdout;
    if !info.lines().any(|line| line.starts_with("Status: Enabled"))
        || read(runner, anchor)? != intended.trim()
    {
        return Err(incompatible());
    }
    Ok(())
}
pub fn replace(runner: &mut impl Runner, anchor: &str, rules: &str) -> io::Result<()> {
    pf(
        runner,
        &["-o", "none", "-R", "-a", anchor, "-f", "-"],
        Some(rules),
    )?;
    if read(runner, anchor)? != rules.trim() {
        return Err(io::Error::other(
            "PF readback mismatch; protection state uncertain",
        ));
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn network() -> Network {
        Network {
            paths: vec![super::super::network::PhysicalPath {
                name: "en0".into(),
                index: 4,
                address: "192.0.2.2".parse().unwrap(),
                addresses: vec!["192.0.2.2/24".parse().unwrap()],
                gateway: "192.0.2.1".parse().unwrap(),
                service: Uuid::new_v4(),
            }],
            dns: vec!["192.0.2.1:53".parse().unwrap()],
        }
    }
    #[test]
    fn standard_darwin_scrub_hook_is_accepted_but_extra_filters_are_refused() {
        let rules =
            "scrub-anchor \"com.apple/*\" all fragment reassemble\nanchor \"com.apple/*\" all\n";
        validate_environment(rules, &[], "", "en0").unwrap();
        assert!(validate_environment(&(rules.to_owned() + "pass all\n"), &[], "", "en0").is_err());
    }
    #[test]
    fn permits_are_exact_root_origin_stateless_tuples_and_both_families_are_blocked() {
        let rules = rules(
            &network(),
            Some("utun4"),
            &[Permit {
                local: "192.0.2.2:50000".parse().unwrap(),
                remote: "198.51.100.7:443".parse().unwrap(),
                protocol: 17,
                interface: "en0".into(),
            }],
        )
        .unwrap();
        assert!(
            rules.contains(
                "from 192.0.2.2 port = 50000 to 198.51.100.7 port = 443 user = 0 no state"
            )
        );
        assert!(
            rules.contains(
                "from 198.51.100.7 port = 443 to 192.0.2.2 port = 50000 user = 0 no state"
            )
        );
        assert!(rules.contains("block drop quick inet all"));
        assert!(rules.contains("block drop quick inet6 all"));
        assert!(!rules.contains("keep state"));
        assert!(super::rules(&network(), Some("utun4\npass all"), &[]).is_err());
    }
    #[test]
    fn refuses_foreign_states_skip_or_early_pass_without_mutating_them() {
        let main = "anchor \"com.apple/*\" all\n";
        validate_environment(main, &[], "", "en0\nlo0\n").unwrap();
        for (main, rules, states, interfaces) in [
            ("pass quick all", vec![], "", "en0"),
            (main, vec!["pass all".into()], "", "en0"),
            (main, vec![], "all tcp 1:2 -> 3:4", "en0"),
            (main, vec![], "", "en0 (skip)"),
        ] {
            assert!(validate_environment(main, &rules, states, interfaces).is_err());
        }
    }
    #[test]
    fn generated_dual_stack_rules_pass_system_parser_without_loading_pf() {
        let mut n = network();
        n.paths.push(super::super::network::PhysicalPath {
            name: "en0".into(),
            index: 4,
            address: "2001:db8::2".parse().unwrap(),
            addresses: vec![
                "2001:db8::2/64".parse().unwrap(),
                "fd00::2/64".parse().unwrap(),
                "fe80::2/64".parse().unwrap(),
            ],
            gateway: "fe80::1".parse().unwrap(),
            service: n.paths[0].service,
        });
        let permits = [
            Permit {
                local: "192.0.2.2:50000".parse().unwrap(),
                remote: "198.51.100.7:443".parse().unwrap(),
                protocol: 6,
                interface: "en0".into(),
            },
            Permit {
                local: "[2001:db8::2]:50001".parse().unwrap(),
                remote: "[2001:db8:1::1]:443".parse().unwrap(),
                protocol: 17,
                interface: "en0".into(),
            },
        ];
        let input = rules(&n, Some("utun4"), &permits).unwrap();
        for direction in ["pass out quick on en0", "pass in quick on en0"] {
            assert!(input.lines().any(|line| line.starts_with(direction)
                && line.contains("2001:db8::/64")
                && line.contains("fd00::/64")
                && line.contains("ff02::1:ff00:0/104")
                && line.contains("neighbrsol, neighbradv")));
        }
        let parsed =
            canonical(&mut command::NativeRunner, &anchor(Uuid::new_v4()), &input).unwrap();
        assert!(parsed.contains("block drop quick inet6 all"));
    }
}
