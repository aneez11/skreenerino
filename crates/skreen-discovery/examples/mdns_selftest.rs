use skreen_discovery::{mdns::MdnsDiscovery, AdvertisedPeer, DiscoveryService};
use skreen_proto::DeviceId;
use std::time::Duration;

fn main() -> anyhow::Result<()> {
    let name = std::env::args().nth(1).unwrap_or_else(|| "selftest".into());
    let host = std::env::args().nth(2).unwrap_or_default();
    let mut d = MdnsDiscovery::new()?;
    d.advertise(&AdvertisedPeer {
        device: DeviceId(name.clone()),
        host: host.clone(),
        port: 9999,
        room: Some("room-a".into()),
    })?;
    println!("registered {name} host={:?}; collecting 6s", host);
    let peers = d.collect(Duration::from_secs(6))?;
    println!("found {} peers: {:?}", peers.len(), peers);
    d.shutdown()?;
    Ok(())
}
