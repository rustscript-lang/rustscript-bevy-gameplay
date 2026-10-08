#![allow(clippy::type_complexity, clippy::drop_non_drop)]

use bevy::{
    asset::RenderAssetUsages,
    camera::{OrthographicProjection, Projection, ScalingMode},
    image::{CompressedImageFormats, ImageSampler, ImageType},
    prelude::*,
};
use bevy_egui::{
    EguiContexts, EguiGlobalSettings, EguiMultipassSchedule, EguiPlugin, EguiPrimaryContextPass,
    PrimaryEguiContext, egui,
};
use rustscript_bevy_gameplay::{
    AttackPower, AttackStyle, Enemy, Health, Player, PlayerProjectileLoadout, Position, RewardItem,
    ShooterData, ShooterEffects, ShooterFrame, ShooterProjectile, Velocity, apply_shooter_script,
    run_shooter_frame_script,
};
#[cfg(not(target_arch = "wasm32"))]
use rustscript_bevy_gameplay::{ShooterSpawnRules, debug_shooter_script, snapshot_shooter_world};
#[cfg(target_arch = "wasm32")]
use rustscript_bevy_gameplay::{cooperative_debug::CooperativeDebugger, snapshot_shooter_world};
use script_editor::{DebugSession, EditorAction, LiveScriptEditor, ScriptTab};
use std::f32::consts::FRAC_PI_2;
#[cfg(not(target_arch = "wasm32"))]
use vm::{DebugCommandBridge, Debugger};
use web_time::Instant;
#[path = "common/script_editor.rs"]
#[allow(dead_code)]
mod script_editor;

const SCRIPT: &str = include_str!("../scripts/shooter_game.rss");
const FLOW_SCRIPT: &str = include_str!("../scripts/shooter_flow.rss");
const PLANE_SCRIPT: &str = include_str!("../scripts/shooter_planes.rss");
const PROJECTILE_SCRIPT: &str = include_str!("../scripts/shooter_projectiles.rss");
const SPAWN_SCRIPT: &str = include_str!("../scripts/shooter_spawns.rss");
const LEFT: f32 = -260.0;
const RIGHT: f32 = 260.0;
const TOP: f32 = 520.0;
const BOTTOM: f32 = -520.0;
const PLAYER_MAX_HEALTH: i64 = 120;
const SCRIPT_PANEL_WIDTH: f32 = 430.0;
const GAMEPLAY_VIEW_WIDTH: u32 = 720;
const GAMEPLAY_VIEW_HEIGHT: u32 = 1180;
const GAMEPLAY_WORLD_PADDING_X: f32 = 220.0;
const GAMEPLAY_WORLD_PADDING_Y: f32 = 220.0;

fn gameplay_camera_transform() -> Transform {
    Transform::from_xyz((LEFT + RIGHT) * 0.5, (TOP + BOTTOM) * 0.5, 0.0)
}

fn gameplay_view_fraction() -> f32 {
    GAMEPLAY_VIEW_WIDTH as f32 / default_window_size().x as f32
}

fn gameplay_camera_projection() -> Projection {
    let gameplay_fraction = gameplay_view_fraction();
    Projection::Orthographic(OrthographicProjection {
        viewport_origin: Vec2::new(gameplay_fraction * 0.5, 0.5),
        scaling_mode: ScalingMode::AutoMin {
            min_width: (RIGHT - LEFT + GAMEPLAY_WORLD_PADDING_X) / gameplay_fraction,
            min_height: TOP - BOTTOM + GAMEPLAY_WORLD_PADDING_Y,
        },
        ..OrthographicProjection::default_2d()
    })
}

fn default_window_size() -> UVec2 {
    UVec2::new(
        GAMEPLAY_VIEW_WIDTH + SCRIPT_PANEL_WIDTH.round() as u32,
        GAMEPLAY_VIEW_HEIGHT,
    )
}

fn main() {
    #[cfg(not(target_arch = "wasm32"))]
    if std::env::args().any(|arg| arg == "--script-smoke") {
        run_script_smoke();
        return;
    }

    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "RustScript Bevy Shooter".to_string(),
                resolution: default_window_size().into(),
                canvas: Some("#game-canvas".into()),
                fit_canvas_to_parent: true,
                prevent_default_event_handling: true,
                ..default()
            }),
            ..default()
        }))
        .add_plugins(EguiPlugin::default())
        .insert_resource(ClearColor(Color::srgb(0.055, 0.085, 0.14)))
        .insert_resource(Score(0))
        .insert_resource(GameFlow::Running)
        .insert_resource(ScriptEditor::default())
        .add_systems(Startup, setup)
        .add_systems(EguiPrimaryContextPass, script_panel)
        .add_systems(
            Update,
            (
                apply_pending_script,
                attach_render_components,
                tick_script_gameplay,
                attach_projectile_visuals,
                play_script_effects,
                tick_visual_lifetimes,
                sync_positions,
                sync_projectile_visuals,
                animate_sprites,
                animate_visual_motion,
            )
                .chain(),
        )
        .run();
}

#[cfg(not(target_arch = "wasm32"))]
fn run_script_smoke() {
    let mut world = bevy_ecs::prelude::World::new();
    let summary = apply_shooter_script(&mut world, SCRIPT).expect("shooter script should apply");
    let (enemy_rules, reward_rules) = world
        .get_resource::<ShooterSpawnRules>()
        .map(|rules| (rules.enemies.len(), rules.rewards.len()))
        .unwrap_or((0, 0));
    println!(
        "player_hp={}, attack={}:{}, projectiles={}:{}, enemies={}, rewards={}, enemy_rules={}, reward_rules={}, jit_enabled={}, jit_traces={}",
        summary.player_health,
        summary.player_attack_style,
        summary.player_attack_power,
        summary.player_projectile_kind,
        summary.player_projectile_count,
        summary.enemies_spawned,
        summary.rewards_spawned,
        enemy_rules,
        reward_rules,
        summary.jit.enabled,
        summary.jit.trace_count
    );
    world.insert_resource(ShooterFrame(std::collections::HashMap::from([(
        "delta_ms".into(),
        300.0,
    )])));
    for source in [FLOW_SCRIPT, PLANE_SCRIPT, PROJECTILE_SCRIPT, SPAWN_SCRIPT] {
        run_shooter_frame_script(&mut world, source).expect("shooter gameplay RSS");
    }
    println!(
        "runtime_projectiles={}, rss_tabs=5",
        world.query::<&ShooterProjectile>().iter(&world).count()
    );
}

#[derive(Resource)]
struct ScriptEditor {
    editor: LiveScriptEditor,
    debug_session: Option<DebugSession>,
    debug_previous_flow: Option<GameFlow>,
    pending_save: bool,
    pending_restart: bool,
    last_initial_source: String,
    status: String,
    jit_enabled: bool,
    jit_trace_count: usize,
}

impl Default for ScriptEditor {
    fn default() -> Self {
        let mut editor = LiveScriptEditor::new(vec![
            ScriptTab::new("shooter_game.rss", SCRIPT, "", SHOOTER_HOST_APIS),
            ScriptTab::new("shooter_flow.rss", FLOW_SCRIPT, "", SHOOTER_HOST_APIS),
            ScriptTab::new("shooter_planes.rss", PLANE_SCRIPT, "", SHOOTER_HOST_APIS),
            ScriptTab::new(
                "shooter_projectiles.rss",
                PROJECTILE_SCRIPT,
                "",
                SHOOTER_HOST_APIS,
            ),
            ScriptTab::new("shooter_spawns.rss", SPAWN_SCRIPT, "", SHOOTER_HOST_APIS),
        ]);
        editor.lint_all();
        Self {
            editor,
            debug_session: None,
            debug_previous_flow: None,
            pending_save: true,
            pending_restart: false,
            status: "Initializing RSS gameplay".into(),
            last_initial_source: String::new(),
            jit_enabled: !cfg!(target_arch = "wasm32"),
            jit_trace_count: 0,
        }
    }
}

#[derive(Resource, Deref, DerefMut)]
struct Score(u32);

#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
enum GameFlow {
    Running,
    Paused,
    GameOver,
}

impl GameFlow {
    fn is_running(self) -> bool {
        self == Self::Running
    }

    fn label(self) -> &'static str {
        match self {
            Self::Running => "Running",
            Self::Paused => "Paused",
            Self::GameOver => "Game Over",
        }
    }
}

const SHOOTER_HOST_APIS: &[&str] = &[
    "bevy::Shooter::set_player_health",
    "bevy::Shooter::set_player_attack",
    "bevy::Shooter::set_player_projectiles",
    "bevy::Shooter::spawn_enemy",
    "bevy::Shooter::spawn_reward",
    "bevy::Shooter::spawn_enemy_every",
    "bevy::Shooter::spawn_reward_every",
    "bevy::Shooter::spawn_enemy_after_kills",
    "bevy::Shooter::entities",
    "bevy::Shooter::entity_count",
    "bevy::Shooter::get",
    "bevy::Shooter::set",
    "bevy::Shooter::text",
    "bevy::Shooter::projectile",
    "bevy::Shooter::reward",
    "bevy::Shooter::despawn",
    "bevy::Shooter::mark_hit",
    "bevy::Shooter::effect",
    "bevy::Shooter::rule_count",
    "bevy::Shooter::rule_get",
    "bevy::Shooter::rule_set",
    "bevy::Shooter::rule_spawn",
];

#[derive(Resource, Clone)]
struct ShooterAssets {
    background: Handle<Image>,
    player_frames: Vec<Handle<Image>>,
    enemy_scout: Handle<Image>,
    enemy_bomber: Handle<Image>,
    enemy_weaver: Handle<Image>,
    enemy_tank: Handle<Image>,
    enemy_sniper: Handle<Image>,
    enemy_carrier: Handle<Image>,
    enemy_striker: Handle<Image>,
    enemy_boss: Handle<Image>,
    bolt_frames: Vec<Handle<Image>>,
    laser_frames: Vec<Handle<Image>>,
    player_missile_frames: Vec<Handle<Image>>,
    enemy_missile_frames: Vec<Handle<Image>>,
    shockwave_frames: Vec<Handle<Image>>,
    hit_effect: Handle<Image>,
    explosion_frames: Vec<Handle<Image>>,
}

struct EmbeddedImage {
    path: &'static str,
    bytes: &'static [u8],
}

const EMBEDDED_SHOOTER_IMAGES: &[EmbeddedImage] = &[
    EmbeddedImage {
        path: "shooter/background_nebula.png",
        bytes: include_bytes!("../assets/shooter/background_nebula.png"),
    },
    EmbeddedImage {
        path: "shooter/player_0.png",
        bytes: include_bytes!("../assets/shooter/player_0.png"),
    },
    EmbeddedImage {
        path: "shooter/player_1.png",
        bytes: include_bytes!("../assets/shooter/player_1.png"),
    },
    EmbeddedImage {
        path: "shooter/player_2.png",
        bytes: include_bytes!("../assets/shooter/player_2.png"),
    },
    EmbeddedImage {
        path: "shooter/enemy_craft_scout.png",
        bytes: include_bytes!("../assets/shooter/enemy_craft_scout.png"),
    },
    EmbeddedImage {
        path: "shooter/enemy_craft_bomber.png",
        bytes: include_bytes!("../assets/shooter/enemy_craft_bomber.png"),
    },
    EmbeddedImage {
        path: "shooter/enemy_craft_weaver.png",
        bytes: include_bytes!("../assets/shooter/enemy_craft_weaver.png"),
    },
    EmbeddedImage {
        path: "shooter/enemy_craft_tank.png",
        bytes: include_bytes!("../assets/shooter/enemy_craft_tank.png"),
    },
    EmbeddedImage {
        path: "shooter/enemy_craft_sniper.png",
        bytes: include_bytes!("../assets/shooter/enemy_craft_sniper.png"),
    },
    EmbeddedImage {
        path: "shooter/enemy_craft_carrier.png",
        bytes: include_bytes!("../assets/shooter/enemy_craft_carrier.png"),
    },
    EmbeddedImage {
        path: "shooter/enemy_craft_striker.png",
        bytes: include_bytes!("../assets/shooter/enemy_craft_striker.png"),
    },
    EmbeddedImage {
        path: "shooter/enemy_craft_boss.png",
        bytes: include_bytes!("../assets/shooter/enemy_craft_boss.png"),
    },
    EmbeddedImage {
        path: "shooter/bolt_0.png",
        bytes: include_bytes!("../assets/shooter/bolt_0.png"),
    },
    EmbeddedImage {
        path: "shooter/bolt_1.png",
        bytes: include_bytes!("../assets/shooter/bolt_1.png"),
    },
    EmbeddedImage {
        path: "shooter/laser_0.png",
        bytes: include_bytes!("../assets/shooter/laser_0.png"),
    },
    EmbeddedImage {
        path: "shooter/laser_1.png",
        bytes: include_bytes!("../assets/shooter/laser_1.png"),
    },
    EmbeddedImage {
        path: "shooter/missile_player_0.png",
        bytes: include_bytes!("../assets/shooter/missile_player_0.png"),
    },
    EmbeddedImage {
        path: "shooter/missile_player_1.png",
        bytes: include_bytes!("../assets/shooter/missile_player_1.png"),
    },
    EmbeddedImage {
        path: "shooter/missile_enemy_0.png",
        bytes: include_bytes!("../assets/shooter/missile_enemy_0.png"),
    },
    EmbeddedImage {
        path: "shooter/missile_enemy_1.png",
        bytes: include_bytes!("../assets/shooter/missile_enemy_1.png"),
    },
    EmbeddedImage {
        path: "shooter/shockwave_0.png",
        bytes: include_bytes!("../assets/shooter/shockwave_0.png"),
    },
    EmbeddedImage {
        path: "shooter/shockwave_1.png",
        bytes: include_bytes!("../assets/shooter/shockwave_1.png"),
    },
    EmbeddedImage {
        path: "shooter/shockwave_2.png",
        bytes: include_bytes!("../assets/shooter/shockwave_2.png"),
    },
    EmbeddedImage {
        path: "shooter/shockwave_3.png",
        bytes: include_bytes!("../assets/shooter/shockwave_3.png"),
    },
    EmbeddedImage {
        path: "shooter/shockwave_4.png",
        bytes: include_bytes!("../assets/shooter/shockwave_4.png"),
    },
    EmbeddedImage {
        path: "shooter/hit_flash.png",
        bytes: include_bytes!("../assets/shooter/hit_flash.png"),
    },
    EmbeddedImage {
        path: "shooter/explosion_0.png",
        bytes: include_bytes!("../assets/shooter/explosion_0.png"),
    },
    EmbeddedImage {
        path: "shooter/explosion_1.png",
        bytes: include_bytes!("../assets/shooter/explosion_1.png"),
    },
    EmbeddedImage {
        path: "shooter/explosion_2.png",
        bytes: include_bytes!("../assets/shooter/explosion_2.png"),
    },
    EmbeddedImage {
        path: "shooter/explosion_3.png",
        bytes: include_bytes!("../assets/shooter/explosion_3.png"),
    },
    EmbeddedImage {
        path: "shooter/explosion_4.png",
        bytes: include_bytes!("../assets/shooter/explosion_4.png"),
    },
    EmbeddedImage {
        path: "shooter/explosion_5.png",
        bytes: include_bytes!("../assets/shooter/explosion_5.png"),
    },
    EmbeddedImage {
        path: "shooter/explosion_6.png",
        bytes: include_bytes!("../assets/shooter/explosion_6.png"),
    },
    EmbeddedImage {
        path: "shooter/explosion_7.png",
        bytes: include_bytes!("../assets/shooter/explosion_7.png"),
    },
    EmbeddedImage {
        path: "shooter/explosion_8.png",
        bytes: include_bytes!("../assets/shooter/explosion_8.png"),
    },
];

impl ShooterAssets {
    fn load(images: &mut Assets<Image>) -> Self {
        Self {
            background: load_embedded_image(images, "shooter/background_nebula.png"),
            player_frames: load_images(
                images,
                &[
                    "shooter/player_0.png",
                    "shooter/player_1.png",
                    "shooter/player_2.png",
                ],
            ),
            enemy_scout: load_embedded_image(images, enemy_asset_path_for_kind("scout")),
            enemy_bomber: load_embedded_image(images, enemy_asset_path_for_kind("bomber")),
            enemy_weaver: load_embedded_image(images, enemy_asset_path_for_kind("weaver")),
            enemy_tank: load_embedded_image(images, enemy_asset_path_for_kind("tank")),
            enemy_sniper: load_embedded_image(images, enemy_asset_path_for_kind("sniper")),
            enemy_carrier: load_embedded_image(images, enemy_asset_path_for_kind("carrier")),
            enemy_striker: load_embedded_image(images, enemy_asset_path_for_kind("striker")),
            enemy_boss: load_embedded_image(images, enemy_asset_path_for_kind("boss")),
            bolt_frames: load_images(images, &["shooter/bolt_0.png", "shooter/bolt_1.png"]),
            laser_frames: load_images(images, &["shooter/laser_0.png", "shooter/laser_1.png"]),
            player_missile_frames: load_images(
                images,
                &[
                    "shooter/missile_player_0.png",
                    "shooter/missile_player_1.png",
                ],
            ),
            enemy_missile_frames: load_images(
                images,
                &["shooter/missile_enemy_0.png", "shooter/missile_enemy_1.png"],
            ),
            shockwave_frames: load_images(
                images,
                &[
                    "shooter/shockwave_0.png",
                    "shooter/shockwave_1.png",
                    "shooter/shockwave_2.png",
                    "shooter/shockwave_3.png",
                    "shooter/shockwave_4.png",
                ],
            ),
            hit_effect: load_embedded_image(images, "shooter/hit_flash.png"),
            explosion_frames: load_images(
                images,
                &[
                    "shooter/explosion_0.png",
                    "shooter/explosion_1.png",
                    "shooter/explosion_2.png",
                    "shooter/explosion_3.png",
                    "shooter/explosion_4.png",
                    "shooter/explosion_5.png",
                    "shooter/explosion_6.png",
                    "shooter/explosion_7.png",
                    "shooter/explosion_8.png",
                ],
            ),
        }
    }

    fn enemy_image(&self, kind: &str) -> Handle<Image> {
        match kind {
            "bomber" => self.enemy_bomber.clone(),
            "weaver" | "ace" => self.enemy_weaver.clone(),
            "tank" => self.enemy_tank.clone(),
            "sniper" => self.enemy_sniper.clone(),
            "carrier" => self.enemy_carrier.clone(),
            "striker" => self.enemy_striker.clone(),
            "boss" => self.enemy_boss.clone(),
            _ => self.enemy_scout.clone(),
        }
    }
}

fn load_images(images: &mut Assets<Image>, paths: &[&'static str]) -> Vec<Handle<Image>> {
    paths
        .iter()
        .map(|path| load_embedded_image(images, path))
        .collect()
}

fn load_embedded_image(images: &mut Assets<Image>, path: &str) -> Handle<Image> {
    let bytes = embedded_image_bytes(path)
        .unwrap_or_else(|| panic!("missing embedded shooter image registration: {path}"));
    let image = Image::from_buffer(
        bytes,
        ImageType::Extension("png"),
        CompressedImageFormats::NONE,
        true,
        ImageSampler::Default,
        RenderAssetUsages::default(),
    )
    .unwrap_or_else(|err| panic!("failed to decode embedded shooter image {path}: {err}"));
    images.add(image)
}

fn embedded_image_bytes(path: &str) -> Option<&'static [u8]> {
    EMBEDDED_SHOOTER_IMAGES
        .iter()
        .find(|image| image.path == path)
        .map(|image| image.bytes)
}

fn enemy_asset_path_for_kind(kind: &str) -> &'static str {
    match kind {
        "bomber" => "shooter/enemy_craft_bomber.png",
        "weaver" | "ace" => "shooter/enemy_craft_weaver.png",
        "tank" => "shooter/enemy_craft_tank.png",
        "sniper" => "shooter/enemy_craft_sniper.png",
        "carrier" => "shooter/enemy_craft_carrier.png",
        "striker" => "shooter/enemy_craft_striker.png",
        "boss" => "shooter/enemy_craft_boss.png",
        _ => "shooter/enemy_craft_scout.png",
    }
}

fn enemy_body_size_for_kind(kind: &str) -> Vec2 {
    match kind {
        "bomber" => Vec2::new(92.0, 54.0),
        "weaver" | "ace" => Vec2::new(76.0, 70.0),
        "tank" => Vec2::splat(104.0),
        "sniper" => Vec2::new(92.0, 36.0),
        "carrier" => Vec2::new(112.0, 62.0),
        "striker" => Vec2::new(68.0, 76.0),
        "boss" => Vec2::splat(150.0),
        _ => Vec2::new(70.0, 80.0),
    }
}

#[derive(Component)]
struct PlayerShip;

#[derive(Component)]
struct EnemyShip;

#[derive(Component)]
struct RewardPickup;

#[derive(Component)]
struct GameCamera;

#[derive(Component)]
struct HitEffect;

#[derive(Component)]
struct ExplosionEffect;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProjectileOwner {
    Player,
    Enemy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProjectileKind {
    Bolt,
    Spread,
    Laser,
    HomingMissile,
    Shockwave,
    Plasma,
    Flak,
    Rail,
}

#[derive(Component)]
struct Lifetime {
    elapsed_ms: f32,
    duration_ms: f32,
}

#[derive(Component)]
struct SpriteFrames {
    frames: Vec<Handle<Image>>,
    index: usize,
    elapsed_ms: f32,
    frame_ms: f32,
}

impl SpriteFrames {
    fn new(frames: Vec<Handle<Image>>, frame_ms: f32) -> Self {
        Self {
            frames,
            index: 0,
            elapsed_ms: 0.0,
            frame_ms,
        }
    }
}

#[derive(Component)]
struct VisualMotion {
    base_scale: Vec3,
    pulse: f32,
    spin: f32,
    phase: f32,
}

type AddedEnemyQuery<'w, 's> =
    Query<'w, 's, (Entity, &'static Enemy, &'static Position), (Added<Enemy>, Without<EnemyShip>)>;
type AddedRewardQuery<'w, 's> = Query<
    'w,
    's,
    (Entity, &'static RewardItem, &'static Position),
    (Added<RewardItem>, Without<RewardPickup>),
>;
fn setup(
    mut commands: Commands,
    mut egui_global_settings: ResMut<EguiGlobalSettings>,
    mut images: ResMut<Assets<Image>>,
) {
    egui_global_settings.auto_create_primary_context = false;
    let assets = ShooterAssets::load(&mut images);

    commands.spawn((
        PrimaryEguiContext,
        EguiMultipassSchedule::new(EguiPrimaryContextPass),
        Camera2d,
        GameCamera,
        gameplay_camera_transform(),
        gameplay_camera_projection(),
    ));
    spawn_background(&mut commands, &assets);
    spawn_starfield(&mut commands, &assets);
    commands.insert_resource(assets);
}

fn spawn_background(commands: &mut Commands, assets: &ShooterAssets) {
    commands.spawn((
        Sprite::from_image(assets.background.clone()),
        Transform {
            translation: Vec3::new((LEFT + RIGHT) * 0.5, (TOP + BOTTOM) * 0.5, -20.0),
            scale: Vec3::splat(1.08),
            ..default()
        },
    ));
}

fn spawn_starfield(commands: &mut Commands, assets: &ShooterAssets) {
    for index in 0..72 {
        let image = assets.bolt_frames[index % assets.bolt_frames.len()].clone();
        let mut sprite = Sprite::from_image(image);
        let alpha = if index % 3 == 0 { 0.24 } else { 0.14 };
        sprite.color = Color::srgba(0.62, 0.78, 1.0, alpha);

        let x = LEFT + 20.0 + ((index * 83) % 500) as f32;
        let y = BOTTOM + 30.0 + ((index * 47) % 1000) as f32;
        let scale = 0.16 + (index % 5) as f32 * 0.025;
        commands.spawn((
            sprite,
            Transform {
                translation: Vec3::new(x, y, -8.0),
                scale: Vec3::splat(scale),
                rotation: Quat::from_rotation_z((index as f32 * 0.37) % std::f32::consts::TAU),
            },
            VisualMotion {
                base_scale: Vec3::splat(scale),
                pulse: 0.18,
                spin: 0.05 + (index % 4) as f32 * 0.02,
                phase: index as f32 * 0.31,
            },
        ));
    }
}

fn apply_pending_script(world: &mut World) {
    let (source, restart) = {
        let mut scripts = world.resource_mut::<ScriptEditor>();
        if !scripts.pending_save && !scripts.pending_restart {
            return;
        }
        let restart = scripts.pending_restart;
        scripts.pending_save = false;
        scripts.pending_restart = false;
        let source = scripts.editor.active_source(0).to_owned();
        scripts.last_initial_source = source.clone();
        (source, restart)
    };
    let result = if restart {
        restart_gameplay(world, &source)
    } else {
        apply_shooter_script(world, &source)
    };
    if result.is_ok() {
        world
            .get_resource_or_insert_with(ShooterFrame::default)
            .0
            .insert("kills_delta".into(), 0.0);
    }
    let mut scripts = world.resource_mut::<ScriptEditor>();
    match result {
        Ok(summary) => {
            scripts.jit_enabled = summary.jit.enabled;
            scripts.jit_trace_count = summary.jit.trace_count;
            scripts.status = format!(
                "Applied: hp {}, enemies {}",
                summary.player_health, summary.enemies_spawned
            );
        }
        Err(error) => {
            scripts.status = error;
        }
    }
}

fn restart_gameplay(
    world: &mut World,
    source: &str,
) -> Result<rustscript_bevy_gameplay::ShooterSummary, String> {
    rustscript_bevy_gameplay::compile_bevy_script(source).map_err(|err| err.to_string())?;
    despawn_entities_with::<ShooterProjectile>(world);
    despawn_entities_with::<Enemy>(world);
    despawn_entities_with::<RewardItem>(world);
    despawn_entities_with::<HitEffect>(world);
    despawn_entities_with::<ExplosionEffect>(world);
    reset_player_runtime(world);
    world.insert_resource(Score(0));
    world.insert_resource(GameFlow::Running);
    world.insert_resource(ShooterFrame::default());
    world.insert_resource(ShooterEffects::default());
    apply_shooter_script(world, source)
}

fn despawn_entities_with<T: Component>(world: &mut World) {
    let entities = world
        .query_filtered::<Entity, With<T>>()
        .iter(world)
        .collect::<Vec<_>>();
    for entity in entities {
        let _despawned = world.despawn(entity);
    }
}

fn reset_player_runtime(world: &mut World) {
    let players = world
        .query_filtered::<Entity, With<Player>>()
        .iter(world)
        .collect::<Vec<_>>();
    for entity in players {
        if let Some(mut position) = world.get_mut::<Position>(entity) {
            position.x = 0.0;
            position.y = -360.0;
        }
        if let Some(mut velocity) = world.get_mut::<Velocity>(entity) {
            velocity.x = 0.0;
            velocity.y = 0.0;
        }
        world.entity_mut(entity).remove::<ShooterData>();
    }
}

fn attach_render_components(
    mut commands: Commands,
    assets: Res<ShooterAssets>,
    players: Query<(Entity, &Position), (Added<Player>, Without<PlayerShip>)>,
    enemies: AddedEnemyQuery,
    rewards: AddedRewardQuery,
) {
    for (entity, position) in &players {
        let image = assets.player_frames[0].clone();
        commands.entity(entity).insert((
            Sprite::from_image(image),
            Transform {
                translation: Vec3::new(position.x, position.y, 2.0),
                scale: Vec3::splat(3.1),
                rotation: Quat::default(),
            },
            PlayerShip,
        ));
    }

    for (entity, enemy, position) in &enemies {
        let image = assets.enemy_image(&enemy.kind);
        let mut sprite = Sprite::from_image(image);
        sprite.custom_size = Some(enemy_body_size_for_kind(&enemy.kind));
        commands.entity(entity).insert((
            sprite,
            Transform {
                translation: Vec3::new(position.x, position.y, 2.0),
                scale: Vec3::ONE,
                rotation: Quat::default(),
            },
            EnemyShip,
        ));
    }

    for (entity, reward, position) in &rewards {
        let image = match reward.kind.as_str() {
            "health" | "hp" => assets.shockwave_frames[0].clone(),
            _ => assets.bolt_frames[0].clone(),
        };
        let mut sprite = Sprite::from_image(image);
        sprite.color = match reward.kind.as_str() {
            "health" | "hp" => Color::srgba(0.38, 1.0, 0.58, 0.94),
            _ => Color::srgba(0.32, 0.86, 1.0, 0.94),
        };
        commands.entity(entity).insert((
            sprite,
            Transform {
                translation: Vec3::new(position.x, position.y, 2.5),
                scale: Vec3::splat(1.55),
                ..default()
            },
            RewardPickup,
            VisualMotion {
                base_scale: Vec3::splat(1.55),
                pulse: 0.12,
                spin: 0.45,
                phase: 0.8,
            },
        ));
    }
}

fn projectile_frames(
    assets: &ShooterAssets,
    owner: ProjectileOwner,
    kind: ProjectileKind,
) -> Vec<Handle<Image>> {
    match kind {
        ProjectileKind::Bolt | ProjectileKind::Spread => assets.bolt_frames.clone(),
        ProjectileKind::Laser | ProjectileKind::Plasma | ProjectileKind::Rail => {
            assets.laser_frames.clone()
        }
        ProjectileKind::HomingMissile => match owner {
            ProjectileOwner::Player => assets.player_missile_frames.clone(),
            ProjectileOwner::Enemy => assets.enemy_missile_frames.clone(),
        },
        ProjectileKind::Shockwave => assets.shockwave_frames.clone(),
        ProjectileKind::Flak => assets.bolt_frames.clone(),
    }
}

fn projectile_color(owner: ProjectileOwner, kind: ProjectileKind) -> Color {
    match (owner, kind) {
        (_, ProjectileKind::Plasma) => Color::srgba(0.55, 0.95, 1.0, 0.96),
        (_, ProjectileKind::Flak) => Color::srgba(1.0, 0.82, 0.3, 0.96),
        (_, ProjectileKind::Rail) => Color::srgba(0.78, 0.62, 1.0, 0.98),
        (ProjectileOwner::Enemy, ProjectileKind::Bolt | ProjectileKind::Spread) => {
            Color::srgba(1.0, 0.58, 0.28, 0.96)
        }
        _ => Color::WHITE,
    }
}

fn projectile_rotation(velocity: Vec2, owner: ProjectileOwner) -> Quat {
    if velocity.length_squared() == 0.0 {
        return match owner {
            ProjectileOwner::Player => Quat::default(),
            ProjectileOwner::Enemy => Quat::from_rotation_z(std::f32::consts::PI),
        };
    }
    Quat::from_rotation_z(velocity.y.atan2(velocity.x) - FRAC_PI_2)
}

fn sync_positions(mut query: Query<(&Position, &mut Transform)>) {
    for (position, mut transform) in &mut query {
        transform.translation.x = position.x;
        transform.translation.y = position.y;
    }
}

fn animate_sprites(time: Res<Time>, mut query: Query<(&mut Sprite, &mut SpriteFrames)>) {
    for (mut sprite, mut animation) in &mut query {
        if animation.frames.len() <= 1 {
            continue;
        }
        animation.elapsed_ms += time.delta_secs() * 1000.0;
        while animation.elapsed_ms >= animation.frame_ms {
            animation.elapsed_ms -= animation.frame_ms;
            animation.index = (animation.index + 1) % animation.frames.len();
            sprite.image = animation.frames[animation.index].clone();
        }
    }
}

fn animate_visual_motion(time: Res<Time>, mut query: Query<(&mut Transform, &mut VisualMotion)>) {
    for (mut transform, mut motion) in &mut query {
        motion.phase += time.delta_secs();
        let pulse = 1.0 + motion.phase.sin() * motion.pulse;
        transform.scale = motion.base_scale * pulse;
        if motion.spin != 0.0 {
            transform.rotate_z(motion.spin * time.delta_secs());
        }
    }
}

fn spawn_hit_effect(
    commands: &mut Commands,
    assets: &ShooterAssets,
    position: Position,
    owner: ProjectileOwner,
) {
    let mut sprite = Sprite::from_image(assets.hit_effect.clone());
    sprite.color = match owner {
        ProjectileOwner::Player => Color::srgba(1.0, 0.92, 0.38, 0.86),
        ProjectileOwner::Enemy => Color::srgba(1.0, 0.32, 0.28, 0.88),
    };
    let scale = match owner {
        ProjectileOwner::Player => 1.05,
        ProjectileOwner::Enemy => 1.25,
    };
    commands.spawn((
        sprite,
        Transform {
            translation: Vec3::new(position.x, position.y, 4.5),
            scale: Vec3::splat(scale),
            ..default()
        },
        HitEffect,
        Lifetime {
            elapsed_ms: 0.0,
            duration_ms: 150.0,
        },
        VisualMotion {
            base_scale: Vec3::splat(scale),
            pulse: 0.2,
            spin: 0.0,
            phase: 0.0,
        },
    ));
}

fn spawn_explosion(
    commands: &mut Commands,
    assets: &ShooterAssets,
    position: Position,
    kind: &str,
) {
    let scale = match kind {
        "boss" => 2.35,
        "carrier" | "tank" => 1.85,
        _ => 1.55,
    };
    commands.spawn((
        Sprite::from_image(assets.explosion_frames[0].clone()),
        Transform {
            translation: Vec3::new(position.x, position.y, 4.8),
            scale: Vec3::splat(scale),
            ..default()
        },
        ExplosionEffect,
        SpriteFrames::new(assets.explosion_frames.clone(), 55.0),
        Lifetime {
            elapsed_ms: 0.0,
            duration_ms: assets.explosion_frames.len() as f32 * 55.0,
        },
        VisualMotion {
            base_scale: Vec3::splat(scale),
            pulse: 0.05,
            spin: 0.0,
            phase: 0.0,
        },
    ));
}

fn jit_status_label(enabled: bool, trace_count: usize) -> String {
    let state = if enabled { "on" } else { "off" };
    format!("JIT: {state}   traces: {trace_count}")
}

fn tick_script_gameplay(world: &mut World) {
    if !world.resource::<GameFlow>().is_running() {
        return;
    }
    let delta_ms = world.resource::<Time>().delta_secs_f64() * 1000.0;
    let input = world.resource::<ButtonInput<KeyCode>>();
    let dx = i32::from(input.pressed(KeyCode::ArrowRight) || input.pressed(KeyCode::KeyD))
        - i32::from(input.pressed(KeyCode::ArrowLeft) || input.pressed(KeyCode::KeyA));
    let dy = i32::from(input.pressed(KeyCode::ArrowUp) || input.pressed(KeyCode::KeyW))
        - i32::from(input.pressed(KeyCode::ArrowDown) || input.pressed(KeyCode::KeyS));
    let mut frame = world.get_resource_or_insert_with(ShooterFrame::default);
    frame.0.insert("delta_ms".into(), delta_ms);
    frame.0.insert("input_x".into(), dx as f64);
    frame.0.insert("input_y".into(), dy as f64);
    frame.0.insert("kills_delta".into(), 0.0);
    let sources = world
        .resource::<ScriptEditor>()
        .editor
        .tabs
        .iter()
        .skip(1)
        .map(|tab| tab.active_source.clone())
        .collect::<Vec<_>>();
    let mut traces = 0;
    for (index, source) in sources.iter().enumerate() {
        match run_shooter_frame_script(world, source) {
            Ok(jit) => traces += jit.trace_count,
            Err(error) => {
                world.insert_resource(GameFlow::Paused);
                let mut scripts = world.resource_mut::<ScriptEditor>();
                scripts.editor.tabs[index + 1].status = format!("Runtime error: {error}");
                scripts.status = error;
                return;
            }
        }
    }
    world.resource_mut::<ScriptEditor>().jit_trace_count = traces;
    let frame = world.resource::<ShooterFrame>();
    let score = frame.0.get("score").copied().unwrap_or(0.0) as u32;
    let game_over = frame.0.get("game_over").copied().unwrap_or(0.0) != 0.0;
    world.resource_mut::<Score>().0 = score;
    if game_over {
        world.insert_resource(GameFlow::GameOver);
    }
}

fn visual_kind(kind: &str) -> ProjectileKind {
    match kind {
        "spread" => ProjectileKind::Spread,
        "laser" => ProjectileKind::Laser,
        "missile" | "homing" => ProjectileKind::HomingMissile,
        "shockwave" => ProjectileKind::Shockwave,
        "plasma" => ProjectileKind::Plasma,
        "flak" => ProjectileKind::Flak,
        "rail" => ProjectileKind::Rail,
        _ => ProjectileKind::Bolt,
    }
}

fn visual_owner(owner: &str) -> ProjectileOwner {
    if owner == "player" {
        ProjectileOwner::Player
    } else {
        ProjectileOwner::Enemy
    }
}

fn attach_projectile_visuals(
    mut commands: Commands,
    assets: Res<ShooterAssets>,
    query: Query<
        (
            Entity,
            &Position,
            &Velocity,
            &ShooterProjectile,
            &ShooterData,
        ),
        Added<ShooterProjectile>,
    >,
) {
    for (entity, position, velocity, projectile, data) in &query {
        let owner = visual_owner(&projectile.owner);
        let kind = visual_kind(&projectile.kind);
        let frames = projectile_frames(&assets, owner, kind);
        let mut sprite = Sprite::from_image(frames[0].clone());
        sprite.color = projectile_color(owner, kind);
        let get = |key: &str| data.0.get(key).copied().unwrap_or(0.0) as f32;
        let scale = Vec3::splat(get("scale"));
        commands.entity(entity).insert((
            sprite,
            Transform {
                translation: Vec3::new(position.x, position.y, 3.0),
                scale,
                rotation: projectile_rotation(Vec2::new(velocity.x, velocity.y), owner),
            },
            SpriteFrames::new(frames, get("frame_ms")),
            VisualMotion {
                base_scale: scale,
                pulse: get("pulse"),
                spin: get("spin"),
                phase: 0.0,
            },
        ));
    }
}

fn sync_projectile_visuals(
    mut query: Query<(
        &ShooterProjectile,
        &ShooterData,
        &Velocity,
        &mut Transform,
        &mut VisualMotion,
    )>,
) {
    for (projectile, data, velocity, mut transform, mut motion) in &mut query {
        motion.base_scale = Vec3::splat(data.0.get("scale").copied().unwrap_or(1.0) as f32);
        transform.rotation = projectile_rotation(
            Vec2::new(velocity.x, velocity.y),
            visual_owner(&projectile.owner),
        );
    }
}

fn play_script_effects(
    mut commands: Commands,
    assets: Res<ShooterAssets>,
    mut effects: Option<ResMut<ShooterEffects>>,
) {
    let Some(effects) = effects.as_mut() else {
        return;
    };
    for (kind, position) in effects.0.drain(..) {
        match kind.strip_prefix("explosion_") {
            Some(enemy_kind) => spawn_explosion(&mut commands, &assets, position, enemy_kind),
            None => spawn_hit_effect(
                &mut commands,
                &assets,
                position,
                if kind == "hit_enemy" {
                    ProjectileOwner::Enemy
                } else {
                    ProjectileOwner::Player
                },
            ),
        }
    }
}

fn tick_visual_lifetimes(
    mut commands: Commands,
    time: Res<Time>,
    mut query: Query<(Entity, &mut Lifetime)>,
) {
    for (entity, mut lifetime) in &mut query {
        lifetime.elapsed_ms += time.delta_secs() * 1000.0;
        if lifetime.elapsed_ms >= lifetime.duration_ms {
            commands.entity(entity).despawn();
        }
    }
}

fn handle_editor_actions(
    world: &mut World,
    scripts: &mut ScriptEditor,
    actions: Vec<EditorAction>,
) {
    for action in actions {
        match action {
            EditorAction::StartDebug(tab) => start_shooter_debug(world, scripts, tab),
            EditorAction::StopDebug => {
                scripts.debug_session = None;
                scripts.editor.clear_debug_state();
                if let Some(previous) = scripts.debug_previous_flow.take() {
                    world.insert_resource(previous);
                }
            }
            EditorAction::StepDebug
            | EditorAction::NextDebug
            | EditorAction::ContinueDebug
            | EditorAction::RefreshLocals => {
                let command = match action {
                    EditorAction::StepDebug => "step",
                    EditorAction::NextDebug => "next",
                    EditorAction::ContinueDebug => "continue",
                    _ => "locals",
                };
                if let Some(session) = scripts.debug_session.as_ref() {
                    session.command(&mut scripts.editor, command);
                }
            }
            EditorAction::RunDebugCommand(command) => {
                if let Some(session) = scripts.debug_session.as_ref() {
                    session.console_command(&mut scripts.editor, &command);
                }
            }
            EditorAction::EvaluateHover { tab, name } => {
                if let Some(session) = scripts.debug_session.as_ref() {
                    session.evaluate_hover(&mut scripts.editor, tab, &name);
                }
            }
            EditorAction::ToggleBreakpoint { tab, line, enabled } => {
                if scripts.editor.debug_tab == Some(tab)
                    && let Some(session) = scripts.debug_session.as_ref()
                {
                    session.set_breakpoint(&mut scripts.editor, line, enabled);
                }
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn start_shooter_debug(world: &mut World, scripts: &mut ScriptEditor, tab: usize) {
    use std::{sync::mpsc, thread};
    scripts.debug_session = None;
    let source = scripts.editor.active_source(tab).to_owned();
    let mut debug_world = snapshot_shooter_world(world);
    let bridge = DebugCommandBridge::new();
    let thread_bridge = bridge.clone();
    let (sender, receiver) = mpsc::channel::<String>();
    thread::spawn(move || {
        let mut debugger = Debugger::with_command_bridge(thread_bridge);
        debugger.stop_on_entry();
        let result = debug_shooter_script(&mut debug_world, &source, &mut debugger);
        let _ = sender.send(
            result
                .map(|_| "shooter debug complete".to_owned())
                .unwrap_or_else(|error| format!("debug error: {error}")),
        );
    });
    if scripts.debug_previous_flow.is_none() {
        scripts.debug_previous_flow = Some(*world.resource::<GameFlow>());
    }
    world.insert_resource(GameFlow::Paused);
    scripts.editor.begin_debug_session(tab);
    scripts.debug_session = Some(DebugSession::new(
        bridge,
        receiver,
        tab,
        scripts.editor.source_line_offset(tab),
        scripts.editor.user_breakpoints(tab),
    ));
}

#[cfg(target_arch = "wasm32")]
fn start_shooter_debug(world: &mut World, scripts: &mut ScriptEditor, tab: usize) {
    scripts.debug_session = None;
    let source = scripts.editor.active_source(tab).to_owned();
    let snapshot = snapshot_shooter_world(world);
    match CooperativeDebugger::from_shooter_snapshot(snapshot, &source) {
        Ok(debugger) => {
            if scripts.debug_previous_flow.is_none() {
                scripts.debug_previous_flow = Some(*world.resource::<GameFlow>());
            }
            world.insert_resource(GameFlow::Paused);
            scripts.editor.begin_debug_session(tab);
            scripts.debug_session = Some(DebugSession::new_web(
                debugger,
                tab,
                scripts.editor.source_line_offset(tab),
                scripts.editor.user_breakpoints(tab),
                |_| {},
            ));
        }
        Err(error) => {
            scripts.editor.clear_debug_state();
            scripts.editor.debug_output = format!("debug error: {error}");
            if let Some(previous) = scripts.debug_previous_flow.take() {
                world.insert_resource(previous);
            }
        }
    }
}

fn script_panel(world: &mut World) {
    let mut editor = world.remove_resource::<ScriptEditor>().unwrap_or_default();
    editor.editor.update_auto_apply(Instant::now());
    if editor.editor.active_source(0) != editor.last_initial_source {
        editor.pending_save = true;
    }
    if let Some(session) = editor.debug_session.as_mut() {
        session.poll(&mut editor.editor);
    }
    if !editor.editor.debug_attached && !editor.editor.debug_starting {
        if let Some(previous) = editor.debug_previous_flow.take() {
            world.insert_resource(previous);
        }
    }
    let mut flow = *world.resource::<GameFlow>();
    let score = world.resource::<Score>().0;
    let player = world
        .query_filtered::<(
            &Health,
            &AttackStyle,
            &AttackPower,
            &PlayerProjectileLoadout,
        ), With<Player>>()
        .iter(world)
        .next()
        .map(|(hp, style, power, loadout)| (*hp, style.clone(), *power, loadout.clone()));
    let enemy_count = world.query::<&Enemy>().iter(world).count();
    let mut actions = Vec::new();
    let mut system_state = bevy::ecs::system::SystemState::<EguiContexts>::new(world);
    let mut contexts = system_state.get_mut(world);
    let Ok(ctx) = contexts.ctx_mut() else {
        world.insert_resource(editor);
        return;
    };
    if flow == GameFlow::GameOver {
        egui::Area::new(egui::Id::new("game_over_overlay"))
            .anchor(
                egui::Align2::CENTER_CENTER,
                egui::vec2(-(SCRIPT_PANEL_WIDTH * 0.5), 0.0),
            )
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(egui::Color32::from_rgba_unmultiplied(8, 14, 24, 215))
                    .stroke(egui::Stroke::new(
                        1.0_f32,
                        egui::Color32::from_rgb(120, 170, 210),
                    ))
                    .corner_radius(egui::CornerRadius::same(6))
                    .inner_margin(egui::Margin::symmetric(18, 14))
                    .show(ui, |ui| {
                        ui.heading("Game Over");
                        ui.label("Press Restart to run the script again.");
                    });
            });
    }

    if let Some((health, _, _, loadout)) = player.as_ref() {
        let ratio = (health.0.max(0) as f32 / PLAYER_MAX_HEALTH as f32).clamp(0.0, 1.0);
        egui::Area::new(egui::Id::new("shooter_hud"))
            .anchor(egui::Align2::LEFT_TOP, egui::vec2(16.0, 16.0))
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(egui::Color32::from_rgba_unmultiplied(8, 16, 28, 190))
                    .stroke(egui::Stroke::new(
                        1.0_f32,
                        egui::Color32::from_rgb(70, 115, 150),
                    ))
                    .corner_radius(egui::CornerRadius::same(6))
                    .inner_margin(egui::Margin::symmetric(10, 8))
                    .show(ui, |ui| {
                        ui.set_width(190.0);
                        ui.label("HP");
                        ui.add(
                            egui::ProgressBar::new(ratio)
                                .fill(egui::Color32::from_rgb(80, 220, 128))
                                .text(format!("{}/{}", health.0.max(0), PLAYER_MAX_HEALTH)),
                        );
                        ui.label(format!("{} x{}", loadout.kind, loadout.count));
                    });
            });
    }

    egui::SidePanel::right("rustscript_panel")
        .resizable(true)
        .default_width(SCRIPT_PANEL_WIDTH)
        .show(ctx, |ui| {
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("Restart").clicked() {
                    editor.pending_restart = true;
                    actions.push(EditorAction::StopDebug);
                }

                let pause_label = if flow == GameFlow::Paused {
                    "Resume"
                } else {
                    "Pause"
                };
                if ui
                    .add_enabled(
                        flow != GameFlow::GameOver && editor.debug_previous_flow.is_none(),
                        egui::Button::new(pause_label),
                    )
                    .clicked()
                {
                    flow = if flow == GameFlow::Paused {
                        GameFlow::Running
                    } else {
                        GameFlow::Paused
                    };
                }
            });
            ui.label(format!("State: {}", flow.label()));
            ui.separator();
            if let Some((health, style, power, loadout)) = player.as_ref() {
                ui.label(format!(
                    "Player: hp {} / attack {} / power {} / {} x{}",
                    health.0.max(0),
                    style.0,
                    power.0,
                    loadout.kind,
                    loadout.count
                ));
            }
            ui.label(format!("Enemies: {}   Score: {}", enemy_count, score));
            ui.label(jit_status_label(editor.jit_enabled, editor.jit_trace_count));
            ui.label(&editor.status);
            ui.separator();
            actions.extend(editor.editor.ui(ui));
        });
    drop(contexts);
    system_state.apply(world);
    world.insert_resource(flow);
    handle_editor_actions(world, &mut editor, actions);
    world.insert_resource(editor);
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::schedule::ScheduleLabel;

    #[test]
    fn every_rss_tab_can_start_step_inspect_and_finish_a_debug_session() {
        use std::{
            thread,
            time::{Duration, Instant as NativeInstant},
        };
        let mut world = World::new();
        apply_shooter_script(&mut world, SCRIPT).unwrap();
        world.insert_resource(GameFlow::Running);
        world.insert_resource(ShooterFrame(std::collections::HashMap::from([(
            "delta_ms".into(),
            16.0,
        )])));
        let mut scripts = ScriptEditor::default();
        assert_eq!(scripts.editor.tabs.len(), 5);
        assert!(
            scripts
                .editor
                .tabs
                .iter()
                .all(|tab| tab.diagnostics.is_empty())
        );
        for tab in 0..5 {
            start_shooter_debug(&mut world, &mut scripts, tab);
            assert_eq!(*world.resource::<GameFlow>(), GameFlow::Paused);
            let deadline = NativeInstant::now() + Duration::from_secs(5);
            while !scripts.editor.debug_attached && NativeInstant::now() < deadline {
                scripts
                    .debug_session
                    .as_mut()
                    .unwrap()
                    .poll(&mut scripts.editor);
                thread::sleep(Duration::from_millis(5));
            }
            assert!(
                scripts.editor.debug_attached,
                "tab {tab}: {}",
                scripts.editor.debug_output
            );
            assert!(scripts.editor.debug_line.is_some());
            handle_editor_actions(&mut world, &mut scripts, vec![EditorAction::StepDebug]);
            while !scripts.editor.debug_attached && NativeInstant::now() < deadline {
                scripts
                    .debug_session
                    .as_mut()
                    .unwrap()
                    .poll(&mut scripts.editor);
                thread::sleep(Duration::from_millis(5));
            }
            handle_editor_actions(&mut world, &mut scripts, vec![EditorAction::NextDebug]);
            while !scripts.editor.debug_attached && NativeInstant::now() < deadline {
                scripts
                    .debug_session
                    .as_mut()
                    .unwrap()
                    .poll(&mut scripts.editor);
                thread::sleep(Duration::from_millis(5));
            }
            handle_editor_actions(&mut world, &mut scripts, vec![EditorAction::RefreshLocals]);
            handle_editor_actions(&mut world, &mut scripts, vec![EditorAction::ContinueDebug]);
            while !scripts
                .editor
                .debug_output
                .contains("shooter debug complete")
                && NativeInstant::now() < deadline
            {
                scripts
                    .debug_session
                    .as_mut()
                    .unwrap()
                    .poll(&mut scripts.editor);
                thread::sleep(Duration::from_millis(5));
            }
            assert!(
                scripts
                    .editor
                    .debug_output
                    .contains("shooter debug complete"),
                "tab {tab}: {}",
                scripts.editor.debug_output
            );
            handle_editor_actions(&mut world, &mut scripts, vec![EditorAction::StopDebug]);
            assert_eq!(*world.resource::<GameFlow>(), GameFlow::Running);
            assert_eq!(world.query::<&Enemy>().iter(&world).count(), 7);
            assert_eq!(world.query::<&ShooterProjectile>().iter(&world).count(), 0);
        }
    }

    #[test]
    fn pause_freezes_rss_and_restart_clears_runtime_entities_and_score() {
        let mut world = World::new();
        apply_shooter_script(&mut world, SCRIPT).unwrap();
        world.insert_resource(Time::<()>::default());
        world.insert_resource(ButtonInput::<KeyCode>::default());
        world.insert_resource(Score(4));
        world.insert_resource(GameFlow::Paused);
        world.insert_resource(ScriptEditor::default());
        let positions = world
            .query::<&Position>()
            .iter(&world)
            .copied()
            .collect::<Vec<_>>();
        tick_script_gameplay(&mut world);
        assert_eq!(
            positions,
            world
                .query::<&Position>()
                .iter(&world)
                .copied()
                .collect::<Vec<_>>()
        );
        run_shooter_frame_script(&mut world, "use bevy; bevy::Shooter::projectile(\"player\", \"bolt\", 0.0, 0.0, 0.0, 0.0, 8.0); true;").unwrap();
        restart_gameplay(&mut world, SCRIPT).unwrap();
        assert_eq!(world.resource::<Score>().0, 0);
        assert_eq!(*world.resource::<GameFlow>(), GameFlow::Running);
        assert_eq!(world.query::<&Enemy>().iter(&world).count(), 7);
        assert_eq!(world.query::<&ShooterProjectile>().iter(&world).count(), 0);
    }

    fn test_shooter_assets() -> ShooterAssets {
        let image = Handle::<Image>::default();
        ShooterAssets {
            background: image.clone(),
            player_frames: vec![image.clone(), image.clone(), image.clone()],
            enemy_scout: image.clone(),
            enemy_bomber: image.clone(),
            enemy_weaver: image.clone(),
            enemy_tank: image.clone(),
            enemy_sniper: image.clone(),
            enemy_carrier: image.clone(),
            enemy_striker: image.clone(),
            enemy_boss: image.clone(),
            bolt_frames: vec![image.clone(), image.clone()],
            laser_frames: vec![image.clone(), image.clone()],
            player_missile_frames: vec![image.clone(), image.clone()],
            enemy_missile_frames: vec![image.clone(), image.clone()],
            shockwave_frames: vec![image.clone()],
            hit_effect: image.clone(),
            explosion_frames: vec![
                image.clone(),
                image.clone(),
                image.clone(),
                image.clone(),
                image,
            ],
        }
    }

    #[test]
    fn player_and_enemy_bodies_do_not_receive_animation_components() {
        let mut app = App::new();
        app.insert_resource(test_shooter_assets())
            .add_systems(Update, attach_render_components);

        app.world_mut()
            .spawn((Player, Position { x: 0.0, y: -360.0 }));
        app.world_mut().spawn((
            Enemy {
                kind: "scout".to_string(),
            },
            Position { x: 0.0, y: 300.0 },
        ));

        app.update();

        let mut player_frames = app
            .world_mut()
            .query_filtered::<&SpriteFrames, With<PlayerShip>>();
        assert_eq!(player_frames.iter(app.world()).count(), 0);
        let mut enemy_frames = app
            .world_mut()
            .query_filtered::<&SpriteFrames, With<EnemyShip>>();
        assert_eq!(enemy_frames.iter(app.world()).count(), 0);

        let mut player_motion = app
            .world_mut()
            .query_filtered::<&VisualMotion, With<PlayerShip>>();
        assert_eq!(player_motion.iter(app.world()).count(), 0);
        let mut enemy_motion = app
            .world_mut()
            .query_filtered::<&VisualMotion, With<EnemyShip>>();
        assert_eq!(enemy_motion.iter(app.world()).count(), 0);
    }

    #[test]
    fn enemy_kinds_use_distinct_non_airplane_visual_assets() {
        let kinds = [
            "scout", "bomber", "weaver", "tank", "sniper", "carrier", "striker", "boss",
        ];
        let paths = kinds
            .iter()
            .map(|kind| enemy_asset_path_for_kind(kind))
            .collect::<std::collections::BTreeSet<_>>();

        assert_eq!(paths.len(), kinds.len());
        assert!(
            paths
                .iter()
                .all(|path| path.starts_with("shooter/enemy_craft_"))
        );
    }

    #[test]
    fn non_boss_enemy_bodies_use_playable_sprite_sizes() {
        let mut app = App::new();
        app.insert_resource(test_shooter_assets())
            .add_systems(Update, attach_render_components);

        for (index, kind) in [
            "scout", "bomber", "weaver", "tank", "sniper", "carrier", "striker", "boss",
        ]
        .iter()
        .enumerate()
        {
            app.world_mut().spawn((
                Enemy {
                    kind: (*kind).to_string(),
                },
                Position {
                    x: index as f32 * 20.0,
                    y: 300.0,
                },
            ));
        }

        app.update();

        let mut enemies = app
            .world_mut()
            .query_filtered::<(&Enemy, &Sprite, &Transform), With<EnemyShip>>();
        for (enemy, sprite, transform) in enemies.iter(app.world()) {
            let size = sprite
                .custom_size
                .expect("enemy body should clamp source art to a gameplay size");
            let max_edge = size.x.max(size.y);
            if enemy.kind == "boss" {
                assert!(max_edge <= 160.0);
            } else {
                assert!(max_edge <= 115.0, "{} rendered too large", enemy.kind);
            }
            assert_eq!(transform.scale, Vec3::ONE);
        }
    }

    #[test]
    fn default_window_reserves_space_for_script_panel() {
        assert_eq!(
            default_window_size(),
            UVec2::new(
                GAMEPLAY_VIEW_WIDTH + SCRIPT_PANEL_WIDTH.round() as u32,
                GAMEPLAY_VIEW_HEIGHT
            )
        );
        assert!((gameplay_view_fraction() - 720.0 / 1150.0).abs() < f32::EPSILON);
    }

    #[test]
    fn game_camera_frames_full_world_inside_reserved_viewport() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Image>()
            .insert_resource(EguiGlobalSettings::default())
            .add_systems(Startup, setup);
        app.update();

        let mut cameras = app
            .world_mut()
            .query_filtered::<(&Transform, &Projection), With<GameCamera>>();
        let (transform, projection) = cameras
            .single(app.world())
            .expect("game camera should have a projection");

        assert_eq!(transform.translation.x, (LEFT + RIGHT) * 0.5);

        let Projection::Orthographic(projection) = projection else {
            panic!("game camera should use orthographic projection");
        };
        let ScalingMode::AutoMin {
            min_width,
            min_height,
        } = projection.scaling_mode
        else {
            panic!("game camera should frame the full world, not raw window pixels");
        };

        assert_eq!(
            projection.viewport_origin,
            Vec2::new(gameplay_view_fraction() * 0.5, 0.5)
        );
        assert!(min_width * gameplay_view_fraction() >= RIGHT - LEFT + GAMEPLAY_WORLD_PADDING_X);
        assert!(min_height >= TOP - BOTTOM + GAMEPLAY_WORLD_PADDING_Y);
    }

    #[test]
    fn shooter_runtime_images_are_embedded_in_binary() {
        let required_paths = [
            "shooter/background_nebula.png",
            "shooter/player_0.png",
            "shooter/player_1.png",
            "shooter/player_2.png",
            "shooter/bolt_0.png",
            "shooter/bolt_1.png",
            "shooter/laser_0.png",
            "shooter/laser_1.png",
            "shooter/missile_player_0.png",
            "shooter/missile_player_1.png",
            "shooter/missile_enemy_0.png",
            "shooter/missile_enemy_1.png",
            "shooter/shockwave_0.png",
            "shooter/shockwave_1.png",
            "shooter/shockwave_2.png",
            "shooter/shockwave_3.png",
            "shooter/shockwave_4.png",
            "shooter/hit_flash.png",
            "shooter/explosion_0.png",
            "shooter/explosion_1.png",
            "shooter/explosion_2.png",
            "shooter/explosion_3.png",
            "shooter/explosion_4.png",
            "shooter/explosion_5.png",
            "shooter/explosion_6.png",
            "shooter/explosion_7.png",
            "shooter/explosion_8.png",
        ];

        for path in required_paths {
            let bytes = embedded_image_bytes(path).unwrap_or_else(|| {
                panic!("shooter image should be embedded for runtime path {path}")
            });
            assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
        }

        for kind in [
            "scout", "bomber", "weaver", "tank", "sniper", "carrier", "striker", "boss",
        ] {
            assert!(
                embedded_image_bytes(enemy_asset_path_for_kind(kind)).is_some(),
                "enemy asset should be embedded for kind {kind}"
            );
        }
    }

    #[test]
    fn setup_uses_one_camera_for_gameplay_and_egui() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Image>()
            .insert_resource(EguiGlobalSettings::default())
            .add_systems(Startup, setup);
        app.update();

        assert!(
            !app.world()
                .resource::<EguiGlobalSettings>()
                .auto_create_primary_context
        );

        let mut game_cameras = app.world_mut().query_filtered::<Entity, With<GameCamera>>();
        assert_eq!(game_cameras.iter(app.world()).count(), 1);

        let mut egui_game_cameras = app
            .world_mut()
            .query_filtered::<(&Camera, &EguiMultipassSchedule), (With<PrimaryEguiContext>, With<GameCamera>)>();
        let (egui_camera, egui_schedule) = egui_game_cameras
            .single(app.world())
            .expect("egui should render through the gameplay camera");
        assert_eq!(egui_camera.order, 0);
        assert!(egui_camera.viewport.is_none());
        assert_eq!(egui_schedule.0, EguiPrimaryContextPass.intern());
    }
}
