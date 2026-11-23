//! Browser viewer gateway: serves an HTML page and an MJPEG stream of the latest
//! decoded frames over plain HTTP, so any LAN browser can watch.

use anyhow::Result;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// Latest frame as JPEG.
pub type SharedJpeg = Arc<Mutex<Option<Vec<u8>>>>;

#[derive(Debug, Default)]
pub struct GatewayStats {
    pub frames_published: std::sync::atomic::AtomicU64,
    pub clients_served: std::sync::atomic::AtomicU64,
}

const PAGE: &str = r#"<!DOCTYPE html>
<html>
<head>
  <meta charset="utf-8">
  <title>Skreenerino</title>
  <style>
    html,body{margin:0;height:100%;background:#111;color:#ddd;font:14px system-ui}
    header{padding:8px 12px;background:#1b1b1b;border-bottom:1px solid #333}
    #wrap{display:flex;align-items:center;justify-content:center;height:calc(100% - 37px)}
    img{max-width:100%;max-height:100%;image-rendering:pixelated}
  </style>
</head>
<body>
  <header>Skreenerino live view</header>
  <div id="wrap"><img id="stream" alt="waiting for stream" src="/stream.mjpeg"></div>
  <script>
    const img = document.getElementById('stream');
    img.onerror = () => setTimeout(() => { img.src = '/stream.mjpeg?t=' + Date.now(); }, 1000);
  </script>
</body>
</html>
"#;

pub async fn run(bind: std::net::SocketAddr, frames: SharedJpeg, stats: Arc<GatewayStats>) -> Result<()> {
    let listener = TcpListener::bind(bind).await?;
    println!("browser gateway on http://{}/", listener.local_addr()?);
    println!("  open that URL from any device on the LAN");

    loop {
        let (mut socket, peer) = match listener.accept().await {
            Ok(pair) => pair,
            Err(e) => {
                tracing::warn!("gateway accept failed: {e}");
                continue;
            }
        };
        let frames = frames.clone();
        let stats = stats.clone();
        tokio::spawn(async move {
            if let Err(e) = handle(&mut socket, frames, stats).await {
                tracing::debug!("gateway client {peer} ended: {e}");
            }
        });
    }
}

async fn handle(
    socket: &mut tokio::net::TcpStream,
    frames: SharedJpeg,
    stats: Arc<GatewayStats>,
) -> Result<()> {
    let mut buf = [0u8; 2048];
    let read = socket.read(&mut buf).await?;
    let request = String::from_utf8_lossy(&buf[..read]).to_string();
    let path = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("/");

    stats
        .clients_served
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

    if path.starts_with("/stream.mjpeg") {
        serve_mjpeg(socket, frames, stats).await
    } else {
        let body = PAGE.as_bytes();
        let header = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
            body.len()
        );
        socket.write_all(header.as_bytes()).await?;
        socket.write_all(body).await?;
        socket.flush().await?;
        Ok(())
    }
}

async fn serve_mjpeg(
    socket: &mut tokio::net::TcpStream,
    frames: SharedJpeg,
    stats: Arc<GatewayStats>,
) -> Result<()> {
    socket
        .write_all(
            b"HTTP/1.1 200 OK\r\nContent-Type: multipart/x-mixed-replace; boundary=frame\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        )
        .await?;

    let mut last_len = 0usize;
    let mut ticker = tokio::time::interval(std::time::Duration::from_millis(33));
    loop {
        ticker.tick().await;
        let jpeg = { frames.lock().expect("frame lock").clone() };
        let Some(jpeg) = jpeg else { continue };
        if jpeg.len() == last_len && last_len != 0 {
            continue;
        }
        last_len = jpeg.len();
        let header = format!(
            "--frame\r\nContent-Type: image/jpeg\r\nContent-Length: {}\r\n\r\n",
            jpeg.len()
        );
        if socket.write_all(header.as_bytes()).await.is_err() {
            return Ok(());
        }
        if socket.write_all(&jpeg).await.is_err() {
            return Ok(());
        }
        if socket.write_all(b"\r\n").await.is_err() {
            return Ok(());
        }
        stats
            .frames_published
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Encode a decoded RGBA frame as JPEG for the gateway.
pub fn rgba_to_jpeg(rgba: &[u8], width: u32, height: u32, quality: u8) -> Result<Vec<u8>> {
    let img = image::RgbaImage::from_raw(width, height, rgba.to_vec())
        .ok_or_else(|| anyhow::anyhow!("frame buffer does not match {width}x{height}"))?;
    let mut out = std::io::Cursor::new(Vec::new());
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality)
        .encode_image(&image::DynamicImage::ImageRgba8(img))?;
    Ok(out.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_jpeg_with_soi_marker() {
        let rgba = vec![128u8; 8 * 8 * 4];
        let jpeg = rgba_to_jpeg(&rgba, 8, 8, 60).unwrap();
        assert_eq!(&jpeg[..2], &[0xFF, 0xD8]);
    }

    #[test]
    fn rejects_mismatched_buffer() {
        let rgba = vec![0u8; 10];
        assert!(rgba_to_jpeg(&rgba, 8, 8, 60).is_err());
    }

    #[test]
    fn page_is_served_html() {
        assert!(PAGE.contains("/stream.mjpeg"));
    }
}
