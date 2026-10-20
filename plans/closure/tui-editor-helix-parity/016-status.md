# Closure record — TUI editor Helix parity Milestone 016

Milestone: **M016 — the bridge: a reviewable preview**
Plan: `plans/implementation/tui-editor-helix-parity/016-preview-apply-bridge.md`
Roadmap: `plans/subsystems/tui-editor-helix-parity-roadmap.md` §7 (M016, Amendment A1)
Closed: 2026-10-19
Baseline: `bb6642688` (`feat/auto-20261018-12255c45`)

## 1. The finding, in five points

1. **The bridge was a real gap, and it was the only gap.** The editor's `=`
   and `space r` produced a *read result* — a picture of what would change —
   and ADR-0008's apply path consumes a *candidate*: the patches, the base each
   was computed against, and the digest that binds a request to them. Nothing
   in the editor path produced one.
2. **The candidate is carried, not registered.** `LspReadGet` registers
   nothing; no protocol request lists or exports a preview; and the frontend's
   own registry — `app.lsp_tool`'s — is `None` in socket/remote mode
   (`src/main.rs:3454`) and empty in production. A path that needed that
   registry would not reach the apply at all in the transport a remote user is
   on. The read result carries the candidate and the TUI relays it.
3. **The apply path needed no registry either.** `apply_preview` recomputes
   the digest over the request's *own* patches
   (`src/lsp/mutation.rs:380-401`) and revalidates every `original_hash`
   against the file on disk. So the daemon mints the digest over the same
   material the request carries, and a candidate altered in transit fails the
   check.
4. **The material already existed and was thrown away.** The rename preview's
   per-file `patch` and `original_hash` are computed by `egglsp` and dropped at
   `src/core/daemon_lsp.rs:664-667`.
5. **The chords' meaning changed, and it was registered before it changed.**
   M015 recorded the transition in three places; M016 landed it, and the
   change is recorded in six.

## 2. What was built

| Work package | Where |
|---|---|
| W1 — the protocol's candidate | `crates/codegg-protocol/src/lsp.rs`: `LspPreviewCandidateDto`, `LspPreviewCandidateInvalid`, the bounds, `validate()`, and `candidate` on the two preview payloads |
| W2 — the daemon mints it | `src/core/daemon_lsp.rs`: `mint_preview_candidate`, `mint_rename_candidate`, `PREVIEW_CANDIDATE_REVISION` |
| W3 — the editor carries it | `src/tui/editor_lsp.rs`: `EditorLspState::preview_candidate`, `candidate_of` |
| W4 — one apply path, two resolutions | `src/tui/commands/change_review.rs`: `ApplySource`, `build_apply_request`, `open_carried`; `src/tui/app/state/change_review.rs`: `carried`, `affected_paths` |
| W5 — the chord leads to the review; the apply reloads the buffer | `src/tui/commands/editor.rs`: the read's arrival opens the review; `src/tui/commands/change_review.rs`: `reload_editor_after_apply` |
| W6 — the wording, the docs, the guard | `src/tui/components/editor.rs`: `preview_disposition`; the seven documents; `scripts/check_tui_editor_apply_path.py` |

## 3. The model

### 3.1 Three chords, two mechanisms

| Chord | Operation | What it now does |
|---|---|---|
| `=` (Normal-mode chord, no prefix) | `LspReadOperation::FormatPreview` | reads the format preview; a complete preview opens the change review on it |
| `space r` (prefix leaf) | `LspReadOperation::RenamePreview` | arms the `:` prompt, then reads the rename preview; a complete preview opens the change review on it |
| `space a` (prefix leaf) | — | still a statement: no code-action **read** exists to preview (M017) |

The two preview chords are unchanged as *keys*. What changed is what they do
with the answer.

### 3.2 The candidate

```rust
pub struct LspPreviewCandidateDto {
    pub preview_id: String,
    pub preview_revision: u64,
    pub preview_digest: String,
    pub kind: String,          // rename | code_action | formatting
    pub title: String,
    pub provenance: String,
    pub patches: Vec<LspPreviewPatchDto>,
}
```

The bounds are the apply path's own — at most 100 patches, at most 50 000
bytes per patch, at most 1 MiB in total, a 64-character digest, a non-zero
revision, an identifying string of at most 4 096 bytes — so a candidate that
passes `validate()` is one the daemon can at least *shape* into a request.
The daemon still decides whether the change may be applied.

### 3.3 The digest is the binding

The daemon computes `preview_digest_for_candidate(kind, title, provenance,
paths, hashes, patches)` over exactly the material it puts in the candidate.
`apply_preview` recomputes the same function over the request's own patches
and refuses a mismatch. **The daemon's digest is therefore recomputable from
the candidate's own fields** — and that is a test, not a hope
(`a_minted_candidate_digest_is_recomputable_from_its_own_fields`).

### 3.4 The bridge invariant, and its evidence

| Claim | Evidence |
|---|---|
| The editor path held no apply artifact before M016 | no `register_preview_artifact`, no `PreviewArtifact`, no `LspPreviewApply` in the editor path; no `change_review` reference in `src/tui/commands/editor.rs` |
| A registry could not have carried it | `LspReadGet` registers nothing; no protocol request exports a preview; `app.lsp_tool` is `None` in socket mode (`src/main.rs:3454`) and empty in production |
| The apply path needs no registry | `apply_preview` recomputes the digest over the request's own patches and revalidates the hashes (`src/lsp/mutation.rs:380-436`) |
| The material existed and was discarded | `daemon_lsp.rs:664-667` drops `patch` and `original_hash` |
| One apply path, structurally | `scripts/check_tui_editor_apply_path.py`: the variant may appear in one file, with one construction, inside `build_apply_request` |

### 3.5 The registered transition, landed

M015 wrote it down while the preview was still what a reader learned:

> *"When the bridge is built — when the editor's preview becomes a reviewable
> artifact ADR-0008's apply path can consume — `=` and `space r` stop showing
> what would change and start doing it."*

It is now true, and it is recorded in six places: the roadmap (M016's entry
and Amendment A1), this closure record, `CHANGELOG.md` (`### Changed`),
`docs/tui.md`, `docs/LSP.md`, `architecture/tui.md` and `architecture/lsp.md`,
plus the two skill files.

## 4. The guards

| Guard | What it says | Result |
|---|---|---|
| `scripts/check_tui_editor_apply_path.py` | one file, one construction, inside `build_apply_request` | pass |
| `scripts/check_tui_editor_apply_path.py --self-test` | seven synthetic cases, each naming the rule it proves | pass |
| `scripts/check_tui_editor_key_authority.py` | the chords' attribution | pass (2 guard-block / 71 Normal / 8 Insert) |
| `scripts/check_tui_editor_leaf_authority.py` | the leaf tables | pass (5 tables / 44 glyphs / 12 bindings) |
| `scripts/check_tui_editor_prefix_state.py` | the prefix state | pass |
| `scripts/check_tui_editor_text_authority.py` | the text authority | pass |

The new guard is registered in `scripts/verify.sh quick` and in the CI
`verify` job.

## 5. Verification

The sweep ran against the final tree. `CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0`
throughout — the incremental cache in this worktree is unreliable
(`dep-graph.part.bin` went missing twice during the M011–M015 work).

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | exit 0 |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | exit 0 |
| `cargo build --locked --bin codegg-sandbox-helper` | exit 0 |
| `cargo test -p codegg-protocol --locked` | **255 passed / 0 failed** |
| `cargo test -p codegg --lib --locked --no-fail-fast` | **6830 passed / 0 failed / 4 ignored** |
| `cargo test -p codegg --lib --locked agent::asset_refresh::tests::same_scope_requests_coalesce_to_one_publication` | **1 passed / 0 failed** (the one test the whole-suite run filters out — see §5.2) |
| `cargo test -p codegg --lib --locked interactive_terminal` | **94 passed / 0 failed** |
| `cargo test -p codegg --lib --locked tui::` | **2308 passed / 0 failed** |
| `cargo test -p codegg --lib --locked tui::commands::editor::tests::` | **224 passed / 0 failed** |
| `cargo test -p codegg --lib --locked tui::commands::change_review::tests::` | **9 passed / 0 failed** |
| `cargo test -p codegg --lib --locked tui::editor_lsp::tests::` | **12 passed / 0 failed** |
| `cargo test -p codegg --locked --test change_review_trajectory` | **7 passed / 0 failed** |
| `cargo test -p codegg --features lsp-test-support --locked --test lsp_read_delivery_m013 --test lsp_read_surface_trajectory --test lsp_completion_trajectory` | **12 + 7 + 14 = 33 passed / 0 failed** |
| `python3 scripts/check_tui_editor_key_authority.py` | pass — **2 guard-block / 71 Normal / 8 Insert** |
| `python3 scripts/check_tui_editor_leaf_authority.py` | pass — **5 tables / 44 glyphs / 12 bindings** |
| `python3 scripts/check_tui_editor_leaf_authority.py --self-test` | pass — 9 cases |
| `python3 scripts/check_tui_editor_prefix_state.py` | pass |
| `python3 scripts/check_tui_editor_text_authority.py` | pass |
| `python3 scripts/check_tui_editor_apply_path.py` | pass — one builder, one construction |
| `python3 scripts/check_tui_editor_apply_path.py --self-test` | pass — 7 cases |
| `bash scripts/verify.sh quick` | **"Quick verification passed."** |

### 5.1 The chord and leaf counts did not move

The key-authority guard still reports **2 guard-block / 71 Normal / 8 Insert**
and the leaf guard still reports **5 tables / 44 glyphs / 12 bindings**. M016
added no chord and no leaf: `=` and `space r` keep the keys M015 gave them,
and what changed is what they do with the answer. A milestone that changed a
count would have said so here.

### 5.2 The one filtered test

`agent::asset_refresh::tests::same_scope_requests_coalesce_to_one_publication`
is filtered out of the whole-suite run and run alone instead. It is not
M016's: it is in the asset-refresh path, which this milestone does not touch,
and it **passes in isolation in 0.00s** while the whole-suite run — with the
machine's load average above 7 — can block in it for minutes. That is the
load-sensitive class this closure record already names. The number reported
here is the whole suite's, with that one test out and its own result beside
it; the sweep script is `/tmp/m016_verify.sh`.

### 5.3 The new tests

| Where | What it proves |
|---|---|
| protocol | the candidate's `validate()` accepts a well-formed candidate and rejects each malformed field with its own reason |
| protocol | a preview without a candidate is still a preview, and an older peer's payload still reads |
| daemon | **the minted candidate's digest is recomputable from the candidate's own fields** — the binding the apply path depends on |
| daemon | two candidates get different revisions |
| daemon | a candidate with no patch is not minted; a rename whose only patch was omitted mints none; a truncated rename mints none |
| daemon | a minted candidate names its files relative to the workspace |
| TUI state | a candidate is scoped to the read that produced it: a new read drops it, and a new scope wipes it |
| TUI commands | a preview with a candidate opens the review on it; a preview without one opens none |
| TUI commands | the carried candidate builds the request the review displayed, with the active tab's session and workspace |
| TUI commands | **the carried path needs no LSP tool** — the socket-mode frontend, where `app.lsp_tool` is `None` |
| TUI commands | a malformed carried candidate is refused with its reason, not opened as a review the user cannot finish |
| TUI commands | a successful apply reloads the file it wrote; a refused apply reloads nothing; an apply over an unsaved buffer keeps the draft |
| components | a candidate says *ready to apply*; a truncated preview says it is truncated; a preview with neither still says *not applied* |

### 5.4 The pre-existing tests that changed

Three call sites of `ChangeReviewState::open` grew a parameter
(`affected_paths`, §17.1): `tests/change_review_trajectory.rs` (three) and
`src/tui/components/change_review.rs`'s test helper (one). No production
behaviour changed in them, and no assertion was weakened.

### 5.5 The guard's injection proof

`/tmp/m016_inject_guard.py` injects into the real tree, runs the guard, and
restores the tree **byte-identically** (the backup is keyed by the relative
path — a flat basename backup clobbered `src/tui/editor.rs` once during the
M009 work).

```
=== injection: a second construction in the builder's file ===
src/tui/commands/change_review.rs:124 [rule-2] expected exactly one construction of `CoreRequest::LspPreviewApply` in src/tui/commands/change_review.rs, found 2
src/tui/commands/change_review.rs:827 [rule-3] `CoreRequest::LspPreviewApply` is constructed outside `build_apply_request`: that is a second place to build an apply request
2 violation(s) found.
--- exit 1 ---
ok: the guard fired on a second construction in the builder's file (['rule-2', 'rule-3'])
--- restored byte-identically (1bf938a0387bbfa5) ---
=== injection: a mention of the variant in another file ===
src/tui/commands/editor.rs:11649 [rule-1] `CoreRequest::LspPreviewApply` is mentioned outside src/tui/commands/change_review.rs: a second apply path
1 violation(s) found.
--- exit 1 ---
ok: the guard fired on a mention of the variant in another file (['rule-1'])
--- restored byte-identically (8967ef9f614fa0cf) ---
```

## 6. Exit conditions

| Condition | Result |
|---|---|
| The editor's format and rename previews are **reviewable artifacts** the apply path can consume | met — the daemon mints the candidate, the editor carries it, the review displays the request's own patch text |
| The candidate travels the **read result**, not a registry | met — `LspReadGet` registers nothing; the carried path needs no LSP tool (§5.3) |
| The daemon's digest is **recomputable from the candidate's own fields** | met — the daemon test proves it, and `apply_preview` recomputes it over the request's own patches |
| A preview that cannot be applied is **not offered** as one | met — truncated, patch-omitted, empty and out-of-workspace previews mint none, and the panel says which |
| The **one apply path** is structural | met — `scripts/check_tui_editor_apply_path.py`, with a seven-case self-test and the injection proof |
| The **dirty-buffer refusal** stands | met — unchanged, and the editor's own draft is kept rather than reloaded over |
| The **buffer reload** does not discard unsaved work | met — the reload is guarded on the editor's `Synced` status (§17.2) |
| The chords **did not move** | met — 2 / 71 / 8 and 5 / 44 / 12, unchanged |
| The meaning change is recorded in **six places** | met — the roadmap, this record, `CHANGELOG.md`, `docs/tui.md`, `docs/LSP.md`, `architecture/tui.md`, `architecture/lsp.md`, and the two skill files |
| The docs and their skills moved **together** | met — one commit |

**All five of the plan's §14 stop conditions did not fire.** The reload's
discard of unsaved work was the one that came close; it was resolved by the
guard in §17.2 rather than reported, and the resolution is recorded there.

## 7. Recorded divergences

The plan's §17 holds the ten deviations, from the plan file itself. The two
that changed the design rather than the wording:

- **§17.2 — the reload needed a guard the plan only named as a risk.** The
  plan's stop condition ("the buffer reload after an apply would discard
  unsaved work") did not fire, because the implementation resolved it: the
  reload happens only when the editor's document status is `Synced`. The
  reason it is necessary and not merely prudent: the daemon's dirty-document
  check is the *daemon's*, and the editor is a replica, so the daemon can
  believe a document is clean while the replica holds edits it has not pushed.
- **§17.6 — the review opens when the preview lands.** One gesture, one flow.
  The focus semantics were not in the plan, and the implementation keeps the
  review surface's existing contract. §9 records what that costs.

## 8. The documents that moved

| Document | What changed |
|---|---|
| `docs/tui.md` | "The preview keys" now describes the review and the apply; the two pointer paragraphs; the space-menu row |
| `docs/LSP.md` | "Previews change nothing" became "A preview you can apply" |
| `architecture/lsp.md` | the bridge's shape, the carried candidate, the one-apply-path invariant, and the transition that landed |
| `architecture/tui.md` | the keymap rows, the editor's place in the apply, and the reload's guard |
| `CHANGELOG.md` | the M013 bullet corrected in place; a new `### Changed` bullet for the transition |
| `.opencode/skills/lsp-ide/SKILL.md` | the leaf table and the write-path paragraph |
| `.opencode/skills/tui/SKILL.md` | the preview bullet |

## 9. Carried forward

- **The review's focus and the asynchronous landing.** The review opens when
  the preview read lands, and it claims the keys a focused review claims —
  `a` accepts, `Esc` rejects. A keystroke typed *just before* the review
  landed could therefore be claimed by it. This is the same exposure the
  permission dialog has had since it existed: an asynchronous modal may take
  the next keystroke. The review is visible when it claims it, and the apply
  is the daemon's checked path — but the exposure is real, and it is recorded
  rather than left implicit.
- **M017** — the code-action read, so `space a` previews instead of stating.
- **M018** — the `Ctrl-x` completion-key blind spot in the key-authority
  guard.
- **The dirty-buffer merge** — unchanged and still deferred. The daemon
  refuses to apply over a dirty buffer; the editor's own draft is what makes
  that common.

## 10. The residual

| Residual | Why it is not this milestone's |
|---|---|
| The `Ctrl-s`/`Ctrl+r` Insert-mode conflict | M011 decided it; both keep their mode-independent meaning |
| The key-authority guard's `Ctrl-x` blind spot | M018 |
| A code-action preview | M017 — it needs a protocol extension first |
| The change quartet's missing data source | It needs its own milestone and its own plan |
| The 69 failing `tests/` integration binaries | Pre-existing; registered, not discharged |

## 11. Handoff

- The candidate travels in the **read result**. Nothing is registered; there
  is no new registry and no new daemon state.
- The preview **keeps its meaning as a preview** when it cannot become a
  candidate, and the panel says which it was.
- The chords do not move. Only what they *do* changes, which is why the change
  is recorded in six places.
- M017 builds on this: a code-action read mints a candidate of kind
  `code_action` through the same path.
