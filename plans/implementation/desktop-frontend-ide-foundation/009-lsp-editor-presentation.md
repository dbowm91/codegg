# Desktop Frontend and IDE Foundation Milestone 006-C — LSP Editor Presentation

Status: ready for handoff (desktop corrective C003 closed)

Repository baseline: `1c4a30563755056c9d16152e48b5bb56612de992` (C003 diagnostics readiness and Git timeout process-group correction hosted-green)

Source roadmap:

- `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md#m006-decomposition`

Audit and prerequisite evidence:

- `plans/subsystems/desktop-frontend-ide-foundation-m006-tui-presentation-audit.md`
- `plans/closure/desktop-frontend-ide-foundation/009-status.md`
- `plans/adrs/ADR-0012-native-lsp-read-surface-delivery-and-authorization.md` (accepted)

Long-term requirements:

- `plans/000-long-term-specification.md#45-locality-by-default`
- `plans/000-long-term-specification.md#47-correctness-before-transparent-magic`
- `plans/000-long-term-specification.md#87-project-authorization`
- `plans/001-terminology-and-domain-model.md`

Applicable ADRs:

- `plans/adrs/ADR-0012-native-lsp-read-surface-delivery-and-authorization.md` (accepted)
- `plans/adrs/ADR-0008-lsp-preview-apply-authorization-and-runtime-ownership.md` (accepted; unchanged write-side boundary)

Primary class: capability

## 1. Objective

Present the existing authorized LSP read surface in the TUI editor: show current-file diagnostics, provide bounded syntax/semantic token styling where the current token legend supports it, and let the user invoke hover, definition, references, and symbol search from the editor. The daemon remains the only LSP owner and M005 `DocumentController` remains the only text owner.

One bounded outcome: an editor user can inspect current-file diagnostics and request navigational information without a second language-server connection or a new protocol surface.

## 2. Why this milestone is ready

- M006-A is strictly closed and supplies `Route::Editor`, `TuiDocumentSession`, and the controller-backed snapshot.
- M006-B is strictly closed with accepted ADR-0012. Its diagnostics pull and read requests already exist in `codegg-protocol`, `codegg-client`, and the daemon.
- C003 is strictly closed: unavailable/no-snapshot diagnostics are typed `NotReady`, while a complete empty set is `Ready`; older responses without the additive status fail closed to `NotReady`.
- M006-D and M006-E are closed; this work does not change the tree or apply path.
- Authorization is already enforced at `file.read`; this plan adds no operation or capability.

## 3. Current implementation evidence

- `src/tui/document_session.rs` holds controller-backed editor presentation state and no text buffer.
- `src/tui/components/editor.rs` renders the current snapshot and owns the editor viewport and key path.
- `crates/codegg-protocol/src/lsp.rs` defines bounded `LspReadRequestDto`, response payloads, semantic-token DTOs, and diagnostic DTOs.
- `src/core/daemon_lsp.rs` resolves reads through the daemon-owned warm `LspService`; cold services return `NotReady` and are not launched by reads.
- `LspDiagnosticsGet` provides the authoritative complete diagnostics result. The subscribe response can provide a snapshot, but the production change publisher is currently undriven; this plan therefore does not promise or depend on ongoing push updates.
- Hosted root CI `38231581158` passed on baseline `1c4a30563755056c9d16152e48b5bb56612de992`, including the complete 12,767-test workspace run. Local focused daemon unit-test linking is unavailable on this host because required native compression libraries are missing; the same tests passed in hosted CI.
- `LspReadOperation` currently includes hover, definition, references, document/workspace symbols, and semantic tokens. It has no completion operation.

Before changing code, verify these interfaces against the current baseline and inspect existing editor rendering/input and projection subscription seams. Repository evidence wins if a cited detail moved.

## 4. Invariants that must not regress

1. `DocumentController` is the sole owner of editor text. The presentation may read a bounded snapshot but may not cache a second text buffer.
2. The daemon-owned `LspService` remains the sole language-server owner; no client-side process or service is created.
3. The M006-B read DTOs, authorization descriptors, and ADR-0012 delivery contract are consumed unchanged. No protocol, storage, or migration changes.
4. Diagnostics are keyed to the exact active workspace/document identity. A late response or old stream event cannot decorate a newly opened document.
5. A missing result is never presented as authoritative emptiness. Diagnostics are refreshed through the pull authority on editor open, after save, and by an explicit refresh action; this plan does not add polling or claim push updates.
6. LSP ranges and semantic tokens are clamped/validated against the current snapshot and UTF-8 boundaries before presentation.
7. `src/lsp/mutation.rs`, `LspPreviewApply`, and M006-E review behavior remain untouched.
8. Cold LSP state remains explicit `NotReady`; reads never start a server implicitly.
9. All input and retained presentation state remain bounded; diagnostics and symbol text are untrusted data.

## 5. Scope

### In scope

- Display current-file diagnostics with severity, message, and range; provide a bounded gutter/line marker and a focused diagnostic-list affordance using existing editor state and rendering primitives.
- Fetch diagnostics through `LspDiagnosticsGet` on editor open, after successful document save, and on an explicit refresh action. Bind every result to the active workspace/document generation. Do not depend on project-stream publication because the delivered publisher has no production driver.
- Add bounded syntax/semantic styling only for tokens whose legend and range can be interpreted safely; semantic token absence, unknown token types, overlap, stale version, or `NotReady` falls back to the existing plain rendering.
- Add explicit editor actions for hover, go to definition, references, document symbols, and workspace symbol search using existing `LspReadGet` client methods. Navigate only to validated workspace-relative results; surface unavailable/out-of-workspace results without opening them.
- Add focused reducer/presentation/input tests for stale response rejection, document switching, diagnostic resync, range validation, fallback, and navigation.
- Update architecture, TUI skill, and user documentation to describe only delivered presentation behavior.

### Explicitly out of scope

- Completion, signature help, rename, code actions, formatting, type definition, or new LSP operations.
- Starting/warming language servers, selecting server installations, or changing diagnostics publication cadence.
- New protocol or authorization operations, storage, migration, or a new push transport.
- Applying edits, agent workflows, dirty-buffer merge, Git operations, or changing M006-E.
- Graphical/Monaco/Tauri editor work, multi-cursor editing, or a complete IDE feature set.
- Adding a new syntax-highlighting dependency without a separate dependency review; prefer the existing token contract and graceful plain-text fallback.

## 6. Required production changes

### Core/domain

None. Consume the existing protocol DTOs and client methods. Any missing operation is a stop condition requiring a separately planned protocol/ADR change.

### Storage and migrations

None. Diagnostics and read results are ephemeral presentation state. Do not persist them or copy document text into another store.

### Protocol and DTOs

None. Use the exact M006-B pull request. Do not infer `Ready` from empty or absent results.

### Runtime and concurrency

- Bind each query/subscription to the active workspace and document generation.
- Use generation identity to discard late completions after document close/switch.
- Apply a complete diagnostics set only if its request still matches the active generation; do not add periodic polling.
- Keep editor rendering responsive: query actions use the existing guarded TUI async-task lifecycle.

### Frontend

- Extend editor presentation state with bounded diagnostics, query status/result, and selection/navigation state.
- Render diagnostics in the editor without altering the underlying snapshot. Any semantic token overlay is derived from the current snapshot plus validated LSP ranges.
- Keep keyboard bindings scoped to the editor route and disclose `NotReady`, invalid location, and no-result states.

### Security and authorization

- Reuse the daemon's existing `file.read` gate; never accept principal, project, or workspace authority from LSP payload fields.
- Treat messages, symbols, hover content, and token types as untrusted bounded display data. Render as text; do not interpret markup or paths as authority.
- Validate every path against the active workspace and every position/range against the current document snapshot.

### Documentation and guards

- Update `architecture/tui.md`, `architecture/lsp.md`, `.opencode/skills/tui/SKILL.md`, `.opencode/skills/lsp-ide/SKILL.md`, and `docs/tui.md` with verified behavior.
- Add or extend a focused static guard only if a source-level invariant can be expressed without a new CI lane. Keep the `check_tui_editor_text_authority.py` invariant non-vacuous.

## 7. Ordered work packages

### WP-A — Interface and lifecycle trace

Confirm client query APIs, diagnostics pull behavior, token encoding/legend, active workspace identity, and editor task completion patterns. Record a short state/generation table before implementation. Stop if any essential action requires a new protocol surface.

### WP-B — Diagnostics presentation and resync

Bind current-file diagnostics to editor identity, render a bounded list/gutter marker, refresh on open/save/manual request, and reject stale generations. Prove document-switch and stale-result cases.

### WP-C — Semantic styling and navigation actions

Render only validated semantic spans with plain-text fallback. Add guarded actions for existing hover/definition/references/symbol requests and workspace-relative navigation. Do not add commands to unrelated routes.

### WP-D — Qualification and documentation

Run focused tests, ownership/auth/static guards affected by the diff, and `scripts/verify.sh quick`. Verify default-feature CLI builds remain free of desktop toolchain requirements. Record exact outcomes in the closure evidence.

## 8. Failure, cancellation, restart, and contention semantics

- A closed/switched document invalidates pending response generations; stale results are discarded.
- A pending or failed pull leaves the previous snapshot visibly stale; only a complete successful pull can replace or clear it.
- `NotReady`, transport loss, authorization denial, invalid path, or malformed response remains visible as a bounded unavailable/error state and never mutates document text.
- A daemon restart follows existing client reconnect and projection reattach behavior. Rebind then pull diagnostics; do not restore old ephemeral ranges as current.
- Concurrent query actions are bounded and correlated; duplicate results cannot navigate after a newer document generation is active.

## 9. Compatibility and migration

No wire or storage compatibility change. Older capability behavior is the existing M006-B behavior; if the API reports unsupported or unavailable, the TUI shows a concise unavailable state and preserves editing. No silent fallback to local LSP.

## 10. Required tests

- Current-file diagnostics render with bounded severity/message/range and do not alter snapshot text.
- Stream event updates the correct file; unrelated file events do not affect the editor.
- Open/save/manual refresh uses the authoritative pull; an empty complete result clears diagnostics, while missing/stale result does not.
- Switch/close invalidates outstanding query and resync completions.
- Malformed, out-of-range, invalid UTF-8-boundary, and over-bound ranges fail closed to plain rendering.
- `NotReady` and no-result outcomes remain distinguishable.
- Definition/navigation rejects absolute, escaping, or stale workspace paths.
- Semantic token overlay handles overlapping/unknown token spans deterministically and falls back to plain rendering.
- Hover/references/symbol results are rendered as bounded text and cannot inject markup or authority.
- Existing editor text-authority guard remains non-vacuous; existing LSP authorization and projection guards pass.

## 11. Required verification commands

```bash
cargo fmt --all -- --check
cargo test -p codegg --lib tui::
cargo test --test document_client_trajectory
python3 scripts/check_tui_editor_text_authority.py
python3 scripts/check_authorization_matrix.py
scripts/verify.sh quick
```

Add the narrowest new focused targets as appropriate. Hosted root CI and Desktop E2E are required before strict closure because the milestone changes a user-facing editor route.

## 12. Documentation updates

Update the files listed in §6. Document which LSP actions are supported, how `NotReady` and stale diagnostics appear, and that the daemon remains the language-server authority. Do not describe this TUI as a complete IDE or claim completion/rename/code-action support.

## 13. Acceptance criteria

- A user can inspect current-file diagnostics and invoke the existing read operations from the editor.
- A workspace/document switch cannot display stale diagnostics, tokens, or navigation results.
- Diagnostics can be refreshed through the existing pull authority and remain visibly stale while a refresh is pending or fails.
- Malformed/out-of-workspace data cannot alter text, open arbitrary paths, or inject rendered markup.
- Semantic styles degrade to plain text when tokens are absent, unsupported, stale, or invalid.
- All changes use the existing protocol, daemon LSP service, authorization, and document controller.

## 14. Stop conditions

- Any action needs a new protocol DTO, authorization capability, push stream, schema, or LSP operation.
- The existing DTO lacks enough version/identity information to prevent stale presentation safely.
- Correct diagnostics association requires caching a second document-text buffer.
- Token encoding or legend cannot be interpreted unambiguously; ship the diagnostics/navigation slice with plain rendering only only if the plan can be amended without changing its objective, otherwise stop for a bounded replan.
- Any authorization, projection-isolation, execution-ownership, or text-authority guard regresses.

## 15. Closure evidence required

Record the generation/state transition table, focused test outcomes, no-protocol/no-storage diff audit, authorization and text-authority guard results, `verify.sh quick`, hosted CI and Desktop E2E URLs/results, and verified architecture/skill/user-doc updates. Strict close only after all acceptance criteria pass; do not infer delivery from compilation alone.

## 16. Handoff notes

The accepted ADR-0012 is the controlling delivery and authorization contract. `LspDiagnosticsGet` is the completeness authority. The diagnostics publisher is known to be undriven in production, so do not describe subscription as live updates or add polling in this milestone. Preserve the M006-A rule that editor text comes only from `DocumentController`.
