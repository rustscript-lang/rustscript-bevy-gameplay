use pretty_assertions::assert_eq;
use rustscript_bevy_gameplay::{
    GomokuBoard, apply_gomoku_move_script, choose_gomoku_ai_move, choose_gomoku_ai_move_with_bias,
    reset_gomoku_board,
};

const MOVE_SCRIPT: &str = include_str!("../scripts/gomoku_move.rss");
const AI_SCRIPT: &str = include_str!("../scripts/gomoku_ai.rss");

fn seeded_world(stones: &[(i64, i64, i64)]) -> bevy_ecs::world::World {
    let mut world = bevy_ecs::world::World::new();
    reset_gomoku_board(&mut world);
    {
        let mut board = world.resource_mut::<GomokuBoard>();
        for &(x, y, stone) in stones {
            board.set_for_test(x, y, stone);
        }
    }
    world
}

#[test]
fn rustscript_move_rejects_occupied_and_out_of_bounds_points() {
    let mut world = seeded_world(&[(7, 7, 1)]);

    let occupied =
        apply_gomoku_move_script(&mut world, MOVE_SCRIPT, 7, 7, 2).expect("move script should run");
    let out_of_bounds = apply_gomoku_move_script(&mut world, MOVE_SCRIPT, -1, 0, 1)
        .expect("move script should run");

    assert!(!occupied.legal);
    assert_eq!(occupied.winner, 0);
    assert!(!out_of_bounds.legal);
    assert_eq!(world.resource::<GomokuBoard>().cell(7, 7), 1);
}

#[test]
fn rustscript_move_places_stone_and_detects_horizontal_win() {
    let mut world = seeded_world(&[(3, 8, 1), (4, 8, 1), (5, 8, 1), (6, 8, 1)]);

    let summary =
        apply_gomoku_move_script(&mut world, MOVE_SCRIPT, 7, 8, 1).expect("move script should run");

    assert!(summary.legal);
    assert_eq!(summary.winner, 1);
    assert!(!summary.draw);
    assert_eq!(world.resource::<GomokuBoard>().cell(7, 8), 1);
}

#[test]
fn rustscript_ai_finishes_its_own_open_four() {
    let mut world = seeded_world(&[(5, 7, 2), (6, 7, 2), (7, 7, 2), (8, 7, 2)]);

    let ai_move =
        choose_gomoku_ai_move(&mut world, AI_SCRIPT, 2).expect("AI script should choose a move");

    assert_eq!((ai_move.x, ai_move.y), (9, 7));
    assert!(ai_move.telemetry.jit_enabled);
    assert!(
        ai_move.telemetry.jit_trace_count > 0,
        "AI scan loops should produce JIT traces"
    );
    assert!(
        ai_move.telemetry.elapsed_micros > 0,
        "AI move should report elapsed time"
    );
}

#[test]
fn rustscript_ai_reuses_cached_jit_traces_for_same_script() {
    let script = AI_SCRIPT.replacen("use bevy;", "use bevy;\nlet cache_probe: int = 0;", 1);
    let mut world = seeded_world(&[(5, 7, 2), (6, 7, 2), (7, 7, 2), (8, 7, 2)]);

    let first =
        choose_gomoku_ai_move(&mut world, &script, 2).expect("AI script should choose a move");
    let second =
        choose_gomoku_ai_move(&mut world, &script, 2).expect("AI script should choose a move");
    let third =
        choose_gomoku_ai_move(&mut world, &script, 2).expect("AI script should choose a move");

    assert_eq!((first.x, first.y), (9, 7));
    assert_eq!((second.x, second.y), (9, 7));
    assert_eq!((third.x, third.y), (9, 7));
    assert!(third.telemetry.jit_enabled);
    assert!(
        third.telemetry.jit_trace_count > 0,
        "AI scan loops should produce cached JIT traces"
    );
    assert_eq!(
        second.telemetry.jit_trace_count, third.telemetry.jit_trace_count,
        "same script and same board should reuse already compiled traces"
    );
}

#[test]
fn rustscript_ai_can_read_bias_parameter() {
    let script = "use bevy;\nlet x: int = if ai_bias > 0 => { 8 } else => { 6 };\nbevy::Gomoku::set_ai_move(x, 7);\nx;";
    let mut world = seeded_world(&[]);

    let cautious = choose_gomoku_ai_move_with_bias(&mut world, script, 2, -100)
        .expect("AI script should choose a move");
    let aggressive = choose_gomoku_ai_move_with_bias(&mut world, script, 2, 100)
        .expect("AI script should choose a move");

    assert_eq!((cautious.x, cautious.y), (6, 7));
    assert_eq!((aggressive.x, aggressive.y), (8, 7));
}

#[test]
fn rustscript_ai_blocks_player_open_four() {
    let mut world = seeded_world(&[(5, 7, 1), (6, 7, 1), (7, 7, 1), (8, 7, 1)]);

    let ai_move =
        choose_gomoku_ai_move(&mut world, AI_SCRIPT, 2).expect("AI script should choose a move");

    assert_eq!((ai_move.x, ai_move.y), (9, 7));
    assert!(ai_move.telemetry.jit_enabled);
}

#[test]
fn rustscript_ai_completes_a_broken_four() {
    let mut world = seeded_world(&[(5, 7, 2), (6, 7, 2), (8, 7, 2), (9, 7, 2)]);

    let ai_move =
        choose_gomoku_ai_move(&mut world, AI_SCRIPT, 2).expect("AI script should choose a move");

    assert_eq!((ai_move.x, ai_move.y), (7, 7));
}

#[test]
fn rustscript_ai_blocks_a_broken_four() {
    let mut world = seeded_world(&[(5, 7, 1), (6, 7, 1), (8, 7, 1), (9, 7, 1)]);

    let ai_move =
        choose_gomoku_ai_move(&mut world, AI_SCRIPT, 2).expect("AI script should choose a move");

    assert_eq!((ai_move.x, ai_move.y), (7, 7));
}

#[test]
fn rustscript_ai_creates_a_double_three() {
    let mut world = seeded_world(&[(6, 7, 2), (8, 7, 2), (7, 6, 2), (7, 8, 2)]);

    let ai_move =
        choose_gomoku_ai_move(&mut world, AI_SCRIPT, 2).expect("AI script should choose a move");

    assert_eq!((ai_move.x, ai_move.y), (7, 7));
}

#[test]
fn scripted_human_sequence_can_play_actual_turns_against_ai() {
    let mut world = seeded_world(&[]);
    let human_moves = [(7, 7), (6, 6), (8, 6), (6, 8), (8, 8), (5, 7)];
    let mut winner = 0;
    let mut turns = 0;

    for &(x, y) in &human_moves {
        if world.resource::<GomokuBoard>().cell(x, y) != 0 {
            continue;
        }
        let human = apply_gomoku_move_script(&mut world, MOVE_SCRIPT, x, y, 1)
            .expect("human script move should run");
        assert!(human.legal);
        turns += 1;
        winner = human.winner;
        if winner != 0 || human.draw {
            break;
        }

        let ai_move =
            choose_gomoku_ai_move(&mut world, AI_SCRIPT, 2).expect("AI script should choose");
        let ai = apply_gomoku_move_script(&mut world, MOVE_SCRIPT, ai_move.x, ai_move.y, 2)
            .expect("AI script move should run");
        assert!(ai.legal);
        turns += 1;
        winner = ai.winner;
        if winner != 0 || ai.draw {
            break;
        }
    }

    let stones = world
        .resource::<GomokuBoard>()
        .cells()
        .iter()
        .filter(|&&stone| stone != 0)
        .count();
    assert_eq!(stones, turns);
    assert!(turns >= 4);
    assert!(winner == 0 || winner == 1 || winner == 2);
}
