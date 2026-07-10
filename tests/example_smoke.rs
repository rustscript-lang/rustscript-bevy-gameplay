use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
};

fn run_script_smoke_from_empty_cwd(example: &str) -> Output {
    let cwd = std::env::temp_dir().join(format!(
        "rustscript_{example}_script_smoke_{}",
        std::process::id()
    ));
    if cwd.exists() {
        fs::remove_dir_all(&cwd).expect("old smoke cwd should be removable");
    }
    fs::create_dir_all(&cwd).expect("smoke cwd should be created");

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let manifest = manifest_dir.join("Cargo.toml");
    let target_dir = manifest_dir.join("target").join("script-smoke");
    let output = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string()))
        .args([
            "run",
            "--quiet",
            "--manifest-path",
            manifest.to_str().expect("manifest path should be UTF-8"),
            "--target-dir",
            target_dir
                .to_str()
                .expect("target dir path should be UTF-8"),
            "--example",
            example,
            "--",
            "--script-smoke",
        ])
        .current_dir(&cwd)
        .output()
        .expect("example smoke should launch");

    fs::remove_dir_all(&cwd).expect("smoke cwd should be removable");
    output
}

#[test]
fn combat_example_runs_end_to_end() {
    let output = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string()))
        .args(["run", "--quiet", "--example", "combat"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("combat example should launch");

    assert!(
        output.status.success(),
        "combat example failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("applied=12, health=18"),
        "unexpected combat example stdout:\n{stdout}"
    );
}

#[test]
fn shooter_example_script_smoke_runs_end_to_end() {
    let output = run_script_smoke_from_empty_cwd("shooter");

    assert!(
        output.status.success(),
        "shooter smoke failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(
            "player_hp=95, attack=straight:8, projectiles=bolt:1, enemies=7, rewards=2, enemy_rules=2, reward_rules=1, jit_enabled=true, jit_traces="
        ),
        "unexpected shooter example stdout:\n{stdout}"
    );
}

#[test]
fn gomoku_example_script_smoke_runs_end_to_end() {
    let output = run_script_smoke_from_empty_cwd("gomoku");

    assert!(
        output.status.success(),
        "gomoku smoke failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("gomoku_turns=")
            && stdout.contains("stones=")
            && stdout.contains("ai_move_us="),
        "unexpected gomoku example stdout:\n{stdout}"
    );
    assert!(
        !stdout.contains("jit_enabled=") && !stdout.contains("jit_traces="),
        "gomoku smoke should not print JIT fields:\n{stdout}"
    );
}

#[test]
fn xiangqi_example_script_smoke_runs_end_to_end() {
    let output = run_script_smoke_from_empty_cwd("xiangqi");

    assert!(
        output.status.success(),
        "xiangqi smoke failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("xiangqi_turns=")
            && stdout.contains("pieces=")
            && stdout.contains("ai_move_us="),
        "unexpected xiangqi example stdout:\n{stdout}"
    );
    assert!(
        !stdout.contains("jit_enabled=") && !stdout.contains("jit_traces="),
        "xiangqi smoke should not print JIT fields:\n{stdout}"
    );
}
