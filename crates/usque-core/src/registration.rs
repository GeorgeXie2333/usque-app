use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use chrono::{SecondsFormat, Utc};
use p256::elliptic_curve::Generate;
use reqwest::header::HeaderValue;
use reqwest::{Client, Method, StatusCode, Url};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use thiserror::Error;
use zeroize::Zeroizing;

use crate::config::EndpointSettings;
use crate::identity::{
    ConsumerEntitlement, EndpointPin, IdentityError, IdentityProvider, MasqueKeyPair, WarpIdentity,
};

const API_ROOT: &str = "https://api.cloudflareclient.com/";
const API_VERSION: &str = "v0a4471";
const CF_CLIENT_VERSION: &str = "a-6.35-4471";
const MAX_API_RESPONSE_BYTES: usize = 1024 * 1024;
const MAX_ZERO_TRUST_CALLBACK_BYTES: usize = 64 * 1024;
pub const ZERO_TRUST_SNI: &str = "zt-masque.cloudflareclient.com";
pub const ZERO_TRUST_PORT: u16 = 443;
pub const REGISTRATION_API_HOST: &str = "api.cloudflareclient.com";
pub const REGISTRATION_API_PORT: u16 = 443;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZeroTrustRegistrationStage {
    DeviceRegistration,
    MasqueEnrollment,
}

impl ZeroTrustRegistrationStage {
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::DeviceRegistration => "device_registration",
            Self::MasqueEnrollment => "masque_enrollment",
        }
    }
}

impl std::fmt::Display for ZeroTrustRegistrationStage {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_code())
    }
}

/// A validated one-time Cloudflare Access callback. Its assertion is always
/// redacted from `Debug` and zeroized on drop.
pub struct ZeroTrustCallback {
    organization: String,
    assertion: Zeroizing<String>,
}

impl ZeroTrustCallback {
    pub fn organization(&self) -> &str {
        &self.organization
    }

    fn assertion(&self) -> &str {
        &self.assertion
    }
}

impl std::fmt::Debug for ZeroTrustCallback {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ZeroTrustCallback")
            .field("organization", &self.organization)
            .field("assertion", &"[REDACTED]")
            .finish()
    }
}

#[derive(Debug)]
pub struct ZeroTrustRegistrationResult {
    pub identity: WarpIdentity,
    pub endpoint: EndpointSettings,
}

pub fn normalize_zero_trust_team(team: &str) -> Result<String, RegistrationError> {
    let normalized = team.trim().to_ascii_lowercase();
    IdentityProvider::zero_trust(normalized.clone())
        .map(|_| normalized)
        .map_err(|_| RegistrationError::InvalidZeroTrustTeam)
}

pub fn zero_trust_login_url(team: &str) -> Result<Url, RegistrationError> {
    let team = normalize_zero_trust_team(team)?;
    Url::parse(&format!("https://{team}.cloudflareaccess.com/warp"))
        .map_err(|_| RegistrationError::InvalidZeroTrustTeam)
}

/// Returns true when a persisted profile still carries a Zero Trust ingress
/// marker. This is intentionally broader than enrollment validation: any one
/// marker is enough to prevent missing identity metadata from silently
/// reclassifying an organization profile as Consumer WARP.
pub fn is_zero_trust_endpoint(endpoint: &EndpointSettings) -> bool {
    endpoint.is_zero_trust_managed()
}

pub fn parse_zero_trust_callback(
    team: &str,
    callback_uri: &str,
) -> Result<ZeroTrustCallback, RegistrationError> {
    let organization = normalize_zero_trust_team(team)?;
    if callback_uri.len() > MAX_ZERO_TRUST_CALLBACK_BYTES {
        return Err(RegistrationError::InvalidZeroTrustCallback);
    }
    let callback_uri = callback_uri.trim();
    if callback_uri.is_empty() {
        return Err(RegistrationError::InvalidZeroTrustCallback);
    }
    let callback =
        Url::parse(callback_uri).map_err(|_| RegistrationError::InvalidZeroTrustCallback)?;
    let expected_host = format!("{organization}.cloudflareaccess.com");
    if callback.scheme() != "com.cloudflare.warp"
        || callback.host_str() != Some(expected_host.as_str())
        || callback.path() != "/auth"
        || !callback.username().is_empty()
        || callback.password().is_some()
        || callback.port().is_some()
        || callback.fragment().is_some()
    {
        return Err(RegistrationError::InvalidZeroTrustCallback);
    }

    let mut assertion = None;
    for (key, value) in callback.query_pairs() {
        if key != "token" || assertion.is_some() || value.is_empty() {
            return Err(RegistrationError::InvalidZeroTrustCallback);
        }
        assertion = Some(value.into_owned());
    }
    let assertion = assertion.ok_or(RegistrationError::InvalidZeroTrustCallback)?;
    HeaderValue::from_str(&assertion).map_err(|_| RegistrationError::InvalidZeroTrustCallback)?;
    Ok(ZeroTrustCallback {
        organization,
        assertion: Zeroizing::new(assertion),
    })
}

#[derive(Debug, Clone)]
pub struct RegistrationOptions {
    pub terms_accepted: bool,
    pub model: String,
    pub device_name: Option<String>,
    pub locale: String,
}

impl Default for RegistrationOptions {
    fn default() -> Self {
        Self {
            terms_accepted: false,
            model: "PC".to_owned(),
            device_name: None,
            locale: "en_US".to_owned(),
        }
    }
}

/// Result of an authenticated enrollment refresh after a pin mismatch.
///
/// The engine may install this pin only after this method succeeds, and may
/// retry the failed tunnel exactly once. The orchestrator owns that retry
/// budget; this type does not make unauthenticated pin replacement possible.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndpointPinRefresh {
    pub endpoint_pin: EndpointPin,
    pub assigned_ipv4: Ipv4Addr,
    pub assigned_ipv6: Ipv6Addr,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WarpAccountStatus {
    pub account_type: String,
    pub entitlement: ConsumerEntitlement,
}

/// A bounded, authenticated endpoint-pin refresh request for a platform
/// transport that must create and protect its own socket.
///
/// The bearer token is always redacted from `Debug` output and zeroized when
/// this value is dropped. The request body contains only the public MASQUE key
/// and an optional device name.
pub struct PreparedEndpointPinRefresh {
    path_and_query: String,
    bearer_token: Zeroizing<String>,
    body: Vec<u8>,
}

impl PreparedEndpointPinRefresh {
    pub const fn user_agent(&self) -> &'static str {
        "WARP for Android"
    }

    pub const fn client_version(&self) -> &'static str {
        CF_CLIENT_VERSION
    }

    pub fn path_and_query(&self) -> &str {
        &self.path_and_query
    }

    pub fn bearer_token(&self) -> &str {
        &self.bearer_token
    }

    pub fn body(&self) -> &[u8] {
        &self.body
    }
}

impl std::fmt::Debug for PreparedEndpointPinRefresh {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PreparedEndpointPinRefresh")
            .field("path_and_query", &self.path_and_query)
            .field("bearer_token", &"[REDACTED]")
            .field("body_bytes", &self.body.len())
            .finish()
    }
}

/// Prepares the exact authenticated PATCH used to refresh an enrolled
/// Consumer WARP endpoint pin. Platform transports use this form when their
/// socket must be exempted from a VPN interface or constrained by a kill
/// switch before it connects.
pub fn prepare_endpoint_pin_refresh(
    identity: &WarpIdentity,
    device_name: Option<&str>,
) -> Result<PreparedEndpointPinRefresh, RegistrationError> {
    let path_and_query = registration_path(identity.device_id())?;
    if identity.access_token().trim().is_empty() {
        return Err(RegistrationError::InvalidApiResponse);
    }
    let body = EnrollmentRequest {
        key: BASE64_STANDARD.encode(identity.key_pair.public_spki_der()?),
        key_type: "secp256r1",
        tunnel_type: "masque",
        name: device_name.filter(|name| !name.trim().is_empty()),
    };
    let body = serde_json::to_vec(&body).map_err(|_| RegistrationError::RequestSerialization)?;
    Ok(PreparedEndpointPinRefresh {
        path_and_query,
        bearer_token: Zeroizing::new(identity.access_token().to_owned()),
        body,
    })
}

/// Validates a bounded registration response returned by a protected platform
/// transport. A non-200 status can never install a replacement pin.
pub fn parse_endpoint_pin_refresh_response(
    status: u16,
    bytes: &[u8],
) -> Result<EndpointPinRefresh, RegistrationError> {
    if bytes.len() > MAX_API_RESPONSE_BYTES {
        return Err(RegistrationError::ApiResponseTooLarge);
    }
    let status = StatusCode::from_u16(status).map_err(|_| RegistrationError::InvalidApiResponse)?;
    if status != StatusCode::OK {
        return Err(api_error(status, bytes));
    }
    let enrollment = serde_json::from_slice::<AccountData>(bytes)
        .map_err(|_| RegistrationError::InvalidApiResponse)?;
    enrollment_snapshot(&enrollment)
}

#[derive(Clone)]
pub struct ConsumerRegistrationClient {
    http: Client,
    api_root: Url,
}

impl ConsumerRegistrationClient {
    pub fn new() -> Result<Self, RegistrationError> {
        let http = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(20))
            .build()?;
        let api_root = Url::parse(API_ROOT).map_err(|_| RegistrationError::InvalidApiUrl)?;
        Ok(Self { http, api_root })
    }

    #[cfg(test)]
    fn with_api_root(api_root: Url) -> Result<Self, RegistrationError> {
        let http = Client::builder()
            .connect_timeout(Duration::from_secs(2))
            .timeout(Duration::from_secs(5))
            .build()?;
        Ok(Self { http, api_root })
    }

    /// Creates a Consumer WARP registration and immediately enrolls a fresh
    /// P-256 MASQUE key.
    pub async fn register(
        &self,
        options: &RegistrationOptions,
    ) -> Result<WarpIdentity, RegistrationError> {
        validate_options(options)?;

        let request = RegistrationRequest::new(options);
        let registration_url = self.registration_url(None)?;
        let registered: AccountData = self
            .send_json(
                Method::POST,
                registration_url,
                RequestAuthentication::None,
                ApiErrorContext::Consumer,
                &request,
            )
            .await?;
        if registered.id.trim().is_empty() || registered.token.trim().is_empty() {
            return Err(RegistrationError::InvalidApiResponse);
        }

        let key_pair = MasqueKeyPair::generate();
        let enrolled = self
            .enroll(
                &registered.id,
                &registered.token,
                &key_pair.public_spki_der()?,
                options.device_name.as_deref(),
                ApiErrorContext::Consumer,
            )
            .await?;
        identity_from_enrollment(
            key_pair,
            registered.token,
            IdentityProvider::Consumer,
            enrolled,
            false,
        )
    }

    /// Exchanges a one-time Cloudflare Access assertion for a persistent
    /// organization device registration, then enrolls a fresh P-256 MASQUE
    /// identity. The Access assertion is never returned or persisted.
    pub async fn register_zero_trust(
        &self,
        options: &RegistrationOptions,
        team: &str,
        callback_uri: &str,
    ) -> Result<ZeroTrustRegistrationResult, RegistrationError> {
        validate_options(options)?;
        let callback = parse_zero_trust_callback(team, callback_uri)?;
        let request = RegistrationRequest::new(options);
        let registered: AccountData = self
            .send_json(
                Method::POST,
                self.registration_url(None)?,
                RequestAuthentication::AccessAssertion(callback.assertion()),
                ApiErrorContext::ZeroTrustLogin,
                &request,
            )
            .await
            .map_err(zero_trust_contract_error)?;
        if registered.id.trim().is_empty() || registered.token.trim().is_empty() {
            return Err(RegistrationError::ZeroTrustContractChanged);
        }

        let registration_id = registered.id;
        let device_token = registered.token;
        let key_pair = MasqueKeyPair::generate();
        let enrolled = self
            .enroll(
                &registration_id,
                &device_token,
                &key_pair.public_spki_der()?,
                options.device_name.as_deref(),
                ApiErrorContext::ZeroTrustEnrollment,
            )
            .await
            .map_err(zero_trust_contract_error)?;
        let endpoint = zero_trust_endpoint(&enrolled)?;
        let provider = IdentityProvider::zero_trust(callback.organization().to_owned())
            .map_err(|_| RegistrationError::InvalidZeroTrustTeam)?;
        let identity = identity_from_enrollment(key_pair, device_token, provider, enrolled, false)
            .map_err(|error| match error {
                RegistrationError::InvalidApiResponse | RegistrationError::Identity(_) => {
                    RegistrationError::ZeroTrustContractChanged
                }
                other => other,
            })?;
        Ok(ZeroTrustRegistrationResult { identity, endpoint })
    }

    /// Creates a new Consumer device, binds it to an existing WARP License,
    /// and re-enrolls the MASQUE key against the resulting account.
    pub async fn register_with_license(
        &self,
        options: &RegistrationOptions,
        license_key: &str,
    ) -> Result<WarpIdentity, RegistrationError> {
        validate_license_key(license_key)?;
        let identity = self.register(options).await?;
        self.bind_license(&identity, license_key).await?;
        let access_token = identity.access_token().to_owned();
        let enrolled = self
            .enroll(
                identity.device_id(),
                identity.access_token(),
                &identity.key_pair.public_spki_der()?,
                options.device_name.as_deref(),
                ApiErrorContext::Consumer,
            )
            .await?;
        identity_from_enrollment(
            identity.key_pair,
            access_token,
            IdentityProvider::Consumer,
            enrolled,
            true,
        )
    }

    pub async fn account_status(
        &self,
        identity: &WarpIdentity,
    ) -> Result<WarpAccountStatus, RegistrationError> {
        let account: Account = self
            .send_without_body(
                Method::GET,
                self.account_url(identity.device_id())?,
                identity.access_token(),
            )
            .await?;
        Ok(WarpAccountStatus {
            account_type: account.account_type.clone(),
            entitlement: consumer_entitlement(&account),
        })
    }

    pub async fn bind_license(
        &self,
        identity: &WarpIdentity,
        license_key: &str,
    ) -> Result<(), RegistrationError> {
        validate_license_key(license_key)?;
        self.send_empty(
            Method::PUT,
            self.account_url(identity.device_id())?,
            identity.access_token(),
            Some(&LicenseUpdate {
                license: license_key,
            }),
        )
        .await
    }

    pub async fn unbind_license(&self, identity: &WarpIdentity) -> Result<(), RegistrationError> {
        self.send_empty::<LicenseUpdate<'_>>(
            Method::DELETE,
            self.account_url(identity.device_id())?,
            identity.access_token(),
            None,
        )
        .await
    }

    /// Re-enrolls the existing public key using the stored device bearer token.
    /// This is the only supported source for replacing an endpoint pin.
    pub async fn refresh_endpoint_pin(
        &self,
        identity: &WarpIdentity,
        device_name: Option<&str>,
    ) -> Result<EndpointPinRefresh, RegistrationError> {
        let enrolled = self
            .enroll(
                identity.device_id(),
                identity.access_token(),
                &identity.key_pair.public_spki_der()?,
                device_name,
                if matches!(identity.provider(), IdentityProvider::ZeroTrust { .. }) {
                    ApiErrorContext::ZeroTrustEnrollment
                } else {
                    ApiErrorContext::Consumer
                },
            )
            .await?;
        enrollment_snapshot(&enrolled)
    }

    async fn enroll(
        &self,
        device_id: &str,
        access_token: &str,
        public_spki_der: &[u8],
        device_name: Option<&str>,
        error_context: ApiErrorContext,
    ) -> Result<AccountData, RegistrationError> {
        validate_device_id(device_id)?;
        if access_token.trim().is_empty() {
            return Err(RegistrationError::InvalidApiResponse);
        }
        let body = EnrollmentRequest {
            key: BASE64_STANDARD.encode(public_spki_der),
            key_type: "secp256r1",
            tunnel_type: "masque",
            name: device_name.filter(|name| !name.trim().is_empty()),
        };
        self.send_json(
            Method::PATCH,
            self.registration_url(Some(device_id))?,
            RequestAuthentication::Bearer(access_token),
            error_context,
            &body,
        )
        .await
    }

    async fn send_json<Request, Response>(
        &self,
        method: Method,
        url: Url,
        authentication: RequestAuthentication<'_>,
        error_context: ApiErrorContext,
        body: &Request,
    ) -> Result<Response, RegistrationError>
    where
        Request: Serialize + ?Sized,
        Response: DeserializeOwned,
    {
        let mut request = self
            .http
            .request(method, url)
            .header("User-Agent", "WARP for Android")
            .header("CF-Client-Version", CF_CLIENT_VERSION)
            .header("Content-Type", "application/json; charset=UTF-8")
            .header("Connection", "Keep-Alive")
            .json(body);
        match authentication {
            RequestAuthentication::None => {}
            RequestAuthentication::Bearer(token) => request = request.bearer_auth(token),
            RequestAuthentication::AccessAssertion(assertion) => {
                let assertion = HeaderValue::from_str(assertion)
                    .map_err(|_| RegistrationError::InvalidZeroTrustCallback)?;
                request = request.header("CF-Access-Jwt-Assertion", assertion);
            }
        }

        let response = request
            .send()
            .await
            .map_err(|error| request_error_for_context(error_context, error))?;
        let (status, bytes) = bounded_response(response)
            .await
            .map_err(|error| response_error_for_context(error_context, error))?;
        if status != StatusCode::OK {
            return Err(api_error_for_context(error_context, status, &bytes));
        }
        serde_json::from_slice(&bytes).map_err(|_| match error_context {
            ApiErrorContext::Consumer => RegistrationError::InvalidApiResponse,
            ApiErrorContext::ZeroTrustLogin | ApiErrorContext::ZeroTrustEnrollment => {
                RegistrationError::ZeroTrustContractChanged
            }
        })
    }

    async fn send_without_body<Response>(
        &self,
        method: Method,
        url: Url,
        bearer_token: &str,
    ) -> Result<Response, RegistrationError>
    where
        Response: DeserializeOwned,
    {
        let response = self
            .http
            .request(method, url)
            .header("User-Agent", "WARP for Android")
            .header("CF-Client-Version", CF_CLIENT_VERSION)
            .header("Connection", "Keep-Alive")
            .bearer_auth(bearer_token)
            .send()
            .await?;
        let (status, bytes) = bounded_response(response).await?;
        if status != StatusCode::OK {
            return Err(api_error(status, &bytes));
        }
        serde_json::from_slice(&bytes).map_err(|_| RegistrationError::InvalidApiResponse)
    }

    async fn send_empty<Request>(
        &self,
        method: Method,
        url: Url,
        bearer_token: &str,
        body: Option<&Request>,
    ) -> Result<(), RegistrationError>
    where
        Request: Serialize + ?Sized,
    {
        let mut request = self
            .http
            .request(method, url)
            .header("User-Agent", "WARP for Android")
            .header("CF-Client-Version", CF_CLIENT_VERSION)
            .header("Connection", "Keep-Alive")
            .bearer_auth(bearer_token);
        if let Some(body) = body {
            request = request
                .header("Content-Type", "application/json; charset=UTF-8")
                .json(body);
        }
        let response = request.send().await?;
        let (status, bytes) = bounded_response(response).await?;
        if status != StatusCode::OK {
            return Err(api_error(status, &bytes));
        }
        Ok(())
    }

    fn registration_url(&self, device_id: Option<&str>) -> Result<Url, RegistrationError> {
        let mut url = self.api_root.clone();
        append_registration_path(&mut url, device_id)?;
        Ok(url)
    }

    fn account_url(&self, device_id: &str) -> Result<Url, RegistrationError> {
        validate_device_id(device_id)?;
        let mut url = self.registration_url(Some(device_id))?;
        url.path_segments_mut()
            .map_err(|_| RegistrationError::InvalidApiUrl)?
            .push("account");
        Ok(url)
    }
}

async fn bounded_response(
    mut response: reqwest::Response,
) -> Result<(StatusCode, Vec<u8>), RegistrationError> {
    let status = response.status();
    if response
        .content_length()
        .is_some_and(|length| length > MAX_API_RESPONSE_BYTES as u64)
    {
        return Err(RegistrationError::ApiResponseTooLarge);
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        let next_length = bytes
            .len()
            .checked_add(chunk.len())
            .ok_or(RegistrationError::ApiResponseTooLarge)?;
        if next_length > MAX_API_RESPONSE_BYTES {
            return Err(RegistrationError::ApiResponseTooLarge);
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok((status, bytes))
}

#[derive(Serialize)]
struct RegistrationRequest<'a> {
    key: String,
    install_id: &'static str,
    fcm_token: &'static str,
    tos: String,
    model: &'a str,
    serial_number: String,
    os_version: &'static str,
    key_type: &'static str,
    tunnel_type: &'static str,
    locale: &'a str,
}

impl<'a> RegistrationRequest<'a> {
    fn new(options: &'a RegistrationOptions) -> Self {
        let wireguard_placeholder = <[u8; 32]>::generate();
        let serial = <[u8; 8]>::generate();
        Self {
            key: BASE64_STANDARD.encode(wireguard_placeholder),
            install_id: "",
            fcm_token: "",
            tos: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
            model: options.model.trim(),
            serial_number: hex_lower(&serial),
            os_version: "",
            key_type: "curve25519",
            tunnel_type: "wireguard",
            locale: options.locale.trim(),
        }
    }
}

#[derive(Serialize)]
struct EnrollmentRequest<'a> {
    key: String,
    key_type: &'static str,
    #[serde(rename = "tunnel_type")]
    tunnel_type: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<&'a str>,
}

#[derive(Serialize)]
struct LicenseUpdate<'a> {
    license: &'a str,
}

#[derive(Serialize, Deserialize)]
struct AccountData {
    id: String,
    #[serde(default)]
    token: String,
    #[serde(default)]
    account: Account,
    config: AccountConfig,
}

#[derive(Default, Serialize, Deserialize)]
struct Account {
    #[serde(default)]
    account_type: String,
    #[serde(default)]
    warp_plus: bool,
    #[serde(default)]
    premium_data: u64,
    #[serde(default)]
    quota: u64,
    #[serde(default)]
    license: Option<String>,
}

/// Cloudflare's `warp_plus` boolean is true for brand-new Free accounts.
/// Entitlement comes from remaining Plus data or an unlimited account type.
fn consumer_entitlement(account: &Account) -> ConsumerEntitlement {
    let kind = account.account_type.trim().to_ascii_lowercase();
    if matches!(kind.as_str(), "unlimited" | "plus" | "premium") {
        return ConsumerEntitlement::WarpPlus;
    }
    if account.premium_data > 0 || account.quota > 0 {
        return ConsumerEntitlement::WarpPlus;
    }
    ConsumerEntitlement::Free
}

#[derive(Serialize, Deserialize)]
struct AccountConfig {
    peers: Vec<Peer>,
    interface: Interface,
}

#[derive(Serialize, Deserialize)]
struct Peer {
    public_key: String,
    #[serde(default)]
    endpoint: Option<PeerEndpoint>,
}

#[derive(Default, Serialize, Deserialize)]
struct PeerEndpoint {
    #[serde(default)]
    v4: String,
    #[serde(default)]
    v6: String,
    #[serde(default)]
    host: String,
    #[serde(default)]
    ports: Vec<u16>,
}

#[derive(Serialize, Deserialize)]
struct Interface {
    addresses: AssignedAddresses,
}

#[derive(Serialize, Deserialize)]
struct AssignedAddresses {
    v4: String,
    v6: String,
}

#[derive(Deserialize)]
struct ApiErrorEnvelope {
    #[serde(default)]
    errors: Vec<ApiErrorItem>,
}

#[derive(Deserialize)]
struct ApiErrorItem {
    #[serde(default)]
    message: String,
}

fn validate_options(options: &RegistrationOptions) -> Result<(), RegistrationError> {
    if !options.terms_accepted {
        return Err(RegistrationError::TermsNotAccepted);
    }
    if options.model.trim().is_empty()
        || options.model.chars().count() > 128
        || options.locale.trim().is_empty()
        || options.locale.chars().count() > 32
        || options
            .device_name
            .as_ref()
            .is_some_and(|name| name.chars().count() > 128)
    {
        return Err(RegistrationError::InvalidRegistrationOptions);
    }
    Ok(())
}

/// Validates the opaque registration identifier returned by Cloudflare.
///
/// Consumer registrations currently look UUID-like, but Zero Trust
/// registrations are not covered by that wire-format guarantee. Keep the
/// value bounded and free of whitespace/control bytes, then always pass it to
/// `Url::path_segments_mut().push()` so it cannot create additional API path
/// segments.
fn validate_device_id(device_id: &str) -> Result<(), RegistrationError> {
    if device_id.is_empty()
        || device_id.len() > 256
        || matches!(device_id, "." | "..")
        || !device_id.bytes().all(|byte| byte.is_ascii_graphic())
    {
        return Err(RegistrationError::InvalidDeviceId);
    }
    Ok(())
}

fn append_registration_path(
    url: &mut Url,
    device_id: Option<&str>,
) -> Result<(), RegistrationError> {
    if let Some(device_id) = device_id {
        validate_device_id(device_id)?;
    }
    let mut segments = url
        .path_segments_mut()
        .map_err(|_| RegistrationError::InvalidApiUrl)?;
    segments.pop_if_empty().push(API_VERSION).push("reg");
    if let Some(device_id) = device_id {
        segments.push(device_id);
    }
    Ok(())
}

fn registration_path(device_id: &str) -> Result<String, RegistrationError> {
    let mut url = Url::parse("https://registration.invalid/")
        .map_err(|_| RegistrationError::InvalidApiUrl)?;
    append_registration_path(&mut url, Some(device_id))?;
    Ok(url.path().to_owned())
}

fn validate_license_key(license_key: &str) -> Result<(), RegistrationError> {
    let value = license_key.trim();
    let valid = value.len() == 26
        && value.as_bytes().get(8) == Some(&b'-')
        && value.as_bytes().get(17) == Some(&b'-')
        && value
            .bytes()
            .enumerate()
            .all(|(index, byte)| matches!(index, 8 | 17) || byte.is_ascii_alphanumeric());
    if valid {
        Ok(())
    } else {
        Err(RegistrationError::InvalidLicenseKey)
    }
}

fn enrollment_snapshot(enrollment: &AccountData) -> Result<EndpointPinRefresh, RegistrationError> {
    let peer = enrollment
        .config
        .peers
        .first()
        .ok_or(RegistrationError::InvalidApiResponse)?;
    Ok(EndpointPinRefresh {
        endpoint_pin: EndpointPin::from_pem(&peer.public_key)?,
        assigned_ipv4: enrollment
            .config
            .interface
            .addresses
            .v4
            .parse()
            .map_err(|_| RegistrationError::InvalidApiResponse)?,
        assigned_ipv6: enrollment
            .config
            .interface
            .addresses
            .v6
            .parse()
            .map_err(|_| RegistrationError::InvalidApiResponse)?,
    })
}

fn zero_trust_endpoint(enrollment: &AccountData) -> Result<EndpointSettings, RegistrationError> {
    let endpoint = enrollment
        .config
        .peers
        .first()
        .and_then(|peer| peer.endpoint.as_ref())
        .ok_or(RegistrationError::ZeroTrustContractChanged)?;
    let ipv4 =
        parse_endpoint_ipv4(&endpoint.v4).ok_or(RegistrationError::ZeroTrustContractChanged)?;
    let ipv6 =
        parse_endpoint_ipv6(&endpoint.v6).ok_or(RegistrationError::ZeroTrustContractChanged)?;
    if ipv4.octets()[..3] != [162, 159, 197] || ipv6.segments()[..3] != [0x2606, 0x4700, 0x0102] {
        return Err(RegistrationError::ZeroTrustContractChanged);
    }
    let settings = EndpointSettings {
        ipv4,
        ipv6,
        port: ZERO_TRUST_PORT,
        sni: ZERO_TRUST_SNI.to_owned(),
    };
    settings
        .validate()
        .map_err(|_| RegistrationError::ZeroTrustContractChanged)?;
    Ok(settings)
}

fn parse_endpoint_ipv4(value: &str) -> Option<Ipv4Addr> {
    value.parse::<Ipv4Addr>().ok().or_else(|| {
        value
            .parse::<SocketAddr>()
            .ok()?
            .ip()
            .to_string()
            .parse()
            .ok()
    })
}

fn parse_endpoint_ipv6(value: &str) -> Option<Ipv6Addr> {
    value.parse::<Ipv6Addr>().ok().or_else(|| {
        value
            .parse::<SocketAddr>()
            .ok()?
            .ip()
            .to_string()
            .parse()
            .ok()
    })
}

fn identity_from_enrollment(
    key_pair: MasqueKeyPair,
    access_token: String,
    provider: IdentityProvider,
    mut enrollment: AccountData,
    retain_consumer_license: bool,
) -> Result<WarpIdentity, RegistrationError> {
    validate_device_id(&enrollment.id)?;
    let snapshot = enrollment_snapshot(&enrollment)?;
    let (license, entitlement) = if matches!(provider, IdentityProvider::ZeroTrust { .. }) {
        // Zero Trust does not use Consumer license binding. Some private API
        // variants may still include the account field, so ignore it without
        // weakening WarpIdentity's provider invariant.
        (None, None)
    } else {
        let entitlement = consumer_entitlement(&enrollment.account);
        let license = if retain_consumer_license {
            enrollment.account.license.take()
        } else {
            None
        };
        (license, Some(entitlement))
    };
    WarpIdentity::new(
        key_pair,
        snapshot.endpoint_pin,
        enrollment.id,
        access_token,
        license,
        provider,
        entitlement,
        snapshot.assigned_ipv4,
        snapshot.assigned_ipv6,
    )
    .map_err(Into::into)
}

#[derive(Debug, Clone, Copy)]
enum ApiErrorContext {
    Consumer,
    ZeroTrustLogin,
    ZeroTrustEnrollment,
}

impl ApiErrorContext {
    const fn zero_trust_stage(self) -> Option<ZeroTrustRegistrationStage> {
        match self {
            Self::Consumer => None,
            Self::ZeroTrustLogin => Some(ZeroTrustRegistrationStage::DeviceRegistration),
            Self::ZeroTrustEnrollment => Some(ZeroTrustRegistrationStage::MasqueEnrollment),
        }
    }
}

#[derive(Clone, Copy)]
enum RequestAuthentication<'a> {
    None,
    Bearer(&'a str),
    AccessAssertion(&'a str),
}

fn zero_trust_contract_error(error: RegistrationError) -> RegistrationError {
    match error {
        RegistrationError::ApiResponseTooLarge
        | RegistrationError::InvalidApiResponse
        | RegistrationError::Identity(_) => RegistrationError::ZeroTrustContractChanged,
        other => other,
    }
}

fn request_error_for_context(context: ApiErrorContext, error: reqwest::Error) -> RegistrationError {
    match context.zero_trust_stage() {
        Some(stage) => RegistrationError::ZeroTrustNetwork { stage },
        None => RegistrationError::Http(error),
    }
}

fn response_error_for_context(
    context: ApiErrorContext,
    error: RegistrationError,
) -> RegistrationError {
    match (context.zero_trust_stage(), error) {
        (Some(stage), RegistrationError::Http(_)) => RegistrationError::ZeroTrustNetwork { stage },
        (_, error) => error,
    }
}

fn api_error_for_context(
    context: ApiErrorContext,
    status: StatusCode,
    bytes: &[u8],
) -> RegistrationError {
    match context {
        ApiErrorContext::Consumer => api_error(status, bytes),
        ApiErrorContext::ZeroTrustLogin | ApiErrorContext::ZeroTrustEnrollment
            if status == StatusCode::UNAUTHORIZED =>
        {
            RegistrationError::ZeroTrustLoginExpired
        }
        ApiErrorContext::ZeroTrustLogin | ApiErrorContext::ZeroTrustEnrollment
            if status == StatusCode::FORBIDDEN =>
        {
            RegistrationError::ZeroTrustLoginDenied
        }
        ApiErrorContext::ZeroTrustLogin | ApiErrorContext::ZeroTrustEnrollment => {
            RegistrationError::ZeroTrustRegistrationFailed {
                stage: context
                    .zero_trust_stage()
                    .expect("Zero Trust context has a stage"),
                status,
            }
        }
    }
}

fn api_error(status: StatusCode, bytes: &[u8]) -> RegistrationError {
    let message = serde_json::from_slice::<ApiErrorEnvelope>(bytes)
        .ok()
        .and_then(|envelope| {
            envelope
                .errors
                .into_iter()
                .find_map(|error| (!error.message.trim().is_empty()).then_some(error.message))
        })
        .map(|message| message.chars().take(256).collect())
        .unwrap_or_else(|| "Cloudflare registration request failed".to_owned());
    RegistrationError::Api { status, message }
}

fn hex_lower(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[derive(Debug, Error)]
pub enum RegistrationError {
    #[error("Cloudflare terms must be accepted before device registration")]
    TermsNotAccepted,
    #[error("registration model, locale, or device name is invalid")]
    InvalidRegistrationOptions,
    #[error("the registration API URL is invalid")]
    InvalidApiUrl,
    #[error("the registration API returned an invalid device identifier")]
    InvalidDeviceId,
    #[error("the WARP License Key format is invalid")]
    InvalidLicenseKey,
    #[error("the Cloudflare Zero Trust team name is invalid")]
    InvalidZeroTrustTeam,
    #[error("the Cloudflare Zero Trust login callback is invalid")]
    InvalidZeroTrustCallback,
    #[error("the Cloudflare Zero Trust login expired; start the organization login again")]
    ZeroTrustLoginExpired,
    #[error("the Cloudflare Zero Trust organization denied this device login")]
    ZeroTrustLoginDenied,
    #[error("Cloudflare Zero Trust {stage} failed with HTTP {status}")]
    ZeroTrustRegistrationFailed {
        stage: ZeroTrustRegistrationStage,
        status: StatusCode,
    },
    #[error("Cloudflare Zero Trust {stage} could not reach the registration service")]
    ZeroTrustNetwork { stage: ZeroTrustRegistrationStage },
    #[error("the experimental Cloudflare Zero Trust registration contract changed")]
    ZeroTrustContractChanged,
    #[error("the registration API returned more than 1 MiB")]
    ApiResponseTooLarge,
    #[error("the registration API returned an invalid response")]
    InvalidApiResponse,
    #[error("the registration request could not be serialized")]
    RequestSerialization,
    #[error("Cloudflare registration failed with {status}: {message}")]
    Api { status: StatusCode, message: String },
    #[error("registration network error: {0}")]
    Http(#[from] reqwest::Error),
    #[error(transparent)]
    Identity(#[from] IdentityError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use p256::PublicKey;
    use p256::pkcs8::{DecodePublicKey, EncodePublicKey, LineEnding};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn enrollment(key: &MasqueKeyPair) -> AccountData {
        let public = PublicKey::from_public_key_der(&key.public_spki_der().unwrap()).unwrap();
        AccountData {
            id: "device-123".to_owned(),
            token: String::new(),
            account: Account {
                account_type: String::new(),
                warp_plus: true,
                premium_data: 0,
                quota: 0,
                license: Some("license".to_owned()),
            },
            config: AccountConfig {
                peers: vec![Peer {
                    public_key: public.to_public_key_pem(LineEnding::LF).unwrap(),
                    endpoint: Some(PeerEndpoint {
                        v4: "162.159.197.2:0".to_owned(),
                        v6: "[2606:4700:102::2]:0".to_owned(),
                        host: ZERO_TRUST_SNI.to_owned(),
                        ports: vec![443],
                    }),
                }],
                interface: Interface {
                    addresses: AssignedAddresses {
                        v4: "172.16.0.2".to_owned(),
                        v6: "2606:4700:110:8f13::2".to_owned(),
                    },
                },
            },
        }
    }

    async fn serve_registration_response(
        listener: &tokio::net::TcpListener,
        response: &[u8],
    ) -> String {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        let header_end = loop {
            let mut chunk = [0_u8; 4096];
            let read = stream.read(&mut chunk).await.unwrap();
            assert!(read > 0);
            request.extend_from_slice(&chunk[..read]);
            if let Some(index) = request.windows(4).position(|value| value == b"\r\n\r\n") {
                break index + 4;
            }
        };
        let headers = String::from_utf8_lossy(&request[..header_end]).to_ascii_lowercase();
        let content_length = headers
            .lines()
            .find_map(|line| line.strip_prefix("content-length:"))
            .and_then(|value| value.trim().parse::<usize>().ok())
            .unwrap_or_default();
        while request.len() < header_end + content_length {
            let mut chunk = [0_u8; 4096];
            let read = stream.read(&mut chunk).await.unwrap();
            assert!(read > 0);
            request.extend_from_slice(&chunk[..read]);
        }
        let head = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            response.len()
        );
        stream.write_all(head.as_bytes()).await.unwrap();
        stream.write_all(response).await.unwrap();
        stream.shutdown().await.unwrap();
        String::from_utf8_lossy(&request).into_owned()
    }

    #[test]
    fn registration_requires_terms_acceptance() {
        assert!(matches!(
            validate_options(&RegistrationOptions::default()),
            Err(RegistrationError::TermsNotAccepted)
        ));
    }

    #[test]
    fn zero_trust_team_and_callback_are_strict_and_redacted() {
        assert_eq!(
            normalize_zero_trust_team(" Example-Team ").unwrap(),
            "example-team"
        );
        for invalid in ["", ".team", "team.example", "-team", "team-", "team_name"] {
            assert!(matches!(
                normalize_zero_trust_team(invalid),
                Err(RegistrationError::InvalidZeroTrustTeam)
            ));
        }

        let callback = parse_zero_trust_callback(
            "example-team",
            "com.cloudflare.warp://example-team.cloudflareaccess.com/auth?token=secret-assertion",
        )
        .unwrap();
        assert_eq!(callback.organization(), "example-team");
        assert!(!format!("{callback:?}").contains("secret-assertion"));

        for invalid in [
            "https://example-team.cloudflareaccess.com/auth?token=x",
            "com.cloudflare.warp://other.cloudflareaccess.com/auth?token=x",
            "com.cloudflare.warp://example-team.cloudflareaccess.com/warp?token=x",
            "com.cloudflare.warp://user@example-team.cloudflareaccess.com/auth?token=x",
            "com.cloudflare.warp://example-team.cloudflareaccess.com:443/auth?token=x",
            "com.cloudflare.warp://example-team.cloudflareaccess.com/auth?token=x#fragment",
            "com.cloudflare.warp://example-team.cloudflareaccess.com/auth",
            "com.cloudflare.warp://example-team.cloudflareaccess.com/auth?token=",
            "com.cloudflare.warp://example-team.cloudflareaccess.com/auth?token=x&token=y",
            "com.cloudflare.warp://example-team.cloudflareaccess.com/auth?token=x&state=y",
        ] {
            assert!(matches!(
                parse_zero_trust_callback("example-team", invalid),
                Err(RegistrationError::InvalidZeroTrustCallback)
            ));
        }

        let oversized = format!(
            "com.cloudflare.warp://example-team.cloudflareaccess.com/auth?token={}",
            "x".repeat(MAX_ZERO_TRUST_CALLBACK_BYTES)
        );
        assert!(matches!(
            parse_zero_trust_callback("example-team", &oversized),
            Err(RegistrationError::InvalidZeroTrustCallback)
        ));

        let oversized_outer_whitespace = format!(
            "{}com.cloudflare.warp://example-team.cloudflareaccess.com/auth?token=x",
            " ".repeat(MAX_ZERO_TRUST_CALLBACK_BYTES)
        );
        assert!(matches!(
            parse_zero_trust_callback("example-team", &oversized_outer_whitespace),
            Err(RegistrationError::InvalidZeroTrustCallback)
        ));
    }

    #[test]
    fn zero_trust_endpoint_requires_the_documented_dual_stack_ranges() {
        let mut enrolled = enrollment(&MasqueKeyPair::generate());
        let endpoint = zero_trust_endpoint(&enrolled).unwrap();
        assert_eq!(endpoint.ipv4.to_string(), "162.159.197.2");
        assert_eq!(endpoint.ipv6.to_string(), "2606:4700:102::2");
        assert_eq!(endpoint.port, 443);
        assert_eq!(endpoint.sni, ZERO_TRUST_SNI);

        enrolled.config.peers[0].endpoint.as_mut().unwrap().v4 = "162.159.198.2:443".to_owned();
        assert!(matches!(
            zero_trust_endpoint(&enrolled),
            Err(RegistrationError::ZeroTrustContractChanged)
        ));

        let mut enrolled = enrollment(&MasqueKeyPair::generate());
        enrolled.config.peers[0].endpoint.as_mut().unwrap().v6 =
            "[2606:4700:103::2]:443".to_owned();
        assert!(matches!(
            zero_trust_endpoint(&enrolled),
            Err(RegistrationError::ZeroTrustContractChanged)
        ));

        // The private API has returned empty/legacy host and port metadata in
        // the wild. The authenticated endpoint IPs remain authoritative while
        // Usque deliberately fixes the Zero Trust SNI and primary port.
        let mut enrolled = enrollment(&MasqueKeyPair::generate());
        enrolled.config.peers[0]
            .endpoint
            .as_mut()
            .unwrap()
            .host
            .clear();
        enrolled.config.peers[0].endpoint.as_mut().unwrap().ports = vec![500];
        let endpoint = zero_trust_endpoint(&enrolled).unwrap();
        assert_eq!(endpoint.port, ZERO_TRUST_PORT);
        assert_eq!(endpoint.sni, ZERO_TRUST_SNI);

        let mut enrolled = enrollment(&MasqueKeyPair::generate());
        enrolled.config.peers.clear();
        assert!(matches!(
            zero_trust_endpoint(&enrolled),
            Err(RegistrationError::ZeroTrustContractChanged)
        ));
    }

    #[test]
    fn zero_trust_enrollment_rejects_invalid_pin_and_assigned_addresses() {
        let mut enrolled = enrollment(&MasqueKeyPair::generate());
        enrolled.config.peers[0].public_key = "not-a-public-key".to_owned();
        assert!(matches!(
            identity_from_enrollment(
                MasqueKeyPair::generate(),
                "device-token".to_owned(),
                IdentityProvider::zero_trust("example-team").unwrap(),
                enrolled,
                false,
            ),
            Err(RegistrationError::Identity(
                IdentityError::InvalidEndpointPin
            ))
        ));

        let mut enrolled = enrollment(&MasqueKeyPair::generate());
        enrolled.config.interface.addresses.v4 = "not-an-ip".to_owned();
        assert!(matches!(
            identity_from_enrollment(
                MasqueKeyPair::generate(),
                "device-token".to_owned(),
                IdentityProvider::zero_trust("example-team").unwrap(),
                enrolled,
                false,
            ),
            Err(RegistrationError::InvalidApiResponse)
        ));

        let mut enrolled = enrollment(&MasqueKeyPair::generate());
        enrolled.config.interface.addresses.v6 = "not-an-ip".to_owned();
        assert!(matches!(
            identity_from_enrollment(
                MasqueKeyPair::generate(),
                "device-token".to_owned(),
                IdentityProvider::zero_trust("example-team").unwrap(),
                enrolled,
                false,
            ),
            Err(RegistrationError::InvalidApiResponse)
        ));
    }

    #[tokio::test]
    async fn zero_trust_assertion_is_post_only_and_device_bearer_is_patch_only() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let socket = listener.local_addr().unwrap();
        let endpoint_key = MasqueKeyPair::generate();
        let mut registered = enrollment(&endpoint_key);
        registered.token = "device-bearer".to_owned();
        let registered = serde_json::to_vec(&registered).unwrap();
        let mut enrolled = enrollment(&endpoint_key);
        enrolled.id = "physical-device-456".to_owned();
        let enrolled = serde_json::to_vec(&enrolled).unwrap();
        let server = tokio::spawn(async move {
            let post = serve_registration_response(&listener, &registered).await;
            let patch = serve_registration_response(&listener, &enrolled).await;
            (post.to_ascii_lowercase(), patch.to_ascii_lowercase())
        });
        let client = ConsumerRegistrationClient::with_api_root(
            Url::parse(&format!("http://{socket}/")).unwrap(),
        )
        .unwrap();
        let identity = client
            .register_zero_trust(
                &RegistrationOptions {
                    terms_accepted: true,
                    ..RegistrationOptions::default()
                },
                "example-team",
                "com.cloudflare.warp://example-team.cloudflareaccess.com/auth?token=assertion-for-test",
            )
            .await
            .unwrap()
            .identity;
        assert_eq!(identity.access_token(), "device-bearer");
        assert_eq!(identity.device_id(), "physical-device-456");
        assert!(identity.license().is_none());

        let (post, patch) = server.await.unwrap();
        assert!(post.starts_with("post /v0a4471/reg "));
        assert!(post.contains("cf-access-jwt-assertion: assertion-for-test"));
        assert!(!post.contains("authorization:"));
        assert!(patch.starts_with("patch /v0a4471/reg/device-123 "));
        assert!(patch.contains("authorization: bearer device-bearer"));
        assert!(!patch.contains("cf-access-jwt-assertion:"));
    }

    #[tokio::test]
    async fn bounded_response_stops_chunked_bodies_at_one_mebibyte() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let socket = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0_u8; 4096];
            let _ = stream.read(&mut request).await.unwrap();
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
                )
                .await
                .unwrap();
            let body = vec![b'x'; MAX_API_RESPONSE_BYTES + 1];
            let _ = stream
                .write_all(format!("{:x}\r\n", body.len()).as_bytes())
                .await;
            let _ = stream.write_all(&body).await;
            let _ = stream.write_all(b"\r\n0\r\n\r\n").await;
        });
        let response = reqwest::get(format!("http://{socket}/")).await.unwrap();
        assert!(matches!(
            bounded_response(response).await,
            Err(RegistrationError::ApiResponseTooLarge)
        ));
        server.await.unwrap();
    }

    #[test]
    fn zero_trust_http_errors_are_structured_without_response_text() {
        let secret_body = br#"{"errors":[{"message":"assertion-for-test"}]}"#;
        assert!(matches!(
            api_error_for_context(
                ApiErrorContext::ZeroTrustLogin,
                StatusCode::UNAUTHORIZED,
                secret_body,
            ),
            RegistrationError::ZeroTrustLoginExpired
        ));
        assert!(matches!(
            api_error_for_context(
                ApiErrorContext::ZeroTrustLogin,
                StatusCode::FORBIDDEN,
                secret_body,
            ),
            RegistrationError::ZeroTrustLoginDenied
        ));
        assert!(matches!(
            api_error_for_context(
                ApiErrorContext::ZeroTrustEnrollment,
                StatusCode::UNAUTHORIZED,
                secret_body,
            ),
            RegistrationError::ZeroTrustLoginExpired
        ));
        let error = api_error_for_context(
            ApiErrorContext::ZeroTrustEnrollment,
            StatusCode::BAD_GATEWAY,
            secret_body,
        );
        match &error {
            RegistrationError::ZeroTrustRegistrationFailed { stage, status } => {
                assert_eq!(*stage, ZeroTrustRegistrationStage::MasqueEnrollment);
                assert_eq!(*status, StatusCode::BAD_GATEWAY);
            }
            other => panic!("unexpected Zero Trust error: {other:?}"),
        }
        assert!(!format!("{error:?}").contains("assertion-for-test"));
    }

    #[test]
    fn request_matches_frozen_android_oracle_contract() {
        let options = RegistrationOptions {
            terms_accepted: true,
            ..RegistrationOptions::default()
        };
        let request = RegistrationRequest::new(&options);
        let json = serde_json::to_value(request).unwrap();
        assert_eq!(json["key_type"], "curve25519");
        assert_eq!(json["tunnel_type"], "wireguard");
        assert_eq!(json["model"], "PC");
        assert_eq!(json["locale"], "en_US");
        assert_eq!(
            BASE64_STANDARD
                .decode(json["key"].as_str().unwrap())
                .unwrap()
                .len(),
            32
        );
        assert_eq!(json["serial_number"].as_str().unwrap().len(), 16);
    }

    #[test]
    fn maps_authenticated_enrollment_to_secret_identity() {
        let key_pair = MasqueKeyPair::generate();
        let identity = identity_from_enrollment(
            key_pair,
            "token".to_owned(),
            IdentityProvider::Consumer,
            enrollment(&MasqueKeyPair::generate()),
            false,
        )
        .unwrap();
        assert_eq!(identity.device_id(), "device-123");
        assert_eq!(identity.access_token(), "token");
        assert_eq!(identity.assigned_ipv4.to_string(), "172.16.0.2");
        assert!(identity.license().is_none());
        assert_eq!(identity.entitlement(), Some(ConsumerEntitlement::Free));
    }

    #[test]
    fn consumer_entitlement_ignores_the_api_warp_plus_boolean() {
        let free = Account {
            warp_plus: true,
            ..Account::default()
        };
        assert_eq!(consumer_entitlement(&free), ConsumerEntitlement::Free);

        let plus_data = Account {
            warp_plus: true,
            premium_data: 1_000_000_000,
            ..Account::default()
        };
        assert_eq!(
            consumer_entitlement(&plus_data),
            ConsumerEntitlement::WarpPlus
        );

        let plus_quota = Account {
            quota: 1,
            ..Account::default()
        };
        assert_eq!(
            consumer_entitlement(&plus_quota),
            ConsumerEntitlement::WarpPlus
        );

        for account_type in ["unlimited", "plus", "premium", " Unlimited "] {
            let unlimited = Account {
                account_type: account_type.to_owned(),
                ..Account::default()
            };
            assert_eq!(
                consumer_entitlement(&unlimited),
                ConsumerEntitlement::WarpPlus,
                "{account_type}"
            );
        }
    }

    #[test]
    fn free_enrollment_does_not_keep_the_api_sharing_license() {
        let mut enrolled = enrollment(&MasqueKeyPair::generate());
        enrolled.account.license = Some("api-sharing-key".to_owned());
        let identity = identity_from_enrollment(
            MasqueKeyPair::generate(),
            "token".to_owned(),
            IdentityProvider::Consumer,
            enrolled,
            false,
        )
        .unwrap();
        assert!(identity.license().is_none());
        assert_eq!(identity.entitlement(), Some(ConsumerEntitlement::Free));
    }

    #[test]
    fn licensed_enrollment_keeps_the_bound_key_when_account_is_plus() {
        let mut enrolled = enrollment(&MasqueKeyPair::generate());
        enrolled.account.premium_data = 1_000_000_000;
        enrolled.account.license = Some("bound-license".to_owned());
        let identity = identity_from_enrollment(
            MasqueKeyPair::generate(),
            "token".to_owned(),
            IdentityProvider::Consumer,
            enrolled,
            true,
        )
        .unwrap();
        assert_eq!(identity.license(), Some("bound-license"));
        assert_eq!(identity.entitlement(), Some(ConsumerEntitlement::WarpPlus));
    }

    #[test]
    fn binding_a_free_sharing_license_is_not_warp_plus() {
        let mut enrolled = enrollment(&MasqueKeyPair::generate());
        enrolled.account.warp_plus = true;
        enrolled.account.license = Some("free-sharing-key".to_owned());
        let identity = identity_from_enrollment(
            MasqueKeyPair::generate(),
            "token".to_owned(),
            IdentityProvider::Consumer,
            enrolled,
            true,
        )
        .unwrap();
        assert_eq!(identity.license(), Some("free-sharing-key"));
        assert_eq!(identity.entitlement(), Some(ConsumerEntitlement::Free));
    }

    #[test]
    fn opaque_registration_id_cannot_escape_the_api_path() {
        let root = Url::parse("http://127.0.0.1:12345/base/").unwrap();
        let client = ConsumerRegistrationClient::with_api_root(root).unwrap();
        let url = client.registration_url(Some("../account:slot=1")).unwrap();
        let segments = url.path_segments().unwrap().collect::<Vec<_>>();

        assert_eq!(segments.len(), 4);
        assert_eq!(&segments[..3], &["base", API_VERSION, "reg"]);
        assert_eq!(segments[3], "..%2Faccount:slot=1");
        assert_eq!(
            registration_path("../account:slot=1").unwrap(),
            url.path()[5..]
        );
        assert!(matches!(
            validate_device_id(".."),
            Err(RegistrationError::InvalidDeviceId)
        ));
        assert!(matches!(
            validate_device_id("registration id"),
            Err(RegistrationError::InvalidDeviceId)
        ));
    }

    #[test]
    fn test_client_keeps_injected_api_root() {
        let root = Url::parse("http://127.0.0.1:12345/base/").unwrap();
        let client = ConsumerRegistrationClient::with_api_root(root).unwrap();
        assert_eq!(
            client.registration_url(Some("device-1")).unwrap().as_str(),
            "http://127.0.0.1:12345/base/v0a4471/reg/device-1"
        );
        assert_eq!(
            client.account_url("device-1").unwrap().as_str(),
            "http://127.0.0.1:12345/base/v0a4471/reg/device-1/account"
        );
    }

    #[tokio::test]
    async fn account_status_treats_zero_quota_as_free_even_when_api_warp_plus_is_true() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let socket = listener.local_addr().unwrap();
        let body = serde_json::to_vec(&Account {
            account_type: "free".to_owned(),
            warp_plus: true,
            premium_data: 0,
            quota: 0,
            license: Some("sharing-key".to_owned()),
        })
        .unwrap();
        let server =
            tokio::spawn(async move { serve_registration_response(&listener, &body).await });
        let identity = identity_from_enrollment(
            MasqueKeyPair::generate(),
            "device-bearer".to_owned(),
            IdentityProvider::Consumer,
            enrollment(&MasqueKeyPair::generate()),
            false,
        )
        .unwrap();
        let client = ConsumerRegistrationClient::with_api_root(
            Url::parse(&format!("http://{socket}/")).unwrap(),
        )
        .unwrap();
        let status = client.account_status(&identity).await.unwrap();
        assert_eq!(status.entitlement, ConsumerEntitlement::Free);
        assert_eq!(status.account_type, "free");
        let request = server.await.unwrap();
        assert!(
            request
                .to_ascii_lowercase()
                .starts_with("get /v0a4471/reg/device-123/account ")
        );
        assert!(
            request
                .to_ascii_lowercase()
                .contains("authorization: bearer device-bearer")
        );
    }

    #[test]
    fn license_key_validation_is_strict_and_never_logs_the_value() {
        assert!(validate_license_key("12345678-abcdefgh-ABCDEFGH").is_ok());
        assert!(matches!(
            validate_license_key("12345678-abcdefgh-too-long"),
            Err(RegistrationError::InvalidLicenseKey)
        ));
        assert!(!format!("{:?}", RegistrationError::InvalidLicenseKey).contains("12345678"));
    }

    #[test]
    fn protected_refresh_request_redacts_the_bearer_and_matches_the_wire_contract() {
        let identity = identity_from_enrollment(
            MasqueKeyPair::generate(),
            "super-secret-token".to_owned(),
            IdentityProvider::Consumer,
            enrollment(&MasqueKeyPair::generate()),
            false,
        )
        .unwrap();
        let request = prepare_endpoint_pin_refresh(&identity, Some("Usque")).unwrap();
        assert_eq!(request.path_and_query(), "/v0a4471/reg/device-123");
        assert_eq!(request.bearer_token(), "super-secret-token");
        let body: serde_json::Value = serde_json::from_slice(request.body()).unwrap();
        assert_eq!(body["key_type"], "secp256r1");
        assert_eq!(body["tunnel_type"], "masque");
        assert_eq!(body["name"], "Usque");
        assert!(!format!("{request:?}").contains("super-secret-token"));
    }

    #[test]
    fn protected_refresh_response_rejects_errors_and_parses_only_success() {
        let key = MasqueKeyPair::generate();
        let response = serde_json::to_vec(&enrollment(&key)).unwrap();
        let refresh = parse_endpoint_pin_refresh_response(200, &response).unwrap();
        assert_eq!(
            refresh.assigned_ipv4,
            "172.16.0.2".parse::<Ipv4Addr>().unwrap()
        );
        assert!(matches!(
            parse_endpoint_pin_refresh_response(
                401,
                br#"{"errors":[{"message":"invalid token"}]}"#
            ),
            Err(RegistrationError::Api { status, .. }) if status == StatusCode::UNAUTHORIZED
        ));
    }
}
