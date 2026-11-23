use anyhow::Result;
use skreen_proto::DeviceId;

pub const SERVICE_TYPE: &str = "_skreenerino._udp.local.";

#[derive(Debug, Clone)]
pub struct AdvertisedPeer {
    pub device: DeviceId,
    pub host: String,
    pub port: u16,
    pub room: Option<String>,
}

pub trait DiscoveryService {
    fn advertise(&mut self, peer: &AdvertisedPeer) -> Result<()>;
    fn browse(&mut self) -> Result<Vec<AdvertisedPeer>>;
    fn shutdown(&mut self) -> Result<()>;
}

#[derive(Debug, Default)]
pub struct MdnsDiscovery {
    advertised: Vec<AdvertisedPeer>,
}

impl DiscoveryService for MdnsDiscovery {
    fn advertise(&mut self, peer: &AdvertisedPeer) -> Result<()> {
        self.advertised.push(peer.clone());
        Ok(())
    }
    fn browse(&mut self) -> Result<Vec<AdvertisedPeer>> { Ok(self.advertised.clone()) }
    fn shutdown(&mut self) -> Result<()> { self.advertised.clear(); Ok(()) }
}

#[derive(Debug, Clone)]
pub struct ManualPeer {
    pub host: String,
    pub port: u16,
}

pub fn parse_manual_endpoint(input: &str) -> Option<ManualPeer> {
    let (host, port) = input.rsplit_once(':')?;
    Some(ManualPeer { host: host.to_string(), port: port.parse().ok()? })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_manual_endpoint() {
        assert_eq!(parse_manual_endpoint("10.0.0.5:7654").unwrap().port, 7654);
        assert!(parse_manual_endpoint("nonsense").is_none());
    }
}
