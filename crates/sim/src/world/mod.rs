mod chunk;
mod ray;
pub mod terrain;

use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasherDefault, Hasher};

use glam::{IVec2, IVec3, Vec3Swizzles};

use chunk::Chunk;
pub use chunk::{Axis, Block, CHUNK_SIZE, Facing, Part};
pub use ray::Hit;

const SIZE: i32 = CHUNK_SIZE as i32;

/// How far the world reaches: `-half_width..half_width` in x and z, centred on the origin, and
/// `0..height` upwards. Its outermost columns are the world border.
#[derive(Clone, Copy, Debug)]
pub struct Extent {
    pub half_width: i32,
    pub height: i32,
}

impl Extent {
    /// The lowest block corner, inclusive.
    pub fn min(self) -> IVec3 {
        IVec3::new(-self.half_width, 0, -self.half_width)
    }

    /// The highest block corner, exclusive.
    pub fn max(self) -> IVec3 {
        IVec3::new(self.half_width, self.height, self.half_width)
    }

    pub fn contains(self, position: IVec3) -> bool {
        position.cmpge(self.min()).all() && position.cmplt(self.max()).all()
    }

    pub fn contains_column(self, x: i32, z: i32) -> bool {
        self.contains(IVec3::new(x, 0, z))
    }

    /// Whether the column is in the outermost ring, where the border stands.
    pub fn on_edge(self, x: i32, z: i32) -> bool {
        let (low, high) = (-self.half_width, self.half_width - 1);
        self.contains_column(x, z) && (x == low || z == low || x == high || z == high)
    }

    /// Every chunk holding at least one block of the world. The outermost ones stick out past
    /// it when the half-width isn't a whole number of chunks.
    pub fn chunk_positions(self) -> impl Iterator<Item = IVec3> {
        self.chunk_columns()
            .flat_map(move |column| self.chunk_levels().map(move |y| IVec3::new(column.x, y, column.y)))
    }

    /// The x and z chunk coordinates of every column of chunks in the world.
    pub fn chunk_columns(self) -> impl Iterator<Item = IVec2> {
        let first = self.min().xz().div_euclid(IVec2::splat(SIZE));
        let last = (self.max().xz() - 1).div_euclid(IVec2::splat(SIZE));
        (first.x..=last.x).flat_map(move |x| (first.y..=last.y).map(move |z| IVec2::new(x, z)))
    }

    /// The chunk y coordinates of every column, bottom to top.
    pub fn chunk_levels(self) -> std::ops::RangeInclusive<i32> {
        self.min().y.div_euclid(SIZE)..=(self.max().y - 1).div_euclid(SIZE)
    }
}

/// All loaded chunks, keyed by chunk coordinate (world block position divided by `CHUNK_SIZE`).
#[derive(Default)]
pub struct World {
    chunks: ChunkMap<Chunk>,
    /// Blocks changed by `set_block` since the last `take_edits`.
    edited: Vec<IVec3>,
}

/// A map keyed by chunk coordinate, hashed cheaply.
pub type ChunkMap<V> = HashMap<IVec3, V, BuildHasherDefault<ChunkHasher>>;
/// A set of chunk coordinates, hashed cheaply.
pub type ChunkSet = HashSet<IVec3, BuildHasherDefault<ChunkHasher>>;

/// Hashes chunk coordinates with a rotate, xor and multiply per coordinate, as rustc's own hasher
/// does. Every block read finds its chunk through the map, and the default SipHash, built to
/// resist crafted keys, cost more than the rest of the read.
#[derive(Default)]
pub struct ChunkHasher(u64);

impl Hasher for ChunkHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.write_u64(u64::from(byte));
        }
    }

    fn write_i32(&mut self, coordinate: i32) {
        self.write_u64(u64::from(coordinate as u32));
    }

    fn write_u64(&mut self, value: u64) {
        self.0 = (self.0.rotate_left(5) ^ value).wrapping_mul(0x517c_c1b7_2722_0a95);
    }
}

impl World {
    /// Empty chunks spanning `chunks` from the origin.
    pub fn empty(chunks: IVec3) -> Self {
        let mut world = Self::default();
        for cx in 0..chunks.x {
            for cy in 0..chunks.y {
                for cz in 0..chunks.z {
                    world.chunks.insert(IVec3::new(cx, cy, cz), Chunk::empty());
                }
            }
        }
        world
    }

    fn insert(&mut self, position: IVec3, chunk: Chunk) {
        self.chunks.insert(position, chunk);
    }

    /// Whether the chunk holding `position` is loaded, so a block can be set there.
    pub fn is_loaded(&self, position: IVec3) -> bool {
        self.chunks.contains_key(&split(position).0)
    }

    /// The block at a world position; unloaded space is air.
    pub fn block(&self, position: IVec3) -> Block {
        let (chunk, local) = split(position);
        self.chunks
            .get(&chunk)
            .map_or(Block::Air, |chunk| chunk.get(local.x, local.y, local.z))
    }

    /// The blocks of the box from `min` up to but not including `max`, layer by layer from the
    /// bottom, each layer row by row along z, each row along x. Unloaded space is air. Each chunk
    /// is looked up once rather than once per block.
    pub fn blocks_in(&self, min: IVec3, max: IVec3) -> Vec<Block> {
        let size = max - min;
        let mut blocks = vec![Block::Air; (size.x * size.y * size.z) as usize];
        let first = min.div_euclid(IVec3::splat(SIZE));
        let last = (max - 1).div_euclid(IVec3::splat(SIZE));
        for cx in first.x..=last.x {
            for cy in first.y..=last.y {
                for cz in first.z..=last.z {
                    let position = IVec3::new(cx, cy, cz);
                    let Some(chunk) = self.chunks.get(&position) else {
                        continue;
                    };
                    let origin = position * SIZE;
                    let (low, high) = (origin.max(min), (origin + SIZE).min(max));
                    for y in low.y..high.y {
                        for z in low.z..high.z {
                            for x in low.x..high.x {
                                let (cell, local) = (IVec3::new(x, y, z) - min, IVec3::new(x, y, z) - origin);
                                let local = local.as_usizevec3();
                                blocks[((cell.y * size.z + cell.z) * size.x + cell.x) as usize] = chunk.get(local.x, local.y, local.z);
                            }
                        }
                    }
                }
            }
        }
        blocks
    }

    /// The height just above the highest solid block in the column at `x`, `z`, where a player
    /// can stand; 0 if the column is empty.
    pub fn column_top(&self, x: i32, z: i32) -> i32 {
        let height = self.chunks.keys().map(|chunk| chunk.y + 1).max().unwrap_or(0) * SIZE;
        (0..height)
            .rev()
            .find(|&y| self.block(IVec3::new(x, y, z)).is_solid())
            .map_or(0, |y| y + 1)
    }

    /// Whether any block above `position` in its column passes `counts`. Unloaded space is air, and a chunk one block fills
    /// is checked once.
    pub fn any_above(&self, position: IVec3, counts: impl Fn(Block) -> bool) -> bool {
        let (mut chunk, local) = split(position);
        let mut from = local.y + 1;
        while let Some(stored) = self.chunks.get(&chunk) {
            let found = match stored {
                Chunk::Uniform(block) => from < CHUNK_SIZE && counts(*block),
                Chunk::Mixed(_) => (from..CHUNK_SIZE).any(|y| counts(stored.get(local.x, y, local.z))),
            };
            if found {
                return true;
            }
            chunk.y += 1;
            from = 0;
        }
        false
    }

    /// The block filling the chunk at chunk coordinate `chunk`, if one block fills all of it.
    pub fn uniform_chunk(&self, chunk: IVec3) -> Option<Block> {
        match self.chunks.get(&chunk)? {
            Chunk::Uniform(block) => Some(*block),
            Chunk::Mixed(_) => None,
        }
    }

    pub fn set_block(&mut self, position: IVec3, block: Block) {
        let (chunk, local) = split(position);
        let chunk = self.chunks.get_mut(&chunk).expect("chunk is loaded");
        if chunk.get(local.x, local.y, local.z) != block {
            chunk.set(local.x, local.y, local.z, block);
            self.edited.push(position);
        }
    }

    /// The chunks whose look the edits since the last call may have changed: each edited block's own chunk and every chunk
    /// touching that block, since faces and shading reach across chunk borders. Edited chunks left one block throughout are
    /// stored as just that block again.
    pub fn take_edits(&mut self) -> ChunkSet {
        let edited = std::mem::take(&mut self.edited);
        let edited_chunks: ChunkSet = edited.iter().map(|&position| split(position).0).collect();
        for chunk in edited_chunks {
            if let Some(chunk) = self.chunks.get_mut(&chunk) {
                chunk.compact();
            }
        }
        let mut touched = ChunkSet::default();
        for position in edited {
            for offset in (-1..=1).flat_map(|x| (-1..=1).flat_map(move |y| (-1..=1).map(move |z| IVec3::new(x, y, z)))) {
                touched.insert(split(position + offset).0);
            }
        }
        touched
    }
}

/// Splits a world block position into its chunk coordinate and the position inside that chunk.
fn split(position: IVec3) -> (IVec3, glam::USizeVec3) {
    let chunk = position.div_euclid(IVec3::splat(SIZE));
    let local = position.rem_euclid(IVec3::splat(SIZE));
    (chunk, local.as_usizevec3())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_read_back_on_both_sides_of_the_origin() {
        let extent = Extent {
            half_width: 32,
            height: 2 * SIZE,
        };
        let mut world = World::default();
        for position in extent.chunk_positions() {
            world.insert(position, Chunk::empty());
        }
        let placed = [
            (IVec3::new(-1, 5, -1), Block::Stone),
            (IVec3::new(0, 6, 0), Block::Dirt),
            (IVec3::new(-17, 20, 15), Block::Sand),
            (IVec3::new(-32, 0, 31), Block::Gravel),
        ];
        for (position, block) in placed {
            world.set_block(position, block);
        }
        for (position, block) in placed {
            assert_eq!(world.block(position), block, "{position}");
            assert_eq!(world.column_top(position.x, position.z), position.y + 1);
        }
        assert_eq!(world.block(IVec3::new(-1, 5, 0)), Block::Air);
        assert_eq!(world.block(IVec3::new(0, 5, -1)), Block::Air);
        assert_eq!(world.column_top(-2, -2), 0);
    }

    #[test]
    fn blocks_above_are_found_in_higher_chunks_and_not_below() {
        let mut world = World::empty(IVec3::new(1, 3, 1));
        world.insert(IVec3::new(0, 1, 0), Chunk::Uniform(Block::Stone));
        world.set_block(IVec3::new(2, 40, 3), Block::Dirt);
        let stone = |block| block == Block::Stone;
        let dirt = |block| block == Block::Dirt;
        assert!(world.any_above(IVec3::new(2, 3, 3), stone));
        assert!(world.any_above(IVec3::new(2, 3, 3), dirt));
        assert!(world.any_above(IVec3::new(2, 39, 3), dirt));
        assert!(!world.any_above(IVec3::new(2, 40, 3), dirt));
        assert!(!world.any_above(IVec3::new(5, 3, 3), dirt));
        // Within the uniform chunk only the cells above count, and there are none above its top row.
        assert!(world.any_above(IVec3::new(5, 30, 3), stone));
        assert!(!world.any_above(IVec3::new(5, 31, 3), stone));
    }

    #[test]
    fn a_box_of_blocks_reads_the_same_as_each_block() {
        let mut world = World::default();
        for position in [IVec3::new(-1, 0, -1), IVec3::new(0, 0, -1), IVec3::new(-1, 1, 0)] {
            world.insert(position, Chunk::empty());
        }
        world.insert(IVec3::new(0, 1, 0), Chunk::Uniform(Block::Stone));
        for (i, position) in [IVec3::new(-3, 5, -2), IVec3::new(2, 7, -9), IVec3::new(-1, 20, 3)]
            .into_iter()
            .enumerate()
        {
            let block = [Block::Dirt, Block::Sand, Block::Gravel][i];
            world.set_block(position, block);
        }
        let (min, max) = (IVec3::new(-5, 3, -10), IVec3::new(4, 22, 6));
        let blocks = world.blocks_in(min, max);
        let size = max - min;
        assert_eq!(blocks.len(), (size.x * size.y * size.z) as usize);
        let mut index = 0;
        for y in min.y..max.y {
            for z in min.z..max.z {
                for x in min.x..max.x {
                    let position = IVec3::new(x, y, z);
                    assert_eq!(blocks[index], world.block(position), "{position}");
                    index += 1;
                }
            }
        }
    }

    #[test]
    fn edits_name_the_chunks_they_touch_and_compact_them() {
        let mut world = World::empty(IVec3::new(2, 2, 2));
        world.set_block(IVec3::new(5, 6, 7), Block::Stone);
        let only_its_own: ChunkSet = [IVec3::ZERO].into_iter().collect();
        assert_eq!(world.take_edits(), only_its_own);
        assert!(world.take_edits().is_empty(), "taking clears them");

        // A block at a chunk corner touches the eight chunks round that corner.
        world.set_block(IVec3::new(15, 15, 15), Block::Dirt);
        let corner = world.take_edits();
        assert_eq!(corner.len(), 8);
        assert!(corner.contains(&IVec3::ONE) && corner.contains(&IVec3::ZERO));

        // Writing what is already there is no edit.
        world.set_block(IVec3::new(5, 6, 7), Block::Stone);
        assert!(world.take_edits().is_empty());

        // Undoing both edits leaves the chunk all air, stored as such once the edits are taken.
        world.set_block(IVec3::new(5, 6, 7), Block::Air);
        world.set_block(IVec3::new(15, 15, 15), Block::Air);
        assert_eq!(world.uniform_chunk(IVec3::ZERO), None);
        world.take_edits();
        assert_eq!(world.uniform_chunk(IVec3::ZERO), Some(Block::Air));
    }

    #[test]
    fn extent_chunks_cover_the_world_and_no_more() {
        let extent = Extent {
            half_width: 24,
            height: 3 * SIZE,
        };
        let chunks: Vec<IVec3> = extent.chunk_positions().collect();
        assert_eq!(chunks.len(), 4 * 3 * 4);
        let low = chunks.iter().fold(IVec3::MAX, |a, &b| a.min(b));
        let high = chunks.iter().fold(IVec3::MIN, |a, &b| a.max(b));
        assert_eq!((low, high), (IVec3::new(-2, 0, -2), IVec3::new(1, 2, 1)));
        assert!(extent.on_edge(-24, 0) && extent.on_edge(5, 23));
        assert!(!extent.on_edge(-23, 0) && !extent.on_edge(-25, 0) && !extent.on_edge(24, 0));
    }
}
