//! WireGuard packets in memory only. WARP owns every network operation.
use boringtun::noise::{Tunn, TunnResult};
use bytes::Bytes;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tokio_util::task::AbortOnDropHandle;
use usque_core::chain_exit::WireGuardProfile;
use usque_openvpn::{Error, Event, NetworkConfig};
#[cfg(test)]
mod tests;

const GENERATION: u64 = 1;
const MAX_PACKET: usize = 65535;
#[derive(Clone)]
pub(crate) struct Input {
    sender: mpsc::Sender<(u8, Bytes)>,
    cancel: CancellationToken,
}
impl Input {
    pub(crate) async fn push(&self, kind: u8, generation: u64, bytes: &[u8]) -> Result<(), Error> {
        if generation != GENERATION || bytes.len() > MAX_PACKET {
            return Err(Error::InvalidPacket);
        }
        tokio::select! {
            biased;
            _ = self.cancel.cancelled() => Err(Error::Closed),
            result = self.sender.send((kind, Bytes::copy_from_slice(bytes))) => result.map_err(|_| Error::Closed),
        }
    }
    pub(crate) fn stop(&self) {
        self.cancel.cancel();
    }
}
pub(crate) struct Session {
    input: Input,
    events: mpsc::Receiver<Event>,
    task: Option<AbortOnDropHandle<()>>,
}
impl Session {
    pub(crate) fn start(profile: WireGuardProfile) -> Self {
        let cancel = CancellationToken::new();
        // Each queue is independently bounded to at most 64 maximum-size packets.
        let (sender, receiver) = mpsc::channel(64);
        let (events, output) = mpsc::channel(64);
        let input = Input {
            sender,
            cancel: cancel.clone(),
        };
        let task = tokio::spawn(async move {
            let work = run(profile, receiver, &events);
            tokio::select! { biased; _ = cancel.cancelled() => {}, _ = work => {} }
            cancel.cancel();
        });
        Self {
            input,
            events: output,
            task: Some(AbortOnDropHandle::new(task)),
        }
    }
    pub(crate) fn input(&self) -> Input {
        self.input.clone()
    }
    pub(crate) async fn next_event(&mut self) -> Result<Event, Error> {
        self.events.recv().await.ok_or(Error::Closed)
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

async fn run(
    profile: WireGuardProfile,
    mut input: mpsc::Receiver<(u8, Bytes)>,
    events: &mpsc::Sender<Event>,
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
        .send(Event::Dial {
            generation: GENERATION,
        })
        .await
        .map_err(|_| Error::Closed)?;
    let mut buffer = vec![0u8; MAX_PACKET + 256];
    let mut timer = tokio::time::interval(Duration::from_millis(100));
    timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut connected = false;
    let mut transport_ready = false;
    let mut pending_rekey: Option<tokio::time::Instant> = None;
    let mut handshake_age = None;
    loop {
        let result = tokio::select! {
            command = input.recv() => {
                match command {
                    Some((3, _)) => { transport_ready = true; tunnel.format_handshake_initiation(&mut buffer, false) },
                    Some((4, _)) | None => return Err(Error::Closed),
                    Some((1, bytes)) if transport_ready => {
                        let mut result = tunnel.decapsulate(None, &bytes, &mut buffer);
                        loop {
                            let drain = matches!(result, TunnResult::WriteToNetwork(_));
                            emit(result, events, &profile).await?;
                            if !drain { break; }
                            result = tunnel.decapsulate(None, &[], &mut buffer);
                        }
                        TunnResult::Done
                    },
                    Some((2, bytes)) if connected => {
                        if bytes.len() > usize::from(profile.mtu) || packet_address(&bytes, false).is_none_or(|ip| !profile.allows(ip)) { continue; }
                        tunnel.encapsulate(&bytes, &mut buffer)
                    },
                    _ => continue,
                }
            },
            _ = timer.tick(), if transport_ready => tunnel.update_timers(&mut buffer),
        };
        if connected
            && matches!(&result, TunnResult::WriteToNetwork(bytes) if bytes.starts_with(&[1, 0, 0, 0]))
        {
            pending_rekey.get_or_insert_with(tokio::time::Instant::now);
        }
        emit(result, events, &profile).await?;
        let age = tunnel.time_since_last_handshake();
        if let (Some(previous), Some(current)) = (handshake_age, age)
            && current < previous
        {
            pending_rekey = None;
        }
        handshake_age = age;
        if !connected && tunnel.time_since_last_handshake().is_some() {
            connected = true;
            let network = NetworkConfig {
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
                .send(Event::Network {
                    generation: GENERATION,
                    config: network,
                })
                .await
                .map_err(|_| Error::Closed)?;
            events
                .send(Event::State {
                    generation: GENERATION,
                    name: "CONNECTED".into(),
                    error: false,
                    fatal: false,
                })
                .await
                .map_err(|_| Error::Closed)?;
        }
        // An idle cryptographic key expiring is normal. Only an unanswered
        // active handshake is a connection failure; idle time alone is not.
        if pending_rekey.is_some_and(|started| started.elapsed() > Duration::from_secs(35)) {
            return Err(Error::Closed);
        }
    }
}
async fn emit(
    result: TunnResult<'_>,
    events: &mpsc::Sender<Event>,
    profile: &WireGuardProfile,
) -> Result<(), Error> {
    let event = match result {
        TunnResult::WriteToNetwork(bytes) => Some(Event::TransportPacket {
            generation: GENERATION,
            packet: Bytes::copy_from_slice(bytes),
        }),
        TunnResult::WriteToTunnelV4(bytes, _) | TunnResult::WriteToTunnelV6(bytes, _) => {
            if packet_address(bytes, true).is_some_and(|ip| profile.allows(ip)) {
                Some(Event::IpPacket {
                    generation: GENERATION,
                    packet: Bytes::copy_from_slice(bytes),
                })
            } else {
                None
            }
        }
        // Unauthenticated, stale and replayed UDP datagrams cannot tear down an
        // authenticated session. Startup/rekey timeouts are handled separately.
        TunnResult::Err(_) | TunnResult::Done => None,
    };
    if let Some(event) = event {
        events.send(event).await.map_err(|_| Error::Closed)?;
    }
    Ok(())
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
