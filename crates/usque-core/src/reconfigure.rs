//! Classify an in-place profile mutation so the engine can keep MASQUE when
//! only local frontends change.

use crate::config::Profile;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReconfigureClass {
    /// Profile id or identity-bound endpoint changed; refuse.
    Reject,
    /// Tear down MASQUE and reconnect with rollback.
    ColdReconnect,
    /// Only Windows system-proxy lease changes.
    HotSystemProxy,
    /// SOCKS/HTTP listeners or those frontend toggles (and proxy DNS/auth).
    HotFrontends,
    /// Only the VPN/TUN frontend flag flipped and no mode-dependent GEO policy
    /// needs to be rebuilt.
    HotTunnelAttach,
}

/// Decide how to apply `next` over the currently connected `previous` profile.
pub fn classify_reconfigure(previous: &Profile, next: &Profile) -> ReconfigureClass {
    if previous.id != next.id {
        return ReconfigureClass::Reject;
    }

    let cold = previous.transport != next.transport
        || previous.endpoint != next.endpoint
        || previous.ip_policy != next.ip_policy
        || previous.mtu != next.mtu
        || previous.dns_mode != next.dns_mode
        || previous.dns_servers != next.dns_servers
        || previous.allow_lan != next.allow_lan
        || previous.split_exclusions != next.split_exclusions
        || previous.kill_switch != next.kill_switch
        || previous.geo_direct_countries != next.geo_direct_countries
        || previous.direct_dns != next.direct_dns
        || previous.frontends.tunnel != next.frontends.tunnel
            && (!previous.geo_direct_countries.is_empty() || !next.geo_direct_countries.is_empty());
    if cold {
        return ReconfigureClass::ColdReconnect;
    }

    let proxy_except_system = proxy_equal_except_system(&previous.proxy, &next.proxy);

    let socks_http_frontends = previous.frontends.socks5 == next.frontends.socks5
        && previous.frontends.http == next.frontends.http;
    let tunnel_same = previous.frontends.tunnel == next.frontends.tunnel;
    let system_proxy_same = previous.proxy.system_proxy == next.proxy.system_proxy;

    if tunnel_same && socks_http_frontends && proxy_except_system && !system_proxy_same {
        return ReconfigureClass::HotSystemProxy;
    }

    if tunnel_same && system_proxy_same && (!socks_http_frontends || !proxy_except_system) {
        return ReconfigureClass::HotFrontends;
    }

    if !tunnel_same && socks_http_frontends && proxy_except_system && system_proxy_same {
        return ReconfigureClass::HotTunnelAttach;
    }

    if previous.frontends == next.frontends && previous.proxy == next.proxy {
        return ReconfigureClass::ColdReconnect;
    }

    ReconfigureClass::ColdReconnect
}

fn proxy_equal_except_system(
    previous: &crate::config::ProxySettings,
    next: &crate::config::ProxySettings,
) -> bool {
    previous.socks5_listeners == next.socks5_listeners
        && previous.http_listeners == next.http_listeners
        && previous.udp_idle_timeout_seconds == next.udp_idle_timeout_seconds
        && previous.dns_mode == next.dns_mode
        && previous.dns_servers == next.dns_servers
        && previous.auth_username == next.auth_username
        && previous.auth_password == next.auth_password
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{FrontendSettings, Profile};

    fn base() -> Profile {
        Profile::default()
    }

    #[test]
    fn socks_port_change_is_hot_frontends() {
        let previous = base();
        let mut next = previous.clone();
        next.proxy.socks5_listeners[0].set_port(1081);
        assert_eq!(
            classify_reconfigure(&previous, &next),
            ReconfigureClass::HotFrontends
        );
    }

    #[test]
    fn system_proxy_only_is_hot_system_proxy() {
        let previous = base();
        let mut next = previous.clone();
        next.proxy.system_proxy = !previous.proxy.system_proxy;
        assert_eq!(
            classify_reconfigure(&previous, &next),
            ReconfigureClass::HotSystemProxy
        );
    }

    #[test]
    fn tunnel_only_flip_is_attach_detach() {
        let previous = base();
        let mut next = previous.clone();
        next.frontends = FrontendSettings {
            tunnel: !previous.frontends.tunnel,
            socks5: previous.frontends.socks5,
            http: previous.frontends.http,
        };
        assert_eq!(
            classify_reconfigure(&previous, &next),
            ReconfigureClass::HotTunnelAttach
        );
    }

    #[test]
    fn endpoint_or_mtu_still_reconnects() {
        let previous = base();
        let mut next = previous.clone();
        next.mtu = 1400;
        assert_eq!(
            classify_reconfigure(&previous, &next),
            ReconfigureClass::ColdReconnect
        );
        next = previous.clone();
        next.endpoint.port = 8443;
        assert_eq!(
            classify_reconfigure(&previous, &next),
            ReconfigureClass::ColdReconnect
        );
    }

    #[test]
    fn different_profile_id_is_rejected() {
        let previous = base();
        let mut next = previous.clone();
        next.id = uuid::Uuid::from_u128(2);
        assert_eq!(
            classify_reconfigure(&previous, &next),
            ReconfigureClass::Reject
        );
    }

    #[test]
    fn geo_direct_country_list_change_is_cold_reconnect() {
        let previous = base();
        let mut next = previous.clone();
        next.geo_direct_countries = vec!["CN".to_owned()];
        assert_eq!(
            classify_reconfigure(&previous, &next),
            ReconfigureClass::ColdReconnect
        );
        next.proxy.socks5_listeners[0].set_port(1081);
        assert_eq!(
            classify_reconfigure(&previous, &next),
            ReconfigureClass::ColdReconnect
        );
        let unchanged = previous.clone();
        assert_ne!(
            classify_reconfigure(&previous, &unchanged),
            ReconfigureClass::Reject
        );
        let socks_only = {
            let mut profile = previous.clone();
            profile.proxy.socks5_listeners[0].set_port(1081);
            profile
        };
        assert_eq!(
            classify_reconfigure(&previous, &socks_only),
            ReconfigureClass::HotFrontends
        );
    }

    #[test]
    fn geo_direct_tunnel_toggle_is_cold_reconnect() {
        let mut proxy_only = base();
        proxy_only.frontends.tunnel = false;
        proxy_only.geo_direct_countries = vec!["CN".to_owned()];
        let mut vpn = proxy_only.clone();
        vpn.frontends.tunnel = true;

        assert_eq!(
            classify_reconfigure(&proxy_only, &vpn),
            ReconfigureClass::ColdReconnect
        );
        assert_eq!(
            classify_reconfigure(&vpn, &proxy_only),
            ReconfigureClass::ColdReconnect
        );
    }

    #[test]
    fn direct_dns_change_is_a_cold_reconnect() {
        let previous = base();
        let mut next = previous.clone();
        next.direct_dns.mode = crate::config::DirectDnsMode::Doh;
        next.direct_dns.server_name = "dns.example.com".to_owned();
        next.direct_dns.doh_path = "/dns-query".to_owned();
        next.direct_dns.bootstrap_ips = vec!["192.0.2.53".parse().unwrap()];
        next.direct_dns.port = 443;
        assert_eq!(
            classify_reconfigure(&previous, &next),
            ReconfigureClass::ColdReconnect
        );
    }
}
