use pretty_assertions::assert_eq;
use rustscript_bevy_gameplay::{
    XiangqiBoard, apply_xiangqi_move_script, choose_xiangqi_ai_move, reset_xiangqi_board,
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
const BLACK_ADVISOR: i64 = -2;
const BLACK_HORSE: i64 = -4;
const BLACK_CHARIOT: i64 = -5;
const BLACK_CANNON: i64 = -6;
const BLACK_SOLDIER: i64 = -7;

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

#[test]
fn rustscript_move_rejects_blocked_horse_leg() {
    let mut world = seeded_world(&[
        (4, 9, RED_GENERAL),
        (4, 0, BLACK_GENERAL),
        (1, 9, RED_HORSE),
        (1, 8, RED_SOLDIER),
    ]);

    let summary = apply_xiangqi_move_script(&mut world, MOVE_SCRIPT, 1, 9, 2, 7, RED)
        .expect("move script should run");

    assert!(!summary.legal);
    assert_eq!(world.resource::<XiangqiBoard>().cell(1, 9), RED_HORSE);
    assert_eq!(world.resource::<XiangqiBoard>().cell(2, 7), 0);
}

#[test]
fn rustscript_move_allows_cannon_capture_with_one_screen() {
    let mut world = seeded_world(&[
        (4, 9, RED_GENERAL),
        (4, 0, BLACK_GENERAL),
        (4, 5, RED_SOLDIER),
        (1, 7, RED_CANNON),
        (1, 5, RED_SOLDIER),
        (1, 3, BLACK_HORSE),
    ]);

    let summary = apply_xiangqi_move_script(&mut world, MOVE_SCRIPT, 1, 7, 1, 3, RED)
        .expect("move script should run");

    assert!(summary.legal);
    assert_eq!(summary.winner, 0);
    assert_eq!(world.resource::<XiangqiBoard>().cell(1, 7), 0);
    assert_eq!(world.resource::<XiangqiBoard>().cell(1, 3), RED_CANNON);
}

#[test]
fn rustscript_move_rejects_generals_facing_after_move() {
    let mut world = seeded_world(&[
        (4, 9, RED_GENERAL),
        (4, 0, BLACK_GENERAL),
        (4, 5, RED_CHARIOT),
    ]);

    let summary = apply_xiangqi_move_script(&mut world, MOVE_SCRIPT, 4, 5, 5, 5, RED)
        .expect("move script should run");

    assert!(!summary.legal);
    assert_eq!(world.resource::<XiangqiBoard>().cell(4, 5), RED_CHARIOT);
    assert_eq!(world.resource::<XiangqiBoard>().cell(5, 5), 0);
}

#[test]
fn rustscript_move_rejects_exposing_own_general_to_a_chariot() {
    let mut world = seeded_world(&[
        (4, 9, RED_GENERAL),
        (4, 0, BLACK_GENERAL),
        (4, 3, BLACK_CHARIOT),
        (4, 7, RED_CHARIOT),
    ]);

    let summary = apply_xiangqi_move_script(&mut world, MOVE_SCRIPT, 4, 7, 5, 7, RED)
        .expect("move script should run");

    assert!(!summary.legal);
    assert_eq!(world.resource::<XiangqiBoard>().cell(4, 7), RED_CHARIOT);
}

#[test]
fn rustscript_move_requires_a_response_to_horse_check() {
    let mut world = seeded_world(&[
        (4, 9, RED_GENERAL),
        (4, 0, BLACK_GENERAL),
        (4, 5, RED_SOLDIER),
        (3, 7, BLACK_HORSE),
        (0, 9, RED_CHARIOT),
    ]);

    let ignored = apply_xiangqi_move_script(&mut world, MOVE_SCRIPT, 0, 9, 0, 8, RED)
        .expect("move script should run");
    assert!(!ignored.legal);

    let escaped = apply_xiangqi_move_script(&mut world, MOVE_SCRIPT, 4, 9, 5, 9, RED)
        .expect("move script should run");
    assert!(escaped.legal);
}

#[test]
fn rustscript_move_awards_a_stalemate_to_the_moving_side() {
    let mut world = seeded_world(&[
        (4, 9, RED_GENERAL),
        (4, 0, BLACK_GENERAL),
        (3, 1, RED_SOLDIER),
        (5, 1, RED_SOLDIER),
        (4, 2, RED_SOLDIER),
        (0, 9, RED_HORSE),
    ]);

    let summary = apply_xiangqi_move_script(&mut world, MOVE_SCRIPT, 0, 9, 1, 7, RED)
        .expect("move script should run");

    assert!(summary.legal);
    assert_eq!(summary.winner, RED);
}

#[test]
fn rustscript_ai_captures_general_when_available() {
    let mut world = seeded_world(&[
        (4, 9, RED_GENERAL),
        (4, 0, BLACK_GENERAL),
        (4, 4, BLACK_CHARIOT),
    ]);

    let ai_move = choose_xiangqi_ai_move(&mut world, AI_SCRIPT, BLACK)
        .expect("AI script should choose a move");

    assert_eq!(
        (ai_move.from_x, ai_move.from_y, ai_move.to_x, ai_move.to_y),
        (4, 4, 4, 9)
    );
    assert!(ai_move.telemetry.jit_enabled);
    assert!(ai_move.telemetry.jit_trace_count > 0);
    assert!(ai_move.telemetry.elapsed_micros > 0);
}

#[test]
fn rustscript_ai_reuses_cached_jit_traces_for_same_script() {
    let script = AI_SCRIPT.replacen("use bevy;", "use bevy;\nlet cache_probe: int = 0;", 1);
    let mut world = seeded_world(&[
        (4, 9, RED_GENERAL),
        (4, 0, BLACK_GENERAL),
        (4, 4, BLACK_CHARIOT),
    ]);

    let mut moves = Vec::new();
    for _ in 0..5 {
        moves.push(
            choose_xiangqi_ai_move(&mut world, &script, BLACK)
                .expect("AI script should choose a move"),
        );
    }

    let first = moves.first().expect("AI script should run at least once");
    let last = moves.last().expect("AI script should run at least once");
    assert_eq!(
        (first.from_x, first.from_y, first.to_x, first.to_y),
        (4, 4, 4, 9)
    );
    assert_eq!(
        (last.from_x, last.from_y, last.to_x, last.to_y),
        (4, 4, 4, 9)
    );
    assert!(last.telemetry.jit_enabled);
    assert!(
        last.telemetry.jit_trace_count > 0,
        "AI search loops should produce cached JIT traces"
    );
    assert!(
        moves
            .windows(2)
            .all(|pair| pair[1].telemetry.jit_trace_count >= pair[0].telemetry.jit_trace_count),
        "cached trace count should never reset for the same script"
    );
    assert!(
        last.telemetry.jit_trace_count > first.telemetry.jit_trace_count,
        "same cached VM should keep warming new trace roots across repeated runs"
    );
}

#[test]
fn rustscript_ai_uses_general_to_answer_close_threat() {
    let mut world = seeded_world(&[
        (4, 9, RED_GENERAL),
        (4, 5, BLACK_SOLDIER),
        (4, 0, BLACK_GENERAL),
        (3, 0, RED_CHARIOT),
        (5, 0, BLACK_ADVISOR),
    ]);

    let ai_move = choose_xiangqi_ai_move(&mut world, AI_SCRIPT, BLACK)
        .expect("AI script should choose a move");

    assert_eq!(
        (ai_move.from_x, ai_move.from_y, ai_move.to_x, ai_move.to_y),
        (4, 0, 3, 0)
    );
}

#[test]
fn rustscript_ai_avoids_opening_an_immediate_general_loss() {
    let mut world = seeded_world(&[
        (8, 9, RED_GENERAL),
        (4, 8, RED_CHARIOT),
        (3, 5, RED_HORSE),
        (4, 5, BLACK_SOLDIER),
        (4, 0, BLACK_GENERAL),
        (1, 2, BLACK_CANNON),
    ]);

    let black_move = choose_xiangqi_ai_move(&mut world, AI_SCRIPT, BLACK)
        .expect("AI script should choose a move");
    let black_summary = apply_xiangqi_move_script(
        &mut world,
        MOVE_SCRIPT,
        black_move.from_x,
        black_move.from_y,
        black_move.to_x,
        black_move.to_y,
        BLACK,
    )
    .expect("black move script should run");
    assert!(black_summary.legal);

    let red_reply = choose_xiangqi_ai_move(&mut world, AI_SCRIPT, RED)
        .expect("red AI script should choose a reply");
    let red_summary = apply_xiangqi_move_script(
        &mut world,
        MOVE_SCRIPT,
        red_reply.from_x,
        red_reply.from_y,
        red_reply.to_x,
        red_reply.to_y,
        RED,
    )
    .expect("red reply script should run");

    assert_ne!(red_summary.winner, RED);
}

#[test]
fn scripted_ai_sequence_uses_only_legal_moves() {
    let mut world = seeded_world(&[
        (4, 9, RED_GENERAL),
        (4, 0, BLACK_GENERAL),
        (0, 9, RED_CHARIOT),
        (8, 0, BLACK_CHARIOT),
        (1, 9, RED_HORSE),
        (7, 0, BLACK_HORSE),
        (4, 6, RED_SOLDIER),
        (4, 3, BLACK_SOLDIER),
    ]);
    let mut winner = 0;
    let mut turns = 0;

    for side in [RED, BLACK, RED, BLACK] {
        let ai_move =
            choose_xiangqi_ai_move(&mut world, AI_SCRIPT, side).expect("AI script should choose");
        let ai = apply_xiangqi_move_script(
            &mut world,
            MOVE_SCRIPT,
            ai_move.from_x,
            ai_move.from_y,
            ai_move.to_x,
            ai_move.to_y,
            side,
        )
        .expect("AI move script should run");
        assert!(ai.legal);
        winner = ai.winner;
        turns += 1;
        if winner != 0 {
            break;
        }
    }

    assert!(turns >= 2);
    assert!(winner == 0 || winner == RED || winner == BLACK);
}
