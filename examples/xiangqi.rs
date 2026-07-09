use bevy::{
    prelude::*,
    window::{Window, WindowResolution},
};
use bevy_egui::{
    EguiContexts, EguiGlobalSettings, EguiMultipassSchedule, EguiPlugin, EguiPrimaryContextPass,
    PrimaryEguiContext, egui,
};
use rustscript_bevy_gameplay::{
    XiangqiAiMove, XiangqiBoard, XiangqiMoveSummary, apply_xiangqi_move_script,
    choose_xiangqi_ai_move, reset_xiangqi_board,
};

const MOVE_SCRIPT: &str = include_str!("../scripts/xiangqi_move.rss");
const AI_SCRIPT: &str = include_str!("../scripts/xiangqi_ai.rss");
const RED: i64 = 1;
const BLACK: i64 = -1;

#[derive(Resource, Clone)]
struct XiangqiUiState {
    message: String,
    selected: Option<(i64, i64)>,
    winner: i64,
    last_ai_move: Option<XiangqiAiMove>,
    jit_enabled: bool,
    jit_trace_count: usize,
    last_ai_move_micros: Option<u128>,
    fonts_ready: bool,
}

impl Default for XiangqiUiState {
    fn default() -> Self {
        Self {
            message: "Red to move".to_string(),
            selected: None,
            winner: 0,
            last_ai_move: None,
            jit_enabled: true,
            jit_trace_count: 0,
            last_ai_move_micros: None,
            fonts_ready: false,
        }
    }
}

fn main() {
    if std::env::args().any(|arg| arg == "--script-smoke") {
        run_script_smoke();
        return;
    }

    App::new()
        .insert_resource(ClearColor(Color::srgb(0.07, 0.075, 0.08)))
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "RustScript Xiangqi".to_string(),
                resolution: WindowResolution::new(860, 980),
                resizable: true,
                ..default()
            }),
            ..default()
        }))
        .add_plugins(EguiPlugin::default())
        .add_systems(Startup, setup)
        .add_systems(EguiPrimaryContextPass, xiangqi_ui)
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
    reset_xiangqi_board(world);
    world.insert_resource(XiangqiUiState::default());
}

fn run_script_smoke() {
    let mut world = World::new();
    reset_xiangqi_board(&mut world);
    let human_moves = [(4, 6, 4, 5), (1, 9, 2, 7), (0, 9, 0, 8), (4, 5, 4, 4)];
    let mut turns = 0;
    let mut winner = 0;
    let mut jit_enabled = false;
    let mut jit_traces = 0;
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
        jit_enabled = ai_move.telemetry.jit_enabled;
        jit_traces = ai_move.telemetry.jit_trace_count;
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
    println!(
        "xiangqi_turns={turns}, pieces={pieces}, winner={winner}, jit_enabled={jit_enabled}, jit_traces={jit_traces}, ai_move_us={ai_move_us}"
    );
}

fn xiangqi_ui(world: &mut World) {
    let board = world.resource::<XiangqiBoard>().clone();
    let state = world.resource::<XiangqiUiState>().clone();
    let mut clicked = None;
    let mut restart = false;
    let mut installed_fonts = false;

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
            ui.vertical_centered(|ui| {
                ui.add_space(10.0);
                ui.heading(egui::RichText::new("RustScript Xiangqi").size(32.0));
                ui.add_space(4.0);
                ui.label(status_text(&state));
                ui.add_space(3.0);
                ui.label(telemetry_text(&state));
                ui.add_space(8.0);
                if ui.button("Restart").clicked() {
                    restart = true;
                }
                ui.add_space(10.0);
            });

            ui.horizontal_centered(|ui| {
                let available_w = ui.available_width() - 12.0;
                let available_h = ui.available_height() - 12.0;
                let board_w = available_w.min(available_h * 8.0 / 9.0).max(360.0);
                let board_h = board_w * 9.0 / 8.0;
                clicked = draw_board(ui, &board, &state, egui::vec2(board_w, board_h));
            });
        });

    drop(contexts);
    system_state.apply(world);

    if installed_fonts {
        world.resource_mut::<XiangqiUiState>().fonts_ready = true;
    }

    if restart {
        reset_xiangqi_board(world);
        world.insert_resource(XiangqiUiState::default());
        return;
    }

    if let Some((x, y)) = clicked {
        handle_click(world, x, y);
    }
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

fn handle_click(world: &mut World, x: i64, y: i64) {
    let state = world.resource::<XiangqiUiState>().clone();
    if state.winner != 0 {
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
    let human = match apply_xiangqi_move_script(world, MOVE_SCRIPT, from_x, from_y, to_x, to_y, RED)
    {
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
    publish_move_state(world, human, "AI thinking");
    if human.winner != 0 {
        return;
    }

    let ai_move = match choose_xiangqi_ai_move(world, AI_SCRIPT, BLACK) {
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
        MOVE_SCRIPT,
        ai_move.from_x,
        ai_move.from_y,
        ai_move.to_x,
        ai_move.to_y,
        BLACK,
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
    world.resource_mut::<XiangqiUiState>().last_ai_move = Some(ai_move);
    publish_move_state(world, ai, "Red to move");
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
    let font_paths = [
        "C:\\Windows\\Fonts\\msyh.ttc",
        "C:\\Windows\\Fonts\\msyh.ttf",
        "C:\\Windows\\Fonts\\simsun.ttc",
        "C:\\Windows\\Fonts\\simhei.ttf",
    ];
    let Some(bytes) = font_paths.iter().find_map(|path| std::fs::read(path).ok()) else {
        return false;
    };

    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "xiangqi_cjk".to_string(),
        std::sync::Arc::new(egui::FontData::from_owned(bytes)),
    );
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
