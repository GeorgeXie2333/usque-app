//! Independent implementation of the public Proton UDP availability wire format.
//! Reference: android-app deb11fef4092a1ecfcf51a7d40bdece9eb4c3e5e ServerPing.kt.
use super::{Generation, PROBE_TIMEOUT, Probe, check};
use crate::{InternalNetwork, proton_vpn::Failure};
use async_trait::async_trait;
use base64::{Engine, engine::general_purpose::STANDARD};
use boring::hash::hmac_sha256;
use std::time::Duration;
use tokio::{sync::Mutex, time::Instant};
use tokio_util::sync::CancellationToken;
use usque_core::proton_vpn::SelectedNode;

// Public protocol constant, not an account credential or an authentication key.
const PING_KEY: &[u8] = b"lci6UYRryo5rcQVpxfJ0fCs6UBY5eGyV";
const RESPONSE: &[u8] = &[0xfe, 1, 1];
const SEND_SPACING: Duration = Duration::from_millis(10);

pub(super) fn packet(server_key: &str, timestamp: u32) -> Result<[u8; 38], Failure> {
    let key = STANDARD
        .decode(server_key)
        .map_err(|_| Failure::new("invalid_server_directory"))?;
    if key.len() != 32 {
        return Err(Failure::new("invalid_server_directory"));
    }
    let mut data = [0u8; 38];
    data[..2].copy_from_slice(&[0xfe, 1]);
    data[2..6].copy_from_slice(&timestamp.to_le_bytes());
    let mut message = [0u8; 36];
    message[..32].copy_from_slice(&key);
    message[32..].copy_from_slice(&data[2..6]);
    let mac = hmac_sha256(PING_KEY, &message)
        .map_err(|_| Failure::new("cryptographic_validation_failed"))?;
    data[6..].copy_from_slice(&mac);
    Ok(data)
}

pub(in crate::proton_vpn) struct WarpProbe {
    network: InternalNetwork,
    next_send: Mutex<Instant>,
}
impl WarpProbe {
    pub(in crate::proton_vpn) fn new(network: InternalNetwork) -> Self {
        Self {
            network,
            next_send: Mutex::new(Instant::now()),
        }
    }
}
#[async_trait]
impl Probe for WarpProbe {
    fn generation(&self) -> Option<Generation> {
        self.network.probe_generation()
    }
    async fn measure(
        &self,
        peer: &SelectedNode,
        cancel: &CancellationToken,
    ) -> Result<Option<Duration>, Failure> {
        let generation = self
            .generation()
            .ok_or_else(|| Failure::new("underlay_changed"))?;
        check(self, generation, cancel)?;
        let address = peer
            .endpoint
            .address()
            .ok_or_else(|| Failure::new("invalid_server_directory"))?;
        let timestamp = u32::try_from(super::super::interaction::now())
            .map_err(|_| Failure::new("api_transport_failed"))?;
        let data = packet(&peer.public_key, timestamp)?;
        // Each sample gets a distinct source port: the fixed three-byte response
        // carries no nonce, so reusing a socket could count a late old response.
        let Ok(socket) = self.network.bind_udp(address, cancel).await else {
            check(self, generation, cancel)?;
            return Ok(None);
        };
        let mut next = tokio::select! { biased;
            _ = cancel.cancelled() => return Err(Failure::new("cancelled")),
            next = self.next_send.lock() => next,
        };
        tokio::select! { biased;
            _ = cancel.cancelled() => return Err(Failure::new("cancelled")),
            _ = tokio::time::sleep_until(*next) => (),
        }
        *next = Instant::now() + SEND_SPACING;
        drop(next);
        check(self, generation, cancel)?;
        let started = Instant::now();
        let exchange = async {
            socket.send(&data).await.ok()?;
            // InternalUdp also rejects packets from any other source endpoint.
            loop {
                let reply = socket.recv().await.ok()?;
                if reply.as_ref() == RESPONSE {
                    return Some(started.elapsed());
                }
            }
        };
        let rtt = tokio::select! { biased;
            _ = cancel.cancelled() => return Err(Failure::new("cancelled")),
            result = tokio::time::timeout(PROBE_TIMEOUT, exchange) => result.ok().flatten(),
        };
        check(self, generation, cancel)?;
        Ok(rtt)
    }
}
