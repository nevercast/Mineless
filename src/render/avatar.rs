use bytemuck::{Pod, Zeroable};
use glam::Mat4;
use image::imageops::{self, FilterType};
use wgpu::util::DeviceExt;

use crate::avatar::{self, Avatar, AvatarVertex, PART_COUNT, Skin};

const VERTEX_ATTRIBUTES: [wgpu::VertexAttribute; 4] =
    wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Float32x3, 3 => Uint32];
/// Every skin is scaled up to this size so they all fit one texture array. 128 is the 64x64
/// Minecraft layout at twice the detail.
const SKIN_SIZE: u32 = 128;

/// Matches `Instance` in avatar.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Instance {
    parts: [Mat4; PART_COUNT],
    skin: u32,
    _pad: [u32; 3],
}

/// Every avatar shares one rest-pose mesh and is drawn as an instance of it. Each instance reads
/// its body part transforms and skin layer from a storage buffer, so posing an avatar is just new
/// matrices.
pub struct AvatarRenderer {
    pipeline: wgpu::RenderPipeline,
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    index_count: u32,
    layout: wgpu::BindGroupLayout,
    skins: wgpu::TextureView,
    sampler: wgpu::Sampler,
    instance_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    instance_count: u32,
}

impl AvatarRenderer {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame_layout: &wgpu::BindGroupLayout,
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
        let skins = load_skins(device, queue);
        // Nearest filtering keeps the skins' texels crisp; the avatar is small enough on screen
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
                        view_dimension: wgpu::TextureViewDimension::D2Array,
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
        let instance_buffer = create_instance_buffer(device, 1);
        let bind_group = create_bind_group(device, &layout, &instance_buffer, &skins, &sampler);

        let shader = super::frame_shader(device, "avatar", include_str!("avatar.wgsl"));
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("avatar"),
            bind_group_layouts: &[Some(frame_layout), Some(&layout)],
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
            skins,
            sampler,
            instance_buffer,
            bind_group,
            instance_count: 0,
        }
    }

    /// Uploads this frame's poses. Must run before the render pass that draws them.
    pub fn prepare(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, avatars: &[Avatar]) {
        let instances: Vec<Instance> = avatars
            .iter()
            .map(|avatar| Instance {
                parts: avatar.part_transforms(),
                skin: avatar.skin as u32,
                _pad: [0; 3],
            })
            .collect();
        if size_of_val(instances.as_slice()) as wgpu::BufferAddress > self.instance_buffer.size() {
            self.instance_buffer = create_instance_buffer(device, avatars.len());
            self.bind_group = create_bind_group(
                device,
                &self.layout,
                &self.instance_buffer,
                &self.skins,
                &self.sampler,
            );
        }
        queue.write_buffer(&self.instance_buffer, 0, bytemuck::cast_slice(&instances));
        self.instance_count = avatars.len() as u32;
    }

    /// Draws everything given to the last `prepare`, with the frame uniforms bound at group 0.
    pub fn draw(&self, pass: &mut wgpu::RenderPass, frame: &wgpu::BindGroup) {
        if self.instance_count == 0 {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, frame, &[]);
        pass.set_bind_group(1, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint16);
        pass.draw_indexed(0..self.index_count, 0, 0..self.instance_count);
    }
}

fn create_instance_buffer(device: &wgpu::Device, avatars: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("avatar instances"),
        size: (avatars.max(1) * size_of::<Instance>()) as wgpu::BufferAddress,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn create_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    parts: &wgpu::Buffer,
    skins: &wgpu::TextureView,
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
                resource: wgpu::BindingResource::TextureView(skins),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    })
}

/// Every skin as one layer of a texture array, indexed by `Skin`.
fn load_skins(device: &wgpu::Device, queue: &wgpu::Queue) -> wgpu::TextureView {
    let mut texels = Vec::new();
    for skin in Skin::ALL {
        let mut image =
            image::load_from_memory_with_format(skin_png(skin), image::ImageFormat::Png)
                .unwrap_or_else(|err| panic!("decode {skin:?} skin: {err}"))
                .to_rgba8();
        assert!(
            image.width() == image.height() && SKIN_SIZE.is_multiple_of(image.width()),
            "{skin:?} skin must be square and divide {SKIN_SIZE}"
        );
        if image.width() != SKIN_SIZE {
            image = imageops::resize(&image, SKIN_SIZE, SKIN_SIZE, FilterType::Nearest);
        }
        texels.extend_from_slice(&image);
    }
    device
        .create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("skins"),
                size: wgpu::Extent3d {
                    width: SKIN_SIZE,
                    height: SKIN_SIZE,
                    depth_or_array_layers: Skin::ALL.len() as u32,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &texels,
        )
        .create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        })
}

fn skin_png(skin: Skin) -> &'static [u8] {
    match skin {
        Skin::Player => include_bytes!("../../textures/entity/skins/player.png"),
        Skin::Autumn => include_bytes!("../../textures/entity/skins/autumn.png"),
        Skin::CyberA => include_bytes!("../../textures/entity/skins/cyber_a.png"),
        Skin::CyberB => include_bytes!("../../textures/entity/skins/cyber_b.png"),
    }
}
