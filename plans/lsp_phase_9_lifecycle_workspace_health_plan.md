# LSP Phase 9 Plan: Lifecycle, Workspace, Server-Health Ergonomics, and Preview Apply Handoff

Status date: 2026-06-26
Phase type: ergonomics / lifecycle control / safe apply handoff
Prerequisites: Phases 6, 7, and 8 closed or closeout-complete.

## Purpose

Phase 9 should make the existing LSP lifecycle and workspace machinery easy to inspect, control, and recover from. The internal implementation already has server health states, generation tracking, restart coordination, document replay, preview artifacts, stale-base detection, and status summaries. The remaining problem is operator ergonomics: users and agents need clear commands and UI surfaces for server/root/generation state, restart/stop actions, degraded-state reasons, root mismatch diagnosis, and the deferred preview apply-flow integration.

This phase also picks up the Phase 8 follow-up: `/lsp-preview-apply` currently exports a read-only apply candidate. Phase 9 should integrate that candidate with the normal mutating apply path without weakening the LSP safety boundary.

## Current baseline

The repo already includes:

- `LspOperationalState` with lifecycle states such as starting, initializing, indexing, ready, degraded, restart scheduled, restarting, failed, stopping, and stopped.
- Per-server keys, generation tracking, operational state snapshots, restart coordination, and stale evidence metadata.
- `/lsp-status` for minimal status display.
- Preview registry/list/detail/clear/refresh/export commands.
- `PreviewApplyCandidate` as a read-only handoff structure with preview ID, patches, original hashes, stale-base flag, affected files, provenance, and applied state.

Phase 9 should not build new semantic context types. It should expose and control what already exists.

## Non-goals

Do not add broad semantic operations; that is Phase 10.

Do not add semantic memory/cache; that is Phase 12.

Do not let `LspTool` write files directly.

Do not execute `workspace/executeCommand` or `workspace/applyEdit` from LSP.

Do not silently auto-restart or auto-apply in a way that hides state transitions from users.

Do not build a large persistent server manager redesign unless a small command/UI surface cannot use the existing service APIs.

## Workstream 1: lifecycle status command suite

### Problem

`/lsp-status` is useful but too compact for server lifecycle debugging. Users need scoped commands for status, clients, roots, generations, capabilities, and recent errors.

### Target files

- `src/tui/command.rs`
- `src/tui/app/mod.rs`
- `src/tool/lsp.rs`
- `crates/egglsp/src/tui_summary.rs`
- `crates/egglsp/src/health.rs`
- `crates/egglsp/src/service.rs`
- `crates/egglsp/src/supervisor.rs`
- `architecture/lsp.md`
- `.opencode/skills/lsp/SKILL.md`

### Proposed commands

Prefer command names that fit existing conventions. Suggested minimum set:

- `/lsp-status`: compact status, already present.
- `/lsp-status --detail`: expanded multi-server detail.
- `/lsp-servers`: list active server/client keys, roots, language, state, generation, pending requests, open docs.
- `/lsp-capabilities [server-key]`: show effective capability snapshot and unsupported reasons.
- `/lsp-errors [server-key]`: show last error and stderr tail if available.
- `/lsp-root [path]`: explain detected workspace root for a file/path, without starting a server unless explicit.

If the command parser does not support flags cleanly, use separate commands such as `/lsp-detail`, `/lsp-capabilities`, `/lsp-errors`, and `/lsp-root`.

### Implementation steps

1. Audit existing service APIs for:
   - client keys,
   - operational state,
   - generation,
   - health snapshot,
   - capability snapshot,
   - stderr tail,
   - root information,
   - open document count,
   - pending request count.
2. Add pure render helpers first. Do not wire TUI commands until formatting is testable.
3. Add a `LspServerStatusDetail` DTO if necessary. Keep it in `egglsp` if it is generic, or `src/tool/lsp.rs` if it depends on Codegg service wiring.
4. Render all states with explicit labels, not just ready/unavailable.
5. Include generation and stale-evidence implications. Example: `generation=7; diagnostics from older generations should be treated stale`.
6. Include root and server key for every row.
7. If stderr tail is available, show a bounded tail with truncation marker.
8. Ensure status commands are read-only and do not start servers unless explicitly documented.

### Acceptance criteria

- Users can list all active LSP clients/servers with root, state, and generation.
- Users can inspect effective capabilities for a server key.
- Users can see degraded/failed/error states with actionable text.
- Status commands do not accidentally start servers.
- Render helpers have tests for ready, indexing, degraded, failed, and restarting states.

## Workstream 2: manual lifecycle controls

### Problem

When a server degrades, fails, or gets wedged, users need manual control. Restart coordination exists internally; Phase 9 should expose safe commands.

### Target files

- `src/tui/command.rs`
- `src/tui/app/mod.rs`
- `src/tool/lsp.rs`
- `crates/egglsp/src/service.rs`
- `crates/egglsp/src/restart.rs`
- `crates/egglsp/src/supervisor.rs`
- tests under `crates/egglsp/tests/` or root TUI command tests

### Proposed commands

- `/lsp-restart [server-key|--all]`
- `/lsp-stop [server-key|--all]`
- `/lsp-start <file|root|server-profile>` only if startup can be scoped safely.
- `/lsp-replay-docs [server-key]` only if document replay is separately controllable and useful.

### Implementation steps

1. Identify the existing restart API and whether it requires server key, root, profile, or generation.
2. Expose restart through a safe wrapper that:
   - validates server key,
   - increments/observes generation correctly,
   - preserves stale-evidence semantics,
   - does not drop preview registry state silently,
   - reports scheduled/restarting/ready/failure outcomes.
3. Expose stop only if it cleanly shuts down server process and marks state stopped.
4. Add `--all` only after single-server flow is tested.
5. Avoid blocking the TUI while restart occurs. Return an immediate toast/status and let `/lsp-status` show progress.
6. Add tests for invalid key, restart scheduled state, generation change, and post-restart stale evidence marking.

### Acceptance criteria

- Users can restart a specific LSP server from the TUI.
- Restart state and generation changes are visible.
- Invalid keys return clear errors.
- Restart does not claim evidence is fresh after generation mismatch.
- Stop/restart commands remain scoped and safe.

## Workstream 3: workspace/root diagnosis

### Problem

Many LSP failures are root selection failures: wrong `Cargo.toml`, nested workspaces, monorepos, missing config, or a file outside the allowed root. Users need root diagnostics before deeper server work.

### Target files

- `crates/egglsp/src/server.rs`
- `crates/egglsp/src/service.rs`
- `crates/egglsp/src/compatibility.rs`
- root detection utilities if present
- `src/tool/lsp.rs`
- `architecture/lsp.md`

### Implementation steps

1. Add or expose a pure root-diagnosis helper:
   - input path,
   - detected language/profile,
   - candidate root markers found,
   - selected root,
   - allowed-root validation result,
   - server profile chosen,
   - reasons if no profile/root found.
2. Add `/lsp-root <path>` command to render the diagnosis.
3. Detect common problems:
   - file outside allowed root,
   - no root marker found,
   - multiple nested markers,
   - server binary missing,
   - profile unsupported for platform,
   - capability unavailable before initialization.
4. Ensure root diagnosis does not spawn a server by default.
5. Add tests with temp directory workspaces for Cargo, Python, TS, and no-root cases.

### Acceptance criteria

- A user can ask why a file did not get LSP context and receive a root/profile explanation.
- Root diagnosis is read-only and deterministic.
- Common no-root and wrong-root cases are tested.

## Workstream 4: preview apply-flow integration

### Problem

Phase 8 intentionally made `/lsp-preview-apply` a read-only apply-candidate export. That preserved the LSP boundary but left actual application as a manual handoff. Phase 9 should integrate the handoff with Codegg's normal mutating apply path.

### Safety invariant

`LspTool` must remain read-only. Preview application must happen through the existing mutating patch/apply path with normal user approval, hash revalidation, and clear stale-base warnings.

### Target files

- `crates/egglsp/src/tui_summary.rs`
- `crates/egglsp/src/preview_registry.rs`
- `src/tool/lsp.rs`
- existing apply patch tool implementation
- `src/tui/app/mod.rs`
- `src/tui/command.rs`
- permission/approval code if present
- tests around apply patch and preview export

### Proposed behavior

`/lsp-preview-apply <id>` should become one of two modes, depending on existing UI affordances:

Mode A, preferred if approval UI exists:

1. Resolve preview ID.
2. Refresh stale-base status before apply.
3. If stale, block by default and show a message: `Preview is stale; refresh/recompute before applying.`
4. If fresh, build apply candidate.
5. Open the normal patch-approval/apply flow with patches preloaded.
6. On successful apply, mark preview applied or remove it from active pending list.
7. On failed apply, keep preview pending and show failure.

Mode B, if patch-approval UI is not easily callable yet:

1. Keep command as export-only.
2. Rename output and docs to `apply candidate export`.
3. Add a follow-up plan item for `apply flow wiring` within Phase 9.
4. Do not mark Phase 9 complete until actual apply path is wired or explicitly deferred with a separate tracked plan.

### Implementation steps

1. Locate existing mutating apply patch APIs and their approval surface.
2. Define a conversion from `PreviewApplyCandidate` to the apply patch input shape.
3. Revalidate file hashes immediately before apply. Do not rely only on registry stale state.
4. If candidate contains no patches, block apply with clear error.
5. If candidate is already applied, block or ask for explicit reapply confirmation.
6. If candidate is stale, block by default. Consider `--force-stale` only if existing apply tools already support explicit risky operations; otherwise do not add force mode.
7. Ensure the command never calls LSP `workspace/applyEdit`.
8. Add tests:
   - fresh candidate opens/apply path without direct LSP write,
   - stale candidate blocked,
   - missing preview ID returns error,
   - no patch candidate blocked,
   - applied candidate not re-applied silently,
   - successful apply marks preview applied or removes it.

### Acceptance criteria

- `/lsp-preview-apply <id>` either opens the normal apply approval flow or remains explicitly export-only with a tracked Phase 9 subtask.
- No LSP code path writes files directly.
- Hashes are revalidated at apply time.
- Stale previews are blocked or require explicit existing approval semantics.
- Applied previews are not still shown as clean pending previews.

## Workstream 5: health-to-agent context policy

### Problem

Agents should not over-trust LSP evidence when servers are indexing, restarting, degraded, or failed. Phase 6 made status visible; Phase 9 should make lifecycle state affect agent-facing context notes consistently.

### Target files

- `src/tool/lsp.rs`
- `crates/egglsp/src/context_renderer.rs`
- `crates/egglsp/src/evidence_collector.rs`
- `crates/egglsp/src/health.rs`
- tests for rendered context

### Implementation steps

1. Ensure packets rendered during non-ready states include explicit operational notes.
2. For restarting/restart-scheduled/server-generation mismatch, mark or render evidence as stale/possibly stale.
3. For indexing, avoid saying diagnostics are complete.
4. For failed/stopped, render fallback notes and do not imply LSP absence means no issues.
5. Add tests for each major health state in rendered agent context.

### Acceptance criteria

- Agent context reflects lifecycle state.
- Non-ready LSP cannot be misread as clean semantic evidence.
- Tests cover indexing, degraded, restarting, failed/stopped.

## Workstream 6: documentation and closeout

### Target files

- `architecture/lsp.md`
- `.opencode/skills/lsp/SKILL.md`
- `README.md` command list if applicable
- `CHANGELOG.md`

### Documentation requirements

Document:

- lifecycle command suite,
- root diagnosis behavior,
- restart/stop semantics,
- preview apply handoff flow,
- stale-base and hash revalidation at apply time,
- the invariant that `LspTool` remains read-only,
- what remains deferred to Phase 10.

## Test matrix

Required focused tests:

```bash
cargo fmt --check
cargo test -p egglsp health
cargo test -p egglsp preview_registry
cargo test -p egglsp tui_summary
cargo test -p egglsp phase6_regression
cargo test --test phase5_context_integration lsp
```

Recommended broader checks:

```bash
cargo test -p egglsp
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

If full workspace tests are blocked by an unrelated known failure, document the failure and prove the focused LSP tests pass.

## Final acceptance criteria

Phase 9 is complete when:

- users can inspect server/root/generation/capability/error state from TUI commands,
- users can restart at least one scoped server safely,
- root diagnosis explains common no-LSP cases without starting servers,
- preview apply handoff is integrated with the mutating apply path or explicitly remains export-only with a tracked follow-up inside Phase 9,
- stale previews are revalidated before apply,
- agents receive lifecycle-state warnings in rendered context,
- docs and tests cover the new lifecycle and apply-handoff behavior.
