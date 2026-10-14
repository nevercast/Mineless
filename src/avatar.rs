//! The blocky body a player avatar is drawn with.

use bytemuck::{Pod, Zeroable};
use glam::{EulerRot, Mat4, Quat, Vec3, vec3};
use sim::avatar::Avatar;

/// Skin texels per block. Minecraft draws its 32-texel-tall player at 15/16 scale.
const TEXELS_PER_BLOCK: f32 = 16.0 / 0.9375;
/// Skins use Minecraft's 64x64 layout. UVs are fractions of it, so higher-resolution skins in the
/// same layout fit too.
const SKIN_LAYOUT: f32 = 64.0;

/// Peak limb angles in radians at full swing.
const LEG_SWING: f32 = 0.8;
const ARM_SWING: f32 = 0.6;

/// One world transform per entry of `PARTS`, in the same order.
pub fn part_transforms(avatar: &Avatar) -> [Mat4; PART_COUNT] {
    let model = Mat4::from_rotation_translation(Quat::from_rotation_y(avatar.yaw), avatar.position);
    let pose = &avatar.pose;
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
        model * Mat4::from_translation(pivot) * Mat4::from_quat(rotations[i]) * Mat4::from_translation(-pivot)
    })
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct AvatarVertex {
    pub position: [f32; 3],
    pub uv: [f32; 2],
    /// Outward unit normal in the rest pose.
    pub normal: [f32; 3],
    /// Index into `PARTS`, picking the transform the vertex moves with.
    pub part: u32,
}

/// One box of the body, in skin texels with the feet at the origin, and the joint it turns about.
/// Each part is drawn twice: its base box, and a slightly larger overlay box (hat, jacket,
/// sleeves, pants) whose transparent texels are cut out. Both nets follow Minecraft's skin layout,
/// so any standard skin fits.
struct Part {
    min: Vec3,
    size: Vec3,
    pivot: Vec3,
    /// Top-left of the base and overlay nets on a 64x64 skin.
    base: (f32, f32),
    overlay: (f32, f32),
    /// Texels the overlay box stands out from the base on every side.
    inflate: f32,
    /// Texels added to each side across X and Z, without changing the skin mapping.
    grow: f32,
}

const fn part(
    min: [f32; 3],
    size: [f32; 3],
    pivot: [f32; 3],
    (base, overlay): ((f32, f32), (f32, f32)),
    (inflate, grow): (f32, f32),
) -> Part {
    Part {
        min: vec3(min[0], min[1], min[2]),
        size: vec3(size[0], size[1], size[2]),
        pivot: vec3(pivot[0], pivot[1], pivot[2]),
        base,
        overlay,
        inflate,
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
    part([-4.0, 24.0, -4.0], [8.0, 8.0, 8.0], [0.0, 24.0, 0.0], ((0.0, 0.0), (32.0, 0.0)), (0.5, 0.0)), // head
    part([-4.0, 12.0, -2.0], [8.0, 12.0, 4.0], [0.0, 12.0, 0.0], ((16.0, 16.0), (16.0, 32.0)), (0.25, 0.0)), // body
    part([4.0, 12.0, -2.0], [4.0, 12.0, 4.0], [5.0, 22.0, 0.0], ((40.0, 16.0), (40.0, 32.0)), (0.25, 0.0)), // right arm
    part([-8.0, 12.0, -2.0], [4.0, 12.0, 4.0], [-5.0, 22.0, 0.0], ((32.0, 48.0), (48.0, 48.0)), (0.25, 0.0)), // left arm
    part([0.0, 0.0, -2.0], [4.0, 12.0, 4.0], [2.0, 12.0, 0.0], ((0.0, 16.0), (0.0, 32.0)), (0.25, LEG_GROW)), // right leg
    part([-4.0, 0.0, -2.0], [4.0, 12.0, 4.0], [-2.0, 12.0, 0.0], ((16.0, 48.0), (0.0, 48.0)), (0.25, LEG_GROW)), // left leg
];

/// The body in its rest pose in model space, as triangles: all the base boxes, then all the
/// overlay boxes, with four vertices and six indices per face.
pub fn build_mesh() -> (Vec<AvatarVertex>, Vec<u16>) {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    for overlay in [false, true] {
        for (index, part) in PARTS.iter().enumerate() {
            let grow = vec3(part.grow, 0.0, part.grow);
            let (net, grow) = if overlay {
                (part.overlay, grow + part.inflate)
            } else {
                (part.base, grow)
            };
            push_box(&mut vertices, &mut indices, part, index as u32, net, grow);
        }
    }
    (vertices, indices)
}

fn push_box(vertices: &mut Vec<AvatarVertex>, indices: &mut Vec<u16>, part: &Part, index: u32, (u, v): (f32, f32), grow: Vec3) {
    let Vec3 { x: w, y: h, z: d } = part.size;
    let centre = part.min + part.size / 2.0;
    let half = part.size / 2.0 + grow;
    // Each face: outward normal, its right and up as seen from outside, and its rect on the
    // skin (left, top, width, height).
    let faces = [
        (Vec3::NEG_Z, Vec3::NEG_X, Vec3::Y, (u + d, v + d, w, h)),     // front
        (Vec3::Z, Vec3::X, Vec3::Y, (u + 2.0 * d + w, v + d, w, h)),   // back
        (Vec3::X, Vec3::NEG_Z, Vec3::Y, (u, v + d, d, h)),             // right
        (Vec3::NEG_X, Vec3::Z, Vec3::Y, (u + d + w, v + d, d, h)),     // left
        (Vec3::Y, Vec3::NEG_X, Vec3::Z, (u + d, v, w, d)),             // top
        (Vec3::NEG_Y, Vec3::NEG_X, Vec3::NEG_Z, (u + d + w, v, w, d)), // bottom
    ];
    for (normal, right, up, (left, top, width, height)) in faces {
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
                uv: [s / SKIN_LAYOUT, t / SKIN_LAYOUT],
                normal: normal.into(),
                part: index,
            });
        }
        indices.extend([0, 1, 2, 0, 2, 3].map(|i| first + i));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim::avatar::{Pose, Skin};

    #[test]
    fn body_stands_on_its_feet_at_minecraft_height() {
        let (vertices, indices) = build_mesh();
        assert_eq!(vertices.len(), 2 * PARTS.len() * 6 * 4);
        assert_eq!(indices.len(), 2 * PARTS.len() * 6 * 6);
        let base = &vertices[..vertices.len() / 2];
        let heights = base.iter().map(|vertex| vertex.position[1]);
        let lowest = heights.clone().fold(f32::MAX, f32::min);
        let highest = heights.fold(f32::MIN, f32::max);
        assert_eq!(lowest, 0.0);
        assert!((highest - 1.875).abs() < 1e-5);
    }

    #[test]
    fn faces_wind_counter_clockwise_from_outside() {
        let (vertices, indices) = build_mesh();
        let centres = PARTS.iter().map(|part| part.min + part.size / 2.0);
        let triangle_centres = centres.clone().chain(centres).flat_map(|centre| [centre; 12]);
        assert_eq!(triangle_centres.clone().count(), indices.len() / 3);
        for (triangle, centre) in indices.chunks(3).zip(triangle_centres) {
            let [a, b, c] = [0, 1, 2].map(|i| Vec3::from(vertices[triangle[i] as usize].position));
            let normal = (b - a).cross(c - a);
            assert!(normal.dot(a - centre / TEXELS_PER_BLOCK) > 0.0);
        }
    }

    #[test]
    fn swinging_limbs_turn_about_their_joints() {
        let avatar = Avatar {
            position: Vec3::ZERO,
            yaw: 0.0,
            pose: Pose {
                swing: 1.0,
                swing_phase: 1.0,
                ..Default::default()
            },
            skin: Skin::Player,
        };
        for (part, transform) in PARTS.iter().zip(part_transforms(&avatar)) {
            let pivot = part.pivot / TEXELS_PER_BLOCK;
            assert!(transform.transform_point3(pivot).abs_diff_eq(pivot, 1e-5));
        }
        let right_leg = part_transforms(&avatar)[4];
        let foot = right_leg.transform_point3(vec3(2.0, 0.0, 0.0) / TEXELS_PER_BLOCK);
        assert!(foot.z.abs() > 0.1, "a running leg leaves the vertical");
    }
}
