# Shell Output Projection Phase 3: Native Git and Rust Projectors

## Objective

Add native structured projectors for the highest-value codegg workflows: Git state/diff inspection and Rust build/test diagnostics. These projectors should produce semantically useful, low-token, source-navigable command projections that outperform generic terminal compression for common coding tasks.

RTK is intentionally still out of scope for this phase. The purpose is to prove that codegg's native projection layer can do better than opaque text compression when command semantics are known.

## Dependency

This phase assumes:

- Phase 1 command event model exists.
- Phase 2 projection trait and generic projectors exist.
- Raw command output is retained and expansion handles are available.
- Projection selection is centralized enough to add command-specific projectors.

## Design Direction

Add command-specific projectors that inspect the command metadata and raw output, then return structured model-facing text with source handles and raw expansion handles.

Native projectors should be preferred over generic projectors when they support a command. They should also be preferred over RTK later, because codegg can attach source spans, hunk summaries, diagnostics, and task-relevant context.

Suggested support classification:

```rust
pub enum ProjectionSupport {
    Unsupported,
    Fallback,
    Supported { confidence: ProjectionConfidence },
    Preferred { confidence: ProjectionConfidence },
}
```

Native Git/Rust projectors should return `Preferred` for exact command shapes they understand and `Fallback` or `Unsupported` for ambiguous commands.

## Git Projectors

### GitStatusProjector

Support:

- `git status`
- `git status --short`
- `git status --porcelain`
- `git status --porcelain=v1`
- `git status --porcelain=v2` if easy

Preferred approach:

If codegg is initiating the command, prefer a compact status form such as `git status --porcelain=v1 --branch`. If the user explicitly ran plain `git status`, parse output conservatively.

Model-facing output should group files by state:

- branch and upstream summary
- staged modifications
- unstaged modifications
- untracked files
- deleted files
- renamed/copied files
- conflicted files
- ignored files only if explicitly requested

Example projection:

```text
Command 12: git status
Exit: 0
Projection: native-git-status; raw retained: cmd://12/raw

Branch: main...origin/main [ahead 1]
Staged: 2 files
  M crates/codegg-core/src/shell.rs
  A plans/shell_output_projection_phase_01_command_event_model.md
Unstaged: 1 file
  M crates/codegg-tui/src/app.rs
Untracked: 0 files
Conflicts: 0 files
```

Acceptance details:

- Do not include the full verbose `git status` prose when a structured form is available.
- Preserve conflict state explicitly.
- Preserve branch/ahead/behind state when present.
- Include raw handle for exact output.

### GitDiffProjector

Support:

- `git diff`
- `git diff --cached`
- `git diff --staged`
- `git show`
- bounded path-specific diffs

Projection should avoid dumping a large full diff by default. It should produce:

- file list
- additions/deletions per file if available
- binary file markers
- rename/copy markers
- hunk count per file
- hunk headers
- optionally a small number of focused hunks if the diff is small or if a failure points to a file

For codegg-initiated diff inspection, prefer running supplemental cheap commands such as:

```text
git diff --name-status
git diff --numstat
git diff --stat
git diff --unified=3 -- path
```

Do not rerun user commands in this phase unless existing codegg patterns already allow safe supplemental read-only Git commands. If supplemental commands are added, keep them read-only and bounded.

Projection example:

```text
Command 18: git diff
Exit: 0
Projection: native-git-diff-summary; raw retained: cmd://18/raw

Changed files: 3
  M crates/codegg-core/src/shell.rs (+84 -11), 4 hunks
  M crates/codegg-tui/src/command_panel.rs (+27 -3), 2 hunks
  A plans/shell_output_projection_rtk_roadmap.md (+210 -0), 1 hunk

Focused hunks included: 2 of 7. Expand raw diff: cmd://18/stdout
```

Acceptance details:

- Large diffs must not silently flood model context.
- Omitted hunks must be counted and expandable.
- Hunk headers should be preserved for navigation.
- Binary diffs should be represented safely.

### GitLogProjector

Support:

- `git log`
- `git log --oneline`
- `git show --stat`

Projection should cap commit count and preserve:

- commit hash
- subject
- author if available
- date if available
- refs if available
- file stats only when present or requested

This projector is lower priority than status and diff. It can be implemented after them within the same phase if time allows.

## Rust Projectors

### CargoCheckProjector / CargoBuildProjector

Support:

- `cargo check`
- `cargo build`
- `cargo clippy`
- common flags and package selectors

When codegg controls command construction, prefer:

```text
cargo check --message-format=json
cargo build --message-format=json
cargo clippy --message-format=json
```

When parsing user-run ordinary output, fall back to rendered diagnostics.

Structured diagnostic fields:

- level: error/warning/note/help
- error code, such as `E0308`
- message
- file path
- line and column range
- rendered snippet if present
- suggested replacement if present
- child notes/help messages

Projection example:

```text
Command 31: cargo check
Exit: 101
Projection: native-cargo-diagnostics; raw retained: cmd://31/raw

Diagnostics: 2 errors, 1 warning

error[E0308]: mismatched types
  --> crates/codegg-core/src/shell.rs:142:17
  expected `ProjectionResult`, found `String`
  suggestion: wrap with `ProjectionResult::raw(...)`

warning: unused import `CommandOutputProjector`
  --> crates/codegg-core/src/projection.rs:8:5

Expand full stderr: cmd://31/stderr
```

Acceptance details:

- Non-zero exit must be preserved.
- Error code and file/line must be preserved when available.
- Suggested fixes should be preserved when available.
- Warnings should not overshadow errors.
- Raw stderr remains expandable.

### CargoTestProjector

Support:

- `cargo test`
- package/test selectors
- default Rust test harness output

Projection should preserve:

- total passed/failed/ignored/measured/filtered counts
- failing test names
- panic messages
- assertion diffs
- file/line references
- stdout/stderr emitted by failed tests
- test binary or crate context if present

Successful long test output should be aggressively compacted. Failed test output should be conservative.

Projection example:

```text
Command 44: cargo test
Exit: 101
Projection: native-cargo-test; raw retained: cmd://44/raw

Result: FAILED. 138 passed; 2 failed; 0 ignored; 0 measured; 4 filtered out.

Failed tests:
1. projection::tests::retains_stderr_on_failure
   panic at crates/codegg-core/src/projection.rs:288:9
   assertion failed: projected.text.contains("stderr")

2. shell::tests::command_ids_are_monotonic
   panic at crates/codegg-core/src/shell.rs:97:5
   left: 7
   right: 8

Expand full failure output: cmd://44/stdout
```

Acceptance details:

- Failed test names must be visible.
- Panic location must be visible when present.
- Assertion left/right blocks should be preserved.
- Successful boilerplate should be collapsed.
- Raw output remains available.

## Projection Selection Changes

Update the selector from Phase 2:

1. If exact output requested, use raw projection.
2. If command matches a native projector with high confidence, use native projector.
3. If command failed and no native projector supports it, use error-retention projector.
4. If output is small, use raw projector.
5. Otherwise use truncated projector.

Native projectors should be allowed to delegate back to generic projectors if parsing fails.

## Tests

Add fixture-driven tests. Store representative command outputs under an appropriate test fixture directory. Include:

Git:

- clean status
- dirty status with staged/unstaged/untracked files
- conflict status
- small diff
- large multi-file diff
- binary diff marker
- renamed file diff

Rust:

- successful `cargo check`
- `cargo check` with one error
- `cargo check` with multiple errors and warnings
- successful `cargo test`
- failing `cargo test` with panic
- failing `cargo test` with assertion left/right
- colored output if currently supported
- JSON message-format diagnostic fixture if parser is implemented

Assertions should verify both compactness and preservation of critical information.

## Success Criteria

- Native Git status and diff projectors exist and are selected for supported commands.
- Native Rust diagnostics and test projectors exist and are selected for supported commands.
- Structured projections include raw expansion handles.
- Large diffs and long successful test logs are compacted.
- Failed Rust commands preserve diagnostic source spans and failure causes.
- Projectors degrade safely to generic projection if parsing fails.
- Tests cover representative Git and Rust outputs.

## Non-Goals

- Do not integrate RTK in this phase.
- Do not support every language ecosystem yet.
- Do not require perfect parsing of every possible cargo output shape.
- Do not build the final expansion UI yet.
- Do not add model-generated summaries.

## Risks and Caveats

Parsing human-rendered command output is brittle. Prefer structured formats where codegg controls command construction. When parsing user-run commands, be conservative and fall back to generic projection rather than producing an incorrect structured summary.

Git diff projection must not hide security-relevant changes or hunk context when the task is review-oriented. Include policy hooks so review/security modes can request richer diff preservation.
