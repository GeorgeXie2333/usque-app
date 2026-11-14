use super::{
    command::{self, Program, Runner},
    resources::RouteReceipt,
};
use ipnet::IpNet;
use std::io;
#[derive(Debug, PartialEq, Eq)]
struct Observed {
    destination: IpNet,
    interface: String,
}
fn parse(output: &str) -> io::Result<Observed> {
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
    let destination = if destination == "default" {
        let mask = fields.get("mask").copied().unwrap_or("default");
        let rewritten = output
            .replace("destination: default", "destination: 0.0.0.0")
            .replace("mask: default", "mask: 0.0.0.0");
        if mask == "default" && !fields.contains_key("mask") {
            return parse(&(rewritten + "\nmask: 0.0.0.0\n"));
        }
        return parse(&rewritten);
    } else if destination.contains('/') {
        destination.parse::<IpNet>().map_err(|_| invalid())?
    } else {
        let ip = destination
            .parse::<std::net::IpAddr>()
            .map_err(|_| invalid())?;
        let mask = fields
            .get("mask")
            .filter(|s| **s != "default")
            .map(|s| s.parse::<std::net::IpAddr>().map_err(|_| invalid()))
            .transpose()?;
        let prefix = match (ip, mask) {
            (std::net::IpAddr::V4(_), Some(std::net::IpAddr::V4(mask))) => {
                let mask = u32::from(mask);
                let n = mask.leading_ones();
                if mask != u32::MAX.checked_shl(32 - n).unwrap_or(0) {
                    return Err(invalid());
                }
                n as u8
            }
            (std::net::IpAddr::V6(_), Some(std::net::IpAddr::V6(mask))) => {
                let mask = u128::from(mask);
                let n = mask.leading_ones();
                if mask != u128::MAX.checked_shl(128 - n).unwrap_or(0) {
                    return Err(invalid());
                }
                n as u8
            }
            (std::net::IpAddr::V4(_), None) => 32,
            (std::net::IpAddr::V6(_), None) => 128,
            _ => return Err(invalid()),
        };
        IpNet::new(ip, prefix).map_err(|_| invalid())?
    };
    Ok(Observed {
        destination,
        interface,
    })
}
fn query(runner: &mut impl Runner, route: &RouteReceipt) -> io::Result<Option<Observed>> {
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
    if !output.success {
        if output.stderr.contains("not in table") || output.stderr.contains("not found") {
            return Ok(None);
        }
        return Err(io::Error::other("route readback unavailable"));
    }
    let observed = parse(&output.stdout)?;
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
    command::checked(
        runner,
        Program::Route,
        &command::arguments(&[
            "-n",
            if restore { "delete" } else { "add" },
            family,
            "-net",
            &route.destination.to_string(),
            "-interface",
            &route.interface,
        ]),
        None,
    )?;
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
    #[test]
    fn zero_destination_uses_darwin_mask_instead_of_default_label() {
        assert_eq!(
            parse("destination: default\nmask: 128.0.0.0\ninterface: utun4")
                .unwrap()
                .destination
                .to_string(),
            "0.0.0.0/1"
        );
        assert_eq!(
            parse("destination: default\nmask: default\ninterface: en0")
                .unwrap()
                .destination
                .to_string(),
            "0.0.0.0/0"
        );
    }
    #[test]
    fn distinguishes_exact_owned_route_from_fallback_or_changed_owner() {
        let value=parse("   route to: 1.2.3.4\ndestination: 0.0.0.0\n       mask: 128.0.0.0\n  interface: utun7\n      flags: <UP,DONE,STATIC>\n").unwrap();
        assert_eq!(
            value,
            Observed {
                destination: "0.0.0.0/1".parse().unwrap(),
                interface: "utun7".into()
            }
        );
        assert_ne!(
            value,
            parse("destination: default\nmask: default\ninterface: en0\n").unwrap()
        );
        assert!(parse("destination: 0.0.0.0\nmask: 255.0.255.0\ninterface: utun7\n").is_err());
        assert!(
            parse("destination: 0.0.0.0\nmask: 128.0.0.0\ninterface: utun7\ninterface: en0")
                .is_err()
        );
    }
}
