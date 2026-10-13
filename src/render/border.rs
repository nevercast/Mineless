use glam::Vec3;
use wgpu::util::DeviceExt;

/// The world border's faint shield, drawn as one big rectangle on the inside face of each side
/// of the border ring rather than as blocks.
pub struct Border {
    pipeline: wgpu::RenderPipeline,
    vertices: wgpu::Buffer,
}

const VERTICES: u32 = 24;

impl Border {
    /// `extent` is the size of the world in blocks.
    pub fn new(device: &wgpu::Device, frame_layout: &wgpu::BindGroupLayout, extent: Vec3) -> Self {
        let shader = super::frame_shader(device, "border", include_str!("border.wgsl"));
        let pipeline = super::scene_pipeline(
            device,
            "border",
            &shader,
            &[Some(frame_layout)],
            &[Some(wgpu::VertexBufferLayout {
                array_stride: size_of::<[f32; 3]>() as wgpu::BufferAddress,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![0 => Float32x3],
            })],
            super::Layer::Translucent,
        );
        let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("border"),
            contents: bytemuck::cast_slice(&wall_vertices(extent).map(|v| v.to_array())),
            usage: wgpu::BufferUsages::VERTEX,
        });
        Self { pipeline, vertices }
    }

    pub fn draw(&self, pass: &mut wgpu::RenderPass, frame: &wgpu::BindGroup) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, frame, &[]);
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        pass.draw(0..VERTICES, 0..1);
    }
}

/// Two triangles for each of the four walls, on the planes where the border ring meets the
/// playable area, from the floor to the top of the world.
fn wall_vertices(extent: Vec3) -> [Vec3; VERTICES as usize] {
    let (low, high) = (1.0, extent - 1.0);
    let walls = [
        [(low, low), (low, high.z)],
        [(high.x, low), (high.x, high.z)],
        [(low, low), (high.x, low)],
        [(low, high.z), (high.x, high.z)],
    ];
    let mut vertices = [Vec3::ZERO; VERTICES as usize];
    for (wall, [(x0, z0), (x1, z1)]) in walls.into_iter().enumerate() {
        let corner = |end: f32, y: f32| Vec3::new(x0 + (x1 - x0) * end, y, z0 + (z1 - z0) * end);
        let (top, bottom) = (extent.y, 0.0);
        let quad = [
            corner(0.0, bottom),
            corner(1.0, bottom),
            corner(1.0, top),
            corner(0.0, bottom),
            corner(1.0, top),
            corner(0.0, top),
        ];
        vertices[wall * 6..wall * 6 + 6].copy_from_slice(&quad);
    }
    vertices
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walls_stand_on_the_inner_faces_of_the_border_ring() {
        let extent = Vec3::new(320.0, 128.0, 320.0);
        let vertices = wall_vertices(extent);
        for wall in vertices.chunks(6) {
            let on_x = wall.iter().all(|v| v.x == wall[0].x);
            let on_z = wall.iter().all(|v| v.z == wall[0].z);
            assert!(on_x != on_z, "{wall:?}");
            let plane = if on_x { wall[0].x } else { wall[0].z };
            assert!(plane == 1.0 || plane == 319.0, "{wall:?}");
            let ys: Vec<f32> = wall.iter().map(|v| v.y).collect();
            assert!(ys.contains(&0.0) && ys.contains(&128.0));
        }
        let min = vertices.iter().fold(Vec3::MAX, |a, &v| a.min(v));
        let max = vertices.iter().fold(Vec3::MIN, |a, &v| a.max(v));
        assert_eq!(
            (min, max),
            (Vec3::new(1.0, 0.0, 1.0), Vec3::new(319.0, 128.0, 319.0))
        );
    }
}
