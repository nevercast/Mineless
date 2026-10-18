//! Runs a script compiled to WASM as one player. The script's exported `run` is called once and is plain blocking code: each
//! tick it gets a fuel budget and carries on where it stopped, which was either a `wait_ticks` call or running out of fuel. It
//! sees and acts only through the `mineless` imports that `agent-api` wraps.

use std::fmt;

use glam::{IVec3, Vec2};
use sim::crafting::{self, RECIPES, Want};
use sim::game::Game;
use sim::player::{Controls, Player};
use sim::world::{Axis, Block, CHUNK_SIZE, Facing, World};
use wasmi::{
    Caller, Config, Engine, Error, Extern, Linker, Module, Store, StoreLimits, StoreLimitsBuilder, TypedFunc, TypedResumableCall,
    TypedResumableCallHostTrap, TypedResumableCallOutOfFuel, errors::HostError,
};

/// Fuel a script gets each tick, roughly one per WASM instruction. Enough for a script to look around and decide, small enough
/// that a script stuck in a loop costs the tick well under a millisecond.
const FUEL_PER_TICK: u64 = 100_000;
/// Linear memory a script may grow to.
const MEMORY_LIMIT: usize = 16 << 20;
/// How far from its feet a script sees blocks, as `agent_api::SIGHT` says.
const SIGHT: i32 = 64;
/// Fuel `find_nearest` charges per chunk it looks at, and per block in the chunks it has to search block by block.
const CHUNK_LOOK_FUEL: u64 = 16;
const BLOCK_SEARCH_FUEL: u64 = 1;
/// What `block` gives for blocks it can't see.
const UNSEEN: u32 = u32::MAX;

/// The number a block crosses to the script as: its `agent_api::Block`'s.
fn wire(block: Block) -> u32 {
    use agent_api::Block as Api;
    (match block {
        Block::Air => Api::Air,
        Block::Grass => Api::Grass,
        Block::Dirt => Api::Dirt,
        Block::Stone => Api::Stone,
        Block::Cobblestone => Api::Cobblestone,
        Block::Sand => Api::Sand,
        Block::Gravel => Api::Gravel,
        Block::Snow => Api::Snow,
        Block::OakLog(_) => Api::OakLog,
        Block::OakLeaves => Api::OakLeaves,
        Block::OakPlanks => Api::OakPlanks,
        Block::OakStairs(_) => Api::OakStairs,
        Block::CraftingTable => Api::CraftingTable,
        Block::Water => Api::Water,
        Block::WorldBorder => Api::WorldBorder,
    }) as u32
}

/// The block a number from the script stands for: the first in the game's blocks that crosses as it.
fn unwire(number: u32) -> Option<Block> {
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
        OakLog(Axis::Y),
        OakLeaves,
        OakPlanks,
        OakStairs(Facing::North),
        CraftingTable,
        Water,
        WorldBorder,
    ]
    .into_iter()
    .find(|&block| wire(block) == number)
}

/// A script driving one player.
pub struct Agent {
    store: Store<Host>,
    run: Run,
}

/// Where the script is in its one long call to `run`.
enum Run {
    Start(TypedFunc<(), ()>),
    Waiting(TypedResumableCallHostTrap<()>),
    OutOfFuel(TypedResumableCallOutOfFuel<()>),
    /// It returned, or it failed and was stopped.
    Over,
}

/// What the script's imports work on. The game is lent to it for the length of a turn.
struct Host {
    game: Game,
    player: usize,
    /// What it's called, as its log lines show.
    name: String,
    /// Ticks still to skip after a `wait_ticks`.
    waiting: u32,
    limits: StoreLimits,
}

/// Raised by `wait_ticks` to end the script's turn; the call resumes from it next time.
#[derive(Debug)]
struct Yield;

impl fmt::Display for Yield {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("waiting for a later tick")
    }
}

impl HostError for Yield {}

impl Agent {
    /// Loads a script to drive the player at `player` in the game's players.
    pub fn load(wasm: &[u8], player: usize, name: String) -> Result<Agent, Error> {
        let mut config = Config::default();
        config.consume_fuel(true);
        let engine = Engine::new(&config);
        let module = Module::new(&engine, wasm)?;
        let host = Host {
            game: Game::default(),
            player,
            name,
            waiting: 0,
            limits: StoreLimitsBuilder::new().memory_size(MEMORY_LIMIT).build(),
        };
        let mut store = Store::new(&engine, host);
        store.limiter(|host| &mut host.limits);
        store.set_fuel(FUEL_PER_TICK)?;
        let instance = linker(&engine)?.instantiate_and_start(&mut store, &module)?;
        let run = instance.get_typed_func::<(), ()>(&store, "run")?;
        Ok(Agent {
            store,
            run: Run::Start(run),
        })
    }

    pub fn name(&self) -> &str {
        &self.store.data().name
    }

    /// Gives the script its turn for this tick, before the game ticks. A script that fails is stopped and its player left
    /// standing still, and the error is returned once.
    pub fn turn(&mut self, game: &mut Game) -> Result<(), Error> {
        if self.store.data().waiting > 0 {
            self.store.data_mut().waiting -= 1;
            return Ok(());
        }
        self.store.data_mut().game = std::mem::take(game);
        let result = self.resume();
        *game = std::mem::take(&mut self.store.data_mut().game);
        if result.is_err() {
            let controls = &mut game.players[self.store.data().player].controls;
            *controls = Controls {
                yaw: controls.yaw,
                head_yaw: controls.head_yaw,
                head_pitch: controls.head_pitch,
                ..Controls::default()
            };
        }
        result
    }

    fn resume(&mut self) -> Result<(), Error> {
        self.store.set_fuel(FUEL_PER_TICK)?;
        let call = match std::mem::replace(&mut self.run, Run::Over) {
            Run::Start(run) => run.call_resumable(&mut self.store, ()),
            Run::Waiting(call) => call.resume(&mut self.store, &[]),
            Run::OutOfFuel(call) => call.resume(&mut self.store),
            Run::Over => return Ok(()),
        };
        self.run = match call? {
            TypedResumableCall::Finished(()) => Run::Over,
            TypedResumableCall::HostTrap(call) if call.host_error().downcast_ref::<Yield>().is_some() => Run::Waiting(call),
            TypedResumableCall::HostTrap(call) => return Err(Error::new(call.host_error().to_string())),
            TypedResumableCall::OutOfFuel(call) => Run::OutOfFuel(call),
        };
        Ok(())
    }
}

fn linker(engine: &Engine) -> Result<Linker<Host>, Error> {
    let mut linker = Linker::new(engine);
    linker
        .func_wrap("mineless", "wait_ticks", wait_ticks)?
        .func_wrap("mineless", "observe", observe)?
        .func_wrap("mineless", "set_controls", set_controls)?
        .func_wrap("mineless", "block", block)?
        .func_wrap("mineless", "find_nearest", find_nearest)?
        .func_wrap("mineless", "inventory", inventory)?
        .func_wrap("mineless", "crafts", crafts)?
        .func_wrap("mineless", "craft", craft)?
        .func_wrap("mineless", "want", want)?
        .func_wrap("mineless", "log", log)?;
    Ok(linker)
}

fn wait_ticks(mut caller: Caller<'_, Host>, ticks: u32) -> Result<(), Error> {
    if ticks == 0 {
        return Ok(());
    }
    caller.data_mut().waiting = ticks - 1;
    Err(Error::host(Yield))
}

/// Writes `agent_api::State`.
fn observe(mut caller: Caller<'_, Host>, address: u32) -> Result<(), Error> {
    let host = caller.data();
    let player = &host.game.players[host.player];
    let feet = player.avatar(1.0).position;
    let target = player.target(&host.game.world);
    let blocked = player.blocked();
    let (block, face) = target.map_or((IVec3::ZERO, IVec3::ZERO), |hit| (hit.block, hit.face));
    let words = [
        feet.x.to_bits(),
        feet.y.to_bits(),
        feet.z.to_bits(),
        player.vertical_velocity().to_bits(),
        player.is_on_ground().into(),
        blocked.x.into(),
        blocked.y.into(),
        blocked.z.into(),
        target.is_some().into(),
        block.x as u32,
        block.y as u32,
        block.z as u32,
        face.x as u32,
        face.y as u32,
        face.z as u32,
    ];
    write(&mut caller, address, words)
}

/// Reads `agent_api::Controls`.
fn set_controls(mut caller: Caller<'_, Host>, address: u32) -> Result<(), Error> {
    let [yaw, head_yaw, head_pitch, right, forward, jump, attack, place, slot] = read(&caller, address)?;
    let host = caller.data_mut();
    host.game.players[host.player].controls = Controls {
        yaw: f32::from_bits(yaw),
        head_yaw: f32::from_bits(head_yaw),
        head_pitch: f32::from_bits(head_pitch),
        walk: Vec2::new(f32::from_bits(right), f32::from_bits(forward)),
        jump: Some(f32::from_bits(jump)).filter(|&height| height > 0.0),
        attack: attack != 0,
        place: place != 0,
        slot: slot as usize,
    };
    Ok(())
}

fn block(caller: Caller<'_, Host>, x: i32, y: i32, z: i32) -> u32 {
    let host = caller.data();
    let position = IVec3::new(x, y, z);
    if in_sight(feet(&host.game.players[host.player]), position) {
        wire(host.game.world.block(position))
    } else {
        UNSEEN
    }
}

/// Writes up to `capacity` positions of `wanted` blocks within sight, nearest first, and returns how many.
fn find_nearest(mut caller: Caller<'_, Host>, wanted: u32, address: u32, capacity: u32) -> Result<u32, Error> {
    let host = caller.data();
    let feet = feet(&host.game.players[host.player]);
    let (mut found, fuel) = search(&host.game.world, feet, wanted);
    found.sort_by_key(|&position| ((position - feet).length_squared(), position.to_array()));
    found.truncate(capacity as usize);
    let left = caller.get_fuel()?.saturating_sub(fuel);
    caller.set_fuel(left)?;
    let count = found.len() as u32;
    write(
        &mut caller,
        address,
        found.into_iter().flat_map(|position| position.to_array()).map(|n| n as u32),
    )?;
    Ok(count)
}

/// Every `wanted` block within sight of `feet`, and the fuel finding them costs. Chunks filled with one other block are
/// skipped whole.
fn search(world: &World, feet: IVec3, wanted: u32) -> (Vec<IVec3>, u64) {
    let size = CHUNK_SIZE as i32;
    let (first, last) = (
        (feet - SIGHT).div_euclid(IVec3::splat(size)),
        (feet + SIGHT).div_euclid(IVec3::splat(size)),
    );
    let mut found = Vec::new();
    let mut fuel = 0;
    for x in first.x..=last.x {
        for y in first.y..=last.y {
            for z in first.z..=last.z {
                let chunk = IVec3::new(x, y, z);
                let min = chunk * size;
                fuel += CHUNK_LOOK_FUEL;
                if !world.is_loaded(min) || world.uniform_chunk(chunk).is_some_and(|block| wire(block) != wanted) {
                    continue;
                }
                let blocks = world.blocks_in(min, min + size);
                fuel += blocks.len() as u64 * BLOCK_SEARCH_FUEL;
                // `blocks_in` goes along x, then z, then y.
                let positions = (0..size).flat_map(|y| (0..size).flat_map(move |z| (0..size).map(move |x| min + IVec3::new(x, y, z))));
                found.extend(
                    positions
                        .zip(blocks)
                        .filter(|&(position, block)| wire(block) == wanted && in_sight(feet, position))
                        .map(|(position, _)| position),
                );
            }
        }
    }
    (found, fuel)
}

/// Writes `agent_api::Stack` for a slot and returns 1, or returns 0 for an empty slot or one past the end.
fn inventory(mut caller: Caller<'_, Host>, slot: u32, address: u32) -> Result<u32, Error> {
    let host = caller.data();
    let Some(stack) = host.game.players[host.player].inventory.slot(slot as usize) else {
        return Ok(0);
    };
    write(&mut caller, address, [wire(stack.block), stack.count.into()])?;
    Ok(1)
}

/// Writes `agent_api::Craft` for up to `capacity` recipes the player can craft now, and returns how many.
fn crafts(mut caller: Caller<'_, Host>, address: u32, capacity: u32) -> Result<u32, Error> {
    let host = caller.data();
    let player = &host.game.players[host.player];
    let (inventory, at_table) = (&player.inventory, player.at_crafting_table(&host.game.world));
    let craftable: Vec<[u32; 3]> = RECIPES
        .iter()
        .map(|recipe| {
            [
                wire(recipe.output),
                recipe.count.into(),
                crafting::times(inventory, recipe, at_table),
            ]
        })
        .filter(|&[.., times]| times > 0)
        .take(capacity as usize)
        .collect();
    let count = craftable.len() as u32;
    write(&mut caller, address, craftable.into_iter().flatten())?;
    Ok(count)
}

/// Crafts what makes `output` up to `times` times, and returns how many times it did.
fn craft(mut caller: Caller<'_, Host>, output: u32, times: u32) -> u32 {
    let Some(recipe) = unwire(output).and_then(crafting::recipe_for) else {
        return 0;
    };
    let host = caller.data_mut();
    let player = &mut host.game.players[host.player];
    let at_table = player.at_crafting_table(&host.game.world);
    crafting::craft(&mut player.inventory, recipe, times, at_table)
}

/// Works towards holding `amount` of `wanted`, as `crafting::want` does. Returns 0 when it holds them, 1 with the missing
/// `agent_api::Stack` written when something has to be gathered first, 2 when a craft's output didn't fit, and 3 when a
/// craft needs a crafting table in reach.
fn want(mut caller: Caller<'_, Host>, wanted: u32, amount: u32, missing: u32) -> Result<u32, Error> {
    let Some(block) = unwire(wanted) else {
        return Err(Error::new("want was asked for a block that doesn't exist"));
    };
    let host = caller.data_mut();
    let player = &mut host.game.players[host.player];
    let at_table = player.at_crafting_table(&host.game.world);
    match crafting::want(&mut player.inventory, block, amount, at_table) {
        Want::Done => Ok(0),
        Want::Missing(block, count) => {
            write(&mut caller, missing, [wire(block), count])?;
            Ok(1)
        }
        Want::NoRoom => Ok(2),
        Want::NeedTable => Ok(3),
    }
}

fn log(caller: Caller<'_, Host>, address: u32, len: u32) -> Result<(), Error> {
    let mut text = vec![0; len as usize];
    memory(&caller)?
        .read(&caller, address as usize, &mut text)
        .map_err(|_| Error::new("log text runs past the end of memory"))?;
    eprintln!("[{}] {}", caller.data().name, String::from_utf8_lossy(&text));
    Ok(())
}

/// The block the player's feet are in.
fn feet(player: &Player) -> IVec3 {
    player.avatar(1.0).position.floor().as_ivec3()
}

fn in_sight(feet: IVec3, position: IVec3) -> bool {
    (position - feet).length_squared() <= SIGHT * SIGHT
}

fn memory(caller: &Caller<'_, Host>) -> Result<wasmi::Memory, Error> {
    caller
        .get_export("memory")
        .and_then(Extern::into_memory)
        .ok_or_else(|| Error::new("the script exports no memory"))
}

/// Writes 32-bit words, little-endian as WASM is, at `address` in the script's memory.
fn write(caller: &mut Caller<'_, Host>, address: u32, words: impl IntoIterator<Item = u32>) -> Result<(), Error> {
    let bytes: Vec<u8> = words.into_iter().flat_map(u32::to_le_bytes).collect();
    memory(caller)?
        .write(caller, address as usize, &bytes)
        .map_err(|_| Error::new("a write runs past the end of the script's memory"))
}

/// Reads `N` 32-bit words at `address` in the script's memory.
fn read<const N: usize>(caller: &Caller<'_, Host>, address: u32) -> Result<[u32; N], Error> {
    let mut bytes = vec![0; N * 4];
    memory(caller)?
        .read(caller, address as usize, &mut bytes)
        .map_err(|_| Error::new("a read runs past the end of the script's memory"))?;
    Ok(std::array::from_fn(|i| {
        u32::from_le_bytes([bytes[i * 4], bytes[i * 4 + 1], bytes[i * 4 + 2], bytes[i * 4 + 3]])
    }))
}

#[cfg(test)]
mod tests {
    use glam::Vec3;
    use sim::avatar::Skin;
    use sim::world::Axis;

    use super::*;

    /// A stone floor at y = 0 with one player standing on it at (8.5, 1, 8.5).
    fn game() -> Game {
        let mut world = World::empty(IVec3::new(1, 4, 1));
        for x in 0..16 {
            for z in 0..16 {
                world.set_block(IVec3::new(x, 0, z), Block::Stone);
            }
        }
        Game {
            world,
            players: vec![Player::new(Vec3::new(8.5, 1.0, 8.5), 0.0, Skin::Player)],
        }
    }

    /// A script whose `run` is `body`, with `wait_ticks` and `set_controls` imported, and controls in memory to walk forward at
    /// address 0 and to stand still at address 64.
    fn script(body: &str) -> Agent {
        let wat = format!(
            r#"(module
                (import "mineless" "wait_ticks" (func $wait (param i32)))
                (import "mineless" "set_controls" (func $set (param i32)))
                (memory (export "memory") 1)
                (data (i32.const 16) "\00\00\80\3f")
                (func (export "run") (local $i i32) {body}))"#
        );
        Agent::load(wat.as_bytes(), 0, "test".to_owned()).expect("the test script loads")
    }

    fn walking(game: &Game) -> bool {
        game.players[0].controls.walk.y == 1.0
    }

    #[test]
    fn layouts_match_the_script_api() {
        assert_eq!(size_of::<agent_api::State>(), 15 * 4);
        assert_eq!(size_of::<agent_api::Controls>(), 9 * 4);
        assert_eq!(wire(Block::OakLog(Axis::X)), wire(Block::OakLog(Axis::Z)));
        assert_eq!(wire(Block::OakStairs(Facing::East)), wire(Block::OakStairs(Facing::West)));
        for number in 0..=wire(Block::CraftingTable) {
            assert_eq!(unwire(number).map(wire), Some(number));
        }
    }

    #[test]
    fn waiting_skips_ticks_and_carries_on_where_it_stopped() {
        let mut game = game();
        let mut agent = script("(call $set (i32.const 0)) (call $wait (i32.const 3)) (call $set (i32.const 64))");
        let mut walked = Vec::new();
        for _ in 0..5 {
            agent.turn(&mut game).expect("the script runs");
            walked.push(walking(&game));
        }
        assert_eq!(walked, [true, true, true, false, false]);
    }

    #[test]
    fn running_out_of_fuel_pauses_until_the_next_tick() {
        let mut game = game();
        let mut agent = script(
            "(loop $count (local.set $i (i32.add (local.get $i) (i32.const 1)))
                (br_if $count (i32.lt_u (local.get $i) (i32.const 1000000))))
            (call $set (i32.const 0))",
        );
        let mut turns = 0;
        while !walking(&game) {
            agent.turn(&mut game).expect("running out of fuel isn't an error");
            turns += 1;
            assert!(turns < 1000);
        }
        assert!(turns > 1, "a million loops fit in one tick's fuel");
    }

    #[test]
    fn a_failing_script_is_stopped_and_its_player_stands_still() {
        let mut game = game();
        let mut agent = script("(call $set (i32.const 0)) unreachable");
        assert!(agent.turn(&mut game).is_err());
        assert!(!walking(&game));
        assert!(agent.turn(&mut game).is_ok());
    }

    #[test]
    fn wanting_planks_crafts_them_from_a_log() {
        let mut game = game();
        game.players[0].inventory.add(Block::OakLog(Axis::Y), 1);
        let wat = format!(
            r#"(module
                (import "mineless" "want" (func $want (param i32 i32 i32) (result i32)))
                (memory (export "memory") 1)
                (func (export "run") (drop (call $want (i32.const {}) (i32.const 4) (i32.const 0)))))"#,
            wire(Block::OakPlanks)
        );
        let mut agent = Agent::load(wat.as_bytes(), 0, "test".to_owned()).expect("the test script loads");
        agent.turn(&mut game).expect("the script runs");
        assert_eq!(game.players[0].inventory.count(Block::OakPlanks), 4);
        assert_eq!(game.players[0].inventory.count(Block::OakLog(Axis::Y)), 0);
    }

    #[test]
    fn search_finds_blocks_within_sight_only_and_charges_for_mixed_chunks() {
        let mut world = World::empty(IVec3::new(8, 1, 1));
        let feet = IVec3::new(8, 1, 8);
        for x in [12, 20, 8 + SIGHT, 9 + SIGHT] {
            world.set_block(IVec3::new(x, 1, 8), Block::Cobblestone);
        }
        let (mut found, fuel) = search(&world, feet, wire(Block::Cobblestone));
        found.sort_by_key(|position| position.x);
        assert_eq!(found, [12, 20, 8 + SIGHT].map(|x| IVec3::new(x, 1, 8)));
        // Nine chunks along each axis are looked at, and the three holding cobblestone are searched.
        assert_eq!(fuel, 9 * 9 * 9 * CHUNK_LOOK_FUEL + 3 * 4096 * BLOCK_SEARCH_FUEL);
    }
}
