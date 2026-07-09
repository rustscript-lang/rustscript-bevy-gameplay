use bevy::{
    camera::{OrthographicProjection, Projection, ScalingMode},
    prelude::*,
};
use bevy_egui::{
    EguiContexts, EguiGlobalSettings, EguiMultipassSchedule, EguiPlugin, EguiPrimaryContextPass,
    PrimaryEguiContext, egui,
};
use rustscript_bevy_gameplay::{
    AttackCooldownMs, AttackPower, AttackStyle, Enemy, Health, Player, PlayerProjectileLoadout,
    Position, RewardItem, ScriptManagedEnemy, ShooterSpawnRules, Velocity, apply_shooter_script,
    tick_shooter_spawn_rules,
};
use std::f32::consts::FRAC_PI_2;
use vm::{SourceError, SourceMap, compile_source};

const SCRIPT: &str = include_str!("../scripts/shooter_game.rss");
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

fn shooter_asset_file_path() -> String {
    let manifest_assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
    let current_exe = std::env::current_exe().ok();
    let current_dir = std::env::current_dir().ok();
    resolve_shooter_asset_file_path(
        current_exe.as_deref(),
        current_dir.as_deref(),
        &manifest_assets,
    )
    .to_string_lossy()
    .to_string()
}

fn resolve_shooter_asset_file_path(
    current_exe: Option<&std::path::Path>,
    current_dir: Option<&std::path::Path>,
    manifest_assets: &std::path::Path,
) -> std::path::PathBuf {
    let mut candidates = Vec::new();
    if let Some(exe_dir) = current_exe.and_then(std::path::Path::parent) {
        candidates.push(exe_dir.join("assets"));
    }
    if let Some(cwd) = current_dir {
        candidates.push(cwd.join("assets"));
    }
    candidates.push(manifest_assets.to_path_buf());

    candidates
        .into_iter()
        .find(|path| shooter_asset_dir_has_required_files(path))
        .unwrap_or_else(|| manifest_assets.to_path_buf())
}

fn shooter_asset_dir_has_required_files(path: &std::path::Path) -> bool {
    path.join("shooter/background_nebula.png").is_file()
        && path.join("shooter/player_0.png").is_file()
        && path.join("shooter/enemy_craft_scout.png").is_file()
}

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
                        resolution: default_window_size().into(),
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
        .insert_resource(ClearColor(Color::srgb(0.055, 0.085, 0.14)))
        .insert_resource(Score(0))
        .insert_resource(SpawnRuleProgress::default())
        .insert_resource(GameFlow::Running)
        .insert_resource(ScriptEditor {
            buffer: SCRIPT.to_string(),
            status: "Press Save or wait one frame for initial RustScript apply".to_string(),
            diagnostics: Vec::new(),
            pending_save: true,
            pending_restart: false,
            jit_enabled: true,
            jit_trace_count: 0,
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
                run_scripted_spawn_rules,
                update_game_flow_after_health,
                collect_rewards,
                despawn_out_of_bounds,
            )
                .chain(),
        )
        .run();
}

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
}

#[derive(Resource)]
struct ScriptEditor {
    buffer: String,
    status: String,
    diagnostics: Vec<ScriptDiagnostic>,
    pending_save: bool,
    pending_restart: bool,
    jit_enabled: bool,
    jit_trace_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ScriptDiagnostic {
    line: usize,
    start_col: usize,
    end_col: usize,
    message: String,
    source_line: String,
    start_byte: usize,
    end_byte: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScriptTokenKind {
    Keyword,
    Type,
    Number,
    String,
    Comment,
    Function,
    HostApi,
    Operator,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ScriptToken {
    start: usize,
    end: usize,
    kind: ScriptTokenKind,
}

impl ScriptToken {
    fn text<'a>(self, source: &'a str) -> &'a str {
        &source[self.start..self.end]
    }
}

#[derive(Resource, Deref, DerefMut)]
struct Score(u32);

#[derive(Resource, Default)]
struct SpawnRuleProgress {
    last_score: u32,
}

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

const SCRIPT_KEYWORDS: &[&str] = &[
    "as", "break", "continue", "else", "false", "fn", "for", "if", "let", "match", "mut", "null",
    "pub", "return", "struct", "true", "use", "while",
];

const SCRIPT_TYPES: &[&str] = &[
    "array", "bool", "bytes", "float", "int", "map", "number", "string",
];

const SHOOTER_HOST_APIS: &[&str] = &[
    "bevy::Shooter::set_player_health",
    "bevy::Shooter::set_player_attack",
    "bevy::Shooter::set_player_projectiles",
    "bevy::Shooter::spawn_enemy",
    "bevy::Shooter::spawn_reward",
    "bevy::Shooter::spawn_enemy_every",
    "bevy::Shooter::spawn_reward_every",
    "bevy::Shooter::spawn_enemy_after_kills",
];

fn rustscript_highlight_tokens(source: &str) -> Vec<ScriptToken> {
    let mut tokens = Vec::new();
    let bytes = source.as_bytes();
    let mut cursor = 0usize;

    while cursor < bytes.len() {
        let ch = bytes[cursor] as char;
        if ch.is_ascii_whitespace() {
            cursor += 1;
            continue;
        }

        if source[cursor..].starts_with("//") {
            let end = source[cursor..]
                .find('\n')
                .map(|offset| cursor + offset)
                .unwrap_or(source.len());
            tokens.push(ScriptToken {
                start: cursor,
                end,
                kind: ScriptTokenKind::Comment,
            });
            cursor = end;
            continue;
        }

        if source[cursor..].starts_with("/*") {
            let end = source[cursor + 2..]
                .find("*/")
                .map(|offset| cursor + 2 + offset + 2)
                .unwrap_or(source.len());
            tokens.push(ScriptToken {
                start: cursor,
                end,
                kind: ScriptTokenKind::Comment,
            });
            cursor = end;
            continue;
        }

        if ch == '"' || source[cursor..].starts_with("b\"") {
            let start = cursor;
            if source[cursor..].starts_with("b\"") {
                cursor += 2;
            } else {
                cursor += 1;
            }
            let mut escaped = false;
            while cursor < bytes.len() {
                let current = bytes[cursor] as char;
                cursor += 1;
                if escaped {
                    escaped = false;
                    continue;
                }
                if current == '\\' {
                    escaped = true;
                    continue;
                }
                if current == '"' {
                    break;
                }
            }
            tokens.push(ScriptToken {
                start,
                end: cursor,
                kind: ScriptTokenKind::String,
            });
            continue;
        }

        if ch.is_ascii_digit() {
            let start = cursor;
            cursor += 1;
            while cursor < bytes.len() {
                let current = bytes[cursor] as char;
                if current.is_ascii_digit() || current == '.' {
                    cursor += 1;
                } else {
                    break;
                }
            }
            tokens.push(ScriptToken {
                start,
                end: cursor,
                kind: ScriptTokenKind::Number,
            });
            continue;
        }

        if is_ident_start(ch) {
            let start = cursor;
            cursor += 1;
            while cursor < bytes.len() {
                let current = bytes[cursor] as char;
                if is_ident_continue(current) {
                    cursor += 1;
                    continue;
                }
                if source[cursor..].starts_with("::") {
                    cursor += 2;
                    continue;
                }
                break;
            }
            let text = &source[start..cursor];
            let kind = if SHOOTER_HOST_APIS.contains(&text) {
                Some(ScriptTokenKind::HostApi)
            } else if SCRIPT_KEYWORDS.contains(&text) {
                Some(ScriptTokenKind::Keyword)
            } else if SCRIPT_TYPES.contains(&text) {
                Some(ScriptTokenKind::Type)
            } else if next_non_ws_starts_with(source, cursor, '(') {
                Some(ScriptTokenKind::Function)
            } else {
                None
            };
            if let Some(kind) = kind {
                tokens.push(ScriptToken {
                    start,
                    end: cursor,
                    kind,
                });
            }
            continue;
        }

        if "=+-*/%<>!&|?:;,.(){}[]".contains(ch) {
            tokens.push(ScriptToken {
                start: cursor,
                end: cursor + 1,
                kind: ScriptTokenKind::Operator,
            });
        }
        cursor += 1;
    }

    tokens
}

fn is_ident_start(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphabetic()
}

fn is_ident_continue(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphanumeric()
}

fn next_non_ws_starts_with(source: &str, cursor: usize, needle: char) -> bool {
    source[cursor..]
        .chars()
        .find(|ch| !ch.is_ascii_whitespace())
        == Some(needle)
}

fn script_compile_diagnostics(source: &str, fallback_error: &str) -> Vec<ScriptDiagnostic> {
    match compile_source(source) {
        Ok(_) => {
            if fallback_error.trim().is_empty() {
                Vec::new()
            } else {
                vec![fallback_script_diagnostic(source, fallback_error)]
            }
        }
        Err(SourceError::Parse(err)) => {
            let mut source_map = SourceMap::new();
            let source_id = source_map.add_source("<editor>", source.to_string());
            let err = err.with_line_span_from_source(&source_map, source_id);
            let line = preferred_parse_diagnostic_line(source, err.line.max(1), &err.message);
            let span = if line == err.line {
                err.span.or_else(|| source_map.line_span(source_id, line))
            } else {
                source_map.line_span(source_id, line)
            };
            vec![script_diagnostic_from_parts(
                &source_map,
                source_id,
                line,
                span.map(|span| (span.lo, span.hi)),
                err.message,
            )]
        }
        Err(SourceError::Compile(err)) => {
            let mut source_map = SourceMap::new();
            let source_id = source_map.add_source("<editor>", source.to_string());
            let line = err.line().unwrap_or(1).max(1);
            let span = source_map.line_span(source_id, line);
            vec![script_diagnostic_from_parts(
                &source_map,
                source_id,
                line,
                span.map(|span| (span.lo, span.hi)),
                err.diagnostic_message(),
            )]
        }
    }
}

fn preferred_parse_diagnostic_line(source: &str, reported_line: usize, message: &str) -> usize {
    if reported_line <= 1 || !message.contains("expected") {
        return reported_line;
    }

    let Some(previous_line) = source.lines().nth(reported_line - 2) else {
        return reported_line;
    };
    if previous_line.matches('(').count() > previous_line.matches(')').count() {
        return reported_line - 1;
    }
    reported_line
}

fn script_diagnostic_from_parts(
    source_map: &SourceMap,
    source_id: u32,
    line: usize,
    span: Option<(usize, usize)>,
    message: String,
) -> ScriptDiagnostic {
    let source_line = source_map
        .file(source_id)
        .and_then(|file| file.line_text(line))
        .unwrap_or_default()
        .to_string();
    let (start_byte, end_byte) = span.unwrap_or_else(|| {
        source_map
            .line_span(source_id, line)
            .map(|span| (span.lo, span.hi))
            .unwrap_or((0, 0))
    });
    let (start_line, start_col) = source_map
        .line_col_for_offset(source_id, start_byte)
        .unwrap_or((line, 1));
    let (_, end_col) = source_map
        .line_col_for_offset(source_id, end_byte)
        .unwrap_or((start_line, start_col + 1));
    ScriptDiagnostic {
        line: start_line,
        start_col,
        end_col: end_col.max(start_col + 1),
        message,
        source_line,
        start_byte,
        end_byte: end_byte.max(start_byte + 1),
    }
}

fn fallback_script_diagnostic(source: &str, fallback_error: &str) -> ScriptDiagnostic {
    let line = parse_line_number(fallback_error).unwrap_or(1);
    let source_line = source
        .lines()
        .nth(line.saturating_sub(1))
        .unwrap_or_default()
        .to_string();
    let start_byte = line_start_byte(source, line).unwrap_or(0);
    let end_byte = start_byte + source_line.len();
    ScriptDiagnostic {
        line,
        start_col: 1,
        end_col: source_line.chars().count().max(1) + 1,
        message: fallback_error.to_string(),
        source_line,
        start_byte,
        end_byte: end_byte.max(start_byte + 1),
    }
}

fn parse_line_number(text: &str) -> Option<usize> {
    let marker = "line ";
    let start = text.find(marker)? + marker.len();
    let digits = text[start..]
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .collect::<String>();
    digits.parse().ok()
}

fn line_start_byte(source: &str, line: usize) -> Option<usize> {
    if line == 0 {
        return None;
    }
    if line == 1 {
        return Some(0);
    }
    let mut current_line = 1usize;
    for (index, ch) in source.char_indices() {
        if ch == '\n' {
            current_line += 1;
            if current_line == line {
                return Some(index + 1);
            }
        }
    }
    None
}

fn rustscript_layout_job(source: &str, diagnostics: &[ScriptDiagnostic]) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    let mut cursor = 0usize;
    for token in rustscript_highlight_tokens(source) {
        if cursor < token.start {
            append_script_text(
                &mut job,
                &source[cursor..token.start],
                plain_format(diagnostics),
            );
        }
        append_script_text(
            &mut job,
            token.text(source),
            token_format(token.kind, token.start, token.end, diagnostics),
        );
        cursor = token.end;
    }
    if cursor < source.len() {
        append_script_text(&mut job, &source[cursor..], plain_format(diagnostics));
    }
    job
}

fn append_script_text(job: &mut egui::text::LayoutJob, text: &str, format: egui::TextFormat) {
    job.append(text, 0.0, format);
}

fn token_format(
    kind: ScriptTokenKind,
    start: usize,
    end: usize,
    diagnostics: &[ScriptDiagnostic],
) -> egui::TextFormat {
    let mut format = plain_format(diagnostics);
    format.color = match kind {
        ScriptTokenKind::Keyword => egui::Color32::from_rgb(117, 190, 255),
        ScriptTokenKind::Type => egui::Color32::from_rgb(106, 214, 179),
        ScriptTokenKind::Number => egui::Color32::from_rgb(255, 206, 112),
        ScriptTokenKind::String => egui::Color32::from_rgb(245, 155, 112),
        ScriptTokenKind::Comment => egui::Color32::from_rgb(130, 148, 166),
        ScriptTokenKind::Function => egui::Color32::from_rgb(209, 184, 255),
        ScriptTokenKind::HostApi => egui::Color32::from_rgb(120, 230, 238),
        ScriptTokenKind::Operator => egui::Color32::from_rgb(182, 192, 210),
    };
    if diagnostics
        .iter()
        .any(|diagnostic| ranges_overlap(start, end, diagnostic.start_byte, diagnostic.end_byte))
    {
        format.background = egui::Color32::from_rgba_unmultiplied(120, 24, 36, 115);
    }
    format
}

fn plain_format(diagnostics: &[ScriptDiagnostic]) -> egui::TextFormat {
    let _ = diagnostics;
    egui::TextFormat {
        font_id: egui::FontId::monospace(13.0),
        color: egui::Color32::from_rgb(220, 228, 238),
        ..Default::default()
    }
}

fn ranges_overlap(a_start: usize, a_end: usize, b_start: usize, b_end: usize) -> bool {
    a_start < b_end && b_start < a_end
}

fn render_script_diagnostics(ui: &mut egui::Ui, diagnostics: &[ScriptDiagnostic]) {
    if diagnostics.is_empty() {
        return;
    }

    ui.add_space(6.0);
    for diagnostic in diagnostics {
        ui.colored_label(
            egui::Color32::from_rgb(255, 120, 135),
            format!(
                "line {}:{} {}",
                diagnostic.line, diagnostic.start_col, diagnostic.message
            ),
        );
        ui.monospace(format!(
            "{:>3} | {}",
            diagnostic.line, diagnostic.source_line
        ));
        let pointer_width = diagnostic
            .end_col
            .saturating_sub(diagnostic.start_col)
            .max(1);
        ui.monospace(format!(
            "    | {}{}",
            " ".repeat(diagnostic.start_col.saturating_sub(1)),
            "^".repeat(pointer_width)
        ));
    }
}

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

impl ShooterAssets {
    fn load(asset_server: &AssetServer) -> Self {
        Self {
            background: asset_server.load("shooter/background_nebula.png"),
            player_frames: load_images(
                asset_server,
                &[
                    "shooter/player_0.png",
                    "shooter/player_1.png",
                    "shooter/player_2.png",
                ],
            ),
            enemy_scout: asset_server.load(enemy_asset_path_for_kind("scout")),
            enemy_bomber: asset_server.load(enemy_asset_path_for_kind("bomber")),
            enemy_weaver: asset_server.load(enemy_asset_path_for_kind("weaver")),
            enemy_tank: asset_server.load(enemy_asset_path_for_kind("tank")),
            enemy_sniper: asset_server.load(enemy_asset_path_for_kind("sniper")),
            enemy_carrier: asset_server.load(enemy_asset_path_for_kind("carrier")),
            enemy_striker: asset_server.load(enemy_asset_path_for_kind("striker")),
            enemy_boss: asset_server.load(enemy_asset_path_for_kind("boss")),
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
            hit_effect: asset_server.load("shooter/hit_flash.png"),
            explosion_frames: load_images(
                asset_server,
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

fn load_images(asset_server: &AssetServer, paths: &[&'static str]) -> Vec<Handle<Image>> {
    paths.iter().map(|path| asset_server.load(*path)).collect()
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
    Plasma,
    Flak,
    Rail,
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
struct EnemyManeuver {
    elapsed_secs: f32,
    anchor_x: f32,
}

impl EnemyManeuver {
    fn new(anchor_x: f32) -> Self {
        Self {
            elapsed_secs: 0.0,
            anchor_x,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct EnemyMotionProfile {
    descent_speed: f32,
    lateral_speed: f32,
    secondary_lateral_speed: f32,
    frequency: f32,
    phase: f32,
    drift_speed: f32,
}

#[derive(Component)]
struct FireClock {
    elapsed_ms: f32,
}

type AddedEnemyQuery<'w, 's> =
    Query<'w, 's, (Entity, &'static Enemy, &'static Position), (Added<Enemy>, Without<EnemyShip>)>;
type AddedRewardQuery<'w, 's> = Query<
    'w,
    's,
    (Entity, &'static RewardItem, &'static Position),
    (Added<RewardItem>, Without<RewardPickup>),
>;
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
    let Some((source, restart)) = ({
        let mut editor = world.resource_mut::<ScriptEditor>();
        if editor.pending_restart {
            editor.pending_restart = false;
            editor.pending_save = false;
            Some((editor.buffer.clone(), true))
        } else if editor.pending_save {
            editor.pending_save = false;
            Some((editor.buffer.clone(), false))
        } else {
            None
        }
    }) else {
        return;
    };

    let result = if restart {
        restart_gameplay(world, &source)
    } else {
        apply_shooter_script(world, &source)
    };
    if result.is_ok() {
        reset_spawn_rule_progress(world);
    }
    let mut editor = world.resource_mut::<ScriptEditor>();
    match result {
        Ok(summary) => {
            let verb = if restart { "Restarted" } else { "Applied live" };
            editor.diagnostics.clear();
            editor.jit_enabled = summary.jit.enabled;
            editor.jit_trace_count = summary.jit.trace_count;
            editor.status = format!(
                "{verb}: hp {}, attack {} / power {}, enemies {}",
                summary.player_health,
                summary.player_attack_style,
                summary.player_attack_power,
                summary.enemies_spawned
            );
        }
        Err(err) => {
            editor.diagnostics = script_compile_diagnostics(&source, &err);
            editor.status = "RustScript has diagnostics below".to_string();
        }
    }
}

fn restart_gameplay(
    world: &mut World,
    source: &str,
) -> Result<rustscript_bevy_gameplay::ShooterSummary, String> {
    despawn_entities_with::<Projectile>(world);
    despawn_entities_with::<Enemy>(world);
    despawn_entities_with::<RewardItem>(world);
    reset_player_runtime(world);

    if let Some(mut score) = world.get_resource_mut::<Score>() {
        score.0 = 0;
    } else {
        world.insert_resource(Score(0));
    }
    if let Some(mut flow) = world.get_resource_mut::<GameFlow>() {
        *flow = GameFlow::Running;
    } else {
        world.insert_resource(GameFlow::Running);
    }

    apply_shooter_script(world, source)
}

fn reset_spawn_rule_progress(world: &mut World) {
    let score = world
        .get_resource::<Score>()
        .map(|score| score.0)
        .unwrap_or(0);
    if let Some(mut progress) = world.get_resource_mut::<SpawnRuleProgress>() {
        progress.last_score = score;
    } else {
        world.insert_resource(SpawnRuleProgress { last_score: score });
    }
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
        if let Some(mut clock) = world.get_mut::<FireClock>(entity) {
            clock.elapsed_ms = 0.0;
        }
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
            FireClock { elapsed_ms: 0.0 },
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
            FireClock { elapsed_ms: 0.0 },
            EnemyManeuver::new(position.x),
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

fn move_player(
    input: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    flow: Res<GameFlow>,
    mut query: Query<(&Health, &mut Position), With<Player>>,
) {
    if !flow.is_running() {
        return;
    }

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
    for (health, mut position) in &mut query {
        if health.0 <= 0 {
            continue;
        }
        position.x =
            (position.x + direction.x * 300.0 * time.delta_secs()).clamp(LEFT + 42.0, RIGHT - 42.0);
        position.y = (position.y + direction.y * 300.0 * time.delta_secs())
            .clamp(BOTTOM + 54.0, TOP - 120.0);
    }
}

fn enemy_motion(
    time: Res<Time>,
    flow: Res<GameFlow>,
    mut query: Query<
        (
            &Enemy,
            &AttackStyle,
            &mut Position,
            &mut Velocity,
            Option<&mut EnemyManeuver>,
        ),
        With<Enemy>,
    >,
) {
    if !flow.is_running() {
        return;
    }

    for (enemy, style, mut position, mut velocity, maneuver) in &mut query {
        let profile = enemy_motion_profile(enemy.kind.as_str(), style.0.as_str());
        let mut elapsed_secs = time.elapsed_secs();
        let mut anchor_x = position.x;
        if let Some(mut maneuver) = maneuver {
            maneuver.elapsed_secs += time.delta_secs();
            elapsed_secs = maneuver.elapsed_secs;
            anchor_x = maneuver.anchor_x;
        }
        let weave = (elapsed_secs * profile.frequency + profile.phase + anchor_x * 0.011).sin();
        let counter = (elapsed_secs * profile.frequency * 0.47 + profile.phase * 0.5).cos();
        let dive = if enemy.kind == "striker" {
            (elapsed_secs * 4.0 + profile.phase).sin().max(0.0) * 34.0
        } else {
            0.0
        };

        velocity.x = profile.drift_speed
            + weave * profile.lateral_speed
            + counter * profile.secondary_lateral_speed;
        velocity.y = -(profile.descent_speed + dive);
        position.y += velocity.y * time.delta_secs();
        position.x = (position.x + velocity.x * time.delta_secs()).clamp(LEFT + 32.0, RIGHT - 32.0);
    }
}

fn enemy_motion_profile(kind: &str, style: &str) -> EnemyMotionProfile {
    let mut profile = match kind {
        "bomber" => EnemyMotionProfile {
            descent_speed: 42.0,
            lateral_speed: 44.0,
            secondary_lateral_speed: 10.0,
            frequency: 1.15,
            phase: 0.9,
            drift_speed: -8.0,
        },
        "weaver" | "ace" => EnemyMotionProfile {
            descent_speed: 48.0,
            lateral_speed: 92.0,
            secondary_lateral_speed: 22.0,
            frequency: 2.8,
            phase: 1.8,
            drift_speed: 0.0,
        },
        "tank" => EnemyMotionProfile {
            descent_speed: 30.0,
            lateral_speed: 8.0,
            secondary_lateral_speed: 0.0,
            frequency: 0.8,
            phase: 2.4,
            drift_speed: 0.0,
        },
        "sniper" => EnemyMotionProfile {
            descent_speed: 34.0,
            lateral_speed: 118.0,
            secondary_lateral_speed: 18.0,
            frequency: 0.95,
            phase: 0.35,
            drift_speed: 0.0,
        },
        "carrier" => EnemyMotionProfile {
            descent_speed: 36.0,
            lateral_speed: 58.0,
            secondary_lateral_speed: 28.0,
            frequency: 1.45,
            phase: 2.1,
            drift_speed: 6.0,
        },
        "striker" => EnemyMotionProfile {
            descent_speed: 82.0,
            lateral_speed: 70.0,
            secondary_lateral_speed: 26.0,
            frequency: 2.35,
            phase: 1.25,
            drift_speed: 18.0,
        },
        "boss" => EnemyMotionProfile {
            descent_speed: 22.0,
            lateral_speed: 76.0,
            secondary_lateral_speed: 34.0,
            frequency: 0.62,
            phase: 2.8,
            drift_speed: 0.0,
        },
        _ => EnemyMotionProfile {
            descent_speed: 58.0,
            lateral_speed: 28.0,
            secondary_lateral_speed: 8.0,
            frequency: 1.8,
            phase: 0.15,
            drift_speed: 0.0,
        },
    };

    match style {
        "burst" => {
            profile.descent_speed += 10.0;
            profile.frequency *= 0.88;
        }
        "wave" => {
            profile.descent_speed = (profile.descent_speed - 5.0).max(18.0);
            profile.lateral_speed += 28.0;
            profile.frequency *= 1.18;
        }
        "missile" | "homing" => {
            profile.descent_speed -= 3.0;
            profile.drift_speed += 10.0;
        }
        "rail" | "laser" => {
            profile.descent_speed -= 6.0;
            profile.lateral_speed += 18.0;
        }
        _ => {}
    }

    profile
}

fn player_fire(
    mut commands: Commands,
    assets: Res<ShooterAssets>,
    time: Res<Time>,
    flow: Res<GameFlow>,
    mut query: Query<
        (
            &Position,
            &AttackStyle,
            &AttackPower,
            &AttackCooldownMs,
            &PlayerProjectileLoadout,
            &Health,
            &mut FireClock,
        ),
        With<Player>,
    >,
) {
    if !flow.is_running() {
        return;
    }

    for (position, style, power, cooldown, loadout, health, mut clock) in &mut query {
        if health.0 <= 0 {
            continue;
        }
        clock.elapsed_ms += time.delta_secs() * 1000.0;
        if clock.elapsed_ms < cooldown.0 as f32 {
            continue;
        }
        clock.elapsed_ms = 0.0;
        for shot in projectile_plan(
            ProjectileOwner::Player,
            style.0.as_str(),
            power.0,
            Some(loadout),
        ) {
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
    flow: Res<GameFlow>,
    mut query: Query<(
        &Enemy,
        &Position,
        &AttackStyle,
        &AttackPower,
        &mut FireClock,
    )>,
) {
    if !flow.is_running() {
        return;
    }

    for (enemy, position, style, power, mut clock) in &mut query {
        clock.elapsed_ms += time.delta_secs() * 1000.0;
        let cooldown = enemy_fire_cooldown_ms(enemy.kind.as_str(), style.0.as_str());
        if clock.elapsed_ms < cooldown {
            continue;
        }
        clock.elapsed_ms = 0.0;
        for shot in enemy_projectile_plan(enemy.kind.as_str(), style.0.as_str(), power.0) {
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

fn enemy_fire_cooldown_ms(kind: &str, style: &str) -> f32 {
    match kind {
        "sniper" => 1850.0,
        "striker" => 980.0,
        "carrier" => 1350.0,
        _ if style == "burst" => 1200.0,
        _ => 1500.0,
    }
}

fn projectile_plan(
    owner: ProjectileOwner,
    style: &str,
    power: i64,
    loadout: Option<&PlayerProjectileLoadout>,
) -> Vec<ProjectileShot> {
    if owner == ProjectileOwner::Player {
        let kind = loadout.map(|value| value.kind.as_str()).unwrap_or(style);
        let count = loadout.map(|value| value.count).unwrap_or(1);
        return player_projectile_plan(kind, count, power);
    }

    enemy_projectile_plan("", style, power)
}

fn enemy_projectile_plan(kind: &str, style: &str, power: i64) -> Vec<ProjectileShot> {
    let enemy_sign = ProjectileOwner::Enemy.forward_sign();
    match kind {
        "sniper" => {
            return vec![ProjectileShot {
                kind: ProjectileKind::Rail,
                damage: power + 12,
                velocity: Vec2::new(0.0, 900.0 * enemy_sign),
            }];
        }
        "carrier" => {
            return vec![
                ProjectileShot {
                    kind: ProjectileKind::HomingMissile,
                    damage: power + 7,
                    velocity: Vec2::new(-45.0, 340.0 * enemy_sign),
                },
                ProjectileShot {
                    kind: ProjectileKind::Plasma,
                    damage: power + 5,
                    velocity: Vec2::new(45.0, 430.0 * enemy_sign),
                },
            ];
        }
        "striker" => {
            return vec![
                ProjectileShot {
                    kind: ProjectileKind::Flak,
                    damage: power,
                    velocity: Vec2::new(-110.0, 640.0 * enemy_sign),
                },
                ProjectileShot {
                    kind: ProjectileKind::Flak,
                    damage: power,
                    velocity: Vec2::new(0.0, 700.0 * enemy_sign),
                },
                ProjectileShot {
                    kind: ProjectileKind::Flak,
                    damage: power,
                    velocity: Vec2::new(110.0, 640.0 * enemy_sign),
                },
            ];
        }
        _ => {}
    }

    match style {
        "spread" => vec![
            ProjectileShot {
                kind: ProjectileKind::Spread,
                damage: power,
                velocity: Vec2::new(-90.0, 460.0 * enemy_sign),
            },
            ProjectileShot {
                kind: ProjectileKind::Spread,
                damage: power,
                velocity: Vec2::new(0.0, 500.0 * enemy_sign),
            },
            ProjectileShot {
                kind: ProjectileKind::Spread,
                damage: power,
                velocity: Vec2::new(90.0, 460.0 * enemy_sign),
            },
            ProjectileShot {
                kind: ProjectileKind::HomingMissile,
                damage: power + 5,
                velocity: Vec2::new(0.0, 330.0 * enemy_sign),
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
                velocity: Vec2::new(0.0, 760.0 * enemy_sign),
            },
            ProjectileShot {
                kind: ProjectileKind::HomingMissile,
                damage: power + 7,
                velocity: Vec2::new(0.0, 360.0 * enemy_sign),
            },
        ],
        "burst" => vec![
            ProjectileShot {
                kind: ProjectileKind::Spread,
                damage: power,
                velocity: Vec2::new(-80.0, 260.0 * enemy_sign),
            },
            ProjectileShot {
                kind: ProjectileKind::Spread,
                damage: power,
                velocity: Vec2::new(80.0, 260.0 * enemy_sign),
            },
            ProjectileShot {
                kind: ProjectileKind::HomingMissile,
                damage: power + 4,
                velocity: Vec2::new(0.0, 300.0 * enemy_sign),
            },
        ],
        "wave" => vec![
            ProjectileShot {
                kind: ProjectileKind::Bolt,
                damage: power + 2,
                velocity: Vec2::new(120.0, 240.0 * enemy_sign),
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
            velocity: Vec2::new(0.0, 360.0 * enemy_sign),
        }],
        "shockwave" => vec![ProjectileShot {
            kind: ProjectileKind::Shockwave,
            damage: power,
            velocity: Vec2::ZERO,
        }],
        "plasma" => vec![ProjectileShot {
            kind: ProjectileKind::Plasma,
            damage: power + 6,
            velocity: Vec2::new(0.0, 430.0 * enemy_sign),
        }],
        "flak" => vec![ProjectileShot {
            kind: ProjectileKind::Flak,
            damage: power,
            velocity: Vec2::new(0.0, 520.0 * enemy_sign),
        }],
        "rail" => vec![ProjectileShot {
            kind: ProjectileKind::Rail,
            damage: power + 10,
            velocity: Vec2::new(0.0, 880.0 * enemy_sign),
        }],
        _ => vec![ProjectileShot {
            kind: ProjectileKind::Bolt,
            damage: power,
            velocity: Vec2::new(0.0, 520.0 * enemy_sign),
        }],
    }
}

fn player_projectile_plan(kind: &str, count: i64, power: i64) -> Vec<ProjectileShot> {
    let count = count.clamp(1, 5) as usize;
    let offsets = lateral_speeds(count);
    let shot_kind = match kind {
        "spread" => ProjectileKind::Spread,
        "laser" => ProjectileKind::Laser,
        "missile" | "homing" => ProjectileKind::HomingMissile,
        "shockwave" => ProjectileKind::Shockwave,
        "plasma" => ProjectileKind::Plasma,
        "flak" => ProjectileKind::Flak,
        "rail" => ProjectileKind::Rail,
        _ => ProjectileKind::Bolt,
    };

    offsets
        .into_iter()
        .map(|lateral| {
            let (damage, speed_y) = match shot_kind {
                ProjectileKind::Bolt => (power, 560.0),
                ProjectileKind::Spread => (power, 520.0),
                ProjectileKind::Laser => (power + 4, 760.0),
                ProjectileKind::HomingMissile => (power + 6, 360.0),
                ProjectileKind::Shockwave => ((power / 2).max(4), 0.0),
                ProjectileKind::Plasma => (power + 5, 470.0),
                ProjectileKind::Flak => (power + 1, 610.0),
                ProjectileKind::Rail => (power + 10, 920.0),
            };
            ProjectileShot {
                kind: shot_kind,
                damage,
                velocity: Vec2::new(lateral, speed_y),
            }
        })
        .collect()
}

fn lateral_speeds(count: usize) -> Vec<f32> {
    match count {
        1 => vec![0.0],
        2 => vec![-55.0, 55.0],
        3 => vec![-95.0, 0.0, 95.0],
        4 => vec![-120.0, -40.0, 40.0, 120.0],
        _ => vec![-135.0, -70.0, 0.0, 70.0, 135.0],
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
    let spawn_position = Position {
        x: origin.x,
        y: origin.y + owner.forward_sign() * spec.spawn_offset,
    };
    let frames = projectile_frames(assets, owner, shot.kind);
    let mut sprite = Sprite::from_image(frames[0].clone());
    sprite.color = projectile_color(owner, shot.kind);
    let mut entity = commands.spawn((
        sprite,
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
        ProjectileKind::Plasma => ProjectileSpec {
            radius: 22.0,
            scale: 1.45,
            speed: 470.0,
            spawn_offset: 38.0,
            frame_ms: 70.0,
            pulse: 0.14,
            spin: 0.08,
            pierces: false,
            lifetime_ms: None,
        },
        ProjectileKind::Flak => ProjectileSpec {
            radius: 13.0,
            scale: 0.95,
            speed: 610.0,
            spawn_offset: 32.0,
            frame_ms: 65.0,
            pulse: 0.08,
            spin: 0.18,
            pierces: false,
            lifetime_ms: None,
        },
        ProjectileKind::Rail => ProjectileSpec {
            radius: 15.0,
            scale: 1.75,
            speed: 920.0,
            spawn_offset: 42.0,
            frame_ms: 45.0,
            pulse: 0.06,
            spin: 0.0,
            pierces: true,
            lifetime_ms: None,
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

fn guide_homing_projectiles(
    time: Res<Time>,
    flow: Res<GameFlow>,
    mut projectiles: Query<(&Position, &mut Velocity, &Projectile, &Homing)>,
    enemies: Query<&Position, With<Enemy>>,
    players: Query<&Position, (With<Player>, Without<Enemy>)>,
) {
    if !flow.is_running() {
        return;
    }

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
    let forward_sign = current.y.signum();
    let desired = forward_homing_direction(to_target, forward_sign) * speed;
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

fn forward_homing_direction(to_target: Vec2, forward_sign: f32) -> Vec2 {
    let target_direction = to_target.normalize();
    if forward_sign == 0.0 || target_direction.y.signum() == forward_sign {
        return target_direction;
    }

    let side = target_direction.x;
    let forward = forward_sign * (1.0 - side.abs()).max(0.08);
    Vec2::new(side, forward).normalize()
}

fn apply_velocity(time: Res<Time>, flow: Res<GameFlow>, mut query: MovingProjectileQuery) {
    if !flow.is_running() {
        return;
    }

    for (velocity, mut position) in &mut query {
        position.x += velocity.x * time.delta_secs();
        position.y += velocity.y * time.delta_secs();
    }
}

fn tick_lifetimes(
    mut commands: Commands,
    time: Res<Time>,
    flow: Res<GameFlow>,
    mut query: Query<(Entity, &mut Lifetime)>,
) {
    if !flow.is_running() {
        return;
    }

    for (entity, mut lifetime) in &mut query {
        lifetime.elapsed_ms += time.delta_secs() * 1000.0;
        if lifetime.elapsed_ms >= lifetime.duration_ms {
            commands.entity(entity).despawn();
        }
    }
}

fn update_shockwaves(
    flow: Res<GameFlow>,
    mut query: Query<(
        &mut Projectile,
        &Shockwave,
        &Lifetime,
        &mut Transform,
        Option<&mut VisualMotion>,
    )>,
) {
    if !flow.is_running() {
        return;
    }

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
    assets: Res<ShooterAssets>,
    mut score: ResMut<Score>,
    flow: Res<GameFlow>,
    mut projectiles: Query<(Entity, &Position, &Projectile, Option<&mut HitTargets>)>,
    mut enemies: Query<(Entity, &Enemy, &Position, &mut Health), Without<Player>>,
    mut players: Query<(&Position, &mut Health), (With<Player>, Without<Enemy>)>,
) {
    if !flow.is_running() {
        return;
    }

    for (projectile_entity, projectile_pos, projectile, mut hit_targets) in &mut projectiles {
        match projectile.owner {
            ProjectileOwner::Player => {
                for (enemy_entity, enemy, enemy_pos, mut health) in &mut enemies {
                    if !overlaps(*projectile_pos, *enemy_pos, projectile.radius) {
                        continue;
                    }
                    if already_hit(&mut hit_targets, enemy_entity) {
                        continue;
                    }

                    health.0 -= projectile.damage;
                    spawn_hit_effect(&mut commands, &assets, *enemy_pos, ProjectileOwner::Player);
                    if !projectile.pierces {
                        commands.entity(projectile_entity).despawn();
                    }
                    if health.0 <= 0 {
                        commands.spawn((reward_drop_for_enemy(enemy.kind.as_str()), *enemy_pos));
                        spawn_explosion(&mut commands, &assets, *enemy_pos, enemy.kind.as_str());
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

                player_health.0 = (player_health.0 - projectile.damage).max(0);
                spawn_hit_effect(&mut commands, &assets, *player_pos, ProjectileOwner::Enemy);
                if !projectile.pierces {
                    commands.entity(projectile_entity).despawn();
                }
            }
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

fn reward_drop_for_enemy(kind: &str) -> RewardItem {
    match kind {
        "tank" => RewardItem {
            kind: "health".to_string(),
            amount: 20,
        },
        "boss" => RewardItem {
            kind: "health".to_string(),
            amount: 35,
        },
        "bomber" | "carrier" => RewardItem {
            kind: "bullets".to_string(),
            amount: 2,
        },
        _ => RewardItem {
            kind: "bullets".to_string(),
            amount: 1,
        },
    }
}

fn run_scripted_spawn_rules(world: &mut World) {
    let is_running = world
        .get_resource::<GameFlow>()
        .map(|flow| flow.is_running())
        .unwrap_or(true);
    if !is_running {
        return;
    }

    let delta_ms = world
        .get_resource::<Time>()
        .map(|time| (time.delta_secs() * 1000.0).round() as i64)
        .unwrap_or(0);
    let score = world
        .get_resource::<Score>()
        .map(|score| score.0)
        .unwrap_or(0);
    let kills_delta = {
        let mut progress = world.resource_mut::<SpawnRuleProgress>();
        let kills_delta = score.saturating_sub(progress.last_score);
        progress.last_score = score;
        kills_delta
    };

    tick_shooter_spawn_rules(world, delta_ms, kills_delta as i64);
}

fn update_game_flow_after_health(
    mut flow: ResMut<GameFlow>,
    players: Query<&Health, With<Player>>,
) {
    if *flow == GameFlow::GameOver {
        return;
    }
    if players.iter().any(|health| health.0 <= 0) {
        *flow = GameFlow::GameOver;
    }
}

fn collect_rewards(
    mut commands: Commands,
    flow: Res<GameFlow>,
    mut players: Query<(&Position, &mut Health, &mut PlayerProjectileLoadout), With<Player>>,
    rewards: Query<(Entity, &Position, &RewardItem), Without<Player>>,
) {
    if !flow.is_running() {
        return;
    }

    let Some((player_position, mut health, mut loadout)) = players.iter_mut().next() else {
        return;
    };

    for (entity, reward_position, reward) in &rewards {
        if !overlaps(*player_position, *reward_position, 42.0) {
            continue;
        }

        match reward.kind.as_str() {
            "health" | "hp" => {
                health.0 = (health.0 + reward.amount).clamp(0, PLAYER_MAX_HEALTH);
            }
            "bullets" | "bullet" | "ammo" => {
                loadout.count = (loadout.count + reward.amount).clamp(1, 5);
            }
            _ => {}
        }

        commands.entity(entity).despawn();
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

fn despawn_out_of_bounds(
    mut commands: Commands,
    flow: Res<GameFlow>,
    bullets: BulletPositionQuery,
    enemies: ScriptManagedEnemyPositionQuery,
) {
    if !flow.is_running() {
        return;
    }

    for (entity, position) in &bullets {
        if position.x < LEFT - 140.0
            || position.x > RIGHT + 140.0
            || position.y < BOTTOM - 160.0
            || position.y > TOP + 160.0
        {
            commands.entity(entity).despawn();
        }
    }
    for (entity, position) in &enemies {
        if position.y < BOTTOM - 120.0 {
            commands.entity(entity).despawn();
        }
    }
}

fn jit_status_label(enabled: bool, trace_count: usize) -> String {
    let state = if enabled { "on" } else { "off" };
    format!("JIT: {state}   traces: {trace_count}")
}

fn script_panel(
    mut contexts: EguiContexts,
    mut editor: ResMut<ScriptEditor>,
    mut flow: ResMut<GameFlow>,
    score: Res<Score>,
    player: Query<
        (
            &Health,
            &AttackStyle,
            &AttackPower,
            &PlayerProjectileLoadout,
        ),
        With<Player>,
    >,
    enemies: Query<&Enemy>,
) -> bevy::prelude::Result {
    let ctx = contexts.ctx_mut()?;
    if *flow == GameFlow::GameOver {
        egui::Area::new(egui::Id::new("game_over_overlay"))
            .anchor(
                egui::Align2::CENTER_CENTER,
                egui::vec2(-(SCRIPT_PANEL_WIDTH * 0.5), 0.0),
            )
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(egui::Color32::from_rgba_unmultiplied(8, 14, 24, 215))
                    .stroke(egui::Stroke::new(
                        1.0,
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

    if let Some((health, _, _, loadout)) = player.iter().next() {
        let ratio = (health.0.max(0) as f32 / PLAYER_MAX_HEALTH as f32).clamp(0.0, 1.0);
        egui::Area::new(egui::Id::new("shooter_hud"))
            .anchor(egui::Align2::LEFT_TOP, egui::vec2(16.0, 16.0))
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(egui::Color32::from_rgba_unmultiplied(8, 16, 28, 190))
                    .stroke(egui::Stroke::new(
                        1.0,
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
            ui.heading("Live RustScript");
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("Restart").clicked() {
                    editor.pending_restart = true;
                }

                let pause_label = if *flow == GameFlow::Paused {
                    "Resume"
                } else {
                    "Pause"
                };
                if ui
                    .add_enabled(*flow != GameFlow::GameOver, egui::Button::new(pause_label))
                    .clicked()
                {
                    *flow = if *flow == GameFlow::Paused {
                        GameFlow::Running
                    } else {
                        GameFlow::Paused
                    };
                }
            });
            ui.label(format!("State: {}", flow.label()));
            ui.separator();
            if let Some((health, style, power, loadout)) = player.iter().next() {
                ui.label(format!(
                    "Player: hp {} / attack {} / power {} / {} x{}",
                    health.0.max(0),
                    style.0,
                    power.0,
                    loadout.kind,
                    loadout.count
                ));
            }
            ui.label(format!(
                "Enemies: {}   Score: {}",
                enemies.iter().count(),
                **score
            ));
            ui.label(jit_status_label(editor.jit_enabled, editor.jit_trace_count));
            ui.label(&editor.status);
            ui.separator();
            let diagnostics = editor.diagnostics.clone();
            let mut layouter =
                move |ui: &egui::Ui, text: &dyn egui::TextBuffer, wrap_width: f32| {
                    let mut job = rustscript_layout_job(text.as_str(), &diagnostics);
                    job.wrap.max_width = wrap_width;
                    ui.fonts_mut(|fonts| fonts.layout_job(job))
                };
            ui.add(
                egui::TextEdit::multiline(&mut editor.buffer)
                    .code_editor()
                    .desired_rows(26)
                    .desired_width(f32::INFINITY)
                    .layouter(&mut layouter),
            );
            render_script_diagnostics(ui, &editor.diagnostics);
            if ui.button("Save and apply now").clicked() {
                editor.pending_save = true;
            }
        });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::schedule::ScheduleLabel;
    use rustscript_bevy_gameplay::{ShooterEnemySpawnRule, ShooterSpawnTrigger};

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
    fn rustscript_highlighter_marks_keywords_host_calls_and_literals() {
        let source = r#"let hp: bool = bevy::Shooter::set_player_health(95);"#;
        let tokens = rustscript_highlight_tokens(source);

        assert!(
            tokens
                .iter()
                .any(|token| token.kind == ScriptTokenKind::Keyword && token.text(source) == "let")
        );
        assert!(
            tokens
                .iter()
                .any(|token| token.kind == ScriptTokenKind::Type && token.text(source) == "bool")
        );
        assert!(
            tokens
                .iter()
                .any(|token| token.kind == ScriptTokenKind::HostApi
                    && token.text(source) == "bevy::Shooter::set_player_health")
        );
        assert!(
            tokens
                .iter()
                .any(|token| token.kind == ScriptTokenKind::Number && token.text(source) == "95")
        );
    }

    #[test]
    fn script_diagnostics_include_line_span_and_source_line() {
        let source = "use bevy;\nlet hp: bool = bevy::Shooter::set_player_health(95\ntrue;\n";
        let diagnostics = script_compile_diagnostics(source, "fallback error");

        assert_eq!(diagnostics.len(), 1);
        let diagnostic = &diagnostics[0];
        assert_eq!(diagnostic.line, 2);
        assert!(diagnostic.start_col >= 1);
        assert!(diagnostic.end_col >= diagnostic.start_col);
        assert_eq!(
            diagnostic.source_line,
            "let hp: bool = bevy::Shooter::set_player_health(95"
        );
        assert!(diagnostic.message.contains("expected"));
    }

    #[test]
    fn jit_status_label_shows_trace_count() {
        assert_eq!(jit_status_label(true, 3), "JIT: on   traces: 3");
        assert_eq!(jit_status_label(false, 0), "JIT: off   traces: 0");
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
    fn collisions_system_accepts_disjoint_player_and_enemy_health_queries() {
        let mut app = App::new();
        app.insert_resource(Score(0))
            .insert_resource(test_shooter_assets())
            .insert_resource(GameFlow::Running)
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
    fn defeated_enemy_drops_reward_at_enemy_position() {
        let mut app = App::new();
        app.insert_resource(Score(0))
            .insert_resource(test_shooter_assets())
            .insert_resource(GameFlow::Running)
            .add_systems(Update, collisions);

        app.world_mut()
            .spawn((Player, Position { x: 0.0, y: -360.0 }, Health(100)));
        app.world_mut().spawn((
            Enemy {
                kind: "tank".to_string(),
            },
            Position { x: 44.0, y: 188.0 },
            Health(5),
        ));
        app.world_mut().spawn((
            Position { x: 44.0, y: 188.0 },
            Projectile {
                owner: ProjectileOwner::Player,
                damage: 8,
                radius: 18.0,
                pierces: false,
            },
        ));

        app.update();

        let (reward, position) = app
            .world_mut()
            .query::<(&RewardItem, &Position)>()
            .single(app.world())
            .expect("defeated enemy should drop a reward");
        assert_eq!(reward.kind, "health");
        assert_eq!(reward.amount, 20);
        assert_eq!(*position, Position { x: 44.0, y: 188.0 });
    }

    #[test]
    fn enemy_projectile_damage_clamps_player_health_to_zero() {
        let mut app = App::new();
        app.insert_resource(Score(0))
            .insert_resource(test_shooter_assets())
            .insert_resource(GameFlow::Running)
            .add_systems(Update, collisions);

        app.world_mut()
            .spawn((Player, Position { x: 0.0, y: 0.0 }, Health(25)));
        app.world_mut().spawn((
            Position { x: 0.0, y: 0.0 },
            Projectile {
                owner: ProjectileOwner::Enemy,
                damage: 99,
                radius: 20.0,
                pierces: false,
            },
        ));

        app.update();

        let (_, health) = app
            .world_mut()
            .query::<(&Player, &Health)>()
            .single(app.world())
            .expect("player should remain");
        assert_eq!(health.0, 0);
    }

    #[test]
    fn game_flow_changes_to_game_over_when_player_health_is_zero() {
        let mut app = App::new();
        app.insert_resource(GameFlow::Running)
            .add_systems(Update, update_game_flow_after_health);

        app.world_mut()
            .spawn((Player, Position { x: 0.0, y: 0.0 }, Health(0)));

        app.update();

        assert_eq!(*app.world().resource::<GameFlow>(), GameFlow::GameOver);
    }

    #[test]
    fn paused_gameplay_does_not_advance_enemy_motion() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(GameFlow::Paused)
            .add_systems(Update, enemy_motion);

        app.world_mut().spawn((
            Enemy {
                kind: "scout".to_string(),
            },
            AttackStyle("straight".to_string()),
            Position { x: 10.0, y: 200.0 },
            Velocity { x: 0.0, y: -50.0 },
        ));

        app.update();

        let position = app
            .world_mut()
            .query::<&Position>()
            .single(app.world())
            .expect("enemy should remain");
        assert_eq!(*position, Position { x: 10.0, y: 200.0 });
    }

    #[test]
    fn enemy_motion_profiles_vary_by_enemy_kind() {
        let scout = enemy_motion_profile("scout", "straight");
        let tank = enemy_motion_profile("tank", "straight");
        let weaver = enemy_motion_profile("weaver", "straight");
        let striker = enemy_motion_profile("striker", "straight");
        let sniper = enemy_motion_profile("sniper", "straight");

        assert!(tank.descent_speed < scout.descent_speed);
        assert!(striker.descent_speed > scout.descent_speed);
        assert!(weaver.lateral_speed > scout.lateral_speed);
        assert!(sniper.lateral_speed > tank.lateral_speed);
        assert_ne!(weaver.phase, sniper.phase);
    }

    #[test]
    fn enemy_motion_applies_kind_specific_trajectories() {
        let mut app = App::new();
        let mut time = Time::<()>::default();
        time.advance_by(std::time::Duration::from_millis(100));
        app.insert_resource(time)
            .insert_resource(GameFlow::Running)
            .add_systems(Update, enemy_motion);

        for kind in ["tank", "weaver", "striker"] {
            app.world_mut().spawn((
                Enemy {
                    kind: kind.to_string(),
                },
                AttackStyle("straight".to_string()),
                Position { x: 0.0, y: 200.0 },
                Velocity { x: 0.0, y: -50.0 },
                EnemyManeuver::new(0.0),
            ));
        }

        app.update();

        let mut enemies = app
            .world_mut()
            .query::<(&Enemy, &Position, &Velocity)>()
            .iter(app.world())
            .map(|(enemy, position, velocity)| (enemy.kind.clone(), *position, *velocity))
            .collect::<Vec<_>>();
        enemies.sort_by(|left, right| left.0.cmp(&right.0));

        let tank = enemies
            .iter()
            .find(|(kind, _, _)| kind == "tank")
            .expect("tank should remain");
        let weaver = enemies
            .iter()
            .find(|(kind, _, _)| kind == "weaver")
            .expect("weaver should remain");
        let striker = enemies
            .iter()
            .find(|(kind, _, _)| kind == "striker")
            .expect("striker should remain");

        assert!(tank.2.y.abs() < striker.2.y.abs());
        assert!(weaver.2.x.abs() > tank.2.x.abs());
        assert!(striker.1.y < tank.1.y);
    }

    #[test]
    fn scripted_spawn_rules_use_score_delta() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(GameFlow::Running)
            .insert_resource(Score(2))
            .insert_resource(SpawnRuleProgress { last_score: 0 })
            .insert_resource(ShooterSpawnRules {
                enemies: vec![ShooterEnemySpawnRule {
                    kind: "boss".to_string(),
                    health: 120,
                    attack_style: "burst".to_string(),
                    x: 0,
                    y: 540,
                    trigger: ShooterSpawnTrigger::AfterKills {
                        kill_count: 2,
                        kills_seen: 0,
                        fired: false,
                    },
                }],
                rewards: vec![],
            })
            .add_systems(Update, run_scripted_spawn_rules);

        app.update();

        let enemies = app
            .world_mut()
            .query::<&Enemy>()
            .iter(app.world())
            .collect::<Vec<_>>();
        assert_eq!(enemies.len(), 1);
        assert_eq!(enemies[0].kind, "boss");
    }

    #[test]
    fn restarting_game_resets_score_player_position_and_dynamic_entities() {
        let mut app = App::new();
        app.insert_resource(Score(9))
            .insert_resource(GameFlow::GameOver);

        app.world_mut().spawn((
            Player,
            Health(0),
            AttackStyle("laser".to_string()),
            AttackPower(99),
            AttackCooldownMs(10),
            PlayerProjectileLoadout {
                kind: "laser".to_string(),
                count: 5,
            },
            Position { x: 99.0, y: 99.0 },
            Velocity { x: 12.0, y: 12.0 },
            FireClock { elapsed_ms: 800.0 },
        ));
        app.world_mut().spawn((
            Position { x: 0.0, y: 0.0 },
            Projectile {
                owner: ProjectileOwner::Player,
                damage: 10,
                radius: 10.0,
                pierces: false,
            },
        ));

        let summary = restart_gameplay(app.world_mut(), SCRIPT).expect("restart should apply");

        assert_eq!(summary.player_health, 95);
        assert_eq!(**app.world().resource::<Score>(), 0);
        assert_eq!(*app.world().resource::<GameFlow>(), GameFlow::Running);

        let (_, health, position, loadout, clock) = app
            .world_mut()
            .query::<(
                &Player,
                &Health,
                &Position,
                &PlayerProjectileLoadout,
                &FireClock,
            )>()
            .single(app.world())
            .expect("player should remain");
        assert_eq!(health.0, 95);
        assert_eq!(*position, Position { x: 0.0, y: -360.0 });
        assert_eq!(loadout.kind, "bolt");
        assert_eq!(loadout.count, 1);
        assert_eq!(clock.elapsed_ms, 0.0);

        let projectile_count = app
            .world_mut()
            .query::<&Projectile>()
            .iter(app.world())
            .count();
        assert_eq!(projectile_count, 0);
    }

    #[test]
    fn applying_script_live_keeps_existing_dynamic_entities() {
        let live_source = r#"
use bevy;
let hp: bool = bevy::Shooter::set_player_health(77);
let enemy: bool = bevy::Shooter::spawn_enemy("ace", 55, "wave", 0, 470);
let reward: bool = bevy::Shooter::spawn_reward("health", 25, 40, -360);
true;
"#;
        let mut app = App::new();
        app.insert_resource(Score(3))
            .insert_resource(GameFlow::Running)
            .insert_resource(ScriptEditor {
                buffer: live_source.to_string(),
                status: String::new(),
                diagnostics: Vec::new(),
                pending_save: true,
                pending_restart: false,
                jit_enabled: true,
                jit_trace_count: 0,
            });
        app.world_mut().spawn((
            Player,
            Health(95),
            AttackStyle("straight".to_string()),
            AttackPower(8),
            AttackCooldownMs(260),
            PlayerProjectileLoadout {
                kind: "bolt".to_string(),
                count: 1,
            },
            Position { x: 0.0, y: -360.0 },
            Velocity { x: 0.0, y: 0.0 },
        ));
        app.world_mut().spawn((
            Enemy {
                kind: "bomber".to_string(),
            },
            Health(42),
            AttackStyle("burst".to_string()),
            AttackPower(3),
            AttackCooldownMs(1400),
            Position { x: -40.0, y: 450.0 },
            Velocity { x: 0.0, y: -50.0 },
            ScriptManagedEnemy,
        ));
        app.world_mut().spawn((
            RewardItem {
                kind: "bullets".to_string(),
                amount: 1,
            },
            Position {
                x: -120.0,
                y: -220.0,
            },
        ));
        app.world_mut().spawn((
            Position { x: 0.0, y: -80.0 },
            Velocity { x: 0.0, y: 560.0 },
            Projectile {
                owner: ProjectileOwner::Player,
                damage: 8,
                radius: 10.0,
                pierces: false,
            },
        ));

        apply_pending_script(app.world_mut());

        let enemies = app
            .world_mut()
            .query::<&Enemy>()
            .iter(app.world())
            .map(|enemy| enemy.kind.as_str())
            .collect::<Vec<_>>();
        assert_eq!(enemies.len(), 2);
        assert!(enemies.contains(&"bomber"));
        assert!(enemies.contains(&"ace"));

        let rewards = app
            .world_mut()
            .query::<&RewardItem>()
            .iter(app.world())
            .map(|reward| reward.kind.as_str())
            .collect::<Vec<_>>();
        assert_eq!(rewards.len(), 2);
        assert!(rewards.contains(&"bullets"));
        assert!(rewards.contains(&"health"));

        let projectile_count = app
            .world_mut()
            .query::<&Projectile>()
            .iter(app.world())
            .count();
        assert_eq!(projectile_count, 1);
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
    fn shooter_asset_file_path_points_at_repo_assets() {
        let asset_path = std::path::PathBuf::from(shooter_asset_file_path());
        assert!(asset_path.join("shooter/player_0.png").is_file());
        assert!(asset_path.join("shooter/shockwave_0.png").is_file());
        assert!(asset_path.join("shooter/background_nebula.png").is_file());
        assert!(asset_path.join("shooter/enemy_craft_scout.png").is_file());
        assert!(asset_path.join("shooter/hit_flash.png").is_file());
        assert!(asset_path.join("shooter/explosion_0.png").is_file());
    }

    #[test]
    fn shooter_asset_path_prefers_packaged_assets_next_to_exe() {
        let temp_root =
            std::env::temp_dir().join(format!("rustscript_shooter_assets_{}", std::process::id()));
        let exe_dir = temp_root.join("package");
        let packaged_shooter_dir = exe_dir.join("assets").join("shooter");
        std::fs::create_dir_all(&packaged_shooter_dir).unwrap();
        std::fs::write(packaged_shooter_dir.join("background_nebula.png"), b"png").unwrap();
        std::fs::write(packaged_shooter_dir.join("player_0.png"), b"png").unwrap();
        std::fs::write(packaged_shooter_dir.join("enemy_craft_scout.png"), b"png").unwrap();
        let exe = exe_dir.join(format!("shooter{}", std::env::consts::EXE_SUFFIX));
        std::fs::write(&exe, b"exe").unwrap();
        let cwd = temp_root.join("cwd");
        std::fs::create_dir_all(&cwd).unwrap();
        let manifest_assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");

        let resolved = resolve_shooter_asset_file_path(Some(&exe), Some(&cwd), &manifest_assets);

        assert_eq!(resolved, exe_dir.join("assets"));
        std::fs::remove_dir_all(temp_root).unwrap();
    }

    #[test]
    fn projectile_plan_shares_advanced_projectiles_between_sides() {
        let player_loadout = PlayerProjectileLoadout {
            kind: "missile".to_string(),
            count: 2,
        };
        let player_spread =
            projectile_plan(ProjectileOwner::Player, "spread", 10, Some(&player_loadout));
        assert!(
            player_spread
                .iter()
                .any(|shot| shot.kind == ProjectileKind::HomingMissile)
        );
        assert!(player_spread.iter().all(|shot| shot.velocity.y > 0.0));
        assert_eq!(player_spread.len(), 2);

        let enemy_burst = projectile_plan(ProjectileOwner::Enemy, "burst", 10, None);
        assert!(
            enemy_burst
                .iter()
                .any(|shot| shot.kind == ProjectileKind::HomingMissile)
        );

        let enemy_wave = projectile_plan(ProjectileOwner::Enemy, "wave", 10, None);
        assert!(
            enemy_wave
                .iter()
                .any(|shot| shot.kind == ProjectileKind::Shockwave)
        );

        let player_missile = projectile_plan(
            ProjectileOwner::Player,
            "missile",
            10,
            Some(&player_loadout),
        );
        let enemy_missile = projectile_plan(ProjectileOwner::Enemy, "missile", 10, None);
        assert_eq!(player_missile[0].kind, enemy_missile[0].kind);
        assert!(player_missile[0].velocity.y > 0.0);
        assert!(enemy_missile[0].velocity.y < 0.0);
    }

    #[test]
    fn enemy_projectile_plan_varies_by_enemy_kind() {
        let sniper = enemy_projectile_plan("sniper", "straight", 10);
        assert_eq!(sniper[0].kind, ProjectileKind::Rail);
        assert!(sniper[0].velocity.y < -800.0);

        let carrier = enemy_projectile_plan("carrier", "burst", 10);
        assert!(
            carrier
                .iter()
                .any(|shot| shot.kind == ProjectileKind::HomingMissile)
        );
        assert!(
            carrier
                .iter()
                .any(|shot| shot.kind == ProjectileKind::Plasma)
        );

        let striker = enemy_projectile_plan("striker", "straight", 10);
        assert!(striker.iter().all(|shot| shot.kind == ProjectileKind::Flak));
        assert!(striker.iter().all(|shot| shot.velocity.y <= -620.0));
    }

    #[test]
    fn player_loadout_supports_new_projectile_types() {
        let plasma = player_projectile_plan("plasma", 2, 10);
        assert_eq!(plasma.len(), 2);
        assert!(
            plasma
                .iter()
                .all(|shot| shot.kind == ProjectileKind::Plasma)
        );

        let rail = player_projectile_plan("rail", 1, 10);
        assert_eq!(rail[0].kind, ProjectileKind::Rail);
        assert!(rail[0].velocity.y > 850.0);

        let flak = player_projectile_plan("flak", 3, 10);
        assert_eq!(flak.len(), 3);
        assert!(flak.iter().all(|shot| shot.kind == ProjectileKind::Flak));
    }

    #[test]
    fn collecting_rewards_updates_player_health_and_projectile_count() {
        let mut app = App::new();
        app.insert_resource(GameFlow::Running);
        app.add_systems(Update, collect_rewards);
        app.world_mut().spawn((
            Player,
            Position { x: 0.0, y: 0.0 },
            Health(90),
            PlayerProjectileLoadout {
                kind: "bolt".to_string(),
                count: 1,
            },
        ));
        app.world_mut().spawn((
            RewardItem {
                kind: "health".to_string(),
                amount: 20,
            },
            Position { x: 8.0, y: 4.0 },
        ));
        app.world_mut().spawn((
            RewardItem {
                kind: "bullets".to_string(),
                amount: 2,
            },
            Position { x: -6.0, y: 4.0 },
        ));

        app.update();

        let (_, health, loadout) = app
            .world_mut()
            .query::<(&Player, &Health, &PlayerProjectileLoadout)>()
            .single(app.world())
            .expect("player should remain");
        assert_eq!(health.0, 110);
        assert_eq!(loadout.count, 3);

        let reward_count = app
            .world_mut()
            .query::<&RewardItem>()
            .iter(app.world())
            .count();
        assert_eq!(reward_count, 0);
    }

    #[test]
    fn player_and_enemy_hits_spawn_feedback_effects() {
        let mut app = App::new();
        app.insert_resource(Score(0))
            .insert_resource(test_shooter_assets())
            .insert_resource(GameFlow::Running)
            .add_systems(Update, collisions);

        app.world_mut()
            .spawn((Player, Position { x: 0.0, y: 0.0 }, Health(30)));
        app.world_mut().spawn((
            Enemy {
                kind: "scout".to_string(),
            },
            Position { x: 90.0, y: 0.0 },
            Health(20),
        ));
        app.world_mut().spawn((
            Position { x: 90.0, y: 0.0 },
            Projectile {
                owner: ProjectileOwner::Player,
                damage: 4,
                radius: 20.0,
                pierces: false,
            },
        ));
        app.world_mut().spawn((
            Position { x: 0.0, y: 0.0 },
            Projectile {
                owner: ProjectileOwner::Enemy,
                damage: 4,
                radius: 20.0,
                pierces: false,
            },
        ));

        app.update();

        let hit_effects = app
            .world_mut()
            .query::<&HitEffect>()
            .iter(app.world())
            .count();
        assert_eq!(hit_effects, 2);
    }

    #[test]
    fn defeated_enemy_spawns_explosion_animation() {
        let mut app = App::new();
        app.insert_resource(Score(0))
            .insert_resource(test_shooter_assets())
            .insert_resource(GameFlow::Running)
            .add_systems(Update, collisions);

        app.world_mut()
            .spawn((Player, Position { x: 0.0, y: -360.0 }, Health(100)));
        app.world_mut().spawn((
            Enemy {
                kind: "bomber".to_string(),
            },
            Position { x: 12.0, y: 140.0 },
            Health(5),
        ));
        app.world_mut().spawn((
            Position { x: 12.0, y: 140.0 },
            Projectile {
                owner: ProjectileOwner::Player,
                damage: 8,
                radius: 18.0,
                pierces: false,
            },
        ));

        app.update();

        let (frames, lifetime) = app
            .world_mut()
            .query_filtered::<(&SpriteFrames, &Lifetime), With<ExplosionEffect>>()
            .single(app.world())
            .expect("enemy defeat should create an explosion animation");
        assert!(frames.frames.len() >= 4);
        assert!(lifetime.duration_ms > 250.0);
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
    fn homing_velocity_keeps_forward_axis_when_target_is_behind() {
        let player_velocity = homing_velocity_step(
            Velocity { x: 0.0, y: 360.0 },
            Position { x: 0.0, y: 0.0 },
            Position {
                x: 120.0,
                y: -200.0,
            },
            360.0,
            1.0,
        );
        assert!(player_velocity.y > 0.0);

        let enemy_velocity = homing_velocity_step(
            Velocity { x: 0.0, y: -360.0 },
            Position { x: 0.0, y: 0.0 },
            Position {
                x: -120.0,
                y: 200.0,
            },
            360.0,
            1.0,
        );
        assert!(enemy_velocity.y < 0.0);
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
