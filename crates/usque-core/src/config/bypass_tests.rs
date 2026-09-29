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
