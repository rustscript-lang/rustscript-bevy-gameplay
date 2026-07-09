use bevy_ecs::prelude::*;
use pretty_assertions::assert_eq;
use rustscript_bevy_gameplay::{
    AttackPower, AttackStyle, Enemy, Health, Player, PlayerProjectileLoadout, Position, RewardItem,
    ScriptManagedEnemy, ScriptManagedReward, ShooterSpawnRules, apply_shooter_script,
    tick_shooter_spawn_rules,
};

#[test]
fn rustscript_spawns_player_and_enemy_waves() {
    let mut world = World::new();

    let summary = apply_shooter_script(&mut world, include_str!("../scripts/shooter_game.rss"))
        .expect("shooter script should apply");

    assert_eq!(summary.player_health, 95);
    assert_eq!(summary.player_attack_style, "straight");
    assert_eq!(summary.player_attack_power, 8);
    assert_eq!(summary.player_projectile_kind, "bolt");
    assert_eq!(summary.player_projectile_count, 1);
    assert_eq!(summary.enemies_spawned, 7);
    assert_eq!(summary.rewards_spawned, 2);

    let (_, health, style, power, loadout) = world
        .query::<(
            &Player,
            &Health,
            &AttackStyle,
            &AttackPower,
            &PlayerProjectileLoadout,
        )>()
        .single(&world)
        .expect("player should exist");
    assert_eq!(health.0, 95);
    assert_eq!(style.0, "straight");
    assert_eq!(power.0, 8);
    assert_eq!(loadout.kind, "bolt");
    assert_eq!(loadout.count, 1);

    let mut enemy_query = world.query::<(&Enemy, &Health, &AttackStyle, &AttackPower, &Position)>();
    let enemies = enemy_query.iter(&world).collect::<Vec<_>>();
    assert_eq!(enemies.len(), 7);
    assert!(enemies.iter().all(|(_, _, _, power, _)| power.0 <= 4));
    assert!(enemies.iter().any(|(enemy, health, style, _, position)| {
        enemy.kind == "bomber" && health.0 == 30 && style.0 == "burst" && position.y == 450.0
    }));
    assert!(enemies.iter().any(|(enemy, health, style, _, position)| {
        enemy.kind == "sniper" && health.0 == 24 && style.0 == "rail" && position.y == 390.0
    }));
    assert!(
        enemies
            .iter()
            .any(|(enemy, health, style, _, _)| enemy.kind == "carrier"
                && health.0 == 55
                && style.0 == "burst")
    );
    assert!(
        enemies
            .iter()
            .any(|(enemy, health, style, _, _)| enemy.kind == "striker"
                && health.0 == 18
                && style.0 == "flak")
    );

    let mut rewards = world.query::<(&RewardItem, &Position)>();
    let rewards = rewards.iter(&world).collect::<Vec<_>>();
    assert_eq!(rewards.len(), 2);
    assert!(
        rewards
            .iter()
            .any(|(reward, position)| reward.kind == "bullets"
                && reward.amount == 1
                && position.y < 0.0)
    );
    assert!(
        rewards
            .iter()
            .any(|(reward, _)| reward.kind == "health" && reward.amount == 20)
    );
}

#[test]
fn reapplying_script_updates_live_world_without_recreating_player() {
    let mut world = World::new();
    apply_shooter_script(&mut world, include_str!("../scripts/shooter_game.rss"))
        .expect("initial script should apply");
    let player_before = world
        .query::<(Entity, &Player)>()
        .single(&world)
        .expect("player should exist")
        .0;

    let updated = r#"
use bevy;
let hp: bool = bevy::Shooter::set_player_health(77);
let attack: bool = bevy::Shooter::set_player_attack("laser", 31, 90);
let projectiles: bool = bevy::Shooter::set_player_projectiles("missile", 3);
let enemy: bool = bevy::Shooter::spawn_enemy("ace", 55, "wave", 0, 470);
let reward: bool = bevy::Shooter::spawn_reward("health", 25, 40, -360);
true;
"#;
    let summary = apply_shooter_script(&mut world, updated).expect("updated script should apply");

    let (player_after, _, health, style, power, loadout) = world
        .query::<(
            Entity,
            &Player,
            &Health,
            &AttackStyle,
            &AttackPower,
            &PlayerProjectileLoadout,
        )>()
        .single(&world)
        .expect("player should still exist");
    assert_eq!(player_after, player_before);
    assert_eq!(health.0, 77);
    assert_eq!(style.0, "laser");
    assert_eq!(power.0, 31);
    assert_eq!(loadout.kind, "missile");
    assert_eq!(loadout.count, 3);
    assert_eq!(summary.enemies_spawned, 8);
    assert_eq!(summary.rewards_spawned, 3);

    let mut enemies = world.query::<(&Enemy, &Health, &AttackStyle)>();
    let spawned = enemies.iter(&world).collect::<Vec<_>>();
    assert_eq!(spawned.len(), 8);
    assert!(
        spawned
            .iter()
            .any(|(enemy, health, style)| enemy.kind == "ace"
                && health.0 == 55
                && style.0 == "wave")
    );

    let mut rewards = world.query::<(&RewardItem, &Position)>();
    let spawned_rewards = rewards.iter(&world).collect::<Vec<_>>();
    assert_eq!(spawned_rewards.len(), 3);
    assert!(
        spawned_rewards
            .iter()
            .any(|(reward, position)| reward.kind == "health"
                && reward.amount == 25
                && position.y == -360.0)
    );
}

#[test]
fn reapplying_script_keeps_existing_script_spawned_entities() {
    let mut world = World::new();
    apply_shooter_script(&mut world, include_str!("../scripts/shooter_game.rss"))
        .expect("initial script should apply");

    let updated = r#"
use bevy;
let hp: bool = bevy::Shooter::set_player_health(77);
let enemy: bool = bevy::Shooter::spawn_enemy("ace", 55, "wave", 0, 470);
let reward: bool = bevy::Shooter::spawn_reward("health", 25, 40, -360);
true;
"#;
    let summary = apply_shooter_script(&mut world, updated).expect("updated script should apply");

    assert_eq!(summary.enemies_spawned, 8);
    assert_eq!(summary.rewards_spawned, 3);

    let mut enemies = world.query::<&Enemy>();
    let enemy_kinds = enemies
        .iter(&world)
        .map(|enemy| enemy.kind.as_str())
        .collect::<Vec<_>>();
    assert_eq!(enemy_kinds.len(), 8);
    assert!(enemy_kinds.contains(&"bomber"));
    assert!(enemy_kinds.contains(&"ace"));

    let script_enemy_count = world
        .query_filtered::<Entity, With<ScriptManagedEnemy>>()
        .iter(&world)
        .count();
    assert_eq!(script_enemy_count, 8);

    let reward_count = world
        .query_filtered::<Entity, With<ScriptManagedReward>>()
        .iter(&world)
        .count();
    assert_eq!(reward_count, 3);
}

#[test]
fn projectile_count_is_clamped_to_a_playable_range() {
    let mut world = World::new();
    let source = r#"
use bevy;
let projectiles: bool = bevy::Shooter::set_player_projectiles("laser", 99);
true;
"#;

    let summary = apply_shooter_script(&mut world, source).expect("script should apply");

    assert_eq!(summary.player_projectile_kind, "laser");
    assert_eq!(summary.player_projectile_count, 5);
}

#[test]
fn script_can_register_timed_and_kill_spawn_rules() {
    let mut world = World::new();
    let source = r#"
use bevy;
let hp: bool = bevy::Shooter::set_player_health(95);
let timed_enemy: bool = bevy::Shooter::spawn_enemy_every("scout", 18, "straight", -120, 520, 2000);
let timed_reward: bool = bevy::Shooter::spawn_reward_every("health", 20, 120, -260, 3000);
let boss: bool = bevy::Shooter::spawn_enemy_after_kills("boss", 120, "burst", 0, 540, 3);
true;
"#;

    let summary = apply_shooter_script(&mut world, source).expect("script should apply");

    assert_eq!(summary.enemies_spawned, 0);
    assert_eq!(summary.rewards_spawned, 0);
    let rules = world
        .get_resource::<ShooterSpawnRules>()
        .expect("script should install spawn rules");
    assert_eq!(rules.enemies.len(), 2);
    assert_eq!(rules.rewards.len(), 1);
}

#[test]
fn spawn_rules_tick_on_intervals_and_kill_counts() {
    let mut world = World::new();
    let source = r#"
use bevy;
let hp: bool = bevy::Shooter::set_player_health(95);
let timed_enemy: bool = bevy::Shooter::spawn_enemy_every("scout", 18, "straight", -120, 520, 2000);
let timed_reward: bool = bevy::Shooter::spawn_reward_every("bullets", 1, 120, -260, 3000);
let boss: bool = bevy::Shooter::spawn_enemy_after_kills("boss", 120, "burst", 0, 540, 3);
true;
"#;
    apply_shooter_script(&mut world, source).expect("script should apply");

    let first = tick_shooter_spawn_rules(&mut world, 1999, 2);
    assert_eq!(first.enemies_spawned, 0);
    assert_eq!(first.rewards_spawned, 0);

    let second = tick_shooter_spawn_rules(&mut world, 1, 1);
    assert_eq!(second.enemies_spawned, 2);
    assert_eq!(second.rewards_spawned, 0);

    let third = tick_shooter_spawn_rules(&mut world, 1000, 0);
    assert_eq!(third.enemies_spawned, 0);
    assert_eq!(third.rewards_spawned, 1);

    let enemies = world.query::<&Enemy>().iter(&world).collect::<Vec<_>>();
    assert_eq!(enemies.len(), 2);
    assert!(enemies.iter().any(|enemy| enemy.kind == "boss"));
    assert!(enemies.iter().any(|enemy| enemy.kind == "scout"));
}
