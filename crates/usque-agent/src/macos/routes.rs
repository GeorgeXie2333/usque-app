use super::{
    command::{self, Program, Runner},
    resources::RouteReceipt,
};
use ipnet::IpNet;
use std::{
    io,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RouteKey {
    Network,
    Host,
    ExplicitFullMask,
}
#[derive(Debug, PartialEq, Eq)]
struct Observed {
    destination: IpNet,
    interface: String,
    key: RouteKey,
}
fn parse(output: &str, ipv4: bool) -> io::Result<Observed> {
    let invalid = || io::Error::new(io::ErrorKind::InvalidData, "route observation unavailable");
    let mut fields = std::collections::HashMap::new();
    for line in output.lines() {
        if let Some((key, value)) = line.trim().split_once(':')
            && matches!(key, "destination" | "mask" | "interface")
            && fields.insert(key, value.trim()).is_some()
        {
            return Err(invalid());
        }
    }
    let destination = *fields.get("destination").ok_or_else(invalid)?;
    let interface = fields.get("interface").ok_or_else(invalid)?.to_string();
    if !super::resources::interface_name(&interface) {
        return Err(invalid());
    }
    let zero = if ipv4 {
        IpAddr::V4(Ipv4Addr::UNSPECIFIED)
    } else {
        IpAddr::V6(Ipv6Addr::UNSPECIFIED)
    };
    let destination = if destination.contains('/') {
        destination.parse::<IpNet>().map_err(|_| invalid())?
    } else {
        let ip = if destination == "default" {
            zero
        } else {
            destination.parse::<IpAddr>().map_err(|_| invalid())?
        };
        match fields.get("mask").copied() {
            Some("default") => IpNet::with_netmask(ip, zero),
            Some(mask) => IpNet::with_netmask(ip, mask.parse::<IpAddr>().map_err(|_| invalid())?),
            None if destination == "default" => IpNet::new(zero, 0),
            None => Ok(IpNet::from(ip)),
        }
        .map_err(|_| invalid())?
    };
    if destination.addr().is_ipv4() != ipv4 {
        return Err(invalid());
    }
    Ok(Observed {
        destination,
        interface,
        key: RouteKey::Network,
    })
}
// An RTM_GET best match can be a cloned or interface-scoped host route, or
// a covering prefix. It cannot establish that the unscoped /128 is gone.
// Read the numeric IPv6 table including clones and select the actual entry.
fn ipv6_host_entry(output: &str, route: &RouteReceipt) -> io::Result<Option<Observed>> {
    let invalid = || io::Error::new(io::ErrorKind::InvalidData, "invalid IPv6 route table");
    let mut lines = output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty());
    if lines.next() != Some("Routing tables") || lines.next() != Some("Internet6:") {
        return Err(invalid());
    }
    let columns: Vec<_> = lines
        .next()
        .ok_or_else(invalid)?
        .split_whitespace()
        .collect();
    if !columns.starts_with(&["Destination", "Gateway", "Flags"]) {
        return Err(invalid());
    }
    let interface_column = columns
        .iter()
        .position(|c| *c == "Netif")
        .ok_or_else(invalid)?;
    let mut found = None;
    for line in lines {
        let fields: Vec<_> = line.split_whitespace().collect();
        let dest = fields[0];
        if dest == "default" {
            continue;
        }
        let (address, prefix) = dest.split_once('/').unwrap_or((dest, "128"));
        let prefix = prefix.parse::<u8>().map_err(|_| invalid())?;
        if prefix > 128 {
            return Err(invalid());
        }
        // netstat can append %enN to link-local addresses, before /prefix.
        let address = address
            .split_once('%')
            .map_or(address, |(ip, _)| ip)
            .parse::<Ipv6Addr>()
            .map_err(|_| invalid())?;
        // Older packages used -net addr/128. Darwin route omits the mask
        // for /128 and -net clears HOST. netstat prints that missing mask
        // as /0. A real IPv6 default has a zero key, never a nonzero host.
        let legacy_key = prefix == 0 && !address.is_unspecified();
        if IpAddr::V6(address) != route.destination.addr() || (!legacy_key && prefix != 128) {
            continue;
        }
        let flags = fields.get(2).ok_or_else(invalid)?;
        if flags.len() >= 10 {
            return Err(invalid()); // Darwin netstat truncates this column to 10.
        }
        if flags.contains('I') || flags.contains('W') {
            continue; // distinct scoped key or a kernel-generated clone
        }
        let interface = *fields.get(interface_column).ok_or_else(invalid)?;
        // -interface creates a direct static route with an AF_LINK gateway.
        // Reject a foreign replacement, including one on the same interface.
        if !flags.contains('S')
            || !flags.bytes().all(|flag| b"USHLCci".contains(&flag))
            || (legacy_key && flags.contains('H'))
            || !flags.contains('U')
            || interface != route.interface
            || (fields[1] != interface && fields[1] != format!("link#{}", route.interface_index))
            || found.is_some()
        {
            return Err(io::Error::other("IPv6 host route ownership conflict"));
        }
        found = Some(Observed {
            destination: route.destination,
            interface: interface.to_owned(),
            key: if flags.contains('H') {
                RouteKey::Host
            } else if legacy_key {
                RouteKey::Network
            } else {
                RouteKey::ExplicitFullMask
            },
        });
    }
    Ok(found)
}
fn query(runner: &mut impl Runner, route: &RouteReceipt) -> io::Result<Option<Observed>> {
    if route.destination.prefix_len() == 128 {
        let output = command::checked(
            runner,
            Program::Netstat,
            &command::arguments(&["-rnaW", "-f", "inet6"]),
            None,
        )?;
        if !output.stderr.trim().is_empty() {
            return Err(io::Error::other("IPv6 route table readback unavailable"));
        }
        return ipv6_host_entry(&output.stdout, route);
    }
    let family = if route.destination.addr().is_ipv4() {
        "-inet"
    } else {
        "-inet6"
    };
    let output = runner.run(
        Program::Route,
        &command::arguments(&["-n", "get", family, "-net", &route.destination.to_string()]),
        None,
    )?;
    // Darwin route get exits successfully even when RTM_GET returns ESRCH.
    // Only the specific kernel miss with no route data proves absence.
    let missing = match output.stderr.trim() {
        "route: writing to routing socket: not in table" => output.stdout.trim().is_empty(),
        "route: message indicates error 3: No such process" => {
            // A miss in the reply (rather than the write) prints the query
            // destination before checking rtm_errno. There must be no result.
            let requested = output.stdout.trim().strip_prefix("route to: ");
            requested.and_then(|s| s.parse::<IpAddr>().ok()) == Some(route.destination.addr())
                || (requested == Some("default") && route.destination.addr().is_unspecified())
        }
        _ => false,
    };
    if missing {
        return Ok(None);
    }
    if !output.success || !output.stderr.trim().is_empty() {
        return Err(io::Error::other("route readback unavailable"));
    }
    let observed = parse(&output.stdout, route.destination.addr().is_ipv4())?;
    Ok((observed.destination == route.destination).then_some(observed))
}
fn owned(route: &RouteReceipt) -> io::Result<()> {
    let name = std::ffi::CString::new(route.interface.as_str()).map_err(io::Error::other)?;
    // SAFETY: interface name is NUL terminated and points to live storage.
    if unsafe { libc::if_nametoindex(name.as_ptr()) } != route.interface_index {
        return Err(io::Error::other("utun identity changed"));
    }
    Ok(())
}
pub fn require_absent(runner: &mut impl Runner, route: &RouteReceipt) -> io::Result<()> {
    owned(route)?;
    if query(runner, route)?.is_some() {
        return Err(io::Error::other(
            "route already exists; refusing replacement",
        ));
    }
    Ok(())
}
pub fn apply(runner: &mut impl Runner, route: &RouteReceipt, restore: bool) -> io::Result<()> {
    let observed = query(runner, route)?;
    if let Some(observed) = &observed {
        if observed.interface != route.interface {
            return Err(io::Error::other(
                "route ownership conflict; recovery incomplete",
            ));
        }
        owned(route)?;
        if !restore {
            return Ok(());
        }
    } else if restore {
        return Ok(());
    } else {
        owned(route)?;
    }
    let family = if route.destination.addr().is_ipv4() {
        "-inet"
    } else {
        "-inet6"
    };
    let key = observed.as_ref().map(|entry| entry.key).unwrap_or_else(|| {
        if route.destination.prefix_len() == 128 {
            RouteKey::Host
        } else {
            RouteKey::Network
        }
    });
    let host = key == RouteKey::Host;
    let target = if host {
        route.destination.addr().to_string()
    } else {
        route.destination.to_string()
    };
    let mut args = command::arguments(&[
        "-n",
        if restore { "delete" } else { "add" },
        family,
        if host { "-host" } else { "-net" },
        &target,
    ]);
    if key == RouteKey::ExplicitFullMask {
        // Supplying only /128 omits RTA_NETMASK. An explicit mask selects the
        // corresponding radix key without enabling RTF_HOST.
        args.extend(command::arguments(&[
            "-netmask",
            "ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff",
        ]));
    }
    args.extend(command::arguments(&["-interface", &route.interface]));
    let change = command::checked(runner, Program::Route, &args, None);
    if let Err(error) = change {
        if restore && query(runner, route)?.is_none() {
            return Ok(());
        }
        return Err(error);
    }
    let next = query(runner, route)?;
    if restore && next.is_some() || !restore && next.is_none_or(|o| o.interface != route.interface)
    {
        return Err(io::Error::other("route readback failed"));
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    // Simulated kernel table. lo0 supplies a real read-only interface index for
    // the ownership check. Every query and mutation runs against this fixture.
    struct HostTable {
        rows: String,
        after: &'static str,
        mutation_ok: bool,
        mutations: Vec<Vec<String>>,
    }
    impl Runner for HostTable {
        fn run(
            &mut self,
            program: Program,
            args: &[String],
            _: Option<&str>,
        ) -> io::Result<command::Output> {
            let (success, stdout) = if args == ["-rnaW", "-f", "inet6"] {
                assert!(matches!(program, Program::Netstat));
                (
                    true,
                    format!(
                        "Routing tables\n\nInternet6:\nDestination Gateway Flags Netif Expire\n{}",
                        self.rows
                    ),
                )
            } else {
                assert!(matches!(program, Program::Route));
                assert!(
                    matches!(args[1].as_str(), "add" | "delete"),
                    "a best-match lookup cannot identify the installed route: {args:?}"
                );
                self.mutations.push(args.to_vec());
                self.rows = self.after.to_owned();
                (self.mutation_ok, String::new())
            };
            Ok(command::Output {
                pid: 1,
                success,
                stdout,
                stderr: String::new(),
            })
        }
    }
    const SHADOW: &str =
        "default fe80::1%en0 UGc en0\n8000::/1 lo0 US lo0\nfd00::1 fe80::1%en0 UHW3I en0\n";
    fn host_table(row: &str) -> (HostTable, RouteReceipt) {
        let mut receipt = receipt("fd00::1/128");
        receipt.interface = "lo0".into();
        // SAFETY: constant NUL-terminated interface name; read-only lookup.
        receipt.interface_index = unsafe { libc::if_nametoindex(c"lo0".as_ptr()) };
        assert_ne!(receipt.interface_index, 0);
        (
            HostTable {
                rows: format!("{SHADOW}{row}"),
                after: SHADOW,
                mutation_ok: true,
                mutations: vec![],
            },
            receipt,
        )
    }
    #[test]
    fn restore_ipv6_host_uses_observed_route_form_and_confirms_absence() {
        for (row, kind, target) in [
            ("fd00::1 lo0 UHS lo0", "-host", "fd00::1"),
            // Darwin ND6 may set LLINFO even on a static P2P host route.
            ("fd00::1 lo0 UHLS lo0", "-host", "fd00::1"),
            ("fd00::1/0 lo0 USc lo0", "-net", "fd00::1/128"),
        ] {
            let (mut table, receipt) = host_table(row);
            // The kernel can remove the route between the snapshot and delete.
            table.mutation_ok = false;
            apply(&mut table, &receipt, true).unwrap();
            apply(&mut table, &receipt, true).unwrap();
            assert_eq!(
                table.mutations,
                [["-n", "delete", "-inet6", kind, target, "-interface", "lo0"]]
            );
        }
    }
    #[test]
    fn restore_explicit_ipv6_full_mask_sends_a_netmask_to_darwin() {
        let (mut table, receipt) = host_table("fd00::1/128 lo0 US lo0");
        apply(&mut table, &receipt, true).unwrap();
        assert_eq!(
            table.mutations,
            [[
                "-n",
                "delete",
                "-inet6",
                "-net",
                "fd00::1/128",
                "-netmask",
                "ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff",
                "-interface",
                "lo0"
            ]]
        );
    }
    #[test]
    fn install_ipv6_host_uses_darwin_host_flag() {
        let (mut table, receipt) = host_table("");
        table.after = "fd00::1 lo0 UHS lo0";
        require_absent(&mut table, &receipt).unwrap();
        apply(&mut table, &receipt, false).unwrap();
        assert_eq!(
            table.mutations,
            [[
                "-n",
                "add",
                "-inet6",
                "-host",
                "fd00::1",
                "-interface",
                "lo0"
            ]]
        );
    }
    #[test]
    fn restore_ipv6_host_retains_unverified_or_foreign_route() {
        let row = "fd00::1 lo0 UHS lo0";
        let (mut table, receipt) = host_table(row);
        table.after = row;
        assert!(apply(&mut table, &receipt, true).is_err());
        for row in [
            "fd00::1 en0 UHS en0",
            "fd00::1 ::1 UGHS lo0",
            "fd00::1 lo0 UH lo0",
            "fd00::1 lo0 UHSB lo0",
            "fd00::1 lo0 UHSY lo0",
            "fd00::1 lo0 UHS1 lo0",
            "fd00::1 lo0 UHSX lo0",
            "fd00::1 lo0 UGSCLWIXXY lo0",
            "fd00::1/invalid lo0 US lo0",
            "fd00::1 link#4294967295 UHS lo0",
            "fd00::1/0 en0 USc en0",
            "fd00::1/0 lo0 UHS lo0",
            "fd00::1 lo0 UHS lo0\nfd00::1/128 lo0 US lo0",
        ] {
            let (mut table, receipt) = host_table(row);
            assert!(apply(&mut table, &receipt, true).is_err());
            assert!(table.mutations.is_empty());
        }
    }
    struct Lookup {
        stdout: &'static str,
        stderr: &'static str,
        success: bool,
    }
    impl Runner for Lookup {
        fn run(
            &mut self,
            program: Program,
            args: &[String],
            _: Option<&str>,
        ) -> io::Result<command::Output> {
            assert!(matches!(program, Program::Route));
            assert_eq!(args[1], "get", "a fallback must never be deleted");
            Ok(command::Output {
                pid: 1,
                stdout: self.stdout.into(),
                stderr: self.stderr.into(),
                success: self.success,
            })
        }
    }
    fn receipt(destination: &str) -> RouteReceipt {
        RouteReceipt {
            destination: destination.parse().unwrap(),
            interface: "utun8".into(),
            interface_index: 42,
            restored: false,
        }
    }
    #[test]
    fn missing_darwin_route_can_exit_zero_during_disconnect() {
        for success in [true, false] {
            let mut lookup = Lookup {
                stdout: "",
                stderr: "route: writing to routing socket: not in table\n",
                success,
            };
            apply(&mut lookup, &receipt("8000::/1"), true).unwrap();
        }
        let mut reply = Lookup {
            stdout: "   route to: 8000::\n",
            stderr: "route: message indicates error 3: No such process\n",
            success: true,
        };
        apply(&mut reply, &receipt("8000::/1"), true).unwrap();
        reply.stdout = "destination: 8000::\nmask: 8000::\ninterface: utun8\n";
        assert!(apply(&mut reply, &receipt("8000::/1"), true).is_err());
        for stderr in [
            "route: writing to routing socket: Operation not permitted\n",
            "",
            "route: bad address: utun8\n",
        ] {
            let mut lookup = Lookup {
                stdout: "",
                stderr,
                success: true,
            };
            assert!(apply(&mut lookup, &receipt("8000::/1"), true).is_err());
        }
    }
    #[test]
    fn ipv6_default_fallback_is_a_network_and_never_an_owned_host() {
        let mut lookup = Lookup {
            stdout: "destination: ::\n       mask: default\n  interface: en1\n",
            stderr: "",
            success: true,
        };
        assert!(query(&mut lookup, &receipt("::/0")).unwrap().is_some());
        assert_ne!(
            parse(lookup.stdout, false).unwrap().destination,
            receipt("::/128").destination
        );
        apply(&mut lookup, &receipt("8000::/1"), true).unwrap();
    }
    #[test]
    fn zero_destination_uses_darwin_mask_instead_of_default_label() {
        assert_eq!(
            parse(
                "destination: default\nmask: 128.0.0.0\ninterface: utun4",
                true
            )
            .unwrap()
            .destination
            .to_string(),
            "0.0.0.0/1"
        );
        assert_eq!(
            parse("destination: default\nmask: default\ninterface: en0", true)
                .unwrap()
                .destination
                .to_string(),
            "0.0.0.0/0"
        );
    }
    #[test]
    fn distinguishes_exact_owned_route_from_fallback_or_changed_owner() {
        let value=parse("   route to: 1.2.3.4\ndestination: 0.0.0.0\n       mask: 128.0.0.0\n  interface: utun7\n      flags: <UP,DONE,STATIC>\n", true).unwrap();
        assert_eq!(
            value,
            Observed {
                destination: "0.0.0.0/1".parse().unwrap(),
                interface: "utun7".into(),
                key: RouteKey::Network,
            }
        );
        assert_ne!(
            value,
            parse(
                "destination: default\nmask: default\ninterface: en0\n",
                true
            )
            .unwrap()
        );
        assert!(
            parse(
                "destination: 0.0.0.0\nmask: 255.0.255.0\ninterface: utun7\n",
                true
            )
            .is_err()
        );
        assert!(
            parse(
                "destination: 0.0.0.0\nmask: 128.0.0.0\ninterface: utun7\ninterface: en0",
                true
            )
            .is_err()
        );
    }
}
