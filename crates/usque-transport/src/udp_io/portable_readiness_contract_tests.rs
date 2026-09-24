//! Portable send contract checks. The readiness transition below is injected;
//! it is not evidence of a naturally occurring actor deadlock.
use super::*;
use crate::network_quality::NetworkQualitySampler;
use std::cell::Cell;
use std::time::Duration;
use tokio::time::timeout;

fn item<'a>(payload: &'a [u8], source: SocketAddr, destination: SocketAddr) -> SendDatagram<'a> {
    SendDatagram {
        payload,
        source,
        destination,
        due_at: Instant::now(),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn outer_readiness_admission_survives_an_injected_cache_transition() {
    timeout(Duration::from_secs(5), async {
        for bind in ["127.0.0.1:0", "[::1]:0"] {
            let sender = UdpSocket::bind(bind).await.unwrap();
            let receiver = UdpSocket::bind(bind).await.unwrap();
            let from = sender.local_addr().unwrap();
            let to = receiver.local_addr().unwrap();
            let quality = NetworkQualityTelemetry::default();
            let batch = [item(b"sequence-1", from, to), item(b"sequence-2", from, to)];
            let entered = Cell::new(false);
            let invalidated = Cell::new(false);
            sender.writable().await.unwrap();
            let result = sender.try_io(Interest::WRITABLE, || {
                entered.set(true);
                // Deliberate fault injection: revoke the cached flag after the
                // REAL outer try_io admitted its raw-I/O closure, without
                // changing the writable kernel socket. Returning WouldBlock
                // without a syscall is invalid for NORMAL callers and is used
                // only to control this regression's readiness state. No await
                // or reactor turn occurs between invalidation and the send.
                let clear: io::Result<()> = sender.try_io(Interest::WRITABLE, || {
                    invalidated.set(true);
                    Err(io::ErrorKind::WouldBlock.into())
                });
                assert_eq!(clear.unwrap_err().kind(), io::ErrorKind::WouldBlock);
                portable::try_send_batch(&sender, &batch, &quality)
            });
            assert!(entered.get() && invalidated.get());
            assert_eq!(
                result
                    .expect("the admitted closure must perform raw I/O, not recheck Tokio's cache"),
                2
            );
            for expected in [b"sequence-1".as_slice(), b"sequence-2"] {
                let mut bytes = [0; 32];
                let (length, actual_from) = receiver.recv_from(&mut bytes).await.unwrap();
                assert_eq!(actual_from, from);
                assert_eq!(&bytes[..length], expected);
            }
            let snapshot = NetworkQualitySampler::new(quality).sample();
            assert_eq!(snapshot.udp_io.send_syscalls, 2);
            assert_eq!(snapshot.udp_io.sent_datagrams, 2);
        }
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn outer_portable_send_preserves_prefix_on_real_emsgsize_and_resumes_tail() {
    timeout(Duration::from_secs(5), async {
        for bind in ["127.0.0.1:0", "[::1]:0"] {
            let quality = NetworkQualityTelemetry::default();
            let sender = UdpBatchIo::with_mode(
                UdpSocket::bind(bind).await.unwrap(),
                UdpBatchMode::Portable,
                quality.clone(),
            )
            .unwrap();
            let receiver = UdpSocket::bind(bind).await.unwrap();
            let from = sender.local_addr();
            let to = receiver.local_addr().unwrap();
            // Exceeds both IPv4 and IPv6 UDP payload limits: an actual kernel
            // EMSGSIZE after one successful prefix, not a scripted send count.
            let oversized = vec![7; 70_000];
            let batch = [
                item(b"first", from, to),
                item(&oversized, from, to),
                item(b"last", from, to),
            ];
            let cancel = CancellationToken::new();
            assert_eq!(sender.send_batch(&[], &cancel).await.unwrap(), 0);
            assert_eq!(sender.send_batch(&batch, &cancel).await.unwrap(), 1);
            let failure = sender.send_batch(&batch[1..], &cancel).await.unwrap_err();
            assert!(is_message_too_long(&failure));
            assert_eq!(sender.mode(), UdpBatchMode::Portable);
            assert!(sender.fallback_reason().is_none());
            // A cancelled retry must not send the retained valid tail.
            let cancelled = CancellationToken::new();
            cancelled.cancel();
            let error = sender
                .send_batch(&batch[2..], &cancelled)
                .await
                .unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::Interrupted);
            assert_eq!(sender.send_batch(&batch[2..], &cancel).await.unwrap(), 1);
            for expected in [b"first".as_slice(), b"last"] {
                let mut bytes = [0; 32];
                let (length, actual_from) = receiver.recv_from(&mut bytes).await.unwrap();
                assert_eq!(actual_from, from);
                assert_eq!(&bytes[..length], expected);
            }
            let snapshot = NetworkQualitySampler::new(quality).sample();
            assert_eq!(snapshot.udp_io.sent_datagrams, 2);
            assert_eq!(snapshot.udp_io.send_syscalls, 4); // two successes, two real EMSGSIZE attempts
            assert_eq!(snapshot.udp_io.batch_fallbacks, 0);
        }
    })
    .await
    .unwrap();
}
