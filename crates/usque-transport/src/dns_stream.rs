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
    server: SocketAddr,
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
    resolvers: Mutex<HashMap<SocketAddr, Weak<Semaphore>>>,
    idle: Mutex<Vec<Entry>>,
    metrics: Arc<crate::l4::L4Metrics>,
}

impl StreamDns {
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
                resolvers.insert(server, Arc::downgrade(&value));
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
                None => self.dial(server, deadline).await?,
            };
            let response = match exchange(&mut stream, query).await {
                Ok(response) => response,
                Err(_) if was_reused => {
                    drop(stream);
                    stream = self.dial(server, deadline).await?;
                    exchange(&mut stream, query).await?
                }
                Err(error) => return Err(error),
            };
            crate::split_dns::validate_response_bytes(query, &response)
                .map_err(|_| DialError::Protocol)?;
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

    async fn dial(&self, server: SocketAddr, deadline: Instant) -> Result<TcpStream, DialError> {
        self.dialer
            .connect(
                TcpTarget::address(server),
                deadline,
                &self.cancellation,
                FlowClass::Dns,
            )
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
