use bytemuck::{Pod, Zeroable};

use glam::IVec3;

use crate::world::{Block, CHUNK_SIZE, World};

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub uv: [f32; 2],
    pub shade: f32,
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
    /// Fixed brightness so blocks read as 3D before there is any lighting.
    shade: f32,
}

#[rustfmt::skip]
const FACES: [Face; 6] = [
    Face { normal: IVec3::new(1, 0, 0), corners: [[1., 0., 1.], [1., 0., 0.], [1., 1., 0.], [1., 1., 1.]], shade: 0.8 },
    Face { normal: IVec3::new(-1, 0, 0), corners: [[0., 0., 0.], [0., 0., 1.], [0., 1., 1.], [0., 1., 0.]], shade: 0.8 },
    Face { normal: IVec3::new(0, 1, 0), corners: [[0., 1., 1.], [1., 1., 1.], [1., 1., 0.], [0., 1., 0.]], shade: 1.0 },
    Face { normal: IVec3::new(0, -1, 0), corners: [[0., 0., 0.], [1., 0., 0.], [1., 0., 1.], [0., 0., 1.]], shade: 0.5 },
    Face { normal: IVec3::new(0, 0, 1), corners: [[0., 0., 1.], [1., 0., 1.], [1., 1., 1.], [0., 1., 1.]], shade: 0.65 },
    Face { normal: IVec3::new(0, 0, -1), corners: [[1., 0., 0.], [0., 0., 0.], [0., 1., 0.], [1., 1., 0.]], shade: 0.65 },
];

const UVS: [[f32; 2]; 4] = [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];

/// Builds the mesh for one chunk, emitting only faces that border air. Neighbours are read
/// from the world so faces between touching chunks are culled too.
pub fn build_chunk_mesh(world: &World, chunk: IVec3) -> Mesh {
    let mut mesh = Mesh::default();
    let origin = chunk * CHUNK_SIZE as i32;
    for y in 0..CHUNK_SIZE as i32 {
        for z in 0..CHUNK_SIZE as i32 {
            for x in 0..CHUNK_SIZE as i32 {
                let position = origin + IVec3::new(x, y, z);
                let block = world.block(position);
                if block == Block::Air {
                    continue;
                }
                for face in &FACES {
                    if world.block(position + face.normal) == Block::Air {
                        let tile = Tile::for_face(block, face.normal.y);
                        push_face(&mut mesh, face, position.as_vec3().to_array(), tile);
                    }
                }
            }
        }
    }
    mesh
}

fn push_face(mesh: &mut Mesh, face: &Face, origin: [f32; 3], tile: Tile) {
    let base = mesh.vertices.len() as u32;
    for (corner, uv) in face.corners.iter().zip(UVS) {
        mesh.vertices.push(Vertex {
            position: [
                origin[0] + corner[0],
                origin[1] + corner[1],
                origin[2] + corner[2],
            ],
            uv,
            shade: face.shade,
            tile: tile as u32,
        });
    }
    // Two counter-clockwise triangles.
    mesh.indices
        .extend([base, base + 1, base + 2, base, base + 2, base + 3]);
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
}
