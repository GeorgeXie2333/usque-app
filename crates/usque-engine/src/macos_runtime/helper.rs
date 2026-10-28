//! Bounded, persistent stdio channel to the bundle-owned XPC host. Neither
//! command-line arguments, environment variables nor files carry credentials.
use super::*;
use serde_json::{Value, json};
use std::{collections::HashMap, process::Stdio};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::{Child, ChildStdin, ChildStdout},
};
use usque_macos_runtime::protocol::MAX_CONTROL_BYTES;

pub struct BundledProviderControl {
    executable: PathBuf,
    process: Mutex<Option<HelperProcess>>,
    active_broker: Mutex<Option<Arc<dyn CredentialBroker>>>,
}
struct HelperProcess {
    _child: Child,
    input: ChildStdin,
    output: ChildStdout,
    locks: HashMap<Uuid, std::fs::File>,
}
impl BundledProviderControl {
    pub fn from_current_bundle() -> Result<Self, String> {
        let engine = std::env::current_exe()
            .map_err(|_| "bundle_path_unavailable")?
            .canonicalize()
            .map_err(|_| "bundle_path_unavailable")?;
        let macos = engine.parent().ok_or("bundle_path_unavailable")?;
        if macos.file_name().and_then(|v| v.to_str()) != Some("MacOS") {
            return Err("bundle_path_unavailable".into());
        }
        let contents = macos.parent().ok_or("bundle_path_unavailable")?;
        if contents.file_name().and_then(|v| v.to_str()) != Some("Contents") {
            return Err("bundle_path_unavailable".into());
        }
        let helpers = contents.join("Helpers");
        let executable = helpers
            .join("usque-macos-control")
            .canonicalize()
            .map_err(|_| "bundled_provider_unavailable")?;
        if executable.parent() != Some(helpers.as_path()) {
            return Err("bundle_path_unavailable".into());
        }
        Ok(Self {
            executable,
            process: Mutex::new(None),
            active_broker: Mutex::new(None),
        })
    }
    async fn spawn(&self) -> Result<HelperProcess, String> {
        let mut child = tokio::process::Command::new(&self.executable)
            .arg("--stdio")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|_| "macos_control_start_failed")?;
        let input = child.stdin.take().ok_or("macos_control_pipe_failed")?;
        let output = child.stdout.take().ok_or("macos_control_pipe_failed")?;
        Ok(HelperProcess {
            _child: child,
            input,
            output,
            locks: HashMap::new(),
        })
    }
    async fn native(&self, native: Value) -> Result<Value, String> {
        tokio::time::timeout(
            std::time::Duration::from_secs(15),
            self.native_inner(native),
        )
        .await
        .map_err(|_| "macos_control_timeout".to_owned())?
    }
    async fn native_inner(&self, native: Value) -> Result<Value, String> {
        let operation_id = Uuid::new_v4();
        let mut slot = self.process.lock().await;
        let mut process = match slot.take() {
            Some(process) => process,
            None => self.spawn().await?,
        };
        process
            .write(&json!({"version":PROTOCOL_VERSION,"operation_id":operation_id,"native":native}))
            .await?;
        let response = loop {
            let response = process.read().await?;
            if response.get("broker").is_none() {
                break response;
            }
            let broker = self.active_broker.lock().await.clone().unwrap_or_else(|| {
                Arc::new(usque_macos_runtime::credentials::UnavailableCredentialBroker)
            });
            let callback = process.broker(response, broker.as_ref()).await;
            process.write(&callback).await?;
        };
        if response.get("operation_id").and_then(Value::as_str)
            != Some(operation_id.to_string().as_str())
        {
            return Err("macos_control_stale_reply".into());
        }
        *slot = Some(process);
        Ok(response)
    }
}
#[async_trait]
impl ProviderControl for BundledProviderControl {
    async fn exchange(
        &self,
        request: Envelope,
        broker: Arc<dyn CredentialBroker>,
    ) -> Result<Reply, String> {
        *self.active_broker.lock().await = Some(Arc::clone(&broker));
        let mut slot = self.process.lock().await;
        // Take ownership while framing: cancellation/timeout drops and kills
        // this CLI, so a partial frame can never contaminate the next request.
        let mut process = match slot.take() {
            Some(process) => process,
            None => self.spawn().await?,
        };
        process.write(&request).await?;
        for _ in 0..64 {
            let response = process.read().await?;
            if response.get("broker").is_some() {
                let callback = process.broker(response, broker.as_ref()).await;
                process.write(&callback).await?;
            } else {
                if response.get("operation_id").and_then(Value::as_str)
                    != Some(request.operation_id.to_string().as_str())
                {
                    return Err("macos_provider_stale_reply".into());
                }
                if let Some(code) = response.get("error_code").and_then(Value::as_str) {
                    let code = match code {
                        "extension_version_mismatch"
                        | "permission_denied"
                        | "awaiting_approval"
                        | "activation_requires_reboot"
                        | "signature_required"
                        | "production_evidence_required" => code,
                        _ => "macos_control_operation_rejected",
                    };
                    return Err(code.into());
                }
                let reply: Reply =
                    serde_json::from_value(response).map_err(|_| "macos_control_invalid_reply")?;
                *slot = Some(process);
                return Ok(reply);
            }
        }
        Err("macos_control_callback_limit".into())
    }
    async fn authorize_flow(
        &self,
        session: &SessionKey,
        intent: usque_transport::SystemFlowIntent,
    ) -> Result<(), String> {
        let reply = self.native(json!({"type":"flow_intent","session":session,"local_ip":intent.local.ip().to_string(),"local_port":intent.local.port(),"remote_ip":intent.remote.ip().to_string(),"remote_port":intent.remote.port(),"protocol":match intent.protocol { usque_transport::SystemFlowProtocol::Tcp=>6, usque_transport::SystemFlowProtocol::Udp=>17 },"hostname":intent.hostname,"epoch":intent.epoch})).await?;
        if reply.pointer("/native/accepted").and_then(Value::as_bool) != Some(true) {
            return Err("macos_flow_intent_rejected".into());
        }
        Ok(())
    }
    async fn network_generation(&self) -> Result<u64, String> {
        let reply = self.native(json!({"type":"status"})).await?;
        reply
            .pointer("/native/physical_network/generation")
            .and_then(Value::as_u64)
            .filter(|value| *value > 0)
            .ok_or_else(|| "physical_network_unavailable".into())
    }
}
impl HelperProcess {
    async fn write<T: serde::Serialize>(&mut self, value: &T) -> Result<(), String> {
        let bytes =
            Zeroizing::new(serde_json::to_vec(value).map_err(|_| "macos_control_encoding_failed")?);
        if bytes.is_empty() || bytes.len() > MAX_CONTROL_BYTES {
            return Err("macos_control_frame_too_large".into());
        }
        self.input
            .write_u32(bytes.len() as u32)
            .await
            .map_err(|_| "macos_control_pipe_failed")?;
        self.input
            .write_all(&bytes)
            .await
            .map_err(|_| "macos_control_pipe_failed")?;
        self.input
            .flush()
            .await
            .map_err(|_| "macos_control_pipe_failed".into())
    }
    async fn read(&mut self) -> Result<Value, String> {
        let size = self
            .output
            .read_u32()
            .await
            .map_err(|_| "macos_control_pipe_failed")? as usize;
        if size == 0 || size > MAX_CONTROL_BYTES {
            return Err("macos_control_frame_too_large".into());
        }
        let mut bytes = Zeroizing::new(vec![0; size]);
        self.output
            .read_exact(&mut bytes)
            .await
            .map_err(|_| "macos_control_pipe_failed")?;
        serde_json::from_slice(&bytes).map_err(|_| "macos_control_invalid_reply".into())
    }
    async fn broker(&mut self, callback: Value, broker: &dyn CredentialBroker) -> Value {
        use usque_macos_runtime::credential_rpc::{
            BrokerResponse, CredentialRequest, CredentialResponse,
        };
        let request = serde_json::from_value::<CredentialRequest>(callback);
        let Ok(request) = request else {
            return json!({"broker":{"accepted":false,"error_code":"invalid_credential_request"}});
        };
        let result = if request.validate_digest().is_ok() {
            self.broker_inner(&request, broker).await
        } else {
            Err("credential_digest_mismatch".into())
        };
        let payload = match result {
            Ok(value) => value,
            Err(_) => BrokerResponse {
                accepted: false,
                error_code: Some("credential_broker_rejected".into()),
                ..Default::default()
            },
        };
        serde_json::to_value(CredentialResponse {
            callback_id: request.callback_id,
            broker: payload,
        })
        .unwrap_or_else(|_| json!({"broker":{"accepted":false}}))
    }
    async fn broker_inner(
        &mut self,
        request: &usque_macos_runtime::credential_rpc::CredentialRequest,
        broker: &dyn CredentialBroker,
    ) -> Result<usque_macos_runtime::credential_rpc::BrokerResponse, String> {
        use usque_macos_runtime::credential_rpc::{BrokerRequest, BrokerResponse};
        let session = request.broker.session();
        let profile_id = request.broker.profile_id();
        let mut response = BrokerResponse {
            accepted: true,
            session: Some(session.clone()),
            profile_id: Some(profile_id),
            request_digest: Some(request.request_digest.clone()),
            ..Default::default()
        };
        if let BrokerRequest::PersistPin {
            previous_identity,
            next_identity,
            ..
        } = &request.broker
        {
            let previous = usque_core::parse_manual_warp_secret(previous_identity)
                .map_err(|_| "invalid_broker_identity")?;
            let next = usque_core::parse_manual_warp_secret(next_identity)
                .map_err(|_| "invalid_broker_identity")?;
            let ack = broker
                .persist_pin(session, profile_id, &previous, &next)
                .await?;
            response.session = Some(ack.session);
            response.profile_id = Some(ack.profile_id);
            response.revision = Some(ack.revision);
            return Ok(response);
        }
        let initial = match &request.broker {
            BrokerRequest::ProtonLoad { initial, .. }
            | BrokerRequest::ProtonSave { initial, .. }
            | BrokerRequest::ProtonLock { initial, .. }
            | BrokerRequest::ProtonUnlock { initial, .. } => initial,
            _ => return Err("invalid_broker_request".into()),
        };
        let lease = broker.proton_persistence(session, profile_id, initial)?;
        match &request.broker {
            BrokerRequest::ProtonLoad { .. } => {
                let record = lease.load().map_err(|_| "proton_load_failed")?;
                response.revision = Some(record.revision);
                response.record = Some(record);
            }
            BrokerRequest::ProtonSave { record, .. } => {
                lease.save(record).map_err(|_| "proton_save_failed")?;
                response.revision = Some(record.revision);
            }
            BrokerRequest::ProtonLock { .. } => {
                if self.locks.len() >= 16 {
                    return Err("broker_lock_limit".into());
                }
                let lock = lease.lock().map_err(|_| "proton_lock_failed")?;
                let token = Uuid::new_v4();
                self.locks.insert(token, lock);
                response.lease_id = Some(token);
            }
            BrokerRequest::ProtonUnlock { lease_id, .. } => {
                self.locks.remove(lease_id).ok_or("unknown_broker_lock")?;
            }
            _ => return Err("invalid_broker_request".into()),
        }
        Ok(response)
    }
}

pub(super) struct FlowAuthorizer {
    pub(super) provider: Arc<dyn ProviderControl>,
    pub(super) session: Arc<StdMutex<Option<SessionKey>>>,
}
#[async_trait]
impl usque_transport::SystemFlowAuthorizer for FlowAuthorizer {
    async fn authorize(&self, intent: usque_transport::SystemFlowIntent) -> Result<(), String> {
        let session = self
            .session
            .lock()
            .map_err(|_| "flow_session_unavailable")?
            .clone()
            .ok_or("flow_session_unavailable")?;
        self.provider.authorize_flow(&session, intent).await
    }
}
