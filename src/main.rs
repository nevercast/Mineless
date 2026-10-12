mod camera;
mod chunk;
mod mesh;
mod renderer;
mod texture;

use std::sync::Arc;
use std::time::Instant;

use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

use camera::Camera;
use chunk::{Block, Chunk};
use renderer::Renderer;

struct App {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    camera: Camera,
    started: Instant,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attributes = Window::default_attributes().with_title("Mineless");
        let window = Arc::new(event_loop.create_window(attributes).expect("create window"));
        self.renderer = Some(pollster::block_on(Renderer::new(
            window.clone(),
            &mesh::build_chunk_mesh(&scene()),
        )));
        self.window = Some(window);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let (Some(window), Some(renderer)) = (&self.window, &mut self.renderer) else {
            return;
        };
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => renderer.resize(size.width, size.height),
            WindowEvent::RedrawRequested => {
                let angle = self.started.elapsed().as_secs_f32() * 0.3;
                self.camera.position =
                    CENTRE + glam::vec3(angle.cos() * 6.0, 3.0, angle.sin() * 6.0);
                renderer.render(self.camera.view_projection(renderer.aspect_ratio()));
                window.request_redraw();
            }
            _ => {}
        }
    }
}

/// Where the hand-placed blocks sit inside the chunk; the camera orbits this point.
const CENTRE: glam::Vec3 = glam::vec3(8.5, 1.0, 8.5);

/// A small hand-placed arrangement until world generation exists: a 3x3 floor with a short pillar.
fn scene() -> Chunk {
    let mut chunk = Chunk::empty();
    for x in 7..=9 {
        for z in 7..=9 {
            chunk.set(x, 0, z, Block::Solid);
        }
    }
    for y in 1..=2 {
        chunk.set(8, y, 8, Block::Solid);
    }
    chunk
}

fn main() {
    env_logger::init();
    let event_loop = EventLoop::new().expect("create event loop");
    let mut app = App {
        window: None,
        renderer: None,
        camera: Camera::looking_at(CENTRE + glam::vec3(6.0, 3.0, 0.0), CENTRE),
        started: Instant::now(),
    };
    event_loop.run_app(&mut app).expect("run event loop");
}
