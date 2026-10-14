//! Which chunks have meshes on the GPU. The whole world stays loaded in the sim, but only chunk
//! columns within the view distance of the camera are meshed and drawn.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use glam::{IVec2, IVec3, Vec2, Vec3Swizzles};
use sim::world::{CHUNK_SIZE, Extent, World};

use crate::mesher::{self, ChunkMesh};
use crate::render::Renderer;

/// Horizontal distance from the camera to the nearest point of a chunk column for it to be
/// drawn. Fog thickens to full strength by here.
pub const VIEW_DISTANCE: f32 = 384.0;
/// Meshes are kept until their column is this much further out than the view distance, so
/// moving back and forth at the edge doesn't rebuild them over and over.
const KEEP_MARGIN: f32 = 2.0 * CHUNK_SIZE as f32;

pub struct MeshedArea {
    columns: Vec<IVec2>,
    /// Chunk y coordinates of every column, bottom to top.
    levels: std::ops::RangeInclusive<i32>,
    meshed: HashSet<IVec2>,
}

impl MeshedArea {
    pub fn new(extent: Extent) -> Self {
        let chunks: Vec<IVec3> = extent.chunk_positions().collect();
        let lowest = chunks.iter().map(|chunk| chunk.y).min().unwrap_or(0);
        let highest = chunks.iter().map(|chunk| chunk.y).max().unwrap_or(0);
        Self {
            columns: chunks
                .iter()
                .filter(|chunk| chunk.y == lowest)
                .map(|chunk| chunk.xz())
                .collect(),
            levels: lowest..=highest,
            meshed: HashSet::new(),
        }
    }

    /// Drops meshes that have fallen out of range of `camera`, then builds missing ones in range,
    /// nearest first, until `budget` has passed. Returns how many columns in range are still
    /// missing.
    pub fn update(
        &mut self,
        world: &World,
        renderer: &mut Renderer,
        camera: Vec2,
        budget: Duration,
    ) -> usize {
        let start = Instant::now();
        for column in self.drop_far(camera) {
            for chunk in self.chunks(column) {
                renderer.upload_chunk_mesh(chunk, &ChunkMesh::default());
            }
        }
        let missing = self.missing(camera);
        for (built, &column) in missing.iter().enumerate() {
            if built > 0 && start.elapsed() >= budget {
                return missing.len() - built;
            }
            for chunk in self.chunks(column) {
                renderer.upload_chunk_mesh(chunk, &mesher::build_chunk_mesh(world, chunk));
            }
            self.meshed.insert(column);
        }
        0
    }

    /// How many columns are in range of `camera`, meshed or not.
    pub fn in_range(&self, camera: Vec2) -> usize {
        let near = |&&column: &&IVec2| distance(camera, column) <= VIEW_DISTANCE;
        self.columns.iter().filter(near).count()
    }

    fn chunks(&self, column: IVec2) -> impl Iterator<Item = IVec3> + use<> {
        self.levels
            .clone()
            .map(move |y| IVec3::new(column.x, y, column.y))
    }

    /// Forgets and returns the meshed columns now too far from `camera` to keep.
    fn drop_far(&mut self, camera: Vec2) -> Vec<IVec2> {
        let far: Vec<IVec2> = self
            .meshed
            .iter()
            .copied()
            .filter(|&column| distance(camera, column) > VIEW_DISTANCE + KEEP_MARGIN)
            .collect();
        for column in &far {
            self.meshed.remove(column);
        }
        far
    }

    /// Columns in range of `camera` without meshes, nearest first.
    fn missing(&self, camera: Vec2) -> Vec<IVec2> {
        let mut missing: Vec<(f32, IVec2)> = self
            .columns
            .iter()
            .filter(|column| !self.meshed.contains(column))
            .map(|&column| (distance(camera, column), column))
            .filter(|&(distance, _)| distance <= VIEW_DISTANCE)
            .collect();
        missing.sort_by(|a, b| a.0.total_cmp(&b.0));
        missing.into_iter().map(|(_, column)| column).collect()
    }
}

/// Horizontal distance from `point` to the nearest point of a chunk column.
fn distance(point: Vec2, column: IVec2) -> f32 {
    let min = (column * CHUNK_SIZE as i32).as_vec2();
    point.clamp(min, min + CHUNK_SIZE as f32).distance(point)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    const EXTENT: Extent = Extent {
        half_width: 1000,
        height: 160,
    };

    #[test]
    fn exactly_the_columns_within_the_view_distance_are_wanted() {
        let area = MeshedArea::new(EXTENT);
        let camera = Vec2::new(10.5, -300.0);
        let missing: HashSet<IVec2> = area.missing(camera).into_iter().collect();
        for &column in &area.columns {
            let corner = (column * CHUNK_SIZE as i32).as_vec2();
            let nearest = camera.clamp(corner, corner + CHUNK_SIZE as f32);
            let wanted = nearest.distance(camera) <= VIEW_DISTANCE;
            assert_eq!(missing.contains(&column), wanted, "{column}");
        }
        // The camera's own column, one just inside the distance and one just beyond it.
        let reach = VIEW_DISTANCE as i32 / CHUNK_SIZE as i32;
        assert!(missing.contains(&IVec2::new(0, -19)));
        assert!(missing.contains(&IVec2::new(reach, -19)));
        assert!(!missing.contains(&IVec2::new(reach + 2, -19)));
        assert!(area.missing(camera).first() == Some(&IVec2::new(0, -19)));
    }

    #[test]
    fn a_column_is_meshed_once_while_it_stays_in_range() {
        let mut area = MeshedArea::new(EXTENT);
        let mut meshed = HashMap::new();
        let frame = |area: &mut MeshedArea, meshed: &mut HashMap<IVec2, u32>, camera: Vec2| {
            area.drop_far(camera);
            for column in area.missing(camera) {
                *meshed.entry(column).or_default() += 1;
                area.meshed.insert(column);
            }
        };
        // Back and forth by less than the margin, many times.
        for step in 0..200 {
            let x = (step % 8) as f32 * 4.0;
            frame(&mut area, &mut meshed, Vec2::new(x, 0.0));
        }
        assert!(meshed.values().all(|&count| count == 1));
        let edge = IVec2::new(VIEW_DISTANCE as i32 / CHUNK_SIZE as i32, 0);
        assert_eq!(meshed.get(&edge), Some(&1));

        // Far enough away that it is dropped, then back: meshed a second time, and only then.
        frame(&mut area, &mut meshed, Vec2::new(-200.0, 0.0));
        assert!(!area.meshed.contains(&edge));
        for _ in 0..10 {
            frame(&mut area, &mut meshed, Vec2::ZERO);
        }
        assert_eq!(meshed.get(&edge), Some(&2));
    }
}
