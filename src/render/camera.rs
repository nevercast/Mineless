use glam::camera::rh::{proj::directx, view};
use glam::{IVec3, Mat4, Vec3, Vec4};
use sim::world::CHUNK_SIZE;

/// A free camera described by where it is and which way it faces.
pub struct Camera {
    pub position: Vec3,
    /// Radians around +Y; zero looks down -Z.
    pub yaw: f32,
    /// Radians above the horizon.
    pub pitch: f32,
    fov_y_radians: f32,
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

    fn forward(&self) -> Vec3 {
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

/// The six planes bounding what a view-projection matrix can see, each with its normal facing in.
pub struct Frustum {
    planes: [Vec4; 6],
}

impl Frustum {
    pub fn new(view_projection: Mat4) -> Self {
        let [x, y, z, w] = [0, 1, 2, 3].map(|i| view_projection.row(i));
        // Clip space keeps -w <= x <= w, -w <= y <= w and, in WebGPU's depth range, 0 <= z <= w.
        Self {
            planes: [w + x, w - x, w + y, w - y, z, w - z],
        }
    }

    /// Whether any of the box from `min` to `max` might be in view. A box just outside a corner
    /// of the frustum can still pass, which only costs a draw that shows nothing.
    pub fn intersects_box(&self, min: Vec3, max: Vec3) -> bool {
        self.planes.iter().all(|plane| {
            let normal = plane.truncate();
            // The box corner furthest along the inward normal is the last one to leave.
            let corner = Vec3::select(normal.cmpge(Vec3::ZERO), max, min);
            normal.dot(corner) + plane.w >= 0.0
        })
    }

    /// Whether any of the chunk at chunk coordinate `chunk` might be in view.
    pub fn contains_chunk(&self, chunk: IVec3) -> bool {
        let min = (chunk * CHUNK_SIZE as i32).as_vec3();
        self.intersects_box(min, min + CHUNK_SIZE as f32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frustum_keeps_boxes_in_view_and_culls_the_rest() {
        // At the origin looking down -Z.
        let camera = Camera::looking_at(Vec3::ZERO, Vec3::NEG_Z);
        let frustum = Frustum::new(camera.view_projection(1.0));
        let unit_box_at = |centre: Vec3| frustum.intersects_box(centre - 1.0, centre + 1.0);
        assert!(unit_box_at(Vec3::new(0.0, 0.0, -10.0)));
        // Straddling the edge of the view.
        assert!(unit_box_at(Vec3::new(7.5, 0.0, -10.0)));
        assert!(!unit_box_at(Vec3::new(0.0, 0.0, 10.0)), "behind");
        assert!(!unit_box_at(Vec3::new(-50.0, 0.0, -10.0)), "far to the left");
        assert!(!unit_box_at(Vec3::new(0.0, 50.0, -10.0)), "far above");
        assert!(!unit_box_at(Vec3::new(0.0, 0.0, -2000.0)), "past the far plane");
    }

    #[test]
    fn looking_at_faces_the_target() {
        let target = Vec3::new(3.0, -2.0, 5.0);
        let camera = Camera::looking_at(Vec3::new(10.0, 4.0, -1.0), target);
        let expected = (target - camera.position).normalize();
        assert!(camera.forward().abs_diff_eq(expected, 1e-5));
    }
}
