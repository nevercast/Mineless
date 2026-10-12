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
        self.renderer = Some(pollster::block_on(Renderer::new(window.clone())));
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
                let spin = self.started.elapsed().as_secs_f32() * 0.8;
                let model = glam::Mat4::from_rotation_y(spin);
                renderer.render(self.camera.view_projection(renderer.aspect_ratio()) * model);
                window.request_redraw();
            }
            _ => {}
        }
    }
}

fn main() {
    env_logger::init();
    let event_loop = EventLoop::new().expect("create event loop");
    let mut app = App {
        window: None,
        renderer: None,
        camera: Camera::looking_at(glam::vec3(2.0, 1.5, 2.5), glam::Vec3::ZERO),
        started: Instant::now(),
    };
    event_loop.run_app(&mut app).expect("run event loop");
}
