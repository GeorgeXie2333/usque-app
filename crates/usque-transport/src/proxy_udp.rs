//! Bounded per-client datagram associations for stack and SOCKS5 final exits.
use crate::tcp::{DialError, FlowClass, TcpTarget};
use async_trait::async_trait;
use bytes::Bytes;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use tokio::io::AsyncReadExt;
use tokio::sync::{Mutex, mpsc};
use tokio::time::{Instant, timeout_at};
use tokio_util::sync::CancellationToken;
use tokio_util::task::AbortOnDropHandle;
use ts_netstack_smoltcp::netcore::Channel;
use ts_netstack_smoltcp::netsock::UdpSocket;

#[async_trait]
pub(crate) trait UdpFactory: Send + Sync {
    async fn open(
        &self,
        cancel: &CancellationToken,
        deadline: Instant,
    ) -> Result<Arc<dyn UdpAssociation>, DialError>;
}
#[async_trait]
pub(crate) trait UdpAssociation: Send + Sync {
    fn accounts_traffic(&self) -> bool {
        true
    }
    async fn prepare(&self) -> Result<(), DialError> {
        Ok(())
    }
    async fn send(&self, target: &TcpTarget, payload: &[u8]) -> Result<(), DialError>;
    async fn recv(&self) -> Result<(TcpTarget, Bytes), DialError>;
}

pub(crate) struct DirectOnly;
#[async_trait]
impl UdpAssociation for DirectOnly {
    async fn send(&self, _target: &TcpTarget, _payload: &[u8]) -> Result<(), DialError> {
        Err(DialError::Rejected(7))
    }
    async fn recv(&self) -> Result<(TcpTarget, Bytes), DialError> {
        std::future::pending().await
    }
}
#[async_trait]
impl UdpFactory for DirectOnly {
    async fn open(
        &self,
        _cancel: &CancellationToken,
        _deadline: Instant,
    ) -> Result<Arc<dyn UdpAssociation>, DialError> {
        Ok(Arc::new(Self))
    }
}

/// DNS-only associations never await an upstream UDP handshake. Only a send
/// initializes this bounded, shared attempt; the receiver waits for its result.
pub(crate) struct LazyAssociation {
    factory: Arc<dyn UdpFactory>,
    cancel: CancellationToken,
    opened: tokio::sync::OnceCell<Result<Arc<dyn UdpAssociation>, DialError>>,
    changed: tokio::sync::Notify,
}
impl LazyAssociation {
    pub(crate) fn new(factory: Arc<dyn UdpFactory>, cancel: CancellationToken) -> Self {
        Self {
            factory,
            cancel,
            opened: tokio::sync::OnceCell::new(),
            changed: tokio::sync::Notify::new(),
        }
    }
}
#[async_trait]
impl UdpAssociation for LazyAssociation {
    fn accounts_traffic(&self) -> bool {
        self.opened
            .get()
            .and_then(|result| result.as_ref().ok())
            .is_none_or(|association| association.accounts_traffic())
    }
    async fn prepare(&self) -> Result<(), DialError> {
        let result = self
            .opened
            .get_or_init(|| {
                self.factory.open(
                    &self.cancel,
                    Instant::now() + std::time::Duration::from_secs(10),
                )
            })
            .await;
        self.changed.notify_waiters();
        match result {
            Ok(_) => Ok(()),
            Err(error) => Err(*error),
        }
    }
    async fn send(&self, target: &TcpTarget, payload: &[u8]) -> Result<(), DialError> {
        self.prepare().await?;
        self.opened
            .get()
            .expect("prepared association")
            .as_ref()
            .map_err(|error| *error)?
            .send(target, payload)
            .await
    }
    async fn recv(&self) -> Result<(TcpTarget, Bytes), DialError> {
        loop {
            let notified = self.changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if let Some(result) = self.opened.get() {
                match result {
                    Ok(association) => return association.recv().await,
                    // These disable ordinary UDP while DNS remains usable.
                    Err(
                        error @ (DialError::Protocol
                        | DialError::Rejected(401 | 403 | 407)
                        | DialError::Cancelled),
                    ) => return Err(*error),
                    Err(_) => self.cancel.cancelled().await,
                }
                return Err(DialError::Cancelled);
            }
            tokio::select! { _ = self.cancel.cancelled() => return Err(DialError::Cancelled), _ = notified => {} }
        }
    }
}

pub(crate) struct StackFactory {
    channel: Channel,
    ipv4: Ipv4Addr,
    ipv6: Ipv6Addr,
}
impl StackFactory {
    pub(crate) fn shared(channel: Channel, ipv4: Ipv4Addr, ipv6: Ipv6Addr) -> Arc<dyn UdpFactory> {
        Arc::new(Self {
            channel,
            ipv4,
            ipv6,
        })
    }
}
struct StackAssociation {
    v4: Arc<UdpSocket>,
    v6: Arc<UdpSocket>,
    received: Mutex<mpsc::Receiver<Result<(TcpTarget, Bytes), DialError>>>,
    cancel: CancellationToken,
    readers: Vec<AbortOnDropHandle<()>>,
}
#[async_trait]
impl UdpFactory for StackFactory {
    async fn open(
        &self,
        cancel: &CancellationToken,
        deadline: Instant,
    ) -> Result<Arc<dyn UdpAssociation>, DialError> {
        use ts_netstack_smoltcp::CreateSocket;
        let bind = async {
            let v4 = Arc::new(
                self.channel
                    .udp_bind(SocketAddr::new(
                        self.ipv4.into(),
                        crate::port_allocator::next_udp_port(),
                    ))
                    .await
                    .map_err(|_| DialError::Closed)?,
            );
            let v6 = Arc::new(
                self.channel
                    .udp_bind(SocketAddr::new(
                        self.ipv6.into(),
                        crate::port_allocator::next_udp_port(),
                    ))
                    .await
                    .map_err(|_| DialError::Closed)?,
            );
            Ok::<_, DialError>((v4, v6))
        };
        let (v4, v6) = tokio::select! { _ = cancel.cancelled() => return Err(DialError::Cancelled), r = timeout_at(deadline, bind) => r.map_err(|_| DialError::Timeout)?? };
        let (tx, rx) = mpsc::channel(16);
        let token = cancel.child_token();
        let mut readers = vec![];
        for socket in [v4.clone(), v6.clone()] {
            let tx = tx.clone();
            let token = token.clone();
            readers.push(AbortOnDropHandle::new(tokio::spawn(async move {
                let work = async {
                    loop {
                        let result = socket
                            .recv_from_bytes()
                            .await
                            .map(|(a, b)| (TcpTarget::address(a), b))
                            .map_err(|_| DialError::Closed);
                        let failed = result.is_err();
                        if tx.send(result).await.is_err() || failed {
                            break;
                        }
                    }
                };
                tokio::select! { _ = token.cancelled() => {}, _ = work => {} }
            })));
        }
        Ok(Arc::new(StackAssociation {
            v4,
            v6,
            received: Mutex::new(rx),
            cancel: token,
            readers,
        }))
    }
}
#[async_trait]
impl UdpAssociation for StackAssociation {
    async fn send(&self, target: &TcpTarget, payload: &[u8]) -> Result<(), DialError> {
        let remote = target.socket_address().ok_or(DialError::InvalidTarget)?;
        let socket = if remote.is_ipv4() { &self.v4 } else { &self.v6 };
        tokio::select! { _ = self.cancel.cancelled() => Err(DialError::Closed), r = socket.send_to(remote, payload) => r.map_err(|_| DialError::Closed) }
    }
    async fn recv(&self) -> Result<(TcpTarget, Bytes), DialError> {
        tokio::select! { _ = self.cancel.cancelled() => Err(DialError::Closed), r = async { self.received.lock().await.recv().await } => r.unwrap_or(Err(DialError::Closed)) }
    }
}
impl Drop for StackAssociation {
    fn drop(&mut self) {
        self.cancel.cancel();
        self.readers.clear();
    }
}

pub(crate) struct SocksFactory(pub(crate) Arc<crate::proxy_exit::ProxyDialer>);
struct SocksAssociation {
    udp: Arc<crate::internal_network::InternalUdp>,
    received: Mutex<mpsc::Receiver<Result<(TcpTarget, Bytes), DialError>>>,
    reader: AbortOnDropHandle<()>,
    cancel: CancellationToken,
    _permit: tokio::sync::OwnedSemaphorePermit,
    _lease: crate::l4::stream::BufferLease,
    targets: Mutex<std::collections::HashMap<String, Instant>>,
}
#[async_trait]
impl UdpFactory for SocksFactory {
    async fn open(
        &self,
        cancel: &CancellationToken,
        deadline: Instant,
    ) -> Result<Arc<dyn UdpAssociation>, DialError> {
        let proxy = &self.0;
        let _pending = proxy
            .pending
            .clone()
            .try_acquire_owned()
            .map_err(|_| DialError::Budget)?;
        if !proxy.admitted.load(std::sync::atomic::Ordering::Acquire)
            || proxy.status.borrow().proxy_udp.as_deref() == Some("unavailable")
        {
            return Err(DialError::Rejected(7));
        }
        let permit = proxy
            .active
            .clone()
            .try_acquire_owned()
            .map_err(|_| DialError::Budget)?;
        let lease = proxy
            .budget
            .reserve_admission(16 * 16 * 1024 * 2)
            .ok_or(DialError::Budget)?;
        let work = async {
            let (mut control, address) =
                proxy.server(deadline, cancel, FlowClass::Business).await?;
            proxy.authenticate(&mut control, deadline, cancel).await?;
            let (host, port) = crate::proxy_exit::socks_command(
                &mut control,
                3,
                &crate::tcp::TcpTarget::address("0.0.0.0:0".parse().expect("constant")),
            )
            .await
            .map_err(|error| match error {
                // A command refusal only denies UDP. TCP CONNECT (including
                // DNS) remains independent; authentication and transport errors
                // above must still fail rather than become a DNS-only relay.
                DialError::Refused => DialError::Rejected(7),
                error => error,
            })?;
            let host = if host
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_unspecified())
            {
                address.ip().to_string()
            } else {
                host
            };
            let endpoint = usque_core::chain_exit::Endpoint::parse(&host, &port.to_string(), 0)
                .map_err(|_| DialError::Protocol)?;
            let relay = proxy
                .network
                .resolve_endpoint(&endpoint, None, cancel)
                .await?;
            let udp = Arc::new(proxy.network.bind_udp(relay, cancel).await?);
            Ok::<_, DialError>((control, udp))
        };
        let result = tokio::select! {
            _ = cancel.cancelled() => Err(DialError::Cancelled),
            _ = proxy.cancellation.cancelled() => Err(DialError::Closed),
            r = timeout_at(deadline, work) => r.map_err(|_| DialError::Timeout)?,
        };
        let (mut control, udp) = match result {
            Ok(r) => r,
            Err(DialError::Rejected(7)) => {
                proxy
                    .status
                    .send_modify(|s| s.proxy_udp = Some("unavailable".into()));
                return Err(DialError::Rejected(7));
            }
            Err(e) => return Err(e),
        };
        proxy
            .status
            .send_modify(|s| s.proxy_udp = Some("available".into()));
        let token = proxy.cancellation.child_token();
        let guard = token.clone().drop_guard();
        let (tx, rx) = mpsc::channel(16);
        let reader_udp = udp.clone();
        let reader_token = token.clone();
        let caller = cancel.clone();
        let reader = AbortOnDropHandle::new(tokio::spawn(async move {
            loop {
                let result = tokio::select! {
                    _ = reader_token.cancelled() => break,
                    _ = caller.cancelled() => break,
                    _ = control.read_u8() => break,
                    r = reader_udp.recv() => r,
                };
                match result {
                    Ok(packet) => {
                        if let Ok(value) = decode(&packet) {
                            tokio::select! { _ = reader_token.cancelled() => break, _ = caller.cancelled() => break, r = tx.send(Ok(value)) => if r.is_err() { break; } }
                        }
                    }
                    Err(_) => break,
                }
            }
            reader_token.cancel();
        }));
        guard.disarm();
        Ok(Arc::new(SocksAssociation {
            udp,
            received: Mutex::new(rx),
            reader,
            cancel: token,
            _permit: permit,
            _lease: lease,
            targets: Mutex::default(),
        }))
    }
}
#[async_trait]
impl UdpAssociation for SocksAssociation {
    fn accounts_traffic(&self) -> bool {
        false
    }
    async fn send(&self, target: &TcpTarget, payload: &[u8]) -> Result<(), DialError> {
        let mut packet = vec![0, 0, 0];
        crate::proxy_exit::encode_target(target, &mut packet);
        if packet.len() + payload.len() > 16 * 1024 - 48 {
            return Err(DialError::Protocol);
        }
        {
            let mut targets = self.targets.lock().await;
            targets.retain(|_, t| t.elapsed() < std::time::Duration::from_secs(60));
            if targets.len() >= 256 && !targets.contains_key(target.authority()) {
                return Err(DialError::Budget);
            }
            targets.insert(target.authority().into(), Instant::now());
        }
        packet.extend_from_slice(payload);
        tokio::select! { _ = self.cancel.cancelled() => Err(DialError::Closed), r = self.udp.send(&packet) => r }
    }
    async fn recv(&self) -> Result<(TcpTarget, Bytes), DialError> {
        tokio::select! { _ = self.cancel.cancelled() => Err(DialError::Closed), r = async { self.received.lock().await.recv().await } => r.unwrap_or(Err(DialError::Closed)) }
    }
}
impl Drop for SocksAssociation {
    fn drop(&mut self) {
        self.cancel.cancel();
        self.reader.abort();
    }
}

pub(crate) fn decode(packet: &[u8]) -> Result<(TcpTarget, Bytes), DialError> {
    if packet.len() > 16 * 1024 - 48 || packet.get(..3) != Some(&[0, 0, 0]) {
        return Err(DialError::Protocol);
    }
    let kind = *packet.get(3).ok_or(DialError::Protocol)?;
    let (start, size) = match kind {
        1 => (4, 4),
        4 => (4, 16),
        3 => (5, usize::from(*packet.get(4).ok_or(DialError::Protocol)?)),
        _ => return Err(DialError::Protocol),
    };
    let body = packet.get(start..start + size).ok_or(DialError::Protocol)?;
    let port = packet
        .get(start + size..start + size + 2)
        .ok_or(DialError::Protocol)?;
    let port = u16::from_be_bytes([port[0], port[1]]);
    let host =
        match kind {
            1 => Ipv4Addr::from(<[u8; 4]>::try_from(body).map_err(|_| DialError::Protocol)?)
                .to_string(),
            4 => Ipv6Addr::from(<[u8; 16]>::try_from(body).map_err(|_| DialError::Protocol)?)
                .to_string(),
            _ => std::str::from_utf8(body)
                .map_err(|_| DialError::Protocol)?
                .to_owned(),
        };
    Ok((
        TcpTarget::new(&host, port)?,
        Bytes::copy_from_slice(&packet[start + size + 2..]),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    #[test]
    fn datagrams_round_trip_all_targets_and_reject_fragments() {
        for host in ["192.0.2.1", "2001:db8::1", "example.test"] {
            let target = TcpTarget::new(host, 443).unwrap();
            let mut bytes = vec![0, 0, 0];
            crate::proxy_exit::encode_target(&target, &mut bytes);
            bytes.extend_from_slice(b"payload");
            let (decoded, payload) = decode(&bytes).unwrap();
            assert_eq!(decoded, target);
            assert_eq!(payload, b"payload"[..]);
            bytes[2] = 1;
            assert!(decode(&bytes).is_err());
        }
    }

    #[test]
    fn target_encoding_accepts_the_maximum_domain_length() {
        let host = [
            "a".repeat(63),
            "b".repeat(63),
            "c".repeat(63),
            "d".repeat(61),
        ]
        .join(".");
        assert_eq!(host.len(), 253);
        let target = TcpTarget::new(&host, 443).unwrap();
        let mut encoded = vec![0, 0, 0];
        crate::proxy_exit::encode_target(&target, &mut encoded);
        assert_eq!(&encoded[3..5], &[3, 253]);
        assert_eq!(decode(&encoded).unwrap().0, target);
        assert_eq!(
            TcpTarget::new(&format!("{host}d"), 443),
            Err(DialError::InvalidTarget)
        );
    }

    #[test]
    fn target_encoding_preserves_zero_port_for_udp_associate() {
        let target = TcpTarget::address("0.0.0.0:0".parse().unwrap());
        let mut encoded = Vec::new();
        crate::proxy_exit::encode_target(&target, &mut encoded);
        assert_eq!(encoded, [1, 0, 0, 0, 0, 0, 0]);
    }

    proptest! {
        #[test]
        fn arbitrary_datagrams_are_bounded_and_never_panic(bytes in prop::collection::vec(any::<u8>(),0..17000)) {
            if let Ok((target,body)) = decode(&bytes) {
                prop_assert!(body.len() < 16384);
                prop_assert!(target.host_port().1 > 0);
                let mut encoded = vec![0,0,0];
                crate::proxy_exit::encode_target(&target,&mut encoded);
                encoded.extend_from_slice(&body);
                prop_assert_eq!(decode(&encoded).unwrap(),(target,body));
            }
        }
    }
}
