//! Asks the player's router to pass a port through to this computer, so friends
//! outside their home can reach a server they host. It only happens when the player
//! switches it on, it asks for a limited time, and it's taken back when they stop.

use std::net::{IpAddr, SocketAddr};
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
    let outside = gateway.get_external_ip().await.ok()?;
    Some(SocketAddr::new(outside, port).to_string())
}

pub async fn close(port: u16) {
    if let Ok(gateway) = search_gateway(options()).await {
        let _ = gateway.remove_port(PortMappingProtocol::TCP, port).await;
    }
}
