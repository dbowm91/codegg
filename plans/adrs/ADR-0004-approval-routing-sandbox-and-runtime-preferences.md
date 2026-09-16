# ADR-0004: Approval Routing, Sandbox Authority, and Runtime Preferences

Status: accepted

Date: 2026-09-16

Decision owners: project maintainers

Related specification sections:

- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#8-deployment-profiles-and-authentication`
- `plans/000-long-term-specification.md#24-protocol-and-storage-requirements`
- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#28-observability`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/001-terminology-and-domain-model.md` — principal, session, turn, execution context, provider connection

Affected subsystem roadmaps:

- `plans/subsystems/execution-reliability-approval-autonomy-roadmap.md`
- `plans/subsystems/runtime-safety-resource-footprint-roadmap.md` (sandbox implementation foundation; not reopened)

Related ADRs:

- `plans/adrs/ADR-0001-programmatic-tool-execution-authority.md`
- `plans/adrs/ADR-0002-agent-run-worktree-isolation-and-control.md`

External design input:

- OpenAI Codex approval-policy/sandbox separation and automatic approvals reviewer in the current Codex CLI/config implementation.
- Anthropic, “How we built Claude Code auto mode: a safer way to skip permissions,” `https://www.anthropic.com/engineering/claude-code-auto-mode`.
- Claude Code permission and sandbox documentation, including deterministic policy before classifier review and separate OS-level containment.

## Context

CodeGG already has useful deterministic permission and security machinery:

- `PermissionChecker` merges config/session/agent rules and persistent decisions;
- read-only and safe-mutating tools normally bypass prompts;
- ordinary non-destructive shell commands are auto-allowed even when the default is `ask`;
- common in-workspace file mutations receive a narrow prompt-fatigue exception;
- destructive command matching, sensitive-path classification, and `SecurityService` can escalate or deny;
- `PermissionDecisionReceipt` records the accepted decision source/revision;
- the managed-process/Landlock implementation can enforce a filesystem sandbox on supported Linux hosts.

The current ownership is nevertheless fragmented. `AgentLoop::check_tool_permission()` directly creates human permission requests in several branches. There is no user-facing approval mode that cleanly selects human approval, automatic reviewer approval, or no approval. `with_exec_mode()` is a special headless bypass rather than a coherent user/runtime mode.

Sandboxing and approval are also insufficiently connected. `SandboxConfig` defaults disabled and the normal `ToolRegistry::with_options()` path builds `BashTool` without a sandbox policy. Workspace-root validation constrains the process working directory, not arbitrary paths a shell command can read or write. Landlock is filesystem containment only; network isolation is not currently equivalent.

Persistence is split as well:

- `PermissionStore` supports atomic/HMAC-backed persistence, but the production constructors inspected at this baseline pass `store_path = None`, so “always allow/deny” decisions on those paths do not survive process restart;
- daemon-owned provider session selection is durable, but the simple `CoreRequest::ModelSelect` path only updates `runtime.selected_model`;
- the TUI restoration manifest remembers a selected model as a display hint, but explicitly cannot own daemon authority or security preferences;
- no daemon-owned durable “last approval mode / sandbox profile / last model” preference contract exists.

The requested Automatic mode introduces a model-backed reviewer and therefore changes authorization semantics. YOLO/Full Host changes effective execution authority. These must be explicit architecture decisions rather than ad-hoc TUI behavior.

## Decision drivers

- Deterministic policy must remain the first and cheapest decision layer.
- A model reviewer must never override a hard deny, widen a sandbox, or grant authority unavailable to the primary principal/session/agent.
- “Stop prompting me” and “give the agent full host access” are different requests and must remain different modes.
- Sandbox enforcement and approval prompting must be orthogonal so an automatic reviewer cannot silently disable containment.
- User-selected risky modes need clear warnings but must remain practical for trusted local development.
- Approval mode, sandbox profile, and model selection must survive restart through daemon-owned state, not model transcript or frontend-only manifests.
- Child/subagent authority remains an intersection/ceiling of parent authority and cannot be expanded by its own reviewer.
- Reviewer failure must be typed and observable; malformed model output is not a security decision.
- Current permission rules, security classification, tool contracts, audit receipts, and runtime-safety process machinery should be reused.

## Considered options

### Option A — Add `auto` and `yolo` directly to `PermissionLevel`

Extend `Allow | Ask | Deny` with reviewer/bypass variants and make each permission check decide how to execute them.

Benefits:

- small surface change;
- no new router type.

Costs and failure modes:

- conflates deterministic policy result with who resolves an escalation;
- repeats reviewer/human/bypass behavior at every call site;
- makes hard-deny precedence and sandbox independence difficult to reason about;
- encourages model-review logic inside low-level policy checks.

Rejected.

### Option B — Let Automatic reviewer classify every tool call

Send every command/tool call to a cheap model and use its answer as the permission decision.

Benefits:

- conceptually uniform;
- potentially nuanced semantic decisions.

Costs and failure modes:

- adds latency/cost to known-safe reads/builds/tests;
- makes deterministic safety policy less authoritative;
- increases prompt-injection surface;
- makes reviewer outages block routine work.

Rejected.

### Option C — Deterministic policy produces `Allow | Deny | Escalate`; one ApprovalRouter resolves only escalations; sandbox policy is independent

Keep current deterministic checks and security classification. Normalize their result into an approval request only when human judgment would otherwise be needed. A single router resolves the request according to `Interactive`, `Automatic`, or `Yolo`. A separately resolved `SandboxProfile` determines process containment. Persist the user's last runtime preferences in daemon-owned state.

Selected.

## Decision

CodeGG will implement these contracts:

1. Deterministic permission, path, security, caller-authority, and explicit deny policy remains authoritative and executes before approval routing. The normalized outcome is `Allow`, `Deny`, or `Escalate(ApprovalRequest)`.
2. `ApprovalRouter` is the sole production owner that resolves an escalation. Tool implementations and `AgentLoop` call sites must not independently invent human/reviewer/bypass behavior.
3. User-facing `ApprovalMode` is a closed enum with at least:
   - `Interactive` — escalations are sent to the human/frontend;
   - `Automatic` — escalations are reviewed by a dedicated bounded approval reviewer;
   - `Yolo` — escalations are accepted automatically within the already-resolved authority/sandbox ceiling.
4. `Yolo` does not override an explicit/hard deny, caller capability ceiling, parent/subagent ceiling, path policy, or administrator/project policy. Those are not escalations.
5. `SandboxProfile` is separate from `ApprovalMode` and has at least:
   - `ReadOnly` — host-enforced filesystem reads only where supported;
   - `WorkspaceWrite` — read/write authority within the effective workspace/approved roots;
   - `FullHost` — no CodeGG filesystem containment beyond OS-user authority and remaining deterministic hard-deny policy.
6. Filesystem and network containment are represented separately in enforcement/audit state. Landlock does not imply network isolation. CodeGG must never label an execution “fully sandboxed” solely because filesystem Landlock succeeded.
7. A turn captures an immutable effective execution-policy snapshot containing approval mode, sandbox profile, policy revision, principal/session/agent ceiling, and reviewer configuration. User changes apply to subsequent turn/tool-policy snapshots and never retroactively mutate a running side effect.
8. The automatic approval reviewer is a special internal agent/runtime role, not an ordinary delegated mutation-capable subagent. It receives a bounded structured request containing user objective/current task, normalized action, predicted effects/scope, deterministic escalation reasons, and optionally the primary model's stated justification as untrusted evidence.
9. Reviewer tools are read-only and non-recursive. It may make a very small bounded number of safe investigation calls when necessary. It cannot write files, run mutating/process tools, expand network/filesystem scope, spawn subagents, or trigger another approval reviewer.
10. Reviewer content treats command text, repository files, tool output, and primary-agent justification as untrusted evidence, never higher-priority instructions.
11. Reviewer output is a strict typed result: `Allow`, `Deny { primary_agent_feedback? }`, or `DeferUser`. Malformed output, timeout, unavailable reviewer model, or internal failure maps to `DeferUser`/typed reviewer-unavailable behavior rather than fabricated approval.
12. Denied Automatic actions return concise feedback to the primary model so it can choose a safer alternative without necessarily interrupting the user. Repeated equivalent denials are bounded and eventually defer to the user or stop in headless mode.
13. `PermissionStore` remains the owner of durable granular always-allow/always-deny decisions. Production constructors must provide the canonical store path; writes remain atomic and tamper verification remains available. Migration must conservatively handle old broad decisions.
14. Daemon-owned `RuntimePreferenceStore` persists non-secret principal-scoped convenience preferences: last selected provider connection/model identity, last `ApprovalMode`, last `SandboxProfile`, timestamps, and revision. Personal-local mode uses the explicit local-owner principal semantics already defined by CodeGG.
15. Runtime preference precedence is:
   explicit invocation/session/turn override > project/administrator policy ceiling > persisted principal preference > configured/built-in default.
   A preference never overrides a deny or authority ceiling.
16. Model preference stores stable connection/model identity only. It is re-resolved against the current bounded provider catalog on use; stale catalog revision is not treated as permission to silently choose another provider/model.
17. `CoreRequest::ModelSelect` and other model-selection surfaces converge on the durable daemon selection service. Runtime-only model mutation is not the final authoritative path.
18. The TUI manifest may continue to store display/restoration hints, but approval/sandbox/model authority is daemon-owned and frontend-neutral. TUI, ACP, and future frontends use the same protocol/settings contract.
19. Subagents inherit an authority ceiling no greater than the parent effective policy. A child cannot select `Yolo`, `FullHost`, or a broader reviewer policy to exceed the parent.
20. `PermissionDecisionReceipt`/tool execution context records the resolved approval source, effective policy/sandbox revision, and reviewer decision identity when applicable without storing hidden reviewer reasoning.

## Consequences

### Positive

- Safe reads, tests, and ordinary workspace edits remain fast and deterministic.
- Automatic mode spends model calls only on genuine escalations.
- YOLO can eliminate prompt fatigue without necessarily giving the agent the entire host filesystem.
- Full Host is explicit and distinguishable from ordinary autonomous workspace work.
- Reviewer prompt injection cannot directly grant itself tools or broaden policy.
- Model/approval/sandbox preferences survive restart consistently across frontends.
- Permission persistence stops depending on an in-memory-only store path.

### Negative

- A frontend-neutral settings/preference protocol and store are required.
- Automatic review introduces provider/model dependency and latency for escalated actions.
- Correctly exposing filesystem versus network containment makes the UI more explicit than a single “sandboxed” boolean.
- Legacy broad persistent permission decisions need compatibility/migration policy.

### Neutral or deferred

- Strong network containment is deferred to a later measured sandbox backend decision. This ADR requires truthful capability reporting, not a new container/network namespace framework.
- Cross-platform sandbox parity is not promised; unsupported hosts expose the enforcement actually available.
- Organization/team policy administration remains part of future team authorization work; the local implementation must already respect the generic policy-ceiling shape.

## Compatibility and migration

- Existing `PermissionLevel`/rules remain readable and map naturally to deterministic Allow/Deny/Escalate semantics (`Ask` becomes Escalate).
- Existing `with_exec_mode()` remains a compatibility/headless adapter until call sites migrate to explicit resolved approval policy.
- Existing permission decision JSON is retained where safe; unsupported/tampered entries are ignored conservatively.
- Runtime preference storage is additive and contains no credentials, prompts, tool output, or filesystem secrets.
- Existing TUI manifests remain readable. Their selected-model hint is not silently promoted to daemon authority without validation.
- Existing session provider selection rows remain canonical for an existing session; “last used” is a preference for new/unselected sessions, not an overwrite of explicit session binding.
- Existing runtime-safety Landlock/process contracts remain canonical implementation machinery and are wired into the effective sandbox profile rather than copied.

## Security and reliability implications

- Hard deny and authority ceiling always precede model review and YOLO.
- Automatic reviewer failure is fail-to-human (or fail-closed in explicitly noninteractive operation), never fail-open.
- Full Host selection requires an explicit high-risk confirmation in interactive frontends; YOLO + Full Host receives the strongest warning.
- Sandbox enforcement failure for an explicitly requested constrained profile fails closed or produces an explicit user-approved escalation; it never silently runs unsandboxed.
- Reviewer/provider retry obeys the same bounded retry contract as other model calls and cannot recursively ask for approval.
- Preference and permission writes are atomic; restart loads only validated records.
- Execution audit distinguishes deterministic allow, human allow, reviewer allow, YOLO allow, deny source, sandbox enforcement obtained, and any escalation delta.

## Verification

Conformance requires evidence that:

- deterministic read/safe actions never call the reviewer;
- explicit denies remain denied in Interactive, Automatic, and Yolo;
- Automatic reviewer cannot write, shell, spawn a subagent, or request broader authority;
- malformed/timeout/unavailable reviewer output never becomes Allow;
- changing reviewer mode cannot change the sandbox profile, including regression coverage for the class of bug seen in other harnesses;
- WorkspaceWrite commands cannot access filesystem paths outside enforced roots on supported Linux;
- FullHost is distinguishable in policy/enforcement/audit output and requires the documented warning path;
- remembered approval/sandbox/model preferences survive daemon/TUI restart while explicit session/project overrides win;
- `ModelSelect` persists through the canonical session-selection service;
- persistent allow/deny decisions survive restart on production construction paths;
- child agents cannot exceed parent approval/sandbox ceilings;
- TUI and headless/core protocol consumers observe the same effective policy state.

## Supersession

None.
