use std::sync::atomic::{AtomicU16, Ordering};

const FIRST_EPHEMERAL_PORT: u16 = 49_152;
const LAST_EPHEMERAL_PORT: u16 = 65_534;

static NEXT_TCP_PORT: AtomicU16 = AtomicU16::new(FIRST_EPHEMERAL_PORT);
static NEXT_UDP_PORT: AtomicU16 = AtomicU16::new(FIRST_EPHEMERAL_PORT);

pub(crate) fn next_tcp_port() -> u16 {
    next_port(&NEXT_TCP_PORT)
}

pub(crate) fn next_udp_port() -> u16 {
    next_port(&NEXT_UDP_PORT)
}

fn next_port(counter: &AtomicU16) -> u16 {
    counter
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
            Some(if value >= LAST_EPHEMERAL_PORT {
                FIRST_EPHEMERAL_PORT
            } else {
                value + 1
            })
        })
        .unwrap_or(FIRST_EPHEMERAL_PORT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocators_stay_in_the_dynamic_port_range() {
        for _ in 0..100 {
            assert!((FIRST_EPHEMERAL_PORT..=LAST_EPHEMERAL_PORT).contains(&next_tcp_port()));
            assert!((FIRST_EPHEMERAL_PORT..=LAST_EPHEMERAL_PORT).contains(&next_udp_port()));
        }
    }
}
