use bytemuck::{Pod, Zeroable};
use glam::Mat4;
use wgpu::util::DeviceExt;

use sim::avatar::Avatar;

use super::skins::SkinTextures;
use crate::avatar::{self, AvatarVertex, PART_COUNT};

const VERTEX_ATTRIBUTES: [wgpu::VertexAttribute; 4] =
    wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Float32x3, 3 => Uint32];
/// Matches `Instance` in avatar.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Instance {
    parts: [Mat4; PART_COUNT],
    skin: u32,
    seed: f32,
    _pad: [u32; 2],
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
    skins: SkinTextures,
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
        let skins = SkinTextures::load(device, queue);
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("avatar instances"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let instance_buffer = create_instance_buffer(device, 1);
        let bind_group = create_bind_group(device, &layout, &instance_buffer);

        let shader = super::frame_shader(
            device,
            "avatar",
            &[include_str!("skin_glow.wgsl"), include_str!("avatar.wgsl")].concat(),
        );
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("avatar"),
            bind_group_layouts: &[Some(frame_layout), Some(&layout), Some(&skins.layout)],
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
            instance_buffer,
            bind_group,
            instance_count: 0,
        }
    }

    /// Uploads this frame's poses. Must run before the render pass that draws them.
    pub fn prepare(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, avatars: &[Avatar]) {
        let instances: Vec<Instance> = avatars
            .iter()
            .enumerate()
            .map(|(index, avatar)| Instance {
                parts: avatar::part_transforms(avatar),
                skin: avatar.skin as u32,
                // Any spread of values keeps avatars sharing a skin from glowing in sync.
                seed: index as f32 * 137.0,
                _pad: [0; 2],
            })
            .collect();
        if size_of_val(instances.as_slice()) as wgpu::BufferAddress > self.instance_buffer.size() {
            self.instance_buffer = create_instance_buffer(device, avatars.len());
            self.bind_group = create_bind_group(device, &self.layout, &self.instance_buffer);
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
        pass.set_bind_group(2, &self.skins.bind_group, &[]);
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
    instances: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("avatar instances"),
        layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: instances.as_entire_binding(),
        }],
    })
}
