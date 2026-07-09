use std::{cell::RefCell, time::Instant};

use bevy_ecs::prelude::*;
pub(crate) use vm::Vm;
use vm::{
    CallOutcome, CallReturn, Debugger, JitConfig, Value, VmError, VmResult, VmStatus,
    compile_source,
};

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Health(pub i64);

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Armor(pub i64);

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Player;

#[derive(Component, Debug, Clone, PartialEq, Eq)]
pub struct Enemy {
    pub kind: String,
}

#[derive(Component, Debug, Clone, PartialEq, Eq)]
pub struct AttackStyle(pub String);

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttackPower(pub i64);

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttackCooldownMs(pub i64);

#[derive(Component, Debug, Clone, PartialEq, Eq)]
pub struct PlayerProjectileLoadout {
    pub kind: String,
    pub count: i64,
}

#[derive(Component, Debug, Clone, PartialEq, Eq)]
pub struct RewardItem {
    pub kind: String,
    pub amount: i64,
}

#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct Position {
    pub x: f32,
    pub y: f32,
}

#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct Velocity {
    pub x: f32,
    pub y: f32,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScriptManagedEnemy;

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScriptManagedReward;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShooterSummary {
    pub player_health: i64,
    pub player_attack_style: String,
    pub player_attack_power: i64,
    pub player_attack_cooldown_ms: i64,
    pub player_projectile_kind: String,
    pub player_projectile_count: i64,
    pub enemies_spawned: usize,
    pub rewards_spawned: usize,
    pub jit: ShooterJitSummary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShooterJitSummary {
    pub enabled: bool,
    pub trace_count: usize,
}

#[derive(Resource, Debug, Clone, Default, PartialEq, Eq)]
pub struct ShooterSpawnRules {
    pub enemies: Vec<ShooterEnemySpawnRule>,
    pub rewards: Vec<ShooterRewardSpawnRule>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShooterEnemySpawnRule {
    pub kind: String,
    pub health: i64,
    pub attack_style: String,
    pub x: i64,
    pub y: i64,
    pub trigger: ShooterSpawnTrigger,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShooterRewardSpawnRule {
    pub kind: String,
    pub amount: i64,
    pub x: i64,
    pub y: i64,
    pub trigger: ShooterSpawnTrigger,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShooterSpawnTrigger {
    EveryMs {
        interval_ms: i64,
        elapsed_ms: i64,
    },
    AfterKills {
        kill_count: i64,
        kills_seen: i64,
        fired: bool,
    },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ShooterRuleTickSummary {
    pub enemies_spawned: usize,
    pub rewards_spawned: usize,
}

pub const GOMOKU_BOARD_SIZE: i64 = 15;

#[derive(Resource, Debug, Clone, PartialEq, Eq)]
pub struct GomokuBoard {
    cells: Vec<i64>,
}

impl Default for GomokuBoard {
    fn default() -> Self {
        Self {
            cells: vec![0; (GOMOKU_BOARD_SIZE * GOMOKU_BOARD_SIZE) as usize],
        }
    }
}

impl GomokuBoard {
    pub fn cell(&self, x: i64, y: i64) -> i64 {
        self.index(x, y)
            .and_then(|index| self.cells.get(index).copied())
            .unwrap_or(3)
    }

    pub fn cells(&self) -> &[i64] {
        &self.cells
    }

    pub fn clear(&mut self) {
        self.cells.fill(0);
    }

    pub fn set_for_test(&mut self, x: i64, y: i64, stone: i64) {
        self.set_raw(x, y, stone);
    }

    fn set_raw(&mut self, x: i64, y: i64, stone: i64) -> bool {
        let Some(index) = self.index(x, y) else {
            return false;
        };
        self.cells[index] = stone;
        true
    }

    fn index(&self, x: i64, y: i64) -> Option<usize> {
        if !(0..GOMOKU_BOARD_SIZE).contains(&x) || !(0..GOMOKU_BOARD_SIZE).contains(&y) {
            return None;
        }
        Some((y * GOMOKU_BOARD_SIZE + x) as usize)
    }
}

#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
struct GomokuScriptState {
    legal: bool,
    winner: i64,
    draw: bool,
    ai_move: Option<(i64, i64)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GomokuMoveSummary {
    pub legal: bool,
    pub winner: i64,
    pub draw: bool,
    pub telemetry: GomokuScriptTelemetry,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GomokuAiMove {
    pub x: i64,
    pub y: i64,
    pub telemetry: GomokuScriptTelemetry,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GomokuScriptTelemetry {
    pub jit_enabled: bool,
    pub jit_trace_count: usize,
    pub elapsed_micros: u128,
}

pub const XIANGQI_BOARD_WIDTH: i64 = 9;
pub const XIANGQI_BOARD_HEIGHT: i64 = 10;

#[derive(Resource, Debug, Clone, PartialEq, Eq)]
pub struct XiangqiBoard {
    cells: Vec<i64>,
}

impl Default for XiangqiBoard {
    fn default() -> Self {
        let mut board = Self {
            cells: vec![0; (XIANGQI_BOARD_WIDTH * XIANGQI_BOARD_HEIGHT) as usize],
        };
        board.reset();
        board
    }
}

impl XiangqiBoard {
    pub fn cell(&self, x: i64, y: i64) -> i64 {
        self.index(x, y)
            .and_then(|index| self.cells.get(index).copied())
            .unwrap_or(99)
    }

    pub fn cells(&self) -> &[i64] {
        &self.cells
    }

    pub fn clear_for_test(&mut self) {
        self.cells.fill(0);
    }

    pub fn set_for_test(&mut self, x: i64, y: i64, piece: i64) {
        self.set_raw(x, y, piece);
    }

    fn reset(&mut self) {
        self.cells.fill(0);
        let black_back = [-5, -4, -3, -2, -1, -2, -3, -4, -5];
        let red_back = [5, 4, 3, 2, 1, 2, 3, 4, 5];
        for x in 0..XIANGQI_BOARD_WIDTH {
            self.set_raw(x, 0, black_back[x as usize]);
            self.set_raw(x, 9, red_back[x as usize]);
        }
        self.set_raw(1, 2, -6);
        self.set_raw(7, 2, -6);
        self.set_raw(1, 7, 6);
        self.set_raw(7, 7, 6);
        for x in [0, 2, 4, 6, 8] {
            self.set_raw(x, 3, -7);
            self.set_raw(x, 6, 7);
        }
    }

    fn set_raw(&mut self, x: i64, y: i64, piece: i64) -> bool {
        let Some(index) = self.index(x, y) else {
            return false;
        };
        self.cells[index] = piece;
        true
    }

    fn index(&self, x: i64, y: i64) -> Option<usize> {
        if !(0..XIANGQI_BOARD_WIDTH).contains(&x) || !(0..XIANGQI_BOARD_HEIGHT).contains(&y) {
            return None;
        }
        Some((y * XIANGQI_BOARD_WIDTH + x) as usize)
    }
}

#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
struct XiangqiScriptState {
    legal: bool,
    winner: i64,
    ai_move: Option<(i64, i64, i64, i64)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct XiangqiScriptTelemetry {
    pub jit_enabled: bool,
    pub jit_trace_count: usize,
    pub elapsed_micros: u128,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct XiangqiMoveSummary {
    pub legal: bool,
    pub winner: i64,
    pub telemetry: XiangqiScriptTelemetry,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct XiangqiAiMove {
    pub from_x: i64,
    pub from_y: i64,
    pub to_x: i64,
    pub to_y: i64,
    pub telemetry: XiangqiScriptTelemetry,
}

#[derive(Resource, Debug, Clone)]
pub struct DamageRules {
    source: String,
}

impl DamageRules {
    pub fn from_source(source: impl Into<String>) -> Result<Self, String> {
        let source = source.into();
        compile_source(&format!(
            "let incoming = 0;\nlet critical = false;\n{source}"
        ))
        .map_err(|err| err.to_string())?;
        Ok(Self { source })
    }
}

pub fn apply_scripted_damage(
    world: &mut World,
    entity: Entity,
    incoming: i64,
    critical: bool,
) -> Result<i64, String> {
    let source = world
        .get_resource::<DamageRules>()
        .ok_or_else(|| "World is missing DamageRules resource".to_string())?
        .source
        .clone();
    evaluate_damage(&source, world, entity, incoming, critical)
}

pub fn apply_shooter_script(world: &mut World, source: &str) -> Result<ShooterSummary, String> {
    compile_source(source).map_err(|err| err.to_string())?;
    world.insert_resource(ShooterSpawnRules::default());
    let (_, jit) = with_shooter_context(world, || run_shooter_script(source))?;
    summarize_shooter_world(world, jit)
}

pub fn reset_gomoku_board(world: &mut World) {
    if let Some(mut board) = world.get_resource_mut::<GomokuBoard>() {
        board.clear();
    } else {
        world.insert_resource(GomokuBoard::default());
    }
    world.insert_resource(GomokuScriptState::default());
}

pub fn apply_gomoku_move_script(
    world: &mut World,
    source: &str,
    x: i64,
    y: i64,
    player: i64,
) -> Result<GomokuMoveSummary, String> {
    ensure_gomoku_resources(world);
    world.insert_resource(GomokuScriptState::default());
    let wrapped = format!(
        "let move_x: int = {x};\nlet move_y: int = {y};\nlet player: int = {player};\n{source}"
    );
    let (_value, telemetry) = with_gomoku_context(world, || run_gomoku_script(&wrapped))?;
    let state = *world
        .get_resource::<GomokuScriptState>()
        .ok_or_else(|| "gomoku script did not publish a result".to_string())?;
    Ok(GomokuMoveSummary {
        legal: state.legal,
        winner: state.winner,
        draw: state.draw,
        telemetry,
    })
}

pub fn choose_gomoku_ai_move(
    world: &mut World,
    source: &str,
    ai_player: i64,
) -> Result<GomokuAiMove, String> {
    ensure_gomoku_resources(world);
    world.insert_resource(GomokuScriptState::default());
    let wrapped = format!("let ai_player: int = {ai_player};\n{source}");
    let (_value, telemetry) = with_gomoku_context(world, || run_gomoku_script(&wrapped))?;
    let state = *world
        .get_resource::<GomokuScriptState>()
        .ok_or_else(|| "gomoku script did not publish an AI move".to_string())?;
    let (x, y) = state
        .ai_move
        .ok_or_else(|| "gomoku AI script did not select a move".to_string())?;
    Ok(GomokuAiMove { x, y, telemetry })
}

pub fn debug_gomoku_move_script(
    world: &mut World,
    source: &str,
    x: i64,
    y: i64,
    player: i64,
    debugger: &mut Debugger,
) -> Result<GomokuMoveSummary, String> {
    ensure_gomoku_resources(world);
    world.insert_resource(GomokuScriptState::default());
    let wrapped = format!(
        "let move_x: int = {x};\nlet move_y: int = {y};\nlet player: int = {player};\n{source}"
    );
    let (_value, telemetry) = with_gomoku_context(world, || {
        run_gomoku_script_with_debugger(&wrapped, debugger)
    })?;
    let state = *world
        .get_resource::<GomokuScriptState>()
        .ok_or_else(|| "gomoku script did not publish a result".to_string())?;
    Ok(GomokuMoveSummary {
        legal: state.legal,
        winner: state.winner,
        draw: state.draw,
        telemetry,
    })
}

pub fn debug_gomoku_ai_script(
    world: &mut World,
    source: &str,
    ai_player: i64,
    debugger: &mut Debugger,
) -> Result<GomokuAiMove, String> {
    ensure_gomoku_resources(world);
    world.insert_resource(GomokuScriptState::default());
    let wrapped = format!("let ai_player: int = {ai_player};\n{source}");
    let (_value, telemetry) = with_gomoku_context(world, || {
        run_gomoku_script_with_debugger(&wrapped, debugger)
    })?;
    let state = *world
        .get_resource::<GomokuScriptState>()
        .ok_or_else(|| "gomoku script did not publish an AI move".to_string())?;
    let (x, y) = state
        .ai_move
        .ok_or_else(|| "gomoku AI script did not select a move".to_string())?;
    Ok(GomokuAiMove { x, y, telemetry })
}

pub fn reset_xiangqi_board(world: &mut World) {
    if let Some(mut board) = world.get_resource_mut::<XiangqiBoard>() {
        board.reset();
    } else {
        world.insert_resource(XiangqiBoard::default());
    }
    world.insert_resource(XiangqiScriptState::default());
}

pub fn apply_xiangqi_move_script(
    world: &mut World,
    source: &str,
    from_x: i64,
    from_y: i64,
    to_x: i64,
    to_y: i64,
    player: i64,
) -> Result<XiangqiMoveSummary, String> {
    ensure_xiangqi_resources(world);
    world.insert_resource(XiangqiScriptState::default());
    let wrapped = format!(
        "let from_x: int = {from_x};\nlet from_y: int = {from_y};\nlet to_x: int = {to_x};\nlet to_y: int = {to_y};\nlet player: int = {player};\n{source}"
    );
    let (_value, telemetry) = with_xiangqi_context(world, || run_xiangqi_script(&wrapped, false))?;
    let state = *world
        .get_resource::<XiangqiScriptState>()
        .ok_or_else(|| "xiangqi script did not publish a result".to_string())?;
    Ok(XiangqiMoveSummary {
        legal: state.legal,
        winner: state.winner,
        telemetry,
    })
}

pub fn choose_xiangqi_ai_move(
    world: &mut World,
    source: &str,
    ai_player: i64,
) -> Result<XiangqiAiMove, String> {
    ensure_xiangqi_resources(world);
    world.insert_resource(XiangqiScriptState::default());
    let wrapped = format!("let ai_player: int = {ai_player};\n{source}");
    let (_value, telemetry) = with_xiangqi_context(world, || run_xiangqi_script(&wrapped, true))?;
    let state = *world
        .get_resource::<XiangqiScriptState>()
        .ok_or_else(|| "xiangqi script did not publish an AI move".to_string())?;
    let (from_x, from_y, to_x, to_y) = state
        .ai_move
        .ok_or_else(|| "xiangqi AI script did not select a move".to_string())?;
    Ok(XiangqiAiMove {
        from_x,
        from_y,
        to_x,
        to_y,
        telemetry,
    })
}

pub fn debug_xiangqi_move_script(
    world: &mut World,
    source: &str,
    from_x: i64,
    from_y: i64,
    to_x: i64,
    to_y: i64,
    player: i64,
    debugger: &mut Debugger,
) -> Result<XiangqiMoveSummary, String> {
    ensure_xiangqi_resources(world);
    world.insert_resource(XiangqiScriptState::default());
    let wrapped = format!(
        "let from_x: int = {from_x};\nlet from_y: int = {from_y};\nlet to_x: int = {to_x};\nlet to_y: int = {to_y};\nlet player: int = {player};\n{source}"
    );
    let (_value, telemetry) = with_xiangqi_context(world, || {
        run_xiangqi_script_with_debugger(&wrapped, debugger)
    })?;
    let state = *world
        .get_resource::<XiangqiScriptState>()
        .ok_or_else(|| "xiangqi script did not publish a result".to_string())?;
    Ok(XiangqiMoveSummary {
        legal: state.legal,
        winner: state.winner,
        telemetry,
    })
}

pub fn debug_xiangqi_ai_script(
    world: &mut World,
    source: &str,
    ai_player: i64,
    debugger: &mut Debugger,
) -> Result<XiangqiAiMove, String> {
    ensure_xiangqi_resources(world);
    world.insert_resource(XiangqiScriptState::default());
    let wrapped = format!("let ai_player: int = {ai_player};\n{source}");
    let (_value, telemetry) = with_xiangqi_context(world, || {
        run_xiangqi_script_with_debugger(&wrapped, debugger)
    })?;
    let state = *world
        .get_resource::<XiangqiScriptState>()
        .ok_or_else(|| "xiangqi script did not publish an AI move".to_string())?;
    let (from_x, from_y, to_x, to_y) = state
        .ai_move
        .ok_or_else(|| "xiangqi AI script did not select a move".to_string())?;
    Ok(XiangqiAiMove {
        from_x,
        from_y,
        to_x,
        to_y,
        telemetry,
    })
}

pub fn tick_shooter_spawn_rules(
    world: &mut World,
    delta_ms: i64,
    kills_delta: i64,
) -> ShooterRuleTickSummary {
    let mut enemy_spawns = Vec::new();
    let mut reward_spawns = Vec::new();
    let delta_ms = delta_ms.max(0);
    let kills_delta = kills_delta.max(0);

    if let Some(mut rules) = world.get_resource_mut::<ShooterSpawnRules>() {
        for rule in &mut rules.enemies {
            let spawn_count = rule.trigger.consume_spawns(delta_ms, kills_delta);
            for _ in 0..spawn_count {
                enemy_spawns.push(rule.clone());
            }
        }
        for rule in &mut rules.rewards {
            let spawn_count = rule.trigger.consume_spawns(delta_ms, kills_delta);
            for _ in 0..spawn_count {
                reward_spawns.push(rule.clone());
            }
        }
    }

    for rule in &enemy_spawns {
        spawn_enemy_entity(
            world,
            &rule.kind,
            rule.health,
            &rule.attack_style,
            rule.x,
            rule.y,
        );
    }
    for rule in &reward_spawns {
        spawn_reward_entity(world, &rule.kind, rule.amount, rule.x, rule.y);
    }

    ShooterRuleTickSummary {
        enemies_spawned: enemy_spawns.len(),
        rewards_spawned: reward_spawns.len(),
    }
}

impl ShooterSpawnTrigger {
    fn consume_spawns(&mut self, delta_ms: i64, kills_delta: i64) -> usize {
        match self {
            Self::EveryMs {
                interval_ms,
                elapsed_ms,
            } => {
                let interval = (*interval_ms).max(1);
                *elapsed_ms += delta_ms;
                let spawn_count = (*elapsed_ms / interval).max(0) as usize;
                if spawn_count > 0 {
                    *elapsed_ms %= interval;
                }
                spawn_count
            }
            Self::AfterKills {
                kill_count,
                kills_seen,
                fired,
            } => {
                if *fired {
                    return 0;
                }
                *kills_seen += kills_delta;
                if *kills_seen >= (*kill_count).max(1) {
                    *fired = true;
                    1
                } else {
                    0
                }
            }
        }
    }
}

fn summarize_shooter_world(
    world: &mut World,
    jit: ShooterJitSummary,
) -> Result<ShooterSummary, String> {
    let (
        player_health,
        player_attack_style,
        player_attack_power,
        player_attack_cooldown_ms,
        player_projectile_kind,
        player_projectile_count,
    ) = {
        let (_, health, style, power, cooldown, loadout) = world
            .query::<(
                &Player,
                &Health,
                &AttackStyle,
                &AttackPower,
                &AttackCooldownMs,
                &PlayerProjectileLoadout,
            )>()
            .single(world)
            .map_err(|err| format!("shooter script must configure exactly one player: {err}"))?;
        (
            health.0,
            style.0.clone(),
            power.0,
            cooldown.0,
            loadout.kind.clone(),
            loadout.count,
        )
    };
    let enemies_spawned = world.query::<&Enemy>().iter(world).count();
    let rewards_spawned = world.query::<&RewardItem>().iter(world).count();
    Ok(ShooterSummary {
        player_health,
        player_attack_style,
        player_attack_power,
        player_attack_cooldown_ms,
        player_projectile_kind,
        player_projectile_count,
        enemies_spawned,
        rewards_spawned,
        jit,
    })
}

fn ensure_player(world: &mut World) -> Entity {
    if let Ok((entity, _)) = world.query::<(Entity, &Player)>().single(world) {
        return entity;
    }
    world
        .spawn((
            Player,
            Health(100),
            AttackStyle("straight".to_string()),
            AttackPower(10),
            AttackCooldownMs(180),
            PlayerProjectileLoadout {
                kind: "bolt".to_string(),
                count: 1,
            },
            Position { x: 0.0, y: -360.0 },
            Velocity { x: 0.0, y: 0.0 },
        ))
        .id()
}

fn ensure_gomoku_resources(world: &mut World) {
    if !world.contains_resource::<GomokuBoard>() {
        world.insert_resource(GomokuBoard::default());
    }
    if !world.contains_resource::<GomokuScriptState>() {
        world.insert_resource(GomokuScriptState::default());
    }
}

fn ensure_xiangqi_resources(world: &mut World) {
    if !world.contains_resource::<XiangqiBoard>() {
        world.insert_resource(XiangqiBoard::default());
    }
    if !world.contains_resource::<XiangqiScriptState>() {
        world.insert_resource(XiangqiScriptState::default());
    }
}

fn spawn_enemy_entity(
    world: &mut World,
    kind: &str,
    health: i64,
    attack_style: &str,
    x: i64,
    y: i64,
) -> Entity {
    world
        .spawn((
            Enemy {
                kind: kind.to_string(),
            },
            Health(health),
            AttackStyle(attack_style.to_string()),
            AttackPower((health / 14).max(2)),
            AttackCooldownMs(1400),
            Position {
                x: x as f32,
                y: y as f32,
            },
            Velocity { x: 0.0, y: -50.0 },
            ScriptManagedEnemy,
        ))
        .id()
}

fn spawn_reward_entity(world: &mut World, kind: &str, amount: i64, x: i64, y: i64) -> Entity {
    world
        .spawn((
            RewardItem {
                kind: kind.to_string(),
                amount,
            },
            Position {
                x: x as f32,
                y: y as f32,
            },
            ScriptManagedReward,
        ))
        .id()
}

fn clamp_spawn_interval_ms(value: i64) -> i64 {
    value.clamp(250, 120_000)
}

fn ensure_spawn_rules(world: &mut World) -> Mut<'_, ShooterSpawnRules> {
    if !world.contains_resource::<ShooterSpawnRules>() {
        world.insert_resource(ShooterSpawnRules::default());
    }
    world.resource_mut::<ShooterSpawnRules>()
}

fn evaluate_damage(
    source: &str,
    world: &mut World,
    entity: Entity,
    incoming: i64,
    critical: bool,
) -> Result<i64, String> {
    let wrapped = format!(
        "let incoming = {incoming};\nlet critical = {};\n{source}",
        if critical { "true" } else { "false" }
    );
    match with_bevy_context(world, entity, || run_value(&wrapped))? {
        Value::Int(value) => Ok(value),
        other => Err(format!("script returned {other:?}; expected int")),
    }
}

#[derive(Clone, Copy)]
struct BevyContext {
    world: *mut World,
    entity: Entity,
}

#[derive(Clone, Copy)]
struct ShooterContext {
    world: *mut World,
}

#[derive(Clone, Copy)]
struct GomokuContext {
    world: *mut World,
}

#[derive(Clone, Copy)]
struct XiangqiContext {
    world: *mut World,
}

thread_local! {
    static BEVY_CONTEXT: RefCell<Option<BevyContext>> = const { RefCell::new(None) };
    static SHOOTER_CONTEXT: RefCell<Option<ShooterContext>> = const { RefCell::new(None) };
    static GOMOKU_CONTEXT: RefCell<Option<GomokuContext>> = const { RefCell::new(None) };
    static XIANGQI_CONTEXT: RefCell<Option<XiangqiContext>> = const { RefCell::new(None) };
}

struct BevyContextGuard;

impl Drop for BevyContextGuard {
    fn drop(&mut self) {
        BEVY_CONTEXT.with(|slot| {
            *slot.borrow_mut() = None;
        });
    }
}

struct ShooterContextGuard;

impl Drop for ShooterContextGuard {
    fn drop(&mut self) {
        SHOOTER_CONTEXT.with(|slot| {
            *slot.borrow_mut() = None;
        });
    }
}

struct GomokuContextGuard;

impl Drop for GomokuContextGuard {
    fn drop(&mut self) {
        GOMOKU_CONTEXT.with(|slot| {
            *slot.borrow_mut() = None;
        });
    }
}

struct XiangqiContextGuard;

impl Drop for XiangqiContextGuard {
    fn drop(&mut self) {
        XIANGQI_CONTEXT.with(|slot| {
            *slot.borrow_mut() = None;
        });
    }
}

fn with_bevy_context<T>(
    world: &mut World,
    entity: Entity,
    f: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    BEVY_CONTEXT.with(|slot| {
        *slot.borrow_mut() = Some(BevyContext { world, entity });
    });
    let _guard = BevyContextGuard;
    f()
}

fn with_shooter_context<T>(
    world: &mut World,
    f: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    SHOOTER_CONTEXT.with(|slot| {
        *slot.borrow_mut() = Some(ShooterContext { world });
    });
    let _guard = ShooterContextGuard;
    f()
}

fn with_gomoku_context<T>(
    world: &mut World,
    f: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    GOMOKU_CONTEXT.with(|slot| {
        *slot.borrow_mut() = Some(GomokuContext { world });
    });
    let _guard = GomokuContextGuard;
    f()
}

fn with_xiangqi_context<T>(
    world: &mut World,
    f: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    XIANGQI_CONTEXT.with(|slot| {
        *slot.borrow_mut() = Some(XiangqiContext { world });
    });
    let _guard = XiangqiContextGuard;
    f()
}

fn with_world<T>(f: impl FnOnce(&mut World, Entity) -> VmResult<T>) -> VmResult<T> {
    BEVY_CONTEXT.with(|slot| {
        let ctx = slot
            .borrow()
            .ok_or_else(|| VmError::HostError("missing Bevy World context".to_string()))?;
        // SAFETY: the pointer is installed only for the synchronous VM run in apply_scripted_damage.
        unsafe { f(&mut *ctx.world, ctx.entity) }
    })
}

fn with_shooter_world<T>(f: impl FnOnce(&mut World) -> VmResult<T>) -> VmResult<T> {
    SHOOTER_CONTEXT.with(|slot| {
        let ctx = slot
            .borrow()
            .ok_or_else(|| VmError::HostError("missing Bevy shooter context".to_string()))?;
        // SAFETY: the pointer is installed only for one synchronous RustScript evaluation.
        unsafe { f(&mut *ctx.world) }
    })
}

fn with_gomoku_world<T>(f: impl FnOnce(&mut World) -> VmResult<T>) -> VmResult<T> {
    GOMOKU_CONTEXT.with(|slot| {
        let ctx = slot
            .borrow()
            .ok_or_else(|| VmError::HostError("missing Bevy gomoku context".to_string()))?;
        // SAFETY: the pointer is installed only for one synchronous RustScript evaluation.
        unsafe { f(&mut *ctx.world) }
    })
}

fn with_xiangqi_world<T>(f: impl FnOnce(&mut World) -> VmResult<T>) -> VmResult<T> {
    XIANGQI_CONTEXT.with(|slot| {
        let ctx = slot
            .borrow()
            .ok_or_else(|| VmError::HostError("missing Bevy xiangqi context".to_string()))?;
        // SAFETY: the pointer is installed only for one synchronous RustScript evaluation.
        unsafe { f(&mut *ctx.world) }
    })
}

fn run_value(source: &str) -> Result<Value, String> {
    let compiled = compile_source(source).map_err(|err| err.to_string())?;
    let mut vm = Vm::new(compiled.program);
    bind_bevy_hosts(&mut vm);
    let status = vm.run().map_err(|err| err.to_string())?;
    if status != VmStatus::Halted {
        return Err(format!("script did not halt: {status:?}"));
    }
    vm.stack()
        .last()
        .cloned()
        .ok_or_else(|| "script returned an empty stack".to_string())
}

fn run_gomoku_script(source: &str) -> Result<(Value, GomokuScriptTelemetry), String> {
    let started = Instant::now();
    let compiled = compile_source(source).map_err(|err| err.to_string())?;
    let mut vm = Vm::new_with_jit_config(compiled.program, shooter_jit_config());
    bind_gomoku_hosts(&mut vm);
    let status = vm.run().map_err(|err| err.to_string())?;
    if status != VmStatus::Halted {
        return Err(format!("script did not halt: {status:?}"));
    }
    let value = vm
        .stack()
        .last()
        .cloned()
        .ok_or_else(|| "script returned an empty stack".to_string())?;
    let jit_snapshot = vm.jit_snapshot();
    Ok((
        value,
        GomokuScriptTelemetry {
            jit_enabled: jit_snapshot.config.enabled,
            jit_trace_count: jit_snapshot.traces.len(),
            elapsed_micros: started.elapsed().as_micros(),
        },
    ))
}

fn run_gomoku_script_with_debugger(
    source: &str,
    debugger: &mut Debugger,
) -> Result<(Value, GomokuScriptTelemetry), String> {
    let started = Instant::now();
    let compiled = compile_source(source).map_err(|err| err.to_string())?;
    let mut vm = Vm::new(compiled.program);
    bind_gomoku_hosts(&mut vm);
    let status = vm
        .run_with_debugger(debugger)
        .map_err(|err| err.to_string())?;
    if status != VmStatus::Halted {
        return Err(format!("script did not halt: {status:?}"));
    }
    let value = vm
        .stack()
        .last()
        .cloned()
        .ok_or_else(|| "script returned an empty stack".to_string())?;
    Ok((
        value,
        GomokuScriptTelemetry {
            jit_enabled: false,
            jit_trace_count: 0,
            elapsed_micros: started.elapsed().as_micros(),
        },
    ))
}

fn run_xiangqi_script(
    source: &str,
    enable_jit: bool,
) -> Result<(Value, XiangqiScriptTelemetry), String> {
    let started = Instant::now();
    let compiled = compile_source(source).map_err(|err| err.to_string())?;
    let mut vm = if enable_jit {
        Vm::new_with_jit_config(compiled.program, shooter_jit_config())
    } else {
        Vm::new(compiled.program)
    };
    bind_xiangqi_hosts(&mut vm);
    let status = vm.run().map_err(|err| err.to_string())?;
    if status != VmStatus::Halted {
        return Err(format!("script did not halt: {status:?}"));
    }
    let value = vm
        .stack()
        .last()
        .cloned()
        .ok_or_else(|| "script returned an empty stack".to_string())?;
    let jit_snapshot = vm.jit_snapshot();
    Ok((
        value,
        XiangqiScriptTelemetry {
            jit_enabled: jit_snapshot.config.enabled,
            jit_trace_count: jit_snapshot.traces.len(),
            elapsed_micros: started.elapsed().as_micros(),
        },
    ))
}

fn run_xiangqi_script_with_debugger(
    source: &str,
    debugger: &mut Debugger,
) -> Result<(Value, XiangqiScriptTelemetry), String> {
    let started = Instant::now();
    let compiled = compile_source(source).map_err(|err| err.to_string())?;
    let mut vm = Vm::new(compiled.program);
    bind_xiangqi_hosts(&mut vm);
    let status = vm
        .run_with_debugger(debugger)
        .map_err(|err| err.to_string())?;
    if status != VmStatus::Halted {
        return Err(format!("script did not halt: {status:?}"));
    }
    let value = vm
        .stack()
        .last()
        .cloned()
        .ok_or_else(|| "script returned an empty stack".to_string())?;
    Ok((
        value,
        XiangqiScriptTelemetry {
            jit_enabled: false,
            jit_trace_count: 0,
            elapsed_micros: started.elapsed().as_micros(),
        },
    ))
}

fn shooter_jit_config() -> JitConfig {
    JitConfig {
        enabled: true,
        hot_loop_threshold: 1,
        max_trace_len: 512,
    }
}

fn run_shooter_script(source: &str) -> Result<(Value, ShooterJitSummary), String> {
    let compiled = compile_source(source).map_err(|err| err.to_string())?;
    let mut vm = Vm::new_with_jit_config(compiled.program, shooter_jit_config());
    bind_shooter_hosts(&mut vm);
    let status = vm.run().map_err(|err| err.to_string())?;
    if status != VmStatus::Halted {
        return Err(format!("script did not halt: {status:?}"));
    }
    let value = vm
        .stack()
        .last()
        .cloned()
        .ok_or_else(|| "script returned an empty stack".to_string())?;
    let jit_snapshot = vm.jit_snapshot();
    Ok((
        value,
        ShooterJitSummary {
            enabled: jit_snapshot.config.enabled,
            trace_count: jit_snapshot.traces.len(),
        },
    ))
}

fn bind_bevy_hosts(vm: &mut Vm) {
    vm.bind_static_args_function(
        "bevy::World::contains_entity",
        host::bevy::world_contains_entity_host,
    );
    vm.bind_static_args_function("bevy::World::get_health", host::bevy::world_get_health_host);
    vm.bind_static_args_function("bevy::World::get_armor", host::bevy::world_get_armor_host);
    vm.bind_static_args_function("bevy::World::set_health", host::bevy::world_set_health_host);
}

fn bind_shooter_hosts(vm: &mut Vm) {
    vm.bind_static_args_function(
        "bevy::Shooter::set_player_health",
        host::bevy::shooter_set_player_health_host,
    );
    vm.bind_static_args_function(
        "bevy::Shooter::set_player_attack",
        host::bevy::shooter_set_player_attack_host,
    );
    vm.bind_static_args_function(
        "bevy::Shooter::spawn_enemy",
        host::bevy::shooter_spawn_enemy_host,
    );
    vm.bind_static_args_function(
        "bevy::Shooter::set_player_projectiles",
        host::bevy::shooter_set_player_projectiles_host,
    );
    vm.bind_static_args_function(
        "bevy::Shooter::spawn_reward",
        host::bevy::shooter_spawn_reward_host,
    );
    vm.bind_static_args_function(
        "bevy::Shooter::spawn_enemy_every",
        host::bevy::shooter_spawn_enemy_every_host,
    );
    vm.bind_static_args_function(
        "bevy::Shooter::spawn_reward_every",
        host::bevy::shooter_spawn_reward_every_host,
    );
    vm.bind_static_args_function(
        "bevy::Shooter::spawn_enemy_after_kills",
        host::bevy::shooter_spawn_enemy_after_kills_host,
    );
}

fn bind_gomoku_hosts(vm: &mut Vm) {
    vm.bind_static_args_function(
        "bevy::Gomoku::board_size",
        host::bevy::gomoku_board_size_host,
    );
    vm.bind_static_args_function("bevy::Gomoku::cell", host::bevy::gomoku_cell_host);
    vm.bind_static_args_function("bevy::Gomoku::set_cell", host::bevy::gomoku_set_cell_host);
    vm.bind_static_args_function(
        "bevy::Gomoku::set_move_result",
        host::bevy::gomoku_set_move_result_host,
    );
    vm.bind_static_args_function(
        "bevy::Gomoku::set_ai_move",
        host::bevy::gomoku_set_ai_move_host,
    );
}

fn bind_xiangqi_hosts(vm: &mut Vm) {
    vm.bind_static_args_function(
        "bevy::Xiangqi::board_width",
        host::bevy::xiangqi_board_width_host,
    );
    vm.bind_static_args_function(
        "bevy::Xiangqi::board_height",
        host::bevy::xiangqi_board_height_host,
    );
    vm.bind_static_args_function("bevy::Xiangqi::cell", host::bevy::xiangqi_cell_host);
    vm.bind_static_args_function("bevy::Xiangqi::set_cell", host::bevy::xiangqi_set_cell_host);
    vm.bind_static_args_function(
        "bevy::Xiangqi::set_move_result",
        host::bevy::xiangqi_set_move_result_host,
    );
    vm.bind_static_args_function(
        "bevy::Xiangqi::set_ai_move",
        host::bevy::xiangqi_set_ai_move_host,
    );
}

mod host {
    use super::*;
    use pd_host_function::pd_host_function;

    pub(super) trait BorrowVmValue<'a>: Sized {
        fn borrow_vm_value(value: &'a Value, label: &str) -> VmResult<Self>;
    }

    pub(super) fn borrow_arg<'a, T>(args: &'a [Value], index: usize, label: &str) -> VmResult<T>
    where
        T: BorrowVmValue<'a>,
    {
        let value = args
            .get(index)
            .ok_or_else(|| VmError::HostError(format!("missing argument: {label}")))?;
        T::borrow_vm_value(value, label)
    }

    impl BorrowVmValue<'_> for i64 {
        fn borrow_vm_value(value: &Value, _label: &str) -> VmResult<Self> {
            match value {
                Value::Int(value) => Ok(*value),
                _ => Err(VmError::TypeMismatch("int")),
            }
        }
    }

    impl BorrowVmValue<'_> for bool {
        fn borrow_vm_value(value: &Value, _label: &str) -> VmResult<Self> {
            match value {
                Value::Bool(value) => Ok(*value),
                _ => Err(VmError::TypeMismatch("bool")),
            }
        }
    }

    impl<'a> BorrowVmValue<'a> for &'a str {
        fn borrow_vm_value(value: &'a Value, _label: &str) -> VmResult<Self> {
            match value {
                Value::String(value) => Ok(value.as_str()),
                _ => Err(VmError::TypeMismatch("string")),
            }
        }
    }

    trait IntoVmValue {
        fn into_vm_value(self) -> Value;
    }

    impl IntoVmValue for bool {
        fn into_vm_value(self) -> Value {
            Value::Bool(self)
        }
    }

    impl IntoVmValue for i64 {
        fn into_vm_value(self) -> Value {
            Value::Int(self)
        }
    }

    fn return_one<T: IntoVmValue>(value: VmResult<T>) -> VmResult<CallOutcome> {
        Ok(CallOutcome::Return(CallReturn::one(value?.into_vm_value())))
    }

    pub(super) mod bevy {
        use super::*;

        /// Calls Bevy World::contains_entity for the current entity.
        #[pd_host_function(name = "bevy::World::contains_entity")]
        pub(crate) fn world_contains_entity_impl() -> VmResult<bool> {
            with_world(|world, entity| Ok(world.get_entity(entity).is_ok()))
        }

        pub(crate) fn world_contains_entity_host(args: &[Value]) -> VmResult<CallOutcome> {
            return_one(world_contains_entity(args))
        }

        /// Reads Health via Bevy World::get for the current entity.
        #[pd_host_function(name = "bevy::World::get_health")]
        pub(crate) fn world_get_health_impl() -> VmResult<i64> {
            with_world(|world, entity| {
                world
                    .get::<Health>(entity)
                    .map(|value| value.0)
                    .ok_or_else(|| {
                        VmError::HostError(format!("entity {entity:?} is missing Health"))
                    })
            })
        }

        pub(crate) fn world_get_health_host(args: &[Value]) -> VmResult<CallOutcome> {
            return_one(world_get_health(args))
        }

        /// Reads Armor via Bevy World::get for the current entity.
        #[pd_host_function(name = "bevy::World::get_armor")]
        pub(crate) fn world_get_armor_impl() -> VmResult<i64> {
            with_world(|world, entity| {
                world
                    .get::<Armor>(entity)
                    .map(|value| value.0)
                    .ok_or_else(|| {
                        VmError::HostError(format!("entity {entity:?} is missing Armor"))
                    })
            })
        }

        pub(crate) fn world_get_armor_host(args: &[Value]) -> VmResult<CallOutcome> {
            return_one(world_get_armor(args))
        }

        /// Writes Health via Bevy World::get_mut for the current entity.
        #[pd_host_function(name = "bevy::World::set_health")]
        pub(crate) fn world_set_health_impl(value: i64) -> VmResult<bool> {
            with_world(|world, entity| {
                let mut health = world.get_mut::<Health>(entity).ok_or_else(|| {
                    VmError::HostError(format!("entity {entity:?} is missing Health"))
                })?;
                health.0 = value;
                Ok(true)
            })
        }

        pub(crate) fn world_set_health_host(args: &[Value]) -> VmResult<CallOutcome> {
            return_one(world_set_health(args))
        }

        /// Updates the live Bevy ECS player Health component from RustScript.
        #[pd_host_function(name = "bevy::Shooter::set_player_health")]
        pub(crate) fn shooter_set_player_health_impl(value: i64) -> VmResult<bool> {
            with_shooter_world(|world| {
                let player = ensure_player(world);
                let mut health = world.get_mut::<Health>(player).ok_or_else(|| {
                    VmError::HostError("player entity is missing Health".to_string())
                })?;
                health.0 = value;
                Ok(true)
            })
        }

        pub(crate) fn shooter_set_player_health_host(args: &[Value]) -> VmResult<CallOutcome> {
            return_one(shooter_set_player_health(args))
        }

        /// Updates the live Bevy ECS player attack style, power, and cooldown from RustScript.
        #[pd_host_function(name = "bevy::Shooter::set_player_attack")]
        pub(crate) fn shooter_set_player_attack_impl(
            style: &str,
            power: i64,
            cooldown_ms: i64,
        ) -> VmResult<bool> {
            with_shooter_world(|world| {
                let player = ensure_player(world);
                let mut attack_style = world.get_mut::<AttackStyle>(player).ok_or_else(|| {
                    VmError::HostError("player entity is missing AttackStyle".to_string())
                })?;
                attack_style.0 = style.to_string();
                let mut attack_power = world.get_mut::<AttackPower>(player).ok_or_else(|| {
                    VmError::HostError("player entity is missing AttackPower".to_string())
                })?;
                attack_power.0 = power;
                let mut cooldown = world.get_mut::<AttackCooldownMs>(player).ok_or_else(|| {
                    VmError::HostError("player entity is missing AttackCooldownMs".to_string())
                })?;
                cooldown.0 = cooldown_ms;
                Ok(true)
            })
        }

        pub(crate) fn shooter_set_player_attack_host(args: &[Value]) -> VmResult<CallOutcome> {
            return_one(shooter_set_player_attack(args))
        }

        /// Updates the player's projectile asset type and simultaneous count from RustScript.
        #[pd_host_function(name = "bevy::Shooter::set_player_projectiles")]
        pub(crate) fn shooter_set_player_projectiles_impl(
            kind: &str,
            count: i64,
        ) -> VmResult<bool> {
            with_shooter_world(|world| {
                let player = ensure_player(world);
                let mut loadout = world
                    .get_mut::<PlayerProjectileLoadout>(player)
                    .ok_or_else(|| {
                        VmError::HostError(
                            "player entity is missing PlayerProjectileLoadout".to_string(),
                        )
                    })?;
                loadout.kind = kind.to_string();
                loadout.count = count.clamp(1, 5);
                Ok(true)
            })
        }

        pub(crate) fn shooter_set_player_projectiles_host(args: &[Value]) -> VmResult<CallOutcome> {
            return_one(shooter_set_player_projectiles(args))
        }

        /// Spawns a script-managed enemy by calling Bevy World::spawn.
        #[pd_host_function(name = "bevy::Shooter::spawn_enemy")]
        pub(crate) fn shooter_spawn_enemy_impl(
            kind: &str,
            health: i64,
            attack_style: &str,
            x: i64,
            y: i64,
        ) -> VmResult<bool> {
            with_shooter_world(|world| {
                spawn_enemy_entity(world, kind, health, attack_style, x, y);
                Ok(true)
            })
        }

        pub(crate) fn shooter_spawn_enemy_host(args: &[Value]) -> VmResult<CallOutcome> {
            return_one(shooter_spawn_enemy(args))
        }

        /// Spawns a script-managed reward pickup by calling Bevy World::spawn.
        #[pd_host_function(name = "bevy::Shooter::spawn_reward")]
        pub(crate) fn shooter_spawn_reward_impl(
            kind: &str,
            amount: i64,
            x: i64,
            y: i64,
        ) -> VmResult<bool> {
            with_shooter_world(|world| {
                spawn_reward_entity(world, kind, amount, x, y);
                Ok(true)
            })
        }

        pub(crate) fn shooter_spawn_reward_host(args: &[Value]) -> VmResult<CallOutcome> {
            return_one(shooter_spawn_reward(args))
        }

        /// Registers a repeated enemy spawn rule from RustScript.
        #[pd_host_function(name = "bevy::Shooter::spawn_enemy_every")]
        pub(crate) fn shooter_spawn_enemy_every_impl(
            kind: &str,
            health: i64,
            attack_style: &str,
            x: i64,
            y: i64,
            interval_ms: i64,
        ) -> VmResult<bool> {
            with_shooter_world(|world| {
                ensure_spawn_rules(world)
                    .enemies
                    .push(ShooterEnemySpawnRule {
                        kind: kind.to_string(),
                        health,
                        attack_style: attack_style.to_string(),
                        x,
                        y,
                        trigger: ShooterSpawnTrigger::EveryMs {
                            interval_ms: clamp_spawn_interval_ms(interval_ms),
                            elapsed_ms: 0,
                        },
                    });
                Ok(true)
            })
        }

        pub(crate) fn shooter_spawn_enemy_every_host(args: &[Value]) -> VmResult<CallOutcome> {
            return_one(shooter_spawn_enemy_every(args))
        }

        /// Registers a repeated reward spawn rule from RustScript.
        #[pd_host_function(name = "bevy::Shooter::spawn_reward_every")]
        pub(crate) fn shooter_spawn_reward_every_impl(
            kind: &str,
            amount: i64,
            x: i64,
            y: i64,
            interval_ms: i64,
        ) -> VmResult<bool> {
            with_shooter_world(|world| {
                ensure_spawn_rules(world)
                    .rewards
                    .push(ShooterRewardSpawnRule {
                        kind: kind.to_string(),
                        amount,
                        x,
                        y,
                        trigger: ShooterSpawnTrigger::EveryMs {
                            interval_ms: clamp_spawn_interval_ms(interval_ms),
                            elapsed_ms: 0,
                        },
                    });
                Ok(true)
            })
        }

        pub(crate) fn shooter_spawn_reward_every_host(args: &[Value]) -> VmResult<CallOutcome> {
            return_one(shooter_spawn_reward_every(args))
        }

        /// Registers a one-shot enemy spawn rule gated by kills since script apply.
        #[pd_host_function(name = "bevy::Shooter::spawn_enemy_after_kills")]
        pub(crate) fn shooter_spawn_enemy_after_kills_impl(
            kind: &str,
            health: i64,
            attack_style: &str,
            x: i64,
            y: i64,
            kill_count: i64,
        ) -> VmResult<bool> {
            with_shooter_world(|world| {
                ensure_spawn_rules(world)
                    .enemies
                    .push(ShooterEnemySpawnRule {
                        kind: kind.to_string(),
                        health,
                        attack_style: attack_style.to_string(),
                        x,
                        y,
                        trigger: ShooterSpawnTrigger::AfterKills {
                            kill_count: kill_count.max(1),
                            kills_seen: 0,
                            fired: false,
                        },
                    });
                Ok(true)
            })
        }

        pub(crate) fn shooter_spawn_enemy_after_kills_host(
            args: &[Value],
        ) -> VmResult<CallOutcome> {
            return_one(shooter_spawn_enemy_after_kills(args))
        }

        /// Returns the square Gomoku board size for RustScript scans.
        #[pd_host_function(name = "bevy::Gomoku::board_size")]
        pub(crate) fn gomoku_board_size_impl() -> VmResult<i64> {
            with_gomoku_world(|_world| Ok(GOMOKU_BOARD_SIZE))
        }

        pub(crate) fn gomoku_board_size_host(args: &[Value]) -> VmResult<CallOutcome> {
            return_one(gomoku_board_size(args))
        }

        /// Reads a board cell; out-of-bounds cells return a sentinel value.
        #[pd_host_function(name = "bevy::Gomoku::cell")]
        pub(crate) fn gomoku_cell_impl(x: i64, y: i64) -> VmResult<i64> {
            with_gomoku_world(|world| {
                ensure_gomoku_resources(world);
                let board = world.resource::<GomokuBoard>();
                Ok(board.cell(x, y))
            })
        }

        pub(crate) fn gomoku_cell_host(args: &[Value]) -> VmResult<CallOutcome> {
            return_one(gomoku_cell(args))
        }

        /// Writes a board cell after RustScript has accepted a move.
        #[pd_host_function(name = "bevy::Gomoku::set_cell")]
        pub(crate) fn gomoku_set_cell_impl(x: i64, y: i64, stone: i64) -> VmResult<bool> {
            with_gomoku_world(|world| {
                ensure_gomoku_resources(world);
                let mut board = world.resource_mut::<GomokuBoard>();
                Ok(board.set_raw(x, y, stone))
            })
        }

        pub(crate) fn gomoku_set_cell_host(args: &[Value]) -> VmResult<CallOutcome> {
            return_one(gomoku_set_cell(args))
        }

        /// Publishes RustScript move legality and board outcome.
        #[pd_host_function(name = "bevy::Gomoku::set_move_result")]
        pub(crate) fn gomoku_set_move_result_impl(
            legal: bool,
            winner: i64,
            draw: bool,
        ) -> VmResult<bool> {
            with_gomoku_world(|world| {
                ensure_gomoku_resources(world);
                let mut state = world.resource_mut::<GomokuScriptState>();
                state.legal = legal;
                state.winner = winner;
                state.draw = draw;
                Ok(true)
            })
        }

        pub(crate) fn gomoku_set_move_result_host(args: &[Value]) -> VmResult<CallOutcome> {
            return_one(gomoku_set_move_result(args))
        }

        /// Publishes the RustScript-selected AI move.
        #[pd_host_function(name = "bevy::Gomoku::set_ai_move")]
        pub(crate) fn gomoku_set_ai_move_impl(x: i64, y: i64) -> VmResult<bool> {
            with_gomoku_world(|world| {
                ensure_gomoku_resources(world);
                let mut state = world.resource_mut::<GomokuScriptState>();
                state.ai_move = Some((x, y));
                Ok(true)
            })
        }

        pub(crate) fn gomoku_set_ai_move_host(args: &[Value]) -> VmResult<CallOutcome> {
            return_one(gomoku_set_ai_move(args))
        }

        /// Returns the Xiangqi board width for RustScript scans.
        #[pd_host_function(name = "bevy::Xiangqi::board_width")]
        pub(crate) fn xiangqi_board_width_impl() -> VmResult<i64> {
            with_xiangqi_world(|_world| Ok(XIANGQI_BOARD_WIDTH))
        }

        pub(crate) fn xiangqi_board_width_host(args: &[Value]) -> VmResult<CallOutcome> {
            return_one(xiangqi_board_width(args))
        }

        /// Returns the Xiangqi board height for RustScript scans.
        #[pd_host_function(name = "bevy::Xiangqi::board_height")]
        pub(crate) fn xiangqi_board_height_impl() -> VmResult<i64> {
            with_xiangqi_world(|_world| Ok(XIANGQI_BOARD_HEIGHT))
        }

        pub(crate) fn xiangqi_board_height_host(args: &[Value]) -> VmResult<CallOutcome> {
            return_one(xiangqi_board_height(args))
        }

        /// Reads a Xiangqi cell; out-of-bounds cells return a sentinel value.
        #[pd_host_function(name = "bevy::Xiangqi::cell")]
        pub(crate) fn xiangqi_cell_impl(x: i64, y: i64) -> VmResult<i64> {
            with_xiangqi_world(|world| {
                ensure_xiangqi_resources(world);
                let board = world.resource::<XiangqiBoard>();
                Ok(board.cell(x, y))
            })
        }

        pub(crate) fn xiangqi_cell_host(args: &[Value]) -> VmResult<CallOutcome> {
            return_one(xiangqi_cell(args))
        }

        /// Writes a Xiangqi cell after RustScript has accepted a move.
        #[pd_host_function(name = "bevy::Xiangqi::set_cell")]
        pub(crate) fn xiangqi_set_cell_impl(x: i64, y: i64, piece: i64) -> VmResult<bool> {
            with_xiangqi_world(|world| {
                ensure_xiangqi_resources(world);
                let mut board = world.resource_mut::<XiangqiBoard>();
                Ok(board.set_raw(x, y, piece))
            })
        }

        pub(crate) fn xiangqi_set_cell_host(args: &[Value]) -> VmResult<CallOutcome> {
            return_one(xiangqi_set_cell(args))
        }

        /// Publishes RustScript move legality and winner.
        #[pd_host_function(name = "bevy::Xiangqi::set_move_result")]
        pub(crate) fn xiangqi_set_move_result_impl(legal: bool, winner: i64) -> VmResult<bool> {
            with_xiangqi_world(|world| {
                ensure_xiangqi_resources(world);
                let mut state = world.resource_mut::<XiangqiScriptState>();
                state.legal = legal;
                state.winner = winner;
                Ok(true)
            })
        }

        pub(crate) fn xiangqi_set_move_result_host(args: &[Value]) -> VmResult<CallOutcome> {
            return_one(xiangqi_set_move_result(args))
        }

        /// Publishes the RustScript-selected Xiangqi move.
        #[pd_host_function(name = "bevy::Xiangqi::set_ai_move")]
        pub(crate) fn xiangqi_set_ai_move_impl(
            from_x: i64,
            from_y: i64,
            to_x: i64,
            to_y: i64,
        ) -> VmResult<bool> {
            with_xiangqi_world(|world| {
                ensure_xiangqi_resources(world);
                let mut state = world.resource_mut::<XiangqiScriptState>();
                state.ai_move = Some((from_x, from_y, to_x, to_y));
                Ok(true)
            })
        }

        pub(crate) fn xiangqi_set_ai_move_host(args: &[Value]) -> VmResult<CallOutcome> {
            return_one(xiangqi_set_ai_move(args))
        }
    }
}
