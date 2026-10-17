use core::net::IpAddr;

use crate::{Error, HasChannel, Netstack, Response, command::stack_control};

/// Extension methods for control channels ([`HasChannel`]) supporting ergonomic operations
/// around [`stack_control::Command`]s.
pub trait NetstackControl: HasChannel {
    /// Set the IPs for the netstack interface.
    ///
    /// The netstack must have space to store all the provided IPs -- this is controlled
    /// by `smoltcp`'s `iface-max-addr-count-*` features. You must configure this as the end
    /// user because it is global to all usages of this version of `smoltcp`.
    fn set_ips_blocking(&self, ips: impl IntoIterator<Item = IpAddr>) -> Result<(), Error> {
        self.request_blocking(
            None,
            stack_control::Command::SetIps {
                new_ips: ips.into_iter().collect(),
            },
        )?
        .to_ok()
    }

    /// Set the IPs for the netstack interface.
    ///
    /// The netstack must have space to store all the provided IPs -- this is controlled
    /// by `smoltcp`'s `iface-max-addr-count-*` features. You must configure this as the end
    /// user because it is global to all usages of this version of `smoltcp`.
    fn set_ips(
        &self,
        ips: impl IntoIterator<Item = IpAddr>,
    ) -> impl Future<Output = Result<(), Error>> + Send {
        let channel = self.command_channel();
        let new_ips = ips.into_iter().collect();

        async move {
            crate::request(channel, None, stack_control::Command::SetIps { new_ips })
                .await?
                .to_ok()
        }
    }
}

#[cfg(test)]
mod budget_tests {
    use super::*;
    use crate::Config;
    use smoltcp::time::Instant;

    #[test]
    fn budget_validates_both_families_before_mutation_and_rejects_old_revisions() {
        let mut stack = Netstack::new(Config::default(), Instant::from_millis(0));
        let mut other = Netstack::new(Config::default(), Instant::from_millis(0));
        assert!(matches!(
            stack.process_stack_control(stack_control::Command::SetTcpPacketBudget {
                revision: 1,
                ipv4: Some(1100),
                ipv6: Some(1280),
            }),
            Response::Ok
        ));
        for (ipv4, ipv6) in [(Some(127), None), (None, Some(1279)), (Some(65536), None)] {
            assert!(matches!(
                stack.process_stack_control(stack_control::Command::SetTcpPacketBudget {
                    revision: 3,
                    ipv4,
                    ipv6,
                }),
                Response::Error(_)
            ));
            assert_eq!(stack.tcp_packet_budget, [Some(1100), Some(1280)]);
            assert_eq!(stack.tcp_packet_budget_revision, 1);
        }
        stack.process_stack_control(stack_control::Command::SetTcpPacketBudget {
            revision: 2,
            ipv4: Some(1000),
            ipv6: Some(1280),
        });
        stack.process_stack_control(stack_control::Command::SetTcpPacketBudget {
            revision: 1,
            ipv4: None,
            ipv6: None,
        });
        assert_eq!(stack.tcp_packet_budget, [Some(1000), Some(1280)]);
        assert_eq!(other.tcp_packet_budget, [None, None]);
        assert!(other.direct_set_ips(["10.0.0.1".parse().unwrap()]));
        assert_eq!(other.tcp_packet_budget, [None, None]);
    }
}

impl<T> NetstackControl for T where T: HasChannel {}

impl Netstack {
    #[tracing::instrument(skip(self), level = "debug")]
    pub(crate) fn process_stack_control(&mut self, command: stack_control::Command) -> Response {
        match command {
            stack_control::Command::SetTcpPacketBudget {
                revision,
                ipv4,
                ipv6,
            } => {
                if revision <= self.tcp_packet_budget_revision {
                    return Response::Ok;
                }
                if ipv4.is_some_and(|value| !(128..=65535).contains(&value))
                    || ipv6.is_some_and(|value| !(1280..=65535).contains(&value))
                {
                    return Response::Error(Error::invalid_packet_budget());
                }
                self.tcp_packet_budget_revision = revision;
                self.tcp_packet_budget = [ipv4, ipv6];
                for (_, socket) in self.socket_set.iter_mut() {
                    if let smoltcp::socket::Socket::Tcp(socket) = socket
                        && let Some(remote) = socket.remote_endpoint()
                    {
                        let index =
                            usize::from(matches!(remote.addr, smoltcp::wire::IpAddress::Ipv6(_)));
                        assert!(socket.set_tx_ip_packet_limit(self.tcp_packet_budget[index]));
                    }
                }
                Response::Ok
            }
            stack_control::Command::SetIps { new_ips } => {
                if !self.direct_set_ips(new_ips.iter().copied()) {
                    tracing::error!(
                        ?new_ips,
                        "not enough address storage space configured in smoltcp"
                    );

                    return Error::buffer_full().into();
                }

                Response::Ok
            }
        }
    }
}
