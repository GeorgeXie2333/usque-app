use std::net::{Ipv4Addr, Ipv6Addr};
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use chrono::{SecondsFormat, Utc};
use p256::elliptic_curve::rand_core::{OsRng, RngCore};
use reqwest::{Client, Method, StatusCode, Url};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use thiserror::Error;
use zeroize::Zeroizing;

use crate::identity::{EndpointPin, IdentityError, MasqueKeyPair, WarpIdentity};

const API_ROOT: &str = "https://api.cloudflareclient.com/";
const API_VERSION: &str = "v0a4471";
const CF_CLIENT_VERSION: &str = "a-6.35-4471";
const MAX_API_RESPONSE_BYTES: usize = 1024 * 1024;
pub const REGISTRATION_API_HOST: &str = "api.cloudflareclient.com";
pub const REGISTRATION_API_PORT: u16 = 443;

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
    validate_device_id(identity.device_id())?;
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
        path_and_query: format!("/{API_VERSION}/reg/{}", identity.device_id()),
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
    /// P-256 MASQUE key. No Zero Trust assertion is accepted by this API.
    pub async fn register(
        &self,
        options: &RegistrationOptions,
    ) -> Result<WarpIdentity, RegistrationError> {
        validate_options(options)?;

        let request = RegistrationRequest::new(options);
        let registration_url = self.registration_url(None)?;
        let registered: AccountData = self
            .send_json(Method::POST, registration_url, None, &request)
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
            )
            .await?;
        identity_from_enrollment(key_pair, registered.token, enrolled)
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
            Some(access_token),
            &body,
        )
        .await
    }

    async fn send_json<Request, Response>(
        &self,
        method: Method,
        url: Url,
        bearer_token: Option<&str>,
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
        if let Some(token) = bearer_token {
            request = request.bearer_auth(token);
        }

        let response = request.send().await?;
        let status = response.status();
        if response
            .content_length()
            .is_some_and(|length| length > MAX_API_RESPONSE_BYTES as u64)
        {
            return Err(RegistrationError::ApiResponseTooLarge);
        }
        let bytes = response.bytes().await?;
        if bytes.len() > MAX_API_RESPONSE_BYTES {
            return Err(RegistrationError::ApiResponseTooLarge);
        }
        if status != StatusCode::OK {
            return Err(api_error(status, &bytes));
        }
        serde_json::from_slice(&bytes).map_err(|_| RegistrationError::InvalidApiResponse)
    }

    fn registration_url(&self, device_id: Option<&str>) -> Result<Url, RegistrationError> {
        let mut url = self.api_root.clone();
        let mut segments = url
            .path_segments_mut()
            .map_err(|_| RegistrationError::InvalidApiUrl)?;
        segments.pop_if_empty().push(API_VERSION).push("reg");
        if let Some(device_id) = device_id {
            segments.push(device_id);
        }
        drop(segments);
        Ok(url)
    }
}

#[derive(Debug, Serialize)]
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
        let mut wireguard_placeholder = [0_u8; 32];
        let mut serial = [0_u8; 8];
        OsRng.fill_bytes(&mut wireguard_placeholder);
        OsRng.fill_bytes(&mut serial);
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

#[derive(Debug, Serialize)]
struct EnrollmentRequest<'a> {
    key: String,
    key_type: &'static str,
    #[serde(rename = "tunnel_type")]
    tunnel_type: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<&'a str>,
}

#[derive(Debug, Serialize, Deserialize)]
struct AccountData {
    id: String,
    #[serde(default)]
    token: String,
    #[serde(default)]
    account: Account,
    config: AccountConfig,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Account {
    #[serde(default)]
    license: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct AccountConfig {
    peers: Vec<Peer>,
    interface: Interface,
}

#[derive(Debug, Serialize, Deserialize)]
struct Peer {
    public_key: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct Interface {
    addresses: AssignedAddresses,
}

#[derive(Debug, Serialize, Deserialize)]
struct AssignedAddresses {
    v4: String,
    v6: String,
}

#[derive(Debug, Deserialize)]
struct ApiErrorEnvelope {
    #[serde(default)]
    errors: Vec<ApiErrorItem>,
}

#[derive(Debug, Deserialize)]
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

fn validate_device_id(device_id: &str) -> Result<(), RegistrationError> {
    if device_id.is_empty()
        || device_id.len() > 128
        || !device_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(RegistrationError::InvalidDeviceId);
    }
    Ok(())
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

fn identity_from_enrollment(
    key_pair: MasqueKeyPair,
    access_token: String,
    mut enrollment: AccountData,
) -> Result<WarpIdentity, RegistrationError> {
    validate_device_id(&enrollment.id)?;
    let snapshot = enrollment_snapshot(&enrollment)?;
    WarpIdentity::new(
        key_pair,
        snapshot.endpoint_pin,
        enrollment.id,
        access_token,
        enrollment.account.license.take(),
        snapshot.assigned_ipv4,
        snapshot.assigned_ipv6,
    )
    .map_err(Into::into)
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
    #[error("Cloudflare terms must be accepted before Consumer WARP registration")]
    TermsNotAccepted,
    #[error("registration model, locale, or device name is invalid")]
    InvalidRegistrationOptions,
    #[error("the registration API URL is invalid")]
    InvalidApiUrl,
    #[error("the registration API returned an invalid device identifier")]
    InvalidDeviceId,
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

    fn enrollment(key: &MasqueKeyPair) -> AccountData {
        let public = PublicKey::from_public_key_der(&key.public_spki_der().unwrap()).unwrap();
        AccountData {
            id: "device-123".to_owned(),
            token: String::new(),
            account: Account {
                license: Some("license".to_owned()),
            },
            config: AccountConfig {
                peers: vec![Peer {
                    public_key: public.to_public_key_pem(LineEnding::LF).unwrap(),
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

    #[test]
    fn registration_requires_terms_acceptance() {
        assert!(matches!(
            validate_options(&RegistrationOptions::default()),
            Err(RegistrationError::TermsNotAccepted)
        ));
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
            enrollment(&MasqueKeyPair::generate()),
        )
        .unwrap();
        assert_eq!(identity.device_id(), "device-123");
        assert_eq!(identity.access_token(), "token");
        assert_eq!(identity.assigned_ipv4.to_string(), "172.16.0.2");
    }

    #[test]
    fn device_id_cannot_escape_the_api_path() {
        assert!(matches!(
            validate_device_id("../account"),
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
    }

    #[test]
    fn protected_refresh_request_redacts_the_bearer_and_matches_the_wire_contract() {
        let identity = identity_from_enrollment(
            MasqueKeyPair::generate(),
            "super-secret-token".to_owned(),
            enrollment(&MasqueKeyPair::generate()),
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
