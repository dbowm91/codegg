# TUI editor Helix parity Milestone 016 — the bridge: a reviewable preview

Status: implemented - closed 2026-10-19
Repository baseline: `bb6642688` (`feat/auto-20261018-12255c45`)

Source roadmap: `plans/subsystems/tui-editor-helix-parity-roadmap.md` §7
(a new milestone added by the M016-planning amendment; see the roadmap's
Amendment A3) and §13 (the residual the M015 closure registered)
Long-term requirements: `plans/000-long-term-specification.md` (IDE-grade
language intelligence); `plans/002-long-term-roadmap.md` (editor capability
sequencing)
Applicable ADRs: `plans/adrs/ADR-0008-lsp-preview-apply-authorization-and-runtime-ownership.md`
(accepted 2026-09-20 — owns the LSP write path this milestone finally
reaches); `plans/adrs/ADR-0011-editor-document-ownership-and-frontend-replication.md`
(the editor replicates the document; it does not own the file);
`plans/adrs/ADR-0018-lsp-workspace-relative-navigation-identity.md`
(the candidate's paths are **workspace-relative**, which is ADR-0018's
identity, not this milestone's invention)
Primary class: capability

## 1. Objective

Turn the editor's LSP **preview** into a **reviewable artifact** that ADR-0008's
apply path can consume, so that `=` and `space r` stop only *showing* what
would change and start *doing* it.

This is the transition M015 registered in three places and deliberately did not
land:

> When the bridge is built — when the editor's preview becomes a reviewable
> artifact ADR-0008's apply path can consume — `=` and `space r` stop showing
> what *would* change and start doing it.
> (`plans/closure/tui-editor-helix-parity/015-status.md` §3.5)

One bounded outcome: **the editor's format and rename previews become
applicable, reviewed edits.**

## 2. Why this milestone is ready

- **ADR-0008 is accepted and owns the write path.** `CoreRequest::LspPreviewApply`
  is authorized `via_session` + `file.modify`
  (`crates/codegg-core/src/authorization/policy.rs:827-831`) and applies a
  digest-bound candidate through `src/lsp/mutation.rs::apply_preview()`. No new
  ADR is needed and none is written.
- **The apply path needs no registry.** `apply_preview` verifies a
  **self-certifying digest** recomputed over the request's own patches and
  revalidates every `original_hash` against the file on disk
  (`src/lsp/mutation.rs:380-436`). It never resolves a preview id against a
  registry. So a candidate that travels in the request is as good as one that
  was looked up — and the transport DTO is already built for exactly this.
- **The material already exists and is thrown away.** The rename preview's
  per-file `patch` and `original_hash` are computed by `egglsp` and dropped at
  `src/core/daemon_lsp.rs:664-667`. The format preview already carries its
  `diff` and `before_hash`.
- **The review surface already exists.** `src/tui/commands/change_review.rs`
  is "a second *step* in front of an apply that already exists", and
  `build_apply_request` is "the single place a `CoreRequest::LspPreviewApply`
  is constructed". The bridge reuses both; it does not add an apply path.

## 3. Current implementation evidence

### 3.1 The editor's preview is a read result, and nothing more

| Check | Result |
|---|---|
| `grep` for `register_preview_artifact` / `PreviewArtifact` / `LspPreviewApply` in `src/tui/commands/editor.rs`, `src/tui/editor.rs`, `src/tui/editor_lsp.rs`, `src/tui/components/editor.rs` | **no hits** |
| `grep` for `change_review` in `src/tui/commands/editor.rs` | **no hits** |
| The chords' request | `CoreRequest::LspReadGet` — the **read** capability (`policy.rs:841-845`) |
| `LspRenamePreviewDto` | `files: Vec<LspRenameFileDto>` — `path` + `edit_count` only; **no patch, no hash** (`crates/codegg-protocol/src/lsp.rs:447-452`) |
| `LspFormatPreviewDto` | `diff` + `before_hash`, but **no per-file patch list** (`crates/codegg-protocol/src/lsp.rs:430-443`) |

### 3.2 The two registries are disjoint, and one of them does not exist in production

| Registry | Owner | Reachable? |
|---|---|---|
| the model's | `ToolRegistryOptions::lsp_preview_registry` → `src/tool/mod.rs:1051-1057` | model only |
| the TUI's | `app.lsp_tool`'s own private registry (`src/main.rs:3454-3463`) | **`None` in socket/remote mode**, and empty in production: nothing in the TUI ever calls a preview-producing operation on it |

`LspReadGet` registers nothing (`src/core/daemon_lsp.rs` contains no
`register`/`preview_registry` call), and there is **no** protocol request that
lists or exports a preview. So "register the editor's preview so the existing
apply path can find it" is **not available**: in socket mode there is no
registry to register into, and in local mode the TUI's registry is a different
object from the model's.

### 3.3 The apply path's contract, read from the code

`apply_preview` (`src/lsp/mutation.rs:322-537`), in order:

| # | Check | Failure |
|---|---|---|
| 1 | `validate_request_shape` — kind ∈ {rename, code_action, formatting}; 1..=100 patches; digest is 64 chars; revision ≠ 0 | `Invalid` |
| 2 | every patch path passes `validate_path` under the canonical root | `Invalid` |
| 3 | no duplicate affected paths | `Invalid` |
| 4 | `documents.lock_clean_paths(...)` — the managed buffer must not be dirty | `Stale` |
| 5 | **digest recomputed** over the request's own patches and compared | `Invalid` |
| 6 | `service.is_managed_document_dirty(path)` | `Stale` |
| 7 | every file present, and `original_hash` matches the file on disk | `Stale` |
| 8 | `apply_unified_diff` per file, atomic writes, rollback | `Apply` |
| 9 | checkpoint persisted, `FileChanged` published, LSP re-synchronized | `Apply` |

**The digest is the binding.** `preview_digest_for_candidate`
(`crates/egglsp/src/preview_registry.rs:320-339`) hashes
`{kind, title, file_edits, original_hashes, provenance, patches}`. Because the
daemon computes it over the same material the request carries, **a candidate
that has been altered in transit fails the check** — the TUI cannot change a
patch and keep the digest.

## 4. Invariants that must not regress

1. **One apply path.** `CoreRequest::LspPreviewApply` is constructed in exactly
   one place. The bridge widens *what a candidate can be*, never *how many
   applies exist*.
2. **A preview that cannot be applied is not offered as one.** A truncated
   preview, a preview whose patch was omitted, a preview with no edits, or a
   file outside the workspace yields **no candidate** — the reader gets the
   preview and the reason, never a button that fails.
3. **The daemon still decides.** The frontend never decides that an apply is
   legal. It shows the diff and sends the request; the daemon's refusal is
   recorded verbatim in the review.
4. **The editor never writes the file itself.** The apply is the daemon's
   checked path, with its checkpoint, its hash revalidation, its rollback and
   its LSP re-synchronization. The editor's buffer is *reloaded* afterwards.
5. **The dirty-buffer refusal stands.** `apply_preview` refuses to apply over
   a dirty buffer, and the editor is the surface that makes buffers dirty. The
   refusal is the correct behavior and this milestone does not change it (it is
   the deferred ADR `src/tui/commands/change_review.rs` already names).
6. **A relative path stays relative.** The candidate's paths are
   workspace-relative, exactly like the preview DTOs they sit beside, and
   `validate_path` joins them to the canonical root.

## 5. Scope

**In:**

- the protocol's candidate DTO and its bounds,
- the daemon minting a candidate for the two preview reads,
- the editor carrying the candidate and opening the review on it,
- the one apply path resolving a carried candidate,
- the editor's buffer being reloaded after a successful apply,
- the preview panel's wording, which changes with the meaning,
- the docs, the CHANGELOG and the skill files that describe the chords.

**Out:**

- **A code-action preview** (`space a` still states its reason). That is M017.
- **The dirty-buffer merge.** Named, deferred, and unchanged.
- **A new ADR.** ADR-0008 already owns this path.
- **A preview registry.** The candidate travels in the read result; nothing new
  is registered anywhere.
- **Changing the model's `lsp_preview_apply` tool** or the `LspTool`'s
  registry. They are a different path and they keep working.

## 6. Required production changes

| Area | Change |
|---|---|
| protocol | `LspPreviewCandidateDto`; `candidate` on the two preview payloads; bounds and a `validate()` |
| daemon | mint the candidate for both previews; mint the id and revision; correct the stale "no accepted ADR" comment at `daemon_lsp.rs:622-624` |
| TUI state | the editor carries the candidate with the scope it was read under |
| TUI commands | the preview's arrival opens the review; the review resolves a carried candidate; a successful apply reloads the open buffer |
| components | the panel's wording: the preview is no longer "not applied" — it is a candidate under review, or a preview with a stated reason why it cannot be applied |
| docs | `docs/tui.md`, `docs/LSP.md`, `architecture/lsp.md`, `architecture/tui.md`, `CHANGELOG.md`, `.opencode/skills/lsp-ide/SKILL.md`, `.opencode/skills/tui/SKILL.md` |
| guards | a new guard: the editor path constructs **no** apply request outside the one builder |

## 7. Ordered work packages

### W1 — The protocol's candidate

`LspPreviewCandidateDto { preview_id, preview_revision, preview_digest, kind,
title, provenance, patches }`, carried by `LspFormatPreviewDto` and
`LspRenamePreviewDto` as `candidate: Option<..>`.

Bounds: at most `MAX_LSP_PREVIEW_CANDIDATE_PATCHES` (100, matching
`apply_preview`), at most `MAX_LSP_PREVIEW_CANDIDATE_BYTES` of patch text in
total, a 64-char digest, a non-empty id, a non-zero revision, and a kind in
{rename, code_action, formatting}. A `validate()` that says *why* a candidate
is unusable, so the TUI refuses a malformed one before it builds a request.

*Acceptance:* the DTO round-trips; `validate()` accepts a well-formed
candidate and rejects each malformed field with its own reason.

### W2 — The daemon mints the candidate

For the **format** preview: one patch — the preview's `diff` and
`before_hash` — kind `formatting`.

For the **rename** preview: one patch per contained file — the file's `patch`
and `original_hash` — kind `rename`. A file whose patch was omitted
(`patch_omitted`) is **dropped**, and if that leaves nothing, no candidate is
minted.

Neither is minted when the preview is `truncated`, when the diff is empty, or
when the file set is empty.

The digest is `preview_digest_for_candidate` over the *same* path strings, the
same hashes and the same patch text the request will carry. The id and the
revision are minted by the daemon (non-empty, non-zero).

*Acceptance:* a daemon test shows the minted candidate's digest equals
`preview_digest_for_candidate` recomputed from the candidate's own fields, and
that a truncated preview mints none.

### W3 — The editor carries the candidate

`EditorLspState` holds the candidate with the scope it was read under, so a
candidate from another document can never be offered against this one. It is
cleared when a new read starts and when the scope changes.

*Acceptance:* a candidate is stored only for the scope it was read under, and
is dropped by the next read.

### W4 — One apply path, two resolutions

`build_apply_request` takes a *source*: the frontend registry (as today) or a
carried candidate. Both produce the same `CoreRequest::LspPreviewApply` from
the same construction site. The carried candidate's `workspace_id` and
`session_id` come from the active tab **at accept time**, exactly as the
registry path's do.

*Acceptance:* the existing "the reviewed change and the applied request are
the same change" test still holds for the registry path, and a new one proves
it for the carried path: the diff on screen is a parse of the request's own
patch text.

### W5 — The chord leads to the review; the apply reloads the buffer

The preview's arrival opens the change-review surface on the candidate. The
review is the apply step: `[a]pply` sends the request, `Esc` rejects.

After a **successful** apply, the editor's open document is reloaded from disk
(`reload_editor`), because the apply changed the file behind the buffer. A
refused apply changes nothing and reloads nothing.

*Acceptance:* the review opens on the candidate; an accept sends exactly the
request the review displayed; a successful apply triggers the reload; a
refusal does not.

### W6 — The wording, the docs and the guard

The panel's "not applied" is no longer true — the candidate is *under review*.
The panel says which it is: a candidate that is ready for review, or a preview
that cannot be applied and why.

A new guard asserts that `src/tui/` constructs `CoreRequest::LspPreviewApply`
in exactly one place (`change_review.rs`), so the one-apply-path invariant is
structural rather than a convention. The guard proves its own non-vacuity by
injection, like the guards M009–M015 added.

*Acceptance:* the guard fails on an injected second construction site and
passes on the real tree.

## 8. Failure, cancellation, restart, contention semantics

- **A refused apply** is recorded in the review verbatim; nothing is written
  and nothing is reloaded.
- **A stale candidate** (the file changed since the preview) is refused by the
  daemon's `original_hash` revalidation. The review shows the refusal.
- **A cancelled review** (`Esc`) sends nothing.
- **A daemon that is gone** produces the existing `CoreClientUnavailable`
  refusal.
- **A scope change while a candidate is held** drops the candidate.
- **Contention:** the daemon holds the workspace lease for the apply, exactly
  as every other checked mutation does.

## 9. Compatibility and migration

- The protocol change is **additive**: `candidate` is `Option`, and
  `#[serde(default)]` keeps an older peer readable. A peer that does not know
  the field ignores it.
- No storage migration. No new durable state: the candidate lives in the read
  result and in the editor's memory.
- No new capability flag.

## 10. Required tests

| Area | Test |
|---|---|
| protocol | the candidate's `validate()` accepts a well-formed candidate and rejects each malformed field |
| daemon | the minted candidate's digest equals the digest recomputed from the candidate's own fields |
| daemon | a truncated preview mints no candidate; a rename whose only file had an omitted patch mints none |
| daemon | the candidate's paths are workspace-relative and contained |
| TUI | the editor carries a candidate only for the scope it was read under |
| TUI | the carried candidate opens a review whose diff is a parse of the request's own patch text |
| TUI | the accept path sends the request the review displayed, with the active tab's session and workspace |
| TUI | a successful apply reloads the open buffer; a refusal does not |
| guard | the one-apply-path guard rejects an injected second construction site |

## 11. Required verification commands

```bash
export CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo build --locked --bin codegg-sandbox-helper
cargo test -p codegg --lib --locked --no-fail-fast
cargo test -p codegg --lib --locked tui::
cargo test -p codegg --lib --locked tui::commands::editor::tests::
cargo test -p codegg --lib --locked tui::commands::change_review::tests::
cargo test -p codegg --features lsp-test-support --locked \
  --test lsp_read_delivery_m013 --test lsp_read_surface_trajectory \
  --test lsp_completion_trajectory
python3 scripts/check_tui_editor_key_authority.py
python3 scripts/check_tui_editor_leaf_authority.py
python3 scripts/check_tui_editor_leaf_authority.py --self-test
python3 scripts/check_tui_editor_prefix_state.py
python3 scripts/check_tui_editor_text_authority.py
python3 scripts/check_tui_editor_apply_path.py
bash scripts/verify.sh quick
```

`CARGO_INCREMENTAL=0` is required in this worktree: the incremental cache is
unreliable here (`dep-graph.part.bin` went missing twice during the M011–M015
work).

## 12. Documentation updates

- `docs/tui.md` — the two chords' meaning; the review step; the refusal.
- `docs/LSP.md` — "Previews change nothing" is no longer true; the preview is
  now a candidate and the review is the apply.
- `architecture/lsp.md` — the bridge's shape; the carried candidate; the
  one-apply-path invariant.
- `architecture/tui.md` — the chords' keymap rows and the editor's place in
  the apply.
- `CHANGELOG.md` — the meaning change under `### Changed`, as M015 registered.
- `.opencode/skills/lsp-ide/SKILL.md` and `.opencode/skills/tui/SKILL.md` —
  the two module guides, in the **same commit** as the code they describe.

## 13. Acceptance criteria

1. Pressing `=` on a formattable document produces a **reviewable candidate**,
   and the review shows the exact diff that would be written.
2. Pressing `space r`, then typing a name, produces the same for a rename,
   across every file the rename touches.
3. Accepting the review sends **one** `CoreRequest::LspPreviewApply` and the
   daemon writes the change through its checked path.
4. The candidate that was reviewed is the candidate that was applied — proven
   by the request's own patch text, not by a comparison of two derivations.
5. A preview that cannot be applied offers no candidate and states why.
6. A refused apply changes nothing on disk and says so in the review.
7. After a successful apply the editor's buffer shows the new content.
8. The editor path constructs no apply request outside the one builder.

## 14. Stop conditions

Report rather than improvise if:

- the daemon cannot compute a digest that `apply_preview` accepts (the two
  sides disagree about the digest's inputs),
- the apply would have to touch a file outside the session's workspace,
- the only way to make the apply succeed is to weaken the dirty-buffer
  refusal,
- the review surface would need a second apply path to carry a carried
  candidate,
- the buffer reload after an apply would discard unsaved work.

## 15. Closure evidence required

- the digest equality test, in both directions,
- the carried-candidate review test,
- the reload-after-apply test,
- the guard's injection proof,
- the sweep in §11 against the final tree,
- the diff between M015's wording and this milestone's, in every surface that
  described the chords.

## 16. Handoff notes

- The candidate travels in the **read result**. Nothing is registered; there
  is no new registry and no new daemon state. This is what makes the bridge
  work in socket/remote mode, where `app.lsp_tool` is `None`.
- The preview **keeps its meaning as a preview** when it cannot become a
  candidate. A truncated preview is still a preview.
- The chords do not move. Only what they *do* changes, which is why the
  change is recorded in six places.
- M017 builds on this: a code-action read mints a candidate of kind
  `code_action` through the same path.

## 17. Recorded deviations

The plan is a forecast; the code is the record. Every place the two differ is
recorded here rather than left implicit.

**17.1 — The reload needed a path list the plan did not name.** §7's W5 said
the editor reloads "the open document" after a successful apply, but did not
say how the editor learns that the apply wrote *its* document. The review's
parsed patch list is bounded at `MAX_REVIEW_PATCHES` (64) while a candidate may
carry 100 patches, so the parsed list cannot answer it. The implementation
added `ChangeReviewState.affected_paths` — the **candidate's** full path list,
captured when the review opens — and the reload compares the editor's
workspace-relative path against it.

**17.2 — The reload is guarded on the editor's own draft.** §7's W5 and §14's
fifth stop condition both name the risk: "the buffer reload after an apply
would discard unsaved work". The stop condition did not fire, because the
implementation resolved it rather than leaving it — the reload happens only
when the editor's document status is `Synced`. A buffer with a local draft is
**not** reloaded, and the editor says the file changed on disk. The reason the
guard is necessary and not merely prudent: the daemon's dirty-document check
is the *daemon's*, and the editor is a replica, so the daemon can believe a
document is clean while the replica holds edits it has not yet pushed.

**17.3 — The protocol's candidate bounds grew a metadata bound.** §7's W1
listed the patch bounds. The implementation also checks the candidate's
identifying strings against `MAX_LSP_PREVIEW_CANDIDATE_STRING_BYTES` (4 096),
because the apply path's own `MAX_STRING_BYTES` is 4 096 and a candidate that
exceeded it would be refused at the far end for a reason the reader could not
see. The check is `LspPreviewCandidateInvalid::OversizedMetadata`.

**17.4 — The plan's W2 said the daemon "mints the candidate for both
previews"; the implementation mints it for neither when the preview is
incomplete.** That is the plan's own §4 invariant 2, stated more precisely
here: a truncated preview, a preview with no edits, a file whose patch was
omitted, and a file outside the workspace each mint **no** candidate. The
rename path's `truncated` is the daemon's own value — the one that accounts
for the file-list cap and the edit-count cap, not only `egglsp`'s.

**17.5 — The candidate's paths are workspace-relative, and the apply path
joins them.** The plan did not say which form the path takes. Relative is
correct on both sides: the preview DTOs already name their files that way
(`daemon_lsp.rs`), and `validate_path` joins a relative path to the canonical
root. An absolute path would also work, but it would put the daemon's
canonical root into the digest's inputs, and the digest has to be recomputable
from the candidate's own fields.

**17.6 — The review opens when the preview lands, not when the user asks.**
§7's W5 said "the preview's arrival opens the change-review surface on the
candidate", and the implementation does exactly that — one gesture, one flow.
The plan did not discuss the focus semantics, and the implementation keeps the
review surface's existing contract: it opens focused and claims the keys the
review claims. The exposure that creates is recorded in the closure record as
a residual, with the permission dialog's long-standing behaviour as the
precedent.

**17.7 — The guard is a new file, not an extension of an existing one.**
§7's W6 said the guard asserts that `src/tui/` constructs the request in one
place. It is `scripts/check_tui_editor_apply_path.py`, with four rules and a
seven-case self-test, registered in `scripts/verify.sh quick` and the CI
`verify` job. It is a separate file because it is a separate invariant: the
leaf guard reads the leaf tables, the key guard reads the dispatch shapes, and
this one reads *where a write request is built*.

**17.8 — `ChangeReviewState::open` grew a parameter.** The state now records
`affected_paths` (§17.1), so the registry path passes the request's patch
paths. Four call sites changed: three in
`tests/change_review_trajectory.rs` and one in
`src/tui/components/change_review.rs`'s test helper.

**17.9 — Three pre-existing comments were false after the bridge landed, and
were corrected in place.** `src/tui/editor.rs`'s `SPACE_LEAVES` doc comment,
its `space r` leaf comment, and `src/tui/commands/editor.rs`'s `=` chord
comment all said the preview "applies nothing" and that "the gap is the
bridge". After M016 the bridge exists. A comment that says a missing thing is
missing, when it is not, is worse than no comment.

**17.10 — The panel's disposition line is new.** §7's W6 said the panel's
wording changes with the meaning. The implementation added
`preview_disposition(truncated, has_candidate)`, which says *ready to apply*,
*not applied — the preview is truncated*, or *not applied*. The truncated case
is named because a partial change is not the change that was previewed, and
offering it as applicable would be a lie.
