use super::*;

pub(crate) fn effective_exclusions(plan: &ValidatedTunnelPlan) -> Vec<ipnet::IpNet> {
    let mut networks = plan.split_exclusions.clone();
    if plan.allow_lan {
        networks.extend(
            [
                "10.0.0.0/8",
                "172.16.0.0/12",
                "192.168.0.0/16",
                "169.254.0.0/16",
                "fc00::/7",
                "fe80::/10",
            ]
            .into_iter()
            .map(|value| value.parse::<ipnet::IpNet>().expect("static LAN network")),
        );
    }
    networks.sort();
    networks.dedup();
    networks
}

pub(crate) fn intersect_exclusions(
    current: &[ipnet::IpNet],
    target: &ValidatedTunnelPlan,
) -> Vec<ipnet::IpNet> {
    let next = effective_exclusions(target);
    let mut intersection = std::collections::BTreeSet::new();
    for left in current {
        for right in &next {
            if contains_network(left, right) {
                intersection.insert(*right);
            } else if contains_network(right, left) {
                intersection.insert(*left);
            }
        }
    }
    let all = intersection.iter().copied().collect::<Vec<_>>();
    all.iter()
        .copied()
        .filter(|network| {
            !all.iter()
                .any(|other| other != network && contains_network(other, network))
        })
        .collect()
}

fn contains_network(outer: &ipnet::IpNet, inner: &ipnet::IpNet) -> bool {
    outer.prefix_len() <= inner.prefix_len() && outer.contains(&inner.addr())
}

/// Only exact bootstrap targets and the already intersected static bypasses.
/// No interface identity is permitted while the Engine has quiesced admission.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ReplacementGuardPlan {
    pub exclusions: Vec<ipnet::IpNet>,
    pub target: ValidatedTunnelPlan,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReplacementPhase {
    InstallingGuard,
    SourceCleanup,
    PreparingTarget,
    Prepared,
    Committing,
    Aborting,
    Complete,
    Aborted,
}

impl ReplacementPhase {
    pub fn pending(self) -> bool {
        !matches!(self, Self::Complete | Self::Aborted)
    }

    pub fn to_proto(self) -> i32 {
        use usque_ipc::agent_v1::TunnelReplacementPhase as Phase;
        let phase = match self {
            Self::InstallingGuard => Phase::InstallingGuard,
            Self::SourceCleanup => Phase::SourceCleanup,
            Self::PreparingTarget => Phase::PreparingTarget,
            Self::Prepared => Phase::Prepared,
            Self::Committing => Phase::Committing,
            Self::Aborting => Phase::Aborting,
            Self::Complete => Phase::Complete,
            Self::Aborted => Phase::Aborted,
        };
        phase as i32
    }
}

/// A bounded write-ahead transaction kept outside ordinary connection steps.
/// Exact request equality is its idempotency key; it carries no credentials.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TunnelReplacement {
    pub original_source_operation_id: Uuid,
    pub original_source_plan: ValidatedTunnelPlan,
    pub source_operation_id: Uuid,
    pub cleanup_operation_id: Uuid,
    pub source_journal_generation: u64,
    pub operation_id: Uuid,
    pub owner_sid: String,
    pub owner_process_id: u32,
    pub guard_plan: ReplacementGuardPlan,
    pub guard: MutationRecord,
    pub phase: ReplacementPhase,
    pub abort_generation: Option<u64>,
}

impl TunnelReplacement {
    pub fn pending(&self) -> bool {
        self.phase.pending()
    }

    pub fn validate(&self) -> Result<(), JournalError> {
        if self.original_source_operation_id.is_nil()
            || self.source_operation_id.is_nil()
            || self.cleanup_operation_id.is_nil()
            || self.operation_id.is_nil()
            || self.source_operation_id == self.operation_id
            || self.source_journal_generation == 0
            || self.owner_process_id == 0
            || !valid_sid_text(&self.owner_sid)
            || !(self.original_source_plan.kill_switch || self.original_source_plan.vpn_chain)
        {
            return Err(JournalError::InvalidReplacement);
        }
        self.original_source_plan.validate()?;
        self.guard_plan.target.validate()?;
        if self.guard_plan.exclusions.len() > 2 * (crate::plan::MAX_SPLIT_EXCLUSIONS + 6)
            || self
                .guard_plan
                .exclusions
                .iter()
                .any(|network| network.prefix_len() == 0)
            || self.guard.kind != MutationKind::KillSwitch
        {
            return Err(JournalError::InvalidReplacement);
        }
        let original = effective_exclusions(&self.original_source_plan);
        let target = effective_exclusions(&self.guard_plan.target);
        if self.guard_plan.exclusions.iter().any(|network| {
            !original
                .iter()
                .any(|outer| contains_network(outer, network))
                || !target.iter().any(|outer| contains_network(outer, network))
        }) {
            return Err(JournalError::InvalidReplacement);
        }
        let MutationReceipt::KillSwitch {
            provider_key,
            sublayer_key,
            filter_keys,
            filter_ids,
        } = &self.guard.receipt
        else {
            return Err(JournalError::InvalidReplacement);
        };
        if *provider_key != REPLACEMENT_WFP_PROVIDER_KEY
            || *sublayer_key != REPLACEMENT_WFP_SUBLAYER_KEY
            || filter_keys.is_empty()
            || filter_keys.len() > REPLACEMENT_MAX_FILTERS
            || filter_keys.iter().enumerate().any(|(index, key)| {
                *key != Uuid::from_u128(REPLACEMENT_FILTER_KEY_BASE + index as u128)
            })
            || !filter_ids.is_empty() && filter_ids.len() != filter_keys.len()
            || !self.pending() && self.guard.state != MutationState::Restored
            || self.pending() && self.guard.state == MutationState::Restored
        {
            return Err(JournalError::InvalidReplacement);
        }
        Ok(())
    }
}
