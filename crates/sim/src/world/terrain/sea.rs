//! The sea fill: air below sea level becomes water where it connects to the open sea.

use glam::IVec3;

use super::SEA_LEVEL;
use crate::world::{Block, Extent, SIZE, World};

/// Floods the sea: air below `SEA_LEVEL` becomes water where it connects, through air below
/// sea level, to the open surface of the sea. Sealed hollows stay dry. Reachability doesn't
/// depend on the order cells are visited, so the result is the same however it is sliced.
pub(super) struct SeaFill {
    min: IVec3,
    /// Sea-level cells inside the world, the ones that can be open sea.
    size: IVec3,
    height: i32,
    /// Columns checked for open sea so far.
    columns: usize,
    /// Water cells whose neighbours haven't been looked at. A cell turns to water as it joins,
    /// so the world itself records which cells have been reached.
    frontier: Vec<IVec3>,
    /// Chunks looked at afterwards to store as one block if the water filled them.
    compacted: usize,
}

impl SeaFill {
    /// Cells flooded between checks of the time budget, which would otherwise cost about as
    /// much as the flooding.
    const BATCH: usize = 1024;

    pub(super) fn new(extent: Extent) -> Self {
        let min = extent.min();
        Self {
            min,
            size: extent.max().with_y(SEA_LEVEL) - min,
            height: extent.height,
            columns: 0,
            frontier: Vec::new(),
            compacted: 0,
        }
    }

    fn column_count(&self) -> usize {
        (self.size.x * self.size.z) as usize
    }

    pub(super) fn done(&self, chunks: &[IVec3]) -> bool {
        self.compacted == chunks.len()
    }

    pub(super) fn progress(&self, chunks: &[IVec3]) -> f32 {
        (self.columns + self.compacted) as f32 / (self.column_count() + chunks.len()) as f32
    }

    pub(super) fn work(&mut self, world: &mut World, chunks: &[IVec3]) {
        for _ in 0..Self::BATCH {
            if let Some(position) = self.frontier.pop() {
                for step in [IVec3::X, IVec3::NEG_X, IVec3::Y, IVec3::NEG_Y, IVec3::Z, IVec3::NEG_Z] {
                    self.flood(world, position + step);
                }
            } else if self.columns < self.column_count() {
                let column = self.columns as i32;
                let (x, z) = (column / self.size.z, column % self.size.z);
                let surface = self.min + IVec3::new(x, SEA_LEVEL - 1, z);
                self.columns += 1;
                if self.open_to_sky(world, surface) {
                    self.flood(world, surface);
                }
            } else if let Some(&position) = chunks.get(self.compacted) {
                self.compacted += 1;
                if position.y * SIZE < SEA_LEVEL
                    && let Some(chunk) = world.chunks.get_mut(&position)
                {
                    chunk.compact();
                }
            } else {
                return;
            }
        }
    }

    fn flood(&mut self, world: &mut World, position: IVec3) {
        let local = position - self.min;
        let inside = local.cmpge(IVec3::ZERO).all() && local.cmplt(self.size).all();
        if inside && world.block(position) == Block::Air {
            world.set_block(position, Block::Water);
            self.frontier.push(position);
        }
    }

    /// Whether nothing but air lies from `position` to the top of the world. Whole chunks of air
    /// are skipped, since most of the sky is stored that way.
    fn open_to_sky(&self, world: &World, mut position: IVec3) -> bool {
        while position.y < self.height {
            let chunk = position.div_euclid(IVec3::splat(SIZE));
            if world.uniform_chunk(chunk) == Some(Block::Air) {
                position.y = (chunk.y + 1) * SIZE;
            } else if world.block(position) == Block::Air {
                position.y += 1;
            } else {
                return false;
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::chunk::Chunk;
    use crate::world::terrain::{HEIGHT, generate};

    #[test]
    fn sea_fills_open_water_and_what_connects_to_it() {
        let extent = Extent {
            half_width: 16,
            height: 4 * SIZE,
        };
        let mut world = World::default();
        for position in extent.chunk_positions() {
            world.insert(position, Chunk::empty());
        }
        let solid_below = SEA_LEVEL + 3;
        for x in -16..16 {
            for z in -16..16 {
                for y in 0..solid_below {
                    world.set_block(IVec3::new(x, y, z), Block::Stone);
                }
            }
        }
        let mut dig = |from: IVec3, to: IVec3| {
            for x in from.x..=to.x {
                for y in from.y..=to.y {
                    for z in from.z..=to.z {
                        world.set_block(IVec3::new(x, y, z), Block::Air);
                    }
                }
            }
        };
        // An open basin, a hollow reaching x = 0 joined to it by a one-block tunnel, and a sealed
        // hollow.
        dig(IVec3::new(-12, 30, -12), IVec3::new(-7, solid_below - 1, -7));
        dig(IVec3::new(-6, 32, -10), IVec3::new(-3, 32, -10));
        dig(IVec3::new(-2, 30, -12), IVec3::new(0, 33, -8));
        dig(IVec3::new(6, 30, 6), IVec3::new(9, 33, 9));
        let chunks: Vec<IVec3> = extent.chunk_positions().collect();
        let mut sea = SeaFill::new(extent);
        while !sea.done(&chunks) {
            sea.work(&mut world, &chunks);
        }

        let at = |x, y, z| world.block(IVec3::new(x, y, z));
        for y in 30..solid_below {
            let expected = if y < SEA_LEVEL { Block::Water } else { Block::Air };
            assert_eq!(at(-10, y, -10), expected, "basin at {y}");
        }
        assert_eq!(at(-4, 32, -10), Block::Water);
        assert_eq!(at(0, 30, -11), Block::Water);
        assert_eq!(at(7, 31, 7), Block::Air);
    }

    #[test]
    fn generated_sea_is_flooded_and_sealed_off() {
        let extent = Extent {
            half_width: 48,
            height: HEIGHT,
        };
        let world = generate(extent, 6);
        let (min, max) = (extent.min(), extent.max());
        let mut water = 0;
        for x in min.x..max.x {
            for z in min.z..max.z {
                for y in 0..HEIGHT {
                    let position = IVec3::new(x, y, z);
                    if world.block(position) != Block::Water {
                        continue;
                    }
                    water += 1;
                    assert!(y < SEA_LEVEL, "water at {position}");
                    for step in [IVec3::X, IVec3::NEG_X, IVec3::Y, IVec3::NEG_Y, IVec3::Z, IVec3::NEG_Z] {
                        let next = position + step;
                        let open = next.y < SEA_LEVEL && world.block(next) == Block::Air;
                        assert!(!open || !extent.contains(next), "air beside water at {next}");
                    }
                }
            }
        }
        assert!(water > 0);
    }
}
