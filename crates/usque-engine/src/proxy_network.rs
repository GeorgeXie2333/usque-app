//! Read-only physical-network observations for desktop proxy sessions.
use std::hash::{Hash, Hasher};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::watch;
use usque_transport::{
    PhysicalNetworkAvailability, PhysicalNetworkSnapshot, SocketHandle, SocketProtector,
};

#[cfg(target_os = "linux")]
mod linux_routes;
#[cfg(target_os = "macos")]
mod macos_routes;
#[cfg(all(test, target_os = "linux"))]
mod tests;

struct ProxyNetwork {
    state: watch::Sender<PhysicalNetworkSnapshot>,
}

impl ProxyNetwork {
    fn update(
        &self,
        previous: &mut u64,
        fingerprint: u64,
        availability: PhysicalNetworkAvailability,
        resumed: bool,
    ) {
        if fingerprint != *previous || resumed {
            *previous = fingerprint;
            let generation = self.state.borrow().generation.saturating_add(1);
            self.state.send_replace(PhysicalNetworkSnapshot {
                generation,
                availability,
            });
        }
    }
}

impl SocketProtector for ProxyNetwork {
    fn protect(&self, _: SocketHandle) -> Result<(), String> {
        Ok(())
    }
    fn subscribe_physical_network(&self) -> Option<watch::Receiver<PhysicalNetworkSnapshot>> {
        Some(self.state.subscribe())
    }
    fn network_generation(&self) -> Option<u64> {
        Some(self.state.borrow().generation)
    }
    fn endpoint_family_available(&self, endpoint: SocketAddr) -> Option<bool> {
        match self.state.borrow().availability {
            PhysicalNetworkAvailability::Unknown => None,
            PhysicalNetworkAvailability::Offline => Some(false),
            PhysicalNetworkAvailability::Online { ipv4, ipv6 } => {
                Some(if endpoint.is_ipv4() { ipv4 } else { ipv6 })
            }
        }
    }
}

pub(super) fn socket_protector() -> Arc<dyn SocketProtector> {
    if cfg!(test) {
        return Arc::new(usque_transport::NoopSocketProtector);
    }
    let (fingerprint, availability) = observe();
    let network = Arc::new(ProxyNetwork {
        state: watch::channel(PhysicalNetworkSnapshot {
            generation: 1,
            availability,
        })
        .0,
    });
    let weak = Arc::downgrade(&network);
    tokio::spawn(async move {
        let mut previous = fingerprint;
        let mut observed_at = SystemTime::now();
        let mut tick = tokio::time::interval(Duration::from_millis(500));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tick.tick().await;
            let Some(network) = weak.upgrade() else {
                return;
            };
            let Ok((fingerprint, availability)) = tokio::task::spawn_blocking(observe).await else {
                continue;
            };
            let now = SystemTime::now();
            let resumed = now
                .duration_since(observed_at)
                .is_ok_and(|gap| gap > Duration::from_secs(5));
            observed_at = now;
            network.update(&mut previous, fingerprint, availability, resumed);
        }
    });
    network
}

fn observe() -> (u64, PhysicalNetworkAvailability) {
    fn source(local: SocketAddr, remote: SocketAddr) -> Option<IpAddr> {
        let socket = UdpSocket::bind(local).ok()?;
        socket.connect(remote).ok()?;
        Some(socket.local_addr().ok()?.ip())
    }
    let v4 = source(
        (Ipv4Addr::UNSPECIFIED, 0).into(),
        (Ipv4Addr::new(162, 159, 199, 2), 443).into(),
    );
    let v6 = source(
        (Ipv6Addr::UNSPECIFIED, 0).into(),
        (Ipv6Addr::new(0x2606, 0x4700, 0x104, 0, 0, 0, 0, 2), 443).into(),
    );
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    v4.hash(&mut hash);
    v6.hash(&mut hash);
    #[cfg(target_os = "linux")]
    {
        std::fs::read("/proc/net/route").ok().hash(&mut hash);
        let ipv6_routes = std::fs::read("/proc/net/ipv6_route").ok();
        linux_routes::hash_ipv6_routes(ipv6_routes.as_deref(), &mut hash);
    }
    #[cfg(windows)]
    default_interfaces().hash(&mut hash);
    #[cfg(target_os = "macos")]
    macos_routes::observe().hash(&mut hash);
    let availability = if v4.is_none() && v6.is_none() {
        PhysicalNetworkAvailability::Offline
    } else {
        PhysicalNetworkAvailability::Online {
            ipv4: v4.is_some(),
            ipv6: v6.is_some(),
        }
    };
    (hash.finish(), availability)
}

#[cfg(windows)]
fn default_interfaces() -> Vec<(u32, u32)> {
    use windows_sys::Win32::NetworkManagement::IpHelper::{FreeMibTable, GetIpForwardTable2};
    use windows_sys::Win32::Networking::WinSock::AF_UNSPEC;
    let mut table = std::ptr::null_mut();
    // SAFETY: Windows initializes the writable table pointer on success.
    if unsafe { GetIpForwardTable2(AF_UNSPEC, &mut table) } != 0 || table.is_null() {
        return Vec::new();
    }
    // SAFETY: the successful API call owns a flexible array with NumEntries rows.
    let result = unsafe {
        let rows =
            std::slice::from_raw_parts((*table).Table.as_ptr(), (*table).NumEntries as usize);
        rows.iter()
            .filter(|row| row.DestinationPrefix.PrefixLength == 0)
            .map(|row| (row.InterfaceIndex, row.Metric))
            .collect()
    };
    // SAFETY: release the table once after copying its scalar route identities.
    unsafe {
        FreeMibTable(table.cast());
    }
    result
}
