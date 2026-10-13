//! Building the whole world before play starts, a slice of work per frame so the window keeps
//! drawing and responding while it happens.

use std::time::{Duration, Instant};

use glam::IVec3;
use sim::world::terrain::{Generation, Stage};
use sim::world::{Extent, World};

use crate::mesher;
use crate::render::Renderer;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    ShapingLand,
    FillingSea,
    BuildingMeshes,
}

impl Phase {
    pub fn name(self) -> &'static str {
        match self {
            Phase::ShapingLand => "Shaping the land",
            Phase::FillingSea => "Filling the sea",
            Phase::BuildingMeshes => "Building meshes",
        }
    }
}

pub struct Loading {
    generation: Generation,
    /// Chunks to mesh, known once generation is done.
    to_mesh: Vec<IVec3>,
    meshed: usize,
    phase: Phase,
    phase_start: Instant,
}

impl Loading {
    pub fn new(extent: Extent, seed: u32) -> Self {
        Self {
            generation: Generation::new(extent, seed),
            to_mesh: Vec::new(),
            meshed: 0,
            phase: Phase::ShapingLand,
            phase_start: Instant::now(),
        }
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// The fraction of the current phase done, in `0..=1`.
    pub fn progress(&self) -> f32 {
        match self.phase {
            Phase::BuildingMeshes => self.meshed as f32 / self.to_mesh.len().max(1) as f32,
            _ => self.generation.progress(),
        }
    }

    /// Builds `world` and its meshes for about `budget`. Returns whether loading is finished.
    /// Prints how long each phase took as it ends.
    pub fn work(&mut self, world: &mut World, renderer: &mut Renderer, budget: Duration) -> bool {
        let start = Instant::now();
        if self.phase == Phase::BuildingMeshes {
            while let Some(&chunk) = self.to_mesh.get(self.meshed) {
                renderer.upload_chunk_mesh(chunk, &mesher::build_chunk_mesh(world, chunk));
                self.meshed += 1;
                if start.elapsed() >= budget {
                    break;
                }
            }
            let finished = self.meshed == self.to_mesh.len();
            if finished {
                self.report_phase();
            }
            return finished;
        }
        self.generation.work(world, budget);
        let phase = match self.generation.stage() {
            Stage::Chunks => Phase::ShapingLand,
            Stage::Sea => Phase::FillingSea,
            Stage::Done => Phase::BuildingMeshes,
        };
        if phase != self.phase {
            self.report_phase();
            self.phase = phase;
            self.phase_start = Instant::now();
        }
        if phase == Phase::BuildingMeshes {
            self.to_mesh = world.chunk_positions().collect();
        }
        false
    }

    fn report_phase(&self) {
        let seconds = self.phase_start.elapsed().as_secs_f32();
        eprintln!("{}: {seconds:.2} s", self.phase.name());
    }
}
