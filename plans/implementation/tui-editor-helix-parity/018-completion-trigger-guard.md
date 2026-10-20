# TUI editor Helix parity Milestone 018 — the completion-trigger guard

Status: implemented - closed 2026-10-19
Closure record: `plans/closure/tui-editor-helix-parity/018-status.md`
Repository baseline: `9b04b59a` (`feat/auto-20261018-12255c45`) — M016's commit, which this milestone builds on

Source roadmap: `plans/subsystems/tui-editor-helix-parity-roadmap.md` §7
(a new milestone added by the M018-planning amendment; see the roadmap's
Amendment A1) and §13.4 question 5 (the open question this milestone
answers) and §13.6 (the third wave's order)
Long-term requirements: `plans/000-long-term-specification.md` (the editor's
keymap is a contract, not an implementation detail)
Applicable ADRs: none — this milestone writes a guard, not a behaviour
Primary class: infrastructure

## 1. Objective

Make the completion triggers **visible to a guard**.

`handle_completion_key` sits outside the key-authority guard's scope, so the
`Ctrl-x` chord M011 bound is invisible to it. That is the same blind spot the
leaf tables had, and M010 closed that one with a **separate** guard
(`scripts/check_tui_editor_leaf_authority.py`) rather than by widening the
key guard. This milestone does the same thing here.

The question the roadmap asked — *"Does M011's `Ctrl-x` need a guard?"* — is
answered by writing one.

## 2. What is actually invisible

`handle_completion_key` claims two kinds of key:

| Kind | Keys | Claimed when |
|---|---|---|
| the **triggers** | `F8` (any mode) and `Ctrl-x` (Insert mode only) | always — they *open* the popup |
| the popup's **own keys** | `Esc`, `Enter`, `Up`, `Down`, `j`, `k` | only while the popup is open |

The triggers are **editor chords**: they are claimed before any mode dispatch
decides otherwise, and they shadow anything the global handler would have
taken. The popup's own keys are **not** editor chords — they are a modal
surface's keys, claimed only while the surface is up, and they overlap the
Normal-mode verbs *by design* (`j`/`k` navigate the popup instead of the
buffer).

A guard that counted both would report `j` and `k` twice and call it a
defect. So the guard must tell the two apart, and the way it tells them apart
is structural: **the triggers are the arms before the popup-open check**.

That is a real structural distinction, not a heuristic: the popup's keys are
unreachable until `completion.is_open()`, and the triggers are the arms that
make it open.

## 3. The guard's invariants

**Violations** (exit code 1):

- **The `F8` trigger is not claimed exactly once.** It is the Normal-mode
  trigger, and a second claim would make one of them unreachable.
- **The `Ctrl-x` trigger is not claimed exactly once.** Same failure, other
  spelling.
- **A trigger shadows a guard-block chord.** The key-authority guard's two
  mode-independent chords (`Ctrl+s`, `Ctrl+r`) are claimed before the editor's
  dispatch chain runs. A trigger that claimed either would be dead in the one
  sense that mattered — and the trigger's own arm would be the lie.
- **`Ctrl-x` is claimed in a mode it does not serve.** The arm's own comment
  says it: *"in normal mode `Ctrl-x` is not the editor's to take, and a chord
  the editor claims in a mode it does not serve is a chord the global handler
  can no longer reach."* The guard checks the predicate, because a comment is
  not a mechanism.

**Not violations, by design:**

- The popup's own keys (`Esc`, `Enter`, `Up`, `Down`, `j`, `k`). They are
  claimed only while the popup is open, and their overlap with the Normal-mode
  verbs is the point of a popup.
- The completion **request** machinery (`Ctrl-x`'s request id, the epoch, the
  scope) — that is M003's contract, not a key claim.

## 4. Why a separate guard and not a wider key guard

The key-authority guard reads **one dispatch chain**: the guard block, then
`handle_normal_key`, then `handle_insert_key`. It explicitly excludes
`handle_editor_key`'s pane gestures as *"a separate stage above this file's
dispatch chain"*. The completion popup is the same kind of thing: a surface
above the chain, with its own keys.

Teaching the key guard a fifth stage would mean teaching it the popup's
modality — that a claim is conditional on a surface being open — which is a
different model from the one it has. The leaf guard made the same call, and
the leaf guard reads the key guard's **model** (its guard-block chords)
rather than re-parsing the chain. This guard does the same: it imports
`scripts/check_tui_editor_key_authority.py` and asks it for the guard-block
chords.

## 5. Scope

**In:**

- `scripts/check_tui_editor_completion_authority.py`, with a self-test,
- its registration in `scripts/verify.sh quick` and the CI `verify` job,
- the roadmap's answer to §13.4 question 5,
- the closure record.

**Out:**

- **Changing the editor's behaviour.** No key changes meaning. This milestone
  makes the existing shape visible.
- **Teaching the key guard about the popup.** Its model is about one dispatch
  chain.
- **A guard for the popup's own keys.** They are not editor chords.

## 6. Ordered work packages

### W1 — The guard's model

Read `handle_completion_key` from `src/tui/commands/editor.rs`. Find the
**popup-open check** — the early return that says the popup is not open — and
treat every arm *before* it as a trigger arm. Every arm *after* it is a popup
key, and is excluded by construction.

Each trigger arm resolves to a claim: a `(modifiers, char)` chord or a
`(modifiers, key code)` claim. A shape the guard cannot resolve is an **error**
(exit 2), not a skip — the same rule the key guard follows, because a guard
that skips what it does not understand passes by not looking.

### W2 — The invariants

The four violations in §3, plus the model assertions that make the guard
non-vacuous by construction: the function exists, the popup-open check exists,
and both triggers are named in the guard's expectations.

### W3 — The self-test

Synthetic sources through the same analysis the real tree goes through:
a missing trigger, a duplicated trigger, a trigger that shadows a guard-block
chord, a `Ctrl-x` that claims in Normal mode, a shape the guard cannot
resolve, and the clean shape.

### W4 — The registration and the answer

`scripts/verify.sh quick`, the CI `verify` job, and the roadmap's §13.4
question 5 answered with the evidence.

## 7. Required verification commands

```bash
export CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
python3 scripts/check_tui_editor_key_authority.py
python3 scripts/check_tui_editor_leaf_authority.py
python3 scripts/check_tui_editor_completion_authority.py
python3 scripts/check_tui_editor_completion_authority.py --self-test
bash scripts/verify.sh quick
```

No production code changes, so the test suites are unchanged; the sweep still
runs them, because a guard that fails is a failed milestone.

## 8. Acceptance criteria

1. The guard sees `F8` and `Ctrl-x`, and the summary says so.
2. A duplicate trigger, a shadowed guard-block chord, a `Ctrl-x` that claims
   in Normal mode, and a missing trigger each fail the guard.
3. The self-test proves the guard discriminates.
4. The roadmap's question 5 is answered with the guard's evidence.

## 9. Stop conditions

Report rather than improvise if:

- the popup's own keys cannot be separated from the triggers structurally,
- the trigger arms' shapes need a new arm model the guard cannot resolve,
- counting the triggers would make the key guard's quoted counts mean
  something else.

## 10. Closure evidence required

- the self-test's cases,
- the injection proof on the real tree,
- the guard's summary line,
- the roadmap's answer.

## 11. Handoff notes

- This milestone changes **no behaviour**. It makes an existing shape
  visible, which is what a guard is for.
- The leaf guard and this one are siblings: each reads a surface the key
  guard does not, and each reads the key guard's model rather than
  re-parsing the chain.
