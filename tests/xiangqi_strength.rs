use pretty_assertions::{assert_eq, assert_ne};
use rustscript_bevy_gameplay::{
    XiangqiAiMove, XiangqiBoard, apply_xiangqi_move_script, choose_xiangqi_ai_move,
    reset_xiangqi_board,
};

const MOVE_SCRIPT: &str = include_str!("../scripts/xiangqi_move.rss");
const AI_SCRIPT: &str = include_str!("../scripts/xiangqi_ai.rss");

const RED: i64 = 1;
const BLACK: i64 = -1;
const RED_GENERAL: i64 = 1;
const RED_HORSE: i64 = 4;
const RED_CHARIOT: i64 = 5;
const RED_CANNON: i64 = 6;
const RED_SOLDIER: i64 = 7;
const BLACK_GENERAL: i64 = -1;
const BLACK_HORSE: i64 = -4;
const BLACK_CHARIOT: i64 = -5;

fn seeded_world(pieces: &[(i64, i64, i64)]) -> bevy_ecs::world::World {
    let mut world = bevy_ecs::world::World::new();
    reset_xiangqi_board(&mut world);
    {
        let mut board = world.resource_mut::<XiangqiBoard>();
        board.clear_for_test();
        for &(x, y, piece) in pieces {
            board.set_for_test(x, y, piece);
        }
    }
    world
}

fn choose(world: &mut bevy_ecs::world::World, side: i64, label: &str) -> XiangqiAiMove {
    let ai_move = choose_xiangqi_ai_move(world, AI_SCRIPT, side)
        .unwrap_or_else(|error| panic!("{label}: AI failed: {error}"));
    eprintln!(
        "{label}: {} us, {} compiled traces, move ({}, {}) -> ({}, {})",
        ai_move.telemetry.elapsed_micros,
        ai_move.telemetry.jit_trace_count,
        ai_move.from_x,
        ai_move.from_y,
        ai_move.to_x,
        ai_move.to_y,
    );
    assert!(
        ai_move.telemetry.jit_enabled,
        "{label}: JIT must be enabled"
    );
    assert!(
        ai_move.telemetry.elapsed_micros < 8_000_000,
        "{label}: move exceeded the interactive latency budget"
    );
    ai_move
}

fn apply_ai_move(
    world: &mut bevy_ecs::world::World,
    side: i64,
    ai_move: &XiangqiAiMove,
) -> rustscript_bevy_gameplay::XiangqiMoveSummary {
    apply_xiangqi_move_script(
        world,
        MOVE_SCRIPT,
        ai_move.from_x,
        ai_move.from_y,
        ai_move.to_x,
        ai_move.to_y,
        side,
    )
    .expect("move rules should run")
}

#[test]
fn ai_script_uses_one_board_snapshot_and_no_per_cell_hosts() {
    let code_lines: Vec<_> = AI_SCRIPT
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("//"))
        .collect();

    assert_eq!(code_lines[0], "use bevy;");
    assert_eq!(
        code_lines[1],
        "let mut board: [int] = bevy::Xiangqi::board();"
    );
    assert_eq!(code_lines[2], "assert(board.length == 90);");
    assert_eq!(AI_SCRIPT.matches("bevy::Xiangqi::board()").count(), 1);
    assert_eq!(AI_SCRIPT.matches("bevy::Xiangqi::set_ai_move(").count(), 1);
    assert_eq!(
        AI_SCRIPT.matches("bevy::Xiangqi::").count(),
        2,
        "AI scripts may only snapshot the board and publish the final move"
    );
    assert!(!AI_SCRIPT.contains("bevy::Xiangqi::cell("));
    assert!(!AI_SCRIPT.contains("bevy::Xiangqi::set_cell("));
    assert!(!AI_SCRIPT.contains("board.copy()"));
    assert!(!AI_SCRIPT.contains("fn find_general("));
    assert!(!AI_SCRIPT.contains("-> [int]"));
    assert!(AI_SCRIPT.contains("fn find_general_square(board: [int], side: int) -> int"));

    for helper in [
        "count_between",
        "pseudo_legal",
        "find_general_square",
        "is_attacked",
        "legal_after_trial",
        "side_in_check",
        "evaluate",
        "has_legal_move",
        "capture_with_recapture",
        "tactical_extension",
        "move_order_score",
    ] {
        let needle = format!("{helper}(");
        for line in AI_SCRIPT.lines().filter(|line| line.contains(&needle)) {
            if !line.trim_start().starts_with("fn ") {
                assert!(
                    line.contains("&board"),
                    "{helper} must borrow the local board: {line}"
                );
            }
        }
    }
}

#[test]
fn takes_an_unprotected_chariot() {
    let mut world = seeded_world(&[
        (4, 9, RED_GENERAL),
        (4, 0, BLACK_GENERAL),
        (4, 5, RED_SOLDIER),
        (0, 2, BLACK_CHARIOT),
        (0, 7, RED_CHARIOT),
    ]);

    let ai_move = choose(&mut world, BLACK, "hanging-chariot");
    assert_eq!(
        (ai_move.from_x, ai_move.from_y, ai_move.to_x, ai_move.to_y),
        (0, 2, 0, 7)
    );
}

#[test]
fn rejects_a_poisoned_cannon_exchange() {
    let mut world = seeded_world(&[
        (4, 9, RED_GENERAL),
        (4, 0, BLACK_GENERAL),
        (4, 5, RED_SOLDIER),
        (0, 2, BLACK_CHARIOT),
        (0, 5, RED_CANNON),
        (0, 7, RED_CHARIOT),
    ]);

    let ai_move = choose(&mut world, BLACK, "poisoned-cannon");
    assert_ne!(
        (ai_move.from_x, ai_move.from_y, ai_move.to_x, ai_move.to_y),
        (0, 2, 0, 5),
        "a chariot must not be traded for a defended cannon"
    );
    assert!(apply_ai_move(&mut world, BLACK, &ai_move).legal);
}

#[test]
fn finds_a_profitable_horse_recapture() {
    let mut world = seeded_world(&[
        (4, 9, RED_GENERAL),
        (4, 0, BLACK_GENERAL),
        (4, 5, RED_SOLDIER),
        (1, 3, BLACK_HORSE),
        (0, 5, RED_CHARIOT),
    ]);

    let ai_move = choose(&mut world, BLACK, "horse-recapture");
    assert_eq!(
        (ai_move.from_x, ai_move.from_y, ai_move.to_x, ai_move.to_y),
        (1, 3, 0, 5)
    );
}

#[test]
fn answers_check_instead_of_searching_irrelevant_material() {
    let mut world = seeded_world(&[
        (4, 9, RED_GENERAL),
        (4, 5, RED_SOLDIER),
        (4, 0, BLACK_GENERAL),
        (3, 0, RED_CHARIOT),
        (8, 0, RED_CANNON),
    ]);

    let ai_move = choose(&mut world, BLACK, "forced-defense");
    let summary = apply_ai_move(&mut world, BLACK, &ai_move);
    assert!(summary.legal);
    assert_eq!(world.resource::<XiangqiBoard>().cell(3, 0), BLACK_GENERAL);
}

#[test]
fn recognizes_stalemate_as_a_win() {
    let mut world = seeded_world(&[
        (4, 9, RED_GENERAL),
        (4, 0, BLACK_GENERAL),
        (3, 2, RED_CHARIOT),
        (5, 2, RED_CHARIOT),
        (4, 3, RED_SOLDIER),
    ]);

    let ai_move = choose(&mut world, RED, "stalemate-in-one");
    let summary = apply_ai_move(&mut world, RED, &ai_move);
    assert!(summary.legal);
    assert_eq!(summary.winner, RED, "the selected move must end the game");
}

#[test]
fn finds_a_quiet_checkmate_in_one() {
    let mut world = seeded_world(&[
        (4, 9, RED_GENERAL),
        (4, 0, BLACK_GENERAL),
        (0, 2, RED_CHARIOT),
        (1, 1, RED_HORSE),
        (7, 1, RED_HORSE),
        (4, 5, RED_SOLDIER),
    ]);

    let ai_move = choose(&mut world, RED, "quiet-mate");
    assert_eq!(
        (ai_move.from_x, ai_move.from_y, ai_move.to_x, ai_move.to_y),
        (0, 2, 4, 2)
    );
    let summary = apply_ai_move(&mut world, RED, &ai_move);
    assert!(summary.legal);
    assert_eq!(summary.winner, RED);
}

#[test]
fn never_moves_a_pinned_chariot_off_the_general_file() {
    let mut world = seeded_world(&[
        (3, 9, RED_GENERAL),
        (4, 0, BLACK_GENERAL),
        (4, 8, RED_CHARIOT),
        (4, 4, BLACK_CHARIOT),
    ]);

    let ai_move = choose(&mut world, BLACK, "pinned-chariot");
    let summary = apply_ai_move(&mut world, BLACK, &ai_move);
    assert!(
        summary.legal,
        "AI helper and move rules must agree on legality"
    );
    if ai_move.from_x == 4 && ai_move.from_y == 4 {
        assert_eq!(
            ai_move.to_x, 4,
            "the pinned chariot must remain on its file"
        );
    }
}

#[test]
fn terminal_position_reports_no_available_move_instead_of_invalid_coordinates() {
    let mut world = seeded_world(&[
        (4, 0, BLACK_GENERAL),
        (4, 9, RED_GENERAL),
        (3, 1, RED_SOLDIER),
        (4, 1, RED_SOLDIER),
        (5, 1, RED_SOLDIER),
    ]);

    let error = choose_xiangqi_ai_move(&mut world, AI_SCRIPT, BLACK)
        .expect_err("a terminal position must not publish an invalid move");
    assert!(
        error.contains("did not select a move"),
        "unexpected error: {error}"
    );
}
