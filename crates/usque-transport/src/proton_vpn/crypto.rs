//! Native Proton v4 cryptography. Public parameters may use variable-time
//! validation; every secret SRP exponent uses fixed-width RustCrypto pow.
//!
//! Protocol/algorithm reference: ProtonMail/go-srp, commit
//! d323688777afbb6265d8bc4a51419991a24630ca, srp.go/hash.go (MIT).
//!
//! Copyright (c) 2019 Proton Technologies AG
//!
//! Permission is hereby granted, free of charge, to any person obtaining a copy
//! of this software and associated documentation files (the "Software"), to deal
//! in the Software without restriction, including without limitation the rights
//! to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
//! copies of the Software, and to permit persons to whom the Software is
//! furnished to do so, subject to the following conditions:
//!
//! The above copyright notice and this permission notice shall be included in
//! all copies or substantial portions of the Software.
//!
//! THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
//! IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
//! FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
//! AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
//! LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
//! OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
//! THE SOFTWARE.

use base64::{Engine, engine::general_purpose::STANDARD};
#[cfg(test)]
use boring::sign::Signer;
use boring::{
    asn1::Asn1Time,
    bn::{BigNum, BigNumContext},
    pkey::{Id, PKey, Private},
    rand::rand_bytes,
    sign::Verifier,
    x509::X509,
};
use boringtun::x25519::{PublicKey, StaticSecret};
use crypto_bigint::{
    NonZero, Odd, U2048, U4096,
    modular::{FixedMontyForm, FixedMontyParams},
};
use curve25519_dalek::edwards::CompressedEdwardsY;
use sha2::{Digest, Sha256, Sha512};
use std::cmp::Ordering;
use std::fmt;
use std::sync::{Mutex, OnceLock};
use subtle::{ConstantTimeEq, ConstantTimeGreater, ConstantTimeLess};
use zeroize::{Zeroize, Zeroizing};

#[cfg(test)]
#[path = "crypto_tests.rs"]
mod tests;

const GROUP_BYTES: usize = 256;
const MODULUS_KEY: [u8; 32] = [
    0x16, 0xea, 0xd6, 0x5d, 0x7c, 0x23, 0x4d, 0xe9, 0xaa, 0x8c, 0x3e, 0xc2, 0x5e, 0x35, 0xd5, 0xc8,
    0xa7, 0xf4, 0xa1, 0xfe, 0xe7, 0xf4, 0x2b, 0x66, 0x2f, 0xcb, 0xfd, 0x7a, 0x79, 0x33, 0x82, 0x01,
];
const SPKI_PREFIX: &[u8] = &[
    0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00,
];
const PKCS8_PREFIX: &[u8] = &[
    0x30, 0x2e, 0x02, 0x01, 0x00, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x04, 0x22, 0x04, 0x20,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(super) enum CryptoError {
    #[error("unsupported_srp_version")]
    UnsupportedVersion,
    #[error("unsupported_srp_password")]
    UnsupportedPassword,
    #[error("invalid_srp_parameters")]
    Parameters,
    #[error("invalid_srp_modulus_signature")]
    ModulusSignature,
    #[error("invalid_srp_server_proof")]
    ServerProof,
    #[error("crypto_unavailable")]
    Unavailable,
    #[error("invalid_proton_key")]
    Key,
    #[error("invalid_proton_certificate")]
    Certificate,
    #[error("proton_certificate_key_mismatch")]
    CertificateKey,
    #[error("proton_certificate_time_mismatch")]
    CertificateTime,
    #[error("proton_certificate_expired_or_not_yet_valid")]
    CertificateExpired,
    #[error("proton_certificate_refresh_due")]
    CertificateRefreshDue,
}

pub(super) struct SrpProofs {
    client_ephemeral: Zeroizing<String>,
    client_proof: Zeroizing<String>,
    expected: Zeroizing<[u8; GROUP_BYTES]>,
}
impl fmt::Debug for SrpProofs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SrpProofs([redacted])")
    }
}
impl SrpProofs {
    pub(super) fn generate(
        version: u32,
        password: &str,
        salt_b64: &str,
        signed_modulus: &str,
        server_ephemeral_b64: &str,
    ) -> Result<Self, CryptoError> {
        Self::generate_inner(
            version,
            password,
            salt_b64,
            signed_modulus,
            server_ephemeral_b64,
            None,
        )
    }

    fn generate_inner(
        version: u32,
        password: &str,
        salt_b64: &str,
        signed_modulus: &str,
        server_ephemeral_b64: &str,
        secret: Option<Zeroizing<U2048>>,
    ) -> Result<Self, CryptoError> {
        if version != 4 {
            return Err(CryptoError::UnsupportedVersion);
        }
        if password.is_empty() || password.len() > 72 {
            return Err(CryptoError::UnsupportedPassword);
        }
        let modulus = signed_modulus_bytes(signed_modulus)?;
        validate_modulus(&modulus)?;
        let salt = decode::<10>(salt_b64, CryptoError::Parameters)?;
        let server = decode::<GROUP_BYTES>(server_ephemeral_b64, CryptoError::Parameters)?;
        let number = U2048::from_le_slice(&modulus);
        let minus_one = number.wrapping_sub(&U2048::ONE);
        let remote = U2048::from_le_slice(&server);
        if !bool::from(remote.ct_gt(&U2048::ONE) & remote.ct_lt(&minus_one)) {
            return Err(CryptoError::Parameters);
        }
        let hashed = hash_password(password.as_bytes(), &salt, &modulus)?;
        let x = Zeroizing::new(U2048::from_le_slice(&*hashed));
        let params = FixedMontyParams::new_vartime(
            Odd::new(number)
                .into_option()
                .ok_or(CryptoError::Parameters)?,
        );
        let generator = FixedMontyForm::new(&U2048::from_u32(2), &params);
        let secret = match secret {
            Some(value) => value,
            None => random_secret(&minus_one)?,
        };
        if !bool::from(secret.ct_gt(&U2048::from_u32(4096)) & secret.ct_lt(&minus_one)) {
            return Err(CryptoError::Parameters);
        }
        // pow processes all 2048 exponent bits. No *_vartime operation receives
        // a password, random exponent, verifier, shared secret or secret base.
        let client = generator.pow(&secret).retrieve().to_le_bytes();
        let scramble = expand_hash(&[client.as_ref(), &server]);
        let u = Zeroizing::new(U2048::from_le_slice(&*scramble));
        if bool::from(u.ct_eq(&U2048::ZERO)) {
            return Err(CryptoError::Parameters);
        }
        let multiplier_hash = expand_hash(&[U2048::from_u32(2).to_le_bytes().as_ref(), &modulus]);
        let multiplier = U2048::from_le_slice(&*multiplier_hash).rem(
            &NonZero::new(number)
                .into_option()
                .ok_or(CryptoError::Parameters)?,
        );
        if !bool::from(multiplier.ct_gt(&U2048::ONE) & multiplier.ct_lt(&minus_one)) {
            return Err(CryptoError::Parameters);
        }
        let verifier = Zeroizing::new(generator.pow(&x));
        let multiplied = Zeroizing::new(FixedMontyForm::new(&multiplier, &params).mul(&verifier));
        let base = Zeroizing::new(FixedMontyForm::new(&remote, &params).sub(&multiplied));
        let raw_base = Zeroizing::new(base.retrieve());
        if bool::from(
            raw_base.ct_eq(&U2048::ZERO) | raw_base.ct_eq(&U2048::ONE) | raw_base.ct_eq(&minus_one),
        ) {
            return Err(CryptoError::Parameters);
        }
        // a + u*x fits in 4096 bits. Using that full fixed width avoids a
        // secret-dependent division by the even integer N-1. Fermat's theorem
        // makes this identical to reducing the exponent modulo N-1.
        let product: Zeroizing<U4096> = Zeroizing::new(u.concatenating_mul(&x));
        let exponent = Zeroizing::new(product.wrapping_add(&secret.resize()));
        let shared_form = Zeroizing::new(base.pow(&exponent));
        let shared_integer = Zeroizing::new(shared_form.retrieve());
        let shared = integer_bytes(&shared_integer);
        let proof = expand_hash(&[client.as_ref(), &server, shared.as_ref()]);
        let expected = expand_hash(&[client.as_ref(), &*proof, shared.as_ref()]);
        Ok(Self {
            client_ephemeral: Zeroizing::new(STANDARD.encode(client.as_ref())),
            client_proof: Zeroizing::new(STANDARD.encode(proof.as_slice())),
            expected,
        })
    }

    pub(super) fn client_ephemeral(&self) -> &str {
        &self.client_ephemeral
    }
    pub(super) fn client_proof(&self) -> &str {
        &self.client_proof
    }
    pub(super) fn verify_server_proof(&self, value: &str) -> Result<(), CryptoError> {
        let received = Zeroizing::new(decode::<GROUP_BYTES>(value, CryptoError::ServerProof)?);
        if bool::from(received.as_slice().ct_eq(self.expected.as_slice())) {
            Ok(())
        } else {
            Err(CryptoError::ServerProof)
        }
    }
}

fn integer_bytes(value: &U2048) -> Zeroizing<[u8; GROUP_BYTES]> {
    let mut encoded = value.to_le_bytes();
    let mut output = Zeroizing::new([0u8; GROUP_BYTES]);
    output.copy_from_slice(encoded.as_ref());
    encoded.as_mut_slice().zeroize();
    output
}

fn decode<const N: usize>(value: &str, failure: CryptoError) -> Result<[u8; N], CryptoError> {
    if value.len() != N.div_ceil(3) * 4 {
        return Err(failure);
    }
    let bytes = STANDARD.decode(value).map_err(|_| failure)?;
    if STANDARD.encode(&bytes) != value {
        return Err(failure);
    }
    bytes.try_into().map_err(|_| failure)
}

fn expand_hash(parts: &[&[u8]]) -> Zeroizing<[u8; GROUP_BYTES]> {
    let mut output = Zeroizing::new([0u8; GROUP_BYTES]);
    for suffix in 0u8..4 {
        let mut hash = Sha512::new();
        for part in parts {
            hash.update(part);
        }
        hash.update([suffix]);
        output[usize::from(suffix) * 64..usize::from(suffix + 1) * 64]
            .copy_from_slice(&hash.finalize());
    }
    output
}

fn hash_password(
    password: &[u8],
    salt: &[u8; 10],
    modulus: &[u8; GROUP_BYTES],
) -> Result<Zeroizing<[u8; GROUP_BYTES]>, CryptoError> {
    let mut bcrypt_salt = [0u8; 16];
    bcrypt_salt[..10].copy_from_slice(salt);
    bcrypt_salt[10..].copy_from_slice(b"proton");
    let hashed =
        bcrypt::hash_with_salt(password, 10, bcrypt_salt).map_err(|_| CryptoError::Unavailable)?;
    let crypted = Zeroizing::new(hashed.format_for_version(bcrypt::Version::TwoY));
    Ok(expand_hash(&[crypted.as_bytes(), modulus]))
}

fn random_secret(minus_one: &U2048) -> Result<Zeroizing<U2048>, CryptoError> {
    for _ in 0..128 {
        let mut bytes = Zeroizing::new([0u8; GROUP_BYTES]);
        rand_bytes(&mut *bytes).map_err(|_| CryptoError::Unavailable)?;
        let value = Zeroizing::new(U2048::from_le_slice(&*bytes));
        if bool::from(value.ct_gt(&U2048::from_u32(4096)) & value.ct_lt(minus_one)) {
            return Ok(value);
        }
    }
    Err(CryptoError::Unavailable)
}

fn validate_modulus(modulus: &[u8; GROUP_BYTES]) -> Result<(), CryptoError> {
    static VALIDATED: OnceLock<Mutex<Vec<[u8; GROUP_BYTES]>>> = OnceLock::new();
    if modulus[GROUP_BYTES - 1] & 0x80 == 0 || modulus[0] % 8 != 3 {
        return Err(CryptoError::Parameters);
    }
    let cache = VALIDATED.get_or_init(|| Mutex::new(Vec::new()));
    if cache
        .lock()
        .map_err(|_| CryptoError::Unavailable)?
        .contains(modulus)
    {
        return Ok(());
    }
    // Only the signed public modulus enters these variable-time BoringSSL APIs.
    let mut big_endian = *modulus;
    big_endian.reverse();
    let number = BigNum::from_slice(&big_endian).map_err(|_| CryptoError::Unavailable)?;
    let mut minus_one = number.to_owned().map_err(|_| CryptoError::Unavailable)?;
    minus_one
        .sub_word(1)
        .map_err(|_| CryptoError::Unavailable)?;
    let mut half = BigNum::new().map_err(|_| CryptoError::Unavailable)?;
    half.rshift1(&number)
        .map_err(|_| CryptoError::Unavailable)?;
    let mut context = BigNumContext::new().map_err(|_| CryptoError::Unavailable)?;
    if !half
        .is_prime(64, &mut context)
        .map_err(|_| CryptoError::Unavailable)?
    {
        return Err(CryptoError::Parameters);
    }
    let two = BigNum::from_u32(2).map_err(|_| CryptoError::Unavailable)?;
    let mut residue = BigNum::new().map_err(|_| CryptoError::Unavailable)?;
    residue
        .mod_exp(&two, &half, &number, &mut context)
        .map_err(|_| CryptoError::Unavailable)?;
    if residue != minus_one {
        return Err(CryptoError::Parameters);
    }
    let mut cache = cache.lock().map_err(|_| CryptoError::Unavailable)?;
    if cache.len() < 8 {
        cache.push(*modulus);
    }
    Ok(())
}

fn signed_modulus_bytes(value: &str) -> Result<[u8; GROUP_BYTES], CryptoError> {
    let failure = CryptoError::ModulusSignature;
    if value.len() > 8192 {
        return Err(failure);
    }
    let normalized = value.replace("\r\n", "\n");
    let normalized = normalized.strip_suffix('\n').unwrap_or(&normalized);
    let lines: Vec<_> = normalized.split('\n').collect();
    if lines.len() < 11
        || lines[..3] != ["-----BEGIN PGP SIGNED MESSAGE-----", "Hash: SHA256", ""]
        || lines[4] != "-----BEGIN PGP SIGNATURE-----"
        || lines.last() != Some(&"-----END PGP SIGNATURE-----")
    {
        return Err(failure);
    }
    // A single cleartext line means there is no ambiguous newline or dash
    // escaping canonicalization. Trailing data and extra packets are rejected.
    let modulus = decode::<GROUP_BYTES>(lines[3], failure)?;
    let mut start = 5;
    while start < lines.len() && !lines[start].is_empty() {
        if start > 13
            || !(lines[start].starts_with("Version: ") || lines[start].starts_with("Comment: "))
            || !lines[start].bytes().all(|b| (32..=126).contains(&b))
        {
            return Err(failure);
        }
        start += 1;
    }
    start += 1;
    let crc_index = lines.len().checked_sub(2).ok_or(failure)?;
    if start >= crc_index || !lines[crc_index].starts_with('=') {
        return Err(failure);
    }
    let encoded = lines[start..crc_index].join("");
    let packet = STANDARD.decode(encoded).map_err(|_| failure)?;
    let checksum = decode::<3>(&lines[crc_index][1..], failure)?;
    if crc24(&packet) != checksum {
        return Err(failure);
    }
    let body = signature_packet(&packet)?;
    if body.len() < 10 || body[..4] != [4, 1, 22, 8] {
        return Err(failure);
    }
    let hashed_end = 6 + usize::from(u16::from_be_bytes([body[4], body[5]]));
    if hashed_end + 2 > body.len() {
        return Err(failure);
    }
    check_subpackets(&body[6..hashed_end])?;
    let unhashed_end =
        hashed_end + 2 + usize::from(u16::from_be_bytes([body[hashed_end], body[hashed_end + 1]]));
    if unhashed_end + 2 > body.len() {
        return Err(failure);
    }
    check_subpackets(&body[hashed_end + 2..unhashed_end])?;
    let mut hash = Sha256::new();
    hash.update(lines[3].as_bytes());
    hash.update(&body[..hashed_end]);
    hash.update([4, 255]);
    hash.update((hashed_end as u32).to_be_bytes());
    let digest = hash.finalize();
    if digest[..2] != body[unhashed_end..unhashed_end + 2] {
        return Err(failure);
    }
    let (r, next) = read_mpi(body, unhashed_end + 2)?;
    let (s, end) = read_mpi(body, next)?;
    if end != body.len() {
        return Err(failure);
    }
    let mut signature = [0u8; 64];
    signature[..32].copy_from_slice(&r);
    signature[32..].copy_from_slice(&s);
    let mut spki = SPKI_PREFIX.to_vec();
    spki.extend_from_slice(&MODULUS_KEY);
    let key = PKey::public_key_from_der(&spki).map_err(|_| failure)?;
    let mut verifier = Verifier::new_without_digest(&key).map_err(|_| failure)?;
    if !verifier
        .verify_oneshot(&signature, &digest)
        .map_err(|_| failure)?
    {
        return Err(failure);
    }
    Ok(modulus)
}

fn crc24(value: &[u8]) -> [u8; 3] {
    let mut crc = 0xb704ce_u32;
    for byte in value {
        crc ^= u32::from(*byte) << 16;
        for _ in 0..8 {
            crc <<= 1;
            if crc & 0x1000000 != 0 {
                crc ^= 0x1864cfb;
            }
        }
    }
    let bytes = crc.to_be_bytes();
    [bytes[1], bytes[2], bytes[3]]
}

fn signature_packet(value: &[u8]) -> Result<&[u8], CryptoError> {
    let failure = CryptoError::ModulusSignature;
    if value.len() < 2 || value[0] != 0xc2 {
        return Err(failure);
    }
    let (length, offset) = match value[1] {
        n @ 0..=191 => (usize::from(n), 2),
        n @ 192..=223 if value.len() >= 3 => (
            ((usize::from(n) - 192) << 8) + usize::from(value[2]) + 192,
            3,
        ),
        255 if value.len() >= 6 => (
            u32::from_be_bytes([value[2], value[3], value[4], value[5]]) as usize,
            6,
        ),
        _ => return Err(failure),
    };
    if length != value.len() - offset {
        return Err(failure);
    }
    Ok(&value[offset..])
}

fn read_mpi(value: &[u8], offset: usize) -> Result<([u8; 32], usize), CryptoError> {
    let failure = CryptoError::ModulusSignature;
    if offset + 2 > value.len() {
        return Err(failure);
    }
    let bits = usize::from(u16::from_be_bytes([value[offset], value[offset + 1]]));
    let length = bits.div_ceil(8);
    let start = offset + 2;
    let end = start + length;
    if !(1..=256).contains(&bits)
        || end > value.len()
        || value[start] == 0
        || (length - 1) * 8 + 8 - value[start].leading_zeros() as usize != bits
    {
        return Err(failure);
    }
    let mut output = [0u8; 32];
    output[32 - length..].copy_from_slice(&value[start..end]);
    Ok((output, end))
}

fn check_subpackets(value: &[u8]) -> Result<(), CryptoError> {
    let mut offset = 0;
    while offset < value.len() {
        let length = usize::from(value[offset]);
        offset += 1;
        if !(1..192).contains(&length) || offset + length > value.len() {
            return Err(CryptoError::ModulusSignature);
        }
        let kind = value[offset];
        if kind & 0x80 != 0 && ![2, 16, 33].contains(&(kind & 0x7f)) {
            return Err(CryptoError::ModulusSignature);
        }
        offset += length;
    }
    Ok(())
}

pub(super) struct KeyMaterial {
    seed: Zeroizing<[u8; 32]>,
    private: Zeroizing<[u8; 32]>,
    #[cfg(test)]
    public: [u8; 32],
    ed_public: [u8; 32],
}
impl fmt::Debug for KeyMaterial {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("KeyMaterial([redacted])")
    }
}
impl KeyMaterial {
    pub(super) fn generate() -> Result<Self, CryptoError> {
        let mut seed = Zeroizing::new([0u8; 32]);
        rand_bytes(&mut *seed).map_err(|_| CryptoError::Unavailable)?;
        Self::from_seed(*seed)
    }
    pub(super) fn from_seed(seed: [u8; 32]) -> Result<Self, CryptoError> {
        let seed = Zeroizing::new(seed);
        let key = private_key(&seed)?;
        let mut ed_public = [0u8; 32];
        if key
            .raw_public_key(&mut ed_public)
            .map_err(|_| CryptoError::Key)?
            .len()
            != 32
        {
            return Err(CryptoError::Key);
        }
        let mut digest = Zeroizing::new([0u8; 64]);
        digest.copy_from_slice(&Sha512::digest(seed.as_slice()));
        let mut private = Zeroizing::new([0u8; 32]);
        private.copy_from_slice(&digest[..32]);
        private[0] &= 248;
        private[31] &= 127;
        private[31] |= 64;
        let scalar = StaticSecret::from(*private);
        let public = PublicKey::from(&scalar).to_bytes();
        let converted = CompressedEdwardsY(ed_public)
            .decompress()
            .ok_or(CryptoError::Key)?
            .to_montgomery()
            .to_bytes();
        if !bool::from(converted.ct_eq(&public)) {
            return Err(CryptoError::Key);
        }
        Ok(Self {
            seed,
            private,
            #[cfg(test)]
            public,
            ed_public,
        })
    }
    pub(super) fn ed25519_seed(&self) -> &[u8; 32] {
        &self.seed
    }
    #[cfg(test)]
    pub(super) fn ed25519_public(&self) -> [u8; 32] {
        self.ed_public
    }
    pub(super) fn wireguard_private(&self) -> &[u8; 32] {
        &self.private
    }
    #[cfg(test)]
    pub(super) fn wireguard_public(&self) -> [u8; 32] {
        self.public
    }
    pub(super) fn spki_pem(&self) -> String {
        let mut der = SPKI_PREFIX.to_vec();
        der.extend_from_slice(&self.ed_public);
        pem("PUBLIC KEY", &der)
    }
    #[cfg(test)]
    pub(super) fn pkcs8_pem(&self) -> Zeroizing<String> {
        let der = self.pkcs8_der();
        Zeroizing::new(pem("PRIVATE KEY", &der))
    }
    pub(super) fn pkcs8_der(&self) -> Zeroizing<Vec<u8>> {
        let mut der = Zeroizing::new(PKCS8_PREFIX.to_vec());
        der.extend_from_slice(&*self.seed);
        der
    }
    #[cfg(test)]
    pub(super) fn sign(&self, message: &[u8]) -> Result<[u8; 64], CryptoError> {
        let key = private_key(&self.seed)?;
        let mut signer = Signer::new_without_digest(&key).map_err(|_| CryptoError::Unavailable)?;
        let mut output = [0u8; 64];
        let length = signer
            .sign_oneshot(&mut output, message)
            .map_err(|_| CryptoError::Unavailable)?;
        if length != output.len() {
            return Err(CryptoError::Unavailable);
        }
        Ok(output)
    }
}

fn private_key(seed: &[u8; 32]) -> Result<PKey<Private>, CryptoError> {
    let mut der = Zeroizing::new(PKCS8_PREFIX.to_vec());
    der.extend_from_slice(seed);
    PKey::private_key_from_pkcs8(&der).map_err(|_| CryptoError::Key)
}
fn pem(label: &str, der: &[u8]) -> String {
    let encoded = Zeroizing::new(STANDARD.encode(der));
    let mut result = format!("-----BEGIN {label}-----\n");
    for line in encoded.as_bytes().chunks(64) {
        // Base64 is ASCII, so this conversion is infallible without unchecked UTF-8.
        result.extend(line.iter().map(|byte| char::from(*byte)));
        result.push('\n');
    }
    result.push_str(&format!("-----END {label}-----\n"));
    result
}

pub(super) struct Certificate {
    pub(super) pem: Zeroizing<String>,
    pub(super) expiration_time: i64,
    pub(super) refresh_time: i64,
}
impl fmt::Debug for Certificate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Certificate([redacted])")
    }
}

/// Verifies the leaf, key and API time binding. The issuing chain is not
/// independently authenticated here; the caller must have obtained this exact
/// response over the authenticated, certificate-checked Proton control API.
pub(super) fn validate_certificate(
    value: &str,
    keys: &KeyMaterial,
    expiration_time: i64,
    refresh_time: i64,
    now: i64,
) -> Result<Certificate, CryptoError> {
    let certificate = parse_leaf(value)?;
    let public = certificate
        .public_key()
        .map_err(|_| CryptoError::Certificate)?;
    let mut raw = [0u8; 32];
    if public.id() != Id::ED25519
        || public
            .raw_public_key(&mut raw)
            .map_err(|_| CryptoError::CertificateKey)?
            .len()
            != 32
        || !bool::from(raw.ct_eq(&keys.ed_public))
    {
        return Err(CryptoError::CertificateKey);
    }
    let expiration = certificate_time(expiration_time)?;
    let current = certificate_time(now)?;
    let start_order = certificate
        .not_before()
        .compare(&current)
        .map_err(|_| CryptoError::CertificateTime)?;
    let end_order = certificate
        .not_after()
        .compare(&current)
        .map_err(|_| CryptoError::CertificateTime)?;
    if start_order == Ordering::Greater || end_order != Ordering::Greater {
        return Err(CryptoError::CertificateExpired);
    }
    if certificate
        .not_after()
        .compare(&expiration)
        .map_err(|_| CryptoError::CertificateTime)?
        != Ordering::Equal
        || refresh_time >= expiration_time
    {
        return Err(CryptoError::CertificateTime);
    }
    let refresh = certificate_time(refresh_time)?;
    if certificate
        .not_before()
        .compare(&refresh)
        .map_err(|_| CryptoError::CertificateTime)?
        == Ordering::Greater
    {
        return Err(CryptoError::CertificateTime);
    }
    if refresh_time <= now {
        return Err(CryptoError::CertificateRefreshDue);
    }
    Ok(Certificate {
        pem: Zeroizing::new(value.to_owned()),
        expiration_time,
        refresh_time,
    })
}

fn parse_leaf(value: &str) -> Result<X509, CryptoError> {
    let failure = CryptoError::Certificate;
    if value.len() > usque_core::proton_vpn::MAX_CERTIFICATE_BYTES || !value.is_ascii() {
        return Err(failure);
    }
    let normalized = value.replace("\r\n", "\n");
    if normalized.contains('\r') {
        return Err(failure);
    }
    let mut remaining = normalized.trim();
    let mut leaf = None;
    let mut seen = Vec::new();
    while !remaining.is_empty() {
        if seen.len() >= 8 {
            return Err(failure);
        }
        let body = remaining
            .strip_prefix("-----BEGIN CERTIFICATE-----\n")
            .ok_or(failure)?;
        let end = body.find("-----END CERTIFICATE-----").ok_or(failure)?;
        let encoded_lines = body[..end].strip_suffix('\n').ok_or(failure)?;
        if encoded_lines.lines().any(str::is_empty) {
            return Err(failure);
        }
        let encoded: String = encoded_lines.lines().collect();
        if encoded.is_empty()
            || !encoded
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"+/=".contains(&b))
        {
            return Err(failure);
        }
        let der = STANDARD.decode(&encoded).map_err(|_| failure)?;
        if STANDARD.encode(&der) != encoded || seen.contains(&der) {
            return Err(failure);
        }
        let certificate = X509::from_der(&der).map_err(|_| failure)?;
        if certificate.to_der().map_err(|_| failure)? != der {
            return Err(failure);
        }
        seen.push(der);
        if leaf.is_none() {
            leaf = Some(certificate);
        }
        let tail = &body[end + "-----END CERTIFICATE-----".len()..];
        if !tail.trim().is_empty() && !tail.starts_with('\n') {
            return Err(failure);
        }
        remaining = tail.trim();
    }
    leaf.ok_or(failure)
}

// libc::time_t is 32 bits on some supported targets. Keep a checked conversion
// even on hosts where it is the same type as the input Unix timestamp.
#[allow(clippy::useless_conversion)]
fn certificate_time(value: i64) -> Result<Asn1Time, CryptoError> {
    Asn1Time::from_unix(value.try_into().map_err(|_| CryptoError::CertificateTime)?)
        .map_err(|_| CryptoError::CertificateTime)
}
