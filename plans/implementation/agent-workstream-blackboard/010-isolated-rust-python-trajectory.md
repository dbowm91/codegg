# Agent Workstream Blackboard M010 — Isolated Rust/Python Coordination Trajectory

Status: closed — deterministic isolated Rust/Python coordination qualified
Repository baseline: `171ef93b6ba67d50b0412ae23b7697eb02d072eb` plus the active M005 branch

Source roadmap: `plans/subsystems/agent-workstream-blackboard-roadmap.md#corrective-m010--isolated-rustpython-trajectory`
Long-term requirements: `plans/000-long-term-specification.md` §§16–17, 26–29; `plans/001-terminology-and-domain-model.md` §§5, 7–8
Applicable ADRs: ADR-0001, ADR-0002, ADR-0004, ADR-0005; ADR-0031 only if an idle continuation is introduced. ADR-0032 is not needed because every writer has a distinct isolated worktree.
Primary class: capability qualification

## 1. Objective

Qualify one bounded two-worker Rust/Python trajectory using the existing workstream claims and isolated Git worktrees. Demonstrate task ownership, separate mutation roots, language-specific verification, board visibility, and explicit sequential integration. Do not make a causal performance claim from scripted workers.

## 2. Why this milestone is ready

M003 task claims and M004 peer observation are strictly closed (M004 through corrective M009). Existing managed worktree identity and generation-fenced lease contracts are available. Shared-write placement is neither used nor required. If the production Git containment backend is unavailable in the test environment, the trajectory fixture may use test-owned local Git repositories solely to verify branch/worktree mechanics; it must not imply production backend availability.

## 3. Current implementation evidence

- `tests/workstream_claim_m003.rs` qualifies durable claim CAS, overlap, handoff and reopen behavior.
- `tests/agent_loop_harness.rs::peer_evidence_arriving_during_provider_stream_fences_returned_tool_batch` qualifies M004's pre-mutation peer observation fence.
- M009's strict closure explicitly records that its provider harness uses two member identities and host-owned fake Git evidence without allocating two physical worktrees.
- This trajectory adds the missing composition evidence: distinct local Git worktrees, two task claims, Rust and Python checks, and clean sequential integration.

## 4. Invariants

- Every mutating worker has its own checkout root and branch.
- Claims are coordination facts only; they do not grant filesystem, Git, or sandbox authority.
- Peer evidence remains bounded/untrusted and is observed at the existing safe boundary.
- Parent integration is explicit, sequential, and Git-verified. No implicit merge or dirty-state discard occurs.
- The single-agent path and ADR-0002 isolated default remain intact.

## 5. Scope

In scope: deterministic two-worker Rust/Python fixture; task claims bound to two durable run identities; separate worktree roots; language test execution; committed results; sequential integration; board read visibility; test evidence and limitations.

Out of scope: shared checkout writes, performance claims about real model agents, provider selection or live API calls, three or more workers, build cache/target sharing, automatic merge, production Git backend qualification, M005 writer containment, and M008's broad comparative economics study.

## 6. Ordered work packages

A. Build a minimal committed Git fixture with a Rust crate and Python package.
B. Allocate two branches/worktrees and bind one durable task claim to each worker identity.
C. Have the scripted Rust worker add a tested Rust function and the Python worker add a tested Python function in its own checkout. Assert that each pre-integration root lacks the other worker's change.
D. Commit each worker result and integrate explicitly in sequence. Verify the parent is clean and both changes are present.
E. Read the same-group claims through the authorized board API and retain M004's separate peer-fence evidence as the observation contract.
F. Record exact commands, outcomes, runtime/toolchain prerequisites, and any failure modes in `plans/closure/agent-workstream-blackboard/010-status.md`.

## 7. Verification and closure criteria

Run the focused trajectory test, relevant claim/peer-observation tests, formatting, and applicable agent/workstream guards. Close only if both language checks pass, worktrees are physically distinct, ownership claims are visible to an authorized peer, no pre-integration cross-write occurs, both commits integrate without conflict, and the parent ends clean.

The closure must state that this is deterministic functional evidence, not a real-model performance comparison. Any measured or claimed improvement in agent success, latency, cost, or throughput requires a later controlled provider-backed comparison with fixed task/model/tool/budget conditions and per-task outcomes. Shared-write support stays unavailable.

## 8. Failure and recovery

Test setup failure, toolchain absence, or Git worktree allocation failure is reported as unavailable evidence rather than skipped success. Failed worker checks do not integrate. Failed integration retains both child commits and reports the conflict; no reset or cleanup of dirty data is allowed.

## 9. Registry/unblock audit

This plan has no M005 dependency. Its closure may inform M008 but does not close M006/M007 or authorize shared checkout placement. Update the registry and roadmap with the closure and explicit unblock result.

## 10. Closure

Strict closure: `plans/closure/agent-workstream-blackboard/010-status.md`.
Hosted CI run `38850114895` passed the full workspace suite (13,961 passed,
0 failed, 6 skipped); Desktop E2E run `38850114846` also passed. The closure
qualifies deterministic worktree/claim/check/integration behavior only. It
does not claim provider-backed model performance or production WorktreeService
allocation.
