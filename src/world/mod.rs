mod chunk;

use std::collections::HashMap;

use glam::IVec3;

use chunk::Chunk;
pub use chunk::{Block, CHUNK_SIZE};

const SIZE: i32 = CHUNK_SIZE as i32;

/// All loaded chunks, keyed by chunk coordinate (world block position divided by `CHUNK_SIZE`).
#[derive(Default)]
pub struct World {
    chunks: HashMap<IVec3, Chunk>,
}

impl World {
    /// Flat ground `ground_height` blocks deep with a `wall_height` wall around the edge,
    /// spanning `chunks` from the origin.
    pub fn flat_with_walls(chunks: IVec3, ground_height: i32, wall_height: i32) -> Self {
        let mut world = Self::empty(chunks);
        let extent = chunks * SIZE;
        for x in 0..extent.x {
            for z in 0..extent.z {
                let on_edge = x == 0 || z == 0 || x == extent.x - 1 || z == extent.z - 1;
                let height = if on_edge { wall_height } else { ground_height };
                for y in 0..height.min(extent.y) {
                    world.set_block(IVec3::new(x, y, z), Block::Solid);
                }
            }
        }
        world
    }

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
