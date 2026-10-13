mod chunk;
pub mod terrain;

use std::collections::HashMap;

use glam::IVec3;

use chunk::Chunk;
pub use chunk::{Axis, Block, CHUNK_SIZE};

const SIZE: i32 = CHUNK_SIZE as i32;

/// All loaded chunks, keyed by chunk coordinate (world block position divided by `CHUNK_SIZE`).
#[derive(Default)]
pub struct World {
    chunks: HashMap<IVec3, Chunk>,
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

    pub fn chunk_positions(&self) -> impl Iterator<Item = IVec3> + '_ {
        self.chunks.keys().copied()
    }

    /// The block at a world position; unloaded space is air.
    pub fn block(&self, position: IVec3) -> Block {
        let (chunk, local) = split(position);
        self.chunks
            .get(&chunk)
            .map_or(Block::Air, |chunk| chunk.get(local.x, local.y, local.z))
    }

    /// The height just above the highest solid block in the column at `x`, `z`, where a player
    /// can stand; 0 if the column is empty.
    pub fn column_top(&self, x: i32, z: i32) -> i32 {
        let height = self
            .chunks
            .keys()
            .map(|chunk| chunk.y + 1)
            .max()
            .unwrap_or(0)
            * SIZE;
        (0..height)
            .rev()
            .find(|&y| self.block(IVec3::new(x, y, z)) != Block::Air)
            .map_or(0, |y| y + 1)
    }

    pub fn set_block(&mut self, position: IVec3, block: Block) {
        let (chunk, local) = split(position);
        let chunk = self.chunks.get_mut(&chunk).expect("chunk is loaded");
        chunk.set(local.x, local.y, local.z, block);
    }
}

/// Splits a world block position into its chunk coordinate and the position inside that chunk.
fn split(position: IVec3) -> (IVec3, glam::USizeVec3) {
    let chunk = position.div_euclid(IVec3::splat(SIZE));
    let local = position.rem_euclid(IVec3::splat(SIZE));
    (chunk, local.as_usizevec3())
}
