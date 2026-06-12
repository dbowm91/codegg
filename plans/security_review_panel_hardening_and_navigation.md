# Security Review Panel Hardening and Navigation Plan

## Purpose

Harden the newly implemented security review result panel so it becomes the primary review UX rather than a secondary view over a message-log report. This pass should align documentation with actual completion behavior, improve read-only source navigation from findings/prompts, and add lightweight receipt persistence if an existing session/message artifact mechanism is available.

Current state after the result-panel pass:

- `SecurityReviewReceipt` exists and stores structured `SecurityReviewOutput`, rendered text, args, completion timestamp, and enrichment metadata.
- `Dialog::SecurityReview` renders a master/detail view over findings, prompts, notes, and preflight results.
- `/security-review-show` reopens the latest in-memory receipt.
- `/security-review-cancel` aborts the active background task through `AbortHandle` and stale completions are ignored.
- `Enter` on a panel item emits a read-only jump action that currently copies `path[:line]` to the clipboard.
- Commit/docs may claim auto-open behavior; verify actual behavior and make docs/implementation match.

## Non-Goals

Do not change the security finding synthesis model.

Do not make LSP enrichment default.

Do not mutate source files.

Do not add exploit generation, network scanning, or dependency/CVE lookup.

Do not require a live LSP server in tests.

Do not build a full editor. A small read-only preview/navigation surface is enough.

Do not add a database migration unless an existing artifact/metadata mechanism makes persistence trivial.

## Phase 1 — Verify and Align Completion Behavior

First determine the actual completion behavior in `handle_security_review_finished` and related helpers.

Inspect:

```bash
rg "handle_security_review_finished|apply_security_review_receipt|set_latest_security_review|open_dialog\(Dialog::SecurityReview" src/tui -n
rg "security-review-show|Security review complete|latest_security_review" src README.md AGENTS.md architecture .opencode -n
```

Choose one policy and make implementation/docs match.

Recommended policy:

- Do **not** auto-open the result panel after every run.
- Store the receipt.
- Push the `[Security Review]` report to the message timeline.
- Show a concise toast: `Security review complete — run /security-review-show to open the result panel.`
- Add optional explicit auto-open later through a flag such as `--panel` or user preference.

Rationale:

Auto-opening a modal panel after a long background task can interrupt the user while they are typing or reviewing other output. The message-log report plus explicit `/security-review-show` command is less disruptive.

Acceptance criteria:

- docs and runtime behavior agree;
- no stale claim says the panel auto-opens unless it actually does;
- completion stores latest receipt and message-log report;
- user is told how to reopen the panel;
- no behavior change weakens cancellation/stale-completion safety.

## Phase 2 — Add Optional Explicit Panel Open Flag

Add an explicit user-controlled way to auto-open the panel on completion.

Suggested command flag:

```text
/security-review --panel
/security-review --changed --enrich --panel
```

Extend `SecurityReviewCommandArgs`:

```rust
pub open_panel_on_complete: bool,
```

Parser:

```rust
"--panel" | "--open-panel" => args.open_panel_on_complete = true,
```

Receipt/completion handling:

- store this preference in the args already embedded in the receipt;
- in the completion handler, after `set_latest_security_review(receipt)`, open `Dialog::SecurityReview` only when `receipt.args.open_panel_on_complete` is true;
- if the user has another modal open, either queue nothing and show toast, or open only if no modal is active. Prefer non-disruptive behavior.

Acceptance criteria:

- default remains non-disruptive;
- `--panel` opens the panel after successful completion;
- `--panel` does not open on failure or cancellation;
- stale completions cannot open the panel;
- tests cover both default and explicit auto-open.

## Phase 3 — Replace Clipboard-Only Jump With Read-Only Source Preview

The current `Enter` behavior copies `path[:line]` to clipboard. Keep that as fallback, but add a more useful read-only source navigation path.

Search for existing file/diff/source preview surfaces:

```bash
rg "File|Source|Preview|Diff|Goto|Jump|open_file|read_to_string|Dialog::" src/tui src -n
rg "goto|jump|preview|diff_dialog|review_dialog" src/tui -n
```

Preferred behavior:

1. If an existing read-only file/hunk viewer exists, open it at `file_path` + `line`.
2. Otherwise add a minimal `Dialog::SourcePreview` or extend an existing goto/dialog surface.
3. If neither is practical in this pass, keep clipboard fallback but make the panel detail clearly say `Enter copies location`.

Minimal read-only preview shape:

```rust
pub struct SourcePreviewDialog {
    pub path: PathBuf,
    pub line: Option<u32>,
    pub context_radius: usize, // default 10
    pub lines: Vec<SourcePreviewLine>,
    pub error: Option<String>,
    pub scroll: u16,
}

pub struct SourcePreviewLine {
    pub number: u32,
    pub text: String,
    pub highlighted: bool,
}
```

Source preview constraints:

- resolve against review root;
- refuse paths outside allowed root after canonicalization when possible;
- read only;
- cap file size and line count;
- show an error instead of panicking for missing/binary/oversized files;
- never open an editor in write mode;
- never apply a patch.

Acceptance criteria:

- pressing Enter on a finding/prompt with file path opens source preview or existing read-only viewer;
- fallback copies path:line if viewer cannot open;
- path traversal is guarded;
- no mutation path is introduced;
- tests cover source preview projection and outside-root rejection.

## Phase 4 — Root-Aware Location Handling

Ensure panel items can resolve selected paths correctly.

Current items store `file_path`, but preview needs a trusted root.

Use `SecurityReviewReceipt.root` as the allowed root. Add helper:

```rust
pub fn resolve_security_review_item_path(
    receipt: &SecurityReviewReceipt,
    item: &SecurityReviewPanelItem,
) -> Result<PathBuf, String>
```

Rules:

- if `item.file_path` is absolute, canonicalize and verify it is under `receipt.root`;
- if relative, join with `receipt.root` then canonicalize if file exists;
- reject missing parent/root escapes with a clear error;
- tolerate deleted/missing files by showing a missing-file message, but do not panic.

Acceptance criteria:

- source preview never reads outside review root;
- missing files produce UI errors;
- file paths from review output remain displayable even if no preview can be opened.

## Phase 5 — Persist Latest Receipt if Existing Artifact Support Exists

In-memory latest receipt is good, but a completed security review is a useful session artifact.

Do not add a DB migration in this pass unless the repo already has a generic artifact/message metadata mechanism.

Search:

```bash
rg "artifact|metadata|message metadata|attachments|receipt|session artifact|Json \{" src crates tests -n
rg "MessageStore|SessionStore|insert_message|append_message|PartData|UIMessage" src crates tests -n
```

Preferred low-risk persistence approach:

- store the rendered report as the existing assistant message, unchanged;
- if message metadata supports JSON blobs, attach the structured receipt there;
- if no metadata support exists, add only a TODO/doc note and defer.

Do not persist if it requires schema churn.

Acceptance criteria:

- either latest receipt can be restored from existing metadata/artifact path, or persistence is explicitly deferred with a reason;
- no schema migration unless already standard for this codebase;
- persisted receipts do not include secrets beyond what the security review already reports;
- cancellation/failure do not persist partial success receipts.

## Phase 6 — Improve Result Panel Detail Quality

Make the detail view more scannable.

Enhancements:

- wrap long detail lines instead of truncating by terminal width, if existing helpers support wrapping;
- group detail fields with labels:
  - Location
  - Severity / Confidence
  - Why it matters
  - Evidence
  - Recommendation
  - Suggested tests
- for prompts, show `Review prompt only — not a confirmed finding` at the top;
- for notes/preflight, separate status/evidence/notes;
- include `Enrichment: local-lsp | unavailable | off` in header.

Acceptance criteria:

- findings and prompts are visually distinct;
- prompt safety marker is visible in detail view;
- notes/preflight do not look like findings;
- detail scroll remains stable after filter changes.

## Phase 7 — Tests

Add tests for the behavior that was ambiguous or newly added.

Suggested tests:

```text
security_review_completion_default_does_not_auto_open_panel
security_review_completion_panel_flag_opens_panel
security_review_show_opens_existing_latest_receipt
security_review_show_without_receipt_warns
security_review_enter_opens_source_preview_when_file_exists
security_review_enter_copies_location_when_preview_unavailable
security_review_source_preview_rejects_outside_root
security_review_source_preview_handles_missing_file
security_review_source_preview_highlights_target_line
security_review_cancel_stale_completion_does_not_open_panel
security_review_receipt_persistence_deferred_or_metadata_roundtrip
```

Panel/projection tests:

```text
security_review_prompt_detail_contains_not_confirmed_marker
security_review_preflight_items_stay_under_notes_filter
security_review_filter_change_clamps_selection
security_review_medium_plus_filter_excludes_prompts
```

Acceptance criteria:

- no test requires live LSP;
- no test mutates repo files;
- no test depends on external editor integration;
- path root checks are covered.

## Phase 8 — Docs Updates

Update:

```text
README.md
AGENTS.md
architecture/lsp.md
architecture/tool.md
.opencode/skills/security/SKILL.md
.opencode/skills/agent-loop/SKILL.md
```

Document:

- default completion behavior: report goes to timeline, panel can be reopened with `/security-review-show`;
- optional `--panel` behavior if implemented;
- Enter behavior in panel: source preview or clipboard fallback;
- source preview is read-only and root-scoped;
- cancellation remains best-effort;
- prompts are review prompts, not confirmed findings;
- receipt persistence status: in-memory only or session artifact if implemented.

Acceptance criteria:

- docs no longer claim auto-open if default behavior does not auto-open;
- docs mention exact command names and keybindings;
- docs preserve read-only/no-exploit/no-network-scan semantics.

## Validation Commands

Run:

```bash
cargo fmt --check
cargo check --workspace --all-targets
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Targeted:

```bash
cargo test -p codegg security_review_panel
cargo test -p codegg security_review_source_preview
cargo test -p codegg security_review_receipt
cargo test -p codegg security_review_cancel
cargo test -p codegg security_review_show
rg "open_panel_on_complete|--panel|SourcePreview|SecurityReviewJump|resolve_security_review_item_path|latest_security_review" src tests README.md AGENTS.md architecture .opencode
rg "open_dialog\(Dialog::SecurityReview|security_review_running|AbortHandle|SecurityReviewFinished" src/tui tests
```

Manual smoke:

```text
1. Run /security-review --changed. Confirm panel does not auto-open by default if that policy is chosen.
2. Run /security-review-show. Confirm latest result opens.
3. Run /security-review --changed --panel. Confirm panel opens after completion.
4. Select a finding/prompt and press Enter. Confirm read-only source preview opens or clipboard fallback fires.
5. Try a missing/deleted file item. Confirm graceful error.
6. Try cancellation before completion. Confirm no stale panel opens later.
7. Run remote/socket --enrich path. Confirm unavailable note appears in panel.
```

## Done Criteria

This pass is complete when:

- completion behavior is explicitly defined and docs match runtime;
- optional panel auto-open is implemented or explicitly deferred;
- selecting a file-backed finding/prompt does something more useful than silently copying, preferably read-only source preview;
- root/path checks prevent reading outside the review root;
- latest receipt persistence is either implemented through existing metadata or explicitly deferred;
- panel details make prompts clearly non-finding review prompts;
- cancellation/stale-completion behavior remains safe;
- all behavior remains read-only, opt-in, bounded, and fail-soft.
