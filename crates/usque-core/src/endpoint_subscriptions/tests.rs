use super::*;
use tempfile::TempDir;

#[test]
fn parser_handles_ipv4_ipv6_domain_notes_comments_and_dedup() {
    let body = "\u{feff}# header comment\n\
        // another comment\n\
        \n\
        162.159.193.10:443@WARP-IPv4-US\n\
        162.159.193.10:443@Duplicate Should Skip\n\
        162.159.193.11:2408#Hash Remark\n\
        162.159.193.12\n\
        [2606:4700:103::2]:443@IPv6 Bracketed\n\
        2606:4700:104::2@IPv6 Bare With Note\n\
        edge.example.com:8443@Domain Edge\n\
        127.0.0.1:443@Loopback Bad\n\
        10.0.0.1:443@Private Bad\n\
        192.168.1.1:443@Private Bad\n\
        100.64.0.1:443@CGNAT Bad\n\
        169.254.1.1:443@LinkLocal Bad\n\
        [::1]:443@V6 Loopback Bad\n\
        [fe80::1]:443@V6 LinkLocal Bad\n\
        [fc00::1]:443@V6 ULA Bad\n\
        [::ffff:162.159.193.10]:443@V4 Mapped Bad\n\
        localhost:443@Localhost Bad\n\
        printer.local:443@mDNS Bad\n\
        bad.host:0@Zero Port Bad\n\
        http://162.159.193.10:443@Scheme Bad\n";

    let parsed = parse_subscription_body(body.as_bytes()).unwrap();
    assert_eq!(parsed.endpoints.len(), 6);
    assert_eq!(parsed.skipped_line_count, 14);

    assert_eq!(
        parsed.endpoints[0].host,
        SubscriptionEndpointHost::Ipv4("162.159.193.10".parse().unwrap())
    );
    assert_eq!(parsed.endpoints[0].port, Some(443));
    assert_eq!(parsed.endpoints[0].note, "WARP-IPv4-US");

    assert_eq!(parsed.endpoints[1].port, Some(2408));
    assert_eq!(parsed.endpoints[1].note, "Hash Remark");

    assert_eq!(parsed.endpoints[2].port, None);
    assert_eq!(parsed.endpoints[2].note, "");

    assert_eq!(
        parsed.endpoints[3].host,
        SubscriptionEndpointHost::Ipv6("2606:4700:103::2".parse().unwrap())
    );
    assert_eq!(parsed.endpoints[3].port, Some(443));

    assert_eq!(
        parsed.endpoints[4].host,
        SubscriptionEndpointHost::Ipv6("2606:4700:104::2".parse().unwrap())
    );
    assert_eq!(parsed.endpoints[4].port, None);
    assert_eq!(parsed.endpoints[4].note, "IPv6 Bare With Note");

    assert_eq!(
        parsed.endpoints[5].host,
        SubscriptionEndpointHost::Domain("edge.example.com".into())
    );
    assert_eq!(parsed.endpoints[5].port, Some(8443));
}

#[test]
fn parser_rejects_empty_or_all_invalid_and_strips_bidi() {
    assert_eq!(
        parse_subscription_body(b"# comment\n127.0.0.1:443\n").unwrap_err(),
        SubscriptionError::Empty
    );
    let body = "162.159.193.10:443@\u{202E}evil\u{2066}\n\x07";
    let parsed = parse_subscription_body(body.as_bytes()).unwrap();
    assert_eq!(parsed.endpoints[0].note, "evil");
}

#[test]
fn url_validation_enforces_https_public_host_and_bounds() {
    assert!(
        validate_subscription_url("https://masque.pages.dev/endpoint/3rd/warp-ipv4.txt").is_ok()
    );
    assert!(validate_subscription_url("https://masque.pages.dev/endpoint/3rd/zt-ipv4.txt").is_ok());
    assert!(validate_subscription_url("http://masque.pages.dev/a.txt").is_err());
    assert!(validate_subscription_url("https://user:pass@masque.pages.dev/a.txt").is_err());
    assert!(validate_subscription_url("https://masque.pages.dev/a.txt#frag").is_err());
    assert!(validate_subscription_url("https://127.0.0.1/a.txt").is_err());
    assert!(validate_subscription_url("https://192.168.1.1/a.txt").is_err());
    assert!(validate_subscription_url("https://[::1]/a.txt").is_err());
    assert!(validate_subscription_url("https://localhost/a.txt").is_err());
    assert!(validate_subscription_url("https://sub.local/a.txt").is_err());
    assert!(validate_subscription_url("https://service.internal/a.txt").is_err());
    assert!(
        validate_subscription_url(&format!("https://example.com/{}", "a".repeat(2050))).is_err()
    );
    // Within the raw bound, but percent-encoding expands it beyond the limit.
    let expanding = format!("https://example.com/{}", "\u{00e9}".repeat(600));
    assert!(expanding.len() <= MAX_SUBSCRIPTION_URL_BYTES);
    assert!(validate_subscription_url(&expanding).is_err());
    assert_eq!(
        resolve_subscription_redirect_url(
            "https://masque.pages.dev/endpoint/list.txt",
            "/v2/list.txt"
        ),
        Ok("https://masque.pages.dev/v2/list.txt".to_owned())
    );
    assert!(
        resolve_subscription_redirect_url(
            "https://masque.pages.dev/list.txt",
            "http://masque.pages.dev/list.txt"
        )
        .is_err()
    );
    assert!(
        resolve_subscription_redirect_url(
            "https://masque.pages.dev/list.txt",
            "https://127.0.0.1/internal.txt"
        )
        .is_err()
    );
}

#[test]
fn special_purpose_ipv6_ranges_are_not_public_endpoints() {
    for address in [
        "2001:2::1",
        "2001:0:4136:e378::1",
        "2001:20::1",
        "3fff::1",
        "3fff:fff::1",
        "2001:db8::1",
    ] {
        assert!(
            !is_usable_public_endpoint_ip(address.parse().unwrap()),
            "{address}"
        );
    }
    for address in ["2606:4700:103::2", "2001:2:1::1", "3fff:1000::1"] {
        assert!(
            is_usable_public_endpoint_ip(address.parse().unwrap()),
            "{address}"
        );
    }
}

#[test]
fn reconcile_preserves_pins_and_retains_removed_active_endpoint() {
    let initial =
        parse_subscription_body(b"162.159.193.10:443@Node A\n162.159.193.11:443@Node B\n").unwrap();
    let mut sub = EndpointSubscription {
        id: Uuid::new_v4(),
        revision: Uuid::new_v4(),
        name: "Test".into(),
        url: "https://masque.pages.dev/endpoint/3rd/warp-ipv4.txt".into(),
        account_tier: EndpointPresetTier::Free,
        auto_refresh: SubscriptionRefreshInterval::Daily,
        last_refreshed_at_unix_ms: Some(1_000),
        next_refresh_at_unix_ms: None,
        last_refresh_error: Some("SUBSCRIPTION_TIMEOUT".into()),
        skipped_line_count: 0,
        endpoints: initial.endpoints,
        selected_endpoint_id: Some("v4:162.159.193.10:443".into()),
        retained_missing_endpoint: None,
    };
    sub.endpoints[1].pinned = true;

    let updated =
        parse_subscription_body(b"162.159.193.11:443@Node B New\n162.159.193.12:443@Node C\n")
            .unwrap();
    sub.reconcile_endpoints(updated, 2_000);

    // Selected endpoint removed upstream -> retained fail-closed, never silently switched
    assert_eq!(
        sub.selected_endpoint_id.as_deref(),
        Some("v4:162.159.193.10:443")
    );
    let (active, removed) = sub.active_endpoint().unwrap();
    assert!(removed);
    assert_eq!(active.note, "Node A");
    assert!(sub.endpoints[0].pinned);
    assert_eq!(sub.endpoints[0].note, "Node B New");
    assert_eq!(sub.last_refresh_error, None);
    assert_eq!(
        sub.next_refresh_at_unix_ms,
        Some(2_000 + SubscriptionRefreshInterval::Daily.interval_ms().unwrap())
    );
}

#[test]
fn store_persists_atomically_and_rejects_unknown_command_fields() {
    let dir = TempDir::new().unwrap();
    let store = EndpointSubscriptionStore::in_directory(dir.path());
    assert!(store.load_or_default().unwrap().subscriptions.is_empty());

    let parsed = parse_subscription_body(b"162.159.193.10:443@Node A\n").unwrap();
    let sub_id = Uuid::new_v4();
    store
        .update::<_, StoreError>(|catalog| {
            catalog.subscriptions.push(EndpointSubscription {
                id: sub_id,
                revision: Uuid::new_v4(),
                name: "WARP".into(),
                url: "https://masque.pages.dev/endpoint/3rd/warp-ipv4.txt".into(),
                account_tier: EndpointPresetTier::Free,
                auto_refresh: SubscriptionRefreshInterval::Off,
                last_refreshed_at_unix_ms: Some(100),
                next_refresh_at_unix_ms: None,
                last_refresh_error: None,
                skipped_line_count: 0,
                endpoints: parsed.endpoints,
                selected_endpoint_id: Some("v4:162.159.193.10:443".into()),
                retained_missing_endpoint: None,
            });
            Ok(())
        })
        .unwrap();

    let reloaded = store.load_or_default().unwrap();
    assert_eq!(reloaded.subscriptions.len(), 1);
    assert_eq!(reloaded.subscriptions[0].id, sub_id);

    assert!(EndpointSubscriptionCommand::parse(r#"{"action":"list","extra":1}"#).is_err());
    assert!(EndpointSubscriptionCommand::parse(r#"{"action":"list"}"#).is_ok());

    store.clear().unwrap();
    assert!(store.load_or_default().unwrap().subscriptions.is_empty());
}
