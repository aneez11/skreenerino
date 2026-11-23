//! Audio capture -> Opus -> datagram thread, mirrored on the viewer side.

use anyhow::Result;
use bytes::Bytes;
use skreen_audio::{
    cpal_source::CpalSource,
    opus_codec::{OpusTalkDecoder, OpusTalkEncoder},
    AudioDecoder, AudioEncoder, AudioSource, FRAME_SAMPLES,
};
use skreen_proto::media::AudioDatagram;
use skreen_transport::quic;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

#[derive(Default)]
pub struct AudioStats {
    pub frames: AtomicU64,
    pub bytes: AtomicU64,
}

/// Capture and stream microphone audio on a dedicated blocking thread.
/// Returns `None` when no input device is available (headless/dev boxes).
pub fn spawn_sender(
    conn: quinn::Connection,
    stream: String,
    stats: Arc<AudioStats>,
    stop: Arc<std::sync::atomic::AtomicBool>,
) -> Result<Option<std::thread::JoinHandle<Result<()>>>> {
    // `cpal::Stream` is not `Send`, so it is constructed on the audio thread.
    let handle = std::thread::Builder::new()
        .name("skreenerino-audio".into())
        .spawn(move || -> Result<()> {
            let mut source = CpalSource::new()?;
            source.start()?;
            let mut encoder = OpusTalkEncoder::new()?;
            let mut sequence = 0u64;

            while !stop.load(Ordering::Relaxed) {
                let Some(frame) = source.next_frame()? else { continue };
                let opus = encoder.encode(&frame.pcm)?;
                let datagram = AudioDatagram {
                    stream: stream.clone(),
                    sequence,
                    timestamp_us: frame.timestamp_us,
                    opus: opus.clone(),
                };
                let body = skreen_proto::media::encode_audio(&datagram)?;
                stats.frames.fetch_add(1, Ordering::Relaxed);
                stats.bytes.fetch_add(body.len() as u64, Ordering::Relaxed);
                let _ = conn.send_datagram(Bytes::from(body));
                sequence += 1;
            }

            source.stop()?;
            Ok(())
        })?;

    Ok(Some(handle))
}

/// Viewer side: decode audio datagrams into interleaved i16 PCM batches.
pub struct AudioReceiver {
    decoder: OpusTalkDecoder,
    pub stats: Arc<AudioStats>,
    pub last_seq: u64,
}

impl AudioReceiver {
    pub fn new() -> Result<Self> {
        Ok(Self {
            decoder: OpusTalkDecoder::new()?,
            stats: Arc::new(AudioStats::default()),
            last_seq: 0,
        })
    }

    pub fn decode(&mut self, datagram: &AudioDatagram) -> Result<Vec<i16>> {
        self.stats.frames.fetch_add(1, Ordering::Relaxed);
        self.stats
            .bytes
            .fetch_add(datagram.opus.len() as u64, Ordering::Relaxed);
        self.last_seq = self.last_seq.max(datagram.sequence);
        self.decoder.decode(&datagram.opus)
    }

    pub fn frame_samples() -> usize { FRAME_SAMPLES }
}

/// Rough playout estimate used by tests and diagnostics: total audio duration
/// represented by `frames` 20 ms packets.
pub fn audio_duration(frames: u64) -> Duration {
    Duration::from_millis(frames * 20)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receiver_decodes_valid_opus() {
        let mut encoder = OpusTalkEncoder::new().unwrap();
        let pcm: Vec<i16> = (0..FRAME_SAMPLES)
            .map(|i| ((i as f32 * 0.05).sin() * 8000.0) as i16)
            .collect();
        let opus = encoder.encode(&pcm).unwrap();

        let mut receiver = AudioReceiver::new().unwrap();
        let decoded = receiver
            .decode(&AudioDatagram {
                stream: "mic".into(),
                sequence: 0,
                timestamp_us: 0,
                opus,
            })
            .unwrap();
        assert_eq!(decoded.len(), FRAME_SAMPLES);
        assert_eq!(receiver.stats.frames.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn duration_tracks_frame_count() {
        assert_eq!(audio_duration(50), Duration::from_secs(1));
    }

    #[test]
    fn encoder_output_is_much_smaller_than_pcm() {
        let mut encoder = OpusTalkEncoder::new().unwrap();
        let pcm = vec![0i16; FRAME_SAMPLES];
        let opus = encoder.encode(&pcm).unwrap();
        assert!(opus.len() < FRAME_SAMPLES * 2 / 4, "opus {} bytes", opus.len());
    }
}

