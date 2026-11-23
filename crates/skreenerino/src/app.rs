//! The application window: Home screen (discover, manual join, start sharing) and
//! Meeting screen (video grid, control bar, participants, stats).
//!
//! Threading contract, from `docs/gui-plan.md`: this thread never blocks on I/O
//! and never runs capture/encode. Frame text goes through bounded channels with
//! newest-wins coalescing; commands go to the tokio runtime over a channel.

use anyhow::Result;
use egui::{Color32, RichText};
use std::collections::HashMap;
use std::sync::mpsc::{Receiver, SyncSender, TrySendError};
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::{Theme, Window, WindowId};

use skreen_app::model::{HomeModel, Screen, Settings};
use skreen_app::{FrameSink, ViewerStats};
use skreen_render::{FrameSize, RgbaFrame};

use crate::{icons, theme, widgets};

/// Messages from the UI thread to the session runtime.
#[derive(Debug, Clone)]
pub enum UiCommand {
    Join {
        target: String,
    },
    StartShare {
        room: Option<String>,
        name: Option<String>,
        fps: u32,
        bitrate: u32,
        scale: f32,
        audio: bool,
    },
    StopShare,
    Leave,
    Rescan,
}

/// Messages from the session runtime back to the UI thread.
#[derive(Debug)]
pub enum UiEvent {
    Frames(Arc<RgbaFrame>, String),
    Stats(ViewerStats),
    Peers(Vec<skreen_discovery::AdvertisedPeer>),
    Warning(String),
    Sharing(bool),
    Ended,
}

pub type UiProxy = EventLoopProxy<UiEvent>;

/// Bounded, newest-wins channel from the decode path into the UI.
#[derive(Clone)]
pub struct UiSink {
    tx: SyncSender<(String, Arc<RgbaFrame>)>,
    proxy: UiProxy,
}

impl FrameSink for UiSink {
    fn push(&self, stream: &str, frame: RgbaFrame) {
        let payload = (stream.to_string(), Arc::new(frame));
        match self.tx.try_send(payload) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                // Dropping the newest frame keeps latency bounded; the next one
                // arrives within a frame interval.
                let _ = self.proxy.send_event(UiEvent::Frames(
                    Arc::new(RgbaFrame {
                        size: FrameSize {
                            width: 0,
                            height: 0,
                        },
                        pixels: Vec::new(),
                    }),
                    String::new(),
                ));
            }
            Err(TrySendError::Disconnected(_)) => {}
        }
    }
}

struct StreamTexture {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    size: FrameSize,
    egui_id: Option<egui::TextureId>,
}

struct Gpu {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    egui_ctx: egui::Context,
    egui_state: egui_winit::State,
    egui_renderer: egui_wgpu::Renderer,
    streams: HashMap<String, StreamTexture>,
    last_size: Option<FrameSize>,
}

impl Gpu {
    fn new(window: Arc<Window>) -> Result<Self> {
        let size = window.inner_size();
        let instance = wgpu::Instance::default();
        let surface = instance.create_surface(window.clone())?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))
        .ok_or_else(|| anyhow::anyhow!("no suitable GPU adapter"))?;

        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("skreenerino-app-device"),
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
        theme::apply(&egui_ctx, theme::detect_dark_mode());
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
            last_size: None,
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
        if frame.size.width == 0 || !frame.is_consistent() {
            return;
        }
        self.last_size = Some(frame.size);
        let recreate = self
            .streams
            .get(key)
            .map(|t| t.size != frame.size)
            .unwrap_or(true);
        if recreate {
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("stream"),
                size: wgpu::Extent3d {
                    width: frame.size.width,
                    height: frame.size.height,
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
            self.streams.insert(
                key.to_string(),
                StreamTexture {
                    texture,
                    view,
                    size: frame.size,
                    egui_id: None,
                },
            );
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
    }

    fn sync_texture_ids(&mut self) {
        for tex in self.streams.values_mut() {
            match tex.egui_id {
                None => {
                    tex.egui_id = Some(self.egui_renderer.register_native_texture(
                        &self.device,
                        &tex.view,
                        wgpu::FilterMode::Linear,
                    ));
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

    fn present(
        &mut self,
        window: &Window,
        ui: impl FnMut(&egui::Context),
    ) -> Result<(), wgpu::SurfaceError> {
        self.sync_texture_ids();
        let raw_input = self.egui_state.take_egui_input(window);
        let output = self.egui_ctx.run(raw_input, ui);
        self.egui_state
            .handle_platform_output(window, output.platform_output);
        let jobs = self
            .egui_ctx
            .tessellate(output.shapes, output.pixels_per_point);
        for (id, delta) in &output.textures_delta.set {
            self.egui_renderer
                .update_texture(&self.device, &self.queue, *id, delta);
        }
        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [self.config.width, self.config.height],
            pixels_per_point: output.pixels_per_point,
        };
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("ui") });
        self.egui_renderer
            .update_buffers(&self.device, &self.queue, &mut encoder, &jobs, &screen);

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
                                r: 0.07,
                                g: 0.07,
                                b: 0.08,
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
            self.egui_renderer.render(&mut pass, &jobs, &screen);
        }
        self.queue.submit(Some(encoder.finish()));
        frame.present();
        for id in &output.textures_delta.free {
            self.egui_renderer.free_texture(id);
        }
        Ok(())
    }
}

/// UI-side application state.
pub(crate) struct App {
    window: Option<Arc<Window>>,
    gpu: Option<Gpu>,
    settings: Settings,
    home: HomeModel,
    screen: Screen,
    stats: ViewerStats,
    sharing: bool,
    join_target: Option<String>,
    last_join: Option<Instant>,
    frames: Receiver<(String, Arc<RgbaFrame>)>,
    commands: SyncSender<UiCommand>,
    pending: HashMap<String, Arc<RgbaFrame>>,
    should_quit: bool,
    /// Set by the harness so automated runs can exit after N seconds.
    quit_after: Option<Duration>,
    started: Instant,
    // ---- design-driven UI state -------------------------------------------
    dark: bool,
    page: NavPage,
    show_share_picker: bool,
    show_pair: bool,
    show_confirm_exit: bool,
    room_label: String,
    viewers: usize,
    resolution: Option<(u32, u32)>,
    encode_ms: f32,
    dropped: u64,
    share_monitor: usize,
    pair_code: String,
}

/// Which sidebar page is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavPage {
    Home,
    Settings,
    About,
}

impl ApplicationHandler<UiEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title("Skreenerino")
            .with_inner_size(winit::dpi::LogicalSize::new(1100.0, 720.0))
            .with_min_inner_size(winit::dpi::LogicalSize::new(760.0, 520.0));
        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                eprintln!("app: window creation failed: {e}");
                event_loop.exit();
                return;
            }
        };
        theme::apply_window_effects(&window, theme::detect_dark_mode());
        match Gpu::new(window.clone()) {
            Ok(gpu) => {
                self.gpu = Some(gpu);
                self.window = Some(window);
                let _ = self.commands.try_send(UiCommand::Rescan);
            }
            Err(e) => {
                eprintln!("app: GPU init failed: {e:#}");
                event_loop.exit();
            }
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UiEvent) {
        match event {
            UiEvent::Frames(frame, stream) => {
                if frame.size.width > 0 {
                    self.pending.insert(stream, frame);
                }
            }
            UiEvent::Stats(stats) => self.stats = stats,
            UiEvent::Peers(peers) => self.home.set_peers(peers),
            UiEvent::Warning(text) => {
                self.home.manual_error = Some(text.clone());
                eprintln!("app: {text}");
            }
            UiEvent::Sharing(on) => {
                self.sharing = on;
                if on {
                    self.screen = Screen::Meeting;
                }
            }
            UiEvent::Ended => {
                self.screen = Screen::Home;
                self.sharing = false;
                let _ = self.commands.try_send(UiCommand::Rescan);
            }
        }
        if let Some(window) = &self.window {
            window.request_redraw();
        }
        let _ = event_loop;
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if let (Some(gpu), Some(window)) = (self.gpu.as_mut(), self.window.as_ref()) {
            if gpu.egui_state.on_window_event(window, &event).repaint {
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
                while let Ok((key, frame)) = self.frames.try_recv() {
                    self.pending.insert(key, frame);
                }
                let drained: Vec<(String, Arc<RgbaFrame>)> = self.pending.drain().collect();
                if let Some(gpu) = &mut self.gpu {
                    for (key, frame) in drained {
                        gpu.upload(&key, &frame);
                    }
                }

                let window = match self.window.clone() {
                    Some(w) => w,
                    None => return,
                };

                let mut commands: Vec<UiCommand> = Vec::new();
                let mut new_target: Option<String> = None;
                let mut reset_recent = false;

                if self.gpu.is_some() {
                    // Split the borrow: `gpu` is taken out of `self` so the screen
                    // code can still mutate app state while rendering.
                    let mut gpu = self.gpu.take().expect("checked above");
                    let app = &mut *self;
                    let result = gpu.present(&window, |ctx| {
                        crate::screens::draw(
                            ctx,
                            app,
                            &mut commands,
                            &mut new_target,
                            &mut reset_recent,
                        );
                    });
                    self.gpu = Some(gpu);
                    if let Err(e) = result {
                        match e {
                            wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated => {
                                if let Some(gpu) = &mut self.gpu {
                                    let (w, h) = (gpu.config.width, gpu.config.height);
                                    gpu.resize(w, h);
                                }
                            }
                            wgpu::SurfaceError::OutOfMemory => event_loop.exit(),
                            _ => {}
                        }
                    }
                }

                for command in commands {
                    let _ = self.commands.try_send(command);
                }
                if let Some(target) = new_target {
                    self.settings.remember(&target);
                    let _ = self.settings.save(&Settings::default_path());
                    self.join_target = Some(target);
                    self.last_join = Some(Instant::now());
                }
                if reset_recent {
                    self.settings.recent.clear();
                    let _ = self.settings.save(&Settings::default_path());
                }
                if self.should_quit {
                    event_loop.exit();
                }
                if let Some(limit) = self.quit_after {
                    if self.started.elapsed() >= limit {
                        event_loop.exit();
                    }
                }
                // Keep animating while a stream is live so stats look alive.
                if self.screen == Screen::Meeting {
                    window.request_redraw();
                }
            }
            _ => {}
        }
    }
}

/// Accessors used by the screen-drawing code (keeps `screens.rs` readable).
impl App {
    /// Disjoint borrow of the two home-screen inputs, so the drawing code can
    /// hold both without borrowing `App` twice.
    pub(crate) fn home_inputs(&mut self) -> (&mut Settings, &mut HomeModel) {
        (&mut self.settings, &mut self.home)
    }
    pub(crate) fn settings_mut(&mut self) -> &mut Settings {
        &mut self.settings
    }
    pub(crate) fn home_mut(&mut self) -> &mut HomeModel {
        &mut self.home
    }
    pub(crate) fn home(&self) -> &HomeModel {
        &self.home
    }
    pub(crate) fn screen(&self) -> &Screen {
        &self.screen
    }
    pub(crate) fn set_screen(&mut self, screen: Screen) {
        self.screen = screen;
    }
    pub(crate) fn stats(&self) -> ViewerStats {
        self.stats
    }
    pub(crate) fn sharing(&self) -> bool {
        self.sharing
    }
    pub(crate) fn streams(&self) -> Vec<(String, egui::TextureId, FrameSize)> {
        let Some(gpu) = &self.gpu else {
            return Vec::new();
        };
        let mut out: Vec<(String, egui::TextureId, FrameSize)> = gpu
            .streams
            .iter()
            .filter_map(|(k, t)| t.egui_id.map(|id| (k.clone(), id, t.size)))
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }
    pub(crate) fn join_target(&self) -> Option<&str> {
        self.join_target.as_deref()
    }
    pub(crate) fn started_secs(&self) -> f32 {
        self.started.elapsed().as_secs_f32()
    }
    pub(crate) fn dark(&self) -> bool {
        self.dark
    }
    pub(crate) fn palette(&self) -> theme::Palette {
        theme::Palette::new(self.dark)
    }
    pub(crate) fn page(&self) -> NavPage {
        self.page
    }
    pub(crate) fn set_page(&mut self, page: NavPage) {
        self.page = page;
    }
    pub(crate) fn share_picker_open(&self) -> bool {
        self.show_share_picker
    }
    pub(crate) fn set_share_picker(&mut self, open: bool) {
        self.show_share_picker = open;
    }
    pub(crate) fn pair_open(&self) -> bool {
        self.show_pair
    }
    pub(crate) fn set_pair_open(&mut self, open: bool) {
        self.show_pair = open;
    }
    pub(crate) fn confirm_exit(&self) -> bool {
        self.show_confirm_exit
    }
    pub(crate) fn set_confirm_exit(&mut self, open: bool) {
        self.show_confirm_exit = open;
    }
    pub(crate) fn set_pair_code(&mut self, code: String) {
        self.pair_code = code;
    }
    /// "Encode" while presenting, "Decode" while watching.
    pub(crate) fn timing_label(&self) -> &'static str {
        if self.sharing {
            "Encode"
        } else {
            "Decode"
        }
    }
    pub(crate) fn encode_ms(&self) -> f32 {
        self.encode_ms
    }
    pub(crate) fn dropped(&self) -> u64 {
        self.dropped
    }
    pub(crate) fn settings_ref(&self) -> &Settings {
        &self.settings
    }
    pub(crate) fn pair_code(&self) -> &str {
        &self.pair_code
    }
    pub(crate) fn room_label(&self) -> &str {
        &self.room_label
    }
    pub(crate) fn viewers(&self) -> usize {
        self.viewers
    }
    /// People in the call as this client sees it: itself plus remote streams.
    pub(crate) fn participant_count(&self) -> usize {
        if self.sharing {
            self.viewers.max(1)
        } else {
            self.stream_count().max(1)
        }
    }
    pub(crate) fn resolution(&self) -> Option<(u32, u32)> {
        self.resolution
    }
    pub(crate) fn share_monitor(&self) -> usize {
        self.share_monitor
    }
    pub(crate) fn set_share_monitor(&mut self, index: usize) {
        self.share_monitor = index;
    }
    pub(crate) fn stream_count(&self) -> usize {
        self.gpu.as_ref().map(|g| g.streams.len()).unwrap_or(0)
    }
    pub(crate) fn quit(&mut self) {
        self.should_quit = true;
    }
}

/// Handle to the running app, used to drive automation from the CLI.
pub struct AppHandle {
    pub proxy: UiProxy,
    pub commands: SyncSender<UiCommand>,
    pub sink_tx: SyncSender<(String, Arc<RgbaFrame>)>,
    pub join_now: Box<dyn Fn(&str) + Send>,
}

/// Run the GUI. `after` lets a test harness close the window automatically.
pub fn run_app(after: Option<Duration>, initial_join: Option<String>) -> Result<()> {
    let event_loop = {
        #[cfg(target_os = "windows")]
        {
            use winit::platform::windows::EventLoopBuilderExtWindows;
            let mut builder = EventLoop::<UiEvent>::with_user_event();
            builder.with_any_thread(true);
            builder.build()?
        }
        #[cfg(not(target_os = "windows"))]
        {
            EventLoop::<UiEvent>::with_user_event().build()?
        }
    };
    event_loop.set_control_flow(ControlFlow::Wait);

    let proxy = event_loop.create_proxy();
    let (frame_tx, frame_rx) = std::sync::mpsc::sync_channel::<(String, Arc<RgbaFrame>)>(4);
    let (cmd_tx, cmd_rx) = std::sync::mpsc::sync_channel::<UiCommand>(32);

    let sink = Arc::new(UiSink {
        tx: frame_tx.clone(),
        proxy: proxy.clone(),
    });
    let handle = AppHandle {
        proxy: proxy.clone(),
        commands: cmd_tx.clone(),
        sink_tx: frame_tx.clone(),
        join_now: Box::new(|_| {}),
    };
    crate::runtime::spawn_runtime(handle, cmd_rx, sink, proxy.clone());

    let settings = Settings::load(&Settings::default_path());
    let mut app = App {
        window: None,
        gpu: None,
        settings,
        home: HomeModel::default(),
        screen: Screen::Home,
        stats: ViewerStats::default(),
        sharing: false,
        join_target: None,
        last_join: None,
        frames: frame_rx,
        commands: cmd_tx,
        pending: HashMap::new(),
        should_quit: false,
        quit_after: after,
        started: Instant::now(),
        dark: theme::detect_dark_mode(),
        page: NavPage::Home,
        show_share_picker: false,
        show_pair: false,
        show_confirm_exit: false,
        room_label: "Office".into(),
        viewers: 0,
        resolution: None,
        encode_ms: 0.0,
        dropped: 0,
        share_monitor: 0,
        pair_code: "472961".into(),
    };
    if let Some(target) = initial_join {
        app.join_target = Some(target.clone());
        app.screen = Screen::Meeting;
        let _ = app.commands.try_send(UiCommand::Join { target });
    }
    event_loop.run_app(&mut app)?;
    // Report session results so scripted/automated runs are verifiable.
    println!(
        "app: {} frames decoded, {:.1} fps, {} stream(s), {} datagrams",
        app.stats.frames,
        app.stats.fps,
        app.stream_count(),
        app.stats.datagrams
    );
    Ok(())
}

/// Format a duration as `m:ss` for the meeting timer.
pub fn format_elapsed(seconds: f32) -> String {
    let total = seconds.max(0.0) as u64;
    format!("{:02}:{:02}", total / 60, total % 60)
}

/// Quality dot colour from fps, matching the Windows 11 accent palette.
pub fn quality_color(fps: f32) -> Color32 {
    if fps >= 20.0 {
        Color32::from_rgb(0x6C, 0xCB, 0x5F)
    } else if fps >= 10.0 {
        Color32::from_rgb(0xFF, 0xC8, 0x3D)
    } else if fps > 0.0 {
        Color32::from_rgb(0xE0, 0x5A, 0x4A)
    } else {
        Color32::from_rgb(0x8A, 0x8A, 0x8A)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elapsed_formatting() {
        assert_eq!(format_elapsed(0.0), "00:00");
        assert_eq!(format_elapsed(65.0), "01:05");
        assert_eq!(format_elapsed(3600.0), "60:00");
        assert_eq!(format_elapsed(-5.0), "00:00");
    }

    #[test]
    fn quality_colors_bucket_by_fps() {
        assert_eq!(quality_color(0.0), Color32::from_rgb(0x8A, 0x8A, 0x8A));
        assert_eq!(quality_color(5.0), Color32::from_rgb(0xE0, 0x5A, 0x4A));
        assert_eq!(quality_color(15.0), Color32::from_rgb(0xFF, 0xC8, 0x3D));
        assert_eq!(quality_color(30.0), Color32::from_rgb(0x6C, 0xCB, 0x5F));
    }
}
