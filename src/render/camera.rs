use glam::camera::rh::{proj::directx, view};
use glam::{Mat4, Vec3};

pub struct Camera {
    pub position: Vec3,
    pub target: Vec3,
    pub fov_y_radians: f32,
}

impl Camera {
    pub fn looking_at(position: Vec3, target: Vec3) -> Self {
        Self {
            position,
            target,
            fov_y_radians: 70f32.to_radians(),
        }
    }

    /// WebGPU shares DirectX's clip space: Y up, depth in [0, 1].
    pub fn view_projection(&self, aspect_ratio: f32) -> Mat4 {
        let view = view::look_at_mat4(self.position, self.target, Vec3::Y);
        let projection = directx::perspective(self.fov_y_radians, aspect_ratio, 0.1, 1000.0);
        projection * view
    }
}
