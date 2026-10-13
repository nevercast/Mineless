use std::sync::Arc;
use std::time::{Duration, Instant};

use bytemuck::{Pod, Zeroable};
use glam::{IVec3, Mat4, Vec3};
use winit::window::Window;

mod avatar;
mod camera;
mod chunk_meshes;
mod post;
mod skins;
mod skybox;
mod texture;

pub use camera::Camera;

use avatar::AvatarRenderer;
use chunk_meshes::ChunkMeshes;
use post::{HDR_FORMAT, PostProcess};
use skybox::Skybox;
use texture::BlockTextures;

use crate::mesher::{Mesh, Vertex};
use crate::sky::Sky;
use sim::avatar::Avatar;

const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// Colour targets of every scene pipeline, matching `SceneOutput` in frame.wgsl.
const SCENE_TARGETS: [Option<wgpu::ColorTargetState>; 2] = [
    Some(wgpu::ColorTargetState {
        format: HDR_FORMAT,
        blend: None,
        write_mask: wgpu::ColorWrites::ALL,
    }),
    Some(wgpu::ColorTargetState {
        format: HDR_FORMAT,
        blend: None,
        write_mask: wgpu::ColorWrites::ALL,
    }),
];
const VERTEX_ATTRIBUTES: [wgpu::VertexAttribute; 5] = wgpu::vertex_attr_array![
    0 => Float32x3, 1 => Float32x2, 2 => Float32x3, 3 => Float32, 4 => Uint32
];
const VERTEX_LAYOUT: wgpu::VertexBufferLayout<'static> = wgpu::VertexBufferLayout {
    array_stride: size_of::<Vertex>() as wgpu::BufferAddress,
    step_mode: wgpu::VertexStepMode::Vertex,
    attributes: &VERTEX_ATTRIBUTES,
};

/// Matches `Frame` in frame.wgsl, where each vec3 is padded out to 16 bytes.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct FrameUniforms {
    view_projection: Mat4,
    inverse_view_projection: Mat4,
    light_direction: Vec3,
    time: f32,
    light_color: Vec3,
    _pad1: f32,
    ambient: Vec3,
    _pad2: f32,
    camera_position: Vec3,
    _pad3: f32,
    fog_color: Vec3,
    _pad4: f32,
    sky_tint: Vec3,
    daylight: f32,
}

pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    depth_view: wgpu::TextureView,
    post: PostProcess,
    pipeline: wgpu::RenderPipeline,
    chunk_meshes: ChunkMeshes,
    avatar: AvatarRenderer,
    skybox: Skybox,
    frame_buffer: wgpu::Buffer,
    frame_bind_group: wgpu::BindGroup,
    texture_bind_group: wgpu::BindGroup,
}

impl Renderer {
    pub async fn new(window: Arc<Window>) -> Self {
        let instance = wgpu::Instance::new(
            wgpu::InstanceDescriptor::new_with_display_handle_from_env(Box::new(window.clone())),
        );
        let size = window.inner_size();
        let surface = instance.create_surface(window).expect("create surface");
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await
            .expect("no suitable GPU adapter");
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await
            .expect("request device");

        let mut config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .expect("surface not supported by adapter");
        config.present_mode = wgpu::PresentMode::AutoVsync;
        surface.configure(&device, &config);

        let frame_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("frame"),
            size: size_of::<FrameUniforms>() as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let frame_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("frame"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let frame_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("frame"),
            layout: &frame_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: frame_buffer.as_entire_binding(),
            }],
        });

        let textures = BlockTextures::load(&device, &queue);
        let pipeline = scene_pipeline(
            &device,
            "chunk",
            &frame_shader(&device, "chunk", include_str!("shader.wgsl")),
            &[Some(&frame_layout), Some(&textures.layout)],
            Some(VERTEX_LAYOUT),
            true,
        );

        let avatar = AvatarRenderer::new(&device, &queue, &frame_layout);
        let skybox = Skybox::new(&device, &queue, &frame_layout);
        let depth_view = create_depth_view(&device, &config);
        let post = PostProcess::new(&device, config.format, (config.width, config.height));
        Self {
            surface,
            device,
            queue,
            config,
            depth_view,
            post,
            pipeline,
            chunk_meshes: ChunkMeshes::default(),
            avatar,
            skybox,
            frame_buffer,
            frame_bind_group,
            texture_bind_group: textures.bind_group,
        }
    }

    /// Replaces whatever mesh the chunk had; an empty mesh just removes it.
    pub fn upload_chunk_mesh(&mut self, chunk: IVec3, mesh: &Mesh) {
        self.chunk_meshes.insert(&self.device, chunk, mesh);
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        self.depth_view = create_depth_view(&self.device, &self.config);
        self.post.resize(&self.device, (width, height));
    }

    /// `time` is seconds since midnight, for animation. Returns how long the frame took to draw,
    /// CPU and GPU together, or `None` if there was no frame to draw into.
    pub fn render(
        &mut self,
        camera: &Camera,
        sky: &Sky,
        time: f32,
        avatars: &[Avatar],
    ) -> Option<Duration> {
        // Acquiring the frame is where vsync blocks, so the draw time starts after it.
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) => frame,
            wgpu::CurrentSurfaceTexture::Suboptimal(frame) => {
                drop(frame);
                self.surface.configure(&self.device, &self.config);
                return None;
            }
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.config);
                return None;
            }
            _ => return None,
        };
        let draw_start = Instant::now();
        let view_projection =
            camera.view_projection(self.config.width as f32 / self.config.height as f32);
        self.queue.write_buffer(
            &self.frame_buffer,
            0,
            bytemuck::bytes_of(&FrameUniforms {
                view_projection,
                inverse_view_projection: view_projection.inverse(),
                light_direction: sky.light_direction,
                time,
                light_color: sky.light_color,
                ambient: sky.ambient,
                camera_position: camera.position,
                fog_color: sky.horizon,
                sky_tint: sky.skybox_tint,
                daylight: sky.daylight,
                ..Zeroable::zeroed()
            }),
        );
        self.avatar.prepare(&self.device, &self.queue, avatars);

        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("main"),
                color_attachments: &[self.post.scene(), self.post.emissive()].map(|view| {
                    Some(wgpu::RenderPassColorAttachment {
                        view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: wgpu::StoreOp::Store,
                        },
                    })
                }),
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            self.skybox.draw(&mut pass, &self.frame_bind_group);
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.frame_bind_group, &[]);
            pass.set_bind_group(1, &self.texture_bind_group, &[]);
            for mesh in self.chunk_meshes.iter() {
                pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
                pass.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..mesh.index_count, 0, 0..1);
            }
            self.avatar.draw(&mut pass, &self.frame_bind_group);
        }
        self.post.draw(&mut encoder, &view);
        self.queue.submit([encoder.finish()]);
        // Waiting for the GPU to finish puts its work in the draw time. It gives up overlapping
        // one frame's GPU work with the next frame's CPU work, which costs nothing while vsync
        // paces the frames. A failed wait means a lost device, which the next frame reports.
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
        let draw = draw_start.elapsed();
        self.queue.present(frame);
        Some(draw)
    }
}

/// A shader whose source is `body` after frame.wgsl's shared uniforms and lighting.
fn frame_shader(device: &wgpu::Device, label: &str, body: &str) -> wgpu::ShaderModule {
    device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl([include_str!("frame.wgsl"), body].concat().into()),
    })
}

/// A pipeline drawing into the scene's colour targets with the shared depth buffer, using the
/// shader's `vs_main` and `fs_main`. `depth_write` is false for what is drawn behind everything.
fn scene_pipeline(
    device: &wgpu::Device,
    label: &str,
    shader: &wgpu::ShaderModule,
    bind_group_layouts: &[Option<&wgpu::BindGroupLayout>],
    vertex_buffer: Option<wgpu::VertexBufferLayout>,
    depth_write: bool,
) -> wgpu::RenderPipeline {
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(label),
        bind_group_layouts,
        immediate_size: 0,
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[vertex_buffer],
        },
        primitive: wgpu::PrimitiveState {
            cull_mode: Some(wgpu::Face::Back),
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(depth_write),
            depth_compare: Some(if depth_write {
                wgpu::CompareFunction::Less
            } else {
                wgpu::CompareFunction::Always
            }),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &SCENE_TARGETS,
        }),
        multiview_mask: None,
        cache: None,
    })
}

/// A filterable float texture read by fragment shaders.
fn texture_entry(
    binding: u32,
    view_dimension: wgpu::TextureViewDimension,
) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension,
            multisampled: false,
        },
        count: None,
    }
}

fn sampler_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
        count: None,
    }
}

fn create_depth_view(
    device: &wgpu::Device,
    config: &wgpu::SurfaceConfiguration,
) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("depth"),
            size: wgpu::Extent3d {
                width: config.width,
                height: config.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
        .create_view(&wgpu::TextureViewDescriptor::default())
}
