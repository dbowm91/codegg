# LSP Phase 13-17 Corrective Verification Plan

Status date: 2026-06-27
Scope: focused verification after implementation of the Phase 13-17 roadmap.

## Purpose

The repo now appears to have implemented the substantive Phase 13-15 work, evaluated and deferred Phase 16 disk cache, and closed Phase 17 as a no-op/deferred decision. This verification pass should not add new LSP features. It should prove the new surfaces are wired, documented, bounded, and safe.

The goal is to close the gap between "implemented by commit message" and "verified by code, tests, docs, and command behavior."

## Current baseline

Observed post-plan changes include:

- `crates/egglsp/src/doctor.rs` with `LspDoctorReport`, `build_doctor_report()`, and `render_doctor_report()`.
- `/lsp-doctor` command registration and TUI handler.
- `LspObservabilitySnapshot` wiring into doctor output.
- Phase 14 workflow UX additions, including user-facing `/lsp-*` workflow commands and `LspWorkflowInvocation` / `LspWorkflowDisplay` style boundaries.
- Composed workflow tests and provenance concepts such as `SubRecipeProvenance`.
- `LspContextDiagnostics` in `context_policy.rs` plus `/lsp-context-diagnostics` command.
- Fixes around renderer/policy propagation for cross-file and hierarchy flags.
- `crates/egglsp/tests/lsp_cache_benchmark.rs` and disk-cache threat model docs.
- `plans/lsp_phase_16_disk_cache_decision.md`, deferring disk cache.
- `plans/lsp_phase_17_decision_note.md`, deferring manual lifecycle controls.

## Non-goals

Do not add new LSP protocol operations.

Do not add new workflow recipes.

Do not implement disk cache.

Do not implement `/lsp-start` or `/lsp-replay-docs`.

Do not execute `workspace/applyEdit` or `workspace/executeCommand`.

Do not change cache defaults.

Do not broaden mutation permissions.

## Workstream 1: command registration and dispatch verification

### Commands to verify

Verify all new Phase 13-15 commands are registered and dispatched:

- `/lsp-doctor [path]`
- `/lsp-context-diagnostics`
- `/lsp-repair-local`
- `/lsp-repair-hunk`
- `/lsp-review-file`
- `/lsp-review-diff`
- `/lsp-security-review`
- `/lsp-impact`
- `/lsp-test-repair`
- `/lsp-interface`
- `/lsp-cross-repair`
- `/lsp-call-neighbors` or final chosen spelling

### Target files

- `src/tui/command.rs`
- `src/tui/app/mod.rs`
- `src/tool/lsp.rs`
- command dispatch tests
- docs listing command names

### Verification steps

1. Build a command table mapping every registered command to its handler.
2. Verify command docs use the same spelling as registration and handler dispatch.
3. Add missing-argument tests for every command that requires args.
4. Add no-LSP-tool tests for every command that needs `LspTool`.
5. Add invalid-path or invalid-position tests for path/position-based workflows.
6. Verify read-only commands do not mutate files, previews, cache, or server lifecycle state.
7. Verify commands emit user-facing errors rather than panicking.

### Acceptance criteria

- Every registered Phase 13-15 command has a handler.
- Every handler has at least one dispatch test.
- Missing and invalid args produce clear usage/errors.
- Docs and command names match exactly.

## Workstream 2: `/lsp-doctor` behavior verification

### Purpose

Ensure `/lsp-doctor` is useful, read-only, and does not accidentally start servers or overclaim readiness.

### Required checks

1. `build_doctor_report()` does not call any server-starting API.
2. No-server state is rendered as diagnostic information, not success.
3. Outside-root paths are detected and remediated.
4. Unsupported language/no server profile is diagnosed.
5. Existing active server state includes key, generation, state, capabilities, and stderr tail when available.
6. Cache status and preview counts render correctly.
7. Observability snapshot renders only when available.
8. Remediation messages are actionable and not contradictory.
9. `/lsp-doctor` does not mutate cache or preview registry.

### Target files

- `crates/egglsp/src/doctor.rs`
- `crates/egglsp/src/root.rs`
- `crates/egglsp/src/health.rs`
- `src/tool/lsp.rs`
- `src/tui/app/mod.rs`

### Required tests

- doctor no service,
- doctor outside root,
- doctor unsupported language,
- doctor no active server but profile exists,
- doctor active server with capabilities,
- doctor with observability snapshot,
- doctor cache disabled/enabled,
- doctor stale previews count,
- TUI command missing path/default path behavior.

### Acceptance criteria

- `/lsp-doctor` is read-only and deterministic.
- It diagnoses common LSP failures without requiring log inspection.

## Workstream 3: workflow UX and composition verification

### Purpose

Phase 14 introduced a wide command surface. Verify it is bounded, read-only, and consistent.

### Checks

1. Every workflow command maps to a named recipe or composed recipe.
2. Workflows preserve `LspContextPacket` provenance, freshness, truncation, preview IDs, and unsupported operation notes.
3. Composed workflows record sub-recipe provenance and skip reasons.
4. Workflow commands never auto-apply previews.
5. Workflow output suggests next actions such as `/lsp-preview <id>` or `/lsp-doctor <path>`.
6. Long workflow output has an appropriate panel/modal/toast strategy.
7. Small/workhorse/frontier model tier differences are honored.
8. Path validation uses the existing allowed-root checks.

### Target files

- `crates/egglsp/src/workflow_recipes.rs`
- `crates/egglsp/src/context_renderer.rs`
- `src/tool/lsp.rs`
- `src/tui/app/mod.rs`
- `.opencode/skills/lsp/SKILL.md`

### Required tests

- one direct workflow per command or recipe,
- all composed workflows,
- sub-recipe provenance rendering,
- stale/truncation/preview/unsupported sections,
- invalid path/position,
- no auto-apply preview invariant,
- tier-specific caps.

### Acceptance criteria

- Workflow commands are workflow-first, not raw protocol wrappers.
- Composition remains bounded and transparent.

## Workstream 4: context diagnostics verification

### Purpose

Ensure Phase 15 diagnostics explain context shaping without prompt bloat or dead fields.

### Checks

1. `LspContextDiagnostics::from_packet_and_policy()` reports model tier, tier source, workflow, risk, stale policy, unavailable policy, max bytes, included/omitted/stale counts, truncation, cache hit, and notes.
2. `render_compact()` is stable and readable.
3. `/lsp-context-diagnostics` works before any context is collected and after context is collected.
4. Normal agent prompts do not include verbose diagnostics by default.
5. Cache-hit diagnostics are accurate.
6. Stale/unavailable policies are reflected in diagnostics.
7. Renderer feature-flag behavior is either propagated or explicitly documented.

### Target files

- `crates/egglsp/src/context_policy.rs`
- `crates/egglsp/src/context_renderer.rs`
- `src/tool/lsp.rs`
- `src/tui/app/mod.rs`

### Required tests

- diagnostics from empty packet,
- diagnostics from truncated packet,
- diagnostics with stale items,
- diagnostics with cache-hit note,
- render compact snapshot or stable substring tests,
- command before/after context collection,
- no prompt bloat regression.

### Acceptance criteria

- Diagnostics are inspectable on demand.
- Normal prompts remain compact.
- No diagnostics field is misleading or permanently dead.

## Workstream 5: disk-cache and lifecycle decision consistency

### Purpose

Phase 16 and 17 were intentionally deferred. Verify docs, config, and command surface do not contradict those decisions.

### Disk-cache checks

1. Config docs do not claim disk mode exists.
2. Runtime config does not expose a functional `disk` mode unless deliberately rejected with an error.
3. Cache docs say memory-only active mode.
4. Threat model and decision record agree on rationale.
5. Benchmark file is test-like and does not enable disk persistence.

### Lifecycle checks

1. `/lsp-start` and `/lsp-replay-docs` are not registered as active commands.
2. Docs say both are deferred unless reopened by evidence.
3. `/lsp-restart` and `/lsp-stop` docs remain accurate.
4. Per-key stop fallback is documented honestly.

### Target files

- `plans/lsp_phase_16_disk_cache_decision.md`
- `architecture/lsp_disk_cache_threat_model.md`
- `plans/lsp_phase_17_decision_note.md`
- `architecture/lsp.md`
- `.opencode/skills/lsp/SKILL.md`
- `src/tui/command.rs`
- config schema/docs

### Acceptance criteria

- Deferred features are not accidentally exposed as implemented.
- Decision docs match runtime behavior.

## Workstream 6: safety-boundary static sweep

### Purpose

Confirm the wide Phase 13-15 changes did not weaken the read-only LSP boundary.

### Required searches

Run and inspect:

```bash
rg "workspace/applyEdit|workspace/executeCommand|executeCommand|applyEdit" src crates/egglsp
rg "std::fs::write|write_all|File::create" src crates/egglsp
rg "mark_preview_applied|mark_applied" src crates/egglsp
rg "lsp-start|lsp-replay-docs" src crates/egglsp docs architecture plans .opencode README.md AGENTS.md
rg "disk" crates/egglsp/src/cache.rs crates/codegg-config src/tool architecture plans
```

### Expected allowed matches

- docs/tests rejecting unsupported LSP mutation,
- preview apply write-side helper and handler,
- decision docs discussing deferred features,
- config docs for memory cache.

### Disallowed matches

- LSP server command execution,
- direct LSP workspace edit application,
- new file writes inside read-only workflow/doctor/diagnostic paths,
- active `/lsp-start` or `/lsp-replay-docs` registration,
- active disk cache mode.

### Acceptance criteria

- Static sweep results are summarized in the closing commit or docs.
- Suspicious matches are removed or justified by tests/comments.

## Workstream 7: docs and roadmap reconciliation

### Target docs

- `architecture/lsp.md`
- `.opencode/skills/lsp/SKILL.md`
- `README.md`
- `AGENTS.md`
- `CHANGELOG.md`
- `plans/lsp_phase_13_17_roadmap.md`
- Phase 13-17 plan files and decision notes

### Checks

1. Phase status labels match actual implementation/defer decisions.
2. Command names are consistent across docs and code.
3. `/lsp-doctor` docs say read-only and no server start.
4. Workflow UX docs say read-only and no auto-apply.
5. Diagnostics docs say on-demand, not default prompt bloat.
6. Disk cache docs say deferred/no active disk mode.
7. Manual lifecycle docs say deferred/no active `/lsp-start` or `/lsp-replay-docs`.
8. Test claims cite exact command results or avoid overclaiming.

### Acceptance criteria

- No doc claims an unimplemented or deferred feature is active.
- Roadmap accurately states what is implemented, deferred, or decision-only.

## Workstream 8: focused test matrix

Run focused tests first:

```bash
cargo fmt --check
cargo test -p egglsp doctor
cargo test -p egglsp health
cargo test -p egglsp workflow_recipes
cargo test -p egglsp context_policy
cargo test -p egglsp context_renderer
cargo test -p egglsp evidence_collector
cargo test --test phase5_context_integration lsp
cargo test --test lsp lsp
```

Then run broader checks if feasible:

```bash
cargo test -p egglsp
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

If there are pre-existing flaky failures, document exact test names and prove the Phase 13-15 focused test set passes.

## Final closure criteria

This corrective verification pass is complete when:

- all Phase 13-15 commands have dispatch tests,
- `/lsp-doctor` is proven read-only and useful in common failure modes,
- workflow commands are bounded, read-only, and consistently rendered,
- context diagnostics are on-demand and not prompt-bloating,
- Phase 16 disk cache remains decision-only/deferred with no active disk mode,
- Phase 17 manual lifecycle controls remain decision-only/deferred with no active commands,
- static safety sweep confirms no LSP mutation boundary regression,
- docs and roadmap match code,
- focused tests and formatting pass, with any unrelated flakes documented precisely.
