# Five-Line Closure Corrective C001 — Closure Status

Status: conditionally closed

Source implementation plan: `plans/implementation/five-line-closure-corrective/001-closure-evidence-and-residue.md`
Source subsystem roadmap: **none — cross-line.** This corrective carries the
residue of the harness, helix, TUI-modularity and sandbox-audit lines plus the
advertised-keys corrective, and each of those roadmaps points here for the
conditions it cannot discharge alone.
Repository baseline reviewed: `7642b9e38` on `feat-auto-20261109-f8abad0d`
Implementation commits: **none — the tree is uncommitted.** WP 0 has not
happened, which is the plan's first stop condition and the reason WP B and
WP C are blocked rather than done.

## 1. Executive finding

**Five of the seven work packages landed; the two Linux ones are blocked by the
plan's own precondition, not by a defect.** WP A, D, E, F and G are implemented
and locally verified. WP B (CI evidence integrity) and WP C (the `/proc` and
`/var/run` narrowing) were **not started**, because §14's first stop condition
says the plan stops after A, D, E, F, G when WP 0 has not happened and reports
the Linux items as blocked on the commit rather than as done. `full` cannot
observe `#[cfg(target_os = "linux")]` code on this host, and an unverifiable
change to a security control is worse than a named blocker.

The status sweep corrected **28 records**: 26 were `conditionally closed` with
stale conditions, and two were stale in the restrictive direction
(`tui-modularity/006` said `corrective pass required`, `007` said `blocked`).
The five lines now read **27 `conditionally closed` and 7 `closed`** — helix
**004** was promoted on two verified checks, and every other record keeps its
residue named in its own words.

Three defects were fixed in code: keybind's dead 131-line `Widget` impl is
deleted with its comment ceiling moved deliberately, `Diff`'s `End` no longer
blanks a two-hunk pane (five per-hunk skips became one continuous walk), and
Export's `Esc` now has the dispatch test it was missing. Both new tests are
mutation-proven. `scripts/verify.sh full` is green on the final tree:
**17,380 workspace tests and 8,444 feature-pass tests, 0 failures.**

## 2. Requirement-to-evidence matrix

| # | Acceptance criterion | Verdict | Evidence |
|---|---|---|---|
| 1 | No conditional record asserts a gate the 2026-11-13 `full` discharged; every corrected sentence names its discharge | **met** | 28 records, each ending in a `## Correction log (2026-11-13)` naming the discharge per sentence |
| 2 | Helix **004** `closed`; helix **001**/**002** either `closed` with the low item accepted in writing, or still conditional with the reason | **met** | 004 promoted (§3); 001 and 002 stay conditional with the owner question stated — no acceptance was invented |
| 3 | `tui-modularity/007`'s record status matches its plan and roadmap | **met** | `blocked` → `conditionally closed`; `006` flipped too, a contradiction the plan did not list (§10) |
| 4 | Four owner decisions listed as questions, each with the record that needs them | **met** | §9, plus three more the work surfaced |
| 5 | The Linux job fails rather than skips when no backend is present | **not met — blocked on WP 0** | WP B not started; `CODEGG_SANDBOX_QUALIFICATION` is still absent from `ci.yml` |
| 6 | The ABI-6 precondition is asserted or pinned | **not met — blocked on WP 0** | WP B not started |
| 7 | `/proc/<other-pid>/environ` denied on Linux, by a fixture **and** a positive control | **not met — blocked on WP 0** | WP C not started; `policy.rs:393,395` still grant both trees |
| 8 | keybind's dead `Widget` impl gone and the comment ceiling moved deliberately | **met** | 882 → 750 lines; `docs/comment-budget.toml` 0.0676 → 0.0724 with the reason in the file |
| 9 | Export's `Esc` has a dispatch test that fails against `close()` | **met** | §4, mutation shown |
| 10 | `End` on a two-hunk diff leaves both bodies painted | **met, with the criterion's wording read narrowly** | §5; at the last line only the tail paints, which is what `End` means. The real defect — the offset applied per hunk — is what the tests pin |
| 11 | The ratchets are unchanged: `[contrast] = 0`, `[advertised_keys] = 4`, `[hint_truncation] = 0` | **met** | `invariants::baseline` 13/13; the four advertised-key rows keep their reasons |
| 12 | `scripts/verify.sh full` is green on the final tree | **met** | `FULL_RC=0`: 17,380 workspace tests passed (10 skipped) and 8,444 feature-pass tests passed (5 skipped), 0 failures, with `cargo fmt --all --check` and clippy `-D warnings` inside the run |

## 3. Production implementation evidence

**WP D — the dead `Widget` impl.** `impl Widget for &KeybindDialog` occupied
`keybind.rs:456-586` and had no caller: the only three `render_widget` call
sites in `src/tui/` are the message list, the prompt and the status bar, and the
modal renders through `Component::render`. Deleted with the 131 lines. The
`Widget` import **stays**, because `render_body` still calls `Paragraph::render`
— the plan's instruction to remove it does not compile, which is a plan
correction (§10), not a deviation.

**WP E — Export's `Esc`.** No production change. The app's
`handle_keybind_key` already calls `kd.cancel_mode()` for `KeybindMode::Export`,
and `mode_footer` already paints `Esc back`. The test is the whole deliverable.

**WP F — `Diff`'s `End`.** Five sites applied the scroll offset per hunk: two in
`src/tui/components/diff.rs` (`render_diff`, `render_hunks_in`) and three in
`src/tui/components/dialogs/diff.rs` (`render`, `render_body`,
`render_legacy`) — the plan counted three. They now share one
`lines_from(hunks, offset)` walk in `components/diff.rs`, so the offset addresses
the diff once. Single-hunk behaviour is byte-identical: at offset 0 nothing is
skipped either way, and the owned/borrowed equality test passes in both modes at
offsets 0, 1, 3 and 50.

**WP G — the two smaller findings, one recorded and one measured-and-reverted.**
The keybind modal-path bypass is recorded in `tui-modularity/006` §10 (medium)
with the reason the fix is a separate milestone. `spellings()` was changed,
measured, and reverted — §5.

**WP A — the status sweep.** Every record now states what discharged its gate
conditions and keeps every residue it carried. Two records changed status
beyond the plan's list, and one plan claim was refuted by the tree (§10).

## 4. Verification executed

Local, on this tree. Nothing here is CI evidence, and the Linux items have none.

| Command | Result |
|---|---|
| `cargo check -p codegg --lib` (after WP D, and again after WP F) | clean |
| `cargo test -p codegg --lib -- tui::components::diff` | 4/4 |
| `cargo test -p codegg --lib -- tui::app::keybind_export_dispatch_tests tui::components::dialogs::diff` | 7/7 |
| the same three tests **under mutation** | 3/3 FAILED, as required — see below |
| `cargo test --test tui_render -- invariants::baseline` | 13/13 |
| `cargo test --test tui_render` (whole corpus) | **1237 passed, 0 failed** — the count is unchanged, so no recorded frame moved |
| `python3 scripts/check_comment_budget.py` | pass, with the three ceilings this pass moved recorded deliberately |
| `scripts/verify.sh quick` | `QUICK_RC=0` |
| `scripts/verify.sh full` | **`FULL_RC=0`** — 17,380 workspace tests passed (10 skipped), 8,444 feature-pass tests passed (5 skipped), 0 failures. `fmt` and clippy `-D warnings` are inside the run and both passed |

**Mutation evidence.** Both new tests were proved non-vacuous by reverting the
fix and watching them fail:

- WP E: replacing `kd.cancel_mode()` with a close (clearing the focus manager)
  fails `export_esc_returns_to_the_list_instead_of_closing` at
  "the box must still be mounted after Esc in Export".
- WP F: restoring the per-hunk skip in `lines_from` fails **both** End tests. The
  dialog test's failure message prints the frame, and the body rows are empty —
  the reported defect reproduced exactly.

Both files were restored byte-identically afterwards (`diff -q` against a
pre-mutation copy).

**The first `full` run failed, and it was right to.** `FULL_RC=101`: clippy
`-D warnings` rejected the collapsed loops with five `explicit_counter_loop`
lints and one `needless_lifetimes` on `lines_from` — the counter survived the
double loop that had justified it. Both were real findings against this pass's
own code, not noise. Fixed by eliding the lifetime and replacing the
`break`-on-counter with `.take(visible_lines).enumerate()`, which is also
shorter. The two code files' comment ratios then moved again (the rewrite removes
code), so both ceilings moved a second time with the reason updated in place.

## 5. Invariant review

- **The ratchets did not move.** `[contrast] = 0`, `[advertised_keys] = 4`,
  `[hint_truncation] = 0`; the manifest's totals are untouched.
- **The comment ratchet moved three times, deliberately, in the same diff as the
  code that needed it**, each with the reason written beside the entry:
  `keybind.rs` 0.0676 → 0.0724 (deleting code raises the ratio),
  `components/diff.rs` 47 → 52 comments and 0.1144 → 0.1221,
  `dialogs/diff.rs` 107 → 109 and 0.2054 → 0.2069. A fourth move was made and
  reverted with the `spellings()` change, so `baseline.rs` is back at its
  recorded 315 / 0.2752.
- **No bundled theme file changed.** `git status --porcelain -- assets/` is
  empty.
- **ADR-0042 key-chord ownership and the command catalog are untouched** — no
  key was added, removed or rebound.
- **`spellings()` was not left in a half-fixed state.** The attempted change is
  fully reverted: the filter, the doc comment, the `[0]` lookup and the
  `UNREADABLE_SPELLINGS` list are all byte-identical to the baseline.

## 6. Failure and recovery review

WP B and WP C have no runtime behaviour to review because neither was started.
WP F is a synchronous paint change with no state. WP D removes code with no
caller, so no cancellation, restart or contention path exists for it. WP E adds
a test only.

## 7. Migration and compatibility review

None. No schema, migration, protocol, storage, DTO or theme change;
`STORAGE_LAYOUT_VERSION` is untouched.

## 8. Security review

**The one live finding stays live, and this record says so plainly.** `WP C` was
not started: `src/security/sandbox/policy.rs:393,395` still grants `/proc` and
`/var/run` as tree roots, so a contained command can read
`/proc/<other-pid>/environ` for a same-uid process today. No run of this plan
closes it. It is a control gap recorded inside a `conditionally closed` record,
and it is now named as such in `sandbox-containment-audit-corrective/002` and in
that line's roadmap note, not as evidence debt.

The second gap is unchanged: `CODEGG_SANDBOX_QUALIFICATION` is never set in
`ci.yml`'s `ubuntu-latest` job, so `require_backend!` returns silently when no
backend is present and the sandbox suite can be a green skip.

No security surface was widened by this pass. WP D removed code; WP F changed a
paint loop; WP E added a test.

## 9. Owner decisions — open, and listed as open

The four the plan named, plus three the work surfaced. None is resolved here.

1. **`tui-modularity/001` — M001's acceptance criterion 6 structural reading.**
   No M001-only diff exists to read it against. Is "the harness exists and is
   wired into both lanes" the reading?
2. **`tui-modularity/008` — criterion 4 at 8 of 10.** `dialog_area` and
   `sidebar_area` remain, and the attribution to `src/tui/commands/**` is
   **correct** (ten production reads in `commands/panels.rs`). Is 8 of 10 the
   accepted end state?
3. **`tui-modularity/003` — the `permission.rs` title-badge exception.** 39 of 40
   dialog surfaces adopt `Section`. Amend criterion 1, or scope a `Section`
   title-adjunct for a follow-up milestone?
4. **`tui-modularity/002` and `/003` — the corpus diff-note substitute.** Is the
   mechanical substitute acceptable in writing: the whole `tui_render` family
   green, `dialog_dispatch_coverage` unmodified and green, and a hosted
   content-row assertion?
5. **`helix/001` — the `space ?` keymap box omits `_` (trim) and `Alt-d`.** An
   owner statement that the box is a condensed aid and `docs/tui.md` is
   complete, or the two `EDITOR_KEYMAP` rows?
6. **`helix/002` — the same box omits `Alt-d`.** Same settlement.
7. **`helix/009` — the single-chain pruning rule is unratified.** The record
   states it as an open question, not a decision.

## 10. Unresolved findings

| Severity | Finding | Where |
|---|---|---|
| **high** | `/proc` and `/var/run` remain tree grants; `/proc/<other-pid>/environ` is readable. WP C, blocked on WP 0 | `sandbox/002`, its roadmap note, §8 |
| **high** | The sandbox suite can be a green skip: `CODEGG_SANDBOX_QUALIFICATION` is unset in `ci.yml` and ABI ≥ 6 is neither pinned nor asserted. WP B, blocked on WP 0 | `sandbox/002` |
| medium | **I4's `Keybind` verdict is a lower bound.** The measurement presses through `Component::handle_key`; production routes `DialogType::Keybind` to `App::handle_keybind_key` (`src/tui/app/mod.rs:4683-4686`) and returns. The other 13 `keymap_hint_line()` overrides are dispatched through `FocusManager` and are unaffected | `tui-modularity/006` §10 |
| medium | **The painted-row reader cannot tell a key from prose.** Surfacing every painted unreadable spelling produced **58** tokens: 23 prose (`Showing`, `Diff:`, `Policy:`, `Confirm`), 18 `PanelManager` panel names, and only the remainder key-ish (`Del`, `jk`, `↑↓`, `←→`, `Ctrl+R`, `1-3`, `1-22`, `?`, `▸`, `[Right]`). `painted` is load-bearing for the completeness cross-check, so the filter cannot simply be removed; the missing piece is a key-shapedness rule in `footer_parts` | this record §5; `baseline.rs` unchanged |
| medium | **`jk` is outside the measurement for a second reason:** it is declared by **panels** (`panels/session.rs:55`, `panels/agents.rs:250`, `chat_browser.rs:822,836`), and the I4 report's subject is the **dialog** fixture set | this record §5 |
| medium | **This plan's own §3 row for M008 was wrong.** `src/tui/commands/panels.rs` reads `App::sidebar_area` at ten production sites and `interactive_terminal.rs` reads `App::dialog_area` from a test helper, so the attribution holds. Corrected in the plan and in 008's record | plan §3 |
| medium | `tui-modularity/006`'s record said `corrective pass required` while its plan, roadmap and registry row said `conditionally closed`. Flipped; the plan did not list it | `tui-modularity/006` |
| medium | **The helix roadmap's §12 table was stale in five rows** — M004 said `conditionally closed`, M005–M007 said `ready for handoff`, M008 said `blocked` — while all nine records existed and eight of them read `conditionally closed`. A roadmap row that says a milestone is ready for handoff when its closure record exists is the contradiction the planning skill calls a closure defect. Reconciled here | `plans/subsystems/tui-editor-helix-capability-parity-roadmap.md` §12 |
| low | The plan told the implementer to delete keybind's `Widget` import, which does not compile; and counted three per-hunk offset sites where there are five. Both corrected in the plan | plan §6, §10 |
| low | `ci.yml` triggers on pull requests, pushes to `main` and dispatch — not literally push-only. The blocking fact stands (it has never seen this tree), the wording was corrected | `sandbox/002` |

## 11. Roadmap disposition

- **TUI modularity and render invariants** — 10 records `conditionally closed`;
  its roadmap now points at this corrective for the `full`-and-a-commit blocker
  and for the two items dropped by research (`/security-review`'s geometry, the
  I4 parser widening).
- **TUI advertised-keys corrective** — both records `conditionally closed`; the
  4-row advertised-key residue stands with its reasons.
- **Harness practice adoption** — 3 conditional, 4 closed. Its roadmap's M001
  blocker cell was stale ("depends on a sibling milestone's uncommitted work")
  and now names the real residue: no replay subsection in
  `architecture/testing.md`, WP-D's live recording, and the uncommitted tree.
- **Sandbox containment audit corrective** — 4 conditional, 2 closed. Its
  roadmap note now says the `/proc` narrowing is a live control gap, not
  outstanding evidence.
- **TUI editor Helix capability parity** — 8 conditional, 1 closed (004
  promoted).

**Unblock audit: this closure unblocks nothing.** No registered plan names WP A,
D, E, F or G as a dependency, and the two items that would unblock the Linux
lines are WP 0 and then WP B/WP C — both still open. That is a valid outcome and
is recorded rather than left implicit.

## 12. Registry updates

- `plans/registry.md` — the five-line corrective row moves from `ready` to
  **`conditionally closed`**, linking this record and naming WP B/WP C as
  blocked on WP 0. The helix subsystem summary moves from "M001–M009 all
  conditionally closed" to "M004 closed, M001–M003 and M005–M009 conditionally
  closed", and the M004 milestone row from `conditionally closed` to
  `closed (promoted 2026-11-13)`.
- The helix roadmap's §12 table was reconciled for M004–M008 (§10).
- The plan's own `Status:` becomes `implemented — WP A, D, E, F and G landed;
  WP B and WP C blocked on WP 0`, with this record linked in its header.
- The two roadmap notes that said "registered `ready`" now say
  `conditionally closed` and point at this record.
- No record in the five lines was moved between the registry's *Blocked work*
  and *Recently closed or conditionally closed work* sections by this pass: all
  28 were already in the latter, and none changes section.
