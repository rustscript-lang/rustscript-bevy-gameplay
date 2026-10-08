use super::*;
use rustscript_bevy_gameplay::cooperative_debug::{CooperativeDebugger, DebugResult};

pub struct DebugSession {
    debugger: CooperativeDebugger,
    tab: usize,
    source_line_offset: u32,
    completed: bool,
    last_output: std::sync::Mutex<String>,
    on_complete: Box<dyn Fn(Result<DebugResult, String>) + Send + Sync>,
}

impl DebugSession {
    pub fn new_web(
        debugger: CooperativeDebugger,
        tab: usize,
        source_line_offset: u32,
        breakpoints: Vec<u32>,
        on_complete: impl Fn(Result<DebugResult, String>) + Send + Sync + 'static,
    ) -> Self {
        for line in breakpoints {
            debugger.command(&format!("break line {}", line + source_line_offset));
        }
        Self {
            debugger,
            tab,
            source_line_offset,
            completed: false,
            last_output: std::sync::Mutex::new(String::new()),
            on_complete: Box::new(on_complete),
        }
    }

    fn refresh(&self, editor: &mut LiveScriptEditor) {
        let snapshot = self.debugger.snapshot();
        editor.debug_tab = Some(self.tab);
        editor.debug_line = visible_debug_line(snapshot.line, self.source_line_offset);
        editor.debug_attached = !snapshot.running && !snapshot.finished;
        editor.debug_starting = snapshot.running;
        editor.debug_pending = false;
        let mut last_output = self.last_output.lock().expect("debug output lock");
        if *last_output != snapshot.output {
            let output = snapshot
                .output
                .rsplit_once(" at ")
                .filter(|(prefix, _)| prefix.starts_with("line breakpoint "))
                .and_then(|(prefix, line)| {
                    line.parse::<u32>().ok().map(|line| {
                        format!(
                            "{prefix} at {}",
                            line.saturating_sub(self.source_line_offset)
                        )
                    })
                })
                .unwrap_or_else(|| snapshot.output.clone());
            append_debug_output(editor, &output);
            *last_output = snapshot.output;
        }
    }

    pub fn poll(&mut self, editor: &mut LiveScriptEditor) {
        self.debugger.advance();
        self.refresh(editor);
        if let Some(result) = self.debugger.take_result() {
            self.completed = true;
            (self.on_complete)(result);
        }
    }

    pub fn is_finished(&self) -> bool {
        self.completed
    }

    pub fn command(&self, editor: &mut LiveScriptEditor, command: &str) {
        self.debugger.command(command);
        self.refresh(editor);
    }

    pub fn console_command(&self, editor: &mut LiveScriptEditor, command: &str) {
        append_debug_output(editor, &format!("> {command}"));
        let parts: Vec<_> = command.split_whitespace().collect();
        match parts.as_slice() {
            [verb @ ("break" | "clear"), "line", line] => {
                if let Ok(line) = line.parse::<u32>() {
                    self.command(
                        editor,
                        &format!(
                            "{verb} line {}",
                            line.saturating_add(self.source_line_offset)
                        ),
                    );
                } else {
                    self.command(editor, command);
                }
            }
            ["where"] => append_debug_output(editor, &format!("line {:?}", editor.debug_line)),
            _ => self.command(editor, command),
        }
    }

    pub fn evaluate_hover(&self, editor: &mut LiveScriptEditor, tab: usize, name: &str) {
        if self.tab != tab || !is_debug_identifier(name) {
            return;
        }
        let value = self.debugger.command(&format!("print {name}"));
        if let Some(hover) = editor.debug_hover.as_mut()
            && hover.tab == tab
            && hover.name == name
        {
            hover.value = Some(value);
        }
    }

    pub fn set_breakpoint(&self, editor: &mut LiveScriptEditor, line: u32, enabled: bool) {
        self.command(
            editor,
            &format!(
                "{} line {}",
                if enabled { "break" } else { "clear" },
                line.saturating_add(self.source_line_offset)
            ),
        );
    }
}
