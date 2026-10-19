//! Aiming: which way to look to see a point, and what a look would hit.

use agent_api::{Block, EYE_HEIGHT, REACH};

use crate::{Cell, solid};

/// Where the eyes are when the feet stand in the middle of `cell`'s floor.
pub fn eye([x, y, z]: Cell) -> [f32; 3] {
    [x as f32 + 0.5, y as f32 + EYE_HEIGHT, z as f32 + 0.5]
}

/// The yaw and pitch that look from `from` at `to`, in the game's terms: yaw turns left from facing -Z, pitch is above level.
pub fn look(from: [f32; 3], to: [f32; 3]) -> (f32, f32) {
    let [dx, dy, dz] = [0, 1, 2].map(|axis| to[axis] - from[axis]);
    ((-dx).atan2(-dz), dy.atan2((dx * dx + dz * dz).sqrt()))
}

/// The middle of `cell`, nudged `side` half a block onto the middle of one of its faces.
pub fn face_middle(cell: Cell, side: Cell) -> [f32; 3] {
    [0, 1, 2].map(|axis| cell[axis] as f32 + 0.5 + side[axis] as f32 * 0.5)
}

/// The first solid cell a look from `from` towards `to` enters within reach, and the side it enters through. Stairs are taken
/// for whole blocks, which the game's own look then checks. The cell the look starts in doesn't count.
pub fn ray(world: &impl Fn(Cell) -> Option<Block>, from: [f32; 3], to: [f32; 3]) -> Option<(Cell, Cell)> {
    let mut direction = [0, 1, 2].map(|axis| to[axis] - from[axis]);
    let length = direction.iter().map(|d| d * d).sum::<f32>().sqrt();
    if length == 0.0 {
        return None;
    }
    direction = direction.map(|d| d / length);
    let mut cell = from.map(|p| p.floor() as i32);
    let mut step = [0; 3];
    let mut next = [f32::INFINITY; 3];
    let mut spacing = [f32::INFINITY; 3];
    for axis in 0..3 {
        if direction[axis] == 0.0 {
            continue;
        }
        step[axis] = if direction[axis] > 0.0 { 1 } else { -1 };
        let boundary = cell[axis] as f32 + if direction[axis] > 0.0 { 1.0 } else { 0.0 };
        next[axis] = (boundary - from[axis]) / direction[axis];
        spacing[axis] = 1.0 / direction[axis].abs();
    }
    loop {
        let axis = (0..3).min_by(|&a, &b| next[a].total_cmp(&next[b])).expect("there are three axes");
        if next[axis] > REACH {
            return None;
        }
        cell[axis] += step[axis];
        next[axis] += spacing[axis];
        if solid(world(cell)) || world(cell).is_none() {
            let mut side = [0; 3];
            side[axis] = -step[axis];
            return Some((cell, side));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::f32::consts::FRAC_PI_2;

    use super::*;
    use crate::tests::world;

    #[test]
    fn looking_turns_left_from_north_and_up_from_level() {
        let (yaw, pitch) = look([0.0; 3], [-1.0, 0.0, 0.0]);
        assert!((yaw - FRAC_PI_2).abs() < 1e-6 && pitch == 0.0);
        let (yaw, pitch) = look([0.0; 3], [0.0, 1.0, -1.0]);
        assert!(yaw.abs() < 1e-6 && (pitch - FRAC_PI_2 / 2.0).abs() < 1e-6);
    }

    #[test]
    fn a_look_stops_at_the_first_solid_face() {
        let world = world(&[([3, 2, 0], Block::OakPlanks), ([5, 2, 0], Block::OakPlanks)]);
        let from = eye([0, 1, 0]);
        assert_eq!(ray(&world, from, [5.0, from[1], 0.5]), Some(([3, 2, 0], [-1, 0, 0])));
        // Down onto the ground, through the top.
        assert_eq!(ray(&world, from, face_middle([2, 0, 0], [0, 1, 0])), Some(([2, 0, 0], [0, 1, 0])));
        assert_eq!(ray(&world, from, face_middle([0, 1, 9], [0, 0, -1])), None, "out of reach");
    }
}
