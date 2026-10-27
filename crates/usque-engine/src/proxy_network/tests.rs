use super::*;
use std::collections::hash_map::DefaultHasher;

const ROUTE: &str = concat!(
    "00000000000000000000000000000000 00 ",
    "00000000000000000000000000000000 00 ",
    "fe800000000000000000000000000001 00000400 00000002 00000000 00000003 eth0\n",
);

fn fingerprint(contents: Option<&[u8]>) -> u64 {
    let mut hash = DefaultHasher::new();
    linux_routes::hash_ipv6_routes(contents, &mut hash);
    hash.finish()
}

fn changed_field(index: usize, value: &str) -> String {
    let mut fields: Vec<_> = ROUTE.split_ascii_whitespace().collect();
    fields[index] = value;
    fields.join(" ") + "\n"
}

#[test]
fn ipv6_reference_and_use_counts_do_not_change_network_fingerprint() {
    let baseline = fingerprint(Some(ROUTE.as_bytes()));
    for index in [6, 7] {
        for value in ["00000000", "00000003", "ffffffff"] {
            assert_eq!(
                fingerprint(Some(changed_field(index, value).as_bytes())),
                baseline,
                "traffic counter column {index} must not invalidate a connection",
            );
        }
    }
}

#[test]
fn ipv6_routes_keep_semantic_changes() {
    let baseline = fingerprint(Some(ROUTE.as_bytes()));
    for (index, value) in [
        (0, "20010db8000000000000000000000000"),
        (1, "40"),
        (2, "20010db8000100000000000000000000"),
        (3, "40"),
        (4, "fe800000000000000000000000000002"),
        (5, "00000800"),
        (8, "00000001"),
        (9, "eth1"),
    ] {
        assert_ne!(
            fingerprint(Some(changed_field(index, value).as_bytes())),
            baseline,
            "routing column {index} must still invalidate the old path",
        );
    }
}

#[test]
fn ipv6_route_order_and_spacing_do_not_change_fingerprint() {
    let second = changed_field(9, "eth1");
    let baseline = format!("{ROUTE}{second}");
    let reordered = format!("\n\t{second}\n{ROUTE}").replace(' ', "\t  ");
    assert_eq!(
        fingerprint(Some(baseline.as_bytes())),
        fingerprint(Some(reordered.as_bytes())),
    );
}

#[test]
fn ipv6_route_additions_and_removals_change_fingerprint() {
    let baseline = fingerprint(Some(ROUTE.as_bytes()));
    assert_ne!(baseline, fingerprint(Some(b"")));
    for additional in [changed_field(9, "eth1"), ROUTE.to_owned()] {
        assert_ne!(
            baseline,
            fingerprint(Some(format!("{ROUTE}{additional}").as_bytes())),
        );
    }
}

#[test]
fn unavailable_empty_and_unrecognized_snapshots_remain_distinct() {
    let snapshots = [
        None,
        Some(b"".as_slice()),
        Some(ROUTE.as_bytes()),
        Some(b"unexpected schema".as_slice()),
        Some(b"\xff".as_slice()),
    ];
    for (index, first) in snapshots.iter().enumerate() {
        for second in &snapshots[index + 1..] {
            assert_ne!(fingerprint(*first), fingerprint(*second));
        }
    }
}

#[test]
fn unrecognized_snapshots_keep_all_input() {
    let baseline = fingerprint(Some(ROUTE.as_bytes()));
    for malformed in [
        format!("{ROUTE}unexpected schema\n"),
        format!("{ROUTE}\u{00a0}\n"),
        ROUTE.trim_end().to_owned() + " extra-column\n",
        changed_field(1, "81"),
        changed_field(4, "not-an-address"),
        changed_field(6, "not-a-counter"),
    ] {
        assert_ne!(baseline, fingerprint(Some(malformed.as_bytes())));
        assert_ne!(
            fingerprint(Some(malformed.as_bytes())),
            fingerprint(Some((malformed.clone() + "changed").as_bytes())),
        );
    }
}

#[test]
fn counter_updates_do_not_publish_a_new_network_generation() {
    let availability = PhysicalNetworkAvailability::Online {
        ipv4: false,
        ipv6: true,
    };
    let (state, mut updates) = watch::channel(PhysicalNetworkSnapshot {
        generation: 7,
        availability,
    });
    let network = ProxyNetwork { state };
    let mut previous = fingerprint(Some(ROUTE.as_bytes()));
    network.update(
        &mut previous,
        fingerprint(Some(changed_field(6, "00000003").as_bytes())),
        availability,
        false,
    );
    assert!(!updates.has_changed().unwrap());
    assert_eq!(network.network_generation(), Some(7));

    let changed_route = fingerprint(Some(changed_field(5, "00000800").as_bytes()));
    network.update(&mut previous, changed_route, availability, false);
    assert!(updates.has_changed().unwrap());
    assert_eq!(updates.borrow_and_update().generation, 8);

    network.update(&mut previous, changed_route, availability, true);
    assert!(updates.has_changed().unwrap());
    assert_eq!(updates.borrow_and_update().generation, 9);
}
