# Human Shell Module

## Purpose

The `shell` module provides human-initiated shell command execution with
bounded output storage, safety policy enforcement, projection pipeline,
and a promotion model that keeps ephemeral commands out of model context.

## Where It Lives

`src/shell/` — 11 files:

| File | Contents |
|------|----------|
| `types.rs` | `ShellOrigin`, `ShellCapturePolicy`, `ShellCommandId`, `ShellRequest`, `ShellEvent`, `ShellStatus`, `ShellPromotionMode`, `ShellEnvPolicy`, `PromptSubmissionKind`, `classify_prompt_submission()` |
| `runtime.rs` | `ShellRuntime` (spawns via `$SHELL -lc`), `ShellHandle` (abort handle) |
| `store.rs` | `ShellOutputStore` (bounded VecDeque), `BoundedOutput` (head/tail split), `ShellOutputEntry` |
| `policy.rs` | `HumanShellPolicyDecision` (Allow/Warn/Block), `evaluate_command()` |
| `digest.rs` | `ShellDigest` (structured failure extraction), `ShellFailure`, `TruncationReport` |
| `projection.rs` | Phase 1: `CommandRun`, `CommandExit`, `CommandOutputStore`, `OutputHandle`, `RedactionState`, `RawStream`, `OutputCompleteness`, `ExpansionExactness`, `ExpansionRequest`, `CommandOutputExpansion` |
| `projection_bridge.rs` | `ShellCommandRunBridge` — sidecar mirror of `ShellEvent`s into `CommandOutputStore` |
| `projector.rs` | Phase 2+: `CommandOutputProjector` trait, `RawProjector`/`TruncatedProjector`/`ErrorRetentionProjector`, `ProjectionSelector`, native projectors (Phase 3), Phase 9 contract types, Phase 10 context metadata, `apply_redaction_hook`, `config_command_projection` |
| `rtk.rs` | Phase 5–6: `RtkDiscovery`, `RtkAvailability`, `RtkCapabilities`, `RtkProjector`, `classify_command()`, eligibility classification, wrapper grammar parsing |
| `redactor.rs` | Phase 8: `Redactor`, `RedactRule` trait, six built-in rules, `RedactedOutput` |
| `mod.rs` | Re-exports, `sanitize_ansi()` |

## How It Works

### Central Invariant

A human `!` command is not model context unless the user explicitly
promotes it. `!command` runs ephemerally (hidden from model).
`!!command` runs and auto-promotes output. `\!command` escapes to a
literal `!` chat message.

### Execution Flow

1. User input parsed by `classify_prompt_submission()` →
   `PromptSubmissionKind::HumanShell { command, promote_after }`.
2. `ShellRuntime::spawn()` launches `$SHELL -lc <command>` with
   `kill_on_drop(true)`, timeout (default 300s), and piped
   stdout/stderr. Returns `ShellHandle` with abort capability.
3. Shell events (`Started`, `Stdout`, `Stderr`, `Exited`, `TimedOut`,
   `FailedToStart`) stream over `mpsc::Sender<ShellEvent>`.
4. `ShellOutputStore` receives bounded head/tail for TUI rendering.
5. `ShellCommandRunBridge` mirrors events into `CommandOutputStore` for
   projection/expansion.
6. Policy gate (`evaluate_command()`) blocks destructive commands,
   warns on risky ones, before execution.

### Projection Pipeline (Phases 1–10)

`CommandOutputStore` retains raw stdout/stderr (32 MiB/stream, 64 MiB
total, 100 history entries). Streams exceeding caps are marked
`OutputCompleteness::Partial`.

`ProjectionSelector` is the single entry point for model-visible text.
Chain: `Raw → Native (GitStatus, GitDiff, GitLog, CargoCheck,
CargoTest) → RTK (if enabled) → ErrorRetention → Truncated`.

Each projector implements `CommandOutputProjector::supports()` returning
`Preferred`, `Supported`, `Fallback`, or `Unsupported`. The selector
picks the first non-`Unsupported`.

Redaction is applied inside `ProjectionSelector::project()` via
`apply_redaction_hook()` — six deterministic `RedactRule`
implementations: `AuthorizationRule`, `EnvSecretRule`, `PemBlockRule`,
`CloudCredentialRule`, `EmbeddedCredentialUrlRule`,
`SessionMaterialRule`. Cannot be bypassed by projectors.

Phase 9 adds `ProjectionId`, `ArtifactSpanRef`, `RedactionRecord`,
`RtkResultMetadata` on `ProjectionResult`, `ProjectionRecord` in
run_store, `evaluate_promotion()`, `preferred_projector_for_run_kind()`.

Phase 10 adds `ProjectionContextMetadata`, `ProjectionFact`,
`ModelTier` (Mini/Workhorse/Frontier), `ContextAwareBudget`, and
double-compression prevention.

### Syntax

| Syntax | Behavior |
|--------|----------|
| `!command` | Ephemeral shell execution |
| `!!command` | Auto-promoted shell execution |
| `\!command` | Literal `!command` chat message |
| `/shell-list` | List recent commands |
| `/shell-show <id>` | Full detail in scrollable dialog |
| `/shell-expand <id> stdout\|stderr [start..end]` | Expand raw output |
| `/shell-include <id> [stdout\|stderr\|all]` | Promote output to context |
| `/shell-ask <id>` | Ask agent about output |
| `/shell-rerun <id>` | Re-execute command |
| `/shell-kill <id>` | Abort running command |

## Key Types & APIs

### Core Types (`types.rs`)

```rust
pub enum ShellOrigin { HumanEphemeral, HumanPromoted, AgentTool }
pub enum ShellCapturePolicy { DisplayOnly, StoreEphemeral, StoreAndPromote }
pub enum ShellPromotionMode { Full, Tail { lines: usize }, StdoutOnly, StderrOnly, Summary, FailureDigest }
pub enum ShellEnvPolicy { Inherit, Clean }
pub struct ShellCommandId(pub u64);  // monotonic, allocated by store
pub enum ShellStatus { Running, Exited, TimedOut, FailedToStart, Killed }
pub enum PromptSubmissionKind { Chat(String), Slash(String), HumanShell { command: String, promote_after: bool } }
```

`ShellRequest` includes `id`, `origin`, `command`, `cwd`, `timeout`,
`capture_policy`, and `env_policy`.

`classify_prompt_submission()`: `!!cmd` → `promote_after=true`,
`!cmd` → `promote_after=false`, `\!cmd` → `Chat("!cmd")`, `/cmd` →
`Slash`, empty/`!`/`!!` → `Chat`.

### ShellRuntime (`runtime.rs:10`)

Spawns via `Command::new($SHELL).arg("-lc").arg(&command)`. Sends
events over `mpsc::Sender<ShellEvent>`. Timeout enforced via
`tokio::time::timeout` on `child.wait()`. Plugin service integration
via `with_plugin_service()` for shell env lifecycle hooks.

### ShellOutputStore (`store.rs:93`)

Bounded `VecDeque<ShellOutputEntry>`. Defaults: 100 entries, 1 MB/cmd
(head 256KB + tail 256KB), 8 MB total. Evicts oldest by count then
bytes. `ShellOutputEntry` includes `promoted: bool` and
`promote_after: bool` (set from `capture_policy`).

### CommandOutputStore (`projection.rs:391`)

Raw byte store. 32 MiB/stream cap, 64 MiB total, 100 entries. LRU
eviction. Handles: `cmd://<id>/stdout`, `cmd://<id>/stderr`. Supports
`expand()`, `expand_stream()`, `parse_handle()`,
`parse_handle_with_range()`.

### ProjectionSelector (`projector.rs:2996`)

`with_defaults()` — conservative chain without RTK.
`with_rtk(config)` — adds RTK projector.
`with_config(config)` — builds from `ShellOutputConfig`.
`project(request, store)` — selects projector, applies redaction hook,
returns `ProjectionResult`.

### Redactor (`redactor.rs:358`)

Six rules: `AuthorizationRule` (bearer/basic/api-key),
`EnvSecretRule` (UPPERCASE var assignments with sensitive keywords),
`PemBlockRule` (private key blocks), `CloudCredentialRule` (AWS/GCP/Azure),
`EmbeddedCredentialUrlRule` (user:pass@host URLs),
`SessionMaterialRule` (cookies, session IDs, CSRF tokens).
Replacement markers: `[REDACTED:<rule-class>]`.

### RTK (`rtk.rs`)

`RtkDiscovery` lazy-probes on first use. `RtkCapabilities` determines
invocation mode: `PostProcess` (stdin pipe, 1 MiB cap) or `Wrapper`
(`rtk <cmd>`). `classify_command()` returns `CompressionEligibility`.
Wrapper grammar rejects shell metacharacters, quotes, env assignments.
`RtkProjector::MAX_STDERR_WARNING_BYTES = 512`.

## Configuration Surface

### Human Shell Config

```toml
[human_shell]
enabled = true                # default: true
default_timeout_secs = 300    # default: 300
max_history_entries = 100     # default: 100
max_bytes_per_command = 1000000   # default: 1MB
max_total_bytes = 8000000     # default: 8MB
ansi = "stripped"             # raw | stripped | sgr_only
confirm_dangerous = true      # default: true
auto_promote_bangbang = true  # default: true
```

### Shell Output Config (`[shell.output]`)

| Field | Values | Default |
|-------|--------|---------|
| `projection` | `off`, `safe`, `rtk`, `aggressive` | `safe` |
| `retain_raw` | bool | `true` |
| `redact_model_visible_output` | `off`, `model_only`, `all` | `model_only` |
| `max_model_output_tokens` | int | `4000` |
| `max_tui_output_bytes` | int | `200000` |
| `show_projection_metadata` | bool | `true` |
| `prefer_native_projectors` | bool | `true` |

RTK sub-config (`[shell.output.rtk]`): `enabled` (default false),
`path`, `eligible_only` (default true), `timeout_ms` (default 5000),
`allow_side_effecting_commands`.

## Invariants & Gotchas

- **Killed status not overwritten**: `mark_killed()` sets `Killed`.
  Late `Exited` events from runtime do NOT overwrite `Killed` — the
  TUI handler checks status before calling `mark_exited()`.
- **Redaction is single-pass**: `apply_redaction_hook()` is called
  inside `ProjectionSelector::project()`. `config_command_projection()`
  does NOT apply a second pass (prevents overwriting replacement counts).
- **RTK env-gated tests**: `CODEGG_RTK_INTEGRATION=1` required.
  Standard CI runs without RTK.
- **AnsiMode**: `sanitize_ansi()` in `mod.rs` handles `Raw`, `Strip`,
  and `SgrOnly` modes. `SgrOnly` preserves color sequences but
  removes cursor/erase sequences.
- **ShellOutputStore late-exit caveat**: The store API itself allows
  `mark_exited()` to overwrite `Killed`, but the TUI handler guards
  against this.

## Testing

```bash
cargo test -p codegg --lib shell::types          # classify_prompt_submission, promotion modes
cargo test -p codegg --lib shell::runtime        # spawn, timeout, stderr
cargo test -p codegg --lib shell::store          # bounded output, eviction
cargo test -p codegg --lib shell::policy         # block/warn patterns
cargo test -p codegg --lib shell::digest         # failure extraction
cargo test -p codegg --lib shell::redactor       # six rules, false positives
cargo test -p codegg --lib shell::rtk            # discovery, eligibility, wrapper grammar
cargo test -p codegg --lib shell::projector      # selector, native projectors, redaction hook
cargo test --test shell_projection_harness       # 11 invariant tests over fixture corpus
cargo test --test shell_projection_phase10       # 33 context budget tests
CODEGG_RTK_INTEGRATION=1 cargo test -p codegg --lib shell::rtk -- rtk_integration  # env-gated
```

## Related Docs

- [tool.md](tool.md) — Agent bash tool (`ShellOrigin::AgentTool`)
- [human-shell/SKILL.md](../.opencode/skills/human-shell/SKILL.md)
- [shell_output_projection_rtk_roadmap.md](../plans/shell_output_projection_rtk_roadmap.md)

## Archived Phase Status

| Phase | Status | Summary |
|-------|--------|---------|
| 1 | Landed | `CommandOutputStore`, `ShellCommandRunBridge`, stable handles |
| 2 | Landed | `CommandOutputProjector` trait, `Raw`/`Truncated`/`ErrorRetention` projectors, `ProjectionSelector` |
| 3 | Landed | Native projectors: `GitStatus`, `GitDiff`, `GitLog`, `CargoCheck`, `CargoTest` |
| 4 | Partial | Config schema + `with_config()`; per-command rules deferred |
| 5 | Landed | RTK discovery, eligibility, capabilities |
| 6 | Landed | Real RTK invocation: `PostProcess`/`Wrapper` modes |
| 7 | Landed | Expansion API, `/shell-expand`, TUI detail panel |
| 8 | Landed | Redaction pipeline with six `RedactRule` implementations |
| 9 | Landed | `ProjectionId`, `ArtifactSpanRef`, `RedactionRecord`, promotion policy |
| 10 | Landed | `ProjectionContextMetadata`, `ModelTier`, `ContextAwareBudget` |
