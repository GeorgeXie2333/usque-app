//! Opt-in, socket-free comparison harness. The same file runs on the baseline.
//! It measures protocol scheduling/allocation, not Android or WAN throughput.
use super::*;
use boringtun::x25519::{PublicKey, StaticSecret};
use std::alloc::{GlobalAlloc, Layout, System};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use tokio::time::Instant;
use zeroize::Zeroizing;

static MEASURING: AtomicBool = AtomicBool::new(false);
static ALLOCATIONS: AtomicU64 = AtomicU64::new(0);
static ALLOCATED_BYTES: AtomicU64 = AtomicU64::new(0);
struct CountingAllocator;
// SAFETY: All allocations retain the System allocator's pointer, layout and
// deallocation contracts. Counters do not allocate or alter returned memory.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if MEASURING.load(Ordering::Relaxed) {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
            ALLOCATED_BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        }
        // SAFETY: Forward the caller's valid layout unchanged.
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: Every pointer was returned by System with this layout.
        unsafe { System.dealloc(pointer, layout) }
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        if MEASURING.load(Ordering::Relaxed) {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
            ALLOCATED_BYTES.fetch_add(size as u64, Ordering::Relaxed);
        }
        // SAFETY: Forward the caller's live allocation and valid size unchanged.
        unsafe { System.realloc(pointer, layout, size) }
    }
}
#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

#[cfg(windows)]
fn cpu_ns() -> u64 {
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};
    let mut times: [FILETIME; 4] = std::array::from_fn(|_| FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    });
    // SAFETY: The pseudo handle belongs to this process and all four output
    // pointers reference distinct initialized FILETIME storage for the call.
    let ok = unsafe {
        GetProcessTimes(
            GetCurrentProcess(),
            &mut times[0],
            &mut times[1],
            &mut times[2],
            &mut times[3],
        )
    };
    assert_ne!(ok, 0);
    let value = |t: FILETIME| (u64::from(t.dwHighDateTime) << 32) | u64::from(t.dwLowDateTime);
    (value(times[2]) + value(times[3])) * 100
}
#[cfg(not(windows))]
fn cpu_ns() -> u64 {
    0
}

fn payload(ipv6: bool, mtu: usize, sequence: usize, streams: usize) -> Vec<u8> {
    let mut packet = vec![0; mtu];
    let offset = if ipv6 {
        packet[0] = 0x60;
        packet[4..6].copy_from_slice(&((mtu - 40) as u16).to_be_bytes());
        packet[6] = 17;
        packet[7] = 64;
        packet[8..24].copy_from_slice(&"fd00::2".parse::<Ipv6Addr>().unwrap().octets());
        packet[24..40].copy_from_slice(&"fd00::1".parse::<Ipv6Addr>().unwrap().octets());
        40
    } else {
        packet[0] = 0x45;
        packet[2..4].copy_from_slice(&(mtu as u16).to_be_bytes());
        packet[8] = 64;
        packet[9] = 17;
        packet[12..16].copy_from_slice(&[10, 8, 0, 2]);
        packet[16..20].copy_from_slice(&[10, 8, 0, 1]);
        20
    };
    packet[offset..offset + 2]
        .copy_from_slice(&((10000 + sequence % streams) as u16).to_be_bytes());
    packet[offset + 2..offset + 4].copy_from_slice(&1194u16.to_be_bytes());
    packet[offset + 4..offset + 6].copy_from_slice(&((mtu - offset) as u16).to_be_bytes());
    packet[offset + 8..offset + 12].copy_from_slice(&(sequence as u32).to_be_bytes());
    packet
}

async fn handshake(session: &mut Session, peer: &mut Tunn) {
    let input = session.input();
    let mut buffer = vec![0; MAX_PACKET + 256];
    let mut connected = false;
    let mut confirmed = false;
    tokio::time::timeout(Duration::from_secs(3), async {
        while !connected || !confirmed {
            match session.next_event().await.unwrap() {
                Event::Dial { generation } => input.push(3, generation, &[]).await.unwrap(),
                Event::State { name, .. } => connected |= name == "CONNECTED",
                Event::TransportPacket { packet, .. } => {
                    confirmed |= packet.starts_with(&[4, 0, 0, 0]);
                    let mut result = peer.decapsulate(None, &packet, &mut buffer);
                    loop {
                        match result {
                            TunnResult::WriteToNetwork(bytes) => {
                                input.push(1, GENERATION, bytes).await.unwrap()
                            }
                            TunnResult::Done => break,
                            _ => panic!("benchmark handshake rejected"),
                        }
                        result = peer.decapsulate(None, &[], &mut buffer);
                    }
                }
                _ => {}
            }
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
#[ignore = "controlled memory benchmark; run alone with --ignored --nocapture --test-threads=1"]
async fn memory_performance() {
    const WINDOW: usize = 32;
    for ipv6 in [false, true] {
        for mtu in [1280, 1420, 1500] {
            for rtt_ms in [0, 20, 80] {
                for streams in [1, 8] {
                    for round in 0..5 {
                        let count = if rtt_ms == 0 { 8192 } else { 256 };
                        let private = StaticSecret::from([1; 32]);
                        let peer_key = StaticSecret::from([2; 32]);
                        let mut peer = Tunn::new(
                            peer_key.clone(),
                            PublicKey::from(&private),
                            None,
                            None,
                            2,
                            None,
                        );
                        let mut session = Session::start(WireGuardProfile {
                            private_key: Zeroizing::new(private.to_bytes()),
                            public_key: PublicKey::from(&peer_key).to_bytes(),
                            preshared_key: None,
                            endpoint: usque_core::chain_exit::Endpoint::parse(
                                "vpn.example",
                                "51820",
                                0,
                            )
                            .unwrap(),
                            addresses: vec![
                                "10.8.0.2/32".parse().unwrap(),
                                "fd00::2/128".parse().unwrap(),
                            ],
                            dns_servers: vec![],
                            allowed_ips: vec![
                                "0.0.0.0/0".parse().unwrap(),
                                "::/0".parse().unwrap(),
                            ],
                            mtu: mtu as u16,
                            keepalive: None,
                        });
                        handshake(&mut session, &mut peer).await;
                        let input = session.input();
                        let mut sent = 0;
                        let mut received = 0;
                        let mut started = vec![Instant::now(); count];
                        let mut latencies = Vec::with_capacity(count);
                        let mut replies: VecDeque<(Instant, Bytes)> = VecDeque::new();
                        let mut buffer = vec![0; MAX_PACKET + 256];
                        let mut echo_buffer = vec![0; MAX_PACKET + 256];
                        let cpu_start = cpu_ns();
                        let start = Instant::now();
                        ALLOCATIONS.store(0, Ordering::Relaxed);
                        ALLOCATED_BYTES.store(0, Ordering::Relaxed);
                        MEASURING.store(true, Ordering::Relaxed);
                        let result = tokio::time::timeout(Duration::from_secs(15), async {
                            while received < count {
                                while sent < count && sent - received < WINDOW {
                                    started[sent] = Instant::now();
                                    input.push(2, GENERATION, &payload(ipv6, mtu, sent, streams)).await.unwrap();
                                    sent += 1;
                                }
                                let due = replies.front().map_or_else(|| Instant::now() + Duration::from_secs(1), |(due, _)| *due);
                                tokio::select! {
                                    _ = tokio::time::sleep_until(due), if !replies.is_empty() => {
                                        let (_, packet) = replies.pop_front().unwrap();
                                        input.push(1, GENERATION, &packet).await.unwrap();
                                    },
                                    event = session.next_event() => match event.unwrap() {
                                        Event::TransportPacket { packet, .. } => match peer.decapsulate(None, &packet, &mut buffer) {
                                            TunnResult::WriteToTunnelV4(bytes, _) | TunnResult::WriteToTunnelV6(bytes, _) => {
                                                let TunnResult::WriteToNetwork(echo) = peer.encapsulate(bytes, &mut echo_buffer) else { panic!("no authenticated echo") };
                                                if rtt_ms == 0 {
                                                    input.push(1, GENERATION, echo).await.unwrap();
                                                } else {
                                                    assert!(replies.len() < WINDOW);
                                                    replies.push_back((Instant::now() + Duration::from_millis(rtt_ms), Bytes::copy_from_slice(echo)));
                                                }
                                            },
                                            TunnResult::Done => {},
                                            _ => panic!("unexpected protocol result in steady-state benchmark"),
                                        },
                                        Event::IpPacket { packet, .. } => {
                                            let offset = if ipv6 {48} else {28};
                                            let sequence = u32::from_be_bytes(packet[offset..offset+4].try_into().unwrap()) as usize;
                                            assert_eq!(packet.len(), mtu);
                                            assert!(sequence < count);
                                            latencies.push(started[sequence].elapsed().as_nanos() as u64);
                                            received += 1;
                                        },
                                        _ => {},
                                    },
                                }
                            }
                        }).await;
                        MEASURING.store(false, Ordering::Relaxed);
                        let wall_ns = start.elapsed().as_nanos() as u64;
                        let cpu_ns = cpu_ns().saturating_sub(cpu_start);
                        let allocations = ALLOCATIONS.load(Ordering::Relaxed);
                        let allocated_bytes = ALLOCATED_BYTES.load(Ordering::Relaxed);
                        latencies.sort_unstable();
                        let latency = |percent: usize| {
                            latencies
                                .get(latencies.len().saturating_sub(1) * percent / 100)
                                .copied()
                        };
                        println!(
                            "CHAIN_PERF {}",
                            serde_json::json!({"ipv6":ipv6,"mtu":mtu,"rtt_ms":rtt_ms,"streams":streams,"round":round,"completed":result.is_ok(),"sent":sent,"received":received,"wall_ns":wall_ns,"cpu_ns":cpu_ns,"allocations":allocations,"allocated_bytes":allocated_bytes,"latency_p50_ns":latency(50),"latency_p95_ns":latency(95),"window":WINDOW})
                        );
                        session.shutdown().await.unwrap();
                        assert!(result.is_ok(), "bounded protocol benchmark stalled");
                    }
                }
            }
        }
    }
}
