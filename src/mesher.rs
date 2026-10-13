use bytemuck::{Pod, Zeroable};

use glam::IVec3;

use sim::world::{Block, CHUNK_SIZE, World};

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub uv: [f32; 2],
    /// Outward unit normal of the face, for lighting.
    pub normal: [f32; 3],
    /// Ambient occlusion brightness, 1 for an open corner.
    pub occlusion: f32,
    /// Which layer of the block texture array to sample.
    pub tile: u32,
}

/// A block face texture. Discriminants are layer indices in the renderer's texture array.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tile {
    GrassTop,
    GrassSide,
    Dirt,
    Stone,
    Cobblestone,
}

impl Tile {
    pub const ALL: [Tile; 5] = [
        Tile::GrassTop,
        Tile::GrassSide,
        Tile::Dirt,
        Tile::Stone,
        Tile::Cobblestone,
    ];

    /// The tile for one face of a block; `normal_y` picks top or bottom faces.
    fn for_face(block: Block, normal_y: i32) -> Tile {
        match (block, normal_y) {
            (Block::Grass, 1) => Tile::GrassTop,
            (Block::Grass, -1) => Tile::Dirt,
            (Block::Grass, _) => Tile::GrassSide,
            (Block::Dirt, _) => Tile::Dirt,
            (Block::Stone, _) => Tile::Stone,
            (Block::Cobblestone, _) => Tile::Cobblestone,
            (Block::Air, _) => unreachable!("air has no faces"),
        }
    }
}

/// CPU-side triangles for one chunk, in world block units, ready for the renderer to upload.
#[derive(Default)]
pub struct Mesh {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
}

struct Face {
    /// Offset to the neighbouring block this face looks at; the face is hidden when that block is solid.
    normal: IVec3,
    /// Corners of a unit block at the origin, wound bottom-left, bottom-right, top-right, top-left
    /// as seen from outside, so each face maps the texture the same way up.
    corners: [[f32; 3]; 4],
}

#[rustfmt::skip]
const FACES: [Face; 6] = [
    Face { normal: IVec3::new(1, 0, 0), corners: [[1., 0., 1.], [1., 0., 0.], [1., 1., 0.], [1., 1., 1.]] },
    Face { normal: IVec3::new(-1, 0, 0), corners: [[0., 0., 0.], [0., 0., 1.], [0., 1., 1.], [0., 1., 0.]] },
    Face { normal: IVec3::new(0, 1, 0), corners: [[0., 1., 1.], [1., 1., 1.], [1., 1., 0.], [0., 1., 0.]] },
    Face { normal: IVec3::new(0, -1, 0), corners: [[0., 0., 0.], [1., 0., 0.], [1., 0., 1.], [0., 0., 1.]] },
    Face { normal: IVec3::new(0, 0, 1), corners: [[0., 0., 1.], [1., 0., 1.], [1., 1., 1.], [0., 1., 1.]] },
    Face { normal: IVec3::new(0, 0, -1), corners: [[1., 0., 0.], [0., 0., 0.], [0., 1., 0.], [1., 1., 0.]] },
];

const UVS: [[f32; 2]; 4] = [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];

/// Blocks along each edge of a chunk plus a one-block border on every side.
const PADDED: i32 = CHUNK_SIZE as i32 + 2;

/// One chunk's blocks and the border around it, copied out of the world once. Meshing reads every
/// block several times over (face visibility and twelve AO samples per face), and a flat array
/// read is far cheaper than finding the block's chunk in the world each time.
struct Snapshot {
    /// World position of the first block in `blocks`, one block below and behind the chunk.
    origin: IVec3,
    blocks: Vec<Block>,
}

impl Snapshot {
    fn new(world: &World, chunk: IVec3) -> Self {
        let origin = chunk * CHUNK_SIZE as i32 - IVec3::ONE;
        let mut blocks = Vec::with_capacity((PADDED * PADDED * PADDED) as usize);
        for y in 0..PADDED {
            for z in 0..PADDED {
                for x in 0..PADDED {
                    blocks.push(world.block(origin + IVec3::new(x, y, z)));
                }
            }
        }
        Self { origin, blocks }
    }

    /// The block at a world position inside the chunk or its border.
    fn block(&self, position: IVec3) -> Block {
        let local = position - self.origin;
        self.blocks[((local.y * PADDED + local.z) * PADDED + local.x) as usize]
    }
}

/// Builds the mesh for one chunk, emitting only faces that border air. Neighbours are read
/// from the world so faces between touching chunks are culled too.
pub fn build_chunk_mesh(world: &World, chunk: IVec3) -> Mesh {
    let blocks = Snapshot::new(world, chunk);
    let mut mesh = Mesh::default();
    let origin = chunk * CHUNK_SIZE as i32;
    for y in 0..CHUNK_SIZE as i32 {
        for z in 0..CHUNK_SIZE as i32 {
            for x in 0..CHUNK_SIZE as i32 {
                let position = origin + IVec3::new(x, y, z);
                let block = blocks.block(position);
                if block == Block::Air {
                    continue;
                }
                for face in &FACES {
                    if blocks.block(position + face.normal) == Block::Air {
                        let tile = Tile::for_face(block, face.normal.y);
                        let occlusion = face_occlusion(&blocks, position, face);
                        push_face(&mut mesh, face, position, tile, occlusion);
                    }
                }
            }
        }
    }
    mesh
}

/// Brightness for a vertex touched by 0, 1, 2 or 3 solid neighbours.
const OCCLUSION_BRIGHTNESS: [f32; 4] = [1.0, 0.8, 0.65, 0.5];

/// How many of the three blocks around each corner of a face are solid, looking from the air
/// cell in front of the face. Two solid sides fully enclose the corner whatever the diagonal is.
fn face_occlusion(blocks: &Snapshot, position: IVec3, face: &Face) -> [usize; 4] {
    let in_front = position + face.normal;
    let solid = |offset: IVec3| blocks.block(in_front + offset) != Block::Air;
    face.corners.map(|corner| {
        // Step towards the corner along each axis the face lies in (the ones the normal ignores).
        let toward = IVec3::from_array(corner.map(|c| if c > 0.5 { 1 } else { -1 }));
        let mut sides = [IVec3::ZERO; 2];
        for (side, axis) in sides
            .iter_mut()
            .zip((0..3).filter(|&a| face.normal[a] == 0))
        {
            side[axis] = toward[axis];
        }
        let (side_a, side_b) = (solid(sides[0]), solid(sides[1]));
        if side_a && side_b {
            3
        } else {
            usize::from(side_a) + usize::from(side_b) + usize::from(solid(sides[0] + sides[1]))
        }
    })
}

fn push_face(mesh: &mut Mesh, face: &Face, position: IVec3, tile: Tile, occlusion: [usize; 4]) {
    let base = mesh.vertices.len() as u32;
    let origin = position.as_vec3();
    for ((corner, uv), occluders) in face.corners.iter().zip(UVS).zip(occlusion) {
        mesh.vertices.push(Vertex {
            position: (origin + glam::Vec3::from_array(*corner)).to_array(),
            uv,
            normal: face.normal.as_vec3().to_array(),
            occlusion: OCCLUSION_BRIGHTNESS[occluders],
            tile: tile as u32,
        });
    }
    // Split the quad along the diagonal whose ends are equally lit, otherwise the darkness
    // interpolates across the wrong triangle and shows a visible crease.
    let [a, b, c, d] = [base, base + 1, base + 2, base + 3];
    if occlusion[0] + occlusion[2] > occlusion[1] + occlusion[3] {
        mesh.indices.extend([b, c, d, b, d, a]);
    } else {
        mesh.indices.extend([a, b, c, a, c, d]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn face_count(world: &World, chunk: IVec3) -> usize {
        build_chunk_mesh(world, chunk).indices.len() / 6
    }

    #[test]
    fn lone_block_has_six_faces() {
        let mut world = World::empty(IVec3::ONE);
        world.set_block(IVec3::new(3, 3, 3), Block::Stone);
        assert_eq!(face_count(&world, IVec3::ZERO), 6);
    }

    #[test]
    fn touching_faces_are_culled() {
        let mut world = World::empty(IVec3::ONE);
        world.set_block(IVec3::new(3, 3, 3), Block::Stone);
        world.set_block(IVec3::new(4, 3, 3), Block::Stone);
        assert_eq!(face_count(&world, IVec3::ZERO), 10);
    }

    #[test]
    fn faces_between_chunks_are_culled() {
        let mut world = World::empty(IVec3::new(2, 1, 1));
        world.set_block(IVec3::new(15, 0, 0), Block::Stone);
        world.set_block(IVec3::new(16, 0, 0), Block::Stone);
        assert_eq!(face_count(&world, IVec3::ZERO), 5);
        assert_eq!(face_count(&world, IVec3::X), 5);
    }

    #[test]
    fn corner_against_a_wall_is_occluded() {
        let mut world = World::empty(IVec3::ONE);
        let floor = IVec3::new(3, 3, 3);
        world.set_block(floor, Block::Stone);
        // A block sitting beside the floor's top face, against its +X edge.
        world.set_block(floor + IVec3::new(1, 1, 0), Block::Stone);
        let top = &FACES[2];
        // Top face corners are (0,1,1), (1,1,1), (1,1,0), (0,1,0): the +X pair touch the wall.
        let blocks = Snapshot::new(&world, IVec3::ZERO);
        assert_eq!(face_occlusion(&blocks, floor, top), [0, 1, 1, 0]);
    }
}
