//! Port probes and reservations. Port of the extension's `src/ports.ts`.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream};
use std::time::{Duration, Instant};

/// Ports handed out stay reserved this long: the server may not be listening yet.
const RESERVATION: Duration = Duration::from_secs(60);
const PROBE_TIMEOUT: Duration = Duration::from_millis(400);

/// Something listens on `host:port`.
pub fn is_port_in_use(port: u16, host: IpAddr) -> bool {
    TcpStream::connect_timeout(&SocketAddr::new(host, port), PROBE_TIMEOUT).is_ok()
}

/// Listening on IPv4 or IPv6 loopback. `localhost` resolves to `::1` only on some Node versions,
/// so one address alone would mislead the health probe.
pub fn is_port_served(port: u16) -> bool {
    is_port_in_use(port, IpAddr::V4(Ipv4Addr::LOCALHOST)) || is_port_in_use(port, IpAddr::V6(Ipv6Addr::LOCALHOST))
}

pub fn find_free_port(from: u16, tries: u16) -> Option<u16> {
    (from..from.saturating_add(tries)).find(|&port| port > 0 && !is_port_served(port))
}

#[derive(Default)]
pub struct Reservations {
    handed_out: HashMap<u16, Instant>,
}

impl Reservations {
    /// The first port at or above `from` that nothing listens on and that wasn't just handed out.
    pub fn reserve_free(&mut self, from: u16) -> Option<u16> {
        self.handed_out.retain(|_, at| at.elapsed() < RESERVATION);

        let mut candidate = from;

        for _ in 0..40 {
            let free = find_free_port(candidate, 40)?;

            if let std::collections::hash_map::Entry::Vacant(slot) = self.handed_out.entry(free) {
                slot.insert(Instant::now());
                return Some(free);
            }

            candidate = free.checked_add(1)?;
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn a_busy_port_and_a_just_handed_out_port_are_both_skipped() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let busy = listener.local_addr().unwrap().port();
        let mut reservations = Reservations::default();

        let first = reservations.reserve_free(busy).unwrap();
        let second = reservations.reserve_free(busy).unwrap();

        assert_ne!(first, busy);
        assert_ne!(second, busy);
        assert_ne!(first, second);
    }
}
