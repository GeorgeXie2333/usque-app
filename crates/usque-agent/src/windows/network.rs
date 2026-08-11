//! Audited Windows IP Helper mutations used by the privileged Agent.
//!
//! Every public mutating function consumes a write-ahead receipt. Recovery
//! therefore names exact routes, addresses, interface values, and DNS state
//! instead of trying to infer what a crashed process may have changed.

use std::{
    collections::BTreeSet,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    ptr,
};

use ipnet::{IpNet, Ipv4Net, Ipv6Net};
use thiserror::Error;
use uuid::Uuid;
use windows_sys::{
    Win32::{
        Foundation::{
            ERROR_ALREADY_EXISTS, ERROR_FILE_NOT_FOUND, ERROR_HOST_UNREACHABLE,
            ERROR_NETWORK_UNREACHABLE, ERROR_NO_NETWORK, ERROR_NOT_FOUND, ERROR_NOT_SUPPORTED,
            ERROR_OBJECT_ALREADY_EXISTS, ERROR_PROTOCOL_UNREACHABLE, NO_ERROR, WIN32_ERROR,
        },
        NetworkManagement::{
            IpHelper::{
                CreateIpForwardEntry2, CreateUnicastIpAddressEntry, DNS_INTERFACE_SETTINGS,
                DNS_INTERFACE_SETTINGS_VERSION1, DNS_SETTING_IPV6, DNS_SETTING_NAMESERVER,
                DeleteIpForwardEntry2, DeleteUnicastIpAddressEntry, FreeInterfaceDnsSettings,
                GetBestInterfaceEx, GetBestRoute2, GetInterfaceDnsSettings, GetIpForwardEntry2,
                GetIpInterfaceEntry, GetUnicastIpAddressEntry, IP_ADDRESS_PREFIX,
                InitializeIpForwardEntry, InitializeIpInterfaceEntry,
                InitializeUnicastIpAddressEntry, MIB_IPFORWARD_ROW2, MIB_IPINTERFACE_ROW,
                MIB_UNICASTIPADDRESS_ROW, SetInterfaceDnsSettings, SetIpInterfaceEntry,
            },
            Ndis::NET_LUID_LH,
        },
        Networking::WinSock::{
            ADDRESS_FAMILY, AF_INET, AF_INET6, IN_ADDR, IN_ADDR_0, IN_ADDR_0_0, IN6_ADDR,
            IN6_ADDR_0, IpDadStatePreferred, IpPrefixOriginManual, IpSuffixOriginManual,
            MIB_IPPROTO_NETMGMT, SOCKADDR, SOCKADDR_IN, SOCKADDR_IN6, SOCKADDR_IN6_0,
            SOCKADDR_INET,
        },
    },
    core::GUID,
};

use crate::{
    journal::{AddressReceipt, MutationReceipt, RouteReceipt},
    plan::ValidatedTunnelPlan,
};

const MAX_DNS_TEXT_UNITS: usize = 16 * 1024;
const DEFAULT_ROUTE_METRIC: u32 = 0;

pub fn plan_endpoint_bypass(
    endpoint_candidates: &[SocketAddr],
    control_api_candidates: &[SocketAddr],
) -> Result<MutationReceipt, NetworkError> {
    plan_endpoint_bypass_with(
        endpoint_candidates,
        control_api_candidates,
        plan_physical_route,
    )
}

fn plan_endpoint_bypass_with(
    endpoint_candidates: &[SocketAddr],
    control_api_candidates: &[SocketAddr],
    mut lookup: impl FnMut(IpNet) -> Result<RouteReceipt, NetworkError>,
) -> Result<MutationReceipt, NetworkError> {
    let endpoint_hosts = endpoint_candidates
        .iter()
        .map(SocketAddr::ip)
        .collect::<BTreeSet<_>>();
    let control_hosts = control_api_candidates
        .iter()
        .map(SocketAddr::ip)
        .collect::<BTreeSet<_>>();
    let all_hosts = endpoint_hosts
        .union(&control_hosts)
        .copied()
        .collect::<BTreeSet<_>>();
    let mut reachable_hosts = BTreeSet::new();
    let mut created = Vec::new();

    for host in all_hosts {
        let destination = host_network(host);
        if let Some(receipt) = plan_candidate_route(destination, &mut lookup)? {
            reachable_hosts.insert(host);
            created.push(receipt);
        }
    }

    if endpoint_hosts.is_disjoint(&reachable_hosts) {
        return Err(NetworkError::NoReachableEndpoint);
    }
    if control_hosts.is_disjoint(&reachable_hosts) {
        return Err(NetworkError::NoReachableControlApi);
    }
    Ok(MutationReceipt::EndpointBypass { created })
}

fn plan_candidate_route(
    destination: IpNet,
    lookup: &mut impl FnMut(IpNet) -> Result<RouteReceipt, NetworkError>,
) -> Result<Option<RouteReceipt>, NetworkError> {
    match lookup(destination) {
        Ok(receipt) => Ok(Some(receipt)),
        Err(error) if error.is_interface_churn() => match lookup(destination) {
            Ok(receipt) => Ok(Some(receipt)),
            Err(retry_error)
                if retry_error.is_interface_churn() || retry_error.is_candidate_unavailable() =>
            {
                Ok(None)
            }
            Err(retry_error) => Err(retry_error),
        },
        Err(error) if error.is_candidate_unavailable() => Ok(None),
        Err(error) => Err(error),
    }
}

pub fn apply_endpoint_bypass(
    mut receipt: MutationReceipt,
) -> Result<MutationReceipt, NetworkError> {
    let MutationReceipt::EndpointBypass { created } = &mut receipt else {
        return Err(NetworkError::ReceiptKind("endpoint bypass"));
    };
    for route in created {
        apply_route(route)?;
    }
    Ok(receipt)
}

pub fn restore_endpoint_bypass(receipt: &MutationReceipt) -> Result<(), NetworkError> {
    let MutationReceipt::EndpointBypass { created } = receipt else {
        return Err(NetworkError::ReceiptKind("endpoint bypass"));
    };
    let mut first_error = None;
    for route in created.iter().rev() {
        retain_first_error(&mut first_error, restore_route(route));
    }
    first_error.map_or(Ok(()), Err)
}

pub fn plan_interface_configuration(
    interface_luid: u64,
    plan: &ValidatedTunnelPlan,
) -> Result<MutationReceipt, NetworkError> {
    require_luid(interface_luid)?;
    let previous_ipv4_mtu = plan
        .assigned_ipv4
        .as_ref()
        .map(|_| interface_mtu(interface_luid, AF_INET))
        .transpose()?;
    let previous_ipv6_mtu = plan
        .assigned_ipv6
        .as_ref()
        .map(|_| interface_mtu(interface_luid, AF_INET6))
        .transpose()?;
    let created_addresses = plan
        .assigned_ipv4
        .iter()
        .chain(plan.assigned_ipv6.iter())
        .map(|network| {
            let owned = !address_exists(interface_luid, network.addr())?;
            Ok(AddressReceipt {
                address: network.to_string(),
                owned,
            })
        })
        .collect::<Result<Vec<_>, NetworkError>>()?;
    Ok(MutationReceipt::InterfaceConfiguration {
        interface_luid,
        previous_ipv4_mtu,
        previous_ipv6_mtu,
        created_addresses,
    })
}

pub fn apply_interface_configuration(
    mut receipt: MutationReceipt,
    plan: &ValidatedTunnelPlan,
) -> Result<MutationReceipt, NetworkError> {
    let MutationReceipt::InterfaceConfiguration {
        interface_luid,
        previous_ipv4_mtu: _,
        previous_ipv6_mtu: _,
        created_addresses,
    } = &mut receipt
    else {
        return Err(NetworkError::ReceiptKind("interface configuration"));
    };
    require_luid(*interface_luid)?;
    // Create addresses before changing per-family MTU, matching the ordering
    // used by the audited Wintun/WireGuard implementations. The write-ahead
    // receipt is conservative, and recovery queries each address before an
    // idempotent delete if any later operation fails.
    for address in created_addresses {
        if !address.owned {
            continue;
        }
        let network = parse_network(&address.address)?;
        match create_address(*interface_luid, network) {
            Ok(()) => {}
            Err(NetworkError::Windows { code, .. })
                if code == ERROR_OBJECT_ALREADY_EXISTS || code == ERROR_ALREADY_EXISTS =>
            {
                address.owned = false;
            }
            Err(error) => return Err(error),
        }
    }
    if plan.assigned_ipv4.is_some() {
        set_interface_mtu(*interface_luid, AF_INET, u32::from(plan.mtu))?;
    }
    if plan.assigned_ipv6.is_some() {
        set_interface_mtu(*interface_luid, AF_INET6, u32::from(plan.mtu))?;
    }
    Ok(receipt)
}

pub fn restore_interface_configuration(receipt: &MutationReceipt) -> Result<(), NetworkError> {
    let MutationReceipt::InterfaceConfiguration {
        interface_luid,
        previous_ipv4_mtu,
        previous_ipv6_mtu,
        created_addresses,
    } = receipt
    else {
        return Err(NetworkError::ReceiptKind("interface configuration"));
    };
    require_luid(*interface_luid)?;
    let mut first_error = None;
    for address in created_addresses.iter().rev().filter(|entry| entry.owned) {
        let result = parse_network(&address.address)
            .and_then(|network| delete_address(*interface_luid, network));
        retain_first_error(&mut first_error, result);
    }
    if let Some(mtu) = previous_ipv6_mtu {
        retain_first_error(
            &mut first_error,
            set_interface_mtu(*interface_luid, AF_INET6, *mtu),
        );
    }
    if let Some(mtu) = previous_ipv4_mtu {
        retain_first_error(
            &mut first_error,
            set_interface_mtu(*interface_luid, AF_INET, *mtu),
        );
    }
    first_error.map_or(Ok(()), Err)
}

pub fn plan_dns(
    interface_luid: u64,
    _plan: &ValidatedTunnelPlan,
) -> Result<MutationReceipt, NetworkError> {
    let interface_guid = interface_guid(interface_luid)?;
    let previous_servers = get_dns_servers(interface_guid)?;
    Ok(MutationReceipt::Dns {
        interface_guid,
        previous_automatic: previous_servers.is_empty(),
        previous_servers,
    })
}

pub fn apply_dns(
    receipt: MutationReceipt,
    plan: &ValidatedTunnelPlan,
) -> Result<MutationReceipt, NetworkError> {
    let MutationReceipt::Dns { interface_guid, .. } = &receipt else {
        return Err(NetworkError::ReceiptKind("DNS"));
    };
    set_dns_servers(*interface_guid, &plan.dns_servers)?;
    Ok(receipt)
}

pub fn restore_dns(receipt: &MutationReceipt) -> Result<(), NetworkError> {
    let MutationReceipt::Dns {
        interface_guid,
        previous_automatic,
        previous_servers,
    } = receipt
    else {
        return Err(NetworkError::ReceiptKind("DNS"));
    };
    if *previous_automatic {
        set_dns_servers(*interface_guid, &[])
    } else {
        set_dns_servers(*interface_guid, previous_servers)
    }
}

pub fn plan_default_routes(
    interface_luid: u64,
    plan: &ValidatedTunnelPlan,
) -> Result<MutationReceipt, NetworkError> {
    require_luid(interface_luid)?;
    let mut routes = Vec::new();
    if plan.assigned_ipv4.is_some() {
        routes.push(plan_route(
            "0.0.0.0/0".parse().expect("static IPv4 default"),
            None,
            0,
            interface_luid,
            DEFAULT_ROUTE_METRIC,
        )?);
    }
    if plan.assigned_ipv6.is_some() {
        routes.push(plan_route(
            "::/0".parse().expect("static IPv6 default"),
            None,
            0,
            interface_luid,
            DEFAULT_ROUTE_METRIC,
        )?);
    }

    let mut exclusions = BTreeSet::new();
    exclusions.extend(plan.split_exclusions.iter().copied());
    for exclusion in exclusions {
        if family_is_available(exclusion, plan) {
            routes.push(plan_physical_route(exclusion)?);
        }
    }
    Ok(MutationReceipt::DefaultRoutes {
        created: routes,
        replaced: Vec::new(),
    })
}

pub fn apply_default_routes(mut receipt: MutationReceipt) -> Result<MutationReceipt, NetworkError> {
    let MutationReceipt::DefaultRoutes { created, .. } = &mut receipt else {
        return Err(NetworkError::ReceiptKind("default routes"));
    };
    for route in created {
        apply_route(route)?;
    }
    Ok(receipt)
}

pub fn restore_default_routes(receipt: &MutationReceipt) -> Result<(), NetworkError> {
    let MutationReceipt::DefaultRoutes { created, replaced } = receipt else {
        return Err(NetworkError::ReceiptKind("default routes"));
    };
    let mut first_error = None;
    for route in created.iter().rev() {
        retain_first_error(&mut first_error, restore_route(route));
    }
    for route in replaced {
        retain_first_error(&mut first_error, create_route(route));
    }
    first_error.map_or(Ok(()), Err)
}

fn family_is_available(network: IpNet, plan: &ValidatedTunnelPlan) -> bool {
    match network {
        IpNet::V4(_) => plan.assigned_ipv4.is_some(),
        IpNet::V6(_) => plan.assigned_ipv6.is_some(),
    }
}

fn plan_physical_route(destination: IpNet) -> Result<RouteReceipt, NetworkError> {
    let lookup = representative_ip(destination);
    let best = best_route(lookup)?;
    let (next_hop_address, next_hop_scope_id) = ip_from_sockaddr(&best.NextHop)?;
    let next_hop = if next_hop_address.is_unspecified() {
        None
    } else {
        Some(next_hop_address)
    };
    plan_route(
        destination,
        next_hop,
        next_hop_scope_id,
        luid_value(best.InterfaceLuid),
        best.Metric,
    )
}

fn plan_route(
    destination: IpNet,
    next_hop: Option<IpAddr>,
    next_hop_scope_id: u32,
    interface_luid: u64,
    metric: u32,
) -> Result<RouteReceipt, NetworkError> {
    require_luid(interface_luid)?;
    if next_hop.is_some_and(|value| value.is_ipv4() != destination.addr().is_ipv4()) {
        return Err(NetworkError::AddressFamily);
    }
    let mut receipt = RouteReceipt {
        destination: destination.to_string(),
        next_hop,
        next_hop_scope_id,
        interface_luid,
        metric,
        owned: true,
    };
    receipt.owned = !route_exists(&receipt)?;
    Ok(receipt)
}

fn apply_route(receipt: &mut RouteReceipt) -> Result<(), NetworkError> {
    if !receipt.owned {
        return Ok(());
    }
    match create_route(receipt) {
        Ok(()) => Ok(()),
        Err(NetworkError::Windows { code, .. })
            if code == ERROR_OBJECT_ALREADY_EXISTS || code == ERROR_ALREADY_EXISTS =>
        {
            receipt.owned = false;
            Ok(())
        }
        Err(error) => Err(error),
    }
}

fn create_route(receipt: &RouteReceipt) -> Result<(), NetworkError> {
    let row = route_row(receipt)?;
    // SAFETY: `row` is fully initialized and remains valid for the call.
    check("CreateIpForwardEntry2", unsafe {
        CreateIpForwardEntry2(&row)
    })
}

fn restore_route(receipt: &RouteReceipt) -> Result<(), NetworkError> {
    if !receipt.owned {
        return Ok(());
    }
    let row = route_row(receipt)?;
    // SAFETY: `row` is fully initialized and remains valid for the call.
    let status = unsafe { DeleteIpForwardEntry2(&row) };
    if status == NO_ERROR || status == ERROR_NOT_FOUND || status == ERROR_FILE_NOT_FOUND {
        Ok(())
    } else {
        Err(NetworkError::windows("DeleteIpForwardEntry2", status))
    }
}

fn route_exists(receipt: &RouteReceipt) -> Result<bool, NetworkError> {
    let mut row = route_row(receipt)?;
    // SAFETY: `row` contains the route key and is writable for API output.
    let status = unsafe { GetIpForwardEntry2(&mut row) };
    if status == NO_ERROR {
        Ok(true)
    } else if status == ERROR_NOT_FOUND || status == ERROR_FILE_NOT_FOUND {
        Ok(false)
    } else {
        Err(NetworkError::windows("GetIpForwardEntry2", status))
    }
}

fn route_row(receipt: &RouteReceipt) -> Result<MIB_IPFORWARD_ROW2, NetworkError> {
    let destination = parse_network(&receipt.destination)?;
    require_luid(receipt.interface_luid)?;
    if receipt
        .next_hop
        .is_some_and(|value| value.is_ipv4() != destination.addr().is_ipv4())
    {
        return Err(NetworkError::AddressFamily);
    }
    let mut row = MIB_IPFORWARD_ROW2::default();
    // SAFETY: the API only writes default scalar values into `row`.
    unsafe { InitializeIpForwardEntry(&mut row) };
    row.InterfaceLuid = luid(receipt.interface_luid);
    row.DestinationPrefix = prefix_from_network(destination);
    row.NextHop = sockaddr_from_ip_scoped(
        receipt.next_hop.unwrap_or_else(|| unspecified(destination)),
        receipt.next_hop_scope_id,
    );
    row.SitePrefixLength = destination.prefix_len();
    row.Metric = receipt.metric;
    row.Protocol = MIB_IPPROTO_NETMGMT;
    Ok(row)
}

fn best_route(destination: IpAddr) -> Result<MIB_IPFORWARD_ROW2, NetworkError> {
    let destination = sockaddr_from_ip(destination);
    let mut interface_index = 0_u32;
    // SAFETY: SOCKADDR_INET begins with the same family field as SOCKADDR and
    // remains alive for the call.
    check("GetBestInterfaceEx", unsafe {
        GetBestInterfaceEx(
            (&raw const destination).cast::<SOCKADDR>(),
            &mut interface_index,
        )
    })?;
    let mut interface_luid = NET_LUID_LH::default();
    // `ConvertInterfaceIndexToLuid` is represented in windows-sys but keeping
    // this import local makes the route discovery dependency explicit.
    // SAFETY: output storage is valid.
    check("ConvertInterfaceIndexToLuid", unsafe {
        windows_sys::Win32::NetworkManagement::IpHelper::ConvertInterfaceIndexToLuid(
            interface_index,
            &mut interface_luid,
        )
    })?;
    let mut route = MIB_IPFORWARD_ROW2::default();
    let mut source = SOCKADDR_INET::default();
    // SAFETY: all input/output pointers are valid for the duration of the call.
    check("GetBestRoute2", unsafe {
        GetBestRoute2(
            &interface_luid,
            0,
            ptr::null(),
            &destination,
            0,
            &mut route,
            &mut source,
        )
    })?;
    Ok(route)
}

fn interface_mtu(interface_luid: u64, family: ADDRESS_FAMILY) -> Result<u32, NetworkError> {
    let row = get_interface(interface_luid, family)?;
    Ok(row.NlMtu)
}

fn set_interface_mtu(
    interface_luid: u64,
    family: ADDRESS_FAMILY,
    mtu: u32,
) -> Result<(), NetworkError> {
    match set_interface_mtu_once(interface_luid, family, mtu) {
        Err(error) if error.is_interface_churn() => {
            set_interface_mtu_once(interface_luid, family, mtu)
        }
        result => result,
    }
}

fn set_interface_mtu_once(
    interface_luid: u64,
    family: ADDRESS_FAMILY,
    mtu: u32,
) -> Result<(), NetworkError> {
    let mut row = get_interface(interface_luid, family)?;
    prepare_mtu_update(&mut row, family, mtu);
    // SAFETY: `row` was populated by GetIpInterfaceEntry for this interface.
    check("SetIpInterfaceEntry", unsafe {
        SetIpInterfaceEntry(&mut row)
    })
}

fn prepare_mtu_update(row: &mut MIB_IPINTERFACE_ROW, family: ADDRESS_FAMILY, mtu: u32) {
    row.NlMtu = mtu;
    if family == AF_INET {
        // SetIpInterfaceEntry rejects an IPv4 row whose SitePrefixLength is
        // non-zero, even when that value came from GetIpInterfaceEntry.
        // Microsoft documents this field as non-modifiable for IPv4.
        row.SitePrefixLength = 0;
    }
}

fn get_interface(
    interface_luid: u64,
    family: ADDRESS_FAMILY,
) -> Result<MIB_IPINTERFACE_ROW, NetworkError> {
    require_luid(interface_luid)?;
    let mut row = MIB_IPINTERFACE_ROW::default();
    // SAFETY: the API initializes defaults in writable storage.
    unsafe { InitializeIpInterfaceEntry(&mut row) };
    row.Family = family;
    row.InterfaceLuid = luid(interface_luid);
    // SAFETY: row contains a valid family and interface LUID.
    check("GetIpInterfaceEntry", unsafe {
        GetIpInterfaceEntry(&mut row)
    })?;
    Ok(row)
}

fn address_exists(interface_luid: u64, address: IpAddr) -> Result<bool, NetworkError> {
    Ok(find_address(interface_luid, address)?.is_some())
}

fn find_address(
    interface_luid: u64,
    address: IpAddr,
) -> Result<Option<MIB_UNICASTIPADDRESS_ROW>, NetworkError> {
    let mut row = address_row(interface_luid, host_network(address))?;
    // SAFETY: row contains the address and interface key and is writable.
    let status = unsafe { GetUnicastIpAddressEntry(&mut row) };
    if status == NO_ERROR {
        Ok(Some(row))
    } else if status == ERROR_NOT_FOUND || status == ERROR_FILE_NOT_FOUND {
        Ok(None)
    } else {
        Err(NetworkError::windows("GetUnicastIpAddressEntry", status))
    }
}

fn create_address(interface_luid: u64, network: IpNet) -> Result<(), NetworkError> {
    let row = address_row(interface_luid, network)?;
    // SAFETY: row is fully initialized and remains valid for the call.
    check("CreateUnicastIpAddressEntry", unsafe {
        CreateUnicastIpAddressEntry(&row)
    })
}

fn delete_address(interface_luid: u64, network: IpNet) -> Result<(), NetworkError> {
    let Some(row) = find_address(interface_luid, network.addr())? else {
        return Ok(());
    };
    // Delete the exact row returned by Windows rather than reconstructing a
    // partially populated row. This makes recovery safe for an Intended
    // receipt whose CreateUnicastIpAddressEntry never ran.
    // SAFETY: `row` is a complete MIB_UNICASTIPADDRESS_ROW from find_address.
    let status = unsafe { DeleteUnicastIpAddressEntry(&row) };
    if status == NO_ERROR || status == ERROR_NOT_FOUND || status == ERROR_FILE_NOT_FOUND {
        Ok(())
    } else {
        Err(NetworkError::windows("DeleteUnicastIpAddressEntry", status))
    }
}

fn address_row(
    interface_luid: u64,
    network: IpNet,
) -> Result<MIB_UNICASTIPADDRESS_ROW, NetworkError> {
    require_luid(interface_luid)?;
    let mut row = MIB_UNICASTIPADDRESS_ROW::default();
    // SAFETY: the API initializes defaults in writable storage.
    unsafe { InitializeUnicastIpAddressEntry(&mut row) };
    row.InterfaceLuid = luid(interface_luid);
    row.Address = sockaddr_from_ip(network.addr());
    row.OnLinkPrefixLength = network.prefix_len();
    row.PrefixOrigin = IpPrefixOriginManual;
    row.SuffixOrigin = IpSuffixOriginManual;
    row.DadState = IpDadStatePreferred;
    Ok(row)
}

fn interface_guid(interface_luid: u64) -> Result<Uuid, NetworkError> {
    require_luid(interface_luid)?;
    let mut guid = GUID::default();
    // SAFETY: input and output storage are valid.
    check("ConvertInterfaceLuidToGuid", unsafe {
        windows_sys::Win32::NetworkManagement::IpHelper::ConvertInterfaceLuidToGuid(
            &luid(interface_luid),
            &mut guid,
        )
    })?;
    Ok(uuid_from_guid(guid))
}

fn get_dns_servers(interface_guid: Uuid) -> Result<Vec<IpAddr>, NetworkError> {
    let mut settings = DNS_INTERFACE_SETTINGS {
        Version: DNS_INTERFACE_SETTINGS_VERSION1,
        ..Default::default()
    };
    // SAFETY: settings has the required version and writable output storage.
    check("GetInterfaceDnsSettings", unsafe {
        GetInterfaceDnsSettings(guid_from_uuid(interface_guid), &mut settings)
    })?;
    let guard = DnsSettingsGuard(settings);
    let text = wide_string(guard.0.NameServer)?;
    parse_dns_servers(&text)
}

fn set_dns_servers(interface_guid: Uuid, servers: &[IpAddr]) -> Result<(), NetworkError> {
    set_dns_servers_for_family(interface_guid, servers, false)?;
    set_dns_servers_for_family(interface_guid, servers, true)
}

fn set_dns_servers_for_family(
    interface_guid: Uuid,
    servers: &[IpAddr],
    ipv6: bool,
) -> Result<(), NetworkError> {
    let mut wide = dns_server_text(servers, ipv6);
    let settings = DNS_INTERFACE_SETTINGS {
        Version: DNS_INTERFACE_SETTINGS_VERSION1,
        Flags: dns_server_flags(ipv6),
        NameServer: wide.as_mut_ptr(),
        ..Default::default()
    };
    let operation = if ipv6 {
        "SetInterfaceDnsSettings (IPv6)"
    } else {
        "SetInterfaceDnsSettings (IPv4)"
    };
    // SAFETY: all fields without matching flags are zero, NameServer points to
    // a live null-terminated buffer, and settings remains valid for the call.
    check(operation, unsafe {
        SetInterfaceDnsSettings(guid_from_uuid(interface_guid), &settings)
    })
}

fn dns_server_text(servers: &[IpAddr], ipv6: bool) -> Vec<u16> {
    let text = servers
        .iter()
        .filter(|server| server.is_ipv6() == ipv6)
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(",");
    text.encode_utf16().chain([0]).collect()
}

fn dns_server_flags(ipv6: bool) -> u64 {
    u64::from(DNS_SETTING_NAMESERVER) | if ipv6 { u64::from(DNS_SETTING_IPV6) } else { 0 }
}

struct DnsSettingsGuard(DNS_INTERFACE_SETTINGS);

impl Drop for DnsSettingsGuard {
    fn drop(&mut self) {
        // SAFETY: a successful GetInterfaceDnsSettings call initialized this
        // object and requires exactly one matching free.
        unsafe { FreeInterfaceDnsSettings(&mut self.0) };
    }
}

fn parse_dns_servers(value: &str) -> Result<Vec<IpAddr>, NetworkError> {
    value
        .split([',', ';', ' '])
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            value
                .parse::<IpAddr>()
                .map_err(|_| NetworkError::DnsSnapshot(value.to_owned()))
        })
        .collect()
}

fn wide_string(value: *const u16) -> Result<String, NetworkError> {
    if value.is_null() {
        return Ok(String::new());
    }
    let mut length = 0;
    // SAFETY: GetInterfaceDnsSettings returns a null-terminated allocation.
    // The explicit cap prevents an unbounded scan if the API contract is ever
    // violated by corrupted system state.
    unsafe {
        while length < MAX_DNS_TEXT_UNITS && *value.add(length) != 0 {
            length += 1;
        }
        if length == MAX_DNS_TEXT_UNITS {
            return Err(NetworkError::DnsSnapshotTooLong);
        }
        String::from_utf16(std::slice::from_raw_parts(value, length))
            .map_err(|_| NetworkError::DnsSnapshotUtf16)
    }
}

fn host_network(address: IpAddr) -> IpNet {
    match address {
        IpAddr::V4(value) => Ipv4Net::new(value, 32).expect("valid host prefix").into(),
        IpAddr::V6(value) => Ipv6Net::new(value, 128).expect("valid host prefix").into(),
    }
}

fn representative_ip(network: IpNet) -> IpAddr {
    match network {
        IpNet::V4(value) => IpAddr::V4(value.addr()),
        IpNet::V6(value) => IpAddr::V6(value.addr()),
    }
}

fn unspecified(network: IpNet) -> IpAddr {
    match network {
        IpNet::V4(_) => IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        IpNet::V6(_) => IpAddr::V6(Ipv6Addr::UNSPECIFIED),
    }
}

fn prefix_from_network(network: IpNet) -> IP_ADDRESS_PREFIX {
    IP_ADDRESS_PREFIX {
        Prefix: sockaddr_from_ip(network.addr()),
        PrefixLength: network.prefix_len(),
    }
}

fn sockaddr_from_ip(address: IpAddr) -> SOCKADDR_INET {
    sockaddr_from_ip_scoped(address, 0)
}

fn sockaddr_from_ip_scoped(address: IpAddr, scope_id: u32) -> SOCKADDR_INET {
    match address {
        IpAddr::V4(address) => SOCKADDR_INET {
            Ipv4: SOCKADDR_IN {
                sin_family: AF_INET,
                sin_port: 0,
                sin_addr: IN_ADDR {
                    S_un: IN_ADDR_0 {
                        S_un_b: IN_ADDR_0_0 {
                            s_b1: address.octets()[0],
                            s_b2: address.octets()[1],
                            s_b3: address.octets()[2],
                            s_b4: address.octets()[3],
                        },
                    },
                },
                sin_zero: [0; 8],
            },
        },
        IpAddr::V6(address) => SOCKADDR_INET {
            Ipv6: SOCKADDR_IN6 {
                sin6_family: AF_INET6,
                sin6_port: 0,
                sin6_flowinfo: 0,
                sin6_addr: IN6_ADDR {
                    u: IN6_ADDR_0 {
                        Byte: address.octets(),
                    },
                },
                Anonymous: SOCKADDR_IN6_0 {
                    sin6_scope_id: scope_id,
                },
            },
        },
    }
}

fn ip_from_sockaddr(value: &SOCKADDR_INET) -> Result<(IpAddr, u32), NetworkError> {
    // SAFETY: the active union member is selected by the common family field.
    unsafe {
        match value.si_family {
            AF_INET => {
                let bytes = value.Ipv4.sin_addr.S_un.S_un_b;
                Ok((
                    IpAddr::V4(Ipv4Addr::new(
                        bytes.s_b1, bytes.s_b2, bytes.s_b3, bytes.s_b4,
                    )),
                    0,
                ))
            }
            AF_INET6 => Ok((
                IpAddr::V6(Ipv6Addr::from(value.Ipv6.sin6_addr.u.Byte)),
                value.Ipv6.Anonymous.sin6_scope_id,
            )),
            _ => Err(NetworkError::AddressFamily),
        }
    }
}

fn parse_network(value: &str) -> Result<IpNet, NetworkError> {
    value
        .parse::<IpNet>()
        .map_err(|_| NetworkError::Network(value.to_owned()))
}

fn luid(value: u64) -> NET_LUID_LH {
    NET_LUID_LH { Value: value }
}

fn luid_value(value: NET_LUID_LH) -> u64 {
    // SAFETY: Value is the complete representation of NET_LUID.
    unsafe { value.Value }
}

fn require_luid(value: u64) -> Result<(), NetworkError> {
    if value == 0 {
        Err(NetworkError::EmptyLuid)
    } else {
        Ok(())
    }
}

fn uuid_from_guid(value: GUID) -> Uuid {
    Uuid::from_fields(value.data1, value.data2, value.data3, &value.data4)
}

fn guid_from_uuid(value: Uuid) -> GUID {
    let (data1, data2, data3, data4) = value.as_fields();
    GUID {
        data1,
        data2,
        data3,
        data4: *data4,
    }
}

fn check(operation: &'static str, code: WIN32_ERROR) -> Result<(), NetworkError> {
    if code == NO_ERROR {
        Ok(())
    } else {
        Err(NetworkError::windows(operation, code))
    }
}

fn retain_first_error(destination: &mut Option<NetworkError>, result: Result<(), NetworkError>) {
    if let Err(error) = result
        && destination.is_none()
    {
        *destination = Some(error);
    }
}

#[derive(Debug, Error)]
pub enum NetworkError {
    #[error("{operation} failed with Windows error {code}")]
    Windows {
        operation: &'static str,
        code: WIN32_ERROR,
    },
    #[error("network receipt is not a {0} receipt")]
    ReceiptKind(&'static str),
    #[error("interface LUID must not be zero")]
    EmptyLuid,
    #[error("address families in the network receipt do not match")]
    AddressFamily,
    #[error("journal route or address is not a CIDR: {0}")]
    Network(String),
    #[error("Windows returned a non-IP DNS server value: {0}")]
    DnsSnapshot(String),
    #[error("Windows returned an unterminated DNS settings string")]
    DnsSnapshotTooLong,
    #[error("Windows returned invalid UTF-16 in DNS settings")]
    DnsSnapshotUtf16,
    #[error("no physical route to a configured WARP endpoint is available")]
    NoReachableEndpoint,
    #[error("no physical route to an authenticated WARP control endpoint is available")]
    NoReachableControlApi,
}

impl NetworkError {
    const fn windows(operation: &'static str, code: WIN32_ERROR) -> Self {
        Self::Windows { operation, code }
    }

    const fn is_candidate_unavailable(&self) -> bool {
        matches!(
            self,
            Self::Windows {
                code: ERROR_NO_NETWORK
                    | ERROR_NOT_SUPPORTED
                    | ERROR_NETWORK_UNREACHABLE
                    | ERROR_HOST_UNREACHABLE
                    | ERROR_PROTOCOL_UNREACHABLE,
                ..
            }
        )
    }

    const fn is_interface_churn(&self) -> bool {
        matches!(
            self,
            Self::Windows {
                code: ERROR_FILE_NOT_FOUND | ERROR_NOT_FOUND,
                ..
            }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sockaddr_round_trips_ipv4_and_ipv6_scope() {
        for (address, scope) in [
            ("162.159.198.2".parse().expect("IPv4"), 0),
            ("2606:4700:103::2".parse().expect("IPv6"), 19),
        ] {
            let encoded = sockaddr_from_ip_scoped(address, scope);
            assert_eq!(
                ip_from_sockaddr(&encoded).expect("decode"),
                (address, scope)
            );
        }
    }

    #[test]
    fn prefix_encoding_preserves_network_and_length() {
        for network in ["192.168.0.0/16", "2606:4700:103::/48"] {
            let network = network.parse::<IpNet>().expect("network");
            let encoded = prefix_from_network(network);
            assert_eq!(encoded.PrefixLength, network.prefix_len());
            assert_eq!(
                ip_from_sockaddr(&encoded.Prefix).expect("decode").0,
                network.addr()
            );
        }
    }

    #[test]
    fn dns_snapshot_parser_accepts_windows_separators() {
        assert_eq!(
            parse_dns_servers("1.1.1.1, 8.8.8.8;2606:4700:4700::1111").expect("servers"),
            vec![
                "1.1.1.1".parse::<IpAddr>().expect("IPv4"),
                "8.8.8.8".parse::<IpAddr>().expect("IPv4"),
                "2606:4700:4700::1111".parse::<IpAddr>().expect("IPv6"),
            ]
        );
        assert!(parse_dns_servers("resolver.example").is_err());
    }

    #[test]
    fn dns_settings_separate_ipv4_and_ipv6_servers() {
        let servers = [
            "1.1.1.1".parse::<IpAddr>().expect("IPv4"),
            "2606:4700:4700::1111".parse::<IpAddr>().expect("IPv6"),
            "8.8.8.8".parse::<IpAddr>().expect("IPv4"),
            "2001:4860:4860::8888".parse::<IpAddr>().expect("IPv6"),
        ];

        assert_eq!(
            dns_server_text(&servers, false),
            "1.1.1.1,8.8.8.8"
                .encode_utf16()
                .chain([0])
                .collect::<Vec<_>>()
        );
        assert_eq!(
            dns_server_text(&servers, true),
            "2606:4700:4700::1111,2001:4860:4860::8888"
                .encode_utf16()
                .chain([0])
                .collect::<Vec<_>>()
        );
        assert_eq!(dns_server_flags(false), u64::from(DNS_SETTING_NAMESERVER));
        assert_eq!(
            dns_server_flags(true),
            u64::from(DNS_SETTING_NAMESERVER | DNS_SETTING_IPV6)
        );
    }

    #[test]
    fn empty_dns_family_uses_a_non_null_empty_string() {
        assert_eq!(dns_server_text(&[], false), vec![0]);
        assert_eq!(
            dns_server_text(&["1.1.1.1".parse().expect("IPv4")], true),
            vec![0]
        );
    }

    #[test]
    fn guid_uuid_conversion_is_lossless() {
        let value = Uuid::parse_str("12345678-9abc-def0-1122-334455667788").expect("UUID");
        assert_eq!(uuid_from_guid(guid_from_uuid(value)), value);
    }

    #[test]
    fn ipv4_mtu_update_clears_the_non_modifiable_site_prefix() {
        let mut ipv4 = MIB_IPINTERFACE_ROW {
            SitePrefixLength: 24,
            ..Default::default()
        };
        prepare_mtu_update(&mut ipv4, AF_INET, 1280);
        assert_eq!(ipv4.NlMtu, 1280);
        assert_eq!(ipv4.SitePrefixLength, 0);

        let mut ipv6 = MIB_IPINTERFACE_ROW {
            SitePrefixLength: 64,
            ..Default::default()
        };
        prepare_mtu_update(&mut ipv6, AF_INET6, 1280);
        assert_eq!(ipv6.NlMtu, 1280);
        assert_eq!(ipv6.SitePrefixLength, 64);
    }

    #[test]
    fn route_row_rejects_mixed_families_without_touching_windows() {
        let receipt = RouteReceipt {
            destination: "0.0.0.0/0".to_owned(),
            next_hop: Some("2001:db8::1".parse().expect("IPv6")),
            next_hop_scope_id: 0,
            interface_luid: 1,
            metric: 0,
            owned: true,
        };
        assert!(matches!(
            route_row(&receipt),
            Err(NetworkError::AddressFamily)
        ));
    }

    fn fake_route(destination: IpNet) -> RouteReceipt {
        RouteReceipt {
            destination: destination.to_string(),
            next_hop: Some(match destination {
                IpNet::V4(_) => "192.0.2.1".parse().expect("IPv4 gateway"),
                IpNet::V6(_) => "2001:db8::1".parse().expect("IPv6 gateway"),
            }),
            next_hop_scope_id: 0,
            interface_luid: 7,
            metric: 5,
            owned: true,
        }
    }

    fn candidates() -> (Vec<SocketAddr>, Vec<SocketAddr>) {
        (
            vec![
                "162.159.198.2:443".parse().expect("IPv4 endpoint"),
                "[2606:4700:103::2]:443".parse().expect("IPv6 endpoint"),
            ],
            vec![
                "198.51.100.10:443".parse().expect("IPv4 control"),
                "[2001:db8::10]:443".parse().expect("IPv6 control"),
            ],
        )
    }

    #[test]
    fn endpoint_bypass_skips_an_unreachable_ipv6_family() {
        let (endpoints, control) = candidates();
        let receipt = plan_endpoint_bypass_with(&endpoints, &control, |destination| {
            if destination.addr().is_ipv6() {
                Err(NetworkError::windows(
                    "GetBestInterfaceEx",
                    ERROR_NETWORK_UNREACHABLE,
                ))
            } else {
                Ok(fake_route(destination))
            }
        })
        .expect("IPv4 routes keep the plan usable");
        let MutationReceipt::EndpointBypass { created } = receipt else {
            panic!("endpoint receipt");
        };
        assert_eq!(created.len(), 2);
        assert!(
            created
                .iter()
                .all(|route| route.destination.ends_with("/32"))
        );
    }

    #[test]
    fn endpoint_bypass_skips_an_unreachable_ipv4_family() {
        let (endpoints, control) = candidates();
        let receipt = plan_endpoint_bypass_with(&endpoints, &control, |destination| {
            if destination.addr().is_ipv4() {
                Err(NetworkError::windows(
                    "GetBestInterfaceEx",
                    ERROR_NETWORK_UNREACHABLE,
                ))
            } else {
                Ok(fake_route(destination))
            }
        })
        .expect("IPv6 routes keep the plan usable");
        let MutationReceipt::EndpointBypass { created } = receipt else {
            panic!("endpoint receipt");
        };
        assert_eq!(created.len(), 2);
        assert!(
            created
                .iter()
                .all(|route| route.destination.ends_with("/128"))
        );
    }

    #[test]
    fn endpoint_bypass_retries_interface_churn_once() {
        let (endpoints, control) = candidates();
        let mut first_ipv4_endpoint_lookup = true;
        let receipt = plan_endpoint_bypass_with(&endpoints, &control, |destination| {
            if destination.addr() == endpoints[0].ip() && first_ipv4_endpoint_lookup {
                first_ipv4_endpoint_lookup = false;
                return Err(NetworkError::windows("GetBestRoute2", ERROR_FILE_NOT_FOUND));
            }
            Ok(fake_route(destination))
        })
        .expect("a replacement interface is discovered");
        let MutationReceipt::EndpointBypass { created } = receipt else {
            panic!("endpoint receipt");
        };
        assert_eq!(created.len(), 4);
        assert!(!first_ipv4_endpoint_lookup);
    }

    #[test]
    fn endpoint_bypass_requires_a_data_route() {
        let (endpoints, control) = candidates();
        let result = plan_endpoint_bypass_with(&endpoints, &control, |destination| {
            if endpoints
                .iter()
                .any(|candidate| candidate.ip() == destination.addr())
            {
                Err(NetworkError::windows(
                    "GetBestInterfaceEx",
                    ERROR_NETWORK_UNREACHABLE,
                ))
            } else {
                Ok(fake_route(destination))
            }
        });
        assert!(matches!(result, Err(NetworkError::NoReachableEndpoint)));
    }

    #[test]
    fn endpoint_bypass_requires_a_control_route() {
        let (endpoints, control) = candidates();
        let result = plan_endpoint_bypass_with(&endpoints, &control, |destination| {
            if control
                .iter()
                .any(|candidate| candidate.ip() == destination.addr())
            {
                Err(NetworkError::windows(
                    "GetBestInterfaceEx",
                    ERROR_PROTOCOL_UNREACHABLE,
                ))
            } else {
                Ok(fake_route(destination))
            }
        });
        assert!(matches!(result, Err(NetworkError::NoReachableControlApi)));
    }

    #[test]
    fn endpoint_bypass_does_not_hide_fatal_route_errors() {
        let (endpoints, control) = candidates();
        let result = plan_endpoint_bypass_with(&endpoints, &control, |_| {
            Err(NetworkError::windows("GetBestInterfaceEx", 5))
        });
        assert!(matches!(result, Err(NetworkError::Windows { code: 5, .. })));
    }
}
