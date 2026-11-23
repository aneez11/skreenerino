//! Audio capture + Opus encode/decode (phase 2).

use anyhow::Result;

pub const SAMPLE_RATE: u32 = 48_000;
pub const CHANNELS: u16 = 1;
pub const FRAME_SAMPLES: usize = 960; // 20 ms at 48 kHz

#[derive(Debug, Clone)]
pub struct AudioFrame {
    pub timestamp_us: u64,
    pub pcm: Vec<i16>,
}

pub trait AudioSource {
    fn start(&mut self) -> Result<()>;
    fn next_frame(&mut self) -> Result<Option<AudioFrame>>;
    fn stop(&mut self) -> Result<()>;
}

pub trait AudioEncoder {
    fn encode(&mut self, pcm: &[i16]) -> Result<Vec<u8>>;
}

pub trait AudioDecoder {
    fn decode(&mut self, packet: &[u8]) -> Result<Vec<i16>>;
}

#[derive(Debug, Default)]
pub struct NullAudioSource;
impl AudioSource for NullAudioSource {
    fn start(&mut self) -> Result<()> { Ok(()) }
    fn next_frame(&mut self) -> Result<Option<AudioFrame>> { Ok(None) }
    fn stop(&mut self) -> Result<()> { Ok(()) }
}

#[cfg(feature = "native")]
pub mod opus_codec {
    use super::*;
    use audiopus::{coder::Decoder as OpusDecoder, coder::Encoder as OpusEncoder, Application, Channels, MutSignals, SampleRate};

    pub struct OpusTalkEncoder {
        inner: OpusEncoder,
        out: Vec<u8>,
    }

    impl OpusTalkEncoder {
        pub fn new() -> Result<Self> {
            let mut inner = OpusEncoder::new(SampleRate::Hz48000, Channels::Mono, Application::Voip)?;
            inner.set_bitrate(audiopus::Bitrate::BitsPerSecond(32_000))?;
            Ok(Self { inner, out: vec![0u8; 4096] })
        }
    }

    impl AudioEncoder for OpusTalkEncoder {
        fn encode(&mut self, pcm: &[i16]) -> Result<Vec<u8>> {
            let len = self.inner.encode(pcm, &mut self.out)?;
            Ok(self.out[..len].to_vec())
        }
    }

    pub struct OpusTalkDecoder {
        inner: OpusDecoder,
        pcm: Vec<i16>,
    }

    impl OpusTalkDecoder {
        pub fn new() -> Result<Self> {
            Ok(Self { inner: OpusDecoder::new(SampleRate::Hz48000, Channels::Mono)?, pcm: vec![0i16; FRAME_SAMPLES] })
        }
    }

    impl AudioDecoder for OpusTalkDecoder {
        fn decode(&mut self, packet: &[u8]) -> Result<Vec<i16>> {
            let len = self.inner.decode(
                Some(audiopus::packet::Packet::try_from(packet)?),
                MutSignals::try_from(&mut self.pcm[..])?,
                false,
            )?;
            Ok(self.pcm[..len].to_vec())
        }
    }
}

#[cfg(feature = "native")]
pub mod cpal_source {
    use super::*;
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use std::sync::mpsc::{channel, Receiver, Sender};
    use std::sync::{Arc, Mutex};
    use std::time::Instant;

    pub struct CpalSource {
        stream: Option<cpal::Stream>,
        started: Instant,
        buffer: Arc<Mutex<Vec<i16>>>,
        ready: Arc<Mutex<Sender<()>>>,
        frames: Option<Receiver<()>>,
    }

    impl CpalSource {
        pub fn new() -> Result<Self> {
            let host = cpal::default_host();
            let device = host.default_input_device().ok_or_else(|| anyhow::anyhow!("no input device"))?;
            let config = cpal::StreamConfig {
                channels: CHANNELS,
                sample_rate: cpal::SampleRate(SAMPLE_RATE),
                buffer_size: cpal::BufferSize::Default,
            };
            let buffer = Arc::new(Mutex::new(Vec::with_capacity(FRAME_SAMPLES * 4)));
            let (tx, rx) = channel();
            let ready = Arc::new(Mutex::new(tx));
            let buf_cb = buffer.clone();
            let ready_cb = ready.clone();
            let stream = device.build_input_stream(
                &config,
                move |data: &[f32], _| {
                    let mut buf = buf_cb.lock().expect("audio buffer poisoned");
                    for sample in data {
                        buf.push((sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16);
                    }
                    if buf.len() >= FRAME_SAMPLES {
                        drop(buf);
                        if let Ok(tx) = ready_cb.lock() {
                            let _ = tx.send(());
                        }
                    }
                },
                |err| tracing::error!("audio capture error: {err}"),
                None,
            )?;
            Ok(Self {
                stream: Some(stream),
                started: Instant::now(),
                buffer,
                ready,
                frames: Some(rx),
            })
        }
    }

    impl AudioSource for CpalSource {
        fn start(&mut self) -> Result<()> {
            if let Some(stream) = &self.stream {
                stream.play()?;
                self.started = Instant::now();
            }
            Ok(())
        }

        fn next_frame(&mut self) -> Result<Option<AudioFrame>> {
            let Some(frames) = &self.frames else { return Ok(None) };
            frames.recv_timeout(std::time::Duration::from_millis(200)).ok();
            let mut buf = self.buffer.lock().expect("audio buffer poisoned");
            if buf.len() < FRAME_SAMPLES {
                return Ok(None);
            }
            let pcm: Vec<i16> = buf.drain(..FRAME_SAMPLES).collect();
            Ok(Some(AudioFrame { timestamp_us: self.started.elapsed().as_micros() as u64, pcm }))
        }

        fn stop(&mut self) -> Result<()> {
            if let Some(stream) = self.stream.take() {
                drop(stream);
            }
            let _ = &self.ready;
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_size_matches_20ms() {
        assert_eq!(FRAME_SAMPLES, (SAMPLE_RATE as usize / 1000) * 20);
    }
}


