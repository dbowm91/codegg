# Agent Run, Async Delegation, and Worktree Concurrency M008 — Call Identity, Authoritative Projection, and Strict Corrective Closure

Status: active

Repository baseline for planning: `b87d1d5b65aca96c700deb27e579374b3d158545`

Implementation agent MUST rebase the evidence section onto the exact M007 closure head before editing.

Source corrective roadmap:

- `plans/subsystems/agent-run-worktree-concurrency-corrective-closure-addendum.md#m008--call-identity-authoritative-projection-and-strict-corrective-closure`

Hard dependency:

- M007 — `plans/implementation/agent-run-worktree-concurrency/007-durable-lineage-context-and-fanout-corrective-pass.md` must be closed.

Superseded strict subsystem closure disposition:

- `plans/closure/agent-run-worktree-concurrency/006-status.md`

Long-term requirements:

- `plans/000-long-term-specification.md#16-durable-multilevel-agent-run-hierarchy`
- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/001-terminology-and-domain-model.md` — agent run, budget, execution context
- `plans/002-long-term-roadmap.md#phase-9--durable-multilevel-agent-run-service`

Applicable ADRs:

- `plans/adrs/ADR-0001-programmatic-tool-execution-authority.md`
- `plans/adrs/ADR-0002-agent-run-worktree-isolation-and-control.md`

Primary class: correctness / compatibility / closure

## 1. Objective

Finish the corrective workstream by making TaskTool retry/idempotency semantics use the runtime’s canonical model tool-call identity, making run depth and group ownership projection derive solely from authoritative durable state, and performing an independent closure pass over M007 plus the remaining post-M006 findings.

This milestone is intentionally small. It must not reopen the worktree, Git integration, scheduler, or run-group architectures that M007 has already corrected unless its verification exposes a concrete defect.

## 2. Why this is separate from M007

M007 fixes the primary execution-ownership and authorization boundary. M008 then verifies that the model-facing call surface and frontend projection consume that corrected authority faithfully.

Separating the passes is important because:

- idempotency bugs can be masked by otherwise-correct run/group stores;
- projection bugs can make a broken hierarchy look correct or a correct hierarchy look flat;
- strict closure should be authored against the corrected production path rather than by the same change set that introduces it.

## 3. Current implementation evidence to reconfirm after M007

### 3.1 Canonical tool-call identity already exists

`src/tool/backend.rs` defines `ToolExecutionContext.invocation_key` as the stable model tool-call identity used for transport retry deduplication.

`src/agent/tool_batch.rs` populates it from session + provider/model tool-call ID for accepted native invocations.

The base `Tool` trait supports `execute_structured(input, Option<ToolExecutionContext>)`, but TaskTool currently relies on `execute(input)` and therefore discards invocation identity.

### 3.2 Control idempotency currently collapses distinct calls

TaskTool control operations (`message`, `interrupt`, `cancel`) currently default to an idempotency key equivalent to action + target run ID when the input does not contain `idempotency_key`.

`AgentRunControlStore` correctly deduplicates `(run_id, idempotency_key)`. The bug is therefore at the caller identity layer: two intentional message calls to the same child can resolve to the first message.

### 3.3 Spawn/group identity is content-derived

Durable delegation currently derives a delegation key from session, turn, agent, prompt, and allowed paths. Group default identity is also request/content-derived.

This provides retry-like behavior for repeated identical content but conflates two intentional identical tool calls in one turn. Conversely, model/provider retries should deduplicate based on the stable tool-call identity that already exists.

### 3.4 Projection depth is supplied by callers

`codegg_core::projection_replay::agent_run_summary` currently accepts `depth` as a parameter instead of deriving it from `AgentRunRecord`.

Scheduler publication has historically supplied `0`/`1` based on parent presence. After M007, actual durable run depth must be the sole projection source.

## 4. Invariants that MUST NOT regress

- Idempotency is keyed to accepted call identity, not model-authored prose or mutable display text.
- Two distinct tool-call IDs with identical input are distinct intentional operations unless an explicit user/model idempotency key requests coalescing.
- Retry/replay of one accepted tool-call identity is idempotent where the underlying operation supports idempotent acceptance.
- An idempotency key cannot be reused with materially incompatible operation identity without an explicit conflict or equivalent safe rejection.
- Tool-call identity is bounded, non-secret provenance; hidden reasoning is never persisted.
- Projection consumes durable stores and remains derived/non-authoritative.
- Projection depth is not recomputed from path names, TUI nesting, parent presence, or event order.
- Existing old snapshot compatibility remains additive.
- Strict closure does not require new CI lanes, live-provider dependencies, scanners, or release automation.

## 5. Required production changes

### 5.1 Make TaskTool consume ToolExecutionContext

Refactor TaskTool execution so production structured invocations retain the accepted invocation identity.

Representative pattern:

```rust
impl TaskTool {
    async fn execute_with_context(
        &self,
        input: serde_json::Value,
        ctx: Option<&ToolExecutionContext>,
    ) -> Result<String, ToolError> {
        // canonical implementation
    }
}

#[async_trait]
impl Tool for TaskTool {
    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        self.execute_with_context(input, None).await
    }

    async fn execute_structured(
        &self,
        input: serde_json::Value,
        ctx: Option<ToolExecutionContext>,
    ) -> Result<StructuredToolResult, ToolError> {
        let output = self.execute_with_context(input, ctx.as_ref()).await?;
        Ok(StructuredToolResult::legacy(self.name(), output))
    }
}
```

Use existing structured-result/provenance helpers if TaskTool can preserve richer metadata without adding unrelated complexity.

Do not fork two implementations of action parsing.

### 5.2 Canonical call identity precedence

For model-originated TaskTool operations, resolve call identity in this order:

1. explicit `idempotency_key` supplied in the tool input when the action exposes one;
2. accepted `ToolExecutionContext.invocation_key`;
3. a compatibility fallback for direct/legacy `execute()` calls.

Production model calls should normally use (2) automatically; models should not need to invent keys.

Compatibility fallback MUST NOT use a constant action+target key that silently collapses separate calls. A fresh bounded ID is acceptable for direct legacy calls because callers that need retry deduplication can provide an explicit key.

### 5.3 Spawn delegation identity

Distinct accepted model tool calls with identical spawn payload must produce distinct delegated tasks/runs.

For one `spawn` call, derive a stable delegation identity from:

- canonical invocation identity;
- target action/type;
- normalized target scope as needed to prevent accidental cross-action collisions.

The existing payload digest may remain as a validation/fingerprint component, but payload equality must not be the primary identity across separate calls.

A retry of the same invocation identity should resolve to the same durable task/run and scheduler job.

If the same invocation identity is presented with a materially different normalized spawn request, return an explicit idempotency/protocol conflict rather than accepting two different children under one call identity.

If supporting that conflict requires an additive request fingerprint on `AgentTaskRecord`, add the smallest bounded field/migration needed and document it. Do not persist the full prompt merely for deduplication if an existing bounded digest suffices.

### 5.4 `spawn_many` member identities

One `spawn_many` model tool call must deterministically derive a distinct child call identity for each list member, for example:

```text
<parent invocation key>/member/0
<parent invocation key>/member/1
...
```

Requirements:

- retry of the same `spawn_many` call resolves the same accepted member runs/group;
- two different `spawn_many` tool calls with the same request array create distinct groups/children;
- member order is stable and bounded;
- partial acceptance/rejection remains explicit;
- recursive implementation must not call a context-free `self.execute(child)` path that loses the parent invocation identity.

Use a small internal spawn helper taking an explicit resolved call identity rather than serializing hidden implementation state into the model input.

### 5.5 Group idempotency

Group acceptance should default to the parent tool-call identity plus group action scope, not request-content hash alone.

For root turn-owned and run-owned groups introduced/corrected in M007:

- same tool-call retry → same group;
- different tool-call IDs → distinct groups even with identical members/policy, unless the caller explicitly supplies the same idempotency key;
- same key + incompatible owner/member/policy → explicit `IdempotencyConflict`.

Preserve current deterministic join semantics.

### 5.6 Control message identity

For `message`, `interrupt`, and `cancel`:

- default durable mailbox idempotency key is derived from invocation identity + action + target;
- explicit input idempotency key may override;
- message payload remains separately persisted/bounded;
- two distinct message call IDs to one target are two mailbox messages even if payload text is identical;
- retry of the same call ID is one mailbox message;
- same key reused with incompatible kind/target/payload should conflict or otherwise fail safely rather than return an unrelated old message.

Consider strengthening `AgentRunControlStore::enqueue` duplicate-key behavior to compare immutable operation fingerprint fields before returning the existing record if it does not already do so.

### 5.7 Wait/status semantics

Read-only `status`/`wait` do not necessarily need durable idempotency records, but their authorization must use M007’s explicit owner rules.

If `wait` is replayed under the same call identity it may execute another bounded wait against the same durable run; it must not create control messages or alter authority.

### 5.8 Authoritative projection depth

After M007 persists authoritative depth:

- remove `depth` as a caller-supplied argument to `agent_run_summary`, or make the adapter ignore external depth and read `run.depth`;
- all incremental publication uses the same adapter as snapshot/replay;
- reconnect/resync reconstructs exact depth from durable stores;
- TUI tree indentation/ordering consumes the DTO depth but does not infer or mutate it;
- depth `2+` is supported and bounded;
- historical rows with unknown depth are represented conservatively according to M007 migration semantics rather than guessed from `parent_run_id` count in presentation code.

### 5.9 Group-owner projection

If M007 adds turn-owned versus run-owned group metadata, expose only the bounded owner information needed by clients/debugging.

Do not project prompts, mailbox bodies, full paths, authority bodies, or hidden reasoning.

Older snapshots/clients must remain compatible via serde defaults/additive fields.

## 6. Storage and migration

Potential M008 additive storage is limited to call/request fingerprints if M007 did not already add them.

Requirements:

- idempotency identity fields are bounded and indexed where necessary;
- full prompts/messages are not duplicated into fingerprint columns;
- migration is additive/idempotent;
- existing M001–M007 records remain readable;
- historical records whose call identity predates this milestone retain their existing identity and are not silently re-keyed;
- exact same accepted invocation can be resolved after daemon restart.

The existing mailbox/run/group stores remain authoritative; do not create a separate deduplication database.

## 7. Ordered work packages

### A — Regression fixtures for distinct-call collapse

Add failing tests proving current undesired behavior or the equivalent behavior present after M007:

1. two `message` calls with different invocation keys both target the same child and both must be delivered;
2. same message invocation key retried produces one mailbox record;
3. two identical `spawn` payloads with different invocation keys create distinct runs;
4. same spawn invocation key retried returns one run/job;
5. `spawn_many` retry returns same group/member IDs;
6. two identical `spawn_many` calls with distinct invocation keys produce distinct groups.

### B — Structured TaskTool execution

- add one context-aware implementation path;
- wire `execute_structured`;
- resolve explicit/invocation/fallback call identity centrally;
- keep direct tests/legacy callers functional.

### C — Spawn/group/control identity conversion

- convert delegation key generation;
- derive per-member spawn-many identity;
- convert group default idempotency;
- convert mailbox default idempotency;
- add conflict checks where same key can otherwise return incompatible old state.

### D — Projection depth authority

- make run DTO adapter read durable depth;
- remove all 0/1 inference sites;
- update snapshot/incremental/replay/resync code;
- update TUI tests/fixtures for depth >= 2;
- keep projection pure.

### E — Independent corrective audit

Before declaring closure, inspect the exact implementation head for:

- any remaining production `parent_run_id: None` or `depth: 1` constructions that can affect durable descendants;
- any TaskTool path that lacks explicit root-turn/run owner context;
- any same-session control authorization shortcut;
- any context-free recursive `spawn_many` execution that loses invocation identity;
- any projection depth inference outside durable state;
- nested worktree repository/base identity;
- scheduler ownership and duplicate pool semaphore behavior;
- dirty/conflicted cleanup behavior;
- compatibility TaskStore accidentally becoming authority.

Use search as evidence discovery, then inspect each production hit rather than relying on naming-only grep.

### F — Documentation and governance

Update as applicable:

- `architecture/agent.md`;
- `architecture/scheduler.md`;
- `architecture/projection.md`;
- `architecture/worktree.md` only if M007 behavior needed correction;
- task tool contract/prompt guidance;
- corrective addendum status;
- registry;
- M008 closure record.

Do not rewrite M006 closure record. Mark it historical/superseded only from the additive corrective documents/registry.

## 8. Required tests

### Tool-call identity

- `ToolExecutionContext.invocation_key` reaches TaskTool production execution;
- explicit idempotency key overrides invocation key where supported;
- direct `execute()` compatibility fallback creates distinct calls rather than constant-key collapse;
- same invocation + same normalized action retries idempotently;
- same explicit key + incompatible action/input returns conflict.

### Mailbox

- message A then message B to one run yields sequence N, N+1;
- identical payload under two call IDs still yields two messages;
- same call ID retry yields one message;
- interrupt/message do not collide;
- cancel retry remains one durable cancellation intent;
- restart preserves deduplication state.

### Spawn

- same payload, two invocation IDs → two tasks/runs/jobs;
- same invocation retry → same task/run/job;
- different payload under same explicit idempotency key → conflict;
- nested spawn uses caller child-run owner from M007.

### Spawn-many/groups

- one call yields deterministic member sub-identities;
- retry yields same member IDs/group;
- second distinct identical call yields new member IDs/group;
- partial rejected members remain stable across retry;
- both turn-owned and run-owned group forms work.

### Projection

- root/top-level delegated child depth `1`;
- grandchild depth `2`;
- deeper permitted fixture depth `3` if configured;
- snapshot and incremental event produce identical DTO for the same record;
- reconnect/resync preserves depth and owner information;
- old snapshot missing additive fields still deserializes;
- projection does not disclose prompt/path/mailbox/authority bodies.

### Corrective production scenario

A deterministic end-to-end fixture should prove:

```text
root turn
  spawn_many[A, B]       (invocation X)
  message A "focus X"   (invocation Y)
  message A "also Y"    (invocation Z)
A depth 1
  spawn C                (invocation A1)
C depth 2, isolated worktree when mutating
root/group wait completes
projection reconnect reconstructs A/B/C and exact depth
```

Retry selected invocation IDs in the fixture and prove no duplicate acceptance.

## 9. Verification posture

Run focused tests first. Expected equivalents:

```text
cargo test -p codegg-core agent_run --locked -- --test-threads=1
cargo test -p codegg-core agent_run_control --locked -- --test-threads=1
cargo test -p codegg-core agent_run_group --locked -- --test-threads=1
cargo test -p codegg-protocol projection --locked -- --test-threads=1
cargo test --lib agent --locked -- --test-threads=1
cargo test --lib scheduler --locked -- --test-threads=1
cargo test --test subagent --locked -- --test-threads=1
cargo test --test session_projection_consumer --locked -- --test-threads=1
cargo test --test scheduler_restart_recovery --locked -- --test-threads=1
cargo test --test scheduler_cancellation --locked -- --test-threads=1
cargo test --test scheduler_contention --locked -- --test-threads=1
cargo test --test worktree --locked -- --test-threads=1
```

Then run the repository’s existing quick broad verification on the exact closure candidate:

```text
scripts/verify.sh quick
cargo fmt --all -- --check
git diff --check
```

Run existing relevant static guards, including scheduler bypass, execution ownership, core boundary, daemon cwd/path identity, Git forbidden patterns, tool-broker boundary if structured execution is touched, and projection disclosure.

Do not add new CI lanes, mandatory workflow-dispatch gates, coverage/benchmark/size gates, dependency bots, third-party scanners, or release automation.

Hosted CI is not a new hard requirement for this subsystem unless repository governance already requires it for the exact candidate. If hosted evidence is unavailable, the closure record must say so rather than fabricate it.

## 10. Acceptance criteria

M008 may recommend strict subsystem closure only when all are true:

1. M007 closure is accepted.
2. TaskTool production structured execution receives canonical invocation identity.
3. Distinct message calls to the same run no longer collapse.
4. Same message call retry remains idempotent.
5. Distinct identical spawn calls create distinct durable runs.
6. Same spawn call retry resolves to one durable run/job.
7. `spawn_many` preserves per-member identity across retry and distinguishes separate parent calls.
8. Group default idempotency uses call identity rather than payload equality alone.
9. Same key + incompatible immutable operation data fails explicitly.
10. Projection depth comes from durable run state, including depth >= 2.
11. Snapshot, incremental replay, reconnect, and resync agree.
12. Root-turn versus run-owned group projection/authorization remains correct.
13. M007 authorization negative cases still pass.
14. Nested mutation/worktree/base tests from M007 still pass.
15. Scheduler remains sole machine-resource authority.
16. No critical/high/medium corrective finding remains.
17. Required focused tests and repository quick verification pass on the exact candidate or any unavailable external evidence is explicitly classified.
18. Registry and corrective addendum are reconciled atomically with the closure disposition.

## 11. Strict closure audit matrix

The M008 closure record MUST explicitly disposition every post-M006 finding:

| Finding | Required disposition |
|---|---|
| F1 root fan-out owner missing | fixed and production-tested by M007 |
| F2 current/parent run conflation | fixed and multilevel-tested by M007 |
| F3 nested context incomplete | fixed including repository/group/turn propagation by M007 |
| F4 depth not authoritative | persisted/enforced by M007; projected here |
| F5 control authorization incorrect | fixed with negative tests by M007 |
| F6 call idempotency collapses distinct operations | fixed by canonical invocation identity in M008 |
| F7 projection depth non-authoritative | fixed by durable-state projection in M008 |

A passing compile/test suite without this requirement-to-evidence matrix is not sufficient strict closure.

## 12. Closure evidence required

Create:

- `plans/closure/agent-run-worktree-concurrency/008-status.md`

The record MUST include:

- exact implementation and closure candidate commits;
- reference to historical M006 strict closure as superseded disposition;
- M007 closure record and implementation evidence;
- F1–F7 matrix;
- production-shaped fan-out/nested/message/projection evidence;
- migration and backward-compatibility evidence;
- authorization/security review;
- cancellation/restart/contention/worktree review;
- exact commands/results;
- hosted evidence status if any;
- unresolved findings by severity;
- final recommendation.

If any medium-or-higher correctness/security issue remains in F1–F7 scope, recommend `corrective pass required`, not `closed`.

## 13. Registry disposition after closure

Only after accepted M008 closure:

- mark the agent-run/worktree corrective addendum `closed`;
- mark subsystem row `closed` with M008 as current/last milestone;
- remove M007/M008 from dependency-ready/blocked sections;
- retain M006 as historical superseded strict closure evidence;
- link `008-status.md` as the controlling final disposition.

No unrelated plan should be unblocked unless its source plan explicitly names this corrective closure as a dependency.

## 14. Stop conditions

Stop and create a follow-up/ADR instead of broadening M008 if fixing call identity or projection appears to require:

- redesigning the Tool Broker globally;
- replacing the projection reducer/event-log architecture;
- converting all tools to durable jobs;
- changing scheduler ownership;
- introducing unrestricted cross-session/sibling communication;
- weakening M007’s direct-owner authorization;
- rewriting historical M001–M006 closure records.

The intended changes should remain local to TaskTool call identity, existing durable store conflict semantics as needed, projection adapters, focused tests, and closure governance.
