//! Explicit live-test harness: loopback SOCKS only, physical socket binding, no Agent/TUN.
#[cfg(windows)]
mod windows_probe {
    use std::net::{Ipv4Addr, SocketAddr};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use async_trait::async_trait;
    use serde_json::json;
    use tokio_util::sync::CancellationToken;
    use usque_core::chain_exit::store::{ChainProfileStore, ProfileCipher, WindowsProfileCipher};
    use usque_core::chain_exit::{ChainSource, ImportSecrets};
    use usque_core::{
        CongestionControlAlgorithm, ConsumerRegistrationClient, FrontendSettings, IpPolicy,
        Profile, RegistrationOptions, TransportPolicy, WarpIdentity,
    };
    use usque_transport::{
        DataPlaneRuntime, DirectEgressLease, DirectProtocol, EndpointPinRefresher, GeoDirectPolicy,
        MasqueTlsIdentity, SocketHandle, SocketProtector, TransportError, VpnGateStart,
    };
    use uuid::Uuid;
    use windows_sys::Win32::Networking::WinSock::{
        IP_UNICAST_IF, IPPROTO_IP, SOCKET, getsockname, getsockopt, setsockopt,
    };

    const IDENTITY_ID: Uuid = Uuid::from_u128(0x38c59eed_7b24_4cc0_a520_b2aa3a440001);

    struct PhysicalOnly {
        index: u32,
        ipv4: Ipv4Addr,
        sockets: Mutex<Vec<(SOCKET, Arc<AtomicBool>)>>,
    }
    struct ObservationLease(Arc<AtomicBool>);
    impl Drop for ObservationLease {
        fn drop(&mut self) {
            self.0.store(false, Ordering::Release);
        }
    }
    #[async_trait]
    impl SocketProtector for PhysicalOnly {
        fn protect(&self, handle: SocketHandle) -> Result<(), String> {
            let socket = handle.value() as SOCKET;
            let value = self.index.to_be();
            // SAFETY: the runtime owns this live IPv4 socket. The option buffer is a
            // valid u32 for the duration of the call; no socket ownership is transferred.
            let result = unsafe {
                setsockopt(
                    socket,
                    IPPROTO_IP,
                    IP_UNICAST_IF,
                    (&value as *const u32).cast(),
                    4,
                )
            };
            if result != 0 {
                return Err("physical_interface_binding_failed".into());
            }
            let mut actual = 0u32;
            let mut length = 4;
            // SAFETY: actual and length are writable and correctly sized; the socket
            // remains owned by the runtime throughout the synchronous readback.
            let result = unsafe {
                getsockopt(
                    socket,
                    IPPROTO_IP,
                    IP_UNICAST_IF,
                    (&mut actual as *mut u32).cast(),
                    &mut length,
                )
            };
            // Winsock sets this option in network order and reads it in host order.
            if result != 0 || length != 4 || actual != self.index {
                return Err("physical_interface_readback_failed".into());
            }
            Ok(())
        }
        async fn protect_for_target(
            &self,
            socket: SocketHandle,
            _remote: SocketAddr,
            _protocol: DirectProtocol,
        ) -> Result<DirectEgressLease, String> {
            self.protect(socket)?;
            let live = Arc::new(AtomicBool::new(true));
            self.sockets
                .lock()
                .map_err(|_| "socket_observation_lock")?
                .push((socket.value() as SOCKET, live.clone()));
            Ok(DirectEgressLease::hold(ObservationLease(live)))
        }
        fn endpoint_family_available(&self, endpoint: SocketAddr) -> Option<bool> {
            Some(endpoint.is_ipv4())
        }
        fn resolve(&self, host: &str, port: u16) -> Result<Vec<SocketAddr>, String> {
            use std::net::ToSocketAddrs;
            (host, port)
                .to_socket_addrs()
                .map(|ips| ips.filter(SocketAddr::is_ipv4).collect())
                .map_err(|_| "control_resolution_failed".into())
        }
    }
    impl PhysicalOnly {
        fn verify_active_source(&self) -> Result<usize, Box<dyn std::error::Error>> {
            let mut verified = 0;
            for (socket, live) in self
                .sockets
                .lock()
                .map_err(|_| "socket_observation_lock")?
                .iter()
            {
                if !live.load(Ordering::Acquire) {
                    continue;
                }
                let socket = *socket;
                let mut binding = 0u32;
                let mut option_length = 4;
                // SAFETY: these writable u32/i32 buffers have the option's exact
                // sizes. Closed sockets are ignored; no handle ownership changes.
                let binding_result = unsafe {
                    getsockopt(
                        socket,
                        IPPROTO_IP,
                        IP_UNICAST_IF,
                        (&mut binding as *mut u32).cast(),
                        &mut option_length,
                    )
                };
                if binding_result != 0 {
                    continue;
                }
                if !live.load(Ordering::Acquire) {
                    continue;
                }
                if option_length != 4 || binding != self.index {
                    return Err("physical_binding_changed".into());
                }
                #[repr(C, align(8))]
                struct AddressBuffer([u8; 128]);
                let mut buffer = AddressBuffer([0u8; 128]);
                let address = &mut buffer.0;
                let mut length = address.len() as i32;
                // SAFETY: the aligned stack buffer is large enough for SOCKADDR_STORAGE;
                // only its byte representation is read after a successful length check.
                let result =
                    unsafe { getsockname(socket, address.as_mut_ptr().cast(), &mut length) };
                if result != 0 {
                    continue;
                }
                if length < 8 || u16::from_ne_bytes([address[0], address[1]]) != 2 {
                    return Err("unexpected_socket_family".into());
                }
                let source = Ipv4Addr::new(address[4], address[5], address[6], address[7]);
                // Unconnected UDP retains a wildcard local address. Its verified
                // IP_UNICAST_IF chooses egress; TCP also exposes the concrete source.
                if !source.is_unspecified() && source != self.ipv4 {
                    return Err("nonphysical_source_detected".into());
                }
                verified += 1;
            }
            if verified == 0 {
                return Err("no_verified_physical_socket".into());
            }
            Ok(verified)
        }
    }

    struct LocalIdentity {
        identity: tokio::sync::Mutex<WarpIdentity>,
        path: PathBuf,
    }
    #[async_trait]
    impl EndpointPinRefresher for LocalIdentity {
        async fn refresh(
            &self,
            protector: Arc<dyn SocketProtector>,
        ) -> Result<MasqueTlsIdentity, TransportError> {
            let mut identity = self.identity.lock().await;
            let refresh = usque_transport::refresh_endpoint_pin_over_protected_socket(
                &identity, None, protector,
            )
            .await?;
            identity.endpoint_pin = refresh.endpoint_pin;
            identity.assigned_ipv4 = refresh.assigned_ipv4;
            identity.assigned_ipv6 = refresh.assigned_ipv6;
            save_identity(&self.path, &identity).map_err(|_| {
                TransportError::EndpointPinRefresh("encrypted_identity_save_failed".into())
            })?;
            MasqueTlsIdentity::from_warp_identity(&identity)
        }
    }
    fn save_identity(
        path: &Path,
        identity: &WarpIdentity,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let secret = identity.to_portable_secret_json()?;
        let encrypted = WindowsProfileCipher.seal(IDENTITY_ID, secret.as_bytes())?;
        std::fs::write(path, encrypted)?;
        Ok(())
    }
    fn state(root: &Path, value: serde_json::Value) -> Result<(), Box<dyn std::error::Error>> {
        let pending = root.join("state.pending.json");
        std::fs::write(&pending, serde_json::to_vec(&value)?)?;
        std::fs::rename(pending, root.join("state.json"))?;
        Ok(())
    }

    #[tokio::main(worker_threads = 2)]
    pub async fn run() -> Result<(), Box<dyn std::error::Error>> {
        let args: Vec<_> = std::env::args_os().collect();
        if !matches!(args.len(), 6 | 7) {
            return Err(
                "usage: probe WORK_DIRECTORY TEMP_WG_FILE h3|h2 chain|warp INTERFACE_JSON [cubic|reno|bbr|bbr3]".into(),
            );
        }
        let root = PathBuf::from(&args[1]);
        std::fs::create_dir_all(&root)?;
        let physical: serde_json::Value = serde_json::from_slice(&std::fs::read(&args[5])?)?;
        if physical["hardware_interface"] != true {
            return Err("physical_interface_required".into());
        }
        let protector = Arc::new(PhysicalOnly {
            index: physical["interface_index"]
                .as_u64()
                .ok_or("interface_index_missing")?
                .try_into()?,
            ipv4: physical["ipv4"]
                .as_str()
                .ok_or("physical_ipv4_missing")?
                .parse()?,
            sockets: Mutex::new(Vec::new()),
        });
        let text = ImportSecrets::new(std::fs::read_to_string(&args[2])?);
        let directory = tempfile::tempdir()?;
        let library = ChainProfileStore::new(directory.path(), &WindowsProfileCipher);
        let summary = library.import(ChainSource::WireguardCustom, "Temporary speed test", text)?;
        let reservation = std::net::TcpListener::bind("127.0.0.1:0")?;
        let proxy = reservation.local_addr()?;
        if proxy.port() <= 1024 {
            return Err("high_loopback_port_required".into());
        }
        let mut profile = Profile {
            frontends: FrontendSettings {
                tunnel: false,
                socks5: true,
                http: false,
            },
            kill_switch: false,
            allow_lan: false,
            ip_policy: IpPolicy::Ipv4Only,
            congestion_control: match args.get(6).and_then(|v| v.to_str()).unwrap_or("cubic") {
                "cubic" => CongestionControlAlgorithm::Cubic,
                "reno" => CongestionControlAlgorithm::Reno,
                "bbr" => CongestionControlAlgorithm::Bbr,
                "bbr3" => CongestionControlAlgorithm::Bbr3,
                _ => return Err("explicit_supported_congestion_algorithm_required".into()),
            },
            transport: match args[3].to_str() {
                Some("h3") => TransportPolicy::Http3,
                Some("h2") => TransportPolicy::Http2,
                _ => return Err("explicit_transport_required".into()),
            },
            ..Profile::default()
        };
        profile.proxy.system_proxy = false;
        profile.proxy.socks5_listeners = vec![proxy];
        profile.geo_direct_countries.clear();
        profile.bypass_domains.clear();
        profile.split_exclusions.clear();
        profile.chain_exit = match args[4].to_str() {
            Some("chain") => Some(summary.selection()),
            Some("warp") => None,
            _ => return Err("explicit_chain_mode_required".into()),
        };
        profile.canonicalize_mode();
        profile.validate()?;
        assert!(!profile.frontends.tunnel && !profile.proxy.system_proxy && !profile.kill_switch);
        let identity_path = root
            .parent()
            .ok_or("test_parent_missing")?
            .join("warp-free.dpapi");
        state(
            &root,
            json!({"stage":"identity", "tun":false, "interface_index":protector.index}),
        )?;
        let identity = if identity_path.exists() {
            let clear = WindowsProfileCipher.open(IDENTITY_ID, &std::fs::read(&identity_path)?)?;
            usque_core::parse_manual_warp_secret(std::str::from_utf8(&clear)?)?
        } else {
            let options = RegistrationOptions {
                terms_accepted: true,
                device_name: Some("Usque temporary speed test".into()),
                ..RegistrationOptions::default()
            };
            let identity = ConsumerRegistrationClient::new()?
                .register(&options)
                .await?;
            save_identity(&identity_path, &identity)?;
            identity
        };
        let tls = MasqueTlsIdentity::from_warp_identity(&identity)?;
        let refresher = Arc::new(LocalIdentity {
            identity: tokio::sync::Mutex::new(identity),
            path: identity_path,
        });
        let selected = usque_core::chain_exit::prepare_selection(
            directory.path(),
            &profile,
            &WindowsProfileCipher,
        )?;
        let cancel = CancellationToken::new();
        drop(reservation);
        state(
            &root,
            json!({"stage":"connecting", "tun":false, "interface_index":protector.index}),
        )?;
        let mut runtime = DataPlaneRuntime::start_with_vpngate(
            &profile,
            tls,
            protector.clone(),
            Some(refresher),
            Arc::new(GeoDirectPolicy::disabled()),
            VpnGateStart {
                selected,
                status: None,
                cancellation: cancel.clone(),
                deadline: Some(tokio::time::Instant::now() + Duration::from_secs(120)),
            },
        )
        .await?;
        let result = async {
        runtime.activate_final().await?;
        let sockets = protector.verify_active_source()?;
        state(&root, json!({"stage":"ready", "tun":false, "interface_index":protector.index, "physical_sockets_verified":sockets, "loopback_port":proxy.port(), "transport":args[3].to_str(), "chain":profile.chain_enabled(), "congestion_control":profile.congestion_control.as_str()}))?;
        for _ in 0..3000 {
            if root.join("stop").exists() { break; }
            if runtime.failure().is_some() { return Err("chain_runtime_failed".into()); }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
            let q = runtime.underlay_monitor().network_quality();
        let traffic = runtime.statistics();
        let performance = runtime.performance();
        std::fs::write(root.join("metrics.json"), serde_json::to_vec_pretty(&json!({"traffic":{"sent":traffic.bytes_sent,"received":traffic.bytes_received}, "outer_datagram_receive_drops":q.loss.datagram_receive_drops.value, "outer_lost_packets":q.loss.lost_packets.value, "outer_pto":q.loss.pto_count.value, "outer_rtt_us":q.rtt.smoothed.value.map(|d|d.as_micros()), "outer_queues":q.queues.iter().map(|s|json!({"kind":format!("{:?}",s.kind),"drop_items":s.drop_items,"high_water_items":s.items_high_water})).collect::<Vec<_>>(), "transport_performance":q.transport_performance.map(|p|p.to_json()), "send_queue_drops":performance.send_queue_drop_count, "preferred_tcp_sockets":performance.preferred_tcp_sockets, "fallback_tcp_sockets":performance.fallback_tcp_sockets}))?)?;
        Ok::<(), Box<dyn std::error::Error>>(())
    }.await;
        cancel.cancel();
        runtime.shutdown().await;
        if tokio::net::TcpStream::connect(proxy).await.is_ok() {
            return Err("listener_cleanup_failed".into());
        }
        state(
            &root,
            json!({"stage":"stopped", "listener_closed":true,"ok":result.is_ok()}),
        )?;
        result
    }
}

#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    windows_probe::run()
}

#[cfg(not(windows))]
fn main() {
    eprintln!("This explicit live-test harness requires Windows DPAPI and socket binding.");
    std::process::exit(1);
}
