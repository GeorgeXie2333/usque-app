use std::{io, sync::Arc};

use bytes::BytesMut;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use usque_ipc::{MAX_FRAME_SIZE, decode_frame, encode_frame, split_frame, v1::ControlRequest};

use crate::ControlService;

const READ_CHUNK_SIZE: usize = 16 * 1024;

pub(crate) async fn handle_stream<Stream>(
    mut stream: Stream,
    service: Arc<ControlService>,
) -> io::Result<()>
where
    Stream: AsyncRead + AsyncWrite + Unpin,
{
    let mut buffer = BytesMut::new();
    let mut chunk = [0_u8; READ_CHUNK_SIZE];

    loop {
        let read = stream.read(&mut chunk).await?;
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
                if buffer.len() > MAX_FRAME_SIZE + 4 {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "IPC frame buffer exceeded its bound",
                    ));
                }
                break;
            };
            let request: ControlRequest = decode_frame(frame).map_err(invalid_wire)?;
            let response = service.handle(request).await;
            let response = encode_frame(&response).map_err(invalid_wire)?;
            stream.write_all(&response).await?;
        }
    }
}

fn invalid_wire(error: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}

#[cfg(test)]
mod tests {
    use tokio::io::{AsyncReadExt, AsyncWriteExt, duplex};
    use usque_core::storage::ConfigStore;
    use usque_ipc::{
        decode_frame, encode_frame,
        v1::{ControlRequest, ControlResponse, GetStatusRequest, control_request},
    };

    use super::*;

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
