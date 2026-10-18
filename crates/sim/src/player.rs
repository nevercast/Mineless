//! A player in the simulation: a collision body driven only through its `Controls`, stepped at a
//! fixed tick and drawn as an avatar interpolated between ticks.

use glam::{BVec3, IVec3, Vec2, Vec3};

use crate::avatar::{Avatar, Pose, Skin};
use crate::inventory::{HOTBAR, Inventory};
use crate::mining;
use crate::physics::Body;
use crate::world::{Axis, Block, Facing, Hit, World};

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
/// Blocks from the feet to the eyes.
pub const EYE_HEIGHT: f32 = 1.62;
/// How far from the eyes a player can reach a block, as in Minecraft.
pub const REACH: f32 = 4.5;
/// Ticks between blocks placed while place is held, as in Minecraft.
const PLACE_INTERVAL: u32 = 4;

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
    /// Held to break the targeted block, which takes as long as `mining::break_ticks` says. Looking away starts over.
    pub attack: bool,
    /// Held to place a block from the selected hotbar slot against the targeted face, one at once and then one every
    /// `PLACE_INTERVAL` ticks. Stairs step up the way the head looks, as in Minecraft. A block isn't placed into a solid block, outside the world, or where any player stands.
    pub place: bool,
    /// The hotbar slot placing takes from, in `0..HOTBAR`; anything higher means the last.
    pub slot: usize,
}

pub struct Player {
    pub controls: Controls,
    pub skin: Skin,
    pub inventory: Inventory,
    body: Body,
    /// The block attack has been held on, and for how many ticks.
    breaking: Option<(IVec3, u32)>,
    /// Blocks broken so far, which picks each one's drop.
    broken: u32,
    /// Ticks until place, held, places again.
    place_wait: u32,
    /// Vertical speed in blocks per tick; up is positive.
    fall_velocity: f32,
    on_ground: bool,
    blocked: BVec3,
    swing: f32,
    swing_phase: f32,
    /// Position and limb animation at the start of the current tick, to interpolate from.
    previous: (Vec3, f32, f32),
}

impl Player {
    pub fn new(feet: Vec3, yaw: f32, skin: Skin) -> Self {
        Self {
            skin,
            controls: Controls { yaw, ..Default::default() },
            inventory: Inventory::default(),
            breaking: None,
            broken: 0,
            place_wait: 0,
            body: Body {
                position: feet,
                half_width: 0.3,
                height: 1.8,
            },
            fall_velocity: 0.0,
            on_ground: false,
            blocked: BVec3::FALSE,
            swing: 0.0,
            swing_phase: 0.0,
            previous: (feet, 0.0, 0.0),
        }
    }

    /// Whether the last tick ended standing on a block.
    pub fn is_on_ground(&self) -> bool {
        self.on_ground
    }

    /// Which axes collision cut the last tick's movement short on.
    pub fn blocked(&self) -> BVec3 {
        self.blocked
    }

    /// Vertical speed in blocks per tick; up is positive.
    pub fn vertical_velocity(&self) -> f32 {
        self.fall_velocity
    }

    /// The block the player is looking at within reach, if any. The world border can't be aimed at, nor anything past it.
    pub fn target(&self, world: &World) -> Option<Hit> {
        let eye = self.body.position + Vec3::Y * EYE_HEIGHT;
        let hit = world.raycast(eye, self.look_direction(), REACH)?;
        (world.block(hit.block) != Block::WorldBorder).then_some(hit)
    }

    /// Unit vector the head looks along.
    fn look_direction(&self) -> Vec3 {
        let (sin_yaw, cos_yaw) = (self.controls.yaw + self.controls.head_yaw).sin_cos();
        let (sin_pitch, cos_pitch) = self.controls.head_pitch.sin_cos();
        Vec3::new(-sin_yaw * cos_pitch, sin_pitch, -cos_yaw * cos_pitch)
    }

    /// Whether the player's body takes up any of the block at `position`.
    pub(crate) fn overlaps_block(&self, position: IVec3) -> bool {
        self.body.overlaps_block(position)
    }

    /// Advances one tick. `occupied` says whether other bodies take up a block, so nothing is placed into them.
    pub(crate) fn tick(&mut self, world: &mut World, occupied: impl Fn(IVec3) -> bool) {
        self.previous = (self.body.position, self.swing, self.swing_phase);
        self.break_target(world);
        self.place_on_target(world, occupied);
        let controls = self.controls;
        if let Some(height) = self.controls.jump.take()
            && self.on_ground
        {
            self.fall_velocity = launch_velocity(height.clamp(*JUMP_HEIGHTS.start(), *JUMP_HEIGHTS.end()));
        }
        let (sin, cos) = controls.yaw.sin_cos();
        let forward = Vec3::new(-sin, 0.0, -cos);
        let right = Vec3::new(cos, 0.0, -sin);
        let walk = controls.walk.clamp_length_max(1.0);
        let step = (forward * walk.y + right * walk.x) * WALK_SPEED / TICK_RATE;
        let step = step.with_y(self.fall_velocity);
        self.blocked = self.body.move_and_collide(world, step);
        // Like Minecraft, move first and then accelerate, so a body at rest drops a tick later.
        // Gravity applies even on the ground, keeping a standing body pressed onto it.
        self.on_ground = self.blocked.y && step.y < 0.0;
        let velocity = if self.blocked.y { 0.0 } else { self.fall_velocity };
        self.fall_velocity = (velocity - GRAVITY) * AIR_DRAG;

        let walked = (self.body.position - self.previous.0).with_y(0.0).length();
        self.swing += ((walked * 4.0).min(1.0) - self.swing) * SWING_EASE;
        self.swing_phase += self.swing * STRIDE_PER_TICK;
    }

    /// Holds attack on the targeted block for another tick, and breaks it into the inventory once it has been held long enough.
    /// A drop that doesn't fit is lost.
    fn break_target(&mut self, world: &mut World) {
        let Some(hit) = self.target(world).filter(|_| self.controls.attack) else {
            self.breaking = None;
            return;
        };
        let held = match self.breaking {
            Some((block, ticks)) if block == hit.block => ticks + 1,
            _ => 1,
        };
        let block = world.block(hit.block);
        if mining::break_ticks(block).is_some_and(|needed| held >= needed) {
            world.set_block(hit.block, Block::Air);
            if let Some(drop) = mining::drop(block, hit.block, self.broken) {
                self.inventory.add(drop, 1);
            }
            self.broken += 1;
            self.breaking = None;
        } else {
            self.breaking = Some((hit.block, held));
        }
    }

    /// Places a block from the selected slot in the open cell in front of the targeted face, if place is held and it's time.
    fn place_on_target(&mut self, world: &mut World, occupied: impl Fn(IVec3) -> bool) {
        if !self.controls.place {
            self.place_wait = 0;
            return;
        }
        if self.place_wait > 0 {
            self.place_wait -= 1;
            return;
        }
        let Some(hit) = self.target(world) else {
            return;
        };
        let cell = hit.block + hit.face;
        let slot = self.controls.slot.min(HOTBAR - 1);
        let Some(stack) = self.inventory.slot(slot) else {
            return;
        };
        if !world.is_loaded(cell) || world.block(cell).is_solid() || self.body.overlaps_block(cell) || occupied(cell) {
            return;
        }
        let block = match stack.block {
            Block::OakLog(_) if hit.face.x != 0 => Block::OakLog(Axis::X),
            Block::OakLog(_) if hit.face.z != 0 => Block::OakLog(Axis::Z),
            Block::OakLog(_) => Block::OakLog(Axis::Y),
            Block::OakStairs(_) => {
                let (sin, cos) = (self.controls.yaw + self.controls.head_yaw).sin_cos();
                Block::OakStairs(Facing::of(Vec3::new(-sin, 0.0, -cos)))
            }
            block => block,
        };
        self.inventory.take(slot);
        world.set_block(cell, block);
        self.place_wait = PLACE_INTERVAL - 1;
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
            skin: self.skin,
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
    use crate::inventory::Stack;

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
        let mut world = floor();
        let mut player = Player::new(Vec3::new(8.5, 51.0, 8.5), 0.0, Skin::Player);
        let mut heights = Vec::new();
        while !player.on_ground {
            player.tick(&mut world, |_| false);
            heights.push(player.body.position.y);
        }
        // Minecraft's first ticks of a fall from rest: 0, 0.0784, 0.2337, 0.4643 blocks.
        for (height, fallen) in heights.iter().zip([0.0, 0.0784, 0.2337, 0.4643]) {
            assert!((51.0 - height - fallen).abs() < 1e-3, "{heights:?}");
        }
        assert_eq!(player.body.position.y, 1.0);
    }

    #[test]
    fn sinks_through_water_to_the_floor() {
        let mut world = floor();
        for x in 0..16 {
            for z in 0..16 {
                for y in 1..6 {
                    world.set_block(IVec3::new(x, y, z), Block::Water);
                }
            }
        }
        let mut player = Player::new(Vec3::new(8.5, 8.0, 8.5), 0.0, Skin::Player);
        for _ in 0..100 {
            player.tick(&mut world, |_| false);
        }
        assert!(player.on_ground);
        assert_eq!(player.body.position.y, 1.0);
    }

    #[test]
    fn falling_speed_settles_at_terminal_velocity() {
        let mut world = World::empty(IVec3::ONE);
        let mut player = Player::new(Vec3::new(8.5, 0.0, 8.5), 0.0, Skin::Player);
        for _ in 0..400 {
            player.tick(&mut world, |_| false);
        }
        assert!((player.fall_velocity + 3.92).abs() < 1e-2);
        assert!(!player.on_ground);
    }

    #[test]
    fn standing_stays_on_the_ground_every_tick() {
        let mut world = floor();
        let mut player = Player::new(Vec3::new(8.5, 1.0, 8.5), 0.0, Skin::Player);
        player.tick(&mut world, |_| false);
        for _ in 0..10 {
            player.tick(&mut world, |_| false);
            assert!(player.on_ground);
            assert_eq!(player.body.position.y, 1.0);
        }
    }

    #[test]
    fn walks_at_minecraft_speed_in_any_direction() {
        let mut world = floor();
        for walk in [Vec2::Y, Vec2::X, Vec2::new(1.0, 1.0), Vec2::new(-0.5, 0.0)] {
            let mut player = Player::new(Vec3::new(8.5, 1.0, 8.5), 0.0, Skin::Player);
            player.controls.walk = walk;
            player.tick(&mut world, |_| false);
            let moved = player.body.position - Vec3::new(8.5, 1.0, 8.5);
            let expected = Vec3::new(walk.x, 0.0, -walk.y).clamp_length_max(1.0);
            assert!(moved.abs_diff_eq(expected * WALK_SPEED / TICK_RATE, 1e-5), "{walk}");
        }
    }

    #[test]
    fn the_world_border_stops_walking_and_jumping() {
        let mut world = floor();
        for y in 1..64 {
            for z in 0..16 {
                world.set_block(IVec3::new(12, y, z), Block::WorldBorder);
            }
        }
        for jumping in [false, true] {
            let mut player = Player::new(Vec3::new(8.5, 1.0, 8.5), 0.0, Skin::Player);
            player.controls.walk = Vec2::X;
            for _ in 0..200 {
                if jumping {
                    player.controls.jump = Some(*JUMP_HEIGHTS.end());
                }
                player.tick(&mut world, |_| false);
                assert!(player.body.position.x < 12.0, "jumping {jumping}");
            }
            assert!(player.body.position.x > 11.0, "jumping {jumping}");
        }
    }

    #[test]
    fn targets_the_block_the_head_looks_at() {
        let mut world = floor();
        world.set_block(IVec3::new(8, 2, 5), Block::Stone);
        let mut player = Player::new(Vec3::new(8.5, 1.0, 8.5), 0.0, Skin::Player);
        // Facing -Z at eye height, the block three ahead and one up is in reach.
        assert_eq!(
            player.target(&world),
            Some(Hit {
                block: IVec3::new(8, 2, 5),
                face: IVec3::Z
            })
        );
        // Turning the head to the right and down onto the floor.
        player.controls.head_yaw = -std::f32::consts::FRAC_PI_2;
        player.controls.head_pitch = -std::f32::consts::FRAC_PI_4;
        assert_eq!(
            player.target(&world).map(|hit| (hit.block, hit.face)),
            Some((IVec3::new(10, 0, 8), IVec3::Y))
        );
        // Straight up there is nothing.
        player.controls.head_pitch = std::f32::consts::FRAC_PI_2;
        assert_eq!(player.target(&world), None);
    }

    #[test]
    fn the_world_border_cannot_be_targeted() {
        let mut world = floor();
        world.set_block(IVec3::new(8, 2, 6), Block::WorldBorder);
        world.set_block(IVec3::new(8, 2, 5), Block::Stone);
        let player = Player::new(Vec3::new(8.5, 1.0, 8.5), 0.0, Skin::Player);
        assert_eq!(player.target(&world), None);
    }

    #[test]
    fn holding_attack_breaks_the_target_in_its_time_and_keeps_the_drop() {
        let mut world = floor();
        world.set_block(IVec3::new(8, 2, 6), Block::Stone);
        let mut player = Player::new(Vec3::new(8.5, 1.0, 8.5), 0.0, Skin::Player);
        player.controls.attack = true;
        for _ in 0..44 {
            player.tick(&mut world, |_| false);
        }
        assert_eq!(world.block(IVec3::new(8, 2, 6)), Block::Stone);
        player.tick(&mut world, |_| false);
        assert_eq!(world.block(IVec3::new(8, 2, 6)), Block::Air);
        assert_eq!(
            player.inventory.slot(0),
            Some(Stack {
                block: Block::Cobblestone,
                count: 1
            })
        );
    }

    #[test]
    fn looking_away_or_letting_go_starts_breaking_over() {
        let mut world = floor();
        world.set_block(IVec3::new(8, 2, 6), Block::Dirt);
        let mut player = Player::new(Vec3::new(8.5, 1.0, 8.5), 0.0, Skin::Player);
        player.controls.attack = true;
        for _ in 0..14 {
            player.tick(&mut world, |_| false);
        }
        player.controls.attack = false;
        player.tick(&mut world, |_| false);
        player.controls.attack = true;
        for _ in 0..14 {
            player.tick(&mut world, |_| false);
        }
        assert_eq!(
            world.block(IVec3::new(8, 2, 6)),
            Block::Dirt,
            "fifteen ticks held, but not in a row"
        );
        // Looking down at the floor for a tick, then back.
        player.controls.head_pitch = -1.0;
        player.tick(&mut world, |_| false);
        player.controls.head_pitch = 0.0;
        for _ in 0..14 {
            player.tick(&mut world, |_| false);
        }
        assert_eq!(world.block(IVec3::new(8, 2, 6)), Block::Dirt);
        player.tick(&mut world, |_| false);
        assert_eq!(world.block(IVec3::new(8, 2, 6)), Block::Air);
    }

    #[test]
    fn placing_puts_the_selected_block_against_the_targeted_face() {
        let mut world = floor();
        world.set_block(IVec3::new(8, 2, 5), Block::Stone);
        let mut player = Player::new(Vec3::new(8.5, 1.0, 8.5), 0.0, Skin::Player);
        player.inventory.add(Block::Dirt, 1);
        player.inventory.add(Block::OakLog(Axis::Y), 2);
        player.controls.place = true;
        player.controls.slot = 1;
        player.tick(&mut world, |_| false);
        assert_eq!(
            world.block(IVec3::new(8, 2, 6)),
            Block::OakLog(Axis::Z),
            "the grain runs out from the face"
        );
        // Held on: the next goes on the log just placed, a few ticks later.
        for _ in 0..PLACE_INTERVAL - 1 {
            player.tick(&mut world, |_| false);
        }
        assert_eq!(world.block(IVec3::new(8, 2, 7)), Block::Air);
        player.tick(&mut world, |_| false);
        assert_eq!(world.block(IVec3::new(8, 2, 7)), Block::OakLog(Axis::Z));
        assert_eq!(player.inventory.slot(1), None);
        // Nothing is placed from an empty slot, nor where the player stands.
        player.controls.slot = 7;
        player.tick(&mut world, |_| false);
        player.controls.slot = 0;
        player.controls.head_pitch = -std::f32::consts::FRAC_PI_2;
        for _ in 0..10 {
            player.tick(&mut world, |_| false);
        }
        assert_eq!(world.block(IVec3::new(8, 1, 8)), Block::Air);
        assert_eq!(player.inventory.slot(0).map(|stack| stack.count), Some(1));
    }

    #[test]
    fn jumping_and_placing_underfoot_builds_a_pillar() {
        let mut world = World::empty(IVec3::new(1, 2, 1));
        for x in 0..16 {
            for z in 0..16 {
                world.set_block(IVec3::new(x, 0, z), Block::Stone);
            }
        }
        let mut player = Player::new(Vec3::new(8.5, 1.0, 8.5), 0.0, Skin::Player);
        player.inventory.add(Block::Cobblestone, 5);
        player.controls.place = true;
        player.controls.head_pitch = -std::f32::consts::FRAC_PI_2;
        for _ in 0..400 {
            if player.on_ground && player.inventory.slot(0).is_some() {
                player.controls.jump = Some(*JUMP_HEIGHTS.end());
            }
            player.tick(&mut world, |_| false);
        }
        for y in 1..=5 {
            assert_eq!(world.block(IVec3::new(8, y, 8)), Block::Cobblestone, "{y}");
        }
        assert_eq!(player.body.position.y, 6.0);
    }

    #[test]
    fn minecraft_jump_velocity_peaks_at_its_usual_height() {
        assert!((peak_height(0.42) - 1.2522).abs() < 1e-3);
    }

    #[test]
    fn jumps_peak_at_the_requested_height() {
        let mut world = floor();
        for height in [0.5, 0.8, 1.25] {
            let mut player = Player::new(Vec3::new(8.5, 1.0, 8.5), 0.0, Skin::Player);
            while !player.on_ground {
                player.tick(&mut world, |_| false);
            }
            player.controls.jump = Some(height);
            let mut peak: f32 = 0.0;
            while {
                player.tick(&mut world, |_| false);
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
