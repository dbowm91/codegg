# Execution Reliability, Approval, and Autonomy M008 — Fault Injection and Reliability Qualification

Status: ready for handoff

Repository baseline: `18365458f881f6ac4524c9ea05224b69923faa4f`

Source roadmap:

- `plans/subsystems/execution-reliability-approval-autonomy-roadmap.md#7-milestones`

Long-term requirements:

- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#28-observability`
- `plans/000-long-term-specification.md#29-system-invariants`

Applicable ADRs:

- `plans/adrs/ADR-0004-approval-routing-sandbox-and-runtime-preferences.md`
- `plans/adrs/ADR-0001-programmatic-tool-execution-authority.md`
- `plans/adrs/ADR-0002-agent-run-worktree-isolation-and-control.md`

Primary class: invariant / closure

Hard dependencies: M001-M007 closure.

## 1. Objective

Qualify the integrated retry, approval, sandbox, reviewer, model-preference, and user-mode architecture under deterministic injected failures. Close the workstream only when failures remain bounded, externally visible state is coherent, no non-idempotent side effect is duplicated, no approval path fails open, no constrained sandbox silently degrades, and restart restores the intended model/policy state.

This milestone is closure-oriented and should add production code only to fix defects it exposes.

## 2. Why this milestone is blocked

Its purpose is integrated qualification of M001-M007. Running it earlier would test placeholders rather than the final authority/retry paths.

## 3. Existing test/harness foundation

Reuse existing repository facilities:

- scripted provider/AgentLoop harnesses;
- provider fallback/circuit tests;
- ToolBroker/structured execution tests;
- scheduler/job/run-store restart and contention fixtures;
- managed-process/sandbox helper tests;
- session selection/provider connection fixtures;
- TUI/core transport resume/snapshot tests;
- static sandbox/core/execution-ownership guards.

Do not create a general chaos framework or live-provider CI requirement.

## 4. Invariants to prove

- one logical operation has a bounded retry chain;
- visible provider attempt generations cannot be silently merged;
- permanent auth/invalid/policy errors do not churn retries;
- ambiguous non-idempotent dispatch is reconciled/surfaced rather than replayed;
- deterministic Deny survives every approval mode;
- Automatic reviewer malformed/timeout/unavailable/prompt-injected state never becomes Allow;
- reviewer mode cannot mutate sandbox profile;
- WorkspaceWrite failure cannot fall through to FullHost;
- FullHost is always explicit/auditable;
- approval/sandbox/model preferences restore through daemon/frontend restart with explicit-session/project ceilings winning;
- production permission decisions survive restart;
- child/subagent effective authority never exceeds parent;
- no credential/hidden reviewer reasoning leaks through diagnostics.

## 5. Scope

### In scope

Deterministic scenarios covering:

1. provider failure before first token/event;
2. provider failure after text/reasoning delta;
3. provider failure after tool-call announcement;
4. 429 + Retry-After, transient 5xx, timeout, connect/DNS/TLS failure;
5. permanent bad auth, invalid request, missing model;
6. circuit open/half-open and cancellation during backoff;
7. nested retry budget exhaustion;
8. ambiguous external/non-idempotent mutation acknowledgement;
9. RuntimePreferenceStore/PermissionStore write/read/corruption failure;
10. daemon restart between selection/mode updates and next turn;
11. sandbox helper unavailable/setup error/unsupported kernel;
12. reviewer Allow/Deny/Defer, malformed output, provider timeout/unavailable, attempted forbidden tools and prompt injection;
13. mode/sandbox change races;
14. Yolo+WorkspaceWrite and Yolo+FullHost;
15. subagent parent-ceiling inheritance;
16. stale model catalog/disabled remembered connection.

### Explicitly out of scope

- random destructive host fuzzing;
- broad cloud provider matrix;
- long-term benchmark dashboards;
- new network sandbox backend;
- unrelated performance optimization.

## 6. Required production changes

None by default. If a scenario exposes a bounded defect, fix it in its canonical owner and add the smallest regression test. If a finding changes architecture/authority, stop and create a corrective plan/ADR rather than silently changing M008 scope.

### Fault injection design

Add explicit test seams/fakes rather than environment-dependent flakiness:

- scripted provider streams with precise event/error positions and retry metadata;
- fake durable mutation backend capable of “commit succeeded but ack lost” plus reconciliation lookup;
- preference/permission store failure injection around atomic write/read;
- fake reviewer provider/tool registry;
- sandbox-helper outcome fixtures using existing status frame APIs;
- model catalog revision/state mutation fixtures;
- parent/child policy snapshots.

Each scenario records expected attempts, final typed outcome, durable side-effect count, emitted events/receipts, and effective policy snapshot.

## 7. Ordered work packages

### Work package A — Provider/retry fault matrix

Run pre-stream/mid-stream/status/transport/circuit/cancel scenarios and nested retry budget assertions.

### Work package B — Side-effect reconciliation matrix

Prove exactly-once/no-blind-replay behavior for idempotent and uncertain non-idempotent representative operations.

### Work package C — Approval/reviewer/sandbox matrix

Exercise deterministic rules across Interactive/Automatic/Yolo and ReadOnly/WorkspaceWrite/FullHost, including reviewer failure/injection and sandbox-helper failure.

### Work package D — Persistence/restart/selection matrix

Restart daemon/frontends around mode/model/permission writes, stale catalog and selected connection lifecycle.

### Work package E — Child authority and closure reconciliation

Verify subagent ceilings, run quick verification/static guards, update docs/registry and create closure status.

## 8. Failure, cancellation, restart, and contention semantics

The test oracle must distinguish:

- explicit user deny vs timeout vs reviewer deny vs reviewer unavailable;
- provider attempt abandoned/superseded vs completed;
- retry budget exhausted vs permanent no-retry;
- sandbox unavailable vs setup failure vs FullHost requested;
- preference write failure vs current in-memory explicit override;
- uncertain side effect vs known failure before dispatch;
- stale revision conflict vs successful preference/model update.

No scenario may treat a missing event as success by default.

## 9. Compatibility and migration

Include at least:

- legacy PermissionConfig/permissions file path;
- pre-RuntimePreference DB;
- old TUI manifest selected-model hint;
- legacy ModelSelect request;
- legacy exec permissive mode/flag mapping;
- unsupported-host sandbox behavior.

## 10. Required tests

### Focused unit tests

Only add for concrete classification/serialization defects discovered.

### Integration tests

Implement the 16 scenario classes above in the smallest existing suitable test targets.

### Restart and recovery tests

Mandatory for preference, permission decision, model selection, uncertain durable job, and sandbox availability re-resolution.

### Contention and cancellation tests

Mandatory for retry backoff cancellation, concurrent preference/model updates, mode-change/pending-approval race, circuit half-open, and child-policy ceiling.

### Security and negative tests

Mandatory reviewer prompt injection/forbidden tools, Yolo hard-deny, sandbox no-fallback, secret-redaction, and FullHost explicitness.

### Migration and compatibility tests

Mandatory as listed in section 9.

## 11. Required verification commands

```bash
cargo test -p codegg-providers
cargo test -p codegg-core -- runtime_preference
cargo test --test permission
cargo test --test session_selection
cargo test --test agent_loop_harness
cargo test --test command_routing_execution_ownership
cargo test --test scheduler_contention
python3 scripts/check_core_boundary.py
python3 scripts/check_sandbox_contract.py
python3 scripts/check_execution_ownership.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
./scripts/verify.sh quick
```

Run supported-Linux Landlock tests when a suitable host is available and record the exact skip/unsupported reason otherwise. Do not add a permanent live-provider matrix.

## 12. Documentation updates

- finalize retry/permission/security/session/tool docs;
- update `plans/subsystems/execution-reliability-approval-autonomy-roadmap.md` statuses;
- update `plans/registry.md`;
- create `plans/closure/execution-reliability-approval-autonomy/008-status.md`.

## 13. Acceptance criteria

- all fault classes produce expected typed outcome and bounded attempts;
- no abandoned provider generation is silently mixed with a successful retry;
- no representative non-idempotent side effect occurs twice after lost acknowledgement;
- Auth/invalid errors stop promptly; transient errors retry within shared bound;
- Automatic reviewer cannot fail open or widen sandbox;
- Yolo cannot override hard deny;
- WorkspaceWrite cannot silently degrade to FullHost;
- permission/mode/model preference persists/restores correctly;
- stale/unavailable model preference does not silently reroute;
- child cannot exceed parent policy;
- diagnostics are secret-safe and useful.

## 14. Stop conditions

Stop and create a corrective plan if qualification exposes a flaw requiring changes to provider identity/selection, scheduler execution authority, PermissionChecker/ApprovalRouter ownership, sandbox backend architecture, or principal authorization rather than a bounded fix.

## 15. Closure evidence required

- scenario matrix with expected/observed outcome;
- attempt/side-effect count evidence;
- approval/sandbox behavior matrix;
- restart/persistence matrix;
- reviewer adversarial cases;
- supported-host sandbox evidence or named operational gap;
- defects found/fixed and regression tests;
- exact verification commands/results;
- recommendation: closed, conditionally closed, or corrective work required.

## 16. Handoff notes

Keep faults deterministic and targeted. The purpose is to prove architectural boundaries, not maximize random chaos volume.
