//! A player in the simulation: a collision body driven only through its `Controls`, stepped at a
//! fixed tick and drawn as an avatar interpolated between ticks.

use glam::{BVec3, Vec2, Vec3};

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
/// Minecraft's falling, per tick: velocity loses this much downward, then keeps `AIR_DRAG` of
/// itself, which settles at a terminal speed of 3.92 blocks per tick.
const GRAVITY: f32 = 0.08;
const AIR_DRAG: f32 = 0.98;
/// Jump heights a player can choose from, in blocks. Minecraft's own jump peaks at about 1.25.
pub const JUMP_HEIGHTS: std::ops::RangeInclusive<f32> = 0.5..=1.25;

/// Everything whoever drives a player can ask of it. Read at the start of every tick.
#[derive(Clone, Copy, Default)]
pub struct Controls {
    /// Radians around +Y the body faces; zero faces -Z.
    pub yaw: f32,
    /// Where the head looks, relative to the body: radians to the left, and radians above level.
    pub head_yaw: f32,
    pub head_pitch: f32,
    /// Walking relative to the facing: `y` forward, `x` to the right. Anything longer than 1 is
    /// scaled down, so walking diagonally is no faster.
    pub walk: Vec2,
    /// Jump to peak this many blocks up, clamped to `JUMP_HEIGHTS`. Taken on the next tick that
    /// starts on the ground, and dropped otherwise.
    pub jump: Option<f32>,
}

pub struct Player {
    pub controls: Controls,
    body: Body,
    /// Vertical speed in blocks per tick; up is positive.
    fall_velocity: f32,
    on_ground: bool,
    swing: f32,
    swing_phase: f32,
    /// Position and limb animation at the start of the current tick, to interpolate from.
    previous: (Vec3, f32, f32),
}

impl Player {
    pub fn new(feet: Vec3, yaw: f32) -> Self {
        Self {
            controls: Controls {
                yaw,
                ..Default::default()
            },
            body: Body {
                position: feet,
                half_width: 0.3,
                height: 1.8,
            },
            fall_velocity: 0.0,
            on_ground: false,
            swing: 0.0,
            swing_phase: 0.0,
            previous: (feet, 0.0, 0.0),
        }
    }

    /// Whether the last tick ended standing on a block.
    pub fn is_on_ground(&self) -> bool {
        self.on_ground
    }

    /// Vertical speed in blocks per tick; up is positive.
    pub fn vertical_velocity(&self) -> f32 {
        self.fall_velocity
    }

    /// Advances one tick. Returns which axes collision cut short.
    pub fn tick(&mut self, world: &World) -> BVec3 {
        self.previous = (self.body.position, self.swing, self.swing_phase);
        let controls = self.controls;
        if let Some(height) = self.controls.jump.take()
            && self.on_ground
        {
            self.fall_velocity =
                launch_velocity(height.clamp(*JUMP_HEIGHTS.start(), *JUMP_HEIGHTS.end()));
        }
        let (sin, cos) = controls.yaw.sin_cos();
        let forward = Vec3::new(-sin, 0.0, -cos);
        let right = Vec3::new(cos, 0.0, -sin);
        let walk = controls.walk.clamp_length_max(1.0);
        let step = (forward * walk.y + right * walk.x) * WALK_SPEED / TICK_RATE;
        let step = step.with_y(self.fall_velocity);
        let blocked = self.body.move_and_collide(world, step);
        // Like Minecraft, move first and then accelerate, so a body at rest drops a tick later.
        // Gravity applies even on the ground, keeping a standing body pressed onto it.
        self.on_ground = blocked.y && step.y < 0.0;
        let velocity = if blocked.y { 0.0 } else { self.fall_velocity };
        self.fall_velocity = (velocity - GRAVITY) * AIR_DRAG;

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
            yaw: self.controls.yaw,
            pose: Pose {
                head_yaw: self.controls.head_yaw,
                head_pitch: self.controls.head_pitch,
                swing: swing + (self.swing - swing) * alpha,
                swing_phase: swing_phase + (self.swing_phase - swing_phase) * alpha,
            },
        }
    }
}

/// How high a leap at `velocity` blocks per tick rises under Minecraft's per-tick gravity.
fn peak_height(mut velocity: f32) -> f32 {
    let mut height = 0.0;
    while velocity > 0.0 {
        height += velocity;
        velocity = (velocity - GRAVITY) * AIR_DRAG;
    }
    height
}

/// The upward velocity whose leap peaks at `height`. Peak height only grows with velocity, so a
/// bisection finds it; Minecraft's own jump is 0.42.
fn launch_velocity(height: f32) -> f32 {
    let (mut low, mut high) = (0.0, 1.0);
    for _ in 0..32 {
        let middle = f32::midpoint(low, high);
        if peak_height(middle) < height {
            low = middle;
        } else {
            high = middle;
        }
    }
    high
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::Block;
    use glam::IVec3;

    /// A stone floor at y = 0.
    fn floor() -> World {
        let mut world = World::empty(IVec3::new(1, 4, 1));
        for x in 0..16 {
            for z in 0..16 {
                world.set_block(IVec3::new(x, 0, z), Block::Stone);
            }
        }
        world
    }

    #[test]
    fn falls_like_minecraft_and_lands_flush() {
        let world = floor();
        let mut player = Player::new(Vec3::new(8.5, 51.0, 8.5), 0.0);
        let mut heights = Vec::new();
        while !player.on_ground {
            player.tick(&world);
            heights.push(player.body.position.y);
        }
        // Minecraft's first ticks of a fall from rest: 0, 0.0784, 0.2337, 0.4643 blocks.
        for (height, fallen) in heights.iter().zip([0.0, 0.0784, 0.2337, 0.4643]) {
            assert!((51.0 - height - fallen).abs() < 1e-3, "{heights:?}");
        }
        assert_eq!(player.body.position.y, 1.0);
    }

    #[test]
    fn falling_speed_settles_at_terminal_velocity() {
        let world = World::empty(IVec3::ONE);
        let mut player = Player::new(Vec3::new(8.5, 0.0, 8.5), 0.0);
        for _ in 0..400 {
            player.tick(&world);
        }
        assert!((player.fall_velocity + 3.92).abs() < 1e-2);
        assert!(!player.on_ground);
    }

    #[test]
    fn standing_stays_on_the_ground_every_tick() {
        let world = floor();
        let mut player = Player::new(Vec3::new(8.5, 1.0, 8.5), 0.0);
        player.tick(&world);
        for _ in 0..10 {
            player.tick(&world);
            assert!(player.on_ground);
            assert_eq!(player.body.position.y, 1.0);
        }
    }

    #[test]
    fn walks_at_minecraft_speed_in_any_direction() {
        let world = floor();
        for walk in [Vec2::Y, Vec2::X, Vec2::new(1.0, 1.0), Vec2::new(-0.5, 0.0)] {
            let mut player = Player::new(Vec3::new(8.5, 1.0, 8.5), 0.0);
            player.controls.walk = walk;
            player.tick(&world);
            let moved = player.body.position - Vec3::new(8.5, 1.0, 8.5);
            let expected = Vec3::new(walk.x, 0.0, -walk.y).clamp_length_max(1.0);
            assert!(
                moved.abs_diff_eq(expected * WALK_SPEED / TICK_RATE, 1e-5),
                "{walk}"
            );
        }
    }

    #[test]
    fn minecraft_jump_velocity_peaks_at_its_usual_height() {
        assert!((peak_height(0.42) - 1.2522).abs() < 1e-3);
    }

    #[test]
    fn jumps_peak_at_the_requested_height() {
        let world = floor();
        for height in [0.5, 0.8, 1.25] {
            let mut player = Player::new(Vec3::new(8.5, 1.0, 8.5), 0.0);
            while !player.on_ground {
                player.tick(&world);
            }
            player.controls.jump = Some(height);
            let mut peak: f32 = 0.0;
            while {
                player.tick(&world);
                assert_eq!(player.controls.jump, None, "a request is used or dropped");
                // Asking again in mid-air does nothing.
                player.controls.jump = Some(height);
                !player.on_ground
            } {
                peak = peak.max(player.body.position.y - 1.0);
            }
            assert!((peak - height).abs() < 1e-3, "{height}: {peak}");
        }
    }
}
