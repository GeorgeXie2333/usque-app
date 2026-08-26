use std::{io, sync::Arc, time::Duration};

use tokio::{
    io::{AsyncWrite, AsyncWriteExt},
    time::{MissedTickBehavior, interval},
};
use usque_ipc::{
    encode_frame,
    v1::{self, EventEnvelope, event_envelope},
};

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
        for _ in 0..3 {
            let state = read_event(&mut client).await.expect("state event");
            assert!(matches!(
                state.payload,
                Some(event_envelope::Payload::StateChanged(_))
            ));
            assert!(state.sequence > previous_sequence);
            previous_sequence = state.sequence;
        }

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
}
