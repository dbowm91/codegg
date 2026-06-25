# LSP Phase 6 Plan: Polish, Docs, Status UI, and Platform Boundary

Status date: 2026-06-25
Phase type: stabilization / UX / documentation accuracy
Primary goal: make the already-implemented LSP system accurate, visible, and easy to reason about before adding broader semantic workflows.

## Current baseline

The core LSP implementation is mature enough that Phase 6 should not be treated as a capability-building phase. The repo already has:

- `crates/egglsp` as the authoritative LSP implementation crate.
- `src/lsp/mod.rs` as a thin compatibility/re-export layer.
- `src/tool/lsp.rs` as the model-facing LSP tool.
- Lifecycle, restart, runtime, supervisor, diagnostics, capability snapshots, preview registry, hunk context, semantic context, security context, context rendering, and TUI summary modules.
- A canonical Phase 5 `egglsp::LspContextPacket` boundary.
- Preview-only edit operations with preview IDs and metadata.
- Operational health states and restart-generation semantics.
- Real-server compatibility docs and CI boundaries for pinned server profiles.

Phase 6 should therefore close presentation, status, and documentation gaps.

## Non-goals

Do not add new LSP protocol operations in this phase.

Do not introduce a new context packet type. Use `egglsp::LspContextPacket` and existing bridges.

Do not change the preview-only safety boundary. LSP edit operations remain read-only from the tool perspective and must not write files.

Do not broaden real-server support claims beyond what the pinned CI matrix proves.

Do not add semantic memory or persistent caches.

## Workstream 1: documentation reconciliation

### Problem

The LSP docs have accumulated phase-by-phase drift. At minimum, the architecture and skill docs must agree on:

- Current server count.
- Which operations are model-facing.
- Which operations are internal-only.
- Which server profiles are pinned/tested versus best-effort definitions.
- Phase 5 status and the canonical packet boundary.
- Current preview artifact behavior.
- Current TUI/status behavior.

There is also older prose that can confuse contributors, such as tables listing an operation as exposed and later describing it as hidden/internal. The docs need one consistent status model.

### Target files

- `architecture/lsp.md`
- `.opencode/skills/lsp/SKILL.md`
- `README.md`
- `CHANGELOG.md` if an unreleased documentation note is appropriate
- `AGENTS.md` if its architecture index or counts mention LSP

### Implementation steps

1. Audit actual server definitions in `crates/egglsp/src/server.rs` and record the exact count.
2. Audit `src/tool/lsp.rs` operation dispatch to determine the actual model-facing operation list.
3. Audit `crates/egglsp/src/operations/**` for internal-only operations that should not be documented as model-facing.
4. Reconcile the architecture docs so every operation appears in exactly one of these categories:
   - model-facing read-only operation,
   - model-facing preview-only operation,
   - context-packet operation,
   - internal-only helper operation,
   - deferred/future operation.
5. Update server-support language to distinguish:
   - server definitions in `server.rs`,
   - compatibility profiles,
   - pinned real-server CI matrix,
   - experimental best-effort support.
6. Add an explicit Phase 6 status section to `architecture/lsp.md` summarizing that Phase 5 is closed and Phase 6 is last-mile polish/status/docs.
7. Update `.opencode/skills/lsp/SKILL.md` with the same high-level source of truth but keep it shorter than the architecture doc.
8. Remove stale phase language such as "in progress" when the implementation is already closed.
9. Add a short "do not add parallel context packets" contributor note near the packet/bridge documentation.

### Acceptance criteria

- Server count is consistent across `architecture/lsp.md`, `.opencode/skills/lsp/SKILL.md`, README, and any architecture index.
- The model-facing operation table matches `src/tool/lsp.rs` dispatch.
- Internal-only operations are clearly marked internal-only.
- Pinned compatibility claims are not generalized to all server definitions.
- The docs clearly say Phase 5 context-packet infrastructure is complete and Phase 6 is polish/status/UX.

## Workstream 2: LSP status summary accuracy

### Problem

`LspTool` has `lsp_summary_status_line`, `lsp_summary_detail`, and `build_lsp_summary`, but the current summary path risks under-reporting useful state. In particular, counts such as diagnostics, references, definitions, total items, and freshness buckets should not appear as meaningful zeros when no packet snapshot was consulted. Status output should distinguish real zero from unavailable/not-collected.

### Target files

- `src/tool/lsp.rs`
- `crates/egglsp/src/tui_summary.rs`
- TUI components that display the LSP status line or detail panel
- Existing tests around TUI summary rendering, or new tests under `tests/` / `crates/egglsp/tests/`

### Implementation steps

1. Inspect `egglsp::tui_summary::LspTuiSummary` and its render helpers.
2. Decide whether count fields should become `Option<usize>` or whether unavailable state should be represented by notes/labels while keeping the struct stable.
3. Update `LspTool::build_lsp_summary` so live service state is accurately represented:
   - active server keys,
   - per-key operational state,
   - server generation,
   - preview registry count,
   - recent preview IDs,
   - stale preview flag,
   - operational notes.
4. Avoid showing placeholder counts as if they were authoritative. Prefer a rendered detail such as `diagnostics: not collected in status snapshot` over `diagnostics: 0` if no diagnostic query was made.
5. If practical, include low-cost diagnostic cache counts from `LspService`/`DiagnosticsCollector` without issuing expensive LSP requests.
6. Expose unsupported/degraded status from capability decisions only when a packet or operation actually observed those decisions.
7. Add summary rendering tests for:
   - no clients,
   - ready server with no previews,
   - degraded/restarting/failed server state,
   - preview registry with stale and fresh previews,
   - unavailable count fields.

### Acceptance criteria

- The status line accurately distinguishes no LSP server, ready, indexing, degraded, restarting, failed, and stopped states.
- The detail panel includes active server/root/generation when available.
- Preview count, recent preview IDs, and stale-preview warning are visible.
- Placeholder zeros are removed or clearly labeled as not collected.
- Tests cover at least one non-ready state and one stale-preview state.

## Workstream 3: TUI/status command ergonomics

### Problem

The core system has health snapshots and summary helpers, but users need a predictable way to inspect LSP status without parsing tool JSON. Phase 6 should add minimal status UI/command affordances without waiting for the fuller Phase 9 lifecycle command surface.

### Target files

- `src/tui/**` status rendering locations
- slash-command registration/dispatch files if `/lsp` or `/lsp-status` exists or should be added
- `src/tool/lsp.rs` summary helpers
- `crates/egglsp/src/tui_summary.rs`

### Implementation steps

1. Locate current status-line rendering and whether `lsp_summary_status_line` is already wired.
2. Ensure the status line can show a compact LSP badge/state without blocking render.
3. Add or harden a detail view/command such as `/lsp-status` if not already present.
4. The status detail should show:
   - active client keys,
   - server ID,
   - workspace root,
   - generation,
   - operational state,
   - last known error if any,
   - stderr tail summary if already available,
   - preview registry count and recent IDs,
   - stale-preview indicator.
5. Keep the command read-only.
6. Avoid spawning or initializing new servers solely to render status unless explicitly requested by the user.
7. Add tests for command parsing or render helpers where feasible.

### Acceptance criteria

- Users can inspect LSP state from the TUI without a model tool call.
- The status detail does not accidentally start expensive server initialization unless that behavior is intentional and documented.
- Non-ready states produce actionable text.
- Preview registry state is visible in detail output.

## Workstream 4: platform and support boundary

### Problem

LSP support spans many server definitions, but real behavior varies by platform, install method, server version, root markers, and language ecosystem. Docs and UI should avoid overclaiming support.

### Target files

- `architecture/lsp.md`
- `.opencode/skills/lsp/SKILL.md`
- `.github/workflows/lsp-real-server.yml`
- `crates/egglsp/src/compatibility.rs`
- `crates/egglsp/tests/real_server_smoke.rs`

### Implementation steps

1. Audit the pinned real-server CI matrix and document exactly which servers and versions are covered.
2. Document that additional server definitions are discovery/launch definitions, not proof of full compatibility.
3. For each pinned server, document root-marker expectations and known limitations.
4. Make platform claims explicit:
   - what default CI covers,
   - what opt-in real-server CI covers,
   - any macOS/Linux/Windows caveats known from profiles,
   - which downloads are available automatically versus PATH-only.
5. Add a short troubleshooting section for common failures:
   - binary not found,
   - root not detected,
   - server initializing/indexing,
   - no diagnostics yet,
   - capability unavailable,
   - stale evidence after restart.

### Acceptance criteria

- Docs no longer imply all server definitions are equally verified.
- Users can tell whether a server is pinned/tested, profile-supported, or best-effort.
- Root marker and binary-install requirements are visible.

## Workstream 5: regression and drift guardrails

### Problem

Phase 6 is mostly documentation/UX, but it should still add guardrails that prevent drift from recurring.

### Target files

- New or existing tests under `tests/` and `crates/egglsp/tests/`
- Optional lightweight doc/check script if the repo has a pattern for this

### Implementation steps

1. Add a test that checks the server definition count against the documented count if a lightweight doc assertion pattern exists. If not, add a comment near the docs count saying how it was verified.
2. Add tests for `render_tui_status_line` and `render_tui_summary_detail` around non-ready and stale-preview cases.
3. Add a test or compile-time assertion around canonical bridge exports if feasible.
4. Ensure `cargo fmt` and the relevant test suite pass.
5. Run at minimum:
   - `cargo fmt --check`
   - `cargo test -p egglsp` or the narrow egglsp tests touched
   - relevant root tests for TUI summary or tool status helpers
   - `cargo clippy --workspace --all-targets --all-features -- -D warnings` if feasible for the handoff model

### Acceptance criteria

- Phase 6 changes are covered by tests where behavior changed.
- Docs-only changes do not leave stale contradictory claims.
- The LSP status UI has at least focused unit tests for render output.

## Suggested implementation order

1. Audit actual operation/server/status state.
2. Fix docs and status labels first.
3. Improve `LspTuiSummary` rendering semantics.
4. Wire/read status in TUI or command surface.
5. Add tests.
6. Update this plan with completion notes or create a closeout note.

## Completion checklist

- [ ] `architecture/lsp.md` reconciled with implementation.
- [ ] `.opencode/skills/lsp/SKILL.md` reconciled with implementation.
- [ ] README/AGENTS/CHANGELOG counts checked and updated if needed.
- [ ] Model-facing operation table matches `src/tool/lsp.rs`.
- [ ] Internal-only operation list is explicit.
- [ ] Pinned CI/platform boundary is explicit.
- [ ] LSP status line/detail avoids misleading placeholder zeros.
- [ ] Preview registry state appears in status detail.
- [ ] Non-ready health states render actionable notes.
- [ ] Tests cover summary rendering and stale-preview state.
- [ ] `cargo fmt --check` passes.
- [ ] Relevant LSP/TUI tests pass.

## Handoff notes for smaller models

Keep changes small and mechanical. Do not refactor the LSP service or restart coordinator in Phase 6 unless a status bug directly requires it.

When editing docs, verify against source files rather than copying older plan text. Treat `crates/egglsp/src/lib.rs`, `crates/egglsp/src/server.rs`, `src/tool/lsp.rs`, and `crates/egglsp/src/context.rs` as the main sources of truth.

When editing status UI, prefer pure render-helper tests over large end-to-end TUI tests unless an existing harness makes that cheap.
