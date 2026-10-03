//! Persistent replacement protection owns a namespace independent of either
//! tunnel operation. Ordinary rollback cannot remove its filters or metadata.
use super::*;
use crate::journal::{
    REPLACEMENT_FILTER_KEY_BASE, REPLACEMENT_MAX_FILTERS, REPLACEMENT_WFP_PROVIDER_KEY,
    REPLACEMENT_WFP_SUBLAYER_KEY, ReplacementGuardPlan,
};
use windows_sys::Win32::NetworkManagement::WindowsFilteringPlatform::{
    FWP_FILTER_ENUM_FLAG_INCLUDE_DISABLED, FWP_FILTER_ENUM_FULLY_CONTAINED,
    FWPM_FILTER_ENUM_TEMPLATE0, FWPM_FILTER_FLAG_DISABLED, FwpmFilterCreateEnumHandle0,
    FwpmFilterDestroyEnumHandle0, FwpmFilterEnum0,
};

// Only the coordinator's automatic MASQUE/control leases may be mirrored here.
// Static bootstrap endpoints are already permitted by the immutable guard plan.
const MAX_CONTROL_MIRRORS: usize = 16;
const MAX_OWNED_FILTERS: usize = REPLACEMENT_MAX_FILTERS + MAX_CONTROL_MIRRORS;
const ENUM_PAGE: u32 = 64;

fn replacement_filter_key(index: usize) -> Uuid {
    debug_assert!(index < REPLACEMENT_MAX_FILTERS);
    Uuid::from_u128(REPLACEMENT_FILTER_KEY_BASE + index as u128)
}

fn receipt_keys(receipt: &MutationReceipt) -> Result<&[Uuid], WfpError> {
    let MutationReceipt::KillSwitch {
        provider_key,
        sublayer_key,
        filter_keys,
        filter_ids,
    } = receipt
    else {
        return Err(WfpError::ReceiptKind);
    };
    if *provider_key != REPLACEMENT_WFP_PROVIDER_KEY
        || *sublayer_key != REPLACEMENT_WFP_SUBLAYER_KEY
        || filter_keys.is_empty()
        || filter_keys.len() > REPLACEMENT_MAX_FILTERS
        || filter_keys
            .iter()
            .enumerate()
            .any(|(i, key)| *key != replacement_filter_key(i))
        || !filter_ids.is_empty()
            && (filter_ids.len() != filter_keys.len()
                || filter_ids.contains(&0)
                || filter_ids.iter().copied().collect::<BTreeSet<_>>().len() != filter_ids.len())
    {
        return Err(WfpError::ReplacementGuard);
    }
    Ok(filter_keys)
}

fn build_replacement_rules(plan: &ReplacementGuardPlan) -> Result<Vec<FilterRule>, WfpError> {
    plan.target
        .validate()
        .map_err(|_| WfpError::ReplacementGuard)?;
    let allowed = effective_exclusions(&plan.target)?;
    let exclusions = plan
        .exclusions
        .iter()
        .map(|network| network.trunc())
        .collect::<BTreeSet<_>>();
    if exclusions.iter().any(|network| {
        network.prefix_len() == 0
            || !allowed.iter().any(|target| {
                target.prefix_len() <= network.prefix_len() && target.contains(&network.network())
            })
    }) {
        return Err(WfpError::ReplacementGuard);
    }
    // Bootstrap families are independent of the future tunnel's assignments.
    // In particular, neither an absent IPv6 assignment nor a retired LUID
    // may leave a physical family outside the replacement block policy.
    let mut bootstrap = plan.target.clone();
    bootstrap.vpn_chain = true;
    let mut rules = Vec::new();
    for family in [AddressFamily::V4, AddressFamily::V6] {
        rules.push(permit(
            family,
            "Replacement loopback",
            vec![ConditionSpec::Loopback],
        ));
        for (endpoint, protocol, label) in bootstrap_endpoints(&bootstrap)
            .filter(|(endpoint, _, _)| family_matches(family, endpoint.ip()))
        {
            rules.push(permit(
                family,
                label,
                vec![
                    ConditionSpec::ApplicationId,
                    ConditionSpec::RemoteNetwork(host_network(endpoint.ip())),
                    ConditionSpec::RemotePort(endpoint.port()),
                    ConditionSpec::Protocol(protocol),
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
                "Replacement shared bypass",
                vec![ConditionSpec::RemoteNetwork(exclusion)],
            ));
        }
        add_link_control_rules(&mut rules, family);
        rules.push(FilterRule {
            name: format!("Replacement block all {family:?} physical traffic"),
            family,
            action: RuleAction::Block,
            weight: BLOCK_WEIGHT,
            conditions: Vec::new(),
        });
    }
    if rules.len() > REPLACEMENT_MAX_FILTERS {
        return Err(WfpError::ReplacementCapacity);
    }
    Ok(rules)
}

pub fn plan_replacement_guard(plan: &ReplacementGuardPlan) -> Result<MutationReceipt, WfpError> {
    let rules = build_replacement_rules(plan)?;
    Ok(MutationReceipt::KillSwitch {
        provider_key: REPLACEMENT_WFP_PROVIDER_KEY,
        sublayer_key: REPLACEMENT_WFP_SUBLAYER_KEY,
        filter_keys: (0..rules.len()).map(replacement_filter_key).collect(),
        filter_ids: Vec::new(),
    })
}

/// Both first installation and retargeting use one WFP transaction. A failed
/// replacement restores the entire old guard, including its dynamic mirrors.
pub fn apply_replacement_guard(
    mut receipt: MutationReceipt,
    plan: &ReplacementGuardPlan,
    engine_path: &Path,
) -> Result<MutationReceipt, WfpError> {
    if !engine_path.is_absolute() {
        return Err(WfpError::EnginePath);
    }
    let keys = receipt_keys(&receipt)?.to_vec();
    let rules = build_replacement_rules(plan)?;
    if keys.len() != rules.len() {
        return Err(WfpError::FilterKeyCount {
            expected: rules.len(),
            actual: keys.len(),
        });
    }
    let engine = WfpEngine::open()?;
    let application = ApplicationId::from_path(engine_path)?;
    let transaction = WfpTransaction::begin(&engine)?;
    add_provider(&engine, REPLACEMENT_WFP_PROVIDER_KEY)?;
    add_sublayer(
        &engine,
        REPLACEMENT_WFP_PROVIDER_KEY,
        REPLACEMENT_WFP_SUBLAYER_KEY,
    )?;
    let previous = owned_filters(&engine)?;
    delete_filter_keys(&engine, &previous.keys)?;
    let mut ids = Vec::with_capacity(keys.len());
    for (index, (rule, key)) in rules.iter().zip(keys).enumerate() {
        ids.push(add_filter(
            &engine,
            REPLACEMENT_WFP_PROVIDER_KEY,
            REPLACEMENT_WFP_SUBLAYER_KEY,
            key,
            index,
            rule,
            application.as_ptr(),
            FWPM_FILTER_FLAG_PERSISTENT,
        )?);
    }
    transaction.commit()?;
    if let MutationReceipt::KillSwitch { filter_ids, .. } = &mut receipt {
        *filter_ids = ids;
    }
    Ok(receipt)
}

/// An Applied journal or surviving metadata is insufficient: every planned
/// static filter and both unconditional family blocks must exist natively.
pub fn replacement_guard_present(receipt: &MutationReceipt) -> Result<bool, WfpError> {
    let keys = receipt_keys(receipt)?;
    let engine = WfpEngine::open()?;
    let mut blocked = [false; 2];
    for key in keys {
        let mut raw = ptr::null_mut();
        // SAFETY: the engine and key live through the call; raw is writable.
        let status = unsafe { FwpmFilterGetByKey0(engine.0, &guid_from_uuid(*key), &mut raw) };
        let allocation = WfpFilterAllocation::new(raw);
        if status == FWP_E_FILTER_NOT_FOUND as u32 {
            return Ok(false);
        }
        check("FwpmFilterGetByKey0 (replacement)", status)?;
        let Some(allocation) = allocation else {
            return Ok(false);
        };
        if !allocation.matches(
            REPLACEMENT_WFP_PROVIDER_KEY,
            REPLACEMENT_WFP_SUBLAYER_KEY,
            true,
        ) {
            return Ok(false);
        }
        // SAFETY: allocation owns the non-null filter until this scope ends.
        let filter = unsafe { allocation.0.as_ref() };
        if filter.flags & FWPM_FILTER_FLAG_DISABLED != 0 {
            return Ok(false);
        }
        note_block(filter, &mut blocked);
    }
    Ok(blocked == [true, true])
}

pub fn restore_replacement_guard(receipt: &MutationReceipt) -> Result<(), WfpError> {
    receipt_keys(receipt)?;
    remove_replacement_resources()
}

pub(super) fn remove_replacement_resources() -> Result<(), WfpError> {
    let engine = WfpEngine::open()?;
    let transaction = WfpTransaction::begin(&engine)?;
    // Mirrors can outlive Commit through held new-underlay leases. Delete
    // only their guard halves; the normal sublayer permits stay live.
    delete_filter_keys(&engine, &owned_filters(&engine)?.keys)?;
    let mut first_error = None;
    retain_delete_error(
        &mut first_error,
        "FwpmSubLayerDeleteByKey0 (replacement)",
        // SAFETY: the engine and fixed replacement namespace key are live.
        unsafe {
            FwpmSubLayerDeleteByKey0(engine.0, &guid_from_uuid(REPLACEMENT_WFP_SUBLAYER_KEY))
        },
        FWP_E_SUBLAYER_NOT_FOUND as u32,
    );
    retain_delete_error(
        &mut first_error,
        "FwpmProviderDeleteByKey0 (replacement)",
        // SAFETY: the engine and fixed replacement namespace key are live.
        unsafe {
            FwpmProviderDeleteByKey0(engine.0, &guid_from_uuid(REPLACEMENT_WFP_PROVIDER_KEY))
        },
        FWP_E_PROVIDER_NOT_FOUND as u32,
    );
    if let Some(error) = first_error {
        return Err(error);
    }
    transaction.commit()
}

/// The authenticated coordinator validates the immutable bootstrap role and
/// network generation before calling this function. No business lease enters
/// the guard. Both exact permits belong to one dynamic WFP session.
pub fn acquire_replacement_control_permit(
    remote: SocketAddr,
    protocol: u8,
    interface_luid: u64,
    engine_path: &Path,
) -> Result<DynamicPermit, WfpError> {
    if !engine_path.is_absolute() {
        return Err(WfpError::EnginePath);
    }
    let rule = dynamic_direct_rule(remote, protocol, interface_luid)?;
    let engine = WfpEngine::open_dynamic()?;
    let application = ApplicationId::from_path(engine_path)?;
    let transaction = WfpTransaction::begin(&engine)?;
    let guard = owned_filters(&engine)?;
    if guard.blocked != [true, true] {
        return Err(WfpError::ReplacementNotPresent);
    }
    if guard.mirrors >= MAX_CONTROL_MIRRORS {
        return Err(WfpError::ReplacementCapacity);
    }
    let filter_keys = vec![Uuid::new_v4(), Uuid::new_v4()];
    for ((provider, sublayer), key) in control_layers().into_iter().zip(&filter_keys) {
        add_filter(
            &engine,
            provider,
            sublayer,
            *key,
            0,
            &rule,
            application.as_ptr(),
            0,
        )?;
    }
    transaction.commit()?;
    Ok(DynamicPermit {
        engine,
        filter_keys,
    })
}

fn control_layers() -> [(Uuid, Uuid); 2] {
    [
        (PROVIDER_KEY, SUBLAYER_KEY),
        (REPLACEMENT_WFP_PROVIDER_KEY, REPLACEMENT_WFP_SUBLAYER_KEY),
    ]
}

fn delete_filter_keys(engine: &WfpEngine, keys: &[Uuid]) -> Result<(), WfpError> {
    for key in keys.iter().rev() {
        // SAFETY: engine is live and the key was enumerated in this transaction.
        let result = unsafe { FwpmFilterDeleteByKey0(engine.0, &guid_from_uuid(*key)) };
        if result != FWP_E_FILTER_NOT_FOUND as u32 {
            check("FwpmFilterDeleteByKey0 (replacement)", result)?;
        }
    }
    Ok(())
}

#[derive(Default)]
struct OwnedFilters {
    keys: Vec<Uuid>,
    mirrors: usize,
    blocked: [bool; 2],
}

fn note_block(filter: &FWPM_FILTER0, blocked: &mut [bool; 2]) {
    if filter.flags & FWPM_FILTER_FLAG_PERSISTENT == 0
        || filter.flags & FWPM_FILTER_FLAG_DISABLED != 0
        || filter.action.r#type != FWP_ACTION_BLOCK
        || filter.numFilterConditions != 0
    {
        return;
    }
    for (index, layer) in [
        FWPM_LAYER_ALE_AUTH_CONNECT_V4,
        FWPM_LAYER_ALE_AUTH_CONNECT_V6,
    ]
    .into_iter()
    .enumerate()
    {
        if uuid_from_guid(filter.layerKey) == uuid_from_guid(layer) {
            blocked[index] = true;
        }
    }
}

fn owned_filters(engine: &WfpEngine) -> Result<OwnedFilters, WfpError> {
    let mut result = OwnedFilters::default();
    for layer in [
        FWPM_LAYER_ALE_AUTH_CONNECT_V4,
        FWPM_LAYER_ALE_AUTH_CONNECT_V6,
    ] {
        let mut provider = guid_from_uuid(REPLACEMENT_WFP_PROVIDER_KEY);
        let template = FWPM_FILTER_ENUM_TEMPLATE0 {
            providerKey: &mut provider,
            layerKey: layer,
            enumType: FWP_FILTER_ENUM_FULLY_CONTAINED,
            flags: FWP_FILTER_ENUM_FLAG_INCLUDE_DISABLED,
            actionMask: u32::MAX,
            ..Default::default()
        };
        let mut handle = ptr::null_mut();
        // SAFETY: template pointers and the output handle live through the call.
        let status = unsafe { FwpmFilterCreateEnumHandle0(engine.0, &template, &mut handle) };
        if status == FWP_E_PROVIDER_NOT_FOUND as u32 {
            continue;
        }
        check("FwpmFilterCreateEnumHandle0 (replacement)", status)?;
        if handle.is_null() {
            return Err(WfpError::FilterEnumeration);
        }
        let enumeration = FilterEnumeration { engine, handle };
        loop {
            let mut entries = ptr::null_mut();
            let mut count = 0;
            // SAFETY: enumeration is live; entries/count are writable outputs.
            let status = unsafe {
                FwpmFilterEnum0(
                    engine.0,
                    enumeration.handle,
                    ENUM_PAGE,
                    &mut entries,
                    &mut count,
                )
            };
            let page = FilterPage(entries);
            check("FwpmFilterEnum0 (replacement)", status)?;
            if count == 0 {
                break;
            }
            if count > ENUM_PAGE || entries.is_null() {
                return Err(WfpError::FilterEnumeration);
            }
            for index in 0..count as usize {
                // SAFETY: a successful enumeration supplies count pointers in
                // the allocation owned by page, retained until this loop ends.
                let filter = unsafe { entries.add(index).read().as_ref() }
                    .ok_or(WfpError::FilterEnumeration)?;
                let Some(provider) = NonNull::new(filter.providerKey) else {
                    return Err(WfpError::ReplacementGuard);
                };
                // SAFETY: providerKey is part of this live WFP filter allocation.
                let provider = unsafe { provider.as_ref() };
                if uuid_from_guid(*provider) != REPLACEMENT_WFP_PROVIDER_KEY
                    || uuid_from_guid(filter.subLayerKey) != REPLACEMENT_WFP_SUBLAYER_KEY
                {
                    return Err(WfpError::ReplacementGuard);
                }
                result.keys.push(uuid_from_guid(filter.filterKey));
                if result.keys.len() > MAX_OWNED_FILTERS {
                    return Err(WfpError::ReplacementCapacity);
                }
                if filter.flags & FWPM_FILTER_FLAG_PERSISTENT == 0 {
                    result.mirrors += 1;
                }
                note_block(filter, &mut result.blocked);
            }
            drop(page);
        }
    }
    Ok(result)
}

struct FilterEnumeration<'a> {
    engine: &'a WfpEngine,
    handle: HANDLE,
}
impl Drop for FilterEnumeration<'_> {
    fn drop(&mut self) {
        // SAFETY: this guard exclusively owns the enumeration handle and its
        // engine borrow prevents closing the engine first.
        unsafe {
            FwpmFilterDestroyEnumHandle0(self.engine.0, self.handle);
        }
    }
}
struct FilterPage(*mut *mut FWPM_FILTER0);
impl Drop for FilterPage {
    fn drop(&mut self) {
        if !self.0.is_null() {
            let mut allocation = self.0.cast::<c_void>();
            // SAFETY: one successful WFP enumeration owns this pointer array
            // and its filter allocations; it is freed exactly once as a unit.
            unsafe {
                FwpmFreeMemory0(&mut allocation);
            }
        }
    }
}

#[cfg(test)]
mod tests;
