pub mod select;

use anyhow::Result;
use bytes::Bytes;

#[derive(Debug, Clone)]
pub struct EncodedPacket {
    pub timestamp_us: u64,
    pub keyframe: bool,
    pub bytes: Bytes,
}

pub trait Encoder {
    fn encode(
        &mut self,
        rgba: &[u8],
        width: u32,
        height: u32,
        timestamp_us: u64,
    ) -> Result<Vec<EncodedPacket>>;
    fn force_keyframe(&mut self);
}

#[derive(Debug, Clone)]
pub struct DecodedFrame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

pub trait Decoder {
    fn decode(&mut self, packet: &[u8]) -> Result<Option<DecodedFrame>>;
}

#[derive(Debug, Default)]
pub struct NullEncoder;

impl Encoder for NullEncoder {
    fn encode(
        &mut self,
        rgba: &[u8],
        _w: u32,
        _h: u32,
        timestamp_us: u64,
    ) -> Result<Vec<EncodedPacket>> {
        Ok(vec![EncodedPacket {
            timestamp_us,
            keyframe: true,
            bytes: Bytes::copy_from_slice(rgba),
        }])
    }
    fn force_keyframe(&mut self) {}
}

#[cfg(feature = "h264")]
pub mod h264 {
    use super::*;
    use openh264::decoder::Decoder as H264Decoder;
    use openh264::encoder::{
        BitRate, Encoder as H264Encoder, EncoderConfig, FrameRate, FrameType, UsageType,
    };
    use openh264::formats::{RgbaSliceU8, YUVBuffer};
    use openh264::OpenH264API;

    pub struct OpenH264Encoder {
        encoder: H264Encoder,
        width: usize,
        height: usize,
        pending_keyframe: bool,
        bitrate_bps: u32,
        fps: f32,
    }

    impl OpenH264Encoder {
        pub fn new(width: u32, height: u32, bitrate_bps: u32) -> Result<Self> {
            let config = EncoderConfig::new()
                .bitrate(BitRate::from_bps(bitrate_bps))
                .max_frame_rate(FrameRate::from_hz(30.0))
                .usage_type(UsageType::ScreenContentRealTime);
            let encoder = H264Encoder::with_api_config(OpenH264API::from_source(), config)?;
            Ok(Self {
                encoder,
                width: width as usize,
                height: height as usize,
                pending_keyframe: false,
                bitrate_bps,
                fps: 30.0,
            })
        }

        pub fn bitrate_bps(&self) -> u32 {
            self.bitrate_bps
        }

        /// Rebuild the encoder with a new target bitrate (used by adaptive bitrate).
        /// Returns `true` when the change was applied.
        pub fn set_bitrate_bps(&mut self, bitrate_bps: u32) -> Result<bool> {
            if bitrate_bps == self.bitrate_bps {
                return Ok(false);
            }
            let config = EncoderConfig::new()
                .bitrate(BitRate::from_bps(bitrate_bps))
                .max_frame_rate(FrameRate::from_hz(self.fps))
                .usage_type(UsageType::ScreenContentRealTime);
            self.encoder = H264Encoder::with_api_config(OpenH264API::from_source(), config)?;
            self.bitrate_bps = bitrate_bps;
            self.pending_keyframe = true;
            Ok(true)
        }
    }

    impl Encoder for OpenH264Encoder {
        fn encode(
            &mut self,
            rgba: &[u8],
            width: u32,
            height: u32,
            timestamp_us: u64,
        ) -> Result<Vec<EncodedPacket>> {
            let (w, h) = (width as usize, height as usize);
            if w != self.width || h != self.height {
                self.encoder = H264Encoder::with_api_config(
                    OpenH264API::from_source(),
                    EncoderConfig::new()
                        .bitrate(BitRate::from_bps(self.bitrate_bps))
                        .max_frame_rate(FrameRate::from_hz(self.fps))
                        .usage_type(UsageType::ScreenContentRealTime),
                )?;
                self.width = w;
                self.height = h;
                self.pending_keyframe = true;
            }
            if self.pending_keyframe {
                self.encoder.force_intra_frame();
                self.pending_keyframe = false;
            }
            let rgba = RgbaSliceU8::new(rgba, (w, h));
            let yuv = YUVBuffer::from_rgba8_source(rgba);
            let bitstream = self.encoder.encode(&yuv)?;
            let keyframe = matches!(bitstream.frame_type(), FrameType::IDR | FrameType::I);
            let bytes = Bytes::from(bitstream.to_vec());
            if bytes.is_empty() {
                return Ok(Vec::new());
            }
            Ok(vec![EncodedPacket {
                timestamp_us,
                keyframe,
                bytes,
            }])
        }

        fn force_keyframe(&mut self) {
            self.pending_keyframe = true;
        }
    }

    pub struct OpenH264Decoder {
        decoder: H264Decoder,
    }

    impl OpenH264Decoder {
        pub fn new() -> Result<Self> {
            Ok(Self {
                decoder: H264Decoder::new()?,
            })
        }
    }

    impl Decoder for OpenH264Decoder {
        fn decode(&mut self, packet: &[u8]) -> Result<Option<DecodedFrame>> {
            let Some(decoded) = self.decoder.decode(packet)? else {
                return Ok(None);
            };
            let (w, h) = openh264::formats::YUVSource::dimensions(&decoded);
            let mut rgba = vec![0u8; w * h * 4];
            decoded.write_rgba8(&mut rgba);
            Ok(Some(DecodedFrame {
                width: w as u32,
                height: h as u32,
                rgba,
            }))
        }
    }
}
