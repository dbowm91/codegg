# Command Authorization M004 — Constrained Command Routing and Tool Coverage Successor

Status: **implemented**
Repository baseline: `main` @ `ec43e495c3619966ac33d0acccf7904812dd37bc` (2026-10-13)

Source roadmap: `plans/subsystems/command-authorization-and-execution-boundaries-roadmap.md#7-milestones`

Long-term requirements:
- `plans/000-long-term-specification.md` §§4, 17, 27–29
- `plans/001-terminology-and-domain-model.md` §8
- `plans/002-long-term-roadmap.md`

Applicable ADRs:
- `plans/adrs/ADR-0032-command-authorization-and-execution-boundaries.md` — accepted command authority
- `plans/adrs/ADR-0031-linux-execution-isolation.md` — Linux backend selection and qualification dependency
- `plans/adrs/ADR-0004-permission-and-sandbox-policy.md` — permission/sandbox separation

Primary class: capability / invariant
Closure record: `plans/closure/command-authorization-and-execution-boundaries/007-status.md`

M001–M003 are already closed; this milestone does **not** reopen their evidence. `src/tool/authorization.rs` owns the accepted execution context, and `ManagedProcessService` owns canonical finite-child launch and obtained filesystem evidence.

## 1. Objective

Inventory model-facing command routes and apply one accepted-authority admission predicate to specialized Bash-intent, Test, and Python subprocess routes. A route may execute under a constrained accepted profile only when the exact profile reaches its enforcement owner and same-or-narrower effects are proven. Otherwise return typed `BackendUnavailable` before job or child creation. Never widen to FullHost or reroute after explicit Deny.

## 2. Why this milestone was ready

The M002 immutable `ExecutionAuthorizationContext` and M003 requested/obtained filesystem evidence contracts were already available. Route inventory and refusal behavior did not depend on completion of the Linux native backend. Linux native qualification, Git hooks/filters, and remote transport remain independent dependencies; they constrain route promotion, not fail-closed admission.

## 3. Current implementation evidence

- `src/tool/authorization.rs` defines `admit_full_host_specialized_route`, which validates the accepted Allow receipt, decision ID, and policy-revision match. It rejects missing/invalid authority and constrained profiles.
- `src/tool/bash/process.rs` uses the predicate before active command-intent dispatch.
- `src/tool/test.rs` and `src/python_script/tool.rs` use the predicate before scheduler submission in structured production calls.
- Constrained typed Test/Python calls return `ToolError::BackendUnavailable`; neither schedules a job. Missing/invalid authority returns a permission error.
- Git filter/hook/remote routes remain unavailable behind Linux C002/C003. No constrained specialized route was promoted.
- `research/command-authorization-m004-route-coverage.md` inventories route owners, authority, effects, lifecycle, and exclusions.

## 4. Invariants that must not regress

- Accepted authority comes from the immutable execution context and matching receipt, never command syntax, argv0, risk label, tool name, or classifier result.
- Approval mode does not change the sandbox profile. FullHost remains explicit host authority.
- No executor may exceed accepted filesystem rights; required unavailable enforcement is a pre-launch refusal.
- No accepted Deny is rerouted. No hidden FullHost fallback, local Git fallback, or credential/environment inheritance.
- Heavy work remains `JobSubmissionService` → `JobScheduler`; `ManagedProcessService` remains the canonical finite-process owner where used.
- Landlock proves filesystem restrictions only; this milestone adds no network, Unix IPC, proc, or seccomp claim.
- Preserve cancellation, timeout, cleanup, bounded output, and existing M001–M003 contracts.

## 5. Scope

**In:** model-facing Bash active routes, typed Test/Python tools, typed Git and native helpers, route inventory, route denial/status, scheduler/tool-program distinctions, security documentation, ownership manifest and regression evidence.

**Out:** parent ApprovalRouter changes; FullHost default or permission relaxation; promotion of constrained Test/Python without end-to-end profile propagation and native proof; Git hooks/filters/remotes before their independent containment/transport gates; network/IPC isolation; rewriting M001–M003 closure evidence.

Operator-invoked TUI `/test`, human shell, plugins/hooks, LSP/MCP, and Tool Programs are recorded as independently authorized owners rather than treated as model ToolBroker routes. No model-route parity claim covers them.

## 6. Required production changes

Implemented in commit `37c4f2f`:

- Shared accepted-receipt admission and route-specific typed refusal.
- Terminal `BackendUnavailable` error mapping; no automatic retry or route change.
- Pre-scheduler admission in structured Test and Python entrypoints.
- Active Bash intent admission through the same predicate.
- Architecture, module-skill, route-audit, and execution-owner updates.

No new dispatcher or process owner was introduced.

## 7. Ordered work packages

1. **Inventory:** complete in `research/command-authorization-m004-route-coverage.md`; records authorization, process/effect owner, cwd/environment/FD authority, sandbox/network evidence, lifecycle, audit, fallback, and exclusions.
2. **Admission:** complete; one production predicate accepts explicit FullHost with valid accepted authority and rejects unsupported constrained routes.
3. **Behavior matrix:** complete in the route audit; constrained Test/Python return BackendUnavailable before submission; active Bash intent is explicit-FullHost-only; raw Bash retains its managed constrained path; Git filters/hooks/remotes remain unavailable.
4. **Negative evidence:** complete; tests cover missing/mismatched/denied authority and no-submission behavior, alongside existing sandbox helper/outside-root and child-ceiling tests.
5. **Integration:** complete; architecture and module skill, ownership manifest/guard, bounded typed status, plan, roadmap, registry, and closure record are synchronized.

## 8. Failure, cancellation, restart, and contention semantics

Admission failure occurs before durable scheduler submission; it creates no job and launches no target. `BackendUnavailable` is terminal for retries and is surfaced as an unavailable execution backend. Explicit permission failure remains a denial. Admitted FullHost Test/Python work retains existing scheduler cancellation, timeout, process cleanup, and result persistence. No new restart or lease behavior was introduced.

## 9. Compatibility and migration

No storage or protocol migration is required. Structured model-facing Test/Python calls now require accepted execution authorization; contextless structured callers fail closed. Existing direct legacy `execute()` harness behavior is not production admission evidence. FullHost calls with a valid accepted receipt retain their existing executor and output contract. Constrained test commands may be issued through managed Bash when equivalent; Python receives no automatic alternate route.

## 10. Required tests

- Shared predicate: explicit FullHost, constrained profile, missing context, mismatched policy revision, and denied receipt.
- Typed Test/Python: constrained refusal before scheduler submission.
- Command routing adversarial suite, permission, approval router, sandbox containment, and managed-process suites.
- Parent/child sandbox ceiling regression.
- Existing helper/status, outside-root secret, and no-target fixtures remain green.

## 11. Required verification commands

```bash
cargo fmt --all -- --check
cargo test --locked -p codegg --lib specialized_admission_requires_matching_accepted_full_host_authority
cargo test --locked -p codegg --lib constrained_test_route_refuses_before_scheduler_submission
cargo test --locked -p codegg --lib constrained_python_route_refuses_before_scheduler_submission
cargo test --locked --test command_routing_adversarial
cargo test -p codegg --lib permission --locked
cargo test --test approval_router --locked -- --test-threads=1
cargo test --test sandbox_containment --locked -- --test-threads=1
cargo test -p codegg --lib managed_process --locked
cargo test --locked --test reliability_qualification_m008 m008_child_effective_authority_never_exceeds_parent
python3 scripts/check_execution_ownership.py
python3 scripts/check_authorization_matrix.py
scripts/verify.sh quick
```

## 12. Documentation updates

Completed: `architecture/tool.md`, `architecture/permission.md`, `architecture/process-tool-execution-ownership.md`, `.opencode/skills/tool-execution/SKILL.md`, `docs/execution-ownership.toml`, and `research/command-authorization-m004-route-coverage.md`.

## 13. Acceptance criteria

- Production specialized routing consumes valid accepted authority through one predicate.
- Constrained Test/Python fail with typed BackendUnavailable before scheduler submission; explicit FullHost remains available.
- Active Bash intent refuses missing/invalid/constrained authority; raw constrained Bash remains managed.
- Git filter/hook/remote work stays unavailable pending independent gates.
- No fallback or reroute after Deny; ownership and cancellation boundaries remain intact.
- Required targeted tests, static guards, and `verify.sh quick` pass; the closure record reports exact evidence and limitations.

## 14. Stop conditions

Stop and report if a route executes under greater authority, a constrained request launches without matching obtained effects, a process spawns outside its canonical owner, cancellation/lease ownership is lost, or diagnostics expose secrets. Do not improvise new sandbox guarantees or bypass Linux C002/C003.

## 15. Closure evidence required

The closure record must include the route disposition matrix, implementation commit, focused test/guard/quick results and counts, explicit FullHost and constrained-refusal behavior, independent Git/Linux gates, and an unblock audit. It must not claim constrained Test/Python parity, network/IPC isolation, or operator-surface equivalence.

## 16. Handoff notes

Plan 007 is implemented and closed by `plans/closure/command-authorization-and-execution-boundaries/007-status.md`. Future constrained Test/Python promotion requires propagating the exact accepted snapshot to the canonical executor and obtaining native same-or-narrower route evidence. Git filters/hooks/remotes remain owned by the Linux C002/C003 dependency chain. Do not reopen M001–M003.
