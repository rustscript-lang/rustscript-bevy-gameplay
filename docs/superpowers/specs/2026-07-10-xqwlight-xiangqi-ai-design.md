# RustScript Xiangqi AI Design

## Objective

Build a complete Xiangqi AI inside `scripts/xiangqi_ai.rss`, following the XQWLight-style engine shape: complete move generation, legal-position filtering, material and piece-square evaluation, alpha-beta search, move ordering, and a simple quiescence pass.

The UI and game loop continue to call `choose_xiangqi_ai_move` exactly as they do today. Rules and AI remain controlled by RustScript scripts, not by an external UCI process.

## References

- XQWLight: small GPL-2.0 Xiangqi engine with piece-square evaluation and search-oriented design.
- ElephantEye: stronger traditional engine; useful as a reference for engine concepts but too large for direct parity.
- Pikafish: modern Stockfish-derived UCI engine; useful as a reference for search concepts, not a fit for embedding into this RustScript demo.

## Scope

The first complete RustScript engine will target playable demo strength, not tournament-level strength.

It must:

- Generate legal moves for general, advisor, elephant, horse, chariot, cannon, and soldier.
- Reject moves that leave the two generals facing.
- Score positions with material, side-relative piece-square tables, soldier crossing bonuses, and simple king exposure penalties.
- Search at fixed shallow depth with negamax alpha-beta.
- Order moves by captures, general capture threat, piece value, and history-like static priority.
- Run a simple quiescence pass over captures and direct general threats.
- Publish one legal move through `bevy::Xiangqi::set_ai_move`.
- Keep JIT enabled and keep trace count reporting.
- Keep the existing UI contract and script smoke output.

It will not add an opening book, transposition table, repetition adjudication, UCI protocol, neural evaluation, pondering, or time management beyond a fixed node budget.

## Architecture

`scripts/xiangqi_ai.rss` will become a single-file RustScript engine with explicit sections:

1. Constants for piece values, search depth, node budget, and move buffer limits.
2. Board helpers that read and write through `bevy::Xiangqi::cell` and `bevy::Xiangqi::set_cell`.
3. Move generator helpers that scan destination squares and validate per-piece movement.
4. Make/unmake helpers that mutate the Bevy-backed board and restore it immediately after testing.
5. Evaluation helper that scores material, piece-square placement, crossed soldiers, and exposed generals.
6. Negamax alpha-beta search that returns centipawn-like integer scores.
7. Root search that selects the best move and publishes it.

RustScript currently has no arrays or user-defined functions in this script style, so implementation will use repeated explicit loops and fixed top-level sections. Where helper extraction is not available, each feature will be introduced with tests before expanding the script.

## Data Flow

The Rust side injects `ai_player` and runs `scripts/xiangqi_ai.rss` with JIT enabled.

The script:

1. Reads board dimensions.
2. Iterates all own pieces.
3. Generates legal candidate moves.
4. Orders candidates by tactical priority.
5. Applies a candidate move.
6. Searches the opponent reply tree through alpha-beta.
7. Restores the board.
8. Tracks the best root move.
9. Publishes the move.

The Rust side receives `XiangqiAiMove` and applies it through `scripts/xiangqi_move.rss`, preserving a single rule path for actual board updates.

## Testing Strategy

Tests will grow in red-green steps in `tests/xiangqi_script.rs`:

- AI can move each piece type from a legal minimal position.
- AI rejects candidates that leave generals facing.
- AI captures an exposed general immediately.
- AI prefers a high-value capture over a low-value capture.
- AI avoids a reply that immediately loses its general.
- AI finds a one-ply checkmate-like direct general capture.
- AI chooses a blocking or fleeing move when a chariot attacks its general.
- AI search reports JIT enabled and compiled trace count.
- Script smoke still completes several turns.

The example tests in `examples/xiangqi.rs` stay focused on UI geometry and smoke behavior. Engine behavior belongs in `tests/xiangqi_script.rs`.

## Performance Constraints

The root search should stay responsive for the demo. Initial limits:

- Search depth: 2 plies, then raise to 3 only if smoke and UI latency stay acceptable.
- Quiescence: captures and direct general threats only.
- Node budget: hard cap in script via a counter.
- Move buffers: implicit loop scanning, no persistent Rust-side move list.

AI move time should remain visible through the existing `AI move` label. A slow move is acceptable during development tests, but the default demo should avoid multi-second response on the starting position.

## Licensing

Using GPL-licensed design ideas is acceptable for this open-source project. Direct source translation will be marked in comments only if copied structure or tables are imported. The preferred route is a clean RustScript implementation guided by public engine behavior and tested board positions.

## Risks

- RustScript script size can become hard to maintain if all engine logic lives in one file.
- Deep nested loops may compile but run too slowly under the current VM.
- Some compact boolean expressions have previously hit VM type errors in JIT paths.
- A full engine may reveal missing RustScript language features such as arrays or functions.

Mitigation: land the engine in small tested slices and keep each commit shippable.

## Acceptance Criteria

- `cargo test --test xiangqi_script` passes with new search behavior tests.
- `cargo run --example xiangqi -- --script-smoke` passes and reports JIT enabled.
- `cargo test --tests` passes.
- The UI still shows JIT trace count and AI move time.
- AI candidates are generated for all seven Xiangqi piece types.
- AI uses alpha-beta search and quiescence rather than one-step greedy scoring.
