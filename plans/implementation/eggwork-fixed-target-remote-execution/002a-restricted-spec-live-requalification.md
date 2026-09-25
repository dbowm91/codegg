# Eggwork Remote Execution M002a — Restricted-Spec Live Requalification

Status: closing

Source roadmap:

- `plans/subsystems/eggwork-fixed-target-remote-execution-roadmap.md`

Predecessor:

- `plans/implementation/eggwork-fixed-target-remote-execution/002-target-capability-projection-and-operator-policy.md`

Hard dependencies:

1. CodeGG M002 closure.
2. Eggwork Security remote-admission corrective C001 closure:
   - `eggstack/eggwork: plans/implementation/security-isolation-resource-remote-admission-corrective/001-remote-enforcement-admission-and-capability-truthfulness.md`
   - Closed by the reviewed implementation commit `6cc813418c3f14740a635fef79208e85219175bb`; closure record is at that revision under `plans/closure/security-isolation-resource-remote-admission-corrective/001-status.md`.

Primary class: invariant/qualification

## 1. Objective

Pin CodeGG to an Eggwork revision that truthfully advertises and remotely admits required Landlock isolation, then prove CodeGG's production policy/spec path executes a real required-isolation job over mTLS without weakening any M001/C001 invariants.

This plan is deliberately small. It is the live integration gate after both sides implement their independent work.

## 2. Required upstream contract

The selected Eggwork revision must:

- advertise `isolation.landlock.workspace-rw.v1` on a qualified Linux node;
- advertise the same execution capability features through `/v1/capabilities` and `/v1/status`;
- remotely admit `IsolationRequirement::Required`;
- apply the trusted `workspace_rw` Landlock profile;
- return typed applied sandbox evidence;
- continue rejecting `NetworkRequirement::Disabled`/`AllowListed` unless a real network backend has separately landed;
- preserve existing lease/idempotency/mTLS behavior.

Reviewed upstream closure evidence is `eggstack/eggwork@6cc813418c3f14740a635fef79208e85219175bb`, Linux x86_64, kernel 6.8, Landlock ABI V4. It includes a production `NodeServer` + controller fixture proving `Required` admission, escape denial, and terminal `Applied` evidence. Pin this exact immutable tested commit unless Cargo/API review finds a concrete incompatibility; do not substitute a newer unqualified head.

Do not start M002a against planning-only upstream evidence.

## 3. Work package A — Pin corrected Eggwork revision

Update CodeGG's immutable Eggwork git rev only after reviewing the upstream corrective closure.

All production and test-helper Eggwork dependencies must use the same corrected immutable revision.

Run Cargo resolution review and confirm no unintended feature/dependency widening.

## 4. Work package B — Required-isolation live path

Configure the existing live fixture node/profile with:

```text
isolation_policy = required
network_policy = unrestricted
```

The fixture must use Eggwork's `TrustedLandlockSetup` and build the
`eggwork-sandbox-helper` binary from the same pinned upstream workspace. A
`NoExecutionSetup` fixture cannot qualify this milestone.

Required assertions:

1. preflight sees `isolation.landlock.workspace-rw.v1` on both capabilities and status;
2. CodeGG builds `IsolationRequirement::Required`;
3. workspace upload/materialization occurs only after policy satisfaction;
4. remote command runs;
5. workspace-local read/write succeeds;
6. outside-workspace read/write fails;
7. terminal sandbox evidence reports applied `workspace_rw`;
8. no local fallback or alternate-node selection occurs.

## 5. Work package C — Negative live policy

Against the same node:

- forge/remove the isolation capability in a scripted seam and prove CodeGG refuses before upload;
- request `network_policy=disabled` and prove CodeGG refuses before upload while the node lacks `network.disabled.v1`;
- if the corrected Eggwork node explicitly returns capability mismatch for a restricted unsupported mode, preserve that typed fact in diagnostics.

## 6. Work package D — Lease/restart regression under isolation

Repeat the C001 live restart path with required isolation enabled:

- one accepted execution;
- exact persisted lease;
- renew succeeds;
- fresh executor reconciles;
- cancel under persisted handle succeeds;
- terminal convergence;
- no second submit.

The isolation change must not regress fencing/restart correctness.

## 7. Documentation and disposition

Update:

- `architecture/jobs.md`;
- `architecture/scheduler.md`;
- Eggwork subsystem roadmap;
- registry;
- operator docs.

Remove wording that current remote jobs necessarily run without filesystem sandboxing.

Keep network access described as unrestricted until a separately qualified network backend exists.

## 8. Verification

Run M002's full verification plus the live Linux fixture against the corrected Eggwork pin.

Hosted Linux CI is required for closure because Landlock is the qualified backend. macOS/Windows compilation is not runtime isolation evidence.

## 9. Acceptance criteria

1. Corrected immutable Eggwork revision is pinned consistently.
2. Required-isolation CodeGG policy passes fresh capability preflight.
3. Production NodeClient/mTLS path executes a real required-isolation job.
4. Filesystem escape is denied.
5. Terminal evidence reports applied isolation.
6. Network remains truthfully unrestricted unless separately supported.
7. Lease/restart/no-duplicate invariants remain green.
8. No unresolved high/medium finding remains.

## 10. Stop conditions

Stop if:

- upstream closure does not include real remote required-isolation evidence;
- capability/status disagree;
- CodeGG must bypass policy to execute;
- the upstream pin regresses lease/idempotency behavior;
- filesystem isolation is advertised but terminal evidence is not applied;
- network-disabled support would need to be fabricated.

## 11. Closure evidence

Create:

- `plans/closure/eggwork-fixed-target-remote-execution/002a-status.md`

Record the upstream Eggwork corrective closure/revision, CodeGG pin, live mTLS required-isolation evidence, filesystem escape denial, restart/fencing evidence, hosted Linux CI, and final M003/M004 disposition.
