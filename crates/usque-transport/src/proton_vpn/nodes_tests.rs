//! Synthetic directory shapes checked against the official ConnectingDomain
//! model and the 2026-10-19 v2 response. No live addresses or peer keys are stored.
use super::nodes::collect;
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};

fn config() -> Value {
    json!({"Code":1000,"DefaultPorts":{"WireGuard":{"UDP":[443,51820]}}})
}

fn directory() -> Value {
    json!({"Code":1000,"StatusID":"synthetic-status","LogicalServers":[{
        "ID":"logical","Name":"Synthetic Free","Tier":0,"Features":0,
        "ExitCountry":"IS","City":null,
        "StatusReference":{"Index":0,"Penalty":0.0,"Cost":0},
        "Servers":[{"ID":"physical","Status":1,"EntryIP":"8.8.8.8",
            "Domain":"agent.example.invalid","X25519PublicKey":STANDARD.encode([8u8;32])}]
    }]})
}

fn physical(value: &mut Value) -> &mut Value {
    &mut value["LogicalServers"][0]["Servers"][0]
}

#[test]
fn v2_entry_ip_directory_without_per_protocol_map_yields_free_wireguard_nodes() {
    let nodes = collect(&directory(), &config()).unwrap();
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].endpoint.host, "8.8.8.8");
    assert_eq!(nodes[0].endpoint.port, 443);
    assert_eq!(nodes[0].metadata.id, "logical");
    // Binary logical status/load is separate; no fabricated zero-load score.
    assert_eq!(nodes[0].metadata.load, 100);
}

#[test]
fn protocol_specific_entry_precedes_default_and_nullable_ports_use_client_config() {
    for (ports, expected) in [(json!([1194]), 1194), (Value::Null, 443)] {
        let mut value = directory();
        physical(&mut value)["EntryPerProtocol"] =
            json!({"WireGuardUDP":{"IPv4":"9.9.9.9","Ports":ports}});
        let nodes = collect(&value, &config()).unwrap();
        assert_eq!(nodes[0].endpoint.host, "9.9.9.9");
        assert_eq!(nodes[0].endpoint.port, expected);
    }
    for protocols in [
        Value::Null,
        json!({}),
        json!({"WireGuardTCP":{"IPv4":"9.9.9.9"}}),
    ] {
        let mut value = directory();
        physical(&mut value)["EntryPerProtocol"] = protocols;
        assert_eq!(
            collect(&value, &config()).unwrap()[0].endpoint.host,
            "8.8.8.8"
        );
    }
}

#[test]
fn malformed_explicit_wireguard_entries_do_not_fall_back_to_entry_ip() {
    for entry in [
        Value::Null,
        json!(true),
        json!({}),
        json!({"IPv4":"127.0.0.1"}),
        json!({"IPv4":"invalid"}),
        json!({"IPv4":"9.9.9.9","Ports":[]}),
        json!({"IPv4":"9.9.9.9","Ports":[0]}),
        json!({"IPv4":"9.9.9.9","Ports":[443,65536]}),
        json!({"IPv4":"9.9.9.9","Ports":"443"}),
        json!({"IPv4":"9.9.9.9","Ports":vec![443;33]}),
    ] {
        let mut value = directory();
        physical(&mut value)["EntryPerProtocol"] = json!({"WireGuardUDP":entry});
        assert_eq!(
            collect(&value, &config()).err().unwrap().reason,
            "no_free_wireguard_nodes"
        );
    }
    let mut value = directory();
    physical(&mut value)["EntryPerProtocol"] = json!([]);
    assert_eq!(
        collect(&value, &config()).err().unwrap().reason,
        "no_free_wireguard_nodes"
    );
}

#[test]
fn default_entry_requires_public_ip_online_peer_key_and_free_unrestricted_server() {
    for address in [
        "127.0.0.1",
        "10.0.0.1",
        "192.168.1.1",
        "169.254.0.1",
        "::1",
        "peer.example.invalid",
    ] {
        let mut value = directory();
        physical(&mut value)["EntryIP"] = json!(address);
        assert!(collect(&value, &config()).is_err());
    }
    for (field, replacement) in [
        ("Status", json!(0)),
        ("Status", Value::Null),
        ("X25519PublicKey", json!(STANDARD.encode([0u8; 32]))),
        ("X25519PublicKey", Value::Null),
    ] {
        let mut value = directory();
        physical(&mut value)[field] = replacement;
        assert!(collect(&value, &config()).is_err());
    }
    for (field, replacement) in [
        ("Tier", 1),
        ("Features", 1),
        ("Features", 32),
        ("Status", 0),
    ] {
        let mut value = directory();
        value["LogicalServers"][0][field] = json!(replacement);
        assert!(collect(&value, &config()).is_err());
    }
    assert_eq!(
        collect(&directory(), &json!({})).err().unwrap().reason,
        "invalid_client_configuration"
    );
}

#[test]
fn catalogue_is_logical_unique_complete_and_filtered_by_authenticated_tier() {
    let mut value = directory();
    let template = value["LogicalServers"][0].clone();
    let mut logicals = Vec::new();
    for i in 0..600 {
        let mut logical = template.clone();
        logical["ID"] = json!(format!("logical-{i}"));
        logical["Tier"] = json!(i % 3);
        let physical = logical["Servers"][0].clone();
        logical["Servers"]
            .as_array_mut()
            .unwrap()
            .extend([physical.clone(), physical]);
        logicals.push(logical.clone());
        logicals.push(logical);
    }
    value["LogicalServers"] = json!(logicals);
    assert_eq!(collect(&value, &config()).unwrap().len(), 200);
    let paid = super::nodes::collect_for_tier(&value, &config(), 2).unwrap();
    assert_eq!(paid.len(), 600);
    let ids: std::collections::BTreeSet<_> = paid.iter().map(|node| &node.metadata.id).collect();
    assert_eq!(ids.len(), 600);
    assert!(paid.iter().all(|node| !node.metadata.id.contains(':')));
}

#[test]
fn physical_label_is_preserved_including_zero_and_malformed_labels_are_rejected() {
    for label in ["0", "1", "partner", "node-3"] {
        let mut value = directory();
        physical(&mut value)["Label"] = json!(label);
        assert_eq!(
            collect(&value, &config()).unwrap()[0].label.as_deref(),
            Some(label)
        );
    }
    for label in [json!(""), json!("  "), Value::Null] {
        let mut value = directory();
        physical(&mut value)["Label"] = label;
        assert!(collect(&value, &config()).unwrap()[0].label.is_none());
    }
    for label in [
        json!(1),
        json!(true),
        json!("x\nsecret"),
        json!("../endpoint"),
        json!("x".repeat(65)),
    ] {
        let mut value = directory();
        physical(&mut value)["Label"] = label;
        assert!(collect(&value, &config()).is_err());
    }
}
