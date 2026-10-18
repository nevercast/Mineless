//! Sample script: wanders between grass blocks. It picks one at random a little way off and walks toward it a block at a time,
//! stepping to whichever neighbouring block looks best, and picks another once it gets there or stops getting any closer.
//! There's no path finding: a wall it can't see round is where it gives up.

use agent_api::Block;

/// A block position: `[x, y, z]`.
type Cell = [i32; 3];

/// How far a step may climb, which takes a jump, and drop.
const CLIMB: i32 = 1;
const DROP: i32 = 3;
/// What a step costs on top of the distance to the target it leaves, in blocks: jumping, wading, and going back to a block it
/// stood in recently, so it edges round small obstacles instead of rocking between two blocks.
const JUMP_COST: f32 = 0.5;
const WATER_COST: f32 = 3.0;
const TRAIL_COST: f32 = 2.0;
/// The eight neighbouring columns, straight first so a tie goes straight.
const NEIGHBOURS: [(i32, i32); 8] = [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)];

/// The neighbouring block to stand in next on the way to `target`, from standing in `here`: whichever leaves it closest, once
/// the costs of getting there are added. `None` if no neighbour can be stood in. `trail` is where it has stood lately.
fn next_step(block: &impl Fn(Cell) -> Option<Block>, here: Cell, target: Cell, trail: &[Cell]) -> Option<Cell> {
    let [x, y, z] = here;
    let headroom = clear(block([x, y + 2, z]));
    NEIGHBOURS
        .iter()
        .filter_map(|&(dx, dz)| {
            let top = if headroom { y + CLIMB } else { y };
            let step_y = stand(block, x + dx, z + dz, top, y - DROP)?;
            // A diagonal step brushes both straight neighbours, so they must be open at the height it passes through.
            let pass = y.max(step_y);
            let brushed = [[x + dx, pass, z], [x, pass, z + dz]];
            if dx != 0
                && dz != 0
                && !brushed
                    .iter()
                    .all(|&[bx, by, bz]| clear(block([bx, by, bz])) && clear(block([bx, by + 1, bz])))
            {
                return None;
            }
            let step = [x + dx, step_y, z + dz];
            let mut cost = distance(step, target);
            if step_y > y {
                cost += JUMP_COST;
            }
            if block(step) == Some(Block::Water) {
                cost += WATER_COST;
            }
            cost += TRAIL_COST * trail.iter().filter(|&&cell| cell == step).count() as f32;
            Some((step, cost))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(step, _)| step)
}

/// The highest block from `top` down to `bottom` in a column that a body can stand in: solid underneath, with room for the
/// body.
fn stand(block: &impl Fn(Cell) -> Option<Block>, x: i32, z: i32, top: i32, bottom: i32) -> Option<i32> {
    (bottom..=top)
        .rev()
        .find(|&y| solid(block([x, y - 1, z])) && clear(block([x, y, z])) && clear(block([x, y + 1, z])))
}

/// Where to stand on the grass at the top of a column, looking `reach` blocks above and below `near_y`: `None` if the column's
/// first solid block there isn't grass, or it's under water.
fn on_grass(block: &impl Fn(Cell) -> Option<Block>, x: i32, z: i32, near_y: i32, reach: i32) -> Option<Cell> {
    let top = (near_y - reach..=near_y + reach).rev().find(|&y| !clear(block([x, y, z])))?;
    (block([x, top, z]) == Some(Block::Grass) && block([x, top + 1, z]) == Some(Block::Air)).then_some([x, top + 1, z])
}

/// Bodies pass through it. Blocks out of sight are neither clear nor solid, so nothing is planned through them.
fn clear(block: Option<Block>) -> bool {
    matches!(block, Some(Block::Air | Block::Water))
}

fn solid(block: Option<Block>) -> bool {
    block.is_some_and(|block| !clear(Some(block)))
}

fn distance(a: Cell, b: Cell) -> f32 {
    let [x, y, z] = [0, 1, 2].map(|axis| (a[axis] - b[axis]) as f32);
    (x * x + y * y + z * z).sqrt()
}

#[cfg(target_arch = "wasm32")]
mod play {
    use std::f32::consts::{PI, TAU};
    use std::ops::RangeInclusive;

    use agent_api::{Controls, block, log, observe, set_controls, wait_ticks};

    use super::{Cell, next_step, on_grass};

    /// How far off across the ground a new target is.
    const TARGET_DISTANCE: RangeInclusive<i32> = 12..=40;
    /// How far above and below the feet it looks for the grass at a target.
    const TARGET_REACH: i32 = 16;
    /// Columns it tries each tick while looking for a target.
    const TARGET_TRIES: u32 = 16;
    /// Ticks it keeps going without getting any closer before it gives up on a target: five seconds.
    const PATIENCE: u32 = 100;
    /// How close the feet must come to the middle of the target.
    const ARRIVED: f32 = 0.8;
    /// Ticks it stands still on reaching a target, at random within this range.
    const REST_TICKS: RangeInclusive<u32> = 20..=60;
    /// The furthest the body turns in a tick.
    const TURN_PER_TICK: f32 = 25.0 * PI / 180.0;
    const JUMP: f32 = 1.25;
    /// Places it remembers standing, to avoid going back to.
    const TRAIL: usize = 16;

    #[unsafe(no_mangle)]
    pub extern "C" fn run() {
        let feet = observe().feet;
        let mut random = Random(feet[0].to_bits() ^ feet[2].to_bits().rotate_left(16) | 1);
        let mut controls = Controls::default();
        loop {
            let target = find_target(&mut random);
            if walk_to(target, &mut controls) {
                log(&format!("reached the grass at {target:?}"));
                controls.walk = [0.0; 2];
                set_controls(&controls);
                wait_ticks(random.within(REST_TICKS));
            } else {
                log(&format!("gave up on the grass at {target:?}"));
            }
        }
    }

    /// A random grass block to stand on, a tick at a time until one turns up.
    fn find_target(random: &mut Random) -> Cell {
        loop {
            let [x, y, z] = feet_cell();
            for _ in 0..TARGET_TRIES {
                let (dx, dz) = random.offset(&TARGET_DISTANCE);
                if let Some(target) = on_grass(&block, x + dx, z + dz, y, TARGET_REACH) {
                    return target;
                }
            }
            wait_ticks(1);
        }
    }

    /// Walks toward `target`, one tick at a time. True once it's there, false if it stopped getting closer first.
    fn walk_to(target: Cell, controls: &mut Controls) -> bool {
        let goal = [target[0] as f32 + 0.5, target[1] as f32, target[2] as f32 + 0.5];
        let mut trail: Vec<Cell> = Vec::with_capacity(TRAIL);
        let mut closest = f32::INFINITY;
        let mut stalled = 0;
        loop {
            let state = observe();
            let off = [0, 1, 2].map(|axis| goal[axis] - state.feet[axis]);
            let distance = off.iter().map(|d| d * d).sum::<f32>().sqrt();
            if distance < ARRIVED {
                return true;
            }
            if distance < closest {
                (closest, stalled) = (distance, 0);
            } else {
                stalled += 1;
                if stalled > PATIENCE {
                    return false;
                }
            }
            let here = feet_cell();
            if trail.last() != Some(&here) {
                if trail.len() == TRAIL {
                    trail.remove(0);
                }
                trail.push(here);
            }
            let Some(step) = next_step(&block, here, target, &trail) else {
                return false;
            };
            let toward = [step[0] as f32 + 0.5 - state.feet[0], step[2] as f32 + 0.5 - state.feet[2]];
            let facing = (-toward[0]).atan2(-toward[1]);
            let turn = ((facing - controls.yaw + PI).rem_euclid(TAU) - PI).clamp(-TURN_PER_TICK, TURN_PER_TICK);
            controls.yaw += turn;
            // Slows into sharp turns, so it doesn't overshoot a narrow way.
            controls.walk = [0.0, (facing - controls.yaw).cos().max(0.0)];
            let blocked = state.blocked[0] != 0 || state.blocked[2] != 0;
            let climbing = step[1] > here[1] || blocked;
            controls.jump = if climbing && state.on_ground != 0 { JUMP } else { 0.0 };
            set_controls(controls);
            wait_ticks(1);
        }
    }

    fn feet_cell() -> Cell {
        observe().feet.map(|axis| axis.floor() as i32)
    }

    /// A small xorshift generator: every wanderer seeds it from where it starts, so each goes its own way.
    struct Random(u32);

    impl Random {
        fn next(&mut self) -> u32 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 17;
            self.0 ^= self.0 << 5;
            self.0
        }

        fn within(&mut self, range: RangeInclusive<u32>) -> u32 {
            range.start() + self.next() % (range.end() - range.start() + 1)
        }

        /// A column offset whose distance across the ground is in `range`.
        fn offset(&mut self, range: &RangeInclusive<i32>) -> (i32, i32) {
            let (near, far) = (*range.start(), *range.end());
            let span = far.unsigned_abs() * 2 + 1;
            loop {
                let dx = (self.next() % span) as i32 - far;
                let dz = (self.next() % span) as i32 - far;
                if (near * near..=far * far).contains(&(dx * dx + dz * dz)) {
                    return (dx, dz);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    /// Flat grass with its top at y = 0, so bodies stand at y = 1, plus whatever `changes` puts in.
    fn world(changes: &[(Cell, Block)]) -> impl Fn(Cell) -> Option<Block> {
        let changes: HashMap<Cell, Block> = changes.iter().copied().collect();
        move |cell| {
            Some(changes.get(&cell).copied().unwrap_or(match cell[1] {
                ..0 => Block::Dirt,
                0 => Block::Grass,
                _ => Block::Air,
            }))
        }
    }

    fn wall(x: i32, zs: std::ops::RangeInclusive<i32>, height: i32) -> Vec<(Cell, Block)> {
        zs.flat_map(|z| (1..=height).map(move |y| ([x, y, z], Block::Stone))).collect()
    }

    #[test]
    fn on_open_ground_it_steps_straight_at_the_target() {
        assert_eq!(next_step(&world(&[]), [0, 1, 0], [10, 1, 0], &[]), Some([1, 1, 0]));
        assert_eq!(next_step(&world(&[]), [0, 1, 0], [10, 1, 10], &[]), Some([1, 1, 1]));
    }

    #[test]
    fn it_climbs_one_block_but_goes_round_two() {
        let step = wall(1, -5..=5, 1);
        assert_eq!(next_step(&world(&step), [0, 1, 0], [10, 1, 0], &[]), Some([1, 2, 0]));
        let high = wall(1, -5..=5, 2);
        let around = next_step(&world(&high), [0, 1, 0], [10, 1, 0], &[]).expect("open ground lies to the sides");
        assert_eq!(around[0], 0, "{around:?}");
    }

    #[test]
    fn it_climbs_only_with_room_to_jump() {
        let mut changes = wall(1, -5..=5, 1);
        changes.push(([0, 3, 0], Block::Stone));
        let step = next_step(&world(&changes), [0, 1, 0], [10, 1, 0], &[]).expect("open ground lies to the sides");
        assert_eq!(step[1], 1, "{step:?}");
    }

    #[test]
    fn it_doesnt_cut_a_corner() {
        let corner = [([1, 1, 0], Block::Stone), ([1, 2, 0], Block::Stone)];
        let step = next_step(&world(&corner), [0, 1, 0], [10, 1, 10], &[]);
        assert_eq!(step, Some([0, 1, 1]));
    }

    #[test]
    fn its_trail_turns_it_off_where_it_has_been() {
        let fresh = next_step(&world(&[]), [0, 1, 0], [10, 1, 0], &[]);
        let worn = next_step(&world(&[]), [0, 1, 0], [10, 1, 0], &[[1, 1, 0], [1, 1, 0]]);
        assert_eq!(fresh, Some([1, 1, 0]));
        assert_ne!(worn, fresh);
    }

    #[test]
    fn it_drops_a_little_way_but_not_off_a_cliff() {
        let hole = |depth: i32| -> Vec<(Cell, Block)> {
            (1 - depth..=0)
                .flat_map(|y| (1..=20).flat_map(move |x| (-20..=20).map(move |z| ([x, y, z], Block::Air))))
                .collect()
        };
        assert_eq!(next_step(&world(&hole(3)), [0, 1, 0], [10, -2, 0], &[]), Some([1, -2, 0]));
        let cliff = next_step(&world(&hole(4)), [0, 1, 0], [10, -3, 0], &[]).expect("open ground lies behind");
        assert!(cliff[0] <= 0, "{cliff:?}");
    }

    #[test]
    fn targets_are_open_grass_only() {
        let changes = [
            ([3, 0, 0], Block::Sand),
            ([4, 1, 0], Block::Water),
            ([5, 1, 0], Block::Stone),
            ([5, 2, 0], Block::Grass),
        ];
        let block = world(&changes);
        assert_eq!(on_grass(&block, 2, 0, 5, 16), Some([2, 1, 0]));
        assert_eq!(on_grass(&block, 3, 0, 5, 16), None);
        assert_eq!(on_grass(&block, 4, 0, 5, 16), None);
        assert_eq!(on_grass(&block, 5, 0, 5, 16), Some([5, 3, 0]));
    }

    #[test]
    fn nothing_out_of_sight_is_stood_on() {
        let unseen = |cell: Cell| (cell[0] == 0).then(|| world(&[])(cell)).flatten();
        assert_eq!(next_step(&unseen, [0, 1, 0], [10, 1, 0], &[]), Some([0, 1, 1]));
    }
}
