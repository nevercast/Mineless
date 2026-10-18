mod agents;
mod avatar;
mod loading;
mod meshed_area;
mod mesher;
mod render;
mod sky;
mod spectator;
mod visibility;

use std::f32::consts::{FRAC_PI_2, PI, TAU};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use winit::application::ApplicationHandler;
use winit::event::{DeviceEvent, DeviceId, ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{CursorGrabMode, Window, WindowId};

use agent::Agent;
use glam::{IVec2, IVec3, Vec3, Vec3Swizzles, vec3};
use loading::Loading;
use meshed_area::MeshedArea;
use render::{Camera, Frustum, Renderer};
use sim::avatar::{Avatar, Skin};
use sim::game::Game;
use sim::player::{EYE_HEIGHT, JUMP_HEIGHTS, Player, TICK_RATE};
use sim::world::{Block, CHUNK_SIZE, ChunkSet, Extent, World};
use sky::{DAY_TICKS, Sky};
use spectator::Spectator;

struct App {
    /// Players are empty until the world is loaded, since they stand on it.
    game: Game,
    /// The agents from the agents file, which drive the players after the test players, each with its player's skin.
    agents: Vec<(Agent, Skin)>,
    seed: u32,
    /// The world still being built, until play starts.
    loading: Option<Loading>,
    meshed: MeshedArea,
    /// When the loading screen was last drawn. Drawing waits for vsync, so the screen is only
    /// redrawn now and then and the frames in between go to loading.
    loading_drawn: Option<Instant>,
    /// When the program started, for reporting how long loading took.
    launched: Instant,
    /// Simulation ticks since the world began.
    ticks: u64,
    /// Seconds of simulation time owed but not yet ticked.
    unsimulated: f32,
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    camera: Camera,
    spectator: Spectator,
    mouse_captured: bool,
    /// Whether chunks walled off from the camera by terrain are left undrawn. `CULLING_KEY` turns
    /// it off and on, to compare.
    culling: bool,
    /// Whether only the walls of caves are drawn, glowing through the rock. `XRAY_KEY` turns it on and off.
    xray: bool,
    last_frame: Instant,
    timings: Timings,
}

/// Frame rate, draw time and tick time over about a second, then shown in the window title so
/// performance changes have numbers to compare. Draw time leaves out waiting for vsync.
#[derive(Default)]
struct Timings {
    frames: u32,
    frame_seconds: f32,
    draws: u32,
    draw_seconds: f32,
    worst_draw_seconds: f32,
    ticks: u32,
    tick_seconds: f32,
    chunks_drawn: usize,
    /// Time spent walking out from the camera to find the chunks it might see, if culling is on.
    walk_seconds: Option<f32>,
}

impl Timings {
    /// The seed is in the title so a screenshot records which world it shows.
    fn title(&self, seed: u32) -> String {
        let ms = |seconds: f32, count: u32| seconds * 1000.0 / count.max(1) as f32;
        format!(
            "Mineless | seed {seed} | {:.0} fps | draw {:.1} ms, worst {:.1} ms | tick {:.2} ms | {} chunks, {}",
            self.frames as f32 / self.frame_seconds,
            ms(self.draw_seconds, self.draws),
            ms(self.worst_draw_seconds, 1),
            ms(self.tick_seconds, self.ticks),
            self.chunks_drawn / self.draws.max(1) as usize,
            self.walk_seconds
                .map_or("culling off".to_string(), |walk| format!("walk {:.2} ms", ms(walk, self.frames))),
        )
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attributes = Window::default_attributes().with_title(format!("Mineless | seed {}", self.seed));
        let window = Arc::new(event_loop.create_window(attributes).expect("create window"));
        let renderer = pollster::block_on(Renderer::new(window.clone(), WORLD));
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
                let pressed = |key| event.physical_key == PhysicalKey::Code(key) && event.state == ElementState::Pressed && !event.repeat;
                self.culling ^= pressed(CULLING_KEY);
                self.xray ^= pressed(XRAY_KEY);
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
            WindowEvent::RedrawRequested if self.loading.is_some() => {
                let loading = self.loading.as_mut().expect("checked by the match guard");
                if loading.work(&mut self.game.world, &mut self.meshed, renderer, LOADING_SLICE) {
                    self.loading = None;
                    eprintln!("Playable after {:.2} s", self.launched.elapsed().as_secs_f32());
                    self.game.players = test_players(&self.game.world);
                    let skins: Vec<Skin> = self.agents.iter().map(|&(_, skin)| skin).collect();
                    self.game.players.extend(agent_players(&self.game.world, &skins));
                    let ground = self.game.world.column_top(START_POSITION.x as i32, START_POSITION.z as i32);
                    self.camera = start_camera(ground);
                    self.last_frame = Instant::now();
                    window.set_title(&format!("Mineless | seed {}", self.seed));
                } else if self.loading_drawn.is_none_or(|drawn| drawn.elapsed() >= LOADING_SCREEN_INTERVAL) {
                    self.loading_drawn = Some(Instant::now());
                    let percent = (loading.progress() * 100.0).floor();
                    let detail = format!("{} {percent:.0}%", loading.phase().name());
                    renderer.render_loading(loading.progress(), &detail);
                    window.set_title(&format!("Mineless | seed {} | {detail}", self.seed));
                }
                window.request_redraw();
            }
            WindowEvent::RedrawRequested => {
                let now = Instant::now();
                let seconds = (now - self.last_frame).as_secs_f32();
                self.last_frame = now;
                self.spectator.update(&mut self.camera, seconds);
                let camera = self.camera.position.xz();
                self.meshed.update(&mut self.game.world, renderer, camera, MESHING_SLICE);
                // Simulate in fixed ticks; a long stall drops time rather than ticking to catch up.
                self.unsimulated = (self.unsimulated + seconds).min(MAX_CATCH_UP);
                while self.unsimulated >= 1.0 / TICK_RATE {
                    self.unsimulated -= 1.0 / TICK_RATE;
                    self.ticks += 1;
                    let tick_start = Instant::now();
                    tick(&mut self.game, &mut self.agents, self.camera.position);
                    self.timings.ticks += 1;
                    self.timings.tick_seconds += tick_start.elapsed().as_secs_f32();
                }
                let alpha = self.unsimulated * TICK_RATE;
                let avatars: Vec<Avatar> = self.game.players.iter().map(|p| p.avatar(alpha)).collect();
                renderer.outline(&targeted_blocks(&self.game));
                let day_fraction = ((self.ticks % DAY_TICKS) as f32 + alpha) / DAY_TICKS as f32;
                // From inside rock the back faces of the rock are culled, so it is see-through and every cave around shows. The
                // walk starts at the camera's chunk as open space and would stop at the solid chunks round it, so it waits until
                // the camera is back in the open. The x-ray view shows caves however walled off they are.
                let in_rock = self.game.world.block(self.camera.position.floor().as_ivec3()).is_solid();
                let culling = self.culling && !in_rock && !self.xray;
                let walk_start = Instant::now();
                let seen = culling.then(|| seen_chunks(&self.camera, &renderer.frustum(&self.camera), &self.meshed));
                let walk_seconds = walk_start.elapsed().as_secs_f32();
                let drawn = renderer.render(
                    &self.camera,
                    &Sky::at(day_fraction),
                    day_fraction * DAY_TICKS as f32 / TICK_RATE,
                    &avatars,
                    seen.as_ref(),
                    self.xray,
                );
                let timings = &mut self.timings;
                timings.frames += 1;
                timings.frame_seconds += seconds;
                if culling {
                    *timings.walk_seconds.get_or_insert(0.0) += walk_seconds;
                }
                if let Some(drawn) = drawn {
                    let draw = drawn.time.as_secs_f32();
                    timings.draws += 1;
                    timings.draw_seconds += draw;
                    timings.worst_draw_seconds = timings.worst_draw_seconds.max(draw);
                    timings.chunks_drawn += drawn.chunks;
                }
                if timings.frame_seconds >= 1.0 {
                    window.set_title(&timings.title(self.seed));
                    *timings = Timings::default();
                }
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

/// The chunks `camera` might see past the terrain, out of those in `frustum` and the view distance.
fn seen_chunks(camera: &Camera, frustum: &Frustum, meshed: &MeshedArea) -> ChunkSet {
    let camera_chunk = camera.position.floor().as_ivec3().div_euclid(IVec3::splat(CHUNK_SIZE as i32));
    // Space outside the world counts as open air, so a camera above it or past the border walks in
    // from there; the walk just mustn't wander off into that air.
    let levels = WORLD.chunk_levels();
    let (lowest, highest) = ((*levels.start()).min(camera_chunk.y), (*levels.end()).max(camera_chunk.y));
    let in_view = |chunk: IVec3| {
        (lowest..=highest).contains(&chunk.y) && meshed_area::in_view_distance(camera.position.xz(), chunk) && frustum.contains_chunk(chunk)
    };
    visibility::visible_chunks(camera_chunk, |chunk| meshed.connections(chunk), in_view)
}

/// The blocks players are aiming at.
fn targeted_blocks(game: &Game) -> Vec<IVec3> {
    game.players
        .iter()
        .filter_map(|player| Some(player.target(&game.world)?.block))
        .collect()
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

/// 2016 blocks square, from -1008 to 1008 so the border lies on chunk edges, generated whole on
/// first load, and 160 tall, which leaves room to build above the highest mountains.
const WORLD: Extent = Extent {
    half_width: 1008,
    height: 160,
};
/// The world unless `--seed` picks another. This one shows sea, beach, uplands, forest and a
/// floating lump from the start view.
const SEED: u32 = 142;
const USAGE: &str = "usage: mineless [--seed <u32>] [--agents <file.toml>]";
/// Read for the agents if `--agents` doesn't name another file, and fine to be missing.
const AGENTS_FILE: &str = "agents.toml";

#[derive(PartialEq, Debug)]
struct Options {
    seed: u32,
    /// The agents file, if one was named.
    agents: Option<PathBuf>,
}

/// The options from the command-line arguments, not counting the program name.
fn parse_args(mut args: impl Iterator<Item = String>) -> Result<Options, String> {
    let mut options = Options { seed: SEED, agents: None };
    while let Some(arg) = args.next() {
        let value = match arg.as_str() {
            "--seed" | "--agents" => args.next().ok_or(format!("{arg} needs a value"))?,
            _ => return Err(format!("unknown argument {arg:?}")),
        };
        if arg == "--seed" {
            options.seed = value
                .parse()
                .map_err(|_| format!("--seed {value:?} is not a whole number from 0 to {}", u32::MAX))?;
        } else {
            options.agents = Some(value.into());
        }
    }
    Ok(options)
}

/// The start view looks over the forest past a snowy upland and an arch to the sea, framed for
/// seed 142.
const START_POSITION: Vec3 = vec3(95.0, 74.0, 65.0);
const START_TARGET: Vec3 = vec3(-30.0, 56.0, -75.0);
/// Blocks the start camera keeps above the ground beneath it.
const START_CLEARANCE: f32 = 4.0;

/// The start view for a world whose ground under `START_POSITION` tops out at `ground`. Other
/// seeds keep its position and direction but rise clear of whatever ground is below.
fn start_camera(ground: i32) -> Camera {
    let lift = Vec3::Y * (ground as f32 + START_CLEARANCE - START_POSITION.y).max(0.0);
    Camera::looking_at(START_POSITION + lift, START_TARGET + lift)
}

/// The top of the ground at the centre of a column, where a player can stand.
fn ground_at(world: &World, x: i32, z: i32) -> Vec3 {
    vec3(x as f32 + 0.5, world.column_top(x, z) as f32, z as f32 + 0.5)
}

const MAX_CATCH_UP: f32 = 0.25;
/// Work done per frame while loading: enough to load quickly, little enough that the window
/// stays responsive.
const LOADING_SLICE: Duration = Duration::from_millis(12);
const LOADING_SCREEN_INTERVAL: Duration = Duration::from_millis(100);
/// Turns culling of chunks walled off from the camera off and on, to compare. V for visibility.
const CULLING_KEY: KeyCode = KeyCode::KeyV;
/// Turns the x-ray view of the caves on and off.
const XRAY_KEY: KeyCode = KeyCode::KeyX;
/// Meshing done per frame as the camera brings chunks into range: far chunks may appear a
/// little late, but the frame rate doesn't hitch.
const MESHING_SLICE: Duration = Duration::from_millis(4);

/// The world starts mid-morning.
const START_TICKS: u64 = DAY_TICKS * 3 / 8;

/// Blocks between neighbouring players in the starting grid, and players along each side of it.
const PLAYER_SPACING: i32 = 64;
const PLAYERS_ACROSS: i32 = 5;
const TEST_PLAYERS: usize = (PLAYERS_ACROSS * PLAYERS_ACROSS) as usize;
/// Blocks from the start camera, toward where it looks, that the agents gather, and how far round that spot they stand.
const AGENTS_DISTANCE: f32 = 6.0;
const AGENTS_RADIUS: f32 = 3.0;

/// A grid of players round the origin, skins in turn, so ticks have a crowd to simulate. The first builds a pillar under itself up
/// out of the sea, the second drops from twelve blocks up, the fifth digs at the ground in front of it, and the rest stand
/// still.
fn test_players(world: &World) -> Vec<Player> {
    (0..PLAYERS_ACROSS)
        .flat_map(|x| (0..PLAYERS_ACROSS).map(move |z| IVec2::new(x, z)))
        .enumerate()
        .map(|(index, cell)| {
            let column = (cell - PLAYERS_ACROSS / 2) * PLAYER_SPACING;
            let mut position = ground_at(world, column.x, column.y);
            if index == 1 {
                position.y += 12.0;
            }
            let yaw = (index as f32 * 123.0).to_radians();
            let mut player = Player::new(position, yaw, Skin::ALL[index % Skin::ALL.len()]);
            if index == 0 {
                player.inventory.add(Block::Cobblestone, PILLAR_HEIGHT);
                player.controls.place = true;
                player.controls.head_pitch = -FRAC_PI_2;
            } else if index == 4 {
                player.controls.attack = true;
                player.controls.head_pitch = DIG_PITCH;
            }
            player
        })
        .collect()
}

/// How far below level a digger looks: at the ground just in front of its feet.
const DIG_PITCH: f32 = -55f32.to_radians();
/// Blocks a builder starts with, and so how tall its pillar grows: enough to rise out of the sea
/// from where the first test player stands.
const PILLAR_HEIGHT: u8 = 24;

/// Advances the game a tick: the agents take their turns, everyone moves, and the stand-in agent decides what the test players
/// do next.
fn tick(game: &mut Game, agents: &mut [(Agent, Skin)], camera: Vec3) {
    for (agent, _) in agents {
        if let Err(error) = agent.turn(game) {
            eprintln!("A script stopped: {error}");
        }
    }
    game.tick();
    for player in game.players.iter_mut().take(TEST_PLAYERS) {
        stand_in(player, camera);
    }
}

/// The agents' players, gathered in a ring in view of the start camera.
fn agent_players(world: &World, skins: &[Skin]) -> Vec<Player> {
    let spot = START_POSITION + (START_TARGET - START_POSITION).with_y(0.0).normalize() * AGENTS_DISTANCE;
    let radius = if skins.len() > 1 { AGENTS_RADIUS } else { 0.0 };
    (0..)
        .zip(skins)
        .map(|(index, &skin)| {
            let angle = index as f32 * TAU / skins.len() as f32;
            let place = spot + vec3(angle.sin(), 0.0, angle.cos()) * radius;
            Player::new(ground_at(world, place.x.floor() as i32, place.z.floor() as i32), angle, skin)
        })
        .collect()
}

/// Stand-in agent for the test players: drives a player only through its controls, as a real agent will. Diggers keep digging
/// whatever they look at, until nothing is left in reach. Builders look straight down and jump while they have blocks, placing
/// one under their feet each jump. Everyone else stands still and watches the spectator's camera when it comes close.
fn stand_in(player: &mut Player, camera: Vec3) {
    if player.controls.place {
        if player.is_on_ground() && player.inventory.slot(0).is_some() {
            player.controls.jump = Some(*JUMP_HEIGHTS.end());
        }
    } else if !player.controls.attack {
        look_toward(player, camera);
    }
}

/// How close the camera must come before an idle player looks at it.
const LOOK_RANGE: f32 = 8.0;
/// How far the head turns from the body, side to side and up and down.
const HEAD_YAW_LIMIT: f32 = 70f32.to_radians();
const HEAD_PITCH_LIMIT: f32 = 45f32.to_radians();
/// Fraction of the remaining turn the head makes each tick.
const HEAD_EASE: f32 = 0.3;

/// Turns the head toward `target` as far as the neck allows while it's within `LOOK_RANGE`, and
/// back to straight ahead otherwise.
fn look_toward(player: &mut Player, target: Vec3) {
    let to_target = target - (player.avatar(1.0).position + Vec3::Y * EYE_HEIGHT);
    let (yaw, pitch) = if to_target.length() < LOOK_RANGE {
        let yaw = (-to_target.x).atan2(-to_target.z) - player.controls.yaw;
        let yaw = (yaw + PI).rem_euclid(TAU) - PI;
        let pitch = to_target.y.atan2(to_target.with_y(0.0).length());
        (
            yaw.clamp(-HEAD_YAW_LIMIT, HEAD_YAW_LIMIT),
            pitch.clamp(-HEAD_PITCH_LIMIT, HEAD_PITCH_LIMIT),
        )
    } else {
        (0.0, 0.0)
    };
    let controls = &mut player.controls;
    controls.head_yaw += (yaw - controls.head_yaw) * HEAD_EASE;
    controls.head_pitch += (pitch - controls.head_pitch) * HEAD_EASE;
}

/// The agents in the named agents file, or in `AGENTS_FILE` if there is one. An agent whose script won't load is left out, so a
/// sample file still runs before its scripts are built.
fn load_agents(named: Option<&Path>) -> Result<Vec<(Agent, Skin)>, String> {
    let path = named.unwrap_or(Path::new(AGENTS_FILE));
    if named.is_none() && !path.exists() {
        return Ok(Vec::new());
    }
    let specs = agents::read(path).map_err(|error| format!("Can't read {}: {error}", path.display()))?;
    let mut loaded = Vec::new();
    for spec in specs {
        let script = std::fs::read(&spec.script).map_err(|error| error.to_string());
        match script.and_then(|wasm| Agent::load(&wasm, TEST_PLAYERS + loaded.len(), spec.name.clone()).map_err(|error| error.to_string()))
        {
            Ok(agent) => loaded.push((agent, spec.skin)),
            Err(error) => eprintln!("Leaving {} out: can't load {}: {error}", spec.name, spec.script.display()),
        }
    }
    Ok(loaded)
}

fn main() {
    env_logger::init();
    let Options { seed, agents } = parse_args(std::env::args().skip(1)).unwrap_or_else(|error| {
        eprintln!("{error}\n{USAGE}");
        std::process::exit(2);
    });
    // Loaded before the world is built, so mistakes are reported straight away.
    let agents = load_agents(agents.as_deref()).unwrap_or_else(|error| {
        eprintln!("{error}");
        std::process::exit(2);
    });
    let launched = Instant::now();
    let event_loop = EventLoop::new().expect("create event loop");
    let mut app = App {
        game: Game::default(),
        agents,
        seed,
        loading: Some(Loading::new(WORLD, seed, START_POSITION.xz())),
        meshed: MeshedArea::new(WORLD),
        loading_drawn: None,
        launched,
        camera: start_camera(0),
        ticks: START_TICKS,
        unsimulated: 0.0,
        window: None,
        renderer: None,
        spectator: Spectator::default(),
        mouse_captured: false,
        culling: true,
        xray: false,
        last_frame: Instant::now(),
        timings: Timings::default(),
    };
    event_loop.run_app(&mut app).expect("run event loop");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options_from(args: &[&str]) -> Result<Options, String> {
        parse_args(args.iter().map(|arg| arg.to_string()))
    }

    fn seed_from(args: &[&str]) -> Result<u32, String> {
        options_from(args).map(|options| options.seed)
    }

    #[test]
    fn seed_defaults_and_comes_from_the_flag() {
        assert_eq!(seed_from(&[]), Ok(142));
        assert_eq!(seed_from(&["--seed", "108"]), Ok(108));
        assert_eq!(seed_from(&["--seed", "4294967295"]), Ok(u32::MAX));
    }

    #[test]
    fn agents_file_comes_from_its_flag_alongside_the_seed() {
        assert_eq!(options_from(&[]).map(|options| options.agents), Ok(None));
        assert_eq!(
            options_from(&["--agents", "crowd.toml", "--seed", "7"]),
            Ok(Options {
                seed: 7,
                agents: Some("crowd.toml".into())
            })
        );
    }

    #[test]
    fn bad_arguments_are_errors() {
        for args in [
            &["--seed"][..],
            &["--seed", "-1"],
            &["--seed", "4294967296"],
            &["--seed", "abc"],
            &["--size", "3"],
            &["--agents"],
            &["--agent", "pillar.wasm"],
            &["108"],
        ] {
            assert!(seed_from(args).is_err(), "{args:?}");
        }
    }

    #[test]
    fn start_camera_rises_over_high_ground_and_keeps_its_direction() {
        let framed = Camera::looking_at(START_POSITION, START_TARGET);
        let low = start_camera(50);
        assert_eq!(low.position, START_POSITION);
        let high = start_camera(90);
        assert_eq!(high.position, START_POSITION.with_y(94.0));
        for camera in [low, high] {
            assert!((camera.yaw - framed.yaw).abs() < 1e-6);
            assert!((camera.pitch - framed.pitch).abs() < 1e-6);
        }
    }

    #[test]
    fn idle_player_looks_at_a_nearby_camera_within_its_neck_limits() {
        let mut player = Player::new(Vec3::ZERO, 0.0, Skin::Player);
        // Up and to the left of a player facing -Z.
        let camera = vec3(-2.0, EYE_HEIGHT + 2.0, -2.0);
        for _ in 0..50 {
            look_toward(&mut player, camera);
        }
        let controls = &player.controls;
        assert!((controls.head_yaw - 45f32.to_radians()).abs() < 1e-3);
        assert!(controls.head_pitch > 0.5 && controls.head_pitch <= HEAD_PITCH_LIMIT);

        // Directly behind: the head turns as far as it can.
        for _ in 0..50 {
            look_toward(&mut player, vec3(0.5, EYE_HEIGHT, 3.0));
        }
        assert!((player.controls.head_yaw.abs() - HEAD_YAW_LIMIT).abs() < 1e-3);

        // Out of range: back to straight ahead.
        for _ in 0..50 {
            look_toward(&mut player, vec3(0.0, EYE_HEIGHT, -LOOK_RANGE * 2.0));
        }
        assert!(player.controls.head_yaw.abs() < 1e-3);
    }
}
