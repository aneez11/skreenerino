//! Peer discovery plus the TOFU-pinned connect helper both front-ends use.

use anyhow::Result;
use skreen_discovery::AdvertisedPeer;

/// Browse for peers advertising over mDNS.
pub fn browse_peers() -> Result<Vec<AdvertisedPeer>> {
    #[cfg(feature = "mdns")]
    {
        use skreen_discovery::DiscoveryService;
        let mut d = skreen_discovery::mdns::MdnsDiscovery::new()?;
        d.browse()
    }
    #[cfg(not(feature = "mdns"))]
    {
        anyhow::bail!("built without mDNS support")
    }
}

/// Resolve `target` to a socket address: `host:port` directly, or by mDNS name.
pub fn resolve_target(target: &str) -> Result<std::net::SocketAddr> {
    if let Some(peer) = skreen_discovery::parse_manual_endpoint(target) {
        return Ok(format!("{}:{}", peer.host, peer.port).parse()?);
    }
    let peers = browse_peers()?;
    let peer = peers
        .into_iter()
        .find(|p| p.device.0 == target || p.room.as_deref() == Some(target))
        .ok_or_else(|| anyhow::anyhow!("no peer or room named `{target}`; pass `host:port`"))?;
    Ok(format!("{}:{}", peer.host, peer.port).parse()?)
}

/// Connect using the pinned certificate for this host when one exists, pinning
/// the certificate on first contact. A mismatch aborts rather than silently
/// retrusting.
#[cfg(feature = "quic")]
pub async fn connect_with_tofu(addr: std::net::SocketAddr) -> Result<quinn::Connection> {
    use skreen_security::TofuStore;
    use skreen_transport::identity::{check_pin, PinDecision};

    let path = skreen_security::default_tofu_path();
    let mut store = TofuStore::load(&path).unwrap_or_default();
    let host = addr.ip().to_string();

    match store.fingerprint(&host) {
        Some(fp) => match skreen_transport::quic::connect_pinned(addr, Some(&fp)).await {
            Ok(conn) => Ok(conn),
            Err(e) => anyhow::bail!(
                "refusing to connect to {host}: certificate does not match the pinned \
                 fingerprint ({e}). Remove the entry from {} if the device was legitimately \
                 reinstalled.",
                path.display()
            ),
        },
        None => {
            let conn = skreen_transport::quic::connect(addr).await?;
            if let Some(cert) = conn
                .peer_identity()
                .and_then(|id| {
                    id.downcast::<Vec<rustls::pki_types::CertificateDer<'static>>>()
                        .ok()
                })
                .and_then(|certs| certs.first().map(|c| c.as_ref().to_vec()))
            {
                if let PinDecision::FirstContact(fp) = check_pin(None, &cert) {
                    let _ = store.verify_or_pin(&host, &fp);
                    let _ = store.save(&path);
                    tracing::info!(host, fingerprint = %fp, "pinned new device certificate");
                }
            }
            Ok(conn)
        }
    }
}

/// Best-effort display name for this device.
pub fn default_device_name() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "skreenerino-host".into())
        .to_lowercase()
        .replace(' ', "-")
}

/// Local IP used for advertising when bound to an unspecified address.
pub fn local_display_host() -> String {
    use std::net::UdpSocket;
    UdpSocket::bind("0.0.0.0:0")
        .and_then(|s| {
            s.connect("8.8.8.8:80")?;
            s.local_addr()
        })
        .map(|addr| addr.ip().to_string())
        .unwrap_or_else(|_| "127.0.0.1".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_endpoint_resolves_without_network() {
        let addr = resolve_target("10.0.0.5:7654").unwrap();
        assert_eq!(addr.port(), 7654);
        assert_eq!(addr.ip().to_string(), "10.0.0.5");
    }

    #[test]
    fn bad_endpoint_is_rejected() {
        assert!(resolve_target("definitely-not-an-address").is_err());
    }

    #[test]
    fn device_name_is_lowercase_and_safe() {
        let name = default_device_name();
        assert!(!name.is_empty());
        assert!(!name.contains(' '));
        assert_eq!(name, name.to_lowercase());
    }
}
