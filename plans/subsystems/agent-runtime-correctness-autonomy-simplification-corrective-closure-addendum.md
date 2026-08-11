# Agent Runtime Correctness, Autonomy, and Simplification — Corrective Closure Addendum

Status: active

Source roadmap:

- `plans/subsystems/agent-runtime-correctness-autonomy-simplification-roadmap.md`

Corrective implementation plans:

- M010: `plans/implementation/agent-runtime-correctness-autonomy-simplification/010-recovery-state-strict-closure-corrective-pass.md`
- M011: `plans/implementation/agent-runtime-correctness-autonomy-simplification/011-typed-tool-outcome-and-hosted-closure-corrective-pass.md`

Historical milestones under reconciliation:

- M005 — recovery and autonomy state-machine simplification
- M009 — integration, documentation, and closure
- M010 — recovery-state strict closure corrective implementation; conditionally closed by historical record
- PR #74 — `Close agent runtime correctness workstream`, predecessor integration evidence

Current repository evidence:

- current reviewed `main` before M011 planning: `7d863763f700d936687ad01005e6a0d19b74c991`
- M010 implementation: `ea4136ff2d644a4eaaf3f97872f6efb61bfaed0d`
- M010 constructor/Clippy follow-up: `cbdc01508391e0cd71f74edb8f0c05634d309716`
- M010 historical closure record: `plans/closure/agent-runtime-correctness-autonomy-simplification/010-status.md`
- current hosted `CI / verify` run `31521674076`, job `93879950640`: failed at Workspace Clippy on the stale empty bootstrap unit test

## 1. Purpose

This addendum preserves the original roadmap and M001-M010 history while keeping the final closure state truthful as later production and hosted evidence becomes available.

M001-M004 and M006-M008 remain closed. M005 and M009 remain historical predecessor records whose overstatements were corrected by later passes rather than rewritten away.

M010 materially succeeded at its main structural objective: the synthetic repository bootstrap implementation, dead narration/missing-call branches, and repository-specific continuation bypass were removed, and primary/follow-up continuation scheduling was reduced to the bounded `AutonomyState` contract. Those corrections remain accepted production work.

M010 is not strict final closure authority. Its conditional closure record was authored when exact hosted evidence was unavailable. A later hosted run on current `main` now exists and failed at Workspace Clippy because an obsolete empty bootstrap test remained. Independent source review also confirmed that the ordinary tool executor still erases known `ToolError` status into rendered strings before recovery consumes it.

M011 is therefore the final narrow corrective closure milestone. It owns only the stale verification artifact, preservation of typed tool-execution status through the recovery boundary, and final exact hosted evidence.

## 2. Remaining corrective findings owned by M011

M011 owns exactly these findings:

1. delete the obsolete `autonomy_bootstrap_is_explicitly_one_shot` test that remained after M010 removed bootstrap state and now fails canonical hosted Clippy;
2. preserve typed native tool execution status before `Result<String, ToolError>` is converted into model-facing text;
3. map known permission and timeout errors to recovery `Denied` / `Timeout` without rendered-string inference;
4. use explicit timeout/cancellation status in MCP/question compatibility branches where the branch itself already knows the cause;
5. restrict `ToolExecutionOutcome::legacy` / `tool_execution_status(rendered)` to a concrete opaque compatibility seam, or delete the classifier if no such seam remains;
6. preserve all M010 structural recovery corrections, M009 broker-principal correction, and explicit workspace ownership;
7. obtain one normal green hosted `CI / verify` run on the exact final M011 candidate and create truthful final closure evidence.

No broader agent-loop, Tool Broker, MCP, scheduler, provider, ACP, storage, CI, or release refactor is authorized by this addendum.

## 3. Why another corrective record is required

`plans/003-planning-process.md` requires a newly discovered defect after closure review to be handled by a new corrective implementation plan rather than rewriting the prior milestone as though it succeeded completely.

M010's historical closure record remains valid evidence of what was known at that time: structural recovery cleanup was implemented and local focused verification passed, while exact hosted evidence was unavailable.

The later hosted run changes the evidence state. It is not missing evidence anymore; it is a concrete failed run. The source-level typed-outcome review also shows one M010 acceptance criterion remains incomplete in the ordinary production path.

M011 records and fixes those facts without reopening unrelated M010 scope.

## 4. Dependency and execution order

M011 is dependency-ready now.

Dependencies:

- M001-M010 production work remains present;
- no external subsystem blocks M011 implementation;
- M011 may change only the internal tool-result plumbing needed to preserve already-known execution status;
- one normal hosted `CI / verify` success on the exact final candidate is an operational dependency for strict closure.

The controlling sequence is:

```text
M001-M009 predecessor work
      |
      v
M010 structural recovery correction
      |
      | conditionally closed; later hosted Clippy failure + typed-outcome gap
      v
M011 typed outcome + hosted closure correction
      |
      v
workstream closed only if M011 closure evidence is accepted
```

M010 is not deleted, renumbered, or rewritten into success. `010-status.md` remains historical conditional evidence. M011 owns current disposition.

## 5. Verification posture

Verification remains deliberately small and mechanism-faithful:

- focused recovery/loop tests;
- `agent_loop_harness`;
- the exact workspace Clippy command used by hosted CI;
- `scripts/verify.sh quick`;
- `git diff --check`;
- one existing hosted `CI / verify` run on the exact final M011 candidate.

Run MCP/question-specific focused tests only if those branches are modified.

Do not add:

- a new CI lane or workflow-dispatch mechanism;
- a matrix or scheduled audit;
- static source-regex guards for the deleted test;
- cargo-audit, coverage, binary-size, fuzz, or benchmark gates;
- artifact publication or release automation;
- an all-features campaign solely for M011.

The separate all-features findings recorded by the Agent Runtime / Model Adaptation / ACP M017 closure are outside this workstream unless one reproduces in the ordinary default hosted `CI / verify` path.

## 6. Closure rule

The workstream remains `active` until `plans/closure/agent-runtime-correctness-autonomy-simplification/011-status.md` exists and is accepted.

Strict closure requires all of the following:

- the stale empty bootstrap test is deleted rather than silenced;
- canonical workspace Clippy passes;
- ordinary native execution preserves known `ToolError` classification into recovery;
- `ToolError::Permission` reaches recovery as `Denied` without string inference;
- `ToolError::Timeout` reaches recovery as `Timeout` without string inference;
- successful output cannot be reclassified by denial/timeout/cancellation words in its text;
- any remaining legacy rendered-text classifier is isolated to a concrete opaque compatibility owner and justified/tested, or removed entirely;
- M010's deleted bootstrap/dead branches and one-continuation invariant remain intact;
- M009 broker principal and explicit workspace corrections remain intact;
- focused verification and `scripts/verify.sh quick` pass;
- the normal hosted `CI / verify` job passes on the exact final M011 candidate and reaches the workspace-test step;
- `011-status.md` records failed predecessor run `31521674076` as well as the final green run;
- no critical, high, or medium finding remains in M011 scope.

Only after those conditions are satisfied may this addendum and the registry move to `closed`.

If the final hosted run fails solely on a newly demonstrated unrelated subsystem defect, M011 must record the exact failure and ownership rather than broadening this pass or representing the run as green. Strict workstream closure remains conditional/blocked until the governing acceptance criteria can be met truthfully.

## 7. Finality

M011 is intended to be the final corrective pass for this workstream. Do not create another follow-up merely for cosmetic cleanup or broader agent-loop refactoring.

A further corrective plan is justified only if implementation or final hosted evidence demonstrates a new correctness/authority defect that cannot be resolved within M011's narrow result-plumbing and verification boundary.