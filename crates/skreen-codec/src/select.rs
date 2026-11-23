//! Encoder selection: prefer a hardware backend when the platform can provide
//! one, otherwise fall back to software with an explicit, inspectable reason.

use anyhow::Result;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncoderKind {
    Software,
    Nvenc,
    Qsv,
    Amf,
    VideoToolbox,
    Vaapi,
}

impl EncoderKind {
    pub fn is_hardware(self) -> bool {
        !matches!(self, Self::Software)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Software => "openh264-software",
            Self::Nvenc => "nvenc",
            Self::Qsv => "qsv",
            Self::Amf => "amf",
            Self::VideoToolbox => "videotoolbox",
            Self::Vaapi => "vaapi",
        }
    }
}

impl fmt::Display for EncoderKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone)]
pub struct EncoderSelection {
    pub chosen: EncoderKind,
    pub rejected: Vec<(EncoderKind, String)>,
}

impl EncoderSelection {
    pub fn summary(&self) -> String {
        if self.rejected.is_empty() {
            return format!("encoder: {}", self.chosen);
        }
        let reasons: Vec<String> = self
            .rejected
            .iter()
            .map(|(kind, why)| format!("{kind} ({why})"))
            .collect();
        format!("encoder: {} [skipped: {}]", self.chosen, reasons.join(", "))
    }
}

/// Which hardware backends are compiled in for this target. Availability of the
/// underlying driver is a separate, runtime concern.
pub fn candidate_backends() -> Vec<EncoderKind> {
    let mut out = Vec::new();
    #[cfg(all(target_os = "windows", feature = "hw-encoders"))]
    {
        out.push(EncoderKind::Nvenc);
        out.push(EncoderKind::Qsv);
        out.push(EncoderKind::Amf);
    }
    #[cfg(all(target_os = "macos", feature = "hw-encoders"))]
    out.push(EncoderKind::VideoToolbox);
    #[cfg(all(target_os = "linux", feature = "hw-encoders"))]
    out.push(EncoderKind::Vaapi);
    out
}

/// Choose an encoder. Hardware backends are reported as unavailable until their
/// driver bindings are implemented, so this always returns software today — but
/// it records why, so callers can surface it in logs and the UI.
pub fn select_encoder() -> Result<EncoderSelection> {
    let mut rejected = Vec::new();
    for kind in candidate_backends() {
        rejected.push((kind, "driver bindings not implemented".to_string()));
    }
    Ok(EncoderSelection {
        chosen: EncoderKind::Software,
        rejected,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn software_is_never_hardware() {
        assert!(!EncoderKind::Software.is_hardware());
        assert!(EncoderKind::Nvenc.is_hardware());
        assert!(EncoderKind::VideoToolbox.is_hardware());
    }

    #[test]
    fn selection_reports_skipped_backends() {
        let selection = select_encoder().unwrap();
        assert_eq!(selection.chosen, EncoderKind::Software);
        let summary = selection.summary();
        assert!(summary.starts_with("encoder: openh264-software"));
        for kind in candidate_backends() {
            assert!(
                summary.contains(kind.as_str()),
                "summary should mention skipped {kind}: {summary}"
            );
        }
    }

    #[test]
    fn display_matches_as_str() {
        for kind in [EncoderKind::Software, EncoderKind::Qsv, EncoderKind::Vaapi] {
            assert_eq!(kind.to_string(), kind.as_str());
        }
    }
}
