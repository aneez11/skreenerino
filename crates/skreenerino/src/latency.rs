//! Offline latency harness: capture, stamp a token, encode, decode, and compare
//! encode/decode round-trip cost without any network hop.

use anyhow::Result;
use skreen_capture::{xcap_backend::XcapCapturer, ScreenCapturer};
use skreen_codec::{h264::OpenH264Decoder, h264::OpenH264Encoder, Decoder, Encoder};
use skreen_proto::latency::{read_token, stamp_token, LatencyHarness, LatencyToken};
use std::time::{Duration, Instant};

pub fn run(frames: u32, bitrate: u32) -> Result<()> {
    let mut capturer = XcapCapturer::primary()?;
    capturer.start()?;
    let first = capturer
        .next_frame()?
        .ok_or_else(|| anyhow::anyhow!("screen capture returned no frames"))?;

    let mut encoder = OpenH264Encoder::new(first.width, first.height, bitrate)?;
    let mut decoder = OpenH264Decoder::new()?;
    let mut harness = LatencyHarness::new(first.width, 512);
    let placement = harness.placement();

    let mut latencies = Vec::new();
    let mut encode_us = Vec::new();
    let mut decode_us = Vec::new();
    let mut frame = Some(first);
    let mut sequence = 0u64;

    while let Some(mut f) = frame.take() {
        let token = LatencyToken { sequence };
        let mut pixels = f.rgba.to_vec();
        stamp_token(&mut pixels, f.width, f.height, token, placement);
        harness.on_send(token, Instant::now());

        let encode_start = Instant::now();
        let packets = encoder.encode(&pixels, f.width, f.height, f.timestamp_us)?;
        let encode_elapsed = encode_start.elapsed();
        encode_us.push(encode_elapsed);

        for packet in &packets {
            let decode_start = Instant::now();
            if let Some(decoded) = decoder.decode(&packet.bytes)? {
                let decode_elapsed = decode_start.elapsed();
                decode_us.push(decode_elapsed);
                if let Some(read) = read_token(&decoded.rgba, decoded.width, decoded.height, placement) {
                    if let Some(latency) = harness.on_receive(read, Instant::now()) {
                        latencies.push(latency);
                    }
                }
            }
        }

        sequence += 1;
        if sequence >= frames as u64 {
            break;
        }
        frame = capturer.next_frame()?;
    }
    capturer.stop()?;

    let report = LatencyHarness::report(&latencies);
    println!("frames processed: {sequence}");
    println!("decode samples:    {}", report.samples);
    if report.samples > 0 {
        println!(
            "encode+decode+tokens round trip: min {:.1} ms, mean {:.1} ms, p95 {:.1} ms, max {:.1} ms",
            report.min_ms, report.mean_ms, report.p95_ms, report.max_ms
        );
    } else {
        println!("no token samples recovered; the decoder output did not preserve the token blocks");
    }
    summarize("encode", &encode_us);
    summarize("decode", &decode_us);
    Ok(())
}

fn summarize(label: &str, samples: &[Duration]) {
    if samples.is_empty() {
        return;
    }
    let total: Duration = samples.iter().sum();
    let mean = total.as_secs_f64() * 1000.0 / samples.len() as f64;
    let mut sorted: Vec<f64> = samples.iter().map(|d| d.as_secs_f64() * 1000.0).collect();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let p95_idx = ((sorted.len() as f64 * 0.95).ceil() as usize).saturating_sub(1).min(sorted.len() - 1);
    println!(
        "{label}: mean {mean:.2} ms, p95 {:.2} ms ({} samples)",
        sorted[p95_idx],
        samples.len()
    );
}

