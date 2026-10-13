//! Where a player avatar stands and how its body is held, for whoever draws it.

use glam::Vec3;

/// A skin an avatar can wear. The renderer uses the discriminant as the skin's texture layer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Skin {
    Player,
    Autumn,
    CyberA,
    CyberB,
}

impl Skin {
    pub const ALL: [Skin; 4] = [Skin::Player, Skin::Autumn, Skin::CyberA, Skin::CyberB];
}

pub struct Avatar {
    /// Centre of the feet.
    pub position: Vec3,
    /// Radians around +Y the body faces; zero faces -Z, like the camera.
    pub yaw: f32,
    pub pose: Pose,
    pub skin: Skin,
}

/// How the body is held. Head angles are relative to the body.
#[derive(Default)]
pub struct Pose {
    pub head_yaw: f32,
    /// Radians above level.
    pub head_pitch: f32,
    /// How far arms and legs swing, from 0 standing still to 1 running.
    pub swing: f32,
    /// Where in the stride the limbs are, in radians.
    pub swing_phase: f32,
}
