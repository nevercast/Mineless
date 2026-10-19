//! Finding a way across the ground: from one block a body stands in to the next, a block at a time, climbing one block with a
//! jump or dropping up to three.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};

use agent_api::Block;

use crate::{Cell, open, solid};

const CLIMB: i32 = 1;
const DROP: i32 = 3;
/// What climbing a block costs on top of the step, so a flat way round a small hill wins.
const CLIMB_COST: u32 = 2;
/// The four neighbouring columns. Diagonal steps would cut corners the body can't fit past.
const NEIGHBOURS: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];

/// Whether a body can stand in `cell`: solid underneath, and room for the body. Water doesn't count as room, as a body sinks in
/// it and can't always climb out.
pub fn standable(world: &impl Fn(Cell) -> Option<Block>, [x, y, z]: Cell) -> bool {
    solid(world([x, y - 1, z])) && world([x, y, z]) == Some(Block::Air) && world([x, y + 1, z]) == Some(Block::Air)
}

/// Where a body standing in `here` can step to in the column `dx, dz` along: the highest place it can stand there, from one
/// above down to three below, if the way there is clear.
fn step(world: &impl Fn(Cell) -> Option<Block>, here: Cell, (dx, dz): (i32, i32)) -> Option<Cell> {
    let [x, y, z] = here;
    let top = if open(world([x, y + 2, z])) { y + CLIMB } else { y };
    let landing = (y - DROP..=top)
        .rev()
        .map(|y| [x + dx, y, z + dz])
        .find(|&cell| standable(world, cell))?;
    // Dropping, the body passes down through the column from head height.
    let passes = (landing[1]..=y + 1).all(|y| open(world([x + dx, y, z + dz])));
    passes.then_some(landing)
}

/// The shortest way from `from` to the nearest place `arrived` accepts, steering towards `toward`, as the cells to stand in
/// one after another, starting with `from`. Gives up after looking at `limit` places.
pub fn path(
    world: &impl Fn(Cell) -> Option<Block>,
    from: Cell,
    toward: Cell,
    arrived: impl Fn(Cell) -> bool,
    limit: usize,
) -> Option<Vec<Cell>> {
    let guess = |[x, y, z]: Cell| (x - toward[0]).unsigned_abs() + (y - toward[1]).unsigned_abs() + (z - toward[2]).unsigned_abs();
    let mut came_from: HashMap<Cell, (Cell, u32)> = HashMap::from([(from, (from, 0))]);
    let mut frontier = BinaryHeap::from([Reverse((guess(from), from))]);
    let mut looked = 0;
    while let Some(Reverse((_, here))) = frontier.pop() {
        if arrived(here) {
            let mut way = vec![here];
            while let Some(&(previous, _)) = came_from.get(way.last().expect("the way is never empty")) {
                if previous == *way.last().expect("the way is never empty") {
                    break;
                }
                way.push(previous);
            }
            way.reverse();
            return Some(way);
        }
        looked += 1;
        if looked > limit {
            return None;
        }
        let cost = came_from[&here].1;
        for neighbour in NEIGHBOURS {
            let Some(next) = step(world, here, neighbour) else {
                continue;
            };
            let next_cost = cost + 1 + if next[1] > here[1] { CLIMB_COST } else { 0 };
            if came_from.get(&next).is_none_or(|&(_, known)| next_cost < known) {
                came_from.insert(next, (here, next_cost));
                frontier.push(Reverse((next_cost + guess(next), next)));
            }
        }
    }
    None
}

/// Every place a body standing in `from` can walk to without going more than `radius` columns from it along either axis.
pub fn reachable(world: &impl Fn(Cell) -> Option<Block>, from: Cell, radius: i32) -> HashSet<Cell> {
    let mut found = HashSet::from([from]);
    let mut frontier = vec![from];
    while let Some(here) = frontier.pop() {
        for neighbour in NEIGHBOURS {
            let Some(next) = step(world, here, neighbour) else {
                continue;
            };
            let near = (next[0] - from[0]).abs() <= radius && (next[2] - from[2]).abs() <= radius;
            if near && found.insert(next) {
                frontier.push(next);
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::world;

    fn wall(x: i32, zs: std::ops::RangeInclusive<i32>, height: i32) -> Vec<(Cell, Block)> {
        zs.flat_map(|z| (1..=height).map(move |y| ([x, y, z], Block::Stone))).collect()
    }

    #[test]
    fn it_goes_round_a_wall_too_high_to_climb() {
        let walled = world(&wall(2, -3..=3, 2));
        let way = path(&walled, [0, 1, 0], [5, 1, 0], |cell| cell == [5, 1, 0], 10_000).expect("there's a way round");
        assert_eq!(way.first(), Some(&[0, 1, 0]));
        assert_eq!(way.last(), Some(&[5, 1, 0]));
        assert!(way.iter().any(|cell| cell[2].abs() > 3), "{way:?}");
        assert!(
            way.windows(2)
                .all(|pair| (0..3).map(|a| (pair[0][a] - pair[1][a]).abs()).sum::<i32>() == 1)
        );
    }

    #[test]
    fn it_climbs_a_step_and_drops_off_a_ledge_but_not_a_cliff() {
        let stepped = world(&wall(2, -30..=30, 1));
        let way = path(&stepped, [0, 1, 0], [4, 1, 0], |cell| cell == [4, 1, 0], 10_000).expect("one block can be climbed");
        assert!(way.contains(&[2, 2, 0]), "{way:?}");
        let hole = |depth: i32| -> Vec<(Cell, Block)> {
            (1 - depth..=0)
                .flat_map(|y| (1..=3).flat_map(move |x| (-1..=1).map(move |z| ([x, y, z], Block::Air))))
                .collect()
        };
        assert_eq!(step(&world(&hole(3)), [0, 1, 0], (1, 0)), Some([1, -2, 0]));
        assert_eq!(step(&world(&hole(4)), [0, 1, 0], (1, 0)), None);
    }

    #[test]
    fn what_can_be_reached_stops_at_walls_and_the_radius() {
        let mut changes = wall(2, -3..=3, 2);
        changes.extend(wall(-2, -3..=3, 2));
        changes.extend((-2..=2).flat_map(|x| [([x, 1, 3], Block::Stone), ([x, 2, 3], Block::Stone)]));
        changes.extend((-2..=2).flat_map(|x| [([x, 1, -3], Block::Stone), ([x, 2, -3], Block::Stone)]));
        let walled_in = reachable(&world(&changes), [0, 1, 0], 10);
        assert_eq!(walled_in.len(), 3 * 5);
        assert_eq!(reachable(&world(&[]), [0, 1, 0], 2).len(), 5 * 5);
    }

    #[test]
    fn it_stops_at_the_first_place_that_will_do() {
        let flat = world(&[]);
        let way = path(&flat, [0, 1, 0], [10, 1, 0], |cell| cell[0] >= 3, 10_000).expect("open ground");
        assert_eq!(way.last(), Some(&[3, 1, 0]));
        assert_eq!(path(&flat, [0, 1, 0], [10, 1, 0], |cell| cell[0] >= 3, 2), None, "the limit");
    }
}
