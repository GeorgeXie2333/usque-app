//! Bounded Proton HTTPS over the caller's selected network. Error strings and response
//! bodies never escape this module into diagnostics or ordinary status.
use super::{ClientConfig, Failure, Interaction};
use crate::internal_network::{InternalHttpError, InternalRequest, InternalResponse};
use async_trait::async_trait;
use bytes::Bytes;
use http::Method;
use serde_json::Value;
use std::{sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;
use usque_core::proton_vpn::{API_ORIGIN, MAX_RESPONSE_BYTES, SessionSecrets};
use zeroize::{Zeroize, Zeroizing};

pub(super) struct Json(pub Value);
impl std::ops::Deref for Json {
    type Target = Value;
    fn deref(&self) -> &Value {
        &self.0
    }
}
impl Drop for Json {
    fn drop(&mut self) {
        clear(&mut self.0);
    }
}
fn clear(value: &mut Value) {
    match value {
        Value::String(text) => text.zeroize(),
        Value::Array(items) => items.iter_mut().for_each(clear),
        Value::Object(items) => items.values_mut().for_each(clear),
        _ => (),
    }
}
struct PrivateBody(Zeroizing<Vec<u8>>);
impl AsRef<[u8]> for PrivateBody {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

#[async_trait]
pub(super) trait Http: Send + Sync {
    async fn shutdown(&self) {}
    async fn exchange(
        &self,
        request: InternalRequest<'_>,
        cancel: &CancellationToken,
    ) -> Result<InternalResponse, InternalHttpError>;
    fn generation(&self) -> Option<u64>;
    fn current(&self, generation: Option<u64>) -> bool;
}

pub(super) struct Client {
    http: Arc<dyn Http>,
    pub session: SessionSecrets,
    pub retired: Vec<SessionSecrets>,
    pub uncertain: bool,
    pub config: ClientConfig,
}
impl Client {
    pub(super) fn network(&self) -> Arc<dyn Http> {
        self.http.clone()
    }
    pub fn generation(&self) -> Option<u64> {
        self.http.generation()
    }
    pub fn checkpoint(
        &self,
        generation: Option<u64>,
        cancel: &CancellationToken,
    ) -> Result<(), Failure> {
        if cancel.is_cancelled() {
            return Err(Failure::new("cancelled"));
        }
        if !self.http.current(generation) {
            return Err(Failure::new("underlay_changed"));
        }
        Ok(())
    }
    pub fn new(http: Arc<dyn Http>) -> Self {
        Self {
            http,
            session: SessionSecrets::default(),
            retired: Vec::new(),
            uncertain: false,
            config: ClientConfig::default(),
        }
    }
    pub fn accept_session(&mut self, response: &Value) -> Result<(), Failure> {
        let uid = token(response.get("UID"))?;
        let access_token = token(response.get("AccessToken"))?;
        // Capture a usable cleanup handle even if a malformed success omits its
        // refresh token. It does not authorize any protected operation.
        let replacement = SessionSecrets {
            uid,
            access_token,
            refresh_token: String::new(),
            user_id: String::new(),
        };
        if !self.session.uid.is_empty() && self.session.uid != replacement.uid {
            self.retired.push(std::mem::take(&mut self.session));
        }
        self.session = replacement;
        self.uncertain = false;
        self.session.refresh_token = token(response.get("RefreshToken"))?;
        self.session.user_id = response
            .get("UserID")
            .map(|value| token(Some(value)))
            .transpose()?
            .unwrap_or_default();
        Ok(())
    }
    pub async fn request(
        &mut self,
        method: Method,
        path: &str,
        body: &Value,
        interaction: &dyn Interaction,
        cancel: &CancellationToken,
    ) -> Result<Json, Failure> {
        if !path.starts_with('/')
            || path.starts_with("//")
            || path.contains(['\r', '\n', '#'])
            || path.len() > 2048
        {
            return Err(Failure::new("invalid_api_request"));
        }
        let bytes = if matches!(method, Method::GET | Method::DELETE) {
            Vec::new()
        } else {
            serde_json::to_vec(body).map_err(|_| Failure::new("invalid_api_request"))?
        };
        let bytes = Bytes::from_owner(PrivateBody(Zeroizing::new(bytes)));
        let mut answer = self
            .send(method.clone(), path, bytes.clone(), None, cancel)
            .await?;
        let mut invalid_verification_retried = false;
        while answer.0.get("Code").and_then(Value::as_u64) == Some(9001) {
            let details = answer
                .0
                .get("Details")
                .ok_or_else(|| Failure::new("invalid_verification_challenge"))?;
            let challenge_token = token(details.get("HumanVerificationToken"))?;
            let methods = details
                .get("HumanVerificationMethods")
                .and_then(Value::as_array)
                .filter(|v| !v.is_empty() && v.len() <= 16)
                .ok_or_else(|| Failure::new("invalid_verification_challenge"))?;
            let methods: Vec<String> = methods
                .iter()
                .map(|v| {
                    v.as_str()
                        .filter(|s| {
                            !s.is_empty()
                                && s.len() <= 32
                                && s.bytes().all(|b| b.is_ascii_alphanumeric())
                        })
                        .map(str::to_owned)
                        .ok_or_else(|| Failure::new("invalid_verification_challenge"))
                })
                .collect::<Result<_, _>>()?;
            if !methods
                .iter()
                .any(|v| matches!(v.as_str(), "captcha" | "sms" | "email"))
            {
                return Err(Failure::new("unsupported_verification_method"));
            }
            let generation = self.http.generation();
            let result = interaction
                .verify(Zeroizing::new(challenge_token), methods, cancel)
                .await?;
            if !self.http.current(generation) {
                return Err(Failure::new("verification_underlay_changed"));
            }
            if cancel.is_cancelled() {
                return Err(Failure::new("cancelled"));
            }
            answer = self
                .send(
                    method.clone(),
                    path,
                    bytes.clone(),
                    Some((&result.0, &result.1)),
                    cancel,
                )
                .await?;
            drop(result);
            if !invalid_verification_retried
                && (400..500).contains(&answer.1)
                && answer.0.get("Code").and_then(Value::as_u64) == Some(12087)
                && !self.uncertain
            {
                // Core clears the rejected HV result and retries the original
                // request, which may issue a fresh challenge. Never reuse the
                // rejected token or replace this job's session. Bound recovery
                // to one reset and one further manual challenge per request.
                self.checkpoint(generation, cancel)?;
                invalid_verification_retried = true;
                answer = self
                    .send(method.clone(), path, bytes.clone(), None, cancel)
                    .await?;
                continue;
            }
            break;
        }
        let code = answer
            .0
            .get("Code")
            .and_then(Value::as_u64)
            .and_then(|v| u32::try_from(v).ok())
            .ok_or_else(|| Failure::new("invalid_api_response"))?;
        if !(200..300).contains(&answer.1) || code != 1000 {
            return Err(Failure::api(
                if code == 9001 {
                    "verification_required"
                } else if code == 9002 {
                    "device_verification_required"
                } else if code == 8002 {
                    "invalid_credentials"
                } else if code == 8004 {
                    "session_not_allowed"
                } else if code == 10200 {
                    "credentialless_disabled"
                } else if code == 12087 {
                    "human_verification_invalid"
                } else if code == 429 || answer.1 == 429 {
                    "rate_limited"
                } else {
                    "api_rejected"
                },
                code,
                answer.1,
            ));
        }
        Ok(answer.0)
    }
    async fn send(
        &mut self,
        method: Method,
        path: &str,
        body: Bytes,
        verification: Option<(&str, &str)>,
        cancel: &CancellationToken,
    ) -> Result<(Json, u16), Failure> {
        let changes_session = method == Method::POST
            && matches!(
                path,
                "/auth/v4/sessions" | "/auth/v4/credentialless" | "/auth/v4" | "/auth/v4/refresh"
            );
        if changes_session {
            self.uncertain = true;
        }
        let mut attempts = 0;
        loop {
            let mut headers = vec![
                ("accept", "application/vnd.protonmail.v1+json"),
                ("x-pm-appversion", self.config.identity.app_version.as_str()),
                ("user-agent", self.config.identity.user_agent.as_str()),
            ];
            if !matches!(method, Method::GET | Method::DELETE) {
                headers.push(("content-type", "application/json"));
            }
            if !self.config.identity.locale.is_empty() {
                headers.push(("x-pm-locale", &self.config.identity.locale));
            }
            let bearer = Zeroizing::new(format!("Bearer {}", self.session.access_token));
            if !self.session.uid.is_empty() {
                headers.push(("x-pm-uid", &self.session.uid));
                headers.push(("authorization", &bearer));
            }
            if let Some((value, kind)) = verification {
                headers.push(("x-pm-human-verification-token", value));
                headers.push(("x-pm-human-verification-token-type", kind));
            }
            let response = self
                .http
                .exchange(
                    InternalRequest {
                        url: &format!("{API_ORIGIN}{path}"),
                        method: method.clone(),
                        headers,
                        body: body.clone(),
                        limit: MAX_RESPONSE_BYTES,
                        ipv6: None,
                    },
                    cancel,
                )
                .await
                .map_err(|failure| {
                    Failure::new(match failure {
                        InternalHttpError::Cancelled => "cancelled",
                        InternalHttpError::Timeout => "api_timeout",
                        InternalHttpError::Tls => "api_tls_failed",
                        InternalHttpError::SizeLimit => "api_size_limit",
                        InternalHttpError::Encoding => "api_encoding_rejected",
                        _ => "api_transport_failed",
                    })
                })?;
            if response.status == 429
                && method == Method::GET
                && verification.is_none()
                && attempts == 0
                && response.retry_after.is_some_and(|n| n <= 30)
            {
                attempts += 1;
                tokio::select! { biased; _ = cancel.cancelled() => return Err(Failure::new("cancelled")), _ = tokio::time::sleep(Duration::from_secs(response.retry_after.unwrap_or(1).into())) => () }
                continue;
            }
            let data = Json(
                serde_json::from_slice::<Value>(&response.body)
                    .map_err(|_| Failure::new("invalid_api_response"))?,
            );
            if !data.0.is_object() {
                return Err(Failure::new("invalid_api_response"));
            }
            let code = data.0.get("Code").and_then(Value::as_u64);
            if changes_session
                && (400..500).contains(&response.status)
                && code.is_some_and(|v| v != 1000 && v != 1001)
            {
                self.uncertain = false;
            }
            return Ok((data, response.status));
        }
    }
    pub async fn revoke_all(&mut self, cancel: &CancellationToken) -> bool {
        let mut sessions = std::mem::take(&mut self.retired);
        if !self.session.uid.is_empty() {
            sessions.push(std::mem::take(&mut self.session));
        }
        let mut seen = std::collections::BTreeSet::new();
        let mut success = !self.uncertain;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(25);
        let cleanup_cancel = cancel.child_token();
        for session in sessions.into_iter().rev() {
            if cleanup_cancel.is_cancelled() || tokio::time::Instant::now() >= deadline {
                success = false;
                break;
            }
            if !seen.insert(session.uid.clone()) {
                continue;
            }
            self.session = session;
            let answer = {
                let request = self.send(
                    Method::DELETE,
                    "/auth/v4",
                    Bytes::new(),
                    None,
                    &cleanup_cancel,
                );
                tokio::pin!(request);
                tokio::select! {
                    biased;
                    _=tokio::time::sleep_until(deadline)=>{cleanup_cancel.cancel();request.await},
                    result=&mut request=>result,
                }
            };
            success &= matches!(answer, Ok((ref data, status)) if (200..300).contains(&status) && data.0.get("Code").and_then(Value::as_u64) == Some(1000));
            self.session = SessionSecrets::default();
        }
        success
    }
    pub async fn revoke_retired(&mut self, cancel: &CancellationToken) -> bool {
        if self.retired.is_empty() {
            return true;
        }
        let current = std::mem::take(&mut self.session);
        let result = self.revoke_all(cancel).await;
        self.session = current;
        result
    }
}
pub(super) fn token(value: Option<&Value>) -> Result<String, Failure> {
    value
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty() && s.len() <= 8192 && s.bytes().all(|b| (33..=126).contains(&b)))
        .map(str::to_owned)
        .ok_or_else(|| Failure::new("invalid_api_response"))
}
