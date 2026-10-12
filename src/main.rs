mod mesher;
mod render;
mod world;

use std::sync::Arc;
use std::time::Instant;

use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

use glam::{IVec3, Vec3, vec3};
use render::{Camera, Renderer};
use world::World;

struct App {
    world: World,
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
        let mut renderer = pollster::block_on(Renderer::new(window.clone()));
        for chunk in self.world.chunk_positions() {
            renderer.upload_chunk_mesh(chunk, &mesher::build_chunk_mesh(&self.world, chunk));
        }
        self.renderer = Some(renderer);
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
                self.camera.position = CENTRE + vec3(angle.cos() * 80.0, 50.0, angle.sin() * 80.0);
                renderer.render(self.camera.view_projection(renderer.aspect_ratio()));
                window.request_redraw();
            }
            _ => {}
        }
    }
}

/// 4x2x4 chunks: 64 blocks square and 32 tall, with walls tall enough to reach the upper chunks.
const WORLD_CHUNKS: IVec3 = IVec3::new(4, 2, 4);
const GROUND_HEIGHT: i32 = 6;
const WALL_HEIGHT: i32 = 20;

/// The middle of the world at ground level; the camera orbits this point.
const CENTRE: Vec3 = vec3(32.0, GROUND_HEIGHT as f32, 32.0);

fn main() {
    env_logger::init();
    let event_loop = EventLoop::new().expect("create event loop");
    let mut app = App {
        world: World::flat_with_walls(WORLD_CHUNKS, GROUND_HEIGHT, WALL_HEIGHT),
        window: None,
        renderer: None,
        camera: Camera::looking_at(CENTRE + vec3(80.0, 50.0, 0.0), CENTRE),
        started: Instant::now(),
    };
    event_loop.run_app(&mut app).expect("run event loop");
}
