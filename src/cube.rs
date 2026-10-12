use bytemuck::{Pod, Zeroable};

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

/// Where one cube sits in the world; the GPU draws the same cube mesh once per instance.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct Instance {
    pub position: [f32; 3],
}

impl Instance {
    pub const ATTRIBUTES: [wgpu::VertexAttribute; 1] = wgpu::vertex_attr_array![3 => Float32x3];

    pub fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: size_of::<Instance>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &Self::ATTRIBUTES,
        }
    }
}

/// Per-face brightness so the cube reads as 3D before there is any lighting.
const TOP: f32 = 1.0;
const SIDE: f32 = 0.8;
const FRONT_BACK: f32 = 0.65;
const BOTTOM: f32 = 0.5;

/// Every face is wound bottom-left, bottom-right, top-right, top-left as seen from outside,
/// so each face maps the whole texture the same way up.
const fn face(corners: [[f32; 3]; 4], shade: f32) -> [Vertex; 4] {
    const UVS: [[f32; 2]; 4] = [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];
    [
        Vertex {
            position: corners[0],
            uv: UVS[0],
            shade,
        },
        Vertex {
            position: corners[1],
            uv: UVS[1],
            shade,
        },
        Vertex {
            position: corners[2],
            uv: UVS[2],
            shade,
        },
        Vertex {
            position: corners[3],
            uv: UVS[3],
            shade,
        },
    ]
}

/// A unit cube centred on the origin, four vertices per face so each face has its own UVs.
#[rustfmt::skip]
pub const VERTICES: [[Vertex; 4]; 6] = [
    face([[ 0.5, -0.5,  0.5], [ 0.5, -0.5, -0.5], [ 0.5,  0.5, -0.5], [ 0.5,  0.5,  0.5]], SIDE),       // +X
    face([[-0.5, -0.5, -0.5], [-0.5, -0.5,  0.5], [-0.5,  0.5,  0.5], [-0.5,  0.5, -0.5]], SIDE),       // -X
    face([[-0.5,  0.5,  0.5], [ 0.5,  0.5,  0.5], [ 0.5,  0.5, -0.5], [-0.5,  0.5, -0.5]], TOP),        // +Y
    face([[-0.5, -0.5, -0.5], [ 0.5, -0.5, -0.5], [ 0.5, -0.5,  0.5], [-0.5, -0.5,  0.5]], BOTTOM),     // -Y
    face([[-0.5, -0.5,  0.5], [ 0.5, -0.5,  0.5], [ 0.5,  0.5,  0.5], [-0.5,  0.5,  0.5]], FRONT_BACK), // +Z
    face([[ 0.5, -0.5, -0.5], [-0.5, -0.5, -0.5], [-0.5,  0.5, -0.5], [ 0.5,  0.5, -0.5]], FRONT_BACK), // -Z
];

/// Two counter-clockwise triangles per face.
pub const INDICES: [u16; 36] = {
    let mut indices = [0u16; 36];
    let mut n = 0;
    while n < 6 {
        let base = (n * 4) as u16;
        let i = n * 6;
        indices[i] = base;
        indices[i + 1] = base + 1;
        indices[i + 2] = base + 2;
        indices[i + 3] = base;
        indices[i + 4] = base + 2;
        indices[i + 5] = base + 3;
        n += 1;
    }
    indices
};
