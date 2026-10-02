//! Bounded DNS-over-TCP pool over a selected stream dialer. No UDP fallback.
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Semaphore;
use tokio::time::{Instant, timeout_at};
use tokio_util::sync::CancellationToken;

use crate::tcp::{DialError, FlowClass, TcpDialer, TcpStream, TcpTarget};

const DNS_TIMEOUT: Duration = Duration::from_secs(4);
const IDLE_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_IDLE: usize = 16;

struct Entry {
    session_generation: Option<u64>,
    server: TcpTarget,
    stream: TcpStream,
    used: Instant,
    generation: Option<u64>,
}

pub(crate) struct StreamDns {
    dialer: Arc<dyn TcpDialer>,
    protector: Arc<dyn crate::SocketProtector>,
    cancellation: CancellationToken,
    admitted: Arc<Semaphore>,
    operations: Arc<Semaphore>,
    resolvers: Mutex<HashMap<TcpTarget, Weak<Semaphore>>>,
    idle: Mutex<Vec<Entry>>,
    metrics: Arc<crate::l4::L4Metrics>,
}

impl StreamDns {
    pub(crate) fn over_stack(
        channel: ts_netstack_smoltcp::netcore::Channel,
        ipv4: std::net::Ipv4Addr,
        ipv6: std::net::Ipv6Addr,
        protector: Arc<dyn crate::SocketProtector>,
        cancellation: CancellationToken,
    ) -> Self {
        Self::new(
            Arc::new(crate::tcp::StackDialer {
                channel,
                ipv4,
                ipv6,
            }),
            protector,
            cancellation,
            Arc::default(),
        )
    }
    pub(crate) fn new(
        dialer: Arc<dyn TcpDialer>,
        protector: Arc<dyn crate::SocketProtector>,
        cancellation: CancellationToken,
        metrics: Arc<crate::l4::L4Metrics>,
    ) -> Self {
        Self {
            dialer,
            protector,
            cancellation,
            admitted: Arc::new(Semaphore::new(80)),
            operations: Arc::new(Semaphore::new(16)),
            resolvers: Mutex::new(HashMap::new()),
            idle: Mutex::new(Vec::new()),
            metrics,
        }
    }

    pub(crate) async fn query(
        &self,
        server: SocketAddr,
        query: &[u8],
        deadline: Instant,
    ) -> Result<Vec<u8>, DialError> {
        self.query_target(TcpTarget::address(server), query, deadline)
            .await
    }

    pub(crate) async fn query_target(
        &self,
        server: TcpTarget,
        query: &[u8],
        deadline: Instant,
    ) -> Result<Vec<u8>, DialError> {
        crate::split_dns::validate_query_bytes(query).map_err(|_| DialError::Protocol)?;
        let _admitted = self
            .admitted
            .clone()
            .try_acquire_owned()
            .map_err(|_| DialError::Budget)?;
        let deadline = deadline.min(Instant::now() + DNS_TIMEOUT);
        let generation = self.protector.network_generation();
        let resolver = {
            let mut resolvers = self.resolvers.lock().unwrap_or_else(|e| e.into_inner());
            resolvers.retain(|_, value| value.strong_count() != 0);
            if let Some(value) = resolvers.get(&server).and_then(Weak::upgrade) {
                value
            } else {
                if resolvers.len() >= 80 {
                    return Err(DialError::Budget);
                }
                let value = Arc::new(Semaphore::new(2));
                resolvers.insert(server.clone(), Arc::downgrade(&value));
                value
            }
        };
        let work = async {
            // Acquire the resolver-local slot before the global active slot;
            // a slow resolver cannot occupy all sixteen active operations.
            let _resolver = resolver
                .acquire_owned()
                .await
                .map_err(|_| DialError::Closed)?;
            let _operation = self
                .operations
                .clone()
                .acquire_owned()
                .await
                .map_err(|_| DialError::Closed)?;
            let reused = {
                let mut idle = self.idle.lock().unwrap_or_else(|e| e.into_inner());
                idle.retain(|e| {
                    e.used.elapsed() < IDLE_TIMEOUT
                        && e.generation == generation
                        && e.session_generation == self.dialer.session_generation()
                });
                let result = idle
                    .iter()
                    .position(|e| e.server == server)
                    .map(|i| idle.swap_remove(i));
                // Don't let idle streams consume all 16 DNS slots when a new
                // resolver is requested. Only idle, exclusively-owned I/O is evicted.
                if result.is_none() {
                    idle.clear();
                }
                result
            };
            let was_reused = reused.is_some();
            let mut stream = match reused {
                Some(entry) => entry.stream,
                None => self.dial(server.clone(), deadline).await?,
            };
            let response = match exchange(&mut stream, query).await {
                Ok(response) => response,
                Err(_) if was_reused => {
                    drop(stream);
                    stream = self.dial(server.clone(), deadline).await?;
                    exchange(&mut stream, query).await?
                }
                Err(error) => return Err(error),
            };
            crate::split_dns::validate_response_bytes(query, &response)
                .map_err(|_| DialError::Protocol)?;
            // A truncated TCP answer cannot satisfy the question. Discard the
            // stream instead of pooling an incomplete response as success.
            if response[2] & 0x02 != 0 {
                return Err(DialError::Protocol);
            }
            if self.protector.network_generation() != generation {
                return Err(DialError::Closed);
            }
            let session_generation = stream.session_generation();
            if session_generation != self.dialer.session_generation() {
                return Err(DialError::Closed);
            }
            let mut idle = self.idle.lock().unwrap_or_else(|e| e.into_inner());
            if idle.len() < MAX_IDLE && idle.iter().filter(|e| e.server == server).count() < 2 {
                idle.push(Entry {
                    session_generation,
                    server,
                    stream,
                    used: Instant::now(),
                    generation,
                });
            }
            Ok(response)
        };
        let result = tokio::select! {
            _ = self.cancellation.cancelled() => Err(DialError::Cancelled),
            result = timeout_at(deadline, work) => result.unwrap_or(Err(DialError::Timeout)),
        };
        self.metrics.update(|m| match result {
            Ok(_) => m.dns_successes += 1,
            Err(error) => {
                m.dns_failures += 1;
                if error == DialError::Timeout {
                    m.dns_timeouts += 1;
                }
            }
        });
        result
    }

    async fn dial(&self, server: TcpTarget, deadline: Instant) -> Result<TcpStream, DialError> {
        self.dialer
            .connect(server, deadline, &self.cancellation, FlowClass::Dns)
            .await
    }

    pub(crate) fn prune(&self) {
        let generation = self.protector.network_generation();
        self.idle
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|e| {
                e.used.elapsed() < IDLE_TIMEOUT
                    && e.generation == generation
                    && e.session_generation == self.dialer.session_generation()
            });
    }

    pub(crate) fn clear(&self) {
        self.idle.lock().unwrap_or_else(|e| e.into_inner()).clear();
        self.resolvers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
    }
}

async fn exchange(stream: &mut TcpStream, query: &[u8]) -> Result<Vec<u8>, DialError> {
    let length = u16::try_from(query.len()).map_err(|_| DialError::Protocol)?;
    stream
        .write_u16(length)
        .await
        .map_err(|_| DialError::Closed)?;
    stream
        .write_all(query)
        .await
        .map_err(|_| DialError::Closed)?;
    stream.flush().await.map_err(|_| DialError::Closed)?;
    let length = stream.read_u16().await.map_err(|_| DialError::Closed)? as usize;
    if length < 12 {
        return Err(DialError::Protocol);
    }
    let mut response = vec![0u8; length];
    stream
        .read_exact(&mut response)
        .await
        .map_err(|_| DialError::Closed)?;
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::io;
    use std::pin::Pin;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::task::{Context, Poll};
    use tokio::io::{AsyncRead, AsyncWrite, DuplexStream, ReadBuf};
    use tokio::sync::Notify;

    struct MemoryStream {
        stream: DuplexStream,
        live: Arc<AtomicUsize>,
    }
    impl crate::tcp::TcpIo for MemoryStream {
        fn local_addr(&self) -> io::Result<SocketAddr> {
            Ok("127.0.0.1:12345".parse().unwrap())
        }
    }
    impl AsyncRead for MemoryStream {
        fn poll_read(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &mut ReadBuf<'_>,
        ) -> Poll<io::Result<()>> {
            Pin::new(&mut self.stream).poll_read(cx, buf)
        }
    }
    impl AsyncWrite for MemoryStream {
        fn poll_write(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &[u8],
        ) -> Poll<io::Result<usize>> {
            Pin::new(&mut self.stream).poll_write(cx, buf)
        }
        fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Pin::new(&mut self.stream).poll_flush(cx)
        }
        fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Pin::new(&mut self.stream).poll_shutdown(cx)
        }
    }
    impl Drop for MemoryStream {
        fn drop(&mut self) {
            self.live.fetch_sub(1, Ordering::SeqCst);
        }
    }
    #[derive(Default)]
    struct MemoryDialer {
        targets: Mutex<Vec<TcpTarget>>,
        live: Arc<AtomicUsize>,
        queried: Arc<Notify>,
        silent: bool,
    }
    #[async_trait]
    impl TcpDialer for MemoryDialer {
        async fn connect(
            &self,
            target: TcpTarget,
            _: Instant,
            cancel: &CancellationToken,
            class: FlowClass,
        ) -> Result<TcpStream, DialError> {
            assert_eq!(class, FlowClass::Dns);
            self.targets.lock().unwrap().push(target);
            let (stream, mut peer) = tokio::io::duplex(4096);
            let silent = self.silent;
            let queried = self.queried.clone();
            let cancel = cancel.clone();
            tokio::spawn(async move {
                let work = async {
                    loop {
                        let Ok(length) = peer.read_u16().await else {
                            return;
                        };
                        let mut query = vec![0; usize::from(length)];
                        if peer.read_exact(&mut query).await.is_err() {
                            return;
                        }
                        queried.notify_one();
                        if silent {
                            std::future::pending::<()>().await;
                        }
                        query[2..4].copy_from_slice(&[0x81, 0x80]);
                        if peer.write_u16(query.len() as u16).await.is_err()
                            || peer.write_all(&query).await.is_err()
                        {
                            return;
                        }
                    }
                };
                tokio::select! { _ = cancel.cancelled() => {}, _ = work => {} }
            });
            self.live.fetch_add(1, Ordering::SeqCst);
            Ok(Box::new(MemoryStream {
                stream,
                live: self.live.clone(),
            }))
        }
    }
    fn query() -> Vec<u8> {
        let mut query = vec![0x12, 0x34, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0];
        query.extend_from_slice(b"\x07example\x04test\0\0\x01\0\x01");
        query
    }
    fn pool(dialer: Arc<MemoryDialer>, cancel: CancellationToken) -> Arc<StreamDns> {
        Arc::new(StreamDns::new(
            dialer,
            Arc::new(crate::socket::NoopSocketProtector),
            cancel,
            Arc::default(),
        ))
    }
    #[tokio::test]
    async fn domain_resolver_targets_reuse_only_their_own_streams() {
        let dialer = Arc::new(MemoryDialer::default());
        let cancel = CancellationToken::new();
        let dns = pool(dialer.clone(), cancel.clone());
        let first = TcpTarget::new("resolver-one.test", 53).unwrap();
        let second = TcpTarget::new("resolver-two.test", 53).unwrap();
        let address: SocketAddr = "192.0.2.53:53".parse().unwrap();
        for target in [&first, &first, &second, &second] {
            let response = dns
                .query_target(target.clone(), &query(), Instant::now() + DNS_TIMEOUT)
                .await
                .unwrap();
            assert_eq!(&response[2..4], &[0x81, 0x80]);
        }
        for _ in 0..2 {
            dns.query(address, &query(), Instant::now() + DNS_TIMEOUT)
                .await
                .unwrap();
        }
        assert_eq!(
            *dialer.targets.lock().unwrap(),
            vec![first, second, TcpTarget::address(address)]
        );
        assert_eq!(dialer.live.load(Ordering::SeqCst), 1);
        dns.clear();
        assert_eq!(dialer.live.load(Ordering::SeqCst), 0);
        cancel.cancel();
    }
    #[tokio::test]
    async fn cancelled_domain_dns_releases_stream_and_all_permits() {
        let dialer = Arc::new(MemoryDialer {
            silent: true,
            ..Default::default()
        });
        let cancel = CancellationToken::new();
        let dns = pool(dialer.clone(), cancel.clone());
        let operation = {
            let dns = dns.clone();
            tokio::spawn(async move {
                dns.query_target(
                    TcpTarget::new("resolver.test", 53).unwrap(),
                    &query(),
                    Instant::now() + DNS_TIMEOUT,
                )
                .await
            })
        };
        tokio::time::timeout(Duration::from_secs(1), dialer.queried.notified())
            .await
            .unwrap();
        cancel.cancel();
        assert_eq!(operation.await.unwrap(), Err(DialError::Cancelled));
        assert_eq!(dialer.live.load(Ordering::SeqCst), 0);
        assert!(dns.idle.lock().unwrap().is_empty());
        assert_eq!(dns.admitted.available_permits(), 80);
        assert_eq!(dns.operations.available_permits(), 16);
        assert!(
            dns.resolvers
                .lock()
                .unwrap()
                .values()
                .all(|entry| entry.strong_count() == 0)
        );
    }
}
