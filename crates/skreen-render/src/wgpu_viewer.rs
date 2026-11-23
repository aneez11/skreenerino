//! Minimal wgpu viewer: renders decoded RGBA frames full-window with nearest sampling.

use anyhow::{Context, Result};
use std::sync::mpsc::{Receiver, SyncSender};
use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::{Window, WindowId};

use crate::{FrameSize, RgbaFrame};

#[derive(Debug, Clone)]
pub enum ViewerEvent {
    Frame { key: String, frame: Arc<RgbaFrame> },
    End,
}

pub type ViewerProxy = EventLoopProxy<ViewerEvent>;

/// Create an event-loop proxy so the network task can push frames to the UI thread.
pub fn proxy(event_loop: &EventLoop<ViewerEvent>) -> ViewerProxy {
    event_loop.create_proxy()
}

const SHADER: &str = r#"
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOutput {
    var positions = array<vec2<f32>, 6>(
        vec2<f32>(-1.0,  1.0),
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 1.0, -1.0),
        vec2<f32>(-1.0,  1.0),
        vec2<f32>( 1.0, -1.0),
        vec2<f32>( 1.0,  1.0),
    );
    var uvs = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, 1.0),
        vec2<f32>(0.0, 0.0),
        vec2<f32>(1.0, 1.0),
        vec2<f32>(1.0, 0.0),
    );
    var out: VertexOutput;
    out.position = vec4<f32>(positions[index], 0.0, 1.0);
    out.uv = uvs[index];
    return out;
}

@group(0) @binding(0) var frame_texture: texture_2d<f32>;
@group(0) @binding(1) var frame_sampler: sampler;

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return textureSample(frame_texture, frame_sampler, in.uv);
}
"#;

struct GpuState {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    textures: std::collections::HashMap<String, (wgpu::Texture, wgpu::BindGroup, FrameSize)>,
}

impl GpuState {
    fn new(window: Arc<Window>) -> Result<Self> {
        let size = window.inner_size();
        let instance = wgpu::Instance::default();
        let surface = instance.create_surface(window).context("create surface")?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))
        .context("no suitable GPU adapter")?;

        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("skreenerino-device"),
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

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("frame-shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("frame-bind-group-layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("frame-pipeline-layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("frame-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview: None,
            cache: None,
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("frame-sampler"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        Ok(Self {
            surface,
            device,
            queue,
            config,
            pipeline,
            bind_group_layout,
            sampler,
            textures: std::collections::HashMap::new(),
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
            tracing::warn!("dropping frame with mismatched buffer size");
            return;
        }
        let reusable = matches!(
            self.textures.get(key),
            Some((_, _, size)) if *size == frame.size
        );
        if reusable {
            let texture = &self.textures[key].0;
            self.queue.write_texture(
                wgpu::ImageCopyTexture {
                    texture,
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
            return;
        }

        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("frame-texture"),
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
        self.queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &texture,
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
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("frame-bind-group"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        self.textures
            .insert(key.to_string(), (texture, bind_group, frame.size));
    }

    fn render(&mut self) -> Result<(), wgpu::SurfaceError> {
        let frame = self.surface.get_current_texture()?;
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame-encoder"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("frame-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.pipeline);

            let keys: Vec<String> = {
                let mut k: Vec<String> = self.textures.keys().cloned().collect();
                k.sort();
                k
            };
            if !keys.is_empty() {
                let layout = crate::layout::TileLayout::compute(crate::layout::Layout::Grid, &keys);
                let (sw, sh) = (self.config.width as f32, self.config.height as f32);
                for tile in &layout.tiles {
                    if let Some((_, bind_group, _)) = self.textures.get(&tile.stream) {
                        let x = (tile.rect.x * sw).round().max(0.0) as u32;
                        let y = (tile.rect.y * sh).round().max(0.0) as u32;
                        let w = ((tile.rect.w * sw).floor() as u32).max(1);
                        let h = ((tile.rect.h * sh).floor() as u32).max(1);
                        pass.set_viewport(x as f32, y as f32, w as f32, h as f32, 0.0, 1.0);
                        pass.set_bind_group(0, bind_group, &[]);
                        pass.draw(0..6, 0..1);
                    }
                }
            }
        }
        self.queue.submit(Some(encoder.finish()));
        frame.present();
        Ok(())
    }
}

struct App {
    window: Option<Arc<Window>>,
    gpu: Option<GpuState>,
    frames: Receiver<(String, Arc<RgbaFrame>)>,
    pending: Option<(String, Arc<RgbaFrame>)>,
    title: String,
}

impl ApplicationHandler<ViewerEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title(&self.title)
            .with_inner_size(winit::dpi::LogicalSize::new(1280.0, 720.0));
        let window = Arc::new(event_loop.create_window(attrs).expect("create window"));
        match GpuState::new(window.clone()) {
            Ok(gpu) => {
                self.gpu = Some(gpu);
                self.window = Some(window);
            }
            Err(e) => {
                eprintln!("viewer: GPU init failed: {e:#}");
                event_loop.exit();
            }
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: ViewerEvent) {
        match event {
            ViewerEvent::Frame { key, frame } => {
                self.pending = Some((key, frame));
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            ViewerEvent::End => {
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
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
                if let (Some(gpu), Some((key, frame))) = (self.gpu.as_mut(), self.pending.take()) {
                    gpu.upload(&key, &frame);
                }
                if let Some(gpu) = &mut self.gpu {
                    match gpu.render() {
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
                }
            }
            _ => {}
        }
    }
}

/// Run a viewer window fed by single-stream frames.
pub fn run_blocking(title: String, frames: Receiver<Arc<RgbaFrame>>) -> Result<()> {
    let (tx, keyed_rx) = std::sync::mpsc::channel();
    let pump = std::thread::spawn(move || {
        while let Ok(frame) = frames.recv() {
            if tx.send(("main".to_string(), frame)).is_err() {
                break;
            }
        }
    });
    let result = run_blocking_keyed(title, keyed_rx);
    let _ = pump.join();
    result
}

/// Run a viewer window fed by keyed streams (multi-presenter wall).
pub fn run_blocking_keyed(title: String, frames: Receiver<(String, Arc<RgbaFrame>)>) -> Result<()> {
    let event_loop = EventLoop::<ViewerEvent>::with_user_event().build()?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut app = App {
        window: None,
        gpu: None,
        frames,
        pending: None,
        title,
    };
    event_loop.run_app(&mut app)?;
    Ok(())
}

/// Sender helper used by transport tasks.
pub fn forward_frames(rx: Receiver<Arc<RgbaFrame>>, proxy: ViewerProxy) {
    while let Ok(frame) = rx.recv() {
        if proxy
            .send_event(ViewerEvent::Frame {
                key: "main".into(),
                frame,
            })
            .is_err()
        {
            break;
        }
    }
    let _ = proxy.send_event(ViewerEvent::End);
}

/// Broadcast-style sender that ignores errors after window close.
/// Keyed frame channel, so a viewer can show several presenter streams at once.
pub fn keyed_frame_channel(
    capacity: usize,
) -> (
    SyncSender<(String, Arc<RgbaFrame>)>,
    Receiver<(String, Arc<RgbaFrame>)>,
) {
    std::sync::mpsc::sync_channel(capacity)
}

/// Forward keyed frames to the window; used for the multi-presenter wall.
pub fn forward_keyed_frames(rx: Receiver<(String, Arc<RgbaFrame>)>, proxy: ViewerProxy) {
    while let Ok((key, frame)) = rx.recv() {
        if proxy.send_event(ViewerEvent::Frame { key, frame }).is_err() {
            break;
        }
    }
    let _ = proxy.send_event(ViewerEvent::End);
}
pub fn frame_channel(capacity: usize) -> (SyncSender<Arc<RgbaFrame>>, Receiver<Arc<RgbaFrame>>) {
    std::sync::mpsc::sync_channel(capacity)
}
