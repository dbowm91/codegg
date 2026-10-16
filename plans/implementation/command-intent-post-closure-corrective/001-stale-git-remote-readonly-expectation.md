# Command Intent Corrective C001 — Stale Git Remote Read-Only Expectation

Status: active — implementation and hosted CI qualification

Repository baseline: `b6d334fe38837b0b0ea1e4a6880fee8b7ccafa3a`

Source corrective: `plans/subsystems/command-intent-post-closure-corrective-addendum.md`
Controlling process: `plans/003-planning-process.md#7-corrective-passes`
Architecture: `architecture/command_intent.md`

## Objective

Correct the stale test expectation and architecture prose for `git remote -v`.
The existing classifier returns `GitReadOnly`; this is the correct outcome for
a command that lists configured remotes.

## Invariants and scope

- Preserve the existing production classifier and routing behavior.
- Assert `GitReadOnly` for `git remote -v`; keep the no-subprocess assertion.
- Correct the architecture example while retaining the actual behavior of
  `git remote show origin`.
- Do not skip tests or alter CI scheduling, budgets, or test counts.

## Verification and closure

Run formatting and diff checks and obtain green hosted `CI / verify` on the
exact head with the complete workspace suite. Record the evidence and unblock
audit in `plans/closure/command-intent-post-closure-corrective/001-status.md`.

