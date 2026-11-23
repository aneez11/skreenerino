//! Fast nearest-neighbour RGBA downscaling for encoding at reduced resolution.

use bytes::Bytes;

use crate::CapturedFrame;

/// Downscale an RGBA buffer by an integer or fractional factor with nearest
/// sampling. Chosen over a filter because screen content stays legible and the
/// cost is a single pass, keeping the software encoder fed.
pub fn scale_rgba_nearest(
    src: &[u8],
    width: u32,
    height: u32,
    factor: f32,
) -> Option<(Bytes, u32, u32)> {
    if !(0.05..=1.0).contains(&factor) {
        return None;
    }
    let dst_w = ((width as f32 * factor).round() as u32).max(2) & !1;
    let dst_h = ((height as f32 * factor).round() as u32).max(2) & !1;
    if dst_w >= width && dst_h >= height {
        return None;
    }
    if src.len() != (width as usize) * (height as usize) * 4 {
        return None;
    }

    let mut out = vec![0u8; (dst_w as usize) * (dst_h as usize) * 4];
    let x_ratio = width as f32 / dst_w as f32;
    let y_ratio = height as f32 / dst_h as f32;

    for dy in 0..dst_h as usize {
        let sy = ((dy as f32 * y_ratio) as usize).min(height as usize - 1);
        let src_row = sy * width as usize * 4;
        let dst_row = dy * dst_w as usize * 4;
        for dx in 0..dst_w as usize {
            let sx = ((dx as f32 * x_ratio) as usize).min(width as usize - 1);
            let si = src_row + sx * 4;
            let di = dst_row + dx * 4;
            out[di..di + 4].copy_from_slice(&src[si..si + 4]);
        }
    }
    Some((Bytes::from(out), dst_w, dst_h))
}

/// Convenience wrapper that leaves the frame untouched when scaling is a no-op.
pub fn maybe_scale(frame: CapturedFrame, factor: f32) -> CapturedFrame {
    match scale_rgba_nearest(&frame.rgba, frame.width, frame.height, factor) {
        Some((rgba, width, height)) => CapturedFrame {
            width,
            height,
            timestamp_us: frame.timestamp_us,
            rgba,
        },
        None => frame,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn halves_dimensions_and_keeps_pixel_count() {
        let (w, h) = (8u32, 4u32);
        let src = vec![200u8; (w * h * 4) as usize];
        let (out, ow, oh) = scale_rgba_nearest(&src, w, h, 0.5).unwrap();
        assert_eq!((ow, oh), (4, 2));
        assert_eq!(out.len(), (ow * oh * 4) as usize);
        assert!(out.iter().all(|b| *b == 200));
    }

    #[test]
    fn preserves_distinct_pixels_by_nearest_sampling() {
        // 4x2 image: left half red, right half blue.
        let (w, h) = (4u32, 2u32);
        let mut src = vec![0u8; (w * h * 4) as usize];
        for y in 0..h as usize {
            for x in 0..w as usize {
                let i = (y * w as usize + x) * 4;
                if x < 2 {
                    src[i] = 255;
                } else {
                    src[i + 2] = 255;
                }
                src[i + 3] = 255;
            }
        }
        let (out, ow, oh) = scale_rgba_nearest(&src, w, h, 0.5).unwrap();
        // Height is rounded up to stay even for the codec.\n        assert_eq!((ow, oh), (2, 2));
        assert_eq!(&out[0..3], &[255, 0, 0]);
        assert_eq!(&out[4..7], &[0, 0, 255]);
    }

    #[test]
    fn rejects_out_of_range_factors_and_bad_buffers() {
        let src = vec![0u8; 4 * 4 * 4];
        assert!(scale_rgba_nearest(&src, 4, 4, 0.0).is_none());
        assert!(scale_rgba_nearest(&src, 4, 4, 1.5).is_none());
        assert!(scale_rgba_nearest(&[], 4, 4, 0.5).is_none());
    }

    #[test]
    fn noop_when_factor_is_one() {
        let frame = CapturedFrame {
            width: 4,
            height: 4,
            timestamp_us: 0,
            rgba: Bytes::from(vec![0u8; 64]),
        };
        let same = maybe_scale(frame, 1.0);
        assert_eq!((same.width, same.height), (4, 4));
    }

    #[test]
    fn output_dimensions_stay_even_for_codecs() {
        let src = vec![0u8; 100 * 100 * 4];
        let (_, w, h) = scale_rgba_nearest(&src, 100, 100, 0.7).unwrap();
        assert_eq!(w % 2, 0);
        assert_eq!(h % 2, 0);
    }
}
