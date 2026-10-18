use glam::{IVec3, Vec3};

/// Thin dark lines round the edges of the blocks players are aiming at, as Minecraft draws them.
pub struct Outline {
    pipeline: wgpu::RenderPipeline,
    vertices: wgpu::Buffer,
    vertex_count: u32,
}

/// Two ends for each of a cube's twelve edges.
const VERTICES_PER_BLOCK: usize = 24;
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
                array_stride: size_of::<[f32; 3]>() as wgpu::BufferAddress,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![0 => Float32x3],
            })],
            super::Layer::Lines,
        );
        Self {
            pipeline,
            vertices: create_vertex_buffer(device, 1),
            vertex_count: 0,
        }
    }

    /// Uploads this frame's outlined blocks. Must run before the render pass that draws them.
    pub fn prepare(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, blocks: &[IVec3]) {
        let vertices: Vec<[f32; 3]> = blocks
            .iter()
            .flat_map(|&block| edges(block))
            .map(|vertex| vertex.to_array())
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
        size: (blocks.max(1) * VERTICES_PER_BLOCK * size_of::<[f32; 3]>()) as wgpu::BufferAddress,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

/// The ends of the twelve edges of the block at `block`, slightly enlarged.
fn edges(block: IVec3) -> [Vec3; VERTICES_PER_BLOCK] {
    let (min, max) = (block.as_vec3() - INFLATE, block.as_vec3() + 1.0 + INFLATE);
    let corner = |i: usize| Vec3::select(glam::BVec3::new(i & 1 != 0, i & 2 != 0, i & 4 != 0), max, min);
    let mut vertices = [Vec3::ZERO; VERTICES_PER_BLOCK];
    let mut next = 0;
    // Corners are numbered by bits x, y, z; an edge joins two corners one bit apart.
    for i in 0..8 {
        for bit in [1, 2, 4] {
            if i & bit == 0 {
                vertices[next] = corner(i);
                vertices[next + 1] = corner(i | bit);
                next += 2;
            }
        }
    }
    vertices
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edges_run_along_one_axis_each_round_the_block() {
        let vertices = edges(IVec3::new(-3, 7, 2));
        let mut axes = [0; 3];
        for edge in vertices.chunks(2) {
            let along = (edge[1] - edge[0]).abs();
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
        let min = vertices.iter().fold(Vec3::MAX, |a, &v| a.min(v));
        assert!(min.abs_diff_eq(Vec3::new(-3.0, 7.0, 2.0) - INFLATE, 1e-5));
    }
}
