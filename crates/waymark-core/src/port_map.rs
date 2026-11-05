//! Asks the player's router to pass a port through to this computer, so friends
//! outside their home can reach a server they host. It only happens when the player
//! switches it on, it asks for a limited time, and it's taken back when they stop.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

use igd_next::aio::tokio::search_gateway;
use igd_next::{PortMappingProtocol, SearchOptions};

use crate::brand::PRODUCT_NAME;

/// Long enough to outlive a short gap in renewing it, short enough that a port
/// left behind by a crash closes by itself.
const LEASE_SECONDS: u32 = 3600;
const SEARCH_TIMEOUT: Duration = Duration::from_secs(4);

/// How often to ask again while a server runs.
pub const RENEW_EVERY: Duration = Duration::from_secs(1800);

fn options() -> SearchOptions {
    SearchOptions {
        timeout: Some(SEARCH_TIMEOUT),
        ..SearchOptions::default()
    }
}

/// Opens `port` and returns the address the rest of the internet can use. `None`
/// means the router won't, which is common, and friends then need to be on the
/// same network or the player has to set the port up by hand.
pub async fn open(local: IpAddr, port: u16) -> Option<String> {
    let gateway = search_gateway(options()).await.ok()?;
    gateway
        .add_port(
            PortMappingProtocol::TCP,
            port,
            SocketAddr::new(local, port),
            LEASE_SECONDS,
            PRODUCT_NAME,
        )
        .await
        .ok()?;
    let outside = gateway.get_external_ip().await.ok();
    match outside.and_then(|outside| public_address(outside, port)) {
        Some(address) => Some(address),
        None => {
            // The router is behind another one, often the internet provider's
            // (carrier-grade NAT), so nobody outside can reach this port anyway.
            // Showing friends that address would only fail; take the port back.
            let _ = gateway.remove_port(PortMappingProtocol::TCP, port).await;
            None
        }
    }
}

/// The address to give friends, if the router's own outside address really is
/// on the internet.
fn public_address(outside: IpAddr, port: u16) -> Option<String> {
    let reachable = match outside {
        IpAddr::V4(address) => {
            !(address.is_private()
                || address.is_loopback()
                || address.is_link_local()
                || address.is_unspecified()
                || address.is_broadcast()
                || is_shared(address))
        }
        IpAddr::V6(address) => {
            let first = address.segments()[0];
            !(address.is_loopback()
                || address.is_unspecified()
                || first & 0xfe00 == 0xfc00
                || first & 0xffc0 == 0xfe80)
        }
    };
    reachable.then(|| SocketAddr::new(outside, port).to_string())
}

/// 100.64.0.0/10, which internet providers use for carrier-grade NAT.
fn is_shared(address: Ipv4Addr) -> bool {
    let [a, b, ..] = address.octets();
    a == 100 && (64..128).contains(&b)
}

pub async fn close(port: u16) {
    if let Ok(gateway) = search_gateway(options()).await {
        let _ = gateway.remove_port(PortMappingProtocol::TCP, port).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Nothing here talks to a router: a test must never change someone's network.

    #[test]
    fn only_an_address_on_the_internet_is_given_to_friends() {
        let at = |address: &str| public_address(address.parse().unwrap(), 25565);
        assert_eq!(at("203.0.113.7").as_deref(), Some("203.0.113.7:25565"));
        assert_eq!(at("2001:db8::1").as_deref(), Some("[2001:db8::1]:25565"));
        for unreachable in [
            "192.168.1.20",
            "10.0.0.2",
            "172.16.5.4",
            "100.64.0.1",
            "100.127.255.254",
            "127.0.0.1",
            "169.254.1.1",
            "0.0.0.0",
            "fd00::1",
            "fe80::1",
            "::1",
        ] {
            assert_eq!(at(unreachable), None, "{unreachable}");
        }
        assert!(at("100.63.255.255").is_some() && at("100.128.0.0").is_some());
    }

    #[test]
    fn the_port_is_renewed_well_before_it_runs_out() {
        assert!(RENEW_EVERY.as_secs() * 2 <= u64::from(LEASE_SECONDS));
    }
}
