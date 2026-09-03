//! Persistent Windows Filtering Platform Kill Switch.
//!
//! The rule set is deliberately small and auditable. It permits loopback,
//! traffic routed through the active Wintun interface, the authenticated
//! Engine to the single selected WARP endpoint, explicit LAN/CIDR bypasses,
//! and the minimum DHCP/IPv6-neighbour control traffic. A terminal block rule
//! exists for every enabled address family. Provider, sublayer, and filters
//! are persistent, so Engine/UI/Agent crashes remain fail-closed.

use std::{
    collections::BTreeSet,
    ffi::c_void,
    net::{IpAddr, SocketAddr},
    os::windows::ffi::OsStrExt,
    path::Path,
    ptr,
};

use ipnet::{IpNet, Ipv4Net, Ipv6Net};
use thiserror::Error;
use uuid::Uuid;
use windows_sys::{
    Win32::{
        Foundation::{
            FWP_E_ALREADY_EXISTS, FWP_E_FILTER_NOT_FOUND, FWP_E_PROVIDER_NOT_FOUND,
            FWP_E_SUBLAYER_NOT_FOUND, HANDLE,
        },
        NetworkManagement::WindowsFilteringPlatform::{
            FWP_ACTION_BLOCK, FWP_ACTION_PERMIT, FWP_BYTE_BLOB, FWP_BYTE_BLOB_TYPE,
            FWP_CONDITION_FLAG_IS_LOOPBACK, FWP_CONDITION_VALUE0, FWP_CONDITION_VALUE0_0,
            FWP_MATCH_EQUAL, FWP_MATCH_FLAGS_ALL_SET, FWP_UINT8, FWP_UINT16, FWP_UINT32,
            FWP_UINT64, FWP_V4_ADDR_AND_MASK, FWP_V4_ADDR_MASK, FWP_V6_ADDR_AND_MASK,
            FWP_V6_ADDR_MASK, FWP_VALUE0, FWP_VALUE0_0, FWPM_ACTION0, FWPM_CONDITION_ALE_APP_ID,
            FWPM_CONDITION_FLAGS, FWPM_CONDITION_IP_LOCAL_INTERFACE, FWPM_CONDITION_IP_LOCAL_PORT,
            FWPM_CONDITION_IP_PROTOCOL, FWPM_CONDITION_IP_REMOTE_ADDRESS,
            FWPM_CONDITION_IP_REMOTE_PORT, FWPM_DISPLAY_DATA0, FWPM_FILTER_CONDITION0,
            FWPM_FILTER_FLAG_PERSISTENT, FWPM_FILTER0, FWPM_LAYER_ALE_AUTH_CONNECT_V4,
            FWPM_LAYER_ALE_AUTH_CONNECT_V6, FWPM_PROVIDER_FLAG_PERSISTENT, FWPM_PROVIDER0,
            FWPM_SESSION_FLAG_DYNAMIC, FWPM_SESSION0, FWPM_SUBLAYER_FLAG_PERSISTENT,
            FWPM_SUBLAYER0, FwpmEngineClose0, FwpmEngineOpen0, FwpmFilterAdd0,
            FwpmFilterDeleteByKey0, FwpmFilterGetByKey0, FwpmFreeMemory0,
            FwpmGetAppIdFromFileName0, FwpmProviderAdd0, FwpmProviderDeleteByKey0,
            FwpmSubLayerAdd0, FwpmSubLayerDeleteByKey0, FwpmTransactionAbort0,
            FwpmTransactionBegin0, FwpmTransactionCommit0,
        },
        Networking::WinSock::{IPPROTO_ICMPV6, IPPROTO_TCP, IPPROTO_UDP},
        System::Rpc::RPC_C_AUTHN_WINNT,
    },
    core::GUID,
};

use crate::{journal::MutationReceipt, plan::ValidatedTunnelPlan};

const PROVIDER_NAME: &str = "Usque Kill Switch";
const PROVIDER_DESCRIPTION: &str =
    "Persistent fail-closed policy for the active Usque VPN operation";
const SUBLAYER_WEIGHT: u16 = 0x7ffe;
const ALLOW_WEIGHT: u8 = 15;
const BLOCK_WEIGHT: u8 = 0;
const MAX_FILTERS: usize = 256;
// Stable identities make it possible for an elevated recovery executable to
// remove Usque's persistent WFP policy even when the recovery journal is
// missing or corrupt. Legacy random-key receipts remain recoverable through
// `restore_kill_switch`.
const PROVIDER_KEY: Uuid = Uuid::from_u128(0x6d70fda5_3fa2_4c36_a86c_88650b58f013);
const SUBLAYER_KEY: Uuid = Uuid::from_u128(0xc93b7042_7b1e_4ab5_96ba_96b4539b67ec);
const FILTER_KEY_BASE: u128 = 0x39ce51c7_ba9d_42f4_ae00_000000000000;

pub fn plan_kill_switch(
    plan: &ValidatedTunnelPlan,
    interface_luid: u64,
) -> Result<MutationReceipt, WfpError> {
    let rules = build_rules(plan, interface_luid)?;
    Ok(MutationReceipt::KillSwitch {
        provider_key: PROVIDER_KEY,
        sublayer_key: SUBLAYER_KEY,
        filter_keys: (0..rules.len()).map(filter_key).collect(),
        filter_ids: Vec::new(),
    })
}

pub fn apply_kill_switch(
    mut receipt: MutationReceipt,
    plan: &ValidatedTunnelPlan,
    interface_luid: u64,
    engine_path: &Path,
) -> Result<MutationReceipt, WfpError> {
    let MutationReceipt::KillSwitch {
        provider_key,
        sublayer_key,
        filter_keys,
        filter_ids,
    } = &mut receipt
    else {
        return Err(WfpError::ReceiptKind);
    };
    if !engine_path.is_absolute() {
        return Err(WfpError::EnginePath);
    }
    let rules = build_rules(plan, interface_luid)?;
    if filter_keys.len() != rules.len() || filter_keys.is_empty() {
        return Err(WfpError::FilterKeyCount {
            expected: rules.len(),
            actual: filter_keys.len(),
        });
    }

    let engine = WfpEngine::open()?;
    let application_id = ApplicationId::from_path(engine_path)?;
    let transaction = WfpTransaction::begin(&engine)?;
    add_provider(&engine, *provider_key)?;
    add_sublayer(&engine, *provider_key, *sublayer_key)?;

    let mut ids = Vec::with_capacity(rules.len());
    for ((rule, filter_key), index) in rules.iter().zip(filter_keys.iter().copied()).zip(0_usize..)
    {
        ids.push(add_filter(
            &engine,
            *provider_key,
            *sublayer_key,
            filter_key,
            index,
            rule,
            application_id.as_ptr(),
            FWPM_FILTER_FLAG_PERSISTENT,
        )?);
    }
    transaction.commit()?;
    *filter_ids = ids;
    Ok(receipt)
}

pub fn restore_kill_switch(receipt: &MutationReceipt) -> Result<(), WfpError> {
    let MutationReceipt::KillSwitch {
        provider_key,
        sublayer_key,
        filter_keys,
        filter_ids: _,
    } = receipt
    else {
        return Err(WfpError::ReceiptKind);
    };
    remove_resources(*provider_key, *sublayer_key, filter_keys.iter().copied())
}

/// Read-only startup verification. Do not recreate missing persistent policy
/// while presenting an old transaction as a live tunnel.
pub fn kill_switch_present(receipt: &MutationReceipt) -> Result<bool, WfpError> {
    let MutationReceipt::KillSwitch {
        provider_key,
        sublayer_key,
        filter_keys,
        ..
    } = receipt
    else {
        return Err(WfpError::ReceiptKind);
    };
    let engine = WfpEngine::open()?;
    if filter_keys.is_empty() {
        return Ok(false);
    }
    for key in filter_keys {
        let mut filter = ptr::null_mut();
        // SAFETY: engine and key are live and filter is writable output storage.
        let status = unsafe { FwpmFilterGetByKey0(engine.0, &guid_from_uuid(*key), &mut filter) };
        if status == FWP_E_FILTER_NOT_FOUND as u32 {
            return Ok(false);
        }
        check("FwpmFilterGetByKey0", status)?;
        if filter.is_null() {
            return Ok(false);
        }
        // SAFETY: the successful lookup returned a live filter and its owned
        // provider pointer, both valid until the matching FwpmFreeMemory0.
        let matches = unsafe {
            (*filter).flags & FWPM_FILTER_FLAG_PERSISTENT != 0
                && uuid_from_guid((*filter).subLayerKey) == *sublayer_key
                && !(*filter).providerKey.is_null()
                && uuid_from_guid(*(*filter).providerKey) == *provider_key
        };
        // SAFETY: WFP owns this lookup allocation; release it exactly once.
        unsafe { FwpmFreeMemory0((&mut filter as *mut *mut FWPM_FILTER0).cast()) };
        if !matches {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Removes every resource that a current Usque build can create without
/// consulting the journal. This is intentionally bounded and targets only
/// stable Usque GUIDs; it is safe to run before MSI recovery and when journal
/// validation fails.
pub fn emergency_remove_kill_switch() -> Result<(), WfpError> {
    remove_resources(PROVIDER_KEY, SUBLAYER_KEY, (0..MAX_FILTERS).map(filter_key))
}

fn filter_key(index: usize) -> Uuid {
    debug_assert!(index < MAX_FILTERS);
    Uuid::from_u128(FILTER_KEY_BASE + index as u128)
}

fn remove_resources(
    provider_key: Uuid,
    sublayer_key: Uuid,
    filter_keys: impl IntoIterator<Item = Uuid>,
) -> Result<(), WfpError> {
    let engine = WfpEngine::open()?;
    let transaction = WfpTransaction::begin(&engine)?;
    let mut first_error = None;
    let filter_keys = filter_keys.into_iter().collect::<Vec<_>>();
    for key in filter_keys.into_iter().rev() {
        retain_delete_error(
            &mut first_error,
            "FwpmFilterDeleteByKey0",
            // SAFETY: engine is live and key storage remains valid.
            unsafe { FwpmFilterDeleteByKey0(engine.0, &guid_from_uuid(key)) },
            FWP_E_FILTER_NOT_FOUND as u32,
        );
    }
    retain_delete_error(
        &mut first_error,
        "FwpmSubLayerDeleteByKey0",
        // SAFETY: engine is live and key storage remains valid.
        unsafe { FwpmSubLayerDeleteByKey0(engine.0, &guid_from_uuid(sublayer_key)) },
        FWP_E_SUBLAYER_NOT_FOUND as u32,
    );
    retain_delete_error(
        &mut first_error,
        "FwpmProviderDeleteByKey0",
        // SAFETY: engine is live and key storage remains valid.
        unsafe { FwpmProviderDeleteByKey0(engine.0, &guid_from_uuid(provider_key)) },
        FWP_E_PROVIDER_NOT_FOUND as u32,
    );
    // Commit every successful deletion even if a later metadata deletion
    // failed. Aborting the transaction here would resurrect block filters that
    // Windows had already accepted for deletion. The caller still receives the
    // first error and can retry the remaining idempotent cleanup.
    let commit_result = transaction.commit();
    match (first_error, commit_result) {
        (Some(error), _) => Err(error),
        (None, result) => result,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AddressFamily {
    V4,
    V6,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FilterRule {
    name: String,
    family: AddressFamily,
    action: RuleAction,
    weight: u8,
    conditions: Vec<ConditionSpec>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuleAction {
    Permit,
    Block,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ConditionSpec {
    Loopback,
    InterfaceLuid(u64),
    ApplicationId,
    RemoteNetwork(IpNet),
    Protocol(u8),
    LocalPort(u16),
    RemotePort(u16),
}

fn build_rules(
    plan: &ValidatedTunnelPlan,
    interface_luid: u64,
) -> Result<Vec<FilterRule>, WfpError> {
    if interface_luid == 0 {
        return Err(WfpError::EmptyLuid);
    }
    let exclusions = effective_exclusions(plan)?;
    let mut rules = Vec::new();
    if plan.assigned_ipv4.is_some() {
        add_family_rules(
            &mut rules,
            AddressFamily::V4,
            plan,
            interface_luid,
            &exclusions,
        );
    }
    if plan.assigned_ipv6.is_some() {
        add_family_rules(
            &mut rules,
            AddressFamily::V6,
            plan,
            interface_luid,
            &exclusions,
        );
    }
    if rules.is_empty() || rules.len() > MAX_FILTERS {
        return Err(WfpError::RuleCount(rules.len()));
    }
    Ok(rules)
}

fn add_family_rules(
    rules: &mut Vec<FilterRule>,
    family: AddressFamily,
    plan: &ValidatedTunnelPlan,
    interface_luid: u64,
    exclusions: &BTreeSet<IpNet>,
) {
    rules.push(permit(family, "Loopback", vec![ConditionSpec::Loopback]));
    rules.push(permit(
        family,
        "Wintun interface",
        vec![ConditionSpec::InterfaceLuid(interface_luid)],
    ));

    for endpoint in plan
        .endpoint_candidates
        .iter()
        .filter(|endpoint| family_matches(family, endpoint.ip()))
    {
        let endpoint_network = host_network(endpoint.ip());
        for (protocol, label) in [
            (IPPROTO_UDP as u8, "Engine H3 endpoint"),
            (IPPROTO_TCP as u8, "Engine H2 endpoint"),
        ] {
            rules.push(permit(
                family,
                &format!("{label} {}", endpoint.ip()),
                vec![
                    ConditionSpec::ApplicationId,
                    ConditionSpec::RemoteNetwork(endpoint_network),
                    ConditionSpec::RemotePort(endpoint.port()),
                    ConditionSpec::Protocol(protocol),
                ],
            ));
        }
    }

    for endpoint in plan
        .control_api_candidates
        .iter()
        .filter(|endpoint| family_matches(family, endpoint.ip()))
    {
        rules.push(permit(
            family,
            &format!("Engine authenticated control API {}", endpoint.ip()),
            vec![
                ConditionSpec::ApplicationId,
                ConditionSpec::RemoteNetwork(host_network(endpoint.ip())),
                ConditionSpec::RemotePort(endpoint.port()),
                ConditionSpec::Protocol(IPPROTO_TCP as u8),
            ],
        ));
    }

    for exclusion in exclusions
        .iter()
        .copied()
        .filter(|network| family_matches(family, network.addr()))
    {
        rules.push(permit(
            family,
            &format!("Bypass {exclusion}"),
            vec![ConditionSpec::RemoteNetwork(exclusion)],
        ));
    }

    match family {
        AddressFamily::V4 => rules.push(permit(
            family,
            "DHCPv4",
            vec![
                ConditionSpec::Protocol(IPPROTO_UDP as u8),
                ConditionSpec::LocalPort(68),
                ConditionSpec::RemotePort(67),
            ],
        )),
        AddressFamily::V6 => {
            rules.push(permit(
                family,
                "DHCPv6",
                vec![
                    ConditionSpec::Protocol(IPPROTO_UDP as u8),
                    ConditionSpec::LocalPort(546),
                    ConditionSpec::RemotePort(547),
                ],
            ));
            for network in ["fe80::/10", "ff02::/16"] {
                rules.push(permit(
                    family,
                    "IPv6 neighbour discovery",
                    vec![
                        ConditionSpec::Protocol(IPPROTO_ICMPV6 as u8),
                        ConditionSpec::RemoteNetwork(
                            network.parse().expect("static IPv6 control prefix"),
                        ),
                    ],
                ));
            }
        }
    }
    rules.push(FilterRule {
        name: format!("Block all {family:?} physical traffic"),
        family,
        action: RuleAction::Block,
        weight: BLOCK_WEIGHT,
        conditions: Vec::new(),
    });
}

fn permit(family: AddressFamily, name: &str, conditions: Vec<ConditionSpec>) -> FilterRule {
    FilterRule {
        name: name.to_owned(),
        family,
        action: RuleAction::Permit,
        weight: ALLOW_WEIGHT,
        conditions,
    }
}

fn effective_exclusions(plan: &ValidatedTunnelPlan) -> Result<BTreeSet<IpNet>, WfpError> {
    let mut output = plan
        .split_exclusions
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    if plan.allow_lan {
        for network in [
            "10.0.0.0/8",
            "172.16.0.0/12",
            "192.168.0.0/16",
            "169.254.0.0/16",
            "fc00::/7",
            "fe80::/10",
        ] {
            output.insert(
                network
                    .parse()
                    .map_err(|_| WfpError::StaticNetwork(network))?,
            );
        }
    }
    Ok(output)
}

fn add_provider(engine: &WfpEngine, provider_key: Uuid) -> Result<(), WfpError> {
    let mut name = wide(PROVIDER_NAME);
    let mut description = wide(PROVIDER_DESCRIPTION);
    let provider = FWPM_PROVIDER0 {
        providerKey: guid_from_uuid(provider_key),
        displayData: FWPM_DISPLAY_DATA0 {
            name: name.as_mut_ptr(),
            description: description.as_mut_ptr(),
        },
        flags: FWPM_PROVIDER_FLAG_PERSISTENT,
        ..Default::default()
    };
    // SAFETY: all pointed-to strings remain alive for the synchronous call.
    check_allow_existing("FwpmProviderAdd0", unsafe {
        FwpmProviderAdd0(engine.0, &provider, ptr::null_mut())
    })
}

fn add_sublayer(
    engine: &WfpEngine,
    provider_key: Uuid,
    sublayer_key: Uuid,
) -> Result<(), WfpError> {
    let mut provider_guid = guid_from_uuid(provider_key);
    let mut name = wide(PROVIDER_NAME);
    let mut description = wide(PROVIDER_DESCRIPTION);
    let sublayer = FWPM_SUBLAYER0 {
        subLayerKey: guid_from_uuid(sublayer_key),
        displayData: FWPM_DISPLAY_DATA0 {
            name: name.as_mut_ptr(),
            description: description.as_mut_ptr(),
        },
        flags: FWPM_SUBLAYER_FLAG_PERSISTENT,
        providerKey: &mut provider_guid,
        weight: SUBLAYER_WEIGHT,
        ..Default::default()
    };
    // SAFETY: all pointed-to values remain alive for the synchronous call.
    check_allow_existing("FwpmSubLayerAdd0", unsafe {
        FwpmSubLayerAdd0(engine.0, &sublayer, ptr::null_mut())
    })
}

#[expect(
    clippy::too_many_arguments,
    reason = "WFP filter install needs provider, sublayer, filter identity, rule fields, and app-id blob as one call"
)]
fn add_filter(
    engine: &WfpEngine,
    provider_key: Uuid,
    sublayer_key: Uuid,
    filter_key: Uuid,
    index: usize,
    rule: &FilterRule,
    application_id: *mut FWP_BYTE_BLOB,
    flags: u32,
) -> Result<u64, WfpError> {
    let mut compiled = CompiledConditions::new(application_id);
    for condition in &rule.conditions {
        compiled.push(condition)?;
    }
    let mut provider_guid = guid_from_uuid(provider_key);
    let mut name = wide(&format!("Usque {:03}: {}", index + 1, rule.name));
    let mut description = wide(PROVIDER_DESCRIPTION);
    let filter = FWPM_FILTER0 {
        filterKey: guid_from_uuid(filter_key),
        displayData: FWPM_DISPLAY_DATA0 {
            name: name.as_mut_ptr(),
            description: description.as_mut_ptr(),
        },
        flags,
        providerKey: &mut provider_guid,
        layerKey: match rule.family {
            AddressFamily::V4 => FWPM_LAYER_ALE_AUTH_CONNECT_V4,
            AddressFamily::V6 => FWPM_LAYER_ALE_AUTH_CONNECT_V6,
        },
        subLayerKey: guid_from_uuid(sublayer_key),
        weight: FWP_VALUE0 {
            r#type: FWP_UINT8,
            Anonymous: FWP_VALUE0_0 { uint8: rule.weight },
        },
        numFilterConditions: compiled.len() as u32,
        filterCondition: compiled.as_mut_ptr(),
        action: FWPM_ACTION0 {
            r#type: match rule.action {
                RuleAction::Permit => FWP_ACTION_PERMIT,
                RuleAction::Block => FWP_ACTION_BLOCK,
            },
            ..Default::default()
        },
        ..Default::default()
    };
    let mut filter_id = 0_u64;
    // SAFETY: filter and every condition backing allocation remain alive for
    // the complete synchronous call.
    check("FwpmFilterAdd0", unsafe {
        FwpmFilterAdd0(engine.0, &filter, ptr::null_mut(), &mut filter_id)
    })?;
    if filter_id == 0 {
        return Err(WfpError::EmptyFilterId);
    }
    Ok(filter_id)
}

/// A non-persistent, exact WFP permit. Closing the dynamic WFP session removes
/// the filter even when the Engine pipe disappears or the Agent is terminated.
pub struct DynamicPermit {
    engine: WfpEngine,
    filter_key: Uuid,
}

// SAFETY: the session handle is owned exclusively by this value and no method
// invokes it through a shared reference, so transferring ownership is safe.
unsafe impl Send for DynamicPermit {}
// SAFETY: immutable sharing cannot access the session handle; Drop requires an
// exclusive reference and therefore runs only after all shared borrows end.
unsafe impl Sync for DynamicPermit {}

impl Drop for DynamicPermit {
    fn drop(&mut self) {
        // Best-effort eager cleanup. Dynamic-session close below is the
        // authoritative crash-safe cleanup path.
        // SAFETY: the engine session remains open for this synchronous call,
        // and the temporary GUID is valid for the duration of the call.
        unsafe {
            FwpmFilterDeleteByKey0(self.engine.0, &guid_from_uuid(self.filter_key));
        }
    }
}

pub fn acquire_dynamic_permit(
    remote: SocketAddr,
    protocol: u8,
    interface_luid: u64,
    engine_path: &Path,
) -> Result<DynamicPermit, WfpError> {
    if interface_luid == 0 {
        return Err(WfpError::EmptyLuid);
    }
    if !engine_path.is_absolute() {
        return Err(WfpError::EnginePath);
    }
    let rule = dynamic_direct_rule(remote, protocol, interface_luid)?;
    let engine = WfpEngine::open_dynamic()?;
    let application_id = ApplicationId::from_path(engine_path)?;
    let filter_key = Uuid::new_v4();
    add_filter(
        &engine,
        PROVIDER_KEY,
        SUBLAYER_KEY,
        filter_key,
        0,
        &rule,
        application_id.as_ptr(),
        0,
    )?;
    Ok(DynamicPermit { engine, filter_key })
}

fn dynamic_direct_rule(
    remote: SocketAddr,
    protocol: u8,
    interface_luid: u64,
) -> Result<FilterRule, WfpError> {
    if remote.port() == 0
        || remote.ip().is_unspecified()
        || remote.ip().is_multicast()
        || !matches!(protocol, value if value == IPPROTO_TCP as u8 || value == IPPROTO_UDP as u8)
    {
        return Err(WfpError::UnsafeDynamicTarget);
    }
    if interface_luid == 0 {
        return Err(WfpError::EmptyLuid);
    }
    Ok(permit(
        if remote.is_ipv4() {
            AddressFamily::V4
        } else {
            AddressFamily::V6
        },
        &format!("Dynamic direct {remote}/{protocol}"),
        vec![
            ConditionSpec::ApplicationId,
            ConditionSpec::InterfaceLuid(interface_luid),
            ConditionSpec::RemoteNetwork(host_network(remote.ip())),
            ConditionSpec::RemotePort(remote.port()),
            ConditionSpec::Protocol(protocol),
        ],
    ))
}

// Each box gives WFP condition unions a stable pointee while their owning
// vectors grow. A plain Vec<T> would invalidate earlier raw pointers on
// reallocation before FwpmFilterAdd0 consumes the condition array.
#[expect(
    clippy::vec_box,
    reason = "boxed condition payloads keep stable addresses while the condition vector grows before FwpmFilterAdd0"
)]
struct CompiledConditions {
    conditions: Vec<FWPM_FILTER_CONDITION0>,
    uint64: Vec<Box<u64>>,
    v4: Vec<Box<FWP_V4_ADDR_AND_MASK>>,
    v6: Vec<Box<FWP_V6_ADDR_AND_MASK>>,
    application_id: *mut FWP_BYTE_BLOB,
}

impl CompiledConditions {
    fn new(application_id: *mut FWP_BYTE_BLOB) -> Self {
        Self {
            conditions: Vec::new(),
            uint64: Vec::new(),
            v4: Vec::new(),
            v6: Vec::new(),
            application_id,
        }
    }

    fn push(&mut self, specification: &ConditionSpec) -> Result<(), WfpError> {
        let condition = match specification {
            ConditionSpec::Loopback => condition_u32(
                FWPM_CONDITION_FLAGS,
                FWP_MATCH_FLAGS_ALL_SET,
                FWP_CONDITION_FLAG_IS_LOOPBACK,
            ),
            ConditionSpec::InterfaceLuid(value) => {
                self.uint64.push(Box::new(*value));
                let value = self.uint64.last_mut().expect("just pushed");
                FWPM_FILTER_CONDITION0 {
                    fieldKey: FWPM_CONDITION_IP_LOCAL_INTERFACE,
                    matchType: FWP_MATCH_EQUAL,
                    conditionValue: FWP_CONDITION_VALUE0 {
                        r#type: FWP_UINT64,
                        Anonymous: FWP_CONDITION_VALUE0_0 {
                            uint64: value.as_mut(),
                        },
                    },
                }
            }
            ConditionSpec::ApplicationId => {
                if self.application_id.is_null() {
                    return Err(WfpError::MissingApplicationId);
                }
                FWPM_FILTER_CONDITION0 {
                    fieldKey: FWPM_CONDITION_ALE_APP_ID,
                    matchType: FWP_MATCH_EQUAL,
                    conditionValue: FWP_CONDITION_VALUE0 {
                        r#type: FWP_BYTE_BLOB_TYPE,
                        Anonymous: FWP_CONDITION_VALUE0_0 {
                            byteBlob: self.application_id,
                        },
                    },
                }
            }
            ConditionSpec::RemoteNetwork(IpNet::V4(network)) => {
                self.v4.push(Box::new(FWP_V4_ADDR_AND_MASK {
                    addr: u32::from_be_bytes(network.addr().octets()),
                    mask: prefix_mask(network.prefix_len()),
                }));
                let value = self.v4.last_mut().expect("just pushed");
                FWPM_FILTER_CONDITION0 {
                    fieldKey: FWPM_CONDITION_IP_REMOTE_ADDRESS,
                    matchType: FWP_MATCH_EQUAL,
                    conditionValue: FWP_CONDITION_VALUE0 {
                        r#type: FWP_V4_ADDR_MASK,
                        Anonymous: FWP_CONDITION_VALUE0_0 {
                            v4AddrMask: value.as_mut(),
                        },
                    },
                }
            }
            ConditionSpec::RemoteNetwork(IpNet::V6(network)) => {
                self.v6.push(Box::new(FWP_V6_ADDR_AND_MASK {
                    addr: network.addr().octets(),
                    prefixLength: network.prefix_len(),
                }));
                let value = self.v6.last_mut().expect("just pushed");
                FWPM_FILTER_CONDITION0 {
                    fieldKey: FWPM_CONDITION_IP_REMOTE_ADDRESS,
                    matchType: FWP_MATCH_EQUAL,
                    conditionValue: FWP_CONDITION_VALUE0 {
                        r#type: FWP_V6_ADDR_MASK,
                        Anonymous: FWP_CONDITION_VALUE0_0 {
                            v6AddrMask: value.as_mut(),
                        },
                    },
                }
            }
            ConditionSpec::Protocol(value) => {
                condition_u8(FWPM_CONDITION_IP_PROTOCOL, FWP_MATCH_EQUAL, *value)
            }
            ConditionSpec::LocalPort(value) => {
                condition_u16(FWPM_CONDITION_IP_LOCAL_PORT, FWP_MATCH_EQUAL, *value)
            }
            ConditionSpec::RemotePort(value) => {
                condition_u16(FWPM_CONDITION_IP_REMOTE_PORT, FWP_MATCH_EQUAL, *value)
            }
        };
        self.conditions.push(condition);
        Ok(())
    }

    fn len(&self) -> usize {
        self.conditions.len()
    }

    fn as_mut_ptr(&mut self) -> *mut FWPM_FILTER_CONDITION0 {
        if self.conditions.is_empty() {
            ptr::null_mut()
        } else {
            self.conditions.as_mut_ptr()
        }
    }
}

fn condition_u8(field_key: GUID, match_type: i32, value: u8) -> FWPM_FILTER_CONDITION0 {
    FWPM_FILTER_CONDITION0 {
        fieldKey: field_key,
        matchType: match_type,
        conditionValue: FWP_CONDITION_VALUE0 {
            r#type: FWP_UINT8,
            Anonymous: FWP_CONDITION_VALUE0_0 { uint8: value },
        },
    }
}

fn condition_u16(field_key: GUID, match_type: i32, value: u16) -> FWPM_FILTER_CONDITION0 {
    FWPM_FILTER_CONDITION0 {
        fieldKey: field_key,
        matchType: match_type,
        conditionValue: FWP_CONDITION_VALUE0 {
            r#type: FWP_UINT16,
            Anonymous: FWP_CONDITION_VALUE0_0 { uint16: value },
        },
    }
}

fn condition_u32(field_key: GUID, match_type: i32, value: u32) -> FWPM_FILTER_CONDITION0 {
    FWPM_FILTER_CONDITION0 {
        fieldKey: field_key,
        matchType: match_type,
        conditionValue: FWP_CONDITION_VALUE0 {
            r#type: FWP_UINT32,
            Anonymous: FWP_CONDITION_VALUE0_0 { uint32: value },
        },
    }
}

fn prefix_mask(prefix: u8) -> u32 {
    if prefix == 0 {
        0
    } else {
        u32::MAX << (32 - prefix)
    }
}

fn host_network(address: IpAddr) -> IpNet {
    match address {
        IpAddr::V4(address) => Ipv4Net::new(address, 32).expect("IPv4 host prefix").into(),
        IpAddr::V6(address) => Ipv6Net::new(address, 128).expect("IPv6 host prefix").into(),
    }
}

fn family_matches(family: AddressFamily, address: IpAddr) -> bool {
    matches!(
        (family, address),
        (AddressFamily::V4, IpAddr::V4(_)) | (AddressFamily::V6, IpAddr::V6(_))
    )
}

struct WfpEngine(HANDLE);

impl WfpEngine {
    fn open() -> Result<Self, WfpError> {
        let mut handle: HANDLE = ptr::null_mut();
        // SAFETY: all optional pointers are null and handle is writable.
        check("FwpmEngineOpen0", unsafe {
            FwpmEngineOpen0(
                ptr::null(),
                RPC_C_AUTHN_WINNT,
                ptr::null(),
                ptr::null(),
                &mut handle,
            )
        })?;
        if handle.is_null() {
            Err(WfpError::EmptyEngineHandle)
        } else {
            Ok(Self(handle))
        }
    }

    fn open_dynamic() -> Result<Self, WfpError> {
        let mut handle: HANDLE = ptr::null_mut();
        let session = FWPM_SESSION0 {
            flags: FWPM_SESSION_FLAG_DYNAMIC,
            ..Default::default()
        };
        // SAFETY: session is fully initialized and handle is writable.
        check("FwpmEngineOpen0 (dynamic)", unsafe {
            FwpmEngineOpen0(
                ptr::null(),
                RPC_C_AUTHN_WINNT,
                ptr::null(),
                &session,
                &mut handle,
            )
        })?;
        if handle.is_null() {
            Err(WfpError::EmptyEngineHandle)
        } else {
            Ok(Self(handle))
        }
    }
}

impl Drop for WfpEngine {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: this object uniquely owns the engine handle.
            unsafe {
                FwpmEngineClose0(self.0);
            }
        }
    }
}

struct WfpTransaction<'a> {
    engine: &'a WfpEngine,
    active: bool,
}

impl<'a> WfpTransaction<'a> {
    fn begin(engine: &'a WfpEngine) -> Result<Self, WfpError> {
        // SAFETY: engine is live.
        check("FwpmTransactionBegin0", unsafe {
            FwpmTransactionBegin0(engine.0, 0)
        })?;
        Ok(Self {
            engine,
            active: true,
        })
    }

    fn commit(mut self) -> Result<(), WfpError> {
        // SAFETY: this transaction is active on the live engine.
        check("FwpmTransactionCommit0", unsafe {
            FwpmTransactionCommit0(self.engine.0)
        })?;
        self.active = false;
        Ok(())
    }
}

impl Drop for WfpTransaction<'_> {
    fn drop(&mut self) {
        if self.active {
            // SAFETY: aborting an active transaction is idempotent cleanup.
            unsafe {
                FwpmTransactionAbort0(self.engine.0);
            }
        }
    }
}

struct ApplicationId(*mut FWP_BYTE_BLOB);

impl ApplicationId {
    fn from_path(path: &Path) -> Result<Self, WfpError> {
        let path = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let mut application_id = ptr::null_mut();
        // SAFETY: path is null-terminated and application_id is writable.
        check("FwpmGetAppIdFromFileName0", unsafe {
            FwpmGetAppIdFromFileName0(path.as_ptr(), &mut application_id)
        })?;
        if application_id.is_null() {
            Err(WfpError::MissingApplicationId)
        } else {
            Ok(Self(application_id))
        }
    }

    fn as_ptr(&self) -> *mut FWP_BYTE_BLOB {
        self.0
    }
}

impl Drop for ApplicationId {
    fn drop(&mut self) {
        if !self.0.is_null() {
            let mut allocation = self.0.cast::<c_void>();
            // SAFETY: WFP allocated this object and requires FwpmFreeMemory0.
            unsafe {
                FwpmFreeMemory0(&mut allocation);
            }
            self.0 = ptr::null_mut();
        }
    }
}

fn check(operation: &'static str, code: u32) -> Result<(), WfpError> {
    if code == 0 {
        Ok(())
    } else {
        Err(WfpError::Windows { operation, code })
    }
}

fn check_allow_existing(operation: &'static str, code: u32) -> Result<(), WfpError> {
    if code == 0 || code == FWP_E_ALREADY_EXISTS as u32 {
        Ok(())
    } else {
        Err(WfpError::Windows { operation, code })
    }
}

fn retain_delete_error(
    destination: &mut Option<WfpError>,
    operation: &'static str,
    code: u32,
    not_found: u32,
) {
    if code != 0 && code != not_found && destination.is_none() {
        *destination = Some(WfpError::Windows { operation, code });
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
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

fn uuid_from_guid(value: GUID) -> Uuid {
    Uuid::from_fields(value.data1, value.data2, value.data3, &value.data4)
}

#[derive(Debug, Error)]
pub enum WfpError {
    #[error("{operation} failed with Windows/WFP error 0x{code:08X}")]
    Windows { operation: &'static str, code: u32 },
    #[error("WFP receipt has the wrong mutation kind")]
    ReceiptKind,
    #[error("Wintun interface LUID must not be zero")]
    EmptyLuid,
    #[error("WFP rule count is outside the audited bound: {0}")]
    RuleCount(usize),
    #[error("WFP receipt has {actual} filter keys; {expected} are required")]
    FilterKeyCount { expected: usize, actual: usize },
    #[error("authenticated Engine path must be absolute")]
    EnginePath,
    #[error("WFP did not return an application identifier")]
    MissingApplicationId,
    #[error("WFP did not return an engine handle")]
    EmptyEngineHandle,
    #[error("WFP did not return a persistent filter ID")]
    EmptyFilterId,
    #[error("dynamic direct target must be a usable numeric TCP/UDP endpoint")]
    UnsafeDynamicTarget,
    #[error("invalid built-in network prefix: {0}")]
    StaticNetwork(&'static str),
}

#[cfg(test)]
mod tests {
    use std::{
        net::{Ipv4Addr, Ipv6Addr, SocketAddrV6},
        str::FromStr,
    };

    use super::*;

    fn plan(endpoint: IpAddr, allow_lan: bool) -> ValidatedTunnelPlan {
        let endpoint = match endpoint {
            IpAddr::V4(address) => (address, 443).into(),
            IpAddr::V6(address) => SocketAddrV6::new(address, 443, 0, 0).into(),
        };
        ValidatedTunnelPlan {
            profile_id: Uuid::new_v4(),
            endpoint,
            endpoint_candidates: vec![endpoint],
            control_api_candidates: vec![(Ipv4Addr::new(198, 51, 100, 10), 443).into()],
            mtu: 1280,
            dns_servers: vec![Ipv4Addr::new(1, 1, 1, 1).into()],
            split_exclusions: vec![IpNet::from_str("198.51.100.0/24").expect("CIDR")],
            allow_lan,
            kill_switch: true,
            split_dns: false,
            assigned_ipv4: Some(IpNet::from_str("172.16.0.2/32").expect("CIDR")),
            assigned_ipv6: Some(IpNet::from_str("2606:4700:110::2/128").expect("CIDR")),
        }
    }

    #[test]
    fn every_family_ends_in_a_terminal_block() {
        let rules =
            build_rules(&plan(Ipv4Addr::new(162, 159, 198, 2).into(), false), 42).expect("rules");
        for family in [AddressFamily::V4, AddressFamily::V6] {
            let family_rules = rules
                .iter()
                .filter(|rule| rule.family == family)
                .collect::<Vec<_>>();
            assert!(!family_rules.is_empty());
            assert_eq!(family_rules.last().expect("last").action, RuleAction::Block);
            assert!(family_rules.last().expect("last").conditions.is_empty());
        }
    }

    #[test]
    fn recovery_resource_keys_are_stable_unique_and_bounded() {
        let keys = (0..MAX_FILTERS).map(filter_key).collect::<BTreeSet<_>>();
        assert_eq!(keys.len(), MAX_FILTERS);
        assert!(!keys.contains(&PROVIDER_KEY));
        assert!(!keys.contains(&SUBLAYER_KEY));
        assert_ne!(PROVIDER_KEY, SUBLAYER_KEY);
    }

    #[test]
    fn endpoint_permits_are_bound_to_engine_address_port_and_protocol() {
        let endpoint = Ipv6Addr::from_str("2606:4700:103::2").expect("IPv6");
        let rules = build_rules(&plan(endpoint.into(), false), 42).expect("rules");
        let endpoint_rules = rules
            .iter()
            .filter(|rule| rule.name.starts_with("Engine H2") || rule.name.starts_with("Engine H3"))
            .collect::<Vec<_>>();
        assert_eq!(endpoint_rules.len(), 2);
        for rule in endpoint_rules {
            assert!(rule.conditions.contains(&ConditionSpec::ApplicationId));
            assert!(
                rule.conditions
                    .contains(&ConditionSpec::RemoteNetwork(host_network(endpoint.into())))
            );
            assert!(rule.conditions.contains(&ConditionSpec::RemotePort(443)));
            assert!(
                rule.conditions
                    .iter()
                    .any(|condition| matches!(condition, ConditionSpec::Protocol(_)))
            );
        }
        let control = rules
            .iter()
            .find(|rule| rule.name.starts_with("Engine authenticated control API"))
            .expect("control API permit");
        assert!(control.conditions.contains(&ConditionSpec::ApplicationId));
        assert!(control.conditions.contains(&ConditionSpec::RemotePort(443)));
        assert!(
            control
                .conditions
                .contains(&ConditionSpec::Protocol(IPPROTO_TCP as u8))
        );
        assert!(
            control
                .conditions
                .contains(&ConditionSpec::RemoteNetwork(host_network(
                    Ipv4Addr::new(198, 51, 100, 10).into()
                )))
        );
    }

    #[test]
    fn dynamic_direct_permit_is_exact_and_application_scoped() {
        let remote = SocketAddr::from((Ipv4Addr::new(203, 0, 113, 9), 53));
        let rule = dynamic_direct_rule(remote, IPPROTO_UDP as u8, 42).expect("rule");
        assert_eq!(rule.action, RuleAction::Permit);
        assert_eq!(rule.family, AddressFamily::V4);
        assert_eq!(
            rule.conditions,
            vec![
                ConditionSpec::ApplicationId,
                ConditionSpec::InterfaceLuid(42),
                ConditionSpec::RemoteNetwork(host_network(remote.ip())),
                ConditionSpec::RemotePort(53),
                ConditionSpec::Protocol(IPPROTO_UDP as u8),
            ]
        );
        assert!(dynamic_direct_rule(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)), 17, 42).is_err());
        assert!(dynamic_direct_rule(remote, 1, 42).is_err());
    }

    #[test]
    fn lan_bypass_is_explicit_and_never_becomes_a_default_route() {
        let rules =
            build_rules(&plan(Ipv4Addr::new(162, 159, 198, 2).into(), true), 42).expect("rules");
        let networks = rules
            .iter()
            .flat_map(|rule| rule.conditions.iter())
            .filter_map(|condition| match condition {
                ConditionSpec::RemoteNetwork(network) => Some(*network),
                _ => None,
            })
            .collect::<BTreeSet<_>>();
        assert!(networks.contains(&IpNet::from_str("10.0.0.0/8").expect("CIDR")));
        assert!(networks.iter().all(|network| network.prefix_len() != 0));
    }

    #[test]
    fn ipv4_masks_are_in_host_order() {
        assert_eq!(prefix_mask(0), 0);
        assert_eq!(prefix_mask(8), 0xff00_0000);
        assert_eq!(prefix_mask(24), 0xffff_ff00);
        assert_eq!(prefix_mask(32), u32::MAX);
    }
}
