use super::*;
use crate::proxy_network::ProxyNetwork;
use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
};
use tokio::sync::watch;
use usque_transport::{PhysicalNetworkAvailability, PhysicalNetworkSnapshot, SocketProtector};

const IPV4: &str = "State:/Network/Global/IPv4";
const IPV6: &str = "State:/Network/Global/IPv6";
const DNS: &str = "State:/Network/Global/DNS";
const LINK: &str = "State:/Network/Interface/en0/Link";
const SERVICE_DNS: &str = "State:/Network/Service/service-a/DNS";

fn string(value: &str) -> CFType {
    CFString::new(value).into_CFType()
}
fn strings(values: &[&str]) -> CFType {
    CFArray::from_CFTypes(
        &values
            .iter()
            .map(|value| CFString::new(value))
            .collect::<Vec<_>>(),
    )
    .into_CFType()
}
fn dictionary(values: &[(&str, CFType)]) -> CFType {
    CFDictionary::from_CFType_pairs(
        &values
            .iter()
            .map(|(key, value)| (CFString::new(key), value.clone()))
            .collect::<Vec<_>>(),
    )
    .into_CFType()
}

struct Fixture {
    values: BTreeMap<String, CFType>,
    identities: BTreeMap<String, u32>,
}
impl Fixture {
    fn new() -> Self {
        Self {
            values: [
                (
                    IPV4.into(),
                    dictionary(&[
                        ("Router", string("192.0.2.1")),
                        ("PrimaryInterface", string("en0")),
                        ("PrimaryService", string("service-a")),
                    ]),
                ),
                (
                    IPV6.into(),
                    dictionary(&[
                        ("Router", string("fe80::1%en0")),
                        ("PrimaryInterface", string("en0")),
                        ("PrimaryService", string("service-a")),
                    ]),
                ),
                (
                    LINK.into(),
                    dictionary(&[("Active", CFBoolean::true_value().into_CFType())]),
                ),
                (
                    DNS.into(),
                    dictionary(&[("ServerAddresses", strings(&["192.0.2.53", "2001:db8::53"]))]),
                ),
                (
                    SERVICE_DNS.into(),
                    dictionary(&[("SearchDomains", strings(&["example.test"]))]),
                ),
            ]
            .into(),
            identities: [("en0".into(), 4)].into(),
        }
    }
    fn observe(&self) -> Observation {
        let values = CFDictionary::from_CFType_pairs(
            &self
                .values
                .iter()
                .map(|(key, value)| (CFString::new(key), value.clone()))
                .collect::<Vec<_>>(),
        )
        .into_untyped();
        snapshot(Some(&values), |name| {
            self.identities.get(name).copied().unwrap_or_default()
        })
    }
    fn set_field(&mut self, key: &str, field: &str, value: CFType) {
        let previous = self.values[key].downcast::<CFDictionary>().unwrap();
        let mut fields: Vec<_> = [
            "Router",
            "PrimaryInterface",
            "PrimaryService",
            "Active",
            "Detaching",
            "ServerAddresses",
            "SearchDomains",
            "SupplementalMatchDomains",
            "SearchOrder",
        ]
        .into_iter()
        .filter_map(|name| entry(&previous, name).map(|value| (name, value)))
        .collect();
        fields.retain(|(name, _)| *name != field);
        fields.push((field, value));
        self.values.insert(key.into(), dictionary(&fields));
    }
}

fn fingerprint(observation: &Observation) -> u64 {
    let mut hash = DefaultHasher::new();
    observation.hash(&mut hash);
    hash.finish()
}

#[test]
fn same_ip_gateway_interface_and_service_changes_are_observable_for_both_families() {
    let baseline = Fixture::new().observe();
    for family in [IPV4, IPV6] {
        for (field, value) in [
            ("Router", "192.0.2.2"),
            ("PrimaryInterface", "en1"),
            ("PrimaryService", "service-b"),
        ] {
            let mut fixture = Fixture::new();
            fixture.set_field(family, field, string(value));
            assert_ne!(fixture.observe(), baseline, "{family} {field}");
        }
    }
}

#[test]
fn recreated_interface_and_selected_link_state_change_identity() {
    let baseline = Fixture::new().observe();
    let mut fixture = Fixture::new();
    fixture.identities.insert("en0".into(), 8);
    assert_ne!(fixture.observe(), baseline);
    for (field, value) in [("Active", false), ("Detaching", true)] {
        let mut fixture = Fixture::new();
        fixture.set_field(LINK, field, CFBoolean::from(value).into_CFType());
        assert_ne!(fixture.observe(), baseline, "{field}");
    }
}

#[test]
fn dns_servers_search_and_supplemental_policy_change_identity() {
    let baseline = Fixture::new().observe();
    for key in [DNS, SERVICE_DNS] {
        for (field, value) in [
            ("ServerAddresses", strings(&["192.0.2.54"])),
            ("SearchDomains", strings(&["other.test"])),
            ("SupplementalMatchDomains", strings(&["corp.test"])),
            ("SearchOrder", CFNumber::from(200_i64).into_CFType()),
        ] {
            let mut fixture = Fixture::new();
            fixture.set_field(key, field, value);
            assert_ne!(fixture.observe(), baseline, "{key} {field}");
        }
    }
    let mut fixture = Fixture::new();
    fixture.set_field(
        DNS,
        "ServerAddresses",
        strings(&["2001:db8::53", "192.0.2.53"]),
    );
    assert_ne!(
        fixture.observe(),
        baseline,
        "resolver preference order is semantic"
    );
}

#[test]
fn unrelated_services_links_and_traffic_counters_do_not_invalidate_paths() {
    let baseline = Fixture::new().observe();
    let mut fixture = Fixture::new();
    fixture.set_field(IPV4, "TrafficCounter", CFNumber::from(19_i64).into_CFType());
    fixture.set_field(LINK, "TrafficCounter", CFNumber::from(27_i64).into_CFType());
    fixture.values.insert(
        "State:/Network/Interface/en9/Link".into(),
        dictionary(&[("Active", CFBoolean::false_value().into_CFType())]),
    );
    fixture.values.insert(
        "State:/Network/Service/unselected/DNS".into(),
        dictionary(&[("ServerAddresses", strings(&["203.0.113.53"]))]),
    );
    fixture.identities.insert("en9".into(), 900);
    assert_eq!(fixture.observe(), baseline);
}

#[test]
fn dictionary_order_and_cf_allocation_do_not_change_fingerprint() {
    let first = Fixture::new();
    let pairs: Vec<_> = first
        .values
        .iter()
        .rev()
        .map(|(key, value)| (CFString::new(key), value.clone()))
        .collect();
    let reordered = CFDictionary::from_CFType_pairs(&pairs).into_untyped();
    assert_eq!(
        fingerprint(&first.observe()),
        fingerprint(&snapshot(Some(&reordered), |_| 4)),
    );
    assert_eq!(first.observe(), Fixture::new().observe());
}

#[test]
fn read_failure_empty_missing_and_invalid_snapshots_are_stable_and_distinct() {
    let empty = CFDictionary::from_CFType_pairs(&[] as &[(CFString, CFType)]).into_untyped();
    let unavailable = snapshot(None, |_| panic!("must not query interfaces"));
    assert_eq!(
        unavailable,
        snapshot(None, |_| panic!("must not query interfaces"))
    );
    let empty_observation = snapshot(Some(&empty), |_| panic!("no selected interface"));
    assert_ne!(unavailable, empty_observation);
    let mut missing = Fixture::new();
    missing.values.remove(IPV4);
    let mut invalid = Fixture::new();
    invalid.values.insert(IPV4.into(), string("wrong schema"));
    assert_ne!(missing.observe(), invalid.observe());
    assert_ne!(missing.observe(), empty_observation);
    assert_ne!(invalid.observe(), unavailable);
    invalid
        .values
        .insert(IPV4.into(), string("another wrong value"));
    let invalid_first = invalid.observe();
    invalid
        .values
        .insert(IPV4.into(), CFBoolean::true_value().into_CFType());
    assert_eq!(
        invalid_first,
        invalid.observe(),
        "unknown schema is a stable marker"
    );
}

#[test]
fn malformed_identity_and_oversized_policy_do_not_call_native_helpers() {
    let mut fixture = Fixture::new();
    for family in [IPV4, IPV6] {
        fixture.set_field(family, "PrimaryInterface", string("../en0"));
        fixture.set_field(family, "PrimaryService", string("service/foreign"));
    }
    fixture.set_field(DNS, "ServerAddresses", strings(&[&"x".repeat(4097)]));
    let values = CFDictionary::from_CFType_pairs(
        &fixture
            .values
            .iter()
            .map(|(key, value)| (CFString::new(key), value.clone()))
            .collect::<Vec<_>>(),
    )
    .into_untyped();
    let observation = snapshot(Some(&values), |_| {
        panic!("invalid interface must not be queried")
    });
    assert!(matches!(observation, Observation::Available(_)));
}

#[test]
fn route_and_dns_changes_advance_generation_without_source_address_changes() {
    let availability = PhysicalNetworkAvailability::Online {
        ipv4: true,
        ipv6: true,
    };
    let (state, mut updates) = watch::channel(PhysicalNetworkSnapshot {
        generation: 12,
        availability,
    });
    let network = ProxyNetwork { state };
    let mut fixture = Fixture::new();
    let mut previous = fingerprint(&fixture.observe());
    network.update(
        &mut previous,
        fingerprint(&fixture.observe()),
        availability,
        false,
    );
    assert!(!updates.has_changed().unwrap());
    for (key, field, value) in [
        (IPV4, "Router", string("192.0.2.2")),
        (DNS, "ServerAddresses", strings(&["192.0.2.54"])),
    ] {
        let generation = network.network_generation().unwrap();
        fixture.set_field(key, field, value);
        network.update(
            &mut previous,
            fingerprint(&fixture.observe()),
            availability,
            false,
        );
        assert_eq!(updates.borrow_and_update().generation, generation + 1);
        assert_eq!(network.state.borrow().availability, availability);
    }
    network.update(
        &mut previous,
        fingerprint(&Observation::Unavailable),
        availability,
        false,
    );
    assert_eq!(updates.borrow_and_update().generation, 15);
    network.update(
        &mut previous,
        fingerprint(&Observation::Unavailable),
        availability,
        false,
    );
    assert!(
        !updates.has_changed().unwrap(),
        "persistent failure must not reconnect every poll"
    );
    network.update(
        &mut previous,
        fingerprint(&fixture.observe()),
        availability,
        false,
    );
    assert_eq!(updates.borrow_and_update().generation, 16);
}
