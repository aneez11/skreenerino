pub mod control;
pub mod identity;
pub mod jitter;
pub mod quality;
pub mod retransmit;
pub mod simulation;

use anyhow::Result;
use bytes::Bytes;

pub trait Transport {
    fn send_control(&self, bytes: Bytes) -> Result<()>;
    fn send_datagram(&self, bytes: Bytes) -> Result<()>;
}

pub fn mtu_safe_payload(mtu: usize, header_len: usize) -> usize {
    mtu.saturating_sub(header_len).max(1)
}

#[cfg(feature = "quic")]
pub mod quic {
    use anyhow::{Context, Result};
    use bytes::Bytes;
    use quinn::{Connection, Endpoint, RecvStream, SendStream, ServerConfig};
    use std::net::SocketAddr;
    use std::sync::Arc;

    pub const ALPN: &[u8] = b"skreenerino/1";
    pub const DEFAULT_MTU: usize = 1200;

    /// Load the device certificate, generating and persisting one on first run.
    /// A stable identity is what makes TOFU pinning meaningful: without it every
    /// server restart would present a new certificate and be refused.
    fn load_or_create_identity() -> Result<(Vec<u8>, Vec<u8>)> {
        let dir = std::env::var_os("SKREENERINO_CONFIG_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from(".skreenerino"));
        let cert_path = dir.join("identity_cert.der");
        let key_path = dir.join("identity_key.der");
        if let (Ok(cert), Ok(key)) = (std::fs::read(&cert_path), std::fs::read(&key_path)) {
            return Ok((cert, key));
        }
        let generated = rcgen::generate_simple_self_signed(vec!["skreenerino.local".to_string()])?;
        let cert_der = generated.cert.der().to_vec();
        let key_der = generated.signing_key.serialize_der();
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(&cert_path, &cert_der)?;
        std::fs::write(&key_path, &key_der)?;
        restrict_key_permissions(&key_path);
        tracing::info!(path = %cert_path.display(), "generated device identity");
        Ok((cert_der, key_der))
    }

    /// Best-effort: keep the private key out of casual view. Failure is not fatal.
    fn restrict_key_permissions(path: &std::path::Path) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
        }
        #[cfg(not(unix))]
        let _ = path;
    }

    fn insecure_server_config() -> Result<ServerConfig> {
        let (cert_der, key_der) = load_or_create_identity()?;
        let mut crypto = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(
                vec![rustls::pki_types::CertificateDer::from(cert_der)],
                rustls::pki_types::PrivateKeyDer::Pkcs8(
                    rustls::pki_types::PrivatePkcs8KeyDer::from(key_der),
                ),
            )?;
        crypto.alpn_protocols = vec![ALPN.to_vec()];
        let crypto = quinn::crypto::rustls::QuicServerConfig::try_from(crypto)?;
        let mut cfg = ServerConfig::with_crypto(Arc::new(crypto));
        let mut transport = quinn::TransportConfig::default();
        transport.max_idle_timeout(Some(std::time::Duration::from_secs(15).try_into()?));
        transport.max_concurrent_bidi_streams(64u32.into());
        transport.max_concurrent_uni_streams(64u32.into());
        cfg.transport_config(Arc::new(transport));
        Ok(cfg)
    }

    /// Client TLS config. When `expected_fingerprint` is set, the server
    /// certificate must hash to it (TOFU pinning); otherwise first contact is
    /// accepted so the caller can record the pin.
    fn client_config(expected_fingerprint: Option<String>) -> Result<quinn::ClientConfig> {
        let mut crypto = rustls::ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(PinningVerifier {
                expected_fingerprint,
                observed: Arc::new(std::sync::Mutex::new(None)),
            }))
            .with_no_client_auth();
        crypto.alpn_protocols = vec![ALPN.to_vec()];
        Ok(quinn::ClientConfig::new(Arc::new(
            quinn::crypto::rustls::QuicClientConfig::try_from(crypto)?,
        )))
    }

    /// Trust-on-first-use certificate verifier.
    ///
    /// With no pinned fingerprint the handshake succeeds and the observed
    /// fingerprint is recorded, so the caller can persist it. Once a
    /// fingerprint is pinned, any different certificate is rejected.
    #[derive(Debug)]
    struct PinningVerifier {
        expected_fingerprint: Option<String>,
        observed: Arc<std::sync::Mutex<Option<String>>>,
    }

    impl PinningVerifier {
        fn observed_fingerprint(&self) -> Option<String> {
            self.observed.lock().ok().and_then(|o| o.clone())
        }
    }

    impl rustls::client::danger::ServerCertVerifier for PinningVerifier {
        fn verify_server_cert(
            &self,
            end_entity: &rustls::pki_types::CertificateDer<'_>,
            _intermediates: &[rustls::pki_types::CertificateDer<'_>],
            _server_name: &rustls::pki_types::ServerName<'_>,
            _ocsp: &[u8],
            _now: rustls::pki_types::UnixTime,
        ) -> std::result::Result<rustls::client::danger::ServerCertVerified, rustls::Error>
        {
            let actual = fingerprint_hex(end_entity.as_ref());
            if let Ok(mut slot) = self.observed.lock() {
                *slot = Some(actual.clone());
            }
            match &self.expected_fingerprint {
                None => Ok(rustls::client::danger::ServerCertVerified::assertion()),
                Some(expected) if expected.eq_ignore_ascii_case(&actual) => {
                    Ok(rustls::client::danger::ServerCertVerified::assertion())
                }
                Some(expected) => Err(rustls::Error::General(format!(
                    "certificate fingerprint mismatch: pinned {expected}, server presented {actual}"
                ))),
            }
        }

        fn verify_tls12_signature(
            &self,
            _message: &[u8],
            _cert: &rustls::pki_types::CertificateDer<'_>,
            _dss: &rustls::DigitallySignedStruct,
        ) -> std::result::Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error>
        {
            Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
        }

        fn verify_tls13_signature(
            &self,
            _message: &[u8],
            _cert: &rustls::pki_types::CertificateDer<'_>,
            _dss: &rustls::DigitallySignedStruct,
        ) -> std::result::Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error>
        {
            Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
        }

        fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
            rustls::crypto::ring::default_provider()
                .signature_verification_algorithms
                .supported_schemes()
        }
    }

    #[derive(Clone)]
    pub struct QuicServer {
        endpoint: Endpoint,
        addr: SocketAddr,
    }

    impl QuicServer {
        pub async fn bind(addr: SocketAddr) -> Result<Self> {
            let endpoint = Endpoint::server(insecure_server_config()?, addr)?;
            let addr = endpoint.local_addr()?;
            Ok(Self { endpoint, addr })
        }

        pub fn local_addr(&self) -> SocketAddr {
            self.addr
        }

        pub async fn accept(&self) -> Result<Connection> {
            let incoming = self.endpoint.accept().await.context("endpoint closed")?;
            Ok(incoming.await?)
        }

        pub fn close(&self) {
            self.endpoint.close(0u32.into(), b"shutdown");
        }
    }

    /// Connect without a pinned certificate (first contact).
    pub async fn connect(addr: SocketAddr) -> Result<Connection> {
        connect_pinned(addr, None).await
    }

    /// Connect, enforcing `pinned_fingerprint` when given.
    pub async fn connect_pinned(
        addr: SocketAddr,
        pinned_fingerprint: Option<&str>,
    ) -> Result<Connection> {
        let verifier = Arc::new(PinningVerifier {
            expected_fingerprint: pinned_fingerprint.map(str::to_string),
            observed: Arc::new(std::sync::Mutex::new(None)),
        });
        let mut crypto = rustls::ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(verifier.clone())
            .with_no_client_auth();
        crypto.alpn_protocols = vec![ALPN.to_vec()];
        let client_config = quinn::ClientConfig::new(Arc::new(
            quinn::crypto::rustls::QuicClientConfig::try_from(crypto)?,
        ));

        let mut endpoint = Endpoint::client("0.0.0.0:0".parse()?)?;
        endpoint.set_default_client_config(client_config);
        let conn = endpoint.connect(addr, "skreenerino.local")?.await?;
        if let Some(observed) = verifier.observed_fingerprint() {
            tracing::debug!(fingerprint = %observed, "server certificate fingerprint");
        }
        Ok(conn)
    }

    /// Lowercase hex SHA-256 of a DER certificate.
    pub fn fingerprint_hex(der: &[u8]) -> String {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(der);
        hasher
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    pub async fn open_control(conn: &Connection) -> Result<(SendStream, RecvStream)> {
        Ok(conn.open_bi().await?)
    }

    /// Accept a control stream opened by the peer (duplex: replies go on the
    /// returned sender, requests arrive on the returned receiver).
    pub async fn accept_control(conn: &Connection) -> Result<(SendStream, RecvStream)> {
        Ok(conn.accept_bi().await?)
    }

    pub async fn send_datagram(conn: &Connection, payload: Bytes) -> Result<()> {
        conn.send_datagram(payload)?;
        Ok(())
    }

    pub async fn recv_datagram(conn: &Connection) -> Result<Bytes> {
        Ok(conn.read_datagram().await?)
    }
}
