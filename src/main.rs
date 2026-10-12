mod camera;
mod cube;
mod renderer;
mod texture;

use std::sync::Arc;
use std::time::Instant;

use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

use camera::Camera;
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
        self.renderer = Some(pollster::block_on(Renderer::new(window.clone(), &scene())));
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
                self.camera.position = glam::vec3(angle.cos() * 6.0, 3.0, angle.sin() * 6.0);
                renderer.render(self.camera.view_projection(renderer.aspect_ratio()));
                window.request_redraw();
            }
            _ => {}
        }
    }
}

/// A small hand-placed arrangement until chunks exist: a 3x3 floor with a short pillar.
fn scene() -> Vec<cube::Instance> {
    let floor = (-1..=1).flat_map(|x| (-1..=1).map(move |z| [x as f32, 0.0, z as f32]));
    let pillar = (1..=2).map(|y| [0.0, y as f32, 0.0]);
    floor
        .chain(pillar)
        .map(|position| cube::Instance { position })
        .collect()
}

fn main() {
    env_logger::init();
    let event_loop = EventLoop::new().expect("create event loop");
    let mut app = App {
        window: None,
        renderer: None,
        camera: Camera::looking_at(glam::vec3(6.0, 3.0, 0.0), glam::vec3(0.0, 0.5, 0.0)),
        started: Instant::now(),
    };
    event_loop.run_app(&mut app).expect("run event loop");
}
