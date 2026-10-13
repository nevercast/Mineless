//! A hand-shaped test arena: rolling ground with a few hills and dug pits inside a cobblestone wall.
//! The hills and pits repeat in tiles, so an arena of any size has the same terrain throughout.

use glam::{IVec3, Vec2};

use super::{Block, CHUNK_SIZE, World};

/// A round feature on the ground: positive `height` raises a hill, negative digs a pit.
struct Mound {
    centre: Vec2,
    radius: f32,
    height: f32,
}

const fn mound(x: f32, z: f32, radius: f32, height: f32) -> Mound {
    Mound {
        centre: Vec2::new(x, z),
        radius,
        height,
    }
}

const HILLS: [Mound; 3] = [
    mound(16.0, 18.0, 9.0, 6.0),
    mound(45.0, 42.0, 11.0, 9.0),
    mound(20.0, 47.0, 6.0, 4.0),
];

const PITS: [Mound; 2] = [mound(44.0, 17.0, 5.0, -6.0), mound(31.0, 31.0, 3.5, -3.0)];

/// Blocks across one tile of hills and pits. Every mound lies wholly inside it, so tiles meet
/// seamlessly on flat ground.
pub const TILE: i32 = 64;

/// Grass on top of this many dirt blocks, then stone below.
const DIRT_DEPTH: i32 = 3;

/// Builds the arena across `chunks` from the origin. The natural surface sits at `ground_height`
/// plus the hills; pits are then dug out of it, exposing the dirt and stone underneath.
pub fn generate(chunks: IVec3, ground_height: i32, wall_height: i32) -> World {
    let mut world = World::empty(chunks);
    let extent = chunks * CHUNK_SIZE as i32;
    for x in 0..extent.x {
        for z in 0..extent.z {
            let on_edge = x == 0 || z == 0 || x == extent.x - 1 || z == extent.z - 1;
            if on_edge {
                for y in 0..wall_height.min(extent.y) {
                    world.set_block(IVec3::new(x, y, z), Block::Cobblestone);
                }
                continue;
            }

            let column = Vec2::new(x as f32 + 0.5, z as f32 + 0.5);
            let natural = ground_height + raise(&HILLS, column);
            let dug = natural + raise(&PITS, column);
            for y in 0..dug.min(extent.y) {
                let depth = natural - 1 - y;
                let block = match depth {
                    0 => Block::Grass,
                    1..=DIRT_DEPTH => Block::Dirt,
                    _ => Block::Stone,
                };
                world.set_block(IVec3::new(x, y, z), block);
            }
        }
    }
    world
}

/// Total height change at a column from a set of mounds, each a smooth cosine bump.
fn raise(mounds: &[Mound], column: Vec2) -> i32 {
    let column = column.rem_euclid(Vec2::splat(TILE as f32));
    mounds
        .iter()
        .map(|mound| {
            let t = (column.distance(mound.centre) / mound.radius).min(1.0);
            mound.height * 0.5 * (1.0 + (t * std::f32::consts::PI).cos())
        })
        .sum::<f32>()
        .round() as i32
}
