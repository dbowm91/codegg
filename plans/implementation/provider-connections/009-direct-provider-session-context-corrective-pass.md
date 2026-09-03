# Provider Connections Milestone 009 — Direct Provider Session-Context Closure Corrective Pass

Status: ready for handoff

Repository baseline: `3628434ef67b520fd3eeba65d75130d79e459d7f`

Source corrective roadmap:

- `plans/subsystems/provider-direct-call-session-context-corrective-addendum.md`

Historical milestones preserved by this pass:

- M007: `plans/implementation/provider-connections/007-independent-closure-ratification-and-governance-reconciliation.md`
- M007 closure: `plans/closure/provider-connections/007-status.md`
- M008: `plans/implementation/provider-connections/008-opencode-go-session-header-corrective-pass.md`
- M008 closure: `plans/closure/provider-connections/008-status.md`

Long-term requirements:

- `plans/000-long-term-specification.md#13-provider-architecture-and-eggpool`
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md#phase-2--eggpool-and-daemon-owned-provider-connections`
- `plans/003-planning-process.md`

Applicable implementation surfaces:

- `crates/codegg-providers/src/provider_core.rs`
- `crates/codegg-providers/src/openai_compatible.rs`
- `src/research/llm.rs`
- `src/research/coordinator.rs`
- `src/research/service.rs`
- `src/tool/backend.rs`
- `src/tool/review.rs`
- `src/tool/commit.rs`
- `src/agent/compaction.rs`
- direct `Provider::stream()` call sites discovered during implementation

Primary class: corrective provider compatibility / direct-call request context

## 1. Objective

Close the production-path gap left after Provider M008 by ensuring every direct provider invocation that can legitimately select OpenCode Go supplies one stable request-context identity for the logical operation before `Provider::stream()` is called.

M008 correctly established:

- typed `ProviderRequestContext`;
- provider-owned OpenCode Go `x-opencode-session` policy;
- local failure when required session context is absent;
- stable session propagation through normal `AgentLoop` requests;
- static `extra_headers` wire emission and collision protection.

The remaining defect is outside the normal `AgentLoop` provider-turn path. Several production helpers construct `ChatRequest { context: Default::default() }` and call `provider.stream()` directly. If the selected provider is OpenCode Go, those helpers now fail locally with `missing_session_context`. Some paths then silently degrade to deterministic behavior, which can conceal the provider-compatibility failure.

M009 must correct those owning call sites without weakening the M008 transport contract, adding a free-form header map, or moving session generation into the provider.

## 2. Corrective findings

### 2.1 Model-backed research loses its stable run identity

`ResearchCoordinator` owns a stable `ResearchRequest.id` / `run_id` for the complete research operation and accepts an arbitrary `Arc<dyn Provider>` for model-backed phases.

The model-backed phases call helpers in `src/research/llm.rs`. Those helpers currently construct `ChatRequest` with:

```rust
context: Default::default(),
```

and call:

```rust
provider.stream(&request)
```

directly.

Therefore a research run configured with OpenCode Go cannot reach the network. Evidence extraction, claim construction, and semantic verification receive `missing_session_context`; several of those callers intentionally fall back to deterministic behavior on LLM failure, so the incompatibility may appear only as reduced model participation rather than a clear provider error.

This is not a reason to make research a daemon `SessionId`. The research run already owns the logical-operation identity required for upstream affinity. M009 should project that stable run identity into `ProviderRequestContext` for all LLM calls belonging to the same run.

### 2.2 Agent-invoked review/commit tools discard an existing canonical session

The tool execution pipeline already constructs `ToolExecutionContext` with:

```text
session_id = AgentLoop.session_id
turn_id
invocation_key
agent_id
provider_name
...
```

`ReviewTool` and `CommitTool` perform nested LLM requests by constructing their own `ChatRequest` and calling a provider directly. Both currently use empty provider request context.

Both tools implement only the legacy `Tool::execute()` entry point for the relevant behavior. The trait's default `execute_structured()` delegates to `execute()` and discards the supplied `ToolExecutionContext`.

Thus an agent turn can have the correct CodeGG session identity at the tool boundary and still lose it before the tool's nested provider request.

M009 must consume the already-authoritative `ToolExecutionContext.session_id` when these tools are invoked from an agent session.

### 2.3 Async LLM compaction has the same direct-call shape

`src/agent/compaction.rs::llm_summarize()` also constructs a direct `ChatRequest` with empty context and calls `provider.stream()`.

The implementation must determine whether the async LLM summarization path is reachable in current production execution. If it is conversation-bound, propagate the owning conversation identity into the compaction request. If it is unused/dead in production, record that evidence and avoid widening the patch merely to refactor dormant code.

Do not classify a reachable OpenCode-capable path as safely "standalone" merely because its current request context is empty.

### 2.4 M008 closure history remains immutable

`plans/closure/provider-connections/008-status.md` states that compaction, review, commit, and research were intentionally standalone/default-context callers. Later production-path review showed that this classification was incomplete for provider compatibility.

Do not rewrite M008 closure to conceal that accepted history. M009 owns the later-discovered direct-call gap and becomes the current strict disposition for this narrow scope after closure.

## 3. Invariants

### 3.1 M008 transport invariants remain authoritative

- `OpenAiCompatibleProvider` must continue to require request context for OpenCode Go.
- Do not generate a fallback identity inside `OpenAiCompatibleProvider::stream()` or `request_builder()`.
- Do not change `missing_session_context` into a retryable network-style error.
- Do not add a free-form per-request header map to `ChatRequest`.
- Do not forward arbitrary inbound/client headers upstream.
- Do not log raw session/run/invocation header values.
- Non-OpenCode providers may continue to ignore `ProviderRequestContext.session_id`.
- `extra_headers` behavior and reserved-header collision semantics from M008 must not regress.

### 3.2 Identity ownership

Use the narrowest existing stable identity owned by the logical operation:

```text
normal agent conversation -> canonical CodeGG session ID
agent-invoked nested tool -> ToolExecutionContext.session_id
research model phases     -> one research-run-scoped affinity identity
standalone one-shot tool  -> one invocation-scoped identity established once by the owning tool invocation
```

Never substitute:

- a fresh random value per provider request;
- provider ID/model ID;
- daemon-global ID;
- current timestamp;
- a different value for each research phase in one run;
- tool-call ID when an enclosing CodeGG session ID already exists.

An invocation-scoped generated ID is acceptable only when there is genuinely no enclosing canonical session and the production surface intentionally supports a provider requiring affinity. It must be generated once at the owning invocation boundary and reused for every provider call in that invocation.

### 3.3 No identity-system redesign

`ProviderRequestContext.session_id` remains a transport projection. M009 must not introduce a second durable session table, protocol-visible provider session identity, provider-side global mutable state, or database migration.

Research run IDs remain research identities. Tool execution context remains tool invocation/session context. They are projected into provider affinity metadata only for the duration of the applicable provider calls.

## 4. Required production changes

### 4.1 Make research LLM helpers context-aware

Change the research LLM helper boundary so callers can supply a bounded `ProviderRequestContext` or equivalent stable affinity value.

Preferred shape:

```rust
pub async fn call_llm(
    provider: &dyn Provider,
    model: &str,
    context: ProviderRequestContext,
    ...
)
```

or a borrowed context if that avoids unnecessary clones.

`call_llm_json()` must pass the same context through unchanged.

Do not make `call_llm()` synthesize a new identity on every call.

At the research coordinator/run boundary, establish one stable affinity identity for the run and reuse it for:

- evidence extraction;
- claim construction;
- semantic verification;
- rerun phases belonging to the new research run.

Preferred source is the existing stable `ResearchRequest.id` / `run_id` after applying the existing bounded header/session-value validation expectations. If the raw run ID does not satisfy the provider request-context constraints, create a deterministic bounded projection once for the run; do not regenerate between phases.

The same run must produce the same header across all model-backed research calls. Different research runs must not share one constant value.

### 4.2 Preserve explicit deterministic fallback semantics

Research intentionally falls back to deterministic extraction/claims in some model-error cases. Preserve those product semantics, but make provider incompatibility observable in focused tests/logging.

Required behavior:

- a correctly context-populated OpenCode Go request must not hit the deterministic fallback solely because `x-opencode-session` was missing;
- existing fallback-on-provider-error behavior may remain;
- ordinary logs may identify the provider error class/code, but must not log the raw affinity value;
- do not special-case OpenCode Go in research logic. Research supplies request context; the provider decides whether it needs it.

### 4.3 Thread agent session context into ReviewTool

Override or refactor `ReviewTool` execution so the structured tool path can consume `ToolExecutionContext`.

Preferred shape:

```text
Tool::execute_structured(input, Some(ctx))
    -> review implementation
    -> nested ChatRequest.context.session_id = ctx.session_id
```

Keep the public tool behavior and result formatting unchanged.

For the legacy/direct `execute()` path with no `ToolExecutionContext`:

- if the selected provider does not require session affinity, current behavior may remain;
- if the production path supports OpenCode Go, establish one invocation-scoped stable identity once before the nested provider request;
- do not generate the ID inside provider transport;
- do not create a new identity per retry or per streamed chunk.

If `ReviewTool` performs only one provider call today, still establish identity at the invocation boundary so future retries/multi-call evolution cannot accidentally become per-request.

### 4.4 Thread agent session context into CommitTool

Apply the same ownership rule to commit-message generation:

- agent/session invocation uses `ToolExecutionContext.session_id`;
- direct standalone invocation uses one invocation-scoped identity if required;
- the nested provider request receives typed request context;
- commit mutation, staging, diff ownership, permission semantics, and generated-message behavior remain unchanged.

Do not broaden M009 into a generic tool-context rewrite. Only change the minimum tool methods/helper signatures required to preserve existing context into nested provider calls.

### 4.5 Audit live async compaction

Trace every current call to:

- `compact_messages_async()`;
- `summarize_old_turns()`;
- `llm_summarize()`.

If the LLM summarization path is production-reachable:

- add an explicit provider request context parameter at the owning conversation boundary;
- propagate the same canonical session identity used by the parent agent request;
- ensure the summarization provider call receives that context.

If it is not production-reachable:

- do not invent a large API refactor;
- record the call-graph evidence in the M009 closure record;
- keep any test-only request context explicit.

If a standalone non-agent caller intentionally uses async LLM compaction with OpenCode Go, it must establish one invocation-scoped stable identity just like other one-shot production surfaces.

### 4.6 Audit all remaining direct `Provider::stream()` calls

Search production source for direct provider invocation patterns, including:

```text
provider.stream(
.stream(&request)
Provider::stream
```

Classify every production call site into one of:

1. normal `AgentLoop` path — already covered by M008;
2. nested operation with an existing canonical session — propagate it;
3. multi-call standalone operation with an existing stable run/job ID — project that ID once;
4. genuine one-shot production invocation — establish one invocation-scoped identity if the provider requires one;
5. provider that can never be OpenCode Go by construction — document why and leave context absent;
6. test/fixture only — explicit default context is acceptable.

Do not mechanically replace every `Default::default()` with a UUID. The objective is correct ownership, not header presence at any cost.

Any additional production gap discovered by this audit that is the same class of direct-call context loss is in M009 scope. If fixing it requires redesigning unrelated provider APIs or application identity architecture, stop and register a narrower follow-up instead.

## 5. Required tests

### 5.1 Research run affinity test

Add a provider fake/capture seam that can enforce required session context without live OpenCode access.

For one research run with multiple model-backed phases, assert:

- every direct provider request contains non-empty context;
- all requests for run R1 use the same affinity value;
- a separate run R2 uses a different value;
- the value is absent from request JSON/model-visible content unless independently present in the actual research prompt;
- model-backed phases no longer fall back solely because context is missing.

The test does not need to reproduce the entire network stack if the existing M008 provider wire tests already prove context -> header. It must prove research -> context propagation across multiple calls.

### 5.2 Structured ReviewTool context test

Invoke `ReviewTool` through `execute_structured()` with a `ToolExecutionContext` containing a known session ID and a capture/mock provider seam.

Assert the nested `ChatRequest` receives that exact session value.

If current tool construction makes provider injection impossible without a broad refactor, introduce the smallest test seam/helper extraction necessary; do not add a new provider registry abstraction solely for this test.

### 5.3 Structured CommitTool context test

Equivalent assertion for commit-message generation. The test should not create a real commit unless existing fixtures already make that trivial; isolate the nested LLM request helper where practical.

### 5.4 Standalone invocation stability

For any corrected standalone production path that generates an invocation-scoped identity, assert:

- it is created once per logical invocation;
- repeated/retried provider calls in that invocation reuse it;
- a second invocation receives a different value when uniqueness is required.

Do not require uniqueness assertions for paths that use an existing stable run/session ID.

### 5.5 Compaction evidence

If async LLM compaction is production-reachable, add one focused propagation test.

If it is not reachable, closure evidence must identify the production call graph and why no code change was required.

### 5.6 Regression tests

Retain the M008 provider tests that prove:

- same-session/different-session header behavior;
- missing-context zero-network failure;
- non-OpenCode isolation;
- `extra_headers` wire emission;
- reserved-header collision rejection;
- request body does not contain session metadata.

M009 must not weaken or delete those tests to make direct callers pass.

## 6. Verification posture

Keep verification minimal and attributable.

Required before closure:

```bash
cargo fmt --all -- --check
git diff --check
cargo test -p codegg-providers openai_compatible -- --test-threads=1
```

Run the narrow tests added for research/tool/compaction context propagation using the smallest package/test targets available.

Then run:

```bash
scripts/verify.sh quick
```

No new CI workflow, live OpenCode Go integration test, benchmark gate, coverage gate, scanner, dependency bot, or release automation is required.

A hosted CI run is not a closure prerequisite unless the implementation candidate already has an attributable hosted failure or `scripts/verify.sh quick` cannot exercise a required supported-platform condition.

## 7. Expected file touch set

Expected production files:

- `src/research/llm.rs`
- `src/research/coordinator.rs`
- research phase call sites needed to carry one run context
- `src/tool/review.rs`
- `src/tool/commit.rs`
- `src/agent/compaction.rs` only if the live-call audit requires it

Expected tests may live beside those modules or in existing integration-test files.

Expected planning/architecture updates:

- `architecture/provider.md` only if the direct-call ownership rule is not already clear;
- `architecture/research.md` if research provider context becomes part of its durable integration contract;
- `plans/closure/provider-connections/009-status.md` at closure;
- `plans/registry.md` status transition.

Do not modify M008 closure except by adding forward references if the planning process explicitly requires them; historical claims remain preserved in Git history and M009 records the later correction.

## 8. Acceptance criteria

M009 may close only when all of the following are true:

1. Model-backed research can use OpenCode Go without failing `missing_session_context`.
2. All model-backed calls in one research run reuse one stable run-scoped affinity identity.
3. Separate research runs do not collapse to one constant affinity identity.
4. Agent-invoked `ReviewTool` nested LLM requests receive the enclosing `ToolExecutionContext.session_id`.
5. Agent-invoked `CommitTool` nested LLM requests receive the enclosing `ToolExecutionContext.session_id`.
6. Direct/legacy tool invocation semantics are explicitly handled rather than accidentally relying on empty context.
7. Any production-reachable async LLM compaction request receives the correct owning context; otherwise non-reachability is documented with call-graph evidence.
8. Every remaining production direct `Provider::stream()` call is classified and no OpenCode-capable direct caller silently uses empty context without an ownership rationale.
9. No identity is generated inside OpenCode/provider transport.
10. No per-request random affinity behavior is introduced.
11. No arbitrary upstream-header passthrough is introduced.
12. M008 provider/header tests remain green.
13. Focused research/tool tests and `scripts/verify.sh quick` pass.
14. No critical/high/medium unresolved defect remains in M009 scope.
15. Closure evidence is recorded in a new `plans/closure/provider-connections/009-status.md`.

## 9. Closure evidence requirements

The M009 closure record must include:

- exact baseline and implementation commit(s);
- direct-provider call-site inventory with classification and disposition;
- research identity source and same-run/different-run evidence;
- ReviewTool structured-context evidence;
- CommitTool structured-context evidence;
- compaction reachability evidence and test/change disposition;
- statement that M008 transport behavior was preserved;
- focused test commands/results;
- `scripts/verify.sh quick` result;
- storage/protocol/migration statement;
- security/logging review;
- unresolved findings by severity;
- final recommendation: closed, conditionally closed, corrective pass required, or blocked.

## 10. Explicit non-goals

M009 does not include:

- rewriting the provider trait around a new universal request object;
- a generalized arbitrary HTTP-header API;
- inbound OpenCode-compatible proxy/header forwarding;
- changes to OpenCode Go authentication;
- Eggpool routing redesign;
- provider storage migrations;
- research architecture redesign beyond carrying stable provider request context;
- generic conversion of every tool to a new execution API;
- changing commit/review product behavior;
- live-provider CI;
- new release automation.

## 11. Stop conditions

Stop and report/register a narrower follow-up instead of broadening M009 if:

- research/provider context cannot be carried without a breaking public API that affects unrelated consumers;
- tool session context cannot reach nested provider calls without replacing the tool execution architecture;
- async compaction reveals a separate ownership problem materially larger than request-context propagation;
- direct-call audit finds a separate provider-compatibility defect not caused by missing request context;
- required fixes create a `codegg-core`/`codegg-providers` dependency cycle;
- existing unrelated verification failures prevent attribution after M009-focused tests pass.

Do not close M009 by reclassifying a failing OpenCode-capable production path as "standalone" while leaving `context: None`. A standalone path still needs one stable invocation identity when it uses a provider that requires affinity.