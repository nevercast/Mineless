//! What a Rust script compiled to `wasm32-unknown-unknown` uses to see and act as its actor. The game calls the script's
//! exported `run` once and resumes it every tick: the script runs as plain blocking code, and `wait_ticks` hands the rest of
//! the tick back. Everything crossing to the game is plain numbers and `#[repr(C)]` structs in the script's memory, so a
//! script in any language that compiles to WASM can import the same functions from the `mineless` module.

/// A kind of block. It crosses to the game as its place in this list, which scripts never need to see, so new kinds only
/// ever go on the end.
#[repr(u32)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Block {
    Air,
    Grass,
    Dirt,
    Stone,
    Cobblestone,
    Sand,
    Gravel,
    Snow,
    /// Whichever way its grain runs.
    OakLog,
    OakLeaves,
    Water,
    WorldBorder,
    OakPlanks,
    /// Whichever way they face.
    OakStairs,
}

#[cfg(any(target_arch = "wasm32", test))]
impl Block {
    const ALL: [Block; 14] = {
        use Block::*;
        [
            Air,
            Grass,
            Dirt,
            Stone,
            Cobblestone,
            Sand,
            Gravel,
            Snow,
            OakLog,
            OakLeaves,
            Water,
            WorldBorder,
            OakPlanks,
            OakStairs,
        ]
    };

    /// `None` for a block added to the game after this version of the API.
    fn from_wire(number: u32) -> Option<Block> {
        Block::ALL.get(number as usize).copied()
    }
}

/// What the actor knows about itself at the start of the tick.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct State {
    /// The middle of the bottom of its body.
    pub feet: [f32; 3],
    /// Blocks per tick; up is positive.
    pub vertical_velocity: f32,
    /// 1 if the last tick ended standing on a block.
    pub on_ground: u32,
    /// 1 for each axis collision cut the last tick's movement short on.
    pub blocked: [u32; 3],
    /// 1 if a block is in reach where the head looks, and then the block and the face the look enters it through.
    pub has_target: u32,
    pub target: [i32; 3],
    pub face: [i32; 3],
}

/// Everything the actor does, held until changed. Angles are radians: yaw turns around +Y with zero facing -Z, and the head's
/// yaw and pitch are to the left of and above the body's facing.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct Controls {
    pub yaw: f32,
    pub head_yaw: f32,
    pub head_pitch: f32,
    /// `[right, forward]`, scaled down if longer than 1.
    pub walk: [f32; 2],
    /// Jump to peak this many blocks up (0.5 to 1.25) on the next tick that starts on the ground; 0 for no jump.
    pub jump: f32,
    /// 1 to hold attack, which breaks the targeted block after a time that depends on the block.
    pub attack: u32,
    /// 1 to hold place, which puts a block from the selected hotbar slot against the targeted face.
    pub place: u32,
    /// The selected hotbar slot, below `HOTBAR`.
    pub slot: u32,
}

/// Something the actor can craft from what it carries: `count` of `output` each time, up to `times` times.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Craft {
    pub output: Block,
    pub count: u32,
    pub times: u32,
}

/// What `want` says stands between the actor and holding what it wants.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Want {
    /// It holds enough.
    Done,
    /// It has to gather this many more of a block first.
    Missing(Block, u32),
    /// A craft's output didn't fit in the inventory.
    NoRoom,
}

/// Slots in an inventory, the first `HOTBAR` of which can be selected for placing.
pub const SLOTS: u32 = 36;
pub const HOTBAR: u32 = 9;
/// How far from its feet an actor sees blocks.
pub const SIGHT: i32 = 64;

// Only scripts have a game to provide these. Leaving them out of native builds lets the game share the types above without
// linking imports nothing provides, which Windows refuses.
#[cfg(target_arch = "wasm32")]
pub use calls::*;

#[cfg(target_arch = "wasm32")]
mod calls {
    use super::{Block, Controls, Craft, State, Want};

    /// Up to 64 of one kind of block in an inventory slot.
    #[repr(C)]
    #[derive(Default)]
    struct Stack {
        block: u32,
        count: u32,
    }

    /// `Craft` as it crosses from the game.
    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct WireCraft {
        output: u32,
        count: u32,
        times: u32,
    }

    mod host {
        use super::{Controls, Stack, State, WireCraft};

        #[link(wasm_import_module = "mineless")]
        unsafe extern "C" {
            pub fn wait_ticks(ticks: u32);
            pub fn observe(state: *mut State);
            pub fn set_controls(controls: *const Controls);
            pub fn block(x: i32, y: i32, z: i32) -> u32;
            pub fn find_nearest(block: u32, found: *mut [i32; 3], capacity: u32) -> u32;
            pub fn inventory(slot: u32, stack: *mut Stack) -> u32;
            pub fn crafts(found: *mut WireCraft, capacity: u32) -> u32;
            pub fn craft(output: u32, times: u32) -> u32;
            pub fn want(block: u32, amount: u32, missing: *mut Stack) -> u32;
            pub fn log(text: *const u8, len: usize);
        }
    }

    /// Ends this tick's turn and carries on `ticks` ticks later; 1 is the next tick.
    pub fn wait_ticks(ticks: u32) {
        // SAFETY: takes plain numbers only.
        unsafe { host::wait_ticks(ticks) }
    }

    pub fn observe() -> State {
        let mut state = State::default();
        // SAFETY: the game writes one `State` through a pointer to a live one.
        unsafe { host::observe(&mut state) };
        state
    }

    /// Replaces every control at once. They take effect on the next tick.
    pub fn set_controls(controls: &Controls) {
        // SAFETY: the game only reads one `Controls` through a pointer to a live one.
        unsafe { host::set_controls(controls) }
    }

    /// The block at a position, or `None` if it's further than `SIGHT` from the feet or the game's block is newer than this API.
    pub fn block(position: [i32; 3]) -> Option<Block> {
        let [x, y, z] = position;
        // SAFETY: takes plain numbers only.
        Block::from_wire(unsafe { host::block(x, y, z) })
    }

    /// The blocks of one kind within `SIGHT` of the feet, nearest first, filling at most `found.len()` of `found`. Returns how
    /// many it found. The search costs the script time in proportion to the space searched.
    pub fn find_nearest(block: Block, found: &mut [[i32; 3]]) -> usize {
        let capacity = u32::try_from(found.len()).unwrap_or(u32::MAX);
        // SAFETY: the game writes at most `capacity` positions into `found`.
        unsafe { host::find_nearest(block as u32, found.as_mut_ptr(), capacity) as usize }
    }

    /// What's in an inventory slot, `0..SLOTS`. Empty slots and slots past the end are `None`.
    pub fn inventory(slot: u32) -> Option<(Block, u32)> {
        let mut stack = Stack::default();
        // SAFETY: the game writes one `Stack` through a pointer to a live one.
        let filled = unsafe { host::inventory(slot, &mut stack) } != 0;
        filled.then(|| Some((Block::from_wire(stack.block)?, stack.count))).flatten()
    }

    /// What the actor can craft right now, filling at most `found.len()` of `found`. Returns how many it filled.
    pub fn crafts(found: &mut [Craft]) -> usize {
        let mut wire = vec![WireCraft::default(); found.len()];
        let capacity = u32::try_from(wire.len()).unwrap_or(u32::MAX);
        // SAFETY: the game writes at most `capacity` crafts into `wire`.
        let filled = unsafe { host::crafts(wire.as_mut_ptr(), capacity) } as usize;
        let known = wire[..filled].iter().filter_map(|craft| {
            Some(Craft {
                output: Block::from_wire(craft.output)?,
                count: craft.count,
                times: craft.times,
            })
        });
        let mut count = 0;
        for (slot, craft) in found.iter_mut().zip(known) {
            *slot = craft;
            count += 1;
        }
        count
    }

    /// Crafts what makes `output` up to `times` times, as often as the inventory allows, and returns how many times it did.
    pub fn craft(output: Block, times: u32) -> u32 {
        // SAFETY: takes plain numbers only.
        unsafe { host::craft(output as u32, times) }
    }

    /// Works towards holding `amount` of `block`: crafts everything it can that leads there, then says what still has to be
    /// gathered. Gathering what it names and asking again ends with the amount held.
    pub fn want(block: Block, amount: u32) -> Want {
        let mut missing = Stack::default();
        // SAFETY: the game writes one `Stack` through a pointer to a live one.
        match unsafe { host::want(block as u32, amount, &mut missing) } {
            0 => Want::Done,
            1 => Block::from_wire(missing.block).map_or(Want::NoRoom, |block| Want::Missing(block, missing.count)),
            _ => Want::NoRoom,
        }
    }

    /// Writes a line to the game's console.
    pub fn log(text: &str) {
        // SAFETY: the game only reads `len` bytes from `text`.
        unsafe { host::log(text.as_ptr(), text.len()) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_cross_as_their_place_in_the_list_and_later_ones_are_unknown() {
        for (number, block) in Block::ALL.into_iter().enumerate() {
            assert_eq!(block as usize, number);
            assert_eq!(Block::from_wire(number as u32), Some(block));
        }
        assert_eq!(Block::from_wire(Block::ALL.len() as u32), None);
    }
}
