//! A player in the simulation: a collision body that walks where it's told, stepped at a fixed
//! tick and drawn as an avatar interpolated between ticks.

use glam::{BVec3, Vec3};

use crate::avatar::{Avatar, Pose};
use crate::physics::Body;
use crate::world::World;

/// Simulation ticks per second, as in Minecraft.
pub const TICK_RATE: f32 = 20.0;
/// Minecraft's walking speed in blocks per second.
pub const WALK_SPEED: f32 = 4.317;
/// Minecraft's limb animation: swing eases towards the distance walked per tick, and the stride
/// advances with it.
const SWING_EASE: f32 = 0.4;
const STRIDE_PER_TICK: f32 = 0.6662;

pub struct Player {
    pub body: Body,
    /// Radians around +Y the body faces; zero faces -Z.
    pub yaw: f32,
    pub head_yaw: f32,
    pub head_pitch: f32,
    /// Whether to walk forward this tick.
    pub walking: bool,
    swing: f32,
    swing_phase: f32,
    /// Position and limb animation at the start of the current tick, to interpolate from.
    previous: (Vec3, f32, f32),
}

impl Player {
    pub fn new(feet: Vec3, yaw: f32) -> Self {
        Self {
            body: Body {
                position: feet,
                half_width: 0.3,
                height: 1.8,
            },
            yaw,
            head_yaw: 0.0,
            head_pitch: 0.0,
            walking: false,
            swing: 0.0,
            swing_phase: 0.0,
            previous: (feet, 0.0, 0.0),
        }
    }

    /// Advances one tick. Returns which axes collision cut short.
    pub fn tick(&mut self, world: &World) -> BVec3 {
        self.previous = (self.body.position, self.swing, self.swing_phase);
        let (sin, cos) = self.yaw.sin_cos();
        let forward = Vec3::new(-sin, 0.0, -cos);
        let step = if self.walking {
            forward * WALK_SPEED / TICK_RATE
        } else {
            Vec3::ZERO
        };
        let blocked = self.body.move_and_collide(world, step);

        let walked = (self.body.position - self.previous.0).with_y(0.0).length();
        self.swing += ((walked * 4.0).min(1.0) - self.swing) * SWING_EASE;
        self.swing_phase += self.swing * STRIDE_PER_TICK;
        blocked
    }

    /// How to draw the player `alpha` of the way from the last tick to the next.
    pub fn avatar(&self, alpha: f32) -> Avatar {
        let (position, swing, swing_phase) = self.previous;
        Avatar {
            position: position.lerp(self.body.position, alpha),
            yaw: self.yaw,
            pose: Pose {
                head_yaw: self.head_yaw,
                head_pitch: self.head_pitch,
                swing: swing + (self.swing - swing) * alpha,
                swing_phase: swing_phase + (self.swing_phase - swing_phase) * alpha,
            },
        }
    }
}
