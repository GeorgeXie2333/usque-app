use super::*;
use crate::macos::command::Output;
use std::{fs, path::PathBuf};
use uuid::Uuid;

struct FakeRunner {
    path: PathBuf,
    foreign_ipv6_host: bool,
    rules: String,
    fail_load: bool,
    fail_enable: bool,
    reference: bool,
    loads: usize,
}
impl Runner for FakeRunner {
    fn run(
        &mut self,
        program: Program,
        args: &[String],
        input: Option<&str>,
    ) -> io::Result<Output> {
        let mut stdout = String::new();
        let mut stderr = String::new();
        let mut success = true;
        if matches!(program, Program::Netstat) {
            stdout =
                "Routing tables\n\nInternet6:\nDestination Gateway Flags Netif Expire\n".into();
            if self.foreign_ipv6_host {
                stdout.push_str("fd00::1 en0 UHS en0\n");
            }
        } else if matches!(program, Program::Route) {
            assert_eq!(args[1], "get", "foreign routes must never be mutated");
            stderr = "route: writing to routing socket: not in table\n".into();
        } else if args.iter().any(|a| a == "-n") {
            stdout = input.unwrap().trim().into();
        } else if let Some(input) = input {
            let evidence: serde_json::Value =
                serde_json::from_slice(&fs::read(&self.path)?).unwrap();
            assert_eq!(
                evidence["transaction"]["pf"]["intended_rules"]
                    .as_str()
                    .unwrap()
                    .trim(),
                input.trim(),
                "exact rules must be durable before mutation"
            );
            self.rules = input.trim().into();
            self.loads += 1;
            success = !self.fail_load;
        } else if args == ["-sr"] {
            stdout = "anchor \"com.apple/*\" all\n".into();
        } else if args.last().is_some_and(|s| s == "-sr") {
            stdout = self.rules.clone();
        } else if args == ["-E"] {
            stderr = "Token : 42\n".into();
            success = !self.fail_enable;
            self.reference = success;
        }
        if args == ["-s", "info"] {
            stdout = if self.reference {
                "Status: Enabled"
            } else {
                "Status: Disabled"
            }
            .into();
        }
        if args == ["-s", "References"] {
            stdout = if self.reference {
                "TOKENS:\nPID Process Name TOKEN TIMESTAMP\n123 pfctl 42 0 days 00:00:00\n".into()
            } else {
                "No pf starter references held\n".into()
            };
        }
        if args.first().is_some_and(|s| s == "-X") {
            self.reference = false;
        }
        Ok(Output {
            pid: 123,
            stdout,
            stderr,
            success,
        })
    }
}
fn physical(_: &[DnsReceipt]) -> io::Result<Network> {
    Ok(Network {
        paths: vec![network::PhysicalPath {
            name: "en0".into(),
            index: 4,
            address: "192.0.2.2".parse().unwrap(),
            addresses: vec!["192.0.2.2/24".parse().unwrap()],
            gateway: "192.0.2.1".parse().unwrap(),
            service: Uuid::from_u128(1),
        }],
        dns: vec!["192.0.2.1:53".parse().unwrap()],
    })
}
fn fixture() -> (tempfile::TempDir, NativeBackend<FakeRunner>, Fence) {
    let dir = tempfile::tempdir().unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let path = dir.path().join("resources.json");
    let mut store = SecureStore::for_test(path.clone()).unwrap();
    store.save_value(None).unwrap();
    let backend = NativeBackend {
        store,
        receipts: None,
        runner: FakeRunner {
            path,
            foreign_ipv6_host: false,
            rules: String::new(),
            fail_load: false,
            fail_enable: false,
            reference: false,
            loads: 0,
        },
        network: None,
        plan: None,
        device: None,
        leases: BTreeMap::new(),
        next_lease: 0,
        active: false,
        uncertain: false,
        preset_candidates: vec![],
        snapshot: physical,
        boot: || Ok(Uuid::from_u128(42)),
    };
    (
        dir,
        backend,
        Fence {
            session: Uuid::new_v4(),
            configuration: 1,
            network: 1,
        },
    )
}
#[test]
fn a_route_conflict_does_not_strand_independent_receipts() {
    let (_dir, mut backend, fence) = fixture();
    backend.apply_and_verify(fence, Step::Protection).unwrap();
    let receipts = backend.receipts.as_mut().unwrap();
    receipts.interface = Some(InterfaceReceipt {
        name: "utun6".into(),
        index: 16,
        addresses: vec![],
        restored: false,
    });
    receipts.routes = ["0.0.0.0/1", "fd00::1/128", "8000::/1"]
        .map(|destination| RouteReceipt {
            destination: destination.parse().unwrap(),
            interface: "utun6".into(),
            interface_index: 16,
            restored: false,
        })
        .to_vec();
    backend.save().unwrap();
    backend.runner.foreign_ipv6_host = true;
    assert!(backend.restore_and_verify(fence, Step::Routes).is_err());
    let saved = backend.store.load().unwrap().unwrap();
    assert_eq!(
        saved.routes.iter().map(|r| r.restored).collect::<Vec<_>>(),
        [true, false, true]
    );
    assert!(!saved.pf.unwrap().restored);
    backend.runner.foreign_ipv6_host = false;
    backend.restore_and_verify(fence, Step::Routes).unwrap();
    assert!(
        backend
            .store
            .load()
            .unwrap()
            .unwrap()
            .routes
            .iter()
            .all(|r| r.restored)
    );
}
#[test]
fn a_dns_conflict_leaves_only_that_service_pending() {
    let (_dir, mut backend, fence) = fixture();
    backend.apply_and_verify(fence, Step::Protection).unwrap();
    backend.receipts.as_mut().unwrap().dns = (1..=3)
        .map(|id| DnsReceipt {
            service: Uuid::from_u128(id),
            before: None,
            applied: vec![1],
            restored: false,
        })
        .collect();
    backend.save().unwrap();
    let conflict = Uuid::from_u128(2);
    assert!(
        backend
            .restore_dns_with(|r| {
                if r.service == conflict {
                    Err(io::Error::other("DNS ownership conflict"))
                } else {
                    Ok(())
                }
            })
            .is_err()
    );
    let saved = backend.store.load().unwrap().unwrap();
    assert_eq!(
        saved.dns.iter().map(|r| r.restored).collect::<Vec<_>>(),
        [true, false, true]
    );
    assert!(saved.pf.is_some());
    backend
        .restore_dns_with(|r| {
            assert_eq!(r.service, conflict);
            Ok(())
        })
        .unwrap();
    assert!(
        backend
            .store
            .load()
            .unwrap()
            .unwrap()
            .dns
            .iter()
            .all(|r| r.restored)
    );
}
#[test]
fn pf_intents_survive_an_uncertain_native_apply_and_can_be_restored() {
    let (_dir, mut backend, fence) = fixture();
    backend.runner.fail_load = true;
    assert!(backend.apply_and_verify(fence, Step::Protection).is_err());
    assert_eq!(backend.runner.loads, 1);
    assert!(backend.store.load().unwrap().unwrap().pf.is_some());
    backend.runner.fail_load = false;
    backend.quiesce_and_revoke_egress(fence).unwrap();
    backend.restore_and_verify(fence, Step::Protection).unwrap();
    assert!(backend.store.load().unwrap().is_none());
    assert!(backend.runner.rules.is_empty());
}
#[test]
fn lost_pf_reference_evidence_never_becomes_clean() {
    let (_dir, mut backend, fence) = fixture();
    backend.runner.fail_enable = true;
    assert!(backend.apply_and_verify(fence, Step::Protection).is_err());
    assert!(backend.restore_and_verify(fence, Step::Protection).is_err());
    assert!(backend.store.load().unwrap().is_some());
    assert!(!backend.runner.rules.is_empty());
}
#[test]
fn persistence_failure_stops_further_mutations() {
    let (_dir, mut backend, fence) = fixture();
    fs::write(&backend.runner.path, b"corrupt").unwrap();
    assert!(backend.apply_and_verify(fence, Step::Protection).is_err());
    assert_eq!(backend.runner.loads, 0);
    assert!(backend.quiesce_and_revoke_egress(fence).is_err());
    assert_eq!(backend.runner.loads, 0);
}
#[test]
fn ordinary_warp_stages_and_finalizes_without_a_chain() {
    let (_dir, mut backend, _) = fixture();
    let final_wire = usque_ipc::agent_v1::TunnelPlan {
        profile_id: Uuid::new_v4().to_string(),
        endpoint: "192.0.2.2:443".into(),
        control_api_candidates: vec!["192.0.2.3:443".into()],
        assigned_ipv4: "172.16.0.2/32".into(),
        dns_servers: vec!["198.18.0.1".into()],
        mtu: 1280,
        split_dns: true,
        ..Default::default()
    };
    let mut prepare = final_wire.clone();
    prepare.defer_network_configuration = true;
    assert!(ValidatedTunnelPlan::try_from(prepare.clone()).is_err());
    assert!(ValidatedTunnelPlan::from_macos_prepare(final_wire.clone()).is_err());
    let plan = ValidatedTunnelPlan::from_macos_prepare(prepare).unwrap();
    assert!(!plan.vpn_chain);
    backend.set_plan(plan).unwrap();
    let mut changed = final_wire.clone();
    changed.kill_switch = true;
    assert!(backend.finalize_plan(changed.try_into().unwrap()).is_err());
    backend
        .finalize_plan(final_wire.try_into().unwrap())
        .unwrap();
    assert_eq!(backend.runner.loads, 0);
}

#[test]
fn ordinary_empty_recovery_needs_no_network_mutation() {
    let (_dir, mut backend, fence) = fixture();
    backend.quiesce_and_revoke_egress(fence).unwrap();
    backend.restore_and_verify(fence, Step::Protection).unwrap();
    assert_eq!(backend.runner.loads, 0);
}

#[test]
fn legacy_rejected_first_pf_load_recovers_without_touching_the_firewall() {
    let (_dir, mut backend, fence) = fixture();
    backend.runner.fail_load = true;
    assert!(backend.apply_and_verify(fence, Step::Protection).is_err());
    // A parser rejection left the dedicated kernel anchor empty. Reopen the
    // persisted intent as a restarted Agent, without a network plan or leases.
    backend.runner.rules.clear();
    backend.network = None;
    backend.receipts = backend.store.load().unwrap();
    let attempts = backend.runner.loads;
    backend.quiesce_and_revoke_egress(fence).unwrap();
    backend.restore_and_verify(fence, Step::Protection).unwrap();
    assert_eq!(backend.runner.loads, attempts);
    assert!(backend.store.load().unwrap().is_none());
    assert!(!backend.runner.reference);
}

#[test]
fn legacy_first_pf_intent_does_not_authorize_removing_foreign_rules() {
    let (_dir, mut backend, fence) = fixture();
    backend.runner.fail_load = true;
    assert!(backend.apply_and_verify(fence, Step::Protection).is_err());
    backend.runner.rules = "block in all".into();
    let attempts = backend.runner.loads;
    assert!(backend.quiesce_and_revoke_egress(fence).is_err());
    assert!(backend.restore_and_verify(fence, Step::Protection).is_err());
    assert_eq!(backend.runner.loads, attempts);
    assert!(backend.store.load().unwrap().is_some());
}

#[test]
fn changed_or_disabled_pf_never_verifies_as_protection() {
    let (_dir, mut b, f) = fixture();
    b.apply_and_verify(f, Step::Protection).unwrap();
    b.verify_protection().unwrap();
    b.runner.reference = false;
    assert!(b.verify_protection().is_err());
    b.runner.reference = true;
    b.runner.rules.push_str("\npass all");
    assert!(b.verify_protection().is_err());
}
#[test]
fn cross_boot_recovery_does_not_release_reused_pf_reference() {
    let (_dir, mut b, f) = fixture();
    b.apply_and_verify(f, Step::Protection).unwrap();
    b.boot = || Ok(Uuid::from_u128(43));
    assert!(b.quiesce_and_revoke_egress(f).is_err());
    b.runner.rules.clear();
    b.quiesce_and_revoke_egress(f).unwrap();
    b.restore_and_verify(f, Step::Protection).unwrap();
    assert!(
        b.runner.reference,
        "previous-boot token must never be released"
    );
}
#[test]
fn lan_capture_is_independent_of_allow_lan_and_keeps_both_families_in_shared_rules() {
    let endpoint = "198.51.100.1:443".parse().unwrap();
    let mut plan = ValidatedTunnelPlan {
        profile_id: Uuid::new_v4(),
        endpoint,
        endpoint_candidates: vec![endpoint],
        control_api_candidates: vec![endpoint],
        mtu: 1280,
        dns_servers: vec!["1.1.1.1".parse().unwrap()],
        split_exclusions: vec![],
        allow_lan: false,
        kill_switch: true,
        split_dns: true,
        assigned_ipv4: Some("172.16.0.2/32".parse().unwrap()),
        assigned_ipv6: Some("fd00::2/128".parse().unwrap()),
        vpn_chain: false,
        defer_network_configuration: false,
        automatic_endpoint_policy: None,
        preset_endpoint_policy: None,
    };
    let interface = InterfaceReceipt {
        name: "utun4".into(),
        index: 4,
        addresses: vec![],
        restored: false,
    };
    let mut n = physical(&[]).unwrap();
    n.paths.push(network::PhysicalPath {
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
    let captured = destinations_for(&plan, &interface, &n).unwrap();
    for address in ["0.0.0.0", "::"] {
        let ip: std::net::IpAddr = address.parse().unwrap();
        assert!(
            !captured.iter().any(|r| r.destination.contains(&ip)),
            "utun shadows the kernel's default-route lookup for {address}"
        );
    }
    for address in [
        "1.0.0.1",
        "162.159.199.2",
        "64:ff9b::7",
        "2606:4700::1",
        "fd00::42",
    ] {
        let ip: std::net::IpAddr = address.parse().unwrap();
        assert!(
            captured.iter().any(|r| r.destination.contains(&ip)),
            "lost capture for {address}"
        );
    }
    plan.allow_lan = true;
    assert_eq!(captured, destinations_for(&plan, &interface, &n).unwrap());
    for cidr in [
        "192.0.2.0/25",
        "192.0.2.128/25",
        "2001:db8::/65",
        "2001:db8:0:0:8000::/65",
        "fd00::/65",
        "fd00:0:0:0:8000::/65",
        "fe80::/65",
        "fe80:0:0:0:8000::/65",
    ] {
        assert!(
            captured
                .iter()
                .any(|r| r.destination == cidr.parse::<ipnet::IpNet>().unwrap())
        );
    }
    plan.assigned_ipv6 = None;
    assert!(
        destinations_for(&plan, &interface, &n)
            .unwrap()
            .iter()
            .all(|r| r.destination.addr().is_ipv4())
    );
}
