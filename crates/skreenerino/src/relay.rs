//! Relay (SFU-lite): forwards encoded media datagrams from a presenter to many
//! viewers without decoding or re-encoding.

use anyhow::Result;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::broadcast;

use skreen_transport::quic;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelayRole {
    Presenter,
    Viewer,
}

/// Decides a connection's role from its first control message.
pub fn classify_first_message(bytes: &[u8]) -> RelayRole {
    match skreen_proto::decode_control(bytes) {
        Ok(skreen_proto::ControlMessage::PresentRequest { .. }) => RelayRole::Presenter,
        Ok(skreen_proto::ControlMessage::ViewRequest { .. }) => RelayRole::Viewer,
        _ => RelayRole::Viewer,
    }
}

pub struct Relay {
    upstream: broadcast::Sender<Vec<u8>>,
    bytes_forwarded: Arc<AtomicU64>,
    dropped: Arc<AtomicU64>,
}

impl Relay {
    pub fn new(capacity: usize) -> Self {
        let (upstream, _) = broadcast::channel(capacity);
        Self {
            upstream,
            bytes_forwarded: Arc::new(AtomicU64::new(0)),
            dropped: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn stats(&self) -> (u64, u64) {
        (
            self.bytes_forwarded.load(Ordering::Relaxed),
            self.dropped.load(Ordering::Relaxed),
        )
    }

    /// Accept a presenter: read its datagrams and publish them upstream.
    pub async fn accept_presenter(&self, conn: quinn::Connection) -> Result<u64> {
        let mut count = 0u64;
        tracing::debug!("presenter connected to relay");
        loop {
            match conn.read_datagram().await {
                Ok(payload) => {
                    // Drop undecodable payloads early so relayed traffic stays valid.
                    if skreen_proto::media::decode_media(&payload).is_err() {
                        self.dropped.fetch_add(1, Ordering::Relaxed);
                        continue;
                    }
                    self.bytes_forwarded
                        .fetch_add(payload.len() as u64, Ordering::Relaxed);
                    if count % 50 == 0 {
                        tracing::debug!(count, "relay forwarding");
                    }
                    let _ = self.upstream.send(payload.to_vec());
                    count += 1;
                }
                Err(e) => {
                    tracing::info!("presenter disconnected: {e}");
                    return Ok(count);
                }
            }
        }
    }

    /// Serve a viewer: forward every upstream datagram to it.
    pub async fn serve_viewer(&self, conn: quinn::Connection) -> Result<u64> {
        let mut rx = self.upstream.subscribe();
        let mut count = 0u64;
        tracing::debug!("viewer subscribed to relay upstream");
        loop {
            match rx.recv().await {
                Ok(payload) => {
                    if let Err(e) = conn.send_datagram(payload.into()) {
                        tracing::info!("viewer send failed: {e}");
                        return Ok(count);
                    }
                    count += 1;
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    self.dropped.fetch_add(n, Ordering::Relaxed);
                }
                Err(broadcast::error::RecvError::Closed) => return Ok(count),
            }
        }
    }
}

/// Run a relay endpoint: first accepted connection that presents becomes the
/// upstream source; all others are viewers.
pub async fn run_relay(bind: std::net::SocketAddr) -> Result<()> {
    let server = quic::QuicServer::bind(bind).await?;
    println!(
        "relay listening on {} (ctrl-c to stop)",
        server.local_addr()
    );
    let relay = Arc::new(Relay::new(256));
    let mut have_presenter = false;

    loop {
        let conn = server.accept().await?;
        println!("relay peer connected: {}", conn.remote_address());

        if !have_presenter {
            have_presenter = true;
            let relay = relay.clone();
            tokio::spawn(async move {
                match relay.accept_presenter(conn).await {
                    Ok(n) => println!("presenter finished after {n} datagrams"),
                    Err(e) => eprintln!("presenter error: {e}"),
                }
            });
        } else {
            let relay = relay.clone();
            tokio::spawn(async move {
                match relay.serve_viewer(conn).await {
                    Ok(n) => println!("viewer left after {n} datagrams"),
                    Err(e) => eprintln!("viewer error: {e}"),
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use skreen_proto::{ControlMessage, StreamId};

    #[test]
    fn classifies_presenter_and_viewer() {
        let present = skreen_proto::encode_control(&ControlMessage::PresentRequest {
            room: skreen_proto::RoomId("r".into()),
            stream: StreamId("s".into()),
        })
        .unwrap();
        assert_eq!(classify_first_message(&present), RelayRole::Presenter);

        let view = skreen_proto::encode_control(&ControlMessage::ViewRequest {
            room: skreen_proto::RoomId("r".into()),
            stream: StreamId("s".into()),
        })
        .unwrap();
        assert_eq!(classify_first_message(&view), RelayRole::Viewer);

        assert_eq!(classify_first_message(&[0xff, 0xff]), RelayRole::Viewer);
    }
}
