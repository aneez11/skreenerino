//! Frame surface abstraction. The `wgpu` feature provides a real window.

#[cfg(feature = "gui")]
pub mod gui;
pub mod layout;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameSize {
    pub width: u32,
    pub height: u32,
}

/// A decoded RGBA frame supplied by the transport layer.
#[derive(Debug, Clone)]
pub struct RgbaFrame {
    pub size: FrameSize,
    pub pixels: Vec<u8>,
}

impl RgbaFrame {
    pub fn byte_len(&self) -> usize {
        self.pixels.len()
    }
    pub fn is_consistent(&self) -> bool {
        self.pixels.len() == (self.size.width as usize) * (self.size.height as usize) * 4
    }
}

#[cfg(feature = "wgpu")]
pub mod wgpu_viewer;
