use image::ImageFormat;
use image::imageops::FilterType;
use wgpu::util::DeviceExt;

use crate::avatar::Skin;

/// Every skin is scaled up to this size so they all fit one texture array. 128 is the 64x64
/// Minecraft layout at twice the detail.
const SKIN_SIZE: u32 = 128;

/// All skins as layers of three texture arrays indexed by `Skin`: the albedo, how strongly each
/// texel glows, and the light the glowing texels spill onto the rest of their body part. Skins
/// that don't glow get black glow and spill layers.
pub struct SkinTextures {
    pub layout: wgpu::BindGroupLayout,
    pub bind_group: wgpu::BindGroup,
}

struct SkinFiles {
    albedo: &'static [u8],
    glow: Option<(&'static [u8], &'static [u8])>,
}

fn skin_files(skin: Skin) -> SkinFiles {
    macro_rules! skin {
        ($name:literal) => {
            include_bytes!(concat!("../../textures/entity/skins/", $name, ".png"))
        };
    }
    match skin {
        Skin::Player => SkinFiles {
            albedo: skin!("player"),
            glow: None,
        },
        Skin::Autumn => SkinFiles {
            albedo: skin!("autumn"),
            glow: None,
        },
        Skin::CyberA => SkinFiles {
            albedo: skin!("cyber_a"),
            glow: Some((skin!("cyber_a_e"), skin!("cyber_a_spill"))),
        },
        Skin::CyberB => SkinFiles {
            albedo: skin!("cyber_b"),
            glow: Some((skin!("cyber_b_e"), skin!("cyber_b_spill"))),
        },
    }
}

impl SkinTextures {
    pub fn load(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let mut albedo = Vec::new();
        let mut emissive = Vec::new();
        let mut spill = Vec::new();
        let layer_texels = (SKIN_SIZE * SKIN_SIZE) as usize;
        for skin in Skin::ALL {
            let files = skin_files(skin);
            albedo.extend_from_slice(&decode(skin, files.albedo).to_rgba8());
            if let Some((emissive_png, spill_png)) = files.glow {
                emissive.extend_from_slice(&decode(skin, emissive_png).to_luma8());
                spill.extend_from_slice(&decode(skin, spill_png).to_rgba8());
            } else {
                emissive.resize(emissive.len() + layer_texels, 0);
                spill.resize(spill.len() + 4 * layer_texels, 0);
            }
        }
        // The albedo is colour, so it's sRGB; glow and spill are data, so they stay linear.
        let views = [
            ("skin albedo", wgpu::TextureFormat::Rgba8UnormSrgb, albedo),
            ("skin emissive", wgpu::TextureFormat::R8Unorm, emissive),
            ("skin spill", wgpu::TextureFormat::Rgba8Unorm, spill),
        ]
        .map(|(label, format, texels)| create_array(device, queue, label, format, &texels));

        // Nearest filtering keeps the skins' texels crisp; avatars are small enough on screen that
        // they don't need mips yet.
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor::default());
        let texture_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2Array,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("skins"),
            entries: &[
                texture_entry(0),
                texture_entry(1),
                texture_entry(2),
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("skins"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&views[0]),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&views[1]),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&views[2]),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        Self { layout, bind_group }
    }
}

/// Decodes one skin image at `SKIN_SIZE`, scaling smaller skins up texel for texel.
fn decode(skin: Skin, png: &[u8]) -> image::DynamicImage {
    let image = image::load_from_memory_with_format(png, ImageFormat::Png)
        .unwrap_or_else(|err| panic!("decode {skin:?} skin: {err}"));
    assert!(
        image.width() == image.height() && SKIN_SIZE.is_multiple_of(image.width()),
        "{skin:?} skin must be square and divide {SKIN_SIZE}"
    );
    image.resize_exact(SKIN_SIZE, SKIN_SIZE, FilterType::Nearest)
}

fn create_array(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &str,
    format: wgpu::TextureFormat,
    texels: &[u8],
) -> wgpu::TextureView {
    device
        .create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: SKIN_SIZE,
                    height: SKIN_SIZE,
                    depth_or_array_layers: Skin::ALL.len() as u32,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            texels,
        )
        .create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        })
}
