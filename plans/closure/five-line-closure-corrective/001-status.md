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

**All seven work packages landed.** WP A, D, E, F and G are implemented and
locally verified; WP 0 (the commit and push) happened, and WP B and WP C
followed it: the read grants are narrowed, the qualification marker is set, and
a hosted Linux run proves the property. The narrowing needed three corrections
that only a Linux compiler and a Linux kernel could find, which is why the plan
gated it on the commit rather than attempting it on the authoring host.

The status sweep corrected **28 records**: 26 were `conditionally closed` with
stale conditions, and two were stale in the restrictive direction
(`tui-modularity/006` said `corrective pass required`, `007` said `blocked`).
With the seven owner decisions taken on 2026-11-14 the five lines read
**13 `closed` and 21 `conditionally closed`**: helix **004** was promoted on two
verified checks, decisions 1, 2, 3, 5, 6 and 7 closed six more records
(`tui-modularity/001`, `/003`, `/008` and `helix/001`, `/002`, `/009`), and the
21 that remain conditional each keep a residue named in its own words —
`tui-modularity/002` now on a **measured** defect rather than a prose one.

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
| 2 | Helix **004** `closed`; helix **001**/**002** either `closed` with the low item accepted in writing, or still conditional with the reason | **met** | 004 promoted (§3); **001 and 002 are now `closed`** by decision 5/6 — the box names `Alt-d` and `_` (B), so nothing was accepted in writing that the code could fix |
| 3 | `tui-modularity/007`'s record status matches its plan and roadmap | **met** | `blocked` → `conditionally closed`; `006` flipped too, a contradiction the plan did not list (§10). `001`, `003` and `008` were promoted to `closed` by decisions 1–3 |
| 4 | Four owner decisions listed as questions, each with the record that needs them | **met, then answered** | §9 lists all seven and the answer each one took on 2026-11-14 |
| 5 | The Linux job fails rather than skips when no backend is present | **met** | `CODEGG_SANDBOX_QUALIFICATION=1` on the sweep in `ci.yml`, and the grant lane's skip arms assert instead of skipping. Run `43532152355` |
| 6 | The ABI-6 precondition is asserted or pinned | **met** | `the_qualification_lane_obtains_landlock_abi_six` passed on the hosted runner (run `43532152355`), and the marker's own spelling is asserted on every host |
| 7 | `/proc/<other-pid>/environ` denied on Linux, by a fixture **and** a positive control | **met** | `the_default_grant_set_denies_another_processes_proc_entry` passed hosted: the entry is denied, the command's own `/proc/self` reads, and the same entry reads once the `/proc` tree is restored. Run `43532152355` |
| 8 | keybind's dead `Widget` impl gone and the comment ceiling moved deliberately | **met** | 882 → 750 lines; `docs/comment-budget.toml` 0.0676 → 0.0724 with the reason in the file |
| 9 | Export's `Esc` has a dispatch test that fails against `close()` | **met** | §4, mutation shown |
| 10 | `End` on a two-hunk diff leaves both bodies painted | **met, with the criterion's wording read narrowly** | §5; at the last line only the tail paints, which is what `End` means. The real defect — the offset applied per hunk — is what the tests pin |
| 11 | The ratchets are unchanged: `[contrast] = 0`, `[advertised_keys] = 4`, `[hint_truncation] = 0` | **met** | `invariants::baseline` 13/13; the four advertised-key rows keep their reasons |
| 12 | `scripts/verify.sh full` is green on the final tree | **met** | `FULL_RC=0` after the decisions landed: 17,382 workspace tests passed (11 skipped) and 8,446 feature-pass tests passed (6 skipped), 0 failures, with `cargo fmt --all --check` and clippy `-D warnings` inside the run |

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
| `scripts/verify.sh full` | **`FULL_RC=0`** — 17,382 workspace tests passed (11 skipped), 8,446 feature-pass tests passed (6 skipped), 0 failures. `fmt` and clippy `-D warnings` are inside the run and both passed. The two extra tests are the ones the decisions added, one of them the `#[ignore]`d Help 20×6 measurement |

**Hosted, on the Linux runner — the only place these can run.** Run
`43532152355` (`Linux sandbox grant qualification`, head `c6898a4d8`,
`ubuntu-24.04`, LSM list `lockdown,capability,landlock,yama,apparmor`):
`running 6 tests` → **`test result: ok. 6 passed; 0 failed`**, including
`the_default_grant_set_denies_another_processes_proc_entry`,
`the_narrowed_grant_set_runs_the_toolchain_and_its_proc_entries_are_load_bearing`
and `the_qualification_lane_obtains_landlock_abi_six`.

Three corrections were needed before that run was green, and each was found by
the mechanism rather than by reading:

1. `/proc/self` was canonicalized **in the parent**, so the grant named the
   daemon's pid directory and the command lost its own. The aliases now travel
   unresolved and the helper resolves them at the pid `exec` preserves.
2. The protected mount view snapshots every read root's device and inode; a
   per-process alias cannot pass that check by construction, so the aliases are
   skipped there.
3. The fixture built a read-only spec from the read allowances alone, so
   `> /dev/null` was denied. Device nodes are writable under **every** profile
   and the spec now carries `device_write_allowances()` as the product does.

A fourth fix was the lane itself: the fixture resolved the helper beside the
profile directory, and on a lane that builds only the helper cargo placed the
test executable under `target/debug/build/...`. It now probes the profile
directory, the `deps` directory and the workspace target directory.

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

**The live finding is closed.** `tool_read_allowances()` no longer grants
`/proc` or `/var/run` as trees. It names the self-scoped `/proc` entries, the
kernel's informational files, the read-only sysctl view, and the resolver file
that exists — and the hosted run proves another same-uid process's
`/proc/<pid>/environ` is unreadable while the command keeps its own
`/proc/self`.

Two residues are stated rather than implied. A **child** of the command has its
own pid and cannot read its own `/proc/self`; the descriptor says so
(`LANDLOCK_LIMITS`), and the self-probe in the fixture uses a shell
redirection because a child's `/proc/self` names the child. And Landlock still
does not mediate **pathname** AF_UNIX sockets, so narrowing `/var/run` removes
file exposure (including secrets mounted under `/run`) but not socket reach;
that limit was already named and is unchanged.

The silent-skip gap is closed too: the marker is set on the sweep and in the
grant lane, so a runner without a backend fails instead of skipping, and a
pre-6.12 kernel fails the ABI-6 probe.

No security surface was widened by this pass. WP D removed code; WP F changed a
paint loop; WP E added a test.

## 9. Owner decisions — taken 2026-11-14

All seven were answered by applying the recommendation each one carried. Each
answer is recorded in the record that needed it, with its reasoning, and the
memo `002-owner-decisions.md` carries the options that were weighed.

| # | Decision | Taken | Effect |
|---|---|---|---|
| 1 | `tui-modularity/001` — criterion 6's structural reading | **A + C**: accepted structurally, criterion amended to "no production rendering file changed as a consequence of M001" | `001` → **closed** |
| 2 | `tui-modularity/008` — criterion 4 at 8 of 10 | **A**: eight is the end state, criterion and roadmap amended; the two fields named as a follow-up milestone. The characterisation matrix's before/after claim accepted on the same structural ground | `008` → **closed** |
| 3 | `tui-modularity/003` — the `permission.rs` title-badge exception | **A**: criterion 1 amended to "every surface whose title row needs no adjunct"; a `Section` title adjunct scoped as a follow-up | `003` → **closed** |
| 4 | `tui-modularity/002` + `/003` — the corpus diff-note substitute | **A**: accepted in writing, **and C was implemented** — a hosted content-row assertion now exists. For `003` that closes the condition. For `002` it turned the other half **red** | `003` → **closed**; `002` stays conditional |
| 5 | `helix/001` — the `space ?` box omits `_` and `Alt-d` | **B**: the box now reads `d / Alt-d / y` and carries `_ · trim the selection to whole lines` | `001` → **closed** |
| 6 | `helix/002` — the same box omits `Alt-d` | **B**: covered by the same edit | `002` → **closed** |
| 7 | `helix/009` — the single-chain pruning rule | **A + C**: ratified as implemented, and the prune is disclosed as `· history trimmed at its bound`, derived from the last edit rather than latched | `009` → **closed** |

**One decision did not survive its own evidence.** Decision 4's third leg — "a
hosted content-row assertion" — was accepted as satisfied in prose. Implementing
it showed that `HelpDialog::render_into` reserves `3 + 3 + Min(1) + 3` rows, so
at the 20×6 host rect the panel paints **no content row at all**. The substitute
is therefore accepted and its assertion is red, which is why
`tui-modularity/002` stays `conditionally closed` on exactly that condition. The
alternative — accepting a substitute whose measured leg fails — would have been
the silent pass this corrective exists to prevent.

## 10. Unresolved findings

| Severity | Finding | Where |
|---|---|---|
| **high** | **`ci.yml` is red on this branch on Linux for a reason that predates the narrowing.** `test_h006_real_background_job_is_observed_by_exact_id_while_work_continues` fails on `a976f7f3c` **and** on its A/B baseline `fe98d86ef` (runs `43518331648`, `43520172459`), identically: the background `cargo test` job is submitted and reported failed 1.2 s later, so the test observes zero running samples. The harness step runs before the workspace sweep, so a green sweep cannot be obtained from that job until this is fixed. The narrowing is **not** the cause — the baseline has no sandbox change | harness line; `dev/harness_lab/tests/test_sustained.py:372` |
| medium | **I4's `Keybind` verdict is a lower bound.** The measurement presses through `Component::handle_key`; production routes `DialogType::Keybind` to `App::handle_keybind_key` (`src/tui/app/mod.rs:4683-4686`) and returns. The other 13 `keymap_hint_line()` overrides are dispatched through `FocusManager` and are unaffected | `tui-modularity/006` §10 |
| medium | **The painted-row reader cannot tell a key from prose.** Surfacing every painted unreadable spelling produced **58** tokens: 23 prose (`Showing`, `Diff:`, `Policy:`, `Confirm`), 18 `PanelManager` panel names, and only the remainder key-ish (`Del`, `jk`, `↑↓`, `←→`, `Ctrl+R`, `1-3`, `1-22`, `?`, `▸`, `[Right]`). `painted` is load-bearing for the completeness cross-check, so the filter cannot simply be removed; the missing piece is a key-shapedness rule in `footer_parts` | this record §5; `baseline.rs` unchanged |
| medium | **`jk` is outside the measurement for a second reason:** it is declared by **panels** (`panels/session.rs:55`, `panels/agents.rs:250`, `chat_browser.rs:822,836`), and the I4 report's subject is the **dialog** fixture set | this record §5 |
| medium | **This plan's own §3 row for M008 was wrong.** `src/tui/commands/panels.rs` reads `App::sidebar_area` at ten production sites and `interactive_terminal.rs` reads `App::dialog_area` from a test helper, so the attribution holds. Corrected in the plan and in 008's record | plan §3 |
| medium | `tui-modularity/006`'s record said `corrective pass required` while its plan, roadmap and registry row said `conditionally closed`. Flipped; the plan did not list it | `tui-modularity/006` |
| medium | **The helix roadmap's §12 table was stale in five rows** — M004 said `conditionally closed`, M005–M007 said `ready for handoff`, M008 said `blocked` — while all nine records existed and eight of them read `conditionally closed`. A roadmap row that says a milestone is ready for handoff when its closure record exists is the contradiction the planning skill calls a closure defect. Reconciled here | `plans/subsystems/tui-editor-helix-capability-parity-roadmap.md` §12 |
| low | The plan told the implementer to delete keybind's `Widget` import, which does not compile; and counted three per-hunk offset sites where there are five. Both corrected in the plan | plan §6, §10 |
| low | `ci.yml` triggers on pull requests, pushes to `main` and dispatch — not literally push-only. The blocking fact stands (it has never seen this tree), the wording was corrected | `sandbox/002` |

## 11. Roadmap disposition

- **TUI modularity and render invariants** — 3 `closed` (001, 003, 008) and
  7 `conditionally closed` (002 on the Help 20×6 defect, plus 004, 005, 006,
  007, 009, 010);
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
- **TUI editor Helix capability parity** — 4 closed (001, 002, 004, 009) and 5
  conditional (003, 005, 006, 007, 008).

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
- **Six records were promoted to `closed` on 2026-11-14** by the owner
  decisions: `tui-modularity/001`, `/003`, `/008` and `helix/001`, `/002`,
  `/009`. Their registry rows, both roadmaps' §12 tables and the subsystem
  summaries now say `closed`.
- **Three follow-ups were registered** in the registry's *Deferred unregistered
  product work*: `App`'s last two frame-geometry fields, a `Section` title
  adjunct, and Help's degradation at small hosted sizes.
- **Two acceptance criteria were amended in their plans**, not only in the
  records: `tui-modularity/001` criterion 6 and `008` criterion 4 (with the
  roadmap's completion criterion 6).
- No record in the five lines was moved between the registry's *Blocked work*
  and *Recently closed or conditionally closed work* sections by this pass: all
  28 were already in the latter, and none changes section.
