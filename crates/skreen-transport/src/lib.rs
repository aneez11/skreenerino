pub mod jitter;
pub mod quality;
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

    fn insecure_server_config() -> Result<ServerConfig> {
        let cert = rcgen::generate_simple_self_signed(vec!["skreenerino.local".to_string()])?;
        let cert_der = cert.cert.der().to_vec();
        let key_der = cert.signing_key.serialize_der();
        let mut crypto = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(
                vec![rustls::pki_types::CertificateDer::from(cert_der)],
                rustls::pki_types::PrivateKeyDer::Pkcs8(rustls::pki_types::PrivatePkcs8KeyDer::from(key_der)),
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

    fn insecure_client_config() -> Result<quinn::ClientConfig> {
        let mut crypto = rustls::ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(SkipVerify))
            .with_no_client_auth();
        crypto.alpn_protocols = vec![ALPN.to_vec()];
        Ok(quinn::ClientConfig::new(Arc::new(
            quinn::crypto::rustls::QuicClientConfig::try_from(crypto)?,
        )))
    }

    #[derive(Debug)]
    struct SkipVerify;

    impl rustls::client::danger::ServerCertVerifier for SkipVerify {
        fn verify_server_cert(
            &self,
            _end_entity: &rustls::pki_types::CertificateDer<'_>,
            _intermediates: &[rustls::pki_types::CertificateDer<'_>],
            _server_name: &rustls::pki_types::ServerName<'_>,
            _ocsp: &[u8],
            _now: rustls::pki_types::UnixTime,
        ) -> std::result::Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
            Ok(rustls::client::danger::ServerCertVerified::assertion())
        }

        fn verify_tls12_signature(
            &self,
            _message: &[u8],
            _cert: &rustls::pki_types::CertificateDer<'_>,
            _dss: &rustls::DigitallySignedStruct,
        ) -> std::result::Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
            Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
        }

        fn verify_tls13_signature(
            &self,
            _message: &[u8],
            _cert: &rustls::pki_types::CertificateDer<'_>,
            _dss: &rustls::DigitallySignedStruct,
        ) -> std::result::Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
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

        pub fn local_addr(&self) -> SocketAddr { self.addr }

        pub async fn accept(&self) -> Result<Connection> {
            let incoming = self.endpoint.accept().await.context("endpoint closed")?;
            Ok(incoming.await?)
        }

        pub fn close(&self) { self.endpoint.close(0u32.into(), b"shutdown"); }
    }

    pub async fn connect(addr: SocketAddr) -> Result<Connection> {
        let mut endpoint = Endpoint::client("0.0.0.0:0".parse()?)?;
        endpoint.set_default_client_config(insecure_client_config()?);
        let conn = endpoint.connect(addr, "skreenerino.local")?.await?;
        Ok(conn)
    }

    pub async fn open_control(conn: &Connection) -> Result<(SendStream, RecvStream)> {
        Ok(conn.open_bi().await?)
    }

    pub async fn send_datagram(conn: &Connection, payload: Bytes) -> Result<()> {
        conn.send_datagram(payload)?;
        Ok(())
    }

    pub async fn recv_datagram(conn: &Connection) -> Result<Bytes> {
        Ok(conn.read_datagram().await?)
    }
}





