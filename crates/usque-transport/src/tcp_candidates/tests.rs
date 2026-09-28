use super::*;
use crate::tcp::TcpIo;
use std::collections::HashMap;
use std::io;
use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

#[derive(Clone, Copy)]
struct Behavior(Duration, Result<(), DialError>);
#[derive(Default)]
struct State {
    calls: Mutex<Vec<(IpAddr, Instant)>>,
    tokens: Mutex<Vec<CancellationToken>>,
    live: AtomicUsize,
    peak: AtomicUsize,
}
struct Lease(Arc<State>);
impl Drop for Lease {
    fn drop(&mut self) {
        self.0.live.fetch_sub(1, Ordering::SeqCst);
    }
}
struct Stream {
    _lease: Lease,
    cancellation: CancellationToken,
}
impl AsyncRead for Stream {
    fn poll_read(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        _: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Poll::Ready(if self.cancellation.is_cancelled() {
            Err(io::ErrorKind::Interrupted.into())
        } else {
            Ok(())
        })
    }
}
impl AsyncWrite for Stream {
    fn poll_write(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        Poll::Ready(Ok(bytes.len()))
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}
impl TcpIo for Stream {
    fn local_addr(&self) -> io::Result<SocketAddr> {
        Ok("127.0.0.1:12345".parse().unwrap())
    }
}
struct Dialer {
    behaviors: HashMap<IpAddr, Behavior>,
    state: Arc<State>,
}
impl Dialer {
    fn new(behaviors: impl IntoIterator<Item = (IpAddr, Behavior)>) -> Arc<Self> {
        Arc::new(Self {
            behaviors: behaviors.into_iter().collect(),
            state: Arc::new(State::default()),
        })
    }
}
#[async_trait::async_trait]
impl TcpDialer for Dialer {
    async fn connect(
        &self,
        target: TcpTarget,
        deadline: Instant,
        cancellation: &CancellationToken,
        _: FlowClass,
    ) -> Result<TcpStream, DialError> {
        let address = target.authority().parse::<SocketAddr>().unwrap().ip();
        self.state
            .calls
            .lock()
            .unwrap()
            .push((address, Instant::now()));
        self.state.tokens.lock().unwrap().push(cancellation.clone());
        let live = self.state.live.fetch_add(1, Ordering::SeqCst) + 1;
        self.state.peak.fetch_max(live, Ordering::SeqCst);
        let lease = Lease(self.state.clone());
        let behavior = self
            .behaviors
            .get(&address)
            .copied()
            .unwrap_or(Behavior(Duration::ZERO, Err(DialError::Refused)));
        tokio::select! {
            _ = cancellation.cancelled() => return Err(DialError::Cancelled),
            result = tokio::time::timeout_at(deadline, tokio::time::sleep(behavior.0)) => {
                result.map_err(|_| DialError::Timeout)?;
            }
        }
        behavior.1?;
        Ok(Box::new(Stream {
            _lease: lease,
            cancellation: cancellation.clone(),
        }))
    }
}
fn v4(n: u8) -> IpAddr {
    IpAddr::from([198, 51, 100, n])
}
fn v6() -> IpAddr {
    "2001:db8::1".parse().unwrap()
}
fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}
fn hung() -> Behavior {
    Behavior(Duration::from_secs(60), Ok(()))
}
fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(10)
}

#[test]
fn cancelled_real_stack_connect_reclaims_its_buffer_reservation() {
    use ts_netstack_smoltcp::netcore::smoltcp::time::Instant as StackInstant;
    use ts_netstack_smoltcp::netcore::{
        Config, Netstack, Request, TcpBufferMetrics, TcpBufferPolicy, TcpBufferTier, flume, tcp,
    };
    let metrics = TcpBufferMetrics::default();
    let tier = TcpBufferTier {
        receive: 16 * 1024,
        transmit: 16 * 1024,
    };
    let mut stack = Netstack::new(
        Config {
            tcp_buffer_policy: Some(TcpBufferPolicy {
                preferred: tier,
                fallback: tier,
                preferred_budget: 32 * 1024,
                total_budget: 32 * 1024,
            }),
            tcp_buffer_metrics: Some(metrics.clone()),
            ..Config::default()
        },
        StackInstant::from_millis(0),
    );
    assert!(stack.direct_set_ips(["172.16.0.2".parse().unwrap()]));
    let (resp, reply) = flume::bounded(1);
    stack.process_one_cmd(Request {
        handle: None,
        command: tcp::stream::Command::Connect {
            local_endpoint: "172.16.0.2:50000".parse().unwrap(),
            remote_endpoint: "198.51.100.1:443".parse().unwrap(),
        }
        .into(),
        resp,
    });
    assert_eq!(metrics.snapshot().total_bytes, 32 * 1024);
    drop(reply);
    let (pipe, _peer) = crate::packet_pipe::PacketPipe::bounded(4);
    let mut device = crate::packet_pipe::PacketDevice::new(pipe, 1280);
    for now in 0..4 {
        stack.poll_device_io(StackInstant::from_millis(now), &mut device);
    }
    assert_eq!(
        metrics.snapshot().total_bytes,
        0,
        "a losing dial must release its TCP budget"
    );
}

#[tokio::test]
async fn cancellation_wakes_an_idle_real_stack_without_another_packet() {
    use ts_netstack_smoltcp::netcore::{
        Config, HasChannel, TcpBufferMetrics, TcpBufferPolicy, TcpBufferTier,
    };
    let metrics = TcpBufferMetrics::default();
    let tier = TcpBufferTier {
        receive: 16384,
        transmit: 16384,
    };
    let (stack, mut pipe) = crate::netstack::bounded_piped(Config {
        tcp_buffer_policy: Some(TcpBufferPolicy {
            preferred: tier,
            fallback: tier,
            preferred_budget: 32768,
            total_budget: 32768,
        }),
        tcp_buffer_metrics: Some(metrics.clone()),
        ..Config::default()
    });
    // Set IPs through the channel once the actor owns the stack.
    let channel = stack.command_channel();
    let _actor = tokio_util::task::AbortOnDropHandle::new(stack.spawn_tokio());
    use ts_netstack_smoltcp::netcore::NetstackControl;
    channel
        .set_ips(["172.16.0.2".parse().unwrap()])
        .await
        .unwrap();
    let dialer = crate::tcp::StackDialer {
        channel,
        ipv4: "172.16.0.2".parse().unwrap(),
        ipv6: std::net::Ipv6Addr::UNSPECIFIED,
    };
    let cancellation = CancellationToken::new();
    let token = cancellation.clone();
    let dial = tokio::spawn(async move {
        dialer
            .connect(
                TcpTarget::address("198.51.100.1:443".parse().unwrap()),
                deadline(),
                &token,
                FlowClass::Business,
            )
            .await
    });
    assert!(
        tokio::time::timeout(Duration::from_secs(1), pipe.rx.recv_async())
            .await
            .unwrap()
            .is_some()
    );
    assert_eq!(metrics.snapshot().total_bytes, 32768);
    cancellation.cancel();
    assert!(matches!(dial.await.unwrap(), Err(DialError::Cancelled)));
    tokio::time::timeout(Duration::from_secs(1), async {
        while metrics.snapshot().total_bytes != 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test(start_paused = true)]
async fn blackholed_first_family_cannot_cancel_the_winning_stream() {
    for addresses in [[v4(1), v6()], [v6(), v4(1)]] {
        let dialer = Dialer::new([
            (addresses[0], hung()),
            (addresses[1], Behavior(ms(20), Ok(()))),
        ]);
        let parent = CancellationToken::new();
        let started = Instant::now();
        let stream = connect_candidates(
            dialer.clone(),
            CandidateResolution::from_addresses(addresses.to_vec()),
            443,
            deadline(),
            &parent,
        )
        .await
        .unwrap();
        assert_eq!(started.elapsed(), ms(270));
        assert_eq!(dialer.state.peak.load(Ordering::SeqCst), 2);
        assert_eq!(dialer.state.live.load(Ordering::SeqCst), 1);
        {
            let tokens = dialer.state.tokens.lock().unwrap();
            assert!(tokens[0].is_cancelled());
            assert!(
                !tokens[1].is_cancelled(),
                "L4 winner must retain its lifetime"
            );
            parent.cancel();
            assert!(
                tokens[1].is_cancelled(),
                "session cancellation still owns the winner"
            );
        }
        drop(stream);
        assert_eq!(dialer.state.live.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test(start_paused = true)]
async fn a_late_address_family_has_a_reserved_race_slot() {
    let dialer = Dialer::new([
        (v4(1), hung()),
        (v4(2), Behavior(ms(1), Ok(()))),
        (v6(), Behavior(ms(20), Ok(()))),
    ]);
    let resolution = CandidateResolution::test_queries(async { Ok(vec![v4(1), v4(2)]) }, async {
        tokio::time::sleep(ms(800)).await;
        Ok(vec![v6()])
    });
    let started = Instant::now();
    let stream = connect_candidates(
        dialer.clone(),
        resolution,
        443,
        deadline(),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(started.elapsed(), ms(820));
    assert_eq!(
        dialer
            .state
            .calls
            .lock()
            .unwrap()
            .iter()
            .map(|(ip, _)| *ip)
            .collect::<Vec<_>>(),
        vec![v4(1), v6()]
    );
    drop(stream);
    assert_eq!(dialer.state.live.load(Ordering::SeqCst), 0);
}

#[tokio::test(start_paused = true)]
async fn two_same_family_attempts_are_allowed_after_negative_other_family() {
    let dialer = Dialer::new([(v4(1), hung()), (v4(2), Behavior(ms(20), Ok(())))]);
    let resolution = CandidateResolution::test_queries(async { Ok(vec![v4(1), v4(2)]) }, async {
        tokio::time::sleep(ms(800)).await;
        Ok(Vec::new())
    });
    let started = Instant::now();
    let stream = connect_candidates(
        dialer.clone(),
        resolution,
        443,
        deadline(),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(started.elapsed(), ms(820));
    assert_eq!(dialer.state.calls.lock().unwrap().len(), 2);
    drop(stream);
}

#[tokio::test(start_paused = true)]
async fn a_fast_connection_drops_the_unfinished_dns_query() {
    let dns_cancelled = CancellationToken::new();
    let token = dns_cancelled.clone();
    let resolution = CandidateResolution::test_queries(async { Ok(vec![v4(1)]) }, async move {
        let _guard = token.drop_guard();
        std::future::pending::<Result<Vec<IpAddr>, TransportError>>().await
    });
    let dialer = Dialer::new([(v4(1), Behavior(ms(20), Ok(())))]);
    let started = Instant::now();
    let stream = connect_candidates(
        dialer,
        resolution,
        443,
        deadline(),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(started.elapsed(), ms(20));
    assert!(dns_cancelled.is_cancelled());
    drop(stream);
}

#[tokio::test(start_paused = true)]
async fn resolution_time_counts_against_the_overall_connect_deadline() {
    let dialer = Dialer::new([(v4(1), hung())]);
    let resolution = CandidateResolution::test_queries(
        async {
            tokio::time::sleep(Duration::from_secs(3)).await;
            Ok(vec![v4(1)])
        },
        async { Ok(Vec::new()) },
    );
    let started = Instant::now();
    let result = connect_candidates(
        dialer.clone(),
        resolution,
        443,
        deadline(),
        &CancellationToken::new(),
    )
    .await;
    assert!(matches!(
        result,
        Err(CandidateDialError::Dial(DialError::Timeout))
    ));
    assert_eq!(started.elapsed(), Duration::from_secs(10));
    assert_eq!(dialer.state.live.load(Ordering::SeqCst), 0);
}

#[tokio::test(start_paused = true)]
async fn budget_exhaustion_stops_expansion_without_killing_an_existing_attempt() {
    let dialer = Dialer::new([
        (v4(1), Behavior(ms(600), Ok(()))),
        (v6(), Behavior(Duration::ZERO, Err(DialError::Budget))),
    ]);
    let parent = CancellationToken::new();
    let stream = connect_candidates(
        dialer.clone(),
        CandidateResolution::from_addresses(vec![v4(1), v6(), v4(2)]),
        443,
        deadline(),
        &parent,
    )
    .await
    .unwrap();
    assert_eq!(dialer.state.calls.lock().unwrap().len(), 2);
    assert!(!dialer.state.tokens.lock().unwrap()[0].is_cancelled());
    drop(stream);
    assert_eq!(dialer.state.live.load(Ordering::SeqCst), 0);
}

#[tokio::test(start_paused = true)]
async fn authentication_failure_or_cancelled_attempt_ends_the_entire_race() {
    for error in [
        DialError::Rejected(401),
        DialError::Rejected(403),
        DialError::Cancelled,
    ] {
        let dialer = Dialer::new([
            (v4(1), hung()),
            (v6(), Behavior(Duration::ZERO, Err(error))),
        ]);
        let parent = CancellationToken::new();
        let result = connect_candidates(
            dialer.clone(),
            CandidateResolution::from_addresses(vec![v4(1), v6()]),
            443,
            deadline(),
            &parent,
        )
        .await;
        assert!(matches!(result, Err(CandidateDialError::Dial(value)) if value == error));
        assert_eq!(dialer.state.live.load(Ordering::SeqCst), 0);
        assert!(
            dialer
                .state
                .tokens
                .lock()
                .unwrap()
                .iter()
                .all(CancellationToken::is_cancelled)
        );
        assert!(!parent.is_cancelled());
    }
}

#[tokio::test(start_paused = true)]
async fn parent_cancellation_drops_the_pending_query_and_dial() {
    let dialer = Dialer::new([(v4(1), hung())]);
    let parent = CancellationToken::new();
    let cancelled_dns = CancellationToken::new();
    let token = cancelled_dns.clone();
    let resolution = CandidateResolution::test_queries(async { Ok(vec![v4(1)]) }, async move {
        let _guard = token.drop_guard();
        std::future::pending::<Result<Vec<IpAddr>, TransportError>>().await
    });
    let task_dialer = dialer.clone();
    let task_parent = parent.clone();
    let task = tokio::spawn(async move {
        connect_candidates(task_dialer, resolution, 443, deadline(), &task_parent).await
    });
    tokio::time::sleep(ms(300)).await;
    parent.cancel();
    assert!(matches!(
        task.await.unwrap(),
        Err(CandidateDialError::Dial(DialError::Cancelled))
    ));
    assert!(cancelled_dns.is_cancelled());
    assert_eq!(dialer.state.live.load(Ordering::SeqCst), 0);
}

#[tokio::test(start_paused = true)]
async fn failed_resolution_keeps_its_error_category_and_opens_no_tcp_socket() {
    let dialer = Dialer::new([]);
    let resolution = CandidateResolution::test_queries(
        async { Err(TransportError::Dns("v4 failed".to_owned())) },
        async { Ok(Vec::new()) },
    );
    let result = connect_candidates(
        dialer.clone(),
        resolution,
        443,
        deadline(),
        &CancellationToken::new(),
    )
    .await;
    assert!(matches!(result, Err(CandidateDialError::Resolve(_))));
    assert!(dialer.state.calls.lock().unwrap().is_empty());
}

#[tokio::test(start_paused = true)]
async fn fast_failures_refill_immediately_and_static_candidates_are_bounded() {
    let dialer = Dialer::new([]);
    let mut addresses: Vec<_> = (1..=31).map(v4).collect();
    addresses.push(v6());
    let started = Instant::now();
    let result = connect_candidates(
        dialer.clone(),
        CandidateResolution::from_addresses(addresses),
        443,
        deadline(),
        &CancellationToken::new(),
    )
    .await;
    assert!(matches!(
        result,
        Err(CandidateDialError::Dial(DialError::Refused))
    ));
    let calls = dialer.state.calls.lock().unwrap();
    assert_eq!(calls.len(), MAX_CANDIDATES);
    assert_eq!(calls[1].0, v6());
    assert!(started.elapsed() < STAGGER);
    assert!(dialer.state.peak.load(Ordering::SeqCst) <= 2);
}

#[tokio::test(start_paused = true)]
async fn the_candidate_limit_reserves_room_for_a_later_dns_family() {
    let dialer = Dialer::new([(v6(), Behavior(ms(20), Ok(())))]);
    let resolution =
        CandidateResolution::test_queries(async { Ok((1..=32).map(v4).collect()) }, async {
            tokio::time::sleep(ms(500)).await;
            Ok(vec![v6()])
        });
    let stream = connect_candidates(
        dialer.clone(),
        resolution,
        443,
        deadline(),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    let calls = dialer.state.calls.lock().unwrap();
    assert_eq!(calls.len(), MAX_CANDIDATES);
    assert_eq!(calls.last().unwrap().0, v6());
    drop(calls);
    drop(stream);
    assert_eq!(dialer.state.live.load(Ordering::SeqCst), 0);
}
