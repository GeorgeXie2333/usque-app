use super::*;
use boring::{
    asn1::{Asn1Integer, Asn1Time},
    bn::BigNum,
    ec::{EcGroup, EcKey},
    hash::MessageDigest,
    nid::Nid,
    pkey::PKey,
    x509::{
        X509, X509NameBuilder,
        extension::{BasicConstraints, ExtendedKeyUsage, KeyUsage, SubjectAlternativeName},
    },
};
use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::test]
async fn client_hello_matches_wgcf_android_fixture_without_a_socket() {
    let (client, mut peer) = tokio::io::duplex(4096);
    let task = tokio::spawn(async move { connect(client).await.is_err() });
    let mut header = [0; 5];
    peer.read_exact(&mut header).await.unwrap();
    let mut body = vec![0; u16::from_be_bytes([header[3], header[4]]) as usize];
    peer.read_exact(&mut body).await.unwrap();
    drop(peer);
    assert!(task.await.unwrap());
    let mut record = header.to_vec();
    record.extend(body);
    record[11..43].fill(0);
    let actual: String = record.iter().map(|b| format!("{b:02x}")).collect();
    // wgcf cloudflare/api_test.go's non-sensitive initial ClientHello fixture.
    let expected = "16030100a10100009d03030000000000000000000000000000000000000000000000000000000000000000000004c02cc030010000700000001d001b0000186170692e636c6f7564666c617265636c69656e742e636f6d00170000ff01000100000a00080006001d00170018000b00020100002300000010000b000908687474702f312e31000500050100000000000d00140012040308040401050308050501080606010201";
    assert_eq!(actual, expected);
}

fn certificates(name: &str, expired: bool) -> (rustls::ServerConfig, RootCertStore) {
    let group = EcGroup::from_curve_name(Nid::X9_62_PRIME256V1).unwrap();
    let ca_key = PKey::from_ec_key(EcKey::generate(&group).unwrap()).unwrap();
    let key = PKey::from_ec_key(EcKey::generate(&group).unwrap()).unwrap();
    let mut subject = X509NameBuilder::new().unwrap();
    subject
        .append_entry_by_text("CN", "WARP registration test")
        .unwrap();
    let subject = subject.build();
    let now = UnixTime::now().as_secs() as i64;
    let mut ca = X509::builder().unwrap();
    ca.set_version(2).unwrap();
    ca.set_serial_number(&Asn1Integer::from_bn(&BigNum::from_u32(1).unwrap()).unwrap())
        .unwrap();
    ca.set_subject_name(&subject).unwrap();
    ca.set_issuer_name(&subject).unwrap();
    ca.set_pubkey(&ca_key).unwrap();
    ca.set_not_before(&Asn1Time::from_unix(now - 3600).unwrap())
        .unwrap();
    ca.set_not_after(&Asn1Time::from_unix(now + 3600).unwrap())
        .unwrap();
    ca.append_extension(BasicConstraints::new().critical().ca().build().unwrap())
        .unwrap();
    ca.append_extension(
        KeyUsage::new()
            .critical()
            .key_cert_sign()
            .crl_sign()
            .build()
            .unwrap(),
    )
    .unwrap();
    ca.sign(&ca_key, MessageDigest::sha256()).unwrap();
    let ca = ca.build();
    let mut leaf = X509::builder().unwrap();
    leaf.set_version(2).unwrap();
    leaf.set_serial_number(&Asn1Integer::from_bn(&BigNum::from_u32(2).unwrap()).unwrap())
        .unwrap();
    leaf.set_subject_name(&subject).unwrap();
    leaf.set_issuer_name(ca.subject_name()).unwrap();
    leaf.set_pubkey(&key).unwrap();
    leaf.set_not_before(&Asn1Time::from_unix(now - 3600).unwrap())
        .unwrap();
    leaf.set_not_after(&Asn1Time::from_unix(if expired { now - 60 } else { now + 3600 }).unwrap())
        .unwrap();
    leaf.append_extension(BasicConstraints::new().critical().build().unwrap())
        .unwrap();
    leaf.append_extension(
        KeyUsage::new()
            .critical()
            .digital_signature()
            .build()
            .unwrap(),
    )
    .unwrap();
    leaf.append_extension(ExtendedKeyUsage::new().server_auth().build().unwrap())
        .unwrap();
    let san = SubjectAlternativeName::new()
        .dns(name)
        .build(&leaf.x509v3_context(Some(&ca), None))
        .unwrap();
    leaf.append_extension(san).unwrap();
    leaf.sign(&ca_key, MessageDigest::sha256()).unwrap();
    let leaf = CertificateDer::from(leaf.build().to_der().unwrap());
    let ca = CertificateDer::from(ca.to_der().unwrap());
    let mut roots = RootCertStore::empty();
    roots.add(ca.clone()).unwrap();
    let mut server = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_protocol_versions(&[&rustls::version::TLS12])
    .unwrap()
    .with_no_client_auth()
    .with_single_cert(
        vec![leaf, ca],
        PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
            key.private_key_to_der_pkcs8().unwrap(),
        )),
    )
    .unwrap();
    server.alpn_protocols = vec![b"http/1.1".to_vec()];
    (server, roots)
}

#[tokio::test]
async fn api_tls_keeps_hostname_expiry_and_trust_verification() {
    for (name, expired, trusted, accepted) in [
        (HOST, false, true, true),
        ("wrong.example", false, true, false),
        (HOST, true, true, false),
        (HOST, false, false, false),
    ] {
        let (server, roots) = certificates(name, expired);
        let roots = if trusted {
            roots
        } else {
            RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned())
        };
        let context = context(roots).unwrap();
        let (client, peer) = tokio::io::duplex(16 * 1024);
        let (result, ()) = tokio::join!(
            async {
                let mut stream = connect_with_context(client, &context).await?;
                let mut data = [0; 2];
                stream
                    .read_exact(&mut data)
                    .await
                    .map_err(|_| InternalHttpError::Protocol)?;
                assert_eq!(&data, b"ok");
                Ok::<(), InternalHttpError>(())
            },
            async {
                if let Ok(mut stream) = tokio_rustls::TlsAcceptor::from(Arc::new(server))
                    .accept(peer)
                    .await
                {
                    let _ = stream.write_all(b"ok").await;
                }
            }
        );
        assert_eq!(
            result.is_ok(),
            accepted,
            "hostname={name}, expired={expired}, trusted={trusted}"
        );
    }
}
