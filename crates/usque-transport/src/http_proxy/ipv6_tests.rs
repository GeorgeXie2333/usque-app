//! Real loopback HTTP frontends with recorded, memory-only outbound streams.
use super::*;
use crate::dns_stream::StreamDns;
use crate::socket::SocketHandle;
use crate::tcp::{DialError, FlowClass, ProxyServices, TcpDialer, TcpIo, TcpTarget};
use async_trait::async_trait;
use std::io;
use std::pin::Pin;
use std::sync::Mutex as StdMutex;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, DuplexStream, ReadBuf};
use tokio_util::task::TaskTracker;
use usque_core::ProxyDnsMode;

struct MemoryStream(DuplexStream);
impl TcpIo for MemoryStream {
    fn local_addr(&self) -> io::Result<SocketAddr> {
        Ok("[2001:db8::2]:41000".parse().unwrap())
    }
}
impl AsyncRead for MemoryStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_read(cx, buffer)
    }
}
impl AsyncWrite for MemoryStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.0).poll_write(cx, buffer)
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_shutdown(cx)
    }
}

#[derive(Clone, Copy, Debug)]
enum RequestForm {
    Connect,
    Absolute,
    Origin,
}
impl RequestForm {
    fn wire(self, authority: &str) -> String {
        match self {
            Self::Connect => {
                format!("CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\n\r\nHELLO")
            }
            Self::Absolute => format!(
                "GET http://{authority}/ipv6?q=1 HTTP/1.1\r\nHost: ignored.example\r\nConnection: close\r\n\r\n"
            ),
            Self::Origin => {
                format!("GET /ipv6?q=1 HTTP/1.1\r\nHost: {authority}\r\nConnection: close\r\n\r\n")
            }
        }
    }
}

struct NoPhysicalNetwork;
impl SocketProtector for NoPhysicalNetwork {
    fn protect(&self, _: SocketHandle) -> Result<(), String> {
        panic!("numeric HTTP targets must not open physical egress sockets")
    }
    fn resolve(&self, _: &str, _: u16) -> Result<Vec<SocketAddr>, String> {
        panic!("numeric HTTP targets must not use physical DNS")
    }
}

struct RecordingDialer {
    form: RequestForm,
    targets: StdMutex<Vec<(TcpTarget, FlowClass)>>,
    headers: Arc<StdMutex<Vec<String>>>,
    peers: TaskTracker,
    cancellation: CancellationToken,
}
#[async_trait]
impl TcpDialer for RecordingDialer {
    async fn connect(
        &self,
        target: TcpTarget,
        _: tokio::time::Instant,
        _: &CancellationToken,
        class: FlowClass,
    ) -> Result<crate::tcp::TcpStream, DialError> {
        self.targets.lock().unwrap().push((target, class));
        assert_eq!(class, FlowClass::Business, "IP literals must bypass DNS");
        let (stream, mut peer) = tokio::io::duplex(4096);
        let form = self.form;
        let headers = self.headers.clone();
        let cancel = self.cancellation.clone();
        self.peers.spawn(async move {
            let work = async {
                match form {
                    RequestForm::Connect => {
                        let mut payload = [0; 5];
                        peer.read_exact(&mut payload).await?;
                        assert_eq!(&payload, b"HELLO");
                        peer.write_all(b"WORLD").await?;
                    }
                    RequestForm::Absolute | RequestForm::Origin => {
                        let header = read_header(&mut peer).await?;
                        headers.lock().unwrap().push(header);
                        peer.write_all(
                            b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
                        )
                        .await?;
                    }
                }
                Ok::<_, io::Error>(())
            };
            tokio::select! {
                _ = cancel.cancelled() => {},
                result = work => result.expect("memory-only upstream exchange"),
            }
        });
        Ok(Box::new(MemoryStream(stream)))
    }
}

async fn read_header(stream: &mut (impl AsyncRead + Unpin)) -> io::Result<String> {
    let mut bytes = Vec::new();
    while !bytes.ends_with(b"\r\n\r\n") {
        assert!(bytes.len() < 4096);
        bytes.push(stream.read_u8().await?);
    }
    Ok(String::from_utf8(bytes).unwrap())
}

struct Fixture {
    frontend: HttpProxyFrontend,
    dialer: Arc<RecordingDialer>,
    cancellation: CancellationToken,
    _health: watch::Sender<RuntimeHealth>,
}
impl Fixture {
    async fn new(mode: ProxyDnsMode, form: RequestForm) -> Self {
        Self::with_policy(
            mode,
            form,
            Arc::new(GeoDirectPolicy::disabled()),
            Arc::new(NoPhysicalNetwork),
        )
        .await
    }

    async fn with_policy(
        mode: ProxyDnsMode,
        form: RequestForm,
        policy: Arc<GeoDirectPolicy>,
        protector: Arc<dyn SocketProtector>,
    ) -> Self {
        let cancellation = CancellationToken::new();
        let dialer = Arc::new(RecordingDialer {
            form,
            targets: StdMutex::default(),
            headers: Arc::default(),
            peers: TaskTracker::new(),
            cancellation: cancellation.clone(),
        });
        let dns = Arc::new(StreamDns::new(
            dialer.clone(),
            protector.clone(),
            cancellation.clone(),
            Arc::default(),
        ));
        let (health, receiver) = watch::channel(RuntimeHealth::Connected {
            path: RuntimePath {
                transport: usque_core::Transport::Http3,
                endpoint_family: usque_core::AddressFamily::Ipv4,
                ipv4_available: true,
                ipv6_available: true,
            },
            reconnect_count: 0,
        });
        let mut profile = Profile::default();
        profile.proxy.dns_mode = mode;
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let services = ProxyServices {
            traffic_policy: Arc::default(),
            admission: None,
            dialer: dialer.clone(),
            udp: None,
            resolver: Resolver::for_streams(
                dns,
                vec!["203.0.113.53".parse().unwrap()],
                mode,
                protector.clone(),
            ),
            protector,
            geo_policy: policy,
            counters: Arc::default(),
            cancellation: cancellation.clone(),
            health: receiver,
        };
        let frontend =
            HttpProxyFrontend::activate_services(&profile, services, vec![listener]).unwrap();
        Self {
            frontend,
            dialer,
            cancellation,
            _health: health,
        }
    }

    async fn request(&self, authority: &str) -> (TcpStream, String) {
        let mut client = TcpStream::connect(self.frontend.listeners()[0])
            .await
            .unwrap();
        client
            .write_all(self.dialer.form.wire(authority).as_bytes())
            .await
            .unwrap();
        let header = tokio::time::timeout(Duration::from_secs(5), read_header(&mut client))
            .await
            .expect("HTTP frontend response deadline")
            .unwrap();
        (client, header)
    }

    async fn shutdown(mut self) {
        self.cancellation.cancel();
        self.frontend.shutdown().await;
        self.dialer.peers.close();
        tokio::time::timeout(Duration::from_secs(2), self.dialer.peers.wait())
            .await
            .expect("upstream tasks must stop");
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.cancellation.cancel();
    }
}

const DNS_MODES: [ProxyDnsMode; 4] = [
    ProxyDnsMode::EdgeResolved,
    ProxyDnsMode::Remote,
    ProxyDnsMode::LocalConfigured,
    ProxyDnsMode::System,
];
const REQUEST_FORMS: [RequestForm; 3] = [
    RequestForm::Connect,
    RequestForm::Absolute,
    RequestForm::Origin,
];

#[tokio::test]
async fn allow_lan_ipv6_http_targets_use_protected_egress_in_every_request_form() {
    use crate::geo_direct::{LanProbeProtector, lan_test_policy};
    let authority = "[fd00::1]:443";
    for allow_lan in [false, true] {
        for mode in DNS_MODES {
            for form in REQUEST_FORMS {
                let protector = Arc::new(LanProbeProtector::default());
                let fixture = Fixture::with_policy(
                    mode,
                    form,
                    Arc::new(lan_test_policy(allow_lan)),
                    protector.clone(),
                )
                .await;
                let (mut client, header) = fixture.request(authority).await;
                assert!(
                    header.starts_with("HTTP/1.1 200 "),
                    "{allow_lan} {mode:?} {form:?}: {header}"
                );
                let expected: &[u8] = if matches!(form, RequestForm::Connect) {
                    b"WORLD"
                } else {
                    b"ok"
                };
                let mut body = vec![0; expected.len()];
                tokio::time::timeout(Duration::from_secs(2), client.read_exact(&mut body))
                    .await
                    .unwrap()
                    .unwrap();
                assert_eq!(body, expected);
                let attempts = if allow_lan {
                    vec![(
                        authority.parse().unwrap(),
                        crate::socket::DirectProtocol::Tcp,
                    )]
                } else {
                    vec![]
                };
                assert_eq!(*protector.attempts.lock().unwrap(), attempts);
                drop(client);
                fixture.shutdown().await;
            }
        }
    }
}

#[tokio::test]
async fn ipv6_targets_reach_the_http_frontend_dialer_in_every_dns_mode() {
    let authority = "[2001:db8::1]:443";
    for mode in DNS_MODES {
        for form in REQUEST_FORMS {
            let fixture = Fixture::new(mode, form).await;
            let (mut client, header) = fixture.request(authority).await;
            assert!(
                header.starts_with("HTTP/1.1 200 "),
                "{mode:?} {form:?}: {header}"
            );
            let expected_body: &[u8] = if matches!(form, RequestForm::Connect) {
                b"WORLD"
            } else {
                b"ok"
            };
            let mut body = vec![0; expected_body.len()];
            tokio::time::timeout(Duration::from_secs(2), client.read_exact(&mut body))
                .await
                .expect("proxied payload deadline")
                .unwrap();
            assert_eq!(body, expected_body);
            assert_eq!(
                *fixture.dialer.targets.lock().unwrap(),
                vec![(
                    TcpTarget::address(authority.parse().unwrap()),
                    FlowClass::Business
                )],
                "{mode:?} {form:?}: only the literal target may be dialed"
            );
            if !matches!(form, RequestForm::Connect) {
                let headers = fixture.dialer.headers.lock().unwrap();
                assert_eq!(headers.len(), 1);
                assert!(headers[0].starts_with("GET /ipv6?q=1 HTTP/1.1\r\n"));
                assert!(headers[0].lines().any(|line| {
                    line.split_once(':').is_some_and(|(name, value)| {
                        name.eq_ignore_ascii_case("host") && value.trim() == authority
                    })
                }));
            }
            drop(client);
            fixture.shutdown().await;
        }
    }
}

#[tokio::test]
async fn invalid_ipv6_authorities_are_rejected_before_the_frontend_dials() {
    for (form, authority) in [
        (RequestForm::Connect, "[not-an-ip]:443"),
        (RequestForm::Absolute, "[2001:db8::1]:0"),
        (RequestForm::Origin, "user@[2001:db8::1]:443"),
        (RequestForm::Origin, "[2001:db8::1:443"),
    ] {
        let fixture = Fixture::new(ProxyDnsMode::EdgeResolved, form).await;
        let (client, header) = fixture.request(authority).await;
        assert!(
            header.starts_with("HTTP/1.1 400 "),
            "{form:?} {authority}: {header}"
        );
        assert!(fixture.dialer.targets.lock().unwrap().is_empty());
        drop(client);
        fixture.shutdown().await;
    }
}
