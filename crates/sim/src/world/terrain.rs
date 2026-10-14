//! Seeded terrain from a 3D density field: low plains a few blocks above the sea, broken by
//! upland regions that rise in escarpments with ledges, overhangs and now and then an arch or a
//! lump of floating land, all inside a full-height border that keeps walkers from falling off
//! the edge of the world. Climate and biome then decide what the top few blocks of every surface are
//! made of: sandy and gravelly coasts and sea floors, grass, bare rock on cliffs and snow on cold
//! high ground.

use std::time::{Duration, Instant};

use glam::{IVec3, Vec2, Vec3};

mod noise;
mod sea;
mod trees;

use super::chunk::Chunk;
use super::{Block, CHUNK_SIZE, Extent, SIZE, World};
use noise::{NOISE_BOUND, fractal, gradient_2d, gradient_3d, lerp, smoothstep};
use sea::SeaFill;

/// The height the terrain is shaped for. The world may be taller, leaving air above it.
pub const HEIGHT: i32 = 128;
/// Air below this height that is open to the sea will be water; the sea surface is the top of
/// the row below it.
pub const SEA_LEVEL: i32 = 40;

/// Where the low ground sits relative to the sea, and how far it strays above and below.
const LOWLAND_SPREAD: f32 = 35.0;
/// Seas deepen faster than land rises, so there is deep water with a varied floor offshore.
const SEA_SPREAD: f32 = 80.0;
/// Shifts base elevation so that most of the world is land.
const ELEVATION_OFFSET: f32 = 0.035;
/// Land climbs this many blocks within a few blocks of the shore, so beaches are a narrow band
/// rather than wide flats of plains that happen to lie just above the sea.
const COAST_RISE: f32 = 4.0;
const COAST_EDGE: f32 = 0.02;
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

/// Rows at the bottom that are always solid, so nothing falls out of the world.
const BEDROCK_ROWS: i32 = 4;
/// Density falls by this much per block above `CEILING_START`, so peaks round off well below
/// the top of the world instead of being sliced flat by it.
const CEILING_START: i32 = 112;
const CEILING_FALL: f32 = 0.75;

/// Blocks across one cell of temperature and humidity, so a world shows one to three climates,
/// and how far they spread around the middle of `0..=1`.
const CLIMATE_SCALE: f32 = 400.0;
const CLIMATE_SPREAD: f32 = 0.8;
/// Fine noise added to climate so biome and surface boundaries are ragged, not smooth curves.
const JITTER_SCALE: f32 = 8.0;
const JITTER: f32 = 0.05;
/// Temperature lost per block above the sea. Surfaces are snow where it falls below zero, about
/// 50 blocks up in a mild climate and lower in cold ones.
const LAPSE: f32 = 0.01;

/// Relief below which a column at the waterline is beach, and above which it is hills.
const BEACH_RELIEF: f32 = 0.3;
const HILLS_RELIEF: f32 = 0.5;
const FOREST_HUMIDITY: f32 = 0.5;

/// Surface runs are this many blocks deep, varying with a small-scale noise.
const RUN_DEPTHS: std::ops::RangeInclusive<i32> = 2..=5;
const RUN_SCALE: f32 = 12.0;
/// Sea floor this close below the surface is sand; deeper it is patches of sand, gravel and dirt.
const SHALLOWS: i32 = 4;
const SEABED_SCALE: f32 = 24.0;
const SEABED_PATCH: f32 = 0.12;
/// Low-frequency noise that turns stretches of beach to gravel.
const SHINGLE_SCALE: f32 = 48.0;
const SHINGLE: f32 = 0.15;
/// A column top this far above or below a neighbour's is a cliff face of bare rock.
const CLIFF_STEP: i32 = 3;
/// Above this, hills are a mix of grass and bare rock in mid-sized patches.
const HIGH_ROCK_LINE: i32 = SEA_LEVEL + 30;
const HIGH_ROCK_SCALE: f32 = 16.0;

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
const TEMPERATURE_SALT: u32 = 0xc2b2_ae35;
const HUMIDITY_SALT: u32 = 0x27d4_eb2f;
const JITTER_SALT: u32 = 0x1656_67b1;
const RUN_SALT: u32 = 0xd3a2_646c;
const SEABED_SALT: u32 = 0xfd70_46c5;
const SHINGLE_SALT: u32 = 0xb55a_4f09;
const HIGH_ROCK_SALT: u32 = 0x7feb_352d;

/// Fills `extent` with the terrain for `seed`. The same seed always gives the same world.
pub fn generate(extent: Extent, seed: u32) -> World {
    let mut world = World::default();
    let mut generation = Generation::new(extent, seed);
    while generation.stage() != Stage::Done {
        generation.work(&mut world, Duration::MAX);
    }
    world
}

/// What a `Generation` is doing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    Chunks,
    Sea,
    Done,
}

/// Generates a world a slice at a time, so a caller can keep its window responsive between
/// slices. The world comes out the same however the work is sliced.
pub struct Generation {
    seed: u32,
    extent: Extent,
    chunks: Vec<IVec3>,
    generated: usize,
    sea: SeaFill,
}

impl Generation {
    pub fn new(extent: Extent, seed: u32) -> Self {
        assert!(
            extent.height >= HEIGHT,
            "the world must be at least as tall as the terrain"
        );
        Self {
            seed,
            extent,
            chunks: extent.chunk_positions().collect(),
            generated: 0,
            sea: SeaFill::new(extent),
        }
    }

    pub fn stage(&self) -> Stage {
        if self.generated < self.chunks.len() {
            Stage::Chunks
        } else if !self.sea.done(&self.chunks) {
            Stage::Sea
        } else {
            Stage::Done
        }
    }

    /// The fraction of the current stage's work done, in `0..=1`.
    pub fn progress(&self) -> f32 {
        match self.stage() {
            Stage::Chunks => self.generated as f32 / self.chunks.len() as f32,
            Stage::Sea => self.sea.progress(&self.chunks),
            Stage::Done => 1.0,
        }
    }

    /// Generates into `world` until `budget` has passed, and always does at least some work
    /// unless generation is done.
    pub fn work(&mut self, world: &mut World, budget: Duration) {
        let start = Instant::now();
        loop {
            match self.stage() {
                Stage::Chunks => {
                    let position = self.chunks[self.generated];
                    world.insert(position, generate_chunk(self.seed, position, self.extent));
                    self.generated += 1;
                }
                Stage::Sea => self.sea.work(world, &self.chunks),
                Stage::Done => return,
            }
            if start.elapsed() >= budget {
                return;
            }
        }
    }
}

/// The chunk at `position` in a world reaching over `extent`, from nothing but the seed and
/// those two, so chunks come out the same whatever order they are generated in.
fn generate_chunk(seed: u32, position: IVec3, extent: Extent) -> Chunk {
    let mut chunk = Chunk::empty();
    let origin = position * SIZE;
    // Nothing of the terrain reaches its own height, so chunks above it skip shaping it.
    let columns = (origin.y < HEIGHT).then(|| Neighbourhood::around(seed, origin));
    for local_x in 0..CHUNK_SIZE {
        for local_z in 0..CHUNK_SIZE {
            let (x, z) = (origin.x + local_x as i32, origin.z + local_z as i32);
            if !extent.contains_column(x, z) {
                continue;
            }
            let on_edge = extent.on_edge(x, z);
            let terrain = columns
                .as_ref()
                .filter(|_| !on_edge)
                .map(|columns| (Surface::at(seed, x, z, columns), columns.column(x, z)));
            for local_y in 0..CHUNK_SIZE {
                let y = origin.y + local_y as i32;
                let block = match &terrain {
                    _ if on_edge => Block::WorldBorder,
                    Some((surface, column)) => surface.block(column, y),
                    None => Block::Air,
                };
                if block != Block::Air {
                    chunk.set(local_x, local_y, local_z, block);
                }
            }
        }
    }
    if let Some(columns) = &columns {
        trees::grow(seed, &mut chunk, origin, columns, extent);
    }
    chunk.compact();
    chunk
}

/// The columns of one chunk and a border round it, with their tops, since the cliff test and
/// trees look at neighbouring columns.
struct Neighbourhood {
    /// World position of the first column.
    origin: IVec3,
    columns: Vec<Column>,
    tops: Vec<i32>,
}

impl Neighbourhood {
    const ACROSS: i32 = SIZE + 2 * trees::REACH;

    fn around(seed: u32, chunk_origin: IVec3) -> Self {
        let lattice = Lattice::around(seed, chunk_origin);
        let origin = chunk_origin - IVec3::new(trees::REACH, 0, trees::REACH);
        let columns: Vec<Column> = (0..Self::ACROSS)
            .flat_map(|i| (0..Self::ACROSS).map(move |k| (i, k)))
            .map(|(i, k)| lattice.column(origin.x + i, origin.z + k))
            .collect();
        let tops = columns.iter().map(Column::top).collect();
        Self {
            origin,
            columns,
            tops,
        }
    }

    fn index(&self, x: i32, z: i32) -> usize {
        let (i, k) = (x - self.origin.x, z - self.origin.z);
        debug_assert!((0..Self::ACROSS).contains(&i) && (0..Self::ACROSS).contains(&k));
        (i * Self::ACROSS + k) as usize
    }

    fn column(&self, x: i32, z: i32) -> &Column {
        &self.columns[self.index(x, z)]
    }

    fn top(&self, x: i32, z: i32) -> i32 {
        self.tops[self.index(x, z)]
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Biome {
    Ocean,
    Beach,
    Hills,
    Forest,
    Plains,
}

impl Biome {
    fn classify(top: i32, relief: f32, humidity: f32) -> Self {
        if top < SEA_LEVEL - 1 {
            Biome::Ocean
        } else if top <= SEA_LEVEL + 2 && relief < BEACH_RELIEF {
            Biome::Beach
        } else if relief > HILLS_RELIEF {
            Biome::Hills
        } else if humidity > FOREST_HUMIDITY {
            Biome::Forest
        } else {
            Biome::Plains
        }
    }
}

/// Temperature and humidity in `0..=1` at one column.
fn climate(seed: u32, x: i32, z: i32) -> (f32, f32) {
    let column = Vec2::new(x as f32, z as f32);
    let field = |salt: u32| {
        let broad = fractal(3, |frequency, octave| {
            gradient_2d(seed ^ salt ^ octave, column * frequency / CLIMATE_SCALE)
        });
        let jitter = gradient_2d(seed ^ salt ^ JITTER_SALT, column / JITTER_SCALE);
        (0.5 + CLIMATE_SPREAD * broad + JITTER * jitter).clamp(0.0, 1.0)
    };
    (field(TEMPERATURE_SALT), field(HUMIDITY_SALT))
}

fn snowy(temperature: f32, y: i32) -> bool {
    y >= SEA_LEVEL && temperature < LAPSE * (y - SEA_LEVEL) as f32
}

/// What the surfaces of one column are made of.
struct Surface {
    top: i32,
    biome: Biome,
    temperature: f32,
    /// How many blocks deep each surface run is.
    run: i32,
    /// Whether the column top stands a cliff's height above or below a neighbour's.
    cliff: bool,
    /// What deep sea floor is made of here.
    seabed: Block,
    /// Whether beach here is gravel rather than sand.
    shingle: bool,
    /// Whether high hills are bare rock here.
    high_rock: bool,
}

impl Surface {
    fn at(seed: u32, x: i32, z: i32, columns: &Neighbourhood) -> Self {
        let top = columns.top(x, z);
        let (temperature, humidity) = climate(seed, x, z);
        let point = Vec2::new(x as f32, z as f32);
        let noise = |salt: u32, scale: f32| {
            fractal(2, |frequency, octave| {
                gradient_2d(seed ^ salt ^ octave, point * frequency / scale)
            })
        };
        let (shallowest, deepest) = (*RUN_DEPTHS.start(), *RUN_DEPTHS.end());
        let run = shallowest
            + ((noise(RUN_SALT, RUN_SCALE) + 0.5) * (deepest - shallowest) as f32).round() as i32;
        let seabed = match noise(SEABED_SALT, SEABED_SCALE) {
            n if n > SEABED_PATCH => Block::Gravel,
            n if n < -SEABED_PATCH => Block::Dirt,
            _ => Block::Sand,
        };
        let cliff = [(1, 0), (-1, 0), (0, 1), (0, -1)]
            .iter()
            .any(|(dx, dz)| (columns.top(x + dx, z + dz) - top).abs() >= CLIFF_STEP);
        Self {
            top,
            biome: Biome::classify(top, columns.column(x, z).relief, humidity),
            temperature,
            run: run.clamp(shallowest, deepest),
            cliff,
            seabed,
            shingle: noise(SHINGLE_SALT, SHINGLE_SCALE) > SHINGLE,
            high_rock: noise(HIGH_ROCK_SALT, HIGH_ROCK_SCALE) > 0.0,
        }
    }

    /// Each exposed top, a solid cell with air above, heads a run of solid cells beneath it whose
    /// material its height and this column decide; everything else solid is stone. Ledges and
    /// floating land are dressed as well as the column top.
    fn block(&self, column: &Column, y: i32) -> Block {
        if !column.solid(y) {
            return Block::Air;
        }
        let Some(depth) = (0..self.run).find(|&depth| !column.solid(y + depth + 1)) else {
            return Block::Stone;
        };
        let (top, beneath) = self.run_blocks(y + depth);
        if depth == 0 { top } else { beneath }
    }

    /// The top block and the blocks beneath it for a surface whose exposed top is at `y`.
    fn run_blocks(&self, y: i32) -> (Block, Block) {
        // Cliffs are judged against neighbouring column tops, so only the column top can be one.
        let cliff = self.cliff && y + 1 == self.top;
        if snowy(self.temperature, y) {
            return (Block::Snow, if cliff { Block::Stone } else { Block::Dirt });
        }
        if y < SEA_LEVEL - 1 {
            let floor = if y >= SEA_LEVEL - 1 - SHALLOWS {
                Block::Sand
            } else {
                self.seabed
            };
            return (floor, floor);
        }
        if self.biome == Biome::Beach && y <= SEA_LEVEL + 2 {
            let shore = if self.shingle {
                Block::Gravel
            } else {
                Block::Sand
            };
            return (shore, shore);
        }
        if cliff || (self.biome == Biome::Hills && y >= HIGH_ROCK_LINE && self.high_rock) {
            return (Block::Stone, Block::Stone);
        }
        // Ground level with the sea surface outside a beach is still never grass.
        if y < SEA_LEVEL {
            return (Block::Sand, Block::Sand);
        }
        (Block::Grass, Block::Dirt)
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
    /// Relief at each lattice column.
    relief: Vec<f32>,
}

impl Lattice {
    fn around(seed: u32, chunk_origin: IVec3) -> Self {
        // One lattice cell of border all round covers the columns next to the chunk.
        let origin = (chunk_origin - LATTICE_STEP).with_y(0);
        let across = SIZE / LATTICE_STEP.x + 3;
        let mut samples = Vec::with_capacity((across * across) as usize * LEVELS);
        let mut relief = Vec::with_capacity((across * across) as usize);
        for i in 0..across {
            for k in 0..across {
                let (x, z) = (origin.x + i * LATTICE_STEP.x, origin.z + k * LATTICE_STEP.z);
                let fields = Fields::at(seed, x, z);
                relief.push(fields.relief);
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
            relief,
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
        let corners = [(i, k), (i + 1, k), (i, k + 1), (i + 1, k + 1)]
            .map(|(i, k)| (i * self.across + k) as usize);
        let blend = |value: &dyn Fn(usize) -> f32| {
            let [a, b, c, d] = corners.map(value);
            lerp(lerp(a, b, t.x), lerp(c, d, t.x), t.y)
        };
        let mut levels = [0.0; LEVELS];
        for (level, value) in levels.iter_mut().enumerate() {
            *value = blend(&|corner| self.samples[corner * LEVELS + level]);
        }
        Column {
            levels,
            relief: blend(&|corner| self.relief[corner]),
        }
    }
}

/// One column's density at each lattice level, linear in between, and its relief.
struct Column {
    levels: [f32; LEVELS],
    relief: f32,
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
        // Density is linear between levels, so a lattice cell with no positive end is all air.
        let highest_solid_cell = (0..LEVELS - 1)
            .rev()
            .find(|&level| self.levels[level] > 0.0 || self.levels[level + 1] > 0.0)
            .map_or(0, |level| (level + 1) as i32 * LATTICE_STEP.y);
        (0..highest_solid_cell.max(BEDROCK_ROWS))
            .rev()
            .find(|&y| self.solid(y))
            .map_or(0, |y| y + 1)
    }
}

/// The 2D fields that shape one column.
struct Fields {
    /// Height the ground tends towards.
    target: f32,
    /// How much the column belongs to an upland region, in `0..=1`.
    relief: f32,
    /// How strongly the 3D detail bends the ground away from the target.
    detail: f32,
}

impl Fields {
    fn at(seed: u32, x: i32, z: i32) -> Self {
        let column = Vec2::new(x as f32, z as f32);
        let elevation = ELEVATION_OFFSET
            + fractal(4, |frequency, salt| {
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
                + elevation
                    * if elevation < 0.0 {
                        SEA_SPREAD
                    } else {
                        LOWLAND_SPREAD
                    }
                + COAST_RISE * smoothstep(0.0, COAST_EDGE, elevation)
                + UPLAND_RISE * relief,
            detail: lerp(PLAINS_DETAIL, UPLAND_DETAIL, relief),
            relief,
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

#[cfg(test)]
mod tests {
    use super::*;
    use glam::IVec2;

    /// Small enough to generate quickly, taller than the terrain, and not a whole number of
    /// chunks across, so the border stands partway into the outermost chunks.
    const EXTENT: Extent = Extent {
        half_width: 24,
        height: HEIGHT + 2 * SIZE,
    };
    /// The 320-block world the showcase windows and the dressed area were picked in.
    const FULL_WORLD: Extent = Extent {
        half_width: 160,
        height: HEIGHT + 2 * SIZE,
    };

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
        let positions: Vec<IVec3> = EXTENT.chunk_positions().collect();
        assert!(
            positions
                .iter()
                .any(|position| position.cmplt(IVec3::ZERO).any())
        );
        let mut in_order = World::default();
        for &position in &positions {
            in_order.insert(position, generate_chunk(5, position, EXTENT));
        }
        let mut reversed = World::default();
        for &position in positions.iter().rev() {
            reversed.insert(position, generate_chunk(5, position, EXTENT));
        }
        for position in positions {
            assert!(
                in_order.chunks[&position] == reversed.chunks[&position],
                "{position}"
            );
        }
    }

    #[test]
    fn a_chunk_generated_alone_matches_the_whole_world() {
        let world = generate(EXTENT, 5);
        let border = IVec3::new(-2, 1, 0);
        let interior = IVec3::new(-1, 2, 0);
        for position in [border, interior] {
            assert!(
                generate_chunk(5, position, EXTENT) == world.chunks[&position],
                "{position}"
            );
        }
        assert_eq!(world.block(IVec3::new(-24, 20, 5)), Block::WorldBorder);
    }

    /// A hash of every block of a generated world, recorded before chunks could be stored as one
    /// block and before the sea fill could be sliced, so neither changed the world.
    #[test]
    fn the_world_is_unchanged_block_for_block() {
        let world = generate(EXTENT, 5);
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for x in -32..32 {
            for y in 0..EXTENT.height {
                for z in -32..32 {
                    for byte in format!("{:?}", world.block(IVec3::new(x, y, z))).bytes() {
                        hash = (hash ^ u64::from(byte)).wrapping_mul(0x100_0000_01b3);
                    }
                }
            }
        }
        assert_eq!(hash, 0xff3f_5c37_a710_5744);
    }

    #[test]
    fn generating_in_the_smallest_slices_makes_the_same_world() {
        let whole = generate(EXTENT, 5);
        let mut world = World::default();
        let mut generation = Generation::new(EXTENT, 5);
        generation.work(&mut world, Duration::ZERO);
        assert_eq!(world.chunks.len(), 1);
        let mut slices = 1;
        let mut seen_sea = false;
        while generation.stage() != Stage::Done {
            seen_sea |= generation.stage() == Stage::Sea;
            let progress = generation.progress();
            assert!((0.0..=1.0).contains(&progress), "{progress}");
            generation.work(&mut world, Duration::ZERO);
            slices += 1;
        }
        assert!(seen_sea && slices > EXTENT.chunk_positions().count() + 1);
        assert_eq!(world.chunks.len(), whole.chunks.len());
        for (position, chunk) in &whole.chunks {
            assert!(world.chunks[position] == *chunk, "{position}");
        }
    }

    #[test]
    fn world_column_top_matches_the_terrain() {
        let world = generate(EXTENT, 5);
        for x in (-23..23).step_by(3) {
            for z in (-23..23).step_by(5) {
                let lattice =
                    Lattice::around(5, IVec3::new(x, 0, z).div_euclid(IVec3::splat(SIZE)) * SIZE);
                let top = lattice.column(x, z).top();
                if (top..HEIGHT).any(|y| is_tree(world.block(IVec3::new(x, y, z)))) {
                    continue;
                }
                assert_eq!(world.column_top(x, z), top, "{x}, {z}");
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

    /// Dressed blocks of a 96x96 area in the middle of the full world, clear of the border, for
    /// seeds 0-3. Built once and shared, since it is the slowest part of these tests.
    fn dressed() -> &'static [World] {
        static WORLDS: std::sync::OnceLock<Vec<World>> = std::sync::OnceLock::new();
        WORLDS.get_or_init(|| {
            (0..4)
                .map(|seed| {
                    let mut world = World::default();
                    for cx in -3..3 {
                        for cy in 0..HEIGHT / SIZE {
                            for cz in -3..3 {
                                let position = IVec3::new(cx, cy, cz);
                                world.insert(position, generate_chunk(seed, position, FULL_WORLD));
                            }
                        }
                    }
                    world
                })
                .collect()
        })
    }

    /// Every block of the dressed area with its seed and position.
    fn dressed_blocks() -> impl Iterator<Item = (u32, IVec3, Block)> {
        dressed().iter().zip(0..).flat_map(|(world, seed)| {
            (-48..48).flat_map(move |x| {
                (0..HEIGHT).flat_map(move |y| {
                    (-48..48).map(move |z| {
                        let position = IVec3::new(x, y, z);
                        (seed, position, world.block(position))
                    })
                })
            })
        })
    }

    fn is_tree(block: Block) -> bool {
        matches!(block, Block::OakLog(_) | Block::OakLeaves)
    }

    fn exposed(world: &World, position: IVec3) -> bool {
        world.block(position) != Block::Air && world.block(position + IVec3::Y) == Block::Air
    }

    #[test]
    fn biomes_follow_the_table() {
        assert_eq!(Biome::classify(SEA_LEVEL - 2, 0.9, 0.9), Biome::Ocean);
        assert_eq!(Biome::classify(SEA_LEVEL - 1, 0.0, 0.9), Biome::Beach);
        assert_eq!(Biome::classify(SEA_LEVEL + 2, 0.0, 0.9), Biome::Beach);
        assert_eq!(Biome::classify(SEA_LEVEL + 2, 0.9, 0.9), Biome::Hills);
        assert_eq!(Biome::classify(SEA_LEVEL + 3, 0.0, 0.9), Biome::Forest);
        assert_eq!(Biome::classify(SEA_LEVEL + 3, 0.0, 0.1), Biome::Plains);
        for seed in 0..4 {
            let columns = Neighbourhood::around(seed, IVec3::new(160, 0, 160));
            for x in 160..176 {
                for z in 160..176 {
                    let surface = Surface::at(seed, x, z, &columns);
                    let top = surface.top;
                    match surface.biome {
                        Biome::Ocean => assert!(top < SEA_LEVEL - 1),
                        Biome::Beach => assert!((SEA_LEVEL - 1..=SEA_LEVEL + 2).contains(&top)),
                        _ => assert!(top >= SEA_LEVEL - 1),
                    }
                }
            }
        }
    }

    #[test]
    fn materials_stay_where_they_belong() {
        for (seed, position, block) in dressed_blocks() {
            let y = position.y;
            match block {
                Block::Grass => assert!(y >= SEA_LEVEL, "grass at {position}"),
                Block::Sand => assert!(y < SEA_LEVEL + 3, "sand at {position}"),
                Block::Snow => {
                    let (temperature, _) = climate(seed, position.x, position.z);
                    assert!(snowy(temperature, y), "snow at {position}, seed {seed}");
                }
                Block::Cobblestone => panic!("terrain placed cobblestone at {position}"),
                _ => {}
            }
        }
    }

    #[test]
    fn border_fills_the_edge_columns_and_terrain_places_no_cobblestone() {
        let world = generate(EXTENT, 5);
        // The outermost chunks reach past the world, from -32 to 31.
        for x in -32..32 {
            for z in -32..32 {
                let inside = (-24..24).contains(&x) && (-24..24).contains(&z);
                let on_edge = inside && (x == -24 || z == -24 || x == 23 || z == 23);
                for y in 0..EXTENT.height {
                    let block = world.block(IVec3::new(x, y, z));
                    assert_eq!(block == Block::WorldBorder, on_edge, "{x}, {y}, {z}");
                    assert_ne!(block, Block::Cobblestone, "{x}, {y}, {z}");
                    assert!(inside || block == Block::Air, "{x}, {y}, {z}");
                }
            }
        }
    }

    #[test]
    fn a_2016_block_world_has_its_border_on_its_outermost_chunk_edges() {
        let extent = Extent {
            half_width: 1008,
            height: HEIGHT + 2 * SIZE,
        };
        // The outermost chunks, -63 and 62, end exactly at the border, -1008 and 1007.
        for position in [
            IVec3::new(-63, 3, 0),
            IVec3::new(62, 0, 5),
            IVec3::new(-2, 9, -63),
            IVec3::new(40, 4, 62),
            IVec3::new(62, 6, 62),
        ] {
            let chunk = generate_chunk(5, position, extent);
            let origin = position * SIZE;
            for local in (0..SIZE).flat_map(|x| (0..SIZE).map(move |z| IVec3::new(x, 0, z))) {
                let (x, z) = (origin.x + local.x, origin.z + local.z);
                let ring = x == -1008 || z == -1008 || x == 1007 || z == 1007;
                for y in 0..SIZE {
                    let block = chunk.get(local.x as usize, y as usize, local.z as usize);
                    assert_eq!(block == Block::WorldBorder, ring, "{x}, {z}");
                }
            }
        }
    }

    #[test]
    fn headroom_above_the_terrain_is_air() {
        let world = generate(EXTENT, 5);
        for x in -23..23 {
            for z in -23..23 {
                for y in 120..EXTENT.height {
                    let block = world.block(IVec3::new(x, y, z));
                    assert_eq!(block, Block::Air, "{x}, {y}, {z}");
                }
            }
        }
    }

    /// Hashes of chunks generated for seed 142 before the world was centred, when it spanned
    /// 0..320. Terrain depends only on world position, so the same chunks come out the same.
    #[test]
    fn centring_the_world_did_not_move_the_terrain() {
        for (position, recorded) in [
            (IVec3::new(2, 3, 4), 0xc182_2f7a_a2ba_7898_u64),
            (IVec3::new(3, 3, 7), 0x0c86_dc2e_6ed6_9068),
            (IVec3::new(6, 2, 3), 0x2eb0_1485_666c_8f55),
        ] {
            let chunk = generate_chunk(142, position, FULL_WORLD);
            let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
            for y in 0..CHUNK_SIZE {
                for z in 0..CHUNK_SIZE {
                    for x in 0..CHUNK_SIZE {
                        for byte in format!("{:?}", chunk.get(x, y, z)).bytes() {
                            hash = (hash ^ u64::from(byte)).wrapping_mul(0x100_0000_01b3);
                        }
                    }
                }
            }
            assert_eq!(hash, recorded, "{position}");
        }
    }

    #[test]
    fn every_surface_material_occurs() {
        let mut found = Vec::new();
        for (seed, position, block) in dressed_blocks() {
            if exposed(&dressed()[seed as usize], position) {
                found.push(block);
            }
        }
        for block in [
            Block::Grass,
            Block::Dirt,
            Block::Stone,
            Block::Sand,
            Block::Gravel,
            Block::Snow,
        ] {
            assert!(found.contains(&block), "{block:?}");
        }
    }

    #[test]
    fn dressing_and_trees_keep_the_shape() {
        for seed in 0..4 {
            let world = &dressed()[seed as usize];
            let lattice = Lattice::around(seed, IVec3::new(-16, 0, -16));
            for x in -16..0 {
                for z in -16..0 {
                    let column = lattice.column(x, z);
                    for y in 0..HEIGHT {
                        let block = world.block(IVec3::new(x, y, z));
                        let solid = block != Block::Air && !is_tree(block);
                        assert_eq!(solid, column.solid(y), "{x}, {y}, {z}");
                    }
                }
            }
        }
    }

    /// The surfaces of every column in the chunk columns from `first` to `last`, inclusive.
    fn surfaces(seed: u32, first: IVec2, last: IVec2) -> Vec<(Surface, Column)> {
        let mut found = Vec::new();
        for cx in first.x..=last.x {
            for cz in first.y..=last.y {
                let origin = IVec3::new(cx, 0, cz) * SIZE;
                let columns = Neighbourhood::around(seed, origin);
                for x in origin.x..origin.x + SIZE {
                    for z in origin.z..origin.z + SIZE {
                        let column = columns.column(x, z);
                        let column = Column {
                            levels: column.levels,
                            relief: column.relief,
                        };
                        found.push((Surface::at(seed, x, z, &columns), column));
                    }
                }
            }
        }
        found
    }

    /// The seeds picked to show each kind of terrain, and the cheap part of why each was picked,
    /// so retuning generation can't quietly lose them.
    #[test]
    fn showcase_seeds_still_show_what_they_were_picked_for() {
        const EXTREME_HILLS: u32 = 149;
        const FOREST: u32 = 242;
        const BEACH_AND_FLOATING: u32 = 108;

        let share = |found: &[(Surface, Column)], biome: Biome| {
            found
                .iter()
                .filter(|(surface, _)| surface.biome == biome)
                .count() as f32
                / found.len() as f32
        };

        let hills = surfaces(EXTREME_HILLS, IVec2::new(1, 1), IVec2::new(8, 8));
        assert!(share(&hills, Biome::Hills) > 0.6);
        let overhung = hills
            .iter()
            .filter(|(surface, column)| (1..surface.top).any(|y| !column.solid(y)))
            .count();
        assert!(overhung > 500, "{overhung}");

        let forest = surfaces(FOREST, IVec2::new(2, 2), IVec2::new(9, 9));
        assert!(share(&forest, Biome::Forest) > 0.8);

        let coast = surfaces(BEACH_AND_FLOATING, IVec2::new(-9, -8), IVec2::new(2, -3));
        let sand = coast
            .iter()
            .filter(|(surface, column)| {
                surface.biome == Biome::Beach
                    && surface.block(column, surface.top - 1) == Block::Sand
            })
            .count();
        assert!(sand > 1000, "{sand}");

        // A lump of land floating at about (76, 73, 54): the ground joined to it never reaches
        // the edges of a box drawn round it, so nothing holds it up. Trees don't count, since a
        // canopy can touch it without holding it up.
        let mut world = World::default();
        let (low, high) = (IVec3::new(4, 4, 2), IVec3::new(5, 5, 4));
        for cx in low.x..=high.x {
            for cy in low.y..=high.y {
                for cz in low.z..=high.z {
                    let position = IVec3::new(cx, cy, cz);
                    let chunk = generate_chunk(BEACH_AND_FLOATING, position, FULL_WORLD);
                    world.insert(position, chunk);
                }
            }
        }
        let (min, max) = (low * SIZE, (high + 1) * SIZE - 1);
        let ground = |position: IVec3| {
            let block = world.block(position);
            block.is_solid() && !matches!(block, Block::OakLog(_) | Block::OakLeaves)
        };
        let start = IVec3::new(74, 72, 56);
        assert!(ground(start));
        let mut reached = std::collections::HashSet::from([start]);
        let mut frontier = vec![start];
        while let Some(position) = frontier.pop() {
            assert!(
                position.cmpgt(min).all() && position.cmplt(max).all(),
                "the lump reaches {position}"
            );
            for step in [
                IVec3::X,
                IVec3::NEG_X,
                IVec3::Y,
                IVec3::NEG_Y,
                IVec3::Z,
                IVec3::NEG_Z,
            ] {
                let next = position + step;
                if ground(next) && reached.insert(next) {
                    frontier.push(next);
                }
            }
        }
    }
}
