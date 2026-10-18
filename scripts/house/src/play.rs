//! The builder at work: everything it does, a tick at a time, through the game's calls.

use std::collections::HashSet;
use std::f32::consts::{FRAC_PI_2, PI, TAU};

use agent_api::{
    Block, Controls, EYE_HEIGHT, HOTBAR, REACH, SLOTS, Want, block, find_nearest, inventory, log, observe, set_controls, swap_slots,
    wait_ticks, want,
};

use crate::path::{path, reachable};
use crate::plan::{Piece, SITE, Site, find_site};
use crate::sight::{eye, face_middle, look, ray};
use crate::{Cell, SIDES, offset, open, solid};

/// How far round itself it looks for level ground, along each axis.
const SITE_RADIUS: i32 = 20;
/// How many places a search for a way looks at before giving up. Gathering gives up sooner: there are other trees.
const PATH_LIMIT: usize = 20_000;
const GATHER_PATH_LIMIT: usize = 3_000;
/// Blocks a body walks in a tick.
const WALK_PER_TICK: f32 = 4.317 / 20.0;
const JUMP: f32 = 1.25;
/// Blocks it builds pillars from, to stand on.
const FILLERS: [Block; 5] = [Block::Dirt, Block::Cobblestone, Block::Sand, Block::Gravel, Block::Snow];
/// The highest pillar it stands on: three blocks puts its eyes above the roof's ridge.
const PILLAR: i32 = 3;
/// Times it tries to break or place a block, from different places, before going without it.
const TRIES: usize = 4;
/// How far the head turns from the body's facing at most, while the body keeps facing the way stairs must.
const NECK: f32 = 100.0 * PI / 180.0;
/// What the house takes besides its crafting table.
const PLANKS: u32 = 56;
const STAIRS: u32 = 20;
/// Planks that make the stairs.
const STAIR_PLANKS: u32 = STAIRS / 4 * 6;

#[unsafe(no_mangle)]
pub extern "C" fn run() {
    let mut builder = Builder::default();
    let site = builder.find_site();
    log(&format!("building at {:?}, on ground {} high", site.corner, site.floor));
    builder.clear(&site);
    log("site cleared");
    if !builder.obtain(Block::CraftingTable, 1, &site) || !builder.place(&site.table(), &site) {
        log("couldn't set up a crafting table, giving up");
        builder.stop();
        return;
    }
    let held = count(Block::OakPlanks) + 4 * count(Block::OakLog);
    builder.gather_logs((PLANKS + STAIR_PLANKS).saturating_sub(held).div_ceil(4));
    if !builder.obtain(Block::OakStairs, STAIRS, &site) || !builder.obtain(Block::OakPlanks, PLANKS, &site) {
        log("couldn't craft what the house takes, giving up");
        builder.stop();
        return;
    }
    log("crafted planks and stairs, building");
    builder.gather_fillers(&site);
    let pieces = site.blueprint();
    let missed = pieces.iter().filter(|piece| !builder.place(piece, &site)).count();
    builder.step_down(site.floor + 1);
    builder.stop();
    if missed == 0 {
        log("house built");
    } else {
        log(&format!("house built, but {missed} blocks of it couldn't be placed"));
    }
}

fn feet() -> [f32; 3] {
    observe().feet
}

fn feet_cell() -> Cell {
    feet().map(|axis| axis.floor() as i32)
}

fn eyes() -> [f32; 3] {
    let [x, y, z] = feet();
    [x, y + EYE_HEIGHT, z]
}

fn centre(cell: Cell) -> [f32; 3] {
    cell.map(|axis| axis as f32 + 0.5)
}

fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    (0..3).map(|axis| (a[axis] - b[axis]).powi(2)).sum::<f32>().sqrt()
}

/// The nearest point of `cell` to `from`.
fn nearest_point(from: [f32; 3], cell: Cell) -> [f32; 3] {
    [0, 1, 2].map(|axis| from[axis].clamp(cell[axis] as f32, cell[axis] as f32 + 1.0))
}

/// How far from the middle of a block the feet may stop. Views are checked from that far off every way, so a look planned from
/// the middle still sees past the edges of nearby blocks.
const STAND_SLACK: f32 = 0.15;

/// Whether a look from `from`, or from anywhere `STAND_SLACK` round it, at `point` first meets `hit`.
fn clear_view(world: &impl Fn(Cell) -> Option<Block>, from: [f32; 3], point: [f32; 3], hit: (Cell, Cell)) -> bool {
    [
        [0.0, 0.0],
        [STAND_SLACK, 0.0],
        [-STAND_SLACK, 0.0],
        [0.0, STAND_SLACK],
        [0.0, -STAND_SLACK],
    ]
    .iter()
    .all(|[dx, dz]| ray(world, [from[0] + dx, from[1], from[2] + dz], point) == Some(hit))
}

/// How many of a block the inventory holds.
fn count(wanted: Block) -> u32 {
    (0..SLOTS)
        .filter_map(inventory)
        .filter(|&(block, _)| block == wanted)
        .map(|(_, count)| count)
        .sum()
}

/// The angle from `from` to `to`, between -PI and PI.
fn turn(from: f32, to: f32) -> f32 {
    (to - from + PI).rem_euclid(TAU) - PI
}

#[derive(Default)]
struct Builder {
    controls: Controls,
    /// Turns which way it explores next.
    explored: u32,
}

impl Builder {
    /// Hands its controls to the game and waits for the next tick.
    fn tick(&mut self) {
        set_controls(&self.controls);
        wait_ticks(1);
    }

    fn stop(&mut self) {
        self.controls = Controls {
            yaw: self.controls.yaw,
            ..Controls::default()
        };
        self.tick();
    }

    /// Level ground near where it is, walking further afield until it finds some.
    fn find_site(&mut self) -> Site {
        loop {
            let here = feet_cell();
            if let Some(site) = find_site(&block, here, SITE_RADIUS, &reachable(&block, here, SITE_RADIUS + SITE)) {
                return site;
            }
            log("no level ground here, looking further on");
            self.explore();
        }
    }

    /// Walks some way off, a different way each time.
    fn explore(&mut self) {
        let here = feet_cell();
        let angle = self.explored as f32 * 2.4;
        self.explored += 1;
        let toward = [
            here[0] + (angle.cos() * 40.0) as i32,
            here[1],
            here[2] + (angle.sin() * 40.0) as i32,
        ];
        let far = |cell: Cell| (cell[0] - here[0]).pow(2) + (cell[2] - here[2]).pow(2) >= 24 * 24;
        if !self.walk(toward, far, PATH_LIMIT) {
            // Somewhere is better than nowhere: stand still a moment and look again.
            self.stop();
        }
    }

    /// Walks to the nearest place `arrived` accepts, heading towards `toward`. False if there's no way there.
    fn walk(&mut self, toward: Cell, arrived: impl Fn(Cell) -> bool, limit: usize) -> bool {
        for _ in 0..3 {
            let Some(way) = path(&block, feet_cell(), toward, &arrived, limit) else {
                return false;
            };
            if self.follow(&way) {
                return true;
            }
        }
        false
    }

    /// Steps along a way, one cell to the next. False if it stops getting anywhere, which is when to look for a way again.
    fn follow(&mut self, way: &[Cell]) -> bool {
        self.controls.attack = 0;
        self.controls.place = 0;
        self.controls.head_yaw = 0.0;
        self.controls.head_pitch = 0.0;
        for (index, &step) in way.iter().enumerate().skip(1) {
            let last = index == way.len() - 1;
            let mut ticks = 0;
            loop {
                let state = observe();
                let [x, y, z] = state.feet;
                let off = [step[0] as f32 + 0.5 - x, step[2] as f32 + 0.5 - z];
                let across = (off[0] * off[0] + off[1] * off[1]).sqrt();
                if across < if last { STAND_SLACK * 0.7 } else { 0.35 } && (y - step[1] as f32).abs() < 0.5 {
                    break;
                }
                ticks += 1;
                if ticks > 60 {
                    self.stop();
                    return false;
                }
                self.controls.yaw = (-off[0]).atan2(-off[1]);
                // Slows into the last step so it stops on it.
                self.controls.walk = [0.0, if last { (across / WALK_PER_TICK).min(1.0) } else { 1.0 }];
                let climbing = step[1] > y.floor() as i32 || state.blocked[0] != 0 || state.blocked[2] != 0;
                self.controls.jump = if climbing && state.on_ground != 0 { JUMP } else { 0.0 };
                self.tick();
            }
        }
        self.controls.walk = [0.0; 2];
        self.controls.jump = 0.0;
        self.tick();
        true
    }

    /// Turns to look at `point`, with the body facing `body_yaw` if given and the head making up the rest.
    fn aim(&mut self, point: [f32; 3], body_yaw: Option<f32>) {
        let (yaw, pitch) = look(eyes(), point);
        let body = body_yaw.unwrap_or(yaw);
        self.controls.yaw = body;
        self.controls.head_yaw = turn(body, yaw);
        self.controls.head_pitch = pitch;
        self.controls.walk = [0.0; 2];
        self.controls.jump = 0.0;
        set_controls(&self.controls);
    }

    /// Breaks the block at `cell`, from wherever it can be reached, breaking whatever `in_the_way` allows on the way to it. True
    /// once the cell is open.
    fn break_block(&mut self, cell: Cell, in_the_way: impl Fn(Cell, Block) -> bool, limit: usize) -> bool {
        let mut tried = HashSet::new();
        for _ in 0..TRIES {
            if open(block(cell)) {
                return true;
            }
            let world = block;
            let sees = |stand: Cell| {
                let from = eye(stand);
                distance(from, centre(cell)) <= REACH - 0.3
                    && !tried.contains(&stand)
                    && ray(&world, from, centre(cell)).is_some_and(|(hit, _)| hit == cell || world(hit).is_some_and(|b| in_the_way(hit, b)))
            };
            let Some(way) = path(&world, feet_cell(), cell, sees, limit) else {
                return false;
            };
            let stand = *way.last().expect("a way is never empty");
            tried.insert(stand);
            if !self.follow(&way) {
                continue;
            }
            for _ in 0..400 {
                self.aim(centre(cell), None);
                let state = observe();
                let target = state.target;
                let breaking = state.has_target != 0
                    && (target == cell || block(target).is_some_and(|block| in_the_way(target, block)) && !open(block(target)));
                if !breaking {
                    break;
                }
                self.controls.attack = 1;
                self.tick();
                if open(block(cell)) {
                    break;
                }
            }
            self.controls.attack = 0;
            set_controls(&self.controls);
        }
        open(block(cell))
    }

    /// Clears the site of everything above the levelled ground that it can reach.
    fn clear(&mut self, site: &Site) {
        let cells: Vec<Cell> = site.clearing().filter(|&cell| solid(block(cell))).collect();
        let on_site = |cell: Cell, block: Block| site.contains(cell) && cell[1] > site.floor || block == Block::OakLeaves;
        let left = cells
            .into_iter()
            .filter(|&cell| !self.break_block(cell, on_site, PATH_LIMIT))
            .count();
        if left > 0 {
            log(&format!("{left} blocks over the site were out of reach"));
        }
    }

    /// Cuts down trees until it holds `wanted` more logs than now.
    fn gather_logs(&mut self, wanted: u32) {
        let goal = count(Block::OakLog) + wanted;
        let mut skipped = HashSet::new();
        while count(Block::OakLog) < goal {
            let mut found = [[0; 3]; 32];
            let filled = find_nearest(Block::OakLog, &mut found);
            let found = &found[..filled];
            let Some(&log_cell) = found.iter().find(|cell| !skipped.contains(*cell)) else {
                log("no trees in reach, looking further on");
                skipped.clear();
                self.explore();
                continue;
            };
            let tree = |_: Cell, block: Block| matches!(block, Block::OakLog | Block::OakLeaves);
            if !self.break_block(log_cell, tree, GATHER_PATH_LIMIT) {
                skipped.insert(log_cell);
            }
        }
    }

    /// Crafts `amount` of `block`, cutting down trees for logs and going to the crafting table as it needs.
    fn obtain(&mut self, wanted: Block, amount: u32, site: &Site) -> bool {
        loop {
            match want(wanted, amount) {
                Want::Done => return true,
                Want::Missing(Block::OakLog, logs) => self.gather_logs(logs),
                Want::NeedTable => {
                    let table = site.table().cell;
                    let by_table = |stand: Cell| distance(eye(stand), nearest_point(eye(stand), table)) <= REACH - 0.3;
                    if !self.walk(table, by_table, PATH_LIMIT) {
                        log("can't get to the crafting table");
                        return false;
                    }
                }
                Want::Missing(block, count) => {
                    log(&format!(
                        "crafting {wanted:?} needs {count} {block:?}, which can't be crafted or gathered"
                    ));
                    return false;
                }
                Want::NoRoom => {
                    log(&format!("no room to craft {wanted:?}"));
                    return false;
                }
            }
        }
    }

    /// Digs up the ground round the site until it has enough to build its tallest pillar.
    fn gather_fillers(&mut self, site: &Site) {
        let mut skipped = HashSet::new();
        while FILLERS.iter().map(|&filler| count(filler)).sum::<u32>() < PILLAR as u32 {
            let mut found = [[0; 3]; 32];
            let filled = find_nearest(Block::Grass, &mut found);
            let found = &found[..filled];
            let Some(&ground) = found.iter().find(|&&cell| !site.contains(cell) && !skipped.contains(&cell)) else {
                log("no ground to dig for a pillar");
                return;
            };
            if !self.break_block(ground, |_, _| false, GATHER_PATH_LIMIT) {
                skipped.insert(ground);
            }
        }
    }

    /// Puts a block it holds into a hotbar slot, and selects it.
    fn select(&mut self, wanted: impl Fn(Block) -> bool) -> bool {
        let Some(slot) = (0..SLOTS).find(|&slot| inventory(slot).is_some_and(|(block, _)| wanted(block))) else {
            return false;
        };
        let slot = if slot < HOTBAR {
            slot
        } else {
            swap_slots(slot, HOTBAR - 1);
            HOTBAR - 1
        };
        self.controls.slot = slot;
        true
    }

    /// Builds a pillar under itself until its feet are at `height`.
    fn pillar_up(&mut self, height: i32) -> bool {
        if !self.select(|block| FILLERS.contains(&block)) {
            return false;
        }
        self.controls.head_yaw = 0.0;
        self.controls.head_pitch = -FRAC_PI_2;
        self.controls.walk = [0.0; 2];
        self.controls.place = 1;
        for _ in 0..40 * PILLAR {
            let state = observe();
            if state.on_ground != 0 && state.feet[1] >= height as f32 - 0.01 {
                break;
            }
            self.controls.jump = if state.on_ground != 0 { JUMP } else { 0.0 };
            self.tick();
        }
        self.controls.place = 0;
        self.controls.jump = 0.0;
        self.tick();
        feet()[1] >= height as f32 - 0.01
    }

    /// Takes down the pillar under itself until its feet are at `height`. Only pillar blocks are broken.
    fn step_down(&mut self, height: i32) {
        self.controls.head_yaw = 0.0;
        self.controls.head_pitch = -FRAC_PI_2;
        self.controls.walk = [0.0; 2];
        for _ in 0..100 * PILLAR {
            let state = observe();
            let on_ground = state.on_ground != 0;
            if on_ground && state.feet[1] < height as f32 + 0.5 {
                break;
            }
            let under = offset(feet_cell(), [0, -1, 0]);
            if on_ground && !block(under).is_some_and(|block| FILLERS.contains(&block)) {
                break;
            }
            self.controls.attack = 1;
            self.tick();
        }
        self.controls.attack = 0;
        self.tick();
    }

    /// Stands at `stand`, on a pillar if it's above the levelled ground of the site.
    fn go_to(&mut self, stand: Cell, site: &Site) -> bool {
        for _ in 0..4 {
            let here = feet_cell();
            if [here[0], here[2]] == [stand[0], stand[2]] {
                match here[1].cmp(&stand[1]) {
                    std::cmp::Ordering::Equal => return true,
                    std::cmp::Ordering::Less => {
                        if !self.pillar_up(stand[1]) {
                            return false;
                        }
                    }
                    std::cmp::Ordering::Greater => self.step_down(stand[1]),
                }
                continue;
            }
            if site.contains(here) && here[1] > site.floor + 1 {
                self.step_down(site.floor + 1);
            }
            let ground = [stand[0], site.floor + 1, stand[2]];
            if !self.walk(ground, |cell| cell == ground, PATH_LIMIT) {
                return false;
            }
        }
        false
    }

    /// Where to stand, from the margin of the site and on a pillar up to `PILLAR` high, and which side of which block to place
    /// against, to put `piece` in place. The nearest such place wins, the lower the better; `tried` are ruled out.
    fn plan_placing(&self, piece: &Piece, site: &Site, tried: &HashSet<(Cell, Cell)>) -> Option<(Cell, Cell)> {
        let world = block;
        let here = feet_cell();
        let mut stands: Vec<Cell> = site
            .margin()
            .flat_map(|ground| (0..=PILLAR).map(move |height| offset(ground, [0, height, 0])))
            .collect();
        stands.sort_by_key(|stand| {
            let moving = (stand[0] - here[0]).abs() + (stand[2] - here[2]).abs();
            let climbing = (stand[1] - site.floor - 1) * 3;
            if moving == 0 {
                (stand[1] - here[1]).abs()
            } else {
                moving + climbing + 4
            }
        });
        stands.into_iter().find_map(|stand| {
            // The pillar and the body need the column clear, except where it's already standing.
            let column_clear = (site.floor + 1..=stand[1] + 1)
                .all(|y| open(world([stand[0], y, stand[2]])) || [stand[0], stand[2]] == [here[0], here[2]] && y < here[1]);
            if !column_clear || [stand[0], stand[2]] == [piece.cell[0], piece.cell[2]] {
                return None;
            }
            let from = eye(stand);
            SIDES.into_iter().find_map(|side| {
                let support = offset(piece.cell, side.map(|axis| -axis));
                let point = face_middle(support, side);
                let fits = solid(world(support))
                    && !tried.contains(&(stand, side))
                    && distance(from, point) <= REACH - 0.3
                    && clear_view(&world, from, point, (support, side))
                    && piece.yaw.is_none_or(|body| turn(body, look(from, point).0).abs() <= NECK);
                fits.then_some((stand, side))
            })
        })
    }

    /// Places `piece` from the inventory. True once it's there.
    fn place(&mut self, piece: &Piece, site: &Site) -> bool {
        let placed = || block(piece.cell) == Some(piece.block);
        let mut tried = HashSet::new();
        for _ in 0..TRIES {
            if placed() {
                return true;
            }
            let Some((stand, side)) = self.plan_placing(piece, site, &tried) else {
                break;
            };
            tried.insert((stand, side));
            if !self.go_to(stand, site) {
                continue;
            }
            // After going there, which may have meant selecting what to build a pillar from.
            if !self.select(|block| block == piece.block) {
                log(&format!("ran out of {:?}", piece.block));
                return false;
            }
            let support = offset(piece.cell, side.map(|axis| -axis));
            for _ in 0..10 {
                self.aim(face_middle(support, side), piece.yaw);
                let state = observe();
                if state.has_target != 0 && state.target == support && state.face == side {
                    self.controls.place = 1;
                    self.tick();
                    self.controls.place = 0;
                    set_controls(&self.controls);
                    break;
                }
                self.tick();
            }
            if placed() {
                return true;
            }
        }
        log(&format!("couldn't place {:?} at {:?}", piece.block, piece.cell));
        false
    }
}
