use glam::{IVec3, Vec3};

use super::World;

/// The block a ray stopped at, and the face it came in through.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Hit {
    pub block: IVec3,
    /// Unit normal of the face the ray entered through, so `block + face` is the open cell in front of it.
    pub face: IVec3,
}

impl World {
    /// The first solid block a ray from `origin` along `direction` enters within `reach` blocks. The block the ray starts in
    /// doesn't count: an eye is never inside a solid block.
    pub fn raycast(&self, origin: Vec3, direction: Vec3, reach: f32) -> Option<Hit> {
        let direction = direction.normalize_or_zero();
        let mut cell = origin.floor().as_ivec3();
        // Steps cell by cell through the grid, always crossing whichever cell boundary along the ray comes next.
        let mut step = IVec3::ZERO;
        let mut next = Vec3::INFINITY;
        let mut spacing = Vec3::INFINITY;
        for axis in 0..3 {
            if direction[axis] == 0.0 {
                continue;
            }
            step[axis] = if direction[axis] > 0.0 { 1 } else { -1 };
            let boundary = cell[axis] as f32 + if direction[axis] > 0.0 { 1.0 } else { 0.0 };
            next[axis] = (boundary - origin[axis]) / direction[axis];
            spacing[axis] = 1.0 / direction[axis].abs();
        }
        loop {
            let axis = if next.x <= next.y && next.x <= next.z {
                0
            } else if next.y <= next.z {
                1
            } else {
                2
            };
            if next[axis] > reach {
                return None;
            }
            cell[axis] += step[axis];
            next[axis] += spacing[axis];
            if self.block(cell).is_solid() {
                let mut face = IVec3::ZERO;
                face[axis] = -step[axis];
                return Some(Hit { block: cell, face });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::Block;

    /// A stone floor at y = 0 and one stone block at (5, 1, 5).
    fn world() -> World {
        let mut world = World::empty(IVec3::ONE);
        for x in 0..16 {
            for z in 0..16 {
                world.set_block(IVec3::new(x, 0, z), Block::Stone);
            }
        }
        world.set_block(IVec3::new(5, 1, 5), Block::Stone);
        world
    }

    #[test]
    fn hits_the_face_it_enters_through() {
        let world = world();
        let block = IVec3::new(5, 1, 5);
        for (origin, face) in [
            (Vec3::new(2.5, 1.5, 5.5), IVec3::NEG_X),
            (Vec3::new(8.5, 1.5, 5.5), IVec3::X),
            (Vec3::new(5.5, 1.5, 2.5), IVec3::NEG_Z),
            (Vec3::new(5.5, 4.5, 5.5), IVec3::Y),
        ] {
            let hit = world.raycast(origin, block.as_vec3() + 0.5 - origin, 4.5);
            assert_eq!(hit, Some(Hit { block, face }), "{origin}");
        }
    }

    #[test]
    fn looks_down_at_the_floor_at_an_angle() {
        let hit = world().raycast(Vec3::new(1.5, 2.62, 1.5), Vec3::new(1.0, -1.0, 0.25), 4.5);
        assert_eq!(
            hit,
            Some(Hit {
                block: IVec3::new(3, 0, 1),
                face: IVec3::Y
            })
        );
    }

    #[test]
    fn stops_at_the_reach() {
        let world = world();
        let origin = Vec3::new(0.5, 1.5, 5.5);
        // The block's near face is 4.5 blocks away.
        assert!(world.raycast(origin, Vec3::X, 4.49).is_none());
        assert_eq!(world.raycast(origin, Vec3::X, 4.5).map(|hit| hit.block), Some(IVec3::new(5, 1, 5)));
    }

    #[test]
    fn passes_through_air_and_water_and_ignores_its_start() {
        let mut world = world();
        for x in 1..5 {
            world.set_block(IVec3::new(x, 1, 5), Block::Water);
        }
        world.set_block(IVec3::new(0, 1, 5), Block::Stone);
        let hit = world.raycast(Vec3::new(0.5, 1.5, 5.5), Vec3::X, 10.0);
        assert_eq!(hit.map(|hit| hit.block), Some(IVec3::new(5, 1, 5)));
        assert!(world.raycast(Vec3::new(0.5, 1.5, 0.5), Vec3::Y, 10.0).is_none());
    }
}
