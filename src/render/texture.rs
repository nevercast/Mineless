use image::RgbaImage;
use image::imageops::{self, FilterType};

use crate::mesher::Tile;

const TILE_SIZE: u32 = 32;

/// Fixed plains-biome tint for the greyscale grass top until biomes exist.
const GRASS_TINT: [u8; 3] = [0x91, 0xBD, 0x59];

/// Every block tile as one layer of a single texture array, with a full mip chain so distant
/// blocks don't shimmer. An array rather than an atlas keeps mips from bleeding between tiles.
pub struct BlockTextures {
    pub layout: wgpu::BindGroupLayout,
    pub bind_group: wgpu::BindGroup,
}

impl BlockTextures {
    pub fn load(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let mip_level_count = TILE_SIZE.ilog2() + 1;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("block tiles"),
            size: wgpu::Extent3d {
                width: TILE_SIZE,
                height: TILE_SIZE,
                depth_or_array_layers: Tile::ALL.len() as u32,
            },
            mip_level_count,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        for tile in Tile::ALL {
            let mut level = tile_image(tile);
            for mip_level in 0..mip_level_count {
                write_layer(queue, &texture, &level, tile as u32, mip_level);
                let half = (level.width() / 2).max(1);
                level = imageops::resize(&level, half, half, FilterType::Triangle);
            }
        }

        // Trilinear minification blends between mips; magnification stays nearest so close
        // blocks keep their pixel-art edges.
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("block tiles"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });

        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });

        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("block tiles"),
            entries: &[
                super::texture_entry(0, wgpu::TextureViewDimension::D2Array),
                super::sampler_entry(1),
            ],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("block tiles"),
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

        Self { layout, bind_group }
    }
}

fn tile_image(tile: Tile) -> RgbaImage {
    let png: &[u8] = match tile {
        Tile::GrassTop => include_bytes!("../../textures/block/grass_block_top.png"),
        Tile::GrassSide => include_bytes!("../../textures/block/grass_block_side.png"),
        Tile::Dirt => include_bytes!("../../textures/block/dirt.png"),
        Tile::Stone => include_bytes!("../../textures/block/stone.png"),
        Tile::Cobblestone => include_bytes!("../../textures/block/cobblestone.png"),
    };
    let mut image = image::load_from_memory_with_format(png, image::ImageFormat::Png)
        .unwrap_or_else(|err| panic!("decode {tile:?} texture: {err}"))
        .to_rgba8();
    assert_eq!(
        image.dimensions(),
        (TILE_SIZE, TILE_SIZE),
        "{tile:?} tile size"
    );
    if tile == Tile::GrassTop {
        for pixel in image.pixels_mut() {
            for (channel, tint) in pixel.0.iter_mut().zip(GRASS_TINT) {
                *channel = (u16::from(*channel) * u16::from(tint) / 255) as u8;
            }
        }
    }
    image
}

fn write_layer(
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    image: &RgbaImage,
    layer: u32,
    mip_level: u32,
) {
    let (width, height) = image.dimensions();
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level,
            origin: wgpu::Origin3d {
                x: 0,
                y: 0,
                z: layer,
            },
            aspect: wgpu::TextureAspect::All,
        },
        image,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4 * width),
            rows_per_image: Some(height),
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
}
