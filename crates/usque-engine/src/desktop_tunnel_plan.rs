//! Shared, append-only Agent plan construction for desktop TUN backends.
use std::net::SocketAddr;
use usque_core::{IpPolicy, Profile};
use usque_ipc::agent_v1;
use usque_transport::{MasqueTlsIdentity, SPLIT_DNS_IPV4, SPLIT_DNS_IPV6};

pub(crate) fn tunnel_plan(
    profile: &Profile,
    identity: &MasqueTlsIdentity,
    registration_api: &[SocketAddr],
    split_dns: bool,
) -> agent_v1::TunnelPlan {
    let mut plan = tunnel_plan_from_assignment(
        profile,
        identity.assigned_ipv4,
        identity.assigned_ipv6,
        registration_api,
        split_dns,
    );
    plan.defer_network_configuration = profile.chain_enabled();
    if profile.endpoint.selection == usque_core::EndpointSelection::Automatic {
        apply_automatic_observation_policy(
            &mut plan,
            profile,
            usque_core::AutomaticEndpointPolicy::for_profile(profile, identity.endpoint_pool()),
        );
    }
    if let Some(region) = profile.endpoint.preset_region {
        plan.preset_endpoint_policy = Some(preset_policy(
            profile,
            identity
                .endpoint_preset_tier()
                .ok()
                .map(|tier| region.hostname(tier))
                .unwrap_or_default(),
        ));
    }
    plan
}

pub(crate) fn tunnel_plan_from_assignment(
    profile: &Profile,
    assigned_ipv4: std::net::Ipv4Addr,
    assigned_ipv6: std::net::Ipv6Addr,
    registration_api: &[SocketAddr],
    split_dns: bool,
) -> agent_v1::TunnelPlan {
    let split_dns = split_dns
        || profile.uses_encrypted_warp_dns()
        || profile.chain_enabled()
            && (profile.dns_mode == usque_core::DnsMode::Tunnel
                || profile.custom_chain().is_some())
        || profile.data_plane == usque_core::DataPlaneMode::L4Proxy && !profile.chain_enabled();
    let automatic_policy = (profile.endpoint.selection == usque_core::EndpointSelection::Automatic)
        .then(|| {
            usque_core::AutomaticEndpointPolicy::for_profile(
                profile,
                usque_core::EndpointPool::WarpPlus,
            )
        });
    let (mut ipv4, mut ipv6) = automatic_policy.map_or_else(
        || {
            (
                profile.endpoint.ipv4_socket(),
                profile.endpoint.ipv6_socket(),
            )
        },
        |policy| automatic_observation_pair(profile, policy),
    );
    if profile.endpoint.preset_region.is_some() {
        ipv4 = SocketAddr::new(
            usque_core::config::DEFAULT_ENDPOINT_V4.into(),
            profile.endpoint.port,
        );
        ipv6 = SocketAddr::new(
            usque_core::config::DEFAULT_ENDPOINT_V6.into(),
            profile.endpoint.port,
        );
    }
    let mut endpoint = match profile.ip_policy {
        IpPolicy::PreferIpv6 | IpPolicy::Ipv6Only => ipv6,
        IpPolicy::Auto | IpPolicy::PreferIpv4 | IpPolicy::Ipv4Only => ipv4,
    };
    let mut endpoint_candidates = match profile.ip_policy {
        IpPolicy::Ipv4Only => vec![ipv4.to_string()],
        IpPolicy::Ipv6Only => vec![ipv6.to_string()],
        IpPolicy::Auto | IpPolicy::PreferIpv4 | IpPolicy::PreferIpv6 => {
            vec![ipv4.to_string(), ipv6.to_string()]
        }
    };
    if profile.endpoint.preset_region.is_some() {
        // Canonical anchors observe physical interfaces only. Preset
        // sockets require explicit, generation-scoped DNS/candidate leases.
        endpoint_candidates = vec![ipv4.to_string(), ipv6.to_string()];
    }
    if automatic_policy.is_some() {
        endpoint_candidates.retain(|candidate| {
            candidate.parse::<SocketAddr>().is_ok_and(|value| {
                !usque_transport::excludes_automatic_endpoint_dns_server(profile, value)
            })
        });
        if !endpoint_candidates.contains(&endpoint.to_string())
            && let Some(first) = endpoint_candidates.first()
        {
            endpoint = first.parse().expect("numeric anchor");
        }
    }
    agent_v1::TunnelPlan {
        profile_id: profile.id.to_string(),
        endpoint: endpoint.to_string(),
        mtu: u32::from(profile.mtu),
        // Endpoint policy selects the physical MASQUE ingress only. DNS is
        // carried inside CONNECT-IP and remains dual-stack over either ingress.
        dns_servers: if split_dns {
            [
                (!assigned_ipv4.is_unspecified()).then_some(SPLIT_DNS_IPV4.to_string()),
                (!assigned_ipv6.is_unspecified()).then_some(SPLIT_DNS_IPV6.to_string()),
            ]
            .into_iter()
            .flatten()
            .collect()
        } else {
            profile
                .dns_servers
                .iter()
                .map(ToString::to_string)
                .collect()
        },
        split_exclusions: Vec::new(),
        allow_lan: profile.allow_lan,
        kill_switch: profile.kill_switch,
        assigned_ipv4: if assigned_ipv4.is_unspecified() {
            String::new()
        } else {
            format!("{assigned_ipv4}/32")
        },
        assigned_ipv6: if assigned_ipv6.is_unspecified() {
            String::new()
        } else {
            format!("{assigned_ipv6}/128")
        },
        endpoint_candidates,
        control_api_candidates: registration_api.iter().map(ToString::to_string).collect(),
        split_dns,
        vpn_chain: profile.chain_enabled(),
        defer_network_configuration: false,
        automatic_endpoint_policy: automatic_policy.map(automatic_policy_to_proto),
        preset_endpoint_policy: None,
    }
}

pub(crate) fn automatic_observation_pair(
    profile: &Profile,
    policy: usque_core::AutomaticEndpointPolicy,
) -> (SocketAddr, SocketAddr) {
    let defaults = policy.representative_pair();
    let choose = |ipv6: bool| {
        [(199_u8, 2_u16), (199, 1)]
            .into_iter()
            .chain(
                [(198, 2), (198, 1)]
                    .into_iter()
                    .filter(|_| policy.pool == usque_core::EndpointPool::Free),
            )
            .chain((3..=255).filter(|_| policy.tcp).map(|last| (199, last)))
            .chain(std::iter::once((199, 0)).filter(|_| policy.tcp))
            .map(|(third, last)| {
                let address = if ipv6 {
                    std::net::IpAddr::V6(std::net::Ipv6Addr::new(
                        0x2606,
                        0x4700,
                        if third == 198 { 0x103 } else { 0x104 },
                        0,
                        0,
                        0,
                        0,
                        last,
                    ))
                } else {
                    std::net::IpAddr::V4(std::net::Ipv4Addr::new(
                        162,
                        159,
                        third,
                        u8::try_from(last).expect("bounded observation host"),
                    ))
                };
                SocketAddr::new(address, policy.port)
            })
            .find(|endpoint| {
                !usque_transport::excludes_automatic_endpoint_dns_server(profile, *endpoint)
            })
            .unwrap_or(if ipv6 { defaults.1 } else { defaults.0 })
    };
    (choose(false), choose(true))
}

pub(crate) fn apply_automatic_observation_policy(
    plan: &mut agent_v1::TunnelPlan,
    profile: &Profile,
    policy: usque_core::AutomaticEndpointPolicy,
) {
    let (ipv4, ipv6) = automatic_observation_pair(profile, policy);
    let mut endpoint = match profile.ip_policy {
        IpPolicy::PreferIpv6 | IpPolicy::Ipv6Only => ipv6,
        IpPolicy::Auto | IpPolicy::PreferIpv4 | IpPolicy::Ipv4Only => ipv4,
    };
    let candidates = [ipv4, ipv6]
        .into_iter()
        .filter(|candidate| {
            (candidate.is_ipv4() && policy.ipv4 || candidate.is_ipv6() && policy.ipv6)
                && !usque_transport::excludes_automatic_endpoint_dns_server(profile, *candidate)
        })
        .collect::<Vec<_>>();
    if !candidates.contains(&endpoint)
        && let Some(first) = candidates.first()
    {
        endpoint = *first;
    }
    plan.endpoint = endpoint.to_string();
    plan.endpoint_candidates = candidates
        .into_iter()
        .map(|candidate| candidate.to_string())
        .collect();
    plan.automatic_endpoint_policy = Some(automatic_policy_to_proto(policy));
}

pub(crate) fn automatic_policy_to_proto(
    policy: usque_core::AutomaticEndpointPolicy,
) -> agent_v1::AutomaticEndpointPolicy {
    agent_v1::AutomaticEndpointPolicy {
        pool: match policy.pool {
            usque_core::EndpointPool::Free => agent_v1::AutomaticEndpointPool::Free,
            usque_core::EndpointPool::WarpPlus => agent_v1::AutomaticEndpointPool::WarpPlus,
        } as i32,
        port: u32::from(policy.port),
        ipv4: policy.ipv4,
        ipv6: policy.ipv6,
        tcp: policy.tcp,
        udp: policy.udp,
    }
}

pub(crate) fn preset_policy(profile: &Profile, hostname: &str) -> agent_v1::PresetEndpointPolicy {
    agent_v1::PresetEndpointPolicy {
        hostname: hostname.to_owned(),
        port: u32::from(profile.endpoint.port),
        ipv4_fallback: if profile.ip_policy == IpPolicy::PreferIpv6 {
            profile.endpoint.ipv4_socket().to_string()
        } else {
            String::new()
        },
        tcp: profile.data_plane == usque_core::DataPlaneMode::ConnectIp
            && profile.transport != usque_core::TransportPolicy::Http3,
        udp: profile.data_plane == usque_core::DataPlaneMode::L4Proxy
            || profile.transport != usque_core::TransportPolicy::Http2,
    }
}
