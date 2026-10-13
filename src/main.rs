mod avatar;
mod mesher;
mod physics;
mod player;
mod render;
mod spectator;
mod world;

use std::sync::Arc;
use std::time::Instant;

use winit::application::ApplicationHandler;
use winit::event::{DeviceEvent, DeviceId, ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{CursorGrabMode, Window, WindowId};

use avatar::Avatar;
use glam::{IVec3, Vec2, Vec3, vec3};
use player::{JUMP_HEIGHTS, Player, TICK_RATE};
use render::{Camera, Renderer};
use spectator::Spectator;
use world::World;

struct App {
    world: World,
    players: Vec<Player>,
    /// Seconds of simulation time owed but not yet ticked.
    unsimulated: f32,
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    camera: Camera,
    spectator: Spectator,
    mouse_captured: bool,
    last_frame: Instant,
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
            WindowEvent::KeyboardInput { event, .. } => {
                if event.physical_key == PhysicalKey::Code(KeyCode::Escape) {
                    self.mouse_captured = set_mouse_capture(window, false);
                }
                self.spectator.key(&event);
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => self.mouse_captured = set_mouse_capture(window, true),
            WindowEvent::Focused(false) => {
                self.spectator.release_all();
                self.mouse_captured = set_mouse_capture(window, false);
            }
            WindowEvent::RedrawRequested => {
                let now = Instant::now();
                let seconds = (now - self.last_frame).as_secs_f32();
                self.last_frame = now;
                self.spectator.update(&mut self.camera, seconds);
                // Simulate in fixed ticks; a long stall drops time rather than ticking to catch up.
                self.unsimulated = (self.unsimulated + seconds).min(MAX_CATCH_UP);
                while self.unsimulated >= 1.0 / TICK_RATE {
                    self.unsimulated -= 1.0 / TICK_RATE;
                    for player in &mut self.players {
                        wander(player, &self.world);
                    }
                }
                let alpha = self.unsimulated * TICK_RATE;
                let avatars: Vec<Avatar> = self.players.iter().map(|p| p.avatar(alpha)).collect();
                renderer.render(
                    self.camera.view_projection(renderer.aspect_ratio()),
                    &avatars,
                );
                window.request_redraw();
            }
            _ => {}
        }
    }

    fn device_event(&mut self, _event_loop: &ActiveEventLoop, _id: DeviceId, event: DeviceEvent) {
        // Raw mouse motion keeps turning even when the locked cursor can't move.
        if let DeviceEvent::MouseMotion { delta: (dx, dy) } = event
            && self.mouse_captured
        {
            self.spectator.look(&mut self.camera, dx, dy);
        }
    }
}

/// Click to capture the mouse for looking around, Escape to give it back. Returns whether the
/// mouse is now captured.
fn set_mouse_capture(window: &Window, capture: bool) -> bool {
    if !capture {
        let _ = window.set_cursor_grab(CursorGrabMode::None);
        window.set_cursor_visible(true);
        return false;
    }
    // Locked is the natural mode but Windows only supports Confined, and some platforms only Locked.
    let grabbed = window
        .set_cursor_grab(CursorGrabMode::Locked)
        .or_else(|_| window.set_cursor_grab(CursorGrabMode::Confined))
        .is_ok();
    window.set_cursor_visible(!grabbed);
    grabbed
}

/// 4x2x4 chunks: 64 blocks square and 32 tall, with walls tall enough to reach the upper chunks.
const WORLD_CHUNKS: IVec3 = IVec3::new(4, 2, 4);
const GROUND_HEIGHT: i32 = 8;
const WALL_HEIGHT: i32 = 20;

/// The middle of the world at ground level; the camera starts out looking at it.
const CENTRE: Vec3 = vec3(32.0, GROUND_HEIGHT as f32, 32.0);

const MAX_CATCH_UP: f32 = 0.25;

/// Two players near the centre: one dropped from twelve blocks up to hop in place looking up and
/// to its left, one walking into the centre pit and jumping out, looking down and to its right.
fn test_players() -> Vec<Player> {
    let mut watcher = Player::new(
        vec3(37.5, GROUND_HEIGHT as f32 + 12.0, 27.5),
        -135f32.to_radians(),
    );
    watcher.controls.head_yaw = 0.6;
    watcher.controls.head_pitch = 0.5;
    let mut walker = Player::new(vec3(39.5, GROUND_HEIGHT as f32, 25.5), 123f32.to_radians());
    walker.controls.head_yaw = -0.35;
    walker.controls.head_pitch = -0.3;
    walker.controls.walk = Vec2::Y;
    vec![watcher, walker]
}

/// Stand-in agent: drives a player only through its controls, as a real agent will. Walkers walk
/// forward, jump at anything in the way, and turn away if it's still blocking on the way back
/// down. Everyone else hops on the spot as low as a jump goes.
fn wander(player: &mut Player, world: &World) {
    let blocked = player.tick(world);
    if player.controls.walk == Vec2::ZERO {
        player.controls.jump = Some(*JUMP_HEIGHTS.start());
    } else if blocked.x || blocked.z {
        if player.is_on_ground() {
            player.controls.jump = Some(*JUMP_HEIGHTS.end());
        } else if player.vertical_velocity() < 0.0 {
            player.controls.yaw += 100f32.to_radians();
        }
    }
}

fn main() {
    env_logger::init();
    let event_loop = EventLoop::new().expect("create event loop");
    let mut app = App {
        world: world::arena::generate(WORLD_CHUNKS, GROUND_HEIGHT, WALL_HEIGHT),
        players: test_players(),
        unsimulated: 0.0,
        window: None,
        renderer: None,
        camera: Camera::looking_at(CENTRE + vec3(40.0, 25.0, 40.0), CENTRE),
        spectator: Spectator::default(),
        mouse_captured: false,
        last_frame: Instant::now(),
    };
    event_loop.run_app(&mut app).expect("run event loop");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wanderer_stays_inside_the_arena_walls() {
        let world = world::arena::generate(WORLD_CHUNKS, GROUND_HEIGHT, WALL_HEIGHT);
        let mut players = test_players();
        let inside = (WORLD_CHUNKS * world::CHUNK_SIZE as i32).as_vec3() - 1.0;
        // An hour of simulation.
        for _ in 0..(3600.0 * TICK_RATE) as u32 {
            for player in &mut players {
                wander(player, &world);
                let position = player.avatar(1.0).position;
                assert!(position.x > 1.0 && position.z > 1.0, "{position}");
                assert!(position.x < inside.x && position.z < inside.z, "{position}");
            }
        }
    }
}
