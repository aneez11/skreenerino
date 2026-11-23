use anyhow::Result;
#[cfg(all(feature = "xcap", feature = "h264"))]
use skreen_capture::{xcap_backend::XcapCapturer, ScreenCapturer};
#[cfg(all(feature = "xcap", feature = "h264"))]
use skreen_codec::{h264::OpenH264Encoder, Encoder};
use std::io::Write;

pub fn run(output: &str, frames: u32, bitrate: u32) -> Result<()> {
    #[cfg(all(feature = "xcap", feature = "h264"))]
    {
        let mut capturer = XcapCapturer::primary()?;
        capturer.start()?;
        let first = capturer.next_frame()?.ok_or_else(|| anyhow::anyhow!("no frames captured"))?;
        let mut encoder = OpenH264Encoder::new(first.width, first.height, bitrate)?;
        let mut file = std::fs::File::create(output)?;
        let mut written = 0u64;
        let mut count = 0u32;
        let mut next_frame = Some(first);
        let started = std::time::Instant::now();

        while count < frames {
            let Some(frame) = next_frame.take() else { break };
            for packet in encoder.encode(&frame.rgba, frame.width, frame.height, frame.timestamp_us)? {
                file.write_all(&packet.bytes)?;
                written += packet.bytes.len() as u64;
            }
            count += 1;
            next_frame = capturer.next_frame()?;
        }

        capturer.stop()?;
        file.flush()?;
        let elapsed = started.elapsed();
        println!(
            "wrote {count} frames ({written} bytes) to {output} in {elapsed:?} ({:.1} fps)",
            count as f64 / elapsed.as_secs_f64()
        );
        Ok(())
    }
    #[cfg(not(all(feature = "xcap", feature = "h264")))]
    {
        let _ = (output, frames, bitrate);
        anyhow::bail!("spike requires the xcap and h264 features")
    }
}
