//! Seeded terrain: rolling grassland broken up by rocky mountain ranges, built from layered value
//! noise, inside a cobblestone wall that keeps walkers from falling off the edge of the world.

use glam::{IVec3, Vec2};

use super::chunk::Chunk;
use super::{Block, CHUNK_SIZE, SIZE, World};

/// Lowest the land gets, so there is always stone under the deepest valley.
const BASE_HEIGHT: f32 = 18.0;
/// How far the gentle hills rise above the base everywhere.
const HILL_HEIGHT: f32 = 14.0;
/// How far mountains rise above the hills where the ranges are.
const MOUNTAIN_HEIGHT: f32 = 58.0;

/// Blocks across one cell of each noise layer. The range layer is broad so mountains come in
/// ranges with plains between them rather than as scattered peaks.
const RANGE_SCALE: f32 = 150.0;
const HILL_SCALE: f32 = 56.0;
const MOUNTAIN_SCALE: f32 = 40.0;

/// Surfaces at or above this are bare rock, so peaks read as mountains rather than grassy lumps.
const ROCK_LINE: i32 = 64;
/// Grass on top of this many dirt blocks, then stone below.
const DIRT_DEPTH: i32 = 3;
/// How far the edge wall stands above the ground just inside it; walkers jump at most 1.25.
const WALL_ABOVE_GROUND: i32 = 3;

/// Fills `chunks` from the origin with the terrain for `seed`. The same seed always gives the
/// same world.
pub fn generate(chunks: IVec3, seed: u32) -> World {
    let mut world = World::default();
    for cx in 0..chunks.x {
        for cy in 0..chunks.y {
            for cz in 0..chunks.z {
                let position = IVec3::new(cx, cy, cz);
                world.insert(position, generate_chunk(seed, position, chunks));
            }
        }
    }
    world
}

/// The chunk at `position` in a world `chunks` across, from nothing but the seed and those two,
/// so chunks come out the same whatever order they are generated in.
fn generate_chunk(seed: u32, position: IVec3, chunks: IVec3) -> Chunk {
    let mut chunk = Chunk::empty();
    let extent = chunks * SIZE;
    let origin = position * SIZE;
    for local_x in 0..CHUNK_SIZE {
        for local_z in 0..CHUNK_SIZE {
            let (x, z) = (origin.x + local_x as i32, origin.z + local_z as i32);
            let column = Column::at(seed, x, z, extent);
            for local_y in 0..CHUNK_SIZE {
                let block = column.block(origin.y + local_y as i32);
                if block != Block::Air {
                    chunk.set(local_x, local_y, local_z, block);
                }
            }
        }
    }
    chunk
}

/// One column of the world: part of the edge wall, or ground.
enum Column {
    Wall { top: i32 },
    Ground { surface: i32 },
}

impl Column {
    /// The column at `x`, `z` in a world `extent` blocks across.
    fn at(seed: u32, x: i32, z: i32, extent: IVec3) -> Self {
        let on_edge = x == 0 || z == 0 || x == extent.x - 1 || z == extent.z - 1;
        if on_edge {
            // The highest ground the wall borders, so it is never shorter than a jump.
            let inside = IVec3::new(x, 0, z).clamp(IVec3::ONE, extent - 2);
            let ground = (-1..=1)
                .flat_map(|dx| (-1..=1).map(move |dz| (dx, dz)))
                .map(|(dx, dz)| surface_height(seed, inside.x + dx, inside.z + dz))
                .max()
                .expect("the neighbourhood is never empty");
            Column::Wall {
                top: ground + WALL_ABOVE_GROUND,
            }
        } else {
            Column::Ground {
                surface: surface_height(seed, x, z),
            }
        }
    }

    fn block(&self, y: i32) -> Block {
        match *self {
            Column::Wall { top } if y < top => Block::Cobblestone,
            Column::Ground { surface } if y < surface => match surface - 1 - y {
                0 if surface < ROCK_LINE => Block::Grass,
                1..=DIRT_DEPTH if surface < ROCK_LINE => Block::Dirt,
                _ => Block::Stone,
            },
            _ => Block::Air,
        }
    }
}

/// How many blocks of ground the column at `x`, `z` has.
pub fn surface_height(seed: u32, x: i32, z: i32) -> i32 {
    let column = Vec2::new(x as f32, z as f32);
    let hills = fractal(seed, column / HILL_SCALE);
    // Ranges are where the broad layer is high; the smoothstep keeps the plains between them flat.
    let range = smoothstep(0.45, 0.7, fractal(seed ^ 0x5bd1_e995, column / RANGE_SCALE));
    // Folding the noise about its middle turns its rounded tops into sharp ridgelines.
    let ridges = 1.0 - (2.0 * fractal(seed ^ 0x9e37_79b9, column / MOUNTAIN_SCALE) - 1.0).abs();
    let height = BASE_HEIGHT + HILL_HEIGHT * hills + MOUNTAIN_HEIGHT * range * ridges * ridges;
    height.round() as i32
}

/// Four octaves of value noise, each twice as fine and half as strong as the last, scaled back
/// into `0..=1`.
fn fractal(seed: u32, point: Vec2) -> f32 {
    let mut total = 0.0;
    let mut strength = 1.0;
    let mut strengths = 0.0;
    let mut point = point;
    for octave in 0..4 {
        total += strength * value_noise(seed.wrapping_add(octave), point);
        strengths += strength;
        strength *= 0.5;
        point *= 2.0;
    }
    total / strengths
}

/// Smoothly interpolated random values on the integer lattice, in `0..=1`.
fn value_noise(seed: u32, point: Vec2) -> f32 {
    let cell = point.floor();
    let (x, z) = (cell.x as i32, cell.y as i32);
    let t = point - cell;
    let t = t * t * (3.0 - 2.0 * t);
    let near = lerp(lattice(seed, x, z), lattice(seed, x + 1, z), t.x);
    let far = lerp(lattice(seed, x, z + 1), lattice(seed, x + 1, z + 1), t.x);
    lerp(near, far, t.y)
}

/// A random value in `0..=1` for one lattice point, from an integer hash so every platform agrees.
fn lattice(seed: u32, x: i32, z: i32) -> f32 {
    let mut hash = seed
        ^ (x as u32).wrapping_mul(0x27d4_eb2d)
        ^ (z as u32).wrapping_mul(0x1656_67b1).rotate_left(16);
    hash ^= hash >> 15;
    hash = hash.wrapping_mul(0x2c1b_3c6d);
    hash ^= hash >> 12;
    hash = hash.wrapping_mul(0x297a_2d39);
    hash ^= hash >> 15;
    // The top 24 bits fit an f32 exactly.
    (hash >> 8) as f32 / ((1 << 24) - 1) as f32
}

fn lerp(from: f32, to: f32, t: f32) -> f32 {
    from + (to - from) * t
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn heights(seed: u32) -> Vec<i32> {
        (0..320)
            .flat_map(|x| (0..320).map(move |z| surface_height(seed, x, z)))
            .collect()
    }

    #[test]
    fn same_seed_same_terrain_and_other_seeds_differ() {
        assert_eq!(heights(7), heights(7));
        assert_ne!(heights(7), heights(8));
    }

    #[test]
    fn terrain_has_plains_and_mountains_within_its_limits() {
        for seed in 0..4 {
            let heights = heights(seed);
            let lowest = *heights.iter().min().expect("heights is not empty");
            let highest = *heights.iter().max().expect("heights is not empty");
            assert!(lowest >= BASE_HEIGHT as i32, "seed {seed}: {lowest}");
            let ceiling = (BASE_HEIGHT + HILL_HEIGHT + MOUNTAIN_HEIGHT) as i32;
            assert!(highest <= ceiling, "seed {seed}: {highest}");
            assert!(highest - lowest > 40, "seed {seed}: {lowest}..{highest}");
        }
    }

    const CHUNKS: IVec3 = IVec3::new(3, 6, 3);

    fn chunk_positions() -> Vec<IVec3> {
        (0..CHUNKS.x)
            .flat_map(|x| (0..CHUNKS.y).flat_map(move |y| (0..CHUNKS.z).map(move |z| (x, y, z))))
            .map(IVec3::from)
            .collect()
    }

    #[test]
    fn generation_order_does_not_change_the_world() {
        let in_order = generate(CHUNKS, 5);
        let mut reversed = World::default();
        for position in chunk_positions().into_iter().rev() {
            reversed.insert(position, generate_chunk(5, position, CHUNKS));
        }
        for position in chunk_positions() {
            assert!(
                in_order.chunks[&position] == reversed.chunks[&position],
                "{position}"
            );
        }
    }

    #[test]
    fn a_chunk_generated_alone_matches_the_whole_world() {
        let world = generate(CHUNKS, 5);
        let wall = IVec3::new(0, 1, 2);
        let interior = IVec3::new(1, 1, 1);
        for position in [wall, interior] {
            assert!(
                generate_chunk(5, position, CHUNKS) == world.chunks[&position],
                "{position}"
            );
        }
        assert_eq!(world.block(IVec3::new(0, 20, 40)), Block::Cobblestone);
    }

    #[test]
    fn column_top_is_the_surface_height_inside_the_walls() {
        let world = generate(CHUNKS, 5);
        for x in (1..47).step_by(3) {
            for z in (1..47).step_by(5) {
                assert_eq!(world.column_top(x, z), surface_height(5, x, z), "{x}, {z}");
            }
        }
    }

    #[test]
    fn lattice_values_stay_in_unit_range() {
        for x in -50..50 {
            for z in -50..50 {
                assert!((0.0..=1.0).contains(&lattice(3, x, z)));
            }
        }
    }
}
