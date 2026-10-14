# Command Authorization M004 Corrective 008 — Constrained Specialist Execution

Status: active
Repository baseline: `21dceaaa` (main reconciled before this implementation)

Source roadmap: `plans/subsystems/command-authorization-and-execution-boundaries-roadmap.md#6-dependency-graph`
Long-term requirements: `plans/000-long-term-specification.md` §§4, 17, 27–29; `plans/001-terminology-and-domain-model.md` §8
Applicable ADRs: `plans/adrs/ADR-0032-command-syntax-and-execution-authority.md`, `plans/adrs/ADR-0004-approval-routing-sandbox-and-runtime-preferences.md`, `plans/adrs/ADR-0016-cross-platform-sandbox-launch-authority.md`
Primary class: capability / invariant
Closure record: `plans/closure/command-authorization-and-execution-boundaries/008-status.md`

## 1. Objective

Allow the model-facing Test and Python tools to execute under accepted constrained profiles by carrying the accepted profile through scheduler admission to the canonical process owner and proving that actual launch effects are no broader. Preserve scheduler ownership and the existing FullHost route.

## 2. Why this corrective is ready

M004 plan 007 closed the route inventory and fail-closed admission predicate. `ManagedProcessService` already refuses constrained authorization with a disabled sandbox; Python already constructs a mode-specific required OS filesystem policy. The missing interface is the accepted profile handoff, and Test still owns a direct child-spawn supervisor that must be migrated or otherwise connected to the process owner.

## 3. Current implementation evidence

- Python validates the accepted Allow receipt and matching policy revision, stores the accepted `SandboxProfile` in the scheduler payload, and applies it at `ManagedProcessService`. Its mode-built sandbox is equal or narrower; ReadOnly transform is rejected before scheduler submission.
- `ManagedProcessService` now has a profile handoff for scheduler jobs and rejects a constrained profile paired with a disabled sandbox before launch.
- A live model-facing Python tool call now passes accepted WorkspaceWrite through the durable scheduler payload, into `ManagedProcessService`, and returns the script's output. The test also verifies fail-closed behavior when the trusted helper is absent; the positive native case uses the installation-owned sibling helper.
- Test validates and persists the accepted profile in its durable job payload. `TestJobExecutor` carries it into the domain runner, whose streaming child lifecycle now uses `ManagedProcessService`; output parsing, redacted logs, reports, run records, timeout, stall, and cancellation handling remain in the domain runner.
- The live Test tool scheduler fixture verifies the accepted profile survives into the durable payload. A native WorkspaceWrite fixture proves a test process can write inside its workspace but cannot create a sibling marker. ReadOnly refuses before submission.
- Hosted exact-head qualification remains required for the Linux protected-mount lane, Windows cross-target/diagnostic lane, and macOS target build.
- Bash managed processes already carry accepted authorization directly into `ManagedProcessService`.

## 4. Invariants that must not regress

- Validate accepted Allow receipt and policy-revision match before submission.
- Carry the accepted profile unchanged through durable scheduling; the executor cannot infer a broader profile from tool name or mode.
- A constrained profile cannot execute if the canonical process owner has no required sandbox request.
- Test/Python effects must be same-or-narrower than the accepted profile; ReadOnly cannot perform workspace-writing transforms/tests.
- Heavy work remains `JobSubmissionService` → `JobScheduler` → canonical process owner.
- Preserve timeout, cancellation, output caps, audit, run records, and no fallback after a denial.
- FullHost remains explicit and existing M001–M003 evidence is not rewritten.

## 5. Scope

**In:** typed Test/Python model tools, scheduler job payload handoff, Test/Python executors, `ManagedProcessService`, profile-specific behavior tests, execution-ownership and authorization guards, current architecture/skill docs.

**Out:** operator TUI `/test`, direct human shell, plugins, Tool Programs, Git filters/hooks/remotes, network or IPC claims, sandbox default changes, and production Bubblewrap promotion.

## 6. Required production changes

- Persist only the immutable accepted `SandboxProfile` in scheduler payload metadata after the model tool validates the matching receipt.
- Make the canonical process owner enforce the profile as a pre-launch ceiling.
- Python: continue using its mode-derived filesystem policy, reject ReadOnly transform, and pass accepted profile into the managed child request.
- Test: replace direct process launch with scheduler-owned managed execution while preserving streaming parser, timeout/stall behavior, logs, and report persistence.
- Refuse constrained profiles before target execution when the platform backend is unavailable.

## 7. Ordered work packages

1. **Python profile handoff:** validate, persist profile, enforce ReadOnly/WorkspaceWrite mode ceiling, attach profile to `ManagedProcessService`, and pass local live-scheduler qualification. Hosted exact-head qualification remains.
2. **Test process ownership:** move the test runner child lifecycle to `ManagedProcessService` streaming; preserve its parser and cancellation/stall semantics; pass local live-scheduler and constrained-effect qualification. Hosted exact-head qualification remains.
3. **Security matrix:** exercise ReadOnly, WorkspaceWrite, FullHost, unavailable backend, missing/mismatched receipt, and no-target behavior.
4. **Cross-platform/docs:** update execution ownership, architecture, and tool-execution skill; qualify Linux/macOS native builds/tests and Windows compile/hosted CI.
5. **Closure:** record exact test and CI evidence; update registry and roadmap without reopening plan 007's historical closure.

## 8. Failure, cancellation, restart, and contention semantics

Profile validation happens before durable submission. A constrained executor refuses before target start when its sandbox is absent or unavailable. Scheduler cancellation and deadlines terminate the full process tree; Test stall detection must cancel through the same owner. Restarted accepted jobs retain their profile and never broaden it. Concurrent jobs retain independent cwd, scratch, output and process ownership.

## 9. Compatibility and migration

New optional profile fields decode as absent for historical/operator payloads. Absence retains existing route semantics and is never interpreted as constrained authorization. No default profile or persisted user preference changes. Existing FullHost calls retain their output contracts.

## 10. Required tests

- Python accepted-profile handoff through scheduler and process owner; ReadOnly transform refuses before submission; constrained launch refuses if sandbox is disabled/unavailable.
- Test output parser, report and RunStore compatibility after managed streaming migration.
- Test positive WorkspaceWrite effects, ReadOnly denial, FullHost behavior, timeout, stall, cancellation and descendant cleanup.
- Missing/mismatched/denied authorization and scheduler-payload legacy cases.
- Existing command-routing, approval, sandbox containment, managed-process, and Python scheduler suites.

## 11. Required verification commands

```bash
cargo fmt --all -- --check
cargo test -p codegg --lib python_script
cargo test --test python_scheduler_execution --locked
cargo test -p codegg --lib managed_process --locked
cargo test --test sandbox_containment --locked -- --test-threads=1
cargo test --test command_routing_adversarial --locked
python3 scripts/check_execution_ownership.py
python3 scripts/check_authorization_matrix.py
scripts/verify.sh quick
```

Hosted CI must qualify Linux and macOS native suites plus Windows cross-target compilation on the exact pushed commit. Do not substitute local Linux evidence for native macOS execution or Windows filesystem enforcement.

## 12. Documentation updates

Update `architecture/process-tool-execution-ownership.md`, `architecture/tool.md`, `.opencode/skills/tool-execution/SKILL.md`, and the subsystem roadmap.

## 13. Acceptance criteria

- A normal typed Test/Python call under accepted WorkspaceWrite works through the canonical scheduler/process owner without a FullHost requirement.
- Actual constrained filesystem effects are demonstrated, not inferred from the stored profile.
- ReadOnly never gains write effects; missing enforcement never launches a child.
- Existing FullHost and operator routes preserve their contracts.
- Native and cross-platform regression suites pass on the final reconciled branch head.

## 14. Stop conditions

Stop if the scheduler cannot carry immutable accepted authority, Test output/stall semantics cannot be preserved without a second process owner, any route launches unconstrained, or native enforcement is unavailable without an explicitly reported refusal. Do not fall back to FullHost or invent network/IPC guarantees.

## 15. Closure evidence required

The closure record must show the authorization-to-process handoff, route-specific effect matrix, exact native/hosted verification, timeout/cancellation evidence, cross-platform build evidence, and a registry unblock audit. It must state which operator surfaces remain independent.

## 16. Handoff notes

Plan 007 remains closed for admission and fail-closed routing. This corrective owns only constrained specialized execution parity. Git C002/C003 and Linux production backend rollout remain independent.
