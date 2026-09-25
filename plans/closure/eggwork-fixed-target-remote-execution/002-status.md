# Eggwork Remote Execution M002 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/eggwork-fixed-target-remote-execution/002-target-capability-projection-and-operator-policy.md`

Source subsystem roadmap:

- `plans/subsystems/eggwork-fixed-target-remote-execution-roadmap.md`

Repository baseline reviewed: `762c9893d18343c2a091767bce3c7b0e783360ad`

Implementation commits:

- `ac29d9db` — typed target isolation/network policy, fresh preflight, posture projection, cache, and health mapping
- `145e660d` — pre-upload policy rejection regression coverage
- `37ebb99f` — posture cache invalidation coverage
- `12b8c62f` — live fixture assertion aligned with M002's capability/status mismatch contract

## 1. Executive finding

M002 is complete and closed. CodeGG now projects authenticated per-node Eggwork posture into secret-free diagnostics and executor health, supports explicit required-filesystem-isolation and disabled-network policies, and fails closed before workspace upload when configured policy is unsupported. Execution decisions continue to use fresh per-attempt evidence; cached posture is diagnostic only. Default legacy profiles preserve their `None` isolation and unrestricted-network execution behavior while exposing that posture. No selection, fallback, or retargeting authority was added.

The current pinned Eggwork node still cannot execute a restricted spec. M002 does not claim live isolation; M002a is unblocked to qualify that behavior against the upstream corrected pin.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Typed, secret-free posture per configured node | `EggworkNodePosture`, bounded feature projection, resource/isolation/network facts and observation age in `src/scheduler/eggwork.rs` | pass | Credential file contents and key paths are not included. |
| Fresh authenticated evidence controls execution | `preflight` obtains both `capabilities()` and `status()` before snapshot/upload; mismatched security features use conservative intersection | pass | Cache is never consulted for admission. |
| Required filesystem policy fails closed before upload | Unit and live fixture cases for missing Landlock capability; upload counter remains zero | pass | No downgrade to `None` or `BestEffort`. |
| Disabled networking fails closed before upload | Unit and live fixture cases for absent `network.disabled.v1`; synthetic support maps to `NetworkRequirement::Disabled` | pass | No network-isolation capability is inferred. |
| Legacy defaults remain compatible and visibly unrestricted | Config defaults/merge tests and live current-node default execution/diagnostics | pass | Legacy absence of the descriptive unrestricted feature is explicitly identified. |
| Diagnostics and health reflect node posture without retargeting | Per-node posture/health unit coverage, warning and conservative mismatch tests | pass | A durable target continues to identify one named node. |
| Posture cache is bounded and reload-safe | TTL expiry and `replace_config()` invalidation tests | pass | At most one entry per configured node; success TTL 30s, failure TTL 3s. |
| Generic required capabilities remain additive | Scripted preflight coverage in `tests/eggwork_remote_execution.rs` | pass | Existing capability policy retained. |
| No local fallback, automatic placement, or scheduler bypass | Existing routing tests and `check_eggwork_target_routing.py`, `check_scheduler_bypass.py` | pass | No alternate-node selection was added. |

## 3. Production implementation evidence

- `codegg-config` defines serde-defaulted typed isolation and network policies on `EggworkNodeProfile`; `Option` fields preserve layered-configuration merge semantics. Existing profiles deserialize to `None` isolation and unrestricted networking.
- The Eggwork executor collects fresh capability and status views before workspace side effects, checks target identity and generic requirements, and applies the conservative intersection for security-sensitive capability claims.
- Required Landlock and disabled-network policies are translated into restricted Eggwork spec requirements only when authenticated evidence supports them. Unsupported settings return a typed preflight error before upload.
- Per-node posture records reachability, drain/capacity facts, configured policy, observed capability dimensions, feature mismatch, and age. A bounded lazy cache supplies diagnostic/health views only and is invalidated on configuration replacement.
- Executor health aggregates configured-node facts without altering the durable target or selecting another node.

## 4. Verification executed

### Commands run locally

```bash
cargo test -p codegg-config --locked
cargo test -p codegg --lib scheduler::eggwork --locked
cargo test --test eggwork_remote_execution --locked
cargo test --test eggwork_remote_execution_live --locked
python3 scripts/check_eggwork_target_routing.py
python3 scripts/check_scheduler_bypass.py
bash scripts/check-core-boundary.sh
cargo fmt --all -- --check
scripts/verify.sh quick
scripts/verify.sh full
git diff --check
```

### Results

- `codegg-config`: 88 tests passed.
- `scheduler::eggwork`: 14 tests passed.
- `eggwork_remote_execution`: 24 tests passed.
- `eggwork_remote_execution_live`: 0 tests on the local macOS host; this integration target is Linux-only.
- All listed static guards and formatting passed. `scripts/verify.sh quick` and `scripts/verify.sh full` passed locally; full included Clippy, workspace tests, and the `server,plugins,lsp-test-support` feature test suite.
- Hosted Linux `CI / verify` run [36077353216](https://github.com/dbowm91/codegg/actions/runs/36077353216) completed successfully on `37ebb99f`. It passed all guards, formatting, Clippy, and workspace tests, including the Linux-only live fixture. At that revision the live fixture also asserted capability/status feature equality. The later `12b8c62f` test-only adjustment removed that equality assertion so the fixture remains valid for M002's explicitly conservative mismatch contract; production code did not change.

## 5. Invariant review

- A job's persisted `EggworkNode { node_id }` remains authoritative; posture in other configured nodes cannot change its destination.
- Scheduler admission, permit lifetime, retries, and recovery remain in CodeGG. The scheduler-bypass and target-routing guards passed.
- Per-attempt fresh authenticated observations remain authoritative. Expiring cached diagnostics cannot admit a job.
- Security-relevant feature disagreement cannot grant required isolation or disabled networking.
- Unsupported policy rejects before workspace snapshot/upload; no fallback executor, alternate node, or silent weakening is introduced.
- Posture and health remain transient diagnostics; no capability snapshot is persisted into jobs.

## 6. Failure and recovery review

Unavailable probes are briefly cached as degraded diagnostic facts but never used for admission. A later attempt re-probes both capability and status. Draining/busy nodes and policy mismatch remain typed preflight failures. Configuration replacement clears cached posture. Existing accepted-handle persistence, lease fencing, restart reconciliation, cancellation, and duplicate-submit behavior are unchanged and remain covered by the predecessor C001 closure and M002's workspace verification.

## 7. Migration and compatibility review

The new node-profile fields are serde-defaulted and require no storage migration. Older configuration retains M001 behavior while newly surfacing unrestricted filesystem/network warnings. Durable target and attempt schemas are unchanged. Rolling back the CodeGG configuration change does not require data migration.

## 8. Security review

Policy satisfaction comes only from authenticated node capability/status responses. Required isolation and disabled networking never silently degrade. Diagnostic feature counts and strings are bounded. Credential contents and key material do not enter posture records or debug output. Upload begins only after preflight passes. No new process owner, network backend, scheduler, or placement authority was introduced.

## 9. Documentation and operations

Updated `architecture/config.md`, `architecture/jobs.md`, and `architecture/scheduler.md` with policy, projection, warning, and preflight behavior. The current unsupported-policy errors are observable before workspace upload. Existing operator configuration remains valid and receives explicit unrestricted-posture warnings.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | The currently pinned Eggwork revision does not provide qualified required Landlock execution or disabled-network enforcement. | Restricted execution cannot be claimed against that pin. | M002a pins the upstream corrective revision and performs required live qualification. |

No unresolved high- or medium-severity M002 finding remains.

## 11. Roadmap disposition

M002 is closed. M003 remains blocked on its separately reviewed optimized materializer contract; its dependency graph does not require M002 or M002a. M004 remains deferred pending M002a, M003, and a stable worker-entry contract. No other registered plan was unblocked by this closure.

M002a is now ready: CodeGG M002 is closed, and upstream Eggwork Security remote-admission corrective C001 is closed with real authenticated remote required-Landlock evidence, truthful capability/status reporting, applied sandbox evidence, and filesystem escape denial. Its implementation must still review and pin the corrected immutable revision before testing.

## 12. Registry updates

- Marked M002 implemented/closed and added this closure record to recently closed work.
- Updated the Eggwork subsystem roadmap and current execution order to M002 closed and M002a ready.
- Unblocked M002a in this same closure commit after auditing both hard dependencies: CodeGG M002 and upstream Eggwork Security remote-admission C001 are closed.
- Left M003 blocked and M004 deferred because their other named dependencies remain unresolved.
