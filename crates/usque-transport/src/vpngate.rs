//! OpenVPN's transport is supplied exclusively by a WARP internal network.
//! Decrypted packets use the same bounded mux as CONNECT-IP frontends.
use crate::chain_session::{EventStream, Input, Session};
use crate::h2::TransportError;
use crate::internal_network::InternalNetwork;
use crate::netstack::{ExternalPacketChannels, ManagedTunnelRuntime, RuntimeHealth, RuntimePath};
use crate::packet_batch::PacketBatch;
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::{oneshot, watch};
use tokio::task::JoinHandle;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use tokio_util::task::AbortOnDropHandle;
use usque_core::vpngate::{
    FinalNetworkParameters, GateFailure, GateStage, GateStatus, PreparedProfile,
};
use usque_openvpn::{Event, NetworkConfig};

#[cfg(test)]
mod authentication_tests;

static NEXT_GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
const AUTHENTICATION_RETRIES: u8 = 1;

fn retry_startup_authentication(reason: GateFailure, attempt: u8) -> bool {
    reason == GateFailure::Authentication && attempt < AUTHENTICATION_RETRIES
}

async fn with_authentication_retries<T, F, Fut>(
    cancel: &CancellationToken,
    mut connect: F,
) -> Result<T, TransportError>
where
    F: FnMut(u8) -> Fut,
    Fut: std::future::Future<Output = Result<T, TransportError>>,
{
    let mut attempt = 0;
    loop {
        if cancel.is_cancelled() {
            return Err(TransportError::TunnelClosed);
        }
        if attempt > 0 {
            tracing::info!(
                gate_event = "AUTHENTICATION_RETRY",
                attempt,
                "Retrying VPN Gate authentication internally"
            );
        }
        // Each attempt finishes its transport/native cleanup before returning
        // an authentication failure. Never overlap workers or change the node.
        match connect(attempt).await {
            Err(TransportError::VpnGate(reason))
                if retry_startup_authentication(reason, attempt) =>
            {
                attempt += 1;
            }
            result => return result,
        }
    }
}

// Cancellation covers the whole operation, including awaits inside a selected
// event branch and the final native failure notification.
async fn until_cancelled<F: std::future::Future>(
    cancel: &CancellationToken,
    work: F,
) -> Option<F::Output> {
    tokio::select! {
        biased;
        _ = cancel.cancelled() => None,
        result = work => Some(result),
    }
}

fn candidate_order(count: usize, random: bool) -> Vec<usize> {
    let mut order: Vec<_> = (0..count).collect();
    if random {
        for index in (1..count).rev() {
            let entropy = uuid::Uuid::new_v4();
            let value =
                u64::from_le_bytes(entropy.as_bytes()[..8].try_into().expect("UUID length"));
            order.swap(index, value as usize % (index + 1));
        }
    }
    order
}
fn candidate_deadline(now: Instant, deadline: Instant, remaining: usize) -> Option<Instant> {
    let available = deadline.saturating_duration_since(now);
    if available.is_zero() || remaining == 0 {
        return None;
    }
    Some(now + (available / remaining as u32).min(Duration::from_secs(35)))
}

async fn attempt_candidates<T, F, Fut>(
    cancel: &CancellationToken,
    deadline: Instant,
    order: &[usize],
    mut attempt: F,
) -> Result<T, TransportError>
where
    F: FnMut(usize, Instant) -> Fut,
    Fut: std::future::Future<Output = Result<T, TransportError>>,
{
    for (position, index) in order.iter().enumerate() {
        if cancel.is_cancelled() {
            return Err(TransportError::TunnelClosed);
        }
        let Some(attempt_deadline) =
            candidate_deadline(Instant::now(), deadline, order.len() - position)
        else {
            break;
        };
        // The attempt owns cleanup; never cancel/drop it to start a successor.
        match attempt(*index, attempt_deadline).await {
            Err(TransportError::VpnGate(GateFailure::Transport)) => {}
            result => return result,
        }
    }
    Err(TransportError::VpnGate(GateFailure::Transport))
}

pub(crate) struct GateDriver {
    pub(crate) status: watch::Receiver<GateStatus>,
    status_tx: watch::Sender<GateStatus>,
    admission: watch::Sender<bool>,
    cancellation: CancellationToken,
    native_input: Input,
    diagnostic_id: u64,
    task: Option<JoinHandle<bool>>,
}

impl GateDriver {
    pub(crate) async fn start(
        profile: &PreparedProfile,
        warp: InternalNetwork,
        transport_telemetry: crate::NetworkQualityTelemetry,
        status_sink: Option<watch::Sender<GateStatus>>,
        startup_cancel: &CancellationToken,
        connection_deadline: Instant,
    ) -> Result<(Self, ManagedTunnelRuntime, FinalNetworkParameters), TransportError> {
        let status = status_sink.unwrap_or_else(|| watch::channel(GateStatus::default()).0);
        if let Some(usque_core::chain_exit::ValidatedProfile::OpenVpn(config)) =
            profile.custom.as_deref()
        {
            let deadline = connection_deadline.min(Instant::now() + Duration::from_secs(120));
            let order = candidate_order(config.candidates.len(), config.remote_random);
            status.send_modify(|s| {
                s.attempt_count = 0;
                s.candidate_count = order.len() as u32;
                s.attempt_failures.clear();
                s.active_endpoint = None;
            });
            let result = attempt_candidates(
                startup_cancel,
                deadline,
                &order,
                |index, attempt_deadline| {
                    let candidate = config.candidates[index].clone();
                    let status = status.clone();
                    let warp = warp.clone();
                    let telemetry = transport_telemetry.clone();
                    async move {
                        status.send_modify(|s| {
                            s.attempt_count += 1;
                            s.attempting_endpoint = Some(candidate.endpoint.clone());
                            s.stage = GateStage::ConnectingServer;
                        });
                        let result = Self::start_attempt(
                            profile,
                            warp,
                            telemetry,
                            status.clone(),
                            startup_cancel,
                            0,
                            Some((candidate, attempt_deadline)),
                            connection_deadline,
                        )
                        .await;
                        if let Err(TransportError::VpnGate(reason)) = &result {
                            status.send_modify(|s| s.attempt_failures.push(*reason));
                        }
                        result
                    }
                },
            )
            .await;
            if let Err(TransportError::VpnGate(failure)) = &result {
                status.send_modify(|s| {
                    s.stage = GateStage::Error;
                    s.failure = Some(*failure);
                    s.attempting_endpoint = None;
                });
            }
            return result;
        }

        if profile.custom.is_some() {
            return Self::start_attempt(
                profile,
                warp,
                transport_telemetry,
                status,
                startup_cancel,
                0,
                None,
                connection_deadline,
            )
            .await;
        }
        with_authentication_retries(startup_cancel, |attempt| {
            Self::start_attempt(
                profile,
                warp.clone(),
                transport_telemetry.clone(),
                status.clone(),
                startup_cancel,
                attempt,
                None,
                connection_deadline,
            )
        })
        .await
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "attempt owns the protocol, underlay, telemetry, status, cancellation, retry and both absolute deadlines"
    )]
    async fn start_attempt(
        profile: &PreparedProfile,
        warp: InternalNetwork,
        transport_telemetry: crate::NetworkQualityTelemetry,
        status_tx: watch::Sender<GateStatus>,
        startup_cancel: &CancellationToken,
        auth_attempt: u8,
        candidate: Option<(usque_core::chain_exit::OpenVpnEndpoint, Instant)>,
        connection_deadline: Instant,
    ) -> Result<(Self, ManagedTunnelRuntime, FinalNetworkParameters), TransportError> {
        if startup_cancel.is_cancelled() {
            return Err(TransportError::TunnelClosed);
        }
        if auth_attempt > 0 && !matches!(warp.health_snapshot(), RuntimeHealth::Connected { .. }) {
            return Err(TransportError::VpnGate(GateFailure::Transport));
        }
        let deadline = candidate.as_ref().map_or_else(
            || connection_deadline.min(Instant::now() + Duration::from_secs(35)),
            |(_, deadline)| *deadline,
        );
        let remote = if let Some((candidate, _)) = &candidate {
            tokio::time::timeout_at(
                deadline,
                warp.resolve_endpoint(&candidate.endpoint, candidate.ipv6, startup_cancel),
            )
            .await
            .map_err(|_| TransportError::VpnGate(GateFailure::Transport))?
            .map_err(|_| TransportError::VpnGate(GateFailure::Transport))?
        } else if let Some(summary) = &profile.summary {
            let ipv6 = match profile.custom.as_deref() {
                Some(usque_core::chain_exit::ValidatedProfile::OpenVpn(p)) => p.endpoint_ipv6,
                _ => None,
            };
            tokio::time::timeout_at(
                deadline,
                warp.resolve_endpoint(&summary.endpoint, ipv6, startup_cancel),
            )
            .await
            .map_err(|_| TransportError::VpnGate(GateFailure::Transport))?
            .map_err(|_| TransportError::VpnGate(GateFailure::Transport))?
        } else {
            profile.remote
        };
        let udp = profile
            .summary
            .as_ref()
            .is_some_and(|s| s.protocol.requires_udp());
        let mut native = Session::start(profile, remote)
            .map_err(|_| TransportError::VpnGate(GateFailure::Configuration))?;
        let (transport_output, ip_output) = native
            .split_packet_outputs()
            .ok_or(TransportError::VpnGate(GateFailure::Configuration))?;
        let cancellation = CancellationToken::new();
        let guard = cancellation.clone().drop_guard();
        status_tx.send_modify(|s| {
            s.stage = if auth_attempt == 0 {
                GateStage::ConnectingServer
            } else {
                GateStage::Negotiating
            };
            s.failure = None;
            s.network = None;
            s.current_profile = profile.summary.clone();
        });
        let status = status_tx.subscribe();
        let (ready_tx, ready_rx) = oneshot::channel();
        let (admission, admitted) = watch::channel(false);
        let native_input = native.input();
        let diagnostic_id = NEXT_GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let actor = Actor {
            diagnostic_id,
            connection_deadline,
            auth_attempt,
            native,
            transport_output,
            ip_output,
            packet_tasks: Vec::new(),
            remote,
            udp,
            underlay_health: warp.health(),
            transport_telemetry,
            warp,
            status: status_tx.clone(),
            ready: Some(ready_tx),
            channels: None,
            network: None,
            generation: 0,
            connected: false,
            connection: None,
            cancellation: cancellation.clone(),
            reconnect_count: 0,
            admitted,
        };
        let task = tokio::spawn(actor.run());
        let mut driver = Self {
            status,
            status_tx,
            admission,
            cancellation,
            native_input,
            diagnostic_id,
            task: Some(task),
        };
        let result = tokio::select! {
            biased;
            _ = startup_cancel.cancelled() => {
                tracing::info!(gate_event = "STARTUP_CANCELLED", "VPN Gate startup cancelled");
                driver.shutdown().await;
                return Err(TransportError::TunnelClosed);
            }
            result = tokio::time::timeout_at(deadline, ready_rx) => result,
        };
        match result {
            Ok(Ok(Ok((runtime, network)))) => {
                guard.disarm();
                Ok((driver, runtime, network))
            }
            other => {
                if other.is_err() {
                    tracing::warn!(gate_event = "STARTUP_TIMEOUT", "VPN Gate startup failed");
                }
                let reason = match other {
                    Ok(Ok(Err(reason))) => reason,
                    _ => GateFailure::Transport,
                };
                let stopped = driver.shutdown().await;
                // A pending native worker is not a completed attempt. Stop the
                // chain instead of starting another worker alongside it.
                let reason = if !stopped {
                    GateFailure::Cleanup
                } else {
                    reason
                };
                Err(TransportError::VpnGate(reason))
            }
        }
    }
    pub(crate) fn cancel(&self) {
        if !self.cancellation.is_cancelled() {
            tracing::info!(
                gate_event = "STOP_REQUESTED",
                gate_driver_id = self.diagnostic_id,
                "VPN Gate stop requested"
            );
        }
        self.cancellation.cancel();
        // Do not put stop behind a join that may itself be waiting for native
        // input capacity. The worker owns no OS transport or TUN resources.
        self.native_input.stop();
    }
    pub(crate) fn fail(&self, reason: GateFailure) {
        self.cancel();
        self.status_tx.send_modify(|s| {
            s.stage = GateStage::Error;
            s.failure = Some(reason);
            s.network = None;
        });
    }
    pub(crate) fn admit(&self) {
        self.admission.send_replace(true);
    }
    pub(crate) async fn shutdown(&mut self) -> bool {
        self.cancel();
        if let Some(task) = self.task.take() {
            let started = Instant::now();
            tracing::info!(
                gate_event = "TASK_JOIN_STARTED",
                gate_driver_id = self.diagnostic_id,
                "Waiting for VPN Gate task shutdown"
            );
            let stopped = task.await.unwrap_or(false);
            tracing::info!(
                gate_event = "TASK_JOIN_FINISHED",
                gate_driver_id = self.diagnostic_id,
                elapsed_ms = started.elapsed().as_millis() as u64,
                "VPN Gate task shutdown finished"
            );
            return stopped;
        }
        true
    }
}
impl Drop for GateDriver {
    fn drop(&mut self) {
        self.cancel();
    }
}

type Startup = Result<(ManagedTunnelRuntime, FinalNetworkParameters), GateFailure>;
struct Actor {
    diagnostic_id: u64,
    connection_deadline: Instant,
    auth_attempt: u8,
    native: Session,
    transport_output: EventStream,
    ip_output: EventStream,
    packet_tasks: Vec<AbortOnDropHandle<()>>,
    warp: InternalNetwork,
    remote: SocketAddr,
    udp: bool,
    underlay_health: watch::Receiver<RuntimeHealth>,
    transport_telemetry: crate::NetworkQualityTelemetry,
    status: watch::Sender<GateStatus>,
    ready: Option<oneshot::Sender<Startup>>,
    channels: Option<ExternalPacketChannels>,
    network: Option<FinalNetworkParameters>,
    generation: u64,
    reconnect_count: u32,
    connected: bool,
    connection: Option<Connection>,
    cancellation: CancellationToken,
    admitted: watch::Receiver<bool>,
}
impl Actor {
    async fn run(mut self) -> bool {
        let cancel = self.cancellation.clone();
        let result = until_cancelled(&cancel, self.drive())
            .await
            .unwrap_or(Ok(()));
        self.connected = false;
        if let Err(reason) = result {
            publish_failure(
                &self.status,
                reason,
                self.ready.is_some().then_some(self.auth_attempt),
            );
            if let Some(ready) = self.ready.take() {
                let _ = ready.send(Err(reason));
            }
            if let Some(channels) = &self.channels {
                let error = TransportError::VpnGate(reason);
                let path = self.path();
                channels.health.send_replace(RuntimeHealth::Failed {
                    last_path: path,
                    reconnect_count: self.reconnect_count,
                    message: error.to_string(),
                    failure: error.failure(Some(path.transport), Some(path.endpoint_family)),
                });
                channels.failure.send_replace(Some(error.to_string()));
            }
        }
        self.cancellation.cancel();
        if let Some(channels) = &self.channels {
            channels.cancellation.cancel();
        }
        self.native.input().stop();
        tracing::info!(
            gate_event = "NATIVE_STOP_REQUESTED",
            gate_driver_id = self.diagnostic_id,
            protocol_generation = self.generation,
            "VPN Gate native stop requested"
        );
        if let Some(connection) = self.connection.take() {
            tracing::info!(
                gate_event = "TRANSPORT_JOIN_STARTED",
                gate_driver_id = self.diagnostic_id,
                "Waiting for VPN Gate transport shutdown"
            );
            connection.shutdown().await;
            tracing::info!(
                gate_event = "TRANSPORT_JOIN_FINISHED",
                gate_driver_id = self.diagnostic_id,
                "VPN Gate transport shutdown finished"
            );
        }
        for task in self.packet_tasks.drain(..) {
            let _ = task.await;
        }
        let started = Instant::now();
        let shutdown = self.native.shutdown().await;
        match &shutdown {
            Ok(()) => tracing::info!(
                gate_event = "NATIVE_STOP_FINISHED",
                gate_driver_id = self.diagnostic_id,
                elapsed_ms = started.elapsed().as_millis() as u64,
                "VPN Gate native worker exited"
            ),
            Err(error) => {
                tracing::warn!(gate_event = "NATIVE_STOP_FAILED", gate_driver_id = self.diagnostic_id, worker_pending = *error == usque_openvpn::Error::ShutdownTimeout, failure_kind = ?error, elapsed_ms = started.elapsed().as_millis() as u64, "VPN Gate native shutdown did not complete successfully")
            }
        }
        shutdown != Err(usque_openvpn::Error::ShutdownTimeout)
    }

    async fn drive(&mut self) -> Result<(), GateFailure> {
        loop {
            let channel_cancel = self
                .channels
                .as_ref()
                .map(|channels| channels.cancellation.clone());
            tokio::select! {
                biased;
                _ = self.cancellation.cancelled() => return Ok(()),
                _ = tokio::time::sleep_until(self.connection_deadline), if !*self.admitted.borrow() => return Err(GateFailure::Transport),
                _ = async {
                    if let Some(cancel) = &channel_cancel { cancel.cancelled().await }
                    else { std::future::pending().await }
                } => return Ok(()),
                event = self.native.next_event() => self.event(event.map_err(|error| {
                    tracing::warn!(gate_event = "NATIVE_EVENT_FAILED", failure_kind = ?error, "VPN Gate native event could not be decoded");
                    GateFailure::Transport
                })?).await?,
                changed = self.underlay_health.changed() => {
                    if changed.is_err() { return Err(GateFailure::Transport); }
                    if !matches!(*self.underlay_health.borrow(), RuntimeHealth::Connected { .. }) {
                        if self.channels.is_some() { return Err(GateFailure::Transport); }
                        self.reconnecting();
                        if let Some(connection) = self.connection.take() { connection.shutdown().await; }
                        let _ = self.native.input().transport_failed(self.generation).await;
                    }
                }
                changed = self.admitted.changed() => {
                    if changed.is_err() { return Ok(()); }
                    if self.connected { self.publish_connected(); }
                }

            }
        }
    }

    async fn event(&mut self, event: Event) -> Result<(), GateFailure> {
        match event {
            Event::Dial { generation } => {
                if self.channels.is_some() {
                    return Err(GateFailure::Transport);
                }
                self.connected = false;
                self.generation = generation;
                if let Some(connection) = self.connection.take() {
                    connection.shutdown().await;
                }
                let status_generation =
                    NEXT_GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                self.status.send_modify(|s| {
                    s.generation = status_generation;
                    s.stage = if self.auth_attempt == 0 {
                        GateStage::ConnectingServer
                    } else {
                        GateStage::Negotiating
                    };
                });
                self.connection = Some(Connection::start(
                    self.warp.clone(),
                    self.remote,
                    self.udp,
                    self.native.input(),
                    self.transport_output.clone(),
                    generation,
                    &self.cancellation,
                ));
            }
            Event::Network { generation, config } if generation == self.generation => {
                let network = final_network(config)?;
                if self.network.as_ref().is_some_and(|old| old != &network)
                    && self.channels.is_some()
                {
                    // Address changes require platform handoff and a fresh
                    // stack. Close admission before reporting this condition.
                    return Err(GateFailure::AddressChanged);
                }
                self.status.send_modify(|s| {
                    s.network = Some(network.clone());
                    s.stage = GateStage::ConfiguringNetwork;
                });
                self.network = Some(network);
            }
            Event::State {
                generation,
                name,
                error,
                fatal,
            } if generation == self.generation => {
                // The C ABI supplies library enum labels only, never free-form
                // peer info, certificate subjects, endpoints or configuration.
                tracing::info!(gate_event = %name, error, fatal, "VPN Gate protocol event");
                if name == "CONNECTED" {
                    let network = self.network.clone().ok_or(GateFailure::Configuration)?;
                    if !matches!(
                        *self.underlay_health.borrow(),
                        RuntimeHealth::Connected { .. }
                    ) {
                        return Err(GateFailure::Transport);
                    }
                    self.connected = true;
                    let path = self.path();
                    if let Some(ready) = self.ready.take() {
                        let (runtime, mut channels) = ManagedTunnelRuntime::for_external_packets(
                            path,
                            self.transport_telemetry.clone(),
                        );
                        self.start_packet_tasks(&mut channels, &network)?;
                        self.channels = Some(channels);
                        self.publish_connected();
                        ready
                            .send(Ok((runtime, network)))
                            .map_err(|_| GateFailure::Transport)?;
                    } else {
                        self.publish_connected();
                    }
                } else if name == "RECONNECTING" || name == "DISCONNECTED" {
                    if self.channels.is_some() {
                        return Err(GateFailure::Transport);
                    }
                    self.reconnecting();
                } else if error || fatal {
                    let reason = event_failure(&name);
                    if fatal || !reason.retryable() || self.channels.is_some() {
                        return Err(reason);
                    }
                    self.reconnecting();
                } else if name == "CONNECTING" {
                    self.status
                        .send_modify(|s| s.stage = GateStage::Negotiating);
                }
            }
            Event::Stopped => return Err(GateFailure::Transport),
            _ => {}
        }
        Ok(())
    }

    fn start_packet_tasks(
        &mut self,
        channels: &mut ExternalPacketChannels,
        network: &FinalNetworkParameters,
    ) -> Result<(), GateFailure> {
        let mut outgoing = channels.outgoing.take().ok_or(GateFailure::Configuration)?;
        let input = self.native.input();
        let generation = self.generation;
        let cancel = self.cancellation.clone();
        let admitted = self.admitted.clone();
        let network = network.clone();
        let counters = channels.counters.clone();
        self.packet_tasks
            .push(AbortOnDropHandle::new(tokio::spawn(async move {
                let result = until_cancelled(&cancel, async {
                    while let Some(packet) = outgoing.recv().await {
                        let packet = packet.freeze();
                        if !*admitted.borrow() || !packet_family_supported(&network, &packet) {
                            continue;
                        }
                        let length = packet.len();
                        input.send_ip_owned(generation, packet).await?;
                        counters.record_sent(length);
                    }
                    Err::<(), _>(usque_openvpn::Error::Closed)
                })
                .await;
                if result.is_some() {
                    input.stop();
                }
            })));
        let output = self.ip_output.clone();
        let incoming = channels.incoming.clone();
        let counters = channels.counters.clone();
        let input = self.native.input();
        let cancel = self.cancellation.clone();
        let admitted = self.admitted.clone();
        self.packet_tasks
            .push(AbortOnDropHandle::new(tokio::spawn(async move {
                let result = until_cancelled(&cancel, async {
                    let mut pending = None;
                    loop {
                        let event = match pending.take() {
                            Some(event) => event,
                            None => output.next().await?,
                        };
                        let mut batch = PacketBatch::new();
                        let mut next = Some(event);
                        for index in 0..crate::packet_batch::MAX_PACKET_BATCH_PACKETS {
                            let Some(event) = next.take() else {
                                break;
                            };
                            match event {
                                Event::IpPacket {
                                    generation: current,
                                    packet,
                                } if current == generation && *admitted.borrow() => {
                                    if let Err(packet) = batch.push_back(packet) {
                                        pending = Some(Event::IpPacket {
                                            generation: current,
                                            packet,
                                        });
                                        break;
                                    }
                                }
                                Event::Stopped => {
                                    return Err::<(), _>(usque_openvpn::Error::Closed);
                                }
                                _ => {}
                            }
                            if index + 1 == crate::packet_batch::MAX_PACKET_BATCH_PACKETS
                                || batch.len() == crate::packet_batch::MAX_PACKET_BATCH_PACKETS
                            {
                                break;
                            }
                            next = output.try_next().await?;
                        }
                        if !batch.is_empty() {
                            let bytes = batch.bytes();
                            incoming
                                .send(batch, bytes)
                                .await
                                .map_err(|_| usque_openvpn::Error::Closed)?;
                            counters.record_received(bytes);
                        }
                        tokio::task::yield_now().await;
                    }
                })
                .await;
                if result.is_some() {
                    input.stop();
                }
            })));
        Ok(())
    }

    fn path(&self) -> RuntimePath {
        let mut path = self.underlay_health.borrow().path();
        path.ipv4_available = self.network.as_ref().is_some_and(|n| n.ipv4.is_some());
        path.ipv6_available = self.network.as_ref().is_some_and(|n| n.ipv6.is_some());
        path
    }
    fn publish_connected(&self) {
        let admitted = *self.admitted.borrow();
        if let Some(channels) = &self.channels {
            let path = self.path();
            let health = if admitted {
                RuntimeHealth::Connected {
                    path,
                    reconnect_count: self.reconnect_count,
                }
            } else {
                RuntimeHealth::Reconnecting {
                    last_path: path,
                    attempt: 0,
                    reconnect_count: self.reconnect_count,
                    reason: "VPN Gate platform configuration pending".into(),
                    failure: TransportError::VpnGate(GateFailure::Transport).failure(None, None),
                }
            };
            channels.health.send_replace(health);
        }
        // Connected is the admission acknowledgement. Readers may immediately
        // start a final-exit probe, which requires the health above to be ready.
        self.status.send_modify(|s| {
            s.stage = if admitted {
                GateStage::Connected
            } else {
                GateStage::ConfiguringNetwork
            };
            s.failure = None;
            s.active_endpoint = admitted.then_some(self.remote);
            if admitted {
                s.attempting_endpoint = None;
            }
        });
    }
    fn reconnecting(&mut self) {
        if self.connected {
            self.reconnect_count = self.reconnect_count.saturating_add(1);
        }
        self.connected = false;
        self.status.send_modify(|s| {
            s.stage = if self.auth_attempt > 0 && self.ready.is_some() {
                GateStage::Negotiating
            } else {
                GateStage::Reconnecting
            };
        });
        if let Some(channels) = &self.channels {
            let path = self.path();
            let error = TransportError::VpnGate(GateFailure::Transport);
            channels.health.send_replace(RuntimeHealth::Reconnecting {
                last_path: path,
                attempt: self.reconnect_count.max(1),
                reconnect_count: self.reconnect_count,
                reason: error.to_string(),
                failure: error.failure(Some(path.transport), Some(path.endpoint_family)),
            });
        }
    }
}

fn publish_failure(
    status: &watch::Sender<GateStatus>,
    reason: GateFailure,
    startup_attempt: Option<u8>,
) {
    // Only pre-admission authentication retries are hidden. Established
    // failures and the last failed attempt still reach normal chain cleanup.
    if startup_attempt.is_some_and(|attempt| retry_startup_authentication(reason, attempt)) {
        return;
    }
    status.send_modify(|s| {
        s.stage = GateStage::Error;
        s.failure = Some(reason);
        s.network = None;
    });
}

fn event_failure(name: &str) -> GateFailure {
    match name {
        "AUTH_FAILED" => GateFailure::Authentication,
        "CERT_VERIFY_FAIL" | "TLS_VERSION_MIN" | "TLS_CERT_VERIFY_FAIL" | "TLS_ALERT" => {
            GateFailure::Certificate
        }
        "CLIENT_SETUP" | "TUN_SETUP_FAILED" | "OPTIONS_ERROR" | "UNUSED_OPTIONS" => {
            GateFailure::Configuration
        }
        "CONNECTION_TIMEOUT" | "INACTIVE_TIMEOUT" | "NETWORK_EOF_ERROR" | "NETWORK_RECV_ERROR"
        | "NETWORK_SEND_ERROR" | "TRANSPORT_ERROR" | "RESOLVE_ERROR" | "TCP_CONNECT_ERROR"
        | "UDP_CONNECT_ERROR" => GateFailure::Transport,
        _ => GateFailure::Protocol,
    }
}
fn final_network(config: NetworkConfig) -> Result<FinalNetworkParameters, GateFailure> {
    if !(1280..=9000).contains(&config.mtu)
        || config.ipv4.is_none() && config.ipv6.is_none()
        || config.ipv4.is_some_and(|ip| {
            ip.is_unspecified() || ip.is_loopback() || ip.is_multicast() || ip.is_broadcast()
        })
        || config
            .ipv6
            .is_some_and(|ip| ip.is_unspecified() || ip.is_loopback() || ip.is_multicast())
    {
        return Err(GateFailure::Configuration);
    }
    let mut network = FinalNetworkParameters {
        ipv4: config.ipv4,
        ipv6: config.ipv6,
        mtu: config.mtu,
        dns_servers: Vec::new(),
    };
    for ip in config.dns_servers {
        if network.supports(ip)
            && !ip.is_unspecified()
            && !ip.is_loopback()
            && !ip.is_multicast()
            && !matches!(ip, IpAddr::V4(address) if address.is_broadcast())
            && !network.dns_servers.contains(&ip)
        {
            network.dns_servers.push(ip);
        }
    }
    Ok(network)
}
fn packet_family_supported(network: &FinalNetworkParameters, packet: &[u8]) -> bool {
    match packet.first().map(|v| v >> 4) {
        Some(4) => network.ipv4.is_some(),
        Some(6) => network.ipv6.is_some(),
        _ => false,
    }
}

struct Connection {
    cancellation: CancellationToken,
    task: AbortOnDropHandle<()>,
}
impl Connection {
    #[expect(
        clippy::too_many_arguments,
        reason = "the WARP transport binds its endpoint, protocol, generation, independent output and cancellation"
    )]
    fn start(
        warp: InternalNetwork,
        remote: SocketAddr,
        udp: bool,
        input: Input,
        packets: EventStream,
        generation: u64,
        parent: &CancellationToken,
    ) -> Self {
        let cancellation = parent.child_token();
        let cancel = cancellation.clone();
        let task = tokio::spawn(async move {
            let work = async {
                if udp {
                    let socket = warp
                        .bind_udp(remote, &cancel)
                        .await
                        .map_err(std::io::Error::from)?;
                    input
                        .transport_connected(generation)
                        .await
                        .map_err(|_| std::io::ErrorKind::ConnectionAborted)?;
                    let receive = async {
                        loop {
                            let packet = socket.recv().await.map_err(std::io::Error::from)?;
                            input
                                .receive_transport_owned(generation, packet)
                                .await
                                .map_err(|_| std::io::ErrorKind::ConnectionAborted)?;
                        }
                        #[allow(unreachable_code)]
                        Ok::<(), std::io::Error>(())
                    };
                    let send = async {
                        loop {
                            let packet = packets
                                .transport(generation)
                                .await
                                .map_err(|_| std::io::ErrorKind::ConnectionAborted)?;
                            socket.send(&packet).await.map_err(std::io::Error::from)?;
                        }
                        #[allow(unreachable_code)]
                        Ok::<(), std::io::Error>(())
                    };
                    return tokio::select! { result = receive => result, result = send => result };
                }
                let stream = warp
                    .connect_address(remote, &cancel, Instant::now() + Duration::from_secs(15))
                    .await
                    .map_err(|error| {
                        tracing::warn!(gate_event = "TCP_DIAL_FAILED", failure_kind = ?error, "VPN Gate WARP dial failed");
                        std::io::Error::from(error)
                    })?;
                tracing::info!(
                    gate_event = "TCP_CONNECTED",
                    "VPN Gate WARP transport ready"
                );
                input
                    .transport_connected(generation)
                    .await
                    .map_err(|_| std::io::ErrorKind::ConnectionAborted)?;
                let (mut reader, mut writer) = tokio::io::split(stream);
                let sent_frames = std::sync::atomic::AtomicU64::new(0);
                let received_frames = std::sync::atomic::AtomicU64::new(0);
                let receive = async {
                    loop {
                        let packet = read_frame(&mut reader).await.inspect_err(|error| {
                            tracing::warn!(gate_event = "TCP_READ_FAILED", io_error_kind = ?error.kind(), "VPN Gate framed transport failed");
                        })?;
                        received_frames.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        input
                            .receive_transport(generation, &packet)
                            .await
                            .map_err(|_| std::io::ErrorKind::ConnectionAborted)?;
                    }
                    #[allow(unreachable_code)]
                    Ok::<(), std::io::Error>(())
                };
                let send = async {
                    loop {
                        let packet = packets
                            .transport(generation)
                            .await
                            .map_err(|_| std::io::ErrorKind::ConnectionAborted)?;
                        write_frame(&mut writer, &packet).await.inspect_err(|error| {
                            tracing::warn!(gate_event = "TCP_WRITE_FAILED", io_error_kind = ?error.kind(), "VPN Gate framed transport failed");
                        })?;
                        sent_frames.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    }
                    #[allow(unreachable_code)]
                    Ok::<(), std::io::Error>(())
                };
                let result = tokio::select! { result = receive => result, result = send => result };
                tracing::info!(
                    gate_event = "TCP_CLOSED",
                    sent_frames = sent_frames.load(std::sync::atomic::Ordering::Relaxed),
                    received_frames = received_frames.load(std::sync::atomic::Ordering::Relaxed),
                    "VPN Gate transport ended"
                );
                result
            };
            until_cancelled(&cancel, async {
                let _ = work.await;
                let _ = input.transport_failed(generation).await;
            })
            .await;
        });
        Self {
            cancellation,
            task: AbortOnDropHandle::new(task),
        }
    }
    async fn shutdown(self) {
        self.cancellation.cancel();
        let _ = self.task.await;
    }
}
async fn read_frame(reader: &mut (impl AsyncRead + Unpin)) -> std::io::Result<Vec<u8>> {
    let length = usize::from(reader.read_u16().await?);
    if length == 0 {
        return Err(std::io::ErrorKind::InvalidData.into());
    }
    let mut packet = vec![0; length];
    reader.read_exact(&mut packet).await?;
    Ok(packet)
}
async fn write_frame(writer: &mut (impl AsyncWrite + Unpin), packet: &[u8]) -> std::io::Result<()> {
    let length = u16::try_from(packet.len())
        .ok()
        .filter(|v| *v != 0)
        .ok_or(std::io::ErrorKind::InvalidData)?;
    writer.write_u16(length).await?;
    writer.write_all(packet).await?;
    writer.flush().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tcp::{DialError, FlowClass, TcpDialer, TcpIo, TcpStream, TcpTarget};
    use std::sync::Arc;

    #[tokio::test(start_paused = true)]
    async fn candidates_only_continue_after_completed_retriable_failures() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let calls = AtomicUsize::new(0);
        let cancel = CancellationToken::new();
        let deadline = Instant::now() + Duration::from_secs(120);
        let result = attempt_candidates(&cancel, deadline, &[3, 1, 2], |index, _| {
            calls.fetch_add(1, Ordering::SeqCst);
            async move {
                if index == 3 {
                    Err(TransportError::VpnGate(GateFailure::Transport))
                } else {
                    Ok(index)
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(result, 1);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        for failure in [
            GateFailure::Authentication,
            GateFailure::Certificate,
            GateFailure::Configuration,
            GateFailure::Protocol,
            GateFailure::Cleanup,
        ] {
            calls.store(0, Ordering::SeqCst);
            let result: Result<(), _> = attempt_candidates(&cancel, deadline, &[0, 1], |_, _| {
                calls.fetch_add(1, Ordering::SeqCst);
                async move { Err(TransportError::VpnGate(failure)) }
            })
            .await;
            assert!(matches!(result, Err(TransportError::VpnGate(reason)) if reason == failure));
            assert_eq!(calls.load(Ordering::SeqCst), 1);
        }
        let now = Instant::now();
        assert_eq!(
            candidate_deadline(now, deadline, 5).unwrap() - now,
            Duration::from_secs(24)
        );
        assert_eq!(
            candidate_deadline(now, deadline, 1).unwrap() - now,
            Duration::from_secs(35)
        );
        calls.store(0, Ordering::SeqCst);
        let _: Result<(), _> =
            attempt_candidates(&cancel, deadline, &[0, 1, 2, 3, 4], |_, until| {
                calls.fetch_add(1, Ordering::SeqCst);
                async move {
                    tokio::time::sleep_until(until).await;
                    Err(TransportError::VpnGate(GateFailure::Transport))
                }
            })
            .await;
        assert_eq!(Instant::now(), deadline);
        assert_eq!(calls.load(Ordering::SeqCst), 5);
        cancel.cancel();
        let _: Result<(), _> = attempt_candidates(&cancel, deadline, &[0], |_, _| async {
            panic!("cancelled connection started a worker")
        })
        .await;
        for random in [false, true] {
            let mut values = candidate_order(16, random);
            if !random {
                assert_eq!(values, (0..16).collect::<Vec<_>>());
            }
            values.sort_unstable();
            assert_eq!(values, (0..16).collect::<Vec<_>>());
        }
    }

    struct MemoryDialer {
        connected: bool,
        dialed: tokio::sync::Notify,
        peer: std::sync::Mutex<Option<oneshot::Sender<tokio::io::DuplexStream>>>,
    }
    impl TcpIo for tokio::io::DuplexStream {
        fn local_addr(&self) -> std::io::Result<SocketAddr> {
            Ok("127.0.0.1:12345".parse().unwrap())
        }
    }
    #[async_trait::async_trait]
    impl TcpDialer for MemoryDialer {
        async fn connect(
            &self,
            _: TcpTarget,
            _: Instant,
            _: &CancellationToken,
            _: FlowClass,
        ) -> Result<TcpStream, DialError> {
            self.dialed.notify_one();
            if !self.connected {
                return std::future::pending().await;
            }
            let (client, peer) = tokio::io::duplex(4096);
            self.peer
                .lock()
                .unwrap()
                .take()
                .unwrap()
                .send(peer)
                .unwrap();
            Ok(Box::new(client))
        }
    }

    #[tokio::test]
    async fn cancellation_closes_transport_during_backpressured_failure_notification() {
        let cancellation = CancellationToken::new();
        let cancel = cancellation.clone();
        let (stream, mut peer) = tokio::io::duplex(64);
        let (reporting, reported) = oneshot::channel();
        let task = tokio::spawn(async move {
            until_cancelled(&cancel, async move {
                // Model the selected TCP-failure branch with a full native
                // queue. The stream must close without that queue progressing.
                let _stream = stream;
                reporting.send(()).unwrap();
                std::future::pending::<()>().await;
            })
            .await;
        });
        let connection = Connection {
            cancellation,
            task: AbortOnDropHandle::new(task),
        };
        reported.await.unwrap();
        tokio::time::timeout(Duration::from_secs(1), connection.shutdown())
            .await
            .expect("failure notification must remain cancellable");
        assert_eq!(peer.read(&mut [0; 1]).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn cancellation_drops_an_event_before_it_can_publish_a_replacement() {
        let cancel = CancellationToken::new();
        let (entered, entering) = oneshot::channel();
        let (resume, waiting) = oneshot::channel::<()>();
        let operation = until_cancelled(&cancel, async {
            entered.send(()).unwrap();
            waiting.await.unwrap();
            panic!("cancelled event must not install a replacement connection");
        });
        let cancellation = async {
            entering.await.unwrap();
            cancel.cancel();
        };
        let (result, ()) = tokio::join!(operation, cancellation);
        assert!(result.is_none());
        assert!(resume.send(()).is_err());
    }

    #[tokio::test]
    async fn cancellation_joins_worker_during_dial_and_tls_wait() {
        let ca = include_str!("../../usque-openvpn/tests/fixtures/ca.crt");
        let cert = include_str!("../../usque-openvpn/tests/fixtures/client.crt");
        let key = include_str!("../../usque-openvpn/tests/fixtures/client.key");
        let prepared = usque_core::vpngate::prepare_profile(
            &format!("client\ndev tun\nproto tcp\nremote 8.8.8.8 1194\ncipher AES-128-CBC\nauth SHA1\n<ca>\n{ca}</ca>\n<cert>\n{cert}</cert>\n<key>\n{key}</key>\n"),
            "8.8.8.8".parse().unwrap(),
        ).unwrap();
        for (connected, auth_attempt) in [false, true].into_iter().flat_map(|connected| {
            (0..=AUTHENTICATION_RETRIES).map(move |attempt| (connected, attempt))
        }) {
            let (peer_tx, peer_rx) = oneshot::channel();
            let dialer = Arc::new(MemoryDialer {
                connected,
                dialed: tokio::sync::Notify::new(),
                peer: std::sync::Mutex::new(Some(peer_tx)),
            });
            let (health, receiver) = watch::channel(RuntimeHealth::Connected {
                path: RuntimePath {
                    transport: usque_core::Transport::Http3,
                    endpoint_family: usque_core::AddressFamily::Ipv4,
                    ipv4_available: true,
                    ipv6_available: false,
                },
                reconnect_count: 0,
            });
            let warp =
                InternalNetwork::for_streams(dialer.clone(), receiver, CancellationToken::new());
            let cancel = CancellationToken::new();
            let startup = with_authentication_retries(&cancel, |attempt| {
                let warp = warp.clone();
                let prepared = &prepared;
                let cancel = &cancel;
                async move {
                    if attempt < auth_attempt {
                        return Err(TransportError::VpnGate(GateFailure::Authentication));
                    }
                    GateDriver::start_attempt(
                        prepared,
                        warp,
                        crate::NetworkQualityTelemetry::default(),
                        watch::channel(GateStatus::default()).0,
                        cancel,
                        attempt,
                        None,
                        Instant::now() + Duration::from_secs(180),
                    )
                    .await
                }
            });
            tokio::pin!(startup);
            let stop = async {
                dialer.dialed.notified().await;
                let mut peer = if connected {
                    let mut peer = peer_rx.await.unwrap();
                    assert_eq!(read_frame(&mut peer).await.unwrap().len(), 14);
                    Some(peer)
                } else {
                    None
                };
                cancel.cancel();
                if let Some(peer) = &mut peer {
                    assert_eq!(
                        peer.read(&mut [0; 1]).await.unwrap(),
                        0,
                        "cancel closes the WARP stream"
                    );
                }
            };
            let (result, ()) = tokio::time::timeout(Duration::from_secs(3), async {
                tokio::join!(startup, stop)
            })
            .await
            .expect("cancel must not wait for the 35-second startup deadline");
            assert!(matches!(result, Err(TransportError::TunnelClosed)));
            drop(health);
        }
    }
    #[tokio::test]
    async fn framing_survives_partial_writes_and_consecutive_frames() {
        let (mut writer, mut reader) = tokio::io::duplex(3);
        let sender = tokio::spawn(async move {
            write_frame(&mut writer, &[7; 100]).await.unwrap();
            write_frame(&mut writer, &[8; 9]).await.unwrap();
        });
        assert_eq!(read_frame(&mut reader).await.unwrap(), vec![7; 100]);
        assert_eq!(read_frame(&mut reader).await.unwrap(), vec![8; 9]);
        sender.await.unwrap();
        assert!(read_frame(&mut reader).await.is_err());
    }
    #[tokio::test]
    async fn rejects_empty_and_truncated_frames() {
        assert!(read_frame(&mut &[0_u8, 0][..]).await.is_err());
        assert!(read_frame(&mut &[0_u8, 5, 1, 2][..]).await.is_err());
        assert!(write_frame(&mut tokio::io::sink(), &[]).await.is_err());
    }
    #[test]
    fn final_addresses_and_address_family_gate_do_not_inherit_warp_addresses() {
        let network = final_network(NetworkConfig {
            ipv4: Some("10.8.0.2".parse().unwrap()),
            ipv6: None,
            mtu: 1500,
            dns_servers: vec![
                "1.1.1.1".parse::<IpAddr>().unwrap(),
                "2606:4700:4700::1111".parse().unwrap(),
            ],
        })
        .unwrap();
        assert_eq!(network.dns_servers.len(), 1);
        assert!(packet_family_supported(&network, &[0x45]));
        assert!(!packet_family_supported(&network, &[0x60]));
        assert!(!event_failure("AUTH_FAILED").retryable());
        assert!(!event_failure("CERT_VERIFY_FAIL").retryable());
    }
    #[test]
    fn broadcast_only_pushed_dns_leaves_the_configured_fallback_available() {
        let network = final_network(NetworkConfig {
            ipv4: Some("10.8.0.2".parse().unwrap()),
            ipv6: None,
            mtu: 1500,
            dns_servers: vec!["255.255.255.255".parse().unwrap()],
        })
        .unwrap();
        // install_gate uses the configured DNS servers when this list is empty.
        assert!(network.dns_servers.is_empty());
    }
    #[test]
    fn filters_broadcast_dns_without_discarding_private_or_public_resolvers() {
        let private: IpAddr = "10.8.0.1".parse().unwrap();
        let public: IpAddr = "1.1.1.1".parse().unwrap();
        let network = final_network(NetworkConfig {
            ipv4: Some("10.8.0.2".parse().unwrap()),
            ipv6: None,
            mtu: 1500,
            dns_servers: vec![
                "255.255.255.255".parse().unwrap(),
                private,
                public,
                private,
                "2606:4700:4700::1111".parse().unwrap(),
            ],
        })
        .unwrap();
        assert_eq!(network.dns_servers, vec![private, public]);
    }
}
