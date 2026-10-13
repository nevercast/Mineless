//! Oak trees, placed from the seed alone: one candidate per cell of a fixed grid, kept or not by
//! the biome it stands in. A chunk works out every tree that could reach into it and writes only
//! its own cells, so a tree across a chunk border comes out whole whichever chunk is first.

use glam::{IVec2, IVec3, Vec3Swizzles};

use super::{Biome, Block, Chunk, Extent, Neighbourhood, SEA_LEVEL, SIZE, Surface, hash};
use crate::world::Axis;

/// Blocks along each side of a grid cell holding one tree candidate.
const CELL: i32 = 4;
/// How far leaves reach from the trunk.
const CANOPY_RADIUS: i32 = 2;
/// Trees keep this far from the world border.
const BORDER_CLEARANCE: i32 = 3;
const TRUNK_HEIGHTS: std::ops::RangeInclusive<i32> = 4..=6;
const TREE_SALT: u32 = 0x632b_e5ab;

/// Chance that a candidate in each biome grows, out of 256: forests dense enough for canopies to
/// touch, plains with a lone tree every few dozen blocks.
fn chance(biome: Biome) -> u32 {
    match biome {
        Biome::Forest => 150,
        Biome::Hills => 20,
        Biome::Plains => 8,
        Biome::Beach | Biome::Ocean => 0,
    }
}

/// How far around a chunk the columns it needs reach: a trunk two blocks outside it can still
/// put leaves in it, and the trunk's surface is judged against its neighbours.
pub(super) const REACH: i32 = CANOPY_RADIUS + 1;

pub(super) struct Tree {
    /// The lowest trunk log.
    base: IVec3,
    height: i32,
    /// One bit per outer corner of the two wide leaf layers: set means that corner is dropped.
    dropped_corners: u32,
}

/// Where grid cell `cell`'s candidate stands, and the hash that makes its random choices.
fn candidate(seed: u32, cell: IVec2) -> (IVec2, u32) {
    let roll = hash(seed ^ TREE_SALT, cell.x, 0, cell.y);
    let offset = IVec2::new((roll & 3) as i32, (roll >> 2 & 3) as i32);
    (cell * CELL + offset, roll)
}

impl Tree {
    /// The tree standing at `column`, if the candidate there grows. `columns` must reach
    /// `REACH - CANOPY_RADIUS` blocks round it.
    fn grown(
        seed: u32,
        column: IVec2,
        roll: u32,
        columns: &Neighbourhood,
        extent: Extent,
    ) -> Option<Tree> {
        let (x, z) = (column.x, column.y);
        let clear_of_border =
            (-extent.half_width + BORDER_CLEARANCE)..(extent.half_width - BORDER_CLEARANCE);
        if !clear_of_border.contains(&x) || !clear_of_border.contains(&z) {
            return None;
        }
        let surface = Surface::at(seed, x, z, columns);
        if roll >> 8 & 0xff >= chance(surface.biome) {
            return None;
        }
        let column = columns.column(x, z);
        let ground = surface.top - 1;
        if ground < SEA_LEVEL || surface.block(column, ground) != Block::Grass {
            return None;
        }
        let (shortest, tallest) = (*TRUNK_HEIGHTS.start(), *TRUNK_HEIGHTS.end());
        let height = shortest + (roll >> 16 & 0xff) as i32 % (tallest - shortest + 1);
        let base = IVec3::new(x, surface.top, z);
        // The trunk and the cell above it must be clear; leaves only fill what air they find.
        if (0..=height).any(|y| column.solid(base.y + y)) {
            return None;
        }
        Some(Tree {
            base,
            height,
            dropped_corners: roll >> 24,
        })
    }

    /// Every leaf cell of the canopy, including ones where something else may already be.
    fn leaves(&self) -> impl Iterator<Item = IVec3> + '_ {
        let top = self.base.y + self.height - 1;
        // Two wide layers round the upper trunk, then a 3x3 at the trunk top and a plus on it.
        (0..4).flat_map(move |layer| {
            let y = top - 2 + layer;
            let radius = if layer < 2 { 2 } else { 1 };
            (-radius..=radius)
                .flat_map(move |dx| (-radius..=radius).map(move |dz| IVec2::new(dx, dz)))
                .filter(move |offset| {
                    let corner = offset.x.abs() == radius && offset.y.abs() == radius;
                    let trunk = *offset == IVec2::ZERO && y <= top;
                    let dropped = layer < 2 && corner && {
                        let bit = layer * 4 + i32::from(offset.x > 0) * 2 + i32::from(offset.y > 0);
                        self.dropped_corners >> bit & 1 == 1
                    };
                    !trunk && !dropped && !(layer == 3 && corner)
                })
                .map(move |offset| self.base.with_y(y) + IVec3::new(offset.x, 0, offset.y))
        })
    }

    fn trunk(&self) -> impl Iterator<Item = IVec3> + '_ {
        (0..self.height).map(|y| self.base + IVec3::Y * y)
    }
}

/// Grows every tree that reaches into the chunk at `origin`, writing only its cells. Leaves go
/// in first and only into air, then logs over anything, so where trees overlap the result is
/// the same in any order.
pub(super) fn grow(
    seed: u32,
    chunk: &mut Chunk,
    origin: IVec3,
    columns: &Neighbourhood,
    extent: Extent,
) {
    let first = (origin.xz() - CANOPY_RADIUS).div_euclid(IVec2::splat(CELL));
    let last = (origin.xz() + SIZE - 1 + CANOPY_RADIUS).div_euclid(IVec2::splat(CELL));
    let reaches_chunk = |column: IVec2| {
        let local = column - origin.xz();
        local.cmpge(IVec2::splat(-CANOPY_RADIUS)).all()
            && local.cmplt(IVec2::splat(SIZE + CANOPY_RADIUS)).all()
    };
    let trees: Vec<Tree> = (first.x..=last.x)
        .flat_map(|x| (first.y..=last.y).map(move |z| IVec2::new(x, z)))
        .map(|cell| candidate(seed, cell))
        .filter(|&(column, _)| reaches_chunk(column))
        .filter_map(|(column, roll)| Tree::grown(seed, column, roll, columns, extent))
        .collect();
    let local = |position: IVec3| {
        let local = position - origin;
        (local.cmpge(IVec3::ZERO).all() && local.cmplt(IVec3::splat(SIZE)).all())
            .then(|| local.as_usizevec3())
    };
    for leaf in trees.iter().flat_map(Tree::leaves) {
        if let Some(at) = local(leaf)
            && chunk.get(at.x, at.y, at.z) == Block::Air
        {
            chunk.set(at.x, at.y, at.z, Block::OakLeaves);
        }
    }
    for tree in &trees {
        // No grass grows under a trunk.
        if let Some(at) = local(tree.base - IVec3::Y) {
            chunk.set(at.x, at.y, at.z, Block::Dirt);
        }
        for log in tree.trunk() {
            if let Some(at) = local(log) {
                chunk.set(at.x, at.y, at.z, Block::OakLog(Axis::Y));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{HEIGHT, generate};
    use super::*;
    use crate::world::World;

    const EXTENT: Extent = Extent {
        half_width: 64,
        height: HEIGHT,
    };

    /// The origins of the bottom chunk of every chunk column in `EXTENT`.
    fn column_origins() -> impl Iterator<Item = IVec3> {
        EXTENT
            .chunk_positions()
            .filter(|chunk| chunk.y == 0)
            .map(|chunk| chunk * SIZE)
    }

    /// The tree of a grid cell inside the chunk `columns` was built for.
    fn in_cell(seed: u32, cell: IVec2, columns: &Neighbourhood, extent: Extent) -> Option<Tree> {
        let (column, roll) = candidate(seed, cell);
        Tree::grown(seed, column, roll, columns, extent)
    }

    fn trees(seed: u32) -> Vec<Tree> {
        column_origins()
            .flat_map(|origin| {
                let columns = Neighbourhood::around(seed, origin);
                let cells = origin.xz() / CELL;
                (0..SIZE / CELL)
                    .flat_map(|i| (0..SIZE / CELL).map(move |k| IVec2::new(i, k)))
                    .filter_map(|cell| in_cell(seed, cells + cell, &columns, EXTENT))
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    #[test]
    fn trees_across_chunk_borders_are_whole() {
        let world = generate(EXTENT, 1);
        let straddling: Vec<Tree> = trees(1)
            .into_iter()
            .filter(|tree| {
                let local = tree.base.xz().rem_euclid(IVec2::splat(SIZE));
                local.min_element() < CANOPY_RADIUS || local.max_element() >= SIZE - CANOPY_RADIUS
            })
            .collect();
        assert!(!straddling.is_empty());
        for tree in &straddling {
            for log in tree.trunk() {
                assert_eq!(world.block(log), Block::OakLog(Axis::Y), "{log}");
            }
            for leaf in tree.leaves() {
                let block = world.block(leaf);
                assert!(block != Block::Air, "missing leaf at {leaf}");
            }
        }
    }

    fn every_block(world: &World) -> impl Iterator<Item = (IVec3, Block)> + '_ {
        let (min, max) = (EXTENT.min(), EXTENT.max());
        (min.x..max.x).flat_map(move |x| {
            (min.y..max.y).flat_map(move |y| {
                (min.z..max.z).map(move |z| {
                    let position = IVec3::new(x, y, z);
                    (position, world.block(position))
                })
            })
        })
    }

    #[test]
    fn logs_are_upright_trunks_on_dirt_under_leaves() {
        for seed in 0..2 {
            let world = generate(EXTENT, seed);
            for (position, block) in every_block(&world) {
                match block {
                    Block::OakLog(axis) => {
                        assert_eq!(axis, Axis::Y);
                        assert!(position.y >= SEA_LEVEL, "log at {position}");
                        let below = world.block(position - IVec3::Y);
                        assert!(
                            matches!(below, Block::Dirt | Block::OakLog(_)),
                            "log at {position} on {below:?}"
                        );
                        if !matches!(world.block(position + IVec3::Y), Block::OakLog(_)) {
                            let leafy = (1..=2).any(|up| {
                                world.block(position + IVec3::Y * up) == Block::OakLeaves
                            });
                            assert!(leafy, "bare trunk top at {position}");
                        }
                    }
                    Block::OakLeaves => assert!(position.y >= SEA_LEVEL, "leaves at {position}"),
                    _ => {}
                }
            }
        }
    }

    #[test]
    fn forests_are_denser_than_plains() {
        let mut forest = (0, 0);
        let mut plains = (0, 0);
        for seed in 0..4 {
            for origin in column_origins() {
                let columns = Neighbourhood::around(seed, origin);
                for x in origin.x..origin.x + SIZE {
                    for z in origin.z..origin.z + SIZE {
                        let tally = match Surface::at(seed, x, z, &columns).biome {
                            Biome::Forest => &mut forest,
                            Biome::Plains => &mut plains,
                            _ => continue,
                        };
                        tally.1 += 1;
                    }
                }
                let cells = origin.xz() / CELL;
                for i in 0..SIZE / CELL {
                    for k in 0..SIZE / CELL {
                        let cell = cells + IVec2::new(i, k);
                        if let Some(tree) = in_cell(seed, cell, &columns, EXTENT) {
                            let base = tree.base;
                            match Surface::at(seed, base.x, base.z, &columns).biome {
                                Biome::Forest => forest.0 += 1,
                                Biome::Plains => plains.0 += 1,
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
        let density = |(trees, columns): (i32, i32)| trees as f32 / columns.max(1) as f32;
        assert!(forest.1 > 0 && plains.1 > 0);
        assert!(
            density(forest) > 4.0 * density(plains),
            "forest {forest:?}, plains {plains:?}"
        );
    }
}
