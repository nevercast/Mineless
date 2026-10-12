use glam::camera::rh::{proj::directx, view};
use glam::{Mat4, Vec3};

/// A free camera described by where it is and which way it faces.
pub struct Camera {
    pub position: Vec3,
    /// Radians around +Y; zero looks down -Z.
    pub yaw: f32,
    /// Radians above the horizon.
    pub pitch: f32,
    pub fov_y_radians: f32,
}

impl Camera {
    pub fn looking_at(position: Vec3, target: Vec3) -> Self {
        let direction = (target - position).normalize();
        Self {
            position,
            yaw: (-direction.x).atan2(-direction.z),
            pitch: direction.y.asin(),
            fov_y_radians: 70f32.to_radians(),
        }
    }

    pub fn forward(&self) -> Vec3 {
        let (sin_yaw, cos_yaw) = self.yaw.sin_cos();
        let (sin_pitch, cos_pitch) = self.pitch.sin_cos();
        Vec3::new(-sin_yaw * cos_pitch, sin_pitch, -cos_yaw * cos_pitch)
    }

    /// WebGPU shares DirectX's clip space: Y up, depth in [0, 1].
    pub fn view_projection(&self, aspect_ratio: f32) -> Mat4 {
        let view = view::look_to_mat4(self.position, self.forward(), Vec3::Y);
        let projection = directx::perspective(self.fov_y_radians, aspect_ratio, 0.1, 1000.0);
        projection * view
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn looking_at_faces_the_target() {
        let target = Vec3::new(3.0, -2.0, 5.0);
        let camera = Camera::looking_at(Vec3::new(10.0, 4.0, -1.0), target);
        let expected = (target - camera.position).normalize();
        assert!(camera.forward().abs_diff_eq(expected, 1e-5));
    }
}
