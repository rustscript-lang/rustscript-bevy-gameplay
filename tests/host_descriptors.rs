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
    HostFunctionSchema, HostTypeSchema, SourceFlavor, Vm, VmError,
    compile_source_with_flavor_and_options,
};

/// Exact hex form of [`bevy_host_catalog`]'s fingerprint (`Display` is 16
/// lowercase hex digits). Bump together with [`BEVY_HOST_CATALOG_FINGERPRINT_U64`]
/// when the guest catalog surface or frozen pd-vm fingerprint encoding changes.
/// See [`bevy_host_catalog`] for the full policy.
const BEVY_HOST_CATALOG_FINGERPRINT_HEX: &str = "61e3eaf5de92afc7";
/// Exact `u64` form of the same digest. Keep in lockstep with the hex snapshot.
const BEVY_HOST_CATALOG_FINGERPRINT_U64: u64 = 0x61e3eaf5de92afc7;

const LISTED_HOST_SOURCE: &str = "use bevy;\nbevy::World::contains_entity();\n";
const UNLISTED_HOST_SOURCE: &str = "use bevy;\nbevy::World::not_a_host();\n";
const LISTED_HOST_NAME: &str = "bevy::World::contains_entity";
const UNLISTED_HOST_NAME: &str = "bevy::World::not_a_host";
const LATE_FAILURE_HOST_NAME: &str = "bevy::Xiangqi::board";

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

fn compile_rss(
    source: &str,
    catalog: Option<std::sync::Arc<HostApiCatalog>>,
) -> Result<vm::CompiledProgram, vm::SourcePathError> {
    let mut options = CompileSourceFileOptions::default();
    if let Some(catalog) = catalog {
        options = options.with_host_api_catalog(catalog);
    }
    compile_source_with_flavor_and_options(source, SourceFlavor::RustScript, options)
}

fn catalog_from_functions(
    functions: impl IntoIterator<Item = HostFunctionSchema>,
) -> HostApiCatalog {
    let production = bevy_host_catalog();
    assert!(
        production.resources().is_empty(),
        "Bevy composition has no guest resources; resource rollback is N/A"
    );
    assert!(
        production.structs().is_empty(),
        "Bevy composition has no named structs; named-struct catalog rollback is N/A"
    );
    let mut builder = HostApiCatalog::builder();
    for function in functions {
        builder.function(function);
    }
    builder.build().expect("test catalog must validate")
}

fn late_failure_xiangqi_catalog() -> HostApiCatalog {
    catalog_from_functions(bevy_host_catalog().functions().iter().map(|function| {
        let mut function = function.clone();
        if function.name == LATE_FAILURE_HOST_NAME {
            assert_eq!(
                function.return_type,
                HostTypeSchema::Array(Box::new(HostTypeSchema::Int)),
                "late-failure fixture must start from the production board return"
            );
            function.return_type = HostTypeSchema::Int;
        }
        function
    }))
}

fn catalog_with_unlisted_host() -> std::sync::Arc<HostApiCatalog> {
    let mut functions: Vec<_> = bevy_host_catalog().functions().to_vec();
    functions.push(HostFunctionSchema::with_return(
        UNLISTED_HOST_NAME,
        Vec::new(),
        HostTypeSchema::Bool,
    ));
    std::sync::Arc::new(catalog_from_functions(functions))
}

fn cargo_lock_package_source(lock: &str, crate_name: &str) -> Option<String> {
    let mut current_name: Option<&str> = None;
    for line in lock.lines() {
        let line = line.trim();
        if line == "[[package]]" {
            current_name = None;
            continue;
        }
        if let Some(name) = line
            .strip_prefix("name = \"")
            .and_then(|rest| rest.strip_suffix('"'))
        {
            current_name = Some(name);
            continue;
        }
        if let Some(source) = line
            .strip_prefix("source = \"")
            .and_then(|rest| rest.strip_suffix('"'))
            && current_name == Some(crate_name)
        {
            return Some(source.to_string());
        }
    }
    None
}

fn assert_bevy_hosts_absent(registry: &HostFunctionRegistry) {
    for name in EXPECTED_HOST_NAMES {
        assert!(
            !registry.contains_name(name),
            "failed composition must not leave {name} installed"
        );
    }
}

fn assert_listed_bind_denied(registry: &HostFunctionRegistry) {
    let compiled = compile_rss(LISTED_HOST_SOURCE, Some(bevy_host_catalog()))
        .expect("listed host compiles against the production catalog");
    assert!(
        compiled
            .program
            .imports
            .iter()
            .any(|import| import.name == LISTED_HOST_NAME),
        "listed compile must record the exact host import, not an unknown-name elision"
    );
    let mut vm = Vm::new(compiled.program);
    match registry.bind_vm_cached(&mut vm) {
        Err(VmError::UnboundImport(name)) => {
            assert_eq!(name, LISTED_HOST_NAME);
        }
        Err(VmError::HostError(message))
            if message.contains("capability profile does not allow") =>
        {
            panic!(
                "listed bind was capability-denied; rollback must drop the registry entry entirely: {message}"
            );
        }
        other => panic!("listed bind must be denied as UnboundImport, got {other:?}"),
    }
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
fn catalog_fingerprint_matches_golden_hex_and_u64() {
    // Snapshot policy is documented on `bevy_host_catalog`: bump both constants
    // together when the guest surface or frozen pd-vm fingerprint encoding
    // changes. Adapter bodies, TLS/HostState, timings, and docs must not.
    let first = compose_catalog();
    let second = compose_catalog();
    let production = bevy_host_catalog();
    assert_eq!(first.fingerprint(), second.fingerprint());
    assert_eq!(first.fingerprint(), production.fingerprint());

    let fingerprint = first.fingerprint();
    assert_eq!(format!("{fingerprint}"), BEVY_HOST_CATALOG_FINGERPRINT_HEX);
    assert_eq!(fingerprint.as_u64(), BEVY_HOST_CATALOG_FINGERPRINT_U64);
    assert_eq!(
        format!("{:016x}", fingerprint.as_u64()),
        BEVY_HOST_CATALOG_FINGERPRINT_HEX
    );

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
fn listed_host_does_not_compile_without_catalog() {
    match compile_rss(LISTED_HOST_SOURCE, None) {
        Err(error) => {
            let message = error.to_string();
            assert!(
                !message.contains("UnboundImport")
                    && !message.to_lowercase().contains("unbound import"),
                "compiler unknown-name failure must not masquerade as a bind error: {message}"
            );
            assert!(
                message.contains(LISTED_HOST_NAME)
                    || message.contains("unknown")
                    || message.contains("not found")
                    || message.contains("host")
                    || message.contains("namespace"),
                "compile-without-catalog error should name the missing host or catalog: {message}"
            );
        }
        Ok(compiled) => {
            assert!(
                compiled
                    .program
                    .host_import_schemas()
                    .iter()
                    .all(|schema| schema
                        .as_ref()
                        .is_none_or(|schema| schema.name != LISTED_HOST_NAME)),
                "without a catalog the listed host must not receive exact import schemas"
            );
        }
    }
}

#[test]
fn listed_host_compiles_against_production_catalog() {
    let compiled = compile_rss(LISTED_HOST_SOURCE, Some(bevy_host_catalog()))
        .expect("listed host compiles against the production catalog");
    assert!(
        compiled
            .program
            .imports
            .iter()
            .any(|import| import.name == LISTED_HOST_NAME && import.arity == 0),
        "production catalog compile must emit the listed host import"
    );
    assert!(
        compiled
            .program
            .host_import_schemas()
            .iter()
            .any(|schema| schema
                .as_ref()
                .is_some_and(|schema| schema.name == LISTED_HOST_NAME)),
        "production catalog compile must emit exact host import schemas"
    );
}

#[test]
fn restricted_registry_denies_listed_host_before_install() {
    let registry = HostFunctionRegistry::restricted();
    assert_bevy_hosts_absent(&registry);
    assert_listed_bind_denied(&registry);
}

#[test]
fn production_install_allows_listed_host_bind() {
    let catalog = bevy_host_catalog();
    let mut registry = HostFunctionRegistry::restricted();
    install_bevy_host_modules(&mut registry, catalog.as_ref()).expect("install composition");
    for name in EXPECTED_HOST_NAMES {
        assert!(
            registry.contains_name(name),
            "restricted registry missing {name}"
        );
    }
    let compiled = compile_rss(LISTED_HOST_SOURCE, Some(catalog))
        .expect("listed host compiles against the production catalog");
    let mut vm = Vm::new(compiled.program);
    registry
        .bind_vm_cached(&mut vm)
        .expect("production install must allow the listed host");
}

#[test]
fn unlisted_host_is_denied_at_bind_when_compilation_reaches_bind() {
    let extra_catalog = catalog_with_unlisted_host();
    let compiled = compile_rss(UNLISTED_HOST_SOURCE, Some(extra_catalog))
        .expect("unlisted host must compile when the catalog is constructed to include it");
    assert!(
        compiled
            .program
            .imports
            .iter()
            .any(|import| import.name == UNLISTED_HOST_NAME),
        "unlisted compile must reach bind with the extra import recorded"
    );

    let production = bevy_host_catalog();
    let mut registry = HostFunctionRegistry::restricted();
    install_bevy_host_modules(&mut registry, production.as_ref()).expect("install composition");
    assert!(
        !registry.contains_name(UNLISTED_HOST_NAME),
        "production registry must not install the unlisted host"
    );

    let mut vm = Vm::new(compiled.program);
    match registry.bind_vm_cached(&mut vm) {
        Err(VmError::UnboundImport(name)) => assert_eq!(name, UNLISTED_HOST_NAME),
        other => panic!("unlisted import must be denied at bind as UnboundImport, got {other:?}"),
    }
}

#[test]
fn transactional_install_rolls_back_when_later_module_schema_does_not_match() {
    let mut registry = HostFunctionRegistry::restricted();
    let named_structs_before = registry.named_struct_schemas().clone();
    let inert = compile_rss("1;\n", None).expect("inert program compiles without a host catalog");
    assert!(
        inert.program.imports.is_empty(),
        "generation probe must not introduce host imports"
    );
    let generation_plan = registry
        .prepare_plan(&inert.program.imports)
        .expect("empty-import plan is the observable generation/capability snapshot");

    let late_failure = late_failure_xiangqi_catalog();
    let error = install_bevy_host_modules(&mut registry, &late_failure)
        .expect_err("install must enter the composition transaction and fail on the last module");
    let message = error.to_string();
    assert!(
        message.contains(LATE_FAILURE_HOST_NAME),
        "late-failure catalog must reach xiangqi schema/adapter mismatch, got: {message}"
    );
    assert!(
        !message.contains("bevy::World::contains_entity")
            && !message.contains("bevy.world")
            && !message.to_lowercase().contains("empty catalog"),
        "failure must not be an early empty-catalog validation miss: {message}"
    );

    assert_bevy_hosts_absent(&registry);
    assert_listed_bind_denied(&registry);
    assert_eq!(
        registry.named_struct_schemas(),
        &named_structs_before,
        "named-struct table must be unchanged after rollback"
    );

    let mut vm = Vm::new(inert.program);
    registry
        .bind_vm_with_plan(&mut vm, &generation_plan)
        .expect("rollback must leave registry generation and capability snapshot unchanged");
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
        let source = cargo_lock_package_source(&cargo_lock, crate_name)
            .unwrap_or_else(|| panic!("{crate_name} must appear with a source in Cargo.lock"));
        assert_eq!(
            source, expected_source,
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
