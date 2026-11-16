# TUI Advertised Keys — Post-Closure Corrective Addendum

Status: conditionally closed — C001 and C002 conditionally closed; their remaining work (the footer dialect, the parser widening and its fixtures) is named in each record

Parent roadmap:

- `plans/subsystems/tui-modularity-and-render-invariants-roadmap.md`

Corrected milestone:

- M006 "Every advertised key is handled" —
  `plans/implementation/tui-modularity-and-render-invariants/006-advertised-keys-are-handled.md`
- Closure record (status `corrective pass required`):
  `plans/closure/tui-modularity-and-render-invariants/006-status.md`

Milestones in this corrective:

- C001 — `plans/implementation/tui-advertised-keys-corrective/001-i4-baseline-to-its-irreducible-residue.md`
- C002 — `plans/implementation/tui-advertised-keys-corrective/002-structured-pairs-from-the-producer.md`

Repository baseline: `7642b9e38` on `feat-auto-20261109-f8abad0d`, worktree
uncommitted; `scripts/verify.sh full` passed at 2026-11-12 (17,326 workspace
tests + 8,400 feature-pass tests, 0 failures; render family 1227 passed).

## 1. Why this addendum exists

M006 closed `corrective pass required` because its I4 exit condition — the
`[advertised_keys]` baseline holding "exactly the retained entry" — was not
met: the baseline holds **61** entries across **18** surfaces.

The corrective research re-derived all 61 from the source and found that the
**number is not a count of defects**. Of 61:

| Cause | Entries | What it means |
|---|---|---|
| the probe could not exercise the key | **53** | 46 mounted the surface with an empty fixture, 2 started at a boundary, 5 depended on probe ordering. The surface is correct; the measurement is not. |
| the key has no arm | 4 | two stale `^d/^u` hints where the handler and the same surface's footer already agree on `PgUp`/`PgDn` |
| the key is handled, but not as advertised | 4 | `Connect`'s `Enter` sets an error the render never paints; two `Tab` conflicts intercepted by `FocusManager` |
| stale advertisement / no longer reproduces | 0 | — |

So the milestone's central number was measuring the harness, and a corrective
that treated the 61 as 61 repairs would have sent an implementer into correct
code. C001 makes the measurement inspectable and state-fresh; C002 converts it
to the structured pairs the plan's §2 specified.

## 2. The decision this corrective cannot take itself

Two surfaces — `Model` and `WorkspaceDashboard` — advertise `Tab`, and neither
can receive it: `FocusManager::handle_key` intercepts `Tab` before the dialog
sees it, and `handle_tab` never forwards. That is the ADR-0042 ownership
contract working as designed, and it is **two** entries, not the one the
closure record names.

Reaching a single entry therefore requires de-advertising one of two identical
conflicts, which is arbitrary; reaching zero requires either an ADR-0042
amendment permitting a dialog to pre-empt `Tab`, or de-advertising both. M006's
acceptance criterion 5 is amended to *"the recorded residue, each entry
carrying a reason"*, and "reach one entry" is a **stop condition** for the
owner, not work for the implementer.

## 3. Invariants this corrective must not regress

- The render corpus stays green (1227 members at the measured baseline).
- `[contrast]` and `[hint_truncation]` (0) are untouched by **this corrective**;
  it is the I4 set only. (`[contrast]` does move under M007's own work — it went
  2,495 → 1,448 — but not from anything the advertised-keys corrective did.)
- The hint vocabulary scan's floor stays at its measured 32 surfaces, and
  `HintLine` remains the sole producer of a rendered hint row.
- ADR-0042 ownership is unchanged unless the owner amends it.
- The two key-authority guards report unchanged counts.

## 4. Verification

Each milestone plan carries its own commands with expected output. The
cross-milestone gate is:

```bash
cargo test --test tui_render -- invariants::baseline   # recorded set reproduces
cargo test --test tui_render -- invariants::vocabulary # floor still 32
cargo test --test tui_render                           # 1227 passed, 0 failed
python3 scripts/check_tui_render_invariant_baseline.py
python3 scripts/check_tui_render_invariant_baseline.py --self-test
scripts/verify.sh quick
```

## 5. What closes this addendum

C001 and C002 each with a closure record under
`plans/closure/tui-advertised-keys-corrective/`, the amended criterion 5
satisfied by a residue whose every entry names a reason, and the `Tab` decision
recorded either as an ADR-0042 amendment or as two de-advertisements.
