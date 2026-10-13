/// Chunks are cubes of blocks this many blocks along each edge.
pub const CHUNK_SIZE: usize = 16;
const VOLUME: usize = CHUNK_SIZE * CHUNK_SIZE * CHUNK_SIZE;

/// What occupies one block cell.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum Block {
    #[default]
    Air,
    Grass,
    Dirt,
    Stone,
    Cobblestone,
    Sand,
    Gravel,
    Snow,
    OakLog(Axis),
    OakLeaves,
    /// Still sea water, which bodies pass through.
    Water,
    /// The barrier round the edge of the world: solid, full height, and drawn as a faint shield
    /// rather than as blocks.
    WorldBorder,
}

impl Block {
    /// Whether bodies collide with it and it hides what is behind it.
    pub fn is_solid(self) -> bool {
        !matches!(self, Block::Air | Block::Water)
    }
}

/// The world axis a log's grain runs along: `Y` for an upright trunk.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Axis {
    X,
    Y,
    Z,
}

/// A fixed-size cube of blocks addressed by local coordinates in `0..CHUNK_SIZE`.
#[derive(PartialEq)]
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
}

/// Y-major layout so horizontal layers are contiguous.
fn index(x: usize, y: usize, z: usize) -> usize {
    debug_assert!(x < CHUNK_SIZE && y < CHUNK_SIZE && z < CHUNK_SIZE);
    (y * CHUNK_SIZE + z) * CHUNK_SIZE + x
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_stay_small() {
        assert!(size_of::<Block>() <= 2);
    }
}
