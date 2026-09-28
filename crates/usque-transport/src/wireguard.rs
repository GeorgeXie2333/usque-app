//! WireGuard packets in memory only. WARP owns every network operation.
use boringtun::noise::{Tunn, TunnResult};
use bytes::Bytes;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use tokio_util::task::AbortOnDropHandle;
use usque_core::chain_exit::WireGuardProfile;
use usque_openvpn::{Error, Event, NetworkConfig};
#[cfg(test)]
mod benchmark;
#[cfg(test)]
mod tests;

const GENERATION: u64 = 1;
const MAX_PACKET: usize = 65535;
// Two input queues total 48 packets; two output queues plus pending packets
// total 48. This leaves room for private UDP readers within the old budget.
const PACKET_QUEUE: usize = 23;
#[derive(Clone)]
pub(crate) struct Input {
    encrypted: mpsc::Sender<Bytes>,
    plaintext: mpsc::Sender<Bytes>,
    control: mpsc::Sender<()>,
    cancel: CancellationToken,
}
impl Input {
    pub(crate) async fn push(&self, kind: u8, generation: u64, bytes: &[u8]) -> Result<(), Error> {
        self.push_owned(kind, generation, Bytes::copy_from_slice(bytes))
            .await
    }
    pub(crate) async fn push_owned(
        &self,
        kind: u8,
        generation: u64,
        bytes: Bytes,
    ) -> Result<(), Error> {
        if generation != GENERATION || bytes.len() > MAX_PACKET {
            return Err(Error::InvalidPacket);
        }
        if kind == 4 {
            self.cancel.cancel();
            return Ok(());
        }
        tokio::select! {
            biased;
            _ = self.cancel.cancelled() => Err(Error::Closed),
            result = async {
                match kind {
                    1 => self.encrypted.send(bytes).await.map_err(|_| Error::Closed),
                    2 => self.plaintext.send(bytes).await.map_err(|_| Error::Closed),
                    3 => self.control.send(()).await.map_err(|_| Error::Closed),
                    _ => Err(Error::InvalidPacket),
                }
            } => result,
        }
    }
    pub(crate) fn stop(&self) {
        self.cancel.cancel();
    }
}
pub(crate) struct Session {
    input: Input,
    events: mpsc::Receiver<Event>,
    transport: Option<mpsc::Receiver<Event>>,
    packets: Option<mpsc::Receiver<Event>>,
    task: Option<AbortOnDropHandle<()>>,
}
impl Session {
    pub(crate) fn start(profile: WireGuardProfile) -> Self {
        let cancel = CancellationToken::new();
        let (encrypted, encrypted_rx) = mpsc::channel(PACKET_QUEUE + 1);
        let (plaintext, plaintext_rx) = mpsc::channel(PACKET_QUEUE + 1);
        let (control, control_rx) = mpsc::channel(2);
        let (events, output) = mpsc::channel(4);
        let (network, transport) = mpsc::channel(PACKET_QUEUE);
        let (ip, packets) = mpsc::channel(PACKET_QUEUE);
        let input = Input {
            encrypted,
            plaintext,
            control,
            cancel: cancel.clone(),
        };
        let task = tokio::spawn(async move {
            let work = run(
                profile,
                encrypted_rx,
                plaintext_rx,
                control_rx,
                &events,
                &network,
                &ip,
            );
            tokio::select! { biased; _ = cancel.cancelled() => {}, _ = work => {} }
            cancel.cancel();
        });
        Self {
            input,
            events: output,
            transport: Some(transport),
            packets: Some(packets),
            task: Some(AbortOnDropHandle::new(task)),
        }
    }
    pub(crate) fn input(&self) -> Input {
        self.input.clone()
    }
    pub(crate) fn split_packet_outputs(
        &mut self,
    ) -> Option<(mpsc::Receiver<Event>, mpsc::Receiver<Event>)> {
        Some((self.transport.take()?, self.packets.take()?))
    }
    pub(crate) async fn next_event(&mut self) -> Result<Event, Error> {
        async fn receive(receiver: &mut Option<mpsc::Receiver<Event>>) -> Option<Event> {
            match receiver {
                Some(r) => r.recv().await,
                None => std::future::pending().await,
            }
        }
        tokio::select! {
            biased;
            event = self.events.recv() => event.ok_or(Error::Closed),
            event = receive(&mut self.transport) => event.ok_or(Error::Closed),
            event = receive(&mut self.packets) => event.ok_or(Error::Closed),
        }
    }
    pub(crate) async fn shutdown(&mut self) -> Result<(), Error> {
        self.input.stop();
        if let Some(task) = self.task.take() {
            task.await.map_err(|_| Error::Worker)?;
        }
        Ok(())
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        self.input.stop();
    }
}

#[derive(Default)]
struct HandshakeWatch {
    age: Option<Duration>,
    pending: Option<Instant>,
}
impl HandshakeWatch {
    fn initiated(&mut self, now: Instant) {
        self.pending.get_or_insert(now);
    }
    fn observe(&mut self, age: Option<Duration>) {
        if age.is_some_and(|current| self.age.is_none_or(|previous| current < previous)) {
            self.pending = None;
        }
        self.age = age;
    }
    fn expired(&self, now: Instant) -> bool {
        self.pending
            .is_some_and(|started| now.saturating_duration_since(started) > Duration::from_secs(35))
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "the actor owns separate bounded control, encrypted and plaintext channels in each direction"
)]
async fn run(
    profile: WireGuardProfile,
    mut encrypted: mpsc::Receiver<Bytes>,
    mut plaintext: mpsc::Receiver<Bytes>,
    mut control: mpsc::Receiver<()>,
    events: &mpsc::Sender<Event>,
    network: &mpsc::Sender<Event>,
    ip: &mpsc::Sender<Event>,
) -> Result<(), Error> {
    let mut tunnel = Tunn::new(
        (*profile.private_key).into(),
        profile.public_key.into(),
        profile.preshared_key.as_ref().map(|key| **key),
        profile.keepalive,
        1,
        None,
    );
    events
        .try_send(Event::Dial {
            generation: GENERATION,
        })
        .map_err(|_| Error::Closed)?;
    let mut buffer = vec![0u8; MAX_PACKET + 256];
    let mut timer = tokio::time::interval(Duration::from_millis(100));
    timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut connected = false;
    let mut transport_ready = false;
    let mut watch = HandshakeWatch::default();
    let mut pending_network = None;
    let mut pending_ip = None;
    let mut drain = false;
    let mut batch = 0usize;
    loop {
        let mut draining_result = false;
        let result = tokio::select! {
            permit = network.reserve(), if pending_network.is_some() => {
                permit.map_err(|_| Error::Closed)?.send(pending_network.take().expect("pending output"));
                continue;
            },
            permit = ip.reserve(), if pending_ip.is_some() => {
                permit.map_err(|_| Error::Closed)?.send(pending_ip.take().expect("pending output"));
                continue;
            },
            command = control.recv(), if !transport_ready => {
                command.ok_or(Error::Closed)?;
                transport_ready = true;
                tunnel.format_handshake_initiation(&mut buffer, false)
            },
            bytes = encrypted.recv(), if transport_ready && pending_network.is_none() && pending_ip.is_none() && !drain => {
                let bytes = bytes.ok_or(Error::Closed)?;
                draining_result = true;
                tunnel.decapsulate(None, &bytes, &mut buffer)
            },
            bytes = plaintext.recv(), if connected && pending_network.is_none() && !drain => {
                let bytes = bytes.ok_or(Error::Closed)?;
                if bytes.len() > usize::from(profile.mtu) || packet_address(&bytes, false).is_none_or(|address| !profile.allows(address)) { continue; }
                tunnel.encapsulate(&bytes, &mut buffer)
            },
            _ = std::future::ready(()), if drain && pending_network.is_none() && pending_ip.is_none() => {
                draining_result = true;
                tunnel.decapsulate(None, &[], &mut buffer)
            },
            _ = timer.tick(), if transport_ready => {
                // Watchdog progress is independent of business output capacity.
                // The core may emit a packet, so defer that part until its
                // bounded pending slot is available.
                if watch.expired(Instant::now()) { return Err(Error::Closed); }
                if pending_network.is_some() || drain { continue; }
                tunnel.update_timers(&mut buffer)
            },
        };
        drain = draining_result && matches!(result, TunnResult::WriteToNetwork(_));
        if connected
            && matches!(&result, TunnResult::WriteToNetwork(bytes) if bytes.starts_with(&[1, 0, 0, 0]))
        {
            watch.initiated(Instant::now());
        }
        match result {
            TunnResult::WriteToNetwork(bytes) => {
                pending_network = Some(Event::TransportPacket {
                    generation: GENERATION,
                    packet: Bytes::copy_from_slice(bytes),
                })
            }
            TunnResult::WriteToTunnelV4(bytes, _) | TunnResult::WriteToTunnelV6(bytes, _) => {
                if packet_address(bytes, true).is_some_and(|address| profile.allows(address)) {
                    pending_ip = Some(Event::IpPacket {
                        generation: GENERATION,
                        packet: Bytes::copy_from_slice(bytes),
                    });
                }
            }
            TunnResult::Err(_) | TunnResult::Done => {}
        }
        watch.observe(tunnel.time_since_last_handshake());
        if !connected && watch.age.is_some() {
            connected = true;
            let config = NetworkConfig {
                ipv4: profile.addresses.iter().find_map(|net| {
                    if let IpAddr::V4(ip) = net.addr() {
                        Some(ip)
                    } else {
                        None
                    }
                }),
                ipv6: profile.addresses.iter().find_map(|net| {
                    if let IpAddr::V6(ip) = net.addr() {
                        Some(ip)
                    } else {
                        None
                    }
                }),
                mtu: profile.mtu,
                dns_servers: profile.dns_servers.clone(),
            };
            events
                .try_send(Event::Network {
                    generation: GENERATION,
                    config,
                })
                .map_err(|_| Error::Closed)?;
            events
                .try_send(Event::State {
                    generation: GENERATION,
                    name: "CONNECTED".into(),
                    error: false,
                    fatal: false,
                })
                .map_err(|_| Error::Closed)?;
        }
        if watch.expired(Instant::now()) {
            return Err(Error::Closed);
        }
        batch += 1;
        if batch == crate::packet_batch::MAX_PACKET_BATCH_PACKETS {
            tokio::task::yield_now().await;
            batch = 0;
        }
    }
}
fn packet_address(packet: &[u8], source: bool) -> Option<IpAddr> {
    match packet.first()? >> 4 {
        4 if packet.len() >= 20
            && usize::from(u16::from_be_bytes([packet[2], packet[3]])) == packet.len()
            && packet[0] & 15 >= 5
            && usize::from(packet[0] & 15) * 4 <= packet.len() =>
        {
            let offset = if source { 12 } else { 16 };
            Some(IpAddr::V4(Ipv4Addr::from(
                <[u8; 4]>::try_from(&packet[offset..offset + 4]).ok()?,
            )))
        }
        6 if packet.len() >= 40
            && usize::from(u16::from_be_bytes([packet[4], packet[5]])) + 40 == packet.len() =>
        {
            let offset = if source { 8 } else { 24 };
            Some(IpAddr::V6(Ipv6Addr::from(
                <[u8; 16]>::try_from(&packet[offset..offset + 16]).ok()?,
            )))
        }
        _ => None,
    }
}
