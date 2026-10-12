//! The player avatar: where it stands and the blocky body it is drawn with.

use bytemuck::{Pod, Zeroable};
use glam::{EulerRot, Mat4, Quat, Vec3, vec3};

/// Skin texels per block. Minecraft draws its 32-texel-tall player at 15/16 scale.
const TEXELS_PER_BLOCK: f32 = 16.0 / 0.9375;
const SKIN_SIZE: f32 = 64.0;

/// Peak limb angles in radians at full swing.
const LEG_SWING: f32 = 0.8;
const ARM_SWING: f32 = 0.6;
/// Radians of swing phase per second at full swing: a little over one stride per second.
const STRIDE_RATE: f32 = 9.0;

pub struct Avatar {
    /// Centre of the feet.
    pub position: Vec3,
    /// Radians around +Y the body faces; zero faces -Z, like the camera.
    pub yaw: f32,
    pub pose: Pose,
}

/// How the body is held. Head angles are relative to the body.
#[derive(Default)]
pub struct Pose {
    pub head_yaw: f32,
    /// Radians above level.
    pub head_pitch: f32,
    /// How far arms and legs swing, from 0 standing still to 1 running.
    pub swing: f32,
    /// Where in the stride the limbs are; it advances with `swing`.
    pub swing_phase: f32,
}

impl Avatar {
    pub fn animate(&mut self, seconds: f32) {
        self.pose.swing_phase += self.pose.swing * STRIDE_RATE * seconds;
    }

    /// One world transform per entry of `PARTS`, in the same order.
    pub fn part_transforms(&self) -> [Mat4; PART_COUNT] {
        let model = Mat4::from_rotation_translation(Quat::from_rotation_y(self.yaw), self.position);
        let pose = &self.pose;
        let limb = pose.swing_phase.sin() * pose.swing;
        // Arms swing against the leg on their own side, as in a natural stride.
        let rotations = [
            Quat::from_euler(EulerRot::YXZ, pose.head_yaw, pose.head_pitch, 0.0),
            Quat::IDENTITY,
            Quat::from_rotation_x(-limb * ARM_SWING),
            Quat::from_rotation_x(limb * ARM_SWING),
            Quat::from_rotation_x(limb * LEG_SWING),
            Quat::from_rotation_x(-limb * LEG_SWING),
        ];
        std::array::from_fn(|i| {
            let pivot = PARTS[i].pivot / TEXELS_PER_BLOCK;
            model
                * Mat4::from_translation(pivot)
                * Mat4::from_quat(rotations[i])
                * Mat4::from_translation(-pivot)
        })
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct AvatarVertex {
    pub position: [f32; 3],
    pub uv: [f32; 2],
    pub shade: f32,
    /// Index into `PARTS`, picking the transform the vertex moves with.
    pub part: u32,
}

/// One box of the body, in skin texels with the feet at the origin; the joint it turns about; and
/// where its net starts on the skin. The net follows Minecraft's skin layout so any standard
/// 64x64 skin fits.
struct Part {
    min: Vec3,
    size: Vec3,
    pivot: Vec3,
    skin_origin: (f32, f32),
    /// Texels added to each side across X and Z, without changing the skin mapping.
    grow: f32,
}

const fn part(
    min: [f32; 3],
    size: [f32; 3],
    pivot: [f32; 3],
    skin_origin: (f32, f32),
    grow: f32,
) -> Part {
    Part {
        min: vec3(min[0], min[1], min[2]),
        size: vec3(size[0], size[1], size[2]),
        pivot: vec3(pivot[0], pivot[1], pivot[2]),
        skin_origin,
        grow,
    }
}

pub const PART_COUNT: usize = 6;

/// Legs share their outer sides with the body's sides, so they're widened a touch to cover them
/// rather than z-fight while swinging.
const LEG_GROW: f32 = 0.05;

/// The avatar faces -Z, so its right side is +X. Pivots are the neck, shoulders and hips.
#[rustfmt::skip]
const PARTS: [Part; PART_COUNT] = [
    part([-4.0, 24.0, -4.0], [8.0, 8.0, 8.0], [0.0, 24.0, 0.0], (0.0, 0.0), 0.0), // head
    part([-4.0, 12.0, -2.0], [8.0, 12.0, 4.0], [0.0, 12.0, 0.0], (16.0, 16.0), 0.0), // body
    part([4.0, 12.0, -2.0], [4.0, 12.0, 4.0], [5.0, 22.0, 0.0], (40.0, 16.0), 0.0), // right arm
    part([-8.0, 12.0, -2.0], [4.0, 12.0, 4.0], [-5.0, 22.0, 0.0], (32.0, 48.0), 0.0), // left arm
    part([0.0, 0.0, -2.0], [4.0, 12.0, 4.0], [2.0, 12.0, 0.0], (0.0, 16.0), LEG_GROW), // right leg
    part([-4.0, 0.0, -2.0], [4.0, 12.0, 4.0], [-2.0, 12.0, 0.0], (16.0, 48.0), LEG_GROW), // left leg
];

/// The body in its rest pose in model space, as triangles: four vertices per face and six indices per face.
pub fn build_mesh() -> (Vec<AvatarVertex>, Vec<u16>) {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    for (index, part) in PARTS.iter().enumerate() {
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
            let half = part.size / 2.0 + vec3(part.grow, 0.0, part.grow);
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
                    part: index as u32,
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

    #[test]
    fn swinging_limbs_turn_about_their_joints() {
        let mut avatar = Avatar {
            position: Vec3::ZERO,
            yaw: 0.0,
            pose: Pose {
                swing: 1.0,
                swing_phase: 1.0,
                ..Default::default()
            },
        };
        avatar.animate(0.1);
        for (part, transform) in PARTS.iter().zip(avatar.part_transforms()) {
            let pivot = part.pivot / TEXELS_PER_BLOCK;
            assert!(transform.transform_point3(pivot).abs_diff_eq(pivot, 1e-5));
        }
        let right_leg = avatar.part_transforms()[4];
        let foot = right_leg.transform_point3(vec3(2.0, 0.0, 0.0) / TEXELS_PER_BLOCK);
        assert!(foot.z.abs() > 0.1, "a running leg leaves the vertical");
    }
}
