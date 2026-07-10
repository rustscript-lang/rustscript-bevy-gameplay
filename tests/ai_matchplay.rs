#![cfg(not(debug_assertions))]

use rustscript_bevy_gameplay::{
    GomokuBoard, apply_gomoku_move_script, apply_xiangqi_move_script, choose_gomoku_ai_move,
    choose_xiangqi_ai_move, reset_gomoku_board, reset_xiangqi_board,
};

const GOMOKU_MOVE: &str = include_str!("../scripts/gomoku_move.rss");
const GOMOKU_AI: &str = include_str!("../scripts/gomoku_ai.rss");
const XIANGQI_MOVE: &str = include_str!("../scripts/xiangqi_move.rss");
const XIANGQI_AI: &str = include_str!("../scripts/xiangqi_ai.rss");

#[test]
#[ignore = "perf"]
fn perf_gomoku_ai_self_play_is_legal_deterministic_and_responsive() {
    let mut first = bevy_ecs::world::World::new();
    let mut second = bevy_ecs::world::World::new();
    reset_gomoku_board(&mut first);
    reset_gomoku_board(&mut second);

    let first_move = choose_gomoku_ai_move(&mut first, GOMOKU_AI, 1).expect("first AI move");
    let repeated_move = choose_gomoku_ai_move(&mut second, GOMOKU_AI, 1).expect("repeated AI move");
    assert_eq!(
        (first_move.x, first_move.y),
        (repeated_move.x, repeated_move.y),
        "the same position must produce the same move"
    );

    let mut world = bevy_ecs::world::World::new();
    reset_gomoku_board(&mut world);
    let mut plies = 0_u128;
    let mut ai_micros = 0_u128;
    for ply in 0..32 {
        let side = if ply % 2 == 0 { 1 } else { 2 };
        let ai = choose_gomoku_ai_move(&mut world, GOMOKU_AI, side).expect("AI move");
        ai_micros += ai.telemetry.elapsed_micros;
        let summary = apply_gomoku_move_script(&mut world, GOMOKU_MOVE, ai.x, ai.y, side)
            .expect("AI move should execute");
        assert!(summary.legal, "AI selected an illegal point at ply {ply}");
        plies += 1;
        if summary.winner != 0 || summary.draw {
            break;
        }
    }

    let stones = world
        .resource::<GomokuBoard>()
        .cells()
        .iter()
        .filter(|&&stone| stone != 0)
        .count() as u128;
    assert_eq!(stones, plies);
    assert!(
        plies >= 18,
        "self-play ended before exercising midgame search"
    );
    let average_micros = ai_micros / plies;
    eprintln!("gomoku self-play: {plies} plies, average AI move {average_micros} us");
    assert!(
        average_micros < 2_500_000,
        "average Gomoku AI move took {average_micros} us"
    );
}

#[test]
#[ignore = "perf"]
fn perf_xiangqi_ai_self_play_produces_legal_moves_with_bounded_latency() {
    let mut world = bevy_ecs::world::World::new();
    reset_xiangqi_board(&mut world);
    let mut plies = 0_u128;
    let mut ai_micros = 0_u128;

    for ply in 0..10 {
        let side = if ply % 2 == 0 { 1 } else { -1 };
        let ai = choose_xiangqi_ai_move(&mut world, XIANGQI_AI, side).expect("AI move");
        ai_micros += ai.telemetry.elapsed_micros;
        let summary = apply_xiangqi_move_script(
            &mut world,
            XIANGQI_MOVE,
            ai.from_x,
            ai.from_y,
            ai.to_x,
            ai.to_y,
            side,
        )
        .expect("AI move should execute");
        assert!(summary.legal, "AI selected an illegal move at ply {ply}");
        plies += 1;
        if summary.winner != 0 {
            break;
        }
    }

    assert!(
        plies >= 6,
        "self-play ended before exercising opening search"
    );
    let average_micros = ai_micros / plies;
    eprintln!("xiangqi self-play: {plies} plies, average AI move {average_micros} us");
    assert!(
        average_micros < 5_500_000,
        "average Xiangqi AI move took {average_micros} us"
    );
}
