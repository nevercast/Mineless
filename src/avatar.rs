//! The player avatar: where it stands and the blocky body it is drawn with.

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Quat, Vec3, vec3};

/// Skin texels per block. Minecraft draws its 32-texel-tall player at 15/16 scale.
const TEXELS_PER_BLOCK: f32 = 16.0 / 0.9375;
const SKIN_SIZE: f32 = 64.0;

pub struct Avatar {
    /// Centre of the feet.
    pub position: Vec3,
    /// Radians around +Y; zero faces -Z, like the camera.
    pub yaw: f32,
}

impl Avatar {
    pub fn model_matrix(&self) -> Mat4 {
        Mat4::from_rotation_translation(Quat::from_rotation_y(self.yaw), self.position)
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct AvatarVertex {
    pub position: [f32; 3],
    pub uv: [f32; 2],
    pub shade: f32,
}

/// One box of the body, in skin texels with the feet at the origin, and where its net starts on
/// the skin. The net follows Minecraft's skin layout so any standard 64x64 skin fits.
struct Part {
    min: Vec3,
    size: Vec3,
    skin_origin: (f32, f32),
}

const fn part(min: [f32; 3], size: [f32; 3], skin_origin: (f32, f32)) -> Part {
    Part {
        min: vec3(min[0], min[1], min[2]),
        size: vec3(size[0], size[1], size[2]),
        skin_origin,
    }
}

/// The avatar faces -Z, so its right side is +X.
const PARTS: [Part; 6] = [
    part([-4.0, 24.0, -4.0], [8.0, 8.0, 8.0], (0.0, 0.0)), // head
    part([-4.0, 12.0, -2.0], [8.0, 12.0, 4.0], (16.0, 16.0)), // body
    part([4.0, 12.0, -2.0], [4.0, 12.0, 4.0], (40.0, 16.0)), // right arm
    part([-8.0, 12.0, -2.0], [4.0, 12.0, 4.0], (32.0, 48.0)), // left arm
    part([0.0, 0.0, -2.0], [4.0, 12.0, 4.0], (0.0, 16.0)), // right leg
    part([-4.0, 0.0, -2.0], [4.0, 12.0, 4.0], (16.0, 48.0)), // left leg
];

/// The body in model space, as triangles: four vertices per face and six indices per face.
pub fn build_mesh() -> (Vec<AvatarVertex>, Vec<u16>) {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    for part in &PARTS {
        let Vec3 { x: w, y: h, z: d } = part.size;
        let (u, v) = part.skin_origin;
        let centre = part.min + part.size / 2.0;
        // Each face: outward normal, its right and up as seen from outside, its rect on the
        // skin (left, top, width, height) and the same directional shade the terrain uses.
        let faces = [
            (Vec3::NEG_Z, Vec3::NEG_X, Vec3::Y, (u + d, v + d, w, h), 0.8), // front
            (
                Vec3::Z,
                Vec3::X,
                Vec3::Y,
                (u + 2.0 * d + w, v + d, w, h),
                0.8,
            ), // back
            (Vec3::X, Vec3::NEG_Z, Vec3::Y, (u, v + d, d, h), 0.65),        // right
            (
                Vec3::NEG_X,
                Vec3::Z,
                Vec3::Y,
                (u + d + w, v + d, d, h),
                0.65,
            ), // left
            (Vec3::Y, Vec3::NEG_X, Vec3::Z, (u + d, v, w, d), 1.0),         // top
            (
                Vec3::NEG_Y,
                Vec3::NEG_X,
                Vec3::NEG_Z,
                (u + d + w, v, w, d),
                0.5,
            ), // bottom
        ];
        for (normal, right, up, (left, top, width, height), shade) in faces {
            let half = part.size / 2.0;
            let face_centre = centre + normal * half;
            let right = right * half;
            let up = up * half;
            let first = vertices.len() as u16;
            let corners = [
                (face_centre - right + up, (left, top)),
                (face_centre - right - up, (left, top + height)),
                (face_centre + right - up, (left + width, top + height)),
                (face_centre + right + up, (left + width, top)),
            ];
            for (position, (s, t)) in corners {
                vertices.push(AvatarVertex {
                    position: (position / TEXELS_PER_BLOCK).into(),
                    uv: [s / SKIN_SIZE, t / SKIN_SIZE],
                    shade,
                });
            }
            indices.extend([0, 1, 2, 0, 2, 3].map(|i| first + i));
        }
    }
    (vertices, indices)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_stands_on_its_feet_at_minecraft_height() {
        let (vertices, indices) = build_mesh();
        assert_eq!(vertices.len(), PARTS.len() * 6 * 4);
        assert_eq!(indices.len(), PARTS.len() * 6 * 6);
        let heights = vertices.iter().map(|vertex| vertex.position[1]);
        let lowest = heights.clone().fold(f32::MAX, f32::min);
        let highest = heights.fold(f32::MIN, f32::max);
        assert_eq!(lowest, 0.0);
        assert!((highest - 1.875).abs() < 1e-5);
    }

    #[test]
    fn faces_wind_counter_clockwise_from_outside() {
        let (vertices, indices) = build_mesh();
        for (triangle, centre) in indices.chunks(3).zip(
            PARTS
                .iter()
                .flat_map(|part| [part.min + part.size / 2.0; 12]),
        ) {
            let [a, b, c] = [0, 1, 2].map(|i| Vec3::from(vertices[triangle[i] as usize].position));
            let normal = (b - a).cross(c - a);
            assert!(normal.dot(a - centre / TEXELS_PER_BLOCK) > 0.0);
        }
    }
}
