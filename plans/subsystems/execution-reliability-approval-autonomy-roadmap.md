# Execution Reliability, Approval, and Autonomy Roadmap

Status: closed

Repository baseline reviewed: `18365458f881f6ac4524c9ea05224b69923faa4f`

Long-term references:

- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#8-deployment-profiles-and-authentication`
- `plans/000-long-term-specification.md#16-durable-multilevel-agent-run-hierarchy`
- `plans/000-long-term-specification.md#17-job-scheduling-and-execution-backends`
- `plans/000-long-term-specification.md#24-protocol-and-storage-requirements`
- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#28-observability`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/001-terminology-and-domain-model.md`
- `plans/003-planning-process.md`

Related ADRs:

- `plans/adrs/ADR-0004-approval-routing-sandbox-and-runtime-preferences.md`
- `plans/adrs/ADR-0001-programmatic-tool-execution-authority.md`
- `plans/adrs/ADR-0002-agent-run-worktree-isolation-and-control.md`

Related closed/conditional foundations:

- `plans/subsystems/runtime-safety-resource-footprint-roadmap.md`
- `plans/subsystems/identity-authorization-audit-roadmap.md`
- `plans/subsystems/provider-connections-roadmap.md`
- `plans/subsystems/agent-runtime-correctness-autonomy-simplification-roadmap.md`
- `architecture/permission.md`
- `architecture/security.md`
- `architecture/session.md`
- `architecture/tool.md`

External research basis:

- OpenAI Codex current provider retry, sandbox/approval policy, `auto_review` approval reviewer, and `--yolo` behavior.
- Public Codex regressions showing why automatic reviewer mode must not silently change sandbox policy and why resumed runtime policy should be explicit/persistent.
- Anthropic Claude Code automatic permission mode and sandbox design: deterministic policy first, classifier only for downside-risk actions, denial feedback, bounded escalation, and separation of permissions from OS containment.

## 1. Purpose and ownership boundary

This workstream makes CodeGG resilient to local/upstream failure and provides explicit low-friction approval modes without weakening deterministic authorization or sandbox semantics.

It owns:

- provider-turn retry attempt identity and transient/permanent error taxonomy;
- one bounded retry budget propagated through nested retry layers;
- uncertain-side-effect reconciliation rules for non-idempotent operations;
- a single approval router over existing deterministic permission/security decisions;
- durable principal-scoped runtime preferences for approval mode, sandbox profile, and last-used provider/model;
- convergence of simple model selection on the existing durable selection service;
- wiring of the existing sandbox implementation into normal production tool construction;
- Automatic reviewer semantics;
- user-facing Interactive/Automatic/Yolo and sandbox profile controls/warnings;
- fault-injection qualification across provider, reviewer, persistence, sandbox, and restart boundaries.

It consumes but does not redefine:

- daemon/scheduler execution authority;
- current ToolEffectClass/idempotency contracts;
- managed-process/Landlock implementation internals owned by runtime-safety;
- principal/project authorization and hard-deny policy;
- provider connection credential ownership;
- AgentRun/worktree isolation and child authority ceilings.

The governing rule is:

> Retry only when the operation's state is known safe to repeat, and skip human prompts only within an explicitly resolved authority and containment envelope.

## 2. Work classification

### Invariants

- A retry never silently duplicates a known non-idempotent side effect.
- Retry layers share a bounded chain/deadline rather than multiplying independently.
- Provider output from superseded attempts is identifiable and cannot be silently merged into one authoritative turn.
- Authentication/invalid-request/policy failures are not treated like transient network failures unless an explicit refresh/recovery operation changes the condition.
- Hard deny, principal/project/session/agent authority, and parent-child ceilings precede Automatic/Yolo.
- `ApprovalMode` cannot alter `SandboxProfile`; `SandboxProfile` cannot grant permissions beyond the authority ceiling.
- Automatic reviewer cannot recursively approve itself or mutate state.
- Explicit sandbox requirements never silently degrade to unsandboxed execution.
- Filesystem and network containment are reported separately.
- Frontends do not own security/model authority; daemon state remains canonical.
- Last-used preferences never override an explicit session selection or policy ceiling.

### Capabilities

- Transient upstream failures recover without confusing duplicate streamed output.
- Users can choose Interactive, Automatic, or Yolo approval behavior.
- Automatic uses a fast inexpensive reviewer only for actions deterministic policy would otherwise ask about.
- Yolo can run autonomously inside a workspace sandbox without requiring Full Host authority.
- Full Host can be selected explicitly with a high-risk warning when genuinely desired.
- Last used model and approval/sandbox mode are restored consistently after restart.
- “Always allow/deny” decisions actually persist on production paths.

### Infrastructure

- `RetryContext`/retry disposition contract.
- Provider attempt identity and lifecycle reporting.
- `ApprovalRouter` and immutable execution-policy snapshot.
- Principal-scoped runtime preference store.
- ToolRegistry sandbox-policy threading.
- Dedicated bounded approval-review runtime role.
- Fault-injection harness extensions using existing scripted providers/process fixtures.

### Polish

- Status/UI labels for effective approval and sandbox modes.
- Clear warnings and diagnostics.
- Bounded reviewer reason/primary-agent feedback.
- Audit reason codes and metrics for retries/escalations.

## 3. Non-goals

This roadmap does not authorize:

- replacing the daemon scheduler, managed-process service, or permission rule language;
- allowing a reviewer to bypass an explicit deny;
- asking an LLM about every safe tool call;
- a generic policy language or third authorization engine;
- pretending Landlock isolates network access;
- introducing containers/VMs/network namespaces solely to complete this workstream;
- retrying arbitrary shell/API mutations after ambiguous acknowledgement;
- storing API credentials in runtime preferences;
- making TUI manifest state authoritative;
- provider failover that silently changes a session's explicitly selected connection/model;
- broad new CI matrices or permanent external-model tests.

## 4. Current state and findings

### 4.1 Provider retry

`src/agent/provider_turn.rs` currently performs three whole-request attempts with exponential delay. `stream_once()` publishes `TextDelta`, `ReasoningDelta`, and `ToolCallStarted` immediately. A retryable mid-stream error therefore causes the outer loop to replay the request after model-visible events have already escaped; the frontend can receive output from an abandoned attempt mixed with a later attempt.

`ProviderError::is_retryable()` currently treats `Auth` as retryable while generic `eggfetch_core` transport errors (except Timeout) are converted to `Api { code: "request_error" }`, which is not retryable. This can make permanent bad credentials more retryable than transient connect/DNS/TLS failures.

`FallbackProvider` circuit-breaks/fails over stream acquisition. Once a stream object is returned, terminal mid-stream failure is observed above that layer and is not naturally charged to the same provider health/circuit state.

### 4.2 Tool/job retry

`ToolContract` already has `ToolEffectClass`, `IdempotencyClass`, and `ToolRetryPolicy`, including retry eligibility. Scheduler/jobs/managed processes also have their own bounded attempt semantics. The missing layer is one propagated retry budget/identity so nested transport/provider/tool/job layers cannot accidentally multiply attempts and so ambiguous external mutations enter reconciliation rather than blind retry.

### 4.3 Permission routing

`PermissionChecker` already avoids many prompts: read-only/safe-mutating tools short-circuit Allow, non-destructive shell is normally auto-allowed, ordinary in-workspace file mutation is narrowly auto-accepted, and security policy can escalate/deny. But `AgentLoop::check_tool_permission()` owns several direct `PermissionPending` branches and 300-second human waits. There is no one router deciding whether an escalation goes to a user, reviewer, or Yolo.

`PermissionStore` supports atomic optional-HMAC persistence, but production `PermissionChecker::new()` construction in `agent_loop_factory`, `main`, `exec`, and `worker` passes no store path. Persisted “always” choices are therefore not reliably durable on those paths.

### 4.4 Sandbox wiring

The runtime-safety workstream delivered a child-only Landlock helper and fail-closed launch status. `BashTool` can opt into it. Normal `ToolRegistry::with_options()` creates `BashTool::default()`, wires workspace cwd bounds and other services, but has no sandbox-policy field and does not call the sandbox builders. The normal agent shell is therefore not automatically protected by the existing OS filesystem sandbox merely because its cwd is inside the workspace.

`SandboxMode::DangerFullAccess` currently behaves as another writable mode over configured roots, which is semantically different from a user-facing “Full Host / no containment” profile. The names/semantics must be made truthful rather than reused ambiguously.

### 4.5 Model and preference persistence

The provider connection/session selection service durably stores selected connection/model with revision/catalog checks. The simple `CoreRequest::ModelSelect` path only updates `runtime.selected_model` and publishes `SessionUpdated`. TUI manifest state has a selected-model display/restoration hint but explicitly is non-authoritative. No daemon-owned durable last-approval/sandbox/model preference exists.

## 5. Target architecture

```text
Tool/model action
      |
      v
hard authority + deterministic policy/security
      |
  +---+----+
  |        |
Deny     Allow
  |        |
  |     execute within
  |     SandboxProfile
  |
Escalate(ApprovalRequest)
      |
      v
 ApprovalRouter
  +----+--------+
  |    |        |
Human Auto    Yolo
      reviewer  |
  |    |        |
  +----+--------+
       |
       v
PermissionDecisionReceipt
       |
       v
ToolExecutionContext + sandbox enforcement report
```

Provider/tool reliability uses a separate but composable boundary:

```text
RetryContext(chain id, deadline, attempts)
      |
      +-> transport
      +-> provider stream attempt
      +-> fallback/circuit health
      +-> tool/job retry eligibility
      `-> uncertain-side-effect reconciliation
```

## 6. Dependency graph

```text
M001 provider retry attempt safety/taxonomy
  |
  v
M002 unified retry budget + side-effect reconciliation

M003 ApprovalRouter + durable approval state
  |             \
  |              +--> M004 model/preference convergence
  v
M005 production sandbox policy wiring
  |
  v
M006 automatic approval reviewer
  |
  +----------+
  |          |
  v          v
M007 Yolo/Automatic/FullHost user surfaces
  |
  v
M008 fault-injection and reliability qualification

M004 -------------------------------------------> M007/M008
M001+M002 --------------------------------------> M008
```

Dependency classification:

- M001 is ready independently.
- M002 hard-depends on M001.
- M003 is ready independently of M001 and may run in parallel.
- M004 has an interface dependency on ADR-0004 and existing provider-selection service; it may run after M003's runtime-preference contract is stable.
- M005 hard-depends on M003 so sandbox policy is represented through the same execution-policy snapshot; it consumes the conditionally closed runtime-safety Landlock implementation but does not require its historical hosted-evidence condition to redesign policy.
- M006 hard-depends on M003 and M005.
- M007 hard-depends on M003-M006.
- M008 hard-depends on M001-M007.

## 7. Milestones

### M001 — Provider retry attempt safety and error taxonomy

Class: invariant/reliability

Plan: `plans/implementation/execution-reliability-approval-autonomy/001-provider-retry-attempt-safety-and-taxonomy.md`

Status: closed.

Add provider-attempt identity/lifecycle, prevent transparent whole-turn replay after externally visible output unless supersession is explicit, classify transient versus permanent errors correctly, honor retry hints, add jitter, and charge mid-stream failure to provider health/circuit state.

Exit condition: every provider attempt is attributable; transient failures before visible output can retry; mid-stream failures cannot silently merge two generations; bad auth is not blindly retried while retryable transport/5xx/429 conditions are classified correctly.

Closure: `plans/closure/execution-reliability-approval-autonomy/001-status.md` (implementation `88694706`).

### M002 — Unified retry budget and uncertain-side-effect reconciliation

Class: invariant/infrastructure

Plan: `plans/implementation/execution-reliability-approval-autonomy/002-unified-retry-budget-and-side-effect-reconciliation.md`

Status: closed.

Introduce a propagated retry chain/deadline/budget and map ToolEffectClass/idempotency plus acknowledgement state into safe retry/reconcile/stop behavior.

Exit condition: nested retry layers cannot multiply beyond the configured chain budget and ambiguous non-idempotent effects are reconciled or surfaced rather than replayed.

Closure: `plans/closure/execution-reliability-approval-autonomy/002-status.md` (implementation `8d810fc3`).

### M003 — ApprovalRouter and durable approval-mode state

Class: authorization capability/invariant

Plan: `plans/implementation/execution-reliability-approval-autonomy/003-approval-router-and-durable-mode-state.md`

Status: closed.

Normalize deterministic `Ask`/security escalations into one router, add Interactive/Automatic/Yolo mode state and immutable per-turn execution-policy snapshot, wire a real PermissionStore path, and persist principal-scoped approval/sandbox preferences without implementing the reviewer yet.

Exit condition: all production escalation paths use one router; Interactive preserves existing UX; Yolo can resolve an escalation only inside the current authority ceiling; “always” decisions and mode preference survive restart.

Closure: `plans/closure/execution-reliability-approval-autonomy/003-status.md` (implementation `79cac319`).

### M004 — Selected-model and runtime-preference convergence

Class: capability/infrastructure

Plan: `plans/implementation/execution-reliability-approval-autonomy/004-selected-model-and-runtime-preference-convergence.md`

Status: closed.

Converge `ModelSelect` on the durable session-selection service, persist principal-scoped last connection/model preference, restore it only when a session has no explicit selection, and expose effective runtime preferences through frontend-neutral protocol.

Closure: `plans/closure/execution-reliability-approval-autonomy/004-status.md` (implementation `2842a25a`).

Exit condition: session model selection and last-used preference survive restart without TUI authority or silent connection failover.

M003 contract closed at `plans/closure/execution-reliability-approval-autonomy/003-status.md` (`RuntimePreferenceStore` with reserved model fields, CAS, and protocol preference shape): M004 is ready.

### M005 — Production sandbox policy wiring and authority matrix

Class: security invariant/capability

Plan: `plans/implementation/execution-reliability-approval-autonomy/005-production-sandbox-policy-wiring.md`

Status: closed.

Thread SandboxProfile through execution context/ToolRegistry construction, enable the existing Landlock path for constrained production shell execution where supported, distinguish FullHost from writable sandbox roots, and expose filesystem/network enforcement truthfully.

Exit condition: selecting WorkspaceWrite results in an actually enforced supported-host filesystem boundary or a typed failure/escalation; approval mode cannot mutate sandbox selection.

Closure: `plans/closure/execution-reliability-approval-autonomy/005-status.md` (implementation `34ceffe5`).

### M006 — Automatic approval reviewer

Class: capability/security

Plan: `plans/implementation/execution-reliability-approval-autonomy/006-automatic-approval-reviewer.md`

Status: closed.

M003+M005 contracts closed; reviewer is authorization-helper only.

Implement the dedicated cheap reviewer for escalated actions only, with strict structured verdicts, bounded read-only investigation, no recursive approval, prompt-injection resistance, primary-agent feedback, and fail-to-user/closed semantics.

Exit condition: Automatic completes ordinary work without human prompts for reviewer-approved escalations while hard denies/sandbox ceilings remain unchangeable and reviewer failure never becomes Allow.

Closure: `plans/closure/execution-reliability-approval-autonomy/006-status.md` (implementation `dd2ddd1a`).

### M007 — Yolo, Automatic, and Full Host user surfaces

Class: capability/polish

Plan: `plans/implementation/execution-reliability-approval-autonomy/007-yolo-auto-and-full-host-user-surfaces.md`

Status: closed.

Expose mode selection in frontend-neutral protocol plus TUI/CLI, show effective containment, add appropriate warnings/confirmation (strongest for Yolo+FullHost), persist last selection, and refine broad remembered decisions toward capability-scoped approvals where practical.

Exit condition: users can deliberately choose Interactive, Automatic, sandboxed Yolo, or dangerous FullHost behavior and can always see the effective mode/sandbox; restarts preserve the last valid preference.

Closure: `plans/closure/execution-reliability-approval-autonomy/007-status.md` (implementation `36f0f48d`).

### M008 — Fault-injection and reliability qualification

Class: invariant/closure

Plan: `plans/implementation/execution-reliability-approval-autonomy/008-fault-injection-and-reliability-qualification.md`

Status: closed.

Run deterministic fault matrices for provider streaming/retries, persistence/restart, reviewer failure, sandbox helper failure, uncertain effects, child ceilings, and mode/model restoration. Add only focused static guards/fixtures needed to prevent recurrence.

Exit condition: closure evidence demonstrates bounded recovery without duplicate side effects, fail-open approval, silent sandbox degradation, or runtime preference drift.

Closure: `plans/closure/execution-reliability-approval-autonomy/008-status.md` (implementation `f9e37930`).

## 8. Cross-cutting requirements

### Storage and migration

- Runtime preferences are additive, principal-scoped, bounded, version/revision aware, and secret-free.
- Permission decision storage uses the existing atomic format unless migration evidence justifies moving it; do not create duplicate decision stores.
- Provider/session selection remains in its canonical session/provider stores.

### Protocol and compatibility

- Additive core protocol exposes effective approval mode/sandbox/model preference and change requests.
- Existing PermissionRespond and ModelSelect clients continue through compatibility adapters during migration.
- Frontend-specific manifests/config files do not become security authority.

### Security and authorization

- All escalation modes operate after hard authorization.
- Automatic reviewer receives the minimum read-only tool surface and no credential material beyond what is necessary to identify action scope.
- Yolo is not “ignore deny.”
- FullHost is explicit and auditable.
- Child agents can only inherit/equal a parent ceiling, never broaden it.

### Concurrency, cancellation, restart, and recovery

- Provider retry attempts and reviewer attempts have cancellation-aware deadlines.
- Runtime policy snapshots are immutable for the accepted tool batch/turn boundary so concurrent UI changes cannot mutate an in-flight authorization.
- Restart loads durable decisions/preferences but does not replay pending non-idempotent work.
- Permission/reviewer timeouts are distinct from user denial.

### Observability and audit

Emit bounded reason/source fields for retry attempt/supersession, permanent/transient classification, approval source, reviewer verdict, Yolo use, sandbox requested/obtained, preference restore/fallback, and uncertain-side-effect reconciliation.

Do not log API keys, full sensitive commands beyond existing redaction policy, reviewer hidden reasoning, or permission-store signing keys.

### Performance and resource use

- Automatic reviewer is not invoked for deterministic Allow/Deny.
- Reviewer defaults to a configurable cheap/fast tool-capable model with a small output budget.
- Retry policy uses server retry hints and full jitter rather than synchronized fixed sleeps.
- No permanent background health/reviewer service is introduced.

### Documentation and operations

Update `architecture/permission.md`, `architecture/security.md`, `architecture/session.md`, `architecture/tool.md`, provider retry docs, CLI/TUI help, and configuration schema docs as relevant.

## 9. Verification strategy

Required subsystem-level evidence includes:

- scripted provider pre-stream and mid-stream failures with attempt identity;
- 429/Retry-After, 5xx, timeout, connect/TLS/DNS, bad auth, invalid request, circuit-open cases;
- nested retry-budget tests proving the global bound;
- idempotent versus uncertain non-idempotent tool tests;
- approval-mode matrix across deterministic allow/deny/escalate and child authority ceilings;
- production PermissionStore restart test;
- runtime-preference/model-selection restart and stale-catalog tests;
- supported-Linux WorkspaceWrite Landlock escape tests plus unsupported-host typed behavior;
- Automatic reviewer allow/deny/defer/malformed/timeout/unavailable/injection tests;
- regression test proving reviewer mode cannot mutate sandbox profile;
- Yolo+WorkspaceWrite versus Yolo+FullHost behavior/warning tests;
- daemon/frontend reconnect preserving effective policy;
- proportional quick verification and existing static ownership guards only.

## 10. Risks and decision points

- If retry correctness requires a provider-specific resume protocol, keep it adapter-specific behind the canonical attempt model rather than redefining provider API contracts globally.
- If a remote API cannot reconcile ambiguous mutation, surface `UncertainSideEffect` instead of inventing success/failure.
- If supported-Linux Landlock fixture evidence remains unavailable, policy wiring may land with explicit operational qualification but must not claim hosted enforcement that was not observed.
- If strong network isolation becomes a requirement, create a separate ADR/workstream after backend evaluation; do not overload filesystem Landlock semantics.
- If Automatic reviewer requires mutation or recursive tools to make useful decisions, the design has exceeded its authorization boundary and must stop.

## 11. Completion definition

This roadmap closes only when:

- provider retries are attempt-safe and transient/permanent classification is corrected;
- nested retries are bounded and uncertain effects are not blindly replayed;
- all escalations flow through one ApprovalRouter;
- approval and sandbox modes remain orthogonal under all frontends/resume paths;
- permission decisions, last approval/sandbox mode, and last model preference are actually durable;
- normal constrained shell execution uses the configured host sandbox where supported and reports enforcement truthfully;
- Automatic reviewer cannot widen authority and fails safely;
- sandboxed Yolo and explicit FullHost modes are usable and clearly distinguished;
- fault-injection tests prove no duplicate non-idempotent effects, fail-open approval, or silent containment loss.

## 12. Milestone status

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| M001 | closed | `plans/implementation/execution-reliability-approval-autonomy/001-provider-retry-attempt-safety-and-taxonomy.md` | `plans/closure/execution-reliability-approval-autonomy/001-status.md` | — |
| M002 | closed | `plans/implementation/execution-reliability-approval-autonomy/002-unified-retry-budget-and-side-effect-reconciliation.md` | `plans/closure/execution-reliability-approval-autonomy/002-status.md` | — |
| M003 | closed | `plans/implementation/execution-reliability-approval-autonomy/003-approval-router-and-durable-mode-state.md` | `plans/closure/execution-reliability-approval-autonomy/003-status.md` | — |
| M004 | closed | `plans/implementation/execution-reliability-approval-autonomy/004-selected-model-and-runtime-preference-convergence.md` | `plans/closure/execution-reliability-approval-autonomy/004-status.md` | — |
| M005 | closed | `plans/implementation/execution-reliability-approval-autonomy/005-production-sandbox-policy-wiring.md` | `plans/closure/execution-reliability-approval-autonomy/005-status.md` | — |
| M006 | closed | `plans/implementation/execution-reliability-approval-autonomy/006-automatic-approval-reviewer.md` | `plans/closure/execution-reliability-approval-autonomy/006-status.md` | — |
| M007 | closed | `plans/implementation/execution-reliability-approval-autonomy/007-yolo-auto-and-full-host-user-surfaces.md` | `plans/closure/execution-reliability-approval-autonomy/007-status.md` | — |
| M008 | closed | `plans/implementation/execution-reliability-approval-autonomy/008-fault-injection-and-reliability-qualification.md` | `plans/closure/execution-reliability-approval-autonomy/008-status.md` | — |