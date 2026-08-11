use alloc::vec;

use smoltcp::socket::tcp;

use crate::{Netstack, command::Error};

mod listener;
mod stream;

pub use listener::{ListenerHandle, TcpListenerState};

#[derive(Debug, Clone, Copy)]
pub(crate) struct TcpBufferAllocation {
    bytes: usize,
    preferred: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct TcpBufferUsage {
    preferred_bytes: usize,
    total_bytes: usize,
    preferred_sockets: usize,
    fallback_sockets: usize,
    rejected_sockets: usize,
}

impl Netstack {
    fn tcp_buffer(&self) -> tcp::SocketBuffer<'static> {
        tcp::SocketBuffer::new(vec![0; self.config.tcp_buffer_size])
    }

    pub(crate) fn new_outbound_tcp_socket(
        &mut self,
    ) -> Result<(tcp::Socket<'static>, TcpBufferAllocation), Error> {
        let Some(policy) = self.config.tcp_buffer_policy else {
            let mut socket = tcp::Socket::new(self.tcp_buffer(), self.tcp_buffer());
            socket.set_nagle_enabled(self.config.tcp_nagle_enabled);
            return Ok((
                socket,
                TcpBufferAllocation {
                    bytes: 0,
                    preferred: false,
                },
            ));
        };

        let preferred_bytes = policy.preferred.total();
        let fallback_bytes = policy.fallback.total();
        let selected = preferred_bytes
            .filter(|bytes| {
                self.tcp_buffer_usage.preferred_bytes.saturating_add(*bytes)
                    <= policy.preferred_budget
                    && self.tcp_buffer_usage.total_bytes.saturating_add(*bytes)
                        <= policy.total_budget
            })
            .map(|bytes| (policy.preferred, bytes, true))
            .or_else(|| {
                fallback_bytes
                    .filter(|bytes| {
                        self.tcp_buffer_usage.total_bytes.saturating_add(*bytes)
                            <= policy.total_budget
                    })
                    .map(|bytes| (policy.fallback, bytes, false))
            });

        let Some((tier, bytes, preferred)) = selected else {
            self.tcp_buffer_usage.rejected_sockets =
                self.tcp_buffer_usage.rejected_sockets.saturating_add(1);
            self.publish_tcp_buffer_metrics();
            tracing::warn!(
                total_bytes = self.tcp_buffer_usage.total_bytes,
                total_budget = policy.total_budget,
                "rejected a new TCP socket because the buffer budget is exhausted"
            );
            return Err(Error::tcp_buffer_budget_exhausted());
        };
        if tier.receive == 0 || tier.transmit == 0 {
            self.tcp_buffer_usage.rejected_sockets =
                self.tcp_buffer_usage.rejected_sockets.saturating_add(1);
            self.publish_tcp_buffer_metrics();
            return Err(Error::zero_buffer());
        }

        self.tcp_buffer_usage.total_bytes =
            self.tcp_buffer_usage.total_bytes.saturating_add(bytes);
        if preferred {
            self.tcp_buffer_usage.preferred_bytes = self
                .tcp_buffer_usage
                .preferred_bytes
                .saturating_add(bytes);
            self.tcp_buffer_usage.preferred_sockets =
                self.tcp_buffer_usage.preferred_sockets.saturating_add(1);
        } else {
            self.tcp_buffer_usage.fallback_sockets =
                self.tcp_buffer_usage.fallback_sockets.saturating_add(1);
            tracing::debug!(
                receive_bytes = tier.receive,
                transmit_bytes = tier.transmit,
                total_bytes = self.tcp_buffer_usage.total_bytes,
                "allocated fallback TCP buffers under memory pressure"
            );
        }
        self.publish_tcp_buffer_metrics();

        let mut socket = tcp::Socket::new(
            tcp::SocketBuffer::new(vec![0; tier.receive]),
            tcp::SocketBuffer::new(vec![0; tier.transmit]),
        );
        socket.set_nagle_enabled(self.config.tcp_nagle_enabled);
        Ok((socket, TcpBufferAllocation { bytes, preferred }))
    }

    pub(crate) fn register_tcp_buffer_allocation(
        &mut self,
        handle: smoltcp::iface::SocketHandle,
        allocation: TcpBufferAllocation,
    ) {
        if allocation.bytes != 0 {
            self.tcp_buffer_allocations.push((handle, allocation));
        }
    }

    pub(crate) fn release_tcp_buffer_allocation(
        &mut self,
        handle: smoltcp::iface::SocketHandle,
    ) {
        if let Some(index) = self
            .tcp_buffer_allocations
            .iter()
            .position(|(candidate, _)| *candidate == handle)
        {
            let (_, allocation) = self.tcp_buffer_allocations.swap_remove(index);
            self.release_unregistered_tcp_buffer(allocation);
        }
    }

    pub(crate) fn release_unregistered_tcp_buffer(
        &mut self,
        allocation: TcpBufferAllocation,
    ) {
        if allocation.bytes == 0 {
            return;
        }
        self.tcp_buffer_usage.total_bytes = self
            .tcp_buffer_usage
            .total_bytes
            .saturating_sub(allocation.bytes);
        if allocation.preferred {
            self.tcp_buffer_usage.preferred_bytes = self
                .tcp_buffer_usage
                .preferred_bytes
                .saturating_sub(allocation.bytes);
            self.tcp_buffer_usage.preferred_sockets =
                self.tcp_buffer_usage.preferred_sockets.saturating_sub(1);
        } else {
            self.tcp_buffer_usage.fallback_sockets =
                self.tcp_buffer_usage.fallback_sockets.saturating_sub(1);
        }
        self.publish_tcp_buffer_metrics();
    }

    fn publish_tcp_buffer_metrics(&self) {
        if let Some(metrics) = &self.config.tcp_buffer_metrics {
            metrics.update(
                self.tcp_buffer_usage.preferred_bytes,
                self.tcp_buffer_usage.total_bytes,
                self.tcp_buffer_usage.preferred_sockets,
                self.tcp_buffer_usage.fallback_sockets,
                self.tcp_buffer_usage.rejected_sockets,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Config, TcpBufferPolicy, TcpBufferTier};
    use smoltcp::socket::tcp::CongestionControl;

    fn bounded_config() -> Config {
        Config {
            tcp_buffer_policy: Some(TcpBufferPolicy {
                preferred: TcpBufferTier {
                    receive: 4 * 1024,
                    transmit: 1024,
                },
                fallback: TcpBufferTier {
                    receive: 1024,
                    transmit: 256,
                },
                preferred_budget: 5 * 1024,
                total_budget: 6 * 1024 + 256,
            }),
            tcp_nagle_enabled: false,
            ..Config::default()
        }
    }

    #[test]
    fn preferred_buffers_downshift_then_reject_without_evicting_existing_sockets() {
        let mut stack = Netstack::new(bounded_config(), smoltcp::time::Instant::from_millis(0));
        let (_, preferred) = stack.new_outbound_tcp_socket().unwrap();
        let (_, fallback) = stack.new_outbound_tcp_socket().unwrap();
        assert!(preferred.preferred);
        assert!(!fallback.preferred);
        assert_eq!(stack.tcp_buffer_usage.preferred_sockets, 1);
        assert_eq!(stack.tcp_buffer_usage.fallback_sockets, 1);

        let error = stack.new_outbound_tcp_socket().unwrap_err();
        assert!(error.is_tcp_buffer_budget_exhausted());
        assert_eq!(stack.tcp_buffer_usage.preferred_sockets, 1);
        assert_eq!(stack.tcp_buffer_usage.fallback_sockets, 1);
        assert_eq!(stack.tcp_buffer_usage.rejected_sockets, 1);

        stack.release_unregistered_tcp_buffer(fallback);
        let (_, replacement) = stack.new_outbound_tcp_socket().unwrap();
        assert!(!replacement.preferred);
    }

    #[test]
    fn optimized_socket_uses_cubic_and_disables_nagle() {
        let mut stack = Netstack::new(bounded_config(), smoltcp::time::Instant::from_millis(0));
        let (socket, _) = stack.new_outbound_tcp_socket().unwrap();
        assert!(!socket.nagle_enabled());
        assert_eq!(socket.congestion_control(), CongestionControl::Cubic);
    }
}
