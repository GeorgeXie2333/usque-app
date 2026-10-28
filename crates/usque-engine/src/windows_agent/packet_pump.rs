//! Bounded, independent packet admission in each shared-ring direction.

use super::*;
use std::{future::Future, pin::Pin};

/// The production implementation transfers owned enqueue state; the receive
/// half remains available while its packet waits for transport capacity.
trait PacketTransport: Send + 'static {
    fn start_send(
        &self,
        packet: Bytes,
    ) -> impl Future<Output = Result<(), TransportError>> + Send + 'static + use<Self>;
    fn receive(&mut self) -> impl Future<Output = Result<Bytes, TransportError>> + Send;
    fn try_receive(&mut self) -> Result<Option<Bytes>, TransportError>;
}

impl PacketTransport for TunPacketIo {
    fn start_send(
        &self,
        packet: Bytes,
    ) -> impl Future<Output = Result<(), TransportError>> + Send + 'static + use<> {
        self.start_send_owned_packet(packet)
    }

    async fn receive(&mut self) -> Result<Bytes, TransportError> {
        self.receive_packet().await
    }

    fn try_receive(&mut self) -> Result<Option<Bytes>, TransportError> {
        self.try_receive_packet()
    }
}

pub(super) fn start_packet_pumps(
    tun_io: TunPacketIo,
    mapping: Arc<PacketSessionMapping>,
    tunnel_monitor: ManagedTunnelMonitor,
    cancellation: CancellationToken,
    failure: watch::Sender<Option<WindowsPumpFailure>>,
) -> Vec<JoinHandle<()>> {
    start_with_io(
        tun_io,
        mapping,
        move || tunnel_monitor.path(),
        cancellation,
        failure,
    )
}

fn start_with_io<T, P>(
    tun_io: T,
    mapping: Arc<PacketSessionMapping>,
    path: P,
    cancellation: CancellationToken,
    failure: watch::Sender<Option<WindowsPumpFailure>>,
) -> Vec<JoinHandle<()>>
where
    T: PacketTransport,
    P: Fn() -> RuntimePath + Send + Sync + 'static,
{
    let (packet_ready_tx, packet_ready_rx) = mpsc::channel(1);
    let wait_mapping = Arc::clone(&mapping);
    let wait_task = spawn_packet_waiter(
        move || wait_for_agent_packets(&wait_mapping, packet_ready_tx),
        cancellation.clone(),
        failure.clone(),
    );
    // Create the guard before spawning: dropping an unpolled/aborted future
    // must also release its native notification waiter.
    let context = PumpContext {
        mapping,
        cancellation,
        failure,
    };
    let pump_task = tokio::spawn(run_packet_pump(tun_io, packet_ready_rx, path, context));
    vec![wait_task, pump_task]
}

struct PumpContext {
    mapping: Arc<PacketSessionMapping>,
    cancellation: CancellationToken,
    failure: watch::Sender<Option<WindowsPumpFailure>>,
}

impl Drop for PumpContext {
    fn drop(&mut self) {
        // Also release the actual blocking notification waiter if this task is
        // cancelled or aborted while an owned transport enqueue is pending.
        self.mapping.signal_shutdown();
    }
}

enum PumpEvent {
    Ready(Option<()>),
    Sent(Result<(), TransportError>),
    Received(Result<Bytes, TransportError>),
    Retry,
}

async fn run_packet_pump<T, P>(
    mut tun_io: T,
    mut packet_ready: mpsc::Receiver<()>,
    path: P,
    context: PumpContext,
) where
    T: PacketTransport,
    P: Fn() -> RuntimePath + Send + Sync + 'static,
{
    let mapping = &context.mapping;
    let cancellation = &context.cancellation;
    let failure = &context.failure;
    let mut upload = std::pin::pin!(None);
    let mut ring_readable = false;
    let mut download: Option<Bytes> = None;
    let mut publish_after = None;
    let mut saturation_count = 0_u64;

    loop {
        if cancellation.is_cancelled() {
            break;
        }
        // There is exactly one AgentToEngine consumer. Do not take a second
        // packet from the ring while the first owned enqueue waits for space.
        if upload.as_ref().get_ref().is_none() && ring_readable {
            match mapping.ring().try_pop(PacketDirection::AgentToEngine) {
                Ok(Some(packet)) => upload.set(Some(tun_io.start_send(Bytes::from(packet)))),
                Ok(None) => ring_readable = false,
                Err(error) => {
                    report_pump_failure(
                        failure,
                        cancellation,
                        WindowsPumpFailure::agent(format!(
                            "Agent-to-Engine packet ring failed: {error}"
                        )),
                    );
                    break;
                }
            }
        }

        if download.is_some()
            && publish_after.is_some_and(|deadline| tokio::time::Instant::now() >= deadline)
        {
            match publish_engine_packet_batch(
                &mut tun_io,
                mapping,
                path(),
                cancellation,
                &mut saturation_count,
                &mut download,
            ) {
                Ok(PublishState::Ready) => publish_after = None,
                Ok(PublishState::Blocked) => {
                    publish_after = Some(tokio::time::Instant::now() + PACKET_RING_RETRY_INTERVAL);
                }
                Ok(PublishState::Cancelled) => break,
                Err(error) => {
                    report_pump_failure(failure, cancellation, error);
                    break;
                }
            }
        }

        let event = tokio::select! {
            biased;
            () = cancellation.cancelled() => break,
            event = async {
                // Keep data directions fair; only cancellation is prioritized.
                tokio::select! {
                    ready = packet_ready.recv() => PumpEvent::Ready(ready),
                    result = wait_pending(upload.as_mut()) => PumpEvent::Sent(result),
                    packet = tun_io.receive(), if download.is_none() => PumpEvent::Received(packet),
                    () = async {
                        match publish_after {
                            Some(deadline) => tokio::time::sleep_until(deadline).await,
                            None => std::future::pending().await,
                        }
                    }, if download.is_some() => PumpEvent::Retry,
                }
            } => event,
        };
        match event {
            PumpEvent::Ready(Some(())) => ring_readable = true,
            PumpEvent::Ready(None) => {
                report_pump_failure(
                    failure,
                    cancellation,
                    WindowsPumpFailure::agent("Agent packet notification channel closed"),
                );
                break;
            }
            PumpEvent::Sent(Ok(())) => upload.set(None),
            PumpEvent::Sent(Err(error)) => {
                report_pump_failure(
                    failure,
                    cancellation,
                    WindowsPumpFailure::transport(
                        "failed to send a TUN packet into MASQUE",
                        &error,
                        path(),
                    ),
                );
                break;
            }
            PumpEvent::Received(Ok(packet)) => {
                download = Some(packet);
                publish_after = Some(tokio::time::Instant::now());
            }
            PumpEvent::Received(Err(error)) => {
                report_pump_failure(
                    failure,
                    cancellation,
                    WindowsPumpFailure::transport(
                        "failed to receive a MASQUE packet",
                        &error,
                        path(),
                    ),
                );
                break;
            }
            PumpEvent::Retry => {}
        }
    }
    // Drop both pending owners and tun_io together. MASQUE itself remains
    // owned by WindowsVpnRuntime; the TUN attach lifetime ends here.
}

async fn wait_pending<F: Future>(pending: Pin<&mut Option<F>>) -> F::Output {
    match pending.as_pin_mut() {
        Some(send) => send.await,
        None => std::future::pending().await,
    }
}

#[derive(Debug, PartialEq, Eq)]
enum PublishState {
    Ready,
    Blocked,
    Cancelled,
}

/// Publish only a bounded, already-ready batch. Full rings retain one owned
/// packet and return immediately so the main select can still upload packets.
fn publish_engine_packet_batch<T: PacketTransport>(
    tun_io: &mut T,
    mapping: &PacketSessionMapping,
    path: RuntimePath,
    cancellation: &CancellationToken,
    saturation_count: &mut u64,
    pending: &mut Option<Bytes>,
) -> Result<PublishState, WindowsPumpFailure> {
    let ring = mapping.ring();
    let wake_bytes = (ring.capacity() as usize / 4).max(1);
    let mut published_bytes = 0_usize;
    let mut packets = 0_usize;
    let result = loop {
        if cancellation.is_cancelled() {
            break Ok(PublishState::Cancelled);
        }
        let Some(packet) = pending.as_ref() else {
            break Ok(PublishState::Ready);
        };
        match ring.try_push_preserving(PacketDirection::EngineToAgent, packet) {
            Ok(true) => {}
            Ok(false) => {
                *saturation_count = saturation_count.saturating_add(1);
                if saturation_count.is_power_of_two() {
                    tracing::warn!(
                        wait_count = *saturation_count,
                        ring_capacity = ring.capacity(),
                        "waiting for capacity in the Engine-to-Agent packet ring"
                    );
                }
                break Ok(PublishState::Blocked);
            }
            Err(error) => {
                break Err(WindowsPumpFailure::agent(format!(
                    "Engine-to-Agent packet ring failed: {error}"
                )));
            }
        }
        published_bytes = published_bytes.saturating_add(packet.len());
        packets += 1;
        *pending = None;
        if packets == PACKET_WAKE_BATCH || published_bytes >= wake_bytes {
            break Ok(PublishState::Ready);
        }
        match tun_io.try_receive() {
            Ok(Some(packet)) => *pending = Some(packet),
            Ok(None) => break Ok(PublishState::Ready),
            Err(error) => {
                break Err(WindowsPumpFailure::transport(
                    "failed to receive a MASQUE packet",
                    &error,
                    path,
                ));
            }
        }
    };
    if packets != 0 {
        // Signal exactly after the published prefix, including when a later
        // packet hit ring capacity or a later receive returned an error.
        mapping
            .signal_engine_to_agent()
            .map_err(|error| WindowsPumpFailure::agent(error.to_string()))?;
    }
    result
}

#[cfg(test)]
#[path = "tests/packet_pump_tests.rs"]
mod tests;
