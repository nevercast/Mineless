use glam::Mat4;
use wgpu::util::DeviceExt;

use crate::avatar::{self, Avatar, AvatarVertex, PART_COUNT};

const VERTEX_ATTRIBUTES: [wgpu::VertexAttribute; 4] =
    wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Float32, 3 => Uint32];
const PART_TRANSFORM_SIZE: wgpu::BufferAddress = size_of::<Mat4>() as wgpu::BufferAddress;

/// Every avatar shares one rest-pose mesh and is drawn as an instance of it. Each instance reads
/// its body part transforms from a storage buffer, so posing an avatar is just new matrices.
pub struct AvatarRenderer {
    pipeline: wgpu::RenderPipeline,
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    index_count: u32,
    layout: wgpu::BindGroupLayout,
    skin: wgpu::TextureView,
    sampler: wgpu::Sampler,
    parts_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    instance_count: u32,
}

impl AvatarRenderer {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        camera_layout: &wgpu::BindGroupLayout,
        color_format: wgpu::TextureFormat,
        depth_format: wgpu::TextureFormat,
    ) -> Self {
        let (vertices, indices) = avatar::build_mesh();
        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("avatar vertices"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("avatar indices"),
            contents: bytemuck::cast_slice(&indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        let skin = load_skin(device, queue);
        // Nearest filtering keeps the skin's texels crisp; the avatar is small enough on screen
        // that it doesn't need mips yet.
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor::default());
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("avatar"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let parts_buffer = create_parts_buffer(device, 1);
        let bind_group = create_bind_group(device, &layout, &parts_buffer, &skin, &sampler);

        let shader = device.create_shader_module(wgpu::include_wgsl!("avatar.wgsl"));
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("avatar"),
            bind_group_layouts: &[Some(camera_layout), Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("avatar"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: size_of::<AvatarVertex>() as wgpu::BufferAddress,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &VERTEX_ATTRIBUTES,
                })],
            },
            primitive: wgpu::PrimitiveState {
                cull_mode: Some(wgpu::Face::Back),
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: depth_format,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(color_format.into())],
            }),
            multiview_mask: None,
            cache: None,
        });

        Self {
            pipeline,
            vertex_buffer,
            index_buffer,
            index_count: indices.len() as u32,
            layout,
            skin,
            sampler,
            parts_buffer,
            bind_group,
            instance_count: 0,
        }
    }

    /// Uploads this frame's poses. Must run before the render pass that draws them.
    pub fn prepare(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, avatars: &[Avatar]) {
        let transforms: Vec<Mat4> = avatars.iter().flat_map(Avatar::part_transforms).collect();
        let size = transforms.len() as wgpu::BufferAddress * PART_TRANSFORM_SIZE;
        if size > self.parts_buffer.size() {
            self.parts_buffer = create_parts_buffer(device, avatars.len());
            self.bind_group = create_bind_group(
                device,
                &self.layout,
                &self.parts_buffer,
                &self.skin,
                &self.sampler,
            );
        }
        queue.write_buffer(&self.parts_buffer, 0, bytemuck::cast_slice(&transforms));
        self.instance_count = avatars.len() as u32;
    }

    /// Draws everything given to the last `prepare`, with the camera bound at group 0.
    pub fn draw(&self, pass: &mut wgpu::RenderPass, camera: &wgpu::BindGroup) {
        if self.instance_count == 0 {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, camera, &[]);
        pass.set_bind_group(1, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint16);
        pass.draw_indexed(0..self.index_count, 0, 0..self.instance_count);
    }
}

fn create_parts_buffer(device: &wgpu::Device, avatars: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("avatar parts"),
        size: (avatars.max(1) * PART_COUNT) as wgpu::BufferAddress * PART_TRANSFORM_SIZE,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn create_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    parts: &wgpu::Buffer,
    skin: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("avatar"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: parts.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(skin),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    })
}

fn load_skin(device: &wgpu::Device, queue: &wgpu::Queue) -> wgpu::TextureView {
    let png = include_bytes!("../../textures/entity/skins/player.png");
    let image = image::load_from_memory_with_format(png, image::ImageFormat::Png)
        .expect("decode player skin")
        .to_rgba8();
    let (width, height) = image.dimensions();
    device
        .create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("player skin"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &image,
        )
        .create_view(&wgpu::TextureViewDescriptor::default())
}
