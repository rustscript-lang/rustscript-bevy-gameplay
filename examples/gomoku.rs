use std::{
    sync::{Arc, Mutex, mpsc},
    thread,
};

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
    choose_gomoku_ai_move, debug_gomoku_ai_script, debug_gomoku_move_script, reset_gomoku_board,
};
use script_editor::{DebugSession, EditorAction, LiveScriptEditor, ScriptTab};
use vm::{DebugCommandBridge, Debugger};

#[path = "common/board_save.rs"]
mod board_save;
#[path = "common/script_editor.rs"]
mod script_editor;

const MOVE_SCRIPT: &str = include_str!("../scripts/gomoku_move.rss");
const AI_SCRIPT: &str = include_str!("../scripts/gomoku_ai.rss");
const HUMAN: i64 = 1;
const COMPUTER: i64 = 2;
const MOVE_TAB: usize = 0;
const AI_TAB: usize = 1;
const SCRIPT_TITLES: &[&str] = &["move.rss", "ai.rss"];
const GOMOKU_MOVE_PREFIX: &str =
    "let move_x: int = 0;\nlet move_y: int = 0;\nlet player: int = 1;\n";
const GOMOKU_AI_PREFIX: &str = "let ai_player: int = 2;\n";
const GOMOKU_HOST_APIS: &[&str] = &[
    "bevy::Gomoku::cell",
    "bevy::Gomoku::set_cell",
    "bevy::Gomoku::set_result",
    "bevy::Gomoku::set_ai_move",
];

#[derive(Resource, Clone)]
struct GomokuUiState {
    message: String,
    winner: i64,
    draw: bool,
    last_ai_move: Option<GomokuAiMove>,
    jit_enabled: bool,
    jit_trace_count: usize,
    last_ai_move_micros: Option<u128>,
    board_io_text: String,
    board_io_status: String,
}

impl Default for GomokuUiState {
    fn default() -> Self {
        Self {
            message: "Your turn".to_string(),
            winner: 0,
            draw: false,
            last_ai_move: None,
            jit_enabled: true,
            jit_trace_count: 0,
            last_ai_move_micros: None,
            board_io_text: String::new(),
            board_io_status: String::new(),
        }
    }
}

#[derive(Resource)]
struct GomokuScripts {
    editor: LiveScriptEditor,
    debug_session: Option<DebugSession>,
    pending_ai_debug: Option<Arc<Mutex<mpsc::Receiver<Result<GomokuAiMove, String>>>>>,
}

impl Default for GomokuScripts {
    fn default() -> Self {
        let mut editor = LiveScriptEditor::new(vec![
            ScriptTab::new(
                "move.rss",
                MOVE_SCRIPT,
                GOMOKU_MOVE_PREFIX,
                GOMOKU_HOST_APIS,
            ),
            ScriptTab::new("ai.rss", AI_SCRIPT, GOMOKU_AI_PREFIX, GOMOKU_HOST_APIS),
        ]);
        editor.lint_all();
        Self {
            editor,
            debug_session: None,
            pending_ai_debug: None,
        }
    }
}

fn main() {
    if std::env::args().any(|arg| arg == "--script-smoke") {
        run_script_smoke();
        return;
    }

    let (window_width, window_height) = initial_window_resolution();
    App::new()
        .insert_resource(ClearColor(Color::srgb(0.075, 0.085, 0.095)))
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "RustScript Gomoku".to_string(),
                resolution: WindowResolution::new(window_width, window_height),
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

fn initial_window_resolution() -> (u32, u32) {
    (1320, 930)
}

fn centered_board_leading_space(available_width: f32, board_width: f32) -> f32 {
    ((available_width - board_width) * 0.5).max(0.0)
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
    world.insert_resource(GomokuScripts::default());
}

fn run_script_smoke() {
    let mut world = World::new();
    reset_gomoku_board(&mut world);
    let human_moves = [(7, 7), (6, 6), (8, 6), (6, 8), (8, 8), (5, 7)];
    let mut turns = 0;
    let mut winner = 0;
    let mut draw = false;
    let mut jit_enabled = false;
    let mut jit_traces = 0;
    let mut ai_move_us = 0;

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
        jit_enabled = ai_move.telemetry.jit_enabled;
        jit_traces = ai_move.telemetry.jit_trace_count;
        ai_move_us = ai_move.telemetry.elapsed_micros;
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
    println!(
        "gomoku_turns={turns}, stones={stones}, winner={winner}, draw={draw}, jit_enabled={jit_enabled}, jit_traces={jit_traces}, ai_move_us={ai_move_us}"
    );
}

fn gomoku_ui(world: &mut World) {
    let board = world.resource::<GomokuBoard>().clone();
    let mut state = world.resource::<GomokuUiState>().clone();
    let mut scripts = world.remove_resource::<GomokuScripts>().unwrap_or_default();
    scripts.editor.update_auto_apply(std::time::Instant::now());
    if let Some(session) = scripts.debug_session.as_mut() {
        session.poll(&mut scripts.editor);
    }
    poll_gomoku_ai_debug_result(world, &mut scripts, &mut state);
    let mut clicked_move = None;
    let mut restart = false;
    let mut pending_import = None;
    let mut editor_actions = Vec::new();

    let mut system_state = bevy::ecs::system::SystemState::<EguiContexts>::new(world);
    let mut contexts = system_state.get_mut(world);
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };

    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(egui::Color32::from_rgb(18, 21, 24)))
        .show(ctx, |ui| {
            let row_height = ui.available_height();
            ui.horizontal(|ui| {
                let editor_width = 640.0;
                let gap = 14.0;
                let board_area_width = (ui.available_width() - editor_width - gap).max(360.0);
                ui.allocate_ui_with_layout(
                    egui::vec2(board_area_width, row_height),
                    egui::Layout::top_down(egui::Align::Center),
                    |ui| {
                        ui.set_min_height(row_height);
                        ui.add_space(10.0);
                        ui.heading(egui::RichText::new("RustScript Gomoku").size(32.0));
                        ui.add_space(4.0);
                        ui.label(status_text(&state));
                        ui.add_space(3.0);
                        ui.label(telemetry_text(&state));
                        ui.add_space(12.0);
                        if ui.button("Restart").clicked() {
                            restart = true;
                        }
                        egui::CollapsingHeader::new("Save / Load")
                            .id_salt("gomoku_save_load")
                            .default_open(false)
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    if ui.button("Export").clicked() {
                                        state.board_io_text =
                                            export_gomoku_save(&board, &scripts.editor);
                                        state.board_io_status =
                                            "Exported board and scripts".to_string();
                                    }
                                    if ui.button("Import").clicked() {
                                        pending_import = Some(state.board_io_text.clone());
                                    }
                                });
                                ui.add(
                                    egui::TextEdit::multiline(&mut state.board_io_text)
                                        .font(egui::TextStyle::Monospace)
                                        .desired_width(ui.available_width().min(620.0))
                                        .desired_rows(4),
                                );
                                if !state.board_io_status.is_empty() {
                                    ui.label(
                                        egui::RichText::new(&state.board_io_status)
                                            .size(12.0)
                                            .color(egui::Color32::from_rgb(174, 184, 188)),
                                    );
                                }
                            });
                        ui.add_space(10.0);

                        let available_width = ui.available_width();
                        let panel_height = ui.max_rect().height();
                        let board_side =
                            (available_width.min(panel_height - 170.0) - 12.0).max(560.0);
                        let vertical_space = ((ui.available_height() - board_side) * 0.5).max(0.0);
                        ui.add_space(vertical_space);
                        let leading_space =
                            centered_board_leading_space(available_width, board_side);
                        ui.horizontal(|ui| {
                            ui.add_space(leading_space);
                            clicked_move = draw_board(ui, &board, &state, board_side);
                        });
                    },
                );
                ui.add_space(gap);
                ui.separator();
                ui.add_space(gap);
                ui.allocate_ui_with_layout(
                    egui::vec2(editor_width, row_height),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_min_height(row_height);
                        editor_actions = scripts.editor.ui(ui);
                    },
                );
            });
        });

    drop(contexts);
    system_state.apply(world);
    handle_gomoku_editor_actions(world, &mut scripts, editor_actions);

    if let Some(text) = pending_import {
        clicked_move = None;
        match import_gomoku_save(world, &mut scripts, &text) {
            Ok(()) => {
                state.message = "Imported board and scripts".to_string();
                state.winner = 0;
                state.draw = false;
                state.last_ai_move = None;
                state.board_io_status = "Imported board and scripts".to_string();
            }
            Err(err) => {
                state.board_io_status = format!("Import error: {err}");
            }
        }
    }

    if restart {
        reset_gomoku_board(world);
        world.insert_resource(GomokuUiState::default());
        world.insert_resource(scripts);
        return;
    }

    world.insert_resource(state);
    world.insert_resource(scripts);
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

fn export_gomoku_save(board: &GomokuBoard, editor: &LiveScriptEditor) -> String {
    board_save::encode_board_save(
        "gomoku",
        board.cells(),
        &[
            ("move.rss", editor.active_source(MOVE_TAB)),
            ("ai.rss", editor.active_source(AI_TAB)),
        ],
    )
}

fn import_gomoku_save(
    world: &mut World,
    scripts: &mut GomokuScripts,
    text: &str,
) -> Result<(), String> {
    let package = board_save::decode_board_save(
        text,
        "gomoku",
        (GOMOKU_BOARD_SIZE * GOMOKU_BOARD_SIZE) as usize,
        SCRIPT_TITLES,
    )?;
    let mut board = GomokuBoard::default();
    board.replace_cells(package.cells)?;
    for script in package.scripts {
        match script.title.as_str() {
            "move.rss" => scripts.editor.set_source(MOVE_TAB, script.source)?,
            "ai.rss" => scripts.editor.set_source(AI_TAB, script.source)?,
            _ => {}
        }
    }
    world.insert_resource(board);
    Ok(())
}

fn play_human_turn(world: &mut World, x: i64, y: i64) {
    let state = world.resource::<GomokuUiState>().clone();
    if state.winner != 0 || state.draw {
        return;
    }
    let (move_script, ai_script) = {
        let scripts = world.resource::<GomokuScripts>();
        (
            scripts.editor.active_source(MOVE_TAB).to_string(),
            scripts.editor.active_source(AI_TAB).to_string(),
        )
    };

    let human = match apply_gomoku_move_script(world, &move_script, x, y, HUMAN) {
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

    if let Some(mut scripts) = world.remove_resource::<GomokuScripts>() {
        let started_debug = start_gomoku_ai_debug_for_turn(world, &mut scripts, ai_script.clone());
        world.insert_resource(scripts);
        if started_debug {
            world.resource_mut::<GomokuUiState>().message = "AI debugger paused".to_string();
            return;
        }
    }

    let ai_move = match choose_gomoku_ai_move(world, &ai_script, COMPUTER) {
        Ok(ai_move) => ai_move,
        Err(err) => {
            world.resource_mut::<GomokuUiState>().message = format!("AI script error: {err}");
            return;
        }
    };
    record_ai_telemetry(world, ai_move.telemetry);
    let ai = match apply_gomoku_move_script(world, &move_script, ai_move.x, ai_move.y, COMPUTER) {
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

fn handle_gomoku_editor_actions(
    world: &mut World,
    scripts: &mut GomokuScripts,
    actions: Vec<EditorAction>,
) {
    for action in actions {
        match action {
            EditorAction::StartDebug(tab) => start_gomoku_debug_session(world, scripts, tab),
            EditorAction::StopDebug => {
                scripts.debug_session = None;
                scripts.pending_ai_debug = None;
                scripts.editor.clear_debug_state();
            }
            EditorAction::StepDebug => {
                if let Some(session) = scripts.debug_session.as_ref() {
                    session.command(&mut scripts.editor, "step");
                }
            }
            EditorAction::NextDebug => {
                if let Some(session) = scripts.debug_session.as_ref() {
                    session.command(&mut scripts.editor, "next");
                }
            }
            EditorAction::ContinueDebug => {
                if let Some(session) = scripts.debug_session.as_ref() {
                    session.command(&mut scripts.editor, "continue");
                }
            }
            EditorAction::RefreshLocals => {
                if let Some(session) = scripts.debug_session.as_ref() {
                    session.command(&mut scripts.editor, "locals");
                }
            }
            EditorAction::ToggleBreakpoint { tab, line, enabled } => {
                if scripts.editor.debug_tab == Some(tab)
                    && let Some(session) = scripts.debug_session.as_ref()
                {
                    session.set_breakpoint(&mut scripts.editor, line, enabled);
                }
            }
        }
    }
}

fn start_gomoku_debug_session(world: &mut World, scripts: &mut GomokuScripts, tab: usize) {
    if tab == AI_TAB {
        scripts.debug_session = None;
        scripts.pending_ai_debug = None;
        scripts.editor.begin_pending_debug_session(tab);
        return;
    }
    let source = scripts.editor.active_source(tab).to_string();
    let source_line_offset = scripts.editor.source_line_offset(tab);
    let board = world.resource::<GomokuBoard>().clone();
    let (debug_x, debug_y) = first_open_gomoku_point(&board);
    let bridge = DebugCommandBridge::new();
    let thread_bridge = bridge.clone();
    let (sender, receiver) = mpsc::channel::<String>();
    thread::spawn(move || {
        let mut debug_world = World::new();
        debug_world.insert_resource(board);
        let mut debugger = Debugger::with_command_bridge(thread_bridge);
        debugger.stop_on_entry();
        let result = if tab == MOVE_TAB {
            debug_gomoku_move_script(
                &mut debug_world,
                &source,
                debug_x,
                debug_y,
                HUMAN,
                &mut debugger,
            )
            .map(|summary| {
                format!(
                    "debug complete: legal={}, winner={}, draw={}",
                    summary.legal, summary.winner, summary.draw
                )
            })
        } else {
            debug_gomoku_ai_script(&mut debug_world, &source, COMPUTER, &mut debugger)
                .map(|mv| format!("debug complete: ai=({}, {})", mv.x, mv.y))
        };
        let _ = sender.send(result.unwrap_or_else(|err| format!("debug error: {err}")));
    });
    scripts.editor.begin_debug_session(tab);
    scripts.debug_session = Some(DebugSession::new(
        bridge,
        receiver,
        tab,
        source_line_offset,
        scripts.editor.user_breakpoints(tab),
    ));
}

fn start_gomoku_ai_debug_for_turn(
    world: &mut World,
    scripts: &mut GomokuScripts,
    source: String,
) -> bool {
    if !(scripts.editor.debug_pending && scripts.editor.debug_tab == Some(AI_TAB)) {
        return false;
    }
    let source_line_offset = scripts.editor.source_line_offset(AI_TAB);
    let board = world.resource::<GomokuBoard>().clone();
    let bridge = DebugCommandBridge::new();
    let thread_bridge = bridge.clone();
    let (output_sender, output_receiver) = mpsc::channel::<String>();
    let (result_sender, result_receiver) = mpsc::channel::<Result<GomokuAiMove, String>>();
    thread::spawn(move || {
        let mut debug_world = World::new();
        debug_world.insert_resource(board);
        let mut debugger = Debugger::with_command_bridge(thread_bridge);
        debugger.stop_on_entry();
        let result = debug_gomoku_ai_script(&mut debug_world, &source, COMPUTER, &mut debugger);
        let output = result
            .as_ref()
            .map(|mv| format!("debug complete: ai=({}, {})", mv.x, mv.y))
            .unwrap_or_else(|err| format!("debug error: {err}"));
        let _ = output_sender.send(output);
        let _ = result_sender.send(result);
    });
    scripts.editor.begin_debug_session(AI_TAB);
    scripts.debug_session = Some(DebugSession::new(
        bridge,
        output_receiver,
        AI_TAB,
        source_line_offset,
        scripts.editor.user_breakpoints(AI_TAB),
    ));
    scripts.pending_ai_debug = Some(Arc::new(Mutex::new(result_receiver)));
    true
}

fn poll_gomoku_ai_debug_result(
    world: &mut World,
    scripts: &mut GomokuScripts,
    state: &mut GomokuUiState,
) {
    let Some(receiver) = scripts.pending_ai_debug.as_ref() else {
        return;
    };
    let result = {
        let Ok(receiver) = receiver.lock() else {
            scripts.pending_ai_debug = None;
            state.message = "AI debug channel error".to_string();
            return;
        };
        let Ok(result) = receiver.try_recv() else {
            return;
        };
        result
    };
    scripts.pending_ai_debug = None;
    scripts.debug_session = None;
    scripts.editor.debug_attached = false;
    scripts.editor.debug_starting = false;
    scripts.editor.debug_pending = false;
    scripts.editor.debug_line = None;

    let ai_move = match result {
        Ok(ai_move) => ai_move,
        Err(err) => {
            state.message = format!("AI debug error: {err}");
            return;
        }
    };
    state.jit_enabled = ai_move.telemetry.jit_enabled;
    state.jit_trace_count = ai_move.telemetry.jit_trace_count;
    state.last_ai_move_micros = Some(ai_move.telemetry.elapsed_micros);

    let move_script = scripts.editor.active_source(MOVE_TAB).to_string();
    match apply_gomoku_move_script(world, &move_script, ai_move.x, ai_move.y, COMPUTER) {
        Ok(summary) if summary.legal => {
            state.winner = summary.winner;
            state.draw = summary.draw;
            state.last_ai_move = Some(ai_move);
            state.message = if summary.winner == HUMAN {
                "Black wins".to_string()
            } else if summary.winner == COMPUTER {
                "White wins".to_string()
            } else if summary.draw {
                "Draw".to_string()
            } else {
                "AI moved".to_string()
            };
        }
        Ok(_) => {
            state.message = "AI selected an unavailable point".to_string();
        }
        Err(err) => {
            state.message = format!("AI move error: {err}");
        }
    }
}

fn first_open_gomoku_point(board: &GomokuBoard) -> (i64, i64) {
    if board.cell(7, 7) == 0 {
        return (7, 7);
    }
    for y in 0..GOMOKU_BOARD_SIZE {
        for x in 0..GOMOKU_BOARD_SIZE {
            if board.cell(x, y) == 0 {
                return (x, y);
            }
        }
    }
    (7, 7)
}

fn record_ai_telemetry(
    world: &mut World,
    telemetry: rustscript_bevy_gameplay::GomokuScriptTelemetry,
) {
    let mut state = world.resource_mut::<GomokuUiState>();
    state.jit_enabled = telemetry.jit_enabled;
    state.jit_trace_count = telemetry.jit_trace_count;
    state.last_ai_move_micros = Some(telemetry.elapsed_micros);
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

fn telemetry_text(state: &GomokuUiState) -> egui::RichText {
    let jit = if state.jit_enabled { "on" } else { "off" };
    let ai_ms = state
        .last_ai_move_micros
        .map(|micros| format!("{:.2} ms", micros as f64 / 1000.0))
        .unwrap_or_else(|| "--".to_string());
    egui::RichText::new(format!(
        "JIT: {jit}   traces: {}   AI move: {ai_ms}",
        state.jit_trace_count
    ))
    .size(14.0)
    .color(egui::Color32::from_rgb(174, 184, 188))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_window_width_keeps_board_tight() {
        let (width, _height) = initial_window_resolution();

        assert_eq!(width, 1320);
    }

    #[test]
    fn centered_board_padding_balances_side_gaps() {
        let available_width = 700.0;
        let board_width = 640.0;
        let left = centered_board_leading_space(available_width, board_width);
        let right = available_width - board_width - left;

        assert!((left - right).abs() < 0.01);
    }

    #[test]
    fn gomoku_save_roundtrips_board_and_scripts() {
        let mut world = World::new();
        let mut board = GomokuBoard::default();
        board.set_for_test(7, 7, HUMAN);
        world.insert_resource(board.clone());
        let mut scripts = GomokuScripts::default();
        let move_source = format!("{MOVE_SCRIPT}\nlet save_marker: int = 1;\n");
        let ai_source = format!("{AI_SCRIPT}\nlet save_marker: int = 2;\n");
        scripts
            .editor
            .set_source(MOVE_TAB, move_source.clone())
            .unwrap();
        scripts
            .editor
            .set_source(AI_TAB, ai_source.clone())
            .unwrap();

        let text = export_gomoku_save(&board, &scripts.editor);
        let mut loaded_scripts = GomokuScripts::default();
        import_gomoku_save(&mut world, &mut loaded_scripts, &text).unwrap();

        assert_eq!(world.resource::<GomokuBoard>().cell(7, 7), HUMAN);
        assert_eq!(loaded_scripts.editor.active_source(MOVE_TAB), move_source);
        assert_eq!(loaded_scripts.editor.active_source(AI_TAB), ai_source);
    }
}
