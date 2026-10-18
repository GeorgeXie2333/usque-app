use std::{io, net::SocketAddr};

use ipnet::IpNet;
use serde_json::{Value, json};

use super::{command, journal::Resources};

const NFT: &str = "/usr/sbin/nft";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EgressLease {
    pub mark: u32,
    pub remote: SocketAddr,
    pub protocol: u32,
    pub socket_cookie: u64,
    pub lease_id: u64,
}

pub fn install(resources: &Resources, exclusions: &[IpNet]) -> io::Result<()> {
    if table_exists(resources)? {
        return Err(io::Error::other("Linux protection table already exists"));
    }
    transaction(protection_transaction(resources, exclusions))?;
    inspect(resources, exclusions, &[])
}

pub fn install_bridge(resources: &Resources, exclusions: &[IpNet]) -> io::Result<()> {
    if table_exists(resources)? {
        // Lost acknowledgements may retry the exact already-installed guard.
        inspect(resources, exclusions, &[])
    } else {
        install(resources, exclusions)
    }
}

pub fn inspect(
    resources: &Resources,
    exclusions: &[IpNet],
    leases: &[EgressLease],
) -> io::Result<()> {
    let current = query(&["list", "table", "inet", &resources.nft_table])?;
    validate_policy(&current, resources, exclusions, leases)
}

fn validate_policy(
    current: &Value,
    resources: &Resources,
    exclusions: &[IpNet],
    leases: &[EgressLease],
) -> io::Result<()> {
    let objects = current
        .get("nftables")
        .and_then(Value::as_array)
        .ok_or_else(|| io::Error::other("invalid nftables inspection"))?;
    let mut expected = protection_transaction(resources, exclusions)["nftables"]
        .as_array()
        .expect("policy commands")
        .clone();
    expected.extend(
        egress_transaction(resources, leases)["nftables"]
            .as_array()
            .expect("egress commands")
            .iter()
            .skip(1)
            .cloned(),
    );
    let mut actual: Vec<Value> = objects
        .iter()
        .filter(|object| object.get("metainfo").is_none())
        .cloned()
        .collect();
    for object in &mut actual {
        if let Some(fields) = object
            .as_object_mut()
            .and_then(|object| object.values_mut().next())
            .and_then(Value::as_object_mut)
        {
            for runtime in ["handle", "index", "use"] {
                fields.remove(runtime);
            }
        }
    }
    let mut expected: Vec<Value> = expected
        .into_iter()
        .map(|command| {
            command
                .get("create")
                .or_else(|| command.get("add"))
                .expect("policy mutation")
                .clone()
        })
        .collect();
    for object in expected.iter_mut().chain(actual.iter_mut()) {
        if let Some(expressions) = object
            .get_mut("rule")
            .and_then(|rule| rule.get_mut("expr"))
            .and_then(Value::as_array_mut)
        {
            normalize_protocol_dependencies(expressions);
        }
    }
    // Rule ordering within each chain is significant; nft may reorder chain
    // declarations relative to rules, so compare independent chain sequences.
    for kind in ["table", "chain", "rule"] {
        let mut ours: Vec<_> = expected
            .iter()
            .filter_map(|item| item.get(kind))
            .cloned()
            .collect();
        let mut theirs: Vec<_> = actual
            .iter()
            .filter_map(|item| item.get(kind))
            .cloned()
            .collect();
        if kind != "rule" {
            ours.sort_by_key(Value::to_string);
            theirs.sort_by_key(Value::to_string);
        } else {
            ours.sort_by_key(|rule| rule["chain"].as_str().unwrap_or_default().to_owned());
            theirs.sort_by_key(|rule| rule["chain"].as_str().unwrap_or_default().to_owned());
        }
        if ours != theirs {
            return Err(io::Error::other(
                "Linux nftables policy differs from the owned guard",
            ));
        }
    }
    if actual.iter().any(|item| {
        item.as_object().is_none_or(|fields| {
            fields.len() != 1
                || fields
                    .keys()
                    .any(|key| !matches!(key.as_str(), "table" | "chain" | "rule"))
        })
    }) {
        return Err(io::Error::other("unexpected object in Linux guard"));
    }
    Ok(())
}

/// nft's netlink delinearizer omits a protocol predicate when a later typed
/// payload match implies it (payload_dependency_kill in nftables). Compare
/// those equivalent forms without discarding the payload match or any other
/// selector. In particular, a UDP port does not imply an IP address family.
fn normalize_protocol_dependencies(expressions: &mut Vec<Value>) {
    let mut index = 0;
    while index < expressions.len() {
        let redundant = [
            ("tcp", 6, "tcp"),
            ("udp", 17, "udp"),
            ("ipv6-icmp", 58, "icmpv6"),
        ]
        .into_iter()
        .any(|(name, number, payload)| {
            let expression = &expressions[index];
            let left = json!({"meta":{"key":"l4proto"}});
            if *expression != matches(left.clone(), json!(name))
                && *expression != matches(left, json!(number))
            {
                return false;
            }
            expressions[index + 1..]
                .iter()
                // Never move a condition past a verdict or side effect.
                .take_while(|next| {
                    next.as_object()
                        .is_some_and(|fields| fields.len() == 1 && fields.contains_key("match"))
                })
                .any(|next| {
                    let matched = &next["match"];
                    matched["op"] == "=="
                        && if payload == "icmpv6" {
                            matched["left"]
                                == json!({"payload":{"protocol":payload,"field":"type"}})
                        } else {
                            ["sport", "dport"].into_iter().any(|field| {
                                matched["left"]
                                    == json!({"payload":{"protocol":payload,"field":field}})
                            })
                        }
                })
        });
        if redundant {
            expressions.remove(index);
        } else {
            index += 1;
        }
    }
}

pub fn remove(resources: &Resources) -> io::Result<()> {
    if !table_exists(resources)? {
        return Ok(());
    }
    verify_table(resources)?;
    transaction(
        json!({ "nftables": [{ "delete": { "table": { "family": "inet", "name": resources.nft_table } } }] }),
    )?;
    if table_exists(resources)? {
        return Err(io::Error::other("Linux protection removal is incomplete"));
    }
    Ok(())
}

pub fn update_egress(resources: &Resources, leases: &[EgressLease]) -> io::Result<()> {
    if leases.len() > 1024 {
        return Err(io::Error::other("Linux egress lease limit reached"));
    }
    verify_table(resources)?;
    transaction(egress_transaction(resources, leases))
}

pub fn protection_transaction(resources: &Resources, exclusions: &[IpNet]) -> Value {
    let table = &resources.nft_table;
    let mut commands = vec![
        json!({"create":{"table":{"family":"inet","name":table,"comment":resources.interface_alias}}}),
        json!({"add":{"chain":{"family":"inet","table":table,"name":"egress"}}}),
        json!({"add":{"chain":{"family":"inet","table":table,"name":"output","type":"filter","hook":"output","prio":-200,"policy":"drop"}}}),
        rule(
            table,
            "output",
            vec![
                matches(json!({"meta":{"key":"oifname"}}), json!("lo")),
                json!({"accept":null}),
            ],
        ),
        rule(
            table,
            "output",
            vec![
                matches(
                    json!({"meta":{"key":"oifname"}}),
                    json!(resources.interface_name),
                ),
                json!({"accept":null}),
            ],
        ),
        rule(table, "output", vec![json!({"jump":{"target":"egress"}})]),
    ];
    // Kernel neighbor discovery is required to retain the physical path. It
    // cannot carry ordinary TCP/UDP payloads, and is restricted to link scope.
    commands.push(rule(
        table,
        "output",
        vec![
            matches(json!({"meta":{"key":"l4proto"}}), json!("ipv6-icmp")),
            matches(
                json!({"payload":{"protocol":"ip6","field":"hoplimit"}}),
                json!(255),
            ),
            matches(
                json!({"payload":{"protocol":"icmpv6","field":"type"}}),
                json!({"set":["nd-router-solicit", "nd-router-advert", "nd-neighbor-solicit", "nd-neighbor-advert"]}),
            ),
            json!({"accept":null}),
        ],
    ));
    // Only the system network manager may renew DHCP while the guard is up.
    for (family, source, destination) in [("ip", 68, 67), ("ip6", 546, 547)] {
        commands.push(rule(
            table,
            "output",
            vec![
                matches(json!({"meta":{"key":"skuid"}}), json!(0)),
                matches(
                    json!({"meta":{"key":"nfproto"}}),
                    json!(if family == "ip" { "ipv4" } else { "ipv6" }),
                ),
                matches(json!({"meta":{"key":"l4proto"}}), json!("udp")),
                matches(
                    json!({"payload":{"protocol":"udp","field":"sport"}}),
                    json!(source),
                ),
                matches(
                    json!({"payload":{"protocol":"udp","field":"dport"}}),
                    json!(destination),
                ),
                json!({"accept":null}),
            ],
        ));
    }
    for network in exclusions {
        let protocol = if network.addr().is_ipv4() {
            "ip"
        } else {
            "ip6"
        };
        commands.push(rule(table, "output", vec![matches(json!({"payload":{"protocol":protocol,"field":"daddr"}}),
            json!({"prefix":{"addr":network.network().to_string(),"len":network.prefix_len()}})), json!({"accept":null})]));
    }
    // Forwarding must not bypass the host output policy, including IPv6.
    commands.push(json!({"add":{"chain":{"family":"inet","table":table,"name":"forward","type":"filter","hook":"forward","prio":-200,"policy":"drop"}}}));
    json!({"nftables":commands})
}

pub fn egress_transaction(resources: &Resources, leases: &[EgressLease]) -> Value {
    let mut commands = vec![
        json!({"flush":{"chain":{"family":"inet","table":resources.nft_table,"name":"egress"}}}),
    ];
    for lease in leases {
        let family = if lease.remote.is_ipv4() { "ip" } else { "ip6" };
        let protocol = if lease.protocol == 6 { "tcp" } else { "udp" };
        commands.push(rule(
            &resources.nft_table,
            "egress",
            vec![
                matches(json!({"meta":{"key":"mark"}}), json!(lease.mark)),
                matches(json!({"meta":{"key":"l4proto"}}), json!(protocol)),
                matches(
                    json!({"payload":{"protocol":family,"field":"daddr"}}),
                    json!(lease.remote.ip().to_string()),
                ),
                matches(
                    json!({"payload":{"protocol":protocol,"field":"dport"}}),
                    json!(lease.remote.port()),
                ),
                json!({"accept":null}),
            ],
        ));
    }
    json!({"nftables":commands})
}

fn matches(left: Value, right: Value) -> Value {
    json!({"match":{"op":"==","left":left,"right":right}})
}
fn rule(table: &str, chain: &str, expressions: Vec<Value>) -> Value {
    json!({"add":{"rule":{"family":"inet","table":table,"chain":chain,"expr":expressions}}})
}

fn transaction(value: Value) -> io::Result<()> {
    let bytes = serde_json::to_vec(&value).map_err(io::Error::other)?;
    command::run(
        NFT,
        &["--json".into(), "--file".into(), "-".into()],
        Some(bytes),
    )
    .map(|_| ())
}

fn query(arguments: &[&str]) -> io::Result<Value> {
    let mut args = vec!["--json".to_owned()];
    args.extend(arguments.iter().map(|value| (*value).to_owned()));
    let bytes = command::run(NFT, &args, None)?;
    serde_json::from_slice(&bytes).map_err(|_| io::Error::other("invalid nftables response"))
}

fn table_exists(resources: &Resources) -> io::Result<bool> {
    let value = query(&["list", "tables"])?;
    let tables = value
        .get("nftables")
        .and_then(Value::as_array)
        .ok_or_else(|| io::Error::other("invalid nftables tables"))?;
    Ok(tables.iter().any(|item| {
        item.get("table")
            .is_some_and(|table| table["family"] == "inet" && table["name"] == resources.nft_table)
    }))
}

fn verify_table(resources: &Resources) -> io::Result<()> {
    let value = query(&["list", "table", "inet", &resources.nft_table])?;
    let objects = value
        .get("nftables")
        .and_then(Value::as_array)
        .ok_or_else(|| io::Error::other("invalid nftables table"))?;
    if !objects.iter().any(|item| {
        item.get("table").is_some_and(|table| {
            table["family"] == "inet"
                && table["name"] == resources.nft_table
                && table["comment"] == resources.interface_alias
        })
    }) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Linux firewall ownership changed",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listed_policy(resources: &Resources, leases: &[EgressLease]) -> Value {
        let mut objects: Vec<_> = protection_transaction(resources, &[])["nftables"]
            .as_array()
            .unwrap()
            .iter()
            .map(|command| {
                command
                    .get("create")
                    .or_else(|| command.get("add"))
                    .unwrap()
                    .clone()
            })
            .collect();
        objects.extend(
            egress_transaction(resources, leases)["nftables"]
                .as_array()
                .unwrap()
                .iter()
                .skip(1)
                .map(|command| command["add"].clone()),
        );
        json!({"nftables": objects})
    }

    fn native_readback(resources: &Resources, leases: &[EgressLease]) -> Value {
        let mut listed = listed_policy(resources, leases);
        let objects = listed["nftables"].as_array_mut().unwrap();
        // Native forms follow nftables' payload_dependency_kill and its typed
        // JSON payload fixtures (tests/py/inet/udp.t.json and ip6/icmpv6.t.json).
        // These fixtures deliberately do not call our normalization helper.
        let rules = objects
            .iter_mut()
            .filter_map(|object| object.get_mut("rule"));
        for (index, rule) in rules.enumerate() {
            rule["handle"] = json!(index + 10);
            match index {
                3 => {
                    rule["expr"] = json!([
                        {"match":{"op":"==","left":{"payload":{"protocol":"ip6","field":"hoplimit"}},"right":255}},
                        {"match":{"op":"==","left":{"payload":{"protocol":"icmpv6","field":"type"}},"right":{"set":["nd-router-solicit","nd-router-advert","nd-neighbor-solicit","nd-neighbor-advert"]}}},
                        {"accept":null}
                    ]);
                }
                4 | 5 => {
                    let (family, source, destination) = if index == 4 {
                        ("ipv4", 68, 67)
                    } else {
                        ("ipv6", 546, 547)
                    };
                    rule["expr"] = json!([
                        {"match":{"op":"==","left":{"meta":{"key":"skuid"}},"right":0}},
                        {"match":{"op":"==","left":{"meta":{"key":"nfproto"}},"right":family}},
                        {"match":{"op":"==","left":{"payload":{"protocol":"udp","field":"sport"}},"right":source}},
                        {"match":{"op":"==","left":{"payload":{"protocol":"udp","field":"dport"}},"right":destination}},
                        {"accept":null}
                    ]);
                }
                index if index >= 6 => {
                    let lease = &leases[index - 6];
                    let family = if lease.remote.is_ipv4() { "ip" } else { "ip6" };
                    let protocol = if lease.protocol == 6 { "tcp" } else { "udp" };
                    rule["expr"] = json!([
                        {"match":{"op":"==","left":{"meta":{"key":"mark"}},"right":lease.mark}},
                        {"match":{"op":"==","left":{"payload":{"protocol":family,"field":"daddr"}},"right":lease.remote.ip().to_string()}},
                        {"match":{"op":"==","left":{"payload":{"protocol":protocol,"field":"dport"}},"right":lease.remote.port()}},
                        {"accept":null}
                    ]);
                }
                _ => {}
            }
        }
        listed
    }

    #[test]
    fn native_readback_accepts_only_implied_protocol_predicate_elision() {
        let resources = Resources::for_operation(uuid::Uuid::new_v4());
        let leases = [
            EgressLease {
                mark: resources.socket_mark | 1,
                remote: "192.0.2.1:443".parse().unwrap(),
                protocol: 17,
                socket_cookie: 42,
                lease_id: 1,
            },
            EgressLease {
                mark: resources.socket_mark | 2,
                remote: "[2001:db8::1]:443".parse().unwrap(),
                protocol: 6,
                socket_cookie: 43,
                lease_id: 2,
            },
        ];
        let native = native_readback(&resources, &leases);
        validate_policy(&native, &resources, &[], &leases).unwrap();
        validate_policy(
            &listed_policy(&resources, &leases),
            &resources,
            &[],
            &leases,
        )
        .unwrap();

        for (object_index, object) in native["nftables"].as_array().unwrap().iter().enumerate() {
            let Some(expressions) = object["rule"]["expr"].as_array() else {
                continue;
            };
            for (expression_index, expression) in expressions.iter().enumerate() {
                let mut missing = native.clone();
                missing["nftables"][object_index]["rule"]["expr"]
                    .as_array_mut()
                    .unwrap()
                    .remove(expression_index);
                assert!(
                    validate_policy(&missing, &resources, &[], &leases).is_err(),
                    "accepted missing rule {object_index} expression {expression_index}"
                );
                if expression.get("match").is_some() {
                    let mut changed = native.clone();
                    changed["nftables"][object_index]["rule"]["expr"][expression_index]["match"]
                        ["right"] = json!("changed condition");
                    assert!(validate_policy(&changed, &resources, &[], &leases).is_err());
                    let mut negated = native.clone();
                    negated["nftables"][object_index]["rule"]["expr"][expression_index]["match"]
                        ["op"] = json!("!=");
                    assert!(validate_policy(&negated, &resources, &[], &leases).is_err());
                }
            }
        }
    }

    #[test]
    fn normalization_keeps_protocol_family_and_unproven_transport_conditions() {
        let udp = matches(json!({"meta":{"key":"l4proto"}}), json!("udp"));
        for following in [
            vec![matches(
                json!({"payload":{"protocol":"tcp","field":"dport"}}),
                json!(443),
            )],
            vec![matches(
                json!({"payload":{"base":"th","offset":16,"len":16}}),
                json!(443),
            )],
            vec![
                json!({"accept":null}),
                matches(
                    json!({"payload":{"protocol":"udp","field":"dport"}}),
                    json!(443),
                ),
            ],
        ] {
            let mut expressions = vec![udp.clone()];
            expressions.extend(following);
            let original = expressions.clone();
            normalize_protocol_dependencies(&mut expressions);
            assert_eq!(expressions, original);
        }
        for right in [json!("udp"), json!(17)] {
            let family = matches(json!({"meta":{"key":"nfproto"}}), json!("ipv4"));
            let port = matches(
                json!({"payload":{"protocol":"udp","field":"dport"}}),
                json!(67),
            );
            let mut expressions = vec![
                family.clone(),
                matches(json!({"meta":{"key":"l4proto"}}), right),
                port.clone(),
            ];
            normalize_protocol_dependencies(&mut expressions);
            assert_eq!(expressions, vec![family, port]);
        }
    }

    #[test]
    fn native_inspection_rejects_an_extra_accept_or_missing_policy() {
        let resources = Resources::for_operation(uuid::Uuid::new_v4());
        let objects: Vec<_> = protection_transaction(&resources, &[])["nftables"]
            .as_array()
            .unwrap()
            .iter()
            .map(|command| {
                command
                    .get("create")
                    .or_else(|| command.get("add"))
                    .unwrap()
                    .clone()
            })
            .collect();
        let valid = json!({"nftables": objects});
        validate_policy(&valid, &resources, &[], &[]).unwrap();
        let mut extra = valid.clone();
        extra["nftables"].as_array_mut().unwrap().push(json!({"rule":{"family":"inet", "table":resources.nft_table,"chain":"output","expr":[{"accept":null}]}}));
        assert!(validate_policy(&extra, &resources, &[], &[]).is_err());
        let mut missing = valid;
        missing["nftables"].as_array_mut().unwrap().pop();
        assert!(validate_policy(&missing, &resources, &[], &[]).is_err());
    }

    #[test]
    fn protection_is_atomic_dual_stack_and_never_flushes_foreign_rules() {
        let resources = Resources::for_operation(uuid::Uuid::new_v4());
        let batch = protection_transaction(&resources, &[]);
        assert_eq!(batch["nftables"][0]["create"]["table"]["family"], "inet");
        let serialized = batch.to_string();
        assert!(serialized.contains("\"policy\":\"drop\""));
        assert!(!serialized.contains("flush"));
        // No blanket permit is attached to the unprivileged Engine UID.
        assert!(!serialized.contains("1000"));
        assert!(!serialized.contains("established"));
    }

    #[test]
    fn every_physical_permit_is_socket_mark_destination_and_protocol_scoped() {
        let resources = Resources::for_operation(uuid::Uuid::new_v4());
        let batch = egress_transaction(
            &resources,
            &[EgressLease {
                mark: resources.socket_mark | 1,
                remote: "[2606:4700::1]:443".parse().unwrap(),
                protocol: 6,
                socket_cookie: 42,
                lease_id: 123,
            }],
        );
        let rule = &batch["nftables"][1]["add"]["rule"];
        assert_eq!(rule["expr"].as_array().unwrap().len(), 5);
        assert_eq!(rule["expr"][0]["match"]["right"], resources.socket_mark | 1);
        assert_eq!(rule["expr"][1]["match"]["right"], "tcp");
        assert_eq!(rule["expr"][2]["match"]["right"], "2606:4700::1");
        assert_eq!(rule["expr"][3]["match"]["right"], 443);
    }
}
