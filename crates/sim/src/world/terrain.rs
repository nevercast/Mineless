//! Seeded terrain from a 3D density field: low plains a few blocks above the sea, broken by
//! upland regions that rise in escarpments with ledges, overhangs and now and then an arch or a
//! lump of floating land, all inside a cobblestone wall that keeps walkers from falling off the
//! edge of the world.

use glam::{IVec3, Vec2, Vec3};

use super::chunk::Chunk;
use super::{Block, CHUNK_SIZE, SIZE, World};

/// The world height the terrain is shaped for.
pub const HEIGHT: i32 = 128;
/// Air below this height that is open to the sea will be water; the sea surface is the top of
/// the row below it.
pub const SEA_LEVEL: i32 = 40;

/// Where the low ground sits relative to the sea, and how far it strays above and below.
const LOWLAND_ABOVE_SEA: f32 = 2.0;
const LOWLAND_SPREAD: f32 = 50.0;
/// How far uplands rise above the lowland they stand on.
const UPLAND_RISE: f32 = 50.0;
/// Blocks over which density turns from solid to air around the target height.
const FALLOFF: f32 = 8.0;
/// How strongly the 3D detail bends the ground in plains and in the steepest uplands.
const PLAINS_DETAIL: f32 = 1.0;
const UPLAND_DETAIL: f32 = 10.0;

/// Blocks across one cell of each noise field. Detail cells are half as tall as they are wide,
/// which makes shelves and overhangs rather than tall thin spikes.
const ELEVATION_SCALE: f32 = 256.0;
const RELIEF_SCALE: f32 = 200.0;
const DETAIL_SCALE: Vec3 = Vec3::new(48.0, 24.0, 48.0);
/// Relief noise is shaped by a smoothstep between these, so most of the world is flat and the
/// uplands rise over a few tens of blocks.
const RELIEF_START: f32 = 0.06;
const RELIEF_FULL: f32 = 0.18;

/// No gradient noise sample, or fractal sum of them, is further than this from 0.
const NOISE_BOUND: f32 = 1.1;

/// Rows at the bottom that are always solid, so nothing falls out of the world.
const BEDROCK_ROWS: i32 = 4;
/// Density falls by this much per block above `CEILING_START`, so peaks round off well below
/// the top of the world instead of being sliced flat by it.
const CEILING_START: i32 = 112;
const CEILING_FALL: f32 = 0.75;

/// Exposed tops at or above this are bare rock, so peaks read as mountains rather than grassy
/// lumps.
const ROCK_LINE: i32 = SEA_LEVEL + 40;
/// Grass on top of this many dirt blocks, then stone below.
const DIRT_DEPTH: i32 = 3;
/// How far the edge wall stands above the ground just inside it; walkers jump at most 1.25.
const WALL_ABOVE_GROUND: i32 = 3;

/// Density is sampled every `LATTICE_STEP` blocks and interpolated between: 3D noise for every
/// block would make generation far too slow. The lattice is anchored to world coordinates, so
/// chunks sharing a face share its samples and meet without seams.
const LATTICE_STEP: IVec3 = IVec3::new(4, 8, 4);
/// Lattice levels from the bottom of the world to the top, inclusive.
const LEVELS: usize = (HEIGHT / LATTICE_STEP.y) as usize + 1;

/// Seed salts so each noise field is independent of the others.
const ELEVATION_SALT: u32 = 0x5bd1_e995;
const RELIEF_SALT: u32 = 0x9e37_79b9;
const DETAIL_SALT: u32 = 0x85eb_ca6b;

/// Fills `chunks` from the origin with the terrain for `seed`. The same seed always gives the
/// same world.
pub fn generate(chunks: IVec3, seed: u32) -> World {
    assert_eq!(
        chunks.y * SIZE,
        HEIGHT,
        "terrain is shaped for its own height"
    );
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
    let lattice = Lattice::around(seed, origin);
    for local_x in 0..CHUNK_SIZE {
        for local_z in 0..CHUNK_SIZE {
            let (x, z) = (origin.x + local_x as i32, origin.z + local_z as i32);
            let on_edge = x == 0 || z == 0 || x == extent.x - 1 || z == extent.z - 1;
            let wall_top = on_edge.then(|| wall_top(&lattice, x, z, extent));
            let column = lattice.column(x, z);
            for local_y in 0..CHUNK_SIZE {
                let y = origin.y + local_y as i32;
                let block = match wall_top {
                    Some(top) if y < top => Block::Cobblestone,
                    Some(_) => Block::Air,
                    None => ground_block(&column, y),
                };
                if block != Block::Air {
                    chunk.set(local_x, local_y, local_z, block);
                }
            }
        }
    }
    chunk
}

/// The highest ground the wall borders, so it is never shorter than a jump.
fn wall_top(lattice: &Lattice, x: i32, z: i32, extent: IVec3) -> i32 {
    let inside = IVec3::new(x, 0, z).clamp(IVec3::ONE, extent - 2);
    let ground = (-1..=1)
        .flat_map(|dx| (-1..=1).map(move |dz| (dx, dz)))
        .map(|(dx, dz)| lattice.column(inside.x + dx, inside.z + dz).top())
        .max()
        .expect("the neighbourhood is never empty");
    ground + WALL_ABOVE_GROUND
}

/// Every solid cell with air above it is grass on a few blocks of dirt, so ledges and floating
/// land are grassed as well as the highest ground in the column.
fn ground_block(column: &Column, y: i32) -> Block {
    if !column.solid(y) {
        return Block::Air;
    }
    // How far below the exposed top of its run of solid cells this one is, if it is near one.
    let depth = (0..=DIRT_DEPTH).find(|&depth| !column.solid(y + depth + 1));
    match depth {
        Some(depth) if y + depth >= ROCK_LINE => Block::Stone,
        Some(0) => Block::Grass,
        Some(_) => Block::Dirt,
        None => Block::Stone,
    }
}

/// Density samples on the lattice points around one chunk, its border columns and the full
/// height of the world, so a chunk and its neighbouring columns can be shaped without reading
/// any other chunk.
struct Lattice {
    /// World position of the first sample.
    origin: IVec3,
    /// Samples along x and z.
    across: i32,
    samples: Vec<f32>,
}

impl Lattice {
    fn around(seed: u32, chunk_origin: IVec3) -> Self {
        // One lattice cell of border all round covers the columns next to the chunk.
        let origin = (chunk_origin - LATTICE_STEP).with_y(0);
        let across = SIZE / LATTICE_STEP.x + 3;
        let mut samples = Vec::with_capacity((across * across) as usize * LEVELS);
        for i in 0..across {
            for k in 0..across {
                let (x, z) = (origin.x + i * LATTICE_STEP.x, origin.z + k * LATTICE_STEP.z);
                let fields = Fields::at(seed, x, z);
                for level in 0..LEVELS as i32 {
                    let y = level * LATTICE_STEP.y;
                    samples.push(fields.density(seed, x, y, z));
                }
            }
        }
        Self {
            origin,
            across,
            samples,
        }
    }

    /// The density profile of one column, interpolated from the four lattice columns around it.
    fn column(&self, x: i32, z: i32) -> Column {
        let local = Vec2::new((x - self.origin.x) as f32, (z - self.origin.z) as f32)
            / Vec2::new(LATTICE_STEP.x as f32, LATTICE_STEP.z as f32);
        let cell = local.floor();
        let t = local - cell;
        let (i, k) = (cell.x as i32, cell.y as i32);
        debug_assert!(i >= 0 && k >= 0 && i + 1 < self.across && k + 1 < self.across);
        let start = |i: i32, k: i32| (i * self.across + k) as usize * LEVELS;
        let corners = [
            start(i, k),
            start(i + 1, k),
            start(i, k + 1),
            start(i + 1, k + 1),
        ];
        let mut levels = [0.0; LEVELS];
        for (level, value) in levels.iter_mut().enumerate() {
            let [a, b, c, d] = corners.map(|corner| self.samples[corner + level]);
            *value = lerp(lerp(a, b, t.x), lerp(c, d, t.x), t.y);
        }
        Column { levels }
    }
}

/// One column's density at each lattice level, linear in between.
struct Column {
    levels: [f32; LEVELS],
}

impl Column {
    fn density(&self, y: i32) -> f32 {
        let level = (y / LATTICE_STEP.y) as usize;
        let t = (y % LATTICE_STEP.y) as f32 / LATTICE_STEP.y as f32;
        lerp(self.levels[level], self.levels[level + 1], t)
    }

    fn solid(&self, y: i32) -> bool {
        match y {
            ..0 | HEIGHT.. => false,
            ..BEDROCK_ROWS => true,
            _ => self.density(y) > 0.0,
        }
    }

    /// The height just above the highest solid cell, where a player can stand.
    fn top(&self) -> i32 {
        (0..HEIGHT)
            .rev()
            .find(|&y| self.solid(y))
            .map_or(0, |y| y + 1)
    }
}

/// The 2D fields that shape one column.
struct Fields {
    /// Height the ground tends towards.
    target: f32,
    /// How strongly the 3D detail bends the ground away from the target.
    detail: f32,
}

impl Fields {
    fn at(seed: u32, x: i32, z: i32) -> Self {
        let column = Vec2::new(x as f32, z as f32);
        let elevation = fractal(4, |frequency, salt| {
            gradient_2d(
                seed ^ ELEVATION_SALT ^ salt,
                column * frequency / ELEVATION_SCALE,
            )
        });
        let relief = smoothstep(
            RELIEF_START,
            RELIEF_FULL,
            fractal(4, |frequency, salt| {
                gradient_2d(seed ^ RELIEF_SALT ^ salt, column * frequency / RELIEF_SCALE)
            }),
        );
        Self {
            target: SEA_LEVEL as f32
                + LOWLAND_ABOVE_SEA
                + LOWLAND_SPREAD * elevation
                + UPLAND_RISE * relief,
            detail: lerp(PLAINS_DETAIL, UPLAND_DETAIL, relief),
        }
    }

    /// Positive is solid. Above the target height the ground thins into air; the detail term
    /// can outweigh that, raising ledges and arches above it or hollowing pockets below it.
    /// Clamped to `-1..=1`: far from any surface only the sign matters, and clamping lets those
    /// samples, most of the world, skip the 3D noise entirely.
    fn density(&self, seed: u32, x: i32, y: i32, z: i32) -> f32 {
        let ceiling = (y - CEILING_START).max(0) as f32 * CEILING_FALL;
        let shape = (self.target - y as f32) / FALLOFF - ceiling;
        if shape.abs() - self.detail * NOISE_BOUND >= 1.0 {
            return shape.signum();
        }
        let point = Vec3::new(x as f32, y as f32, z as f32) / DETAIL_SCALE;
        let detail = fractal(5, |frequency, salt| {
            gradient_3d(seed ^ DETAIL_SALT ^ salt, point * frequency)
        });
        (shape + self.detail * detail).clamp(-1.0, 1.0)
    }
}

/// `octaves` layers of noise, each twice as fine and half as strong as the last, scaled back to
/// the range of one layer. `layer` gets the layer's frequency and a salt that makes the layers
/// independent.
fn fractal(octaves: u32, layer: impl Fn(f32, u32) -> f32) -> f32 {
    let mut total = 0.0;
    let mut strength = 1.0;
    let mut strengths = 0.0;
    let mut frequency = 1.0;
    for octave in 0..octaves {
        total += strength * layer(frequency, octave.wrapping_mul(0x68e3_1da4));
        strengths += strength;
        strength *= 0.5;
        frequency *= 2.0;
    }
    total / strengths
}

/// Gradient noise in about `-1..=1`: a hashed gradient at each lattice corner, dotted with the
/// offset to the point and blended with a smooth fade. Plain arithmetic only, so every platform
/// generates the same world.
fn gradient_2d(seed: u32, point: Vec2) -> f32 {
    let cell = point.floor();
    let (x, z) = (cell.x as i32, cell.y as i32);
    let t = point - cell;
    let corner = |dx: i32, dz: i32| {
        let offset = t - Vec2::new(dx as f32, dz as f32);
        let (gx, gz) = match hash(seed, x + dx, 0, z + dz) & 7 {
            0 => (1.0, 1.0),
            1 => (-1.0, 1.0),
            2 => (1.0, -1.0),
            3 => (-1.0, -1.0),
            4 => (1.0, 0.0),
            5 => (-1.0, 0.0),
            6 => (0.0, 1.0),
            _ => (0.0, -1.0),
        };
        gx * offset.x + gz * offset.y
    };
    let (fx, fz) = (fade(t.x), fade(t.y));
    lerp(
        lerp(corner(0, 0), corner(1, 0), fx),
        lerp(corner(0, 1), corner(1, 1), fx),
        fz,
    )
}

/// 3D gradient noise in about `-1..=1`, with gradients towards the twelve edges of a cube.
fn gradient_3d(seed: u32, point: Vec3) -> f32 {
    let cell = point.floor();
    let base = cell.as_ivec3();
    let t = point - cell;
    let corner = |d: IVec3| {
        let o = t - d.as_vec3();
        let p = base + d;
        match hash(seed, p.x, p.y, p.z) % 12 {
            0 => o.x + o.y,
            1 => -o.x + o.y,
            2 => o.x - o.y,
            3 => -o.x - o.y,
            4 => o.x + o.z,
            5 => -o.x + o.z,
            6 => o.x - o.z,
            7 => -o.x - o.z,
            8 => o.y + o.z,
            9 => -o.y + o.z,
            10 => o.y - o.z,
            _ => -o.y - o.z,
        }
    };
    let f = Vec3::new(fade(t.x), fade(t.y), fade(t.z));
    let plane = |z: i32| {
        lerp(
            lerp(
                corner(IVec3::new(0, 0, z)),
                corner(IVec3::new(1, 0, z)),
                f.x,
            ),
            lerp(
                corner(IVec3::new(0, 1, z)),
                corner(IVec3::new(1, 1, z)),
                f.x,
            ),
            f.y,
        )
    };
    lerp(plane(0), plane(1), f.z)
}

/// An integer hash of a lattice point, so every platform agrees.
fn hash(seed: u32, x: i32, y: i32, z: i32) -> u32 {
    let mut hash = seed
        ^ (x as u32).wrapping_mul(0x27d4_eb2d)
        ^ (y as u32).wrapping_mul(0x9e37_79b1).rotate_left(8)
        ^ (z as u32).wrapping_mul(0x1656_67b1).rotate_left(16);
    hash ^= hash >> 15;
    hash = hash.wrapping_mul(0x2c1b_3c6d);
    hash ^= hash >> 12;
    hash = hash.wrapping_mul(0x297a_2d39);
    hash ^= hash >> 15;
    hash
}

/// Eases lattice blending so the noise has no visible creases at cell edges.
fn fade(t: f32) -> f32 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
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

    const CHUNKS: IVec3 = IVec3::new(3, HEIGHT / SIZE, 3);

    fn chunk_positions() -> Vec<IVec3> {
        (0..CHUNKS.x)
            .flat_map(|x| (0..CHUNKS.y).flat_map(move |y| (0..CHUNKS.z).map(move |z| (x, y, z))))
            .map(IVec3::from)
            .collect()
    }

    /// Every column of the 256x256 area from the origin, `x` major.
    fn columns(seed: u32) -> Vec<Column> {
        let lattices: Vec<Lattice> = (0..16)
            .flat_map(|cx| (0..16).map(move |cz| IVec3::new(cx, 0, cz) * SIZE))
            .map(|origin| Lattice::around(seed, origin))
            .collect();
        (0..256)
            .flat_map(|x| (0..256).map(move |z| (x, z)))
            .map(|(x, z)| lattices[(x / SIZE * 16 + z / SIZE) as usize].column(x, z))
            .collect()
    }

    fn tops(seed: u32) -> Vec<i32> {
        columns(seed).iter().map(Column::top).collect()
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
        let interior = IVec3::new(1, 2, 1);
        for position in [wall, interior] {
            assert!(
                generate_chunk(5, position, CHUNKS) == world.chunks[&position],
                "{position}"
            );
        }
        assert_eq!(world.block(IVec3::new(0, 20, 40)), Block::Cobblestone);
    }

    #[test]
    fn world_column_top_matches_the_terrain() {
        let world = generate(CHUNKS, 5);
        for x in (1..47).step_by(3) {
            for z in (1..47).step_by(5) {
                let lattice =
                    Lattice::around(5, IVec3::new(x, 0, z).div_euclid(IVec3::splat(SIZE)) * SIZE);
                assert_eq!(
                    world.column_top(x, z),
                    lattice.column(x, z).top(),
                    "{x}, {z}"
                );
            }
        }
    }

    #[test]
    fn same_seed_same_terrain_and_other_seeds_differ() {
        assert_eq!(tops(7), tops(7));
        assert_ne!(tops(7), tops(8));
    }

    #[test]
    fn floor_is_solid_and_ceiling_is_clear() {
        for seed in 0..4 {
            for column in columns(seed) {
                assert!(column.solid(0));
                assert!((120..HEIGHT).all(|y| !column.solid(y)));
            }
        }
    }

    #[test]
    fn mostly_lowland_with_some_sea_and_some_uplands() {
        let tops: Vec<i32> = (0..4).flat_map(tops).collect();
        let share = |test: &dyn Fn(i32) -> bool| {
            tops.iter().filter(|&&top| test(top)).count() as f32 / tops.len() as f32
        };
        let below_sea = share(&|top| top < SEA_LEVEL);
        let lowland = share(&|top| (SEA_LEVEL + 1..=SEA_LEVEL + 12).contains(&top));
        assert!((0.1..=0.4).contains(&below_sea), "{below_sea}");
        assert!(lowland >= 0.4, "{lowland}");
        assert!(tops.iter().any(|&top| top >= SEA_LEVEL + 40));
    }

    #[test]
    fn overhangs_are_rare() {
        let columns: Vec<Column> = (0..4).flat_map(columns).collect();
        let overhung = columns
            .iter()
            .filter(|column| {
                let top = column.top();
                (1..top).any(|y| !column.solid(y) && (0..y).any(|below| column.solid(below)))
            })
            .count() as f32
            / columns.len() as f32;
        assert!((0.002..=0.15).contains(&overhung), "{overhung}");
    }

    #[test]
    fn chunk_borders_are_as_smooth_as_chunk_insides() {
        let mut border = (0, 0);
        let mut inside = (0, 0);
        for seed in 0..4 {
            let tops = tops(seed);
            let top = |x: i32, z: i32| tops[(x * 256 + z) as usize];
            for x in 0..255 {
                for z in 0..256 {
                    let step = (top(x + 1, z) - top(x, z)).abs();
                    let tally = if (x + 1) % SIZE == 0 {
                        &mut border
                    } else {
                        &mut inside
                    };
                    tally.0 += step;
                    tally.1 += 1;
                }
            }
        }
        let mean = |(sum, count): (i32, i32)| sum as f32 / count as f32;
        let (border, inside) = (mean(border), mean(inside));
        assert!(
            (border - inside).abs() <= inside * 0.25,
            "{border} vs {inside}"
        );
    }

    #[test]
    fn noise_stays_near_unit_range() {
        for x in -40..40 {
            for z in -40..40 {
                let point = Vec2::new(x as f32, z as f32) * 0.37;
                assert!(gradient_2d(3, point).abs() <= NOISE_BOUND);
                assert!(gradient_3d(3, point.extend(z as f32 * 0.21)).abs() <= NOISE_BOUND);
            }
        }
    }
}
