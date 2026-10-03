//! Pure manifests and inert native structures; never open the WFP engine.
use super::*;

fn plan() -> ReplacementGuardPlan {
    ReplacementGuardPlan {
        exclusions: Vec::new(),
        target: super::super::tests::plan("162.159.198.2".parse().unwrap(), false),
    }
}

#[test]
fn intent_keys_are_bounded_and_disjoint_from_normal_rollback() {
    let receipt = plan_replacement_guard(&plan()).unwrap();
    let keys = receipt_keys(&receipt).unwrap();
    assert!(!keys.is_empty());
    let MutationReceipt::KillSwitch { filter_ids, .. } = &receipt else {
        unreachable!()
    };
    assert!(
        filter_ids.is_empty(),
        "planning performs no native mutation"
    );
    let normal = (0..MAX_FILTERS)
        .map(filter_key)
        .chain([PROVIDER_KEY, SUBLAYER_KEY])
        .collect::<BTreeSet<_>>();
    let replacement = (0..REPLACEMENT_MAX_FILTERS)
        .map(replacement_filter_key)
        .chain([REPLACEMENT_WFP_PROVIDER_KEY, REPLACEMENT_WFP_SUBLAYER_KEY])
        .collect::<BTreeSet<_>>();
    assert_eq!(replacement.len(), REPLACEMENT_MAX_FILTERS + 2);
    assert!(normal.is_disjoint(&replacement));
    assert!(receipt_keys(&plan_kill_switch(&plan().target, 42).unwrap()).is_err());
    let mut invalid = receipt.clone();
    if let MutationReceipt::KillSwitch { filter_keys, .. } = &mut invalid {
        filter_keys[0] = filter_key(0);
    }
    assert!(receipt_keys(&invalid).is_err());
    let mut invalid = receipt.clone();
    if let MutationReceipt::KillSwitch { filter_ids, .. } = &mut invalid {
        *filter_ids = vec![0; keys.len()];
    }
    assert!(receipt_keys(&invalid).is_err());
    if let MutationReceipt::KillSwitch { filter_ids, .. } = &mut invalid {
        *filter_ids = vec![1; keys.len()];
    }
    assert!(receipt_keys(&invalid).is_err());
}

#[test]
fn replacement_always_blocks_both_families_without_any_tun_interface_permit() {
    let mut plan = plan();
    plan.target.assigned_ipv6 = None;
    let rules = build_replacement_rules(&plan).unwrap();
    for family in [AddressFamily::V4, AddressFamily::V6] {
        let final_rule = rules.iter().rfind(|r| r.family == family).unwrap();
        assert_eq!(final_rule.action, RuleAction::Block);
        assert!(final_rule.conditions.is_empty());
    }
    assert!(rules.iter().all(|r| {
        !r.conditions
            .iter()
            .any(|c| matches!(c, ConditionSpec::InterfaceLuid(_)))
    }));
}

#[test]
fn guard_accepts_only_narrow_exclusions_already_in_the_target_policy() {
    let mut plan = plan();
    plan.target.allow_lan = true;
    plan.exclusions = vec!["10.1.0.0/16".parse().unwrap(), "fc12::/16".parse().unwrap()];
    let rules = build_replacement_rules(&plan).unwrap();
    let bypasses = rules
        .iter()
        .filter(|r| r.name == "Replacement shared bypass")
        .flat_map(|r| &r.conditions)
        .collect::<Vec<_>>();
    assert_eq!(
        bypasses,
        vec![
            &ConditionSpec::RemoteNetwork("10.1.0.0/16".parse().unwrap()),
            &ConditionSpec::RemoteNetwork("fc12::/16".parse().unwrap()),
        ]
    );
    plan.exclusions = vec!["0.0.0.0/0".parse().unwrap()];
    assert!(build_replacement_rules(&plan).is_err());
    plan.exclusions = vec!["203.0.113.0/24".parse().unwrap()];
    assert!(build_replacement_rules(&plan).is_err());
    plan.target.allow_lan = false;
    plan.target.split_exclusions = vec!["10.1.0.0/16".parse().unwrap()];
    plan.exclusions = vec!["10.0.0.0/8".parse().unwrap()];
    assert!(build_replacement_rules(&plan).is_err());
}

#[test]
fn only_the_new_plans_exact_bootstrap_roles_survive_retargeting() {
    let previous = plan();
    let mut next = previous.clone();
    next.target.endpoint = "162.159.198.3:8443".parse().unwrap();
    next.target.endpoint_candidates = vec![next.target.endpoint];
    next.target.control_api_candidates = vec!["198.51.100.11:443".parse().unwrap()];
    let rules = build_replacement_rules(&next).unwrap();
    let controls = rules
        .iter()
        .filter(|r| r.conditions.contains(&ConditionSpec::ApplicationId))
        .collect::<Vec<_>>();
    assert_eq!(controls.len(), 3);
    for rule in controls {
        assert_eq!(rule.conditions.len(), 4);
        assert!(
            rule.conditions
                .iter()
                .any(|c| matches!(c, ConditionSpec::RemotePort(_)))
        );
        assert!(
            rule.conditions
                .iter()
                .any(|c| matches!(c, ConditionSpec::Protocol(6 | 17)))
        );
    }
    for obsolete in [
        previous.target.endpoint,
        previous.target.control_api_candidates[0],
    ] {
        assert!(rules.iter().all(|r| {
            !r.conditions
                .contains(&ConditionSpec::RemoteNetwork(host_network(obsolete.ip())))
        }));
    }
    assert!(
        rules
            .iter()
            .all(|r| !r.conditions.contains(&ConditionSpec::RemotePort(53)))
    );
}

#[test]
fn automatic_masque_has_no_static_pool_permit_and_control_mirrors_are_exact() {
    let mut plan = plan();
    plan.target.automatic_endpoint_policy = Some(usque_core::AutomaticEndpointPolicy {
        pool: usque_core::EndpointPool::Free,
        port: 443,
        ipv4: true,
        ipv6: true,
        tcp: true,
        udp: true,
    });
    let rules = build_replacement_rules(&plan).unwrap();
    assert!(
        rules.iter().all(|r| !r
            .conditions
            .contains(&ConditionSpec::RemoteNetwork(host_network(
                plan.target.endpoint.ip()
            ))))
    );
    let remote = "162.159.198.3:443".parse().unwrap();
    let rule = dynamic_direct_rule(remote, 17, 72).unwrap();
    assert_eq!(
        rule.conditions,
        vec![
            ConditionSpec::ApplicationId,
            ConditionSpec::InterfaceLuid(72),
            ConditionSpec::RemoteNetwork(host_network(remote.ip())),
            ConditionSpec::RemotePort(443),
            ConditionSpec::Protocol(17),
        ]
    );
    assert_eq!(
        control_layers(),
        [
            (PROVIDER_KEY, SUBLAYER_KEY),
            (REPLACEMENT_WFP_PROVIDER_KEY, REPLACEMENT_WFP_SUBLAYER_KEY),
        ]
    );
}

#[test]
fn native_presence_requires_enabled_persistent_unconditional_blocks_for_both_families() {
    let mut observed = [false; 2];
    for layer in [
        FWPM_LAYER_ALE_AUTH_CONNECT_V4,
        FWPM_LAYER_ALE_AUTH_CONNECT_V6,
    ] {
        let mut filter = FWPM_FILTER0 {
            layerKey: layer,
            flags: FWPM_FILTER_FLAG_PERSISTENT,
            action: FWPM_ACTION0 {
                r#type: FWP_ACTION_BLOCK,
                ..Default::default()
            },
            ..Default::default()
        };
        filter.flags |= FWPM_FILTER_FLAG_DISABLED;
        note_block(&filter, &mut observed);
        assert_eq!(observed, [false; 2]);
        filter.flags = 0;
        note_block(&filter, &mut observed);
        assert_eq!(observed, [false; 2]);
        filter.flags = FWPM_FILTER_FLAG_PERSISTENT;
        filter.numFilterConditions = 1;
        note_block(&filter, &mut observed);
        assert_eq!(observed, [false; 2]);
    }
    for (index, layer) in [
        FWPM_LAYER_ALE_AUTH_CONNECT_V4,
        FWPM_LAYER_ALE_AUTH_CONNECT_V6,
    ]
    .into_iter()
    .enumerate()
    {
        let filter = FWPM_FILTER0 {
            layerKey: layer,
            flags: FWPM_FILTER_FLAG_PERSISTENT,
            action: FWPM_ACTION0 {
                r#type: FWP_ACTION_BLOCK,
                ..Default::default()
            },
            ..Default::default()
        };
        note_block(&filter, &mut observed);
        assert!(observed[index]);
    }
    assert_eq!(observed, [true, true]);
}
