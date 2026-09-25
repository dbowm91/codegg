# Eggwork Remote Execution M002 — Target Capability Projection and Operator Policy

Status: implemented

Source roadmap:

- `plans/subsystems/eggwork-fixed-target-remote-execution-roadmap.md`

Predecessor evidence:

- M001 implementation `67f8f3d33651846bbdbd3e4a3bc239e50a0237a6`
- historical M001 closure `plans/closure/eggwork-fixed-target-remote-execution/001-status.md`
- post-closure corrective C001 closure `plans/closure/eggwork-fixed-target-remote-execution-corrective/001-status.md`
- current CodeGG baseline `d3d390d56620fb5c6f755a5dfb0987e0a1c01651`

Upstream interface dependency:

- Eggwork remote-admission corrective addendum at `eggstack/eggwork@db4c08b50909116cb248f1957dac320f74370423`
- implementation handoff `eggstack/eggwork@3b5fca586c6036b06f6cb442009f205db41e0ad5`

The upstream implementation is not a hard dependency for the policy/projection code. Its feature contract is the interface dependency. Real restricted-spec qualification is a later operational gate and is registered separately as M002a.

Primary class: infrastructure/polish

## 1. Objective

Project one named Eggwork node's actual execution posture into CodeGG configuration, preflight, executor health, and operator diagnostics, and let operators explicitly require a minimum remote isolation/network posture.

CodeGG continues to choose the node. This milestone does not implement automatic placement.

## 2. Current state

At the baseline:

- `ExecutionTarget::EggworkNode { node_id }` durably selects one node.
- `EggworkNodeProfile` contains endpoint/mTLS file references and generic `required_capabilities`.
- `preflight` performs fresh `capabilities()` and `status()` calls before workspace transfer.
- `EggworkExecutor` currently builds `IsolationRequirement::None + NetworkRequirement::Unrestricted` because the pinned Eggwork node rejects all restricted specs.
- CodeGG has `ExecutorHealth::{Healthy, Degraded, Unavailable}` and scheduler-visible health snapshots.
- C001 live qualification records unrestricted execution as a medium finding requiring M002 operator policy.

## 3. Stable upstream feature contract

Consume these Eggwork feature names exactly:

```text
exec.argv.v1
isolation.landlock.workspace-rw.v1
network.unrestricted.v1
resources.cgroups-v2.memory
resources.cgroups-v2.cpu
resources.cgroups-v2.pids
```

Semantics:

- `isolation.landlock.workspace-rw.v1` means a required `workspace_rw` filesystem isolation request is runtime-qualified on that node.
- `network.unrestricted.v1` is descriptive: remote jobs can run with unrestricted networking.
- absence of a network-isolation capability means CodeGG MUST NOT infer that networking can be disabled.
- unknown future features are preserved for diagnostics but grant no implicit policy satisfaction.

A later Eggwork release may add a separately versioned `network.disabled.v1` capability only when an actual backend exists. M002 may recognize that name prospectively but must not claim it from the current node.

## 4. Work package A — Typed CodeGG node posture

Add a CodeGG-owned, secret-free posture projection, for example:

```rust
struct EggworkNodePosture {
    node_id: String,
    reachable: bool,
    draining: bool,
    active_executions: u32,
    max_active_executions: u32,
    workspace_isolation: bool,
    network: EggworkNetworkPosture,
    resources: EggworkResourcePosture,
    raw_features: Vec<String>,
    observed_at: InstantOrTimestamp,
}
```

The exact type names may differ.

Requirements:

- derive only from authenticated `NodeCapabilities` + `NodeStatus`;
- do not persist mTLS paths or key material;
- keep raw feature strings bounded;
- distinguish unavailable probe from capability absence;
- preserve node id;
- never infer a capability from OS name, endpoint hostname, or configured intent.

## 5. Work package B — Per-node operator policy

Extend `EggworkNodeProfile` with typed policy fields.

Required minimum policy:

### Filesystem isolation

```text
isolation_policy = "none" | "required"
```

Semantics:

- absent/default -> `none` for M001 backward compatibility;
- `none` -> CodeGG may execute with `IsolationRequirement::None`, but diagnostics MUST classify the node/job posture as unrestricted filesystem execution;
- `required` -> preflight requires `isolation.landlock.workspace-rw.v1` before any workspace upload and the execution spec MUST use `IsolationRequirement::Required`.

Do not expose `best_effort` as an operator policy in this milestone. A CodeGG operator asking for isolation should get a hard guarantee or a preflight refusal, not an ambiguous containment promise.

### Network

```text
network_policy = "unrestricted" | "disabled"
```

Semantics:

- absent/default -> `unrestricted` for compatibility;
- `unrestricted` -> spec uses `NetworkRequirement::Unrestricted` and diagnostics warn that the process has network access;
- `disabled` -> preflight requires an explicitly advertised `network.disabled.v1` capability and spec uses `NetworkRequirement::Disabled`;
- with the current Eggwork feature contract, `disabled` therefore fails before upload with a clear operator-facing capability error.

The generic `required_capabilities` list remains supported and is additive.

## 6. Work package C — Fresh execution preflight remains authoritative

Do not make execution depend on stale diagnostic cache.

For every new remote attempt, before workspace snapshot/upload:

1. call `capabilities()`;
2. call `status()`;
3. validate protocol/`exec.argv.v1`;
4. validate generic configured `required_capabilities`;
5. project current posture;
6. apply isolation/network policy;
7. reject draining/busy nodes;
8. only then begin workspace side effects.

When capability and status feature sets disagree:

- treat the inconsistency as `Degraded` for diagnostics;
- for execution policy, use the conservative intersection for security-relevant capabilities;
- do not accept required isolation because only one endpoint claimed it.

After Eggwork corrective C001 closes, the two endpoints are expected to agree; M002a live qualification proves that.

## 7. Work package D — Spec construction follows policy

Replace the hardcoded M001 posture with explicit policy mapping:

| CodeGG policy | Required node capability | Eggwork spec |
|---|---|---|
| isolation none | none | `IsolationRequirement::None` |
| isolation required | `isolation.landlock.workspace-rw.v1` | `IsolationRequirement::Required` |
| network unrestricted | `network.unrestricted.v1` or legacy compatibility disposition below | `NetworkRequirement::Unrestricted` |
| network disabled | `network.disabled.v1` | `NetworkRequirement::Disabled` |

Legacy compatibility:

- current pinned Eggwork C001-era nodes may not advertise `network.unrestricted.v1`;
- for `network_policy=unrestricted` only, absence of the descriptive feature MAY remain compatible if protocol/exec capability is otherwise valid;
- posture must still display `unrestricted (legacy/unadvertised)`, never `unknown safe`;
- once CodeGG pins a corrected Eggwork revision advertising `network.unrestricted.v1`, M002a removes or explicitly re-evaluates this compatibility allowance.

No policy may silently fall back from required isolation/disabled network to none/unrestricted.

## 8. Work package E — Bounded diagnostic cache

Add a bounded, lazy cache only for operator/health surfaces.

Requirements:

- at most one entry per configured node, capped by the existing 64-node config bound;
- TTL <= 30 seconds;
- no background polling loop required;
- probe failures are cached only briefly and marked unavailable/degraded;
- execution preflight does not trust the cache for required policy decisions;
- config reload invalidates or replaces affected node entries;
- secret paths never enter cached diagnostics.

## 9. Work package F — Executor health projection

Project the configured-node fleet into the existing coarse Eggwork executor health:

- `Healthy`: all configured nodes were recently reachable and no posture/policy mismatch is known;
- `Degraded`: at least one node is usable but another is unreachable, draining, capability-inconsistent, or operating under explicit unrestricted posture;
- `Unavailable`: no configured node can satisfy its own configured policy or no node is configured.

This is diagnostics only. It MUST NOT cause the scheduler to choose a different node for a job whose durable target names a specific node.

For a job targeted to node A, node B's health never retargets the attempt.

## 10. Work package G — Operator diagnostics

Use existing diagnostics/status surfaces rather than creating a second management subsystem.

At minimum expose, per named node:

- node id;
- reachable/unreachable;
- draining;
- active/max executions;
- filesystem posture: `none` or `landlock workspace-rw`;
- network posture: `unrestricted` / `disabled-capable` / `unknown`;
- resource capability dimensions;
- configured isolation/network policy;
- policy satisfied yes/no;
- capability/status inconsistency;
- observation age.

Warnings MUST prominently state when:

- filesystem isolation policy is `none`;
- network policy is `unrestricted`;
- the node does not satisfy configured required isolation;
- disabled networking was requested but unsupported.

Do not print certificate/private-key contents. Avoid printing the private-key path unless an existing diagnostic convention already considers secret references safe; prefer a redacted `configured` fact.

## 11. Work package H — Tests against current and future node contracts

Scripted tests:

- default policy preserves M001 None + Unrestricted behavior;
- default posture generates a warning/degraded diagnostic;
- required isolation + missing feature fails before upload/submit;
- required isolation + advertised feature builds `IsolationRequirement::Required`;
- disabled network + no capability fails before upload;
- disabled network + synthetic `network.disabled.v1` builds `NetworkRequirement::Disabled`;
- capability/status disagreement uses conservative intersection;
- required-capabilities remains additive;
- no remote-to-local fallback;
- no retarget to another configured node;
- diagnostic cache expiration/config reload;
- health aggregation.

Live tests against the current pinned node:

- default policy still executes;
- node is explicitly reported as unrestricted;
- required isolation is rejected by CodeGG preflight before workspace upload;
- disabled network is rejected by CodeGG preflight.

Do not require the upstream Eggwork corrective to close M002's current-node policy work.

## 12. M002a operational successor

Restricted live execution against corrected Eggwork is not hidden inside this milestone.

Registered successor:

- `plans/implementation/eggwork-fixed-target-remote-execution/002a-restricted-spec-live-requalification.md`

M002a is blocked on:

1. M002 implementation closure; and
2. Eggwork Security remote-admission corrective C001 closure.

M002a will pin the corrected Eggwork revision and prove required Landlock execution through CodeGG's production mTLS path.

## 13. Migration and compatibility

Config additions must be serde-defaulted.

Historical node profiles with no new fields preserve M001 execution behavior:

```text
isolation_policy = none
network_policy = unrestricted
```

but now surface explicit warnings.

No job schema migration is required: posture policy is daemon/node configuration, not durable job intent. The durable target remains only the selected node id.

Do not persist a transient capability snapshot into `JobRecord`.

## 14. Verification

At minimum:

```bash
cargo test -p codegg-config --locked
cargo test -p codegg --lib scheduler::eggwork --locked
cargo test --test eggwork_remote_execution --locked
cargo test --test eggwork_remote_execution_live --locked
python3 scripts/check_eggwork_target_routing.py
python3 scripts/check_scheduler_bypass.py
bash scripts/check-core-boundary.sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
scripts/verify.sh quick
scripts/verify.sh full
git diff --check
```

Linux live tests may use the existing bounded Eggwork test-node helper.

## 15. Acceptance criteria

1. Every named node has a typed current posture projection.
2. Execution preflight uses fresh authenticated node evidence.
3. Operators can require filesystem isolation and disabled networking without silent downgrade.
4. Default legacy profiles continue to run but are visibly classified as filesystem/network unrestricted.
5. Required isolation is rejected before upload when the node lacks the feature.
6. Required-isolation specs are constructed when the feature is advertised.
7. Network-disabled policy cannot execute against current unrestricted-only nodes.
8. Generic required capabilities continue to work.
9. Executor health reflects degraded/unavailable remote posture without performing placement.
10. Diagnostics are bounded and secret-safe.
11. No second scheduler, node selector, or background monitoring authority is introduced.
12. No unresolved high/medium M002 finding remains; strict live-isolation evidence is explicitly delegated to M002a.

## 16. Stop conditions

Stop and record a blocker if:

- satisfying policy requires CodeGG to infer capabilities not authenticated by Eggwork;
- executor health would need to retarget durable jobs;
- node policy would need to be persisted into JobRecord credentials/secret fields;
- disabled networking would need to be simulated or silently ignored;
- required isolation would need to execute as `None`/`BestEffort`;
- the diagnostic cache becomes an admission authority;
- M002 requires implementing the upstream Eggwork isolation backend itself.

## 17. Closure evidence

Create:

- `plans/closure/eggwork-fixed-target-remote-execution/002-status.md`

Record:

- implementation SHA(s);
- config compatibility;
- posture feature mapping;
- fresh preflight ordering;
- policy/spec mapping;
- current-node unrestricted warning evidence;
- pre-upload policy rejection evidence;
- health/diagnostic projection;
- cache bounds;
- scripted + current live-node verification;
- security/no-fallback review;
- exact M002a blocker state.
