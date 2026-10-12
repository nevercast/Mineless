use bytemuck::{Pod, Zeroable};

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub color: [f32; 3],
}

impl Vertex {
    pub const ATTRIBUTES: [wgpu::VertexAttribute; 2] =
        wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3];

    pub fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBUTES,
        }
    }
}

/// Each face is shaded differently so the cube reads as 3D before we have lighting or textures.
const TOP: [f32; 3] = [0.45, 0.75, 0.30];
const SIDE: [f32; 3] = [0.35, 0.60, 0.24];
const FRONT_BACK: [f32; 3] = [0.30, 0.52, 0.20];
const BOTTOM: [f32; 3] = [0.20, 0.36, 0.14];

const fn v(position: [f32; 3], color: [f32; 3]) -> Vertex {
    Vertex { position, color }
}

/// A unit cube centred on the origin, four vertices per face so each face keeps its own colour.
#[rustfmt::skip]
pub const VERTICES: [Vertex; 24] = [
    // +X
    v([ 0.5, -0.5,  0.5], SIDE), v([ 0.5, -0.5, -0.5], SIDE), v([ 0.5,  0.5, -0.5], SIDE), v([ 0.5,  0.5,  0.5], SIDE),
    // -X
    v([-0.5, -0.5, -0.5], SIDE), v([-0.5, -0.5,  0.5], SIDE), v([-0.5,  0.5,  0.5], SIDE), v([-0.5,  0.5, -0.5], SIDE),
    // +Y
    v([-0.5,  0.5,  0.5], TOP), v([ 0.5,  0.5,  0.5], TOP), v([ 0.5,  0.5, -0.5], TOP), v([-0.5,  0.5, -0.5], TOP),
    // -Y
    v([-0.5, -0.5, -0.5], BOTTOM), v([ 0.5, -0.5, -0.5], BOTTOM), v([ 0.5, -0.5,  0.5], BOTTOM), v([-0.5, -0.5,  0.5], BOTTOM),
    // +Z
    v([-0.5, -0.5,  0.5], FRONT_BACK), v([ 0.5, -0.5,  0.5], FRONT_BACK), v([ 0.5,  0.5,  0.5], FRONT_BACK), v([-0.5,  0.5,  0.5], FRONT_BACK),
    // -Z
    v([ 0.5, -0.5, -0.5], FRONT_BACK), v([-0.5, -0.5, -0.5], FRONT_BACK), v([-0.5,  0.5, -0.5], FRONT_BACK), v([ 0.5,  0.5, -0.5], FRONT_BACK),
];

/// Two counter-clockwise triangles per face.
pub const INDICES: [u16; 36] = {
    let mut indices = [0u16; 36];
    let mut face = 0;
    while face < 6 {
        let base = (face * 4) as u16;
        let i = face * 6;
        indices[i] = base;
        indices[i + 1] = base + 1;
        indices[i + 2] = base + 2;
        indices[i + 3] = base;
        indices[i + 4] = base + 2;
        indices[i + 5] = base + 3;
        face += 1;
    }
    indices
};
