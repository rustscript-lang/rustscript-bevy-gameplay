use bevy_ecs::prelude::*;
use pretty_assertions::assert_eq;
use rustscript_bevy_gameplay::{
    AttackPower, AttackStyle, Enemy, Health, Player, PlayerProjectileLoadout, Position, RewardItem,
    apply_shooter_script,
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
    assert_eq!(summary.enemies_spawned, 4);
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
    assert_eq!(enemies.len(), 4);
    assert!(enemies.iter().all(|(_, _, _, power, _)| power.0 <= 4));
    assert!(enemies.iter().any(|(enemy, health, style, _, position)| {
        enemy.kind == "bomber" && health.0 == 42 && style.0 == "burst" && position.y == 450.0
    }));

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
    assert_eq!(summary.enemies_spawned, 1);
    assert_eq!(summary.rewards_spawned, 1);

    let mut enemies = world.query::<(&Enemy, &Health, &AttackStyle)>();
    let spawned = enemies.iter(&world).collect::<Vec<_>>();
    assert_eq!(spawned.len(), 1);
    assert_eq!(spawned[0].0.kind, "ace");
    assert_eq!(spawned[0].1.0, 55);
    assert_eq!(spawned[0].2.0, "wave");

    let mut rewards = world.query::<(&RewardItem, &Position)>();
    let spawned_rewards = rewards.iter(&world).collect::<Vec<_>>();
    assert_eq!(spawned_rewards.len(), 1);
    assert_eq!(spawned_rewards[0].0.kind, "health");
    assert_eq!(spawned_rewards[0].0.amount, 25);
    assert_eq!(spawned_rewards[0].1.y, -360.0);
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
