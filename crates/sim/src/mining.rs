//! How long blocks take to break by hand, and what they drop.

use glam::IVec3;

use crate::world::terrain::noise::hash;
use crate::world::{Axis, Block};

/// Ticks of holding attack on a block before it breaks, or `None` if it can't be broken. These are Minecraft's times by hand,
/// one and a half seconds per point of hardness, as if every block could be gathered without a tool.
pub fn break_ticks(block: Block) -> Option<u32> {
    match block {
        Block::Snow | Block::OakLeaves => Some(6),
        Block::Dirt | Block::Sand => Some(15),
        Block::Grass | Block::Gravel => Some(18),
        Block::Stone => Some(45),
        Block::Cobblestone | Block::OakLog(_) => Some(60),
        Block::Air | Block::Water | Block::WorldBorder => None,
    }
}

/// What breaking a block can leave behind, each with its weight: `None` drops nothing.
type DropTable = &'static [(u32, Option<Block>)];

fn drop_table(block: Block) -> DropTable {
    match block {
        Block::Grass | Block::Dirt => &[(1, Some(Block::Dirt))],
        Block::Stone | Block::Cobblestone => &[(1, Some(Block::Cobblestone))],
        // Logs stack together whichever way their grain ran, and placing one sets its grain afresh.
        Block::OakLog(_) => &[(1, Some(Block::OakLog(Axis::Y)))],
        Block::OakLeaves => &[(19, None), (1, Some(Block::OakLeaves))],
        Block::Sand => &[(1, Some(Block::Sand))],
        Block::Gravel => &[(1, Some(Block::Gravel))],
        Block::Snow => &[(1, Some(Block::Snow))],
        Block::Air | Block::Water | Block::WorldBorder => &[],
    }
}

/// What breaking `block` at `position` drops. `roll` picks from its drop table, so the same position and roll always drop
/// the same.
pub fn drop(block: Block, position: IVec3, roll: u32) -> Option<Block> {
    let table = drop_table(block);
    let total: u32 = table.iter().map(|&(weight, _)| weight).sum();
    if total == 0 {
        return None;
    }
    let mut pick = hash(roll, position.x, position.y, position.z) % total;
    for &(weight, drop) in table {
        if pick < weight {
            return drop;
        }
        pick -= weight;
    }
    unreachable!("the pick is below the total weight")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_drop_what_their_table_says() {
        let position = IVec3::new(3, 40, -7);
        assert_eq!(drop(Block::Grass, position, 0), Some(Block::Dirt));
        assert_eq!(drop(Block::Stone, position, 0), Some(Block::Cobblestone));
        assert_eq!(drop(Block::OakLog(Axis::X), position, 0), Some(Block::OakLog(Axis::Y)));
        assert_eq!(drop(Block::WorldBorder, position, 0), None);
    }

    #[test]
    fn weights_set_how_often_each_drop_comes_up() {
        let leaves = (0..20_000)
            .filter(|&roll| drop(Block::OakLeaves, IVec3::new(roll as i32, 64, 0), roll).is_some())
            .count();
        // One in twenty, give or take.
        assert!((800..1200).contains(&leaves), "{leaves}");
        assert_eq!(drop(Block::OakLeaves, IVec3::ONE, 5), drop(Block::OakLeaves, IVec3::ONE, 5));
    }

    #[test]
    fn everything_breakable_has_a_drop_table() {
        let blocks = [
            Block::Grass,
            Block::Dirt,
            Block::Stone,
            Block::Cobblestone,
            Block::Sand,
            Block::Gravel,
            Block::Snow,
            Block::OakLog(Axis::Y),
            Block::OakLeaves,
        ];
        for block in blocks {
            assert!(break_ticks(block).is_some() && !drop_table(block).is_empty(), "{block:?}");
        }
    }
}
