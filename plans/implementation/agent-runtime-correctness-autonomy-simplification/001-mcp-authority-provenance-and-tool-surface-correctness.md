# Agent Runtime Correctness, Autonomy, and Simplification M001 — MCP Authority, Provenance, and Tool-Surface Correctness

Status: implemented

Source subsystem roadmap:

- `plans/subsystems/agent-runtime-correctness-autonomy-simplification-roadmap.md`
- Milestone M001

Repository baseline reviewed: `e88d6f4f67ff729c894b228ddb8b5324582f3fbc`

Primary class: security/correctness invariant

Dependencies:

- hard: none
- interface: existing `PermissionChecker`, Tool Broker, MCP service/tool-definition APIs, and `ToolExecutionContext`
- soft: M005 will consume the corrected authority/provenance semantics

Relevant ADRs and long-term requirements:

- `plans/adrs/ADR-0001-programmatic-tool-execution-authority.md`
- `plans/000-long-term-specification.md` — daemon-owned execution authority and bounded agent/tool authority
- `plans/001-terminology-and-domain-model.md` — execution context, principal, run, tool, workspace
- `plans/003-planning-process.md` — invariants and security effects

Target closure record:

- `plans/closure/agent-runtime-correctness-autonomy-simplification/001-status.md`

## 1. Objective

Correct external/MCP tool authorization and provenance so tool origin never expands authority, permission decisions remain attributable, and the model-facing MCP tool surface cannot remain stale when tool identities or schemas change.

This milestone is deliberately narrow. It fixes three related authority-boundary defects:

1. remove blanket loop-level auto-approval of MCP tools when `PermissionChecker` returned `Ask`;
2. carry a truthful permission decision/receipt into `ToolExecutionContext` rather than synthesizing authoritative-looking decision revisions after the fact;
3. replace MCP tool-count cache invalidation with identity/schema-aware revisioning or hashing.

## 2. Explicit non-goals

Do not:

- disable MCP support or force every MCP call to prompt regardless of explicit policy;
- add a second MCP-specific permission subsystem;
- redesign the Tool Broker or Tool Program authorization model;
- add a general-purpose effect type lattice beyond what is needed to distinguish known read-only/trusted behavior from unknown mutation;
- persist a new database permission-receipt schema unless repository evidence proves ephemeral plumbing is impossible;
- change public MCP protocol semantics or server interoperability;
- add network sandboxing, OAuth policy, secret vaulting, or plugin permission redesign;
- move external-tool policy into string-prefix heuristics under another name;
- broaden CI or verification beyond focused authority tests and the normal quick check.

## 3. Current implementation evidence

Inspect at minimum:

- `src/agent/loop.rs` permission evaluation and `ToolPermissionOutcome` handling;
- `src/permission/mod.rs` category/default/rule evaluation and persistence;
- `src/tool/backend.rs` and `src/tool/broker.rs` execution context/provenance fields;
- `src/tool/tool_program_context.rs` if decision/provenance metadata is shared with nested execution;
- `src/mcp/` tool metadata and tool-list refresh paths;
- `src/agent/loop.rs::build_tool_definitions()` and `ToolDefCache`;
- managed backend wrapper metadata for `websearch`/`webfetch` and any configured external backend abstractions;
- tests around MCP permissions, tool definitions, broker authority, and Tool Program authority.

Known defects at the reviewed baseline:

- `PermissionChecker::check()` treats unknown tool names as mutating and normally returns the configured/default permission result. The default ruleset is `Ask`.
- `AgentLoop` has a special path that auto-allows an `Ask` if the tool is `mcp__*` or a local file mutation and its path is considered within the working directory. For MCP calls with no meaningful local path, that local containment test does not prove the remote side effect is safe.
- an MCP tool can represent remote mutation such as issue creation, email, database writes, infrastructure changes, or third-party API state; external origin therefore cannot justify auto-approval.
- `build_tool_execution_context()` labels decision outcome as `allowed` and constructs decision/policy revision strings from session/workspace identity. These values are not the actual permission decision artifact described by the comments.
- `build_tool_definitions()` uses MCP tool count as part of cache invalidation and explicitly documents that equal-count identity/schema changes can return stale cached definitions.

## 4. Invariants that cannot regress

- `PermissionResult::Ask` may become `Allowed` only through an explicit user decision, persisted allow rule, configured policy, or a directly provable permission-free/read-only classification.
- `mcp__` naming is never sufficient evidence of safety.
- lack of a local path is not equivalent to workspace containment or read-only behavior.
- managed CodeGG wrappers that delegate to external backends retain the wrapper's established effect/permission semantics; raw MCP tools are evaluated independently.
- a persistent `Deny` remains authoritative.
- Tool Program nested calls continue through the canonical Tool Broker and may not bypass the corrected decision path.
- execution provenance fields represent real evaluated state. If a revision/decision identifier does not exist, the field is absent rather than fabricated.
- secrets and raw credentials never enter provenance identifiers or cache keys.
- model-facing tool definitions change whenever MCP identity, parameter schema, defer-loading state, or other provider-visible definition metadata changes.
- the cache must not require blocking the main loop on long MCP writes solely to obtain a revision.

## 5. Target authorization model

Prefer one common permission result structure that can carry both the decision and its provenance. A representative internal shape is:

```text
PermissionDecisionReceipt {
    outcome: Allow | Deny | Ask,
    source: configured-rule | persisted-decision | permission-free-category | user-choice | ...,
    principal/session/workspace identity as already known,
    matched policy/rule identity when one actually exists,
    path/effect scope when applicable,
    issued_at,
}
```

The exact type/name is implementation-dependent. The important properties are:

- permission evaluation owns the decision receipt;
- execution consumes the receipt;
- execution does not reconstruct a more authoritative story from unrelated IDs;
- `Ask` remains a live interaction state until explicitly resolved;
- when the user resolves `Ask`, the final allow/deny receipt records that transition.

Do not add fields whose values are merely hashes/formatted strings without a real policy concept behind them.

## 6. MCP effect/category requirements

First inspect whether MCP tool definitions already carry annotations/hints sufficient to establish read-only versus mutating behavior. If a supported MCP protocol annotation is present and CodeGG currently trusts it, preserve that behavior only after reviewing the trust boundary.

Default posture:

- known CodeGG-managed read-only wrapper: use its existing native/wrapper category;
- raw MCP tool with explicit locally configured allow rule: allow according to normal rules;
- raw MCP tool with persisted allow/deny: honor the persisted decision;
- raw MCP tool with an explicit trusted read-only classification that CodeGG intentionally supports: may be permission-free only through the same typed category path used by native tools;
- unknown raw MCP tool: mutating/ask by default;
- raw MCP tool with no path: do not run workspace-containment auto-approval logic.

If MCP annotations are self-declared by an untrusted server, do not automatically convert them into local authority without an explicit CodeGG trust/configuration decision.

## 7. MCP tool-surface revision requirements

Replace count-only invalidation with one of these preferred mechanisms, in order:

1. MCP service exposes a monotonically increasing catalog revision whenever provider-visible tool definitions change;
2. MCP service exposes a stable digest over sorted provider-visible definitions;
3. the loop computes a bounded stable digest over the current filtered MCP definitions before using the cache.

The revision/hash must incorporate at least:

- external server identity/name where relevant;
- model-facing tool name;
- description if providers receive it;
- JSON parameter schema;
- defer-loading state;
- any provider-visible annotation used to construct the definition.

A pure tool count is insufficient.

Do not hash secrets, tokens, transport credentials, or raw connection configuration.

## 8. Ordered work packages

### Work package A — Trace current permission decision ownership

1. map native, MCP, wrapper, plugin, and Tool Program permission entry points;
2. identify exactly where `Ask` becomes allow/deny;
3. list all cases currently auto-approved by `AgentLoop`;
4. distinguish local file mutation auto-approval from external MCP auto-approval;
5. preserve existing safe local mutation UX unless a concrete defect is found.

### Work package B — Remove blanket MCP auto-allow

1. delete `is_mcp_tool()` from the loop-level `Ask` auto-approval condition or otherwise prevent external origin from granting authority;
2. ensure no-path MCP calls remain `Ask` unless another explicit policy allows them;
3. retain persisted/configured allows and denies;
4. verify managed read-only wrappers continue to use their typed native category/policy path;
5. add focused regression tests for raw external mutation tools.

Required regression cases include:

- unknown `mcp__mail__send` with no path -> Ask, not Allow;
- unknown `mcp__db__update` with a path-looking argument -> Ask unless explicit rule allows;
- configured explicit allow -> Allow;
- persisted deny -> Deny;
- trusted/read-only classification, if supported -> permission-free through typed category path only.

### Work package C — Introduce truthful permission receipt plumbing

1. identify the smallest type/change needed for permission evaluation to return decision provenance;
2. carry the accepted final allow decision into tool execution context;
3. remove synthesized `permission-revision:<workspace>` / equivalent strings unless they correspond to real policy revisions;
4. leave optional fields `None` when no real revision/receipt identifier exists;
5. ensure Tool Program/broker consumers still receive enough lineage to enforce ADR-0001 without duplicating policy.

Do not make the new receipt a second permission evaluator.

### Work package D — Correct MCP tool-definition cache invalidation

1. add/expose service catalog revision or stable hash;
2. update `ToolDefCache` keying;
3. remove the documented equal-count stale-cache limitation;
4. test same-count schema/name replacement invalidates the cache;
5. test unchanged definitions reuse the cache;
6. preserve bounded/nonblocking behavior around MCP service locking.

### Work package E — Documentation and diagnostics

Update only docs that describe actual authority/cache ownership, likely:

- `architecture/tool.md`;
- `architecture/permission.md`;
- `architecture/agent.md` if it documents loop-level approval;
- MCP architecture docs if a catalog revision becomes part of service ownership.

Diagnostics should state why a tool is asking/denied without logging secrets or full sensitive arguments.

## 9. Storage, protocol, migration, and compatibility effects

Storage:

- no schema migration is expected;
- existing permission persistence remains readable;
- avoid durable receipt storage unless required for an existing audit contract.

Protocol:

- no public MCP or daemon protocol change is expected;
- an internal MCP catalog revision may remain in-process.

Compatibility:

- explicit user/configured MCP allows remain supported;
- users who previously relied on blanket raw-MCP auto-allow may now receive an approval prompt. This is an intentional security correction, not a feature removal;
- managed wrapper tools keep their existing user-facing names and normal behavior.

## 10. Concurrency, cancellation, and failure semantics

- permission prompts retain the existing bounded timeout/cancellation behavior;
- catalog revision reads must not introduce an unbounded wait on an MCP write lock;
- if MCP tool definitions cannot be read during refresh, prefer a clearly logged temporary omission/retry behavior over serving a known-stale schema indefinitely;
- cache revisioning must remain race-safe if tool refresh occurs between two turns;
- a denied external call produces a normal typed tool denial result and must not trigger authority-broadening recovery in M005.

## 11. Focused verification

Add/run focused Rust tests for:

```text
MCP Ask remains Ask
configured/persisted allow and deny precedence
same-count MCP schema replacement invalidates cache
unchanged MCP catalog reuses cache
execution context receives actual permission receipt fields
missing revision fields remain absent rather than synthesized
```

Then run:

```bash
scripts/verify.sh quick
```

Run broader Tool Program authority tests only if shared broker/permission types changed. Do not require a full workspace test solely for this milestone if focused tests plus quick verification are green.

## 12. Static guards

Do not add a new regex/static guard for `is_mcp_tool` or decision strings.

Prefer regression tests and type ownership. If construction makes it impossible to execute without a permission receipt, that is stronger than a textual guard.

## 13. Acceptance criteria

M001 closes only when:

- blanket MCP `Ask` auto-approval is removed;
- raw external tools with unknown effect default to normal mutating/Ask behavior;
- no-path MCP calls are not treated as workspace-contained authority;
- configured/persisted allow/deny behavior remains intact;
- any trusted read-only MCP path is explicit and typed, not name-prefix based;
- `ToolExecutionContext` provenance is derived from the real evaluated decision or leaves unavailable fields absent;
- synthetic policy revision strings that do not represent actual revisions are removed;
- equal-count MCP identity/schema changes invalidate the tool-definition cache;
- unchanged MCP definitions may still reuse cached model-facing schemas;
- focused tests and `scripts/verify.sh quick` pass;
- no new permission subsystem, storage migration, CI lane, or release machinery is added.

## 14. Stop conditions

Stop and escalate rather than broadening scope if:

- the only way to distinguish safe versus mutating MCP behavior requires a public protocol/effect-system redesign;
- existing external clients depend on fabricated provenance strings as a public contract;
- permission-receipt correctness requires a persistent audit schema migration not already planned;
- the MCP service cannot provide stable tool-definition identity without a larger ownership redesign.

In those cases, document the evidence and create a narrower follow-up/ADR instead of improvising a broad subsystem change.

## 15. Required closure evidence

`plans/closure/agent-runtime-correctness-autonomy-simplification/001-status.md` must include:

- implementation commit/PR;
- before/after permission decision path for raw MCP;
- regression test matrix for unknown/allow/deny/read-only cases;
- description of permission receipt/provenance fields and removed synthetic fields;
- MCP cache revision/hash mechanism and equal-count replacement test evidence;
- focused verification and quick-check outcomes;
- compatibility note for users who now see an approval prompt where blanket MCP auto-allow previously occurred;
- unresolved findings classified by severity.
