//! Sample script: digs the ground in front of it, then builds a pillar under itself from what it dug.

use std::f32::consts::{FRAC_PI_2, PI};

use agent_api::{Controls, HOTBAR, SLOTS, inventory, log, observe, set_controls, wait_ticks};

/// Blocks to dig, and so the pillar's height.
const BLOCKS: u32 = 3;
/// How far below level it looks to dig: at the ground just in front of its feet.
const DIG_PITCH: f32 = -55.0 * PI / 180.0;

#[unsafe(no_mangle)]
pub extern "C" fn run() {
    let mut controls = Controls {
        head_pitch: DIG_PITCH,
        attack: 1,
        ..Controls::default()
    };
    set_controls(&controls);
    while carried() < BLOCKS {
        wait_ticks(1);
    }
    log("dug, building a pillar");
    controls.head_pitch = -FRAC_PI_2;
    controls.attack = 0;
    controls.place = 1;
    while let Some(slot) = (0..HOTBAR).find(|&slot| inventory(slot).is_some()) {
        controls.slot = slot;
        controls.jump = if observe().on_ground != 0 { 1.25 } else { 0.0 };
        set_controls(&controls);
        wait_ticks(1);
    }
    controls.place = 0;
    set_controls(&controls);
    log("pillar built");
}

fn carried() -> u32 {
    (0..SLOTS).filter_map(inventory).map(|(_, count)| count).sum()
}
