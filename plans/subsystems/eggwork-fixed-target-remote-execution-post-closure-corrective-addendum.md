# Eggwork Fixed-Target Remote Execution Post-Closure Corrective Addendum

Status: active corrective; C001 ready

Predecessor roadmap:

- `plans/subsystems/eggwork-fixed-target-remote-execution-roadmap.md`

Historical predecessor closure:

- `plans/closure/eggwork-fixed-target-remote-execution/001-status.md`
- implementation `67f8f3d33651846bbdbd3e4a3bc239e50a0237a6`
- closure commit `338074062e3e8748ba83708ea3c2a7e33de12f74`

Canonical authority remains:

- `plans/000-long-term-specification.md`
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md`
- `plans/003-planning-process.md`

Reviewed Eggwork dependency baseline:

- `128f808c62f176d414dd18a705773e45f5e2891a`

## 1. Findings

A post-closure review found one correctness defect and one qualification defect in CodeGG Eggwork M001.

### 1.1 Persisted lease identity diverges from the live Eggwork handle

`src/scheduler/eggwork.rs::derive_handle` currently creates:

1. one random `LeaseId` for the live `eggwork_core::ExecutionHandle` sent to Eggwork; and
2. a second, unrelated random string for `RemoteExecutionHandle.lease_id` persisted in CodeGG.

The execution id and generation match, but the lease token does not.

Eggwork treats the lease token as a fencing credential. At the reviewed Eggwork baseline, execution acceptance stores a hash of `ExecutionHandle.lease_id` and control operations compare subsequent cancel/renew lease hashes against that stored value. A mismatch returns `invalid_lease`.

Therefore the current restart path can observe an execution by id/generation, but a reconstructed persisted handle may not be authorized to cancel or renew that execution.

This violates the M001 durable invariant that reconnect/reconciliation acts on the exact accepted Eggwork execution handle.

### 1.2 M001 did not exercise the real CodeGG -> NodeClient -> eggworkd mTLS boundary

The original M001 handoff required a local Eggwork daemon/test-CA fixture. Closure instead qualified the adapter through `ScriptedClient` / `ScriptedFactory` seams.

Those tests provide useful scheduler/workspace/idempotency coverage, but they do not enforce Eggwork's actual:

- lease-token fencing;
- HTTPS/mTLS handshake;
- principal resolution;
- client/server serialization;
- real control-route semantics;
- event stream and restart behavior across the public client/server boundary.

The scripted restart test therefore accepted the divergent persisted lease token and did not catch finding 1.1.

## 2. Correctness impact

Severity: medium/high for restart ownership correctness.

The normal uninterrupted execution path uses the live handle and can renew/cancel correctly. The defect becomes material after durable-handle recovery/reconciliation, exactly where M001 claimed duplicate-safe restart behavior.

Potential outcomes include:

- cancellation of a still-live pre-restart execution being rejected;
- future renewal/reconciliation using the persisted handle being rejected;
- CodeGG reporting an execution interrupted while the remote process remains live until Eggwork lease expiry;
- scheduler/resource/accounting state converging later than CodeGG's local attempt state.

No evidence currently shows a cross-node or privilege escalation issue. Eggwork's fencing rejects the bad token as designed.

## 3. Corrective milestone

### C001 — Lease identity coherence and real-node qualification

Class: invariant/corrective

Status: ready

Implementation plan:

- `plans/implementation/eggwork-fixed-target-remote-execution-corrective/001-lease-identity-and-live-node-qualification.md`

Objective:

Make the durable CodeGG remote handle reconstruct the exact lease-fenced Eggwork handle accepted by the node, and qualify the production NodeClient path against a real local Eggwork server with mTLS.

## 4. Required invariants

C001 MUST preserve all M001 architectural boundaries:

1. CodeGG remains the only scheduler/placement authority.
2. Eggwork remains fixed-target and scheduler-free.
3. Remote target failure never falls back to local execution.
4. One CodeGG attempt maps to at most one accepted Eggwork execution.
5. The scheduler resource permit spans the remote lifetime.
6. Credentials remain daemon/config-owned.
7. Local workspaces are not mutated in place by remote execution.
8. Remote process creation remains owned by Eggwork, not CodeGG.
9. Existing local executors remain unchanged for `ExecutionTarget::Local`.
10. Historical M001 plan/closure evidence remains immutable; this corrective owns the new disposition.

## 5. Downstream gates

Until C001 closes:

- Eggwork fixed-target remote execution M001 is **historically closed but corrective-required**.
- M002 target capability/operator policy remains blocked.
- M003 workspace-transfer optimization remains blocked even if a materializer interface becomes available.
- M004 whole remote AgentRun remains deferred/blocked.
- No production documentation should claim restart reconciliation is fully qualified.

The corrective does not block unrelated CodeGG subsystems.

## 6. Exit conditions

C001 closes only when:

- live and durable handles use one identical lease token;
- round-trip reconstruction preserves execution id, generation, and lease token exactly;
- a regression fails against the two-random-token M001 implementation;
- a real NodeClient communicates with a real local Eggwork server over mTLS;
- real-node renew succeeds using the accepted handle;
- real-node restart/reconcile/cancel succeeds with the persisted handle;
- a deliberately wrong lease token is rejected by the real Eggwork server;
- duplicate/reconnect behavior does not create a second execution;
- scripted tests remain useful and green;
- canonical verification passes with no unresolved medium-or-higher finding.
