//! Turning blocks in an inventory into other blocks. There is no crafting grid: a recipe is just what it takes and what it
//! makes, and `want` works out the crafts that lead to a goal.

use crate::inventory::Inventory;
use crate::world::{Axis, Block};

pub struct Recipe {
    pub inputs: &'static [(Block, u32)],
    pub output: Block,
    /// How many of the output one craft makes.
    pub count: u8,
}

pub const RECIPES: &[Recipe] = &[Recipe {
    inputs: &[(Block::OakLog(Axis::Y), 1)],
    output: Block::OakPlanks,
    count: 4,
}];

/// The recipe that makes `block`, if any. Each block is made by at most one.
pub fn recipe_for(block: Block) -> Option<&'static Recipe> {
    RECIPES.iter().find(|recipe| recipe.output == block)
}

/// How many times `recipe` can be crafted from what's in `inventory`.
pub fn times(inventory: &Inventory, recipe: &Recipe) -> u32 {
    recipe
        .inputs
        .iter()
        .map(|&(block, count)| inventory.count(block) / count)
        .min()
        .unwrap_or(0)
}

/// Crafts `recipe` up to `times` times, as many as the inventory has inputs and room for. Returns how many crafts were made.
pub fn craft(inventory: &mut Inventory, recipe: &Recipe, times: u32) -> u32 {
    for made in 0..times {
        let mut after = inventory.clone();
        let taken = recipe.inputs.iter().all(|&(block, count)| after.remove(block, count));
        if !taken || after.add(recipe.output, recipe.count) > 0 {
            return made;
        }
        *inventory = after;
    }
    times
}

/// What stands between an inventory and holding some number of a block.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Want {
    /// It holds enough.
    Done,
    /// It needs this many more of a block no recipe makes, or that it has no inputs for in turn.
    Missing(Block, u32),
    /// A craft's output didn't fit.
    NoRoom,
}

/// Works towards `inventory` holding `amount` of `block`: crafts whatever it can that leads there, inputs first, and says what
/// it still needs. Calling it again after gathering what was missing gets closer each time, and in the end holds the amount.
pub fn want(inventory: &mut Inventory, block: Block, amount: u32) -> Want {
    let short = amount.saturating_sub(inventory.count(block));
    if short == 0 {
        return Want::Done;
    }
    let Some(recipe) = recipe_for(block) else {
        return Want::Missing(block, short);
    };
    let crafts = short.div_ceil(u32::from(recipe.count));
    for &(input, count) in recipe.inputs {
        let input_want = want(inventory, input, crafts * count);
        if input_want != Want::Done {
            return input_want;
        }
    }
    if craft(inventory, recipe, crafts) < crafts {
        return Want::NoRoom;
    }
    Want::Done
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::{SLOTS, STACK_SIZE};

    const LOG: Block = Block::OakLog(Axis::Y);

    #[test]
    fn a_log_makes_four_planks() {
        let mut inventory = Inventory::default();
        inventory.add(LOG, 3);
        let planks = recipe_for(Block::OakPlanks).expect("planks have a recipe");
        assert_eq!(times(&inventory, planks), 3);
        assert_eq!(craft(&mut inventory, planks, 2), 2);
        assert_eq!(inventory.count(LOG), 1);
        assert_eq!(inventory.count(Block::OakPlanks), 8);
        assert_eq!(craft(&mut inventory, planks, 5), 1);
        assert_eq!(times(&inventory, planks), 0);
    }

    #[test]
    fn wanting_names_what_is_missing_until_it_has_been_gathered() {
        let mut inventory = Inventory::default();
        inventory.add(Block::OakPlanks, 2);
        assert_eq!(want(&mut inventory, Block::OakPlanks, 2), Want::Done);
        // Ten planks need two more logs' worth, and wanting crafts nothing until both are there.
        assert_eq!(want(&mut inventory, Block::OakPlanks, 10), Want::Missing(LOG, 2));
        inventory.add(LOG, 1);
        assert_eq!(want(&mut inventory, Block::OakPlanks, 10), Want::Missing(LOG, 1));
        inventory.add(LOG, 1);
        assert_eq!(want(&mut inventory, Block::OakPlanks, 10), Want::Done);
        assert_eq!(inventory.count(Block::OakPlanks), 10);
        assert_eq!(want(&mut inventory, Block::Stone, 1), Want::Missing(Block::Stone, 1));
    }

    #[test]
    fn crafting_stops_when_the_output_does_not_fit() {
        let full = |planks, logs| {
            let mut inventory = Inventory::default();
            for _ in 0..SLOTS - 2 {
                inventory.add(Block::Stone, STACK_SIZE);
            }
            inventory.add(Block::OakPlanks, planks);
            inventory.add(LOG, logs);
            inventory
        };
        // The last log's slot is freed as it's used, so the planks spill into it.
        let mut inventory = full(62, 1);
        assert_eq!(want(&mut inventory, Block::OakPlanks, 66), Want::Done);
        let mut inventory = full(64, 2);
        assert_eq!(want(&mut inventory, Block::OakPlanks, 72), Want::NoRoom);
        assert_eq!(inventory.count(LOG), 2);
    }
}
