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

/// A fixed-size cube of blocks addressed by local coordinates in `0..CHUNK_SIZE`. Most of a world
/// is open sky or solid rock, so a chunk that is one block throughout is stored as just that
/// block.
#[derive(PartialEq)]
pub enum Chunk {
    Uniform(Block),
    Mixed(Box<[Block; VOLUME]>),
}

impl Chunk {
    pub fn empty() -> Self {
        Chunk::Uniform(Block::Air)
    }

    pub fn get(&self, x: usize, y: usize, z: usize) -> Block {
        match self {
            Chunk::Uniform(block) => *block,
            Chunk::Mixed(blocks) => blocks[index(x, y, z)],
        }
    }

    pub fn set(&mut self, x: usize, y: usize, z: usize, block: Block) {
        if let Chunk::Uniform(all) = *self {
            if all == block {
                return;
            }
            *self = Chunk::Mixed(Box::new([all; VOLUME]));
        }
        if let Chunk::Mixed(blocks) = self {
            blocks[index(x, y, z)] = block;
        }
    }

    /// Stores the chunk as one block if every cell holds the same one. Edits don't do this
    /// themselves, since a chunk being built up would flip back and forth.
    pub fn compact(&mut self) {
        if let Chunk::Mixed(blocks) = self
            && blocks.iter().all(|&block| block == blocks[0])
        {
            *self = Chunk::Uniform(blocks[0]);
        }
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

    #[test]
    fn a_uniform_chunk_reads_its_block_everywhere() {
        let chunk = Chunk::Uniform(Block::Stone);
        for (x, y, z) in [(0, 0, 0), (15, 15, 15), (3, 9, 12)] {
            assert_eq!(chunk.get(x, y, z), Block::Stone);
        }
    }

    #[test]
    fn writing_the_same_block_keeps_a_chunk_uniform() {
        let mut chunk = Chunk::Uniform(Block::Water);
        chunk.set(4, 5, 6, Block::Water);
        assert!(chunk == Chunk::Uniform(Block::Water));
    }

    #[test]
    fn writing_another_block_makes_a_chunk_mixed_and_keeps_the_rest() {
        let mut chunk = Chunk::Uniform(Block::Stone);
        chunk.set(4, 5, 6, Block::Air);
        assert!(matches!(chunk, Chunk::Mixed(_)));
        for x in 0..CHUNK_SIZE {
            for y in 0..CHUNK_SIZE {
                for z in 0..CHUNK_SIZE {
                    let expected = if (x, y, z) == (4, 5, 6) {
                        Block::Air
                    } else {
                        Block::Stone
                    };
                    assert_eq!(chunk.get(x, y, z), expected);
                }
            }
        }
        chunk.set(4, 5, 6, Block::Stone);
        assert!(matches!(chunk, Chunk::Mixed(_)));
        chunk.compact();
        assert!(chunk == Chunk::Uniform(Block::Stone));
    }
}
