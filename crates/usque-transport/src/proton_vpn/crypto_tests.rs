//! Offline MIT SRP vectors and synthetic certificate/key checks.
use super::*;

const CERTIFICATE: &str = r#"-----BEGIN CERTIFICATE-----
MIHuMIGhoAMCAQICAQEwBQYDK2VwMCExHzAdBgNVBAMMFnN5bnRoZXRpYy1vZmZs
aW5lLW9ubHkwHhcNMjYxMDE4MjM1OTAwWhcNMjYxMDIwMDAwMDAwWjAhMR8wHQYD
VQQDDBZzeW50aGV0aWMtb2ZmbGluZS1vbmx5MCowBQYDK2VwAyEA11qYAYKxCrfV
S/7TyWQHOg7hcvPapiMlrwIaaPcHURowBQYDK2VwA0EAF2WgL69Jpbkd+Swlq0Yx
jomTKNaxVlak9rgcNd3BuaLqMzV0wOBu0Za+i0voiAzhEh3amlZs5P6rzxO7aJ1X
CQ==
-----END CERTIFICATE-----
"#;
const NOW: i64 = 1792368000;
const EXPIRES: i64 = 1792454400;
const REFRESH: i64 = 1792450800;
const X_PRIVATE: &str = "307c83864f2833cb427a2ef1c00a013cfdff2768d980c0a3a520f006904de94f";
const X_PUBLIC: &str = "d85e07ec22b0ad881537c2f44d662d1a143cf830c57aca4305d85c7a90f6b62e";
const RFC_SEED: &str = "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60";
const RFC_PUBLIC: &str = "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a";
const RFC_SIGNATURE: &str = concat!(
    "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555f",
    "b8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b"
);

fn from_hex<const N: usize>(value: &str) -> [u8; N] {
    assert_eq!(value.len(), N * 2);
    std::array::from_fn(|index| u8::from_str_radix(&value[index * 2..index * 2 + 2], 16).unwrap())
}
fn fixture() -> serde_json::Value {
    serde_json::from_str(include_str!(
        "../../../../tool/proton_poc/testdata/srp_v4.json"
    ))
    .unwrap()
}
fn official_proofs(password: &str) -> SrpProofs {
    let v = fixture();
    SrpProofs::generate_inner(
        4,
        password,
        v["salt"].as_str().unwrap(),
        v["signed_modulus"].as_str().unwrap(),
        v["server_ephemeral"].as_str().unwrap(),
        Some(Zeroizing::new(U2048::from_be_hex(
            v["client_secret_hex"].as_str().unwrap(),
        ))),
    )
    .unwrap()
}
fn keys() -> KeyMaterial {
    KeyMaterial::from_seed(from_hex(RFC_SEED)).unwrap()
}

#[test]
fn proton_mit_full_srp_v4_proof_vector() {
    let v = fixture();
    let proofs = official_proofs(v["password"].as_str().unwrap());
    assert_eq!(proofs.client_proof(), v["client_proof"].as_str().unwrap());
    assert_eq!(
        STANDARD.decode(proofs.client_ephemeral()).unwrap().len(),
        GROUP_BYTES
    );
    proofs
        .verify_server_proof(v["server_proof"].as_str().unwrap())
        .unwrap();
    for wrong in [
        "",
        "invalid-sensitive-server-proof",
        &STANDARD.encode([0u8; GROUP_BYTES]),
    ] {
        assert_eq!(
            proofs.verify_server_proof(wrong),
            Err(CryptoError::ServerProof)
        );
    }
    assert_eq!(format!("{proofs:?}"), "SrpProofs([redacted])");
}

#[test]
fn incorrect_password_cannot_verify_official_server_proof() {
    let v = fixture();
    let proofs = official_proofs("synthetic-wrong-password");
    assert_eq!(
        proofs.verify_server_proof(v["server_proof"].as_str().unwrap()),
        Err(CryptoError::ServerProof)
    );
}

#[test]
fn only_v4_and_bounded_utf8_passwords_are_supported() {
    for version in [0, 1, 2, 3, 5, u32::MAX] {
        assert!(matches!(
            SrpProofs::generate(version, "example", "", "", ""),
            Err(CryptoError::UnsupportedVersion)
        ));
    }
    for password in ["".to_owned(), "x".repeat(73), "é".repeat(37)] {
        assert!(matches!(
            SrpProofs::generate(4, &password, "", "", ""),
            Err(CryptoError::UnsupportedPassword)
        ));
    }
}

#[test]
fn signed_modulus_authentication_rejects_tampering_and_extra_data() {
    let v = fixture();
    let signed = v["signed_modulus"].as_str().unwrap();
    let expected =
        decode::<GROUP_BYTES>(v["modulus"].as_str().unwrap(), CryptoError::Parameters).unwrap();
    assert_eq!(signed_modulus_bytes(signed).unwrap(), expected);
    assert_eq!(
        signed_modulus_bytes(&signed.replace('\n', "\r\n")).unwrap(),
        expected
    );
    for altered in [
        format!("{signed}private-trailing"),
        format!("private-leading{signed}"),
        signed.replace("W2z5", "X2z5"),
        signed.replace("SHA256", "SHA1"),
        signed.replace("wl4EA", "xl4EA"),
        signed.replace("=q6vu", "=q6vv"),
        signed.repeat(2),
        "x".repeat(8193),
    ] {
        assert_eq!(
            signed_modulus_bytes(&altered),
            Err(CryptoError::ModulusSignature)
        );
    }
}

#[test]
fn invalid_groups_and_server_ephemerals_fail_closed() {
    for number in [
        U2048::ZERO,
        U2048::from_u32(23),
        U2048::ONE
            .shl_vartime(2047)
            .wrapping_add(&U2048::from_u32(11)),
    ] {
        let mut bytes = [0u8; GROUP_BYTES];
        bytes.copy_from_slice(number.to_le_bytes().as_ref());
        assert_eq!(validate_modulus(&bytes), Err(CryptoError::Parameters));
    }
    let v = fixture();
    let modulus = signed_modulus_bytes(v["signed_modulus"].as_str().unwrap()).unwrap();
    let number = U2048::from_le_slice(&modulus);
    for remote in [
        U2048::ZERO,
        U2048::ONE,
        number.wrapping_sub(&U2048::ONE),
        number,
        U2048::MAX,
    ] {
        assert!(matches!(
            SrpProofs::generate(
                4,
                "example",
                v["salt"].as_str().unwrap(),
                v["signed_modulus"].as_str().unwrap(),
                &STANDARD.encode(remote.to_le_bytes().as_ref()),
            ),
            Err(CryptoError::Parameters)
        ));
    }
}

#[test]
fn malformed_openpgp_packet_corpus_cannot_panic_or_authenticate() {
    for size in 0..128 {
        let bytes = vec![0xffu8; size];
        assert!(signature_packet(&bytes).is_err());
        assert!(read_mpi(&bytes, 0).is_err());
        assert!(check_subpackets(&bytes).is_err() || size == 0);
    }
    let signed = fixture()["signed_modulus"].as_str().unwrap().to_owned();
    let expected = signed_modulus_bytes(&signed).unwrap();
    let unsigned_values: Vec<_> = ["Version: ", "Comment: "]
        .iter()
        .map(|prefix| {
            let start = signed.find(prefix).unwrap() + prefix.len();
            let end = start + signed[start..].find('\n').unwrap();
            start..end
        })
        .collect();
    for offset in (0..signed.len()).step_by(11) {
        let mut altered = signed.as_bytes().to_vec();
        altered[offset] = b'!';
        let altered = String::from_utf8(altered).unwrap();
        if unsigned_values.iter().any(|range| range.contains(&offset)) {
            // OpenPGP armor Version/Comment values are explicitly unsigned.
            // Changing them must never change the authenticated modulus.
            assert_eq!(signed_modulus_bytes(&altered).unwrap(), expected);
        } else {
            assert!(signed_modulus_bytes(&altered).is_err(), "offset {offset}");
        }
    }
}

#[test]
fn modified_signature_with_recomputed_transport_crc_still_fails() {
    let signed = fixture()["signed_modulus"].as_str().unwrap().to_owned();
    let lines: Vec<_> = signed.lines().collect();
    let end = lines.len() - 2;
    let start = lines
        .iter()
        .position(|line| line.starts_with("wl4E"))
        .unwrap();
    let mut packet = STANDARD.decode(lines[start..end].join("")).unwrap();
    *packet.last_mut().unwrap() ^= 1;
    let encoded = STANDARD.encode(&packet);
    let encoded: String = encoded
        .as_bytes()
        .chunks(64)
        .map(|line| format!("{}\n", std::str::from_utf8(line).unwrap()))
        .collect();
    let altered = format!(
        "-----BEGIN PGP SIGNED MESSAGE-----\nHash: SHA256\n\n{}\n-----BEGIN PGP SIGNATURE-----\n\n{encoded}={}\n-----END PGP SIGNATURE-----\n",
        lines[3],
        STANDARD.encode(crc24(&packet))
    );
    assert_eq!(
        signed_modulus_bytes(&altered),
        Err(CryptoError::ModulusSignature)
    );
}

#[test]
fn rfc8032_ed25519_and_independent_x25519_conversion() {
    let keys = keys();
    assert_eq!(keys.ed25519_public(), from_hex::<32>(RFC_PUBLIC));
    assert_eq!(keys.sign(b"").unwrap(), from_hex::<64>(RFC_SIGNATURE));
    assert_eq!(*keys.wireguard_private(), from_hex::<32>(X_PRIVATE));
    assert_eq!(keys.wireguard_public(), from_hex::<32>(X_PUBLIC));
    assert_ne!(keys.wireguard_private(), keys.ed25519_seed());
    let spki = PKey::public_key_from_pem(keys.spki_pem().as_bytes()).unwrap();
    let private = PKey::private_key_from_pem(keys.pkcs8_pem().as_bytes()).unwrap();
    assert_eq!(spki.id(), Id::ED25519);
    assert!(spki.public_eq(&private));
    let signature = keys.sign(b"synthetic-payload").unwrap();
    let mut verify = Verifier::new_without_digest(&spki).unwrap();
    assert!(
        verify
            .verify_oneshot(&signature, b"synthetic-payload")
            .unwrap()
    );
    assert!(!verify.verify_oneshot(&signature, b"different").unwrap());
    assert_eq!(format!("{keys:?}"), "KeyMaterial([redacted])");
}

#[test]
fn generated_keys_are_distinct_and_internally_consistent() {
    let left = KeyMaterial::generate().unwrap();
    let right = KeyMaterial::generate().unwrap();
    assert_ne!(left.ed25519_public(), right.ed25519_public());
    assert_ne!(left.wireguard_public(), right.wireguard_public());
}

#[test]
fn certificate_leaf_key_and_time_binding() {
    let keys = keys();
    let certificate = validate_certificate(CERTIFICATE, &keys, EXPIRES, REFRESH, NOW).unwrap();
    assert_eq!(certificate.expiration_time, EXPIRES);
    assert_eq!(certificate.refresh_time, REFRESH);
    assert_eq!(&*certificate.pem, CERTIFICATE);
    assert_eq!(format!("{certificate:?}"), "Certificate([redacted])");
    assert!(matches!(
        validate_certificate(
            CERTIFICATE,
            &KeyMaterial::from_seed([0; 32]).unwrap(),
            EXPIRES,
            REFRESH,
            NOW
        ),
        Err(CryptoError::CertificateKey)
    ));
    assert!(matches!(
        validate_certificate(CERTIFICATE, &keys, EXPIRES + 1, REFRESH, NOW),
        Err(CryptoError::CertificateTime)
    ));
    assert!(matches!(
        validate_certificate(CERTIFICATE, &keys, EXPIRES, NOW, NOW),
        Err(CryptoError::CertificateRefreshDue)
    ));
    for now in [NOW - 3600, EXPIRES, EXPIRES + 1] {
        assert!(matches!(
            validate_certificate(CERTIFICATE, &keys, EXPIRES, REFRESH, now),
            Err(CryptoError::CertificateExpired)
        ));
    }
    for value in [
        "private-invalid-pem".to_owned(),
        CERTIFICATE.repeat(2),
        format!("{CERTIFICATE}trailing"),
        "x".repeat(65537),
    ] {
        assert!(matches!(
            validate_certificate(&value, &keys, EXPIRES, REFRESH, NOW),
            Err(CryptoError::Certificate)
        ));
    }
}

#[test]
fn certificate_accepts_bounded_distinct_pem_chain_and_rejects_garbage() {
    use boring::{
        asn1::Asn1Integer,
        ec::{EcGroup, EcKey},
        hash::MessageDigest,
        nid::Nid,
        x509::{X509NameBuilder, extension::BasicConstraints},
    };
    let keys = keys();
    let leaf_key = private_key(keys.ed25519_seed()).unwrap();
    let group = EcGroup::from_curve_name(Nid::X9_62_PRIME256V1).unwrap();
    let issuer_key = PKey::from_ec_key(EcKey::generate(&group).unwrap()).unwrap();
    let mut name = X509NameBuilder::new().unwrap();
    name.append_entry_by_text("CN", "synthetic-offline-issuer")
        .unwrap();
    let issuer_name = name.build();
    let make = |serial, leaf| {
        let mut value = X509::builder().unwrap();
        value.set_version(2).unwrap();
        value
            .set_serial_number(&Asn1Integer::from_bn(&BigNum::from_u32(serial).unwrap()).unwrap())
            .unwrap();
        value.set_subject_name(&issuer_name).unwrap();
        value.set_issuer_name(&issuer_name).unwrap();
        value
            .set_pubkey(if leaf { &leaf_key } else { &issuer_key })
            .unwrap();
        value
            .set_not_before(&certificate_time(NOW - 60).unwrap())
            .unwrap();
        value
            .set_not_after(&certificate_time(EXPIRES).unwrap())
            .unwrap();
        if !leaf {
            value
                .append_extension(BasicConstraints::new().critical().ca().build().unwrap())
                .unwrap();
        }
        value.sign(&issuer_key, MessageDigest::sha256()).unwrap();
        String::from_utf8(value.build().to_pem().unwrap()).unwrap()
    };
    let leaf = make(1, true);
    let issuer = make(2, false);
    let chain = format!("{leaf}{issuer}");
    let validated = validate_certificate(&chain, &keys, EXPIRES, REFRESH, NOW).unwrap();
    assert_eq!(&*validated.pem, &chain);
    validate_certificate(&chain.replace('\n', "\r\n"), &keys, EXPIRES, REFRESH, NOW).unwrap();
    let mut maximum = leaf.clone();
    for serial in 2..=8 {
        maximum.push_str(&make(serial, false));
    }
    validate_certificate(&maximum, &keys, EXPIRES, REFRESH, NOW).unwrap();
    maximum.push_str(&make(9, false));
    for malformed in [
        format!("{chain}{leaf}"),
        format!("{chain}{issuer}"),
        maximum,
        format!("{leaf}garbage\n{issuer}"),
        format!("{}{issuer}", leaf.trim_end()),
        format!("{chain}trailing"),
        format!("garbage{chain}"),
        format!("{chain}-----BEGIN CERTIFICATE-----\n"),
        format!("{chain}-----BEGIN PRIVATE KEY-----\nAA==\n-----END PRIVATE KEY-----\n"),
        format!(
            "{chain}{}",
            " ".repeat(usque_core::proton_vpn::MAX_CERTIFICATE_BYTES)
        ),
    ] {
        assert!(matches!(
            validate_certificate(&malformed, &keys, EXPIRES, REFRESH, NOW),
            Err(CryptoError::Certificate)
        ));
    }
}
