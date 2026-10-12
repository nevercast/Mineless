use bytemuck::{Pod, Zeroable};

use crate::chunk::{Block, CHUNK_SIZE, Chunk};

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub uv: [f32; 2],
    pub shade: f32,
}

impl Vertex {
    pub const ATTRIBUTES: [wgpu::VertexAttribute; 3] =
        wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Float32];

    pub fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBUTES,
        }
    }
}

/// Triangles ready to upload, in chunk-local block units.
#[derive(Default)]
pub struct Mesh {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
}

struct Face {
    /// Offset to the neighbouring block this face looks at; the face is hidden when that block is solid.
    normal: [i32; 3],
    /// Corners of a unit block at the origin, wound bottom-left, bottom-right, top-right, top-left
    /// as seen from outside, so each face maps the texture the same way up.
    corners: [[f32; 3]; 4],
    /// Fixed brightness so blocks read as 3D before there is any lighting.
    shade: f32,
}

#[rustfmt::skip]
const FACES: [Face; 6] = [
    Face { normal: [ 1, 0, 0], corners: [[1., 0., 1.], [1., 0., 0.], [1., 1., 0.], [1., 1., 1.]], shade: 0.8 },
    Face { normal: [-1, 0, 0], corners: [[0., 0., 0.], [0., 0., 1.], [0., 1., 1.], [0., 1., 0.]], shade: 0.8 },
    Face { normal: [ 0, 1, 0], corners: [[0., 1., 1.], [1., 1., 1.], [1., 1., 0.], [0., 1., 0.]], shade: 1.0 },
    Face { normal: [ 0,-1, 0], corners: [[0., 0., 0.], [1., 0., 0.], [1., 0., 1.], [0., 0., 1.]], shade: 0.5 },
    Face { normal: [ 0, 0, 1], corners: [[0., 0., 1.], [1., 0., 1.], [1., 1., 1.], [0., 1., 1.]], shade: 0.65 },
    Face { normal: [ 0, 0,-1], corners: [[1., 0., 0.], [0., 0., 0.], [0., 1., 0.], [1., 1., 0.]], shade: 0.65 },
];

const UVS: [[f32; 2]; 4] = [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];

/// Builds one mesh for the whole chunk, emitting only faces that border air.
pub fn build_chunk_mesh(chunk: &Chunk) -> Mesh {
    let mut mesh = Mesh::default();
    for y in 0..CHUNK_SIZE {
        for z in 0..CHUNK_SIZE {
            for x in 0..CHUNK_SIZE {
                if chunk.get(x, y, z) == Block::Air {
                    continue;
                }
                let (bx, by, bz) = (x as i32, y as i32, z as i32);
                for face in &FACES {
                    let [nx, ny, nz] = face.normal;
                    if chunk.get_or_air(bx + nx, by + ny, bz + nz) != Block::Air {
                        continue;
                    }
                    push_face(&mut mesh, face, [x as f32, y as f32, z as f32]);
                }
            }
        }
    }
    mesh
}

fn push_face(mesh: &mut Mesh, face: &Face, origin: [f32; 3]) {
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
        });
    }
    // Two counter-clockwise triangles.
    mesh.indices
        .extend([base, base + 1, base + 2, base, base + 2, base + 3]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lone_block_has_six_faces() {
        let mut chunk = Chunk::empty();
        chunk.set(3, 3, 3, Block::Solid);
        assert_eq!(build_chunk_mesh(&chunk).indices.len(), 6 * 6);
    }

    #[test]
    fn touching_faces_are_culled() {
        let mut chunk = Chunk::empty();
        chunk.set(3, 3, 3, Block::Solid);
        chunk.set(4, 3, 3, Block::Solid);
        assert_eq!(build_chunk_mesh(&chunk).indices.len(), 10 * 6);
    }
}
