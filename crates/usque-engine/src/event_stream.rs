use std::{io, sync::Arc, time::Duration};

use tokio::{
    io::{AsyncWrite, AsyncWriteExt},
    time::{Instant, MissedTickBehavior, interval},
};
use usque_ipc::{
    encode_frame,
    v1::{self, EventEnvelope, event_envelope},
};

use crate::diagnostics::{self, DiagnosticEvent};
use crate::{ControlService, current_capabilities};

const SNAPSHOT_INTERVAL: Duration = Duration::from_secs(1);

/// Streams versioned protobuf events over an already-authenticated IPC
/// connection. The stream is intentionally separate from request/response IPC:
/// a slow UI event consumer can never stall a control command.
pub(crate) async fn handle_event_stream<Stream>(
    mut stream: Stream,
    service: Arc<ControlService>,
) -> io::Result<()>
where
    Stream: AsyncWrite + Unpin,
{
    write_event(
        &mut stream,
        EventEnvelope {
            sequence: service.next_event_sequence(),
            payload: Some(event_envelope::Payload::CapabilitiesChanged(
                v1::CapabilitiesChanged {
                    capabilities: Some(current_capabilities()),
                },
            )),
        },
    )
    .await?;

    let mut ticker = interval(SNAPSHOT_INTERVAL);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut geo_progress = service.subscribe_geo_progress();
    let mut diagnostics = service.subscribe_diagnostics();
    let mut quality_updates = service.subscribe_network_quality();
    let initial_quality =
        crate::network_quality::snapshot_to_proto(&quality_updates.borrow_and_update().clone());
    let mut quality_gate = NetworkQualityEventGate::new(initial_quality);

    loop {
        tokio::select! {
            _ = ticker.tick() => {
                let snapshot = service.event_snapshot().await;
                // A snapshot is intentionally emitted every second even when the
                // state is unchanged. Besides keeping UI rates current, the write is
                // the liveness check for this strictly one-way pipe.
                write_event(
                    &mut stream,
                    EventEnvelope {
                        sequence: service.next_event_sequence(),
                        payload: Some(event_envelope::Payload::StateChanged(Box::new(
                            v1::StateChanged {
                                snapshot: Some(snapshot),
                            },
                        ))),
                    },
                )
                .await?;
                if let Some(snapshot) = quality_gate.take_periodic(Instant::now()) {
                    write_network_quality_event(&mut stream, &service, snapshot).await?;
                }
            }
            changed = quality_updates.changed() => {
                if changed.is_err() {
                    return Ok(());
                }
                let snapshot = crate::network_quality::snapshot_to_proto(
                    &quality_updates.borrow_and_update().clone(),
                );
                if let Some(snapshot) = quality_gate.observe(snapshot, Instant::now()) {
                    write_network_quality_event(&mut stream, &service, snapshot).await?;
                }
            }
            progress = geo_progress.recv() => {
                match progress {
                    Ok(progress) => {
                        write_event(
                            &mut stream,
                            EventEnvelope {
                                sequence: service.next_event_sequence(),
                                payload: Some(event_envelope::Payload::GeoRulesProgress(progress)),
                            },
                        )
                        .await?;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return Ok(()),
                }
            }
            event = diagnostics.recv() => {
                match event {
                    Ok(event) => {
                        write_event(
                            &mut stream,
                            EventEnvelope {
                                sequence: service.next_event_sequence(),
                                payload: Some(diagnostic_event_payload(event)),
                            },
                        )
                        .await?;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return Ok(()),
                }
            }
        }
    }
}

struct NetworkQualityEventGate {
    last_observed: v1::NetworkQualitySnapshot,
    last_sent: Option<v1::NetworkQualitySnapshot>,
    last_sent_at: Option<Instant>,
    pending: Option<v1::NetworkQualitySnapshot>,
}

impl NetworkQualityEventGate {
    fn new(initial: v1::NetworkQualitySnapshot) -> Self {
        Self {
            last_observed: initial.clone(),
            last_sent: None,
            last_sent_at: None,
            pending: Some(initial),
        }
    }

    fn observe(
        &mut self,
        snapshot: v1::NetworkQualitySnapshot,
        now: Instant,
    ) -> Option<v1::NetworkQualitySnapshot> {
        let major = crate::network_quality::is_major_change(&self.last_observed, &snapshot);
        self.last_observed = snapshot.clone();
        if self
            .last_sent
            .as_ref()
            .is_some_and(|last| crate::network_quality::same_content(last, &snapshot))
        {
            self.pending = None;
            return None;
        }
        self.pending = Some(snapshot);
        if major && self.can_send(now) {
            return self.take_pending(now);
        }
        None
    }

    fn take_periodic(&mut self, now: Instant) -> Option<v1::NetworkQualitySnapshot> {
        self.can_send(now).then(|| self.take_pending(now)).flatten()
    }

    fn can_send(&self, now: Instant) -> bool {
        self.pending.is_some()
            && self
                .last_sent_at
                .is_none_or(|last| now.saturating_duration_since(last) >= SNAPSHOT_INTERVAL)
    }

    fn take_pending(&mut self, now: Instant) -> Option<v1::NetworkQualitySnapshot> {
        let snapshot = self.pending.take()?;
        self.last_sent = Some(snapshot.clone());
        self.last_sent_at = Some(now);
        Some(snapshot)
    }
}

async fn write_network_quality_event(
    writer: &mut (impl AsyncWrite + Unpin),
    service: &ControlService,
    snapshot: v1::NetworkQualitySnapshot,
) -> io::Result<()> {
    write_event(
        writer,
        EventEnvelope {
            sequence: service.next_event_sequence(),
            payload: Some(event_envelope::Payload::NetworkQualityUpdated(Box::new(
                v1::NetworkQualityUpdated {
                    snapshot: Some(Box::new(snapshot)),
                },
            ))),
        },
    )
    .await
}

fn diagnostic_event_payload(event: DiagnosticEvent) -> event_envelope::Payload {
    match event {
        DiagnosticEvent::SessionStarted(session) => {
            event_envelope::Payload::DiagnosticSessionStarted(v1::DiagnosticSessionStarted {
                session: Some(diagnostics::session_to_proto(&session)),
            })
        }
        DiagnosticEvent::CheckStarted {
            session_id,
            finding,
        } => event_envelope::Payload::DiagnosticCheckStarted(v1::DiagnosticCheckStarted {
            session_id: session_id.to_string(),
            finding: Some(diagnostics::finding_to_proto(&finding)),
        }),
        DiagnosticEvent::CheckCompleted {
            session_id,
            finding,
        } => event_envelope::Payload::DiagnosticCheckCompleted(v1::DiagnosticCheckCompleted {
            session_id: session_id.to_string(),
            finding: Some(diagnostics::finding_to_proto(&finding)),
        }),
        DiagnosticEvent::SessionCompleted(session) => {
            event_envelope::Payload::DiagnosticSessionCompleted(v1::DiagnosticSessionCompleted {
                session: Some(diagnostics::session_to_proto(&session)),
            })
        }
        DiagnosticEvent::SessionCancelled(session) => {
            event_envelope::Payload::DiagnosticSessionCancelled(v1::DiagnosticSessionCancelled {
                session: Some(diagnostics::session_to_proto(&session)),
            })
        }
    }
}

async fn write_event(
    writer: &mut (impl AsyncWrite + Unpin),
    event: EventEnvelope,
) -> io::Result<()> {
    let frame = encode_frame(&event).map_err(invalid_wire)?;
    writer.write_all(&frame).await
}

fn invalid_wire(error: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}

#[cfg(test)]
mod tests {
    use bytes::BytesMut;
    use tokio::{
        io::{AsyncRead, AsyncReadExt, duplex},
        time::timeout,
    };
    use usque_core::storage::ConfigStore;
    use usque_ipc::{
        decode_frame,
        v1::{EventEnvelope, event_envelope},
    };

    use super::*;

    async fn read_event(stream: &mut (impl AsyncRead + Unpin)) -> io::Result<EventEnvelope> {
        let mut header = [0_u8; 4];
        stream.read_exact(&mut header).await?;
        let mut payload = vec![0_u8; u32::from_be_bytes(header) as usize];
        stream.read_exact(&mut payload).await?;
        let mut frame = BytesMut::from(header.as_slice());
        frame.extend_from_slice(&payload);
        decode_frame(frame.freeze()).map_err(invalid_wire)
    }

    fn service() -> Arc<ControlService> {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.keep().join("config.json");
        Arc::new(ControlService::open(ConfigStore::new(path)).expect("service"))
    }

    #[tokio::test]
    async fn streams_capabilities_then_continuous_snapshots() {
        let (mut client, server) = duplex(128 * 1024);
        let task = tokio::spawn(handle_event_stream(server, service()));

        let capabilities = read_event(&mut client).await.expect("capabilities event");
        assert!(matches!(
            capabilities.payload,
            Some(event_envelope::Payload::CapabilitiesChanged(_))
        ));

        let mut previous_sequence = capabilities.sequence;
        let mut state_count = 0;
        let mut quality_count = 0;
        while state_count < 3 {
            let event = read_event(&mut client).await.expect("stream event");
            assert!(event.sequence > previous_sequence);
            previous_sequence = event.sequence;
            match event.payload {
                Some(event_envelope::Payload::StateChanged(_)) => state_count += 1,
                Some(event_envelope::Payload::NetworkQualityUpdated(_)) => quality_count += 1,
                payload => panic!("unexpected stream payload: {payload:?}"),
            }
        }
        assert_eq!(quality_count, 1, "unchanged quality emits only once");

        drop(client);
        let error = timeout(Duration::from_secs(2), task)
            .await
            .expect("event writer notices a closed reader")
            .expect("join")
            .expect_err("closed reader must stop the writer");
        assert!(matches!(
            error.kind(),
            io::ErrorKind::BrokenPipe
                | io::ErrorKind::ConnectionReset
                | io::ErrorKind::UnexpectedEof
        ));
    }

    fn quality_snapshot(failures: u64, pmtu_changes: u64) -> v1::NetworkQualitySnapshot {
        v1::NetworkQualitySnapshot {
            level: v1::NetworkQualityLevel::Good as i32,
            pmtu: Some(v1::PmtuQuality {
                change_count: pmtu_changes,
                ..v1::PmtuQuality::default()
            }),
            migration: Some(v1::MigrationQuality {
                phase_code: "idle".to_owned(),
                ..v1::MigrationQuality::default()
            }),
            direct_dns: Some(v1::DirectDnsQuality {
                phase_code: "system".to_owned(),
                failure_count: failures,
                ..v1::DirectDnsQuality::default()
            }),
            ..v1::NetworkQualitySnapshot::default()
        }
    }

    #[tokio::test(start_paused = true)]
    async fn quality_gate_is_one_hertz_and_major_changes_are_due_immediately() {
        let start = Instant::now();
        let initial = quality_snapshot(0, 0);
        let mut gate = NetworkQualityEventGate::new(initial.clone());
        assert_eq!(gate.take_periodic(start), Some(initial.clone()));
        assert_eq!(gate.observe(initial, start), None);

        let mut timestamp_only = quality_snapshot(0, 0);
        timestamp_only.sampled_at_unix_ms = 999;
        assert_eq!(gate.observe(timestamp_only, start), None);

        tokio::time::advance(SNAPSHOT_INTERVAL).await;
        let major = quality_snapshot(0, 1);
        assert_eq!(gate.observe(major.clone(), Instant::now()), Some(major));

        let ordinary = quality_snapshot(1, 1);
        assert_eq!(gate.observe(ordinary.clone(), Instant::now()), None);
        tokio::time::advance(SNAPSHOT_INTERVAL - Duration::from_millis(1)).await;
        assert_eq!(gate.take_periodic(Instant::now()), None);
        tokio::time::advance(Duration::from_millis(1)).await;
        assert_eq!(gate.take_periodic(Instant::now()), Some(ordinary));

        tokio::time::advance(SNAPSHOT_INTERVAL).await;
        let mut first_drop = quality_snapshot(1, 1);
        first_drop.queues.push(v1::QueueQuality {
            drop_items: 1,
            ..v1::QueueQuality::default()
        });
        assert_eq!(
            gate.observe(first_drop.clone(), Instant::now()),
            Some(first_drop)
        );
    }

    #[test]
    fn slow_quality_consumer_retains_only_the_latest_snapshot() {
        let start = Instant::now();
        let mut gate = NetworkQualityEventGate::new(quality_snapshot(0, 0));
        gate.take_periodic(start).expect("initial snapshot");
        for failures in 1..=10_000 {
            assert_eq!(gate.observe(quality_snapshot(failures, 0), start), None);
        }
        assert_eq!(
            gate.pending
                .as_ref()
                .and_then(|snapshot| snapshot.direct_dns.as_ref())
                .map(|quality| quality.failure_count),
            Some(10_000)
        );
    }
}
