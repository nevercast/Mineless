//! Collision between moving boxes and the block grid, resolved the way Minecraft does it: the
//! move is clipped one axis at a time, Y first, then X, then Z.

use glam::{BVec3, IVec3, Vec3};

use crate::world::World;

/// How far boxes may sink into each other through rounding and still count as touching, not
/// overlapping. Without it a box left a hair inside a face it rested on is no longer stopped by it.
const EPSILON: f32 = 1e-4;

#[derive(Clone, Copy, Debug)]
pub struct Aabb {
    pub min: Vec3,
    pub max: Vec3,
}

impl Aabb {
    fn block(position: IVec3) -> Self {
        let min = position.as_vec3();
        Self {
            min,
            max: min + Vec3::ONE,
        }
    }

    /// How far this box can move along `axis` towards `other`, limited to `distance`. Boxes that
    /// don't overlap on the other two axes never block each other.
    fn clip(&self, other: &Aabb, axis: usize, distance: f32) -> f32 {
        let overlaps_across = (0..3)
            .filter(|&a| a != axis)
            .all(|a| self.max[a] - EPSILON > other.min[a] && self.min[a] + EPSILON < other.max[a]);
        if !overlaps_across {
            distance
        } else if distance > 0.0 && self.max[axis] - EPSILON <= other.min[axis] {
            distance.min(other.min[axis] - self.max[axis])
        } else if distance < 0.0 && self.min[axis] + EPSILON >= other.max[axis] {
            distance.max(other.max[axis] - self.min[axis])
        } else {
            distance
        }
    }
}

/// A box standing on its base: `position` is the centre of the bottom face.
pub struct Body {
    pub position: Vec3,
    pub half_width: f32,
    pub height: f32,
}

impl Body {
    pub fn aabb(&self) -> Aabb {
        let half = Vec3::new(self.half_width, 0.0, self.half_width);
        Aabb {
            min: self.position - half,
            max: self.position + half + Vec3::Y * self.height,
        }
    }

    /// Moves by up to `delta`, stopping flush against any solid block in the way. Returns which
    /// axes were cut short.
    pub fn move_and_collide(&mut self, world: &World, delta: Vec3) -> BVec3 {
        let start = self.aabb();
        let swept = Aabb {
            min: start.min + delta.min(Vec3::ZERO),
            max: start.max + delta.max(Vec3::ZERO),
        };
        let blocks: Vec<Aabb> = blocks_touching(&swept)
            .filter(|&position| world.block(position).is_solid())
            .map(Aabb::block)
            .collect();

        let mut aabb = start;
        let mut moved = Vec3::ZERO;
        for axis in [1, 0, 2] {
            let distance = blocks.iter().fold(delta[axis], |distance, block| {
                aabb.clip(block, axis, distance)
            });
            aabb.min[axis] += distance;
            aabb.max[axis] += distance;
            moved[axis] = distance;
        }
        self.position += moved;
        moved.cmpne(delta)
    }
}

fn blocks_touching(aabb: &Aabb) -> impl Iterator<Item = IVec3> {
    let min = aabb.min.floor().as_ivec3();
    let max = aabb.max.floor().as_ivec3();
    (min.x..=max.x).flat_map(move |x| {
        (min.y..=max.y).flat_map(move |y| (min.z..=max.z).map(move |z| IVec3::new(x, y, z)))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::Block;

    /// A stone floor at y = 0 with a stone wall along x = 3.
    fn room() -> World {
        let mut world = World::empty(IVec3::ONE);
        for x in 0..16 {
            for z in 0..16 {
                world.set_block(IVec3::new(x, 0, z), Block::Stone);
            }
        }
        for y in 1..4 {
            for z in 0..16 {
                world.set_block(IVec3::new(3, y, z), Block::Stone);
            }
        }
        world
    }

    fn body_at(x: f32, y: f32, z: f32) -> Body {
        Body {
            position: Vec3::new(x, y, z),
            half_width: 0.3,
            height: 1.8,
        }
    }

    #[test]
    fn lands_on_the_floor() {
        let mut body = body_at(1.5, 1.5, 5.5);
        let blocked = body.move_and_collide(&room(), Vec3::new(0.0, -2.0, 0.0));
        assert!(body.position.abs_diff_eq(Vec3::new(1.5, 1.0, 5.5), 1e-5));
        assert_eq!(blocked, BVec3::new(false, true, false));
    }

    #[test]
    fn stops_flush_against_a_wall_and_slides_along_it() {
        let mut body = body_at(1.5, 1.0, 5.5);
        let blocked = body.move_and_collide(&room(), Vec3::new(2.0, 0.0, 0.5));
        assert!(body.position.abs_diff_eq(Vec3::new(2.7, 1.0, 6.0), 1e-5));
        assert_eq!(blocked, BVec3::new(true, false, false));
    }

    #[test]
    fn rounding_past_a_face_does_not_let_a_body_through() {
        let mut body = body_at(2.7 + 1e-5, 1.0 - 1e-5, 5.5);
        body.move_and_collide(&room(), Vec3::new(0.5, 0.0, 0.0));
        assert!(body.aabb().max.x <= 3.0);
        let blocked = body.move_and_collide(&room(), Vec3::new(0.0, 0.0, 0.5));
        assert_eq!(
            blocked,
            BVec3::FALSE,
            "resting a hair inside the floor doesn't snag"
        );
    }

    #[test]
    fn moves_freely_through_air() {
        let mut body = body_at(1.5, 1.0, 5.5);
        let blocked = body.move_and_collide(&room(), Vec3::new(-0.5, 0.25, 0.5));
        assert!(body.position.abs_diff_eq(Vec3::new(1.0, 1.25, 6.0), 1e-5));
        assert_eq!(blocked, BVec3::FALSE);
    }
}
