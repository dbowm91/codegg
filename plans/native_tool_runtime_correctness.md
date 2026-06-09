# Native Tool Runtime Correctness Plan

## Status

**Complete.** All nine phases below are checked off; see the per-phase
notes for what was actually done and where. The follow-up docs pass
(`plans/native_tool_runtime_correctness.md` Phase 9) has also been
delivered: `architecture/native_crates.md`, `architecture/tool.md`,
`architecture/lsp.md`, `architecture/security.md`,
`architecture/git.md`, `architecture/overview.md`,
`.opencode/skills/tool/SKILL.md`, and `AGENTS.md` all reflect the
behaviour described here.

## Purpose

Finish the hardening work for Codegg's library-first, MCP-second tool architecture.

The previous pass put most of the right pieces in place: config-to-runtime backend conversion, backend-aware registration for LSP/security, structured provenance methods on important tools, MCP raw-tool filtering, and `egggit` consumption from review/commit paths. This pass should focus narrowly on correctness: every runtime path must preserve backend config, diagnostics must describe actual behavior, and central tool execution must actually use the structured execution path.

Do not extract new tool crates in this pass.

## Current State Summary

The repo currently has:

```text
src/tool/backend.rs              backend/provenance/report types
src/tool/backend_config.rs       config schema -> runtime backend conversion
src/tool/disabled.rs             disabled placeholder tool
crates/egglsp                    LSP implementation crate
crates/egggit                    read-only git/worktree facts
crates/eggcontext                token estimation
crates/eggsentry                    deterministic security scanning
```

Good progress already completed:

- `ToolBackendConfig::from_config(&Config)` exists.
- `ToolRegistry::with_config(&Config)` exists.
- `ToolRegistry` stores resolved `tool_backends`.
- LSP/security registration consults backend config.
- `McpService::list_filtered_tools(policy)` is used during tool definition construction.
- `websearch`, `webfetch`, `lsp`, and `security` have or are intended to have structured provenance.
- `review` and `commit` now use `egggit::diff_text()` for read-only diff acquisition.

Remaining risks:

- Some registry construction paths may still drop `[tool_backends]` and use `ToolBackendConfig::default()`.
- `execute_capture()` exists but may not be used by the central agent tool-call execution path.
- MCP fallback diagnostics can say fallback-native even though MCP-unimplemented registration currently installs a disabled placeholder.
- Disabled backend behavior may still expose disabled tools to the model, increasing tool-surface noise.
- Root dependency ownership needs one final audit.

## Non-Goals

Do not add MCP server binaries for `egglsp`, `eggsentry`, `egggit`, or `eggcontext`.

Do not add new crates.

Do not rewrite the provider layer.

Do not change model-facing tool schemas unless required to fix incorrect disabled/MCP behavior.

Do not convert filesystem/edit/shell tools into external backends.

Do not expand `eggcontext` beyond token-estimation correctness in this pass.

## Phase 1: Find Every ToolRegistry Construction Path — done

Audited every `ToolRegistry::with_*` and `ToolRegistryOptions` call
site. Classified each:

- `with_defaults()` — static / test / non-config-aware callers; the
  all-native default is correct.
- `with_session_config_defaults(&Config, ...)` — added in Phase 2 and
  used by the agent loop and daemon session construction; preserves
  the resolved backend config.
- `with_session_defaults(...)` — kept for tests and non-config-aware
  callers; documented as a footgun.
- `with_options(ToolRegistryOptions { tool_backends, .. })` — single
  authoritative registration path; the new `ToolBackendConfig` is
  threaded through every caller that has a `Config` in hand.

No production path silently drops `[tool_backends]`.

## Phase 2: Replace or Fix `with_session_defaults()` — done

Added the config-aware session constructor at
`src/tool/mod.rs:477`:

```rust
pub fn with_session_config_defaults(
    config: &crate::config::schema::Config,
    todo_state: std::sync::Arc<tokio::sync::Mutex<crate::task_state::TodoState>>,
    policy: crate::model_profile::types::TaskStatePolicy,
    pool: Option<sqlx::SqlitePool>,
    session_id: Option<String>,
) -> Self
```

`with_session_defaults(...)` is kept (and now carries a doc comment
warning that it builds `ToolBackendConfig::default()` and should not
be used in config-aware runtime paths). Production session code in
the agent loop and daemon now calls
`with_session_config_defaults(&config, ...)`. New
`tests/tool_structured_execution.rs` covers disabled LSP/security in a
session registry.

## Phase 3: Decide Disabled Backend Semantics — done

Chose **Option A** (hidden disabled tools) for the model-facing
surface, with the `DisabledTool` stub retained in the registry for
diagnostics. The mechanism is the new `Tool::expose_in_definitions()`
predicate (default `true`, overridden to `false` by `DisabledTool` at
`src/tool/disabled.rs:78`). `ToolRegistry::definitions()` and
`AgentLoop::build_tool_definitions()` both filter through it, so
`[tool_backends.lsp|security].backend = "disabled"` keeps the model
tool surface clean while `/tool-backends` and tests can still
introspect the disabled reason.

## Phase 4: Reconcile MCP Fallback Semantics — done

Applied the matrix in `ToolRegistry::with_options` for both LSP and
security; `ToolRegistry::backend_report(...)` reports the matching
status. Specifically:

- `mcp + fallback_to_native = true` (default) → register the real
  native wrapper; live path is the native crate, not the MCP server;
  report `fallback-native`.
- `mcp + fallback_to_native = false` → register a hidden
  `DisabledTool` stub; model never sees the tool; report
  `ConfiguredButUnavailable` (`unavailable`) regardless of whether
  the MCP server is connected.
- `disabled` → register a hidden `DisabledTool` stub; report
  `disabled`.

A shared `classify_registered` helper inside `backend_report` uses
`Tool::expose_in_definitions()` to distinguish "real native wrapper"
from "disabled stub", so the report cannot claim `fallback-native`
unless the native wrapper is actually registered. The cases
`native`, `builtin`, `disabled`, `mcp + fallback true`, and
`mcp + fallback false` are each exercised in
`tests/tool_structured_execution.rs`.

## Phase 5: Wire `execute_capture()` Into Central Tool Execution — done

`AgentLoop::execute_tool_calls` at `src/agent/loop.rs:3249` now calls
`ToolRegistry::execute_capture(&tc.name, tc.arguments.clone(), exec_ctx)`
for native Codegg tool calls. Only `structured.output` is fed back to
the model; the model-facing string is unchanged. Provenance
(`backend`, `implementation`, `elapsed_ms`) is recorded via
`tracing::debug!` in `execute_capture` itself, with a fallback
`ToolProvenance::legacy(name)` for tools that do not override
`execute_structured()`. MCP tools continue to dispatch via
`McpService::call_tool` and are not funnelled through
`execute_capture` in this pass.

## Phase 6: Add a Minimal Structured Execution Smoke Test — done

New `tests/tool_structured_execution.rs` (265 lines) covers the
five required cases without network or external MCP servers:

1. `read` / `list` through `execute_capture()` return
   `ToolProvenance::legacy(...)` provenance.
2. `security` through `execute_capture()` returns
   `implementation = "eggsentry"`.
3. Disabled `security` is filtered from
   `ToolRegistry::definitions()` and `build_tool_definitions()`.
4. `mcp + fallback_to_native = true` registers the native
   `SecurityTool`; `backend_report(...)` reports `fallback-native`.
5. `mcp + fallback_to_native = false` exposes nothing; the report
   says `unavailable`/`ConfiguredButUnavailable`.

## Phase 7: Audit Actual Tool Definition Visibility — done

The model-facing catalog is now consistent: both
`ToolRegistry::definitions()` (`src/tool/mod.rs:455`) and
`AgentLoop::build_tool_definitions()` (`src/agent/loop.rs:1883`) use
the same `Tool::expose_in_definitions()` predicate, so disabled and
MCP-stub tools are filtered identically in both paths. `tool_search`
receives the filtered list because it goes through
`ToolRegistry::definitions()` to derive the available-tools set, so
hidden stubs are never advertised there either.

## Phase 8: Final Dependency Ownership Audit — done

Root-crate dependencies are now intentionally owned:

- `flate2`, `tar` — used in `src/plugin/install.rs` for plugin
  tarball extraction (also pulled in transitively by `egglsp`).
- `notify` — used in `src/config/watcher.rs` for config file
  watching (not used by `egglsp`).
- `lsp-types`, `lsp-server`, `zip`, `xz2` — owned exclusively by
  `egglsp`. The root crate has no direct dependency on them.

`cargo check --workspace --all-features` passes; no dead root
dependency remains.

## Phase 9: Update Architecture Notes and Plan Status — done

All listed architecture docs and this plan have been updated:

- `architecture/native_crates.md` — `with_session_config_defaults`,
  `expose_in_definitions`, MCP fallback matrix, central
  `execute_capture` path, and the new
  `tests/tool_structured_execution.rs` are all documented.
- `architecture/tool.md` — `ToolRegistry` constructor table, new
  `expose_in_definitions` section, central `execute_capture` path.
- `architecture/lsp.md` and `architecture/security.md` — MCP fallback
  matrix and `expose_in_definitions` note.
- `architecture/git.md` — confirmed `egggit` is the read-only source
  of git facts and native tool calls go through `execute_capture`.
- `architecture/overview.md` — `with_session_defaults` mention now
  references both constructors.
- `plans/native_tool_crates_hardening.md` — "Superseded by
  `native_tool_runtime_correctness.md`" note added at the top.
- `plans/native_tool_runtime_correctness.md` (this file) — converted
  to a checklist with each phase marked complete.
- `.opencode/skills/tool/SKILL.md` — `Tool` trait and `ToolRegistry`
  blocks updated to mention `expose_in_definitions` and the new
  constructors.
- `AGENTS.md` — `with_session_defaults` row updated and a brief
  mention added in the "Verified Codebase Facts" table.

## Validation Commands

Run before considering this pass complete:

```bash
cargo fmt --all --check
cargo check --workspace --all-features
cargo test --workspace
cargo clippy --workspace --all-features --all-targets
```

If clippy remains noisy, record the specific failures and do not hide compile/test failures behind clippy cleanup.

## Done Criteria

This pass is complete when:

- All real runtime/session registry constructors preserve `[tool_backends]` config.
- `with_session_defaults()` is no longer a production footgun.
- Disabled backend semantics are explicit and tested.
- MCP fallback behavior and diagnostics agree with actual registration.
- Native Codegg tool calls go through `execute_capture()` centrally.
- Structured provenance is available internally without changing model-visible tool output.
- Tool definitions and `tool_search` do not expose hidden disabled tools.
- Root dependencies reflect actual ownership after extraction.
- Tests cover registry visibility, backend fallback, and structured execution.
