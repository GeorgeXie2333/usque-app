//! Only explicit disconnect aborts an owned replacement. Failed or ambiguous
//! requests retain their identity and never enter ordinary Agent recovery.
use super::*;

pub(super) fn pending_replacement(
    state: &AgentState,
) -> Result<Option<&agent_v1::TunnelReplacementStatus>, WindowsVpnError> {
    let Some(replacement) = state.replacement.as_deref() else {
        return Ok(None);
    };
    use agent_v1::TunnelReplacementPhase as Phase;
    match Phase::try_from(replacement.phase) {
        Ok(Phase::Complete | Phase::Aborted) => Ok(None),
        Ok(
            Phase::InstallingGuard
            | Phase::SourceCleanup
            | Phase::PreparingTarget
            | Phase::Prepared
            | Phase::Committing
            | Phase::Aborting,
        ) if [
            replacement.source_operation_id.as_str(),
            replacement.operation_id.as_str(),
        ]
        .into_iter()
        .all(|id| Uuid::parse_str(id).is_ok_and(|id| !id.is_nil()))
            && replacement.source_operation_id != replacement.operation_id
            && replacement.target_plan.is_some() =>
        {
            Ok(Some(replacement))
        }
        _ => Err(WindowsVpnError::RecoveryConflict),
    }
}

pub(super) fn owned_pending_replacement<'a>(
    state: &'a AgentState,
    operation_id: Uuid,
    request: Option<&agent_v1::ReplaceTunnelRequest>,
) -> Result<Option<&'a agent_v1::TunnelReplacementStatus>, WindowsVpnError> {
    let Some(replacement) = pending_replacement(state)? else {
        return Ok(None);
    };
    if replacement.operation_id == operation_id.to_string()
        || request.is_some_and(|request| replacement_matches(replacement, request))
    {
        Ok(Some(replacement))
    } else {
        // A source ID is not ownership of its successor. Old attempts cannot
        // abort a retarget even when both attempts use the same profile.
        Err(WindowsVpnError::RecoveryConflict)
    }
}

fn replacement_matches(
    replacement: &agent_v1::TunnelReplacementStatus,
    request: &agent_v1::ReplaceTunnelRequest,
) -> bool {
    replacement.operation_id == request.operation_id
        && replacement.source_operation_id == request.source_operation_id
        && replacement.target_plan.as_deref() == request.plan.as_ref()
}

pub(super) fn canonical_plan(mut plan: agent_v1::TunnelPlan) -> agent_v1::TunnelPlan {
    // Match Agent validation's set ordering before recording the immutable
    // request. Otherwise an automatic IPv6 preference can make its own reply
    // look like a different replacement after numeric candidates are sorted.
    for values in [
        &mut plan.endpoint_candidates,
        &mut plan.control_api_candidates,
    ] {
        values.sort_by_key(|value| value.parse::<SocketAddr>().ok());
        values.dedup();
    }
    plan.split_exclusions
        .sort_by_key(|value| value.parse::<ipnet::IpNet>().ok());
    plan.split_exclusions.dedup();
    plan
}

pub(super) fn target_is_committed(state: &AgentState, operation_id: Uuid) -> bool {
    state.phase == agent_v1::AgentPhase::Active as i32
        && state.operation_id == operation_id.to_string()
        && pending_replacement(state).is_ok_and(|pending| pending.is_none())
}

pub(super) fn abort_completed(
    state: &AgentState,
    expected: &agent_v1::TunnelReplacementStatus,
    generation: u64,
) -> bool {
    require_recovered_state(state).is_ok()
        && state.journal_generation >= generation
        && state.replacement.as_deref().is_some_and(|actual| {
            actual.phase == agent_v1::TunnelReplacementPhase::Aborted as i32
                && actual.operation_id == expected.operation_id
                && actual.source_operation_id == expected.source_operation_id
                && actual.source_journal_generation == expected.source_journal_generation
                && actual.target_plan == expected.target_plan
                && !actual.guard_active
        })
}

pub(super) fn active_chain_needs_handoff(state: &AgentState, requested_profile: Uuid) -> bool {
    state.phase == agent_v1::AgentPhase::Active as i32
        && (state.plan.as_ref().is_some_and(|plan| plan.vpn_chain)
            || state.kill_switch_active && state.profile_id != requested_profile.to_string())
}

fn validate_bootstrap_candidates(
    mut candidates: Vec<SocketAddr>,
) -> Result<Vec<SocketAddr>, WindowsVpnError> {
    if candidates.is_empty()
        || candidates.len() > 16
        || candidates.iter().any(|candidate| {
            candidate.port() != REGISTRATION_API_PORT
                || candidate.ip().is_unspecified()
                || candidate.ip().is_multicast()
                || candidate.ip().is_loopback()
        })
    {
        return Err(WindowsVpnError::ControlEndpointResolution(
            "protected reconnect has no validated registration API candidates".into(),
        ));
    }
    candidates.sort();
    candidates.dedup();
    Ok(candidates)
}

pub(super) fn state_bootstrap_candidates(
    state: &AgentState,
) -> Result<Vec<SocketAddr>, WindowsVpnError> {
    let plan = pending_replacement(state)?
        .and_then(|r| r.target_plan.as_deref())
        .or(state.plan.as_deref())
        .ok_or(WindowsVpnError::MissingMasqueRuntime)?;
    let candidates = plan
        .control_api_candidates
        .iter()
        .map(|value| value.parse::<SocketAddr>())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| {
            WindowsVpnError::ControlEndpointResolution(
                "invalid protected registration API candidates".into(),
            )
        })?;
    validate_bootstrap_candidates(candidates)
}

impl WindowsVpnRuntime {
    pub(crate) fn failure_retained(&self) -> bool {
        self.held_failure
    }
    pub(crate) fn replacement_pending(&self) -> bool {
        (self.handoff_intent || self.replacement_request.is_some()) && !self.target_committed
    }
    pub(crate) fn target_committed(&self) -> bool {
        self.target_committed
    }

    pub(crate) fn take_startup_error(&mut self) -> Option<WindowsVpnError> {
        self.startup_error.take()
    }

    /// Replace has already restored the source operation's sidecar. Its old
    /// pipe is not ownership of the target's proxy setting; dropping it is
    /// safe because the Agent scopes EOF cleanup to the old operation ID.
    pub(super) fn discard_retired_sidecar(&mut self, state: &AgentState) {
        if matches!(
            agent_v1::AgentPhase::try_from(state.phase),
            Ok(agent_v1::AgentPhase::Prepared | agent_v1::AgentPhase::Active)
        ) && state.operation_id == self.operation_id.to_string()
            && self
                .system_proxy
                .as_ref()
                .is_some_and(|sidecar| sidecar.operation_id != self.operation_id)
        {
            self.system_proxy = None;
        }
    }

    pub(crate) async fn retain_failed_gate(
        &mut self,
        reason: usque_core::vpngate::GateFailure,
    ) -> Result<(), WindowsVpnError> {
        self.held_failure = true;
        self.quiesce_final();
        self.stop_packet_pumps().await;
        // Keep startup/liveness pipes: their EOF must not schedule ordinary
        // rollback while this recoverable failed transaction is retained.
        if let Some(tunnel) = &mut self.tunnel {
            tunnel.fail_gate(reason).await;
        }
        if self.mapping.is_some() {
            self.agent.close_packet_session(self.operation_id).await?;
            self.mapping = None;
        }
        let state = self.agent.get_state().await?;
        if state.operation_id == self.operation_id.to_string() && state.kill_switch_active
            || owned_pending_replacement(
                &state,
                self.operation_id,
                self.replacement_request.as_ref(),
            )?
            .is_some_and(|r| r.guard_active)
        {
            Ok(())
        } else {
            Err(WindowsVpnError::RecoveryRequired {
                phase: state.phase,
                operation_id: state.operation_id,
            })
        }
    }

    async fn retained_bootstrap_candidates(
        &self,
        state: &AgentState,
        cancellation: &CancellationToken,
    ) -> Result<Vec<SocketAddr>, WindowsVpnError> {
        for cached in [
            self.socket_protector
                .as_ref()
                .map(|p| p.registration_api.clone()),
            self.bootstrap.as_ref().map(|b| b.registration_api.clone()),
            state_bootstrap_candidates(state).ok(),
        ]
        .into_iter()
        .flatten()
        {
            if let Ok(valid) = validate_bootstrap_candidates(cached) {
                return Ok(valid);
            }
        }
        // The only refresh path uses the fixed control host through the old
        // private WARP network, before shutting that underlay down.
        if let Some(tunnel) = &self.tunnel {
            let addresses = tunnel
                .warp_internal_network()
                .resolve_registration_api(cancellation)
                .await
                .map_err(|_| {
                    WindowsVpnError::ControlEndpointResolution(
                        "protected registration API resolution failed".into(),
                    )
                })?;
            return validate_bootstrap_candidates(addresses);
        }
        Err(WindowsVpnError::ControlEndpointResolution(
            "protected reconnect requires retained registration API candidates".into(),
        ))
    }

    /// Rebuild stopped transports with fresh Vault inputs, retaining the same
    /// Agent operation. No identity material is accepted from IPC status.
    #[expect(
        clippy::too_many_arguments,
        reason = "restart binds profile, identity, policy and cancellation to one retained transaction"
    )]
    pub(crate) async fn retry_protected_chain(
        &mut self,
        profile: &Profile,
        identity: MasqueTlsIdentity,
        refresher: Arc<dyn EndpointPinRefresher>,
        policy: Arc<GeoDirectPolicy>,
        selected: Option<(
            usque_core::vpngate::ServerSummary,
            usque_core::vpngate::PreparedProfile,
        )>,
        status: watch::Sender<usque_core::vpngate::GateStatus>,
        cancellation: &CancellationToken,
        device: &WindowsDeviceOwner,
    ) -> Result<(), WindowsVpnError> {
        if self.target_committed {
            self.handoff_intent = false;
            self.replacement_request = None;
        }
        self.target_committed = false;
        let state = self.agent.get_state().await?;
        let startup_lost = self.startup_lease.as_ref().is_some_and(|pipe| {
            let mut byte = [0];
            !matches!(pipe.try_read(&mut byte), Err(error) if error.kind() == io::ErrorKind::WouldBlock)
        });
        if pending_replacement(&state)?.is_some()
            || self.monitor.agent_disconnected()
            || startup_lost
            || self.lifetime.is_cancelled()
        {
            return Box::pin(self.replace_protected_connection(
                profile,
                identity,
                refresher,
                policy,
                selected,
                status,
                cancellation,
                device,
            ))
            .await;
        }
        if state.operation_id != self.operation_id.to_string() {
            return Err(WindowsVpnError::RecoveryConflict);
        }
        let registration_api = self
            .retained_bootstrap_candidates(&state, cancellation)
            .await?;
        self.quiesce_final();
        self.stop_packet_pumps().await;
        if self.mapping.is_some() {
            self.agent.close_packet_session(self.operation_id).await?;
            self.mapping = None;
        }
        if let Some(mut tunnel) = self.tunnel.take() {
            tunnel.shutdown().await;
        }
        if let Some(protector) = self.socket_protector.take() {
            protector.monitor_cancel.cancel();
        }
        self.bootstrap = Some(WarpBootstrap {
            identity,
            refresher,
            registration_api,
            status: status.borrow().clone(),
        });
        let result = self
            .replace_gate(profile, selected, policy, status, cancellation)
            .await;
        self.held_failure = result.is_err();
        if self.target_committed {
            self.handoff_intent = false;
            self.replacement_request = None;
        }
        result
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "replacement binds target transport inputs while preserving Agent ownership"
    )]
    pub(crate) async fn replace_protected_connection(
        &mut self,
        profile: &Profile,
        identity: MasqueTlsIdentity,
        refresher: Arc<dyn EndpointPinRefresher>,
        policy: Arc<GeoDirectPolicy>,
        selected: Option<(
            usque_core::vpngate::ServerSummary,
            usque_core::vpngate::PreparedProfile,
        )>,
        status: watch::Sender<usque_core::vpngate::GateStatus>,
        cancellation: &CancellationToken,
        device: &WindowsDeviceOwner,
    ) -> Result<(), WindowsVpnError> {
        self.target_committed = false;
        // A requested live handoff owns its source protection even before an
        // old Agent can acknowledge the capability. This intent is distinct
        // from ordinary same-operation failure retention with Kill Switch off.
        self.handoff_intent = true;
        let capabilities = self.agent.get_capabilities().await?;
        if !capabilities.protected_tunnel_replacement
            || !capabilities.deferred_network_configuration
        {
            return Err(WindowsVpnError::MissingCapabilities(
                "protected_tunnel_replacement/deferred_network_configuration".into(),
            ));
        }
        validate_capabilities(&capabilities, profile.kill_switch)?;
        validate_automatic_endpoint_capability(&capabilities, profile)?;
        let mut state = self.agent.get_state().await?;
        let registration_api = self
            .retained_bootstrap_candidates(&state, cancellation)
            .await?;
        let grant = device.acquire(&self.agent, &capabilities).await?;
        state = self.agent.get_state().await?;
        let mut plan = tunnel_plan(
            profile,
            &identity,
            &registration_api,
            profile.has_domain_direct_rules(),
        );
        plan.vpn_chain = true;
        plan.defer_network_configuration = true;
        let plan = canonical_plan(plan);
        let pending = pending_replacement(&state)?;
        let same = pending.filter(|r| r.target_plan.as_deref() == Some(&plan));
        let retained = self.replacement_request.as_ref().filter(|request| {
            pending.is_none()
                && request.source_operation_id == state.operation_id
                && request.plan.as_ref() == Some(&plan)
        });
        let (source_operation_id, operation_id) = if let Some(same) = same {
            (same.source_operation_id.clone(), same.operation_id.clone())
        } else if let Some(retained) = retained {
            (
                retained.source_operation_id.clone(),
                retained.operation_id.clone(),
            )
        } else {
            (
                pending.map_or_else(|| state.operation_id.clone(), |r| r.operation_id.clone()),
                Uuid::new_v4().to_string(),
            )
        };
        let mut request = agent_v1::ReplaceTunnelRequest {
            source_operation_id,
            expected_journal_generation: state.journal_generation,
            operation_id: operation_id.clone(),
            plan: Some(plan),
            device_lease_id: grant.lease_id,
            device_lease_generation: grant.lease_generation,
        };
        if cancellation.is_cancelled() {
            return Err(TransportError::TunnelClosed.into());
        }
        self.quiesce_final();
        self.stop_packet_pumps().await;
        if self.mapping.is_some() {
            self.agent.close_packet_session(self.operation_id).await?;
            self.mapping = None;
        }
        if let Some(mut tunnel) = self.tunnel.take() {
            tunnel.shutdown().await;
        }
        self.held_failure = true;
        self.bootstrap = Some(WarpBootstrap {
            identity,
            refresher,
            registration_api,
            status: status.borrow().clone(),
        });
        // Keep old liveness/startup pipes until acknowledgement. Premature EOF
        // can otherwise run the old operation's orphan cleanup before install.
        request.expected_journal_generation = self.agent.get_state().await?.journal_generation;
        self.replacement_request = Some(request.clone());
        let (lease, prepared) = match self.agent.replace_tunnel_lease(request.clone()).await {
            Ok(outcome) => outcome,
            Err(error) => {
                // A lost reply may have transferred ownership. Preserve the
                // fingerprint on every uncertain outcome; retry compares it.
                if let Ok(current) = self.agent.get_state().await
                    && current
                        .replacement
                        .as_ref()
                        .is_some_and(|r| replacement_matches(r, &request))
                {
                    self.operation_id = Uuid::parse_str(&operation_id)
                        .map_err(|_| WindowsVpnError::InvalidAgentOperationId)?;
                    self.target_committed = target_is_committed(&current, self.operation_id);
                    self.discard_retired_sidecar(&current);
                }
                return Err(error);
            }
        };
        self.operation_id =
            Uuid::parse_str(&operation_id).map_err(|_| WindowsVpnError::InvalidAgentOperationId)?;
        self.discard_retired_sidecar(&prepared);
        drop(self.liveness.take());
        self.startup_lease = Some(lease);
        self.transaction_open = true;
        self.lifetime = CancellationToken::new();
        self.cancellation = self.lifetime.child_token();
        if let Some(protector) = self.socket_protector.take() {
            protector.monitor_cancel.cancel();
        }
        self.target_committed = target_is_committed(&prepared, self.operation_id);
        if cancellation.is_cancelled() {
            return Err(TransportError::TunnelClosed.into());
        }
        let result = self
            .replace_gate(profile, selected, policy, status, cancellation)
            .await;
        self.held_failure = result.is_err();
        if self.target_committed {
            self.handoff_intent = false;
            self.replacement_request = None;
        }
        result
    }
}

impl WindowsAgentClient {
    async fn replace_tunnel_lease(
        &self,
        request: agent_v1::ReplaceTunnelRequest,
    ) -> Result<(NamedPipeClient, AgentState), WindowsVpnError> {
        let mut pipe = self.open_pipe().await?;
        let response = timeout(
            AGENT_RPC_TIMEOUT,
            self.exchange(
                &mut pipe,
                agent_request::Payload::ReplaceTunnel(request.clone()),
            ),
        )
        .await
        .map_err(|_| WindowsVpnError::RpcTimeout)??;
        match response {
            agent_response::Payload::State(state)
                if state.operation_id == request.operation_id
                    && state
                        .replacement
                        .as_ref()
                        .is_some_and(|r| replacement_matches(r, &request))
                    && (state.phase == agent_v1::AgentPhase::Prepared as i32
                        && pending_replacement(&state)?.is_some_and(|r| r.guard_active)
                        || state.phase == agent_v1::AgentPhase::Active as i32
                            && pending_replacement(&state)?.is_none()) =>
            {
                Ok((pipe, state))
            }
            agent_response::Payload::State(state) => {
                Err(WindowsVpnError::UnexpectedAgentPhase(state.phase))
            }
            payload => Err(WindowsVpnError::UnexpectedResponse(payload_name(&payload))),
        }
    }

    pub(super) async fn abort_replacement(
        &self,
        expected: &agent_v1::TunnelReplacementStatus,
        generation: u64,
    ) -> Result<AgentState, WindowsVpnError> {
        let result = self
            .call(agent_request::Payload::AbortReplacement(
                agent_v1::AbortReplacementRequest {
                    operation_id: expected.operation_id.clone(),
                    expected_journal_generation: generation,
                },
            ))
            .await;
        let payload = match result {
            Ok(payload) => payload,
            Err(error) => {
                // The acknowledged mutation may outlive its pipe. Readback
                // proves only this exact abort; never issue a speculative one.
                if let Ok(state) = self.get_state().await
                    && abort_completed(&state, expected, generation)
                {
                    return Ok(state);
                }
                return Err(error);
            }
        };
        match payload {
            agent_response::Payload::State(state) => {
                require_recovered_state(&state)?;
                if pending_replacement(&state)?.is_some()
                    || state.replacement.is_some() && !abort_completed(&state, expected, generation)
                {
                    return Err(WindowsVpnError::RecoveryConflict);
                }
                Ok(state)
            }
            payload => Err(WindowsVpnError::UnexpectedResponse(payload_name(&payload))),
        }
    }
}

#[cfg(test)]
mod tests;
