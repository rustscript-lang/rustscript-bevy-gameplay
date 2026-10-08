//! Resumable script debugging without a blocking command bridge or OS threads.
use super::*;
use std::{cell::Cell, collections::BTreeSet, time::Duration};

#[derive(Debug, Clone, Copy)]
pub enum DebugInvocation {
    GomokuMove {
        x: i64,
        y: i64,
        player: i64,
    },
    GomokuAi {
        player: i64,
        bias: i64,
    },
    XiangqiMove {
        from_x: i64,
        from_y: i64,
        to_x: i64,
        to_y: i64,
        player: i64,
    },
    XiangqiAi {
        player: i64,
        bias: i64,
    },
    Shooter,
}

#[derive(Debug, Clone)]
pub enum DebugResult {
    GomokuMove(GomokuMoveSummary),
    GomokuAi(GomokuAiMove),
    XiangqiMove(XiangqiMoveSummary),
    XiangqiAi(XiangqiAiMove),
    Shooter,
}

#[derive(Debug, Clone)]
pub struct DebugSnapshot {
    pub line: Option<u32>,
    pub running: bool,
    pub finished: bool,
    pub output: String,
}

#[derive(Clone, Copy)]
enum Resume {
    Entry(u32),
    Continue,
    Step,
    Next {
        depth: usize,
        line: Option<u32>,
        ip: usize,
    },
    Out(usize),
}

struct Session {
    vm: Vm,
    world: World,
    invocation: DebugInvocation,
    mode: Option<Resume>,
    breakpoints: BTreeSet<u32>,
    result: Option<Result<DebugResult, String>>,
    finished: bool,
    output: String,
    execution_micros: u128,
    breakpoint_ip: Option<usize>,
}

thread_local! {
    // VM host state stays on the browser's execution thread. Bevy resources
    // contain only a handle, never a world pointer borrowed across frames.
    static SESSIONS: RefCell<HashMap<u64, Session>> = RefCell::new(HashMap::new());
    static NEXT_ID: Cell<u64> = const { Cell::new(1) };
}

pub struct CooperativeDebugger {
    id: u64,
    #[cfg(not(target_arch = "wasm32"))]
    _thread: std::marker::PhantomData<std::rc::Rc<()>>,
}

impl CooperativeDebugger {
    pub fn new(world: &World, source: &str, invocation: DebugInvocation) -> Result<Self, String> {
        let mut snapshot = World::new();
        let (prefix, offset) = match invocation {
            DebugInvocation::GomokuMove { x, y, player } => {
                snapshot.insert_resource(
                    world
                        .get_resource::<GomokuBoard>()
                        .cloned()
                        .unwrap_or_default(),
                );
                ensure_gomoku_resources(&mut snapshot);
                (
                    format!(
                        "let move_x: int = {x};\nlet move_y: int = {y};\nlet player: int = {player};\n"
                    ),
                    3,
                )
            }
            DebugInvocation::GomokuAi { player, bias } => {
                snapshot.insert_resource(
                    world
                        .get_resource::<GomokuBoard>()
                        .cloned()
                        .unwrap_or_default(),
                );
                ensure_gomoku_resources(&mut snapshot);
                (
                    format!("let ai_player: int = {player};\nlet ai_bias: int = {bias};\n"),
                    2,
                )
            }
            DebugInvocation::XiangqiMove {
                from_x,
                from_y,
                to_x,
                to_y,
                player,
            } => {
                snapshot.insert_resource(
                    world
                        .get_resource::<XiangqiBoard>()
                        .cloned()
                        .unwrap_or_default(),
                );
                ensure_xiangqi_resources(&mut snapshot);
                (
                    format!(
                        "let from_x: int = {from_x};\nlet from_y: int = {from_y};\nlet to_x: int = {to_x};\nlet to_y: int = {to_y};\nlet player: int = {player};\n"
                    ),
                    5,
                )
            }
            DebugInvocation::XiangqiAi { player, bias } => {
                snapshot.insert_resource(
                    world
                        .get_resource::<XiangqiBoard>()
                        .cloned()
                        .unwrap_or_default(),
                );
                ensure_xiangqi_resources(&mut snapshot);
                (
                    format!("let ai_player: int = {player};\nlet ai_bias: int = {bias};\n"),
                    2,
                )
            }
            DebugInvocation::Shooter => {
                // An isolated rule evaluation does not spawn duplicate live ships.
                (String::new(), 0)
            }
        };
        Self::from_source_snapshot(snapshot, &format!("{prefix}{source}"), invocation, offset)
    }

    /// Creates a resumable Shooter debugger using a supplied gameplay snapshot.
    pub fn from_shooter_snapshot(snapshot: World, source: &str) -> Result<Self, String> {
        Self::from_source_snapshot(snapshot, source, DebugInvocation::Shooter, 0)
    }

    fn from_source_snapshot(
        snapshot: World,
        source: &str,
        invocation: DebugInvocation,
        offset: u32,
    ) -> Result<Self, String> {
        let compiled = compile_bevy_script(source).map_err(|err| err.to_string())?;
        let mut vm = Vm::new(compiled.program.with_local_count(compiled.locals));
        bind_composed_bevy_hosts(&mut vm).map_err(|err| err.to_string())?;
        let mut jit = *vm.jit_config();
        jit.enabled = false;
        vm.set_jit_config(jit);
        vm.set_fuel_check_interval(1)
            .map_err(|err| err.to_string())?;
        let session = Session {
            vm,
            world: snapshot,
            invocation,
            mode: Some(Resume::Entry(offset)),
            breakpoints: BTreeSet::new(),
            result: None,
            finished: false,
            output: "debugger attached (isolated world)".to_string(),
            execution_micros: 0,
            breakpoint_ip: None,
        };
        let id = NEXT_ID.with(|next| {
            let id = next.get();
            next.set(id + 1);
            id
        });
        SESSIONS.with(|sessions| {
            sessions.borrow_mut().insert(id, session);
        });
        Ok(Self {
            id,
            #[cfg(not(target_arch = "wasm32"))]
            _thread: std::marker::PhantomData,
        })
    }

    pub fn snapshot(&self) -> DebugSnapshot {
        SESSIONS.with(|sessions| {
            let sessions = sessions.borrow();
            let session = &sessions[&self.id];
            DebugSnapshot {
                line: if session.finished {
                    None
                } else {
                    session.line()
                },
                running: session.mode.is_some(),
                finished: session.finished,
                output: session.output.clone(),
            }
        })
    }

    /// Advances at most 4,096 instructions and 3 ms per Bevy frame.
    pub fn advance(&self) {
        SESSIONS.with(|sessions| {
            let mut sessions = sessions.borrow_mut();
            let session = sessions.get_mut(&self.id).expect("live debug handle");
            session.advance();
        });
    }

    pub fn command(&self, command: &str) -> String {
        SESSIONS.with(|sessions| {
            let mut sessions = sessions.borrow_mut();
            sessions
                .get_mut(&self.id)
                .expect("live debug handle")
                .command(command)
        })
    }

    pub fn take_result(&self) -> Option<Result<DebugResult, String>> {
        SESSIONS.with(|sessions| sessions.borrow_mut().get_mut(&self.id)?.result.take())
    }
}

impl Drop for CooperativeDebugger {
    fn drop(&mut self) {
        SESSIONS.with(|sessions| {
            sessions.borrow_mut().remove(&self.id);
        });
    }
}

impl Session {
    fn line(&self) -> Option<u32> {
        self.vm
            .debug_info()
            .and_then(|info| info.line_for_offset(self.vm.ip()))
    }

    fn resolve_line(&self, line: u32) -> Option<u32> {
        self.vm
            .debug_info()?
            .lines
            .iter()
            .map(|entry| entry.line)
            .filter(|&candidate| candidate >= line)
            .min()
    }

    fn command(&mut self, command: &str) -> String {
        let parts: Vec<_> = command.split_whitespace().collect();
        let output = match parts.as_slice() {
            ["locals"] => self.locals(None),
            ["print", name] => self.locals(Some(name)),
            ["stack"] => format!("stack: {:?}", self.vm.stack()),
            ["where"] => format!("line {:?}", self.line()),
            ["break", "line", line] | ["clear", "line", line] => {
                match line.parse::<u32>().ok().filter(|&line| line > 0).and_then(|line| self.resolve_line(line)) {
                    Some(line) => {
                        if parts[0] == "break" { self.breakpoints.insert(line); }
                        else { self.breakpoints.remove(&line); }
                        format!("line breakpoint {} at {line}", if parts[0] == "break" { "set" } else { "cleared" })
                    }
                    None => "no executable line at this location".to_string(),
                }
            }
            ["continue"] | ["step"] | ["next"] | ["out"] if !self.finished && self.mode.is_none() => {
                self.mode = Some(match parts[0] {
                    "step" => Resume::Step,
                    "next" => Resume::Next { depth: self.vm.call_depth(), line: self.line(), ip: self.vm.ip() },
                    "out" => Resume::Out(self.vm.call_depth()),
                    _ => Resume::Continue,
                });
                return "running".to_string();
            }
            ["pause"] if !self.finished => { self.mode = None; "paused".to_string() }
            ["continue"] | ["step"] | ["next"] | ["out"] => {
                if self.finished { "program halted" } else { "debugger is running" }.to_string()
            }
            _ => "commands: step, next, out, continue, pause, where, locals, stack, print NAME, break line N, clear line N".to_string(),
        };
        self.output = output.clone();
        output
    }

    fn locals(&self, name: Option<&str>) -> String {
        let Some(info) = self.vm.debug_info() else {
            return "no debug info".to_string();
        };
        let line = self.line();
        let values: Vec<_> = info
            .locals
            .iter()
            .filter(|local| name.is_none_or(|name| local.name == name))
            .filter(|local| {
                line.is_none_or(|line| {
                    local.declared_line.is_none_or(|start| line >= start)
                        && local.last_line.is_none_or(|end| line <= end)
                })
            })
            .map(|local| {
                format!(
                    "{} = {:?}",
                    local.name,
                    self.vm
                        .locals()
                        .get(local.index as usize)
                        .unwrap_or(&Value::Null)
                )
            })
            .collect();
        if values.is_empty() {
            "locals: <none visible>".to_string()
        } else {
            values.join("\n")
        }
    }

    fn advance(&mut self) {
        let Some(mode) = self.mode else {
            return;
        };
        let started = Instant::now();
        for _ in 0..4096 {
            if started.elapsed() >= Duration::from_millis(3) {
                break;
            }
            let line = self.line();
            if !matches!(mode, Resume::Entry(_))
                && line.is_some_and(|line| self.breakpoints.contains(&line))
                && self.breakpoint_ip != Some(self.vm.ip())
            {
                self.breakpoint_ip = Some(self.vm.ip());
                self.mode = None;
                self.output = format!("line breakpoint hit at {}", line.unwrap());
                break;
            }
            if matches!(mode, Resume::Entry(offset) if line.is_some_and(|line| line > offset)) {
                self.mode = None;
                self.output = "paused at script entry".to_string();
                break;
            }
            let prior_line = line;
            self.vm.set_fuel(1);
            let outcome = match self.invocation {
                DebugInvocation::GomokuMove { .. } | DebugInvocation::GomokuAi { .. } => {
                    with_gomoku_context(&mut self.world, || {
                        self.vm.run().map_err(|err| err.to_string())
                    })
                }
                DebugInvocation::XiangqiMove { .. } | DebugInvocation::XiangqiAi { .. } => {
                    with_xiangqi_context(&mut self.world, || {
                        self.vm.run().map_err(|err| err.to_string())
                    })
                }
                DebugInvocation::Shooter => with_shooter_context(&mut self.world, || {
                    self.vm.run().map_err(|err| err.to_string())
                }),
            };
            self.vm.clear_fuel();
            // A line breakpoint pauses once until execution leaves that line.
            if self.line() != prior_line {
                self.breakpoint_ip = None;
            } else if self.breakpoint_ip.is_some() {
                self.breakpoint_ip = Some(self.vm.ip());
            }
            match outcome {
                Ok(VmStatus::Halted) => {
                    self.finish(None);
                    break;
                }
                Ok(VmStatus::Yielded) => {}
                Ok(status) => {
                    self.finish(Some(format!("unsupported debug VM status: {status:?}")));
                    break;
                }
                Err(error) => {
                    self.finish(Some(error));
                    break;
                }
            }
            let pause = match mode {
                Resume::Step => true,
                Resume::Next { depth, line, ip } => {
                    self.vm.call_depth() <= depth && self.vm.ip() != ip && self.line() != line
                }
                Resume::Out(depth) => self.vm.call_depth() < depth,
                _ => false,
            };
            if pause {
                self.mode = None;
                self.output = "paused".to_string();
                break;
            }
        }
        self.execution_micros += started.elapsed().as_micros();
    }

    fn finish(&mut self, error: Option<String>) {
        self.finished = true;
        self.mode = None;
        let telemetry = GomokuScriptTelemetry {
            jit_enabled: false,
            jit_trace_count: 0,
            elapsed_micros: self.execution_micros,
        };
        let xiangqi_telemetry = XiangqiScriptTelemetry {
            jit_enabled: false,
            jit_trace_count: 0,
            elapsed_micros: self.execution_micros,
        };
        let result = if let Some(error) = error {
            Err(error)
        } else {
            match self.invocation {
                DebugInvocation::GomokuMove { .. } => {
                    let state = self.world.resource::<GomokuScriptState>();
                    Ok(DebugResult::GomokuMove(GomokuMoveSummary {
                        legal: state.legal,
                        winner: state.winner,
                        draw: state.draw,
                        telemetry,
                    }))
                }
                DebugInvocation::GomokuAi { .. } => self
                    .world
                    .resource::<GomokuScriptState>()
                    .ai_move
                    .map(|(x, y)| DebugResult::GomokuAi(GomokuAiMove { x, y, telemetry }))
                    .ok_or_else(|| "gomoku AI script did not select a move".to_string()),
                DebugInvocation::XiangqiMove { .. } => {
                    let state = self.world.resource::<XiangqiScriptState>();
                    Ok(DebugResult::XiangqiMove(XiangqiMoveSummary {
                        legal: state.legal,
                        winner: state.winner,
                        telemetry: xiangqi_telemetry,
                    }))
                }
                DebugInvocation::XiangqiAi { .. } => self
                    .world
                    .resource::<XiangqiScriptState>()
                    .ai_move
                    .map(|(from_x, from_y, to_x, to_y)| {
                        DebugResult::XiangqiAi(XiangqiAiMove {
                            from_x,
                            from_y,
                            to_x,
                            to_y,
                            telemetry: xiangqi_telemetry,
                        })
                    })
                    .ok_or_else(|| "xiangqi AI script did not select a move".to_string()),
                DebugInvocation::Shooter => Ok(DebugResult::Shooter),
            }
        };
        self.output = match &result {
            Ok(result) => format!("debug complete: {result:?}"),
            Err(error) => format!("debug error: {error}"),
        };
        self.result = Some(result);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paused(debugger: &CooperativeDebugger) {
        for _ in 0..1000 {
            debugger.advance();
            if !debugger.snapshot().running {
                return;
            }
        }
        panic!("debugger did not pause");
    }

    fn completed(debugger: &CooperativeDebugger) -> Result<DebugResult, String> {
        debugger.command("continue");
        for _ in 0..10_000 {
            debugger.advance();
            if let Some(result) = debugger.take_result() {
                return result;
            }
        }
        panic!("debugger did not complete");
    }

    #[test]
    fn breakpoints_locals_and_step_preserve_vm_state() {
        let debug = CooperativeDebugger::new(
            &World::new(),
            "let a = 1;\nlet b = 2;\nlet sum = a + b;\nsum;",
            DebugInvocation::Shooter,
        )
        .unwrap();
        paused(&debug);
        debug.command("break line 3");
        debug.command("continue");
        paused(&debug);
        assert_eq!(debug.snapshot().line, Some(3));
        assert!(debug.command("locals").contains("a = Int(1)"));
        assert!(debug.command("print b").contains("Int(2)"));
        debug.command("step");
        paused(&debug);
        assert!(!debug.snapshot().finished);
        debug.command("clear line 3");
        completed(&debug).unwrap();
        assert_eq!(debug.command("stack"), "stack: [Int(3)]");
    }

    #[test]
    fn next_skips_function_body_and_out_returns_to_caller() {
        let source = "fn inc(value: int) -> int {\nlet answer = value + 1;\nanswer\n}\nlet start = 4;\nlet result = inc(start);\nresult;";
        let debug =
            CooperativeDebugger::new(&World::new(), source, DebugInvocation::Shooter).unwrap();
        paused(&debug);
        debug.command("break line 6");
        debug.command("continue");
        paused(&debug);
        assert_eq!(debug.snapshot().line, Some(6));
        debug.command("clear line 6");
        debug.command("next");
        paused(&debug);
        assert_eq!(debug.snapshot().line, Some(7));
        assert!(debug.command("print result").contains("Int(5)"));
        let debug =
            CooperativeDebugger::new(&World::new(), source, DebugInvocation::Shooter).unwrap();
        paused(&debug);
        debug.command("break line 2");
        debug.command("continue");
        paused(&debug);
        assert_eq!(debug.snapshot().line, Some(2));
        debug.command("clear line 2");
        debug.command("out");
        paused(&debug);
        assert!(debug.snapshot().line.is_some_and(|line| line >= 6));
    }

    #[test]
    fn endless_script_yields_to_frames_and_can_be_cancelled() {
        let debug = CooperativeDebugger::new(
            &World::new(),
            "let mut i = 0;\nwhile true { i = i + 1; }\ni;",
            DebugInvocation::Shooter,
        )
        .unwrap();
        paused(&debug);
        debug.command("continue");
        let started = Instant::now();
        debug.advance();
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(debug.snapshot().running);
        debug.command("pause");
        assert!(!debug.snapshot().running);
        let id = debug.id;
        drop(debug);
        assert!(SESSIONS.with(|sessions| !sessions.borrow().contains_key(&id)));
    }

    #[test]
    fn board_hosts_are_rebound_for_each_slice_and_snapshot_is_isolated() {
        let mut world = World::new();
        world.insert_resource(GomokuBoard::default());
        let source = "use bevy;\nlet cell = bevy::Gomoku::cell(move_x, move_y);\nbevy::Gomoku::set_cell(move_x, move_y, player);\nbevy::Gomoku::set_move_result(cell == 0, 0, false);\ntrue;";
        let debug = CooperativeDebugger::new(
            &world,
            source,
            DebugInvocation::GomokuMove {
                x: 2,
                y: 3,
                player: 1,
            },
        )
        .unwrap();
        paused(&debug);
        assert_eq!(debug.snapshot().line, Some(5));
        debug.command("break line 7");
        debug.command("continue");
        paused(&debug);
        assert!(debug.command("print cell").contains("Int(0)"));
        assert_eq!(world.resource::<GomokuBoard>().cell(2, 3), 0);
        debug.command("clear line 7");
        assert!(matches!(
            completed(&debug),
            Ok(DebugResult::GomokuMove(GomokuMoveSummary {
                legal: true,
                ..
            }))
        ));
        assert_eq!(world.resource::<GomokuBoard>().cell(2, 3), 0);
    }

    #[test]
    fn ai_results_survive_pause_and_resume_for_both_boards() {
        let debug = CooperativeDebugger::new(
            &World::new(),
            "use bevy;\nbevy::Gomoku::set_ai_move(ai_player, ai_bias);\ntrue;",
            DebugInvocation::GomokuAi { player: 2, bias: 4 },
        )
        .unwrap();
        paused(&debug);
        assert!(matches!(
            completed(&debug),
            Ok(DebugResult::GomokuAi(GomokuAiMove { x: 2, y: 4, .. }))
        ));
        let debug = CooperativeDebugger::new(
            &World::new(),
            "use bevy;\nbevy::Xiangqi::set_ai_move(1, 2, 3, 4);\ntrue;",
            DebugInvocation::XiangqiAi { player: 2, bias: 0 },
        )
        .unwrap();
        paused(&debug);
        assert!(matches!(
            completed(&debug),
            Ok(DebugResult::XiangqiAi(XiangqiAiMove {
                from_x: 1,
                from_y: 2,
                to_x: 3,
                to_y: 4,
                ..
            }))
        ));
    }

    #[test]
    fn vm_error_releases_execution_and_allows_a_new_session() {
        let debug = CooperativeDebugger::new(
            &World::new(),
            "let zero = 0;\n1 / zero;",
            DebugInvocation::Shooter,
        )
        .unwrap();
        paused(&debug);
        assert!(completed(&debug).is_err());
        assert!(debug.snapshot().finished);
        let debug = CooperativeDebugger::new(
            &World::new(),
            include_str!("../scripts/shooter_game.rss"),
            DebugInvocation::Shooter,
        )
        .unwrap();
        paused(&debug);
        assert!(matches!(completed(&debug), Ok(DebugResult::Shooter)));
    }

    #[test]
    fn all_shooter_tabs_resume_against_gameplay_snapshots() {
        let mut world = World::new();
        apply_shooter_script(&mut world, include_str!("../scripts/shooter_game.rss")).unwrap();
        world.insert_resource(ShooterFrame(HashMap::from([
            ("delta_ms".into(), 300.0),
            ("input_x".into(), 1.0),
        ])));
        let player = world
            .query_filtered::<Entity, With<Player>>()
            .single(&world)
            .unwrap();
        let position = *world.get::<Position>(player).unwrap();
        for source in [
            include_str!("../scripts/shooter_game.rss"),
            include_str!("../scripts/shooter_flow.rss"),
            include_str!("../scripts/shooter_planes.rss"),
            include_str!("../scripts/shooter_projectiles.rss"),
            include_str!("../scripts/shooter_spawns.rss"),
        ] {
            let snapshot = snapshot_shooter_world(&mut world);
            let debug = CooperativeDebugger::from_shooter_snapshot(snapshot, source).unwrap();
            paused(&debug);
            assert!(debug.snapshot().line.is_some());
            assert!(matches!(completed(&debug), Ok(DebugResult::Shooter)));
            assert_eq!(*world.get::<Position>(player).unwrap(), position);
            assert_eq!(world.query::<&Enemy>().iter(&world).count(), 7);
            assert_eq!(world.query::<&ShooterProjectile>().iter(&world).count(), 0);
        }
        let debug = CooperativeDebugger::from_shooter_snapshot(
            snapshot_shooter_world(&mut world),
            "use bevy;\nlet count = bevy::Shooter::entity_count();\ncount;",
        )
        .unwrap();
        paused(&debug);
        debug.command("break line 3");
        debug.command("continue");
        paused(&debug);
        assert!(debug.command("print count").contains("Int(10)"));
    }
}
