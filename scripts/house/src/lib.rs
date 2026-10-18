//! Sample script: builds a small wooden house. It finds a patch of level ground, clears it, cuts down trees for logs, crafts a
//! crafting table, planks and stairs, and builds: plank walls round a three by three floor with a doorway at the front and a
//! window at the back, under a gable roof of stairs. It reaches the high parts by building a dirt pillar under itself, and takes
//! the pillar down again before moving on.

mod path;
mod plan;
#[cfg(target_arch = "wasm32")]
mod play;
mod sight;

use agent_api::Block;

/// A block position: `[x, y, z]`.
type Cell = [i32; 3];

/// Bodies pass through it. Blocks out of sight are neither open nor solid, so nothing is planned through them.
fn open(block: Option<Block>) -> bool {
    matches!(block, Some(Block::Air | Block::Water))
}

fn solid(block: Option<Block>) -> bool {
    block.is_some_and(|block| !open(Some(block)))
}

fn offset([x, y, z]: Cell, [dx, dy, dz]: Cell) -> Cell {
    [x + dx, y + dy, z + dz]
}

/// The six directions to a cell's neighbours.
const SIDES: [Cell; 6] = [[1, 0, 0], [-1, 0, 0], [0, 1, 0], [0, -1, 0], [0, 0, 1], [0, 0, -1]];

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    /// Flat grass with its top at y = 0, so bodies stand at y = 1, plus whatever `changes` puts in.
    pub fn world(changes: &[(Cell, Block)]) -> impl Fn(Cell) -> Option<Block> + use<> {
        let changes: HashMap<Cell, Block> = changes.iter().copied().collect();
        move |cell| {
            Some(changes.get(&cell).copied().unwrap_or(match cell[1] {
                ..0 => Block::Dirt,
                0 => Block::Grass,
                _ => Block::Air,
            }))
        }
    }
}
