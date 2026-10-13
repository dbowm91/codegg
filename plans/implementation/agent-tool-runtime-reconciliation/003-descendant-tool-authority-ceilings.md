# Agent Tool Runtime Reconciliation M003 — Descendant Tool Authority Ceilings

Status: sequenced; blocked until M002 closure

Repository baseline: `8535f6d7c2e68f5d134ba0c73bd146ca8bc0bd03`

Source roadmap: `plans/subsystems/agent-tool-runtime-reconciliation-roadmap.md#6-dependencies-and-milestones`

Long-term requirements: `plans/000-long-term-specification.md#16-durable-multilevel-agent-run-hierarchy`, `#27-security-requirements`, `#29-system-invariants`, `#87-project-authorization`; `plans/003-planning-process.md`.

Applicable ADRs: retain the durable AgentRun, permission and sandbox owners. Proposed ADR required if existing durable grant policy cannot express immutable inherited ceilings.

Primary class: invariant

## 1. Objective

Make subagent **model discovery and actual tool execution** inherit an explicit, host-attested ceiling from the parent turn, including both the typed capability set and exact eligible tool identities. Close the gap between the existing `ResolvedToolSurface` intersection API and production subagent builders that currently pass `None`.

## 2. Readiness and evidence

Hard dependency: M002 external catalog/generation closure; this allows the descendant suite to include native, plugin and MCP tools, not a partial identity universe.

- `src/agent/tool_surface.rs::resolve_with_categories` already accepts an optional `AgentCapabilitySet` and filters capabilities; `from_registry_with_aliases` does likewise.
- `src/agent/worker.rs` constructs isolated child registries, carries `denied_tools` and parent/child `SandboxProfile`, and uses `resolve_child_sandbox` to prevent sandbox widening, but calls resolved-surface construction with `None` as parent ceiling.
- `src/agent/turn_runtime.rs` also passes `None`, and `src/agent/request_preparation.rs` resolves its own surface with `None`. Parent inheritance is thus not a proven invariant at the model-facing boundary, despite other existing restrictions.
- Typed capability intersection **alone is insufficient** for exact-name restrictions: two different tools may share `FilesystemRead` or `PluginExecute`. Losing permission for one exact name cannot be undone by a broad true capability bit.
- Existing agent permissions, WorkPlan/task controls, path allowlists, workspace locks, sandbox, project auth and durable AgentRun owner must remain authoritative, not be replaced.

## 3. Mandatory invariants

A child's eligible tool names/effects are a subset of its parent's **effective** eligible names/effects, except explicit harmless host-internal operations that are not model-visible and still need separate authorization. A child may narrow but never widen through requested role, aliases, provider quirks, plugin/MCP registration, tool_search, skill assets, configuration refresh or nested delegation. Backend availability and user/project approval remain additional requirements. A stale/forged/missing ceiling on a child must fail closed, not become `None` unrestricted.

A parent deciding to delegate never implies authorization to execute child file/network mutations that the parent cannot perform. Worktree isolation and `SandboxProfile` narrowing continue independently of the capability ceiling: capability membership is **not** a substitute for syscall sandbox or scoped path grant. The effective tool contract's effect class must be checked against the parent's host-minted maximum effect envelope and the child's current grant at invocation.

No model-authored field can forge a trusted parent ceiling. Credentials, tokens, remote endpoint URLs, and raw policy artifacts are not exposed to agents.

## 4. Scope

### A — Authority carrier and producer

Introduce a typed, immutable `ParentToolAuthoritySnapshot` (or extend an existing trusted run/turn envelope) containing: stable parent run/turn/workspace/principal IDs, eligible canonical tool names, `AgentCapabilitySet`, per-tool effect/caller constraints where needed, resolved tool surface fingerprint, registry generation/MCP and plugin activation generation, policy revision and authority expiry/revocation context. Use a host-only constructor at the decision/permission boundary, never deserialize directly from model tool arguments.

When submitting a child via `task`, scheduler/durable AgentRun and compatibility subagent paths must carry or resolvably reference the parent snapshot; use existing identity/session contexts rather than a separate global store. If durable resumption requires persisted authority binding, first audit the current AgentRun policy snapshot format and determine the smallest additive, versioned change with migration and cleanup. Do not invent a new database table merely to pass 15 booleans.

### B — Child surface intersection

Supply the trusted snapshot in `SubAgentRequest` and child `TurnRuntime` /`AgentLoop` preparation; update all production `ResolvedToolSurface` call sites for true child turns. Explicit root/no-parent cases are distinct and permitted; only a root may omit a parent ceiling. Intersect exact-name allow set **and** capability bits, then agent-role denies/plan restrictions/disabled tools, backend availability and sandbox. Canonicalize wire aliases before membership checking, reject alias collisions, and include parent generation/policy in child surface/cache identity.

For a role with a new safer wrapper that was not advertised to parent, default deny unless an explicit, narrower host-held allow-rule is reviewed and documented. Do not rely on blanket capability membership to grant arbitrary external MCP/plugin tools.

### C — Invocation authorization

At the **broker admission** for every child tool, verify the run-bound authority envelope, exact canonical name, contract effect class, workspace/principal, expiration/revocation/generation and actual permission receipt, in addition to already implemented broker scope and policy validation. Check the existing `BrokerInvocationContext`/`ToolAuthorityGrant` fields and extend minimally; never trust a child-provided string such as `parent_id` as the grant. If an MCP/plugin tool is newly injected after child launch, its absence from the pinned parent identity set means deny, even if advertised in a later catalog.

Nested children re-intersect against the **immediate** parent's effective ceiling, retaining the full ancestry upper bound without broadening. New user approval may authorize a previously denied operation only through the canonical approval/delegation policy — not through a model-authored escape hatch; if semantics conflict, stop for ADR and fail closed.

### D — Runtime/diagnostics

Update `src/agent/worker.rs`, `src/agent/turn_runtime.rs`, `src/agent/request_preparation.rs`, `src/agent/tool_surface.rs`, task/scheduler integration, and broker authority only as needed. Preserve read-only child fast paths, max-depth, worktree permissions, delegation counts and durable restart behavior. Emit bounded deny reasons (`ParentCeiling`, stale generation, forbidden effect, invalid parent receipt) with fingerprints but no sensitive policy payloads. Update architecture documentation and add a static check prohibiting `None` parent ceiling in child production constructors.

## 5. Ordered work packages

1. **Evidence map**: enumerate root/child/nested/durable/restart and tool-call entry points; trace existing permission receipts, request DTOs and producer lifetime. Output explicit policy carrier design that avoids redundant state.
2. **Trusted snapshot**: implement host-only creation and transport, strong typing and validation; define compatibility for already running child tasks and resumed durable runs.
3. **Surface projection**: compute child eligible canonical set and typed ceiling at all call sites; make discovery, exact schema, provider payload, cached definitions and prompt tool-name lists match.
4. **Broker enforcement**: validate the inherited snapshot and per-tool effect grant at call time; test read-only, plugin and raw MCP, deny even if called by hand-crafted tool-call JSON.
5. **Qualification and docs**: nested delegation, generation/drift, cancellation, restart and contention; add static guard, docs, closure matrix.

## 6. Tests

Matrix across root, plan, general child, verifier/read-only child, nested child depth 2+, durable resumed child, and compatibility in-memory child:

- Parent may use `read` but denies `git_query`: child cannot discover/invoke `git_query` despite a common read capability.
- Parent denies `plugin__one__write` but allows another plugin tool: no child amplification via generic `PluginExecute`.
- Parent includes one MCP server/tool but not another: no new raw tool after server reconnect/tool-list expansion.
- Parent lacks file write, terminal, shell mutating, network research, image, memory, goal or work-order capability: assert denial for every related tool, including `tool_search` negative results and manual dispatch.
- Child lower sandbox/read-only restriction holds even if parent has FullHost and requested child asks for broader profile; existing failure rules remain.
- Nested child cannot resurrect an ancestor-denied tool through aliases, profile changes or newly activated plugin.
- Forged parent ID, forged/stale snapshot fingerprint, mismatched workspace/principal, expired/revoked permission, tool contract effect change: reject before external side effect.
- Durable rehydration/restart, cancellation and concurrent parent policy revocation: defined fail-closed response, no orphaned authorization or silent fallback.

Ensure test fixture runs both immediate and deferred catalogs and non-deferral provider paths. Record any live-parent authority refresh limitations.

## 7. Failure, cancellation, restart and compatibility

The authority snapshot must be immutable for one child invocation; later revocations narrow future calls immediately through the canonical current-policy check, not via mutation of an in-flight cache. Child continues only while its pinned parent envelope remains valid. On restart, fail closed on missing/stale authority and follow existing durable resumption reconciliation; do not reissue user permissions automatically. If parent has terminated, use the existing durable delegation/run policy to determine whether an authorized child continues — never infer continued authority merely from serialized `parent_id`.

No public model tool rename. Preserve `task` argument compatibility and current session, worktree and sandbox APIs. Any durable or wire DTO additions must be additive, versioned as appropriate with migration/rollback tests. Stop for architectural approval if durable run authority must be changed rather than consumed.

## 8. Required verification

```bash
cargo fmt --all -- --check
cargo test --workspace tool_surface
cargo test --workspace subagent
cargo test --workspace agent_run
cargo test --workspace tool_broker
cargo test --workspace tool_search
cargo clippy --workspace --all-targets -- -D warnings
bash scripts/verify.sh quick
```

Reconcile test selectors against actual names at implementation; run existing sandbox/approval/worktree and tool Program guard suites. Security negatives and multi-child concurrent tests are mandatory. Record commands and run/not-run platform evidence in closure.

## 9. Acceptance criteria and stop conditions

Completion requires a real parent-to-child-to-broker trace showing the same **host-attested** effective tool ceiling; model discovery, provider definitions, cache and invocation agree; exact-name denies and capability denies enforce independently; forged child calls fail before execution; nested/durable/restart cases tested; no child constructor defaults to unrestricted `None`; current sandbox/worktree/approval invariants are green.

Stop when trusted authority is unavailable, the requested child grants more authority, durable run recovery cannot prove a valid parent policy chain, the runtime requires an unreviewed authority database or protocol rewrite, or a new tool cannot be classified conservatively. Classify these as blocked/corrective rather than claiming closure.

Closure: `plans/closure/agent-tool-runtime-reconciliation/003-status.md`, with commit/PR SHAs, matrix, tests/guards, policy revocation and restart evidence, compatibility/migration findings and residual security risks.
