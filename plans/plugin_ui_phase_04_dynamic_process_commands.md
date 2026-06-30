# Phase 4 Plan: Extend Dynamic Slash Commands into Process-Backed Command Plugins

## Objective

Extend Codegg’s existing dynamic slash command system so local commands can optionally execute a process/script and return either plain stdout or a structured `PluginResponse` JSON payload.

This phase delivers the first useful plugin path: a developer can add a project-local `/quota`-style command that runs Python, shell, or another executable without recompiling Codegg.

## Existing Foundation

Codegg already loads dynamic command definitions from:

- config-defined commands;
- project-local `command/` and `commands/` markdown files;
- YAML frontmatter + body templates.

The current dynamic commands are template-oriented. This phase preserves that behavior and adds a runtime-backed option when a command declares `runtime: process`.

## User-Facing Command Format

A simple stdout command should be possible:

```markdown
---
description: Show quota
runtime: process
command: python3
args: ["scripts/quota.py"]
stdout: text
timeout_ms: 5000
---
```

A structured command should also be possible:

```markdown
---
description: Show quota as a dialog
runtime: process
command: python3
args: ["scripts/quota.py", "--json"]
stdin: json
stdout: json
timeout_ms: 5000
output: ["chat", "dialog"]
---
```

If `runtime` is absent, existing template behavior must remain unchanged.

## Schema Changes

### `crates/codegg-config` or relevant config schema

Extend the command config/frontmatter schema with optional fields. Use the existing schema location for `CommandConfig`.

Recommended optional fields:

```rust
pub runtime: Option<CommandRuntimeKind>,
pub command: Option<String>,
pub args: Option<Vec<String>>,
pub stdin: Option<CommandStdinMode>,
pub stdout: Option<CommandStdoutMode>,
pub timeout_ms: Option<u64>,
pub cwd: Option<String>,
pub env: Option<Vec<String>>,
pub output: Option<Vec<String>>,
```

Recommended enums:

```rust
#[serde(rename_all = "snake_case")]
pub enum CommandRuntimeKind {
    Template,
    Process,
}

#[serde(rename_all = "snake_case")]
pub enum CommandStdinMode {
    None,
    Json,
}

#[serde(rename_all = "snake_case")]
pub enum CommandStdoutMode {
    Text,
    Json,
    Auto,
}
```

Keep all fields optional to avoid breaking existing configs.

### `src/command/mod.rs`

Extend the internal `Command` struct or add a parallel `CommandRuntime` field.

Recommended shape:

```rust
pub enum CommandExecution {
    Template { template: String },
    Process(ProcessCommandSpec),
}

pub struct ProcessCommandSpec {
    pub command: String,
    pub args: Vec<String>,
    pub stdin: CommandStdinMode,
    pub stdout: CommandStdoutMode,
    pub timeout_ms: u64,
    pub cwd: Option<String>,
    pub env: Vec<String>,
    pub output: Vec<String>,
}
```

If changing the existing `Command` struct is too invasive, keep `template` for compatibility and add optional `process: Option<ProcessCommandSpec>`.

## Invocation Envelope

When running a process command with `stdin: json`, pass a `PluginInvocation` from `codegg_protocol::plugin` to stdin.

Minimum context:

- invocation id;
- plugin id or command source id;
- command name;
- args;
- session id if available;
- project dir;
- selected model;
- selected agent;
- frontend capability list if known.

For `stdin: none`, do not write JSON to stdin.

## Runtime Execution

This phase can implement a minimal process execution helper directly under command/TUI code, but it should be shaped so Phase 6 can extract it into `ProcessRuntime`.

Preferred location for now:

- `src/plugin/runtime/process.rs` if starting the runtime abstraction early is not too disruptive; or
- `src/tui/commands/plugins.rs` helper if keeping this phase smaller.

Recommended execution constraints:

- timeout default: 5 seconds;
- stdout cap: 1 MiB initially;
- stderr cap: 256 KiB initially;
- cwd default: project dir/current dir;
- env default: minimal/inherited policy as existing shell behavior allows;
- command and args are not shell-expanded unless explicitly using `sh -c`.

Do not execute via shell by default.

## Output Handling

### `stdout: text`

Treat stdout as plain text. Apply through Phase 3 stdout fallback.

### `stdout: json`

Require stdout to parse as `PluginResponse`. Invalid JSON should return an error completion with diagnostics.

### `stdout: auto`

Try parsing as `PluginResponse`. If parsing fails, treat stdout as text.

### stderr

- On success: include stderr only as diagnostics/debug information.
- On failure: show stderr in the error info dialog or toast summary.

## TUI Integration

### `src/tui/command.rs`

When appending dynamic commands, include process-backed metadata. The command palette should not need to know the runtime, but the command execution path must be able to recover it from the registry.

### Command execution path

Locate the slash command execution branch that currently handles static commands, template dynamic commands, and dialog commands. Add logic:

1. Resolve command by name.
2. If it is a template command, keep existing behavior.
3. If it is a process command, send `TuiCommand::PluginCommandRun { command, args }` or call the Phase 3 start helper with the resolved process spec.

If the current `TuiCommand::PluginCommandRun` only carries command name/args, add a registry lookup from the handler. Avoid putting full process specs into prompt/completion display structs unless necessary.

## Tests

Add command parser tests:

- existing template command still parses as template;
- process command frontmatter parses correctly;
- process command defaults timeout/stdin/stdout correctly;
- invalid process command without `command` is rejected with useful error;
- command name validation remains unchanged.

Add execution tests:

- stdout text command returns text fallback;
- stdout JSON command returns `PluginResponse`;
- auto mode falls back to text on invalid JSON;
- explicit JSON mode errors on invalid JSON;
- timeout produces error completion;
- nonzero exit produces error completion with stderr;
- stdout cap is enforced.

Use small platform-portable commands where possible. For unit tests, a Rust test helper binary or `std::env::current_exe` subcommand pattern is more portable than relying on shell utilities.

## Acceptance Criteria

- Existing dynamic template commands continue to work unchanged.
- A process-backed local command can be discovered and shown in completions.
- A process-backed command can be executed from a slash command.
- Plain stdout renders usefully.
- Structured `PluginResponse` JSON can open a dialog/toast/chat effect through Phase 3 plumbing.
- Timeout, nonzero exit, invalid JSON, and oversized output are handled safely.
- No WASM work is introduced in this phase.

## Security Notes

Process-backed command plugins are local executable code. They should be treated similarly to manually running a local script. This phase should not describe them as sandboxed.

Minimal safety controls are still required:

- no shell execution unless explicitly configured;
- timeout;
- output caps;
- cwd control;
- explicit env allowlist or conservative inherited-env policy;
- clear diagnostics for command path/source.

## Non-Goals

- Do not add plugin install/uninstall commands.
- Do not implement WASM runtime invocation.
- Do not add lifecycle hooks.
- Do not add PyO3.
- Do not implement OS-level sandboxing.

## Handoff Notes for Phase 5

Phase 5 should move from project-local dynamic commands to installed plugin manifests and a unified plugin registry. The process command spec built here should map naturally into `runtime = process` + `capability = command`.
