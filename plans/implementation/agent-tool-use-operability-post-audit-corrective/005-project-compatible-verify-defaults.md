# Agent Tool-Use Operability Corrective C005 — Make Verify Defaults Project-Compatible and Scoped

Status: closed — `plans/closure/agent-tool-use-operability-post-audit-corrective/005-status.md`.
Repository baseline: `4c86bac2a608dd717be085247838fd2f37d81ab6` (main, 2026-10-19)
Source roadmap: `plans/subsystems/agent-tool-use-operability-post-audit-corrective-addendum.md#7-milestones`
Long-term requirements: `plans/000-long-term-specification.md#26-reliability-and-recovery`; `#28-observability`; `#29-system-invariants`; `plans/001-terminology-and-domain-model.md`.
Applicable ADRs: ADR-0001 (programmatic authority), ADR-0004 (approval routing), ADR-0028 (execution authority), only insofar as touched. No ownership-changing decision is authorized.
Primary class: capability

## 1. Objective
Provide safe, useful routine Rust lint/check defaults and optional bounded package/feature scope without routing model input through arbitrary shell execution.

## 2. Why this milestone is ready
No hard dependencies; existing tool `src/tool/verify.rs`, accepted managed process path, and AGENTS project verification rules are available. No new execution privilege needed.

## 3. Current implementation evidence
- `src/tool/verify.rs::ResolvedVerification::resolve` constructs `cargo clippy --offline --all-targets --all-features -- -D warnings` and supports workspace-only scope. `AGENTS.md` says never --all-features for workspace sweeps; `plans/registry.md`'s generic normal broad posture currently shows all features, so policy docs disagree. Report's failed tool invocation involved optional-dependency rust-version mismatch; do not infer that removing --all-features makes every platform pass.

## 4. Invariants that must not regress
- `verify` remains bounded, offline by default, structured and non-mutating; no arbitrary command/shell args or unsanitized package flags; no silent replacement of `AGENTS.md` or `scripts/verify.sh` authority. An unsupported project returns specific error rather than guessing a compiler invocation.

## 5. Scope
In: the bounded tool contract and owning files described here, focused negative/positive regression tests, documentation and explicit compatibility corrections.
Out: new schedulers or authorization engines, Python sandbox expansion, new public network clients, multi-provider live CI, new release/packaging work, unrelated UI refactors, full repository feature sweeps and SBC compile/startup benchmarks.

## 6. Required production changes
Remove unconditional `--all-features` from lint's default. Resolve project-compatible defaults from fixed conservative Rust command or explicitly provided safe scope; add an allowlisted package target and explicit feature selection as bounded typed fields if needed. Decide default `--all-targets` based on existing project contract, not all-feature inclusion. Keep explicit `all_features` opt-in only with accurate warning. Provide a surfaced direct route to `scripts/verify.sh quick/full` only if an existing approved non-arbitrary execution route owns it; otherwise document that these are separate commands. Reconcile registry's illustrative all-features sentence with AGENTS guidance.
Storage: no migration planned; stop if a persisted schema change becomes necessary.
Protocol: preserve existing durable identities and transport messages; any model-visible schema evolution must be backward compatible or explicitly justified.
Security: no widening of command, workspace, tool or user authority. No explicit Deny recovery by retry or silent fallback.
Documentation/static guards: prefer focused existing Rust tests over new broad static checkers.

## 7. Ordered work packages
### A — Define feature/scope resolver
Capture current Cargo workspace metadata only if needed; reject nonexistent packages, incompatible combinations and flag injection. No fallback to arbitrary shell.
### B — Preserve process authority and results
Use existing process runner, timeout/output bounds, cwd and error classification for scoped commands. Verify that a Rust crate lint in workspace doesn't pull unrelated optional features.
### C — Docs and focused tests
Test explicit/implicit features, workspace/package, unsupported project, MSRV-incompatible optional package, and timeout output; reconcile documentation conflict.

## 8. Failure, cancellation, restart, and contention semantics
Cancellation and timeouts return partial bounded diagnostics and terminate managed process per current policy. Parallel invocations remain governed by existing execution locks and scheduler policies; no out-of-workspace CWD fallback after session switch.

## 9. Compatibility and migration
Existing `verify(action=lint)` remains supported but changes to a narrower, project-compatible default; explicit feature opt-in preserves expansive behavior for users who want it. No migration. Do not hide unknown feature or package flags.

## 10. Required tests
- Exact argv regression: lint contains --offline, -D warnings, and no --all-features by default.
- Explicit package/features/all_features schema validation and injection negatives.
- Real temporary Cargo workspace fixture with optional feature and a second crate; verify scoped resolution and unsupported non-Rust error.
- CWD isolation, bounded timeout/result reporting.

## 11. Required verification commands
```bash
cargo test -p codegg --lib tool::verify
cargo test -p codegg --test tool_execution
scripts/verify.sh quick
```
Required handoff commands, not executed during this planning-only pass. Constrain tests to the touched crate/feature and record any unavailable binary/platform evidence.

## 12. Documentation updates
`AGENTS.md`, `plans/registry.md` verification policy, `docs/tools.md`, `architecture/testing.md` with one coherent canonical scope rule.

## 13. Acceptance criteria (externally observable)
- Routine Rust lint defaults no longer force all optional features.
- An agent can select a package and explicit features using typed validation, and error output tells it when it must use `bash` for unsupported scope.
- Tests demonstrate no arbitrary command injection or workspace escape.

## 14. Stop conditions
If correcting documentation requires contradicting an accepted workspace-wide policy, or the only proposed implementation is passing free-form Cargo flags/commands directly to a shell.
Do not improvise an unapproved ADR, silently weaken security, substitute mocks for required production-path qualification or rewrite historical closure records.

## 15. Closure evidence required
Closure `plans/closure/agent-tool-use-operability-post-audit-corrective/005-status.md`: exact implementation SHAs, before/after repro, evidence matrix, tests/commands with pass/fail/unrun, failure/restart/contention/security analysis, residual findings and unblock audit; update roadmap and registry in the same closure commit.

## 16. Handoff notes
Prefer a small typed Rust resolver; no bespoke language/framework detection service. The project-wide verify.sh full path remains canonical for broad qualification.
