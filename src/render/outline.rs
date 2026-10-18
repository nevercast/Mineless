use glam::{IVec3, Vec3};

/// Dark lines round the edges of the blocks players are aiming at, as Minecraft draws them. Each edge is a ribbon facing the
/// camera, because GPU lines are one pixel wide and can't be smoothed.
pub struct Outline {
    pipeline: wgpu::RenderPipeline,
    vertices: wgpu::Buffer,
    vertex_count: u32,
}

const EDGES_PER_BLOCK: usize = 12;
/// Two triangles per edge.
const VERTICES_PER_EDGE: usize = 6;
/// Both ends of the edge, how far along it the vertex sits (0 or 1), and which side of it (-1 or 1).
type Vertex = [f32; 8];
/// The outline sits this far outside the block so its faces don't hide it.
const INFLATE: f32 = 0.002;

impl Outline {
    pub fn new(device: &wgpu::Device, frame_layout: &wgpu::BindGroupLayout) -> Self {
        let shader = super::frame_shader(device, "outline", include_str!("outline.wgsl"));
        let pipeline = super::scene_pipeline(
            device,
            "outline",
            &shader,
            &[Some(frame_layout)],
            &[Some(wgpu::VertexBufferLayout {
                array_stride: size_of::<Vertex>() as wgpu::BufferAddress,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2],
            })],
            super::Layer::Translucent,
        );
        Self {
            pipeline,
            vertices: create_vertex_buffer(device, 1),
            vertex_count: 0,
        }
    }

    /// Uploads this frame's outlined blocks. Must run before the render pass that draws them.
    pub fn prepare(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, blocks: &[IVec3]) {
        let vertices: Vec<Vertex> = blocks
            .iter()
            .flat_map(|&block| edges(block))
            .flat_map(|(start, end)| {
                [(0.0, -1.0), (1.0, -1.0), (1.0, 1.0), (0.0, -1.0), (1.0, 1.0), (0.0, 1.0)]
                    .map(|(along, side)| [start.x, start.y, start.z, end.x, end.y, end.z, along, side])
            })
            .collect();
        if size_of_val(vertices.as_slice()) as wgpu::BufferAddress > self.vertices.size() {
            self.vertices = create_vertex_buffer(device, blocks.len());
        }
        queue.write_buffer(&self.vertices, 0, bytemuck::cast_slice(&vertices));
        self.vertex_count = vertices.len() as u32;
    }

    pub fn draw(&self, pass: &mut wgpu::RenderPass, frame: &wgpu::BindGroup) {
        if self.vertex_count == 0 {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, frame, &[]);
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        pass.draw(0..self.vertex_count, 0..1);
    }
}

fn create_vertex_buffer(device: &wgpu::Device, blocks: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("outline"),
        size: (blocks.max(1) * EDGES_PER_BLOCK * VERTICES_PER_EDGE * size_of::<Vertex>()) as wgpu::BufferAddress,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

/// The ends of the twelve edges of the block at `block`, slightly enlarged.
fn edges(block: IVec3) -> [(Vec3, Vec3); EDGES_PER_BLOCK] {
    let (min, max) = (block.as_vec3() - INFLATE, block.as_vec3() + 1.0 + INFLATE);
    let corner = |i: usize| Vec3::select(glam::BVec3::new(i & 1 != 0, i & 2 != 0, i & 4 != 0), max, min);
    let mut edges = [(Vec3::ZERO, Vec3::ZERO); EDGES_PER_BLOCK];
    let mut next = 0;
    // Corners are numbered by bits x, y, z; an edge joins two corners one bit apart.
    for i in 0..8 {
        for bit in [1, 2, 4] {
            if i & bit == 0 {
                edges[next] = (corner(i), corner(i | bit));
                next += 1;
            }
        }
    }
    edges
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edges_run_along_one_axis_each_round_the_block() {
        let edges = edges(IVec3::new(-3, 7, 2));
        let mut axes = [0; 3];
        for edge in edges {
            let along = (edge.1 - edge.0).abs();
            let axis = (0..3)
                .find(|&a| along[a] > 0.5)
                .expect("every edge spans the block along some axis");
            assert!(
                (along[axis] - (1.0 + 2.0 * INFLATE)).abs() < 1e-5 && along.element_sum() == along[axis],
                "{edge:?}"
            );
            axes[axis] += 1;
        }
        assert_eq!(axes, [4, 4, 4]);
        let min = edges.iter().fold(Vec3::MAX, |a, &(start, end)| a.min(start).min(end));
        assert!(min.abs_diff_eq(Vec3::new(-3.0, 7.0, 2.0) - INFLATE, 1e-5));
    }
}
