//! Where the house goes and what it's made of.

use std::collections::HashSet;
use std::f32::consts::FRAC_PI_2;

use agent_api::Block;

use crate::Cell;

/// The house's walls stand on a square this many blocks across, leaving three by three inside.
pub const HOUSE: i32 = 5;
/// Open ground left all round the house, to work from.
pub const MARGIN: i32 = 2;
/// Blocks across the whole site, house and margin.
pub const SITE: i32 = HOUSE + 2 * MARGIN;
/// How high above the ground the site is cleared: the roof's ridge is five up, and a builder standing on a pillar beside it
/// needs room for its head.
pub const CLEARANCE: i32 = 6;
/// How far above and below a height it looks for the ground in a column.
const GROUND_REACH: i32 = 16;

/// Body yaws that face east and west, which the roof's stairs are placed facing.
const EAST: f32 = -FRAC_PI_2;
const WEST: f32 = FRAC_PI_2;

/// A square of ground to build on, `SITE` blocks across from its lowest corner.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Site {
    pub corner: [i32; 2],
    /// The height of the top of the ground once it's levelled.
    pub floor: i32,
}

impl Site {
    pub fn contains(&self, [x, _, z]: Cell) -> bool {
        let [cx, cz] = self.corner;
        (cx..cx + SITE).contains(&x) && (cz..cz + SITE).contains(&z)
    }

    /// Whether a column is under the house itself rather than the margin round it.
    pub fn in_house(&self, [x, _, z]: Cell) -> bool {
        let [cx, cz] = self.corner.map(|c| c + MARGIN);
        (cx..cx + HOUSE).contains(&x) && (cz..cz + HOUSE).contains(&z)
    }

    /// Every cell above the ground that has to be emptied, top down.
    pub fn clearing(&self) -> impl Iterator<Item = Cell> {
        let [cx, cz] = self.corner;
        let floor = self.floor;
        (floor + 1..=floor + CLEARANCE)
            .rev()
            .flat_map(move |y| (cx..cx + SITE).flat_map(move |x| (cz..cz + SITE).map(move |z| [x, y, z])))
    }

    /// The crafting table, in a corner of the margin out of the way.
    pub fn table(&self) -> Piece {
        Piece {
            cell: [self.corner[0], self.floor + 1, self.corner[1]],
            block: Block::CraftingTable,
            yaw: None,
        }
    }

    /// The cells on the levelled ground of the margin, where a builder can stand.
    pub fn margin(&self) -> impl Iterator<Item = Cell> {
        let [cx, cz] = self.corner;
        let floor = self.floor;
        (cx..cx + SITE)
            .flat_map(move |x| (cz..cz + SITE).map(move |z| [x, floor + 1, z]))
            .filter(move |&cell| !self.in_house(cell) && cell != self.table().cell)
    }

    /// What to build, in an order where each block has one already built, or the ground, to be placed against. The door is
    /// at the front, on the low z side, and the roof's ridge runs from front to back.
    pub fn blueprint(&self) -> Vec<Piece> {
        let [x0, z0] = self.corner.map(|c| c + MARGIN);
        let (x1, z1) = (x0 + HOUSE - 1, z0 + HOUSE - 1);
        let floor = self.floor;
        let door = |[x, y, z]: Cell| x == x0 + 2 && z == z0 && (floor + 1..=floor + 2).contains(&y);
        let window = |[x, y, z]: Cell| x == x0 + 2 && z == z1 && y == floor + 2;
        // Round the walls one layer at a time, each block beside the last.
        let round = (x0..x1)
            .map(|x| (x, z0))
            .chain((z0..z1).map(|z| (x1, z)))
            .chain((x0 + 1..=x1).rev().map(|x| (x, z1)))
            .chain((z0 + 1..=z1).rev().map(|z| (x0, z)));
        let round: Vec<(i32, i32)> = round.collect();
        let mut pieces: Vec<Piece> = (floor + 1..=floor + 3)
            .flat_map(|y| round.iter().map(move |&(x, z)| [x, y, z]))
            .filter(|&cell| !door(cell) && !window(cell))
            .map(Piece::planks)
            .collect();
        let (eaves, ridge) = (floor + 4, floor + 5);
        for z in [z0, z1] {
            pieces.extend((x0 + 1..x1).map(|x| Piece::planks([x, eaves, z])));
        }
        for z in z0..=z1 {
            pieces.push(Piece::stairs([x0, eaves, z], EAST));
            pieces.push(Piece::stairs([x1, eaves, z], WEST));
        }
        // From both ends of the ridge inwards, so each block has one to go against.
        for z in [z0, z1, z0 + 1, z1 - 1, z0 + 2] {
            pieces.push(Piece::planks([x0 + 2, ridge, z]));
        }
        for z in z0..=z1 {
            pieces.push(Piece::stairs([x0 + 1, ridge, z], EAST));
            pieces.push(Piece::stairs([x1 - 1, ridge, z], WEST));
        }
        pieces
    }
}

/// One block of the house.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Piece {
    pub cell: Cell,
    pub block: Block,
    /// The way the body must face to place it, for stairs, which face the way their builder does.
    pub yaw: Option<f32>,
}

impl Piece {
    fn planks(cell: Cell) -> Piece {
        Piece {
            cell,
            block: Block::OakPlanks,
            yaw: None,
        }
    }

    fn stairs(cell: Cell, yaw: f32) -> Piece {
        Piece {
            cell,
            block: Block::OakStairs,
            yaw: Some(yaw),
        }
    }
}

/// The height of the top of the ground in a column, looking `GROUND_REACH` above and below `near`: the first block from the top
/// that isn't air or a tree. `None` if that's water or anything a site shouldn't be built on, or it's out of sight.
pub fn ground(world: &impl Fn(Cell) -> Option<Block>, x: i32, z: i32, near: i32) -> Option<i32> {
    for y in (near - GROUND_REACH..=near + GROUND_REACH).rev() {
        match world([x, y, z])? {
            Block::Air | Block::OakLog | Block::OakLeaves => {}
            Block::Grass | Block::Dirt | Block::Stone | Block::Cobblestone | Block::Sand | Block::Gravel | Block::Snow => {
                return Some(y);
            }
            _ => return None,
        }
    }
    None
}

/// The site nearest `centre`, with its middle at most `radius` columns away along each axis, whose ground is level to within a
/// block: levelling it only cuts the higher blocks away, and nothing needs filling in. Fewer blocks to cut wins between sites
/// at the same distance. Some of its ground must be `reachable` to stand on, so the builder can get there.
pub fn find_site(world: &impl Fn(Cell) -> Option<Block>, centre: Cell, radius: i32, reachable: &HashSet<Cell>) -> Option<Site> {
    let [cx, cy, cz] = centre;
    let half = SITE / 2;
    let (first_x, first_z, span) = (cx - radius - half, cz - radius - half, 2 * (radius + half) + 1);
    let heights: Vec<Option<i32>> = (0..span * span)
        .map(|index| ground(world, first_x + index / span, first_z + index % span, cy - 1))
        .collect();
    let mut best: Option<(i32, Site)> = None;
    for mx in cx - radius..=cx + radius {
        for mz in cz - radius..=cz + radius {
            let near = (mx - cx).pow(2) + (mz - cz).pow(2);
            if best.is_some_and(|(known, _)| near >= known) {
                continue;
            }
            let corner = [mx - half, mz - half];
            let (mut low, mut high, mut sum) = (i32::MAX, i32::MIN, 0);
            let level = (0..SITE).all(|dx| {
                (0..SITE).all(|dz| {
                    let Some(height) = heights[((corner[0] + dx - first_x) * span + corner[1] + dz - first_z) as usize] else {
                        return false;
                    };
                    (low, high, sum) = (low.min(height), high.max(height), sum + height);
                    high - low <= 1
                })
            });
            let mut columns = (0..SITE).flat_map(|dx| (0..SITE).map(move |dz| (corner[0] + dx, corner[1] + dz)));
            if !level
                || !columns.any(|(x, z)| {
                    heights[((x - first_x) * span + z - first_z) as usize].is_some_and(|height| reachable.contains(&[x, height + 1, z]))
                })
            {
                continue;
            }
            let cuts = sum - low * SITE * SITE;
            let score = near + cuts;
            if best.is_none_or(|(known, _)| score < known) {
                best = Some((score, Site { corner, floor: low }));
            }
        }
    }
    best.map(|(_, site)| site)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::path::reachable;
    use crate::tests::world;
    use crate::{SIDES, offset};

    #[test]
    fn the_house_takes_what_was_planned_for_it() {
        let site = Site { corner: [0, 0], floor: 0 };
        let pieces = site.blueprint();
        let count = |block| pieces.iter().filter(|piece| piece.block == block).count();
        assert_eq!((count(Block::OakPlanks), count(Block::OakStairs)), (56, 20));
        assert!(pieces.iter().all(|piece| site.in_house(piece.cell)));
        // The doorway and the window stay open.
        for cell in [[4, 1, 2], [4, 2, 2], [4, 2, 6]] {
            assert!(pieces.iter().all(|piece| piece.cell != cell), "{cell:?}");
        }
        // Each piece is built against the ground or one before it.
        for (index, piece) in pieces.iter().enumerate() {
            let held = piece.cell[1] == 1
                || SIDES
                    .iter()
                    .any(|&side| pieces[..index].iter().any(|before| before.cell == offset(piece.cell, side)));
            assert!(held, "{piece:?}");
        }
    }

    #[test]
    fn the_margin_is_what_is_left_of_the_site_round_the_house_and_the_table() {
        let site = Site {
            corner: [-4, 7],
            floor: 30,
        };
        let margin: Vec<Cell> = site.margin().collect();
        assert_eq!(margin.len(), (SITE * SITE - HOUSE * HOUSE - 1) as usize);
        assert!(
            margin
                .iter()
                .all(|&cell| site.contains(cell) && cell[1] == 31 && !site.in_house(cell))
        );
        assert!(site.contains(site.table().cell) && !margin.contains(&site.table().cell));
        let clearing: Vec<Cell> = site.clearing().collect();
        assert_eq!(clearing.len(), (SITE * SITE * CLEARANCE) as usize);
        assert_eq!(clearing.first().map(|cell| cell[1]), Some(30 + CLEARANCE));
        assert!(!site.contains([5, 0, 7]) && !site.contains([-4, 0, 6]));
    }

    #[test]
    fn a_site_is_level_and_as_near_as_it_can_be() {
        let mut changes: Vec<(Cell, Block)> = Vec::new();
        // Two steps up to the east and a pond to the west leave no level ground round the start.
        for x in -30..=30 {
            for z in -30..=30 {
                if x >= 2 {
                    changes.push(([x, 1, z], Block::Grass));
                }
                if x >= 6 {
                    changes.push(([x, 2, z], Block::Grass));
                }
            }
        }
        for z in -30..=30 {
            changes.push(([-3, 0, z], Block::Water));
        }
        let world = world(&changes);
        let everywhere: HashSet<Cell> = (-30..=30)
            .flat_map(|x| (-30..=30).flat_map(move |z| (1..=3).map(move |y| [x, y, z])))
            .collect();
        let site = find_site(&world, [0, 1, 0], 10, &everywhere).expect("there's level ground");
        // Past the pond is nearer than on the higher step, and needs no cutting.
        assert_eq!(
            site,
            Site {
                corner: [-12, -4],
                floor: 0
            }
        );
        // But a body can't walk across the pond, so it builds on the step, cutting it back where it rises again.
        let site = find_site(&world, [0, 1, 0], 10, &reachable(&world, [0, 1, 0], 30)).expect("there's level ground");
        assert_eq!(site, Site { corner: [2, -4], floor: 1 });
        assert_eq!(ground(&world, -3, 0, 0), None);
        assert_eq!(ground(&world, 8, 0, 0), Some(2));
    }
}
