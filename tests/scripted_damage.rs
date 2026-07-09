use bevy_ecs::prelude::*;
use pretty_assertions::assert_eq;
use rustscript_bevy_gameplay::{Armor, DamageRules, Health, apply_scripted_damage};

#[test]
fn rustscript_damage_formula_calls_bevy_component_host_function() {
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
fn rustscript_keeps_bevy_component_host_function_in_critical_formula() {
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
fn rustscript_can_call_bevy_armor_from_inline_formula() {
    let mut world = World::new();
    world.insert_resource(
        DamageRules::from_source(
            r#"
fn bevy_armor() -> int;
fn damage_floor(damage) -> int;

let raw = incoming - bevy_armor();
let crit_bonus = if critical => { raw } else => { 0 };
let damage = raw + crit_bonus;
damage_floor(damage);
"#,
        )
        .expect("rules should compile"),
    );
    let entity = world.spawn((Health(30), Armor(4))).id();

    let applied = apply_scripted_damage(&mut world, entity, 10, true).expect("damage should apply");

    assert_eq!(applied, 12);
    assert_eq!(world.get::<Health>(entity).unwrap().0, 18);
}
