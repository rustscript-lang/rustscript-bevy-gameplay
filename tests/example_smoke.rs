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
            "player_hp=95, attack=straight:8, projectiles=bolt:1, enemies=4, rewards=2, enemy_rules=2, reward_rules=1"
        ),
        "unexpected shooter example stdout:\n{stdout}"
    );
}
