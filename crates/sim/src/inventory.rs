//! What a player carries: stacks of blocks in numbered slots, the first `HOTBAR` of which are the hotbar.

use crate::world::Block;

pub const SLOTS: usize = 36;
/// Slots `0..HOTBAR` are the hotbar, which a player places blocks from.
pub const HOTBAR: usize = 9;
/// Most blocks of one kind a slot holds.
pub const STACK_SIZE: u8 = 64;

/// Some number of one kind of block, from 1 to `STACK_SIZE`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Stack {
    pub block: Block,
    pub count: u8,
}

#[derive(Clone)]
pub struct Inventory {
    slots: [Option<Stack>; SLOTS],
}

impl Default for Inventory {
    fn default() -> Self {
        Self { slots: [None; SLOTS] }
    }
}

impl Inventory {
    pub fn slot(&self, slot: usize) -> Option<Stack> {
        self.slots[slot]
    }

    /// Adds `count` of `block`, topping up stacks of it that have room before starting new ones, each in slot order as
    /// Minecraft does. Returns how many didn't fit.
    pub fn add(&mut self, block: Block, mut count: u8) -> u8 {
        for slot in self.slots.iter_mut().flatten().filter(|stack| stack.block == block) {
            let moved = count.min(STACK_SIZE - slot.count);
            slot.count += moved;
            count -= moved;
        }
        for slot in self.slots.iter_mut().filter(|slot| slot.is_none()) {
            if count == 0 {
                break;
            }
            let moved = count.min(STACK_SIZE);
            *slot = Some(Stack { block, count: moved });
            count -= moved;
        }
        count
    }

    /// Swaps what two slots hold, as moving stacks round the inventory does.
    pub fn swap(&mut self, a: usize, b: usize) {
        self.slots.swap(a, b);
    }

    /// How many of `block` it holds across every slot.
    pub fn count(&self, block: Block) -> u32 {
        self.slots
            .iter()
            .flatten()
            .filter(|stack| stack.block == block)
            .map(|stack| u32::from(stack.count))
            .sum()
    }

    /// Takes `count` of `block`, from the last slots first so the hotbar keeps what it holds as long as it can. Takes nothing
    /// and returns false if it holds fewer.
    pub fn remove(&mut self, block: Block, mut count: u32) -> bool {
        if self.count(block) < count {
            return false;
        }
        for slot in self.slots.iter_mut().rev() {
            let Some(stack) = slot.as_mut().filter(|stack| stack.block == block) else {
                continue;
            };
            let taken = count.min(u32::from(stack.count));
            stack.count -= taken as u8;
            count -= taken;
            if stack.count == 0 {
                *slot = None;
            }
        }
        true
    }

    /// Takes one block out of `slot`, if it holds any.
    pub fn take(&mut self, slot: usize) -> Option<Block> {
        let stack = self.slots[slot].as_mut()?;
        let block = stack.block;
        stack.count -= 1;
        if stack.count == 0 {
            self.slots[slot] = None;
        }
        Some(block)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_stack_up_to_the_limit_then_fill_the_next_empty_slot() {
        let mut inventory = Inventory::default();
        assert_eq!(inventory.add(Block::Dirt, 10), 0);
        assert_eq!(inventory.add(Block::Stone, 1), 0);
        assert_eq!(inventory.add(Block::Dirt, 60), 0);
        assert_eq!(
            inventory.slot(0),
            Some(Stack {
                block: Block::Dirt,
                count: 64
            })
        );
        assert_eq!(
            inventory.slot(1),
            Some(Stack {
                block: Block::Stone,
                count: 1
            })
        );
        assert_eq!(
            inventory.slot(2),
            Some(Stack {
                block: Block::Dirt,
                count: 6
            })
        );
        assert_eq!(inventory.slot(3), None);
    }

    #[test]
    fn a_full_inventory_hands_back_what_does_not_fit() {
        let mut inventory = Inventory::default();
        for _ in 0..SLOTS - 1 {
            assert_eq!(inventory.add(Block::Stone, STACK_SIZE), 0);
        }
        assert_eq!(inventory.add(Block::Sand, 63), 0);
        assert_eq!(inventory.add(Block::Sand, 3), 2);
        assert_eq!(inventory.add(Block::Dirt, 5), 5);
        assert_eq!(
            inventory.slot(SLOTS - 1),
            Some(Stack {
                block: Block::Sand,
                count: 64
            })
        );
    }

    #[test]
    fn removing_takes_from_the_last_slots_first() {
        let mut inventory = Inventory::default();
        inventory.add(Block::Sand, 64);
        inventory.add(Block::Dirt, 1);
        inventory.add(Block::Sand, 10);
        assert_eq!(inventory.count(Block::Sand), 74);
        assert!(!inventory.remove(Block::Sand, 75));
        assert!(inventory.remove(Block::Sand, 12));
        assert_eq!(inventory.slot(2), None);
        assert_eq!(inventory.count(Block::Sand), 62);
        assert_eq!(inventory.slot(1).map(|stack| stack.block), Some(Block::Dirt));
    }

    #[test]
    fn taking_empties_a_slot_one_block_at_a_time() {
        let mut inventory = Inventory::default();
        inventory.add(Block::Gravel, 2);
        assert_eq!(inventory.take(0), Some(Block::Gravel));
        assert_eq!(
            inventory.slot(0),
            Some(Stack {
                block: Block::Gravel,
                count: 1
            })
        );
        assert_eq!(inventory.take(0), Some(Block::Gravel));
        assert_eq!(inventory.slot(0), None);
        assert_eq!(inventory.take(0), None);
        // An emptied slot is filled again before later ones.
        inventory.add(Block::Gravel, 1);
        assert!(inventory.slot(0).is_some());
    }
}
