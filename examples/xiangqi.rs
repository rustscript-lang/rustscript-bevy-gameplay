use std::{
    sync::{Arc, Mutex, mpsc},
    thread,
    time::{Duration, Instant},
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
    XIANGQI_BOARD_HEIGHT, XIANGQI_BOARD_WIDTH, XiangqiAiMove, XiangqiBoard, XiangqiMoveSummary,
    apply_xiangqi_move_script, choose_xiangqi_ai_move, choose_xiangqi_ai_move_with_bias,
    debug_xiangqi_ai_script_with_bias, debug_xiangqi_move_script, reset_xiangqi_board,
};
use script_editor::{DebugSession, EditorAction, LiveScriptEditor, ScriptTab};
use vm::{DebugCommandBridge, Debugger};

#[path = "common/board_save.rs"]
mod board_save;
#[path = "common/script_editor.rs"]
mod script_editor;

const MOVE_SCRIPT: &str = include_str!("../scripts/xiangqi_move.rss");
const AI_SCRIPT: &str = include_str!("../scripts/xiangqi_ai.rss");
const RED: i64 = 1;
const BLACK: i64 = -1;
const MOVE_TAB: usize = 0;
const AI_TAB: usize = 1;
const AI_TAKEOVER_MOVE_DELAY: Duration = Duration::from_secs(1);
const SCRIPT_TITLES: &[&str] = &["move.rss", "ai.rss"];
const XIANGQI_MOVE_PREFIX: &str = "let from_x: int = 4;\nlet from_y: int = 6;\nlet to_x: int = 4;\nlet to_y: int = 5;\nlet player: int = 1;\n";
const XIANGQI_AI_PREFIX: &str = "let ai_player: int = -1;\nlet ai_bias: int = 0;\n";
const XIANGQI_HOST_APIS: &[&str] = &[
    "bevy::Xiangqi::board",
    "bevy::Xiangqi::cell",
    "bevy::Xiangqi::set_cell",
    "bevy::Xiangqi::set_result",
    "bevy::Xiangqi::set_ai_move",
];

#[derive(Resource, Clone)]
struct XiangqiUiState {
    message: String,
    selected: Option<(i64, i64)>,
    current_player: i64,
    ai_takeover: f32,
    ai_bias: i64,
    last_ai_takeover_move_at: Option<Instant>,
    winner: i64,
    last_ai_move: Option<XiangqiAiMove>,
    jit_enabled: bool,
    jit_trace_count: usize,
    last_ai_move_micros: Option<u128>,
    fonts_ready: bool,
    board_io_status: String,
}

impl Default for XiangqiUiState {
    fn default() -> Self {
        Self {
            message: "Red to move".to_string(),
            selected: None,
            current_player: RED,
            ai_takeover: 0.0,
            ai_bias: 0,
            last_ai_takeover_move_at: None,
            winner: 0,
            last_ai_move: None,
            jit_enabled: true,
            jit_trace_count: 0,
            last_ai_move_micros: None,
            fonts_ready: false,
            board_io_status: String::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct XiangqiHistorySnapshot {
    cells: Vec<i64>,
    current_player: i64,
    winner: i64,
}

#[derive(Resource, Debug, Clone, Default, PartialEq, Eq)]
struct XiangqiHistory {
    undo: Vec<XiangqiHistorySnapshot>,
    redo: Vec<XiangqiHistorySnapshot>,
}

#[derive(Resource)]
struct XiangqiScripts {
    editor: LiveScriptEditor,
    debug_session: Option<DebugSession>,
    pending_ai_debug: Option<PendingXiangqiAiDebug>,
}

struct PendingXiangqiAiDebug {
    player: i64,
    receiver: Arc<Mutex<mpsc::Receiver<Result<XiangqiAiMove, String>>>>,
}

impl Default for XiangqiScripts {
    fn default() -> Self {
        let mut editor = LiveScriptEditor::new(vec![
            ScriptTab::new(
                "move.rss",
                MOVE_SCRIPT,
                XIANGQI_MOVE_PREFIX,
                XIANGQI_HOST_APIS,
            ),
            ScriptTab::new("ai.rss", AI_SCRIPT, XIANGQI_AI_PREFIX, XIANGQI_HOST_APIS),
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
        .insert_resource(ClearColor(Color::srgb(0.07, 0.075, 0.08)))
        .add_plugins(
            DefaultPlugins
                .set(bevy::log::LogPlugin {
                    filter: runtime_log_filter().to_string(),
                    level: bevy::log::Level::WARN,
                    ..default()
                })
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "RustScript Xiangqi".to_string(),
                        resolution: WindowResolution::new(window_width, window_height),
                        resizable: true,
                        ..default()
                    }),
                    ..default()
                }),
        )
        .add_plugins(EguiPlugin::default())
        .add_systems(Startup, setup)
        .add_systems(EguiPrimaryContextPass, xiangqi_ui)
        .run();
}

fn runtime_log_filter() -> &'static str {
    "warn,cranelift_codegen=off,cranelift_jit=off,cranelift_module=off,cranelift_native=off,pd_vm=off,vm=off"
}

fn initial_window_resolution() -> (u32, u32) {
    (1320, 980)
}

fn centered_board_leading_space(available_width: f32, board_width: f32) -> f32 {
    ((available_width - board_width) * 0.5).max(0.0)
}

fn ai_assist_switch(ui: &mut egui::Ui, enabled: &mut bool) -> egui::Response {
    let desired = egui::vec2(42.0, 22.0);
    let (rect, mut response) = ui.allocate_exact_size(desired, egui::Sense::click());
    if response.clicked() {
        *enabled = !*enabled;
        response.mark_changed();
    }
    let t = ui.ctx().animate_bool(response.id, *enabled);
    let bg = if *enabled {
        egui::Color32::from_rgb(54, 164, 92)
    } else {
        egui::Color32::from_rgb(72, 78, 82)
    };
    ui.painter().rect_filled(rect, 11.0, bg);
    let knob_x = egui::lerp((rect.left() + 11.0)..=(rect.right() - 11.0), t);
    ui.painter().circle_filled(
        egui::pos2(knob_x, rect.center().y),
        8.5,
        egui::Color32::from_rgb(238, 241, 242),
    );
    response
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
    reset_xiangqi_board(world);
    world.insert_resource(XiangqiUiState::default());
    world.insert_resource(XiangqiHistory::default());
    world.insert_resource(XiangqiScripts::default());
}

fn run_script_smoke() {
    let mut world = World::new();
    reset_xiangqi_board(&mut world);
    let human_moves = [(4, 6, 4, 5), (1, 9, 2, 7), (0, 9, 0, 8), (4, 5, 4, 4)];
    let mut turns = 0;
    let mut winner = 0;
    let mut ai_move_us = 0;

    for &(from_x, from_y, to_x, to_y) in &human_moves {
        let human =
            apply_xiangqi_move_script(&mut world, MOVE_SCRIPT, from_x, from_y, to_x, to_y, RED)
                .expect("human move script should run");
        if !human.legal {
            break;
        }
        turns += 1;
        winner = human.winner;
        if winner != 0 {
            break;
        }

        let ai_move =
            choose_xiangqi_ai_move(&mut world, AI_SCRIPT, BLACK).expect("AI script should run");
        ai_move_us = ai_move.telemetry.elapsed_micros;
        let ai = apply_xiangqi_move_script(
            &mut world,
            MOVE_SCRIPT,
            ai_move.from_x,
            ai_move.from_y,
            ai_move.to_x,
            ai_move.to_y,
            BLACK,
        )
        .expect("AI move script should run");
        if !ai.legal {
            break;
        }
        turns += 1;
        winner = ai.winner;
        if winner != 0 {
            break;
        }
    }

    let pieces = world
        .resource::<XiangqiBoard>()
        .cells()
        .iter()
        .filter(|&&piece| piece != 0)
        .count();
    println!("xiangqi_turns={turns}, pieces={pieces}, winner={winner}, ai_move_us={ai_move_us}");
}

fn xiangqi_ui(world: &mut World) {
    let board = world.resource::<XiangqiBoard>().clone();
    let history = world.resource::<XiangqiHistory>().clone();
    let mut state = world.resource::<XiangqiUiState>().clone();
    let mut scripts = world
        .remove_resource::<XiangqiScripts>()
        .unwrap_or_default();
    scripts.editor.update_auto_apply(std::time::Instant::now());
    if let Some(session) = scripts.debug_session.as_mut() {
        session.poll(&mut scripts.editor);
    }
    poll_xiangqi_ai_debug_result(world, &mut scripts, &mut state);
    let mut clicked = None;
    let mut restart = false;
    let mut undo = false;
    let mut redo = false;
    let mut pending_import = None;
    let mut installed_fonts = false;
    let mut editor_actions = Vec::new();

    let mut system_state = bevy::ecs::system::SystemState::<EguiContexts>::new(world);
    let mut contexts = system_state.get_mut(world);
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    if !state.fonts_ready {
        installed_fonts = install_cjk_font(ctx);
    }

    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(egui::Color32::from_rgb(18, 20, 22)))
        .show(ctx, |ui| {
            let row_height = ui.available_height();
            ui.horizontal(|ui| {
                let editor_width = 640.0;
                let gap = 14.0;
                let board_area_width = (ui.available_width() - editor_width - gap).max(400.0);
                ui.allocate_ui_with_layout(
                    egui::vec2(board_area_width, row_height),
                    egui::Layout::top_down(egui::Align::Center),
                    |ui| {
                        ui.set_min_height(row_height);
                        ui.add_space(10.0);
                        ui.heading(egui::RichText::new("RustScript Xiangqi").size(32.0));
                        ui.add_space(4.0);
                        ui.label(status_text(&state));
                        ui.add_space(3.0);
                        ui.label(telemetry_text(&state));
                        ui.add_space(10.0);

                        let available_width = ui.available_width();
                        let available_w = available_width - 12.0;
                        let available_h = (ui.max_rect().height() - 170.0).max(620.0);
                        let board_w = available_w.min(available_h * 8.0 / 9.0).max(360.0);
                        let board_h = board_w * 9.0 / 8.0;
                        let vertical_space = ((ui.available_height() - board_h) * 0.5).max(0.0);
                        ui.add_space(vertical_space);
                        let leading_space =
                            centered_board_leading_space(available_width, board_w) + gap * 0.5;
                        ui.horizontal(|ui| {
                            ui.add_space(leading_space);
                            clicked = draw_board(ui, &board, &state, egui::vec2(board_w, board_h));
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
                        ui.horizontal(|ui| {
                            if ui.button("Restart").clicked() {
                                restart = true;
                            }
                            if ui
                                .add_enabled(!history.undo.is_empty(), egui::Button::new("Undo"))
                                .clicked()
                            {
                                undo = true;
                            }
                            if ui
                                .add_enabled(!history.redo.is_empty(), egui::Button::new("Redo"))
                                .clicked()
                            {
                                redo = true;
                            }
                            if ui.button("Save").clicked() {
                                let contents =
                                    export_xiangqi_save(&board, &state, &history, &scripts.editor);
                                state.board_io_status = match board_save::save_board_file(
                                    "Save Xiangqi game",
                                    "xiangqi.rssboard",
                                    &contents,
                                ) {
                                    Ok(Some(path)) => {
                                        format!("Saved {}", board_save::display_file_name(&path))
                                    }
                                    Ok(None) => "Save cancelled".to_string(),
                                    Err(err) => format!("Save error: {err}"),
                                };
                            }
                            if ui.button("Load").clicked() {
                                match board_save::load_board_file("Load Xiangqi game") {
                                    Ok(Some((path, contents))) => {
                                        pending_import = Some((path, contents));
                                    }
                                    Ok(None) => {
                                        state.board_io_status = "Load cancelled".to_string();
                                    }
                                    Err(err) => {
                                        state.board_io_status = format!("Load error: {err}");
                                    }
                                }
                            }
                        });
                        if !state.board_io_status.is_empty() {
                            ui.label(
                                egui::RichText::new(&state.board_io_status)
                                    .size(12.0)
                                    .color(egui::Color32::from_rgb(176, 185, 188)),
                            );
                        }
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new("AI Assist")
                                    .size(12.0)
                                    .color(egui::Color32::from_rgb(176, 185, 188)),
                            );
                            let mut assist_enabled = xiangqi_ai_takeover_enabled(&state);
                            let changed = ai_assist_switch(ui, &mut assist_enabled).changed();
                            if changed {
                                state.ai_takeover = if assist_enabled { 1.0 } else { 0.0 };
                                state.last_ai_takeover_move_at =
                                    xiangqi_ai_takeover_enabled(&state).then(Instant::now);
                                state.selected = None;
                                if xiangqi_ai_takeover_enabled(&state) && state.winner == 0 {
                                    state.message = format!(
                                        "{} AI to move",
                                        xiangqi_player_label(state.current_player)
                                    );
                                } else if state.winner == 0 {
                                    state.message = if state.current_player == RED {
                                        "Red to move".to_string()
                                    } else {
                                        "AI thinking".to_string()
                                    };
                                }
                            }
                            ui.label(
                                egui::RichText::new(if xiangqi_ai_takeover_enabled(&state) {
                                    "On"
                                } else {
                                    "Off"
                                })
                                .size(12.0)
                                .color(egui::Color32::from_rgb(176, 185, 188)),
                            );
                        });
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new("AI Bias")
                                    .size(12.0)
                                    .color(egui::Color32::from_rgb(176, 185, 188)),
                            );
                            ui.add(
                                egui::Slider::new(&mut state.ai_bias, -100..=100)
                                    .show_value(true)
                                    .clamping(egui::SliderClamping::Always),
                            );
                        });
                        ui.separator();
                        editor_actions = scripts.editor.ui(ui);
                    },
                );
            });
        });

    drop(contexts);
    system_state.apply(world);

    if installed_fonts {
        state.fonts_ready = true;
    }
    handle_xiangqi_editor_actions(world, &mut scripts, editor_actions);

    if let Some((path, text)) = pending_import {
        clicked = None;
        match import_xiangqi_save(world, &mut scripts, &text) {
            Ok(snapshot) => {
                apply_xiangqi_snapshot_to_state(&mut state, &snapshot);
                state.last_ai_takeover_move_at = None;
                state.last_ai_move = None;
                state.board_io_status = format!("Loaded {}", board_save::display_file_name(&path));
            }
            Err(err) => {
                state.board_io_status = format!("Load error: {err}");
            }
        }
    }

    if restart {
        reset_xiangqi_board(world);
        world.insert_resource(XiangqiUiState::default());
        world.insert_resource(XiangqiHistory::default());
        world.insert_resource(scripts);
        return;
    }

    world.insert_resource(state);
    world.insert_resource(scripts);
    if undo {
        undo_xiangqi_turn(world);
        return;
    }
    if redo {
        redo_xiangqi_turn(world);
        return;
    }
    if let Some((x, y)) = clicked {
        handle_click(world, x, y);
    }
    maybe_run_xiangqi_ai_turn(world);
}

fn draw_board(
    ui: &mut egui::Ui,
    board: &XiangqiBoard,
    state: &XiangqiUiState,
    size: egui::Vec2,
) -> Option<(i64, i64)> {
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    let painter = ui.painter_at(rect);
    let board_color = egui::Color32::from_rgb(204, 156, 84);
    let line_color = egui::Color32::from_rgb(82, 50, 22);
    let shadow = egui::Color32::from_rgba_unmultiplied(0, 0, 0, 58);
    painter.rect_filled(rect, 8.0, board_color);

    let grid = rect.shrink2(egui::vec2(44.0, 42.0));
    let step_x = grid.width() / 8.0;
    let step_y = grid.height() / 9.0;

    for x in 0..9 {
        let px = grid.left() + x as f32 * step_x;
        painter.line_segment(
            [egui::pos2(px, grid.top()), egui::pos2(px, grid.bottom())],
            egui::Stroke::new(1.25, line_color),
        );
    }
    for y in 0..10 {
        let py = grid.top() + y as f32 * step_y;
        painter.line_segment(
            [egui::pos2(grid.left(), py), egui::pos2(grid.right(), py)],
            egui::Stroke::new(1.25, line_color),
        );
    }

    painter.rect_filled(
        egui::Rect::from_min_max(
            egui::pos2(grid.left() + 1.0, grid.top() + step_y * 4.0 + 1.0),
            egui::pos2(grid.right() - 1.0, grid.top() + step_y * 5.0 - 1.0),
        ),
        0.0,
        board_color,
    );
    painter.text(
        egui::pos2(grid.center().x, grid.top() + step_y * 4.5),
        egui::Align2::CENTER_CENTER,
        "RIVER",
        egui::FontId::proportional(20.0),
        egui::Color32::from_rgba_unmultiplied(82, 50, 22, 125),
    );

    draw_palace(&painter, grid, step_x, step_y, 0, line_color);
    draw_palace(&painter, grid, step_x, step_y, 7, line_color);

    if let Some((x, y)) = state.selected {
        painter.circle_stroke(
            board_point(grid, step_x, step_y, x, y),
            step_x.min(step_y) * 0.42,
            egui::Stroke::new(3.0, egui::Color32::from_rgb(60, 105, 190)),
        );
    }
    if let Some(last) = state.last_ai_move {
        painter.circle_stroke(
            board_point(grid, step_x, step_y, last.to_x, last.to_y),
            step_x.min(step_y) * 0.47,
            egui::Stroke::new(2.0, egui::Color32::from_rgb(36, 118, 88)),
        );
    }

    for y in 0..10 {
        for x in 0..9 {
            let piece = board.cell(x, y);
            if piece != 0 {
                draw_piece(&painter, grid, step_x, step_y, x, y, piece, shadow);
            }
        }
    }

    if response.clicked() && state.winner == 0 {
        response
            .interact_pointer_pos()
            .and_then(|position| pointer_to_cell(grid, step_x, step_y, position))
    } else {
        None
    }
}

fn draw_palace(
    painter: &egui::Painter,
    grid: egui::Rect,
    step_x: f32,
    step_y: f32,
    top_y: i64,
    color: egui::Color32,
) {
    let left = board_point(grid, step_x, step_y, 3, top_y);
    let right = board_point(grid, step_x, step_y, 5, top_y);
    let bottom_left = board_point(grid, step_x, step_y, 3, top_y + 2);
    let bottom_right = board_point(grid, step_x, step_y, 5, top_y + 2);
    painter.line_segment([left, bottom_right], egui::Stroke::new(1.2, color));
    painter.line_segment([right, bottom_left], egui::Stroke::new(1.2, color));
}

fn draw_piece(
    painter: &egui::Painter,
    grid: egui::Rect,
    step_x: f32,
    step_y: f32,
    x: i64,
    y: i64,
    piece: i64,
    shadow: egui::Color32,
) {
    let center = board_point(grid, step_x, step_y, x, y);
    let radius = step_x.min(step_y) * 0.36;
    let red_piece = piece > 0;
    let fill = if red_piece {
        egui::Color32::from_rgb(246, 222, 178)
    } else {
        egui::Color32::from_rgb(34, 37, 39)
    };
    let stroke = if red_piece {
        egui::Color32::from_rgb(154, 38, 30)
    } else {
        egui::Color32::from_rgb(230, 219, 196)
    };
    let text_color = stroke;
    painter.circle_filled(center + egui::vec2(2.0, 3.0), radius, shadow);
    painter.circle_filled(center, radius, fill);
    painter.circle_stroke(center, radius, egui::Stroke::new(2.0, stroke));
    painter.text(
        center,
        egui::Align2::CENTER_CENTER,
        piece_label(piece),
        egui::FontId::proportional(radius * 0.9),
        text_color,
    );
}

fn piece_label(piece: i64) -> &'static str {
    match piece {
        1 => "帥",
        2 => "仕",
        3 => "相",
        4 => "馬",
        5 => "俥",
        6 => "炮",
        7 => "兵",
        -1 => "將",
        -2 => "士",
        -3 => "象",
        -4 => "馬",
        -5 => "車",
        -6 => "砲",
        -7 => "卒",
        _ => "?",
    }
}

fn board_point(rect: egui::Rect, step_x: f32, step_y: f32, x: i64, y: i64) -> egui::Pos2 {
    egui::pos2(
        rect.left() + x as f32 * step_x,
        rect.top() + y as f32 * step_y,
    )
}

fn pointer_to_cell(
    rect: egui::Rect,
    step_x: f32,
    step_y: f32,
    position: egui::Pos2,
) -> Option<(i64, i64)> {
    let x = ((position.x - rect.left()) / step_x).round() as i64;
    let y = ((position.y - rect.top()) / step_y).round() as i64;
    if !(0..9).contains(&x) || !(0..10).contains(&y) {
        return None;
    }
    let center = board_point(rect, step_x, step_y, x, y);
    if center.distance(position) <= step_x.min(step_y) * 0.48 {
        Some((x, y))
    } else {
        None
    }
}

fn export_xiangqi_save(
    board: &XiangqiBoard,
    state: &XiangqiUiState,
    history: &XiangqiHistory,
    editor: &LiveScriptEditor,
) -> String {
    let current = encode_xiangqi_snapshot(&xiangqi_snapshot(board, state));
    let undo_history = history
        .undo
        .iter()
        .map(encode_xiangqi_snapshot)
        .collect::<Vec<_>>();
    let redo_history = history
        .redo
        .iter()
        .map(encode_xiangqi_snapshot)
        .collect::<Vec<_>>();
    board_save::encode_board_save_with_history(
        "xiangqi",
        board.cells(),
        &[
            ("move.rss", editor.active_source(MOVE_TAB)),
            ("ai.rss", editor.active_source(AI_TAB)),
        ],
        Some(&current),
        &undo_history,
        &redo_history,
    )
}

fn import_xiangqi_save(
    world: &mut World,
    scripts: &mut XiangqiScripts,
    text: &str,
) -> Result<XiangqiHistorySnapshot, String> {
    let package = board_save::decode_board_save(
        text,
        "xiangqi",
        (XIANGQI_BOARD_WIDTH * XIANGQI_BOARD_HEIGHT) as usize,
        SCRIPT_TITLES,
    )?;
    let current = if let Some(state) = package.state.as_deref() {
        decode_xiangqi_snapshot(state)?
    } else {
        XiangqiHistorySnapshot {
            cells: package.cells.clone(),
            current_player: RED,
            winner: 0,
        }
    };
    let mut board = XiangqiBoard::default();
    board.replace_cells(current.cells.clone())?;
    for script in package.scripts {
        match script.title.as_str() {
            "move.rss" => scripts.editor.set_source(MOVE_TAB, script.source)?,
            "ai.rss" => scripts.editor.set_source(AI_TAB, script.source)?,
            _ => {}
        }
    }
    let undo = package
        .undo_history
        .iter()
        .map(|entry| decode_xiangqi_snapshot(entry))
        .collect::<Result<Vec<_>, _>>()?;
    let redo = package
        .redo_history
        .iter()
        .map(|entry| decode_xiangqi_snapshot(entry))
        .collect::<Result<Vec<_>, _>>()?;
    world.insert_resource(board);
    world.insert_resource(XiangqiHistory { undo, redo });
    Ok(current)
}

fn xiangqi_snapshot(board: &XiangqiBoard, state: &XiangqiUiState) -> XiangqiHistorySnapshot {
    XiangqiHistorySnapshot {
        cells: board.cells().to_vec(),
        current_player: state.current_player,
        winner: state.winner,
    }
}

fn current_xiangqi_snapshot(world: &World) -> XiangqiHistorySnapshot {
    xiangqi_snapshot(
        world.resource::<XiangqiBoard>(),
        world.resource::<XiangqiUiState>(),
    )
}

fn encode_xiangqi_snapshot(snapshot: &XiangqiHistorySnapshot) -> String {
    format!(
        "current={};winner={};cells={}",
        snapshot.current_player,
        snapshot.winner,
        snapshot
            .cells
            .iter()
            .map(i64::to_string)
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn decode_xiangqi_snapshot(text: &str) -> Result<XiangqiHistorySnapshot, String> {
    let mut current_player = None;
    let mut winner = None;
    let mut cells = None;
    for part in text.split(';') {
        let Some((key, value)) = part.split_once('=') else {
            return Err(format!("invalid xiangqi history field: {part}"));
        };
        match key {
            "current" => {
                current_player = Some(
                    value
                        .parse::<i64>()
                        .map_err(|_| format!("invalid xiangqi current player: {value}"))?,
                );
            }
            "winner" => {
                winner = Some(
                    value
                        .parse::<i64>()
                        .map_err(|_| format!("invalid xiangqi winner: {value}"))?,
                );
            }
            "cells" => {
                cells = Some(parse_xiangqi_cells(value)?);
            }
            _ => return Err(format!("unknown xiangqi history field: {key}")),
        }
    }
    Ok(XiangqiHistorySnapshot {
        cells: cells.ok_or_else(|| "xiangqi history is missing cells".to_string())?,
        current_player: current_player
            .ok_or_else(|| "xiangqi history is missing current player".to_string())?,
        winner: winner.ok_or_else(|| "xiangqi history is missing winner".to_string())?,
    })
}

fn parse_xiangqi_cells(value: &str) -> Result<Vec<i64>, String> {
    let cells = value
        .split(',')
        .map(|cell| {
            cell.parse::<i64>()
                .map_err(|_| format!("invalid xiangqi cell value: {cell}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let expected = (XIANGQI_BOARD_WIDTH * XIANGQI_BOARD_HEIGHT) as usize;
    if cells.len() != expected {
        return Err(format!(
            "xiangqi history has {} cells; expected {expected}",
            cells.len()
        ));
    }
    Ok(cells)
}

fn apply_xiangqi_snapshot_to_state(state: &mut XiangqiUiState, snapshot: &XiangqiHistorySnapshot) {
    state.selected = None;
    state.current_player = snapshot.current_player;
    state.winner = snapshot.winner;
    state.last_ai_move = None;
    state.message = xiangqi_snapshot_message(state);
}

fn restore_xiangqi_snapshot(
    world: &mut World,
    snapshot: &XiangqiHistorySnapshot,
) -> Result<(), String> {
    let mut board = XiangqiBoard::default();
    board.replace_cells(snapshot.cells.clone())?;
    world.insert_resource(board);
    let mut state = world.resource_mut::<XiangqiUiState>();
    apply_xiangqi_snapshot_to_state(&mut state, snapshot);
    state.last_ai_takeover_move_at = None;
    Ok(())
}

fn xiangqi_snapshot_message(state: &XiangqiUiState) -> String {
    if state.winner == RED {
        "Red wins".to_string()
    } else if state.winner == BLACK {
        "Black wins".to_string()
    } else if xiangqi_ai_takeover_enabled(state) {
        format!("{} AI to move", xiangqi_player_label(state.current_player))
    } else if state.current_player == RED {
        "Red to move".to_string()
    } else {
        "AI thinking".to_string()
    }
}

fn push_xiangqi_undo_snapshot(world: &mut World, snapshot: XiangqiHistorySnapshot) {
    let mut history = world.resource_mut::<XiangqiHistory>();
    history.undo.push(snapshot);
    history.redo.clear();
}

fn undo_xiangqi_turn(world: &mut World) {
    let current = current_xiangqi_snapshot(world);
    let previous = {
        let mut history = world.resource_mut::<XiangqiHistory>();
        let previous = history.undo.pop();
        if previous.is_some() {
            history.redo.push(current);
        }
        previous
    };
    if let Some(previous) = previous {
        if let Err(err) = restore_xiangqi_snapshot(world, &previous) {
            world.resource_mut::<XiangqiUiState>().board_io_status = format!("Undo error: {err}");
        } else {
            world.resource_mut::<XiangqiUiState>().board_io_status = "Undo".to_string();
        }
    }
}

fn redo_xiangqi_turn(world: &mut World) {
    let current = current_xiangqi_snapshot(world);
    let next = {
        let mut history = world.resource_mut::<XiangqiHistory>();
        let next = history.redo.pop();
        if next.is_some() {
            history.undo.push(current);
        }
        next
    };
    if let Some(next) = next {
        if let Err(err) = restore_xiangqi_snapshot(world, &next) {
            world.resource_mut::<XiangqiUiState>().board_io_status = format!("Redo error: {err}");
        } else {
            world.resource_mut::<XiangqiUiState>().board_io_status = "Redo".to_string();
        }
    }
}

fn handle_click(world: &mut World, x: i64, y: i64) {
    let state = world.resource::<XiangqiUiState>().clone();
    if state.winner != 0 || state.current_player != RED || xiangqi_ai_takeover_enabled(&state) {
        return;
    }
    let piece = world.resource::<XiangqiBoard>().cell(x, y);
    let Some((from_x, from_y)) = state.selected else {
        if piece > 0 {
            let mut state = world.resource_mut::<XiangqiUiState>();
            state.selected = Some((x, y));
            state.message = "Choose target".to_string();
        }
        return;
    };

    if piece > 0 {
        let mut state = world.resource_mut::<XiangqiUiState>();
        state.selected = Some((x, y));
        state.message = "Choose target".to_string();
        return;
    }

    play_human_turn(world, from_x, from_y, x, y);
}

fn play_human_turn(world: &mut World, from_x: i64, from_y: i64, to_x: i64, to_y: i64) {
    let move_script = world
        .resource::<XiangqiScripts>()
        .editor
        .active_source(MOVE_TAB)
        .to_string();
    let before_turn = current_xiangqi_snapshot(world);
    let human =
        match apply_xiangqi_move_script(world, &move_script, from_x, from_y, to_x, to_y, RED) {
            Ok(summary) => summary,
            Err(err) => {
                let mut state = world.resource_mut::<XiangqiUiState>();
                state.message = format!("Script error: {err}");
                return;
            }
        };
    if !human.legal {
        let mut state = world.resource_mut::<XiangqiUiState>();
        state.message = "Illegal move".to_string();
        state.selected = None;
        return;
    }
    push_xiangqi_undo_snapshot(world, before_turn);
    publish_move_state(world, human, "AI thinking");
    if human.winner != 0 {
        return;
    }
    world.resource_mut::<XiangqiUiState>().current_player = BLACK;
}

fn maybe_run_xiangqi_ai_turn(world: &mut World) {
    maybe_run_xiangqi_ai_turn_at(world, Instant::now());
}

fn maybe_run_xiangqi_ai_turn_at(world: &mut World, now: Instant) {
    let state = world.resource::<XiangqiUiState>().clone();
    if state.winner != 0 {
        return;
    }
    let player = state.current_player;
    let takeover_enabled = xiangqi_ai_takeover_enabled(&state);
    if player != BLACK && !takeover_enabled {
        return;
    }
    if takeover_enabled
        && state
            .last_ai_takeover_move_at
            .is_some_and(|last| now.duration_since(last) < AI_TAKEOVER_MOVE_DELAY)
    {
        return;
    }
    let scripts = world.resource::<XiangqiScripts>();
    if scripts.debug_session.is_some() || scripts.pending_ai_debug.is_some() {
        return;
    }
    play_xiangqi_ai_turn(world, player);
    if takeover_enabled {
        world
            .resource_mut::<XiangqiUiState>()
            .last_ai_takeover_move_at = Some(now);
    }
}

fn play_xiangqi_ai_turn(world: &mut World, player: i64) {
    let state = world.resource::<XiangqiUiState>().clone();
    let ai_bias = state.ai_bias;
    let takeover_enabled = xiangqi_ai_takeover_enabled(&state);
    let before_turn = takeover_enabled.then(|| current_xiangqi_snapshot(world));
    let (move_script, ai_script) = {
        let scripts = world.resource::<XiangqiScripts>();
        (
            scripts.editor.active_source(MOVE_TAB).to_string(),
            scripts.editor.active_source(AI_TAB).to_string(),
        )
    };
    if let Some(mut scripts) = world.remove_resource::<XiangqiScripts>() {
        let started_debug = start_xiangqi_ai_debug_for_turn(
            world,
            &mut scripts,
            ai_script.clone(),
            player,
            ai_bias,
        );
        world.insert_resource(scripts);
        if started_debug {
            let mut state = world.resource_mut::<XiangqiUiState>();
            state.message = "AI debugger paused".to_string();
            state.selected = None;
            return;
        }
    }

    let ai_move = match choose_xiangqi_ai_move_with_bias(world, &ai_script, player, ai_bias) {
        Ok(ai_move) => ai_move,
        Err(err) => {
            let mut state = world.resource_mut::<XiangqiUiState>();
            state.message = format!("AI script error: {err}");
            return;
        }
    };
    record_ai_telemetry(world, ai_move.telemetry);
    let ai = match apply_xiangqi_move_script(
        world,
        &move_script,
        ai_move.from_x,
        ai_move.from_y,
        ai_move.to_x,
        ai_move.to_y,
        player,
    ) {
        Ok(summary) => summary,
        Err(err) => {
            let mut state = world.resource_mut::<XiangqiUiState>();
            state.message = format!("AI move error: {err}");
            return;
        }
    };
    if !ai.legal {
        let mut state = world.resource_mut::<XiangqiUiState>();
        state.message = "AI selected an illegal move".to_string();
        state.selected = None;
        return;
    }
    if let Some(snapshot) = before_turn {
        push_xiangqi_undo_snapshot(world, snapshot);
    }
    world.resource_mut::<XiangqiUiState>().last_ai_move = Some(ai_move);
    let message = if xiangqi_ai_takeover_enabled(world.resource::<XiangqiUiState>()) {
        format!("{} AI moved", xiangqi_player_label(player))
    } else {
        "Red to move".to_string()
    };
    publish_move_state(world, ai, &message);
    if ai.winner == 0 {
        world.resource_mut::<XiangqiUiState>().current_player = other_xiangqi_player(player);
    }
}

fn handle_xiangqi_editor_actions(
    world: &mut World,
    scripts: &mut XiangqiScripts,
    actions: Vec<EditorAction>,
) {
    for action in actions {
        match action {
            EditorAction::StartDebug(tab) => start_xiangqi_debug_session(world, scripts, tab),
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
            EditorAction::RunDebugCommand(command) => {
                if let Some(session) = scripts.debug_session.as_ref() {
                    session.console_command(&mut scripts.editor, &command);
                }
            }
            EditorAction::EvaluateHover { tab, name } => {
                if let Some(session) = scripts.debug_session.as_ref() {
                    session.evaluate_hover(&mut scripts.editor, tab, &name);
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

fn start_xiangqi_debug_session(world: &mut World, scripts: &mut XiangqiScripts, tab: usize) {
    if tab == AI_TAB {
        scripts.debug_session = None;
        scripts.pending_ai_debug = None;
        scripts.editor.begin_pending_debug_session(tab);
        return;
    }
    let source = scripts.editor.active_source(tab).to_string();
    let source_line_offset = scripts.editor.source_line_offset(tab);
    let board = world.resource::<XiangqiBoard>().clone();
    let ai_bias = world.resource::<XiangqiUiState>().ai_bias;
    let bridge = DebugCommandBridge::new();
    let thread_bridge = bridge.clone();
    let (sender, receiver) = mpsc::channel::<String>();
    thread::spawn(move || {
        let mut debug_world = World::new();
        debug_world.insert_resource(board);
        let mut debugger = Debugger::with_command_bridge(thread_bridge);
        debugger.stop_on_entry();
        let result = if tab == MOVE_TAB {
            debug_xiangqi_move_script(&mut debug_world, &source, 4, 6, 4, 5, RED, &mut debugger)
                .map(|summary| {
                    format!(
                        "debug complete: legal={}, winner={}",
                        summary.legal, summary.winner
                    )
                })
        } else {
            debug_xiangqi_ai_script_with_bias(
                &mut debug_world,
                &source,
                BLACK,
                ai_bias,
                &mut debugger,
            )
            .map(|mv| {
                format!(
                    "debug complete: ai=({}, {}) -> ({}, {})",
                    mv.from_x, mv.from_y, mv.to_x, mv.to_y
                )
            })
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

fn start_xiangqi_ai_debug_for_turn(
    world: &mut World,
    scripts: &mut XiangqiScripts,
    source: String,
    player: i64,
    ai_bias: i64,
) -> bool {
    if !(scripts.editor.debug_pending && scripts.editor.debug_tab == Some(AI_TAB)) {
        return false;
    }
    let source_line_offset = scripts.editor.source_line_offset(AI_TAB);
    let board = world.resource::<XiangqiBoard>().clone();
    let bridge = DebugCommandBridge::new();
    let thread_bridge = bridge.clone();
    let (output_sender, output_receiver) = mpsc::channel::<String>();
    let (result_sender, result_receiver) = mpsc::channel::<Result<XiangqiAiMove, String>>();
    thread::spawn(move || {
        let mut debug_world = World::new();
        debug_world.insert_resource(board);
        let mut debugger = Debugger::with_command_bridge(thread_bridge);
        debugger.stop_on_entry();
        let result = debug_xiangqi_ai_script_with_bias(
            &mut debug_world,
            &source,
            player,
            ai_bias,
            &mut debugger,
        );
        let output = result
            .as_ref()
            .map(|mv| {
                format!(
                    "debug complete: ai=({}, {}) -> ({}, {})",
                    mv.from_x, mv.from_y, mv.to_x, mv.to_y
                )
            })
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
    scripts.pending_ai_debug = Some(PendingXiangqiAiDebug {
        player,
        receiver: Arc::new(Mutex::new(result_receiver)),
    });
    true
}

fn poll_xiangqi_ai_debug_result(
    world: &mut World,
    scripts: &mut XiangqiScripts,
    state: &mut XiangqiUiState,
) {
    let Some(pending) = scripts.pending_ai_debug.as_ref() else {
        return;
    };
    let player = pending.player;
    let result = {
        let Ok(receiver) = pending.receiver.lock() else {
            scripts.pending_ai_debug = None;
            state.message = "AI debug channel error".to_string();
            state.selected = None;
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
            state.selected = None;
            return;
        }
    };
    state.jit_enabled = ai_move.telemetry.jit_enabled;
    state.jit_trace_count = ai_move.telemetry.jit_trace_count;
    state.last_ai_move_micros = Some(ai_move.telemetry.elapsed_micros);

    let move_script = scripts.editor.active_source(MOVE_TAB).to_string();
    let before_turn = xiangqi_ai_takeover_enabled(state)
        .then(|| xiangqi_snapshot(world.resource::<XiangqiBoard>(), state));
    match apply_xiangqi_move_script(
        world,
        &move_script,
        ai_move.from_x,
        ai_move.from_y,
        ai_move.to_x,
        ai_move.to_y,
        player,
    ) {
        Ok(summary) if summary.legal => {
            if let Some(snapshot) = before_turn {
                push_xiangqi_undo_snapshot(world, snapshot);
            }
            state.selected = None;
            state.winner = summary.winner;
            state.last_ai_move = Some(ai_move);
            if summary.winner == 0 {
                state.current_player = other_xiangqi_player(player);
                if xiangqi_ai_takeover_enabled(state) {
                    state.last_ai_takeover_move_at = Some(Instant::now());
                }
            }
            state.message = if summary.winner == RED {
                "Red wins".to_string()
            } else if summary.winner == BLACK {
                "Black wins".to_string()
            } else if xiangqi_ai_takeover_enabled(state) {
                format!("{} AI moved", xiangqi_player_label(player))
            } else {
                "Red to move".to_string()
            };
        }
        Ok(_) => {
            state.selected = None;
            state.message = "AI selected an illegal move".to_string();
        }
        Err(err) => {
            state.selected = None;
            state.message = format!("AI move error: {err}");
        }
    }
}

fn publish_move_state(world: &mut World, summary: XiangqiMoveSummary, message: &str) {
    let mut state = world.resource_mut::<XiangqiUiState>();
    state.selected = None;
    state.winner = summary.winner;
    state.message = if summary.winner == RED {
        "Red wins".to_string()
    } else if summary.winner == BLACK {
        "Black wins".to_string()
    } else {
        message.to_string()
    };
}

fn record_ai_telemetry(
    world: &mut World,
    telemetry: rustscript_bevy_gameplay::XiangqiScriptTelemetry,
) {
    let mut state = world.resource_mut::<XiangqiUiState>();
    state.jit_enabled = telemetry.jit_enabled;
    state.jit_trace_count = telemetry.jit_trace_count;
    state.last_ai_move_micros = Some(telemetry.elapsed_micros);
}

fn other_xiangqi_player(player: i64) -> i64 {
    if player == RED { BLACK } else { RED }
}

fn xiangqi_ai_takeover_enabled(state: &XiangqiUiState) -> bool {
    state.ai_takeover >= 0.5
}

fn xiangqi_player_label(player: i64) -> &'static str {
    if player == RED { "Red" } else { "Black" }
}

fn status_text(state: &XiangqiUiState) -> egui::RichText {
    let text = if state.winner == RED {
        "Red wins"
    } else if state.winner == BLACK {
        "Black wins"
    } else {
        state.message.as_str()
    };
    egui::RichText::new(text)
        .size(18.0)
        .color(egui::Color32::from_rgb(224, 220, 208))
}

fn telemetry_text(state: &XiangqiUiState) -> egui::RichText {
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
    .color(egui::Color32::from_rgb(176, 185, 188))
}

fn install_cjk_font(ctx: &egui::Context) -> bool {
    let Some((bytes, face_index)) = load_system_cjk_font() else {
        return false;
    };

    let mut fonts = egui::FontDefinitions::default();
    let mut font_data = egui::FontData::from_owned(bytes);
    font_data.index = face_index;
    fonts
        .font_data
        .insert("xiangqi_cjk".to_string(), std::sync::Arc::new(font_data));
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .insert(0, "xiangqi_cjk".to_string());
    }
    ctx.set_fonts(fonts);
    true
}

fn load_system_cjk_font() -> Option<(Vec<u8>, u32)> {
    let mut database = fontdb::Database::new();
    database.load_system_fonts();

    for name in cjk_font_family_candidates() {
        let families = [fontdb::Family::Name(name)];
        let query = fontdb::Query {
            families: &families,
            ..fontdb::Query::default()
        };
        if let Some(id) = database.query(&query) {
            return database.with_face_data(id, |data, face_index| (data.to_vec(), face_index));
        }
    }

    for face in database.faces() {
        if face.families.iter().any(|(family, _)| {
            cjk_font_family_candidates()
                .iter()
                .any(|candidate| family.eq_ignore_ascii_case(candidate))
        }) {
            return database
                .with_face_data(face.id, |data, face_index| (data.to_vec(), face_index));
        }
    }

    None
}

fn cjk_font_family_candidates() -> &'static [&'static str] {
    &[
        "Microsoft YaHei UI",
        "Microsoft YaHei",
        "SimSun",
        "SimHei",
        "DengXian",
        "Microsoft JhengHei UI",
        "Microsoft JhengHei",
        "PingFang SC",
        "Heiti SC",
        "Songti SC",
        "STHeiti",
        "Hiragino Sans GB",
        "Noto Sans CJK SC",
        "Noto Sans SC",
        "Noto Serif CJK SC",
        "Source Han Sans SC",
        "Source Han Serif SC",
        "WenQuanYi Micro Hei",
        "WenQuanYi Zen Hei",
        "AR PL UMing CN",
        "Droid Sans Fallback",
    ]
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
    fn runtime_log_filter_disables_jit_backend_logs() {
        let filter = runtime_log_filter();

        assert!(filter.contains("cranelift_codegen=off"));
        assert!(filter.contains("cranelift_jit=off"));
        assert!(filter.contains("pd_vm=off"));
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
    fn cjk_font_candidates_cover_common_desktop_platforms() {
        let candidates = cjk_font_family_candidates();

        assert!(candidates.contains(&"Microsoft YaHei"));
        assert!(candidates.contains(&"PingFang SC"));
        assert!(candidates.contains(&"Noto Sans CJK SC"));
        assert!(candidates.contains(&"WenQuanYi Micro Hei"));
        assert!(candidates.iter().all(|name| !name.contains('\\')));
    }

    #[test]
    fn xiangqi_save_roundtrips_board_and_scripts() {
        let mut world = World::new();
        let mut board = XiangqiBoard::default();
        board.clear_for_test();
        board.set_for_test(4, 9, RED);
        world.insert_resource(board.clone());
        let mut state = XiangqiUiState::default();
        state.current_player = BLACK;
        world.insert_resource(state.clone());
        let history = XiangqiHistory {
            undo: vec![XiangqiHistorySnapshot {
                cells: board.cells().to_vec(),
                current_player: RED,
                winner: 0,
            }],
            redo: vec![],
        };
        world.insert_resource(history.clone());
        let mut scripts = XiangqiScripts::default();
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

        let text = export_xiangqi_save(&board, &state, &history, &scripts.editor);
        let mut loaded_scripts = XiangqiScripts::default();
        let loaded_state = import_xiangqi_save(&mut world, &mut loaded_scripts, &text).unwrap();

        assert_eq!(world.resource::<XiangqiBoard>().cell(4, 9), RED);
        assert_eq!(loaded_state.current_player, BLACK);
        assert_eq!(world.resource::<XiangqiHistory>().undo.len(), 1);
        assert_eq!(loaded_scripts.editor.active_source(MOVE_TAB), move_source);
        assert_eq!(loaded_scripts.editor.active_source(AI_TAB), ai_source);
    }

    #[test]
    fn ai_takeover_blocks_human_selection() {
        let mut world = fast_xiangqi_world();
        world.resource_mut::<XiangqiUiState>().ai_takeover = 1.0;

        handle_click(&mut world, 4, 6);

        assert_eq!(world.resource::<XiangqiUiState>().selected, None);
        assert_eq!(world.resource::<XiangqiBoard>().cell(4, 6), 7);
    }

    #[test]
    fn ai_takeover_advances_both_xiangqi_sides() {
        let mut world = fast_xiangqi_world();
        world.resource_mut::<XiangqiUiState>().ai_takeover = 1.0;
        let now = Instant::now();

        maybe_run_xiangqi_ai_turn_at(&mut world, now);
        assert_eq!(world.resource::<XiangqiBoard>().cell(4, 5), 7);
        assert_eq!(world.resource::<XiangqiUiState>().current_player, BLACK);

        maybe_run_xiangqi_ai_turn_at(&mut world, now + AI_TAKEOVER_MOVE_DELAY);
        assert_eq!(world.resource::<XiangqiBoard>().cell(4, 4), -7);
        assert_eq!(world.resource::<XiangqiUiState>().current_player, RED);
    }

    #[test]
    fn ai_takeover_waits_between_xiangqi_moves() {
        let mut world = fast_xiangqi_world();
        let now = Instant::now();
        {
            let mut state = world.resource_mut::<XiangqiUiState>();
            state.ai_takeover = 1.0;
            state.last_ai_takeover_move_at = Some(now);
        }

        maybe_run_xiangqi_ai_turn_at(&mut world, now + Duration::from_millis(999));
        assert_eq!(world.resource::<XiangqiBoard>().cell(4, 5), 0);

        maybe_run_xiangqi_ai_turn_at(&mut world, now + AI_TAKEOVER_MOVE_DELAY);
        assert_eq!(world.resource::<XiangqiBoard>().cell(4, 5), 7);
        assert!(
            world
                .resource::<XiangqiUiState>()
                .last_ai_move_micros
                .is_some_and(|micros| micros < 1_000_000),
            "AI move telemetry should not include the 1s takeover delay"
        );
    }

    #[test]
    fn xiangqi_undo_and_redo_restore_player_turn_boundaries() {
        let mut world = fast_xiangqi_world();

        play_human_turn(&mut world, 4, 6, 4, 5);
        maybe_run_xiangqi_ai_turn_at(&mut world, Instant::now());

        assert_eq!(world.resource::<XiangqiBoard>().cell(4, 5), 7);
        assert_eq!(world.resource::<XiangqiBoard>().cell(4, 4), -7);
        assert_eq!(world.resource::<XiangqiUiState>().current_player, RED);

        undo_xiangqi_turn(&mut world);
        assert_eq!(world.resource::<XiangqiBoard>().cell(4, 6), 7);
        assert_eq!(world.resource::<XiangqiBoard>().cell(4, 3), -7);
        assert_eq!(world.resource::<XiangqiUiState>().current_player, RED);

        redo_xiangqi_turn(&mut world);
        assert_eq!(world.resource::<XiangqiBoard>().cell(4, 5), 7);
        assert_eq!(world.resource::<XiangqiBoard>().cell(4, 4), -7);
        assert_eq!(world.resource::<XiangqiUiState>().current_player, RED);
    }

    fn fast_xiangqi_world() -> World {
        let mut world = World::new();
        world.insert_resource(XiangqiBoard::default());
        world.insert_resource(XiangqiUiState::default());
        world.insert_resource(XiangqiHistory::default());
        let mut scripts = XiangqiScripts::default();
        scripts
            .editor
            .set_source(
                AI_TAB,
                "use bevy;\nlet from_y: int = if ai_player == 1 => { 6 } else => { 3 };\nlet to_y: int = if ai_player == 1 => { 5 } else => { 4 };\nbevy::Xiangqi::set_ai_move(4, from_y, 4, to_y);\n0;",
            )
            .unwrap();
        world.insert_resource(scripts);
        world
    }
}
