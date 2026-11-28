use super::*;

#[test]
fn stored_subscription_bindings_keep_their_invariants() {
    let valid = AppConfig::default();
    assert!(valid.validate().is_ok());

    // Shared storage keeps only the underlying fallback, never a binding.
    let mut shared = valid.clone();
    shared.network.endpoint.selection = EndpointSelection::Custom;
    shared.network.endpoint.subscription_id = Some(uuid::Uuid::new_v4());
    assert!(matches!(
        shared.validate(),
        Err(ConfigError::InvalidEndpointSubscription)
    ));

    // A nil identifier in an inactive tier slot is corrupt.
    let mut nil_slot = valid.clone();
    nil_slot.network.consumer_tier_subscriptions.plus = Some(uuid::Uuid::nil());
    assert!(matches!(
        nil_slot.validate(),
        Err(ConfigError::InvalidEndpointSubscription)
    ));

    // Only a Zero Trust account owns a Zero Trust binding.
    let mut consumer = valid.clone();
    consumer.profiles[0].zero_trust_subscription_id = Some(uuid::Uuid::new_v4());
    assert!(matches!(
        consumer.validate(),
        Err(ConfigError::InvalidEndpointSubscription)
    ));
}

#[test]
fn a_resolved_endpoint_requires_its_binding() {
    let mut profile = Profile {
        subscription_endpoint: Some(crate::endpoint_subscriptions::SubscriptionEndpoint {
            id: "v4:162.159.193.10:2408".into(),
            host: crate::endpoint_subscriptions::SubscriptionEndpointHost::Ipv4(
                "162.159.193.10".parse().unwrap(),
            ),
            port: Some(2408),
            note: String::new(),
            pinned: false,
        }),
        ..Profile::default()
    };
    assert!(matches!(
        profile.validate(),
        Err(ConfigError::InvalidEndpointSubscription)
    ));
    profile.endpoint.selection = EndpointSelection::Custom;
    profile.endpoint.preset_region = None;
    profile.endpoint.subscription_id = Some(uuid::Uuid::new_v4());
    assert!(profile.validate().is_ok());
}
