//! A bounded Darwin packet bridge; one upload and one download may wait.
use bytes::Bytes;
use std::{
    io,
    os::fd::{AsRawFd, OwnedFd},
};
use tokio::{
    io::unix::AsyncFd,
    sync::{oneshot, watch},
};
use tokio_util::{sync::CancellationToken, task::AbortOnDropHandle};
use usque_platform::darwin_packet;
use usque_transport::{TransportError, TunPacketIo};

type Upload =
    std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), TransportError>> + Send>>;
trait PacketTransport: Send + 'static {
    fn send(&self, packet: Bytes) -> Upload;
    fn receive(
        &mut self,
    ) -> impl std::future::Future<Output = Result<Bytes, TransportError>> + Send;
}
impl PacketTransport for TunPacketIo {
    fn send(&self, packet: Bytes) -> Upload {
        Box::pin(self.start_send_owned_packet(packet))
    }
    async fn receive(&mut self) -> Result<Bytes, TransportError> {
        self.receive_packet().await
    }
}
pub async fn spawn(
    transport: TunPacketIo,
    descriptor: OwnedFd,
    cancel: CancellationToken,
    failure: watch::Sender<Option<String>>,
) -> io::Result<AbortOnDropHandle<()>> {
    spawn_with(transport, descriptor, cancel, failure).await
}
async fn spawn_with(
    mut transport: impl PacketTransport,
    descriptor: OwnedFd,
    cancel: CancellationToken,
    failure: watch::Sender<Option<String>>,
) -> io::Result<AbortOnDropHandle<()>> {
    // SAFETY: the registration owns this nonblocking FD until deregistration;
    // the pump never replaces or closes it while AsyncFd is alive.
    let device = unsafe { AsyncFd::register(descriptor) }.map_err(io::Error::from)?;
    let (ready_tx, ready_rx) = oneshot::channel();
    let task = AbortOnDropHandle::new(tokio::spawn(async move {
        let mut inbound = vec![0; darwin_packet::MAX_IP_PACKET + darwin_packet::HEADER_BYTES];
        let mut outbound: Option<Vec<u8>> = None;
        let mut upload: Option<Upload> = None;
        if ready_tx.send(()).is_err() {
            return;
        }
        let result = async { loop {
            tokio::select! {
                biased;
                () = cancel.cancelled() => return Ok::<(), io::Error>(()),
                result = async { match upload.as_mut() { Some(send) => send.await, None => std::future::pending().await } }, if upload.is_some() => { result.map_err(io::Error::other)?; upload = None; },
                result = read_packet(&device, &mut inbound), if upload.is_none() => { let size = result?; let packet = darwin_packet::decode(&inbound[..size])?; upload = Some(transport.send(Bytes::copy_from_slice(packet))); },
                result = transport.receive(), if outbound.is_none() => { outbound = Some(darwin_packet::encode(&result.map_err(io::Error::other)?)?); },
                result = async { match outbound.as_ref() { Some(packet) => write_packet(&device, packet).await, None => std::future::pending().await } }, if outbound.is_some() => { result?; outbound = None; },
            }
        }}.await;
        if let Err(error) = result
            && !cancel.is_cancelled()
        {
            crate::macos_agent_runtime::log_tun_failure("packet_io", &error);
            failure.send_replace(Some(
                "macOS packet transport failed; recovery is required".into(),
            ));
            cancel.cancel();
        }
    }));
    ready_rx.await.map_err(io::Error::other)?;
    Ok(task)
}
async fn read_packet(device: &AsyncFd<OwnedFd>, buffer: &mut [u8]) -> io::Result<usize> {
    loop {
        let mut ready = device.readable().await?;
        match ready.try_io(|descriptor| {
            // SAFETY: the owned FD stays alive and the buffer is valid and exclusive.
            let size = unsafe {
                libc::read(
                    descriptor.get_ref().as_raw_fd(),
                    buffer.as_mut_ptr().cast(),
                    buffer.len(),
                )
            };
            if size < 0 {
                Err(io::Error::last_os_error())
            } else if size == 0 {
                Err(io::Error::new(io::ErrorKind::UnexpectedEof, "utun closed"))
            } else {
                Ok(size as usize)
            }
        }) {
            Ok(value) => return value,
            Err(_) => continue,
        }
    }
}
async fn write_packet(device: &AsyncFd<OwnedFd>, packet: &[u8]) -> io::Result<()> {
    loop {
        let mut ready = device.writable().await?;
        match ready.try_io(|descriptor| {
            // SAFETY: the descriptor and packet slice outlive the synchronous write.
            let size = unsafe {
                libc::write(
                    descriptor.get_ref().as_raw_fd(),
                    packet.as_ptr().cast(),
                    packet.len(),
                )
            };
            if size < 0 {
                Err(io::Error::last_os_error())
            } else if size as usize != packet.len() {
                Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "partial utun packet write",
                ))
            } else {
                Ok(())
            }
        }) {
            Ok(Err(error)) if error.raw_os_error() == Some(libc::ENOBUFS) => {
                // Darwin datagram queues may report ENOBUFS while kqueue
                // remains writable. Bound retries without spinning; dropping
                // this future on cancellation still closes the owned pump.
                tokio::time::sleep(std::time::Duration::from_millis(1)).await;
            }
            Ok(value) => return value,
            Err(_) => continue,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{os::unix::net::UnixDatagram, time::Duration};
    #[tokio::test]
    async fn packet_adapter_preserves_family_header_and_yields_under_backpressure() {
        let (fd, peer) = UnixDatagram::pair().unwrap();
        fd.set_nonblocking(true).unwrap();
        peer.set_nonblocking(true).unwrap();
        // SAFETY: uniquely owned nonblocking test socket.
        let device = unsafe { AsyncFd::register(OwnedFd::from(fd)) }.unwrap();
        let mut packet = vec![0u8; 20];
        packet[0] = 0x45;
        packet[3] = 20;
        let wire = darwin_packet::encode(&packet).unwrap();
        peer.send(&wire).unwrap();
        let mut buffer = [0u8; 128];
        let size = read_packet(&device, &mut buffer).await.unwrap();
        assert_eq!(darwin_packet::decode(&buffer[..size]).unwrap(), packet);
        write_packet(&device, &wire).await.unwrap();
        assert_eq!(peer.recv(&mut buffer).unwrap(), wire.len());
        assert_eq!(&buffer[..wire.len()], wire);
        // Fill the outbound datagram queue. A blocked write must yield and be
        // safely cancellable; it must not accumulate another packet internally.
        let stopped = tokio::time::timeout(Duration::from_millis(40), async {
            loop {
                write_packet(&device, &wire).await.unwrap();
            }
        })
        .await;
        assert!(stopped.is_err());
        drop(device);
        assert!(peer.send(&wire).is_err());
    }
    struct Queue {
        upload: tokio::sync::mpsc::Sender<Bytes>,
        download: tokio::sync::mpsc::Receiver<Bytes>,
    }
    impl PacketTransport for Queue {
        fn send(&self, packet: Bytes) -> Upload {
            let sender = self.upload.clone();
            Box::pin(async move {
                sender
                    .send(packet)
                    .await
                    .map_err(|_| TransportError::TunnelClosed)
            })
        }
        async fn receive(&mut self) -> Result<Bytes, TransportError> {
            self.download
                .recv()
                .await
                .ok_or(TransportError::TunnelClosed)
        }
    }
    #[tokio::test]
    async fn both_families_cross_the_running_packet_pump_in_both_directions() {
        let (fd, peer) = UnixDatagram::pair().unwrap();
        fd.set_nonblocking(true).unwrap();
        peer.set_nonblocking(true).unwrap();
        let peer = tokio::net::UnixDatagram::from_std(peer).unwrap();
        let (upload, mut received) = tokio::sync::mpsc::channel(1);
        let (sender, download) = tokio::sync::mpsc::channel(1);
        let cancel = CancellationToken::new();
        let (failure, observed) = watch::channel(None);
        let pump = spawn_with(
            Queue { upload, download },
            fd.into(),
            cancel.clone(),
            failure,
        )
        .await
        .unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            for (family, size) in [(2u32, 20usize), (30, 40)] {
                let mut packet = vec![0u8; size];
                if family == 2 {
                    packet[0] = 0x45;
                    packet[3] = 20;
                } else {
                    packet[0] = 0x60;
                }
                let wire = [family.to_be_bytes().as_slice(), packet.as_slice()].concat();
                peer.send(&wire).await.unwrap();
                assert_eq!(received.recv().await.unwrap().as_ref(), packet);
                sender.send(Bytes::from(packet)).await.unwrap();
                let mut reply = [0; 64];
                let length = peer.recv(&mut reply).await.unwrap();
                assert_eq!(&reply[..length], wire);
            }
        })
        .await
        .unwrap();
        cancel.cancel();
        pump.await.unwrap();
        assert!(observed.borrow().is_none());
    }
    #[tokio::test]
    async fn invalid_frame_revokes_pump_and_reports_failure_without_forwarding() {
        let (fd, peer) = UnixDatagram::pair().unwrap();
        fd.set_nonblocking(true).unwrap();
        peer.set_nonblocking(true).unwrap();
        let (upload, mut received) = tokio::sync::mpsc::channel(1);
        let (_sender, download) = tokio::sync::mpsc::channel(1);
        let cancel = CancellationToken::new();
        let (failure, mut observed) = watch::channel(None);
        let pump = spawn_with(
            Queue { upload, download },
            fd.into(),
            cancel.clone(),
            failure,
        )
        .await
        .unwrap();
        peer.send(&[0, 0, 0, 2, 0]).unwrap();
        tokio::time::timeout(Duration::from_secs(1), observed.changed())
            .await
            .unwrap()
            .unwrap();
        assert!(observed.borrow().is_some());
        assert!(cancel.is_cancelled());
        pump.await.unwrap();
        assert!(received.recv().await.is_none());
        assert!(peer.send(&[0]).is_err());
    }
    #[tokio::test]
    async fn cancellation_closes_descriptor_while_upload_queue_is_full() {
        let (fd, peer) = UnixDatagram::pair().unwrap();
        fd.set_nonblocking(true).unwrap();
        peer.set_nonblocking(true).unwrap();
        let (upload, _received) = tokio::sync::mpsc::channel(1);
        upload
            .send(Bytes::from_static(b"already full"))
            .await
            .unwrap();
        let (_sender, download) = tokio::sync::mpsc::channel(1);
        let cancel = CancellationToken::new();
        let (failure, observed) = watch::channel(None);
        let pump = spawn_with(
            Queue { upload, download },
            fd.into(),
            cancel.clone(),
            failure,
        )
        .await
        .unwrap();
        let mut packet = vec![0; 20];
        packet[0] = 0x45;
        packet[3] = 20;
        peer.send(&darwin_packet::encode(&packet).unwrap()).unwrap();
        tokio::task::yield_now().await;
        cancel.cancel();
        tokio::time::timeout(Duration::from_secs(1), pump)
            .await
            .unwrap()
            .unwrap();
        assert!(observed.borrow().is_none());
        assert!(peer.send(&[0]).is_err());
    }
}
