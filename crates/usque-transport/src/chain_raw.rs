//! Owned private IPv6 fragment socket. Close retries a full stack queue.
use bytes::Bytes;
use ts_netstack_smoltcp::netcore::{
    Channel, Error, HasChannel, Response, raw,
    smoltcp::{
        iface::SocketHandle,
        wire::{IpProtocol, IpVersion},
    },
};

pub(crate) struct RawSocket {
    channel: Channel,
    handle: SocketHandle,
}
impl RawSocket {
    pub(crate) async fn open(channel: Channel) -> Result<Self, Error> {
        match channel
            .request(
                None,
                raw::Command::Open {
                    ip_version: IpVersion::Ipv6,
                    protocol: IpProtocol::Ipv6Frag,
                },
            )
            .await?
        {
            Response::Raw(raw::Response::Opened { handle }) => Ok(Self { channel, handle }),
            Response::Error(error) => Err(error),
            _ => Err(Error::wrong_type()),
        }
    }
    pub(crate) async fn send(&self, bytes: &[u8]) -> Result<(), Error> {
        self.channel
            .request(
                Some(self.handle),
                raw::Command::Send {
                    buf: Bytes::copy_from_slice(bytes),
                },
            )
            .await?
            .to_ok()
    }
    pub(crate) async fn recv_bytes(&self) -> Result<Bytes, Error> {
        match self
            .channel
            .request(Some(self.handle), raw::Command::Recv { max_len: None })
            .await?
        {
            Response::Raw(raw::Response::Recv {
                buf,
                truncated: None,
            }) => Ok(buf),
            Response::Error(error) => Err(error),
            _ => Err(Error::wrong_type()),
        }
    }
}
impl Drop for RawSocket {
    fn drop(&mut self) {
        crate::stack_tcp::cleanup(&self.channel, Some(self.handle), || {
            raw::Command::Close.into()
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn full_command_queue_retries_socket_close_without_network_progress() {
        use std::future::Future;
        use std::task::{Context, Waker};
        use ts_netstack_smoltcp::netcore::{
            Config, Netstack, stack_control, try_request_nonblocking,
        };
        let mut stack = Netstack::new(
            Config {
                command_channel_capacity: Some(1),
                ..Default::default()
            },
            ts_netstack_smoltcp::netcore::smoltcp::time::Instant::from_millis(0),
        );
        let channel = stack.command_channel();
        let allocate = || RawSocket::open(channel.clone());
        let mut opening = Box::pin(allocate());
        assert!(
            opening
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_pending()
        );
        stack.process_cmds();
        let socket = opening.await.unwrap();
        let original = socket.handle;
        try_request_nonblocking(
            &channel,
            None,
            stack_control::Command::SetIps { new_ips: vec![] },
        )
        .unwrap();
        drop(socket);
        for _ in 0..4 {
            tokio::task::yield_now().await;
            stack.process_cmds();
        }
        let mut opening = Box::pin(allocate());
        assert!(
            opening
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_pending()
        );
        stack.process_cmds();
        let replacement = opening.await.unwrap();
        assert_eq!(
            replacement.handle, original,
            "the old socket must release its slot"
        );
        drop(replacement);
        stack.process_cmds();
    }
}
