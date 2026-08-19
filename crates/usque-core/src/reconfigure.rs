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
    /// Only the VPN/TUN frontend flag flipped.
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
        || previous.kill_switch != next.kill_switch;
    if cold {
        return ReconfigureClass::ColdReconnect;
    }

    let mut previous_proxy = previous.proxy.clone();
    let mut next_proxy = next.proxy.clone();
    previous_proxy.system_proxy = false;
    next_proxy.system_proxy = false;
    let proxy_except_system = previous_proxy == next_proxy;

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
}
