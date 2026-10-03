use super::*;

#[test]
fn domains_normalize_idna_suffixes_and_duplicates_without_accepting_urls() {
    assert_eq!(
        normalize_bypass_domains(&["BÜCHER.example.".into(), "xn--bcher-kva.example".into()])
            .unwrap(),
        ["xn--bcher-kva.example"]
    );
    for invalid in [
        "",
        ".",
        "example..com",
        "*.example.com",
        "https://example.com",
        "example.com:443",
        "example.com/path",
        "example.com?x",
        "example.com#x",
        " example.com",
        "example.com ",
        "a_b.example",
        "127.0.0.1",
        "127.1",
        "[::1]",
        "-bad.example",
        "bad-.example",
        "example.com..",
    ] {
        assert!(canonical_bypass_domain(invalid).is_none(), "{invalid}");
    }
    assert!(normalize_bypass_domains(&vec!["example.com".into(); 257]).is_err());
    assert!(normalize_bypass_domains(&vec!["example.com".into(); 256]).is_ok());
}

#[test]
fn domain_length_limits_apply_after_removing_one_trailing_dot() {
    for (length, labels) in [
        (128, vec![63, 62, 1]),
        (129, vec![63, 63, 1]),
        (253, vec![63, 63, 63, 61]),
    ] {
        let domain = labels
            .into_iter()
            .map(|length| "A".repeat(length))
            .collect::<Vec<_>>()
            .join(".");
        assert_eq!(domain.len(), length);
        let normalized = domain.to_ascii_lowercase();
        assert_eq!(canonical_bypass_domain(&domain), Some(normalized.clone()));
        let absolute = format!("{domain}.");
        assert_eq!(canonical_bypass_domain(&absolute), Some(normalized.clone()));
        assert_eq!(
            normalize_bypass_domains(&[domain, absolute]).unwrap(),
            [normalized]
        );
    }

    let too_long = [
        "a".repeat(63),
        "a".repeat(63),
        "a".repeat(63),
        "a".repeat(62),
    ]
    .join(".");
    assert_eq!(too_long.len(), 254);
    for domain in [too_long.clone(), format!("{too_long}.")] {
        assert!(canonical_bypass_domain(&domain).is_none());
        assert_eq!(
            normalize_bypass_domains(&[domain]),
            Err(ConfigError::InvalidBypassDomain(1))
        );
    }
}

#[test]
fn unicode_domain_length_is_counted_in_codepoints_before_idna_mapping() {
    let domain = [
        "Ａ".repeat(63),
        "Ａ".repeat(63),
        "Ａ".repeat(63),
        "Ａ".repeat(61),
    ]
    .join(".");
    assert_eq!(domain.chars().count(), 253);
    assert!(domain.len() > 253);
    let normalized = domain.replace('Ａ', "a");
    assert_eq!(canonical_bypass_domain(&domain), Some(normalized.clone()));
    assert_eq!(
        normalize_bypass_domains(&[domain.clone(), format!("{domain}."), normalized.clone()])
            .unwrap(),
        [normalized]
    );

    let too_long = format!("{domain}Ａ");
    assert_eq!(too_long.chars().count(), 254);
    assert!(canonical_bypass_domain(&too_long).is_none());
    assert_eq!(
        normalize_bypass_domains(&["example.com".into(), too_long]),
        Err(ConfigError::InvalidBypassDomain(2))
    );
}

#[test]
fn supplementary_unicode_domains_remain_within_core_idna_limits() {
    let label = "\u{20000}".repeat(32);
    let domain = [label.as_str(); 4].join(".");
    assert_eq!(domain.chars().count(), 131);
    assert_eq!(domain.encode_utf16().count(), 259);
    let normalized = canonical_bypass_domain(&domain).unwrap();
    assert!(normalized.is_ascii());
    assert!(normalized.len() <= 253);
    assert!(normalized.split('.').all(|label| label.len() <= 63));
    assert_eq!(
        normalize_bypass_domains(&[domain.clone(), format!("{domain}."), normalized.clone()])
            .unwrap(),
        [normalized]
    );
}

#[test]
fn canonical_rules_preserve_dns_conflict_checks_and_reconnect() {
    let previous = Profile::default();
    let mut next = previous.clone();
    next.bypass_domains = vec!["Example.COM.".into()];
    next.split_exclusions = vec![
        "192.0.2.5/24".parse().unwrap(),
        "192.0.2.0/24".parse().unwrap(),
    ];
    next.canonicalize_geo_direct().unwrap();
    assert_eq!(next.split_exclusions.len(), 1);
    assert_eq!(next.split_exclusions[0].to_string(), "192.0.2.0/24");
    assert_eq!(next.bypass_domains, ["example.com"]);
    assert_eq!(
        crate::classify_reconfigure(&previous, &next),
        crate::ReconfigureClass::ColdReconnect
    );
    next.frontends.tunnel = true;
    next.split_exclusions.push("1.1.1.1/32".parse().unwrap());
    assert!(matches!(
        next.validate(),
        Err(ConfigError::VpnDnsServerBypassed(_))
    ));
}

#[test]
fn bypass_domains_are_shared_and_reset() {
    let mut config = AppConfig::default();
    config.network.bypass_domains = vec!["example.com".into()];
    let id = Uuid::new_v4();
    config.insert_account(id, "Second".into(), None).unwrap();
    assert_eq!(
        config.runtime_profile(id).unwrap().bypass_domains,
        ["example.com"]
    );
    config.network.reset_user_defaults();
    assert!(
        config
            .runtime_profile(id)
            .unwrap()
            .bypass_domains
            .is_empty()
    );
}
