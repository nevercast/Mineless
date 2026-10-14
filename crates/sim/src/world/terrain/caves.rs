//! Caves carved out of the shaped and dressed terrain: winding, mostly level tunnels where two
//! noise fields are both near zero, and rare chambers where a third is high. Every carve decision
//! comes from the seed and the cell's world position alone, so a chunk carves its own cells without
//! reading any other chunk.

use glam::{IVec3, Vec3};

use super::noise::{fractal, gradient_3d, lerp, smoothstep};
use super::{BEDROCK_ROWS, Block, Chunk, SEA_LEVEL, SIZE};

/// Blocks across one cell of the two tunnel fields. Cells are half as tall as they are wide, so
/// each field's zero sheet lies mostly level and the tunnels where two sheets cross do too.
const TUNNEL_SCALE: Vec3 = Vec3::new(48.0, 24.0, 48.0);
/// How far either tunnel field may stray from zero for a cell to be carved, the narrowest and the
/// widest a slow noise picks between, so some passages are squeezes and some are corridors.
const NARROW: f32 = 0.025;
const WIDE: f32 = 0.062;
const WIDTH_SCALE: f32 = 160.0;
/// Blocks across one cell of the chamber field, and the value it must pass to be carved: high
/// enough that chambers are rare.
const CAVERN_SCALE: Vec3 = Vec3::new(96.0, 48.0, 96.0);
const CAVERN: f32 = 0.76;
/// Caves are fullest up to `FULL_DEPTH` and narrow above it, to `HIGH_WIDTH` of their width and
/// with chambers out of reach, by `THIN_HEIGHT`. They still reach the uplands enough to open as
/// mouths in cliffs.
const FULL_DEPTH: i32 = SEA_LEVEL - 6;
const THIN_HEIGHT: i32 = 100;
const HIGH_WIDTH: f32 = 0.45;
/// Rows of the column below its top where a cave may only open through bare rock.
const ENTRANCE_ROWS: i32 = 3;

/// Cave fields are sampled every `STEP` blocks on every axis and interpolated between, as the
/// terrain density is, but finer, since tunnels are only a few blocks across. The lattice is
/// anchored to world coordinates, so neighbouring chunks agree where they meet.
const STEP: i32 = 4;
const PER_CHUNK: i32 = SIZE / STEP;

const TUNNEL_A_SALT: u32 = 0x3c6e_f372;
const TUNNEL_B_SALT: u32 = 0xa54f_f53a;
const WIDTH_SALT: u32 = 0x510e_527f;
const CAVERN_SALT: u32 = 0x9b05_688c;

/// The cave fields at one lattice point: the two tunnel fields, the tunnel width there and the
/// chamber field.
#[derive(Clone, Copy)]
struct Sample {
    a: f32,
    b: f32,
    width: f32,
    cavern: f32,
}

impl Sample {
    fn at(seed: u32, position: IVec3) -> Self {
        let point = position.as_vec3();
        let tunnel = |salt: u32| {
            fractal(2, |frequency, octave| {
                gradient_3d(seed ^ salt ^ octave, point * frequency / TUNNEL_SCALE)
            })
        };
        let width = gradient_3d(seed ^ WIDTH_SALT, point / WIDTH_SCALE);
        Self {
            a: tunnel(TUNNEL_A_SALT),
            b: tunnel(TUNNEL_B_SALT),
            width: lerp(NARROW, WIDE, smoothstep(-0.3, 0.3, width)),
            cavern: gradient_3d(seed ^ CAVERN_SALT, point / CAVERN_SCALE),
        }
    }

    fn lerp(self, other: Sample, t: f32) -> Sample {
        Sample {
            a: lerp(self.a, other.a, t),
            b: lerp(self.b, other.b, t),
            width: lerp(self.width, other.width, t),
            cavern: lerp(self.cavern, other.cavern, t),
        }
    }
}

/// How full caves are at height `y`, from 1 deep down to 0 at `THIN_HEIGHT`.
fn fullness(y: i32) -> f32 {
    1.0 - smoothstep(FULL_DEPTH as f32, THIN_HEIGHT as f32, y as f32)
}

/// How far the tunnel fields may stray from zero at height `y` where the width field gives `width`.
fn tunnel_width(width: f32, y: i32) -> f32 {
    width * lerp(HIGH_WIDTH, 1.0, fullness(y))
}

/// What the chamber field must pass at height `y`. Above `THIN_HEIGHT` it is out of reach.
fn cavern_threshold(y: i32) -> f32 {
    lerp(1.5, CAVERN, fullness(y))
}

/// Whether the cave fields carve a cell at height `y` where they interpolate to `sample`.
fn carves(sample: Sample, y: i32) -> bool {
    let width = tunnel_width(sample.width, y);
    (sample.a.abs() < width && sample.b.abs() < width) || sample.cavern > cavern_threshold(y)
}

/// Only these are ever carved: no logs, leaves, water, border, snow or anything placed.
fn carvable(block: Block) -> bool {
    matches!(block, Block::Stone | Block::Dirt | Block::Gravel | Block::Sand)
}

/// The cave fields on the lattice points of one chunk column, up to the highest ground in it.
pub(super) struct Caves {
    /// World position of the first sample.
    origin: IVec3,
    /// Lattice levels sampled, from the bottom of the world.
    levels: i32,
    samples: Vec<Sample>,
}

impl Caves {
    const ACROSS: i32 = PER_CHUNK + 1;

    /// Samples the column of chunks whose lowest corner is `chunk_origin`, from the bottom of the
    /// world up to height `highest`, above which nothing is carved.
    pub(super) fn around(seed: u32, chunk_origin: IVec3, highest: i32) -> Self {
        let origin = chunk_origin.with_y(0);
        let levels = highest.max(0) / STEP + 2;
        let samples = (0..Self::ACROSS)
            .flat_map(|i| (0..Self::ACROSS).flat_map(move |k| (0..levels).map(move |level| IVec3::new(i, level, k))))
            .map(|lattice| Sample::at(seed, origin + lattice * STEP))
            .collect();
        Self { origin, levels, samples }
    }

    fn sample(&self, lattice: IVec3) -> Sample {
        self.samples[((lattice.x * Self::ACROSS + lattice.z) * self.levels + lattice.y) as usize]
    }

    /// Carves the caves into `chunk`, whose lowest corner is `origin`, leaving any cell that
    /// `keep` says must stay. Only terrain materials are carved, and nothing in the bottom rows.
    pub(super) fn carve(&self, chunk: &mut Chunk, origin: IVec3, keep: impl Fn(IVec3) -> bool) {
        let first = (origin - self.origin) / STEP;
        for cell in (0..PER_CHUNK).flat_map(|i| (0..PER_CHUNK).flat_map(move |j| (0..PER_CHUNK).map(move |k| IVec3::new(i, j, k)))) {
            let lattice = first + cell;
            if lattice.y + 1 >= self.levels {
                continue;
            }
            let corners = [0, 1].map(|dx| [0, 1].map(|dy| [0, 1].map(|dz| self.sample(lattice + IVec3::new(dx, dy, dz)))));
            if !Self::may_carve(&corners, lattice.y * STEP) {
                continue;
            }
            for local in (0..STEP).flat_map(|x| (0..STEP).flat_map(move |y| (0..STEP).map(move |z| IVec3::new(x, y, z)))) {
                let position = self.origin + lattice * STEP + local;
                let in_chunk = (position - origin).as_uvec3();
                let (x, y, z) = (in_chunk.x as usize, in_chunk.y as usize, in_chunk.z as usize);
                if position.y < BEDROCK_ROWS || !carvable(chunk.get(x, y, z)) {
                    continue;
                }
                let t = local.as_vec3() / STEP as f32;
                let along_z = |c: [Sample; 2]| c[0].lerp(c[1], t.z);
                let along_y = |c: [[Sample; 2]; 2]| along_z(c[0]).lerp(along_z(c[1]), t.y);
                let sample = along_y(corners[0]).lerp(along_y(corners[1]), t.x);
                if carves(sample, position.y) && !keep(position) {
                    chunk.set(x, y, z, Block::Air);
                }
            }
        }
    }

    /// Whether any cell of a lattice cell with these corners and lowest row `y` could be carved.
    /// Interpolation never leaves the range of the corners, so a field whose corners all lie on
    /// one side of its carving range rules the cell out, and most cells skip the per-cell work.
    fn may_carve(corners: &[[[Sample; 2]; 2]; 2], y: i32) -> bool {
        let all = corners.iter().flatten().flatten();
        let range = |field: fn(&Sample) -> f32| {
            all.clone()
                .map(field)
                .fold((f32::MAX, f32::MIN), |(low, high), value| (low.min(value), high.max(value)))
        };
        // Width and threshold are widest and lowest at the cell's lowest row.
        let width = tunnel_width(range(|sample| sample.width).1, y);
        let near_zero = |(low, high): (f32, f32)| low < width && high > -width;
        let tunnel = near_zero(range(|sample| sample.a)) && near_zero(range(|sample| sample.b));
        tunnel || range(|sample| sample.cavern).1 > cavern_threshold(y)
    }
}

/// Whether a cell at height `y` in a column whose top is `top` may be carved, given whether the
/// column's surface block is bare stone. Caves open only as mouths in rock faces, never through
/// grass, sand or snow.
pub(super) fn entrance_allowed(y: i32, top: i32, bare_rock: bool) -> bool {
    y < top - ENTRANCE_ROWS || bare_rock
}

const _: () = assert!(SIZE % STEP == 0, "lattice cells must not straddle chunks");
