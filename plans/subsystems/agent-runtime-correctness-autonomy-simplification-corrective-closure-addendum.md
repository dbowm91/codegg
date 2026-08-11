# Agent Runtime Correctness, Autonomy, and Simplification — Corrective Closure Addendum

Status: conditionally closed

Source roadmap:

- `plans/subsystems/agent-runtime-correctness-autonomy-simplification-roadmap.md`

Corrective implementation plan:

- `plans/implementation/agent-runtime-correctness-autonomy-simplification/010-recovery-state-strict-closure-corrective-pass.md`

Historical milestones under reconciliation:

- M005 — recovery and autonomy state-machine simplification
- M009 — integration, documentation, and closure
- PR #74 — `Close agent runtime correctness workstream`

Repository evidence reviewed:

- `main` baseline before this addendum: `5449aa2f589aa10d4e6eeda439b97d426506c759`
- PR #74 head: `7ae157e9c482760dac5c68b91146c5d36ad60a9a`
- PR #74 green predecessor candidate: `c51547011bab6d44b41f1ce3cc0a2aec8ddf28f0`, hosted `CI / verify` run `31515706555`

## 1. Purpose

This addendum preserves the original roadmap and M001-M009 implementation history while correcting the final closure state after post-implementation review found that M005's stated simplification contract was not fully realized in the production control flow.

M001-M004 and M006-M008 remain closed. Their accepted authority, workspace, turn/accounting, prompt, footprint, and CI outcomes are not reopened.

M005's implementation remains useful and substantially correct, but its closure record overstates completion because the generic loop still contains dead bootstrap/retry branches and a reachable repository-specific continuation path outside the claimed single `AutonomyState` continuation budget.

M009/PR #74 contains valid integration corrections, including the broker-principal fix and workspace fixture reconciliation, but its `closed` claim is not accepted as the final workstream closure authority until M010 closes the remaining recovery-state discrepancies.

## 2. Corrective findings

The corrective pass owns only these remaining findings:

1. physically delete the unreachable synthetic `list .` bootstrap implementation rather than leaving it behind `bootstrap_allowed = false`;
2. delete disabled `if false` narration/missing-tool retry branches in primary and follow-up loops;
3. eliminate the standalone repository-specific generic continuation that can request another provider turn without consulting the `AutonomyState` transition budget;
4. make primary/follow-up continuation scheduling obey one bounded state-machine contract;
5. consume typed tool execution status where the executor/broker already knows it instead of treating rendered model text as authoritative denial/timeout/cancellation state;
6. retain PR #74's broker principal correction and reconcile closure documentation only after the corrective tree passes focused and hosted verification.

No broader agent-loop refactor is authorized by this addendum.

## 3. Dependency and status correction

M010 is dependency-ready now.

Dependencies:

- M001-M008 are closed;
- PR #74/M009 supplies predecessor integration changes that should be retained or rebased equivalently;
- no external subsystem blocks implementation;
- final strict closure has one operational dependency: the existing hosted `verify` job must pass on the exact final M010 candidate.

The controlling sequence is now:

```text
M001-M008 closed
      |
      v
M009 / PR #74 integration candidate
      |
      | post-closure review found recovery-state mismatch
      v
M010 strict corrective closure
      |
      v
workstream closed only if M010 closure record is accepted
```

M009 is not deleted, rewritten, or renumbered. It remains predecessor integration evidence. M010 is the corrective closure milestone required by `plans/003-planning-process.md` section 7.

## 4. Verification posture

Verification remains intentionally minimal:

- focused recovery/agent-loop/harness tests for the corrected branches;
- `scripts/verify.sh quick` once the corrective tree is coherent;
- one existing hosted `CI / verify` run on the final candidate.

Do not add a new CI lane, matrix, static dead-code guard, scheduled audit, size gate, coverage/benchmark gate, artifact workflow, release automation, or fixed release cadence.

## 5. Closure rule

The workstream must remain active until `plans/closure/agent-runtime-correctness-autonomy-simplification/010-status.md` exists and demonstrates every acceptance criterion in M010.

Strict closure requires:

- no medium-or-higher unresolved finding in M010 scope;
- dead bootstrap/narration branches physically removed;
- all reachable autonomous provider-turn continuations bounded by the single autonomy authority;
- typed status used where available and string classification isolated to compatibility fallback only, if retained;
- PR #74's principal/workspace integration fixes retained;
- focused verification and `scripts/verify.sh quick` pass;
- the normal hosted `verify` job passes on the final corrective candidate;
- registry and this addendum are updated to `closed` only after that evidence exists.

If those conditions are met, no further corrective plan should be created for this line. Low-severity future model compatibility or dependency maintenance items remain deferred and unregistered unless separately prioritized.
