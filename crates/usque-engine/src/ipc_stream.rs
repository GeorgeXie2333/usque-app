use std::{io, sync::Arc, time::Duration};

use bytes::BytesMut;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use usque_ipc::{decode_frame, split_frame, v1::ControlRequest};

use crate::ControlService;
use zeroize::{Zeroize, Zeroizing};

const READ_CHUNK_SIZE: usize = 16 * 1024;

#[cfg(any(windows, test))]
pub(crate) async fn handle_stream<Stream>(
    stream: Stream,
    service: Arc<ControlService>,
) -> io::Result<()>
where
    Stream: AsyncRead + AsyncWrite + Unpin,
{
    handle_stream_with_timeouts(stream, service, None).await
}

pub(crate) async fn handle_stream_with_timeouts<Stream>(
    mut stream: Stream,
    service: Arc<ControlService>,
    idle_timeout: Option<Duration>,
) -> io::Result<()>
where
    Stream: AsyncRead + AsyncWrite + Unpin,
{
    let mut buffer = BytesMut::new();
    let mut chunk = [0_u8; READ_CHUNK_SIZE];

    loop {
        let read = match idle_timeout {
            Some(timeout) => tokio::time::timeout(timeout, stream.read(&mut chunk))
                .await
                .map_err(|_| {
                    io::Error::new(io::ErrorKind::TimedOut, "IPC frame read timed out")
                })??,
            None => stream.read(&mut chunk).await?,
        };
        if read == 0 {
            return if buffer.is_empty() {
                Ok(())
            } else {
                Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "IPC stream closed during a protobuf frame",
                ))
            };
        }
        buffer.extend_from_slice(&chunk[..read]);

        loop {
            let Some(frame) = split_frame(&mut buffer).map_err(invalid_wire)? else {
                break;
            };
            let request: ControlRequest = decode_frame(frame).map_err(invalid_wire)?;
            let mut reply = service.handle(request).await;
            let response = encode_sensitive_response(&reply)?;
            if let Some(usque_ipc::v1::control_response::Payload::SensitiveClipboard(value)) =
                &mut reply.payload
            {
                value.utf8.zeroize();
            }
            if let Some(timeout) = idle_timeout {
                tokio::time::timeout(timeout, stream.write_all(&response))
                    .await
                    .map_err(|_| {
                        io::Error::new(io::ErrorKind::TimedOut, "IPC response write timed out")
                    })??;
            } else {
                stream.write_all(&response).await?;
            }
        }
    }
}

fn encode_sensitive_response(
    reply: &usque_ipc::v1::ControlResponse,
) -> io::Result<Zeroizing<Vec<u8>>> {
    usque_ipc::encode_frame_vec(reply)
        .map(Zeroizing::new)
        .map_err(invalid_wire)
}

fn invalid_wire(error: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}

#[cfg(test)]
mod tests {
    use tokio::io::{AsyncReadExt, AsyncWriteExt, duplex};
    use usque_core::storage::ConfigStore;
    use usque_ipc::{
        MAX_FRAME_SIZE, decode_frame, encode_frame,
        v1::{ControlRequest, ControlResponse, GetStatusRequest, control_request},
    };

    use super::*;

    #[test]
    fn clipboard_payload_is_only_in_the_authenticated_binary_reply() {
        let response = ControlResponse {
            request_id: "clipboard".into(),
            error: None,
            payload: Some(
                usque_ipc::v1::control_response::Payload::SensitiveClipboard(
                    usque_ipc::v1::SensitiveClipboardValue {
                        utf8: b"sensitive-license-token".to_vec(),
                        clear_after_seconds: 30,
                    },
                ),
            ),
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(!json.contains("utf8"));
        assert!(!json.contains("sensitive-license-token"));
        let debug = format!("{response:?}");
        assert!(debug.contains("REDACTED"));
        assert!(!debug.contains("115, 101, 110"));
        let frame = encode_sensitive_response(&response).unwrap();
        let decoded: ControlResponse = decode_frame(bytes::Bytes::copy_from_slice(&frame)).unwrap();
        let Some(usque_ipc::v1::control_response::Payload::SensitiveClipboard(value)) =
            decoded.payload
        else {
            panic!("clipboard response expected")
        };
        assert_eq!(value.utf8, b"sensitive-license-token");
        assert_eq!(value.clear_after_seconds, 30);
    }

    fn status_request(request_id: &str) -> ControlRequest {
        ControlRequest {
            request_id: request_id.to_owned(),
            payload: Some(control_request::Payload::GetStatus(GetStatusRequest {})),
        }
    }

    async fn read_response(stream: &mut (impl AsyncRead + Unpin)) -> io::Result<ControlResponse> {
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
    async fn accepts_fragmented_and_back_to_back_frames() {
        let (mut client, server) = duplex(128 * 1024);
        let task = tokio::spawn(handle_stream(server, service()));
        let first = encode_frame(&status_request("one")).expect("first");
        let second = encode_frame(&status_request("two")).expect("second");

        client.write_all(&first[..2]).await.expect("fragment");
        client.write_all(&first[2..]).await.expect("remainder");
        client.write_all(&second).await.expect("second frame");

        assert_eq!(
            read_response(&mut client)
                .await
                .expect("first response")
                .request_id,
            "one"
        );
        assert_eq!(
            read_response(&mut client)
                .await
                .expect("second response")
                .request_id,
            "two"
        );

        client.shutdown().await.expect("shutdown");
        task.await.expect("join").expect("server");
    }

    #[tokio::test]
    async fn rejects_fragmented_oversized_header_without_waiting_for_body_or_eof() {
        let (mut client, server) = duplex(2);
        let task = tokio::spawn(handle_stream(server, service()));
        let header = (MAX_FRAME_SIZE as u32 + 1).to_be_bytes();

        client
            .write_all(&header[..2])
            .await
            .expect("partial header");
        // A two-byte pipe forces the reader to consume the first fragment
        // before this write can finish. Keep it open without sending a body.
        client
            .write_all(&header[2..])
            .await
            .expect("complete header");

        assert_eq!(
            tokio::time::timeout(std::time::Duration::from_secs(5), task)
                .await
                .expect("oversized header must be rejected before a body or EOF")
                .expect("join")
                .expect_err("must reject oversized frame")
                .kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[tokio::test]
    async fn rejects_truncated_frame_at_eof() {
        let (mut client, server) = duplex(1024);
        let task = tokio::spawn(handle_stream(server, service()));
        client
            .write_all(&encode_frame(&status_request("cut")).expect("frame")[..5])
            .await
            .expect("partial write");
        client.shutdown().await.expect("shutdown");

        assert_eq!(
            task.await
                .expect("join")
                .expect_err("must reject truncation")
                .kind(),
            io::ErrorKind::UnexpectedEof
        );
    }
}
