use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

use bevy_ecs::prelude::*;
use pretty_assertions::assert_eq;
use rustscript_bevy_gameplay::{
    Armor, FROZEN_RUSTSCRIPT_REV, Health, bevy_host_catalog, bevy_host_modules,
    compile_bevy_script, install_bevy_host_modules,
};
use vm::{
    CompileSourceFileOptions, HostApiCatalog, HostFunctionDescriptor, HostFunctionRegistry,
    HostTypeSchema, SourceFlavor, Vm, compile_source_with_flavor_and_options,
};

const EXPECTED_HOST_NAMES: &[&str] = &[
    "bevy::World::contains_entity",
    "bevy::World::get_health",
    "bevy::World::get_armor",
    "bevy::World::set_health",
    "bevy::Shooter::set_player_health",
    "bevy::Shooter::set_player_attack",
    "bevy::Shooter::set_player_projectiles",
    "bevy::Shooter::spawn_enemy",
    "bevy::Shooter::spawn_reward",
    "bevy::Shooter::spawn_enemy_every",
    "bevy::Shooter::spawn_reward_every",
    "bevy::Shooter::spawn_enemy_after_kills",
    "bevy::Gomoku::board",
    "bevy::Gomoku::board_size",
    "bevy::Gomoku::cell",
    "bevy::Gomoku::set_cell",
    "bevy::Gomoku::set_move_result",
    "bevy::Gomoku::set_ai_move",
    "bevy::Xiangqi::board",
    "bevy::Xiangqi::board_width",
    "bevy::Xiangqi::board_height",
    "bevy::Xiangqi::cell",
    "bevy::Xiangqi::set_cell",
    "bevy::Xiangqi::set_move_result",
    "bevy::Xiangqi::set_ai_move",
];

fn compose_catalog() -> HostApiCatalog {
    let descriptors: Vec<_> = bevy_host_modules()
        .iter()
        .flat_map(|module| module.descriptors())
        .collect();
    HostFunctionDescriptor::collect_catalog(&descriptors).expect("composed catalog")
}

fn schema_is_dynamic(schema: &HostTypeSchema) -> bool {
    match schema {
        HostTypeSchema::Unknown | HostTypeSchema::Map(_) => true,
        HostTypeSchema::Array(inner) | HostTypeSchema::Optional(inner) => schema_is_dynamic(inner),
        HostTypeSchema::Callable { params, result } => {
            params.iter().any(schema_is_dynamic) || schema_is_dynamic(result)
        }
        HostTypeSchema::Named { fields, .. } => {
            fields.iter().any(|field| schema_is_dynamic(&field.ty))
        }
        _ => false,
    }
}

fn bundled_rss_scripts() -> Vec<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scripts");
    let mut paths: Vec<PathBuf> = fs::read_dir(&dir)
        .expect("scripts dir")
        .map(|entry| entry.expect("script entry").path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("rss"))
        .collect();
    paths.sort();
    paths
}

#[test]
fn composition_owns_every_host_function_in_declaration_order() {
    let modules = bevy_host_modules();
    assert_eq!(
        modules.iter().map(|module| module.name).collect::<Vec<_>>(),
        ["bevy.world", "bevy.shooter", "bevy.gomoku", "bevy.xiangqi"]
    );

    let names: Vec<String> = modules
        .iter()
        .flat_map(|module| module.descriptors())
        .map(|descriptor| descriptor.schema.name.clone())
        .collect();
    assert_eq!(names, EXPECTED_HOST_NAMES);

    let unique: BTreeSet<_> = names.iter().cloned().collect();
    assert_eq!(
        unique.len(),
        names.len(),
        "host function names must be unique"
    );

    let catalog_names: Vec<String> = bevy_host_catalog()
        .functions()
        .iter()
        .map(|function| function.name.clone())
        .collect();
    assert_eq!(catalog_names, names);
}

#[test]
fn catalog_fingerprint_is_deterministic_and_schema_stable() {
    let first = compose_catalog();
    let second = compose_catalog();
    assert_eq!(first.fingerprint(), second.fingerprint());
    assert_eq!(first.fingerprint(), bevy_host_catalog().fingerprint());
    assert_eq!(format!("{}", first.fingerprint()).len(), 16);

    let board = first
        .functions_named("bevy::Gomoku::board")
        .into_iter()
        .next()
        .expect("gomoku board schema");
    assert_eq!(board.params.len(), 0);
    assert_eq!(
        board.return_type,
        HostTypeSchema::Array(Box::new(HostTypeSchema::Int))
    );

    let health = first
        .functions_named("bevy::World::get_health")
        .into_iter()
        .next()
        .expect("get_health schema");
    assert_eq!(health.params.len(), 0);
    assert_eq!(health.return_type, HostTypeSchema::Int);
}

#[test]
fn dynamic_map_any_unknown_allowlist_is_empty() {
    // Guest host returns are bool, int, or Array(Int). World/entity access is a
    // raw host-private thread-local boundary, not a guest Map/Unknown slot.
    let catalog = bevy_host_catalog();
    let mut dynamic = Vec::new();
    for function in catalog.functions() {
        if schema_is_dynamic(&function.return_type) {
            dynamic.push(format!("{} return", function.name));
        }
        for param in &function.params {
            if schema_is_dynamic(&param.ty) {
                dynamic.push(format!("{} param {}", function.name, param.name));
            }
        }
    }
    assert_eq!(dynamic, Vec::<String>::new());
}

#[test]
fn exact_restricted_binding_installs_composition_and_rejects_undeclared_hosts() {
    let catalog = bevy_host_catalog();
    let mut registry = HostFunctionRegistry::restricted();
    install_bevy_host_modules(&mut registry, catalog.as_ref()).expect("install composition");
    for name in EXPECTED_HOST_NAMES {
        assert!(
            registry.contains_name(name),
            "restricted registry missing {name}"
        );
    }
    match compile_source_with_flavor_and_options(
        "use bevy;\nbevy::World::not_a_host();\n",
        SourceFlavor::RustScript,
        CompileSourceFileOptions::default().with_host_api_catalog(catalog.clone()),
    ) {
        Err(_) => {}
        Ok(compiled) => {
            let mut vm = Vm::new(compiled.program);
            match registry.bind_vm_cached(&mut vm) {
                Err(_) => {}
                Ok(()) => {
                    assert!(
                        vm.run().is_err(),
                        "undeclared host must not execute through exact binding"
                    );
                }
            }
        }
    }

    let compiled = compile_source_with_flavor_and_options(
        r#"
use bevy;
bevy::World::contains_entity();
"#,
        SourceFlavor::RustScript,
        CompileSourceFileOptions::default().with_host_api_catalog(catalog.clone()),
    )
    .expect("script compiles against production catalog");
    let mut vm = Vm::new(compiled.program);
    registry
        .bind_vm_cached(&mut vm)
        .expect("exact bind against composed catalog");
}

#[test]
fn transactional_install_rolls_back_when_catalog_does_not_match() {
    let mut registry = HostFunctionRegistry::restricted();
    let empty = HostApiCatalog::builder()
        .build()
        .expect("empty catalog builds");
    let error = install_bevy_host_modules(&mut registry, &empty)
        .expect_err("install against an empty catalog must fail");
    let _ = error;
    for name in EXPECTED_HOST_NAMES {
        assert!(
            !registry.contains_name(name),
            "failed composition must not leave {name} installed"
        );
    }
}

#[test]
fn rustscript_crates_are_pinned_to_the_frozen_full_sha() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let cargo_toml = fs::read_to_string(manifest_dir.join("Cargo.toml")).expect("Cargo.toml");
    let cargo_lock = fs::read_to_string(manifest_dir.join("Cargo.lock")).expect("Cargo.lock");
    assert!(
        cargo_toml.contains(FROZEN_RUSTSCRIPT_REV),
        "Cargo.toml must pin the frozen full SHA"
    );
    assert!(
        !cargo_toml.contains("path = \"../rustscript"),
        "production RustScript crates must not use sibling path deps"
    );
    let expected_source = format!(
        "git+https://github.com/rustscript-lang/rustscript?rev={FROZEN_RUSTSCRIPT_REV}#{FROZEN_RUSTSCRIPT_REV}"
    );
    for crate_name in ["pd-vm", "pd-host-function", "pd-host-schema"] {
        assert!(
            cargo_lock.contains(&format!("name = \"{crate_name}\"")),
            "{crate_name} must appear in Cargo.lock"
        );
        assert!(
            cargo_lock.contains(&expected_source),
            "{crate_name} must resolve to the frozen git SHA"
        );
    }
}

#[test]
fn bundled_rss_examples_compile_through_the_production_catalog() {
    let scripts = bundled_rss_scripts();
    assert_eq!(
        scripts
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect::<Vec<_>>(),
        [
            "damage_formula.rss",
            "gomoku_ai.rss",
            "gomoku_move.rss",
            "shooter_game.rss",
            "xiangqi_ai.rss",
            "xiangqi_move.rss",
        ]
    );
    for path in scripts {
        compile_rss_file(&path);
    }
}

#[test]
fn exact_binding_runs_damage_formula_against_live_ecs() {
    let mut world = World::new();
    world.insert_resource(
        rustscript_bevy_gameplay::DamageRules::from_source(include_str!(
            "../scripts/damage_formula.rss"
        ))
        .expect("damage formula compiles through the production catalog"),
    );
    let entity = world.spawn((Health(30), Armor(4))).id();
    let applied = rustscript_bevy_gameplay::apply_scripted_damage(&mut world, entity, 10, false)
        .expect("damage applies through exact composed binding");
    assert_eq!(applied, 6);
    assert_eq!(world.get::<Health>(entity).unwrap().0, 24);
}

fn compile_rss_file(path: &Path) {
    let source =
        fs::read_to_string(path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()));
    let wrapped = match path.file_name().and_then(|name| name.to_str()) {
        Some("damage_formula.rss") => {
            format!("let incoming = 0;\nlet critical = false;\n{source}")
        }
        Some("gomoku_move.rss") => {
            format!("let move_x: int = 0;\nlet move_y: int = 0;\nlet player: int = 1;\n{source}")
        }
        Some("gomoku_ai.rss") => {
            format!("let ai_player: int = 1;\nlet ai_bias: int = 0;\n{source}")
        }
        Some("xiangqi_move.rss") => format!(
            "let from_x: int = 0;\nlet from_y: int = 0;\nlet to_x: int = 0;\nlet to_y: int = 0;\nlet player: int = 1;\n{source}"
        ),
        Some("xiangqi_ai.rss") => {
            format!("let ai_player: int = 1;\nlet ai_bias: int = 0;\n{source}")
        }
        _ => source,
    };
    compile_bevy_script(&wrapped)
        .unwrap_or_else(|err| panic!("{} failed to compile: {err}", path.display()));
}

#[test]
fn composed_registry_executes_contains_entity_without_world_context_error_shape() {
    // Without a live Bevy context the host still binds exactly; the raw
    // world/entity boundary reports a host error instead of an unbound import.
    let catalog = bevy_host_catalog();
    let compiled = compile_source_with_flavor_and_options(
        "use bevy;\nbevy::World::contains_entity();\n",
        SourceFlavor::RustScript,
        CompileSourceFileOptions::default().with_host_api_catalog(catalog.clone()),
    )
    .expect("compile");
    let mut vm = Vm::new(compiled.program);
    let mut registry = HostFunctionRegistry::restricted();
    install_bevy_host_modules(&mut registry, catalog.as_ref()).expect("install");
    registry.bind_vm_cached(&mut vm).expect("bind");
    let error = vm.run().expect_err("missing world context is a host error");
    assert!(
        error.to_string().contains("missing Bevy World context"),
        "unexpected error: {error}"
    );
}
