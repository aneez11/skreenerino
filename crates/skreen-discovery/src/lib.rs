use anyhow::Result;
use serde::{Deserialize, Serialize};
use skreen_proto::DeviceId;
use std::net::IpAddr;

pub const SERVICE_TYPE: &str = "_skreenerino._udp.local.";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdvertisedPeer {
    pub device: DeviceId,
    pub host: String,
    pub port: u16,
    pub room: Option<String>,
}

impl AdvertisedPeer {
    pub fn endpoint(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

pub trait DiscoveryService {
    fn advertise(&mut self, peer: &AdvertisedPeer) -> Result<()>;
    fn browse(&mut self) -> Result<Vec<AdvertisedPeer>>;
    fn shutdown(&mut self) -> Result<()>;
}

#[cfg(feature = "mdns")]
pub mod mdns {
    use super::*;
    use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
    use std::collections::HashMap;
    use std::time::{Duration, Instant};

    pub struct MdnsDiscovery {
        daemon: ServiceDaemon,
        fullnames: Vec<String>,
        browse: Option<(
            mdns_sd::Receiver<ServiceEvent>,
            std::collections::HashMap<String, AdvertisedPeer>,
        )>,
    }

    impl MdnsDiscovery {
        pub fn new() -> Result<Self> {
            Ok(Self {
                daemon: ServiceDaemon::new()?,
                fullnames: Vec::new(),
                browse: None,
            })
        }

        fn ensure_browse(&mut self) -> Result<()> {
            if self.browse.is_none() {
                let receiver = self.daemon.browse(SERVICE_TYPE)?;
                self.browse = Some((receiver, std::collections::HashMap::new()));
            }
            Ok(())
        }

        fn drain(&mut self) {
            let Some((receiver, found)) = self.browse.as_mut() else {
                return;
            };
            while let Ok(event) = receiver.try_recv() {
                match event {
                    ServiceEvent::ServiceResolved(info) => {
                        let device = info
                            .get_property_val_str("device")
                            .unwrap_or_else(|| {
                                info.get_fullname().split('.').next().unwrap_or("unknown")
                            })
                            .to_string();
                        let room = info.get_property_val_str("room").map(str::to_string);
                        let host = info
                            .get_addresses_v4()
                            .iter()
                            .next()
                            .map(|ip| ip.to_string())
                            .unwrap_or_else(|| {
                                info.get_hostname().trim_end_matches('.').to_string()
                            });
                        found.insert(
                            device.clone(),
                            AdvertisedPeer {
                                device: DeviceId(device),
                                host,
                                port: info.get_port(),
                                room,
                            },
                        );
                    }
                    ServiceEvent::ServiceRemoved(_, fullname) => {
                        let name = fullname.split('.').next().unwrap_or_default().to_string();
                        found.remove(&name);
                    }
                    _ => {}
                }
            }
        }

        /// Accumulate peers for `window`, returning everything seen so far.
        pub fn collect(&mut self, window: Duration) -> Result<Vec<AdvertisedPeer>> {
            self.ensure_browse()?;
            let deadline = Instant::now() + window;
            while Instant::now() < deadline {
                self.drain();
                std::thread::sleep(Duration::from_millis(100));
            }
            self.drain();
            Ok(self
                .browse
                .as_ref()
                .map(|(_, f)| f.values().cloned().collect())
                .unwrap_or_default())
        }
    }

    impl DiscoveryService for MdnsDiscovery {
        fn advertise(&mut self, peer: &AdvertisedPeer) -> Result<()> {
            let mut props = HashMap::new();
            props.insert("device".to_string(), peer.device.0.clone());
            if let Some(room) = &peer.room {
                props.insert("room".to_string(), room.clone());
            }
            let host_ip = peer
                .host
                .parse::<IpAddr>()
                .ok()
                .filter(|ip| !ip.is_loopback() && !ip.is_unspecified());
            let info = match host_ip {
                Some(ip) => ServiceInfo::new(
                    SERVICE_TYPE,
                    &peer.device.0,
                    &format!("{}.local.", peer.device.0),
                    ip,
                    peer.port,
                    props,
                )?,
                None => ServiceInfo::new(
                    SERVICE_TYPE,
                    &peer.device.0,
                    &format!("{}.local.", peer.device.0),
                    "",
                    peer.port,
                    props,
                )?
                .enable_addr_auto(),
            };
            self.fullnames.push(info.get_fullname().to_string());
            self.daemon.register(info)?;
            Ok(())
        }

        fn browse(&mut self) -> Result<Vec<AdvertisedPeer>> {
            self.collect(Duration::from_secs(2))
        }

        fn shutdown(&mut self) -> Result<()> {
            if self.browse.take().is_some() {
                let _ = self.daemon.stop_browse(SERVICE_TYPE);
            }
            for name in self.fullnames.drain(..) {
                let _ = self.daemon.unregister(&name);
            }
            let _ = self.daemon.shutdown();
            Ok(())
        }
    }

    impl Drop for MdnsDiscovery {
        fn drop(&mut self) {
            for name in self.fullnames.drain(..) {
                let _ = self.daemon.unregister(&name);
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManualPeer {
    pub host: String,
    pub port: u16,
}

pub fn parse_manual_endpoint(input: &str) -> Option<ManualPeer> {
    let (host, port) = input.rsplit_once(':')?;
    if host.is_empty() {
        return None;
    }
    Some(ManualPeer {
        host: host.to_string(),
        port: port.parse().ok()?,
    })
}

#[derive(Debug, Default)]
pub struct InMemoryDiscovery {
    advertised: Vec<AdvertisedPeer>,
}

impl DiscoveryService for InMemoryDiscovery {
    fn advertise(&mut self, peer: &AdvertisedPeer) -> Result<()> {
        self.advertised.push(peer.clone());
        Ok(())
    }
    fn browse(&mut self) -> Result<Vec<AdvertisedPeer>> {
        Ok(self.advertised.clone())
    }
    fn shutdown(&mut self) -> Result<()> {
        self.advertised.clear();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_manual_endpoint() {
        assert_eq!(parse_manual_endpoint("10.0.0.5:7654").unwrap().port, 7654);
        assert!(parse_manual_endpoint("nonsense").is_none());
        assert!(parse_manual_endpoint(":80").is_none());
    }

    #[test]
    fn in_memory_roundtrip() {
        let mut d = InMemoryDiscovery::default();
        let peer = AdvertisedPeer {
            device: DeviceId("bob".into()),
            host: "10.0.0.5".into(),
            port: 7654,
            room: Some("classroom".into()),
        };
        d.advertise(&peer).unwrap();
        assert_eq!(d.browse().unwrap(), vec![peer]);
    }
}
