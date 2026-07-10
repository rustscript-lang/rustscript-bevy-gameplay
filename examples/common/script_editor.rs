use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex, mpsc::Receiver},
    time::{Duration, Instant},
};

use bevy_egui::egui;
use vm::{DebugCommandBridge, DebugCommandBridgeError, SourceError, SourceMap, compile_source};

const CODE_FONT_SIZE: f32 = 13.0;

#[derive(Debug, Clone)]
pub struct ScriptTab {
    pub title: &'static str,
    pub default_source: &'static str,
    pub buffer: String,
    pub active_source: String,
    pub lint_prefix: &'static str,
    pub host_apis: &'static [&'static str],
    pub diagnostics: Vec<ScriptDiagnostic>,
    pub status: String,
    pub breakpoints: BTreeSet<u32>,
    edited_at: Option<Instant>,
    applied_at: Option<Instant>,
}

impl ScriptTab {
    pub fn new(
        title: &'static str,
        source: &'static str,
        lint_prefix: &'static str,
        host_apis: &'static [&'static str],
    ) -> Self {
        Self {
            title,
            default_source: source,
            buffer: source.to_string(),
            active_source: source.to_string(),
            lint_prefix,
            host_apis,
            diagnostics: Vec::new(),
            status: "Applied".to_string(),
            breakpoints: BTreeSet::new(),
            edited_at: None,
            applied_at: Some(Instant::now()),
        }
    }

    pub fn active_source(&self) -> &str {
        &self.active_source
    }
}

#[derive(Debug, Clone)]
pub struct LiveScriptEditor {
    pub tabs: Vec<ScriptTab>,
    pub active: usize,
    pub debug_output: String,
    pub debug_line: Option<u32>,
    pub debug_tab: Option<usize>,
    pub debug_attached: bool,
    pub debug_starting: bool,
    pub debug_pending: bool,
    pub console_input: String,
    console_history: Vec<String>,
    console_history_cursor: Option<usize>,
    debug_hover: Option<DebugHoverState>,
    cooldown: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorAction {
    StartDebug(usize),
    StopDebug,
    StepDebug,
    NextDebug,
    ContinueDebug,
    RefreshLocals,
    RunDebugCommand(String),
    EvaluateHover {
        tab: usize,
        name: String,
    },
    ToggleBreakpoint {
        tab: usize,
        line: u32,
        enabled: bool,
    },
}

#[derive(Debug, Clone)]
struct DebugHoverState {
    tab: usize,
    name: String,
    started_at: Instant,
    requested: bool,
    value: Option<String>,
}

pub struct DebugSession {
    pub bridge: DebugCommandBridge,
    receiver: Arc<Mutex<Receiver<String>>>,
    tab: usize,
    source_line_offset: u32,
    seek_user_source: bool,
    temporary_line_breakpoint: Option<u32>,
    pending_breakpoints: Vec<u32>,
}

impl DebugSession {
    pub fn new(
        bridge: DebugCommandBridge,
        receiver: Receiver<String>,
        tab: usize,
        source_line_offset: u32,
        user_breakpoints: Vec<u32>,
    ) -> Self {
        Self {
            bridge,
            receiver: Arc::new(Mutex::new(receiver)),
            tab,
            source_line_offset,
            seek_user_source: source_line_offset > 0,
            temporary_line_breakpoint: None,
            pending_breakpoints: user_breakpoints
                .into_iter()
                .map(|line| source_line_offset.saturating_add(line))
                .collect(),
        }
    }

    pub fn poll(&mut self, editor: &mut LiveScriptEditor) {
        let status = self.bridge.status();
        let visible_line = visible_debug_line(status.current_line, self.source_line_offset);
        if status.attached {
            let pending_breakpoints = std::mem::take(&mut self.pending_breakpoints);
            for full_line in pending_breakpoints {
                if let Err(err) = self.bridge.execute(
                    format!("break line {full_line}"),
                    Duration::from_millis(120),
                ) {
                    append_debug_output(editor, &format!("{err}\n"));
                }
            }
        }
        if self.seek_user_source && status.attached {
            if visible_line.is_some() {
                if let Some(target_line) = self.temporary_line_breakpoint {
                    match self.bridge.execute(
                        format!("clear line {target_line}"),
                        Duration::from_millis(120),
                    ) {
                        Ok(_) => {
                            self.temporary_line_breakpoint = None;
                            self.seek_user_source = false;
                        }
                        Err(err) => append_debug_output(editor, &format!("{err}\n")),
                    }
                } else {
                    self.seek_user_source = false;
                }
            } else if self.temporary_line_breakpoint.is_some() {
                if let Err(err) = self.bridge.execute("continue", Duration::from_millis(120)) {
                    append_debug_output(editor, &format!("{err}\n"));
                }
            } else {
                let target_line = self.source_line_offset.saturating_add(1);
                match self.bridge.execute(
                    format!("break line {target_line}"),
                    Duration::from_millis(120),
                ) {
                    Ok(_) => {
                        self.temporary_line_breakpoint = Some(target_line);
                        if let Err(err) =
                            self.bridge.execute("continue", Duration::from_millis(120))
                        {
                            append_debug_output(editor, &format!("{err}\n"));
                        }
                    }
                    Err(err) => append_debug_output(editor, &format!("{err}\n")),
                }
            }
        }

        let status = self.bridge.status();
        editor.debug_attached = status.attached;
        editor.debug_line = visible_debug_line(status.current_line, self.source_line_offset);
        editor.debug_tab = Some(self.tab);
        if status.attached && editor.debug_line.is_some() {
            editor.debug_starting = false;
            editor.debug_pending = false;
        }
        if let Ok(receiver) = self.receiver.lock() {
            while let Ok(message) = receiver.try_recv() {
                editor.debug_starting = false;
                editor.debug_pending = false;
                append_debug_output(editor, &message);
            }
        }
    }

    pub fn command(&self, editor: &mut LiveScriptEditor, command: &str) {
        match self.bridge.execute(command, Duration::from_millis(120)) {
            Ok(response) => {
                editor.debug_attached = response.attached;
                editor.debug_line =
                    visible_debug_line(response.current_line, self.source_line_offset);
                editor.debug_starting = response.resumed;
                editor.debug_pending = false;
                append_debug_output(editor, &response.output);
            }
            Err(DebugCommandBridgeError::NotAttached) => {
                editor.debug_attached = false;
                editor.debug_starting = true;
                append_debug_output(editor, "debugger is running\n");
            }
            Err(err) => {
                editor.debug_starting = false;
                append_debug_output(editor, &format!("{err}\n"));
            }
        }
    }

    pub fn console_command(&self, editor: &mut LiveScriptEditor, command: &str) {
        append_debug_output(editor, &format!("> {command}\n"));
        self.command(editor, command);
    }

    pub fn evaluate_hover(&self, editor: &mut LiveScriptEditor, tab: usize, name: &str) {
        if self.tab != tab || !is_debug_identifier(name) {
            return;
        }
        let value = match self
            .bridge
            .execute(format!("print {name}"), Duration::from_millis(120))
        {
            Ok(response) => response.output.trim().to_string(),
            Err(err) => err.to_string(),
        };
        if let Some(hover) = editor.debug_hover.as_mut()
            && hover.tab == tab
            && hover.name == name
        {
            hover.value = Some(value);
        }
    }

    pub fn set_breakpoint(&self, editor: &mut LiveScriptEditor, line: u32, enabled: bool) {
        let full_line = self.source_line_offset.saturating_add(line);
        let command = if enabled {
            format!("break line {full_line}")
        } else {
            format!("clear line {full_line}")
        };
        match self.bridge.execute(command, Duration::from_millis(120)) {
            Ok(response) => {
                editor.debug_attached = response.attached;
                editor.debug_line =
                    visible_debug_line(response.current_line, self.source_line_offset);
                editor.debug_starting = response.resumed;
                editor.debug_pending = false;
                append_debug_output(editor, &response.output);
            }
            Err(DebugCommandBridgeError::NotAttached) => {}
            Err(err) => append_debug_output(editor, &format!("{err}\n")),
        }
    }
}

impl Drop for DebugSession {
    fn drop(&mut self) {
        self.bridge.close();
    }
}

fn visible_debug_line(line: Option<u32>, source_line_offset: u32) -> Option<u32> {
    line.and_then(|line| line.checked_sub(source_line_offset))
        .filter(|line| *line > 0)
}

fn append_debug_output(editor: &mut LiveScriptEditor, text: &str) {
    if text.trim().is_empty() {
        return;
    }
    editor.debug_output.push_str(text);
    if !editor.debug_output.ends_with('\n') {
        editor.debug_output.push('\n');
    }
    const MAX_DEBUG_CHARS: usize = 5000;
    if editor.debug_output.len() > MAX_DEBUG_CHARS {
        let keep_from = editor.debug_output.len() - MAX_DEBUG_CHARS;
        editor.debug_output = editor.debug_output[keep_from..].to_string();
    }
}

impl LiveScriptEditor {
    pub fn new(tabs: Vec<ScriptTab>) -> Self {
        Self {
            tabs,
            active: 0,
            debug_output: String::new(),
            debug_line: None,
            debug_tab: None,
            debug_attached: false,
            debug_starting: false,
            debug_pending: false,
            console_input: String::new(),
            console_history: Vec::new(),
            console_history_cursor: None,
            debug_hover: None,
            cooldown: Duration::from_millis(850),
        }
    }

    pub fn begin_debug_session(&mut self, tab: usize) {
        self.debug_output = "starting debugger...\n".to_string();
        self.debug_line = None;
        self.debug_tab = Some(tab);
        self.debug_attached = false;
        self.debug_starting = true;
        self.debug_pending = false;
        self.debug_hover = None;
    }

    pub fn begin_pending_debug_session(&mut self, tab: usize) {
        self.debug_output = "waiting for next AI move...\n".to_string();
        self.debug_line = None;
        self.debug_tab = Some(tab);
        self.debug_attached = false;
        self.debug_starting = false;
        self.debug_pending = true;
        self.debug_hover = None;
    }

    pub fn clear_debug_state(&mut self) {
        self.debug_output.clear();
        self.debug_line = None;
        self.debug_tab = None;
        self.debug_attached = false;
        self.debug_starting = false;
        self.debug_pending = false;
        self.debug_hover = None;
    }

    pub fn reset_active_tab(&mut self, active: usize) -> EditorAction {
        let _ = self.reset_tab_to_default(active);
        self.clear_debug_state();
        EditorAction::StopDebug
    }

    pub fn active_source(&self, index: usize) -> &str {
        self.tabs[index].active_source()
    }

    pub fn lint_all(&mut self) {
        for tab in &mut self.tabs {
            lint_tab(tab);
        }
    }

    pub fn set_source(&mut self, index: usize, source: impl Into<String>) -> Result<(), String> {
        let tab = self
            .tabs
            .get_mut(index)
            .ok_or_else(|| format!("script tab {index} does not exist"))?;
        tab.buffer = source.into();
        lint_tab(tab);
        tab.edited_at = None;
        tab.applied_at = Some(Instant::now());
        if tab.diagnostics.is_empty() {
            tab.active_source = tab.buffer.clone();
            tab.status = "Applied".to_string();
            Ok(())
        } else {
            tab.status = "Lint error".to_string();
            Err(format!("{} has lint errors", tab.title))
        }
    }

    pub fn reset_tab_to_default(&mut self, index: usize) -> Result<(), String> {
        let source = self
            .tabs
            .get(index)
            .ok_or_else(|| format!("script tab {index} does not exist"))?
            .default_source;
        self.set_source(index, source)
    }

    pub fn source_line_offset(&self, index: usize) -> u32 {
        self.tabs[index].lint_prefix.lines().count() as u32
    }

    pub fn user_breakpoints(&self, index: usize) -> Vec<u32> {
        self.tabs[index].breakpoints.iter().copied().collect()
    }

    pub fn update_auto_apply(&mut self, now: Instant) -> Vec<usize> {
        let mut applied = Vec::new();
        for (index, tab) in self.tabs.iter_mut().enumerate() {
            let Some(edited_at) = tab.edited_at else {
                continue;
            };
            if now.duration_since(edited_at) < self.cooldown {
                continue;
            }
            lint_tab(tab);
            if tab.diagnostics.is_empty() {
                tab.active_source = tab.buffer.clone();
                tab.status = "Applied".to_string();
                tab.applied_at = Some(now);
                applied.push(index);
            } else {
                tab.status = "Waiting for fixes".to_string();
            }
            tab.edited_at = None;
        }
        applied
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) -> Vec<EditorAction> {
        let mut actions = Vec::new();
        let panel_width = ui.available_width().max(180.0);
        let panel_height = ui.available_height().max(420.0);
        ui.set_width(panel_width);
        ui.heading("Live RustScript");
        ui.add_space(6.0);

        let active = self.active.min(self.tabs.len().saturating_sub(1));
        ui.horizontal_wrapped(|ui| {
            if ui.button("Reset").clicked() {
                actions.push(self.reset_active_tab(active));
            }
            let debug_label = if self.debug_starting {
                "Starting..."
            } else if self.debug_pending && self.debug_tab == Some(active) {
                "Armed"
            } else {
                "Debug"
            };
            if ui
                .add_enabled(
                    !self.debug_starting && !self.debug_attached && !self.debug_pending,
                    egui::Button::new(debug_label),
                )
                .clicked()
            {
                actions.push(EditorAction::StartDebug(active));
            }
            if ui
                .add_enabled(
                    self.debug_attached && self.debug_tab == Some(active),
                    egui::Button::new("Step"),
                )
                .clicked()
            {
                actions.push(EditorAction::StepDebug);
            }
            if ui
                .add_enabled(
                    self.debug_attached && self.debug_tab == Some(active),
                    egui::Button::new("Next"),
                )
                .clicked()
            {
                actions.push(EditorAction::NextDebug);
            }
            if ui
                .add_enabled(
                    self.debug_attached && self.debug_tab == Some(active),
                    egui::Button::new("Continue"),
                )
                .clicked()
            {
                actions.push(EditorAction::ContinueDebug);
            }
            if ui
                .add_enabled(
                    self.debug_attached && self.debug_tab == Some(active),
                    egui::Button::new("Locals"),
                )
                .clicked()
            {
                actions.push(EditorAction::RefreshLocals);
            }
        });

        ui.horizontal_wrapped(|ui| {
            for (index, tab) in self.tabs.iter().enumerate() {
                if ui
                    .selectable_label(self.active == index, tab.title)
                    .clicked()
                {
                    self.active = index;
                }
            }
        });
        ui.separator();

        let active = self.active.min(self.tabs.len().saturating_sub(1));
        let tab = &mut self.tabs[active];
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(&tab.status).color(status_color(tab)));
            if self.debug_starting {
                ui.label("debugger starting");
            } else if self.debug_pending && self.debug_tab == Some(active) {
                ui.label("debugger armed");
            } else if let Some(line) = self.debug_line {
                if self.debug_tab == Some(active) {
                    ui.label(format!("debug line {line}"));
                }
            } else if self.debug_attached {
                ui.label("debugger attached");
            }
        });

        let code_width = ui.available_width().max(160.0);
        let console_height = 178.0;
        let remaining_height = ui.available_height().max(panel_height - 140.0);
        let code_height = (remaining_height - console_height - 48.0).max(240.0);
        let line_count = tab.buffer.lines().count().max(1);
        let row_height = (CODE_FONT_SIZE + 4.0).max(16.0);
        let content_height = (line_count as f32 * row_height + 24.0).max(code_height);
        let gutter_width = 42.0;
        let text_width = (code_width - gutter_width - 8.0).max(140.0);
        let active_debug_line = (self.debug_tab == Some(active))
            .then_some(self.debug_line)
            .flatten();
        let mut layouter = |ui: &egui::Ui, text: &dyn egui::TextBuffer, wrap_width: f32| {
            let mut job = rustscript_layout_job(
                text.as_str(),
                &tab.diagnostics,
                tab.host_apis,
                active_debug_line,
            );
            job.wrap.max_width = wrap_width.min(text_width).max(120.0);
            ui.fonts_mut(|fonts| fonts.layout_job(job))
        };
        let mut response_changed = false;
        let mut hovered_name = None;
        let mut editor_response = None;
        egui::ScrollArea::both()
            .id_salt(("live_rustscript_code", active))
            .max_height(code_height)
            .min_scrolled_height(code_height)
            .min_scrolled_width(text_width)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    ui.set_height(content_height);
                    ui.vertical(|ui| {
                        ui.set_width(gutter_width);
                        ui.add_space(4.0);
                        for line_index in 1..=line_count {
                            let line = line_index as u32;
                            let has_breakpoint = tab.breakpoints.contains(&line);
                            let text = if has_breakpoint { "●" } else { " " };
                            let button = egui::Button::new(
                                egui::RichText::new(text).monospace().size(13.0).color(
                                    if has_breakpoint {
                                        egui::Color32::from_rgb(245, 70, 82)
                                    } else {
                                        egui::Color32::from_rgb(92, 102, 116)
                                    },
                                ),
                            )
                            .frame(false)
                            .min_size(egui::vec2(18.0, row_height));
                            let line_response = ui
                                .horizontal(|ui| {
                                    let clicked = ui.add(button).clicked();
                                    ui.label(
                                        egui::RichText::new(format!("{line_index:>2}"))
                                            .monospace()
                                            .size(11.0)
                                            .color(egui::Color32::from_rgb(108, 118, 132)),
                                    );
                                    clicked
                                })
                                .inner;
                            if line_response {
                                let enabled = !has_breakpoint;
                                if enabled {
                                    tab.breakpoints.insert(line);
                                } else {
                                    tab.breakpoints.remove(&line);
                                }
                                actions.push(EditorAction::ToggleBreakpoint {
                                    tab: active,
                                    line,
                                    enabled,
                                });
                            }
                        }
                    });
                    let output = ui
                        .scope(|ui| {
                            ui.set_min_size(egui::vec2(text_width, content_height));
                            egui::TextEdit::multiline(&mut tab.buffer)
                                .font(code_font_id())
                                .desired_width(text_width)
                                .desired_rows(line_count)
                                .lock_focus(true)
                                .layouter(&mut layouter)
                                .show(ui)
                        })
                        .inner;
                    response_changed = output.response.changed();
                    if self.debug_attached
                        && self.debug_tab == Some(active)
                        && output.response.hovered()
                        && let Some(pointer_pos) = ui.ctx().pointer_hover_pos()
                        && output.text_clip_rect.contains(pointer_pos)
                    {
                        let cursor = output
                            .galley
                            .cursor_from_pos(pointer_pos - output.galley_pos);
                        hovered_name = identifier_at_char_index(&tab.buffer, cursor.index);
                    }
                    editor_response = Some(output.response);
                });
            });
        if response_changed {
            tab.edited_at = Some(Instant::now());
            lint_tab(tab);
            tab.status = if tab.diagnostics.is_empty() {
                "Pending apply".to_string()
            } else {
                "Lint error".to_string()
            };
        }
        render_script_diagnostics(ui, &tab.diagnostics);

        let now = Instant::now();
        match hovered_name {
            Some(name) => {
                let is_same = self
                    .debug_hover
                    .as_ref()
                    .is_some_and(|hover| hover.tab == active && hover.name == name);
                if !is_same {
                    self.debug_hover = Some(DebugHoverState {
                        tab: active,
                        name,
                        started_at: now,
                        requested: false,
                        value: None,
                    });
                }
            }
            None => self.debug_hover = None,
        }
        if let Some(hover) = self.debug_hover.as_mut()
            && !hover.requested
            && now.duration_since(hover.started_at) >= Duration::from_millis(280)
        {
            hover.requested = true;
            actions.push(EditorAction::EvaluateHover {
                tab: hover.tab,
                name: hover.name.clone(),
            });
        }
        if let (Some(response), Some(hover)) = (editor_response, self.debug_hover.as_ref()) {
            let value = hover.value.as_deref().unwrap_or("loading...");
            response.on_hover_ui_at_pointer(|ui| {
                ui.label(egui::RichText::new(&hover.name).monospace().strong());
                ui.label(egui::RichText::new(value).monospace());
            });
        }

        ui.separator();
        render_debug_console(ui, self, code_width, console_height, &mut actions);
        actions
    }
}

fn render_debug_console(
    ui: &mut egui::Ui,
    editor: &mut LiveScriptEditor,
    width: f32,
    height: f32,
    actions: &mut Vec<EditorAction>,
) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("DEBUG CONSOLE").strong().size(11.0));
        let status = if editor.debug_attached {
            "paused"
        } else if editor.debug_starting || editor.debug_pending {
            "waiting"
        } else {
            "inactive"
        };
        ui.label(
            egui::RichText::new(status)
                .monospace()
                .size(10.0)
                .color(egui::Color32::from_rgb(132, 146, 164)),
        );
    });
    egui::Frame::new()
        .fill(egui::Color32::from_rgb(22, 25, 30))
        .inner_margin(egui::Margin::same(6))
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("live_rustscript_debug_console")
                .max_height(height - 48.0)
                .min_scrolled_height(height - 48.0)
                .stick_to_bottom(true)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.set_width((width - 16.0).max(120.0));
                    let output = if editor.debug_output.trim().is_empty() {
                        "Start a debug session to run commands."
                    } else {
                        &editor.debug_output
                    };
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(output)
                                .monospace()
                                .size(10.5)
                                .color(egui::Color32::from_rgb(202, 212, 224)),
                        )
                        .selectable(true),
                    );
                });
        });

    let mut submit = false;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(">").monospace().strong());
        let input_width = (ui.available_width() - 48.0).max(80.0);
        let response = ui.add_enabled(
            editor.debug_attached,
            egui::TextEdit::singleline(&mut editor.console_input)
                .font(code_font_id())
                .desired_width(input_width)
                .hint_text("help | locals | print name | where"),
        );
        let enter = ui.input(|input| input.key_pressed(egui::Key::Enter));
        submit |= console_enter_submits(response.has_focus(), response.lost_focus(), enter);

        if response.has_focus() && ui.input(|input| input.key_pressed(egui::Key::ArrowUp)) {
            move_console_history(editor, -1);
        }
        if response.has_focus() && ui.input(|input| input.key_pressed(egui::Key::ArrowDown)) {
            move_console_history(editor, 1);
        }
        submit |= ui
            .add_enabled(editor.debug_attached, egui::Button::new("Run"))
            .clicked();
    });
    if submit {
        submit_console_input(editor, actions);
    }
}

fn console_enter_submits(has_focus: bool, lost_focus: bool, enter: bool) -> bool {
    (has_focus || lost_focus) && enter
}

fn submit_console_input(editor: &mut LiveScriptEditor, actions: &mut Vec<EditorAction>) {
    let command = editor.console_input.trim().to_string();
    if command.is_empty() {
        return;
    }
    if editor.console_history.last() != Some(&command) {
        editor.console_history.push(command.clone());
        const MAX_CONSOLE_HISTORY: usize = 64;
        if editor.console_history.len() > MAX_CONSOLE_HISTORY {
            editor.console_history.remove(0);
        }
    }
    editor.console_history_cursor = None;
    editor.console_input.clear();
    actions.push(EditorAction::RunDebugCommand(command));
}

fn move_console_history(editor: &mut LiveScriptEditor, direction: i32) {
    if editor.console_history.is_empty() {
        return;
    }
    let len = editor.console_history.len();
    let next = match (editor.console_history_cursor, direction.signum()) {
        (None, -1) => len - 1,
        (Some(index), -1) => index.saturating_sub(1),
        (Some(index), 1) if index + 1 < len => index + 1,
        (Some(_), 1) | (None, 1) => {
            editor.console_history_cursor = None;
            editor.console_input.clear();
            return;
        }
        _ => return,
    };
    editor.console_history_cursor = Some(next);
    editor
        .console_input
        .clone_from(&editor.console_history[next]);
}

fn status_color(tab: &ScriptTab) -> egui::Color32 {
    if tab.diagnostics.is_empty() && tab.buffer == tab.active_source {
        egui::Color32::from_rgb(118, 218, 166)
    } else if tab.diagnostics.is_empty() {
        egui::Color32::from_rgb(245, 201, 98)
    } else {
        egui::Color32::from_rgb(255, 120, 135)
    }
}

fn lint_tab(tab: &mut ScriptTab) {
    tab.diagnostics = script_compile_diagnostics(&tab.buffer, tab.lint_prefix);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptDiagnostic {
    pub line: usize,
    pub start_col: usize,
    pub end_col: usize,
    pub message: String,
    pub source_line: String,
    pub start_byte: usize,
    pub end_byte: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScriptTokenKind {
    Keyword,
    Type,
    Number,
    String,
    Comment,
    Function,
    HostApi,
    Operator,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ScriptToken {
    start: usize,
    end: usize,
    kind: ScriptTokenKind,
}

impl ScriptToken {
    fn text<'a>(self, source: &'a str) -> &'a str {
        &source[self.start..self.end]
    }
}

const SCRIPT_KEYWORDS: &[&str] = &[
    "as", "break", "continue", "else", "false", "fn", "for", "if", "let", "match", "mut", "null",
    "pub", "return", "struct", "true", "use", "while",
];

const SCRIPT_TYPES: &[&str] = &[
    "array", "bool", "bytes", "float", "int", "map", "number", "string",
];

fn rustscript_highlight_tokens(source: &str, host_apis: &[&str]) -> Vec<ScriptToken> {
    let mut tokens = Vec::new();
    let bytes = source.as_bytes();
    let mut cursor = 0usize;

    while cursor < bytes.len() {
        let ch = bytes[cursor] as char;
        if ch.is_ascii_whitespace() {
            cursor += 1;
            continue;
        }

        if source[cursor..].starts_with("//") {
            let end = source[cursor..]
                .find('\n')
                .map(|offset| cursor + offset)
                .unwrap_or(source.len());
            tokens.push(ScriptToken {
                start: cursor,
                end,
                kind: ScriptTokenKind::Comment,
            });
            cursor = end;
            continue;
        }

        if source[cursor..].starts_with("/*") {
            let end = source[cursor + 2..]
                .find("*/")
                .map(|offset| cursor + 2 + offset + 2)
                .unwrap_or(source.len());
            tokens.push(ScriptToken {
                start: cursor,
                end,
                kind: ScriptTokenKind::Comment,
            });
            cursor = end;
            continue;
        }

        if ch == '"' || source[cursor..].starts_with("b\"") {
            let start = cursor;
            if source[cursor..].starts_with("b\"") {
                cursor += 2;
            } else {
                cursor += 1;
            }
            let mut escaped = false;
            while cursor < bytes.len() {
                let current = bytes[cursor] as char;
                cursor += 1;
                if escaped {
                    escaped = false;
                    continue;
                }
                if current == '\\' {
                    escaped = true;
                    continue;
                }
                if current == '"' {
                    break;
                }
            }
            tokens.push(ScriptToken {
                start,
                end: cursor,
                kind: ScriptTokenKind::String,
            });
            continue;
        }

        if ch.is_ascii_digit() {
            let start = cursor;
            cursor += 1;
            while cursor < bytes.len() {
                let current = bytes[cursor] as char;
                if current.is_ascii_digit() || current == '.' {
                    cursor += 1;
                } else {
                    break;
                }
            }
            tokens.push(ScriptToken {
                start,
                end: cursor,
                kind: ScriptTokenKind::Number,
            });
            continue;
        }

        if is_ident_start(ch) {
            let start = cursor;
            cursor += 1;
            while cursor < bytes.len() {
                let current = bytes[cursor] as char;
                if is_ident_continue(current) {
                    cursor += 1;
                    continue;
                }
                if source[cursor..].starts_with("::") {
                    cursor += 2;
                    continue;
                }
                break;
            }
            let text = &source[start..cursor];
            let kind = if host_apis.contains(&text) {
                Some(ScriptTokenKind::HostApi)
            } else if SCRIPT_KEYWORDS.contains(&text) {
                Some(ScriptTokenKind::Keyword)
            } else if SCRIPT_TYPES.contains(&text) {
                Some(ScriptTokenKind::Type)
            } else if next_non_ws_starts_with(source, cursor, '(') {
                Some(ScriptTokenKind::Function)
            } else {
                None
            };
            if let Some(kind) = kind {
                tokens.push(ScriptToken {
                    start,
                    end: cursor,
                    kind,
                });
            }
            continue;
        }

        if "=+-*/%<>!&|?:;,.(){}[]".contains(ch) {
            tokens.push(ScriptToken {
                start: cursor,
                end: cursor + 1,
                kind: ScriptTokenKind::Operator,
            });
        }
        cursor += 1;
    }

    tokens
}

fn is_ident_start(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphabetic()
}

fn is_ident_continue(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphanumeric()
}

fn is_debug_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(is_ident_start) && chars.all(is_ident_continue)
}

fn identifier_at_char_index(source: &str, char_index: usize) -> Option<String> {
    let chars = source.char_indices().collect::<Vec<_>>();
    if chars.is_empty() {
        return None;
    }
    let target = if char_index < chars.len() && is_ident_continue(chars[char_index].1) {
        char_index
    } else if char_index > 0 && is_ident_continue(chars[char_index.min(chars.len()) - 1].1) {
        char_index.min(chars.len()) - 1
    } else {
        return None;
    };
    let mut start = target;
    while start > 0 && is_ident_continue(chars[start - 1].1) {
        start -= 1;
    }
    let mut end = target + 1;
    while end < chars.len() && is_ident_continue(chars[end].1) {
        end += 1;
    }
    let start_byte = chars[start].0;
    let end_byte = chars
        .get(end)
        .map(|(byte, _)| *byte)
        .unwrap_or(source.len());
    let name = &source[start_byte..end_byte];
    if !is_debug_identifier(name)
        || SCRIPT_KEYWORDS.contains(&name)
        || SCRIPT_TYPES.contains(&name)
        || source[..start_byte].ends_with("::")
        || source[end_byte..].starts_with("::")
        || source[end_byte..].trim_start().starts_with('(')
    {
        return None;
    }
    Some(name.to_string())
}

fn next_non_ws_starts_with(source: &str, cursor: usize, needle: char) -> bool {
    source[cursor..]
        .chars()
        .find(|ch| !ch.is_ascii_whitespace())
        == Some(needle)
}

fn script_compile_diagnostics(source: &str, prefix: &str) -> Vec<ScriptDiagnostic> {
    let full_source = format!("{prefix}{source}");
    let prefix_lines = prefix.lines().count();
    match compile_source(&full_source) {
        Ok(_) => Vec::new(),
        Err(SourceError::Parse(err)) => {
            let mut source_map = SourceMap::new();
            let source_id = source_map.add_source("<editor>", full_source);
            let err = err.with_line_span_from_source(&source_map, source_id);
            let line = preferred_parse_diagnostic_line(
                source,
                err.line.saturating_sub(prefix_lines).max(1),
                &err.message,
            );
            let full_line = line + prefix_lines;
            let span = source_map.line_span(source_id, full_line);
            vec![script_diagnostic_from_parts(
                source,
                &source_map,
                source_id,
                full_line,
                prefix_lines,
                span.map(|span| (span.lo, span.hi)),
                err.message,
            )]
        }
        Err(SourceError::Compile(err)) => {
            let mut source_map = SourceMap::new();
            let source_id = source_map.add_source("<editor>", full_source);
            let full_line = err.line().unwrap_or(1).max(1);
            let span = source_map.line_span(source_id, full_line);
            vec![script_diagnostic_from_parts(
                source,
                &source_map,
                source_id,
                full_line,
                prefix_lines,
                span.map(|span| (span.lo, span.hi)),
                err.diagnostic_message(),
            )]
        }
    }
}

fn preferred_parse_diagnostic_line(source: &str, reported_line: usize, message: &str) -> usize {
    if reported_line <= 1 || !message.contains("expected") {
        return reported_line;
    }

    let Some(previous_line) = source.lines().nth(reported_line - 2) else {
        return reported_line;
    };
    if previous_line.matches('(').count() > previous_line.matches(')').count() {
        return reported_line - 1;
    }
    reported_line
}

fn script_diagnostic_from_parts(
    source: &str,
    source_map: &SourceMap,
    source_id: u32,
    full_line: usize,
    prefix_lines: usize,
    span: Option<(usize, usize)>,
    message: String,
) -> ScriptDiagnostic {
    let line = full_line.saturating_sub(prefix_lines).max(1);
    let source_line = source
        .lines()
        .nth(line.saturating_sub(1))
        .unwrap_or_default()
        .to_string();
    let (full_start_byte, full_end_byte) = span.unwrap_or_else(|| {
        source_map
            .line_span(source_id, full_line)
            .map(|span| (span.lo, span.hi))
            .unwrap_or((0, 0))
    });
    let prefix_bytes = line_start_byte_for_full_source(source_map, source_id, prefix_lines + 1);
    let start_byte = full_start_byte.saturating_sub(prefix_bytes);
    let end_byte = full_end_byte
        .saturating_sub(prefix_bytes)
        .max(start_byte + 1);
    let (_, start_col) = source_map
        .line_col_for_offset(source_id, full_start_byte)
        .unwrap_or((full_line, 1));
    let (_, end_col) = source_map
        .line_col_for_offset(source_id, full_end_byte)
        .unwrap_or((full_line, start_col + 1));
    ScriptDiagnostic {
        line,
        start_col,
        end_col: end_col.max(start_col + 1),
        message,
        source_line,
        start_byte,
        end_byte,
    }
}

fn line_start_byte_for_full_source(source_map: &SourceMap, source_id: u32, line: usize) -> usize {
    source_map
        .line_span(source_id, line)
        .map(|span| span.lo)
        .unwrap_or(0)
}

fn rustscript_layout_job(
    source: &str,
    diagnostics: &[ScriptDiagnostic],
    host_apis: &[&str],
    debug_line: Option<u32>,
) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    let mut cursor = 0usize;
    for token in rustscript_highlight_tokens(source, host_apis) {
        if cursor < token.start {
            append_script_text(
                &mut job,
                &source[cursor..token.start],
                plain_format(diagnostics, cursor, token.start, debug_line, source),
            );
        }
        append_script_text(
            &mut job,
            token.text(source),
            token_format(
                token.kind,
                token.start,
                token.end,
                diagnostics,
                debug_line,
                source,
            ),
        );
        cursor = token.end;
    }
    if cursor < source.len() {
        append_script_text(
            &mut job,
            &source[cursor..],
            plain_format(diagnostics, cursor, source.len(), debug_line, source),
        );
    }
    job
}

fn append_script_text(job: &mut egui::text::LayoutJob, text: &str, format: egui::TextFormat) {
    job.append(text, 0.0, format);
}

fn code_font_id() -> egui::FontId {
    egui::FontId::monospace(CODE_FONT_SIZE)
}

fn token_format(
    kind: ScriptTokenKind,
    start: usize,
    end: usize,
    diagnostics: &[ScriptDiagnostic],
    debug_line: Option<u32>,
    source: &str,
) -> egui::TextFormat {
    let mut format = plain_format(diagnostics, start, end, debug_line, source);
    format.color = match kind {
        ScriptTokenKind::Keyword => egui::Color32::from_rgb(117, 190, 255),
        ScriptTokenKind::Type => egui::Color32::from_rgb(106, 214, 179),
        ScriptTokenKind::Number => egui::Color32::from_rgb(255, 206, 112),
        ScriptTokenKind::String => egui::Color32::from_rgb(245, 155, 112),
        ScriptTokenKind::Comment => egui::Color32::from_rgb(130, 148, 166),
        ScriptTokenKind::Function => egui::Color32::from_rgb(209, 184, 255),
        ScriptTokenKind::HostApi => egui::Color32::from_rgb(120, 230, 238),
        ScriptTokenKind::Operator => egui::Color32::from_rgb(182, 192, 210),
    };
    format
}

fn plain_format(
    diagnostics: &[ScriptDiagnostic],
    start: usize,
    end: usize,
    debug_line: Option<u32>,
    source: &str,
) -> egui::TextFormat {
    let mut format = egui::TextFormat {
        font_id: code_font_id(),
        color: egui::Color32::from_rgb(220, 228, 238),
        ..Default::default()
    };
    if diagnostics
        .iter()
        .any(|diagnostic| ranges_overlap(start, end, diagnostic.start_byte, diagnostic.end_byte))
    {
        format.background = egui::Color32::from_rgba_unmultiplied(120, 24, 36, 115);
    }
    if let Some(line) = debug_line
        && range_touches_line(source, start, end, line as usize)
    {
        format.background = egui::Color32::from_rgba_unmultiplied(58, 108, 178, 105);
    }
    format
}

fn range_touches_line(source: &str, start: usize, end: usize, line: usize) -> bool {
    let line_start = line_start_byte(source, line).unwrap_or(0);
    let line_end = source[line_start..]
        .find('\n')
        .map(|offset| line_start + offset)
        .unwrap_or(source.len());
    ranges_overlap(
        start,
        end.max(start + 1),
        line_start,
        line_end.max(line_start + 1),
    )
}

fn ranges_overlap(a_start: usize, a_end: usize, b_start: usize, b_end: usize) -> bool {
    a_start < b_end && b_start < a_end
}

fn render_script_diagnostics(ui: &mut egui::Ui, diagnostics: &[ScriptDiagnostic]) {
    if diagnostics.is_empty() {
        return;
    }

    ui.add_space(6.0);
    for diagnostic in diagnostics {
        ui.colored_label(
            egui::Color32::from_rgb(255, 120, 135),
            format!(
                "line {}:{} {}",
                diagnostic.line, diagnostic.start_col, diagnostic.message
            ),
        );
        ui.monospace(format!(
            "{:>3} | {}",
            diagnostic.line, diagnostic.source_line
        ));
        let pointer_width = diagnostic
            .end_col
            .saturating_sub(diagnostic.start_col)
            .max(1);
        ui.monospace(format!(
            "    | {}{}",
            " ".repeat(diagnostic.start_col.saturating_sub(1)),
            "^".repeat(pointer_width)
        ));
    }
}

fn line_start_byte(source: &str, line: usize) -> Option<usize> {
    if line == 0 {
        return None;
    }
    if line == 1 {
        return Some(0);
    }
    let mut current_line = 1usize;
    for (index, ch) in source.char_indices() {
        if ch == '\n' {
            current_line += 1;
            if current_line == line {
                return Some(index + 1);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOSTS: &[&str] = &["bevy::Gomoku::cell"];

    #[test]
    fn auto_apply_keeps_previous_source_when_lint_fails() {
        let mut editor = LiveScriptEditor::new(vec![ScriptTab::new(
            "move.rss",
            "let ok: int = 1;\nok",
            "let move_x: int = 0;\n",
            HOSTS,
        )]);
        editor.lint_all();
        editor.tabs[0].buffer = "let broken = ".to_string();
        editor.tabs[0].edited_at = Some(Instant::now() - Duration::from_secs(2));
        let applied = editor.update_auto_apply(Instant::now());

        assert!(applied.is_empty());
        assert_eq!(editor.tabs[0].active_source, "let ok: int = 1;\nok");
        assert!(!editor.tabs[0].diagnostics.is_empty());
    }

    #[test]
    fn diagnostics_are_mapped_after_lint_prefix() {
        let diagnostics =
            script_compile_diagnostics("let value: int = ;", "let move_x: int = 0;\n");

        assert_eq!(diagnostics[0].line, 1);
        assert!(diagnostics[0].start_byte < "let value: int = ;".len());
    }

    #[test]
    fn reset_active_tab_restores_default_source_and_stops_debugging() {
        let mut editor = LiveScriptEditor::new(vec![ScriptTab::new(
            "move.rss",
            "let ok: int = 1;",
            "let move_x: int = 0;\n",
            HOSTS,
        )]);
        editor.set_source(0, "let changed: int = 2;").unwrap();
        editor.debug_starting = true;
        editor.debug_attached = true;
        editor.debug_line = Some(8);
        editor.debug_output = "old output".to_string();

        let action = editor.reset_active_tab(0);

        assert_eq!(action, EditorAction::StopDebug);
        assert_eq!(editor.tabs[0].buffer, "let ok: int = 1;");
        assert_eq!(editor.tabs[0].active_source, "let ok: int = 1;");
        assert_eq!(editor.tabs[0].status, "Applied");
        assert!(!editor.debug_starting);
        assert!(!editor.debug_attached);
        assert_eq!(editor.debug_line, None);
        assert!(editor.debug_output.is_empty());
    }

    #[test]
    fn debug_start_reports_pending_state_immediately() {
        let mut editor = LiveScriptEditor::new(vec![ScriptTab::new(
            "move.rss",
            "let ok: int = 1;",
            "let move_x: int = 0;\n",
            HOSTS,
        )]);
        editor.debug_output = "old output".to_string();
        editor.debug_line = Some(8);
        editor.debug_attached = true;

        editor.begin_debug_session(0);

        assert!(editor.debug_starting);
        assert!(!editor.debug_attached);
        assert_eq!(editor.debug_line, None);
        assert_eq!(editor.debug_output, "starting debugger...\n");
    }

    #[test]
    fn debug_lines_are_mapped_after_injected_prefix() {
        assert_eq!(visible_debug_line(Some(1), 3), None);
        assert_eq!(visible_debug_line(Some(3), 3), None);
        assert_eq!(visible_debug_line(Some(4), 3), Some(1));
        assert_eq!(visible_debug_line(Some(9), 3), Some(6));
        assert_eq!(visible_debug_line(None, 3), None);
    }

    #[test]
    fn hover_identifier_selects_locals_and_skips_language_tokens() {
        let source = "let score: int = board_value + helper(score);";
        let score_index = source.find("score").unwrap() + 2;
        let board_index = source.find("board_value").unwrap() + 5;
        let helper_index = source.find("helper").unwrap() + 2;
        let let_index = source.find("let").unwrap() + 1;

        assert_eq!(
            identifier_at_char_index(source, score_index),
            Some("score".to_string())
        );
        assert_eq!(
            identifier_at_char_index(source, board_index),
            Some("board_value".to_string())
        );
        assert_eq!(identifier_at_char_index(source, helper_index), None);
        assert_eq!(identifier_at_char_index(source, let_index), None);
    }

    #[test]
    fn debugger_hover_uses_print_local_command() {
        use std::{sync::mpsc, thread};
        use vm::{Debugger, Vm};

        let compiled = compile_source("let value: int = 42;\nvalue;\n").unwrap();
        let bridge = DebugCommandBridge::new();
        let thread_bridge = bridge.clone();
        let (_sender, receiver) = mpsc::channel();
        let mut session = DebugSession::new(bridge, receiver, 0, 0, Vec::new());
        let mut editor = LiveScriptEditor::new(vec![ScriptTab::new(
            "move.rss",
            "let value: int = 42;\nvalue;\n",
            "",
            HOSTS,
        )]);
        editor.begin_debug_session(0);
        thread::spawn(move || {
            let mut debugger = Debugger::with_command_bridge(thread_bridge);
            debugger.stop_on_entry();
            let mut vm = Vm::new(compiled.program);
            let _ = vm.run_with_debugger(&mut debugger);
        });

        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            session.poll(&mut editor);
            if editor.debug_attached {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        editor.debug_hover = Some(DebugHoverState {
            tab: 0,
            name: "value".to_string(),
            started_at: Instant::now(),
            requested: true,
            value: None,
        });

        session.evaluate_hover(&mut editor, 0, "value");

        assert!(
            editor
                .debug_hover
                .as_ref()
                .and_then(|hover| hover.value.as_deref())
                .is_some_and(|value| value.contains("value ="))
        );
        session.command(&mut editor, "continue");
    }

    #[test]
    fn console_enter_after_focus_loss_submits_command() {
        let mut editor = LiveScriptEditor::new(vec![ScriptTab::new(
            "move.rss",
            "let value: int = 1;",
            "",
            HOSTS,
        )]);
        editor.console_input = "  help  ".to_string();
        let mut actions = Vec::new();

        assert!(console_enter_submits(false, true, true));
        submit_console_input(&mut editor, &mut actions);

        assert_eq!(
            actions,
            vec![EditorAction::RunDebugCommand("help".to_string())]
        );
        assert!(editor.console_input.is_empty());
        assert_eq!(editor.console_history, vec!["help".to_string()]);
    }

    #[test]
    fn debug_session_skips_injected_prefix_before_showing_user_source() {
        use std::{sync::mpsc, thread};
        use vm::{Debugger, Vm};

        let compiled =
            compile_source("let hidden: int = 0;\nlet shown: int = hidden + 1;\n").unwrap();
        let bridge = DebugCommandBridge::new();
        let thread_bridge = bridge.clone();
        let (sender, receiver) = mpsc::channel();
        let join = thread::spawn(move || {
            let mut debugger = Debugger::with_command_bridge(thread_bridge);
            debugger.stop_on_entry();
            let mut vm = Vm::new(compiled.program);
            let result = vm.run_with_debugger(&mut debugger);
            let _ = sender.send(format!("{result:?}"));
        });
        let mut session = DebugSession::new(bridge, receiver, 0, 1, Vec::new());
        let mut editor = LiveScriptEditor::new(vec![ScriptTab::new(
            "move.rss",
            "let shown: int = hidden + 1;\n",
            "let hidden: int = 0;\n",
            HOSTS,
        )]);
        editor.begin_debug_session(0);

        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            session.poll(&mut editor);
            if editor.debug_attached && editor.debug_line == Some(1) {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }

        assert!(editor.debug_attached);
        assert_eq!(editor.debug_line, Some(1));
        session.command(&mut editor, "continue");
        join.join().unwrap();
    }

    #[test]
    fn dropping_debug_session_closes_pending_bridge() {
        use std::{sync::mpsc, thread};
        use vm::{Debugger, Vm};

        let compiled = compile_source("let value: int = 1;\n").unwrap();
        let bridge = DebugCommandBridge::new();
        let thread_bridge = bridge.clone();
        let (_sender, receiver) = mpsc::channel();
        let (done_sender, done_receiver) = mpsc::channel();
        let mut session = DebugSession::new(bridge, receiver, 0, 0, Vec::new());
        let mut editor = LiveScriptEditor::new(vec![ScriptTab::new(
            "move.rss",
            "let value: int = 1;\n",
            "",
            HOSTS,
        )]);
        editor.begin_debug_session(0);
        thread::spawn(move || {
            let mut debugger = Debugger::with_command_bridge(thread_bridge);
            debugger.stop_on_entry();
            let mut vm = Vm::new(compiled.program);
            let _ = vm.run_with_debugger(&mut debugger);
            let _ = done_sender.send(());
        });

        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            session.poll(&mut editor);
            if editor.debug_attached {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert!(editor.debug_attached);

        drop(session);

        assert!(done_receiver.recv_timeout(Duration::from_secs(1)).is_ok());
    }
}
