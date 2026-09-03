# Provider Connections — Direct Provider Session Context Corrective Addendum

Status: ready

Repository baseline reviewed: `3628434ef67b520fd3eeba65d75130d79e459d7f`

Parent provider planning and historical closure:

- `plans/subsystems/provider-connections-roadmap.md`
- `plans/subsystems/provider-opencode-session-affinity-corrective-addendum.md`
- Provider M007 closure: `plans/closure/provider-connections/007-status.md`
- Provider M008 closure: `plans/closure/provider-connections/008-status.md`

Long-term references:

- `plans/000-long-term-specification.md#13-provider-architecture-and-eggpool`
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md#phase-2--eggpool-and-daemon-owned-provider-connections`
- `plans/003-planning-process.md`

## 1. Corrective purpose

Provider M008 correctly repaired the OpenCode Go transport seam: `ProviderRequestContext` now carries bounded affinity metadata, OpenCode Go requires `x-opencode-session`, the normal `AgentLoop` path reprojects canonical session identity, missing required context fails locally, and static `extra_headers` are now emitted safely.

A later production-path audit found that M008's closure classification of several direct provider callers as standalone/default-context was incomplete. Model-backed research, nested review/commit LLM calls, and the async LLM compaction helper can bypass `AgentLoop` and invoke `Provider::stream()` with `ChatRequest.context == Default::default()`.

For OpenCode Go, that is not a harmless default: M008 intentionally converts missing context into a local `missing_session_context` failure. Model-backed research can then silently fall back to deterministic behavior, while review/commit can fail their nested LLM request even though the agent tool boundary already carries the canonical CodeGG session ID.

This addendum preserves M008 history and registers one narrow corrective milestone for direct-call context ownership. It does not reopen the M008 transport/header implementation itself.

## 2. Corrective findings

### Finding A — research run identity is not projected to provider context

`ResearchCoordinator` owns a stable `ResearchRequest.id` / `run_id` and uses one configured `Arc<dyn Provider>` across model-backed evidence extraction, claim generation, and semantic verification.

`src/research/llm.rs` constructs direct `ChatRequest` values with empty provider context and calls `provider.stream()` directly. An OpenCode Go-backed research run therefore fails before network send instead of carrying one stable affinity value across the run.

The research run already supplies the appropriate logical-operation boundary. M009 must reuse one stable run-scoped affinity identity across all model-backed calls in that run.

### Finding B — nested tool LLM calls discard available `ToolExecutionContext.session_id`

The agent tool pipeline constructs `ToolExecutionContext` with `session_id: Some(AgentLoop.session_id)`.

`ReviewTool` and `CommitTool` perform their own direct provider calls but implement only the legacy tool execution path for those operations. The default `execute_structured()` drops the supplied execution context before delegating to `execute()`.

M009 must preserve the existing tool/session identity into those nested provider requests rather than creating a parallel identity mechanism.

### Finding C — async LLM compaction requires a reachability disposition

`src/agent/compaction.rs::llm_summarize()` directly invokes the provider with empty request context. M009 must determine whether that path is production-reachable.

If reachable from a conversation, it must receive the conversation's stable provider request context. If dormant/test-only, closure evidence must record that call-graph result and avoid unnecessary architectural churn.

## 3. Corrective milestone

### M009 — Direct provider session-context closure corrective pass

Status: ready

Implementation plan:

- `plans/implementation/provider-connections/009-direct-provider-session-context-corrective-pass.md`

Class: corrective provider compatibility / direct-call request context

Dependencies:

- Provider M008 implementation and typed `ProviderRequestContext` on `main`;
- existing `ToolExecutionContext.session_id` for agent-invoked tools;
- existing stable research run identity;
- no hard external dependency.

Exit conditions:

- all model-backed calls in one research run use one stable run-scoped provider context;
- OpenCode Go research no longer fails solely because context is absent;
- ReviewTool and CommitTool consume enclosing agent session identity for nested provider calls when invoked through the structured tool path;
- direct/legacy tool invocation has an explicit one-shot affinity policy rather than accidental empty context when using a provider that requires it;
- production-reachable async LLM compaction receives correct owning context, or non-reachability is documented;
- every direct production `Provider::stream()` caller is inventoried and classified;
- no random-per-request provider-side identity, arbitrary header passthrough, or provider transport weakening is introduced;
- M008 transport and header regression tests remain green;
- focused direct-call tests plus `scripts/verify.sh quick` pass;
- closure evidence is recorded at `plans/closure/provider-connections/009-status.md`.

## 4. Governance and history

Provider M008 remains immutable historical closure for the transport implementation it accepted. M009 records the later-discovered incomplete production-path classification rather than editing M008 evidence retroactively.

Until M009 closes, Provider Connections should be shown as `ready`, with M009 as the current corrective milestone. M008 remains listed in recently closed control points as historical strict closure for its accepted scope, while M009 becomes the current strict disposition for direct-call request-context compatibility after implementation and closure.

No other provider roadmap milestone is unblocked or reopened by this registration.

## 5. Verification posture

Verification remains deliberately small:

- focused research/tool/direct-call request-context tests;
- retained M008 OpenAI-compatible header tests;
- `cargo fmt --all -- --check`;
- `git diff --check`;
- `scripts/verify.sh quick`.

No new hosted CI requirement, workflow lane, live OpenCode Go test, scanner, benchmark, coverage gate, release automation, or fixed release cadence is introduced.

## 6. Architecture disposition

No ADR is required for M009 unless implementation discovers that the existing provider request context cannot be threaded through direct callers without redesigning the public provider/tool/research APIs across unrelated subsystems.

The intended architecture remains:

```text
owning logical operation
  |-- agent session -> ProviderRequestContext(session_id)
  |-- nested tool   -> ToolExecutionContext.session_id -> ProviderRequestContext
  |-- research run  -> stable run-scoped affinity -> ProviderRequestContext
  `-- one-shot      -> one invocation-scoped affinity -> ProviderRequestContext

Provider::stream()
  -> provider-owned transport policy
  -> x-opencode-session only when required
```

The provider remains a consumer of stable context, never the owner or generator of conversation identity.