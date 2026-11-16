//! Destructive only on an explicitly selected, disposable GitHub-hosted Mac.
//! The preceding Engine test leaves the installed daemon stopped and its
//! root-owned journals clean, so production administrator recovery also works.
use super::*;
use crate::macos::{
    journal::{self, JournalStore},
    socket::EndpointConnectFailure,
    transaction::{Coordinator, Owner, Phase},
};
use core_foundation::{array::CFArray, base::TCFType, string::CFString};
use std::{
    ffi::OsString,
    net::{IpAddr, TcpListener, TcpStream},
    os::fd::AsRawFd,
    process::Command,
    time::{Duration, Instant},
};
use usque_ipc::agent_v1::{MacosSocketPurpose, TunnelPlan};
use uuid::Uuid;

const DNS: [&str; 2] = ["198.18.0.1", "fd00::1"];
const PHYSICAL_HTTPS: &str = "1.1.1.1:443";
// Both IPv4 capture prefixes, both IPv6 capture prefixes, and the explicit
// tunnel DNS destinations (including the incident's /128 host route).
const ROUTE_PROBES: [&str; 6] = [
    "64.0.0.19",
    "203.0.113.29",
    "2001:db8::29",
    "fd29::29",
    DNS[0],
    DNS[1],
];

fn check(condition: bool, message: &str) -> io::Result<()> {
    if !condition {
        return Err(io::Error::other(message.to_owned()));
    }
    Ok(())
}

fn checked_command(program: &str, args: &[&str]) -> io::Result<String> {
    let out = Command::new(program)
        .args(args)
        .env("LC_ALL", "C")
        .output()?;
    if !out.status.success() {
        return Err(io::Error::other(format!(
            "{program} {args:?}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    String::from_utf8(out.stdout).map_err(io::Error::other)
}

struct Observer {
    program: OsString,
    failures: Vec<String>,
}
impl Observer {
    fn probe(&mut self, phase: &str) {
        eprintln!("external probe: {phase}");
        let failure = match Command::new(&self.program).args(["probe", phase]).status() {
            Ok(status) if status.success() => return,
            Ok(status) => format!("{phase}: {status}"),
            Err(error) => format!("{phase}: {error}"),
        };
        // On this disposable Mac, finish the bounded incident and explicitly
        // verify network recovery even when independent evidence is missing.
        eprintln!("external observation failed: {failure}");
        self.failures.push(failure);
    }

    fn finish(self) -> io::Result<()> {
        if self.failures.is_empty() {
            return Ok(());
        }
        Err(io::Error::other(format!(
            "external observations failed: {}",
            self.failures.join(", ")
        )))
    }
}

/// Read the OS defaults independently of the Agent's journal and receipt flags.
fn effective_dns() -> io::Result<Vec<String>> {
    let text = checked_command("/usr/sbin/scutil", &["--dns"])?;
    let resolver = text
        .split("resolver #1\n")
        .nth(1)
        .ok_or_else(|| io::Error::other("no published default DNS resolver"))?;
    let servers = resolver
        .lines()
        .take_while(|s| !s.trim().is_empty())
        .map(str::trim)
        .filter(|s| s.starts_with("nameserver["))
        .filter_map(|s| s.split_once(" : ").map(|(_, v)| v.to_owned()))
        .collect::<Vec<_>>();
    check(
        !servers.is_empty(),
        "default DNS resolver returned no servers",
    )?;
    Ok(servers)
}

fn await_dns(expected: &[String]) -> io::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        let actual = effective_dns();
        if actual.as_deref().ok() == Some(expected) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(io::Error::other(format!(
                "expected default DNS {expected:?}, observed {actual:?}"
            )));
        }
        std::thread::sleep(Duration::from_millis(150));
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Route {
    interface: String,
    gateway: Option<String>,
}
fn lookup(ip: &str) -> io::Result<Option<Route>> {
    let family = if ip.contains(':') { "-inet6" } else { "-inet" };
    let out = Command::new("/sbin/route")
        .args(["-n", "get", family, ip])
        .env("LC_ALL", "C")
        .output()?;
    let text = String::from_utf8(out.stdout).map_err(io::Error::other)?;
    let stderr = String::from_utf8_lossy(&out.stderr);
    if !text.contains("interface:")
        && matches!(
            stderr.trim(),
            "route: writing to routing socket: not in table"
                | "route: message indicates error 3: No such process"
        )
    {
        return Ok(None);
    }
    check(
        out.status.success() && stderr.trim().is_empty(),
        &format!("route get {ip}: {stderr}"),
    )?;
    let field = |key| {
        text.lines()
            .find_map(|s| s.trim().strip_prefix(key).map(|v| v.trim().to_owned()))
    };
    let interface = field("interface:")
        .ok_or_else(|| io::Error::other(format!("invalid route to {ip}: {text}")))?;
    Ok(Some(Route {
        interface,
        gateway: field("gateway:"),
    }))
}

fn route_probes() -> io::Result<Vec<Option<Route>>> {
    ROUTE_PROBES.into_iter().map(lookup).collect()
}

fn require_endpoint_failure(
    backend: &mut NativeBackend,
    fence: Fence,
    addr: SocketAddr,
) -> io::Result<()> {
    let result = backend.create_socket(fence, addr, 6, MacosSocketPurpose::Masque);
    match result {
        Err(error)
            if error
                .get_ref()
                .and_then(|e| e.downcast_ref::<EndpointConnectFailure>())
                .is_some_and(|e| e.0.raw_os_error() == Some(libc::ECONNREFUSED)) =>
        {
            eprintln!("bound endpoint connect failed before FD handoff: {error}");
            Ok(())
        }
        Err(error) => Err(io::Error::other(format!(
            "expected the reserved endpoint to refuse the connection: {error}"
        ))),
        Ok((lease, fd, _)) => {
            // Nonblocking connect may deliver the refusal via SO_ERROR.
            let socket = TcpStream::from(fd);
            let mut pollfd = libc::pollfd {
                fd: socket.as_raw_fd(),
                events: libc::POLLOUT,
                revents: 0,
            };
            // SAFETY: pollfd refers to one live socket; poll only writes revents.
            let ready = unsafe { libc::poll(&mut pollfd, 1, 4000) };
            let error = socket.take_error()?;
            drop(socket);
            backend.release_socket(fence, lease)?;
            check(
                ready > 0
                    && error.as_ref().and_then(io::Error::raw_os_error) == Some(libc::ECONNREFUSED),
                &format!("expected bound endpoint refusal, poll={ready}, SO_ERROR={error:?}"),
            )?;
            eprintln!("bound endpoint connect failed after FD handoff: {error:?}");
            Ok(())
        }
    }
}

fn fixture_plan(endpoint: SocketAddr) -> TunnelPlan {
    TunnelPlan {
        profile_id: Uuid::new_v4().to_string(),
        endpoint: endpoint.to_string(),
        control_api_candidates: vec![PHYSICAL_HTTPS.into()],
        assigned_ipv4: "198.19.0.2/32".into(),
        assigned_ipv6: "fd29::2/128".into(),
        dns_servers: DNS.map(str::to_owned).to_vec(),
        mtu: 1280,
        split_dns: true,
        kill_switch: true,
        ..Default::default()
    }
}

fn check_physical_https(backend: &mut NativeBackend, fence: Fence) -> io::Result<()> {
    let remote: SocketAddr = PHYSICAL_HTTPS.parse().unwrap();
    let (lease, fd, _) = backend.create_socket(fence, remote, 6, MacosSocketPurpose::Control)?;
    let socket = TcpStream::from(fd);
    let result = (|| {
        let mut poll = libc::pollfd {
            fd: socket.as_raw_fd(),
            events: libc::POLLOUT,
            revents: 0,
        };
        // SAFETY: poll borrows one live socket and writes its returned events.
        let ready = unsafe { libc::poll(&mut poll, 1, 5000) };
        if ready < 0 {
            return Err(io::Error::last_os_error());
        }
        if let Some(error) = socket.take_error()? {
            return Err(error);
        }
        check(
            ready > 0 && socket.peer_addr()? == remote,
            "physical HTTPS handshake timed out",
        )
    })();
    drop(socket);
    backend.release_socket(fence, lease)?;
    result
}

#[test]
fn fixture_plan_meets_agent_prepare_and_configure_contracts() {
    let wire = fixture_plan("192.0.2.193:443".parse().unwrap());
    ValidatedTunnelPlan::from_macos_prepare(TunnelPlan {
        defer_network_configuration: true,
        ..wire.clone()
    })
    .unwrap();
    ValidatedTunnelPlan::try_from(wire).unwrap();
}

#[test]
#[ignore = "mutates DNS, routing and PF; only on a disposable hosted macOS runner"]
fn configured_tun_disconnect_restores_network() -> io::Result<()> {
    // SAFETY: geteuid only reads the process identity.
    check(unsafe { libc::geteuid() } == 0, "requires root")?;
    for (name, value) in [
        ("USQUE_DISPOSABLE_MAC", "1"),
        ("GITHUB_ACTIONS", "true"),
        ("RUNNER_ENVIRONMENT", "github-hosted"),
    ] {
        check(
            std::env::var(name).as_deref() == Ok(value),
            &format!("missing {name}"),
        )?;
    }
    let mut observer = Observer {
        program: std::env::var_os("USQUE_TUN_OBSERVER")
            .ok_or_else(|| io::Error::other("missing independent observer"))?,
        failures: Vec::new(),
    };
    let _single = journal::service_lock()?;
    let mut store = JournalStore::open()?;
    check(
        store.load()?.is_none(),
        "existing transaction requires recovery",
    )?;
    let mut backend = NativeBackend::open()?;
    check(
        backend.receipts().is_none(),
        "existing network resources require recovery",
    )?;
    let physical = backend.preflight()?;
    let path = physical
        .paths
        .iter()
        .find(|p| p.address.is_ipv4())
        .ok_or_else(|| io::Error::other("missing physical IPv4 uplink"))?;
    // Darwin silently drops SYNs to a bound, non-listening TCP socket. Close
    // the listener to observe a real refusal, then reserve its exact port up
    // to the protected dial so another process cannot occupy it in between.
    let endpoint = TcpListener::bind((path.address, 0))?.local_addr()?;
    let baseline_dial = TcpStream::connect_timeout(&endpoint, Duration::from_secs(2));
    check(
        baseline_dial.as_ref().err().and_then(|e| e.raw_os_error()) == Some(libc::ECONNREFUSED),
        &format!("baseline dial did not get ECONNREFUSED: {baseline_dial:?}"),
    )?;
    let reserved = TcpListener::bind(endpoint)?;
    let dns_servers = DNS.map(|v| v.parse::<IpAddr>().unwrap());
    let original_dns = dns::snapshot(&dns_servers)?;
    let original_effective = effective_dns()?;
    let original_routes = route_probes()?;
    check(original_routes[0].is_some(), "no baseline IPv4 route")?;
    check(original_effective != DNS, "test DNS already in use")?;
    observer.probe("before");
    let wire = fixture_plan(endpoint);
    let staging = TunnelPlan {
        defer_network_configuration: true,
        ..wire.clone()
    };
    backend
        .set_plan(ValidatedTunnelPlan::from_macos_prepare(staging).map_err(io::Error::other)?)?;
    let mut coordinator = Coordinator::new(store, backend, None)?;
    let uid = std::env::var("SUDO_UID")
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|uid| *uid > 0)
        .ok_or_else(|| io::Error::other("missing ordinary invoking user"))?;
    let owner = Owner {
        uid,
        pid: std::process::id(),
        pid_version: 1,
    };
    let fence = Fence {
        session: Uuid::new_v4(),
        configuration: 1,
        network: 1,
    };

    let run = (|| {
        eprintln!("preparing guard");
        coordinator.prepare(owner, fence, true)?;
        check_physical_https(coordinator.backend_mut(), fence)?;
        eprintln!("Authorized physical TCP connected before utun routes.");
        coordinator
            .backend_mut()
            .finalize_plan(wire.try_into().map_err(io::Error::other)?)?;
        eprintln!("configuring real utun, DNS and routes");
        coordinator.configure(owner, fence)?;
        let (fd, interface, _) = coordinator.backend().descriptor(fence)?;
        coordinator.backend_mut().unchanged_network(fence)?;
        coordinator.commit(owner, fence)?;
        coordinator.backend_mut().set_active();
        // configd's IPMonitor publishes only DNS families routable on the
        // primary service. A v4-only uplink can still route IPv6 via utun.
        let published_dns: Vec<_> = dns_servers
            .iter()
            .filter(|ip| {
                physical
                    .paths
                    .iter()
                    .any(|p| p.service == path.service && p.address.is_ipv4() == ip.is_ipv4())
            })
            .map(ToString::to_string)
            .collect();
        await_dns(&published_dns)?;
        let expected = CFArray::from_CFTypes(&DNS.map(CFString::new)).as_CFType();
        let active_dns = dns::snapshot(&dns_servers)?;
        check(
            active_dns.len() == original_dns.len(),
            "DNS service set changed",
        )?;
        for saved in &original_dns {
            let actual = active_dns
                .iter()
                .find(|r| r.service == saved.service)
                .and_then(|r| r.before.as_deref())
                .ok_or_else(|| io::Error::other("missing configured DNS service"))?;
            let dictionary = dns::decode(actual)?;
            check(
                dictionary
                    .find(CFString::new("ServerAddresses"))
                    .is_some_and(|servers| *servers == expected),
                "service preferences must contain both tunnel DNS servers",
            )?;
        }
        for (ip, route) in ROUTE_PROBES.iter().zip(route_probes()?) {
            check(
                route.as_ref().is_some_and(|r| r.interface == interface),
                &format!("active route to {ip} bypassed {interface}: {route:?}"),
            )?;
        }
        eprintln!(
            "verified active default DNS {published_dns:?}, both service DNS addresses and both route families on {interface}"
        );
        observer.probe("active");
        if let Err(error) = check_physical_https(coordinator.backend_mut(), fence) {
            // Diagnose a broken data path on the disposable host. The session's
            // PF remains active. Even if the control succeeds, fail acceptance
            // and let ordinary recovery compare and undo all recorded routes.
            let backend = coordinator.backend_mut();
            let receipts = backend.receipts().unwrap().routes.clone();
            for ip in [
                PHYSICAL_HTTPS.parse::<SocketAddr>().unwrap().ip(),
                path.gateway,
            ] {
                for scope in [Vec::new(), vec!["-ifscope", path.name.as_str()]] {
                    let out = Command::new("/sbin/route")
                        .args(["-n", "get", "-inet"])
                        .args(&scope)
                        .arg(ip.to_string())
                        .env("LC_ALL", "C")
                        .output()?;
                    let text = String::from_utf8_lossy(&out.stdout);
                    eprintln!(
                        "Route for {} (scoped={}): physical={}, utun={}, has gateway={}",
                        if ip == path.gateway {
                            "gateway"
                        } else {
                            "public peer"
                        },
                        !scope.is_empty(),
                        text.lines()
                            .any(|s| s.trim().strip_prefix("interface: ") == Some(&path.name)),
                        text.lines()
                            .any(|s| s.trim().starts_with("interface: utun")),
                        text.lines().any(|s| s.trim().starts_with("gateway:"))
                    );
                }
            }
            for route in receipts
                .iter()
                .filter(|r| r.destination.prefix_len() > 1 && r.destination.contains(&path.gateway))
            {
                routes::apply(&mut backend.runner, route, true)?;
            }
            let router_trial = check_physical_https(backend, fence);
            eprintln!(
                "Physical TCP after removing only the LAN capture over the router: {router_trial:?}"
            );
            if router_trial.is_err() {
                for route in receipts
                    .iter()
                    .filter(|r| r.destination.addr().is_ipv4() && r.destination.prefix_len() == 1)
                {
                    routes::apply(&mut backend.runner, route, true)?;
                }
                eprintln!(
                    "Physical TCP after also removing IPv4 split defaults: {:?}",
                    check_physical_https(backend, fence)
                );
            }
            return Err(error);
        }
        eprintln!("Authorized physical TCP connected with utun routes active.");
        drop(reserved);
        require_endpoint_failure(coordinator.backend_mut(), fence, endpoint)?;
        drop(fd);
        coordinator.owner_lost(owner, fence)?;
        check(
            coordinator
                .state()
                .is_some_and(|t| t.phase == Phase::Retained),
            "lost owner did not retain protection",
        )?;
        coordinator.backend_mut().verify_protection()?;
        observer.probe("retained");
        coordinator.disconnect_retained(owner, fence)?;
        check(
            coordinator.state().is_none() && coordinator.backend().receipts().is_none(),
            "disconnect left recovery pending",
        )?;
        let interfaces = checked_command("/sbin/ifconfig", &["-l"])?;
        check(
            !interfaces.split_whitespace().any(|n| n == interface),
            "utun survived disconnect",
        )?;
        Ok::<(), io::Error>(())
    })();
    if let Err(error) = &run {
        eprintln!("configured test failed: {error}; attempting production recovery");
        if let Err(restore) = coordinator.recover_as_administrator() {
            eprintln!("administrator recovery incomplete: {restore}");
        }
    }
    run?;
    await_dns(&original_effective)?;
    let restored_dns = dns::snapshot(&dns_servers)?;
    check(
        restored_dns.len() == original_dns.len(),
        "DNS service set changed",
    )?;
    for saved in original_dns {
        let actual = restored_dns
            .iter()
            .find(|r| r.service == saved.service)
            .ok_or_else(|| io::Error::other("original DNS service disappeared"))?;
        check(
            dns::equivalent(actual.before.as_deref(), saved.before.as_deref())?,
            "service DNS settings differ from baseline",
        )?;
    }
    let restored_routes = route_probes()?;
    check(
        restored_routes == original_routes,
        &format!(
            "post-disconnect routes differ: before={original_routes:?}; after={restored_routes:?}"
        ),
    )?;
    eprintln!(
        "clean disconnect: restored effective DNS, each service's DNS, IPv4/IPv6 routes and released utun"
    );
    observer.probe("after");
    observer.finish()
}
