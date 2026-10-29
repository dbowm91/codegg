# M004 — Daemon LSP Composite Workflow and Checked-Preview Parity (M007-F Part 2)

Status: closed — 2026-10-28; M003 is closed and the bounded workflow/preview routes are qualified

Primary class: capability.

Repository planning baseline: `7de6f237c0c4ab2abff120f39b7ae98e7107bec3` (`main`, 2026-10-28). Roadmap: `plans/subsystems/daemon-lsp-frontend-convergence-roadmap.md`; governance: `plans/003-planning-process.md`; long-term specification §§4,13,24–25.

Binding: ADR-0008, ADR-0010, ADR-0011, ADR-0012, ADR-0018, ADR-0021, ADR-0052, and accepted ADR-0056 Option B. M003 closure is recorded at `plans/closure/daemon-lsp-frontend-convergence/003-status.md`. This implements the *remaining composite* M007-F forms after M003, not a second LSP execution authority.

## Objective and existing evidence

Make the remaining useful `/lsp-*` repair, review, semantic-context, impact and preview workflows callable through the same daemon-owned LSP service and command catalog from socket TUI and desktop. `egglsp::workflow_recipes`, `src/tool/lsp.rs`, `src/tool/lsp_read.rs`, `src/core/daemon_lsp.rs`, `src/tool/lsp_preview_apply.rs` and `src/tui/commands/change_review.rs` already provide parts of this. The typed `LspWorkflowExecute` route now serves root diagnosis plus eight file-scoped recipes from the session workspace; socket TUI commands and the desktop palette share that daemon route. Native editor format/rename previews remain point reads, and the desktop host now returns inert candidates and applies only through the existing authorized `CoreRequest::LspPreviewApply`. This milestone closes **user-command transport parity** for the implemented forms, not agent tool capability or preview-registry slash-command parity.

## Scope / ordered work

1. Begin with M003's current-head form inventory. Classify every outstanding command by real semantic primitive or composite recipe, arguments, input root/attachment, output size, authorization, whether it returns inert evidence or proposes a checked mutation, and exactly what it does in standalone mode. Reject false `daemon_served` announcements.
2. Move representative `semanticContext`/repair/review recipe invocations behind daemon-owned typed bounded requests built from existing `egglsp` operations and approved capability. Use session-bound workspace resolution; no caller-specified root, process argv or unconstrained file glob. Reuse `LspContextBudget`, path containment, optional section truncation and provenance; keep full-context model packets separate from concise human summaries.
3. Add the remaining forms incrementally in one coherent low-frequency command catalogue path, or mark unavailable with reason when there is no safe mapping. Do not expose raw server workspace edit, `workspace/executeCommand`, arbitrary code actions or unbounded hunk scans. Distinguish no server, unsupported capability, ambiguous server, denied file and failed server.
4. For human rename/format/code-action proposals, send only **inert preview candidates** to a review surface. For accepted work, call the canonical `CoreRequest::LspPreviewApply` with `file.modify`, workspace lock, original hash/digest checks and dirty-buffer rejection; do not apply automatically when the response arrives. Preserve model-facing preview registry turn lifetime and independent agent write fence.
5. Consume in both frontends through `codegg-client` and shared catalogue: TUI command gestures, desktop palette and GUI review display must have the same authoritative outcome even if UIs differ. Non-socket legacy path remains intentionally compatible. The desktop format/rename controls show an inert diff and require an explicit Apply action; edits, route/session changes and disconnect clear the candidate, and dirty documents cannot apply. Update generated classification, docs and any static command-route guard.

## Non-goals and semantics

No new file mutation primitive, persistent preview registry, silent multi-file autoformat, RPC passthrough, arbitrary shell commands, model untrusted tool upgrade, new language-server process, local cache parity, or use of `CommandExecute` for per-keystroke operations. Preview registry list/show/clear/refresh/apply slash forms, `/lsp-doctor`, `/lsp-repair-hunk`, `/lsp-review-diff`, and `/lsp-security-review` remain unavailable: the former require registry/session-turn binding, hunk selectors have ambiguous legacy argument semantics, and diff/security workflows lack bounded canonical source semantics. The native format/rename editor preview is distinct from those registry commands. Separate user authorization from model tool permissions and prevent information leakage from project-wide semantic packets.

## Required verification

Production fake-LSP protocol trajectory with two files and two frontends; mixed authorized/unauthorized references, symlink and traversal refusal, stale hash, edited dirty buffer, overlapping preview, server-supplied commands, truncated candidate, restart midway and two clients racing acceptance. Ensure unchanged disk on failed apply and exactly one checked mutation on explicit acceptance. Include `cargo test -p egglsp --locked`, `cargo test -p codegg --locked lsp`, `cargo check -p codegg --locked --features server`, `python3 scripts/check_authorization_matrix.py`, classification generator `--check`, change-review static guard and desktop hosted gate; annotate unrun platform/real-server tests honestly.

## Closure

The closure record is `plans/closure/daemon-lsp-frontend-convergence/004-status.md`.
It contains the current form-by-form route classification, TUI/GUI comparison,
authorization evidence, preview-race evidence, results and deferred forms. No
new authority or persistence contract was introduced.
