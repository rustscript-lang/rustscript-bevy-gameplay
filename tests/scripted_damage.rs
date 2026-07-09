use bevy_ecs::prelude::*;
use pretty_assertions::assert_eq;
use rustscript_bevy_gameplay::{Armor, DamageRules, Health, apply_scripted_damage};

#[test]
fn rustscript_damage_formula_reads_and_writes_bevy_world() {
    let mut world = World::new();
    world.insert_resource(
        DamageRules::from_source(include_str!("../scripts/damage_formula.rss"))
            .expect("rules should compile"),
    );
    let entity = world.spawn((Health(30), Armor(4))).id();

    let applied =
        apply_scripted_damage(&mut world, entity, 10, false).expect("damage should apply");

    assert_eq!(applied, 6);
    assert_eq!(world.get::<Health>(entity).unwrap().0, 24);
}

#[test]
fn rustscript_keeps_bevy_world_write_in_critical_formula() {
    let mut world = World::new();
    world.insert_resource(
        DamageRules::from_source(include_str!("../scripts/damage_formula.rss"))
            .expect("rules should compile"),
    );
    let entity = world.spawn((Health(30), Armor(4))).id();

    let applied = apply_scripted_damage(&mut world, entity, 10, true).expect("damage should apply");

    assert_eq!(applied, 12);
    assert_eq!(world.get::<Health>(entity).unwrap().0, 18);
}

#[test]
fn rustscript_inline_formula_uses_bevy_namespace_hosts() {
    let mut world = World::new();
    world.insert_resource(
        DamageRules::from_source(
            r#"
use bevy;
let armor: int = bevy::World::get_armor();
let current_health: int = bevy::World::get_health();
let raw = incoming - armor;
let crit_bonus = if critical => { raw } else => { 0 };
let applied = raw + crit_bonus;
let updated: bool = bevy::World::set_health(current_health - applied);
applied;
"#,
        )
        .expect("rules should compile"),
    );
    let entity = world.spawn((Health(30), Armor(4))).id();

    let applied = apply_scripted_damage(&mut world, entity, 10, true).expect("damage should apply");

    assert_eq!(applied, 12);
    assert_eq!(world.get::<Health>(entity).unwrap().0, 18);
}
