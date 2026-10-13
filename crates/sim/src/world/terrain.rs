//! Seeded terrain: rolling grassland broken up by rocky mountain ranges, built from layered value
//! noise, inside a cobblestone wall that keeps walkers from falling off the edge of the world.

use glam::{IVec3, Vec2};

use super::{Block, CHUNK_SIZE, World};

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
    let mut world = World::empty(chunks);
    let extent = chunks * CHUNK_SIZE as i32;
    for x in 0..extent.x {
        for z in 0..extent.z {
            let on_edge = x == 0 || z == 0 || x == extent.x - 1 || z == extent.z - 1;
            if on_edge {
                // The highest ground the wall borders, so it is never shorter than a jump.
                let inside = IVec3::new(x, 0, z).clamp(IVec3::ONE, extent - 2);
                let ground = (-1..=1)
                    .flat_map(|dx| (-1..=1).map(move |dz| (dx, dz)))
                    .map(|(dx, dz)| surface_height(seed, inside.x + dx, inside.z + dz))
                    .max()
                    .expect("the neighbourhood is never empty");
                for y in 0..(ground + WALL_ABOVE_GROUND).min(extent.y) {
                    world.set_block(IVec3::new(x, y, z), Block::Cobblestone);
                }
                continue;
            }

            let surface = surface_height(seed, x, z).min(extent.y);
            for y in 0..surface {
                let depth = surface - 1 - y;
                let block = match depth {
                    0 if surface < ROCK_LINE => Block::Grass,
                    1..=DIRT_DEPTH if surface < ROCK_LINE => Block::Dirt,
                    _ => Block::Stone,
                };
                world.set_block(IVec3::new(x, y, z), block);
            }
        }
    }
    world
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

    #[test]
    fn lattice_values_stay_in_unit_range() {
        for x in -50..50 {
            for z in -50..50 {
                assert!((0.0..=1.0).contains(&lattice(3, x, z)));
            }
        }
    }
}
