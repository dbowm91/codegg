# Human Ephemeral Shell Execution Plan

## Goal

Add a Codex-like human shell execution path to Codegg using `!command` syntax. The feature should let the human run commands such as `!cargo test`, `!rg "Foo"`, or `!git diff --stat` inside the TUI for local exploration and convenience without automatically injecting the command or its output into model-visible conversation context.

The central invariant for the entire implementation is:

> A human `!` command is not model context unless the user explicitly promotes it.

This plan intentionally separates the feature from the existing agent-facing `bash` tool. The existing `BashTool` is useful as a reference point for command policy, environment shaping, output truncation, and command-risk tests, but the human shell path needs streaming, cancellation, session-local output history, and different trust semantics.

## Current repo shape relevant to this work

The current implementation already has several pieces that should be reused or extended rather than replaced.

`src/tool/bash.rs` defines the agent-facing `BashTool`. It executes through `tokio::process::Command`, currently shells through `sh -c`, clears the environment, restores selected development variables, applies timeout handling, blocks dangerous patterns, supports optional path restrictions, can enforce a Landlock sandbox, and returns a single truncated string result. This is model/tool execution infrastructure, not a human convenience shell. Do not simply route `!cmd` through `BashTool::execute()` because that would conflate human-originated ephemeral execution with agent-visible tool execution and would force a non-streaming output model.

The TUI is already structured around an `App` with state domains for UI, session, prompt, messages, dialogs, and agents. `src/tui/app/mod.rs` contains `TuiCommand` for async TUI-side work, while `src/tui/app/types.rs` contains `TuiMsg` for UI events. `architecture/tui.md` documents that the TUI routes session/history/task/memory/worktree actions through `CoreClient`, that prompt submission flows through `TuiMsg::SubmitPrompt`, and that rendering already has message, tool-output, prompt, sidebar, status-bar, dialog, completion, and toast layers.

`src/tui/components/messages.rs` already supports `MsgPart::ToolCall` with status, duration, exit code, output line metadata, and collapsed/expanded rendering semantics. `src/tui/components/tool_output.rs` has a standalone `ToolOutputWidget` with `ToolCallEntry`, risk badges, status labels, duration display, cwd display, summary display, and expandable output. This is close to the visual treatment we want for shell cells, but human shell output should not be labeled as an agent tool unless the code introduces a generic execution-cell abstraction.

The repo has a clear core-boundary policy: `codegg-core` must not depend on UI, tool, permission, TUI, server, plugin, auth, or other app-layer modules. Do not put process spawning, ratatui rendering, or human shell UI state into `codegg-core`.

## Non-goals

Do not make `!command` a slash command. It should be a prompt-prefix command intercepted before normal chat submission.

Do not append `!command` or its raw output to the model transcript by default.

Do not replace the agent-facing `bash` tool in this pass.

Do not implement a full terminal emulator, PTY, job control, or interactive stdin in the first pass. Long-term PTY support can be added later, but the initial feature should target non-interactive developer commands.

Do not persist full shell output to the session database in the first pass. Keep output in a bounded in-memory session-local store. Persistence can be added after an artifact/message-metadata mechanism exists.

## Desired user experience

The minimum useful UX should be:

```text
!cargo test
```

Codegg should run `cargo test` in the current project directory, stream stdout/stderr into a shell output cell or panel, show command id, cwd, status, elapsed time, exit code, truncation status, and make it visually clear that the output was not added to model context.

When the command completes, the user should see something equivalent to:

```text
[cmd #17] cargo test
cwd: /path/to/repo
status: exited 101 after 8.2s
stdout: 180 lines, stderr: 42 lines
model context: not included
next: /shell-include 17, /shell-rerun 17, /shell-kill 17
```

The explicit-promotion path should be:

```text
/shell-include last
/shell-include 17 --tail 200
/shell-include 17 --stderr
/shell-ask 17 why did this fail?
```

A convenience syntax can be added after the base path:

```text
!!cargo test
```

This should run the command and automatically promote an output digest or tail into model-visible context after completion. Implement `!!` only after the explicit `/shell-include` path exists and is tested.

## Phase 1: Human shell domain model and execution substrate

Add a small shell runtime module under the root app layer, not under `codegg-core`. Recommended initial location:

```text
src/shell/mod.rs
src/shell/types.rs
src/shell/runtime.rs
src/shell/store.rs
src/shell/policy.rs
src/shell/digest.rs
```

Register it from `src/lib.rs` or the existing module tree as appropriate.

Define the domain model around origin and capture policy:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellOrigin {
    HumanEphemeral,
    HumanPromoted,
    AgentTool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellCapturePolicy {
    DisplayOnly,
    StoreEphemeral,
    StoreAndPromote,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ShellCommandId(pub u64);

#[derive(Debug, Clone)]
pub struct ShellRequest {
    pub id: ShellCommandId,
    pub origin: ShellOrigin,
    pub command: String,
    pub cwd: PathBuf,
    pub timeout: Duration,
    pub capture_policy: ShellCapturePolicy,
    pub env_policy: ShellEnvPolicy,
}
```

The initial human path should use `ShellOrigin::HumanEphemeral` and `ShellCapturePolicy::StoreEphemeral`. It should store bounded output for later promotion but must not touch the model transcript.

Define streaming events as byte-oriented rather than line-only:

```rust
pub enum ShellEvent {
    Started { id: ShellCommandId, command: String, cwd: PathBuf },
    Stdout { id: ShellCommandId, bytes: bytes::Bytes },
    Stderr { id: ShellCommandId, bytes: bytes::Bytes },
    Exited { id: ShellCommandId, status: Option<i32>, elapsed: Duration },
    TimedOut { id: ShellCommandId, elapsed: Duration },
    FailedToStart { id: ShellCommandId, error: String },
}
```

Byte events preserve progress output, ANSI sequences, partial lines, and tools that do not flush newline-delimited output. The renderer can normalize or sanitize later.

Implement `ShellRuntime::spawn(req, tx)` using `tokio::process::Command`, `stdout(Stdio::piped())`, `stderr(Stdio::piped())`, `kill_on_drop(true)`, and two spawned reader tasks. The runtime should send `ShellEvent`s over an async channel consumed by the TUI event loop.

The first pass may run commands through the user shell for ergonomics:

```text
$SHELL -lc '<command>'
```

Fallback to `sh -lc` when `SHELL` is absent. This preserves pipes, redirects, globs, `&&`, environment expansion, and normal developer expectations. Do not reuse the `BashTool` hard blocklist unchanged for human commands; apply only human-safety warnings/blocking as described below.

Implement timeout and cancellation from the beginning. Timeout defaults should be separate from the agent `bash` tool. Suggested defaults:

```text
human_shell.timeout_secs = 300
human_shell.max_bytes_per_command = 1_000_000
human_shell.max_total_bytes = 8_000_000
human_shell.max_history_entries = 100
```

These can initially be constants, then move into config in Phase 5.

## Phase 2: Bounded session-local shell output store

Add an in-memory `ShellOutputStore` owned by TUI `App` state or by a shell controller held by `App`. It should not be stored in `codegg-core`.

Suggested shape:

```rust
pub struct ShellOutputStore {
    entries: VecDeque<ShellOutputEntry>,
    next_id: u64,
    max_entries: usize,
    max_bytes_per_command: usize,
    max_total_bytes: usize,
}

pub struct ShellOutputEntry {
    pub id: ShellCommandId,
    pub command: String,
    pub cwd: PathBuf,
    pub started_at: SystemTime,
    pub finished_at: Option<SystemTime>,
    pub status: ShellStatus,
    pub stdout: BoundedOutput,
    pub stderr: BoundedOutput,
    pub elapsed: Option<Duration>,
    pub promoted: bool,
}
```

Use a head/tail bounded buffer for each stream, not a naive growing `String`. This preserves early context and final failure context while preventing runaway output from consuming memory:

```rust
pub struct BoundedOutput {
    pub head: Vec<u8>,
    pub tail: Vec<u8>,
    pub omitted_bytes: usize,
    pub total_bytes: usize,
    pub total_lines: usize,
}
```

For small output, `head` can contain all bytes and `tail` can remain empty. For large output, keep a fixed-size head and tail split. Track omitted bytes and lines explicitly so the renderer and promotion logic can state exactly what was truncated.

Add store operations:

```rust
alloc_id() -> ShellCommandId
insert_started(req)
append_stdout(id, bytes)
append_stderr(id, bytes)
mark_exited(id, status, elapsed)
mark_timeout(id, elapsed)
mark_failed_to_start(id, error)
get(id)
get_last()
list_recent(n)
mark_promoted(id)
```

Test the store heavily with small byte caps, UTF-8 boundary cases, stdout/stderr interleaving, and eviction.

## Phase 3: TUI input interception for `!` and `!!`

Intercept bang commands at prompt submission before normal chat submission and before slash-command dispatch.

Recommended flow:

1. User presses Enter.
2. Existing input handling emits `TuiMsg::SubmitPrompt`.
3. In the `SubmitPrompt` handling path, read the current prompt text.
4. If the text begins with `!!` and has a non-empty command after it, dispatch a human shell command with `StoreAndPromote` or defer `!!` until Phase 6.
5. If the text begins with `!` and has a non-empty command after it, dispatch a human shell command with `StoreEphemeral`.
6. Clear the prompt and add the command to prompt history.
7. Do not call the normal chat/model submission path.

Add a parser with narrow, testable behavior:

```rust
pub enum PromptSubmissionKind {
    Chat(String),
    Slash(String),
    HumanShell { command: String, promote_after: bool },
}

pub fn classify_prompt_submission(input: &str) -> PromptSubmissionKind
```

Rules:

```text
!cargo test        => HumanShell { command: "cargo test", promote_after: false }
!!cargo test       => HumanShell { command: "cargo test", promote_after: true }
!                  => Chat or validation error; choose validation error with toast
\!not-shell        => Chat("!not-shell") if escape support is implemented
/command           => Slash, unchanged
normal text        => Chat, unchanged
```

Preserve existing slash and file completion behavior. `!` should not enter slash command completion. Consider adding a simple help hint in the prompt/status bar when the prompt starts with `!`:

```text
shell: run locally; not included in model context
```

## Phase 4: TUI command dispatch and event-loop integration

Add `TuiCommand` variants for shell lifecycle:

```rust
RunHumanShell {
    command: String,
    promote_after: bool,
}
ShellEvent(ShellEvent),
ShellInclude {
    target: ShellTarget,
    mode: ShellPromotionMode,
    question: Option<String>,
}
ShellRerun {
    target: ShellTarget,
}
ShellKill {
    target: ShellTarget,
}
ShellList,
```

Alternatively, keep raw `ShellEvent`s on a separate `mpsc::Receiver<ShellEvent>` owned by the event loop if that is cleaner than routing them through `TuiCommand`. The key requirement is that process I/O must not block rendering or keyboard input.

When `RunHumanShell` is received:

1. Allocate a command id.
2. Resolve cwd from `app.session_state.project_dir` or the active session directory.
3. Insert a started entry into the shell store.
4. Add a visible shell cell to `messages_state` or a dedicated shell output panel.
5. Spawn `ShellRuntime::spawn` in a tokio task.
6. Keep an abort handle keyed by command id for `/shell-kill`.

When shell events arrive:

1. Append stdout/stderr bytes to the store.
2. Update the visible shell cell/panel incrementally.
3. Update status on exit, timeout, or spawn error.
4. Show a toast only for failure/timeout or very short completion; avoid noisy toasts for every command.

## Phase 5: Rendering shell output without confusing it with agent tools

Do not label human `!` commands as model tool calls unless the UI copy clearly says they are human-local shell cells. The user needs to know the model did not see them.

There are two acceptable implementation strategies.

Preferred strategy: add a new message part:

```rust
pub enum MsgPart {
    Text { content: String },
    Reasoning { ... },
    ToolCall { ... },
    ShellCell {
        id: ShellCommandId,
        command: String,
        cwd: String,
        stdout_preview: String,
        stderr_preview: String,
        status: ShellStatus,
        elapsed_ms: Option<u64>,
        exit_code: Option<i32>,
        truncated: bool,
        promoted: bool,
        expanded: bool,
    },
    Image { ... },
}
```

This keeps human shell output distinct from agent tool output.

Fallback strategy: reuse `ToolOutputWidget` internally but rename the visible title for this path to `Shell` and set a human-origin flag. If this path is chosen, add an explicit `ExecutionOrigin` field rather than overloading `ToolRisk` or `ToolStatus`.

Rendering requirements:

```text
- Header: [cmd #N] command, status, elapsed, exit code.
- Cwd line in expanded view.
- Output preview shows stdout and stderr distinctly.
- Collapsed view shows summary and tail.
- Footer/action hint says: not in model context; /shell-include N to attach.
- Preserve ANSI color only after sanitizing unsafe terminal control sequences.
```

Use a conservative ANSI policy initially. Either strip ANSI completely or allow only SGR color/style sequences. Do not render OSC sequences, cursor movement, alternate-screen control, title changes, hyperlinks, or bracketed paste controls from command output.

## Phase 6: Promotion into model-visible context

Add explicit shell promotion commands to the slash-command registry:

```text
/shell-include last
/shell-include <id>
/shell-include <id> --tail <n>
/shell-include <id> --stderr
/shell-include <id> --stdout
/shell-include <id> --summary
/shell-ask <id> <question>
/shell-list
/shell-rerun <id|last>
/shell-kill <id|last>
```

Define promotion modes:

```rust
pub enum ShellPromotionMode {
    Full,
    Tail { lines: usize },
    StdoutOnly,
    StderrOnly,
    Summary,
    FailureDigest,
}
```

Promotion should create a normal user-visible message/artifact whose content clearly marks provenance:

```text
Shell command output attached by user
Command: cargo test
Cwd: /path/to/repo
Exit code: 101
Elapsed: 8.2s
Mode: tail 200 lines

--- stderr ---
...
```

Only this promoted message should enter the model-visible conversation. The original shell cell remains local UI state.

For `/shell-ask <id> <question>`, construct a user message containing a failure digest/tail plus the question. Example:

```text
Using the attached shell output, answer: why did this fail?

[attached shell digest]
```

Mark the shell entry as promoted after successful attachment. If the message-store/core write fails, do not mark it promoted.

After `/shell-include` exists, implement `!!cmd` as equivalent to:

1. Run `!cmd`.
2. On completion, promote a `FailureDigest` if non-zero exit; otherwise promote a compact summary/tail.

Keep `!!` optional behind a config flag if there is concern about accidental context injection.

## Phase 7: Deterministic output digesting

Add deterministic digest helpers before adding any model-based summarization. The first target should be Rust command output because the common path is `cargo check`, `cargo test`, `cargo clippy`, and `cargo fmt --check`.

Add `src/shell/digest.rs` with:

```rust
pub struct ShellDigest {
    pub command: String,
    pub cwd: PathBuf,
    pub exit_code: Option<i32>,
    pub elapsed: Duration,
    pub stdout_summary: Option<String>,
    pub stderr_summary: Option<String>,
    pub extracted_failures: Vec<ShellFailure>,
    pub truncation: TruncationReport,
}

pub enum ShellFailureKind {
    RustCompilerError,
    RustCompilerWarning,
    RustTestFailure,
    Panic,
    GenericNonZeroExit,
}
```

Rust extraction should detect:

```text
error[E....]
warning:
--> path:line:col
thread '...' panicked at ...
failures:
test result: FAILED
expected / actual assertion snippets
```

Do not overfit to one exact cargo format. Use robust regexes and tests with representative fixtures.

Promotion defaults should use digest output first, then append bounded raw tail. This keeps model context small and better suited for automated repair.

## Phase 8: Safety policy for human shell

The human shell path has a different trust model from the agent `bash` tool. It should be less obstructive than agent tool execution, but it still needs guardrails against accidental catastrophic commands.

Implement a soft policy layer:

```rust
pub enum HumanShellPolicyDecision {
    Allow,
    Warn { reason: String },
    Block { reason: String },
}
```

Recommended initial behavior:

Block only obvious catastrophic commands:

```text
rm -rf /
rm -rf /*
mkfs*
dd if=/dev/zero of=/dev/*
:(){:|:&};:
shutdown
reboot
poweroff
halt
```

Warn, but allow after confirmation, for commands such as:

```text
rm -rf .
git clean -xfd
sudo ...
curl ... | sh
wget ... | sh
chmod -R 777
chown -R
```

The confirmation should use the existing confirm-dialog pattern rather than the agent permission registry. This is human-initiated execution, not model-initiated permissioning.

Do not apply the existing `BashTool` injection blocklist wholesale to `!` commands. Human shell commands should support `$HOME`, `$(...)`, backticks, pipes, redirects, `&&`, and normal shell idioms. The stricter `BashTool` policy remains appropriate for agent-originated commands.

Log shell command metadata at debug/info level, but do not log full output by default. Avoid logging secrets that may appear in stdout/stderr.

## Phase 9: Configuration

Add config after the hard-coded defaults work.

Suggested config schema:

```toml
[human_shell]
enabled = true
default_timeout_secs = 300
max_history_entries = 100
max_bytes_per_command = 1000000
max_total_bytes = 8000000
ansi = "sgr-only" # strip | sgr-only | raw
confirm_dangerous = true
auto_promote_bangbang = true
```

Wire this through `codegg-config` if the config schema already owns TUI/application settings. Keep `codegg-core` independent.

Config validation should reject zero byte limits, zero history entries, and unreasonable maximums unless there is already a broader config validation convention for warnings.

## Phase 10: Tests and verification

Add unit tests for:

```text
- prompt classification: chat vs slash vs ! vs !! vs empty bang.
- shell output store head/tail truncation.
- shell output store total-byte eviction.
- UTF-8 lossy rendering does not panic on split codepoints.
- human shell policy allow/warn/block decisions.
- promotion mode selection and rendered attachment text.
- Rust failure digest extraction.
```

Add async integration tests for the runtime using portable commands:

```text
sh -lc 'printf hello'
sh -lc 'printf err >&2; exit 7'
sh -lc 'sleep 5' with a tiny timeout
```

Use `cargo test` and the repo’s documented check commands:

```bash
cargo fmt
cargo test --all-features
cargo clippy --all-features -- -D warnings
scripts/check-core-boundary.sh
```

If full `--all-features` is too heavy during development, at minimum run focused tests for `shell`, `tui`, and command parsing, then run the full suite before merging.

## Suggested implementation order

1. Add `src/shell/` types, output store, policy, digest stubs, and tests.
2. Add `ShellRuntime` streaming process execution and tests.
3. Add prompt classification and tests without wiring execution.
4. Add `App` shell state fields: output store, running task handles, and optional shell event receiver.
5. Add `TuiCommand::RunHumanShell` and event-loop handling.
6. Render shell cells or a shell panel with `not in model context` copy.
7. Add `/shell-list`, `/shell-include`, `/shell-rerun`, `/shell-kill`.
8. Add Rust failure digest and make promotion use digest/tail defaults.
9. Add `!!cmd` automatic promotion.
10. Add config schema and docs/help text.

## Acceptance criteria

`!cargo test` runs from the current project directory, streams output, and does not send command or output to the model.

The TUI remains responsive while a command is running.

A long-output command is bounded in memory and reports truncation.

`/shell-include last` attaches explicit shell output to the model-visible conversation.

`/shell-ask last why did this fail?` attaches a digest/tail and submits the user’s question.

`/shell-kill last` cancels a running command.

Dangerous human commands are blocked or confirmed according to the human-shell policy.

The agent-facing `bash` tool continues to behave as before.

No new dependency from `codegg-core` to TUI, tool, process execution, or ratatui is introduced.

Tests cover parser, store, policy, digest, runtime timeout, and promotion rendering.

## Future work

A later pass can add PTY support for interactive commands, persistent shell artifacts, side-panel search/filtering, clickable file spans from compiler output, shell-output-to-LSP repair workflows, model-generated deterministic command suggestions, and remote shell execution via core transport. These should be deferred until the non-interactive local path is stable.
