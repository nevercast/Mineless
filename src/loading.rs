//! Building the whole world before play starts, a slice of work per frame so the window keeps
//! drawing and responding while it happens.

use std::time::{Duration, Instant};

use glam::Vec2;
use sim::world::terrain::{Generation, Stage};
use sim::world::{Extent, World};

use crate::meshed_area::MeshedArea;
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
    /// Where the camera will start, so the meshes around it are ready for the first frame.
    camera: Vec2,
    /// Columns to mesh and how many of them are left, known once generation is done.
    to_mesh: usize,
    unmeshed: usize,
    phase: Phase,
    phase_start: Instant,
}

impl Loading {
    pub fn new(extent: Extent, seed: u32, camera: Vec2) -> Self {
        Self {
            generation: Generation::new(extent, seed),
            camera,
            to_mesh: 0,
            unmeshed: 0,
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
            Phase::BuildingMeshes => 1.0 - self.unmeshed as f32 / self.to_mesh.max(1) as f32,
            _ => self.generation.progress(),
        }
    }

    /// Builds `world` and the meshes in view of the start camera for about `budget`. Returns
    /// whether loading is finished. Prints how long each phase took as it ends.
    pub fn work(
        &mut self,
        world: &mut World,
        area: &mut MeshedArea,
        renderer: &mut Renderer,
        budget: Duration,
    ) -> bool {
        if self.phase == Phase::BuildingMeshes {
            self.unmeshed = area.update(world, renderer, self.camera, budget);
            let finished = self.unmeshed == 0;
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
            self.to_mesh = area.in_range(self.camera);
            self.unmeshed = self.to_mesh;
        }
        false
    }

    fn report_phase(&self) {
        let seconds = self.phase_start.elapsed().as_secs_f32();
        eprintln!("{}: {seconds:.2} s", self.phase.name());
    }
}
