use super::{Failure, Interaction, api::Json};
use async_trait::async_trait;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    sync::Mutex,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{sync::oneshot, time::Instant};
use tokio_util::sync::CancellationToken;
use usque_core::proton_vpn::{Job, Node, Request};
use uuid::Uuid;
use zeroize::Zeroizing;

pub(super) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|v| v.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}
pub(super) fn now() -> i64 {
    now_ms() / 1000
}
enum Kind {
    Verification(Uuid),
    Totp,
    Node,
}
pub(super) struct Shared {
    pub job: Job,
    handoff: Option<Zeroizing<Vec<u8>>>,
    waiting: Option<(Kind, oneshot::Sender<Zeroizing<Vec<u8>>>)>,
}
pub(super) struct JobIo(pub Mutex<Shared>);
impl JobIo {
    pub fn new() -> Self {
        Self(Mutex::new(Shared {
            job: Job::new(),
            handoff: None,
            waiting: None,
        }))
    }
    pub fn snapshot(&self) -> Result<Job, Failure> {
        Ok(self
            .0
            .lock()
            .map_err(|_| Failure::new("unavailable"))?
            .job
            .clone())
    }
    pub fn finish(&self, result: Result<Uuid, Failure>, cancelled: bool) {
        if let Ok(mut shared) = self.0.lock() {
            shared.handoff = None;
            shared.waiting = None;
            shared.job.nodes.clear();
            shared.job.challenge_id = None;
            shared.job.expires_at = None;
            match result {
                Ok(id) => {
                    shared.job.profile_id = (!id.is_nil()).then_some(id);
                    shared.job.state = "completed".into();
                    shared.job.phase = "complete".into();
                }
                Err(failure) => {
                    shared.job.state = if cancelled { "cancelled" } else { "failed" }.into();
                    shared.job.failure = Some(failure.reason);
                    shared.job.api_code = failure.code;
                    shared.job.http_status = failure.status;
                }
            }
        }
    }
    pub fn command(
        &self,
        request: &Request,
        input: Zeroizing<Vec<u8>>,
    ) -> Result<Zeroizing<Vec<u8>>, Failure> {
        let mut shared = self.0.lock().map_err(|_| Failure::new("unavailable"))?;
        if request.job_id != Some(shared.job.id) {
            return Err(Failure::new("job_not_found"));
        }
        if shared.job.expires_at.is_some_and(|v| v <= now_ms()) {
            return Err(Failure::new("interaction_expired"));
        }
        if request.action == "handoff" {
            if request.challenge_id != shared.job.challenge_id {
                return Err(Failure::new("challenge_not_found"));
            }
            return shared
                .handoff
                .take()
                .ok_or_else(|| Failure::new("handoff_already_consumed"));
        }
        let valid = match shared.waiting.as_ref().map(|v| &v.0) {
            Some(Kind::Verification(id)) => {
                request.action == "verify" && request.challenge_id == Some(*id) && !input.is_empty()
            }
            Some(Kind::Totp) => {
                request.action == "totp" && input.len() == 6 && input.iter().all(u8::is_ascii_digit)
            }
            Some(Kind::Node) => {
                request.action == "select_node"
                    && request
                        .node_id
                        .as_ref()
                        .is_some_and(|id| shared.job.nodes.iter().any(|node| &node.id == id))
            }
            None => false,
        };
        if !valid {
            return Err(Failure::new("unexpected_interaction"));
        }
        let value = if request.action == "select_node" {
            Zeroizing::new(
                request
                    .node_id
                    .as_ref()
                    .ok_or_else(|| Failure::new("invalid_node"))?
                    .as_bytes()
                    .to_vec(),
            )
        } else {
            input
        };
        let (_, sender) = shared
            .waiting
            .take()
            .ok_or_else(|| Failure::new("unexpected_interaction"))?;
        shared.handoff = None;
        sender
            .send(value)
            .map_err(|_| Failure::new("interaction_expired"))?;
        Ok(Zeroizing::new(Vec::new()))
    }
    async fn wait(
        &self,
        kind: Kind,
        handoff: Option<Zeroizing<Vec<u8>>>,
        seconds: u64,
        cancel: &CancellationToken,
    ) -> Result<Zeroizing<Vec<u8>>, Failure> {
        let (sender, receiver) = oneshot::channel();
        {
            let mut shared = self.0.lock().map_err(|_| Failure::new("unavailable"))?;
            if shared.waiting.is_some() {
                return Err(Failure::new("interaction_busy"));
            }
            shared.job.state = match &kind {
                Kind::Verification(_) => "waiting_verification",
                Kind::Totp => "waiting_totp",
                Kind::Node => "waiting_node",
            }
            .into();
            shared.job.challenge_id = match &kind {
                Kind::Verification(id) => Some(*id),
                _ => None,
            };
            shared.job.expires_at = Some(now_ms() + seconds as i64 * 1000);
            shared.handoff = handoff;
            shared.waiting = Some((kind, sender));
        }
        let result = tokio::select! {
            biased;
            _ = cancel.cancelled() => Err(Failure::new("cancelled")),
            _ = tokio::time::sleep_until(Instant::now()+Duration::from_secs(seconds)) => Err(Failure::new("interaction_expired")),
            result = receiver => result.map_err(|_| Failure::new("interaction_cancelled")),
        };
        let mut shared = self.0.lock().map_err(|_| Failure::new("unavailable"))?;
        shared.waiting = None;
        shared.handoff = None;
        shared.job.challenge_id = None;
        shared.job.expires_at = None;
        shared.job.nodes.clear();
        if result.is_ok() {
            shared.job.state = "running".into();
        }
        result
    }
}
fn encode(value: &str) -> String {
    let mut out = String::new();
    for b in value.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            use std::fmt::Write;
            let _ = write!(out, "%{b:02X}");
        }
    }
    out
}
#[async_trait]
impl Interaction for JobIo {
    async fn verify(
        &self,
        challenge: Zeroizing<String>,
        methods: Vec<String>,
        cancel: &CancellationToken,
    ) -> Result<(Zeroizing<String>, String), Failure> {
        let id = Uuid::new_v4();
        let expiry = now_ms() + 300000;
        let digest = Sha256::digest(challenge.as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let handoff = Json(
            json!({ "job_id":self.snapshot()?.id, "challenge_id":id, "url":format!("https://verify.proton.me?embed=true&token={}&methods={}&theme=2&vpn=true",encode(&challenge),encode(&methods.join(","))), "expires_at":expiry, "methods":methods, "challenge_sha256":digest }),
        );
        let input = self
            .wait(
                Kind::Verification(id),
                Some(Zeroizing::new(
                    serde_json::to_vec(&handoff.0).map_err(|_| Failure::new("invalid_handoff"))?,
                )),
                300,
                cancel,
            )
            .await?;
        let value = Json(
            serde_json::from_slice(&input)
                .map_err(|_| Failure::new("invalid_verification_result"))?,
        );
        if value.get("type").and_then(|v| v.as_str()) != Some("HUMAN_VERIFICATION_SUCCESS") {
            return Err(Failure::new("invalid_verification_result"));
        }
        let token = super::api::token(value.pointer("/payload/token"))?;
        let kind = value
            .pointer("/payload/type")
            .and_then(|v| v.as_str())
            .filter(|v| {
                methods.iter().any(|method| method == v)
                    && matches!(*v, "captcha" | "sms" | "email")
            })
            .ok_or_else(|| Failure::new("unsupported_verification_method"))?;
        if now_ms() >= expiry {
            return Err(Failure::new("verification_expired"));
        }
        Ok((Zeroizing::new(token), kind.into()))
    }
    async fn totp(&self, cancel: &CancellationToken) -> Result<Zeroizing<String>, Failure> {
        let input = self.wait(Kind::Totp, None, 300, cancel).await?;
        if input.len() != 6 || !input.iter().all(u8::is_ascii_digit) {
            return Err(Failure::new("invalid_totp"));
        }
        Ok(Zeroizing::new(
            String::from_utf8(input.to_vec()).map_err(|_| Failure::new("invalid_totp"))?,
        ))
    }
    async fn node(&self, nodes: Vec<Node>, cancel: &CancellationToken) -> Result<String, Failure> {
        self.0
            .lock()
            .map_err(|_| Failure::new("unavailable"))?
            .job
            .nodes = nodes;
        let value = self.wait(Kind::Node, None, 600, cancel).await?;
        String::from_utf8(value.to_vec()).map_err(|_| Failure::new("invalid_node"))
    }
    fn phase(&self, phase: &str) {
        if let Ok(mut shared) = self.0.lock() {
            shared.job.phase = phase.into();
        }
    }
}
