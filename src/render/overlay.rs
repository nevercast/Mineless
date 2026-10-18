//! Flat quads drawn straight onto the window, with text in a built-in pixel font: the screen shown while the world generates,
//! and the spectator's list of agents over the world.

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
const TEXT: [f32; 4] = [0.85, 0.88, 0.92, 1.0];
const BAR_TRACK: [f32; 4] = [0.12, 0.15, 0.2, 1.0];
const BAR_FILL: [f32; 4] = [0.35, 0.8, 1.0, 1.0];
const HEADING: &str = "Generating terrain";
/// Behind the list, dark enough to read over a bright sky; and the selected line, in the loading bar's colour.
const PANEL: [f32; 4] = [0.0, 0.0, 0.0, 0.55];
const SELECTED: [f32; 4] = [0.35, 0.8, 1.0, 1.0];

/// A rectangle in pixels from the top left, with its colour.
type Quad = (Vec2, Vec2, [f32; 4]);

/// One line of the agent list.
pub struct ListLine {
    pub text: String,
    pub selected: bool,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct QuadVertex {
    position: [f32; 2],
    color: [f32; 4],
}

pub struct Overlay {
    pipeline: wgpu::RenderPipeline,
}

impl Overlay {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::include_wgsl!("overlay.wgsl"));
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("overlay"),
            bind_group_layouts: &[],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("overlay"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: size_of::<QuadVertex>() as wgpu::BufferAddress,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x4],
                })],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        Self { pipeline }
    }

    /// Draws the loading screen into `view`, sized `size` in pixels, with `progress` in `0..=1` and `detail` under the bar.
    pub fn draw_loading(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        size: Vec2,
        progress: f32,
        detail: &str,
    ) {
        let quads = loading_layout(size, progress, detail);
        self.draw(device, encoder, view, size, &quads, wgpu::LoadOp::Clear(BACKGROUND));
    }

    /// Draws `lines` in a panel at the top left of `view`, over what's already there.
    pub fn draw_list(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        size: Vec2,
        lines: &[ListLine],
    ) {
        let quads = list_layout(size, lines);
        self.draw(device, encoder, view, size, &quads, wgpu::LoadOp::Load);
    }

    fn draw(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        size: Vec2,
        quads: &[Quad],
        load: wgpu::LoadOp<wgpu::Color>,
    ) {
        let vertices: Vec<QuadVertex> = quads
            .iter()
            .flat_map(|&(min, max, color)| {
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
            label: Some("overlay"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("overlay"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load,
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

/// The heading above a bar across the middle of the screen, and `detail` below it.
fn loading_layout(size: Vec2, progress: f32, detail: &str) -> Vec<Quad> {
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
    let centred = |line: &str| (centre.x - text_width(line, unit) / 2.0).round();
    text(
        &mut quads,
        HEADING,
        Vec2::new(centred(HEADING), centre.y - bar_half.y - 12.0 * unit),
        unit,
        TEXT,
    );
    text(
        &mut quads,
        detail,
        Vec2::new(centred(detail), centre.y + bar_half.y + 3.0 * unit),
        unit,
        TEXT,
    );
    quads
}

/// A panel at the top left holding `lines`, the selected one picked out in colour.
fn list_layout(size: Vec2, lines: &[ListLine]) -> Vec<Quad> {
    // Smaller than the loading screen's, so a list of agents leaves the world in view.
    let unit = (size.y / 300.0).floor().max(1.0);
    let line_height = 11.0 * unit;
    let margin = 4.0 * unit;
    let width = lines.iter().map(|line| text_width(&line.text, unit)).fold(0.0, f32::max);
    let corner = Vec2::splat(margin);
    let mut quads = vec![(
        corner,
        corner + Vec2::new(width, lines.len() as f32 * line_height) + 2.0 * margin,
        PANEL,
    )];
    for (row, line) in lines.iter().enumerate() {
        let color = if line.selected { SELECTED } else { TEXT };
        text(
            &mut quads,
            &line.text,
            corner + margin + Vec2::Y * row as f32 * line_height,
            unit,
            color,
        );
    }
    quads
}

fn text_width(line: &str, unit: f32) -> f32 {
    line.chars().count() as f32 * (GLYPH_WIDTH + 1) as f32 * unit
}

/// Adds a quad per lit font pixel of `line`, with its top left at `corner`. Characters without a glyph are left as gaps.
fn text(quads: &mut Vec<Quad>, line: &str, corner: Vec2, unit: f32, color: [f32; 4]) {
    let advance = (GLYPH_WIDTH + 1) as f32 * unit;
    for (index, character) in line.chars().enumerate() {
        let Some(rows) = glyph(character) else {
            continue;
        };
        for (row, bits) in rows.iter().enumerate() {
            for column in 0..GLYPH_WIDTH {
                if bits >> (GLYPH_WIDTH - 1 - column) & 1 == 1 {
                    let min = corner + Vec2::new(index as f32 * advance + column as f32 * unit, row as f32 * unit);
                    quads.push((min, min + unit, color));
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
            let (min, max, _) = loading_layout(size, progress, "")[1];
            max.x - min.x
        };
        let (track_min, track_max, _) = loading_layout(size, 0.0, "")[0];
        assert_eq!(fill_width(0.0), 0.0);
        assert_eq!(fill_width(1.0), track_max.x - track_min.x);
        assert!((fill_width(0.5) - fill_width(1.0) / 2.0).abs() <= 1.0);
    }

    #[test]
    fn the_list_panel_holds_its_widest_line_and_colours_the_selected_one() {
        let size = Vec2::new(800.0, 600.0);
        let lines = [
            ListLine {
                text: "ab".to_owned(),
                selected: false,
            },
            ListLine {
                text: "abcd".to_owned(),
                selected: true,
            },
        ];
        let quads = list_layout(size, &lines);
        let (min, max, _) = quads[0];
        assert_eq!(max.x - min.x, text_width("abcd", 2.0) + 16.0);
        assert_eq!(max.y - min.y, 2.0 * 22.0 + 16.0);
        let in_row = |row: f32| {
            quads[1..]
                .iter()
                .filter(move |(quad, _, _)| (0.0..22.0).contains(&(quad.y - 8.0 - row * 22.0)))
        };
        assert!(in_row(0.0).all(|&(_, _, color)| color == TEXT));
        assert!(in_row(1.0).all(|&(_, _, color)| color == SELECTED));
    }
}
