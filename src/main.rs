mod agents;
mod avatar;
mod loading;
mod meshed_area;
mod mesher;
mod render;
mod sky;
mod spectator;
mod visibility;

use std::f32::consts::TAU;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use winit::application::ApplicationHandler;
use winit::event::{DeviceEvent, DeviceId, ElementState, KeyEvent, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{CursorGrabMode, Window, WindowId};

use agent::Agent;
use glam::{IVec3, Vec3, Vec3Swizzles, vec3};
use loading::Loading;
use meshed_area::MeshedArea;
use render::{Camera, Frustum, ListLine, Renderer};
use sim::avatar::{Avatar, Skin};
use sim::game::Game;
use sim::player::{EYE_HEIGHT, Player, TICK_RATE};
use sim::world::{CHUNK_SIZE, ChunkSet, Extent, World};
use sky::{DAY_TICKS, Sky};
use spectator::{AgentList, Spectator};

struct App {
    /// Players are empty until the world is loaded, since they stand on it.
    game: Game,
    /// The agents from the agents file, each driving the player at its own place in the game's players, with that player's skin.
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
    agent_list: AgentList,
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

impl App {
    /// Acts on any key but Escape, which needs the window.
    fn key(&mut self, event: &KeyEvent) {
        self.spectator.key(event);
        let PhysicalKey::Code(code) = event.physical_key else {
            return;
        };
        if event.state != ElementState::Pressed || event.repeat {
            return;
        }
        self.culling ^= code == CULLING_KEY;
        self.xray ^= code == XRAY_KEY;
        if let Some(index) = self.agent_list.key(code, self.agents.len())
            && let Some(player) = &self.game.players.get(index)
        {
            spectator::face(&mut self.camera, player.controls.yaw);
            look_back_at(&mut self.camera, &self.game.world, eyes(player, 1.0));
        }
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
                self.key(&event);
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
                    let skins: Vec<Skin> = self.agents.iter().map(|&(_, skin)| skin).collect();
                    self.game.players = agent_players(&self.game.world, &skins);
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
                if self.spectator.is_moving() {
                    self.agent_list.stop_following();
                }
                self.spectator.update(&mut self.camera, seconds);
                let camera = self.camera.position.xz();
                self.meshed.update(&mut self.game.world, renderer, camera, MESHING_SLICE);
                // Simulate in fixed ticks; a long stall drops time rather than ticking to catch up.
                self.unsimulated = (self.unsimulated + seconds).min(MAX_CATCH_UP);
                while self.unsimulated >= 1.0 / TICK_RATE {
                    self.unsimulated -= 1.0 / TICK_RATE;
                    self.ticks += 1;
                    let tick_start = Instant::now();
                    tick(&mut self.game, &mut self.agents);
                    self.timings.ticks += 1;
                    self.timings.tick_seconds += tick_start.elapsed().as_secs_f32();
                }
                let alpha = self.unsimulated * TICK_RATE;
                renderer.list(watch_agents(&self.agent_list, &self.agents, &self.game, &mut self.camera, alpha));
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

/// Blocks from the start camera, toward where it looks, that the agents gather, and how far round that spot they stand.
const AGENTS_DISTANCE: f32 = 6.0;
const AGENTS_RADIUS: f32 = 3.0;

/// Advances the game a tick: the agents take their turns, then everyone moves.
fn tick(game: &mut Game, agents: &mut [(Agent, Skin)]) {
    for (agent, _) in agents {
        if let Err(error) = agent.turn(game) {
            eprintln!("A script stopped: {error}");
        }
    }
    game.tick();
}

/// Keeps the camera on the agent being followed, and lists the agents with how far they are from the camera, `alpha` of the
/// way from the last tick to this one.
fn watch_agents(list: &AgentList, agents: &[(Agent, Skin)], game: &Game, camera: &mut Camera, alpha: f32) -> Vec<ListLine> {
    let players = &game.players;
    if let Some(player) = list.following().and_then(|index| players.get(index)) {
        look_back_at(camera, &game.world, eyes(player, alpha));
    }
    let distances: Vec<(&str, f32)> = agents
        .iter()
        .zip(players)
        .map(|((agent, _), player)| (agent.name(), player.avatar(alpha).position.distance(camera.position)))
        .collect();
    list.lines(&distances)
}

/// Puts the camera back from `focus` along its view, as far as `FOLLOW_DISTANCE` but in front of any block in the way.
fn look_back_at(camera: &mut Camera, world: &World, focus: Vec3) {
    // Steps back a tenth of a block at a time, and keeps a tenth clear of the block it meets so the near plane doesn't cut it.
    let back = -camera.forward();
    let steps = (spectator::FOLLOW_DISTANCE * 10.0) as u32;
    let clear = (1..=steps)
        .find(|&step| world.block((focus + back * (step as f32 / 10.0)).floor().as_ivec3()).is_solid())
        .map_or(spectator::FOLLOW_DISTANCE, |step| (step as f32 - 2.0).max(0.0) / 10.0);
    spectator::orbit(camera, focus, clear);
}

/// Where a player's eyes are, `alpha` of the way from the last tick to this one.
fn eyes(player: &Player, alpha: f32) -> Vec3 {
    player.avatar(alpha).position + Vec3::Y * EYE_HEIGHT
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
        match script.and_then(|wasm| Agent::load(&wasm, loaded.len(), spec.name.clone()).map_err(|error| error.to_string())) {
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
        agent_list: AgentList::default(),
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
}
