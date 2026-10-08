//! ECS data access for RSS gameplay. Rules and balancing live in scripts/.
use super::*;
use pd_host_function::pd_host_function;
#[allow(unused_imports)]
use vm::{arg, borrow_arg, take_arg};

#[derive(Component, Debug, Clone, Default)]
pub struct ShooterData(pub HashMap<String, f64>);

#[derive(Component, Debug, Clone)]
pub struct ShooterProjectile {
    pub kind: String,
    pub owner: String,
    pub hits: Vec<Entity>,
}

#[derive(Resource, Debug, Clone, Default)]
pub struct ShooterFrame(pub HashMap<String, f64>);

/// Fractional millisecond storage; public spawn rules retain their integer API.
#[derive(Resource, Debug, Clone, Default)]
pub(crate) struct ShooterRuleClocks(pub HashMap<usize, f64>);

#[derive(Resource, Debug, Clone, Default)]
pub struct ShooterEffects(pub Vec<(String, Position)>);

thread_local! {
    static FRAME_VMS: RefCell<HashMap<String, Vm>> = RefCell::new(HashMap::new());
}

/// Compile once per edited source, retaining JIT traces between frames.
pub fn run_shooter_frame_script(
    world: &mut World,
    source: &str,
) -> Result<ShooterJitSummary, String> {
    with_shooter_context(world, || {
        FRAME_VMS.with(|cache| {
            let mut cache = cache.borrow_mut();
            if !cache.contains_key(source) {
                let compiled = compile_bevy_script(source).map_err(|err| err.to_string())?;
                let mut machine = Vm::new_with_jit_config(
                    compiled.program.with_local_count(compiled.locals),
                    shooter_jit_config(),
                );
                bind_composed_bevy_hosts(&mut machine)?;
                if cache.len() >= 16 {
                    cache.clear();
                }
                cache.insert(source.to_owned(), machine);
            }
            let machine = cache.get_mut(source).unwrap();
            machine.reset_for_reuse().map_err(|err| err.to_string())?;
            let status = machine.run().map_err(|err| err.to_string())?;
            if status != VmStatus::Halted {
                return Err(format!("shooter frame did not halt: {status:?}"));
            }
            let jit = machine.jit_snapshot();
            Ok(ShooterJitSummary {
                enabled: jit.config.enabled,
                trace_count: jit.traces.len(),
            })
        })
    })
}

pub fn debug_shooter_script(
    world: &mut World,
    source: &str,
    debugger: &mut Debugger,
) -> Result<(), String> {
    with_shooter_context(world, || {
        let compiled = compile_bevy_script(source).map_err(|err| err.to_string())?;
        let mut machine = Vm::new(compiled.program.with_local_count(compiled.locals));
        bind_composed_bevy_hosts(&mut machine)?;
        let status = machine
            .run_with_debugger(debugger)
            .map_err(|err| err.to_string())?;
        if status != VmStatus::Halted {
            return Err(format!("debugger did not halt: {status:?}"));
        }
        Ok(())
    })
}

/// Copy gameplay data for an isolated debugger invocation.
pub fn snapshot_shooter_world(world: &mut World) -> World {
    let mut copy = World::new();
    if let Some(frame) = world.get_resource::<ShooterFrame>() {
        copy.insert_resource(frame.clone());
    }
    if let Some(rules) = world.get_resource::<ShooterSpawnRules>() {
        copy.insert_resource(rules.clone());
    }
    if let Some(clocks) = world.get_resource::<ShooterRuleClocks>() {
        copy.insert_resource(clocks.clone());
    }
    let ids = gameplay_entities(world);
    let mut remap = HashMap::new();
    for &id in &ids {
        let mut target = copy.spawn_empty();
        remap.insert(id, target.id());
        macro_rules! clone_component {
            ($ty:ty) => {
                if let Some(value) = world.get::<$ty>(id) {
                    target.insert(value.clone());
                }
            };
        }
        clone_component!(Player);
        clone_component!(Enemy);
        clone_component!(Health);
        clone_component!(AttackStyle);
        clone_component!(AttackPower);
        clone_component!(AttackCooldownMs);
        clone_component!(PlayerProjectileLoadout);
        clone_component!(Position);
        clone_component!(Velocity);
        clone_component!(RewardItem);
        clone_component!(ShooterData);
    }
    for id in ids {
        if let Some(projectile) = world.get::<ShooterProjectile>(id) {
            let hits = projectile
                .hits
                .iter()
                .filter_map(|hit| remap.get(hit).copied())
                .collect();
            copy.entity_mut(remap[&id]).insert(ShooterProjectile {
                hits,
                ..projectile.clone()
            });
        }
    }
    copy
}

fn gameplay_entities(world: &mut World) -> Vec<Entity> {
    world
        .query_filtered::<Entity, Or<(
            With<Player>,
            With<Enemy>,
            With<RewardItem>,
            With<ShooterProjectile>,
        )>>()
        .iter(world)
        .collect()
}

fn entity(id: i64) -> Entity {
    Entity::try_from_bits(id as u64).unwrap_or(Entity::PLACEHOLDER)
}

pub fn number(world: &World, id: Entity, field: &str) -> f64 {
    match field {
        "x" => world.get::<Position>(id).map(|p| p.x as f64),
        "y" => world.get::<Position>(id).map(|p| p.y as f64),
        "vx" => world.get::<Velocity>(id).map(|p| p.x as f64),
        "vy" => world.get::<Velocity>(id).map(|p| p.y as f64),
        "health" => world.get::<Health>(id).map(|p| p.0 as f64),
        "power" => world.get::<AttackPower>(id).map(|p| p.0 as f64),
        "cooldown" => world.get::<AttackCooldownMs>(id).map(|p| p.0 as f64),
        "count" => world
            .get::<PlayerProjectileLoadout>(id)
            .map(|p| p.count as f64),
        "amount" => world.get::<RewardItem>(id).map(|p| p.amount as f64),
        _ => world
            .get::<ShooterData>(id)
            .and_then(|data| data.0.get(field).copied()),
    }
    .unwrap_or(0.0)
}

pub(crate) mod host {
    use super::*;

    fn entities_contract() -> vm::HostFunctionSchema {
        vm::HostFunctionSchema::with_return(
            "bevy::Shooter::entities",
            Vec::new(),
            HostTypeSchema::Array(Box::new(HostTypeSchema::Int)),
        )
    }

    /// Snapshots the IDs of gameplay entities for RSS iteration.
    #[pd_host_function(name = "bevy::Shooter::entities", contract = entities_contract)]
    pub(crate) fn entities_impl() -> VmResult<Value> {
        with_shooter_world(|world| {
            Ok(Value::array(
                gameplay_entities(world)
                    .into_iter()
                    .map(|id| Value::Int(id.to_bits() as i64))
                    .collect(),
            ))
        })
    }

    /// Returns the current number of gameplay entities.
    #[pd_host_function(name = "bevy::Shooter::entity_count")]
    pub(crate) fn entity_count_impl() -> VmResult<i64> {
        with_shooter_world(|world| Ok(gameplay_entities(world).len() as i64))
    }

    /// Reads a numeric component field, or frame state when id is -1.
    #[pd_host_function(name = "bevy::Shooter::get")]
    pub(crate) fn get_impl(id: i64, field: &str) -> VmResult<f64> {
        with_shooter_world(|world| {
            Ok(if id == -1 {
                world
                    .get_resource::<ShooterFrame>()
                    .and_then(|frame| frame.0.get(field).copied())
                    .unwrap_or(0.0)
            } else {
                number(world, entity(id), field)
            })
        })
    }

    /// Writes a numeric component field, or frame state when id is -1.
    #[pd_host_function(name = "bevy::Shooter::set")]
    pub(crate) fn set_impl(id: i64, field: &str, value: f64) -> VmResult<bool> {
        with_shooter_world(|world| {
            if id == -1 {
                world
                    .get_resource_or_insert_with(ShooterFrame::default)
                    .0
                    .insert(field.to_owned(), value);
                return Ok(true);
            }
            let id = entity(id);
            if world.get_entity(id).is_err() {
                return Ok(false);
            }
            match field {
                "x" | "y" => {
                    let mut p = world
                        .get_mut::<Position>(id)
                        .ok_or_else(|| VmError::HostError("missing Position".into()))?;
                    if field == "x" {
                        p.x = value as f32;
                    } else {
                        p.y = value as f32;
                    }
                }
                "vx" | "vy" => {
                    if world.get::<Velocity>(id).is_none() {
                        world.entity_mut(id).insert(Velocity { x: 0.0, y: 0.0 });
                    }
                    let mut p = world.get_mut::<Velocity>(id).unwrap();
                    if field == "vx" {
                        p.x = value as f32;
                    } else {
                        p.y = value as f32;
                    }
                }
                "health" => {
                    world.entity_mut(id).insert(Health(value as i64));
                }
                "power" => {
                    world.entity_mut(id).insert(AttackPower(value as i64));
                }
                "count" => {
                    if let Some(mut loadout) = world.get_mut::<PlayerProjectileLoadout>(id) {
                        loadout.count = value as i64;
                    }
                }
                _ => {
                    if world.get::<ShooterData>(id).is_none() {
                        world.entity_mut(id).insert(ShooterData::default());
                    }
                    world
                        .get_mut::<ShooterData>(id)
                        .unwrap()
                        .0
                        .insert(field.to_owned(), value);
                }
            }
            Ok(true)
        })
    }

    /// Reads an entity role, kind, owner, attack style or loadout.
    #[pd_host_function(name = "bevy::Shooter::text")]
    pub(crate) fn text_impl(id: i64, field: &str) -> VmResult<String> {
        with_shooter_world(|world| {
            let id = entity(id);
            Ok(match field {
                "role" if world.get::<Player>(id).is_some() => "player".to_owned(),
                "role" if world.get::<Enemy>(id).is_some() => "enemy".to_owned(),
                "role" if world.get::<RewardItem>(id).is_some() => "reward".to_owned(),
                "role" if world.get::<ShooterProjectile>(id).is_some() => "projectile".to_owned(),
                "kind" => world
                    .get::<Enemy>(id)
                    .map(|p| p.kind.clone())
                    .or_else(|| world.get::<RewardItem>(id).map(|p| p.kind.clone()))
                    .or_else(|| world.get::<ShooterProjectile>(id).map(|p| p.kind.clone()))
                    .unwrap_or_default(),
                "owner" => world
                    .get::<ShooterProjectile>(id)
                    .map(|p| p.owner.clone())
                    .unwrap_or_default(),
                "style" => world
                    .get::<AttackStyle>(id)
                    .map(|p| p.0.clone())
                    .unwrap_or_default(),
                "loadout" => world
                    .get::<PlayerProjectileLoadout>(id)
                    .map(|p| p.kind.clone())
                    .unwrap_or_default(),
                _ => String::new(),
            })
        })
    }

    /// Creates a projectile with the position, velocity and damage supplied by RSS.
    #[pd_host_function(name = "bevy::Shooter::projectile")]
    pub(crate) fn projectile_impl(
        owner: &str,
        kind: &str,
        x: f64,
        y: f64,
        vx: f64,
        vy: f64,
        damage: f64,
    ) -> VmResult<i64> {
        with_shooter_world(|world| {
            Ok(world
                .spawn((
                    Position {
                        x: x as f32,
                        y: y as f32,
                    },
                    Velocity {
                        x: vx as f32,
                        y: vy as f32,
                    },
                    ShooterProjectile {
                        owner: owner.to_owned(),
                        kind: kind.to_owned(),
                        hits: Vec::new(),
                    },
                    ShooterData(HashMap::from([("damage".into(), damage)])),
                ))
                .id()
                .to_bits() as i64)
        })
    }

    /// Creates a reward entity using the script's selected data.
    #[pd_host_function(name = "bevy::Shooter::reward")]
    pub(crate) fn reward_impl(kind: &str, amount: f64, x: f64, y: f64) -> VmResult<bool> {
        with_shooter_world(|world| {
            let id = spawn_reward_entity(world, kind, amount as i64, x as i64, y as i64);
            world.entity_mut(id).insert(Position {
                x: x as f32,
                y: y as f32,
            });
            Ok(true)
        })
    }

    /// Removes the requested ECS entity.
    #[pd_host_function(name = "bevy::Shooter::despawn")]
    pub(crate) fn despawn_impl(id: i64) -> VmResult<bool> {
        with_shooter_world(|world| Ok(world.despawn(entity(id))))
    }

    /// Records a target in the projectile hit set; returns false for existing entries.
    #[pd_host_function(name = "bevy::Shooter::mark_hit")]
    pub(crate) fn mark_hit_impl(id: i64, target: i64) -> VmResult<bool> {
        with_shooter_world(|world| {
            let Some(mut projectile) = world.get_mut::<ShooterProjectile>(entity(id)) else {
                return Ok(false);
            };
            let target = entity(target);
            if projectile.hits.contains(&target) {
                return Ok(false);
            }
            projectile.hits.push(target);
            Ok(true)
        })
    }

    /// Queues a presentation effect at the coordinates supplied by RSS.
    #[pd_host_function(name = "bevy::Shooter::effect")]
    pub(crate) fn effect_impl(kind: &str, x: f64, y: f64) -> VmResult<bool> {
        with_shooter_world(|world| {
            world
                .get_resource_or_insert_with(ShooterEffects::default)
                .0
                .push((
                    kind.to_owned(),
                    Position {
                        x: x as f32,
                        y: y as f32,
                    },
                ));
            Ok(true)
        })
    }

    /// Returns the number of enemy and reward spawn rules.
    #[pd_host_function(name = "bevy::Shooter::rule_count")]
    pub(crate) fn rule_count_impl() -> VmResult<i64> {
        with_shooter_world(|world| {
            Ok(world
                .get_resource::<ShooterSpawnRules>()
                .map(|r| r.enemies.len() + r.rewards.len())
                .unwrap_or(0) as i64)
        })
    }

    fn trigger(rules: &ShooterSpawnRules, index: usize) -> Option<&ShooterSpawnTrigger> {
        if index < rules.enemies.len() {
            Some(&rules.enemies[index].trigger)
        } else {
            rules
                .rewards
                .get(index - rules.enemies.len())
                .map(|r| &r.trigger)
        }
    }
    fn trigger_mut(
        rules: &mut ShooterSpawnRules,
        index: usize,
    ) -> Option<&mut ShooterSpawnTrigger> {
        if index < rules.enemies.len() {
            Some(&mut rules.enemies[index].trigger)
        } else {
            let index = index - rules.enemies.len();
            rules.rewards.get_mut(index).map(|r| &mut r.trigger)
        }
    }

    /// Reads raw timer or kill-counter data from a spawn rule.
    #[pd_host_function(name = "bevy::Shooter::rule_get")]
    pub(crate) fn rule_get_impl(index: i64, field: &str) -> VmResult<f64> {
        with_shooter_world(|world| {
            if field == "elapsed"
                && let Some(value) = world
                    .get_resource::<ShooterRuleClocks>()
                    .and_then(|clocks| clocks.0.get(&(index as usize)))
            {
                return Ok(*value);
            }
            Ok(world
                .get_resource::<ShooterSpawnRules>()
                .and_then(|r| trigger(r, index as usize))
                .map(|t| match (t, field) {
                    (ShooterSpawnTrigger::EveryMs { interval_ms, .. }, "interval") => {
                        *interval_ms as f64
                    }
                    (ShooterSpawnTrigger::EveryMs { elapsed_ms, .. }, "elapsed") => {
                        *elapsed_ms as f64
                    }
                    (ShooterSpawnTrigger::AfterKills { kill_count, .. }, "kills") => {
                        *kill_count as f64
                    }
                    (ShooterSpawnTrigger::AfterKills { kills_seen, .. }, "seen") => {
                        *kills_seen as f64
                    }
                    (ShooterSpawnTrigger::AfterKills { fired, .. }, "fired") => {
                        f64::from(u8::from(*fired))
                    }
                    _ => 0.0,
                })
                .unwrap_or(0.0))
        })
    }

    /// Stores timer or kill-counter data calculated by RSS.
    #[pd_host_function(name = "bevy::Shooter::rule_set")]
    pub(crate) fn rule_set_impl(index: i64, field: &str, value: f64) -> VmResult<bool> {
        with_shooter_world(|world| {
            if let Some(mut rules) = world.get_resource_mut::<ShooterSpawnRules>()
                && let Some(t) = trigger_mut(&mut rules, index as usize)
            {
                match (t, field) {
                    (ShooterSpawnTrigger::EveryMs { elapsed_ms, .. }, "elapsed") => {
                        *elapsed_ms = value as i64
                    }
                    (ShooterSpawnTrigger::AfterKills { kills_seen, .. }, "seen") => {
                        *kills_seen = value as i64
                    }
                    (ShooterSpawnTrigger::AfterKills { fired, .. }, "fired") => {
                        *fired = value != 0.0
                    }
                    _ => return Ok(false),
                }
                if field == "elapsed" {
                    world
                        .get_resource_or_insert_with(ShooterRuleClocks::default)
                        .0
                        .insert(index as usize, value);
                }
                return Ok(true);
            }
            Ok(false)
        })
    }

    /// Creates the entity described by a registered spawn rule.
    #[pd_host_function(name = "bevy::Shooter::rule_spawn")]
    pub(crate) fn rule_spawn_impl(index: i64) -> VmResult<bool> {
        with_shooter_world(|world| {
            let Some(rules) = world.get_resource::<ShooterSpawnRules>() else {
                return Ok(false);
            };
            let index = index as usize;
            if index < rules.enemies.len() {
                let r = rules.enemies[index].clone();
                spawn_enemy_entity(world, &r.kind, r.health, &r.attack_style, r.x, r.y);
            } else if let Some(r) = rules.rewards.get(index - rules.enemies.len()) {
                let r = r.clone();
                spawn_reward_entity(world, &r.kind, r.amount, r.x, r.y);
            } else {
                return Ok(false);
            }
            Ok(true)
        })
    }
}
