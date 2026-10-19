use glam::Vec3;

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
    OakPlanks,
    /// Steps up towards the way it faces.
    OakStairs(Facing),
    CraftingTable,
    /// Still sea water, which bodies pass through.
    Water,
    /// The barrier round the edge of the world: solid, full height, and drawn as a faint shield
    /// rather than as blocks.
    WorldBorder,
}

/// A box filling part of a block cell, as its lowest and highest corners within the cell's unit cube.
pub type Part = [Vec3; 2];

const CUBE: &[Part] = &[[Vec3::ZERO, Vec3::ONE]];
const SLAB: Part = [Vec3::ZERO, Vec3::new(1.0, 0.5, 1.0)];
/// A slab, and the half above it towards the way they face: north, east, south, west.
const STAIRS: [&[Part]; 4] = [
    &[SLAB, [Vec3::new(0.0, 0.5, 0.0), Vec3::new(1.0, 1.0, 0.5)]],
    &[SLAB, [Vec3::new(0.5, 0.5, 0.0), Vec3::ONE]],
    &[SLAB, [Vec3::new(0.0, 0.5, 0.5), Vec3::ONE]],
    &[SLAB, [Vec3::new(0.0, 0.5, 0.0), Vec3::new(0.5, 1.0, 1.0)]],
];

impl Block {
    /// Whether bodies collide with it and rays stop at it.
    pub fn is_solid(self) -> bool {
        !matches!(self, Block::Air | Block::Water)
    }

    /// Whether it fills its whole cell, hiding the faces of blocks against it.
    pub fn is_opaque(self) -> bool {
        self.is_solid() && !matches!(self, Block::OakStairs(_))
    }

    /// The boxes a solid block fills, which bodies collide with and rays hit.
    pub fn shape(self) -> &'static [Part] {
        match self {
            Block::OakStairs(facing) => STAIRS[facing as usize],
            block if block.is_solid() => CUBE,
            _ => &[],
        }
    }
}

/// A horizontal direction, as Minecraft names them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Facing {
    /// -Z
    North,
    /// +X
    East,
    /// +Z
    South,
    /// -X
    West,
}

impl Facing {
    /// The way most of `direction` points across the ground.
    pub fn of(direction: Vec3) -> Facing {
        if direction.x.abs() > direction.z.abs() {
            if direction.x > 0.0 { Facing::East } else { Facing::West }
        } else if direction.z > 0.0 {
            Facing::South
        } else {
            Facing::North
        }
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
    fn stairs_fill_a_slab_and_the_half_above_it_they_face() {
        for (facing, toward) in [
            (Facing::North, Vec3::NEG_Z),
            (Facing::East, Vec3::X),
            (Facing::South, Vec3::Z),
            (Facing::West, Vec3::NEG_X),
        ] {
            assert_eq!(Facing::of(toward + Vec3::new(0.2, -0.9, 0.2)), facing);
            let [slab, step] = Block::OakStairs(facing).shape() else {
                panic!("stairs are two boxes");
            };
            assert_eq!(*slab, SLAB);
            let centre = (step[0] + step[1]) / 2.0;
            assert_eq!(centre, Vec3::new(0.5, 0.75, 0.5) + toward / 4.0);
            assert_eq!((step[1] - step[0]).element_product(), 0.25);
        }
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
                    let expected = if (x, y, z) == (4, 5, 6) { Block::Air } else { Block::Stone };
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
