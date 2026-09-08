//! Real loopback QUIC + local proxy interoperability, with ephemeral test keys.
use super::*;
use crate::dns::Resolver;
use crate::dns_stream::StreamDns;
use crate::h2::MasqueTlsIdentity;
use crate::h3_buffer::H3BufferFactory;
use crate::tcp::{FlowClass, ProxyServices, TcpDialer, TcpTarget};
use quiche::h3::NameValue;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, UdpSocket};
use tokio::time::{Instant, timeout};
use tokio_util::sync::CancellationToken;
use tokio_util::task::AbortOnDropHandle;
use usque_core::{
    DataPlaneMode, IdentityProvider, IpPolicy, MasqueKeyPair, Profile, ProxyDnsMode,
    TransportPolicy,
};

fn identities() -> (MasqueTlsIdentity, MasqueTlsIdentity) {
    let client = MasqueKeyPair::generate();
    let server = MasqueKeyPair::generate();
    let build = |own: &MasqueKeyPair, peer: &MasqueKeyPair| {
        MasqueTlsIdentity::new(
            own.private_sec1_der().unwrap(),
            &peer.public_spki_der().unwrap(),
            "172.16.0.2".parse().unwrap(),
            "2001:db8::2".parse().unwrap(),
        )
        .unwrap()
    };
    (build(&client, &server), build(&server, &client))
}

async fn server(
    socket: UdpSocket,
    identity: MasqueTlsIdentity,
    expected_sni: &'static str,
    targets: Arc<Mutex<Vec<String>>>,
) {
    let address = socket.local_addr().unwrap();
    let mut incoming = vec![0; 65536];
    let (n, remote) = socket.recv_from(&mut incoming).await.unwrap();
    let (mut config, _) = crate::h3::quic_config(&identity, 1232).unwrap();
    config.set_initial_max_streams_bidi(64);
    config.enable_dgram(false, 0, 0);
    config.enable_pacing(false);
    let mut quic = quiche::accept_with_buf_factory::<H3BufferFactory>(
        &quiche::ConnectionId::from_ref(&[0x51; 20]),
        None,
        address,
        remote,
        &mut config,
    )
    .unwrap();
    quic.recv(
        &mut incoming[..n],
        quiche::RecvInfo {
            from: remote,
            to: address,
        },
    )
    .unwrap();
    let mut h3: Option<quiche::h3::Connection> = None;
    let mut pending: HashMap<u64, (VecDeque<u8>, bool)> = HashMap::new();
    let mut output = vec![0; 65536];
    loop {
        if quic.is_closed() {
            break;
        }
        if quic.is_established() && h3.is_none() {
            assert_eq!(quic.server_name(), Some(expected_sni));
            h3 = Some(
                quiche::h3::Connection::with_transport(
                    &mut quic,
                    &quiche::h3::Config::new().unwrap(),
                )
                .unwrap(),
            );
        }
        if let Some(h3) = h3.as_mut() {
            loop {
                match h3.poll(&mut quic) {
                    Ok((id, quiche::h3::Event::Headers { list, .. })) => {
                        assert_eq!(list.len(), 2);
                        assert_eq!(list[0].name(), b":method");
                        assert_eq!(list[0].value(), b"CONNECT");
                        assert_eq!(list[1].name(), b":authority");
                        let target = String::from_utf8(list[1].value().to_vec()).unwrap();
                        targets.lock().unwrap().push(target.clone());
                        let rejected = target.starts_with("refused.test:");
                        h3.send_response(
                            &mut quic,
                            id,
                            &[quiche::h3::Header::new(
                                b":status",
                                if rejected { b"403" } else { b"200" },
                            )],
                            rejected,
                        )
                        .unwrap();
                        if !rejected {
                            pending.insert(id, (VecDeque::new(), false));
                        }
                    }
                    Ok((id, quiche::h3::Event::Data)) => {
                        let mut bytes = [0; 4096];
                        while let Ok(n) = h3.recv_body(&mut quic, id, &mut bytes) {
                            if n == 0 {
                                break;
                            }
                            if let Some((queue, _)) = pending.get_mut(&id) {
                                queue.extend(&bytes[..n]);
                            }
                        }
                    }
                    Ok((id, quiche::h3::Event::Finished)) => {
                        if let Some((_, fin)) = pending.get_mut(&id) {
                            *fin = true;
                        }
                    }
                    Ok((id, quiche::h3::Event::Reset(_))) => {
                        pending.remove(&id);
                    }
                    Ok(_) => {}
                    Err(quiche::h3::Error::Done) => break,
                    Err(error) => panic!("loopback H3 error: {error:?}"),
                }
            }
            pending.retain(|id, (queue, fin)| {
                if !queue.is_empty()
                    && let Ok(n) = h3.send_body(&mut quic, *id, queue.make_contiguous(), false)
                {
                    queue.drain(..n);
                }
                !(*fin && queue.is_empty() && h3.send_body(&mut quic, *id, &[], true).is_ok())
            });
        }
        while let Ok((n, info)) = quic.send(&mut output) {
            socket.send_to(&output[..n], info.to).await.unwrap();
        }
        tokio::select! {
            received = socket.recv_from(&mut incoming) => {
                let (n, from) = received.unwrap();
                assert_eq!(from, remote, "all targets must reuse one QUIC session");
                let _ = quic.recv(&mut incoming[..n], quiche::RecvInfo { from, to: address });
            }
            _ = tokio::time::sleep(quic.timeout().unwrap_or(Duration::from_secs(1))) => quic.on_timeout(),
        }
    }
}

#[tokio::test]
async fn shared_session_serves_parallel_connects_socks_http_and_half_close_without_datagrams() {
    for provider in [
        IdentityProvider::Consumer,
        IdentityProvider::zero_trust("fixture-team").unwrap(),
    ] {
        timeout(Duration::from_secs(8), async {
            let (mut identity, peer) = identities();
            let expected_sni = usque_core::l4_server_name(&provider);
            identity.provider = Some(provider);
            let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
            let endpoint = socket.local_addr().unwrap();
            let targets = Arc::new(Mutex::new(Vec::new()));
            let server = AbortOnDropHandle::new(tokio::spawn(server(socket, peer, expected_sni, targets.clone())));
            let mut profile = Profile { data_plane: DataPlaneMode::L4Proxy, transport: TransportPolicy::Http2, ip_policy: IpPolicy::Ipv4Only, ..Profile::default() };
            profile.endpoint.ipv4 = "127.0.0.1".parse().unwrap();
            profile.endpoint.port = endpoint.port();
            profile.endpoint.sni = "legacy.example.com".to_owned();
            profile.proxy.dns_mode = ProxyDnsMode::EdgeResolved;
            let cancel = CancellationToken::new();
            let protector = Arc::new(crate::NoopSocketProtector);
            let counters = Arc::default();
            let client = L4Client::start(profile.clone(), identity, protector.clone(), None, crate::telemetry::ConnectionTelemetry::default(), Arc::clone(&counters), &cancel).await.unwrap();
            assert!(!client.snapshot().connect_verified);
            let mut flows = tokio::task::JoinSet::new();
            for i in 0..24 {
                let client = client.clone();
                let cancel = cancel.clone();
                flows.spawn(async move {
                    let mut stream = client.connect(TcpTarget::new(&format!("target-{i}.test"), 443).unwrap(), Instant::now() + Duration::from_secs(3), &cancel, FlowClass::Business).await.unwrap();
                    stream.write_all(b"shared session").await.unwrap();
                    stream.shutdown().await.unwrap();
                    let mut bytes = Vec::new();
                    stream.read_to_end(&mut bytes).await.unwrap();
                    assert_eq!(bytes, b"shared session");
                });
            }
            while let Some(result) = flows.join_next().await { result.unwrap(); }
            assert!(client.snapshot().connect_verified);
            assert_eq!(client.snapshot().sessions, 1);
            let dns = Arc::new(StreamDns::new(client.clone(), protector.clone(), cancel.clone(), client.metrics.clone()));
            let services = ProxyServices { admission: None, dialer: client.clone(), udp: None,
                resolver: Resolver::for_streams(dns, vec![], ProxyDnsMode::EdgeResolved, protector.clone()),
                protector, geo_policy: Arc::default(), counters, ipv4: "172.16.0.2".parse().unwrap(), ipv6: "2001:db8::2".parse().unwrap(), cancellation: cancel.clone(), health: client.health.clone() };
            let socks_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let socks_address = socks_listener.local_addr().unwrap();
            let mut socks = crate::socks5::Socks5Frontend::activate_services(&profile, services.clone(), vec![socks_listener]).unwrap();
            let mut local = tokio::net::TcpStream::connect(socks_address).await.unwrap();
            local.write_all(&[5,1,0]).await.unwrap();
            let mut reply = [0;2]; local.read_exact(&mut reply).await.unwrap(); assert_eq!(reply, [5,0]);
            local.write_all(&[5,3,0,1,0,0,0,0,0,0]).await.unwrap();
            let mut reply = [0;10]; local.read_exact(&mut reply).await.unwrap(); assert_eq!(reply[1], 7);
            drop(local);
            let http_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let http_address = http_listener.local_addr().unwrap();
            let mut http = crate::http_proxy::HttpProxyFrontend::activate_services(&profile, services, vec![http_listener]).unwrap();
            let mut local = tokio::net::TcpStream::connect(http_address).await.unwrap();
            local.write_all(b"CONNECT buffered.test:443 HTTP/1.1\r\nHost: buffered.test:443\r\nProxy-Authorization: Basic unused\r\n\r\nearly bytes").await.unwrap();
            let mut response = Vec::new();
            while !response.ends_with(b"\r\n\r\n") { response.push(local.read_u8().await.unwrap()); }
            assert!(response.starts_with(b"HTTP/1.1 200"));
            let mut echoed = [0;11]; local.read_exact(&mut echoed).await.unwrap(); assert_eq!(&echoed, b"early bytes");
            drop(local);
            let rejected = client.connect(TcpTarget::new("refused.test", 443).unwrap(), Instant::now()+Duration::from_secs(1), &cancel, FlowClass::Business).await;
            assert!(matches!(rejected, Err(crate::tcp::DialError::Rejected(403))));
            assert_eq!(client.snapshot().sessions, 1);
            assert_eq!(targets.lock().unwrap().len(), 26);
            socks.shutdown().await; http.shutdown().await;
            client.shutdown().await;
            drop(server);
        }).await.unwrap();
    }
}
