/// Chunks are cubes of blocks this many blocks along each edge.
pub const CHUNK_SIZE: usize = 16;
const VOLUME: usize = CHUNK_SIZE * CHUNK_SIZE * CHUNK_SIZE;

/// What occupies one block cell. Distinct block types arrive with step 6.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Block {
    #[default]
    Air,
    Solid,
}

/// A fixed-size cube of blocks addressed by local coordinates in `0..CHUNK_SIZE`.
pub struct Chunk {
    blocks: Box<[Block; VOLUME]>,
}

impl Chunk {
    pub fn empty() -> Self {
        Self {
            blocks: Box::new([Block::Air; VOLUME]),
        }
    }

    pub fn get(&self, x: usize, y: usize, z: usize) -> Block {
        self.blocks[index(x, y, z)]
    }

    pub fn set(&mut self, x: usize, y: usize, z: usize, block: Block) {
        self.blocks[index(x, y, z)] = block;
    }

    /// The block at a signed local position, treating anything outside the chunk as air.
    pub fn get_or_air(&self, x: i32, y: i32, z: i32) -> Block {
        let size = CHUNK_SIZE as i32;
        if (0..size).contains(&x) && (0..size).contains(&y) && (0..size).contains(&z) {
            self.get(x as usize, y as usize, z as usize)
        } else {
            Block::Air
        }
    }
}

/// Y-major layout so horizontal layers are contiguous.
fn index(x: usize, y: usize, z: usize) -> usize {
    debug_assert!(x < CHUNK_SIZE && y < CHUNK_SIZE && z < CHUNK_SIZE);
    (y * CHUNK_SIZE + z) * CHUNK_SIZE + x
}
