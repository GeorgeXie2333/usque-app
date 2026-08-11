use std::fmt;
use std::net::{Ipv4Addr, Ipv6Addr};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use p256::PublicKey;
use p256::SecretKey;
use p256::elliptic_curve::Generate;
use p256::pkcs8::{DecodePublicKey, EncodePublicKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use thiserror::Error;
use zeroize::{Zeroize, Zeroizing};

const MAX_MANUAL_SECRET_BYTES: usize = 128 * 1024;

/// A validated P-256 SubjectPublicKeyInfo pin returned by the WARP enrollment API.
///
/// The Go oracle compares the endpoint certificate's ECDSA public key with this
/// value. Usque normalizes both values to DER SPKI and compares SHA-256 digests
/// in constant time. This intentionally does not perform hostname verification:
/// the user-selected SNI can differ from the endpoint certificate name.
#[derive(Clone, PartialEq, Eq)]
pub struct EndpointPin {
    spki_der: Vec<u8>,
    sha256: [u8; 32],
}

impl EndpointPin {
    pub fn from_pem(pem: &str) -> Result<Self, IdentityError> {
        let public_key =
            PublicKey::from_public_key_pem(pem).map_err(|_| IdentityError::InvalidEndpointPin)?;
        Self::from_public_key(public_key)
    }

    pub fn from_spki_der(spki_der: &[u8]) -> Result<Self, IdentityError> {
        let public_key = PublicKey::from_public_key_der(spki_der)
            .map_err(|_| IdentityError::InvalidEndpointPin)?;
        Self::from_public_key(public_key)
    }

    fn from_public_key(public_key: PublicKey) -> Result<Self, IdentityError> {
        let spki_der = public_key
            .to_public_key_der()
            .map_err(|_| IdentityError::InvalidEndpointPin)?
            .as_bytes()
            .to_vec();
        let sha256 = Sha256::digest(&spki_der).into();
        Ok(Self { spki_der, sha256 })
    }

    /// Verifies the SubjectPublicKeyInfo bytes extracted from the peer's leaf
    /// certificate. A production TLS adapter must call this before allowing any
    /// CONNECT-IP traffic.
    pub fn verify_peer_spki(&self, peer_spki_der: &[u8]) -> Result<(), IdentityError> {
        let peer = Self::from_spki_der(peer_spki_der)?;
        if bool::from(self.sha256.ct_eq(&peer.sha256)) {
            Ok(())
        } else {
            Err(IdentityError::EndpointPinMismatch)
        }
    }

    pub fn spki_der(&self) -> &[u8] {
        &self.spki_der
    }

    pub fn sha256_hex(&self) -> String {
        self.sha256
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }
}

impl fmt::Debug for EndpointPin {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EndpointPin")
            .field("sha256", &self.sha256_hex())
            .finish()
    }
}

/// A MASQUE P-256 keypair. The private scalar is zeroized by `p256` on drop.
pub struct MasqueKeyPair {
    secret_key: SecretKey,
}

impl MasqueKeyPair {
    pub fn generate() -> Self {
        Self {
            secret_key: SecretKey::generate(),
        }
    }

    pub fn from_base64_sec1(value: &str) -> Result<Self, IdentityError> {
        let decoded = Zeroizing::new(
            BASE64_STANDARD
                .decode(value.trim())
                .map_err(|_| IdentityError::InvalidPrivateKeyEncoding)?,
        );
        Self::from_sec1_der(&decoded)
    }

    /// Reconstructs a keypair from a platform vault record.
    ///
    /// Callers must keep the supplied DER buffer in secure, zeroizing storage.
    pub fn from_sec1_der(value: &[u8]) -> Result<Self, IdentityError> {
        let secret_key =
            SecretKey::from_sec1_der(value).map_err(|_| IdentityError::InvalidPrivateKey)?;
        Ok(Self { secret_key })
    }

    /// Returns a temporary SEC1 DER copy for transfer into a platform vault.
    /// The returned buffer zeroizes itself on drop.
    pub fn private_sec1_der(&self) -> Result<Zeroizing<Vec<u8>>, IdentityError> {
        self.secret_key
            .to_sec1_der()
            .map_err(|_| IdentityError::InvalidPrivateKey)
    }

    pub fn public_spki_der(&self) -> Result<Vec<u8>, IdentityError> {
        self.secret_key
            .public_key()
            .to_public_key_der()
            .map(|document| document.as_bytes().to_vec())
            .map_err(|_| IdentityError::InvalidPrivateKey)
    }
}

impl fmt::Debug for MasqueKeyPair {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("MasqueKeyPair([REDACTED])")
    }
}

/// Secret identity material. It must be persisted as separate records in the
/// platform vault, never serialized with the non-secret profile JSON.
pub struct WarpIdentity {
    pub key_pair: MasqueKeyPair,
    pub endpoint_pin: EndpointPin,
    device_id: Zeroizing<String>,
    access_token: Zeroizing<String>,
    license: Option<Zeroizing<String>>,
    pub assigned_ipv4: Ipv4Addr,
    pub assigned_ipv6: Ipv6Addr,
}

impl WarpIdentity {
    /// Reconstructs a validated identity from secure platform-vault records.
    ///
    /// This constructor intentionally accepts owned credential strings so the
    /// returned identity can zeroize them. It must never be used for values
    /// loaded from the non-secret profile JSON.
    #[expect(
        clippy::too_many_arguments,
        reason = "vault reconstruction must accept every zeroizing credential and address field as one atomic constructor"
    )]
    pub fn from_secure_records(
        key_pair: MasqueKeyPair,
        endpoint_pin: EndpointPin,
        device_id: String,
        access_token: String,
        license: Option<String>,
        assigned_ipv4: Ipv4Addr,
        assigned_ipv6: Ipv6Addr,
    ) -> Result<Self, IdentityError> {
        if device_id.trim().is_empty() || access_token.trim().is_empty() {
            return Err(IdentityError::MissingCredential);
        }
        Ok(Self {
            key_pair,
            endpoint_pin,
            device_id: Zeroizing::new(device_id),
            access_token: Zeroizing::new(access_token),
            license: license.map(Zeroizing::new),
            assigned_ipv4,
            assigned_ipv6,
        })
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "identity factory mirrors from_secure_records field set for registration and vault paths"
    )]
    pub(crate) fn new(
        key_pair: MasqueKeyPair,
        endpoint_pin: EndpointPin,
        device_id: String,
        access_token: String,
        license: Option<String>,
        assigned_ipv4: Ipv4Addr,
        assigned_ipv6: Ipv6Addr,
    ) -> Result<Self, IdentityError> {
        Self::from_secure_records(
            key_pair,
            endpoint_pin,
            device_id,
            access_token,
            license,
            assigned_ipv4,
            assigned_ipv6,
        )
    }

    pub fn device_id(&self) -> &str {
        &self.device_id
    }

    pub fn access_token(&self) -> &str {
        &self.access_token
    }

    pub fn license(&self) -> Option<&str> {
        self.license.as_deref().map(String::as_str)
    }

    /// Serializes an oracle-compatible identity for immediate transfer into a
    /// platform vault. Every owned buffer returned or created here is
    /// zeroized; callers must never persist this value outside secure storage.
    pub fn to_portable_secret_json(&self) -> Result<Zeroizing<String>, IdentityError> {
        let private_der = self.key_pair.private_sec1_der()?;
        let private_key = Zeroizing::new(BASE64_STANDARD.encode(&private_der));
        let endpoint_public = PublicKey::from_public_key_der(self.endpoint_pin.spki_der())
            .map_err(|_| IdentityError::InvalidEndpointPin)?;
        let endpoint_pub_key = Zeroizing::new(
            endpoint_public
                .to_public_key_pem(p256::pkcs8::LineEnding::LF)
                .map_err(|_| IdentityError::InvalidEndpointPin)?,
        );
        let envelope = PortableIdentityEnvelope {
            private_key: &private_key,
            endpoint_pub_key: &endpoint_pub_key,
            id: self.device_id(),
            access_token: self.access_token(),
            license: self.license(),
            ipv4: self.assigned_ipv4.to_string(),
            ipv6: self.assigned_ipv6.to_string(),
        };
        serde_json::to_string(&envelope)
            .map(Zeroizing::new)
            .map_err(|_| IdentityError::IdentitySerialization)
    }
}

#[derive(Serialize)]
struct PortableIdentityEnvelope<'a> {
    private_key: &'a str,
    endpoint_pub_key: &'a str,
    id: &'a str,
    access_token: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    license: Option<&'a str>,
    ipv4: String,
    ipv6: String,
}

impl fmt::Debug for WarpIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WarpIdentity")
            .field("key_pair", &"[REDACTED]")
            .field("endpoint_pin", &self.endpoint_pin)
            .field("device_id", &"[REDACTED]")
            .field("access_token", &"[REDACTED]")
            .field("license", &self.license.as_ref().map(|_| "[REDACTED]"))
            .field("assigned_ipv4", &self.assigned_ipv4)
            .field("assigned_ipv6", &self.assigned_ipv6)
            .finish()
    }
}

/// Parses an explicitly pasted oracle-compatible identity JSON.
///
/// For ergonomic transfer from another device, standard Base64-wrapped JSON is
/// accepted as well. Usque never scans for or imports legacy `config.json`
/// files. Unknown fields are ignored so H2 endpoint additions from newer oracle
/// versions do not make an otherwise valid identity unusable.
pub fn parse_manual_warp_secret(input: &str) -> Result<WarpIdentity, IdentityError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(IdentityError::EmptyManualSecret);
    }
    if trimmed.len() > MAX_MANUAL_SECRET_BYTES {
        return Err(IdentityError::ManualSecretTooLarge);
    }

    let mut decoded_json = if trimmed.starts_with('{') {
        Zeroizing::new(trimmed.as_bytes().to_vec())
    } else {
        Zeroizing::new(
            BASE64_STANDARD
                .decode(trimmed)
                .map_err(|_| IdentityError::InvalidManualSecret)?,
        )
    };
    if decoded_json.len() > MAX_MANUAL_SECRET_BYTES {
        return Err(IdentityError::ManualSecretTooLarge);
    }

    let envelope: OracleIdentityEnvelope =
        serde_json::from_slice(&decoded_json).map_err(|_| IdentityError::InvalidManualSecret)?;
    decoded_json.zeroize();

    envelope.try_into()
}

#[derive(Deserialize)]
struct OracleIdentityEnvelope {
    private_key: String,
    endpoint_pub_key: String,
    id: String,
    access_token: String,
    #[serde(default)]
    license: Option<String>,
    ipv4: String,
    ipv6: String,
}

impl TryFrom<OracleIdentityEnvelope> for WarpIdentity {
    type Error = IdentityError;

    fn try_from(mut value: OracleIdentityEnvelope) -> Result<Self, Self::Error> {
        if value.id.trim().is_empty() || value.access_token.trim().is_empty() {
            value.zeroize_secrets();
            return Err(IdentityError::MissingCredential);
        }

        let key_pair = match MasqueKeyPair::from_base64_sec1(&value.private_key) {
            Ok(key_pair) => key_pair,
            Err(error) => {
                value.zeroize_secrets();
                return Err(error);
            }
        };
        let endpoint_pin = match EndpointPin::from_pem(&value.endpoint_pub_key) {
            Ok(pin) => pin,
            Err(error) => {
                value.zeroize_secrets();
                return Err(error);
            }
        };
        let assigned_ipv4 = match value.ipv4.parse() {
            Ok(address) => address,
            Err(_) => {
                value.zeroize_secrets();
                return Err(IdentityError::InvalidAssignedAddress);
            }
        };
        let assigned_ipv6 = match value.ipv6.parse() {
            Ok(address) => address,
            Err(_) => {
                value.zeroize_secrets();
                return Err(IdentityError::InvalidAssignedAddress);
            }
        };

        let identity = Self::new(
            key_pair,
            endpoint_pin,
            std::mem::take(&mut value.id),
            std::mem::take(&mut value.access_token),
            value.license.take(),
            assigned_ipv4,
            assigned_ipv6,
        )?;
        value.zeroize_secrets();
        Ok(identity)
    }
}

impl OracleIdentityEnvelope {
    fn zeroize_secrets(&mut self) {
        self.private_key.zeroize();
        self.endpoint_pub_key.zeroize();
        self.id.zeroize();
        self.access_token.zeroize();
        if let Some(license) = &mut self.license {
            license.zeroize();
        }
        self.ipv4.zeroize();
        self.ipv6.zeroize();
    }
}

impl Drop for OracleIdentityEnvelope {
    fn drop(&mut self) {
        self.zeroize_secrets();
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum IdentityError {
    #[error("the WARP Secret is empty")]
    EmptyManualSecret,
    #[error("the WARP Secret exceeds the 128 KiB safety limit")]
    ManualSecretTooLarge,
    #[error("the WARP Secret is neither valid JSON nor Base64-wrapped JSON")]
    InvalidManualSecret,
    #[error("the MASQUE private key is not valid Base64")]
    InvalidPrivateKeyEncoding,
    #[error("the MASQUE private key is not a P-256 SEC1 key")]
    InvalidPrivateKey,
    #[error("the endpoint pin is not a PEM-encoded P-256 public key")]
    InvalidEndpointPin,
    #[error("the peer endpoint public key does not match the enrolled pin")]
    EndpointPinMismatch,
    #[error("the identity is missing its device ID or access token")]
    MissingCredential,
    #[error("the identity contains an invalid assigned IPv4 or IPv6 address")]
    InvalidAssignedAddress,
    #[error("the identity could not be serialized for secure platform transfer")]
    IdentitySerialization,
}

#[cfg(test)]
mod tests {
    use super::*;
    use p256::pkcs8::LineEnding;
    use serde_json::json;

    fn sample_secret() -> (String, MasqueKeyPair, EndpointPin) {
        let identity_key = MasqueKeyPair::generate();
        let endpoint_key = MasqueKeyPair::generate();
        let endpoint_public = PublicKey::from_public_key_der(
            &endpoint_key.public_spki_der().expect("endpoint public key"),
        )
        .expect("parse endpoint public key");
        let endpoint_pem = endpoint_public
            .to_public_key_pem(LineEnding::LF)
            .expect("endpoint PEM");
        let private_der = identity_key.private_sec1_der().expect("private key");
        let endpoint_pin = EndpointPin::from_pem(&endpoint_pem).expect("endpoint pin");
        let json = json!({
            "private_key": BASE64_STANDARD.encode(private_der.as_slice()),
            "endpoint_pub_key": endpoint_pem,
            "id": "device-id",
            "access_token": "access-token",
            "license": "license",
            "ipv4": "172.16.0.2",
            "ipv6": "2606:4700:110:8f13::2",
            "endpoint_h2_v4": "162.159.198.2"
        })
        .to_string();
        (json, identity_key, endpoint_pin)
    }

    #[test]
    fn parses_oracle_json_without_exposing_secrets_in_debug() {
        let (json, identity_key, endpoint_pin) = sample_secret();
        let parsed = parse_manual_warp_secret(&json).expect("valid identity");

        assert_eq!(
            parsed.key_pair.public_spki_der().unwrap(),
            identity_key.public_spki_der().unwrap()
        );
        assert_eq!(parsed.endpoint_pin, endpoint_pin);
        assert_eq!(parsed.assigned_ipv4.to_string(), "172.16.0.2");
        assert!(!format!("{parsed:?}").contains("access-token"));
        assert!(!format!("{parsed:?}").contains("device-id"));
    }

    #[test]
    fn accepts_base64_wrapped_json() {
        let (json, _, _) = sample_secret();
        let wrapped = BASE64_STANDARD.encode(json);
        assert!(parse_manual_warp_secret(&wrapped).is_ok());
    }

    #[test]
    fn portable_secret_round_trips_without_debug_exposure() {
        let (json, _, _) = sample_secret();
        let identity = parse_manual_warp_secret(&json).expect("parse");
        let portable = identity.to_portable_secret_json().expect("serialize");
        let reparsed = parse_manual_warp_secret(&portable).expect("reparse");
        assert_eq!(reparsed.device_id(), identity.device_id());
        assert_eq!(reparsed.access_token(), identity.access_token());
        assert_eq!(reparsed.license(), identity.license());
        assert_eq!(reparsed.assigned_ipv4, identity.assigned_ipv4);
        assert_eq!(reparsed.assigned_ipv6, identity.assigned_ipv6);
        assert_eq!(
            reparsed.key_pair.public_spki_der().expect("reparsed key"),
            identity.key_pair.public_spki_der().expect("identity key")
        );
        assert_eq!(reparsed.endpoint_pin, identity.endpoint_pin);
    }

    #[test]
    fn endpoint_pin_match_is_strict() {
        let (_, _, endpoint_pin) = sample_secret();
        let other_key = MasqueKeyPair::generate();

        assert!(
            endpoint_pin
                .verify_peer_spki(endpoint_pin.spki_der())
                .is_ok()
        );
        assert_eq!(
            endpoint_pin
                .verify_peer_spki(&other_key.public_spki_der().unwrap())
                .unwrap_err(),
            IdentityError::EndpointPinMismatch
        );
    }

    #[test]
    fn rejects_non_p256_or_malformed_material() {
        assert_eq!(
            parse_manual_warp_secret("{}").unwrap_err(),
            IdentityError::InvalidManualSecret
        );
        assert_eq!(
            EndpointPin::from_pem("not a key").unwrap_err(),
            IdentityError::InvalidEndpointPin
        );
    }
}
