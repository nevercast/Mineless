//! The screen shown while the world generates: a heading, a progress bar and a line saying what
//! is happening, all flat quads drawn straight onto the window with a built-in pixel font.

use bytemuck::{Pod, Zeroable};
use glam::Vec2;
use wgpu::util::DeviceExt;

use super::font::{GLYPH_WIDTH, glyph};

const BACKGROUND: wgpu::Color = wgpu::Color {
    r: 0.02,
    g: 0.03,
    b: 0.06,
    a: 1.0,
};
const TEXT: [f32; 3] = [0.85, 0.88, 0.92];
const BAR_TRACK: [f32; 3] = [0.12, 0.15, 0.2];
const BAR_FILL: [f32; 3] = [0.35, 0.8, 1.0];
const HEADING: &str = "Generating terrain";

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct QuadVertex {
    position: [f32; 2],
    color: [f32; 3],
}

pub struct LoadingScreen {
    pipeline: wgpu::RenderPipeline,
}

impl LoadingScreen {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::include_wgsl!("loading.wgsl"));
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("loading"),
            bind_group_layouts: &[],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("loading"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: size_of::<QuadVertex>() as wgpu::BufferAddress,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x3],
                })],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(format.into())],
            }),
            multiview_mask: None,
            cache: None,
        });
        Self { pipeline }
    }

    /// Draws the screen into `view`, sized `size` in pixels, with `progress` in `0..=1` and
    /// `detail` under the bar.
    pub fn draw(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        size: Vec2,
        progress: f32,
        detail: &str,
    ) {
        let vertices: Vec<QuadVertex> = layout(size, progress, detail)
            .into_iter()
            .flat_map(|(min, max, color)| {
                // Pixels from the top left to clip space.
                let clip = |x: f32, y: f32| [x / size.x * 2.0 - 1.0, 1.0 - y / size.y * 2.0];
                [
                    clip(min.x, min.y),
                    clip(min.x, max.y),
                    clip(max.x, max.y),
                    clip(min.x, min.y),
                    clip(max.x, max.y),
                    clip(max.x, min.y),
                ]
                .map(|position| QuadVertex { position, color })
            })
            .collect();
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("loading"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("loading"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(BACKGROUND),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_vertex_buffer(0, buffer.slice(..));
        pass.draw(0..vertices.len() as u32, 0..1);
    }
}

/// Rectangles in pixels from the top left, with their colours: the heading above a bar across
/// the middle of the screen, and `detail` below it.
fn layout(size: Vec2, progress: f32, detail: &str) -> Vec<(Vec2, Vec2, [f32; 3])> {
    // One font pixel, scaled with the window but kept whole so glyphs stay crisp.
    let unit = (size.y / 120.0).floor().max(2.0);
    let centre = size / 2.0;
    let bar_half = Vec2::new((size.x * 0.3).round(), unit * 1.5);
    let filled = (bar_half.x * 2.0 * progress.clamp(0.0, 1.0)).round();
    let mut quads = vec![
        (centre - bar_half, centre + bar_half, BAR_TRACK),
        (
            centre - bar_half,
            Vec2::new(centre.x - bar_half.x + filled, centre.y + bar_half.y),
            BAR_FILL,
        ),
    ];
    text(&mut quads, HEADING, centre.y - bar_half.y - 12.0 * unit, unit, centre.x);
    text(&mut quads, detail, centre.y + bar_half.y + 3.0 * unit, unit, centre.x);
    quads
}

/// Adds a quad per lit font pixel of `line`, centred on `centre_x` with its top at `top`.
/// Characters without a glyph are left as gaps.
fn text(quads: &mut Vec<(Vec2, Vec2, [f32; 3])>, line: &str, top: f32, unit: f32, centre_x: f32) {
    let advance = (GLYPH_WIDTH + 1) as f32 * unit;
    let left = (centre_x - line.chars().count() as f32 * advance / 2.0).round();
    for (index, character) in line.chars().enumerate() {
        let Some(rows) = glyph(character) else {
            continue;
        };
        for (row, bits) in rows.iter().enumerate() {
            for column in 0..GLYPH_WIDTH {
                if bits >> (GLYPH_WIDTH - 1 - column) & 1 == 1 {
                    let min = Vec2::new(left + index as f32 * advance + column as f32 * unit, top + row as f32 * unit);
                    quads.push((min, min + unit, TEXT));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loading::Phase;

    #[test]
    fn every_character_on_the_screen_has_a_glyph() {
        let phases = [Phase::ShapingLand, Phase::BuildingMeshes];
        let lines = phases.map(|phase| phase.name());
        for line in lines.into_iter().chain([HEADING, "0123456789%"]) {
            for character in line.chars() {
                assert!(glyph(character).is_some(), "{character:?} in {line:?}");
            }
        }
    }

    #[test]
    fn the_bar_fills_with_progress() {
        let size = Vec2::new(1280.0, 720.0);
        let fill_width = |progress| {
            let (min, max, _) = layout(size, progress, "")[1];
            max.x - min.x
        };
        let (track_min, track_max, _) = layout(size, 0.0, "")[0];
        assert_eq!(fill_width(0.0), 0.0);
        assert_eq!(fill_width(1.0), track_max.x - track_min.x);
        assert!((fill_width(0.5) - fill_width(1.0) / 2.0).abs() <= 1.0);
    }
}
