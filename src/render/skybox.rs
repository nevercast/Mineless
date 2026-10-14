use image::ImageFormat;
use wgpu::util::DeviceExt;

/// The daytime sky panorama, drawn behind everything and tinted by the time of day.
pub struct Skybox {
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
}

impl Skybox {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, frame_layout: &wgpu::BindGroupLayout) -> Self {
        let view = load_panorama(device, queue);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("skybox"),
            // Longitude wraps round; latitude stops at the poles.
            address_mode_u: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("skybox"),
            entries: &[super::texture_entry(0, wgpu::TextureViewDimension::D2), super::sampler_entry(1)],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("skybox"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });

        let shader = super::frame_shader(device, "skybox", include_str!("skybox.wgsl"));
        let pipeline = super::scene_pipeline(
            device,
            "skybox",
            &shader,
            &[Some(frame_layout), Some(&layout)],
            &[],
            super::Layer::Background,
            "fs_main",
        );
        Self { pipeline, bind_group }
    }

    pub fn draw(&self, pass: &mut wgpu::RenderPass, frame: &wgpu::BindGroup) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, frame, &[]);
        pass.set_bind_group(1, &self.bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
}

/// Uploads the equirectangular panorama: longitude across, latitude down.
fn load_panorama(device: &wgpu::Device, queue: &wgpu::Queue) -> wgpu::TextureView {
    let panorama = image::load_from_memory_with_format(include_bytes!("../../textures/sky/day.png"), ImageFormat::Png)
        .expect("the skybox is a PNG embedded at build time")
        .to_rgba8();
    device
        .create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("skybox"),
                size: wgpu::Extent3d {
                    width: panorama.width(),
                    height: panorama.height(),
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
            &panorama,
        )
        .create_view(&wgpu::TextureViewDescriptor::default())
}
