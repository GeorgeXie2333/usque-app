use std::{
    path::Path,
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use usque_ipc::agent_v1::AgentCapabilities;
use uuid::Uuid;
use windows_sys::Win32::System::Threading::GetProcessId;

use crate::{
    AGENT_PROTOCOL_VERSION, AuthenticatedCaller,
    coordinator::{
        BackendError, PrivilegedBackend, StepOutput, StepParameter, SystemProxySettings,
    },
    journal::{MutationKind, MutationReceipt},
    plan::ValidatedTunnelPlan,
    windows::{
        network,
        packet_session::{PacketMapping, PacketPump, close_remote_packet_handles},
        system_proxy, wfp,
        wintun::{WintunAdapter, WintunLibrary},
    },
};

#[derive(Clone)]
pub struct WindowsBackend {
    inner: Arc<BackendInner>,
}

struct BackendInner {
    resources: Mutex<WindowsResources>,
}

#[derive(Default)]
struct WindowsResources {
    library: Option<Arc<WintunLibrary>>,
    adapter: Option<WintunAdapter>,
    pump: Option<PacketPump>,
}

impl WindowsBackend {
    /// Loads and hash-verifies the pinned Wintun DLL without creating an
    /// adapter or changing host networking.
    pub fn open(wintun_path: &Path) -> Result<Self, BackendError> {
        let library =
            WintunLibrary::load(wintun_path).map_err(|error| backend_error(error.to_string()))?;
        Ok(Self {
            inner: Arc::new(BackendInner {
                resources: Mutex::new(WindowsResources {
                    library: Some(library),
                    adapter: None,
                    pump: None,
                }),
            }),
        })
    }

    pub fn capabilities(&self) -> AgentCapabilities {
        AgentCapabilities {
            wintun: true,
            // These are enabled only as each audited backend is linked.
            wfp_kill_switch: true,
            interface_addresses: true,
            interface_dns: true,
            system_proxy: true,
            shared_packet_ring: true,
            operating_system: "windows".to_owned(),
            architecture: std::env::consts::ARCH.to_owned(),
            protocol_version: AGENT_PROTOCOL_VERSION,
            dynamic_direct_egress: true,
            physical_dns_snapshot: true,
            exact_generation_egress: true,
        }
    }
}

#[async_trait]
impl PrivilegedBackend for WindowsBackend {
    async fn plan_step(
        &self,
        kind: MutationKind,
        plan: &ValidatedTunnelPlan,
        _caller: &AuthenticatedCaller,
        parameter: StepParameter,
    ) -> Result<MutationReceipt, BackendError> {
        match kind {
            MutationKind::WintunAdapter => Ok(MutationReceipt::WintunAdapter {
                adapter_name: adapter_name(plan),
                adapter_guid: Uuid::new_v4(),
                interface_luid: 0,
            }),
            MutationKind::PacketSession => {
                let StepParameter::PacketRing { capacity } = parameter else {
                    return Err(backend_error("packet-ring capacity is missing"));
                };
                Ok(MutationReceipt::PacketSession {
                    session_id: Uuid::new_v4(),
                    ring_capacity: capacity,
                })
            }
            MutationKind::EndpointBypass => network::plan_endpoint_bypass(
                &plan.endpoint_candidates,
                &plan.control_api_candidates,
            )
            .map_err(network_backend_error),
            MutationKind::KillSwitch => {
                let interface_luid = current_adapter_luid(&self.inner)?;
                wfp::plan_kill_switch(plan, interface_luid)
                    .map_err(|error| backend_error(error.to_string()))
            }
            MutationKind::InterfaceConfiguration => {
                let interface_luid = current_adapter_luid(&self.inner)?;
                network::plan_interface_configuration(interface_luid, plan)
                    .map_err(|error| backend_error(error.to_string()))
            }
            MutationKind::Dns => {
                let interface_luid = current_adapter_luid(&self.inner)?;
                network::plan_dns(interface_luid, plan)
                    .map_err(|error| backend_error(error.to_string()))
            }
            MutationKind::DefaultRoutes => {
                let interface_luid = current_adapter_luid(&self.inner)?;
                network::plan_default_routes(interface_luid, plan)
                    .map_err(|error| backend_error(error.to_string()))
            }
            MutationKind::SystemProxy => Err(unavailable("system proxy")),
        }
    }

    async fn apply_step(
        &self,
        receipt: MutationReceipt,
        plan: &ValidatedTunnelPlan,
        caller: &AuthenticatedCaller,
    ) -> Result<(MutationReceipt, StepOutput), BackendError> {
        let inner = Arc::clone(&self.inner);
        let plan = plan.clone();
        let caller = caller.clone();
        tokio::task::spawn_blocking(move || apply_sync(&inner, receipt, &plan, &caller))
            .await
            .map_err(|error| backend_error(format!("privileged worker failed: {error}")))?
    }

    async fn restore_step(&self, receipt: &MutationReceipt) -> Result<(), BackendError> {
        let inner = Arc::clone(&self.inner);
        let receipt = receipt.clone();
        tokio::task::spawn_blocking(move || restore_sync(&inner, &receipt))
            .await
            .map_err(|error| backend_error(format!("privileged recovery worker failed: {error}")))?
    }

    async fn resume_packet_session(
        &self,
        adapter: &MutationReceipt,
        session: &MutationReceipt,
        plan: &ValidatedTunnelPlan,
        caller: &AuthenticatedCaller,
    ) -> Result<crate::coordinator::PacketSessionHandles, BackendError> {
        let inner = Arc::clone(&self.inner);
        let adapter = adapter.clone();
        let session = session.clone();
        let plan = plan.clone();
        let caller = caller.clone();
        tokio::task::spawn_blocking(move || {
            resume_packet_session_sync(&inner, &adapter, &session, &plan, &caller)
        })
        .await
        .map_err(|error| backend_error(format!("packet-session resume worker failed: {error}")))?
    }

    async fn plan_system_proxy(
        &self,
        operation_id: Uuid,
        caller: &AuthenticatedCaller,
        settings: &SystemProxySettings,
    ) -> Result<MutationReceipt, BackendError> {
        let caller = caller.clone();
        let settings = settings.clone();
        tokio::task::spawn_blocking(move || {
            system_proxy::plan(operation_id, &caller, &settings)
                .map_err(|error| backend_error(error.to_string()))
        })
        .await
        .map_err(|error| backend_error(format!("system-proxy planning worker failed: {error}")))?
    }

    async fn apply_system_proxy(
        &self,
        receipt: MutationReceipt,
    ) -> Result<MutationReceipt, BackendError> {
        tokio::task::spawn_blocking(move || {
            system_proxy::apply(receipt).map_err(|error| backend_error(error.to_string()))
        })
        .await
        .map_err(|error| backend_error(format!("system-proxy worker failed: {error}")))?
    }
}

fn apply_sync(
    inner: &BackendInner,
    receipt: MutationReceipt,
    plan: &ValidatedTunnelPlan,
    caller: &AuthenticatedCaller,
) -> Result<(MutationReceipt, StepOutput), BackendError> {
    match receipt {
        MutationReceipt::WintunAdapter {
            adapter_name: name,
            adapter_guid,
            interface_luid: _,
        } => {
            if name != adapter_name(plan) {
                return Err(backend_error("Wintun receipt does not match the Profile"));
            }
            let mut resources = lock_resources(inner)?;
            if resources.adapter.is_some() {
                return Err(backend_error("a Wintun adapter is already active"));
            }
            let library = resources
                .library
                .as_ref()
                .cloned()
                .ok_or_else(|| backend_error("Wintun library is unavailable"))?;
            let adapter = library
                .create_adapter(&name, adapter_guid)
                .map_err(|error| backend_error(error.to_string()))?;
            let interface_luid = adapter.luid();
            if interface_luid == 0 {
                return Err(backend_error("Wintun returned an empty interface LUID"));
            }
            resources.adapter = Some(adapter);
            Ok((
                MutationReceipt::WintunAdapter {
                    adapter_name: name,
                    adapter_guid,
                    interface_luid,
                },
                StepOutput::default(),
            ))
        }
        MutationReceipt::PacketSession {
            session_id,
            ring_capacity,
        } => apply_packet_session(inner, session_id, ring_capacity, caller),
        other @ MutationReceipt::EndpointBypass { .. } => {
            apply_enriched(other, network::apply_endpoint_bypass)
        }
        other @ MutationReceipt::InterfaceConfiguration { .. } => {
            apply_enriched(other, |receipt| {
                network::apply_interface_configuration(receipt, plan)
            })
        }
        other @ MutationReceipt::Dns { .. } => {
            let receipt = network::apply_dns(other, plan)
                .map_err(|error| backend_error(error.to_string()))?;
            Ok((receipt, StepOutput::default()))
        }
        other @ MutationReceipt::DefaultRoutes { .. } => {
            apply_enriched(other, network::apply_default_routes)
        }
        other @ MutationReceipt::KillSwitch { .. } => {
            let interface_luid = current_adapter_luid(inner)?;
            let receipt =
                wfp::apply_kill_switch(other, plan, interface_luid, &caller.executable_path)
                    .map_err(|error| backend_error(error.to_string()))?;
            Ok((receipt, StepOutput::default()))
        }
        MutationReceipt::SystemProxy { .. } => Err(unavailable("system proxy")),
    }
}

fn resume_packet_session_sync(
    inner: &BackendInner,
    adapter_receipt: &MutationReceipt,
    session_receipt: &MutationReceipt,
    plan: &ValidatedTunnelPlan,
    caller: &AuthenticatedCaller,
) -> Result<crate::coordinator::PacketSessionHandles, BackendError> {
    let (
        MutationReceipt::WintunAdapter {
            adapter_name: journal_adapter_name,
            interface_luid,
            ..
        },
        MutationReceipt::PacketSession {
            session_id,
            ring_capacity,
        },
    ) = (adapter_receipt, session_receipt)
    else {
        return Err(backend_error(
            "packet-session resume receipts have unexpected kinds",
        ));
    };
    if journal_adapter_name != &adapter_name(plan)
        || !valid_recovery_adapter_name(journal_adapter_name)
    {
        return Err(backend_error(
            "journal Wintun adapter does not match the active Profile",
        ));
    }
    if *interface_luid == 0 {
        return Err(backend_error(
            "journal Wintun adapter has an empty interface LUID",
        ));
    }

    let mut resources = lock_resources(inner)?;
    if resources.pump.is_some() {
        return Err(backend_error("a Wintun packet session is already active"));
    }
    let adapter = match resources.adapter.as_ref().cloned() {
        Some(adapter) => adapter,
        None => {
            let library = resources
                .library
                .as_ref()
                .cloned()
                .ok_or_else(|| backend_error("Wintun library is unavailable"))?;
            library
                .open_adapter(journal_adapter_name)
                .map_err(|error| backend_error(error.to_string()))?
        }
    };
    if adapter.name() != journal_adapter_name || adapter.luid() != *interface_luid {
        return Err(backend_error(
            "reopened Wintun adapter identity does not match the recovery journal",
        ));
    }
    resources.adapter = Some(adapter);
    let handles = start_packet_session(&mut resources, *ring_capacity, caller)?;
    // The journaled session ID is deliberately retained. It identifies the
    // logical packet step across Agent process generations.
    let _ = session_id;
    Ok(handles)
}

fn apply_packet_session(
    inner: &BackendInner,
    session_id: Uuid,
    ring_capacity: u32,
    caller: &AuthenticatedCaller,
) -> Result<(MutationReceipt, StepOutput), BackendError> {
    let mut resources = lock_resources(inner)?;
    let handles = start_packet_session(&mut resources, ring_capacity, caller)?;
    Ok((
        MutationReceipt::PacketSession {
            session_id,
            ring_capacity,
        },
        StepOutput {
            packet_session: Some(handles),
        },
    ))
}

fn start_packet_session(
    resources: &mut WindowsResources,
    ring_capacity: u32,
    caller: &AuthenticatedCaller,
) -> Result<crate::coordinator::PacketSessionHandles, BackendError> {
    let target = caller
        .process_handle
        .map(|value| value as windows_sys::Win32::Foundation::HANDLE)
        .ok_or_else(|| backend_error("authenticated Engine process handle is missing"))?;
    // SAFETY: authentication owns this process handle for the complete request
    // and the PID comparison closes accidental handle confusion.
    if unsafe { GetProcessId(target) } != caller.process_id {
        return Err(backend_error("authenticated Engine process handle changed"));
    }
    if resources.pump.is_some() {
        return Err(backend_error("a Wintun packet session is already active"));
    }
    let adapter = resources
        .adapter
        .as_ref()
        .cloned()
        .ok_or_else(|| backend_error("Wintun adapter is not prepared"))?;
    let session = adapter
        .start_session(ring_capacity)
        .map_err(|error| backend_error(error.to_string()))?;
    let (mapping, handles) = PacketMapping::create(ring_capacity, target)
        .map_err(|error| backend_error(error.to_string()))?;
    let pump = match PacketPump::start(session, mapping) {
        Ok(pump) => pump,
        Err(error) => {
            close_remote_packet_handles(target, &handles);
            return Err(backend_error(error.to_string()));
        }
    };
    resources.pump = Some(pump);
    Ok(handles)
}

fn restore_sync(inner: &BackendInner, receipt: &MutationReceipt) -> Result<(), BackendError> {
    match receipt {
        MutationReceipt::PacketSession { .. } => {
            let pump = lock_resources(inner)?.pump.take();
            if let Some(pump) = pump {
                pump.stop()
                    .map_err(|error| backend_error(error.to_string()))?;
            }
            Ok(())
        }
        MutationReceipt::WintunAdapter {
            adapter_name,
            adapter_guid,
            interface_luid,
        } => {
            if !valid_recovery_adapter_name(adapter_name) {
                return Err(backend_error("journal Wintun adapter name is invalid"));
            }
            let (pump, adapter, library) = {
                let mut resources = lock_resources(inner)?;
                (
                    resources.pump.take(),
                    resources.adapter.take(),
                    resources
                        .library
                        .as_ref()
                        .cloned()
                        .ok_or_else(|| backend_error("Wintun library is unavailable"))?,
                )
            };
            if let Some(pump) = pump {
                pump.stop()
                    .map_err(|error| backend_error(error.to_string()))?;
            }
            if let Some(adapter) = adapter {
                drop(adapter);
            }
            library
                .remove_adapter_if_present(adapter_name, *adapter_guid, *interface_luid)
                .map_err(|error| backend_error(error.to_string()))
        }
        receipt @ MutationReceipt::EndpointBypass { .. } => {
            network::restore_endpoint_bypass(receipt)
                .map_err(|error| backend_error(error.to_string()))
        }
        receipt @ MutationReceipt::InterfaceConfiguration { .. } => {
            network::restore_interface_configuration(receipt)
                .map_err(|error| backend_error(error.to_string()))
        }
        receipt @ MutationReceipt::Dns { .. } => {
            network::restore_dns(receipt).map_err(|error| backend_error(error.to_string()))
        }
        receipt @ MutationReceipt::DefaultRoutes { .. } => network::restore_default_routes(receipt)
            .map_err(|error| backend_error(error.to_string())),
        receipt @ MutationReceipt::KillSwitch { .. } => {
            wfp::restore_kill_switch(receipt).map_err(|error| backend_error(error.to_string()))
        }
        receipt @ MutationReceipt::SystemProxy { .. } => {
            system_proxy::restore(receipt).map_err(|error| backend_error(error.to_string()))
        }
    }
}

fn adapter_name(plan: &ValidatedTunnelPlan) -> String {
    let compact = plan.profile_id.simple().to_string();
    format!("Usque-{}", &compact[..12])
}

fn valid_recovery_adapter_name(value: &str) -> bool {
    value.len() == 18
        && value.starts_with("Usque-")
        && value[6..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn lock_resources(
    inner: &BackendInner,
) -> Result<std::sync::MutexGuard<'_, WindowsResources>, BackendError> {
    inner
        .resources
        .lock()
        .map_err(|_| backend_error("Windows resource lock was poisoned"))
}

fn current_adapter_luid(inner: &BackendInner) -> Result<u64, BackendError> {
    let resources = lock_resources(inner)?;
    let adapter = resources
        .adapter
        .as_ref()
        .ok_or_else(|| backend_error("Wintun adapter is not prepared"))?;
    let interface_luid = adapter.luid();
    if interface_luid == 0 {
        Err(backend_error("Wintun returned an empty interface LUID"))
    } else {
        Ok(interface_luid)
    }
}

fn apply_enriched(
    mut receipt: MutationReceipt,
    apply: impl FnOnce(&mut MutationReceipt) -> Result<(), network::NetworkError>,
) -> Result<(MutationReceipt, StepOutput), BackendError> {
    match apply(&mut receipt) {
        Ok(()) => Ok((receipt, StepOutput::default())),
        Err(error) => Err(BackendError::PartialApply {
            message: error.to_string(),
            receipt: Box::new(receipt),
        }),
    }
}

fn backend_error(message: impl Into<String>) -> BackendError {
    BackendError::Operation(message.into())
}

fn network_backend_error(error: network::NetworkError) -> BackendError {
    match error {
        network::NetworkError::NoReachableEndpoint => BackendError::EndpointUnreachable,
        network::NetworkError::NoReachableControlApi => BackendError::ControlApiUnreachable,
        error => backend_error(error.to_string()),
    }
}

fn unavailable(feature: &'static str) -> BackendError {
    BackendError::Unavailable(feature.to_owned())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn official_dll() -> PathBuf {
        let architecture = if cfg!(target_arch = "aarch64") {
            "arm64"
        } else {
            "amd64"
        };
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../third_party/wintun-0.14.1/wintun/bin")
            .join(architecture)
            .join("wintun.dll")
            .canonicalize()
            .expect("official Wintun")
    }

    #[test]
    fn opening_backend_only_verifies_and_loads_the_dependency() {
        let backend = WindowsBackend::open(&official_dll()).expect("backend");
        let capabilities = backend.capabilities();
        assert!(capabilities.wintun);
        assert!(capabilities.shared_packet_ring);
        assert!(capabilities.interface_addresses);
        assert!(capabilities.interface_dns);
        assert!(capabilities.wfp_kill_switch);
    }
}
