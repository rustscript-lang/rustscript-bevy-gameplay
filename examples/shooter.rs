use bevy::{
    camera::{Viewport, visibility::RenderLayers},
    prelude::*,
    window::PrimaryWindow,
};
use bevy_egui::{
    EguiContexts, EguiGlobalSettings, EguiPlugin, EguiPrimaryContextPass, PrimaryEguiContext, egui,
};
use rustscript_bevy_gameplay::{
    AttackCooldownMs, AttackPower, AttackStyle, Enemy, Health, Player, Position,
    ScriptManagedEnemy, Velocity, apply_shooter_script,
};
use std::f32::consts::FRAC_PI_2;

const SCRIPT: &str = include_str!("../scripts/shooter_game.rss");
const LEFT: f32 = -430.0;
const RIGHT: f32 = 520.0;
const TOP: f32 = 260.0;
const BOTTOM: f32 = -260.0;
const SCRIPT_PANEL_WIDTH: f32 = 430.0;

fn shooter_asset_file_path() -> String {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("assets")
        .to_string_lossy()
        .to_string()
}

fn main() {
    if std::env::args().any(|arg| arg == "--script-smoke") {
        run_script_smoke();
        return;
    }

    App::new()
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "RustScript Bevy Shooter".to_string(),
                        resolution: (1180, 720).into(),
                        ..default()
                    }),
                    ..default()
                })
                .set(AssetPlugin {
                    file_path: shooter_asset_file_path(),
                    ..default()
                }),
        )
        .add_plugins(EguiPlugin::default())
        .insert_resource(ClearColor(Color::srgb(0.02, 0.025, 0.045)))
        .insert_resource(Score(0))
        .insert_resource(ScriptEditor {
            buffer: SCRIPT.to_string(),
            status: "Press Save or wait one frame for initial RustScript apply".to_string(),
            pending_save: true,
        })
        .add_systems(Startup, setup)
        .add_systems(EguiPrimaryContextPass, script_panel)
        .add_systems(
            Update,
            (
                apply_pending_script,
                attach_render_components,
                move_player,
                enemy_motion,
                player_fire,
                enemy_fire,
                guide_homing_projectiles,
                apply_velocity,
                tick_lifetimes,
                update_shockwaves,
                sync_positions,
                animate_sprites,
                animate_visual_motion,
                collisions,
                despawn_out_of_bounds,
            )
                .chain(),
        )
        .run();
}

fn run_script_smoke() {
    let mut world = bevy_ecs::prelude::World::new();
    let summary = apply_shooter_script(&mut world, SCRIPT).expect("shooter script should apply");
    println!(
        "player_hp={}, attack={}:{}, enemies={}",
        summary.player_health,
        summary.player_attack_style,
        summary.player_attack_power,
        summary.enemies_spawned
    );
}

#[derive(Resource)]
struct ScriptEditor {
    buffer: String,
    status: String,
    pending_save: bool,
}

#[derive(Resource, Deref, DerefMut)]
struct Score(u32);

#[derive(Resource, Clone)]
struct ShooterAssets {
    player_frames: Vec<Handle<Image>>,
    enemy_red_frames: Vec<Handle<Image>>,
    enemy_green_frames: Vec<Handle<Image>>,
    enemy_yellow_frames: Vec<Handle<Image>>,
    bolt_frames: Vec<Handle<Image>>,
    laser_frames: Vec<Handle<Image>>,
    player_missile_frames: Vec<Handle<Image>>,
    enemy_missile_frames: Vec<Handle<Image>>,
    shockwave_frames: Vec<Handle<Image>>,
}

impl ShooterAssets {
    fn load(asset_server: &AssetServer) -> Self {
        Self {
            player_frames: load_images(
                asset_server,
                &[
                    "shooter/player_0.png",
                    "shooter/player_1.png",
                    "shooter/player_2.png",
                ],
            ),
            enemy_red_frames: load_images(
                asset_server,
                &[
                    "shooter/enemy_red_0.png",
                    "shooter/enemy_red_1.png",
                    "shooter/enemy_red_2.png",
                ],
            ),
            enemy_green_frames: load_images(
                asset_server,
                &[
                    "shooter/enemy_green_0.png",
                    "shooter/enemy_green_1.png",
                    "shooter/enemy_green_2.png",
                ],
            ),
            enemy_yellow_frames: load_images(
                asset_server,
                &[
                    "shooter/enemy_yellow_0.png",
                    "shooter/enemy_yellow_1.png",
                    "shooter/enemy_yellow_2.png",
                ],
            ),
            bolt_frames: load_images(asset_server, &["shooter/bolt_0.png", "shooter/bolt_1.png"]),
            laser_frames: load_images(
                asset_server,
                &["shooter/laser_0.png", "shooter/laser_1.png"],
            ),
            player_missile_frames: load_images(
                asset_server,
                &[
                    "shooter/missile_player_0.png",
                    "shooter/missile_player_1.png",
                ],
            ),
            enemy_missile_frames: load_images(
                asset_server,
                &["shooter/missile_enemy_0.png", "shooter/missile_enemy_1.png"],
            ),
            shockwave_frames: load_images(
                asset_server,
                &[
                    "shooter/shockwave_0.png",
                    "shooter/shockwave_1.png",
                    "shooter/shockwave_2.png",
                    "shooter/shockwave_3.png",
                    "shooter/shockwave_4.png",
                ],
            ),
        }
    }

    fn enemy_frames(&self, kind: &str) -> Vec<Handle<Image>> {
        match kind {
            "tank" => self.enemy_yellow_frames.clone(),
            "weaver" | "ace" => self.enemy_green_frames.clone(),
            _ => self.enemy_red_frames.clone(),
        }
    }
}

fn load_images(asset_server: &AssetServer, paths: &[&'static str]) -> Vec<Handle<Image>> {
    paths.iter().map(|path| asset_server.load(*path)).collect()
}

#[derive(Component)]
struct PlayerShip;

#[derive(Component)]
struct EnemyShip;

#[derive(Component)]
struct GameCamera;

#[derive(Component)]
struct PlayerBullet;

#[derive(Component)]
struct EnemyBullet;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProjectileOwner {
    Player,
    Enemy,
}

impl ProjectileOwner {
    fn forward_sign(self) -> f32 {
        match self {
            ProjectileOwner::Player => 1.0,
            ProjectileOwner::Enemy => -1.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProjectileKind {
    Bolt,
    Spread,
    Laser,
    HomingMissile,
    Shockwave,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct ProjectileShot {
    kind: ProjectileKind,
    damage: i64,
    velocity: Vec2,
}

#[derive(Component)]
struct Projectile {
    owner: ProjectileOwner,
    damage: i64,
    radius: f32,
    pierces: bool,
}

#[derive(Component)]
struct Homing {
    speed: f32,
    turn_rate: f32,
}

#[derive(Component)]
struct Lifetime {
    elapsed_ms: f32,
    duration_ms: f32,
}

#[derive(Component)]
struct Shockwave {
    start_radius: f32,
    end_radius: f32,
    start_scale: f32,
    end_scale: f32,
}

#[derive(Component, Default)]
struct HitTargets(Vec<Entity>);

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

#[derive(Component)]
struct FireClock {
    elapsed_ms: f32,
}

type AddedEnemyQuery<'w, 's> =
    Query<'w, 's, (Entity, &'static Enemy), (Added<Enemy>, Without<EnemyShip>)>;
type BulletPositionQuery<'w, 's> = Query<'w, 's, (Entity, &'static Position), With<Projectile>>;
type MovingProjectileQuery<'w, 's> =
    Query<'w, 's, (&'static Velocity, &'static mut Position), With<Projectile>>;
type ScriptManagedEnemyPositionQuery<'w, 's> =
    Query<'w, 's, (Entity, &'static Position), (With<Enemy>, With<ScriptManagedEnemy>)>;

fn setup(
    mut commands: Commands,
    mut egui_global_settings: ResMut<EguiGlobalSettings>,
    asset_server: Res<AssetServer>,
) {
    egui_global_settings.auto_create_primary_context = false;
    let assets = ShooterAssets::load(&asset_server);

    commands.spawn((Camera2d, GameCamera));
    commands.spawn((
        PrimaryEguiContext,
        Camera2d,
        RenderLayers::none(),
        Camera {
            order: 1,
            clear_color: ClearColorConfig::None,
            ..default()
        },
    ));
    spawn_starfield(&mut commands, &assets);
    commands.insert_resource(assets);
}

fn spawn_starfield(commands: &mut Commands, assets: &ShooterAssets) {
    for index in 0..48 {
        let image = assets.bolt_frames[index % assets.bolt_frames.len()].clone();
        let mut sprite = Sprite::from_image(image);
        let alpha = if index % 3 == 0 { 0.24 } else { 0.14 };
        sprite.color = Color::srgba(0.62, 0.78, 1.0, alpha);

        let x = LEFT + 40.0 + ((index * 83) % 880) as f32;
        let y = BOTTOM + 20.0 + ((index * 47) % 500) as f32;
        let scale = 0.16 + (index % 5) as f32 * 0.025;
        commands.spawn((
            sprite,
            Transform {
                translation: Vec3::new(x, y, -8.0),
                scale: Vec3::splat(scale),
                rotation: Quat::from_rotation_z((index as f32 * 0.37) % std::f32::consts::TAU),
                ..default()
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
    let Some(source) = ({
        let mut editor = world.resource_mut::<ScriptEditor>();
        if editor.pending_save {
            editor.pending_save = false;
            Some(editor.buffer.clone())
        } else {
            None
        }
    }) else {
        return;
    };

    let result = apply_shooter_script(world, &source);
    let mut editor = world.resource_mut::<ScriptEditor>();
    match result {
        Ok(summary) => {
            editor.status = format!(
                "Applied live: hp {}, attack {} / power {}, enemies {}",
                summary.player_health,
                summary.player_attack_style,
                summary.player_attack_power,
                summary.enemies_spawned
            );
        }
        Err(err) => {
            editor.status = format!("RustScript error: {err}");
        }
    }
}

fn attach_render_components(
    mut commands: Commands,
    assets: Res<ShooterAssets>,
    players: Query<Entity, (Added<Player>, Without<PlayerShip>)>,
    enemies: AddedEnemyQuery,
) {
    for entity in &players {
        let frames = assets.player_frames.clone();
        commands.entity(entity).insert((
            Sprite::from_image(frames[0].clone()),
            Transform {
                translation: Vec3::new(-360.0, 0.0, 2.0),
                scale: Vec3::splat(3.1),
                rotation: Quat::from_rotation_z(-FRAC_PI_2),
            },
            PlayerShip,
            FireClock { elapsed_ms: 0.0 },
            SpriteFrames::new(frames, 120.0),
            VisualMotion {
                base_scale: Vec3::splat(3.1),
                pulse: 0.035,
                spin: 0.0,
                phase: 0.0,
            },
        ));
    }

    for (entity, enemy) in &enemies {
        let frames = assets.enemy_frames(&enemy.kind);
        commands.entity(entity).insert((
            Sprite::from_image(frames[0].clone()),
            Transform {
                translation: Vec3::new(0.0, 0.0, 2.0),
                scale: Vec3::splat(3.0),
                rotation: Quat::from_rotation_z(FRAC_PI_2),
            },
            EnemyShip,
            FireClock { elapsed_ms: 0.0 },
            SpriteFrames::new(frames, 150.0),
            VisualMotion {
                base_scale: Vec3::splat(3.0),
                pulse: 0.045,
                spin: 0.0,
                phase: 1.5,
            },
        ));
    }
}

fn move_player(
    input: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut query: Query<&mut Position, With<Player>>,
) {
    let mut direction = Vec2::ZERO;
    if input.pressed(KeyCode::ArrowLeft) || input.pressed(KeyCode::KeyA) {
        direction.x -= 1.0;
    }
    if input.pressed(KeyCode::ArrowRight) || input.pressed(KeyCode::KeyD) {
        direction.x += 1.0;
    }
    if input.pressed(KeyCode::ArrowUp) || input.pressed(KeyCode::KeyW) {
        direction.y += 1.0;
    }
    if input.pressed(KeyCode::ArrowDown) || input.pressed(KeyCode::KeyS) {
        direction.y -= 1.0;
    }
    if direction.length_squared() > 0.0 {
        direction = direction.normalize();
    }
    for mut position in &mut query {
        position.x = (position.x + direction.x * 300.0 * time.delta_secs()).clamp(LEFT, 130.0);
        position.y = (position.y + direction.y * 300.0 * time.delta_secs()).clamp(BOTTOM, TOP);
    }
}

fn enemy_motion(
    time: Res<Time>,
    mut query: Query<(&AttackStyle, &mut Position, &mut Velocity), With<Enemy>>,
) {
    for (style, mut position, mut velocity) in &mut query {
        velocity.x = match style.0.as_str() {
            "burst" => -65.0,
            "wave" => -45.0,
            _ => -55.0,
        };
        if style.0 == "wave" {
            velocity.y = (time.elapsed_secs() * 3.0 + position.x * 0.02).sin() * 80.0;
        } else {
            velocity.y = 0.0;
        }
        position.x += velocity.x * time.delta_secs();
        position.y = (position.y + velocity.y * time.delta_secs()).clamp(BOTTOM, TOP);
    }
}

fn player_fire(
    mut commands: Commands,
    assets: Res<ShooterAssets>,
    time: Res<Time>,
    mut query: Query<
        (
            &Position,
            &AttackStyle,
            &AttackPower,
            &AttackCooldownMs,
            &mut FireClock,
        ),
        With<Player>,
    >,
) {
    for (position, style, power, cooldown, mut clock) in &mut query {
        clock.elapsed_ms += time.delta_secs() * 1000.0;
        if clock.elapsed_ms < cooldown.0 as f32 {
            continue;
        }
        clock.elapsed_ms = 0.0;
        for shot in projectile_plan(ProjectileOwner::Player, style.0.as_str(), power.0) {
            spawn_projectile(
                &mut commands,
                &assets,
                ProjectileOwner::Player,
                *position,
                shot,
            );
        }
    }
}

fn enemy_fire(
    mut commands: Commands,
    assets: Res<ShooterAssets>,
    time: Res<Time>,
    mut query: Query<(&Position, &AttackStyle, &AttackPower, &mut FireClock), With<Enemy>>,
) {
    for (position, style, power, mut clock) in &mut query {
        clock.elapsed_ms += time.delta_secs() * 1000.0;
        let cooldown = if style.0 == "burst" { 700.0 } else { 1100.0 };
        if clock.elapsed_ms < cooldown {
            continue;
        }
        clock.elapsed_ms = 0.0;
        for shot in projectile_plan(ProjectileOwner::Enemy, style.0.as_str(), power.0) {
            spawn_projectile(
                &mut commands,
                &assets,
                ProjectileOwner::Enemy,
                *position,
                shot,
            );
        }
    }
}

fn projectile_plan(owner: ProjectileOwner, style: &str, power: i64) -> Vec<ProjectileShot> {
    let sign = owner.forward_sign();
    match style {
        "spread" => vec![
            ProjectileShot {
                kind: ProjectileKind::Spread,
                damage: power,
                velocity: Vec2::new(460.0 * sign, 90.0),
            },
            ProjectileShot {
                kind: ProjectileKind::Spread,
                damage: power,
                velocity: Vec2::new(500.0 * sign, 0.0),
            },
            ProjectileShot {
                kind: ProjectileKind::Spread,
                damage: power,
                velocity: Vec2::new(460.0 * sign, -90.0),
            },
            ProjectileShot {
                kind: ProjectileKind::HomingMissile,
                damage: power + 5,
                velocity: Vec2::new(330.0 * sign, 0.0),
            },
            ProjectileShot {
                kind: ProjectileKind::Shockwave,
                damage: (power / 2).max(4),
                velocity: Vec2::ZERO,
            },
        ],
        "laser" => vec![
            ProjectileShot {
                kind: ProjectileKind::Laser,
                damage: power * 2,
                velocity: Vec2::new(760.0 * sign, 0.0),
            },
            ProjectileShot {
                kind: ProjectileKind::HomingMissile,
                damage: power + 7,
                velocity: Vec2::new(360.0 * sign, 0.0),
            },
        ],
        "burst" => vec![
            ProjectileShot {
                kind: ProjectileKind::Spread,
                damage: power,
                velocity: Vec2::new(260.0 * sign, 80.0),
            },
            ProjectileShot {
                kind: ProjectileKind::Spread,
                damage: power,
                velocity: Vec2::new(260.0 * sign, -80.0),
            },
            ProjectileShot {
                kind: ProjectileKind::HomingMissile,
                damage: power + 4,
                velocity: Vec2::new(300.0 * sign, 0.0),
            },
        ],
        "wave" => vec![
            ProjectileShot {
                kind: ProjectileKind::Bolt,
                damage: power + 2,
                velocity: Vec2::new(240.0 * sign, 120.0),
            },
            ProjectileShot {
                kind: ProjectileKind::Shockwave,
                damage: power,
                velocity: Vec2::ZERO,
            },
        ],
        "missile" | "homing" => vec![ProjectileShot {
            kind: ProjectileKind::HomingMissile,
            damage: power + 8,
            velocity: Vec2::new(360.0 * sign, 0.0),
        }],
        "shockwave" => vec![ProjectileShot {
            kind: ProjectileKind::Shockwave,
            damage: power,
            velocity: Vec2::ZERO,
        }],
        _ => vec![ProjectileShot {
            kind: ProjectileKind::Bolt,
            damage: power,
            velocity: Vec2::new(520.0 * sign, 0.0),
        }],
    }
}

fn spawn_projectile(
    commands: &mut Commands,
    assets: &ShooterAssets,
    owner: ProjectileOwner,
    origin: Position,
    shot: ProjectileShot,
) {
    let spec = projectile_spec(shot.kind);
    let spawn_x = origin.x + owner.forward_sign() * spec.spawn_offset;
    let spawn_position = Position {
        x: spawn_x,
        y: origin.y,
    };
    let frames = projectile_frames(assets, owner, shot.kind);
    let mut entity = commands.spawn((
        Sprite::from_image(frames[0].clone()),
        Transform {
            translation: Vec3::new(spawn_position.x, spawn_position.y, 3.0),
            scale: Vec3::splat(spec.scale),
            rotation: projectile_rotation(shot.velocity, owner),
        },
        spawn_position,
        Velocity {
            x: shot.velocity.x,
            y: shot.velocity.y,
        },
        Projectile {
            owner,
            damage: shot.damage,
            radius: spec.radius,
            pierces: spec.pierces,
        },
        SpriteFrames::new(frames, spec.frame_ms),
        VisualMotion {
            base_scale: Vec3::splat(spec.scale),
            pulse: spec.pulse,
            spin: spec.spin * owner.forward_sign(),
            phase: 0.0,
        },
    ));

    match owner {
        ProjectileOwner::Player => {
            entity.insert(PlayerBullet);
        }
        ProjectileOwner::Enemy => {
            entity.insert(EnemyBullet);
        }
    }

    if spec.pierces {
        entity.insert(HitTargets::default());
    }
    if let Some(duration_ms) = spec.lifetime_ms {
        entity.insert(Lifetime {
            elapsed_ms: 0.0,
            duration_ms,
        });
    }
    if shot.kind == ProjectileKind::HomingMissile {
        entity.insert(Homing {
            speed: spec.speed,
            turn_rate: 3.2,
        });
    }
    if shot.kind == ProjectileKind::Shockwave {
        entity.insert(Shockwave {
            start_radius: 18.0,
            end_radius: 96.0,
            start_scale: 0.36,
            end_scale: 2.0,
        });
    }
}

#[derive(Clone, Copy)]
struct ProjectileSpec {
    radius: f32,
    scale: f32,
    speed: f32,
    spawn_offset: f32,
    frame_ms: f32,
    pulse: f32,
    spin: f32,
    pierces: bool,
    lifetime_ms: Option<f32>,
}

fn projectile_spec(kind: ProjectileKind) -> ProjectileSpec {
    match kind {
        ProjectileKind::Bolt => ProjectileSpec {
            radius: 16.0,
            scale: 1.25,
            speed: 520.0,
            spawn_offset: 34.0,
            frame_ms: 90.0,
            pulse: 0.08,
            spin: 0.0,
            pierces: false,
            lifetime_ms: None,
        },
        ProjectileKind::Spread => ProjectileSpec {
            radius: 14.0,
            scale: 1.05,
            speed: 500.0,
            spawn_offset: 34.0,
            frame_ms: 80.0,
            pulse: 0.1,
            spin: 0.0,
            pierces: false,
            lifetime_ms: None,
        },
        ProjectileKind::Laser => ProjectileSpec {
            radius: 18.0,
            scale: 1.55,
            speed: 760.0,
            spawn_offset: 38.0,
            frame_ms: 55.0,
            pulse: 0.12,
            spin: 0.0,
            pierces: true,
            lifetime_ms: None,
        },
        ProjectileKind::HomingMissile => ProjectileSpec {
            radius: 20.0,
            scale: 0.58,
            speed: 360.0,
            spawn_offset: 42.0,
            frame_ms: 110.0,
            pulse: 0.04,
            spin: 0.0,
            pierces: false,
            lifetime_ms: None,
        },
        ProjectileKind::Shockwave => ProjectileSpec {
            radius: 18.0,
            scale: 0.36,
            speed: 0.0,
            spawn_offset: 22.0,
            frame_ms: 70.0,
            pulse: 0.0,
            spin: 0.35,
            pierces: true,
            lifetime_ms: Some(620.0),
        },
    }
}

fn projectile_frames(
    assets: &ShooterAssets,
    owner: ProjectileOwner,
    kind: ProjectileKind,
) -> Vec<Handle<Image>> {
    match kind {
        ProjectileKind::Bolt | ProjectileKind::Spread => assets.bolt_frames.clone(),
        ProjectileKind::Laser => assets.laser_frames.clone(),
        ProjectileKind::HomingMissile => match owner {
            ProjectileOwner::Player => assets.player_missile_frames.clone(),
            ProjectileOwner::Enemy => assets.enemy_missile_frames.clone(),
        },
        ProjectileKind::Shockwave => assets.shockwave_frames.clone(),
    }
}

fn projectile_rotation(velocity: Vec2, owner: ProjectileOwner) -> Quat {
    if velocity.length_squared() == 0.0 {
        return Quat::from_rotation_z(-FRAC_PI_2 * owner.forward_sign());
    }
    Quat::from_rotation_z(velocity.y.atan2(velocity.x) - FRAC_PI_2)
}

fn guide_homing_projectiles(
    time: Res<Time>,
    mut projectiles: Query<(&Position, &mut Velocity, &Projectile, &Homing)>,
    enemies: Query<&Position, With<Enemy>>,
    players: Query<&Position, (With<Player>, Without<Enemy>)>,
) {
    for (position, mut velocity, projectile, homing) in &mut projectiles {
        let target = match projectile.owner {
            ProjectileOwner::Player => nearest_target(*position, enemies.iter()),
            ProjectileOwner::Enemy => nearest_target(*position, players.iter()),
        };
        let Some(target) = target else {
            continue;
        };
        let updated = homing_velocity_step(
            Velocity {
                x: velocity.x,
                y: velocity.y,
            },
            *position,
            target,
            homing.speed,
            homing.turn_rate * time.delta_secs(),
        );
        velocity.x = updated.x;
        velocity.y = updated.y;
    }
}

fn nearest_target<'a>(
    origin: Position,
    positions: impl Iterator<Item = &'a Position>,
) -> Option<Position> {
    positions.copied().min_by(|a, b| {
        distance_squared(origin, *a)
            .partial_cmp(&distance_squared(origin, *b))
            .unwrap_or(std::cmp::Ordering::Equal)
    })
}

fn homing_velocity_step(
    current: Velocity,
    origin: Position,
    target: Position,
    speed: f32,
    turn_amount: f32,
) -> Velocity {
    let to_target = Vec2::new(target.x - origin.x, target.y - origin.y);
    if to_target.length_squared() == 0.0 {
        return current;
    }

    let current = Vec2::new(current.x, current.y);
    let desired = to_target.normalize() * speed;
    let next = if current.length_squared() == 0.0 {
        desired
    } else {
        current.lerp(desired, turn_amount.clamp(0.0, 1.0))
    };
    Velocity {
        x: next.x,
        y: next.y,
    }
}

fn apply_velocity(time: Res<Time>, mut query: MovingProjectileQuery) {
    for (velocity, mut position) in &mut query {
        position.x += velocity.x * time.delta_secs();
        position.y += velocity.y * time.delta_secs();
    }
}

fn tick_lifetimes(
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

fn update_shockwaves(
    mut query: Query<(
        &mut Projectile,
        &Shockwave,
        &Lifetime,
        &mut Transform,
        Option<&mut VisualMotion>,
    )>,
) {
    for (mut projectile, shockwave, lifetime, mut transform, motion) in &mut query {
        let radius = shockwave_radius_at(
            lifetime.elapsed_ms,
            lifetime.duration_ms,
            shockwave.start_radius,
            shockwave.end_radius,
        );
        projectile.radius = radius;
        let t = (lifetime.elapsed_ms / lifetime.duration_ms).clamp(0.0, 1.0);
        let scale = shockwave.start_scale.lerp(shockwave.end_scale, t);
        let scale = Vec3::splat(scale);
        transform.scale = scale;
        if let Some(mut motion) = motion {
            motion.base_scale = scale;
        }
    }
}

fn shockwave_radius_at(age_ms: f32, duration_ms: f32, start_radius: f32, end_radius: f32) -> f32 {
    let t = if duration_ms <= 0.0 {
        1.0
    } else {
        (age_ms / duration_ms).clamp(0.0, 1.0)
    };
    start_radius.lerp(end_radius, t)
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

fn collisions(
    mut commands: Commands,
    mut score: ResMut<Score>,
    mut projectiles: Query<(Entity, &Position, &Projectile, Option<&mut HitTargets>)>,
    mut enemies: Query<(Entity, &Position, &mut Health), (With<Enemy>, Without<Player>)>,
    mut players: Query<(&Position, &mut Health), (With<Player>, Without<Enemy>)>,
) {
    for (projectile_entity, projectile_pos, projectile, mut hit_targets) in &mut projectiles {
        match projectile.owner {
            ProjectileOwner::Player => {
                for (enemy_entity, enemy_pos, mut health) in &mut enemies {
                    if !overlaps(*projectile_pos, *enemy_pos, projectile.radius) {
                        continue;
                    }
                    if already_hit(&mut hit_targets, enemy_entity) {
                        continue;
                    }

                    health.0 -= projectile.damage;
                    if !projectile.pierces {
                        commands.entity(projectile_entity).despawn();
                    }
                    if health.0 <= 0 {
                        commands.entity(enemy_entity).despawn();
                        **score += 1;
                    }
                    if !projectile.pierces {
                        break;
                    }
                }
            }
            ProjectileOwner::Enemy => {
                let Some((player_pos, mut player_health)) = players.iter_mut().next() else {
                    continue;
                };
                if !overlaps(*projectile_pos, *player_pos, projectile.radius) {
                    continue;
                }
                if already_hit(&mut hit_targets, Entity::PLACEHOLDER) {
                    continue;
                }

                player_health.0 -= projectile.damage;
                if !projectile.pierces {
                    commands.entity(projectile_entity).despawn();
                }
            }
        }
    }
}

fn already_hit(hit_targets: &mut Option<Mut<HitTargets>>, target: Entity) -> bool {
    let Some(hit_targets) = hit_targets.as_mut() else {
        return false;
    };
    if hit_targets.0.contains(&target) {
        return true;
    }
    hit_targets.0.push(target);
    false
}

fn overlaps(a: Position, b: Position, radius: f32) -> bool {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    dx * dx + dy * dy <= radius * radius
}

fn distance_squared(a: Position, b: Position) -> f32 {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    dx * dx + dy * dy
}

fn gameplay_viewport_size(window_size: UVec2, panel_width: f32, scale_factor: f32) -> UVec2 {
    let panel_width = (panel_width.max(0.0) * scale_factor.max(0.0)).round() as u32;
    UVec2::new(
        window_size.x.saturating_sub(panel_width).max(1),
        window_size.y.max(1),
    )
}

fn set_gameplay_viewport(
    camera: &mut Camera,
    window_size: UVec2,
    panel_width: f32,
    scale_factor: f32,
) {
    camera.viewport = Some(Viewport {
        physical_position: UVec2::ZERO,
        physical_size: gameplay_viewport_size(window_size, panel_width, scale_factor),
        ..default()
    });
}

fn despawn_out_of_bounds(
    mut commands: Commands,
    bullets: BulletPositionQuery,
    enemies: ScriptManagedEnemyPositionQuery,
) {
    for (entity, position) in &bullets {
        if position.x < LEFT - 160.0 || position.x > RIGHT + 160.0 || position.y.abs() > TOP + 120.0
        {
            commands.entity(entity).despawn();
        }
    }
    for (entity, position) in &enemies {
        if position.x < LEFT - 120.0 {
            commands.entity(entity).despawn();
        }
    }
}

fn script_panel(
    mut contexts: EguiContexts,
    mut camera: Single<&mut Camera, With<GameCamera>>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut editor: ResMut<ScriptEditor>,
    score: Res<Score>,
    player: Query<(&Health, &AttackStyle, &AttackPower), With<Player>>,
    enemies: Query<&Enemy>,
) -> bevy::prelude::Result {
    let ctx = contexts.ctx_mut()?;
    let panel_response = egui::SidePanel::right("rustscript_panel")
        .resizable(true)
        .default_width(SCRIPT_PANEL_WIDTH)
        .show(ctx, |ui| {
            ui.heading("Live RustScript");
            ui.label("Edit the script, then Save. The running Bevy world updates in place.");
            ui.separator();
            if let Some((health, style, power)) = player.iter().next() {
                ui.label(format!(
                    "Player: hp {} / attack {} / power {}",
                    health.0, style.0, power.0
                ));
            }
            ui.label(format!(
                "Enemies: {}   Score: {}",
                enemies.iter().count(),
                **score
            ));
            ui.label(&editor.status);
            ui.separator();
            ui.add(
                egui::TextEdit::multiline(&mut editor.buffer)
                    .code_editor()
                    .desired_rows(26)
                    .desired_width(f32::INFINITY),
            );
            if ui.button("Save and apply now").clicked() {
                editor.pending_save = true;
            }
        });
    set_gameplay_viewport(
        &mut camera,
        window.physical_size(),
        panel_response.response.rect.width(),
        window.scale_factor(),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collisions_system_accepts_disjoint_player_and_enemy_health_queries() {
        let mut app = App::new();
        app.insert_resource(Score(0))
            .add_systems(Update, collisions);

        app.world_mut()
            .spawn((Player, Position { x: 0.0, y: 0.0 }, Health(100)));
        app.world_mut().spawn((
            Enemy {
                kind: "grunt".to_string(),
            },
            Position { x: 50.0, y: 0.0 },
            Health(30),
        ));

        app.update();
    }

    #[test]
    fn gameplay_viewport_reserves_space_for_script_panel() {
        assert_eq!(
            gameplay_viewport_size(UVec2::new(1180, 720), SCRIPT_PANEL_WIDTH, 1.0),
            UVec2::new(750, 720)
        );
    }

    #[test]
    fn shooter_asset_file_path_points_at_repo_assets() {
        let asset_path = std::path::PathBuf::from(shooter_asset_file_path());
        assert!(asset_path.join("shooter/player_0.png").is_file());
        assert!(asset_path.join("shooter/shockwave_0.png").is_file());
    }

    #[test]
    fn projectile_plan_shares_advanced_projectiles_between_sides() {
        let player_spread = projectile_plan(ProjectileOwner::Player, "spread", 10);
        assert!(
            player_spread
                .iter()
                .any(|shot| shot.kind == ProjectileKind::HomingMissile)
        );
        assert!(
            player_spread
                .iter()
                .any(|shot| shot.kind == ProjectileKind::Shockwave)
        );

        let enemy_burst = projectile_plan(ProjectileOwner::Enemy, "burst", 10);
        assert!(
            enemy_burst
                .iter()
                .any(|shot| shot.kind == ProjectileKind::HomingMissile)
        );

        let enemy_wave = projectile_plan(ProjectileOwner::Enemy, "wave", 10);
        assert!(
            enemy_wave
                .iter()
                .any(|shot| shot.kind == ProjectileKind::Shockwave)
        );

        let player_missile = projectile_plan(ProjectileOwner::Player, "missile", 10);
        let enemy_missile = projectile_plan(ProjectileOwner::Enemy, "missile", 10);
        assert_eq!(player_missile[0].kind, enemy_missile[0].kind);
        assert!(player_missile[0].velocity.x > 0.0);
        assert!(enemy_missile[0].velocity.x < 0.0);
    }

    #[test]
    fn homing_velocity_turns_toward_target() {
        let velocity = homing_velocity_step(
            Velocity { x: 100.0, y: 0.0 },
            Position { x: 0.0, y: 0.0 },
            Position { x: 0.0, y: 100.0 },
            200.0,
            0.5,
        );

        assert!(velocity.y > 0.0);
        assert!(velocity.x > 0.0);
    }

    #[test]
    fn shockwave_radius_expands_with_age() {
        let start = shockwave_radius_at(0.0, 600.0, 18.0, 96.0);
        let middle = shockwave_radius_at(300.0, 600.0, 18.0, 96.0);
        let end = shockwave_radius_at(600.0, 600.0, 18.0, 96.0);

        assert_eq!(start, 18.0);
        assert!(middle > start);
        assert_eq!(end, 96.0);
    }

    #[test]
    fn setup_uses_separate_cameras_for_gameplay_and_egui() {
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

        let mut egui_cameras = app
            .world_mut()
            .query_filtered::<&Camera, (With<PrimaryEguiContext>, Without<GameCamera>)>();
        let egui_camera = egui_cameras
            .single(app.world())
            .expect("egui should render through its own camera");
        assert_eq!(egui_camera.order, 1);
        assert!(egui_camera.viewport.is_none());
    }
}
