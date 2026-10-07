use super::*;

fn encrypted(mode: WarpDnsMode) -> WarpDnsSettings {
    let mut settings = WarpDnsSettings {
        mode,
        server_name: "DNS.Example.COM".into(),
        bootstrap_ips: vec!["192.0.2.53".parse().unwrap()],
        ..WarpDnsSettings::default()
    };
    settings.canonicalize();
    settings
}

#[test]
fn default_and_protocol_defaults_are_backward_compatible() {
    assert_eq!(Profile::default().warp_dns, WarpDnsSettings::default());
    assert!(!WarpDnsSettings::default().is_encrypted());
    assert!(WarpDnsSettings::default().encrypted_settings().is_none());
    for (mode, direct_mode, port, path) in [
        (WarpDnsMode::Doh, DirectDnsMode::Doh, 443, "/dns-query"),
        (WarpDnsMode::Dot, DirectDnsMode::Dot, 853, ""),
    ] {
        let settings = encrypted(mode);
        assert_eq!(settings.server_name, "dns.example.com");
        assert_eq!(settings.port, port);
        assert_eq!(settings.doh_path, path);
        assert!(settings.is_encrypted());
        assert_eq!(settings.encrypted_settings().unwrap().mode, direct_mode);
        assert_eq!(settings.validate(), Ok(()));
        let restored: WarpDnsSettings =
            serde_json::from_value(serde_json::to_value(&settings).unwrap()).unwrap();
        assert_eq!(settings, restored);
    }
    assert!(
        serde_json::from_value::<WarpDnsSettings>(serde_json::json!({
            "mode": "unknown"
        }))
        .is_err()
    );
}

#[test]
fn validation_uses_stable_warp_codes_and_encrypted_dns_restrictions() {
    let valid = encrypted(WarpDnsMode::Doh);
    let mut cases = Vec::new();
    let mut settings = valid.clone();
    settings.server_name = "https://dns.example.com".into();
    cases.push((settings, "WARP_DNS_SERVER_NAME_INVALID"));
    let mut settings = valid.clone();
    settings.port = 0;
    cases.push((settings, "WARP_DNS_PORT_INVALID"));
    let mut settings = valid.clone();
    settings.bootstrap_ips = vec!["192.0.2.53".parse().unwrap(); 9];
    cases.push((settings, "WARP_DNS_BOOTSTRAP_TOO_MANY"));
    let mut settings = valid.clone();
    settings.bootstrap_ips.push(settings.bootstrap_ips[0]);
    cases.push((settings, "WARP_DNS_BOOTSTRAP_DUPLICATE"));
    for address in ["0.0.0.0", "224.0.0.1", "::", "fe80::53", "ff02::1"] {
        let mut settings = valid.clone();
        settings.bootstrap_ips = vec![address.parse().unwrap()];
        cases.push((settings, "WARP_DNS_BOOTSTRAP_INVALID"));
    }
    for path in [
        "//other/dns-query",
        "/dns-query?x=1",
        "/dns-query#x",
        "/dns\r\nquery",
    ] {
        let mut settings = valid.clone();
        settings.bootstrap_ips.clear();
        settings.doh_path = path.into();
        cases.push((settings, "WARP_DNS_DOH_PATH_INVALID"));
    }
    let mut settings = valid.clone();
    settings.mode = WarpDnsMode::Dot;
    cases.push((settings, "WARP_DNS_DOT_PATH_FORBIDDEN"));
    let mut settings = valid;
    settings.mode = WarpDnsMode::Plain;
    cases.push((settings.clone(), "WARP_DNS_PLAIN_NOT_CANONICAL"));
    for (settings, code) in cases {
        assert_eq!(settings.validate().unwrap_err().stable_code(), Some(code));
    }
    settings.canonicalize();
    assert_eq!(settings, WarpDnsSettings::default());
}

#[test]
fn encrypted_warp_bootstrap_ips_are_optional_but_direct_dns_stays_explicit() {
    for mode in [WarpDnsMode::Doh, WarpDnsMode::Dot] {
        let mut settings = encrypted(mode);
        settings.bootstrap_ips.clear();
        assert_eq!(settings.validate(), Ok(()));
        assert_eq!(
            settings.encrypted_settings().unwrap().validate(),
            Err(ConfigError::MissingDirectDnsBootstrapIp)
        );
    }
}

#[test]
fn numeric_dns_remains_validated_but_dormant_overlap_does_not_block_encrypted_warp() {
    let mut profile = Profile {
        warp_dns: encrypted(WarpDnsMode::Doh),
        split_exclusions: vec!["1.1.1.1/32".parse().unwrap()],
        ..Profile::default()
    };
    profile.frontends.tunnel = true;
    assert!(profile.uses_encrypted_warp_dns());
    assert_eq!(profile.validate(), Ok(()));
    profile.dns_mode = DnsMode::System;
    assert_eq!(profile.validate(), Err(ConfigError::VpnSystemDnsForbidden));
    profile.dns_mode = DnsMode::Tunnel;
    profile.dns_servers.push(profile.dns_servers[0]);
    assert_eq!(profile.validate(), Err(ConfigError::DuplicateDnsServer));
    profile.dns_servers.pop();
    profile.dns_servers[0] = "0.0.0.0".parse().unwrap();
    assert!(matches!(
        profile.validate(),
        Err(ConfigError::InvalidVpnDnsServer(_))
    ));
    profile.dns_servers[0] = "1.1.1.1".parse().unwrap();
    profile.chain_exit = Some(crate::chain_exit::ChainExitSettings {
        enabled: true,
        source: crate::chain_exit::ChainSource::HttpProxy,
        profile_id: Some(Uuid::new_v4()),
        revision: Some(Uuid::new_v4()),
        ..Default::default()
    });
    assert!(!profile.uses_encrypted_warp_dns());
    assert!(matches!(
        profile.validate(),
        Err(ConfigError::VpnDnsServerBypassed(_))
    ));
    profile.disable_chain();
    profile.warp_dns = WarpDnsSettings::default();
    assert!(matches!(
        profile.validate(),
        Err(ConfigError::VpnDnsServerBypassed(_))
    ));
}

#[test]
fn encrypted_warp_dns_is_shared_across_accounts_and_reset_with_network_settings() {
    let mut config = AppConfig::default();
    config.network.warp_dns = encrypted(WarpDnsMode::Dot);
    let saved = config.network.warp_dns.clone();
    let id = Uuid::new_v4();
    config.insert_account(id, "Second".into(), None).unwrap();
    assert_eq!(config.runtime_profile(id).unwrap().warp_dns, saved);
    let restored: AppConfig =
        serde_json::from_str(&serde_json::to_string(&config).unwrap()).unwrap();
    assert_eq!(restored.runtime_profile(id).unwrap().warp_dns, saved);
    config.network.reset_user_defaults();
    assert_eq!(
        config.runtime_profile(id).unwrap().warp_dns,
        WarpDnsSettings::default()
    );
}
