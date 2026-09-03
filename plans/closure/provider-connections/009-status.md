# Provider Connections Milestone 009 — Closure Status

Status: closed

Source implementation plan: `plans/implementation/provider-connections/009-direct-provider-session-context-corrective-pass.md`

Source corrective roadmap: `plans/subsystems/provider-direct-call-session-context-corrective-addendum.md`

Repository baseline reviewed: `3628434ef67b520fd3eeba65d75130d79e459d7f`

Implementation commits:

- `1a6f696f54b351449c807bfc91e53f3c5e6f0b72` — propagate provider request context through research, tools, and compaction, with architecture and planning updates.
- `54a05b6a66448cb110781e01ff5228702e229eb1` — exercise ReviewTool's structured execution path with a real staged temporary repository.

## 1. Executive finding

M009 is strictly closed. Every production direct `Provider::stream()` caller
that can use an OpenCode-compatible provider now receives an explicit context
owned by its enclosing logical operation. Research uses one stable run-scoped
projection, nested ReviewTool and CommitTool requests consume the existing
`ToolExecutionContext.session_id`, and reachable LLM compaction receives the
agent session identity. Direct legacy tool calls receive one invocation-scoped
identity when a provider request is made. The M008 provider transport contract
was preserved.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Model-backed research has provider context | `ResearchCoordinator::run` projects `ResearchRequest.id` before model phases | pass |
| Same research run keeps one affinity | `provider_context_for_run` is created once and cloned through extraction, claims, and semantic verification; stability test passes | pass |
| Different research runs remain distinct | same unit test asserts `run-1` and `run-2` differ | pass |
| Invalid research IDs remain bounded/deterministic | bounded SHA-256 projection test passes | pass |
| Structured ReviewTool preserves enclosing session | `execute_structured` forwards `ToolExecutionContext`; temporary staged-repository regression passes | pass |
| Structured CommitTool preserves enclosing session | `execute_structured` forwards `ToolExecutionContext`; nested provider capture test passes | pass |
| Legacy/standalone tool semantics are explicit | shared tool helper chooses supplied session or generates one invocation value before the provider call | pass |
| Reachable compaction has owning context | `AgentLoop` passes its session to `compact_with_policy`/`auto_compact_async`; LLM summarization and semantic checkpoint use it | pass |
| All production direct calls classified | inventory below covers research, tools, compaction, normal turn, and provider-internal forwarding | pass |
| Provider does not generate identity | no provider transport changes; context is consumed from `ChatRequest` | pass |
| No per-request random identity | research/agent contexts are created once per run/session; tool context is created once per invocation | pass |
| No arbitrary header passthrough | only the existing typed provider context is threaded; no header API was added | pass |
| M008 behavior preserved | retained OpenAI-compatible regression suite passes | pass |
| Verification and closure evidence complete | focused tests, quick verification, this record, roadmap, and registry updates | pass |

## 3. Direct-provider call-site inventory

| Call site | Production reachability | Identity source and disposition |
|---|---|---|
| `src/agent/provider_turn.rs` | Normal agent turn | Existing canonical `AgentLoop`/turn-runtime `ProviderRequestContext`; unchanged and preserved |
| `src/research/llm.rs` | Research evidence, claim, and semantic-verification phases | `ResearchCoordinator` projects one stable run identity once and passes clones to every model-backed phase |
| `src/tool/review.rs` | Agent-invoked and direct/legacy review | Structured path consumes `ToolExecutionContext.session_id`; legacy path gets one invocation-scoped value; provider selection remains unchanged |
| `src/tool/commit.rs` | Agent-invoked and direct/legacy commit message generation | Structured path consumes `ToolExecutionContext.session_id`; legacy path gets one invocation-scoped value; manual-message/mutation behavior unchanged |
| `src/agent/compaction.rs::llm_summarize` | Reachable from summarization compaction | Receives the agent's canonical session projection through `auto_compact_async` |
| `src/agent/compaction.rs::semantic_checkpoint` | Reachable from agent/hybrid compaction | Receives the same owning session projection through `compact_with_policy` |
| `crates/codegg-providers/src/fallback.rs` | Provider fallback/retry internals | Forwards the immutable request unchanged; it does not create or replace context |
| Test fixtures and mock providers | Test-only | Default contexts remain fixtures and are not production callers |

No remaining production OpenCode-capable direct caller silently relies on an
empty context without an ownership rationale.

## 4. Research identity evidence

`ResearchCoordinator::run` derives `provider_context_for_run(&request.id)` once
before model-backed phases. Valid bounded IDs are used directly; invalid or
oversized IDs receive a deterministic bounded SHA-256 projection. The same
`ProviderRequestContext` is passed through evidence extraction, claim
construction, and semantic verification. The unit tests
`research_context_is_stable_per_run_and_distinct_between_runs` and
`invalid_research_id_gets_bounded_deterministic_context` pass. No research
database schema, run identifier, or protocol type was changed.

## 5. ReviewTool and CommitTool evidence

Both tools now retain an optional provider override for the owning tool/provider
boundary, centralize request construction in context-aware helpers, and override
`execute_structured` so `ToolExecutionContext` is not discarded. The focused
provider-capture tests assert the exact enclosing session value. ReviewTool's
additional structured-path test runs the actual staged-diff flow in a temporary
Git repository and observes the same session at `Provider::stream()`.

## 6. Compaction reachability evidence

The live call graph is:

```text
AgentLoop compaction
  -> auto_compact_async / compact_with_policy
  -> compact_messages_async / semantic_checkpoint
  -> llm_summarize
  -> Provider::stream(ChatRequest { context })
```

Agent and hybrid paths now construct the context from `AgentLoop.session_id`
once per compaction operation and reuse it for fallback or semantic requests.
The compaction integration suite, including
`llm_compaction_uses_supplied_session_context`, passed all 65 tests.

## 7. M008 preservation and invariant review

M008 closure remains immutable. Its typed `ProviderRequestContext`, OpenCode Go
`x-opencode-session` policy, missing-context local failure, static-header
validation, reserved-header protection, request-body exclusion, retry/fallback
behavior, and non-OpenCode isolation were not weakened or rewritten. The
provider crate's retained OpenAI-compatible regression tests passed 8/8.

No identity is logged, persisted, placed in prompts/request bodies, or moved
into arbitrary headers. No provider-global mutable session state or random
per-request affinity behavior was introduced.

## 8. Verification executed

All results are local and use the repository's bounded test settings.

```text
cargo fmt --all -- --check                         passed
git diff --check                                   passed
cargo test -p codegg --lib research --locked       122 passed
cargo test --test compaction --locked              65 passed
cargo test -p codegg-providers openai_compatible   8 passed
ReviewTool focused context tests                    passed
CommitTool focused context test                    passed
scripts/verify.sh quick                             passed
```

The first unqualified provider-linked test invocation selected the host's
`/opt/local/lib/liblzma.dylib` for an x86_64 target and failed at link time.
Re-running with the available x86_64 pkg-config path
`PKG_CONFIG_PATH=/usr/local/lib/pkgconfig` passed; this was host library
selection, not a source or test failure. No live OpenCode Go request was made.

## 9. Storage, protocol, and migration review

No database schema, migration, provider-connection record, credential format,
public protocol DTO, or storage layout changed. The work only carries the
existing transport-only provider context through owning call paths.

## 10. Security and logging review

The implementation does not accept arbitrary upstream headers, expose provider
session identity to model content, or log raw identity values. Standalone
identity generation is bounded to the owning invocation helper; normal agent
and research identities come from existing logical-operation owners. Existing
provider header ownership and pre-network validation remain authoritative.

## 11. Unresolved findings

None at critical, high, medium, or low severity within M009 scope.

## 12. Roadmap, registry, and dependency disposition

- The implementation plan is marked `implemented` and this closure record is
  the accepted formal `closed` status.
- The provider direct-call corrective addendum is marked `closed`; the registry
  subsystem row is closed and M009 was removed from dependency-ready work.
- The dependency audit found no registered future provider plan with a hard or
  interface dependency on M009. Therefore no future plan was unblocked or had
  its status changed.
- Provider M008 remains a historical strict closure for its accepted transport
  scope; M009 is the current strict disposition for direct-call context
  compatibility.
- The unrelated supported-Linux Landlock condition remains the only recorded
  blocked/conditional item and is unaffected.

Final recommendation: **closed**.
