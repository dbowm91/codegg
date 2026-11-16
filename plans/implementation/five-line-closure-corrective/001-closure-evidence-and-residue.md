# Five-Line Closure Corrective C001 — Closure Evidence and Residue

Status: implemented — WP A, D, E, F and G landed; WP B and WP C blocked on WP 0
Closure record: `plans/closure/five-line-closure-corrective/001-status.md`

Repository baseline: `7642b9e38` on `feat-auto-20261109-f8abad0d`

> **Tree state at authoring.** 249 paths modified or untracked, **nothing
> committed and nothing staged**. `scripts/verify.sh full` passed on this tree
> at 2026-11-13 — 17,377 workspace tests and 8,441 feature-pass tests, 0
> failures. The render ratchets read `[contrast] = 0`, `[advertised_keys] = 4`
> (each with a `reason`), `[hint_truncation] = 0`; the comment ratchet is banked
> at 139,245 lines.
>
> **Why this plan exists.** Research on 2026-11-13 found that the five planning
> lines are *nearly* closed and that the remaining distance is not one kind of
> thing: some conditions are already met and still asserted as open, some need a
> commit, one is a **live security finding recorded as though it were evidence
> debt**, and two are worth deliberately **dropping**. This plan converts that
> finding into ordered work.

Source documents:

- `plans/closure/tui-modularity-and-render-invariants/013-deferred-items-research.md`
  — the four deferred items, priced and adjudicated.
- The evidence classification of all 26 `conditionally closed` records across
  the five lines (see §3).
- `plans/adrs/ADR-0062-bundled-palette-contrast-authority.md` and its
  `## Amendment (2026-11-13)` — the authority rule this plan's status work
  depends on.

## 1. Objective

Make the five lines' closure records **true**, and finish the residue that is
real work rather than evidence: correct the conditions that a green `full` has
already discharged, wire the Linux evidence so it cannot silently skip, implement
the one live security narrowing that is recorded as debt, and delete or test the
two TUI items worth doing — while **dropping** the two that research showed cannot
pay for themselves.

The line closes when every record's stated conditions are either met or named as
an owner decision, and when the sandbox evidence is produced by a run rather than
assumed.

## 2. Why this milestone is ready

Every input is measured rather than assumed:

- `run_full()` calls `run_quick()`, which calls `run_guards()`; `cargo fmt
  --check` is inside `run_guards` (`scripts/verify.sh:57-58,679-692,715`). So one
  green `full` discharges the gate class **tree-wide** — this is a code fact, not
  an inference.
- The 26 `conditionally closed` records were read individually and their
  conditions classified (§3). Nothing here rests on a record's own summary.
- The four deferred items were priced against the comment ratchet and the render
  corpus, and two of them were **refuted** rather than deferred (§5).
- The Linux evidence path was traced to `ci.yml`'s `ubuntu-latest` job, which
  builds the sandbox helper and runs the workspace sweep — so the debt is a
  commit and a missing marker, not a host.

No dependency on an unmerged branch, no external credential, no host that does
not exist.

## 3. Current implementation evidence

**The gate class is exhausted.** Across the 26 `conditionally closed` records,
every `scripts/verify.sh quick` / `full` / `cargo fmt --check` /
`check_comment_budget.py` condition is discharged by the 2026-11-13 run. About
**27 sentences** still assert the opposite. The substantive ones, each verified
against the tree:

| Record | Sentence now false | Truth |
|---|---|---|
| tui-modularity/001 | "`quick` cannot be green today: `check_comment_budget.py` reports 20 violations" | guard passes; corpus banked at 139,245 |
| tui-modularity/002 | "the docking defect means the panel manager is not in a state a closure record should call fully sound" | `dock` selects the borrow pair by index; the regression test passes |
| tui-modularity/008 | "blocked by `src/tui/commands/**`" | **this table's own claim was wrong.** `src/tui/commands/panels.rs` reads `App::sidebar_area` at ten production sites and `interactive_terminal.rs` reads `App::dialog_area` from a test helper, so the attribution **holds**; only 008's doc/comment gate rows were stale. Corrected during WP A against the tree, not against this table. |
| tui-modularity/009 | "`ListCursor`/`ScrollPolicy` have no doc comment — blocked by §5's measured overage" | the overage is gone; the stated blocker no longer exists |
| tui-modularity/010 | "`architecture/tui.md`'s delegation rule was not checked" | `architecture/tui.md:3272-3292` states it |
| advertised-keys/001 | "`architecture/tui.md` contains no I4 section" | `:4516-4575` — the rule, the retained rows, the hint-versus-footer rule |
| advertised-keys/002 | "WP C blocked: 16 of 18 footer sites change rendered bytes" | owner authorised the visible change; 35 rows converted |
| harness/001 | "`check_session_replay_fixtures.py` is **red** on this baseline" | it passes |
| helix/004 | "Neither `verify.sh quick` nor CI runs the guard's `--self-test`" | both do; 13 cases, including the case it said needed a decision |
| helix/005 | "`space j` … `UNAVAILABLE_SPACE_LEAVES` still records `j`, so the real run reports one rule-6 violation" | `j` is gone; the guard prints 4 reasons |
| tui-modularity/007 | record status `blocked` | `[contrast] = 0` via the M011 mapping; the roadmap and plan say conditionally closed |

**What a commit does *not* do.** Strictly **one** record is commit-only — helix
**004**. Two more are commit-only modulo one low documentation item — helix
**001**, helix **002**. The other **22** carry at least one non-gate residue: a
missing test, an unimplemented affordance, or a decision. A commit is a
precondition of this plan, not its completion.

**Linux evidence: five items are one CI run away, two are not.**

`ci.yml` runs on `ubuntu-latest`, builds the sandbox helper, and executes the
workspace sweep — so C002's kernel-scope assertions, C003's two comparison
commands, C004's control run and C006's containment family all execute there.
The blockers are that **nothing is committed** (CI is push-only) and that
**`CODEGG_SANDBOX_QUALIFICATION` is never set**, so `require_backend!` silently
returns when no backend is present — a runner regression would turn the sandbox
suite into a **green skip**.

Two items the lane does not cover at all: the committed three-run toolchain
matrix (no artifact exists and no CI step names it) and the `/proc`·`/var/run`
narrowing — which is **not evidence debt but an unimplemented control**:
`src/security/sandbox/policy.rs:393,395` still grants both as tree roots, so
`/proc/<other-pid>/environ` remains readable. That is a live High finding inside
a record marked "conditionally closed".

**Two items are worth dropping, and the research says why.** `/security-review`'s
eight verbs need 113–126 inner columns — a ≥192-column terminal, against a 90%
dialog cap that gives 90 columns at a 100-column frame, so **no layout choice
reaches it**. The I4 parser widening records 6–10 rows that are fixture gaps:
`TaskView`'s `Home`/`End` and `Queue`'s `[`/`]` are handled (not rows at all),
and `Home`/`PgUp` are inert *by construction* at a fresh mount.

**Two incidental findings with real defects in them.**

- **`Diff`'s `End` blanks the pane.** `scroll_to_bottom` sets the offset to
  `total_lines() − 1` while all three paint paths apply that offset *per hunk*
  (`dialogs/diff.rs:165,296,397`), so a two-hunk diff skips every hunk's body.
- **`spellings()` drops painted-but-unreadable spellings**, so `jk` sits outside
  even the documented blind spot; and `/keybind`'s `Component::handle_key` is
  bypassed in the modal path (`app/mod.rs:4683-4686`), so I4's Keybind verdicts
  measure a path the modal never uses.

## 4. Invariants that must not regress

- **The ratchets do not move** except where a work package explicitly re-banks
  and says why: `[contrast] = 0`, `[advertised_keys] = 4`, `[hint_truncation] = 0`.
- **No record is promoted to `closed` on a green gate alone.** `closed` requires
  every stated condition met; `conditionally closed` is the correct status for a
  record whose residue is an owner decision or evidence no run on this host can
  produce.
- **A status correction must not quietly delete a condition.** If a condition is
  met, say what met it; if it is superseded, say by what. Deleting the sentence
  without the reason is the failure this plan exists to prevent.
- **The comment ratchet is exact-value per file.** No work package may exceed a
  file's ceiling; where a change needs headroom, the manifest is edited
  deliberately in the same diff, never trimmed into silence.
- **Nothing in this plan touches a bundled theme file** (`assets/` stays
  byte-identical to `HEAD`).
- **ADR-0042 key-chord ownership and the command catalog are untouched.**

## 5. Scope

### In scope

- **Status truth** for the 26 conditional records and the two records whose
  status is stale in the restrictive direction.
- **CI evidence integrity**: the qualification marker, an explicit assertion that
  the runner can enforce ABI 6, and wiring the three-run toolchain matrix.
- **The `/proc`·`/var/run` narrowing** — the one live security control recorded
  as debt.
- **Delete `keybind.rs`'s `Widget for &KeybindDialog`** (131 lines, no caller).
- **A dispatch test for Export's `Esc`** — the verb is already right; the
  coverage is missing.
- **The `Diff` `End` bug.**
- **The two smaller findings** (`spellings()` blind spot; keybind's modal-path
  bypass) as recorded or fixed items.

### Explicitly out of scope, with the reason

| Dropped | Why |
|---|---|
| `/security-review`'s full item set | 113–126 inner columns needed; unreachable under the 90% dialog cap at any realistic terminal. A two-row footer fixes only the ≥80-column half and costs a new `HintLine` API. Recorded, not attempted. |
| The I4 parser widening | The rows are fixture gaps; two named surfaces are not rows; `Home`/`PgUp` are inert by construction. The real blind spot (`jk`) is closed by fixing `spellings()`, not by widening `key_of`. |
| Re-authoring any bundled theme | Owner instruction 2026-11-13: the Halloy themes are third-party and stay as they are. |
| The per-panel and per-dialog corpus diff notes (tui-modularity/002, /003) | They need a pre-change frame that no longer exists. The plan requires the **substitute** to be accepted in writing (§7 WP A) rather than the artifact invented. |
| Live recording for harness/001 WP-D | Needs a real provider credential and a committed fixture — external state, not work. |

## 6. Required production changes

### Core/domain, storage, migrations, protocol, DTOs

None.

### Runtime and concurrency

None.

### Security and authorization

- `src/security/sandbox/policy.rs` — narrow the `/proc` and `/var/run` tree
  grants, with the `/proc/<other-pid>/environ` denial fixture and its positive
  control.

### Frontend or operator surface

- `src/tui/components/dialogs/keybind.rs` — delete `Widget for &KeybindDialog`.
  **The `Widget` import stays**: `render_body` still calls `Paragraph::render`,
  which is a `Widget` trait method, so removing the import does not compile.
  Measured, not assumed.
- `src/tui/components/dialogs/diff.rs` — the `scroll_to_bottom` offset must be
  hunk-relative, or the paint paths must stop applying a total offset per hunk.
  **Five** sites applied it, not the three §3 counted: two in
  `src/tui/components/diff.rs` and three in the dialog.
- `tests/tui_render/invariants/baseline.rs` — `spellings()` was to stop silently
  dropping a painted spelling it cannot read. **Superseded by measurement**; see
  WP G. The file is unchanged.

### Documentation and static guards

- The 26 closure records' stale conditions, plus `tui-modularity/007`'s status.
- `.github/workflows/ci.yml` — set the qualification marker and assert the
  runner's ABI, or record why not.
- A new workflow or step for the three-run toolchain matrix.
- `docs/comment-budget.toml` — a deliberate bank for `keybind.rs` (see WP D).

## 7. Ordered work packages

**WP 0 — commit (owner action, precondition).** Nothing below can produce CI
evidence until the tree is committed and pushed. This plan assumes that happens
first; every Linux item is otherwise unobservable.

### WP A — Status truth

Intent: a record that says "not run" about a green run teaches the next reader
that the gate is unreliable.

Required changes: for each of the 26 conditional records, correct every condition
now discharged by the 2026-11-13 `full`, **naming what discharged it**. Promote
helix **004** to `closed` (its three conditions are met and its one medium finding
is addressed by the guard's new case). Promote helix **001** and **002** only if
their low documentation item is accepted as non-gating — record the acceptance
explicitly if so. Flip `tui-modularity/007`'s record from `blocked` to
`conditionally closed`, matching its plan and roadmap rows. Record the two
**decisions** the research surfaced rather than deciding them: M001's criterion 6
and M008's criterion 4 structural readings, M003's `permission.rs` exception, and
the corpus-diff-note substitute.

Acceptance evidence: no record asserts a gate that has not run; every corrected
sentence names its discharge; the four decisions are listed as owner questions,
not as resolutions.

### WP B — CI evidence integrity

Intent: make the sandbox evidence impossible to fake by skipping.

Required changes: set `CODEGG_SANDBOX_QUALIFICATION` in the Linux job so
`require_backend!` fails loudly instead of returning; add an assertion (or a
documented pin) that the runner enforces Landlock ABI ≥ 6, because a pre-6.12
kernel would **fail** the ABI-6 assertions while a backend-less runner would
**skip** them — the two failure modes are currently indistinguishable from a
green run; wire the committed three-run toolchain matrix (current / no `/proc` /
no `/var/run`) as a CI step.

Acceptance evidence: a run where removing the backend turns the suite red, quoted;
the matrix artifact committed and referenced from C002's record.

### WP C — the `/proc` and `/var/run` narrowing

Intent: the one live security control recorded as though it were evidence.

Required changes: narrow both tree grants in `policy.rs`; add the
`/proc/<other-pid>/environ` denial fixture **with a positive control** so the
fixture cannot pass by being unable to read anything.

Acceptance evidence: the fixture denies on Linux; the positive control reads a
path it should; the three-run matrix shows the narrowing's effect on both
directions.

### WP D — delete keybind's `Widget` impl

Intent: remove 131 lines that no production call site renders and that are wrong
in three ways the live path already fixed.

Required changes: delete `keybind.rs:456-586` and the `Widget` import at `:7`.
**The comment manifest must be edited in the same diff**: deleting 120 code lines
and 0 comment lines raises that file's comment/code ratio from 0.0608 to ≈0.0723
against a recorded 0.0676, so the ceiling has to move deliberately.

Acceptance evidence: `cargo test -p codegg --lib -- tui::components::dialogs::keybind`
unchanged; `check_comment_budget.py` green with the recorded ceiling and the
reason for the change.

### WP E — Export's dispatch test

Intent: the verb is right; nothing exercises the path.

Required changes: one app-level test through `App::new_for_testing` pressing
`Esc` in Keybind's Export mode and asserting it returns to the list rather than
closing. No production change.

Acceptance evidence: the test fails if `cancel_mode()` is replaced by `close()`.

### WP F — `Diff`'s `End`

Intent: a key that blanks the pane is a defect, not a verdict.

Required changes: make the offset hunk-relative, or stop applying a total offset
per hunk — whichever keeps `End` meaning "last line of the last hunk" and leaves
single-hunk behaviour byte-identical.

Acceptance evidence: a two-hunk diff where `End` leaves both bodies painted; the
existing single-hunk tests unchanged.

### WP G — the two smaller findings

**Outcome, both halves measured.**

The keybind bypass is **recorded** in `tui-modularity/006`'s §10 as a medium
finding: I4 presses `Keybind` through `Component::handle_key`, which the modal
path never reaches, so that one surface's verdict is a lower bound. The other 13
`keymap_hint_line()` overrides are dispatched through `FocusManager` and are not
affected.

`spellings()` **was changed and then reverted, with the measurement recorded.**
Making it surface every painted spelling it cannot read produced **58** tokens
across the dialog fixtures, and the classes say the reader is the problem, not
the filter: 23 are prose the row reader mistakes for a key (`Showing`,
`Diff:`, `Policy:`, `Confirm`, `Question`), 18 are `PanelManager`'s panel names
(`Chat`, `Context`, … `Terminal`), and only the remainder are key-ish
(`Del`, `jk`, `↑↓`, `←→`, `Ctrl+R`, `1-3`, `1-22`, `?`, `▸`, `[Right]`).
`painted` feeds the completeness cross-check — a surface that paints a key it
does not declare — so the filter is load-bearing there: without it every prose
row becomes a "key painted outside the declaration". The real defect is that
`footer_parts` has no key-shapedness rule, which is the same class of work as
the parser widening §5 drops. So the finding is recorded rather than forced, and
`jk`'s absence from the report has a second, separate cause: it is declared by
**panels** (`panels/session.rs:55`, `panels/agents.rs:250`,
`chat_browser.rs:822,836`), and the measurement's subject is the **dialog**
fixture set.

## 8. Failure, cancellation, restart, and contention semantics

Not applicable to WP A, D, E. WP B and C run only on CI's Linux job: a cancelled
run produces no evidence and must not be recorded as one. WP F is a synchronous
render change.

## 9. Compatibility and migration

None. No schema, protocol, storage or theme change. `assets/` stays byte-identical
to `HEAD`.

## 10. Required tests

- **WP A** — no test; the evidence is the corrected records against a green run.
- **WP B** — a negative-control run (backend removed) that turns the suite red.
- **WP C** — the denial fixture plus its positive control, on Linux.
- **WP D** — the existing keybind tests, unchanged.
- **WP E** — the dispatch test, mutation-proven against `close()`.
- **WP F** — a two-hunk `End` test, plus the existing single-hunk ones.
- **WP G** — a `spellings()` case with a painted unreadable key.

## 11. Required verification commands

Each selector below was verified against this tree. Integration tests are
**families**: `cargo test --test <family> -- <member>` selects one member, and
`cargo test --test <family>` runs all 1,237.

```bash
# WP D
cargo test -p codegg --lib -- tui::components::dialogs::keybind     # 4 tests
# WP E
cargo test -p codegg --lib -- tui::app                              # app-level dispatch
# WP F
cargo test -p codegg --lib -- tui::components::dialogs::diff        # 5 tests
# WP G
cargo test --test tui_render -- invariants::baseline                # 13 tests
# WP A (records only) + the whole-tree gate
python3 scripts/check_comment_budget.py
scripts/verify.sh quick
scripts/verify.sh full
```

**Linux evidence cannot be produced on this host.** `#[cfg(target_os = "linux")]`
modules are not compiled on darwin, so WP B and WP C are verified by a CI run and
**must be recorded as CI evidence with the run's URL or id**, never as "verified".

## 12. Documentation updates

- The 26 records (§7 WP A), plus `tui-modularity/007`'s status line.
- `plans/registry.md` — the rows whose status changes; `architecture/security.md`
  for the narrowed grants; `architecture/tui.md` for the `Diff` fix if it changes
  a documented gesture.
- `docs/comment-budget.toml` — the `keybind.rs` ceiling with its reason.
- `.github/workflows/ci.yml` — the marker, the ABI assertion, the matrix step.

## 13. Acceptance criteria

1. No conditional record asserts a gate condition that the 2026-11-13 `full`
   discharged, and every corrected sentence names its discharge.
2. Helix **004** is `closed`; helix **001**/**002** are either `closed` with the
   low item accepted in writing, or still conditional with the reason.
3. `tui-modularity/007`'s record status matches its plan and roadmap.
4. Four owner decisions are listed as questions, each with the record that needs
   them.
5. The Linux job fails rather than skips when no backend is present.
6. The ABI-6 precondition is asserted or pinned, not assumed.
7. `/proc/<other-pid>/environ` is denied on Linux, proven by a fixture **and** a
   positive control.
8. `keybind.rs`'s dead `Widget` impl is gone and the comment ceiling moved
   deliberately.
9. Export's `Esc` has a dispatch test that fails against `close()`.
10. `End` on a two-hunk diff leaves both bodies painted.
11. The ratchets are unchanged: `[contrast] = 0`, `[advertised_keys] = 4`,
    `[hint_truncation] = 0`.
12. `scripts/verify.sh full` is green on the final tree.

## 14. Stop conditions

- **WP 0 has not happened.** Then WP B and WP C cannot be verified, and the plan
  stops after WP A, D, E, F, G — reporting the Linux items as blocked on the
  commit rather than as done.
- **The runner cannot be made to fail on a missing backend** without changing
  test semantics for every other platform: stop and report, rather than leaving a
  suite that can skip.
- **Narrowing `/proc` breaks a legitimate tool** that reads another process's
  environment by design: stop; that is a capability decision, not a narrowing.
- **The `keybind.rs` deletion moves a rendered byte** (it should not — the impl
  has no caller): stop, because that means the impl is reachable and the research
  was wrong.
- **`End`'s fix cannot keep single-hunk behaviour byte-identical**: stop and
  report; a gesture change needs its own record.

## 15. Closure evidence required

A closure record per line, or one cross-line record, carrying: the corrected
status table, the CI run ids for WP B and C, the `/proc` fixture output **from
Linux**, the mutation evidence for WP E, and the corpus statement for WP F. The
four owner decisions are listed with their resolution or their open state — an
open decision is a legitimate closure condition, an unrecorded one is not.

## 16. Handoff notes

- **Order matters only at the front**: WP 0 gates B and C; A, D, E, F and G are
  independent of each other and of the commit.
- **The comment ratchet is the binding constraint on any code deletion**, which is
  counter-intuitive and worth stating in the record: removing 120 code lines
  *raises* a file's ratio. Every deletion needs its ceiling checked before it is
  proposed, not after.
- **Two items were refuted rather than deferred.** A future reader who finds
  `/security-review`'s item set or the parser widening in a residue list should
  read §5 before "finishing" them: both were priced and both lose.
- **The `/proc` finding is the reason this plan is not just documentation.** It
  was recorded as evidence debt for a day and is a live control gap; whoever
  picks this up should treat WP C as the first real work, not the last.
