# Shell Output Projection Cleanup: Phase 1-5 Corrective Pass

## Objective

Clean up the first implementation tranche of the shell-output projection roadmap before continuing into real RTK invocation, expansion UX, redaction, and evaluation work. The current repo appears to have landed Phase 1, Phase 2, Phase 3, and Phase 5. Phase 4 appears partially represented through config and TUI files, but docs and implementation status are inconsistent. This corrective pass should reconcile the implementation, docs, config semantics, tests, and user-facing affordances so the system has a stable baseline.

The primary risk to eliminate is ambiguity: codegg must not imply RTK is actively compressing output when the current `RtkProjector` is only a skeleton, and docs must not simultaneously say Phase 4 is both pending and implemented.

## Current State Summary

The repository now contains:

- `src/shell/projection.rs` for command-event/raw-output retention.
- `src/shell/projection_bridge.rs` for mirroring `ShellEvent` streams into `CommandOutputStore`.
- `src/shell/projector.rs` for the projector trait, generic projectors, native Git/Rust projectors, selector, budgets, metadata, and redaction hook.
- `src/shell/rtk.rs` for RTK discovery, capability state, eligibility classification, and the RTK projector skeleton.
- Config and UI changes in `crates/codegg-config/src/schema.rs`, `crates/codegg-protocol/src/ui.rs`, `src/tui/components/messages.rs`, and `src/tui/commands/shell.rs`.
- Docs updates in `AGENTS.md`, `.codegg/skills/human_shell/SKILL.md`, `README.md`, and `architecture/human_shell.md`.

The implementation is broadly aligned with the roadmap, but needs a tightening pass before more features are layered on.

## Corrective Work Items

### 1. Reconcile Phase 4 status

Audit all references to Phase 4 in docs and skills:

- `architecture/human_shell.md`
- `AGENTS.md`
- `.codegg/skills/human_shell/SKILL.md`
- `README.md`
- the shell-output projection plan files

Decide whether Phase 4 is:

1. Fully landed,
2. Partially landed, or
3. Still pending.

Then make the docs consistent. If config types and TUI metadata exist but are incomplete, label this as “Phase 4 partial: config schema and metadata display present; escape hatches/per-command rules deferred.” Avoid leaving old “Phase 4 not implemented” notes next to code that already exposes `ShellOutputConfig` and `ProjectionSelector::with_config()`.

### 2. Verify config semantics against the plan

Inspect `crates/codegg-config/src/schema.rs` and any config loading/resolution code. Confirm the config can express at least:

```toml
[shell.output]
projection = "safe" # off | safe | rtk | aggressive
retain_raw = true
redact_model_visible_output = true
max_model_output_tokens = 4000
show_projection_metadata = true
prefer_native_projectors = true

[shell.output.rtk]
enabled = false
path = "rtk"
eligible_only = true
timeout_ms = 5000
allow_side_effecting_commands = false
```

If the implementation uses different names, document the actual names and make sure the example config matches reality. Add validation tests for unknown policies, zero budgets, invalid RTK timeout, disabled RTK with `projection = "rtk"`, and `retain_raw = false` with lossy projection.

### 3. Make RTK skeleton behavior impossible to misread

The current Phase 5 plan intentionally adds an RTK projector skeleton. Ensure that all user-facing and model-facing surfaces use language such as:

- “RTK available” only after discovery succeeds.
- “RTK backend skeleton” or “RTK dry-run placeholder” if the projector is selected but does not invoke RTK.
- “External/lossy placeholder; no RTK compression performed” if placeholder output is ever visible.

Prefer making placeholder selection impossible in normal runtime. A safe rule is:

- `RtkProjector::supports()` may advertise capability for tests and future wiring.
- `RtkProjector::project()` should return a recoverable `BackendUnavailable` or `NotImplemented` error unless an explicit internal/test flag permits placeholder output.
- The selector should fall back to safe projection and record a warning.

The important point is that `projection = "rtk"` must not produce a fake compressed result.

### 4. Ensure selector ordering is documented and tested

Verify the selector ordering for default, config-driven, and RTK-enabled paths. Expected conservative behavior:

- Exact requested output uses raw projection.
- Native projectors win for supported Git/Rust commands when `prefer_native_projectors = true`.
- RTK is never used under `safe` unless a command rule explicitly asks for it.
- RTK is attempted only when enabled, available, eligible, and non-placeholder invocation is implemented.
- Unknown failed commands use error-retention.
- Unknown long successful commands use truncation.

Add tests for these matrix cases:

| Policy | Command | Expected projector |
|--------|---------|--------------------|
| safe | `cargo test` failure | `native-cargo-test` |
| safe | `git diff` | `native-git-diff` |
| safe | unknown failed command | `error-retention` |
| safe | unknown long success | `truncated` |
| off | small output | `raw` |
| rtk, unavailable | eligible read-only command | safe fallback |
| rtk, skeleton only | eligible read-only command | safe fallback with warning |
| aggressive | long success | compact/truncated with smaller budget |

### 5. Verify raw retention remains authoritative

Add focused tests ensuring every projection result produced by generic, native, and RTK-skeleton paths includes expansion handles when raw output is retained. If `retain_raw = false` is supported, ensure lossy or parsed projections display a strong warning and never claim recoverability.

Also verify that output partiality propagates into projection exactness. A raw projection over a partial raw artifact should not report `Exact`; it should report `PartialRawArtifact` or equivalent.

### 6. Tighten redaction-hook status

The current redaction hook is intentionally a placeholder. Clean up any misleading naming where `RedactionState::Applied` could imply secrets were actually removed. Prefer separating:

- `RedactionState::NotApplied`
- `RedactionState::HookAppliedNoRules`
- `RedactionState::Applied`
- `RedactionState::Skipped`

If changing the enum is too invasive, add docs and tests showing the current hook is a placeholder and must not be considered a real secret filter. This prevents future RTK work from assuming redaction is complete.

### 7. Documentation cleanup

Update `architecture/human_shell.md` with a single canonical “Current Projection Pipeline Status” section:

- Phase 1: landed
- Phase 2: landed
- Phase 3: landed
- Phase 4: landed/partial/pending, with exact state
- Phase 5: skeleton landed, does not perform RTK compression yet
- Phase 6+: pending

Update `.codegg/skills/human_shell/SKILL.md` to match the exact current state. Skills are operational guidance; stale status here will mislead future agents.

### 8. Validation pass

Run and record:

```bash
cargo fmt --check
cargo clippy --all-features --all-targets -- -D warnings
cargo test --all-features
scripts/check-core-boundary.sh
```

If full all-features testing is too slow in the agent environment, record the exact subset run and why. Do not claim full validation unless it actually ran.

## Success Criteria

- Docs and skills have a single consistent status for Phases 1-5.
- Phase 4 config/TUI state is explicitly categorized as landed, partial, or pending.
- RTK skeleton cannot produce misleading fake compressed output in normal runtime.
- Config semantics are tested and documented.
- Selector behavior is covered for safe/off/rtk/aggressive policy paths.
- Raw handles and partial-output exactness are verified across projector classes.
- Redaction-hook placeholder status is explicit and cannot be mistaken for real secret filtering.
- `cargo fmt`, clippy, tests, and core-boundary validation are run or explicitly scoped.

## Non-Goals

- Do not implement real RTK invocation in this cleanup pass.
- Do not build the TUI raw-output expansion panel yet.
- Do not implement the full redaction ruleset yet.
- Do not expand native projectors beyond Git/Rust here.
- Do not redesign the command event model unless a correctness bug is found.

## Handoff Notes

This pass should be completed before Phase 6. Phase 6 will rely on the distinction between RTK skeleton, RTK available, and RTK actively invoked. If that distinction is not clear in code and docs, real RTK integration will be harder to validate safely.
