use std::sync::Arc;
use std::time::{Duration, Instant};

use bytemuck::{Pod, Zeroable};
use glam::{IVec3, Mat4, Vec2, Vec3};
use winit::window::Window;

mod avatar;
mod border;
mod camera;
mod chunk_meshes;
mod font;
mod loading;
mod outline;
mod post;
mod skins;
mod skybox;
mod texture;

pub use camera::{Camera, Frustum};

use avatar::AvatarRenderer;
use border::Border;
use chunk_meshes::ChunkMeshes;
use loading::LoadingScreen;
use outline::Outline;
use post::{HDR_FORMAT, PostProcess};
use skybox::Skybox;
use texture::BlockTextures;

use crate::meshed_area::VIEW_DISTANCE;
use crate::mesher::{ChunkMesh, Vertex};
use crate::sky::Sky;
use sim::avatar::Avatar;
use sim::world::{CHUNK_SIZE, ChunkSet, Extent, terrain};

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
/// Chunk vertices, then each chunk's world position as a one-instance attribute.
const CHUNK_VERTEX_LAYOUTS: [Option<wgpu::VertexBufferLayout<'static>>; 2] = [
    Some(wgpu::VertexBufferLayout {
        array_stride: size_of::<Vertex>() as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &wgpu::vertex_attr_array![0 => Uint32],
    }),
    Some(wgpu::VertexBufferLayout {
        array_stride: size_of::<[f32; 3]>() as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &wgpu::vertex_attr_array![1 => Float32x3],
    }),
];

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
    water_pipeline: wgpu::RenderPipeline,
    xray_pipeline: wgpu::RenderPipeline,
    chunk_meshes: ChunkMeshes,
    water_meshes: ChunkMeshes,
    avatar: AvatarRenderer,
    skybox: Skybox,
    border: Border,
    outline: Outline,
    loading: LoadingScreen,
    frame_buffer: wgpu::Buffer,
    frame_bind_group: wgpu::BindGroup,
    texture_bind_group: wgpu::BindGroup,
}

impl Renderer {
    /// `world` is how far the world reaches, for the border round its edge.
    pub async fn new(window: Arc<Window>, world: Extent) -> Self {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_with_display_handle_from_env(Box::new(window.clone())));
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
        let chunk_shader = frame_shader(&device, "chunk", include_str!("shader.wgsl"));
        let chunk_bind_groups = [Some(&frame_layout), Some(&textures.layout)];
        let [pipeline, water_pipeline, xray_pipeline] = [("chunk", Layer::Opaque), ("water", Layer::Translucent), ("x-ray", Layer::XRay)]
            .map(|(label, layer)| scene_pipeline(&device, label, &chunk_shader, &chunk_bind_groups, &CHUNK_VERTEX_LAYOUTS, layer));

        let avatar = AvatarRenderer::new(&device, &queue, &frame_layout);
        let skybox = Skybox::new(&device, &queue, &frame_layout);
        let border = Border::new(&device, &frame_layout, world);
        let outline = Outline::new(&device, &frame_layout);
        let depth_view = create_depth_view(&device, &config);
        let post = PostProcess::new(&device, config.format, (config.width, config.height));
        let loading = LoadingScreen::new(&device, config.format);
        Self {
            surface,
            device,
            queue,
            config,
            depth_view,
            post,
            pipeline,
            water_pipeline,
            xray_pipeline,
            chunk_meshes: ChunkMeshes::default(),
            water_meshes: ChunkMeshes::default(),
            avatar,
            skybox,
            border,
            outline,
            loading,
            frame_buffer,
            frame_bind_group,
            texture_bind_group: textures.bind_group,
        }
    }

    /// Replaces whatever meshes the chunk had; an empty mesh just removes it.
    pub fn upload_chunk_mesh(&mut self, chunk: IVec3, mesh: &ChunkMesh) {
        self.chunk_meshes.insert(&self.device, chunk, &mesh.opaque);
        self.water_meshes.insert(&self.device, chunk, &mesh.water);
    }

    /// Outlines `blocks` from the next frame on, until called again.
    pub fn outline(&mut self, blocks: &[IVec3]) {
        self.outline.prepare(&self.device, &self.queue, blocks);
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

    /// The next window frame to draw into, or `None` if there isn't one this time.
    fn acquire_frame(&mut self) -> Option<wgpu::SurfaceTexture> {
        match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) => Some(frame),
            wgpu::CurrentSurfaceTexture::Suboptimal(frame) => {
                drop(frame);
                self.surface.configure(&self.device, &self.config);
                None
            }
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.config);
                None
            }
            _ => None,
        }
    }

    /// Draws the loading screen in place of the world, with `progress` in `0..=1` on its bar
    /// and `detail` beneath it.
    pub fn render_loading(&mut self, progress: f32, detail: &str) {
        let Some(frame) = self.acquire_frame() else {
            return;
        };
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        let size = Vec2::new(self.config.width as f32, self.config.height as f32);
        self.loading.draw(&self.device, &mut encoder, &view, size, progress, detail);
        self.queue.submit([encoder.finish()]);
        self.queue.present(frame);
    }

    /// What `camera` can see with this window's shape.
    pub fn frustum(&self, camera: &Camera) -> Frustum {
        Frustum::new(self.view_projection(camera))
    }

    fn view_projection(&self, camera: &Camera) -> Mat4 {
        camera.view_projection(self.config.width as f32 / self.config.height as f32)
    }

    /// `time` is seconds since midnight, for animation. Only chunks in `seen` are drawn, if it is
    /// given. With `xray`, only the walls of caves are drawn, glowing through the rock on black, with the avatars. Returns what
    /// was drawn and how long it took, or `None` if there was no frame to draw into.
    pub fn render(
        &mut self,
        camera: &Camera,
        sky: &Sky,
        time: f32,
        avatars: &[Avatar],
        seen: Option<&ChunkSet>,
        xray: bool,
    ) -> Option<Drawn> {
        // Acquiring the frame is where vsync blocks, so the draw time starts after it.
        let frame = self.acquire_frame()?;
        let draw_start = Instant::now();
        let view_projection = self.view_projection(camera);
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

        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        let mut drawn_chunks = ChunkSet::default();
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
            let frustum = Frustum::new(view_projection);
            if xray {
                self.avatar.draw(&mut pass, &self.frame_bind_group);
                pass.set_pipeline(&self.xray_pipeline);
                pass.set_bind_group(0, &self.frame_bind_group, &[]);
                pass.set_bind_group(1, &self.texture_bind_group, &[]);
                for (chunk, mesh) in self.chunk_meshes.visible(&frustum, seen) {
                    draw_chunk(&mut pass, mesh);
                    drawn_chunks.insert(chunk);
                }
            } else {
                self.skybox.draw(&mut pass, &self.frame_bind_group);
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &self.frame_bind_group, &[]);
                pass.set_bind_group(1, &self.texture_bind_group, &[]);
                for (chunk, mesh) in self.chunk_meshes.visible(&frustum, seen) {
                    draw_chunk(&mut pass, mesh);
                    drawn_chunks.insert(chunk);
                }
                self.avatar.draw(&mut pass, &self.frame_bind_group);
                self.outline.draw(&mut pass, &self.frame_bind_group);
                // The border is nearly always the farthest translucent thing, so it goes under water.
                self.border.draw(&mut pass, &self.frame_bind_group);

                // Water blends over everything opaque, far chunks first so nearer water blends over
                // them. Faces within a chunk are unsorted; flat still water of one colour hides that.
                let mut water: Vec<_> = self.water_meshes.visible(&frustum, seen).collect();
                let distance = |chunk: IVec3| {
                    let centre = (chunk.as_vec3() + 0.5) * CHUNK_SIZE as f32;
                    centre.distance_squared(camera.position)
                };
                water.sort_by(|(a, _), (b, _)| distance(*b).total_cmp(&distance(*a)));
                pass.set_pipeline(&self.water_pipeline);
                pass.set_bind_group(0, &self.frame_bind_group, &[]);
                pass.set_bind_group(1, &self.texture_bind_group, &[]);
                for (chunk, mesh) in water {
                    draw_chunk(&mut pass, mesh);
                    drawn_chunks.insert(chunk);
                }
            }
        }
        self.post.draw(&mut encoder, &view);
        self.queue.submit([encoder.finish()]);
        // Waiting for the GPU to finish puts its work in the draw time. It gives up overlapping
        // one frame's GPU work with the next frame's CPU work, which costs nothing while vsync
        // paces the frames. A failed wait means a lost device, which the next frame reports.
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
        let time = draw_start.elapsed();
        self.queue.present(frame);
        Some(Drawn {
            time,
            chunks: drawn_chunks.len(),
        })
    }
}

/// What a frame drew.
pub struct Drawn {
    /// How long drawing took, CPU and GPU together.
    pub time: Duration,
    /// Chunks with terrain or water drawn.
    pub chunks: usize,
}

fn draw_chunk(pass: &mut wgpu::RenderPass, mesh: &chunk_meshes::GpuMesh) {
    pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
    pass.set_vertex_buffer(1, mesh.origin_buffer.slice(..));
    pass.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint16);
    pass.draw_indexed(0..mesh.index_count, 0, 0..1);
}

/// How a scene pipeline treats depth, blending and back faces.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Layer {
    /// Depth tested and written, back faces culled.
    Opaque,
    /// Drawn first and never written to depth, so the world always covers it.
    Background,
    /// Depth tested but not written, blended over the scene colour and seen from both sides, and
    /// never glowing.
    Translucent,
    /// Drawn through everything from both sides, adding to the scene colour, using the shader's `vs_xray` and `fs_xray`.
    XRay,
}

/// A shader whose source is `body` after frame.wgsl's shared uniforms, lighting and fog. Fog is
/// densest at sea level and complete at the view distance, so the shader takes both from where
/// they are defined rather than copies.
fn frame_shader(device: &wgpu::Device, label: &str, body: &str) -> wgpu::ShaderModule {
    let constants = format!(
        "const SEA_LEVEL: f32 = {}.0;\nconst VIEW_DISTANCE: f32 = {:.1};\n",
        terrain::SEA_LEVEL,
        VIEW_DISTANCE
    );
    device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl([&constants, include_str!("frame.wgsl"), body].concat().into()),
    })
}

/// A pipeline drawing into the scene's colour targets with the shared depth buffer, using the
/// shader's `vs_main` and `fs_main` unless the layer says otherwise.
fn scene_pipeline(
    device: &wgpu::Device,
    label: &str,
    shader: &wgpu::ShaderModule,
    bind_group_layouts: &[Option<&wgpu::BindGroupLayout>],
    vertex_buffers: &[Option<wgpu::VertexBufferLayout>],
    layer: Layer,
) -> wgpu::RenderPipeline {
    let additive = wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::One,
        operation: wgpu::BlendOperation::Add,
    };
    let blend = match layer {
        Layer::Translucent => Some(wgpu::BlendState::ALPHA_BLENDING),
        Layer::XRay => Some(wgpu::BlendState {
            color: additive,
            alpha: additive,
        }),
        Layer::Opaque | Layer::Background => None,
    };
    let targets = if blend.is_some() {
        let [scene, emissive] = SCENE_TARGETS;
        [
            scene.map(|target| wgpu::ColorTargetState { blend, ..target }),
            emissive.map(|target| wgpu::ColorTargetState {
                write_mask: wgpu::ColorWrites::empty(),
                ..target
            }),
        ]
    } else {
        SCENE_TARGETS
    };
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
            entry_point: Some(if layer == Layer::XRay { "vs_xray" } else { "vs_main" }),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: vertex_buffers,
        },
        primitive: wgpu::PrimitiveState {
            cull_mode: matches!(layer, Layer::Opaque | Layer::Background).then_some(wgpu::Face::Back),
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(layer == Layer::Opaque),
            depth_compare: Some(if matches!(layer, Layer::Background | Layer::XRay) {
                wgpu::CompareFunction::Always
            } else {
                wgpu::CompareFunction::Less
            }),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some(if layer == Layer::XRay { "fs_xray" } else { "fs_main" }),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &targets,
        }),
        multiview_mask: None,
        cache: None,
    })
}

/// A filterable float texture read by fragment shaders.
fn texture_entry(binding: u32, view_dimension: wgpu::TextureViewDimension) -> wgpu::BindGroupLayoutEntry {
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

fn create_depth_view(device: &wgpu::Device, config: &wgpu::SurfaceConfiguration) -> wgpu::TextureView {
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
