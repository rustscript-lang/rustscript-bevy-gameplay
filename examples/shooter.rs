use bevy::prelude::*;
use bevy_egui::{EguiContexts, EguiPlugin, EguiPrimaryContextPass, egui};
use rustscript_bevy_gameplay::{
    AttackCooldownMs, AttackPower, AttackStyle, Enemy, Health, Player, Position,
    ScriptManagedEnemy, Velocity, apply_shooter_script,
};

const SCRIPT: &str = include_str!("../scripts/shooter_game.rss");
const LEFT: f32 = -430.0;
const RIGHT: f32 = 520.0;
const TOP: f32 = 260.0;
const BOTTOM: f32 = -260.0;

fn main() {
    if std::env::args().any(|arg| arg == "--script-smoke") {
        run_script_smoke();
        return;
    }

    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "RustScript Bevy Shooter".to_string(),
                resolution: (1180, 720).into(),
                ..default()
            }),
            ..default()
        }))
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
                apply_velocity,
                sync_positions,
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

#[derive(Component)]
struct PlayerShip;

#[derive(Component)]
struct EnemyShip;

#[derive(Component)]
struct PlayerBullet {
    damage: i64,
}

#[derive(Component)]
struct EnemyBullet {
    damage: i64,
}

#[derive(Component)]
struct FireClock {
    elapsed_ms: f32,
}

type AddedEnemyQuery<'w, 's> =
    Query<'w, 's, (Entity, &'static Enemy), (Added<Enemy>, Without<EnemyShip>)>;
type BulletPositionQuery<'w, 's> =
    Query<'w, 's, (Entity, &'static Position), Or<(With<PlayerBullet>, With<EnemyBullet>)>>;
type MovingProjectileQuery<'w, 's> =
    Query<'w, 's, (&'static Velocity, &'static mut Position), (Without<Player>, Without<Enemy>)>;
type ScriptManagedEnemyPositionQuery<'w, 's> =
    Query<'w, 's, (Entity, &'static Position), (With<Enemy>, With<ScriptManagedEnemy>)>;

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);
    commands.spawn((
        Sprite::from_color(Color::srgb(0.05, 0.08, 0.14), Vec2::ONE),
        Transform {
            translation: Vec3::new(30.0, 0.0, -10.0),
            scale: Vec3::new(940.0, 560.0, 1.0),
            ..default()
        },
    ));
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
    players: Query<Entity, (Added<Player>, Without<PlayerShip>)>,
    enemies: AddedEnemyQuery,
) {
    for entity in &players {
        commands.entity(entity).insert((
            Sprite::from_color(Color::srgb(0.1, 0.9, 1.0), Vec2::new(42.0, 24.0)),
            Transform::from_xyz(-360.0, 0.0, 2.0),
            PlayerShip,
            FireClock { elapsed_ms: 0.0 },
        ));
    }

    for (entity, enemy) in &enemies {
        commands.entity(entity).insert((
            Sprite::from_color(enemy_color(&enemy.kind), Vec2::new(38.0, 28.0)),
            Transform::from_xyz(0.0, 0.0, 2.0),
            EnemyShip,
            FireClock { elapsed_ms: 0.0 },
        ));
    }
}

fn enemy_color(kind: &str) -> Color {
    match kind {
        "bomber" => Color::srgb(1.0, 0.35, 0.15),
        "weaver" => Color::srgb(0.85, 0.45, 1.0),
        "tank" => Color::srgb(0.9, 0.75, 0.25),
        "ace" => Color::srgb(1.0, 0.1, 0.5),
        _ => Color::srgb(0.95, 0.2, 0.25),
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
        match style.0.as_str() {
            "spread" => {
                spawn_player_bullet(&mut commands, *position, power.0, 460.0, 90.0);
                spawn_player_bullet(&mut commands, *position, power.0, 500.0, 0.0);
                spawn_player_bullet(&mut commands, *position, power.0, 460.0, -90.0);
            }
            "laser" => spawn_player_bullet(&mut commands, *position, power.0 * 2, 760.0, 0.0),
            _ => spawn_player_bullet(&mut commands, *position, power.0, 520.0, 0.0),
        }
    }
}

fn spawn_player_bullet(commands: &mut Commands, position: Position, damage: i64, vx: f32, vy: f32) {
    commands.spawn((
        Sprite::from_color(Color::srgb(0.45, 1.0, 0.95), Vec2::new(18.0, 5.0)),
        Transform::from_xyz(position.x + 32.0, position.y, 3.0),
        Position {
            x: position.x + 32.0,
            y: position.y,
        },
        Velocity { x: vx, y: vy },
        PlayerBullet { damage },
    ));
}

fn enemy_fire(
    mut commands: Commands,
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
        match style.0.as_str() {
            "burst" => {
                spawn_enemy_bullet(&mut commands, *position, power.0, -260.0, 80.0);
                spawn_enemy_bullet(&mut commands, *position, power.0, -260.0, -80.0);
            }
            "wave" => spawn_enemy_bullet(&mut commands, *position, power.0 + 2, -240.0, 120.0),
            _ => spawn_enemy_bullet(&mut commands, *position, power.0, -300.0, 0.0),
        }
    }
}

fn spawn_enemy_bullet(commands: &mut Commands, position: Position, damage: i64, vx: f32, vy: f32) {
    commands.spawn((
        Sprite::from_color(Color::srgb(1.0, 0.25, 0.1), Vec2::new(14.0, 6.0)),
        Transform::from_xyz(position.x - 28.0, position.y, 3.0),
        Position {
            x: position.x - 28.0,
            y: position.y,
        },
        Velocity { x: vx, y: vy },
        EnemyBullet { damage },
    ));
}

fn apply_velocity(time: Res<Time>, mut query: MovingProjectileQuery) {
    for (velocity, mut position) in &mut query {
        position.x += velocity.x * time.delta_secs();
        position.y += velocity.y * time.delta_secs();
    }
}

fn sync_positions(mut query: Query<(&Position, &mut Transform)>) {
    for (position, mut transform) in &mut query {
        transform.translation.x = position.x;
        transform.translation.y = position.y;
    }
}

fn collisions(
    mut commands: Commands,
    mut score: ResMut<Score>,
    player_bullets: Query<(Entity, &Position, &PlayerBullet)>,
    mut enemies: Query<(Entity, &Position, &mut Health), With<Enemy>>,
    enemy_bullets: Query<(Entity, &Position, &EnemyBullet)>,
    mut players: Query<(&Position, &mut Health), With<Player>>,
) {
    for (bullet_entity, bullet_pos, bullet) in &player_bullets {
        for (enemy_entity, enemy_pos, mut health) in &mut enemies {
            if overlaps(*bullet_pos, *enemy_pos, 28.0) {
                health.0 -= bullet.damage;
                commands.entity(bullet_entity).despawn();
                if health.0 <= 0 {
                    commands.entity(enemy_entity).despawn();
                    **score += 1;
                }
                break;
            }
        }
    }

    if let Some((player_pos, mut player_health)) = players.iter_mut().next() {
        for (bullet_entity, bullet_pos, bullet) in &enemy_bullets {
            if overlaps(*bullet_pos, *player_pos, 24.0) {
                player_health.0 -= bullet.damage;
                commands.entity(bullet_entity).despawn();
            }
        }
    }
}

fn overlaps(a: Position, b: Position, radius: f32) -> bool {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    dx * dx + dy * dy <= radius * radius
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
    mut editor: ResMut<ScriptEditor>,
    score: Res<Score>,
    player: Query<(&Health, &AttackStyle, &AttackPower), With<Player>>,
    enemies: Query<&Enemy>,
) -> bevy::prelude::Result {
    let ctx = contexts.ctx_mut()?;
    egui::SidePanel::right("rustscript_panel")
        .resizable(true)
        .default_width(430.0)
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
    Ok(())
}
