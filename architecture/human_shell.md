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
| `runtime.rs` | `ShellRuntime` (streams `$SHELL -lc` through `ManagedProcessService`), `ShellHandle` (cancellation handle) |
| `daemon.rs` | `HumanShellRunService` (scheduler-admitted, bounded daemon run registry and owner-scoped output cursor) |
| `store.rs` | `ShellOutputStore` (bounded VecDeque), `BoundedOutput` (head/tail split), `ShellOutputEntry` |
| `policy.rs` | `HumanShellPolicyDecision` (Allow/Warn/Block), `evaluate_command()` |
| `digest.rs` | `ShellDigest` (structured failure extraction), `ShellFailure`, `TruncationReport` |
| `projection.rs` | Phase 1: `CommandRun`, `CommandExit`, `CommandOutputStore`, `OutputHandle`, `RedactionState`, `RawStream`, `OutputCompleteness`, `ExpansionExactness`, `ExpansionRequest`, `CommandOutputExpansion` |
| `projection_bridge.rs` | `ShellCommandRunBridge` — sidecar mirror of `ShellEvent`s into `CommandOutputStore` |
| `projector.rs` | Phase 2+: `CommandOutputProjector` trait, `RawProjector`/`TruncatedProjector`/`ErrorRetentionProjector`, `ProjectionSelector`, native projectors (Phase 3), Phase 9 contract types, Phase 10 context metadata, `apply_redaction_hook`, `config_command_projection` |
| `rtk.rs` | Phase 5–6: `RtkDiscovery`, `RtkAvailability`, `RtkCapabilities`, `RtkProjector`, `classify_command()`, eligibility classification, wrapper grammar parsing |
| `redactor.rs` | Phase 8: `Redactor`, `RedactRule` trait, seven built-in rules, `RedactedOutput` |
| `mod.rs` | Re-exports, `sanitize_ansi()` |

## How It Works

### Central Invariant

A human `!` command is not model context unless the user explicitly
promotes it. `!command` runs ephemerally (command and output hidden from model).
`!!command` runs and asks the authoritative daemon to stage a bounded,
redacted projection for the exact session when
`human_shell.auto_promote_bangbang` is enabled. `/shell-include` stages a
selected output view in the same way. The daemon acknowledges the durable
record before the TUI presents it as staged. Pending records are principal,
session, project, and workspace scoped, limited to 32 KiB each, 32 pending and
256 retained per session, and expire after 24 hours. A submitted turn claims
them atomically with the session controller lease and a durable admission
receipt (SQLite migrations v71–v72). The request envelope's `request_id` is
the stable submission identity; retry the identical request with the same ID
to receive the original receipt without starting another turn. Receipt rows
are retained for a bounded idempotency window. Daemon recovery marks
still-accepted receipts interrupted and never replays them; turn
completion/failure updates the receipt. Reconnect restores pending records,
while consumed records are never replayed automatically. `/shell-ask` stages
the question and evidence under a stable identity before submitting one
session-scoped prompt turn.
Promotions require a bound non-observer session.
Neither staging command starts inference. `\!command` escapes to a literal
`!` chat message.

Human-shell promotion always runs the shared secret redactor and fails closed
if redaction does not report an applied state, regardless of
`redact_model_visible_output`; the general shell projection `off` setting does
not disable this human-consent boundary. Projection text is ANSI-stripped and
limited to 32 KiB before staging. Without a connected authoritative owner, the
TUI leaves the evidence as an unsubmitted composer draft. The WebSocket TUI
transport accepts prompt strings only, so it also uses an explicitly labeled
composer draft and makes no staging or acceptance claim from queue delivery.

### Execution Flow

1. User input parsed by `classify_prompt_submission()` →
   `PromptSubmissionKind::HumanShell { command, promote_after }`.
2. The TUI negotiates `human_shell.v1` and submits create/read/cancel requests
   through `CoreClient`. It supplies project/workspace/session locators and
   an idempotency key, never a cwd or environment.
3. The daemon requires `command.execute`, resolves the registered workspace
   and project/session bindings, refuses remote-node bindings, enforces the
   daemon user's sandbox profile and safety classifier, and reserves a shared
   scheduler process slot before accepting the run.
4. `ShellRuntime::spawn()` submits daemon-host `$SHELL -lc <command>` to
   `ManagedProcessService::run_streaming()` with a sanitized daemon environment,
   canonical workspace cwd, 300-second timeout, and bounded stdout/stderr.
   Constrained sandbox profiles fail closed if the host cannot enforce them.
5. The daemon retains at most eight runs per principal, 512 total runs, and
   bounded output/status for ten minutes. A connection drop does not orphan an
   unbounded process; reconnect reads require the same principal, project,
   workspace, and session binding. Restart drops handles and never replays.
6. Shell events (`Started`, `Stdout`, `Stderr`, `Exited`, `TimedOut`,
   `FailedToStart`) stream over `mpsc::Sender<ShellEvent>`.
7. `ShellOutputStore` receives bounded head/tail for TUI rendering only after
   daemon admission.
8. `ShellCommandRunBridge` mirrors events into `CommandOutputStore` for
   projection/expansion.
9. `evaluate_command()` is enforced by both the TUI confirmation flow and the
   daemon boundary; a warning-class command needs explicit confirmation.

Human model-context promotions pass through one redaction hook regardless of
the general shell-output redaction setting and are capped at 32 KiB. `!`
output remains private. Local `!!`/include entries are marked promoted only
after a turn is accepted and the durable stage is claimed. `/shell-ask` waits
for both stage acknowledgement and session turn acceptance. Remote TUI queue
acceptance is not session acceptance, so the remote path remains an
unsubmitted composer draft. Finite command execution still runs in the
frontend and is being corrected separately by C002 at
`plans/implementation/human-execution-corrective/002-workspace-owned-human-shell-dispatch.md`.

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
`apply_redaction_hook()` — seven deterministic `RedactRule`
implementations: `AuthorizationRule`, `EnvSecretRule`, `PemBlockRule`,
`CloudCredentialRule`, `ProviderTokenRule`, `EmbeddedCredentialUrlRule`,
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

### ShellRuntime (`runtime.rs:9`)

Builds a typed `$SHELL -lc` request and sends events over
`mpsc::Sender<ShellEvent>`. Generic timeout, cancellation, process-group,
bounded-output, and reap behavior belongs to `ManagedProcessService`.
Plugin service integration remains available via `with_plugin_service()` for
shell environment lifecycle hooks. See
`architecture/process-tool-execution-ownership.md`.

### ShellOutputStore (`store.rs:96`)

Bounded `VecDeque<ShellOutputEntry>`. Defaults: 100 entries, 1 MB per-command
byte budget (`max_bytes_per_command`), 8 MB total (`max_total_bytes`).
Retained output per stream is bounded separately by `BoundedOutput`:
`HEAD_CAP` 256 KiB + `TAIL_CAP` 256 KiB = **512 KiB retained** per stream,
with `omitted_bytes` tracking what the tail dropped (the overflow is drained
from the *front* of the tail, `store.rs:51-54`). Evicts oldest by count
first, then by total bytes, both `pop_front` — but the byte loop stops at one
remaining entry (`store.rs:253-272`), so a single oversized command is never
evicted. `ShellOutputEntry` includes `promoted: bool` and
`promote_after: bool` (set from `capture_policy`).

### CommandOutputStore (`projection.rs:391`)

Raw byte store. 32 MiB/stream cap, 64 MiB total, 100 entries. LRU
eviction. Handles: `cmd://<id>/stdout`, `cmd://<id>/stderr`. Supports
`expand()`, `expand_stream()`, `parse_handle()`,
`parse_handle_with_range()`.

### ProjectionSelector (`projector.rs:2991`)

`with_defaults()` — conservative chain without RTK.
`with_rtk(config)` — adds RTK projector.
`with_config(config)` — builds from `ShellOutputConfig`.
`project(request, store)` — selects projector, applies redaction hook,
returns `ProjectionResult`.

### Redactor (`redactor.rs:386`)

Seven rules: `AuthorizationRule` (bearer/basic/api-key),
`EnvSecretRule` (UPPERCASE var assignments with sensitive keywords),
`PemBlockRule` (private key blocks), `CloudCredentialRule` (AWS/GCP/Azure),
`ProviderTokenRule` (provider-issued token shapes),
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
cargo test -p codegg --lib shell::redactor       # seven rules, false positives
cargo test -p codegg --lib shell::rtk            # discovery, eligibility, wrapper grammar
cargo test -p codegg --lib shell::projector      # selector, native projectors, redaction hook
cargo test --test shell_projection_harness       # 11 invariant tests over fixture corpus
cargo test --test shell_projection_phase10       # 33 context budget tests
CODEGG_RTK_INTEGRATION=1 cargo test -p codegg --lib shell::rtk -- rtk_integration  # env-gated
```

## Related Docs

- [tool.md](tool.md) — Agent bash tool (entirely separate: it never
  references `crate::shell` types; `ShellOrigin::AgentTool` is declared but
  never constructed)
- [human-shell/SKILL.md](../.opencode/skills/human-shell/SKILL.md)
- [shell_output_projection_rtk_roadmap.md](../plans/shell_output_projection_rtk_roadmap.md)

## Source verification

Verified 2026-10-06 against `src/shell/` (11 files, as tabulated):
`ShellRuntime` (`runtime.rs:9`), `ShellOutputStore` (`store.rs:96`),
`CommandOutputStore` (`projection.rs:391`), `ProjectionSelector`
(`projector.rs:2991`), `Redactor` (`redactor.rs:386`); the seven
`RedactRule` impls (`redactor.rs:56-253`, including `ProviderTokenRule`
at 205); `HEAD_CAP`/`TAIL_CAP` = 256 KiB each (`store.rs:10-11`);
`COMMAND_OUTPUT_MAX_RETAINED_BYTES` 64 MiB, `..._SINGLE_STREAM_BYTES`
32 MiB, `..._HISTORY_ENTRIES` 100 (`projection.rs:369-375`);
`RtkProjector::MAX_STDERR_WARNING_BYTES = 512` (`rtk.rs:610`); and
`HumanShellConfig` defaults 100 / 1,000,000 / 8,000,000 / 300s
(`crates/codegg-config/src/schema.rs:2955-2996`). The prior "1 MB/cmd
(head 256KB + tail 256KB)" wording was arithmetically wrong — head plus
tail is 512 KiB, distinct from the 1 MB per-command eviction budget.

Verified 2026-10-06 against source after upstream `2573f9c0` ("Decision runtime
ownership migration and M006 closure"), which inserted `DecisionEngineConfig`
near the top of `crates/codegg-config/src/schema.rs` and shifted later
declarations. Corrected: the `HumanShellConfig` range `2914-2955`
->`2955-2996` in the working tree; both endpoints were re-checked to land on the
struct declaration and the same trailing `auto_promote_bangbang` default, so the
range still spans exactly the same members. No `src/shell/` reference in this
document was affected by that commit.

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
| 8 | Landed | Redaction pipeline with seven `RedactRule` implementations |
| 9 | Landed | `ProjectionId`, `ArtifactSpanRef`, `RedactionRecord`, promotion policy |
| 10 | Landed | `ProjectionContextMetadata`, `ModelTier`, `ContextAwareBudget` |

## Source verification

Re-verified 2026-10-10 against `src/tui/app/mod.rs`,
`src/tui/commands/shell.rs`, `src/tui/app/prompt_turn.rs`,
`src/tui/runtime/event_loop.rs`, and `crates/codegg-config/src/schema.rs`:
shell cells are omitted from provider context; explicit shell evidence is
ANSI-stripped, redacted, bounded, and included only after user promotion;
submission acknowledgement controls the promoted marker; config enablement,
timeout, ANSI, and automatic `!!` behavior are read at startup and reload.
The WebSocket adapter has no staged-context request, so evidence is placed in
an empty composer for explicit remote submission and queue delivery is not
treated as acknowledgement.
