//! Loopback TLS only. No WARP identity, VPN, public API or platform mutation.
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
use bytes::Bytes;
use http::Method;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use std::{sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
use tokio_util::task::AbortOnDropHandle;

fn certificates() -> (Arc<rustls::ServerConfig>, reqwest::Certificate) {
    let group = EcGroup::from_curve_name(Nid::X9_62_PRIME256V1).unwrap();
    let key = PKey::from_ec_key(EcKey::generate(&group).unwrap()).unwrap();
    let mut name = X509NameBuilder::new().unwrap();
    name.append_entry_by_text("CN", "Proton system HTTP loopback test")
        .unwrap();
    let name = name.build();
    let mut cert = X509::builder().unwrap();
    cert.set_version(2).unwrap();
    cert.set_serial_number(&Asn1Integer::from_bn(&BigNum::from_u32(1).unwrap()).unwrap())
        .unwrap();
    cert.set_subject_name(&name).unwrap();
    cert.set_issuer_name(&name).unwrap();
    cert.set_pubkey(&key).unwrap();
    cert.set_not_before(&Asn1Time::days_from_now(0).unwrap())
        .unwrap();
    cert.set_not_after(&Asn1Time::days_from_now(1).unwrap())
        .unwrap();
    cert.append_extension(BasicConstraints::new().critical().build().unwrap())
        .unwrap();
    cert.append_extension(
        KeyUsage::new()
            .critical()
            .digital_signature()
            .build()
            .unwrap(),
    )
    .unwrap();
    cert.append_extension(ExtendedKeyUsage::new().server_auth().build().unwrap())
        .unwrap();
    let san = SubjectAlternativeName::new()
        .dns("localhost")
        .build(&cert.x509v3_context(None, None))
        .unwrap();
    cert.append_extension(san).unwrap();
    cert.sign(&key, MessageDigest::sha256()).unwrap();
    let der = cert.build().to_der().unwrap();
    let root = reqwest::Certificate::from_der(&der).unwrap();
    let config = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_no_client_auth()
    .with_single_cert(
        vec![CertificateDer::from(der)],
        PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
            key.private_key_to_der_pkcs8().unwrap(),
        )),
    )
    .unwrap();
    (Arc::new(config), root)
}

async fn peer(response: &'static str) -> (SystemHttp, String, AbortOnDropHandle<Vec<u8>>) {
    peer_with_timeout(response, RESPONSE_TIMEOUT).await
}

async fn peer_with_timeout(
    response: &'static str,
    timeout: Duration,
) -> (SystemHttp, String, AbortOnDropHandle<Vec<u8>>) {
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let port = listener.local_addr().unwrap().port();
    let (config, root) = certificates();
    let http = SystemHttp {
        client: builder()
            .no_proxy()
            .add_root_certificate(root)
            .timeout(timeout)
            .build()
            .unwrap(),
    };
    let task = AbortOnDropHandle::new(tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let Ok(mut tls) = tokio_rustls::TlsAcceptor::from(config).accept(socket).await else {
            return Vec::new();
        };
        let mut input = Vec::new();
        while input.len() < 4096 && !input.windows(4).any(|chunk| chunk == b"\r\n\r\n") {
            let mut byte = [0];
            if tls.read_exact(&mut byte).await.is_err() {
                break;
            }
            input.push(byte[0]);
        }
        if response.is_empty() {
            // The client must close a cancelled/timed-out request. Keeping this
            // future alive also exercises cancellation after a socket is opened.
            let mut rest = Vec::new();
            let _ = tls.read_to_end(&mut rest).await;
        } else {
            let _ = tls.write_all(response.as_bytes()).await;
            let _ = tls.shutdown().await;
        }
        input
    }));
    (http, format!("https://localhost:{port}/auth/v4/info"), task)
}

fn request(url: &str, limit: usize) -> InternalRequest<'_> {
    InternalRequest {
        url,
        method: Method::GET,
        headers: vec![
            ("user-agent", "synthetic Android device"),
            ("x-pm-appversion", "android-vpn@synthetic"),
            ("authorization", "Bearer synthetic-private-token"),
        ],
        body: Bytes::new(),
        limit,
        ipv6: None,
    }
}

#[tokio::test]
async fn system_sockets_work_without_a_warp_identity_and_keep_identity_and_error_bodies() {
    let (http, url, peer) = peer(
        "HTTP/1.1 422 Unprocessable Entity\r\nContent-Length: 14\r\nRetry-After: 3\r\n\r\n{\"Code\":12087}",
    )
    .await;
    assert_eq!(http.generation(), None);
    assert!(http.current(None));
    let response = http
        .exchange(request(&url, 256), &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(response.status, 422);
    assert_eq!(response.retry_after, Some(3));
    assert_eq!(&*response.body, br#"{"Code":12087}"#);
    let wire = String::from_utf8(peer.await.unwrap()).unwrap();
    assert!(wire.starts_with("GET /auth/v4/info HTTP/1.1\r\n"));
    for header in [
        "accept-encoding: identity\r\n",
        "user-agent: synthetic Android device\r\n",
        "x-pm-appversion: android-vpn@synthetic\r\n",
        "authorization: Bearer synthetic-private-token\r\n",
    ] {
        assert!(wire.contains(header));
    }
}

#[tokio::test]
async fn redirects_cannot_forward_credentials_to_another_endpoint() {
    let (http, url, peer) = peer(
        "HTTP/1.1 307 Temporary Redirect\r\nLocation: https://never-request.invalid/\r\nContent-Length: 0\r\n\r\n",
    )
    .await;
    let response = http
        .exchange(request(&url, 256), &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(response.status, 307);
    assert!(!peer.await.unwrap().is_empty());
}

#[tokio::test]
async fn response_limits_cover_declared_and_streamed_bodies_and_reject_compression() {
    for (response, failure) in [
        (
            "HTTP/1.1 200 OK\r\nContent-Length: 100000000\r\n\r\n",
            InternalHttpError::SizeLimit,
        ),
        (
            "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n9\r\n123456789\r\n0\r\n\r\n",
            InternalHttpError::SizeLimit,
        ),
        (
            "HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\nContent-Length: 0\r\n\r\n",
            InternalHttpError::Encoding,
        ),
    ] {
        let (http, url, peer) = peer(response).await;
        assert_eq!(
            http.exchange(request(&url, 8), &CancellationToken::new())
                .await
                .err(),
            Some(failure)
        );
        peer.await.unwrap();
    }
}

#[tokio::test]
async fn cancellation_and_timeout_close_inflight_sockets_without_replay() {
    for cancelled in [true, false] {
        let (http, url, peer) = peer_with_timeout("", Duration::from_secs(1)).await;
        let cancel = CancellationToken::new();
        let stop = async {
            if cancelled {
                tokio::time::sleep(Duration::from_millis(100)).await;
                cancel.cancel();
            }
        };
        let (result, ()) = tokio::join!(http.exchange(request(&url, 256), &cancel), stop);
        assert_eq!(
            result.err(),
            Some(if cancelled {
                InternalHttpError::Cancelled
            } else {
                InternalHttpError::Timeout
            })
        );
        tokio::time::timeout(Duration::from_secs(2), peer)
            .await
            .unwrap()
            .unwrap();
    }
}

#[tokio::test]
async fn untrusted_tls_and_invalid_requests_fail_closed() {
    let (_, url, peer) = peer("HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n").await;
    let http = SystemHttp {
        client: builder().no_proxy().build().unwrap(),
    };
    assert_eq!(
        http.exchange(request(&url, 256), &CancellationToken::new())
            .await
            .err(),
        Some(InternalHttpError::Tls)
    );
    peer.await.unwrap();
    let (trusted, mismatch, peer) =
        self::peer("HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n").await;
    assert_eq!(
        trusted
            .exchange(
                request(&mismatch.replace("localhost", "127.0.0.1"), 256),
                &CancellationToken::new(),
            )
            .await
            .err(),
        Some(InternalHttpError::Tls)
    );
    peer.await.unwrap();
    for url in [
        "http://localhost/",
        "https://username:password@localhost/",
        "https://localhost/#fragment",
    ] {
        assert_eq!(
            http.exchange(request(url, 256), &CancellationToken::new())
                .await
                .err(),
            Some(InternalHttpError::Protocol)
        );
    }
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert_eq!(
        http.exchange(request(&url, 256), &cancel).await.err(),
        Some(InternalHttpError::Cancelled)
    );
}
