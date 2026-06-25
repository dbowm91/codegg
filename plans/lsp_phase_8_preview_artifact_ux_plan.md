# LSP Phase 8 Plan: Preview Artifact UX and Stale-Base Lifecycle

Status date: 2026-06-25
Phase type: preview lifecycle / safety UX / apply handoff
Primary goal: make preview-only LSP edit artifacts inspectable, refreshable, invalidatable, and safely handoff-able to the mutating apply path without weakening the LSP read-only boundary.

## Current baseline

The repo already has substantial preview infrastructure:

- `renamePreview`, `formatPreview`, `sourceActionPreview`, and `codeActionPreview` produce preview-only edit artifacts.
- `LspToolOutput` can include `preview_id` and `PreviewMetadata` with `not_applied`, `edit_count`, `affected_files`, and `stale_base`.
- `LspTool` owns a shared `PreviewArtifactRegistry` protected by a mutex.
- `egglsp::LspContextPacket` can carry preview artifacts and parallel preview IDs.
- Rename and formatting previews track base-stale behavior, including per-file stale-base evidence in the richer operation DTOs.
- Command-only code actions are rejected rather than executed.
- Applying a preview is intentionally not part of `LspTool` and should continue to flow through a separate mutating operation such as `apply_patch`.

Phase 8 should not reinvent preview generation. It should make preview artifacts usable and safe after they exist.

## Non-goals

Do not make `LspTool` write files.

Do not add direct `workspace/executeCommand` support.

Do not silently auto-apply previews.

Do not ignore stale-base flags. A stale preview must require warning, recomputation, or explicit user acceptance through the mutating apply path.

Do not persist preview artifacts across process restarts unless a later phase designs a persistence format with hash/root/server-generation validation.

## Preview lifecycle model

Adopt this explicit lifecycle for every LSP preview artifact:

```text
created -> inspectable -> applicable candidate
created -> stale -> recompute or discard
created -> expired -> discard
created -> applied by external mutating path -> historical/applied marker or removal
created -> cleared by user -> removed
```

Phase 8 does not need to implement every state as a stored enum on day one, but the UX and docs should use this vocabulary consistently.

## Workstream 1: registry metadata audit and extension

### Problem

The registry exists, but the lifecycle metadata may not be rich enough for list/detail/recompute/apply handoff. Phase 8 should audit what is stored and extend it only where necessary.

### Target files

- `crates/egglsp/src/preview_registry.rs`
- `crates/egglsp/src/context.rs`
- `src/tool/lsp.rs`
- `crates/egglsp/src/tui_summary.rs`
- Tests around preview registry

### Required metadata

Each registry entry should be able to answer:

- preview ID,
- preview kind (`rename`, `formatting`, `source_action`, `code_action` or equivalent),
- creation timestamp,
- provenance string,
- affected files,
- original hashes by file,
- current stale-base status,
- edit count,
- patch availability / patch omitted status,
- not-applied invariant,
- optional server ID,
- optional server generation,
- optional workspace root,
- optional operation-specific title/description,
- optional stale files with expected/actual hashes.

### Implementation steps

1. Audit `PreviewArtifactRegistry` entry fields.
2. Add missing fields only if needed by list/detail/status UX.
3. Preserve backward compatibility for serialized packet fields where practical.
4. Add helper methods:
   - `list_recent(limit)` or reuse existing `recent(limit)`,
   - `get(id)`,
   - `remove(id)`,
   - `clear()`,
   - `mark_applied(id)` only if applied state is tracked,
   - `refresh_staleness(id)` if staleness can be recomputed from current disk hashes.
5. Ensure all helper methods are pure/synchronous where possible and do not perform LSP requests.
6. Add tests for registry insertion, lookup, removal, clearing, ordering, and stale-base refresh.

### Acceptance criteria

- Registry entries contain enough metadata for a user-facing detail view.
- Registry APIs support list, detail, clear/remove, and stale-base refresh.
- Tests cover stale and non-stale preview entries.

## Workstream 2: preview list and detail UX

### Problem

Preview IDs are useful only if users can inspect them. Phase 8 should add a read-only preview list/detail surface in the TUI and/or slash commands.

### Target files

- `src/tool/lsp.rs`
- TUI command parsing/dispatch files
- TUI dialog/panel components
- `crates/egglsp/src/tui_summary.rs`
- New tests for render helpers and command output

### Suggested commands

Use names that fit existing command conventions. Suggested public surface:

- `/lsp-previews` or `/preview-list`: list recent preview artifacts.
- `/lsp-preview <id>` or `/preview-show <id>`: show detail and patch snippets.
- `/lsp-preview-clear [id|--all]`: clear one preview or all previews.
- `/lsp-preview-refresh <id>`: recompute stale-base status from current file hashes.

If the repo already has a preview/dialog convention, follow it instead of adding inconsistent commands.

### List view should show

- preview ID,
- kind,
- title/description,
- affected file count,
- edit count,
- stale-base warning,
- created age,
- server generation if available,
- not-applied status.

### Detail view should show

- full preview ID,
- provenance,
- operation kind,
- affected files,
- edit count,
- original hashes,
- current stale-base state,
- stale files with expected/actual hashes when available,
- patch text or patch omitted marker,
- explicit message: `This preview has not been applied.`

### Implementation steps

1. Add pure formatting helpers for list and detail output before wiring TUI.
2. Add command handlers that read from the registry without mutating files.
3. Render stale-base warnings prominently.
4. For large patches, show bounded snippets and indicate omitted/truncated data.
5. Ensure missing preview IDs return a useful error.
6. Add tests for list/detail formatting:
   - empty registry,
   - one fresh preview,
   - one stale preview,
   - missing preview ID,
   - patch omitted/truncated preview.

### Acceptance criteria

- Users can list and inspect preview artifacts without model intervention.
- Stale previews are visually distinct.
- Detail output states that the preview is not applied.
- Large/omitted patches are handled explicitly.

## Workstream 3: stale-base lifecycle

### Problem

A preview computed against old file content can be unsafe to apply. The repo already records stale-base metadata; Phase 8 should make it operational.

### Target files

- `crates/egglsp/src/preview_registry.rs`
- `crates/egglsp/src/edit.rs`
- `src/tool/lsp.rs`
- TUI preview display files
- Tests around stale-base behavior

### Stale-base policy

A preview is stale if any affected file's current content hash differs from the original hash recorded when the preview was computed. Stale previews may still be inspectable, but the UI and agent context must not present them as clean apply candidates.

Stale preview handling should offer one of these paths:

- recompute the semantic preview from the current file state,
- discard/clear the preview,
- allow explicit user-directed application through the mutating apply path with a strong warning, if the apply path supports that mode.

Phase 8 should prefer recompute/discard over stale apply.

### Implementation steps

1. Add or harden `refresh_staleness` helper that re-hashes affected files and updates/returns stale status.
2. Surface per-file stale details when available.
3. Ensure status summary marks preview registry stale if any recent preview is stale.
4. Add stale warning to rendered agent context when preview IDs are included.
5. Add tests:
   - unchanged file remains fresh,
   - changed file becomes stale,
   - missing affected file becomes stale or invalid with clear reason,
   - multi-file preview reports only the stale files,
   - stale preview warning renders in list/detail.

### Acceptance criteria

- Stale-base status can be recomputed from disk hashes.
- Stale preview details identify the specific affected files where possible.
- Stale previews are not silently treated as apply-ready.
- Status summary and detail views expose stale state.

## Workstream 4: recompute flow

### Problem

When a preview is stale, users need a path to regenerate it from the current file state. Since the registry may not currently store the full original request, Phase 8 must decide how far recomputation should go.

### Target files

- `crates/egglsp/src/preview_registry.rs`
- `src/tool/lsp.rs`
- Preview-producing operation handlers
- TUI preview command handlers

### Design options

Option A: store enough request metadata to recompute.

Pros: best UX. Users can run `/lsp-preview-refresh <id> --recompute` and receive a new preview ID.
Cons: registry entries need operation-specific request fields such as file path, position, new name, action kind, action index, formatting options, and selected code action metadata.

Option B: stale refresh only, with instructions to rerun the original command.

Pros: simple and safe.
Cons: weaker UX.

Recommended Phase 8 approach: implement Option B first unless storing request metadata is already easy. Add a clearly named extension point for future recompute. If implementing Option A, keep it narrow to rename/format/sourceAction first and defer arbitrary code-action recomputation.

### Minimal implementation steps

1. Add `refresh_staleness` command/helper.
2. If stale, render a message explaining how to rerun the original LSP preview command.
3. Store operation kind and provenance string so the detail view can guide recomputation manually.
4. Optionally add request metadata for `formatPreview` and `sourceActionPreview`, because they are easier to recompute than selected code actions.
5. Defer full code-action recompute unless action identity is stable.

### Acceptance criteria

- Users can at least refresh stale-base status.
- Stale output tells users how to regenerate the preview.
- Any recompute support returns a new preview ID rather than mutating the original entry in-place, unless explicitly designed otherwise.

## Workstream 5: apply handoff boundary

### Problem

LSP previews need a safe route into the existing mutating apply path. The handoff must preserve the rule that `LspTool` is read-only.

### Target files

- `src/tool/lsp.rs`
- apply patch tool implementation
- TUI command handlers/dialogs
- Permission/approval code if apply handoff touches it
- Docs around tool safety boundaries

### Required behavior

The preview detail view should make clear that applying requires a separate mutating operation. The handoff should pass patch text, affected files, original hashes, and preview ID/provenance to the mutating path where possible.

Applying should require normal user approval. It should recheck original hashes before applying, or at minimum warn if preview stale-base is true. The mutating path should not trust stale registry state without revalidation.

### Implementation steps

1. Identify the existing `apply_patch` tool input shape.
2. Determine whether a preview can be converted to one or more patch inputs without losing file identity.
3. Add a read-only helper that exports a preview as an apply candidate:
   - preview ID,
   - patches by file,
   - original hashes,
   - affected files,
   - stale flag,
   - provenance.
4. Add TUI action/keybinding or command that starts the apply flow but does not bypass approval.
5. Revalidate current hashes at apply time.
6. On successful apply, either mark preview applied or remove it from active preview list.
7. On failed apply, preserve the preview and show the failure reason.

### Acceptance criteria

- `LspTool` still performs no disk writes.
- Applying a preview goes through the existing mutating path and approval model.
- Apply handoff includes stale-base/provenance metadata.
- Current file hashes are checked before apply or a clear warning blocks silent apply.
- Applied previews are not still shown as active unapplied candidates.

## Workstream 6: agent-facing preview policy

### Problem

Agents can receive preview IDs through context packets and tool output, but they need clear instructions about what they may and may not do with them.

### Target files

- `crates/egglsp/src/context_renderer.rs`
- `crates/egglsp/src/bridges.rs`
- `src/agent/loop.rs`
- Tool/system prompt assembly files
- `.opencode/skills/lsp/SKILL.md`

### Policy

Rendered agent context should state:

- preview exists,
- preview ID,
- preview is not applied,
- affected files,
- edit count,
- stale-base status,
- user approval is required for application,
- stale previews should be recomputed before apply.

Agents should not treat preview output as evidence that the workspace already changed.

### Implementation steps

1. Update context rendering for preview IDs to include not-applied and stale-base wording.
2. Add tests that preview IDs render with safety wording.
3. Ensure stale previews are not suggested as clean next actions.
4. Document in the LSP skill that preview artifacts are candidate edits only.

### Acceptance criteria

- Agent-facing context cannot easily confuse previewed edits with applied edits.
- Stale previews render with a warning.
- Tests cover preview render text.

## Workstream 7: cleanup/expiry policy

### Problem

Preview registries can grow stale and noisy. Phase 8 should define cleanup behavior even if the initial implementation is simple.

### Target files

- `crates/egglsp/src/preview_registry.rs`
- Config schema if a TTL/cap is configurable
- TUI preview list/detail code

### Suggested policy

- Keep a bounded number of preview artifacts per `LspTool` instance.
- Default cap: choose a conservative number such as 32 or 64.
- Drop oldest previews when cap is exceeded.
- Provide manual clear all.
- Optional TTL can be added later; count-based cap is enough for Phase 8.

### Implementation steps

1. Audit whether registry already has a cap.
2. If not, add a default max entries cap.
3. Ensure recent previews are ordered newest-first.
4. Add manual clear command.
5. Add tests for cap eviction and clear.

### Acceptance criteria

- Registry cannot grow without bound.
- Users can clear stale/noisy preview state.
- Eviction behavior is deterministic.

## Workstream 8: documentation

### Target files

- `architecture/lsp.md`
- `.opencode/skills/lsp/SKILL.md`
- README if feature list mentions LSP preview behavior

### Documentation content

Add a Phase 8 section describing:

- preview artifact lifecycle,
- preview ID semantics,
- not-applied invariant,
- stale-base detection,
- list/detail/clear/refresh/apply handoff commands,
- apply path boundary,
- command-only code action rejection,
- preview registry cap/expiry behavior.

### Acceptance criteria

- Docs make it clear that preview operations do not write files.
- Docs explain what stale-base means.
- Docs explain how to inspect and apply previews safely.

## Suggested implementation order

1. Audit registry fields and add missing metadata.
2. Add pure list/detail/stale formatting helpers and tests.
3. Add preview list/detail/clear commands or TUI panel.
4. Add stale-base refresh behavior.
5. Add apply-candidate export and mutating apply handoff, preserving approval boundary.
6. Add agent render policy for preview IDs.
7. Add registry cap/cleanup.
8. Update docs and closeout notes.

## Completion checklist

- [ ] Registry metadata supports list/detail UX.
- [ ] Registry supports lookup by preview ID.
- [ ] Registry supports remove/clear.
- [ ] Registry has bounded growth or documented cap behavior.
- [ ] Stale-base refresh can re-hash affected files.
- [ ] Preview list view exists.
- [ ] Preview detail view exists.
- [ ] Stale previews render warnings.
- [ ] Missing preview ID returns a clear error.
- [ ] Apply handoff goes through mutating path and approval model.
- [ ] `LspTool` remains read-only and does not write files.
- [ ] Applied previews are removed or marked applied.
- [ ] Agent context states preview IDs are not applied.
- [ ] Docs explain lifecycle and stale-base semantics.
- [ ] Tests cover fresh, stale, missing-file, clear, cap, and render cases.
- [ ] `cargo fmt --check` passes.
- [ ] Relevant LSP/tool/TUI tests pass.

## Handoff notes for smaller models

Start with registry and pure render helpers. Do not begin with TUI wiring.

Keep stale-base refresh separate from recompute. Refreshing staleness is hash comparison; recomputing a preview may require a fresh LSP operation and operation-specific request metadata.

Never make preview application a method on `LspTool`. Export an apply candidate and let the existing mutating apply path own the write and approval boundary.
