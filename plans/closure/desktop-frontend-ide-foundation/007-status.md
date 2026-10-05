# Desktop Frontend and IDE Foundation Milestone 006-E — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/desktop-frontend-ide-foundation/007-agent-change-review.md`

Source subsystem roadmap:

- `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md#m006-decomposition`

Repository baseline reviewed: `66d40ec1`

Implementation commits or pull requests:

- `cde7dbfe` — M006-E agent change review (PR `#96`, squash of `23badaf7` + `1e35c427`)
- `37245245519` — hosted `CI / verify`, success: all twelve guard, lint, and test steps green; workspace sweep `12250 passed, 7 skipped` in 405.196 s
- `37244728738` — hosted `CI / verify`, **failure** on `23badaf7`: two clippy errors (`enum_variant_names`, `manual_clamp`). Fixed in `1e35c427`; recorded here rather than omitted, because it is the only red run in this milestone's history and a reader diffing run ids would otherwise find it unexplained.

## 1. Executive finding

The milestone's capability boundary is complete: a pending agent change can be
reviewed in the TUI before it reaches disk, and the review is a *step in front
of* the apply that already existed rather than a second apply.

The central claim is that **"one apply path" is structural, not conventional.**
`change_review::build_apply_request` is the only function in the frontend that
turns a preview id into a `CoreRequest::LspPreviewApply`, and the pre-existing
`/lsp-preview-apply` command was refactored to route through it rather than
keeping a private copy. There is consequently no second constructor that could
drift, so the review adds a step and never a meaning.

The scope decision that made this possible without an ADR was recorded before
implementation: M006-E reviews changes destined for **saved** documents. The
apply-into-dirty-buffer question was deferred to a future ADR rather than
decided here, which is why `src/lsp/mutation.rs` is byte-identical in this
milestone's diff and its existing rejection is still correct as written.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| A review shows a pending change and waits | `opening_a_review_applies_nothing_and_waits_for_a_decision`; `B::Review` arm calls `change_review::open` | pass | Opening resolves the candidate and parses patches; no request is sent |
| Reject costs nothing | `rejecting_issues_no_request_and_leaves_nothing_behind`; `reject()` sends nothing | pass | Purely local state clear plus one toast |
| Exactly one apply path | `the_reviewed_change_and_the_applied_request_are_the_same_change`; legacy `B::LspPreviewApply` now calls `build_apply_request` | pass | See §5 for why the test asserts the *consequence* rather than comparing two calls to one function |
| Daemon keeps apply authority | `a_daemon_refusal_is_recorded_verbatim_and_keeps_the_review_readable`; frontend `ApplyRefusal` enum is preconditions only | pass | Frontend never judges legality |
| Dirty-buffer rejection unchanged | `git diff 66d40ec1..cde7dbfe -- src/lsp/mutation.rs` is empty | pass | See §8 |
| Daemon's message surfaced verbatim | Trajectory test quotes `DIRTY_BUFFER_REFUSAL` as a literal | pass | A change to that string is a visible diff in the test |
| Stale candidate refused before a review is built | `open()` returns before building on `refresh_preview_staleness` | pass | A diff that no longer matches the candidate is never shown as approvable |
| Already-applied candidate fails closed | `an_applied_candidate_is_refused_by_the_shared_builder` | pass | `PreviewUnknown`; no second request is constructible |
| Diff parsing bounded | `unified_diff.rs` bounds + 12 parser tests | pass | See §6 |
| Rendering reuses `DiffViewer` | `the_parsed_diff_feeds_the_existing_viewer_without_a_second_renderer` | pass | Public `hunks` replaced; no forked renderer |
| Multi-file review | `a_candidate_with_several_files_is_reviewed_file_by_file` | pass | `j`/`k` move between files |
| Late completion cannot apply a rejected change | `a_completion_after_a_reject_cannot_apply_anything` | pass | Request id and generation both checked |
| Command registry invariants | `built_in_command_count_matches_release_docs`, `command_docs_count_matches_registry`, `every_builtin_action_is_referenced_by_a_canonical_command` | pass | Count 152 → 153; doc table row added |
| Static guards unaffected | All `scripts/` absent from the diff | pass | `SCANNED_FILES` byte-identical by construction |
| No protocol/storage/migration/authorization change | Diff scope | pass | No `codegg-protocol`, `codegg-core`, or storage changes |
| Hosted CI green | `37245245519` | pass | 20/20 steps; 12250/12250 sweep |
| Built-app trajectory evidence | — | **not run** | See §10, low finding |

## 3. Production implementation evidence

Landed, in dependency order:

- `src/tui/unified_diff.rs` (new, 459 lines) — bounded unified-diff parser over
  `similar`. Emits `ParsedReview`/`ParsedPatch` and an explicit `Truncation`
  record naming which bound stopped the parse.
- `src/tui/app/state/change_review.rs` (new, 361 lines) — `ChangeReviewState`
  with `ReviewStatus` and `ReviewVerdict`. `Default` is *closed*, so the TUI
  surface is unchanged until a user opens a review. A refusal keeps the review
  open so the message stays readable next to the diff it refused.
- `src/tui/components/change_review.rs` (new, 380 lines) — `ChangeReviewView`.
  Renders refusal, stale warning, per-file list with an active marker, and
  truncation/malformed notices; then delegates the diff itself to `DiffViewer`.
- `src/tui/commands/change_review.rs` (new, 544 lines) — `build_apply_request`
  (the single request constructor), `open`, `accept`, `reject`,
  `apply_completion`, `handle_key`, `is_active`, and the `ApplyRefusal` enum.
- `src/tui/app/mod.rs` — `App.change_review_state` (two `Default` inits), the
  `B::Review` dispatch arm, the key pre-dispatch, and the legacy
  `B::LspPreviewApply` refactor.
- `src/tui/app/commands.rs`, `src/tui/runtime/command_dispatch.rs` —
  `TuiCommand::ChangeReviewAccepted` and its dispatch arm.
- `src/tui/command.rs`, `architecture/command.md` — `BuiltinSlashAction::Review`,
  the `/review` registry entry, `all_builtin_variants()`, count 152 → 153.
- `src/tui/app/render.rs` — `render_viewport` shows the review when open.
- `src/tool/lsp.rs` — one `#[cfg(test)]` test-support registrar
  (`register_preview_artifact_for_test`), 17 lines. See §10.

**Implemented vs. absent.** The review surface, its state machine, the bounded
parser, and the single-builder wiring are implemented. The *dirty-buffer merge*
capability is deliberately absent and is not a gap in this milestone — it is
the deferred ADR, recorded in the plan, the registry, the roadmap, and the M006
presentation audit.

## 4. Verification executed

### Commands run

```bash
cargo fmt --all
CARGO_BUILD_JOBS=2 cargo test --lib tui::                      # 1117 passed
CARGO_BUILD_JOBS=2 cargo test --test change_review_trajectory  # 7 passed
CARGO_BUILD_JOBS=2 cargo clippy --workspace --all-targets --locked -- -D warnings
python3 scripts/check_tui_editor_text_authority.py
python3 scripts/check_tui_project_authority.py
python3 scripts/check_execution_ownership.py
scripts/verify.sh quick                                        # exit 0
```

### Results

All local commands passed. `verify.sh quick` reported every guard green,
including `codegg-core boundary`, `sandbox contract`, `HTTP route disposition`,
`audit coverage`, `scheduler bypass`, and the three TUI guards, ending with
`Quick verification passed.`

Hosted `CI / verify` run `37245245519`: all twelve guard, lint, and test steps
green — agent schema, core boundary, sandbox, execution ownership, both TUI
guards, HTTP route disposition, audit coverage, scheduler bypass, fmt, clippy,
and the test sweep;
`Workspace tests` completed `12250 tests run: 12250 passed, 7 skipped` in
405.196 s.

**One red run, recorded rather than omitted.** Run `37244728738` on the first
push (`23badaf7`) failed `Workspace Clippy` with two errors:
`enum_variant_names` on `ApplyRefusal` (every variant shared a prefix) and
`manual_clamp` on the refusal-notice height. Both were fixed in `1e35c427` and
the next run passed. The first fix attempt renamed `No*` to `Missing*` and
failed the *same* lint, which is itself the useful detail: the problem was
never the particular prefix. The final names — `SessionUnavailable`,
`WorkspaceUnavailable`, `CoreClientUnavailable`, `PreviewUnknown` — are distinct
by construction, and `PreviewUnknown` is deliberately not "unavailable"
because an already-applied id is a different fact from an unreachable client.

**Not run, stated plainly:** no Desktop E2E / built-app trajectory evidence
exists for this milestone. The `verify` job does not trigger the E2E workflow,
and this change is a TUI-only surface that the existing E2E specs do not
exercise. Recorded as a low finding in §10 rather than presented as covered.

**On hosted CI for the closure PR itself:** this closure changes only
`plans/**`, which `.github/workflows/ci.yml` lists under `paths-ignore`, so the
`verify` workflow correctly does not trigger for it. That is the repo's own CI
economy policy, not a gap — the workflow asserts that docs-only changes cannot
break code or guards. To avoid leaning on that assertion, the guard that
actually reads a `plans/` file (`check_projection_transport_lifecycle.py`) was
run locally against this change. It fails, and the failure is pre-existing:
the same error reproduces at `cde7dbfe` on a clean worktree. See §10.

## 5. Invariant review

- **One apply path.** Structural: one constructor, two call sites, both
  funnelling through it. Note what the test does *not* claim — comparing two
  invocations of `build_apply_request` would be true by construction and prove
  nothing. The assertion is on the consequence that actually protects the user:
  a test stages a real candidate in a real `LspTool` registry, resolves the real
  digest-bound request, opens the review over it, and asserts the review's hunks
  are a parse of *that request's own patch text* under the same digest, revision,
  and base hash. If a future change re-derived the displayed diff from disk, or
  opened the review from a different source than the apply, the user would be
  approving something other than what lands, and the test fails.
- **The daemon keeps apply authority.** The frontend decides preconditions only
  (`ApplyRefusal`); legality is the daemon's judgement and its message is
  stored verbatim. `a_frontend_refusal_is_distinct_from_a_daemon_refusal` pins
  the distinction so the UI cannot imply a change was rejected on its merits
  when it never reached the daemon.
- **Non-modal composer contract intact.** `is_active` gates the pre-dispatch
  before `handle_key` is consulted, and it requires *open and focused*. With no
  review open — the `Default` — the input path is byte-for-byte unchanged. The
  review claims `a`, `Esc`, `j`/`k` and returns `false` for everything else.
- **No new text authority.** The review reads LSP patch text from a transport
  DTO; it does not read documents or files. `check_tui_editor_text_authority.py`
  is green with a byte-identical `SCANNED_FILES` list, and the whole `scripts/`
  tree is absent from this milestone's diff.
- **Frozen constants untouched.** The 5.0 ms causal budgets
  (`CAUSAL_OBSERVE_P95_BUDGET_MS`, `CAUSAL_ACTIVE_P95_BUDGET_MS`) are not
  referenced or changed by this milestone.

## 6. Failure and recovery review

- **Stale generation.** Covered: a candidate whose base no longer hashes to its
  registered original is refused before a review is built.
- **Spent candidate.** Covered: `mark_applied` makes the shared builder return
  `PreviewUnknown`, so neither the review nor `/lsp-preview-apply` can produce a
  second request for the same id.
- **Cancellation / late completion.** Covered: a completion carries the request
  id *and* the review generation; one arriving after a reject is discarded
  rather than applied, and the stale request is cancelled.
- **Malformed input.** Covered: the parser bounds patches (64), patch bytes
  (512 KiB), hunks per patch (256), and lines per hunk (2000), recording which
  bound stopped it so the view can show the diff is partial rather than
  presenting a partial diff as complete. Byte cuts land on UTF-8 character
  boundaries. An empty change (`"(no changes)"`) is treated as known-empty, not
  malformed — a real no-op must not read as a parse failure.
- **Rejection during in-flight apply.** The review stays open on a daemon
  refusal rather than closing, so the user can read the reason next to the diff.
- **Not applicable here:** duplicate delivery and idempotency, daemon restart,
  and partial persistence failure. The milestone adds no storage and no
  daemon-owned execution path; it adds a frontend review step in front of an
  apply whose idempotency properties are unchanged.

## 7. Migration and compatibility review

None. No schema migration, no protocol change, no configuration surface, no
negotiation. The native protocol is untouched: `CoreRequest::LspPreviewApply`
and its result DTO are consumed exactly as the legacy command already consumed
them. Rollback is a revert; the only shared code path a revert would unhook is
`build_apply_request`, which the legacy command calls by name.

## 8. Security review

- **Authorization.** Unchanged and not re-implemented. The review resolves a
  candidate through the existing `preview_apply_request` export, which is the
  same digest-bound path `/lsp-preview-apply` used. The daemon performs the
  real authorization on apply.
- **Path validation.** Not applicable: this milestone introduces no path
  resolution. Paths are displayed from a daemon-produced DTO and are never used
  to read or write anything by the frontend.
- **Privilege boundary.** The frontend cannot bypass a daemon precondition. The
  review surface can only *ask*; the daemon decides, and the answer is shown
  verbatim. `open()` even refuses a stale candidate locally, which is a
  usability guard, not a security control — stated so it is not mistaken for one.
- **Denial-of-service bounds.** The parser bounds are the relevant control and
  are tested. The refusal-notice height is clamped (1–4 lines + 1) so a long
  daemon message cannot push the diff off the viewport; the clamp is commented
  as load-bearing so it is not later "simplified" back into a raw count.
- **Secrets.** None handled. The review shows patch text that the daemon
  already returned to this session.
- **Audit.** No new audit surface; the apply it fronts is unchanged.

## 9. Documentation and operations

- `architecture/tui.md` — new "Agent change review (M006-E)" section covering
  the saved-documents-only scope, the structural one-apply-path argument, the
  verbatim-refusal contract, fail-closed staleness, parser bounds, `DiffViewer`
  reuse, and why this is the one modal surface in M006.
- `architecture/document.md` — records that `/review` sits in front of the
  existing apply without weakening the dirty-buffer rejection, and that the
  merge-into-dirty-buffer question is deferred to a future ADR.
- `architecture/command.md` — count 152 → 153 in all five places the docs test
  parses, plus the `/review` table row.
- `plans/registry.md`, the subsystem roadmap, and the M006 presentation audit —
  M006-E registered as planned/closed, the "M006-B/M006-E need ADRs" claim
  corrected to M006-B alone, and the deferred dirty-buffer ADR recorded.

**No new static guard.** The invariants that matter here are structural — the
review calls the same function the legacy command calls, readable in one place.
A regex guard over that would be weaker than reading the call site, and the
M006-A text-authority guard is left untouched rather than widened.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | No Desktop E2E / built-app trajectory evidence. The `verify` job does not trigger the E2E workflow, and the existing E2E specs do not exercise a modal review. | The modal takeover and accept/reject keys are covered by unit and integration tests only, not by a real terminal. | Optional. If M006 gains more modal surfaces, add a spec that opens a review and rejects it. |
| low | `ApplyRefusal` messages and the review's on-screen text are English literals with no i18n seam, consistent with the rest of the TUI. | Localized builds would show untranslated review text. | None for this milestone; matches existing TUI practice. |
| low | The test fixture recomputes SHA-256 rather than importing `egglsp::edit::sha256_hex`, which is `pub(crate)`. | If the registry's hash algorithm ever changes, the fixture fails loudly at the staleness check rather than silently staging a fresh-looking candidate. | None — the failure mode is the safe one, and it is commented in the test. |
| low | **Pre-existing, not caused by this milestone:** `scripts/check_projection_transport_lifecycle.py` fails on `main` with `daemon_socket.rs: raw forwarder is spawned without an owned handle`. | A change-triggered guard is red on the baseline. It is not in the `verify.sh quick` subset and this milestone is not a projection-transport change, so hosted CI is unaffected. | Separate corrective. Verified pre-existing, not inferred: the same error reproduces at `cde7dbfe` on a clean worktree, and this milestone's diff is three `plans/**` files, none of which that guard reads. |
| medium | The apply-into-dirty-buffer capability is absent by decision. | A user who edits a file after generating a candidate must save and regenerate; the review surfaces that refusal. | Deferred ADR. Deliberately out of scope here; recorded in the plan, registry, roadmap, and audit. |

No critical or high findings. No known defect blocks merge.

## 11. Roadmap disposition

**Milestone closed and the next dependency may proceed.**

M006 is now decomposed into three closed/shipped slices and two remaining:
M006-A (closed, `94f38421`), M006-D (closed, `de29e49e`), and M006-E (closed,
`cde7dbfe`). The scoped-out dirty-buffer half is not a loose end inside M006-E;
it is a named future ADR, recorded in four places so it cannot be lost.

**M006-B is now the only thing gating the rest of M006.** It requires an ADR
for the LSP read surface and its delivery/authorization path, because the
native protocol has *no* LSP read operation at all — the only LSP variant is
the `LspPreviewApply` write. M006-C depends on M006-B. Graphical/Monaco IDE work
remains long-term.

## 12. Registry updates

`plans/registry.md`:

- Roadmap row: "M006-E ready to scope" → M006-E planned/closed; the claim
  "M006-B/M006-E need ADRs" corrected to M006-B alone, with the deferred
  dirty-buffer ADR and the unchanged rejection recorded.
- Milestone table: M006-E row added as **closed**, pointing at this record, the
  plan, implementation `cde7dbfe` (PR `#96`), and hosted run `37245245519`,
  with the red run `37244728738` and its clippy cause noted.

`plans/subsystems/desktop-frontend-ide-foundation-roadmap.md`:

- M006-E decomposition row: scoped to saved documents, marked **planned**, no
  ADR, dirty-buffer half deferred.
- Summary row: M006-E plan path added; "ready to scope" → planned/closed.

`plans/subsystems/desktop-frontend-ide-foundation-m006-tui-presentation-audit.md`:

- M006-E decomposition bullet and outstanding-item 6 updated to record that the
  plan exists, that saved-documents-only removes the ADR requirement, and that
  the dirty-buffer decision is deferred rather than taken. The M006-B ADR
  remains outstanding and is renumbered as item 7.

No accepted closure record, ADR, or canonical long-term document
(`plans/000-*`, `001-*`, `002-*`) was edited by this milestone. The M006-A and
M006-D closure records remain immutable historical evidence.
