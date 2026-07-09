use bevy::{
    prelude::*,
    window::{Window, WindowResolution},
};
use bevy_egui::{
    EguiContexts, EguiGlobalSettings, EguiMultipassSchedule, EguiPlugin, EguiPrimaryContextPass,
    PrimaryEguiContext, egui,
};
use rustscript_bevy_gameplay::{
    GOMOKU_BOARD_SIZE, GomokuAiMove, GomokuBoard, GomokuMoveSummary, apply_gomoku_move_script,
    choose_gomoku_ai_move, reset_gomoku_board,
};

const MOVE_SCRIPT: &str = include_str!("../scripts/gomoku_move.rss");
const AI_SCRIPT: &str = include_str!("../scripts/gomoku_ai.rss");
const HUMAN: i64 = 1;
const COMPUTER: i64 = 2;

#[derive(Resource, Clone)]
struct GomokuUiState {
    message: String,
    winner: i64,
    draw: bool,
    last_ai_move: Option<GomokuAiMove>,
}

impl Default for GomokuUiState {
    fn default() -> Self {
        Self {
            message: "Your turn".to_string(),
            winner: 0,
            draw: false,
            last_ai_move: None,
        }
    }
}

fn main() {
    if std::env::args().any(|arg| arg == "--script-smoke") {
        run_script_smoke();
        return;
    }

    App::new()
        .insert_resource(ClearColor(Color::srgb(0.075, 0.085, 0.095)))
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "RustScript Gomoku".to_string(),
                resolution: WindowResolution::new(920, 960),
                resizable: true,
                ..default()
            }),
            ..default()
        }))
        .add_plugins(EguiPlugin::default())
        .add_systems(Startup, setup)
        .add_systems(EguiPrimaryContextPass, gomoku_ui)
        .run();
}

fn setup(world: &mut World) {
    world
        .resource_mut::<EguiGlobalSettings>()
        .auto_create_primary_context = false;
    world.spawn((
        PrimaryEguiContext,
        EguiMultipassSchedule::new(EguiPrimaryContextPass),
        Camera2d,
    ));
    reset_gomoku_board(world);
    world.insert_resource(GomokuUiState::default());
}

fn run_script_smoke() {
    let mut world = World::new();
    reset_gomoku_board(&mut world);
    let human_moves = [(7, 7), (6, 6), (8, 6), (6, 8), (8, 8), (5, 7)];
    let mut turns = 0;
    let mut winner = 0;
    let mut draw = false;

    for &(x, y) in &human_moves {
        let human = apply_gomoku_move_script(&mut world, MOVE_SCRIPT, x, y, HUMAN)
            .expect("human move script should run");
        if !human.legal {
            break;
        }
        turns += 1;
        winner = human.winner;
        draw = human.draw;
        if winner != 0 || draw {
            break;
        }

        let ai_move =
            choose_gomoku_ai_move(&mut world, AI_SCRIPT, COMPUTER).expect("AI script should run");
        let ai = apply_gomoku_move_script(&mut world, MOVE_SCRIPT, ai_move.x, ai_move.y, COMPUTER)
            .expect("AI move script should run");
        if !ai.legal {
            break;
        }
        turns += 1;
        winner = ai.winner;
        draw = ai.draw;
        if winner != 0 || draw {
            break;
        }
    }

    let stones = world
        .resource::<GomokuBoard>()
        .cells()
        .iter()
        .filter(|&&stone| stone != 0)
        .count();
    println!("gomoku_turns={turns}, stones={stones}, winner={winner}, draw={draw}");
}

fn gomoku_ui(world: &mut World) {
    let board = world.resource::<GomokuBoard>().clone();
    let state = world.resource::<GomokuUiState>().clone();
    let mut clicked_move = None;
    let mut restart = false;

    let mut system_state = bevy::ecs::system::SystemState::<EguiContexts>::new(world);
    let mut contexts = system_state.get_mut(world);
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };

    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(egui::Color32::from_rgb(18, 21, 24)))
        .show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(16.0);
                ui.heading(egui::RichText::new("RustScript Gomoku").size(32.0));
                ui.add_space(4.0);
                ui.label(status_text(&state));
                ui.add_space(12.0);
                if ui.button("Restart").clicked() {
                    restart = true;
                }
                ui.add_space(14.0);
            });

            ui.horizontal_centered(|ui| {
                let board_side = ui.available_width().min(ui.available_height()).min(760.0);
                clicked_move = draw_board(ui, &board, &state, board_side);
            });
        });

    drop(contexts);
    system_state.apply(world);

    if restart {
        reset_gomoku_board(world);
        world.insert_resource(GomokuUiState::default());
        return;
    }

    if let Some((x, y)) = clicked_move {
        play_human_turn(world, x, y);
    }
}

fn draw_board(
    ui: &mut egui::Ui,
    board: &GomokuBoard,
    state: &GomokuUiState,
    board_side: f32,
) -> Option<(i64, i64)> {
    let (rect, response) =
        ui.allocate_exact_size(egui::Vec2::splat(board_side), egui::Sense::click());
    let painter = ui.painter_at(rect);
    let wood = egui::Color32::from_rgb(202, 163, 98);
    let line = egui::Color32::from_rgb(79, 54, 28);
    let shadow = egui::Color32::from_rgba_unmultiplied(0, 0, 0, 64);
    painter.rect_filled(rect, 8.0, wood);

    let grid = rect.shrink(38.0);
    let step = grid.width() / (GOMOKU_BOARD_SIZE as f32 - 1.0);
    for index in 0..GOMOKU_BOARD_SIZE {
        let offset = index as f32 * step;
        let x = grid.left() + offset;
        let y = grid.top() + offset;
        painter.line_segment(
            [egui::pos2(x, grid.top()), egui::pos2(x, grid.bottom())],
            egui::Stroke::new(1.2, line),
        );
        painter.line_segment(
            [egui::pos2(grid.left(), y), egui::pos2(grid.right(), y)],
            egui::Stroke::new(1.2, line),
        );
    }

    for &(x, y) in &[(3, 3), (7, 7), (11, 11), (3, 11), (11, 3)] {
        let center = board_point(grid, step, x, y);
        painter.circle_filled(center, step * 0.11, line);
    }

    if let Some(last) = state.last_ai_move {
        let center = board_point(grid, step, last.x, last.y);
        painter.circle_stroke(
            center,
            step * 0.48,
            egui::Stroke::new(2.0, egui::Color32::from_rgb(94, 132, 196)),
        );
    }

    for y in 0..GOMOKU_BOARD_SIZE {
        for x in 0..GOMOKU_BOARD_SIZE {
            match board.cell(x, y) {
                HUMAN => {
                    let center = board_point(grid, step, x, y);
                    painter.circle_filled(center + egui::vec2(2.0, 3.0), step * 0.38, shadow);
                    painter.circle_filled(center, step * 0.38, egui::Color32::from_rgb(24, 27, 30));
                }
                COMPUTER => {
                    let center = board_point(grid, step, x, y);
                    painter.circle_filled(center + egui::vec2(2.0, 3.0), step * 0.38, shadow);
                    painter.circle_filled(
                        center,
                        step * 0.38,
                        egui::Color32::from_rgb(238, 233, 218),
                    );
                    painter.circle_stroke(
                        center,
                        step * 0.38,
                        egui::Stroke::new(1.0, egui::Color32::from_rgb(168, 160, 144)),
                    );
                }
                _ => {}
            }
        }
    }

    if response.clicked() && state.winner == 0 && !state.draw {
        response
            .interact_pointer_pos()
            .and_then(|position| pointer_to_cell(grid, step, position))
    } else {
        None
    }
}

fn board_point(rect: egui::Rect, step: f32, x: i64, y: i64) -> egui::Pos2 {
    egui::pos2(rect.left() + x as f32 * step, rect.top() + y as f32 * step)
}

fn pointer_to_cell(rect: egui::Rect, step: f32, position: egui::Pos2) -> Option<(i64, i64)> {
    let x = ((position.x - rect.left()) / step).round() as i64;
    let y = ((position.y - rect.top()) / step).round() as i64;
    if !(0..GOMOKU_BOARD_SIZE).contains(&x) || !(0..GOMOKU_BOARD_SIZE).contains(&y) {
        return None;
    }
    let center = board_point(rect, step, x, y);
    if center.distance(position) <= step * 0.42 {
        Some((x, y))
    } else {
        None
    }
}

fn play_human_turn(world: &mut World, x: i64, y: i64) {
    let state = world.resource::<GomokuUiState>().clone();
    if state.winner != 0 || state.draw {
        return;
    }

    let human = match apply_gomoku_move_script(world, MOVE_SCRIPT, x, y, HUMAN) {
        Ok(summary) => summary,
        Err(err) => {
            world.resource_mut::<GomokuUiState>().message = format!("Script error: {err}");
            return;
        }
    };
    if !human.legal {
        world.resource_mut::<GomokuUiState>().message = "Point unavailable".to_string();
        return;
    }
    publish_move_state(world, human, "Your move", None);
    if human.winner != 0 || human.draw {
        return;
    }

    let ai_move = match choose_gomoku_ai_move(world, AI_SCRIPT, COMPUTER) {
        Ok(ai_move) => ai_move,
        Err(err) => {
            world.resource_mut::<GomokuUiState>().message = format!("AI script error: {err}");
            return;
        }
    };
    let ai = match apply_gomoku_move_script(world, MOVE_SCRIPT, ai_move.x, ai_move.y, COMPUTER) {
        Ok(summary) => summary,
        Err(err) => {
            world.resource_mut::<GomokuUiState>().message = format!("AI move error: {err}");
            return;
        }
    };
    if !ai.legal {
        world.resource_mut::<GomokuUiState>().message =
            "AI selected an unavailable point".to_string();
        return;
    }
    publish_move_state(world, ai, "AI moved", Some(ai_move));
}

fn publish_move_state(
    world: &mut World,
    summary: GomokuMoveSummary,
    message: &str,
    ai_move: Option<GomokuAiMove>,
) {
    let mut state = world.resource_mut::<GomokuUiState>();
    state.winner = summary.winner;
    state.draw = summary.draw;
    state.last_ai_move = ai_move.or(state.last_ai_move);
    state.message = if summary.winner == HUMAN {
        "Black wins".to_string()
    } else if summary.winner == COMPUTER {
        "White wins".to_string()
    } else if summary.draw {
        "Draw".to_string()
    } else {
        message.to_string()
    };
}

fn status_text(state: &GomokuUiState) -> egui::RichText {
    let text = if state.winner == 0 && !state.draw {
        state.message.as_str()
    } else if state.winner == HUMAN {
        "Black wins"
    } else if state.winner == COMPUTER {
        "White wins"
    } else {
        "Draw"
    };
    egui::RichText::new(text)
        .size(18.0)
        .color(egui::Color32::from_rgb(221, 224, 218))
}
