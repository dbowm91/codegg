# Closure record — TUI editor Helix parity Milestone 018

Milestone: **M018 — the completion-trigger guard: `Ctrl-x` becomes visible**
Plan: `plans/implementation/tui-editor-helix-parity/018-completion-trigger-guard.md`
Roadmap: `plans/subsystems/tui-editor-helix-parity-roadmap.md` §7 (M018,
Amendments A1 and A2) and §13.4 question 5
Closed: 2026-10-19
Baseline: `9b04b59a` (`feat/auto-20261018-12255c45`)

## 1. The question, and the answer

Roadmap §13.4 question 5 asked:

> **Does M011's `Ctrl-x` need a guard?** `handle_completion_key` is outside
> the key-authority guard's scope, so the chord is invisible to it — the same
> blind spot the plans note for the leaf tables.

**Yes.** The answer is a **separate** guard, for the same reason M010's leaf
guard is a separate one: the completion popup is a surface *above* the
dispatch chain, with a modality the key guard's model does not have — a claim
that is conditional on a surface being open.

The guard is `scripts/check_tui_editor_completion_authority.py`. It changes
**no behaviour**: no key changed meaning, no chord moved, and the key guard's
counts are untouched.

## 2. What was actually invisible

`handle_completion_key` claims two kinds of key:

| Kind | Keys | Claimed when |
|---|---|---|
| the **triggers** | `F8` (any mode) and `Ctrl-x` (Insert mode only) | always — they *open* the popup |
| the popup's **own keys** | `Esc`, `Enter`, `Up`, `Down`, `j`, `k` | only while the popup is open |

The triggers are **editor chords**: `handle_completion_key` is called from
`handle_editor_key` (`src/tui/commands/editor.rs:2251`), and the editor's
dispatch chain — whose first stage is the guard block — runs later
(`src/tui/commands/editor.rs:2382`). A trigger therefore shadows anything
the guard block claims.

The popup's own keys are **not** editor chords: they are unreachable until
`completion.is_open()`, and their overlap with the Normal-mode verbs is *by
design* (`j`/`k` navigate the popup instead of the buffer). A guard that
counted both would report `j` and `k` twice and call it a defect.

**The boundary is structural, not a heuristic.** The triggers are the arms
**before** the popup-open check; the popup's own keys are after it. The guard
finds that check and never reads past it.

## 3. The guard

| Rule | What it polices |
|---|---|
| 1 | the `F8` trigger is claimed **exactly once** |
| 2 | the `Ctrl-x` trigger is claimed **exactly once** |
| 3 | a trigger **shadows neither** of the guard block's mode-independent chords (`Ctrl+s`, `Ctrl+r`) |
| 4 | `Ctrl-x` is claimed in **Insert mode only** |

Three design points are load-bearing:

1. **It reads the key guard's model, not the chain.**
   `scripts/check_tui_editor_completion_authority.py` imports
   `scripts/check_tui_editor_key_authority.py` as a module and asks it for the
   guard-block chords — exactly the relationship M010's leaf guard has. Two
   parsers for one dispatch shape would be two models of it.
2. **The shadowing direction is the one the code has.** The completion surface
   is claimed *before* the guard block, so a trigger claiming `Ctrl+s` would
   make the **guard block's** claim a lie. The modifier comparison uses the
   key guard's own `combos_intersect` helper, because Rust's `==` on a
   bitmask admits exactly one combination: `CONTROL` and `CONTROL | SHIFT` are
   disjoint, and flattening both into modifier *names* would make them look
   like they overlap.
3. **A shape the guard cannot resolve is an error (exit 2), not a skip.** The
   popup-open check must be found **exactly once**; a boundary the guard
   cannot find is a boundary it would guess, and a guess here would read the
   popup's own keys as triggers.

## 4. Non-vacuity

Two things make the guard non-vacuous.

**The model** asserts the function exists, that the popup-open check exists,
and that both triggers are found — so the guard cannot pass by the file being
emptied, the function renamed, the check moved, or the triggers deleted.

**The detector** is exercised by `--self-test`, which feeds synthetic sources
through the *same* analysis the real tree goes through:

```
  ok   the clean shape has no violations
  ok   a missing `F8` trigger is a rule-1 violation
  ok   a duplicated `F8` trigger is a rule-1 violation
  ok   a missing `Ctrl-x` trigger is a rule-2 violation
  ok   a duplicated `Ctrl-x` trigger is a rule-2 violation
  ok   a `Ctrl-x` that claims in Normal mode is a rule-4 violation
  ok   a trigger that shadows the guard block's `Ctrl+s` is a rule-3 violation
  ok   a trigger that shadows the guard block's `Ctrl+r` is a rule-3 violation
  ok   a shape the guard cannot resolve is an error, not a skip

self-test passed: 9 case(s).
```

The last two cases are the pair that matter most: the same `Ctrl+s` trigger
is a violation against a guard block that claims `Ctrl+s` and **not** one
against a guard block that claims `Ctrl+r`. The guard discriminates on the
chord, not on the presence of a shadow.

## 5. The injection proof

`/tmp/m018_inject_guard.py` injects into the real tree, runs the guard, and
restores the tree **byte-identically** (the backup is keyed by the relative
path — a flat basename backup clobbered `src/tui/editor.rs` once during the
M009 work).

```
=== injection: a trigger that shadows the guard block's Ctrl+s ===
--- exit 1 ---
ok: the guard fired on a trigger that shadows the guard block's Ctrl+s (['3'])
--- restored byte-identically (65863d67e5e2) ---
=== injection: a duplicated F8 trigger ===
--- exit 1 ---
ok: the guard fired on a duplicated F8 trigger (['1'])
--- restored byte-identically (65863d67e5e2) ---
=== injection: a Ctrl-x that claims in Normal mode ===
--- exit 1 ---
ok: the guard fired on a Ctrl-x that claims in Normal mode (['4'])
--- restored byte-identically (65863d67e5e2) ---
=== injection: a missing Ctrl-x trigger ===
--- exit 1 ---
ok: the guard fired on a missing Ctrl-x trigger (['2'])
--- restored byte-identically (65863d67e5e2) ---
```

All four rules fire on the real tree, and the tree came back byte-identically
every time.

## 6. The summary line

```
TUI editor completion-authority guard passed — 2 completion trigger(s)
(any modifier+F(8), CONTROL+'x'), each claimed exactly once, none shadowing
the guard block
```

The two triggers are named in the output, so the line says *what* is guarded
and not merely that something was.

## 7. Verification

The sweep ran against the final tree. `CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0`
throughout — the incremental cache in this worktree is unreliable.

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | exit 0 |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | exit 0 |
| `cargo test -p codegg --lib --locked --no-fail-fast` | **6838 passed / 2 failed / 4 ignored** — the two failures are the pre-existing load-sensitive `interactive_terminal` class; see the M017 closure record's §5.2 for the full evidence and the four runs that show it is load-sensitive and not this wave's |
| `cargo test -p codegg --lib --locked interactive_terminal` | **94 passed / 0 failed** |
| `python3 scripts/check_tui_editor_key_authority.py` | pass — **2 guard-block / 71 Normal / 8 Insert** |
| `python3 scripts/check_tui_editor_leaf_authority.py` | pass — **5 tables / 44 glyphs / 12 bindings** |
| `python3 scripts/check_tui_editor_completion_authority.py` | pass — **2 triggers**, each claimed once |
| `python3 scripts/check_tui_editor_completion_authority.py --self-test` | pass — **9 cases** |
| `bash scripts/verify.sh quick` | **"Quick verification passed."** |

The full sweep, including the whole `codegg` lib suite and every focused
subset, is recorded in the M017 closure record's §5 — M018 changed no
production code, so the suites are the same run.

**The sweep was not wholly green, and this record does not say it was.** The
whole-`codegg`-lib step failed with the two load-sensitive
`interactive_terminal` failures, which this milestone's diff cannot reach.
Everything else passed.

### 7.1 The counts did not move

The key-authority guard still reports **2 guard-block / 71 Normal / 8 Insert**
and the leaf guard still reports **5 tables / 44 glyphs / 12 bindings**. This
milestone adds a guard, not a chord: a milestone that changed a count would
have said so here.

## 8. Exit conditions

| Condition | Result |
|---|---|
| The guard sees `F8` and `Ctrl-x`, and the summary says so | met — the summary names both triggers |
| A duplicate trigger, a shadowed guard-block chord, a `Ctrl-x` in Normal mode, and a missing trigger each fail the guard | met — the four injections, each firing its own rule |
| The self-test proves the guard discriminates | met — nine cases, including the pair that differ only in the guard block's chord |
| The roadmap's question 5 is answered with the guard's evidence | met — §13.4 question 5, and Amendment A2 |
| **No behaviour changed** | met — no production code changed at all |
| The docs and their skills moved **together** | met — one commit |

**All three of the plan's §9 stop conditions did not fire.** The one that came
closest was the third — *"counting the triggers would make the key guard's
quoted counts mean something else"* — and the structural boundary is what
discharged it: the triggers are counted by a *separate* guard, so the key
guard's counts keep meaning exactly what they meant.

## 9. Recorded divergences

The plan's design was followed as written. Two things the plan did not spell
out, recorded here rather than left implicit:

1. **The trigger's modifier is modelled as exact bitmasks, not names.** The
   plan said "a `(modifiers, char)` chord"; the implementation keeps the key
   guard's distinction between the modifier *names* a claim admits and the
   exact *bitmasks* it admits, and compares with the key guard's own
   `combos_intersect`. A `CONTROL` claim and a `CONTROL | SHIFT` claim are
   disjoint in Rust's `==` on the bitmask, and flattening them into names
   would have made the shadowing rule fire on chords that cannot collide.
2. **The popup-open check must be found exactly once.** The plan said the
   triggers are "the arms before the popup-open check"; the implementation
   treats an ambiguous boundary as an **error** (exit 2) rather than picking
   one, because a guessed boundary would read the popup's own keys as
   triggers — which is the exact false positive the boundary exists to
   prevent.

## 10. The documents that moved

| Document | What changed |
|---|---|
| `plans/subsystems/tui-editor-helix-parity-roadmap.md` | §13.4 question 5 answered; M018's entry gained Amendment A2; the milestone table's row; §13.6's closing text; the status line |
| `plans/registry.md` | M018's row moved to `closed`; the roadmap-status row's status and closure list |
| `plans/implementation/tui-editor-helix-parity/018-completion-trigger-guard.md` | status and the closure pointer |
| `scripts/verify.sh` | the new guard, in `quick` |
| `.github/workflows/ci.yml` | the new guard, in the `verify` job |

No user-facing document moved, because no user-facing behaviour moved. That is
the point of the milestone, and it is why the skill files did not need to
change either: they describe the editor's keys, and the editor's keys are
the same.

## 11. Carried forward

- **The popup's own keys are still unguarded**, and deliberately so. They are
  a modal surface's keys, claimed only while the surface is up, and their
  overlap with the Normal-mode verbs is the point of a popup. A guard for them
  would report `j` and `k` as defects.
- **The third wave is done** — M016, M017 and M018 are all closed. M014 stays
  blocked on ADR-0034's acceptance, which is a decision the implementation is
  not free to make.
- **The change quartet's missing data source** — it needs its own milestone
  and its own plan.
- **The dirty-buffer merge** — unchanged and still deferred.
