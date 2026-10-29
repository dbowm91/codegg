# M004 — Daemon LSP Workflows and Checked-Preview Parity: Closure

Status: **closed on branch**
Closed on: 2026-10-28
Branch: `plan/daemon-lsp-m003-m004-20261028`
Dependency: M003 closed at `plans/closure/daemon-lsp-frontend-convergence/003-status.md`

## Implementation and route inventory

The implementation is present in the branch worktree based on `dacf8d3c3`.
`LspWorkflowExecute` is the single bounded daemon route for root diagnosis and
eight session-rooted semantic recipes. Socket TUI commands and the desktop
palette use that route. The desktop editor host also returns inert format and
rename candidates; the renderer displays the candidate and requires an
explicit Apply action through `CoreRequest::LspPreviewApply`. No renderer-side
LSP process or second apply primitive was added.

The generated M007-D inventory is the complete per-form disposition. M004
serves `/lsp-root`, `/lsp-repair-local`, `/lsp-review-file`, `/lsp-impact`,
`/lsp-test-repair`, `/lsp-interface`, `/lsp-cross-repair`, and
`/lsp-call-neighbors` through bounded daemon requests. M003's status, server,
capability, error, restart, stop, cache, and diagnostics commands remain served
through their corresponding typed routes. `/lsp-previews`, `/lsp-preview`,
`/lsp-preview-clear`, `/lsp-preview-refresh`, `/lsp-preview-apply`,
`/lsp-doctor`, `/lsp-repair-hunk`, `/lsp-review-diff`, and
`/lsp-security-review` remain explicitly unserved in socket mode. The reasons
are session/turn-bound registry semantics, ambiguous hunk selectors, or lack
of bounded canonical review semantics. The desktop native format/rename
preview is not represented as a migration of those registry commands.

## Frontend behavior and authority

| Surface | Behavior | Authority |
|---|---|---|
| Socket TUI | `/lsp-*` recipe commands send `LspWorkflowExecute`; outputs are bounded summaries. Standalone mode keeps the pre-existing local tool path. | Session-derived workspace, `file.read`; lifecycle mutation stays under `lsp.control`. |
| Desktop palette | Uses the same workflow DTO and daemon response variant as TUI. | Same session-derived root and read capability. |
| Desktop editor | Format/rename produces a display-only candidate. Changes/session route/disconnect invalidate it; dirty documents cannot apply. | Explicit Apply reaches `LspPreviewApply` and the existing `file.modify`, writer-fence, workspace-lock, hash/digest and dirty-document checks. |

The fake-daemon trajectory reviews two different files from two authorized
client attachments while sharing one warm server; the transcript proves both
document-symbol requests reached that server. TUI route dispatch and desktop
host/renderer behavior are qualified in their respective focused tests. These
tests establish the shared typed contract and two-client server reuse; they do
not claim that a TUI window and a desktop window were simultaneously attached
to one live OS daemon process.

## Security and preview evidence

| Invariant | Evidence |
|---|---|
| File paths stay under the active session workspace | Fake daemon rejects traversal and an in-root symlink to an outside file before starting an LSP server. |
| Workflow reads remain bounded | Eight fixed recipes reuse existing `LspContextBudget`, provenance, truncation and structured server-capability outcomes; no caller root, argv, glob or arbitrary LSP command is accepted. |
| Server-proposed commands do not execute | `egglsp` code-action preview tests prove command-only and mixed command/edit actions yield no applicable preview. |
| Candidate size/truncation is visible | `egglsp::edit` bounds multi-file/edit/replacement previews and carries a structured `truncated` flag; desktop treats truncated/base-stale preview state as stale. |
| Stale base cannot be written | `stale_preview_is_rejected_before_any_write` leaves externally changed bytes intact. |
| Dirty managed document cannot be written | `dirty_document_service_rejects_preview_before_disk_write`. |
| Competing acceptance writes exactly once | `concurrent_acceptance_of_one_preview_writes_exactly_once`: one success, one stale refusal, final bytes contain one mutation. |
| Overlapping server edits are rejected | `egglsp::edit::reject_overlapping_edits` and bounded workspace-edit parsing tests. |
| User approval is explicit | `EditorSurface.test.tsx` proves preview generation is inert and Apply is a separate action; dirty state removes the apply action. |

## Verification

| Qualification | Result |
|---|---|
| `cargo test -p codegg --features server,lsp-test-support --test lsp_test_support` | 71 passed, 0 failed; includes shared two-client/two-file workflow, session-root containment, status/control/cache behavior, and fake protocol reads. |
| `cargo test -p codegg --lib concurrent_acceptance_of_one_preview_writes_exactly_once` | 1 passed; two concurrent acceptances produce one checked mutation. |
| `cargo test -p egglsp --lib cache::tests::cache_root_stats_and_clear_do_not_cross_workspace_boundaries` | 1 passed. |
| `cargo test -p egglsp --locked` | 1,038 passed across 5 suites, including code-action command refusal, bounded preview parsing and overlap handling. |
| Desktop Tauri host `cargo check` | Passed. |
| `CARGO_TARGET_X86_64_APPLE_DARWIN_LINKER=/tmp/codegg-lsp-test-linker/clang apps/desktop/scripts/verify-desktop.sh host` | Passed: 97 tests, 1 ignored (requires an external daemon fixture), desktop `cargo fmt --check`, and Clippy `-D warnings`. |
| Desktop renderer typecheck | Passed, including E2E TypeScript project. |
| Focused `EditorSurface` Vitest suite | 18/18 passed. |
| Authorization matrix, desktop panel/editor authority guards, comment budget, generated command classification `--check`, `git diff --check` | All passed. |

The host is x86_64 macOS; the fake-LSP runtime family used a compatible
x86_64 linker wrapper. The desktop local verification did not run the hosted
desktop CI lane, real language-server matrix, or a simultaneous TUI+GUI
window trajectory against one live daemon. No latency SLO or real-server
performance claim is made by this milestone.

## Decision

M004 is closed for the bounded user-command and editor-preview scope above.
Deferred preview-registry slash forms, doctor, hunk repair, diff review and
security review remain unavailable rather than silently approximated. Agent
LSP tool capability, local structural-selection syntax policy (M005), and
bounded process admission (M006) remain separate, ready work.

## Closure audit

Source plan: `plans/implementation/daemon-lsp-frontend-convergence/004-daemon-lsp-workflows-and-checked-previews.md`.
Source roadmap: `plans/subsystems/daemon-lsp-frontend-convergence-roadmap.md`.
Baseline reviewed: `dacf8d3c3` on `plan/daemon-lsp-m003-m004-20261028`; the
implementation and closure are currently the branch worktree changes.

### Invariants, recovery and compatibility

- Daemon workflow inputs are fixed recipes with session-derived roots and
  bounded context. Traversal and symlink escapes are refused before launch;
  unsupported or ambiguous server capabilities remain typed outcomes.
- Desktop preview generation is inert. Apply requires explicit user action
  and goes through `LspPreviewApply`; dirty, stale, truncated and conflicting
  previews are refused before an unreviewed write. Competing acceptance writes
  once, with the loser rejected as stale.
- Server-proposed commands are never executed as part of code-action preview.
  Old peers receive typed unsupported outcomes; there is no new persistence or
  migration contract and standalone TUI remains compatible.
- The fake daemon qualifies two authorized client attachments reviewing two
  files through one warm server. It does not qualify simultaneous TUI and GUI
  windows against one live OS daemon process. Hosted exact-head CI and installed
  real-language-server qualification were not run.

### Documentation, unresolved findings and registry

Architecture, authorization, generated command inventory, M003/M004 plans,
roadmap and registry references were reconciled. No unresolved product
findings remain within the bounded workflow and checked-preview scope. The
hosted/real-server and simultaneous-window checks above remain external
qualification gaps, not claims of completed evidence.

Registry disposition: M004 is closed; M007-F is closed by M003/M004; M005 and
M006 remain ready under their accepted decisions. The deferred command forms
remain explicitly unserved and require new bounded contracts before they can
be reconsidered.
