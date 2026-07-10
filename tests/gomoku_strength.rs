use rustscript_bevy_gameplay::{GomokuBoard, choose_gomoku_ai_move, reset_gomoku_board};

const AI_SCRIPT: &str = include_str!("../scripts/gomoku_ai.rss");
const SIZE: i64 = 15;

#[test]
fn ai_uses_one_snapshot_and_one_result_host_call() {
    assert!(AI_SCRIPT.starts_with("use bevy;\n\nlet mut board: [int] = bevy::Gomoku::board();"));
    assert_eq!(AI_SCRIPT.matches("bevy::Gomoku::board()").count(), 1);
    assert_eq!(AI_SCRIPT.matches("bevy::Gomoku::set_ai_move").count(), 1);
    assert_eq!(
        AI_SCRIPT.matches("bevy::Gomoku::").count(),
        2,
        "AI must use only the board snapshot and final move hosts"
    );
    assert!(!AI_SCRIPT.contains("bevy::Gomoku::cell"));
    assert!(!AI_SCRIPT.contains("bevy::Gomoku::set_cell"));
    assert!(!AI_SCRIPT.contains("bevy::Gomoku::board_size"));
    assert!(!AI_SCRIPT.contains("fn set_cell"));
    assert!(!AI_SCRIPT.contains(".copy()"));
    assert!(AI_SCRIPT.contains("while root < 8"));
    assert!(AI_SCRIPT.contains("while reply < 8"));
}

fn seeded_board(stones: &[(i64, i64, i64)]) -> GomokuBoard {
    let mut board = GomokuBoard::default();
    for &(x, y, stone) in stones {
        board.set_for_test(x, y, stone);
    }
    board
}

fn choose(board: &GomokuBoard, player: i64) -> (i64, i64, u128) {
    let mut world = bevy_ecs::world::World::new();
    reset_gomoku_board(&mut world);
    world
        .resource_mut::<GomokuBoard>()
        .replace_cells(board.cells().to_vec())
        .expect("test board dimensions should match");
    let choice = choose_gomoku_ai_move(&mut world, AI_SCRIPT, player)
        .expect("Gomoku AI script should choose a move");
    (choice.x, choice.y, choice.telemetry.elapsed_micros)
}

fn after_move(board: &GomokuBoard, mv: (i64, i64), player: i64) -> GomokuBoard {
    assert_eq!(board.cell(mv.0, mv.1), 0, "AI move must be legal");
    let mut next = board.clone();
    next.set_for_test(mv.0, mv.1, player);
    next
}

fn has_five(board: &GomokuBoard, x: i64, y: i64, player: i64) -> bool {
    [(1, 0), (0, 1), (1, 1), (1, -1)]
        .into_iter()
        .any(|(dx, dy)| {
            let mut total = 1;
            for sign in [-1, 1] {
                let mut step = 1;
                while board.cell(x + sign * step * dx, y + sign * step * dy) == player {
                    total += 1;
                    step += 1;
                }
            }
            total >= 5
        })
}

fn winning_moves(board: &GomokuBoard, player: i64) -> Vec<(i64, i64)> {
    let mut wins = Vec::new();
    for y in 0..SIZE {
        for x in 0..SIZE {
            if board.cell(x, y) != 0 {
                continue;
            }
            let mut next = board.clone();
            next.set_for_test(x, y, player);
            if has_five(&next, x, y, player) {
                wins.push((x, y));
            }
        }
    }
    wins
}

fn open_three_directions(board: &GomokuBoard, mv: (i64, i64), player: i64) -> usize {
    let next = after_move(board, mv, player);
    [(1, 0), (0, 1), (1, 1), (1, -1)]
        .into_iter()
        .filter(|&(dx, dy)| {
            let mut forward = 0;
            while next.cell(mv.0 + (forward + 1) * dx, mv.1 + (forward + 1) * dy) == player {
                forward += 1;
            }
            let mut backward = 0;
            while next.cell(mv.0 - (backward + 1) * dx, mv.1 - (backward + 1) * dy) == player {
                backward += 1;
            }
            forward + backward + 1 == 3
                && next.cell(mv.0 + (forward + 1) * dx, mv.1 + (forward + 1) * dy) == 0
                && next.cell(mv.0 - (backward + 1) * dx, mv.1 - (backward + 1) * dy) == 0
        })
        .count()
}

fn has_forced_win_next_turn(board: &GomokuBoard, attacker: i64, defender: i64) -> bool {
    for y in 0..SIZE {
        for x in 0..SIZE {
            if board.cell(x, y) != 0 {
                continue;
            }
            let mut reply = board.clone();
            reply.set_for_test(x, y, defender);
            if has_five(&reply, x, y, defender) || winning_moves(&reply, attacker).is_empty() {
                return false;
            }
        }
    }
    true
}

fn tactical_opponent_move(board: &GomokuBoard, player: i64) -> (i64, i64) {
    let opponent = 3 - player;
    if let Some(&mv) = winning_moves(board, player).first() {
        return mv;
    }
    if let Some(&mv) = winning_moves(board, opponent).first() {
        return mv;
    }

    let mut best = (-1, -1);
    let mut best_score = i64::MIN;
    for y in 0..SIZE {
        for x in 0..SIZE {
            if board.cell(x, y) != 0 {
                continue;
            }
            let mut next = board.clone();
            next.set_for_test(x, y, player);
            let wins = winning_moves(&next, player).len() as i64;
            let mut length_score = 0;
            for (dx, dy) in [(1, 0), (0, 1), (1, 1), (1, -1)] {
                let mut length = 1;
                for sign in [-1, 1] {
                    let mut step = 1;
                    while next.cell(x + sign * step * dx, y + sign * step * dy) == player {
                        length += 1;
                        step += 1;
                    }
                }
                length_score += length * length;
            }
            let score = wins * 1_000_000 + length_score * 100 - (x - 7).abs() - (y - 7).abs();
            if score > best_score {
                best_score = score;
                best = (x, y);
            }
        }
    }
    best
}

#[test]
#[ignore = "perf"]
fn perf_ai_takes_its_win_before_answering_an_enemy_win() {
    let board = seeded_board(&[
        (3, 4, 2),
        (4, 4, 2),
        (5, 4, 2),
        (6, 4, 2),
        (8, 10, 1),
        (9, 10, 1),
        (10, 10, 1),
        (11, 10, 1),
    ]);

    let (x, y, _) = choose(&board, 2);
    assert!(has_five(&after_move(&board, (x, y), 2), x, y, 2));
}

#[test]
#[ignore = "perf"]
fn perf_ai_handles_open_and_broken_fours_for_both_sides() {
    let cases = [
        (vec![(5, 7, 2), (6, 7, 2), (7, 7, 2), (8, 7, 2)], 2),
        (vec![(5, 7, 1), (6, 7, 1), (7, 7, 1), (8, 7, 1)], 1),
        (vec![(5, 7, 2), (6, 7, 2), (8, 7, 2), (9, 7, 2)], 2),
        (vec![(5, 7, 1), (6, 7, 1), (8, 7, 1), (9, 7, 1)], 1),
    ];

    for (stones, owner) in cases {
        let board = seeded_board(&stones);
        let (x, y, _) = choose(&board, 2);
        if owner == 2 {
            assert!(has_five(&after_move(&board, (x, y), 2), x, y, 2));
        } else {
            assert!(
                winning_moves(&board, 1).contains(&(x, y)),
                "AI did not occupy an immediate enemy winning point ({x}, {y})"
            );
        }
    }
}

#[test]
#[ignore = "perf"]
fn perf_ai_creates_a_double_threat_from_an_open_three() {
    let board = seeded_board(&[
        (6, 7, 2),
        (7, 7, 2),
        (8, 7, 2),
        (2, 2, 1),
        (3, 2, 1),
        (2, 3, 1),
    ]);

    let (x, y, _) = choose(&board, 2);
    assert!(
        has_forced_win_next_turn(&after_move(&board, (x, y), 2), 2, 1),
        "AI move ({x}, {y}) missed its forced continuation"
    );
}

#[test]
#[ignore = "perf"]
fn perf_ai_blocks_an_enemy_cross_fork_despite_distant_attack_material() {
    let board = seeded_board(&[
        (6, 7, 1),
        (8, 7, 1),
        (7, 6, 1),
        (7, 8, 1),
        (2, 2, 2),
        (3, 2, 2),
        (5, 3, 2),
        (6, 3, 2),
    ]);

    assert_eq!(open_three_directions(&board, (7, 7), 1), 2);
    let (x, y, _) = choose(&board, 2);
    assert_eq!((x, y), (7, 7), "AI must occupy the unique fork point");
}

#[test]
#[ignore = "perf"]
fn perf_ai_rejects_a_one_ply_blunder_that_allows_an_enemy_fork() {
    let board = seeded_board(&[
        (5, 7, 1),
        (7, 7, 1),
        (6, 6, 1),
        (6, 8, 1),
        (3, 3, 2),
        (4, 3, 2),
        (9, 10, 2),
    ]);

    assert_eq!(open_three_directions(&board, (6, 7), 1), 2);
    let (x, y, _) = choose(&board, 2);
    assert_eq!(
        (x, y),
        (6, 7),
        "AI move ({x}, {y}) leaves a double-three fork"
    );
}

#[test]
#[ignore = "perf"]
fn perf_ai_blocks_an_enemy_open_three_before_chasing_shapes() {
    let board = seeded_board(&[
        (7, 7, 1),
        (7, 6, 2),
        (6, 7, 1),
        (8, 7, 2),
        (6, 5, 1),
        (6, 6, 2),
        (8, 6, 1),
        (5, 6, 2),
        (4, 6, 1),
        (5, 7, 2),
        (7, 5, 1),
        (9, 5, 2),
        (5, 5, 1),
    ]);

    let (x, y, _) = choose(&board, 2);
    assert!(
        [(4, 5), (8, 5)].contains(&(x, y)),
        "AI move ({x}, {y}) ignored an enemy open-three extension"
    );
}

#[test]
#[ignore = "perf"]
fn perf_ai_survives_a_deterministic_tactical_match() {
    let mut board = GomokuBoard::default();
    let mut ai_moves = 0;
    let mut total_micros = 0;

    for _ in 0..7 {
        let human = tactical_opponent_move(&board, 1);
        board.set_for_test(human.0, human.1, 1);
        assert!(!has_five(&board, human.0, human.1, 1));

        let (x, y, micros) = choose(&board, 2);
        assert_eq!(board.cell(x, y), 0);
        board.set_for_test(x, y, 2);
        ai_moves += 1;
        total_micros += micros;
        if has_five(&board, x, y, 2) {
            break;
        }
    }

    eprintln!(
        "Gomoku tactical match: ai_moves={ai_moves} total={}us average={}us",
        total_micros,
        total_micros / ai_moves
    );
    assert!(ai_moves >= 4);
}

#[test]
#[cfg(not(debug_assertions))]
#[ignore = "perf"]
fn perf_tactical_suite_has_practical_release_latency() {
    let cases = [
        seeded_board(&[(7, 7, 1)]),
        seeded_board(&[(7, 7, 1), (7, 8, 2), (8, 7, 1), (6, 8, 2)]),
        seeded_board(&[(5, 5, 1), (6, 6, 2), (7, 7, 1), (8, 8, 2), (8, 7, 1)]),
        seeded_board(&[(6, 7, 1), (8, 7, 1), (7, 6, 1), (7, 8, 1)]),
    ];
    let mut samples = Vec::new();

    for board in &cases {
        let (x, y, micros) = choose(board, 2);
        assert_eq!(board.cell(x, y), 0);
        samples.push(micros);
    }

    samples.sort_unstable();
    let median = samples[samples.len() / 2];
    let maximum = *samples.last().expect("latency samples should not be empty");
    eprintln!("Gomoku AI release latency: median={median}us max={maximum}us samples={samples:?}");
    assert!(
        maximum < 3_500_000,
        "single move exceeded the interactive latency budget"
    );
}

#[test]
#[ignore = "perf"]
fn perf_full_board_reports_no_available_move_instead_of_invalid_coordinates() {
    let mut board = GomokuBoard::default();
    for y in 0..SIZE {
        for x in 0..SIZE {
            board.set_for_test(x, y, if (x + y) % 2 == 0 { 1 } else { 2 });
        }
    }

    let mut world = bevy_ecs::world::World::new();
    reset_gomoku_board(&mut world);
    world
        .resource_mut::<GomokuBoard>()
        .replace_cells(board.cells().to_vec())
        .expect("test board dimensions should match");
    let error = choose_gomoku_ai_move(&mut world, AI_SCRIPT, 2)
        .expect_err("a full board must not publish an invalid move");
    assert!(
        error.contains("did not select a move"),
        "unexpected error: {error}"
    );
}
