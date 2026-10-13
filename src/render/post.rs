//! The scene renders to HDR targets; this blooms the emissive one and tonemaps both onto the
//! swapchain.

pub const HDR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// Halvings of the bloom chain below the half-resolution first level. More levels spread glow
/// wider.
const BLOOM_LEVELS: u32 = 6;

pub struct PostProcess {
    blur_layout: wgpu::BindGroupLayout,
    composite_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    downsample: wgpu::RenderPipeline,
    upsample: wgpu::RenderPipeline,
    composite: wgpu::RenderPipeline,
    targets: Targets,
}

/// Everything sized to the window, rebuilt on resize.
struct Targets {
    scene: wgpu::TextureView,
    emissive: wgpu::TextureView,
    /// One view per level of the bloom chain, finest first.
    bloom: Vec<wgpu::TextureView>,
    /// Reads the emissive target, then each level, to fill the next coarser one.
    downsample: Vec<wgpu::BindGroup>,
    /// Reads each level to add onto the next finer one; `upsample[i]` reads level `i + 1`.
    upsample: Vec<wgpu::BindGroup>,
    composite: wgpu::BindGroup,
}

impl PostProcess {
    pub fn new(
        device: &wgpu::Device,
        output_format: wgpu::TextureFormat,
        size: (u32, u32),
    ) -> Self {
        let texture_entry = |binding| super::texture_entry(binding, wgpu::TextureViewDimension::D2);
        let sampler_entry = super::sampler_entry(1);
        let blur_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bloom"),
            entries: &[texture_entry(0), sampler_entry],
        });
        let composite_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("composite"),
            entries: &[texture_entry(0), sampler_entry, texture_entry(2)],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("post"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let shader = device.create_shader_module(wgpu::include_wgsl!("post.wgsl"));
        let pipeline = |entry_point, layout, format, blend| {
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(entry_point),
                bind_group_layouts: &[Some(layout)],
                immediate_size: 0,
            });
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry_point),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_fullscreen"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &[],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry_point),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let additive = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Add,
        };
        let downsample = pipeline("fs_downsample", &blur_layout, HDR_FORMAT, None);
        let upsample = pipeline(
            "fs_upsample",
            &blur_layout,
            HDR_FORMAT,
            Some(wgpu::BlendState {
                color: additive,
                alpha: additive,
            }),
        );
        let composite = pipeline("fs_composite", &composite_layout, output_format, None);

        let targets = Targets::new(device, &blur_layout, &composite_layout, &sampler, size);
        Self {
            blur_layout,
            composite_layout,
            sampler,
            downsample,
            upsample,
            composite,
            targets,
        }
    }

    pub fn resize(&mut self, device: &wgpu::Device, size: (u32, u32)) {
        self.targets = Targets::new(
            device,
            &self.blur_layout,
            &self.composite_layout,
            &self.sampler,
            size,
        );
    }

    /// The HDR colour target the scene draws into.
    pub fn scene(&self) -> &wgpu::TextureView {
        &self.targets.scene
    }

    /// The HDR target the scene writes self-lit colour into, for bloom.
    pub fn emissive(&self) -> &wgpu::TextureView {
        &self.targets.emissive
    }

    pub fn draw(&self, encoder: &mut wgpu::CommandEncoder, output: &wgpu::TextureView) {
        let targets = &self.targets;
        for (level, bind_group) in targets.bloom.iter().zip(&targets.downsample) {
            fullscreen(encoder, level, CLEAR, &self.downsample, bind_group);
        }
        for (level, bind_group) in targets.bloom.iter().zip(&targets.upsample).rev() {
            fullscreen(
                encoder,
                level,
                wgpu::LoadOp::Load,
                &self.upsample,
                bind_group,
            );
        }
        fullscreen(encoder, output, CLEAR, &self.composite, &targets.composite);
    }
}

impl Targets {
    fn new(
        device: &wgpu::Device,
        blur_layout: &wgpu::BindGroupLayout,
        composite_layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
        (width, height): (u32, u32),
    ) -> Self {
        let render_target = |label, width, height, mip_level_count| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: HDR_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
        };
        let scene = render_target("scene", width, height, 1)
            .create_view(&wgpu::TextureViewDescriptor::default());
        let emissive = render_target("emissive", width, height, 1)
            .create_view(&wgpu::TextureViewDescriptor::default());

        let (bloom_width, bloom_height) = ((width / 2).max(1), (height / 2).max(1));
        // A level can't be smaller than one texel.
        let levels = (BLOOM_LEVELS + 1).min(bloom_width.min(bloom_height).ilog2() + 1);
        let bloom_texture = render_target("bloom", bloom_width, bloom_height, levels);
        let bloom: Vec<_> = (0..levels)
            .map(|level| {
                bloom_texture.create_view(&wgpu::TextureViewDescriptor {
                    base_mip_level: level,
                    mip_level_count: Some(1),
                    ..Default::default()
                })
            })
            .collect();

        let bind_group = |layout, entries: &[wgpu::BindGroupEntry]| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout,
                entries,
            })
        };
        let blur = |source| {
            bind_group(
                blur_layout,
                &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(source),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(sampler),
                    },
                ],
            )
        };
        let downsample = std::iter::once(&emissive)
            .chain(&bloom[..bloom.len() - 1])
            .map(blur)
            .collect();
        let upsample = bloom[1..].iter().map(blur).collect();
        let composite = bind_group(
            composite_layout,
            &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&scene),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&bloom[0]),
                },
            ],
        );
        Self {
            scene,
            emissive,
            bloom,
            downsample,
            upsample,
            composite,
        }
    }
}

const CLEAR: wgpu::LoadOp<wgpu::Color> = wgpu::LoadOp::Clear(wgpu::Color::BLACK);

fn fullscreen(
    encoder: &mut wgpu::CommandEncoder,
    target: &wgpu::TextureView,
    load: wgpu::LoadOp<wgpu::Color>,
    pipeline: &wgpu::RenderPipeline,
    bind_group: &wgpu::BindGroup,
) {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: None,
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: target,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load,
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, bind_group, &[]);
    pass.draw(0..3, 0..1);
}
