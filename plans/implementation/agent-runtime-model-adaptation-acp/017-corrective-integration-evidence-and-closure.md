# Agent Runtime, Model Adaptation, and ACP Milestone 017 — Corrective Integration Evidence and Closure

Status: ready for handoff

Repository baseline: `7d8657e60aad85f677144b1bd0e7fb5d2929faa3`

Source corrective addendum:

- `plans/subsystems/agent-runtime-model-adaptation-acp-corrective-closure-addendum.md#milestone-017--corrective-integration-evidence-and-closure`

Corrective disposition to replace:

- `plans/closure/agent-runtime-model-adaptation-acp/011-corrective-status.md`

Primary class: closure

## 1. Objective

Perform an independent production-path audit of corrective Milestones 012–016, run focused mechanism-faithful fixtures, reconcile architecture and planning records, and determine whether the Agent Runtime, Model Adaptation, and ACP subsystem may return from conditionally closed to strictly closed.

This milestone owns evidence and disposition, not another feature implementation pass. Any newly discovered critical/high/medium production finding must produce a precise corrective follow-up rather than being waived, hidden behind focused tests, or relabeled as unrelated without evidence.

## 2. Dependencies

Hard dependencies:

- strict closure records for M012 ACP lifecycle/correlation correctness;
- M013 specialized finalization/research coordination;
- M014 canonical prompt/context convergence;
- M015 adapter-driven reasoning safety;
- M016 descendant admission/cancellation/execution context.

The implementation commits and closure records must identify exact reviewed heads. Do not begin from plan-file status alone.

## 3. Required audit questions

### ACP

- Does every prompt bind to exactly one native submission and turn?
- Can cancel/close before `TurnStarted` be lost?
- Can stale, replayed, or same-session neighboring events bind or terminate the active prompt?
- Does load/replay preserve supported user/assistant/tool roles and omit private content?
- Does close/EOF/shutdown release subscriptions/correlation state and suppress later updates?
- Is stdout protocol-pure under success, error, cancellation, and diagnostics?

### Specialized runtimes

- Is local parsing/validation authoritative after provider completion?
- Can unsupported security findings leave as confirmed findings?
- Does research actually execute bounded host-owned child tasks, or merely prompt the model to do so?
- Are child reports typed evidence records with narrow authority?
- Are sources/claims/evidence/conflicts/citations/minimum-evidence locally validated?
- Does finalizer failure prevent a successful native terminal state?

### Prompt/context

- Are all behavior-affecting blocks assembled before compilation?
- Is any production system-string mutation performed after compiler fingerprinting?
- Does plan-mode guidance appear once?
- Do root and descendants use the same typed block/order/fingerprint contract?
- Do adapter, tool surface, asset, agent, specialized mode, reasoning mode, and effective context separate cache identity?
- Are required protocol/evidence blocks protected from omission?

### Model adaptation and reasoning

- Can multibyte private reasoning near the bound panic or become invalid UTF-8?
- Are request transforms selected solely from the resolved adapter?
- Do aliases/exclusions/custom model IDs behave according to adapter resolution rather than substrings?
- Are inbound aliases canonicalized before permission/execution?
- Does private reasoning remain absent from public serialization, ACP, projections, logs, diagnostics, and error bodies?

### Descendant/execution ownership

- Can concurrent enqueue oversubscribe active-descendant capacity?
- Are reservations released exactly once on every terminal/rejection path?
- Does root cancellation affect only its lineage while global shutdown affects all?
- Does every production native tool dispatch use explicit workspace context?
- Is any process-global cwd still used as agent/tool execution authority?
- Are durable AgentRun/worktree/team capabilities still stated as deferred?

## 4. Invariants required for closure

- One daemon-owned execution authority at every boundary.
- One prompt compiler, resolved tool surface, adapter policy, and context plan per turn.
- One shared bounded descendant admission/cancellation owner.
- One ACP adapter state machine correlated to native turns, not session heuristics.
- Security/research local validation cannot be bypassed by provider output.
- Private reasoning remains opaque provider round-trip state.
- Required context/tool protocol state remains lossless.
- Broad verification status is described truthfully and reproducibly.
- No critical/high/medium finding remains in addendum scope.

## 5. Scope

### In scope

- Independent review of M012–M016 code and closure records.
- Production call-site tracing from native turn submission through provider/tool/child/finalizer/projection/ACP completion.
- Focused unit/integration/process/static verification.
- One canonical broad local workspace verification attempt under repository-prescribed resource bounds.
- Reproduction and ownership attribution for any broad failure.
- Documentation/planning/registry reconciliation.
- Final closure record or precise follow-up plan.

### Explicitly out of scope

- New product capabilities.
- Durable AgentRun/worktree/team implementation.
- Live external model/editor/search/scanner requirements.
- CI/release expansion.
- Refactoring unrelated code for style.
- Fixing unrelated subsystem failures unless they invalidate evidence for this subsystem.

## 6. Required evidence matrix

Create a table mapping every corrective acceptance criterion to:

- production file/function;
- focused test/fixture;
- exact command and result;
- reviewed implementation commit;
- reviewer disposition;
- remaining limitation.

At minimum include:

1. ACP pre-ID cancel, close, stale-event isolation, terminal uniqueness, role-correct replay, stdout purity.
2. Security malformed/unsupported/valid finalization.
3. Research host-owned child execution, typed evidence, dedupe/conflict/citation/minimum-evidence validation.
4. Complete root/child prompt block inventory, no post-compile mutation, one plan contract, cache-key separation.
5. UTF-8 reasoning boundaries, adapter-driven transforms, alias/exclusion behavior, privacy negatives.
6. Atomic admission, release matrix, root-cancel isolation, global shutdown.
7. Two-project explicit tool cwd/workspace ownership and static guards.

A closure claim without production call-site evidence is insufficient even if a helper/unit test exists.

## 7. Ordered work packages

### Work package A — Baseline and closure-record audit

- identify exact current head and implementation/closure commits;
- verify plan/registry statuses and predecessor closure records;
- inspect post-closure merges for code drift;
- list every acceptance criterion before running tests.

Acceptance evidence:

- immutable reviewed baseline;
- no stale closure hash/status;
- complete audit checklist.

### Work package B — Production-path tracing

Trace representative flows:

- ACP new → prompt → pre-ID cancel → matching turn → terminal;
- security prepare → ordinary loop → local finalize → projection/ACP terminal;
- research classify → coordinate children → aggregate → synthesize → validate → terminal;
- root and nested prompt/context construction → provider request → cache identity;
- Laguna reasoning/tool round trip through adapter policy;
- concurrent descendant admission/cancel/release;
- root and child native tool dispatch in two workspaces.

Acceptance evidence:

- one authority at each seam;
- no helper-only closure claim;
- no parallel legacy production path.

### Work package C — Focused verification

Run the exact focused suites from M012–M016 closure records, including process-level ACP, specialized-runtime fixtures, prompt/context convergence, provider transcripts, subagent concurrency/cancellation, and static ownership/privacy guards.

Acceptance evidence:

- exact pass/fail counts and commands;
- failures reproduced and classified, not omitted.

### Work package D — Broad verification and attribution

Run the repository's canonical broad local command under documented resource bounds, for example the current prescribed workspace library/quick verification command. If it fails:

- reproduce minimally;
- determine whether M012–M016 caused or expose the failure;
- record owning subsystem/file/test and exact error;
- fix it only if in scope and bounded;
- otherwise keep this subsystem conditionally closed if the failure prevents trustworthy evidence, or record a precise unrelated blocker if focused production evidence remains independently sufficient under the planning process.

Do not state that broad verification is green when it aborts, is skipped, or is blocked by packaging/dependency issues.

### Work package E — Documentation and final disposition

- update architecture docs to match production behavior;
- reconcile original roadmap, corrective addendum, registry, implementation statuses, and closure references;
- write `plans/closure/agent-runtime-model-adaptation-acp/017-status.md`;
- mark subsystem closed only if strict criteria are met;
- otherwise write/register the next narrow corrective plan.

## 8. Required integration scenarios

### Scenario A — ACP cancellation race

Submit a prompt, cancel before the native turn ID is visible, then deliver interleaved stale/current turn events. The current turn is cancelled once; stale events produce no update/terminal; one terminal ACP response is emitted.

### Scenario B — Security local validation

A provider returns one evidence-backed finding and one unsupported finding despite a requested schema. Local finalization accepts only the supported item and records the other as a review prompt/gap or fails according to contract.

### Scenario C — Research coordination

A comparative request generates bounded scouts/verifier, duplicate and conflicting sources, one malformed branch, and a final synthesis. Host aggregation deduplicates sources, retains conflict/limitation, rejects fabricated citation, and enforces minimum evidence.

### Scenario D — Prompt/cache convergence

Equivalent root and child turns receive the same shared contracts; memory/goal/LSP/Git/specialized evidence are fingerprinted before request creation; plan guidance appears once; changing adapter/tool/evidence/mode changes cache identity.

### Scenario E — Laguna reasoning safety

A multibyte reasoning stream crosses the byte limit without panic. An alias resolved to the Laguna adapter round-trips private reasoning; an excluded model containing the same substring does not. No public output contains the reasoning body.

### Scenario F — Descendant contention and isolation

Concurrent enqueue at the configured active limit cannot oversubscribe. Cancelling root A interrupts A descendants but not root B. Every lease releases. Root and child tools execute in their explicit, distinct workspaces despite process cwd changes.

## 9. Required verification commands

Use exact repository commands current at implementation time. At minimum:

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
cargo test --test acp_stdio -- --nocapture
cargo test --test session_projection_transport -- --test-threads=4
cargo test --test security_review_runner -- --test-threads=4
cargo test --test security_review_receipt -- --test-threads=4
cargo test --test agent_loop_harness -- --test-threads=4
cargo test --test subagent -- --test-threads=4
cargo test --test context_plan_convergence -- --test-threads=4
cargo test --test provider_transcripts -- --test-threads=4
cargo test --test event_processor -- --test-threads=4
python3 scripts/check_daemon_cwd_usage.py
python3 scripts/check_project_agent_pwd_inference.py
python3 scripts/check_scheduler_bypass.py
python3 scripts/check_execution_ownership.py
python3 scripts/check_tool_broker_boundary.py
python3 scripts/check_builtin_agents.py
python3 scripts/generate_builtin_agents.py --check
scripts/check_projection_disclosure.sh
scripts/check_projection_publication_seam.sh
python3 scripts/check_projection_transport_isolation.py
python3 scripts/check_projection_transport_lifecycle.py
scripts/verify.sh quick
```

Then run one canonical broad local workspace command with bounded jobs/test threads. Record the exact command and full disposition. Do not add clippy, external services, or a new CI matrix unless the repository's current verification contract already requires them.

## 10. Failure and evidence semantics

- A failed focused production fixture blocks closure until corrected or proven stale/invalid with an updated fixture.
- A missing test target is not a pass; reconcile the command with actual repository targets.
- A broad crash/abort is recorded as failed evidence, not silently ignored.
- An unrelated failure must include reproducible command, error, current owner, and explanation of why it does not invalidate this subsystem's production evidence.
- Optional live provider/editor validation is supplemental only and cannot substitute for deterministic fixtures.
- Closure review must be separate from the implementation agent/commit series where practical under repository convention.

## 11. Documentation reconciliation

Audit and update:

- `architecture/acp.md`;
- `architecture/agent.md`;
- `architecture/cache-aware-context.md`;
- `architecture/model-adapters.md`;
- `architecture/provider.md`;
- tool/scheduler/workspace/config documentation;
- original roadmap and corrective addendum statuses;
- M011 corrective status and M012–M017 closure records;
- `plans/registry.md`.

Remove claims that are no longer true, including helper-only finalization, prompt-only research coordination, model-substring adapter authority, session-only ACP correlation, or process-cwd tool ownership.

## 12. Acceptance criteria

- Every M012–M016 acceptance criterion has production evidence.
- All required focused tests and static guards pass.
- ACP lifecycle/correlation is race-safe and role-correct.
- Security/research local finalization is authoritative.
- Research child execution/evidence aggregation is host-owned and bounded.
- Prompt/context/cache identity is complete before provider execution.
- Reasoning handling is UTF-8 safe, adapter-driven, and private.
- Descendant admission/cancellation/context ownership is exact and isolated.
- Broad verification is truthfully documented.
- No unresolved critical/high/medium finding remains in addendum scope.
- Planning/architecture/registry records agree.

## 13. Closure outcomes

### Strictly closed

Use only when all acceptance criteria pass and no high/medium finding remains. Mark the corrective addendum and subsystem closed and record deferred AgentRun/worktree/team items explicitly.

### Conditionally closed

Use when substantial implementation is correct but a named medium correctness/evidence finding remains. Register a precise follow-up; do not leave the blocked-work table empty.

### Blocked

Use when external/unrelated verification prevents a trustworthy decision. Name the owner, reproducer, and unblock condition.

### Rejected

Use when the implementation introduces a second authority path, privacy leak, unsafe cancellation/admission semantics, or other high-severity regression. Reopen the relevant milestone with a corrective plan.

## 14. Stop conditions

Stop and report rather than expanding implementation if:

- a finding belongs to durable AgentRun/worktree/team scope;
- closure requires live external providers/editors/search/scanners;
- broad failure belongs to development-verification/release and cannot be fixed narrowly;
- repository history changed materially after the audited baseline and implementation commits cannot be isolated;
- a new protocol/storage architecture decision requires an ADR.

## 15. Required closure record contents

`plans/closure/agent-runtime-model-adaptation-acp/017-status.md` must include:

- exact reviewed head and implementation commits;
- requirement-to-evidence matrix;
- production-path call-site findings;
- scenario A–F results;
- focused command results/pass counts;
- broad command result and attribution;
- security/privacy/authority review;
- compatibility/migration review;
- unresolved findings by severity;
- registry/roadmap disposition;
- explicit final recommendation.
