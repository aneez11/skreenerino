//! Glass-to-glass latency measurement using an embedded timestamp token.

#[cfg(feature = "async")]
use anyhow::Result;
use std::time::{Duration, Instant};

/// A frame stamp embedded in the presented pixels as a high-contrast pattern
/// (3x3 blocks encoding the low bits of a monotonically increasing counter).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LatencyToken {
    pub sequence: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenPlacement {
    pub x: u32,
    pub y: u32,
    pub block: u32,
}

impl TokenPlacement {
    pub fn top_right(width: u32, margin: u32) -> Self {
        let block = 8;
        Self { x: width.saturating_sub(margin + block * 3), y: margin, block }
    }
}

const BITS: usize = 16;

/// Stamp the low 16 bits of `sequence` as black/white blocks.
pub fn stamp_token(pixels: &mut [u8], width: u32, height: u32, token: LatencyToken, at: TokenPlacement) {
    let stride = width as usize * 4;
    for bit in 0..BITS {
        let cx = bit % 3;
        let cy = bit / 3;
        if cy >= 3 {
            break;
        }
        let on = (token.sequence >> bit) & 1 == 1;
        let value = if on { 255u8 } else { 0u8 };
        let x0 = at.x as usize + cx * at.block as usize;
        let y0 = at.y as usize + cy * at.block as usize;
        for y in y0..(y0 + at.block as usize).min(height as usize) {
            for x in x0..(x0 + at.block as usize).min(width as usize) {
                let idx = y * stride + x * 4;
                if idx + 3 < pixels.len() {
                    pixels[idx] = value;
                    pixels[idx + 1] = value;
                    pixels[idx + 2] = value;
                    pixels[idx + 3] = 255;
                }
            }
        }
    }
}

/// Read back the token. Returns `None` when the blocks are not clearly black/white.
pub fn read_token(pixels: &[u8], width: u32, height: u32, at: TokenPlacement) -> Option<LatencyToken> {
    let stride = width as usize * 4;
    let mut sequence = 0u64;
    for bit in 0..BITS {
        let cx = bit % 3;
        let cy = bit / 3;
        if cy >= 3 {
            break;
        }
        let x = at.x as usize + cx * at.block as usize + at.block as usize / 2;
        let y = at.y as usize + cy * at.block as usize + at.block as usize / 2;
        if x >= width as usize || y >= height as usize {
            return None;
        }
        let idx = y * stride + x * 4;
        if idx + 2 >= pixels.len() {
            return None;
        }
        let luma = (pixels[idx] as u32 + pixels[idx + 1] as u32 + pixels[idx + 2] as u32) / 3;
        if luma > 160 {
            sequence |= 1 << bit;
        } else if luma < 96 {
        } else {
            return None;
        }
    }
    Some(LatencyToken { sequence })
}

#[derive(Debug)]
pub struct LatencyHarness {
    placement: TokenPlacement,
    sent: Vec<(u64, Instant)>,
    max_samples: usize,
}

#[derive(Debug, Default, Clone)]
pub struct LatencyReport {
    pub samples: usize,
    pub min_ms: f64,
    pub max_ms: f64,
    pub mean_ms: f64,
    pub p95_ms: f64,
}

impl LatencyHarness {
    pub fn new(width: u32, max_samples: usize) -> Self {
        Self { placement: TokenPlacement::top_right(width, 16), sent: Vec::new(), max_samples }
    }

    pub fn placement(&self) -> TokenPlacement { self.placement }

    pub fn on_send(&mut self, token: LatencyToken, at: Instant) {
        self.sent.push((token.sequence & 0xffff, at));
        if self.sent.len() > self.max_samples {
            self.sent.remove(0);
        }
    }

    pub fn on_receive(&self, token: LatencyToken, at: Instant) -> Option<Duration> {
        let seq = token.sequence & 0xffff;
        self.sent
            .iter()
            .rev()
            .find(|(s, _)| *s == seq)
            .map(|(_, sent_at)| at.saturating_duration_since(*sent_at))
    }

    pub fn report(samples: &[Duration]) -> LatencyReport {
        if samples.is_empty() {
            return LatencyReport::default();
        }
        let mut ms: Vec<f64> = samples.iter().map(|d| d.as_secs_f64() * 1000.0).collect();
        ms.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let mean = ms.iter().sum::<f64>() / ms.len() as f64;
        let p95_index = ((ms.len() as f64 * 0.95).ceil() as usize).saturating_sub(1).min(ms.len() - 1);
        LatencyReport {
            samples: ms.len(),
            min_ms: ms[0],
            max_ms: ms[ms.len() - 1],
            mean_ms: mean,
            p95_ms: ms[p95_index],
        }
    }
}

/// Convenience for the CLI: wait until the harness has enough samples.
#[cfg(feature = "async")]
pub async fn await_samples(mut have: impl FnMut() -> usize, target: usize, timeout: Duration) -> Result<usize> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        let n = have();
        if n >= target {
            return Ok(n);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Ok(have())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamp_then_read_back() {
        let (w, h) = (160u32, 90u32);
        let mut pixels = vec![0u8; w as usize * h as usize * 4];
        let placement = TokenPlacement::top_right(w, 16);
        let token = LatencyToken { sequence: 0b1011_0101 };
        stamp_token(&mut pixels, w, h, token, placement);
        assert_eq!(read_token(&pixels, w, h, placement), Some(token));
    }

    #[test]
    fn blank_pixels_read_as_zero() {
        let (w, h) = (160u32, 90u32);
        let pixels = vec![0u8; w as usize * h as usize * 4];
        let placement = TokenPlacement::top_right(w, 16);
        assert_eq!(read_token(&pixels, w, h, placement), Some(LatencyToken { sequence: 0 }));
    }

    #[test]
    fn report_percentiles() {
        let samples: Vec<Duration> = (1..=100).map(Duration::from_millis).collect();
        let report = LatencyHarness::report(&samples);
        assert_eq!(report.samples, 100);
        assert_eq!(report.min_ms, 1.0);
        assert_eq!(report.max_ms, 100.0);
        assert!(report.p95_ms >= 94.0 && report.p95_ms <= 96.0);
    }

    #[test]
    fn matching_sample() {
        let mut harness = LatencyHarness::new(1920, 32);
        let at = Instant::now();
        harness.on_send(LatencyToken { sequence: 7 }, at);
        let latency = harness.on_receive(LatencyToken { sequence: 7 }, at + Duration::from_millis(12));
        assert_eq!(latency.map(|d| d.as_millis()), Some(12));
        assert!(harness.on_receive(LatencyToken { sequence: 9 }, at).is_none());
    }
}

