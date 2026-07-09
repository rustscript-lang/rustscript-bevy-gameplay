use std::process::Command;

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
    let output = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string()))
        .args([
            "run",
            "--quiet",
            "--example",
            "shooter",
            "--",
            "--script-smoke",
        ])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("shooter example smoke should launch");

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
    let output = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string()))
        .args([
            "run",
            "--quiet",
            "--example",
            "gomoku",
            "--",
            "--script-smoke",
        ])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("gomoku example smoke should launch");

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
            && stdout.contains("jit_enabled=true")
            && stdout.contains("jit_traces=")
            && stdout.contains("ai_move_us="),
        "unexpected gomoku example stdout:\n{stdout}"
    );
    let traces = stdout
        .split("jit_traces=")
        .nth(1)
        .and_then(|tail| tail.split(',').next())
        .and_then(|value| value.trim().parse::<usize>().ok())
        .expect("gomoku smoke should report numeric JIT traces");
    assert!(
        traces < 50,
        "gomoku smoke should report compiled traces for the latest AI script, not an accumulated run count: {stdout}"
    );
}

#[test]
fn xiangqi_example_script_smoke_runs_end_to_end() {
    let output = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string()))
        .args([
            "run",
            "--quiet",
            "--example",
            "xiangqi",
            "--",
            "--script-smoke",
        ])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("xiangqi example smoke should launch");

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
            && stdout.contains("jit_enabled=true")
            && stdout.contains("jit_traces=")
            && stdout.contains("ai_move_us="),
        "unexpected xiangqi example stdout:\n{stdout}"
    );
}
