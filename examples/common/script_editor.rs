use std::{
    sync::{Arc, Mutex, mpsc::Receiver},
    time::{Duration, Instant},
};

use bevy_egui::egui;
use vm::{DebugCommandBridge, DebugCommandBridgeError, SourceError, SourceMap, compile_source};

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
    pub debug_attached: bool,
    cooldown: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorAction {
    StartDebug(usize),
    StepDebug,
    NextDebug,
    ContinueDebug,
    RefreshLocals,
}

pub struct DebugSession {
    pub bridge: DebugCommandBridge,
    receiver: Arc<Mutex<Receiver<String>>>,
}

impl DebugSession {
    pub fn new(bridge: DebugCommandBridge, receiver: Receiver<String>) -> Self {
        Self {
            bridge,
            receiver: Arc::new(Mutex::new(receiver)),
        }
    }

    pub fn poll(&self, editor: &mut LiveScriptEditor) {
        let status = self.bridge.status();
        editor.debug_attached = status.attached;
        editor.debug_line = status.current_line;
        if let Ok(receiver) = self.receiver.lock() {
            while let Ok(message) = receiver.try_recv() {
                append_debug_output(editor, &message);
            }
        }
    }

    pub fn command(&self, editor: &mut LiveScriptEditor, command: &str) {
        match self.bridge.execute(command, Duration::from_millis(120)) {
            Ok(response) => {
                editor.debug_attached = response.attached;
                editor.debug_line = response.current_line;
                append_debug_output(editor, &response.output);
            }
            Err(DebugCommandBridgeError::NotAttached) => {
                editor.debug_attached = false;
                append_debug_output(editor, "debugger is running\n");
            }
            Err(err) => append_debug_output(editor, &format!("{err}\n")),
        }
    }
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
            debug_attached: false,
            cooldown: Duration::from_millis(850),
        }
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
                let _ = self.reset_tab_to_default(active);
                self.debug_output.clear();
                self.debug_line = None;
                self.debug_attached = false;
            }
            if ui.button("Debug").clicked() {
                actions.push(EditorAction::StartDebug(active));
            }
            if ui
                .add_enabled(self.debug_attached, egui::Button::new("Step"))
                .clicked()
            {
                actions.push(EditorAction::StepDebug);
            }
            if ui
                .add_enabled(self.debug_attached, egui::Button::new("Next"))
                .clicked()
            {
                actions.push(EditorAction::NextDebug);
            }
            if ui
                .add_enabled(self.debug_attached, egui::Button::new("Continue"))
                .clicked()
            {
                actions.push(EditorAction::ContinueDebug);
            }
            if ui
                .add_enabled(self.debug_attached, egui::Button::new("Locals"))
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
            if let Some(line) = self.debug_line {
                ui.label(format!("debug line {line}"));
            }
        });

        let code_width = ui.available_width().max(160.0);
        let debug_output_height = if self.debug_output.trim().is_empty() {
            0.0
        } else {
            190.0
        };
        let remaining_height = ui.available_height().max(panel_height - 140.0);
        let code_height = (remaining_height - debug_output_height - 18.0).max(320.0);
        let mut layouter = |ui: &egui::Ui, text: &dyn egui::TextBuffer, wrap_width: f32| {
            let mut job = rustscript_layout_job(
                text.as_str(),
                &tab.diagnostics,
                tab.host_apis,
                self.debug_line,
            );
            job.wrap.max_width = wrap_width.min(code_width).max(120.0);
            ui.fonts_mut(|fonts| fonts.layout_job(job))
        };
        let response = ui.add_sized(
            [code_width, code_height],
            egui::TextEdit::multiline(&mut tab.buffer)
                .font(egui::TextStyle::Monospace)
                .desired_width(code_width)
                .desired_rows(36)
                .lock_focus(true)
                .layouter(&mut layouter),
        );
        if response.changed() {
            tab.edited_at = Some(Instant::now());
            lint_tab(tab);
            tab.status = if tab.diagnostics.is_empty() {
                "Pending apply".to_string()
            } else {
                "Lint error".to_string()
            };
        }
        render_script_diagnostics(ui, &tab.diagnostics);

        if !self.debug_output.trim().is_empty() {
            ui.separator();
            egui::ScrollArea::vertical()
                .id_salt("live_rustscript_debug_output")
                .max_height(180.0)
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    ui.set_width(code_width);
                    ui.monospace(&self.debug_output);
                });
        }
        actions
    }
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
        font_id: egui::FontId::monospace(13.0),
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
    fn reset_active_tab_restores_default_source() {
        let mut editor = LiveScriptEditor::new(vec![ScriptTab::new(
            "move.rss",
            "let ok: int = 1;",
            "let move_x: int = 0;\n",
            HOSTS,
        )]);
        editor.set_source(0, "let changed: int = 2;").unwrap();

        editor.reset_tab_to_default(0).unwrap();

        assert_eq!(editor.tabs[0].buffer, "let ok: int = 1;");
        assert_eq!(editor.tabs[0].active_source, "let ok: int = 1;");
        assert_eq!(editor.tabs[0].status, "Applied");
    }
}
