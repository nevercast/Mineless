use image::RgbaImage;
use image::imageops::{self, FilterType};

use crate::mesher::Tile;

const TILE_SIZE: u32 = 32;

/// Fixed plains-biome tint for the greyscale grass top until biomes exist.
const GRASS_TINT: [u8; 3] = [0x91, 0xBD, 0x59];
/// Fixed tint for the greyscale leaves, a darker green than the grass so trees stand out on it.
const FOLIAGE_TINT: [u8; 3] = [0x6A, 0xA8, 0x3C];
/// Fills the holes in the leaf texture, so leaf cubes read as dense foliage until leaves are cut
/// out properly.
const FOLIAGE_SHADOW: [u8; 3] = [0x1E, 0x3A, 0x12];
/// Fixed tint and opacity for the greyscale water.
const WATER_TINT: [u8; 3] = [0x3F, 0x76, 0xE4];
const WATER_ALPHA: u8 = 0xB4;

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
        Tile::Sand => include_bytes!("../../textures/block/sand.png"),
        Tile::Gravel => include_bytes!("../../textures/block/gravel.png"),
        Tile::Snow => include_bytes!("../../textures/block/snow.png"),
        Tile::OakLog => include_bytes!("../../textures/block/oak_log.png"),
        Tile::OakLogTop => include_bytes!("../../textures/block/oak_log_top.png"),
        Tile::OakLeaves => include_bytes!("../../textures/block/oak_leaves.png"),
        Tile::Water => include_bytes!("../../textures/block/water_still.png"),
    };
    let mut image = image::load_from_memory_with_format(png, image::ImageFormat::Png)
        .unwrap_or_else(|err| panic!("decode {tile:?} texture: {err}"))
        .to_rgba8();
    if tile == Tile::Water {
        // The file is a strip of animation frames; still water uses the first.
        image = imageops::crop_imm(&image, 0, 0, TILE_SIZE, TILE_SIZE).to_image();
    }
    assert_eq!(image.dimensions(), (TILE_SIZE, TILE_SIZE), "{tile:?} tile size");
    match tile {
        Tile::GrassTop => tint(&mut image, GRASS_TINT),
        Tile::OakLeaves => {
            tint(&mut image, FOLIAGE_TINT);
            for pixel in image.pixels_mut() {
                let [r, g, b, alpha] = pixel.0;
                let blend = |leaf: u8, shadow: u8| {
                    let alpha = u16::from(alpha);
                    ((u16::from(leaf) * alpha + u16::from(shadow) * (255 - alpha)) / 255) as u8
                };
                let [sr, sg, sb] = FOLIAGE_SHADOW;
                pixel.0 = [blend(r, sr), blend(g, sg), blend(b, sb), 255];
            }
        }
        Tile::Water => {
            tint(&mut image, WATER_TINT);
            for pixel in image.pixels_mut() {
                pixel.0[3] = WATER_ALPHA;
            }
        }
        _ => {}
    }
    image
}

fn tint(image: &mut RgbaImage, tint: [u8; 3]) {
    for pixel in image.pixels_mut() {
        for (channel, tint) in pixel.0.iter_mut().zip(tint) {
            *channel = (u16::from(*channel) * u16::from(tint) / 255) as u8;
        }
    }
}

fn write_layer(queue: &wgpu::Queue, texture: &wgpu::Texture, image: &RgbaImage, layer: u32, mip_level: u32) {
    let (width, height) = image.dimensions();
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level,
            origin: wgpu::Origin3d { x: 0, y: 0, z: layer },
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
