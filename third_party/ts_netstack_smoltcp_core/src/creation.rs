//! Ownership while an allocation response is in flight to its caller.
use crate::{Command, Netstack, Response, TcpListenerHandle, tcp, udp};
use smoltcp::iface::SocketHandle;

#[derive(Clone, Copy)]
pub(crate) enum CreatedSocket {
    Tcp(SocketHandle),
    Udp(SocketHandle),
    Listener(TcpListenerHandle),
}

#[cfg(test)]
mod tests;

impl CreatedSocket {
    pub(crate) fn from_response(response: &Response) -> Option<Self> {
        match response {
            Response::TcpStream(tcp::stream::Response::Connected { handle }) => {
                Some(Self::Tcp(*handle))
            }
            Response::Udp(udp::Response::Bound { handle, .. }) => Some(Self::Udp(*handle)),
            Response::TcpListen(tcp::listen::Response::Listening { handle }) => {
                Some(Self::Listener(*handle))
            }
            _ => None,
        }
    }
}

pub(crate) struct UnclaimedCreation {
    pub(crate) response: flume::Sender<Response>,
    pub(crate) socket: CreatedSocket,
}

impl Netstack {
    pub(crate) fn reclaim_creation(&mut self, socket: CreatedSocket) {
        match socket {
            CreatedSocket::Tcp(handle) => {
                drop(self.process_tcp_stream(tcp::stream::Command::Abort, Some(handle)));
                self.drain_tcp_closes();
            }
            CreatedSocket::Udp(handle) => {
                drop(self.process_udp(udp::Command::Close, Some(handle)));
            }
            CreatedSocket::Listener(handle) => {
                drop(self.process_tcp_listen(tcp::listen::Command::Close { handle }, None));
            }
        }
    }

    pub(crate) fn reap_unclaimed_creations(&mut self) {
        let mut index = 0;
        while index < self.unclaimed_creations.len() {
            let creation = &self.unclaimed_creations[index];
            // The pinned flume keeps an undelivered response in its queue even
            // after the last receiver drops. Only receiving consumes it. Async
            // receive hooks wake the caller without removing the response.
            // A received handle is synchronously turned into its socket owner.
            // Check disconnection first: after that observation the queue is
            // stable. Checking emptiness first could race a successful receive
            // followed by receiver drop and mistakenly abort the winner.
            if creation.response.is_disconnected() {
                let creation = self.unclaimed_creations.swap_remove(index);
                if !creation.response.is_empty() {
                    self.reclaim_creation(creation.socket);
                }
            } else if creation.response.is_empty() {
                self.unclaimed_creations.swap_remove(index);
            } else {
                index += 1;
            }
        }
        if self.unclaimed_creations.is_empty() {
            self.unclaimed_creations = Default::default();
        }
    }

    pub(crate) fn cancel_creation(&mut self, command: &Command, handle: Option<SocketHandle>) {
        if matches!(
            command,
            Command::TcpStream(tcp::stream::Command::Connect { .. })
        ) && let Some(handle) = handle
        {
            self.reclaim_creation(CreatedSocket::Tcp(handle));
        }
    }

    pub(crate) fn reap_cancelled_commands(&mut self) {
        for _ in 0..self.blocked_commands.len() {
            let request = self.blocked_commands.pop_front().unwrap();
            if request.resp.is_disconnected() {
                self.cancel_creation(&request.command, request.handle);
            } else {
                self.blocked_commands.push_back(request);
            }
        }
    }
}
