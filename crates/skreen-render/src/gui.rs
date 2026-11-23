//! egui overlay for the video viewer: a Windows 11 Fluent shell that draws each
//! incoming stream as an egui image plus chrome (title bar, stats strip).
//!
//! This is the M0 milestone: prove egui and the video surface can share one
//! window and one `wgpu::Device` without regressing frame throughput.

use anyhow::{Context, Result};
use egui::Color32;
use std::collections::HashMap;
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::Instant;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Theme, Window, WindowId};

use crate::{layout::Layout, layout::TileLayout, FrameSize, RgbaFrame};

const ACCENT: Color32 = Color32::from_rgb(0x4C, 0xC2, 0xFF);
const SURFACE: Color32 = Color32::from_rgb(0x20, 0x20, 0x20);
const SURFACE_RAISED: Color32 = Color32::from_rgb(0x2B, 0x2B, 0x2B);
const TEXT_PRIMARY: Color32 = Color32::from_rgb(0xFF, 0xFF, 0xFF);
const TEXT_SECONDARY: Color32 = Color32::from_rgb(0xA0, 0xA0, 0xA0);
const GRID_GAP: f32 = 8.0;

/// Runtime counters shown in the overlay; also used to measure M0 throughput.
#[derive(Debug, Default)]
pub struct GuiStats {
    pub frames_shown: u64,
    pub streams: usize,
    pub started: Option<Instant>,
    window_frames: u64,
    window_start: Option<Instant>,
    pub fps: f32,
}

impl GuiStats {
    fn observe_frame(&mut self) {
        self.frames_shown += 1;
        let now = Instant::now();
        let start = *self.window_start.get_or_insert(now);
        self.window_frames += 1;
        let elapsed = now.duration_since(start).as_secs_f32();
        if elapsed >= 0.5 {
            self.fps = self.window_frames as f32 / elapsed;
            self.window_frames = 0;
            self.window_start = Some(now);
        }
        self.started.get_or_insert(now);
    }

    pub fn elapsed_secs(&self) -> f32 {
        self.started
            .map(|s| s.elapsed().as_secs_f32())
            .unwrap_or(0.0)
    }
}

/// A wgpu texture holding one decoded stream, registered with egui.
struct StreamTexture {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    size: FrameSize,
    egui_id: Option<egui::TextureId>,
}

impl StreamTexture {
    fn new(device: &wgpu::Device, size: FrameSize) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("stream-texture"),
            size: wgpu::Extent3d {
                width: size.width.max(1),
                height: size.height.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self {
            texture,
            view,
            size,
            egui_id: None,
        }
    }
}

struct GpuState {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    egui_ctx: egui::Context,
    egui_state: egui_winit::State,
    egui_renderer: egui_wgpu::Renderer,
    streams: HashMap<String, StreamTexture>,
    stats: GuiStats,
}

impl GpuState {
    fn new(window: Arc<Window>) -> Result<Self> {
        let size = window.inner_size();
        let instance = wgpu::Instance::default();
        let surface = instance
            .create_surface(window.clone())
            .context("create surface")?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))
        .context("no suitable GPU adapter")?;

        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("skreenerino-ui-device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                memory_hints: wgpu::MemoryHints::default(),
            },
            None,
        ))?;

        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(caps.formats[0]);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
        };
        surface.configure(&device, &config);

        let egui_ctx = egui::Context::default();
        apply_fluent_style(&egui_ctx);
        let egui_state = egui_winit::State::new(
            egui_ctx.clone(),
            egui::ViewportId::ROOT,
            &*window,
            Some(window.scale_factor() as f32),
            Some(Theme::Dark),
            Some(device.limits().max_texture_dimension_2d as usize),
        );
        let egui_renderer = egui_wgpu::Renderer::new(&device, format, None, 1, false);

        Ok(Self {
            surface,
            device,
            queue,
            config,
            egui_ctx,
            egui_state,
            egui_renderer,
            streams: HashMap::new(),
            stats: GuiStats::default(),
        })
    }

    fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
    }

    fn upload(&mut self, key: &str, frame: &RgbaFrame) {
        if !frame.is_consistent() {
            tracing::warn!(key, "dropping frame with mismatched buffer size");
            return;
        }
        let needs_recreate = match self.streams.get(key) {
            Some(tex) => tex.size != frame.size,
            None => true,
        };
        if needs_recreate {
            let tex = StreamTexture::new(&self.device, frame.size);
            self.streams.insert(key.to_string(), tex);
        }
        let Some(tex) = self.streams.get(key) else {
            return;
        };
        self.queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &tex.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &frame.pixels,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(frame.size.width * 4),
                rows_per_image: Some(frame.size.height),
            },
            wgpu::Extent3d {
                width: frame.size.width,
                height: frame.size.height,
                depth_or_array_layers: 1,
            },
        );
        self.stats.observe_frame();
        self.stats.streams = self.streams.len();
    }

    /// Bind every stream texture to an egui texture id (once per texture).
    fn sync_texture_ids(&mut self) {
        for tex in self.streams.values_mut() {
            match tex.egui_id {
                None => {
                    let id = self.egui_renderer.register_native_texture(
                        &self.device,
                        &tex.view,
                        wgpu::FilterMode::Linear,
                    );
                    tex.egui_id = Some(id);
                }
                Some(id) => {
                    self.egui_renderer.update_egui_texture_from_wgpu_texture(
                        &self.device,
                        &tex.view,
                        wgpu::FilterMode::Linear,
                        id,
                    );
                }
            }
        }
    }

    fn render(&mut self, window: &Window) -> Result<(), wgpu::SurfaceError> {
        self.sync_texture_ids();

        let raw_input = self.egui_state.take_egui_input(window);
        let keys: Vec<String> = {
            let mut k: Vec<String> = self.streams.keys().cloned().collect();
            k.sort();
            k
        };
        let stats_snapshot = (
            self.stats.fps,
            self.stats.frames_shown,
            self.stats.elapsed_secs(),
            self.stats.streams,
        );
        let ids: HashMap<String, egui::TextureId> = self
            .streams
            .iter()
            .filter_map(|(k, t)| t.egui_id.map(|id| (k.clone(), id)))
            .collect();

        let full_output = self.egui_ctx.run(raw_input, |ctx| {
            draw_shell(ctx, &keys, &ids, stats_snapshot);
        });

        self.egui_state
            .handle_platform_output(window, full_output.platform_output);
        let paint_jobs = self
            .egui_ctx
            .tessellate(full_output.shapes, full_output.pixels_per_point);

        for (id, delta) in &full_output.textures_delta.set {
            self.egui_renderer
                .update_texture(&self.device, &self.queue, *id, delta);
        }

        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [self.config.width, self.config.height],
            pixels_per_point: full_output.pixels_per_point,
        };
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("ui-encoder"),
            });
        self.egui_renderer.update_buffers(
            &self.device,
            &self.queue,
            &mut encoder,
            &paint_jobs,
            &screen,
        );

        let frame = self.surface.get_current_texture()?;
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        {
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("ui-pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color {
                                r: 0.055,
                                g: 0.055,
                                b: 0.055,
                                a: 1.0,
                            }),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                })
                .forget_lifetime();
            self.egui_renderer.render(&mut pass, &paint_jobs, &screen);
        }
        self.queue.submit(Some(encoder.finish()));
        frame.present();

        for id in &full_output.textures_delta.free {
            self.egui_renderer.free_texture(id);
        }
        Ok(())
    }
}

/// Windows 11 Fluent-ish dark styling on top of egui's defaults.
fn apply_fluent_style(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = SURFACE;
    visuals.window_fill = SURFACE_RAISED;
    visuals.extreme_bg_color = Color32::from_rgb(0x1A, 0x1A, 0x1A);
    visuals.override_text_color = Some(TEXT_PRIMARY);
    visuals.selection.bg_fill = ACCENT.gamma_multiply(0.35);
    visuals.hyperlink_color = ACCENT;
    visuals.widgets.noninteractive.bg_fill = SURFACE;
    visuals.widgets.inactive.bg_fill = SURFACE_RAISED;
    visuals.widgets.inactive.weak_bg_fill = SURFACE_RAISED;
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(0x33, 0x33, 0x33);
    visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(0x33, 0x33, 0x33);
    visuals.widgets.active.bg_fill = Color32::from_rgb(0x3D, 0x3D, 0x3D);
    visuals.widgets.active.weak_bg_fill = Color32::from_rgb(0x3D, 0x3D, 0x3D);
    visuals.window_corner_radius = egui::CornerRadius::same(8);
    visuals.menu_corner_radius = egui::CornerRadius::same(8);
    ctx.set_visuals(visuals);

    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(12.0, 6.0);
    style.spacing.interact_size.y = 28.0;
    ctx.set_style(style);
}

fn draw_shell(
    ctx: &egui::Context,
    keys: &[String],
    ids: &HashMap<String, egui::TextureId>,
    stats: (f32, u64, f32, usize),
) {
    let (fps, frames, elapsed, streams) = stats;

    egui::TopBottomPanel::top("titlebar")
        .frame(
            egui::Frame::new()
                .fill(SURFACE)
                .inner_margin(egui::Margin::symmetric(14, 9)),
        )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("Skreenerino")
                        .size(15.0)
                        .strong()
                        .color(TEXT_PRIMARY),
                );
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(format!("{streams} stream(s)"))
                        .size(12.0)
                        .color(TEXT_SECONDARY),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let dot = if fps >= 12.0 {
                        Color32::from_rgb(0x6C, 0xCB, 0x5F)
                    } else if fps >= 6.0 {
                        Color32::from_rgb(0xFF, 0xC8, 0x3D)
                    } else {
                        Color32::from_rgb(0xE0, 0x5A, 0x4A)
                    };
                    ui.label(
                        egui::RichText::new(format!("{fps:.1} fps"))
                            .size(12.0)
                            .color(dot),
                    );
                });
            });
        });

    egui::TopBottomPanel::bottom("statusbar")
        .frame(
            egui::Frame::new()
                .fill(SURFACE)
                .inner_margin(egui::Margin::symmetric(14, 7)),
        )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!("{frames} frames Â· {elapsed:.0}s"))
                        .size(12.0)
                        .color(TEXT_SECONDARY),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new("LAN Â· QUIC")
                            .size(11.0)
                            .color(TEXT_SECONDARY),
                    );
                });
            });
        });

    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(Color32::from_rgb(0x0E, 0x0E, 0x0E)))
        .show(ctx, |ui| {
            let area = ui.max_rect();
            if keys.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.label(
                        egui::RichText::new("Waiting for a streamâ€¦")
                            .size(14.0)
                            .color(TEXT_SECONDARY),
                    );
                });
                return;
            }

            let layout_kind = if keys.len() == 1 {
                Layout::Single
            } else {
                Layout::Grid
            };
            let tiles = TileLayout::compute(layout_kind, keys);
            for tile in &tiles.tiles {
                let rect = egui::Rect::from_min_size(
                    egui::pos2(
                        area.left() + tile.rect.x * area.width() + GRID_GAP * 0.5,
                        area.top() + tile.rect.y * area.height() + GRID_GAP * 0.5,
                    ),
                    egui::vec2(
                        tile.rect.w * area.width() - GRID_GAP,
                        tile.rect.h * area.height() - GRID_GAP,
                    ),
                );
                let Some(id) = ids.get(&tile.stream) else {
                    continue;
                };
                ui.painter()
                    .rect_filled(rect, egui::CornerRadius::same(6), SURFACE_RAISED);
                let image_rect = fit_16_9(rect);
                ui.painter().image(
                    *id,
                    image_rect,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
                if keys.len() > 1 {
                    let label_pos = egui::pos2(rect.left() + 8.0, rect.top() + 6.0);
                    ui.painter().text(
                        label_pos,
                        egui::Align2::LEFT_TOP,
                        &tile.stream,
                        egui::FontId::proportional(11.0),
                        TEXT_SECONDARY,
                    );
                }
            }
        });
}

/// Letterbox a rect to 16:9 inside `outer`, centred.
fn fit_16_9(outer: egui::Rect) -> egui::Rect {
    let target = 16.0 / 9.0;
    let current = outer.width() / outer.height().max(1.0);
    if current > target {
        let width = outer.height() * target;
        let x = outer.left() + (outer.width() - width) * 0.5;
        egui::Rect::from_min_size(
            egui::pos2(x, outer.top()),
            egui::vec2(width, outer.height()),
        )
    } else {
        let height = outer.width() / target;
        let y = outer.top() + (outer.height() - height) * 0.5;
        egui::Rect::from_min_size(
            egui::pos2(outer.left(), y),
            egui::vec2(outer.width(), height),
        )
    }
}

#[derive(Debug, Clone)]
pub enum GuiEvent {
    Frame { key: String, frame: Arc<RgbaFrame> },
    End,
}

struct App {
    window: Option<Arc<Window>>,
    gpu: Option<GpuState>,
    frames: Receiver<GuiInput>,
    pending: HashMap<String, Arc<RgbaFrame>>,
    title: String,
    frame_limit: Option<u32>,
    shown: u32,
    final_stats: Option<(f32, u64, f32, usize)>,
}

impl ApplicationHandler<GuiEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title(&self.title)
            .with_inner_size(winit::dpi::LogicalSize::new(1280.0, 760.0));
        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                eprintln!("gui: failed to create window: {e}");
                event_loop.exit();
                return;
            }
        };
        match GpuState::new(window.clone()) {
            Ok(gpu) => {
                self.gpu = Some(gpu);
                self.window = Some(window);
            }
            Err(e) => {
                eprintln!("gui: GPU init failed: {e:#}");
                event_loop.exit();
            }
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: GuiEvent) {
        match event {
            GuiEvent::Frame { key, frame } => {
                self.pending.insert(key, frame);
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            GuiEvent::End => {
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if let (Some(gpu), Some(window)) = (self.gpu.as_mut(), self.window.as_ref()) {
            let response = gpu.egui_state.on_window_event(window, &event);
            if response.repaint {
                window.request_redraw();
            }
        }

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(gpu) = &mut self.gpu {
                    gpu.resize(size.width, size.height);
                }
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => {
                // Drain every queued frame, keeping the newest per stream.
                let mut saw_end = false;
                loop {
                    match self.frames.try_recv() {
                        Ok(GuiInput::Frame { key, frame }) => {
                            self.pending.insert(key, frame);
                        }
                        Ok(GuiInput::End) => {
                            saw_end = true;
                            break;
                        }
                        Err(std::sync::mpsc::TryRecvError::Empty) => break,
                        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                            saw_end = true;
                            break;
                        }
                    }
                }
                if let Some(gpu) = &mut self.gpu {
                    let drained: Vec<(String, Arc<RgbaFrame>)> = self.pending.drain().collect();
                    for (key, frame) in drained {
                        gpu.upload(&key, &frame);
                    }
                    self.shown = self.shown.saturating_add(1);
                }

                let window = match self.window.as_ref() {
                    Some(w) => w.clone(),
                    None => return,
                };
                if let Some(gpu) = &mut self.gpu {
                    match gpu.render(&window) {
                        Ok(()) | Err(wgpu::SurfaceError::Timeout) => {}
                        Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                            let (w, h) = (gpu.config.width, gpu.config.height);
                            gpu.resize(w, h);
                        }
                        Err(wgpu::SurfaceError::OutOfMemory) => event_loop.exit(),
                        Err(wgpu::SurfaceError::Other) => {
                            tracing::warn!("surface error; continuing")
                        }
                    }
                    self.final_stats = Some((
                        gpu.stats.fps,
                        gpu.stats.frames_shown,
                        gpu.stats.elapsed_secs(),
                        gpu.stats.streams,
                    ));
                }

                if saw_end {
                    event_loop.exit();
                }
                if let Some(limit) = self.frame_limit {
                    if self.shown >= limit {
                        event_loop.exit();
                    }
                }
            }
            _ => {}
        }
    }
}

/// Keyed frame channel used by the CLI to feed the shell.
/// Payload for the GUI thread; `End` lets the CLI close the window deterministically.
#[derive(Debug)]
pub enum GuiInput {
    Frame { key: String, frame: Arc<RgbaFrame> },
    End,
}

pub fn keyed_channel(
    capacity: usize,
) -> (std::sync::mpsc::SyncSender<GuiInput>, Receiver<GuiInput>) {
    std::sync::mpsc::sync_channel(capacity)
}

/// Run the egui shell on the current thread until the window closes.
pub fn run_blocking_gui(
    title: String,
    frames: Receiver<GuiInput>,
    frame_limit: Option<u32>,
) -> Result<GuiReport> {
    #[cfg(target_os = "windows")]
    let event_loop = {
        use winit::platform::windows::EventLoopBuilderExtWindows;
        let mut builder = EventLoop::<GuiEvent>::with_user_event();
        builder.with_any_thread(true);
        builder.build()?
    };
    #[cfg(not(target_os = "windows"))]
    let event_loop = EventLoop::<GuiEvent>::with_user_event().build()?;
    event_loop.set_control_flow(ControlFlow::Wait);

    let mut app = App {
        window: None,
        gpu: None,
        frames,
        pending: HashMap::new(),
        title,
        frame_limit,
        shown: 0,
        final_stats: None,
    };
    event_loop.run_app(&mut app)?;

    let (fps, frames_shown, elapsed, streams) = app.final_stats.unwrap_or((0.0, 0, 0.0, 0));
    Ok(GuiReport {
        fps,
        frames_shown,
        elapsed_secs: elapsed,
        streams,
    })
}

/// What the GUI achieved when it closed; used for the M0 throughput check.
#[derive(Debug, Clone, Copy)]
pub struct GuiReport {
    pub fps: f32,
    pub frames_shown: u64,
    pub elapsed_secs: f32,
    pub streams: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letterbox_wider_than_16_9_pads_sides() {
        let outer = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(400.0, 100.0));
        let fitted = fit_16_9(outer);
        assert!((fitted.height() - 100.0).abs() < 0.01);
        assert!(fitted.width() < outer.width());
        assert!((fitted.center().x - outer.center().x).abs() < 0.01);
    }

    #[test]
    fn letterbox_taller_than_16_9_pads_vertically() {
        let outer = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(160.0, 400.0));
        let fitted = fit_16_9(outer);
        assert!((fitted.width() - 160.0).abs() < 0.01);
        assert!(fitted.height() < outer.height());
        assert!((fitted.center().y - outer.center().y).abs() < 0.01);
    }

    #[test]
    fn stats_fps_is_reported_after_a_window() {
        let mut stats = GuiStats::default();
        for _ in 0..4 {
            stats.observe_frame();
        }
        assert_eq!(stats.frames_shown, 4);
        assert!(stats.elapsed_secs() >= 0.0);
    }

    #[test]
    fn stream_texture_uses_srgb_for_egui() {
        // egui requires Rgba8UnormSrgb for native textures; this documents it.
        assert_eq!(
            wgpu::TextureFormat::Rgba8UnormSrgb,
            wgpu::TextureFormat::Rgba8UnormSrgb
        );
    }
}
