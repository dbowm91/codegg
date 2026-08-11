# Agent Runtime Correctness, Autonomy, and Simplification Roadmap

Status: active

Repository baseline reviewed: `e88d6f4f67ff729c894b228ddb8b5324582f3fbc`

Long-term references:

- `plans/000-long-term-specification.md`
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md`
- `plans/003-planning-process.md`
- `architecture/agent.md`
- `architecture/core.md`
- `architecture/tool.md`
- `architecture/testing.md`
- `architecture/cache-aware-context.md`

Related ADRs:

- `plans/adrs/ADR-0001-programmatic-tool-execution-authority.md`

No new ADR is required for the work as currently scoped. The existing single-daemon, explicit-workspace, normal-tool-authorization, scheduler-authority, and provider-adapter decisions remain authoritative. If implementation discovers that a public protocol, persistent schema, executable topology, or authority model must change, stop and create or amend an ADR before proceeding.

## 1. Purpose and ownership boundary

This workstream addresses the correctness, security, autonomous-execution, prompt/harness complexity, dependency-footprint, and routine-verification findings identified in the August 2026 CodeGG repository audit.

It owns:

- permission handling for raw and managed MCP tools, including removal of blanket external-tool auto-approval;
- truthful execution-decision/provenance metadata and MCP tool-surface cache invalidation;
- model-output repair when a provider/model emits textual representations of tool calls rather than structured calls;
- explicit workspace ownership throughout `AgentLoop` construction, snapshots, local mutation checks, and related runtime helpers;
- separation of current-turn input from session-origin input for routing, research triggering, recovery, and autonomous continuation;
- per-turn versus cumulative goal/accounting counters and unambiguous terminal event ownership;
- simplification of overlapping narration/tool-call/bootstrap/continuation recovery mechanisms into one bounded repair state machine;
- consolidation of stable prompt compilation and startup control policy so the same behavioral contracts are not injected by parallel mechanisms;
- deletion of transitional construction/factory layers where they create invalid intermediate states without a distinct architectural boundary;
- measured binary-footprint and upstream-dependency review, including optional plugin-runtime/Wasmtime patch-level verification;
- contraction of routine CI/static guards where checks duplicate generated-source verification, compiler-enforced boundaries, or adjacent tests;
- final architecture/documentation reconciliation and closure evidence.

It consumes, but does not redefine:

- the singleton daemon and scheduler as execution owners;
- `ExecutionContext` and workspace identity as authoritative turn-scoped execution identity;
- the canonical Tool Broker and ADR-0001 nested-call authorization boundary;
- runtime-asset snapshot immutability;
- provider/model adaptation as the location for provider-specific quirks;
- manual release cadence and the existing one-job routine CI posture;
- the single-binary product topology.

The governing rule is:

> Correct authority and workspace ownership first. Then simplify the harness by deleting overlapping repair and prompt machinery. Measure footprint and verification changes; do not trade supported behavior for nominal reduction.

## 2. Work classification

### Invariants

- An external/MCP tool never receives more authority because it is external or because it omits a local path.
- `Ask` remains `Ask` unless an explicit, attributable decision or policy changes it to `Allow`.
- Tool execution provenance reports real accepted decision state or explicitly reports that no persisted/typed receipt exists; it does not synthesize authoritative-looking revisions.
- Text emitted by a model is not executable merely because it contains JSON/XML/fenced text resembling a tool call.
- Textual tool-call compatibility is explicit per model/provider adapter and is bounded by one repair contract.
- Every production agent turn is constructed with explicit workspace identity. Process-global current working directory is not an authoritative fallback.
- Snapshots, permission path checks, subprocess/tool context, and mutation classification use the same turn workspace root.
- Current-turn routing and recovery operate on the current user turn, not the oldest user message in session history.
- Cumulative hard limits and per-turn accounting are distinct counters.
- One runtime layer owns terminal `AgentFinished` publication for a turn.
- MCP tool-definition cache identity changes when tool identity or schema changes, not merely when tool count changes.
- Autonomous recovery is bounded, observable, deterministic enough to test, and cannot create an unbounded cascade of synthetic/provider turns.
- Prompt/control contracts have one authoritative startup composition path.
- Routine CI remains small and release cadence remains manual.
- Supported user-facing features remain intact during footprint reduction.

### Capabilities

- External tools preserve the same approval semantics as equivalent native effects.
- Tool-fragile/local models may still use explicitly supported textual-tool repair without making prose generally executable.
- Multiple projects may execute concurrently through one daemon without snapshot/path ownership leaking through process CWD.
- Long-horizon goals account accurately across continuation turns.
- Models that narrate instead of acting receive one understandable, bounded correction rather than several overlapping nudges.
- Current model/profile behavior remains adaptable without hard-coding provider quirks into the generic turn engine.

### Infrastructure and polish

- `AgentLoop` construction may be consolidated around one typed build input.
- Recovery may consume a typed tool outcome/status instead of inferring failure solely from rendered strings.
- Stable prompt blocks may be reduced when the provider schema or another prompt block already carries the same capability information.
- Custom static guards may be deleted or narrowed when Rust types, crate ownership, code generation checks, or focused tests provide stronger signal.

## 3. Explicit non-goals

This roadmap must not:

- redesign the daemon, scheduler, ACP, Tool Program IR, plugin architecture, project catalog, or session projection system;
- add a second permission model for MCP;
- disable MCP, plugins, research, LSP, goals, subagents, context projection, or textual-tool compatibility for models that demonstrably require it;
- turn all external tools into blanket `Ask` if an explicit trusted/read-only classification already exists and can be proven;
- move arbitrary policy into model-name substring heuristics;
- add a generalized workflow engine or planner in place of the agent loop;
- require a provider call after every recovery transition;
- add new long-lived compatibility layers while deleting old ones;
- split daemon and TUI binaries solely for size reduction;
- replace RustPython, Comrak, Syntect, SQLx, Reqwest, or Wasmtime without measured benefit and compatibility evidence;
- add automatic dependency bots, cargo-audit on every PR, binary-size gates, coverage gates, benchmark gates, matrices, scheduled workflows, artifact publication, or release automation;
- reopen the previously closed post-audit or agent-runtime roadmaps except to link historical context;
- require repeated full-workspace validation after each milestone when focused tests plus `scripts/verify.sh quick` are sufficient.

## 4. Current-state summary

At baseline `e88d6f4f`:

- `src/agent/loop.rs` auto-allows an `Ask` path for `mcp__*` tools when the local workspace/path conditions pass. An MCP tool may mutate remote state, so local path containment is not a valid authority proof.
- `PermissionChecker` classifies unknown tools as mutating, so the loop-level MCP auto-allow can override the intended default `Ask` result.
- `build_tool_execution_context()` constructs decision/provenance fields from session/workspace strings and labels the result `allowed` rather than carrying the actual permission decision as a typed receipt.
- MCP tool-definition caching explicitly uses tool count as a proxy for MCP changes; replacing one tool/schema with another at equal count can therefore leave stale model-facing schemas.
- the generic agent loop invokes `parse_text_as_tool_calls()` whenever structured calls are absent. The parser accepts `invoke(...)`, fenced blocks, XML, and raw JSON embedded in prose, so explanatory text can become executable.
- `AgentLoop::new()` constructs `SnapshotManager` from `std::env::current_dir()` before `runtime_factory::build_agent_loop()` later applies `set_workspace_root()`. The setter does not rebuild the manager.
- `AgentLoopFactory` is explicitly transitional and repacks `AgentLoopBuildInput` into a many-argument legacy factory, preserving partially initialized loop construction.
- routing/research/repository-task heuristics in the loop derive an `original_prompt` by scanning forward for the first user message, while `turn_runtime.rs` already contains logic for the latest user question.
- goal accounting reads cumulative `tool_call_count`, while autonomous continuation resets token deltas but not the cumulative tool-call count, risking repeated charging of earlier calls.
- `AgentLoop` and `TurnRuntime` both publish `AgentFinished`, with differing stop-reason/usage fidelity.
- the no-tool-call branch contains several overlapping mechanisms: textual parsing, recovery-controller narration handling, synthetic `list .` bootstrap calls, post-tool continuation, generic continue instructions, narration retries, and missing-tool-call retries.
- prompt compilation includes harness, planning, role, model-profile, capability, agent identity, textual tool/skill lists, and model identity, while startup profile policy separately injects tool-use, patch, and todo contracts into the message list.
- historical footprint work already narrowed several dependency feature sets but produced only small binary reductions relative to the roughly 54 MiB single release binary; further work should measure whole contributors rather than repeat blanket feature trimming.
- routine CI is already one bounded job, but generated-agent synchronization is checked by both the generator's `--check` mode and a second handwritten parser, and some static guards may duplicate stronger direct boundaries.

## 5. Target architecture

### 5.1 Tool authority and provenance

The generic execution path should be:

```text
resolved tool surface
    -> permission/effect evaluation
    -> typed decision receipt
    -> canonical broker/executor
    -> typed outcome + provenance
```

External/MCP origin is metadata, not authority. Managed wrappers may have explicit trusted/read-only effect metadata, but unknown external tools must not be auto-approved solely by naming convention.

A decision receipt should contain only state that was actually evaluated: decision outcome, scope/path policy identity where applicable, principal/caller identity, effect class, issuance identity/time, and optional persistence/revision metadata if such metadata truly exists. Missing state remains missing rather than synthesized.

MCP tool-definition caching uses a service/catalog revision or stable hash over the model-facing tool identities and schemas.

### 5.2 Tool-call compatibility and repair

Structured provider tool calls are canonical. Textual tool-call parsing is a compatibility adapter for explicitly classified model/provider profiles.

The generic loop should receive normalized outcomes such as:

- structured calls;
- adapter-repaired calls;
- final text;
- malformed tool protocol;
- truncated/provider failure.

Textual repair must not scan arbitrary final prose under a broad raw-JSON grammar. It should require an adapter/profile capability and one bounded parse/repair attempt with schema validation against the resolved tool surface.

### 5.3 Workspace-bound construction

A production `AgentLoop` should not exist without explicit execution identity. The preferred construction input is the existing typed build input or `ExecutionContext`, not setters applied after construction.

Snapshot manager, path checks, tool execution context, mutation classification, subprocess CWD, and related helpers derive from the same workspace root. Legacy standalone/test fixtures may use explicit fixture roots, not process-global CWD as production authority.

### 5.4 Turn state and lifecycle

Distinguish:

- session-origin user goal;
- current-turn user input;
- cumulative execution limits;
- per-provider-turn accounting deltas;
- current autonomous-goal continuation state.

One layer owns `AgentFinished`; daemon-facing `TurnCompleted`/`TurnFailed` remain a separate projection/lifecycle event if needed.

### 5.5 Recovery/autonomy state machine

Replace the overlapping no-tool-call branch with one explicit bounded state machine. A representative shape is:

```text
ProviderOutcome
  -> tool calls: execute
  -> final answer with no pending work: finish
  -> malformed/fragile tool protocol: adapter repair once
  -> unfinished explicit task/goal with soft stop: one continuation/replan action
  -> repeated no-progress/failure: typed stalled outcome
```

Do not synthesize repository inspection merely because natural-language heuristics suggest more work. If bootstrap behavior remains for a specific weak model family, it belongs in a profile/adapter policy with explicit bounds and tests.

Recovery should prefer typed tool status (`success`, `denied`, `timeout`, `cancelled`, `tool_error`, `protocol_error`) over parsing rendered output strings. Human/model text remains a presentation surface.

### 5.6 Prompt/control composition

One startup compiler owns stable behavior contracts. It should assemble only information that changes model behavior and is not already represented authoritatively elsewhere.

Provider tool schemas remain authoritative for actual tool availability. Plan-mode text must derive from the resolved tool surface rather than a separately hard-coded list. Startup profile deltas become prompt blocks or equivalent compiler inputs instead of a second mutation pass over provider messages.

Dynamic recovery, steering, todo reminders, and runtime notifications remain late control messages because they are genuinely turn-volatile.

### 5.7 Footprint and verification posture

Binary work begins from fresh current measurements (`cargo bloat`, release size, feature tree) and tests only measured candidates. Possible candidates include notification support, syntax/highlighting assets, archive/install support, optional plugin runtime, and release optimization settings, but no candidate is presumed removable.

Routine CI remains one job. Keep only distinct generated-source, security/authority, formatting, lint, and test signal. Manual/full verification remains change-specific and release cadence remains manual.

## 6. Dependency graph

```text
M001 MCP authority/provenance/tool-surface -----------+
M002 textual tool-call repair safety ----------------+
M003 workspace-bound loop construction --------------+
M004 turn identity/accounting/lifecycle -------------+----+
                                                         |
M001 + M002 + M004 ------------------------------------> M005 recovery/autonomy simplification
M005 --------------------------------------------------> M006 prompt/control consolidation

M007 binary/dependency measurement -------------------+
M008 CI/static-guard contraction ---------------------+----> M009 integration/documentation/closure
M003 + M006 ------------------------------------------+
M001 + M002 + M004 + M005 ----------------------------+
```

Dependency classes:

- M001, M002, M003, M004, M007, and M008 have no hard dependency on one another and are dependency-ready against the reviewed baseline, subject to rebasing on current `main` before implementation.
- M005 has hard dependencies on M001, M002, and M004 because the recovery state machine must be built on correct authority, tool-call normalization, and turn/lifecycle semantics. M003 is a soft dependency because construction cleanup reduces incidental state but does not define recovery semantics.
- M006 has a hard dependency on M005 because recovery/control instructions must be stabilized before startup prompt contracts are consolidated.
- M007 has a soft dependency on M006 for final measurement because deleting prompt/control code can slightly affect release size, but the dependency/upstream review itself may proceed earlier.
- M008 is independently executable; it should avoid editing production behavior owned by M001-M007.
- M009 has hard dependencies on M001-M008 and is the only milestone that may close this workstream.

## 7. Ordered milestones

### M001 — MCP authority, provenance, and tool-surface cache correctness

Status: closed

Plan: `plans/implementation/agent-runtime-correctness-autonomy-simplification/001-mcp-authority-provenance-and-tool-surface-correctness.md`

Remove blanket MCP `Ask` auto-approval, preserve explicit trusted/read-only classifications where proven, carry truthful permission decision receipts into execution provenance, and invalidate MCP tool-definition caches on identity/schema changes rather than count only.

### M002 — Textual tool-call repair safety

Status: closed

Plan: `plans/implementation/agent-runtime-correctness-autonomy-simplification/002-textual-tool-call-repair-safety.md`

Move textual-tool compatibility behind explicit model/provider adapter capability, constrain accepted grammars, validate repaired calls against the current tool surface, and eliminate generic arbitrary-prose-to-execution behavior.

### M003 — Workspace-bound AgentLoop construction and snapshot ownership

Status: closed; see `plans/closure/agent-runtime-correctness-autonomy-simplification/003-status.md`.

Plan: `plans/implementation/agent-runtime-correctness-autonomy-simplification/003-workspace-bound-agent-loop-construction.md`

Require explicit execution/workspace identity during production loop construction, build snapshots from that identity, make mutation/path helpers workspace-relative, and collapse the transitional factory/setter chain where it creates invalid intermediate state. Closed at `8c2638db`.

### M004 — Current-turn identity, goal accounting, and terminal lifecycle correctness

Status: closed; see `plans/closure/agent-runtime-correctness-autonomy-simplification/004-status.md`.

Plan: `plans/implementation/agent-runtime-correctness-autonomy-simplification/004-turn-identity-accounting-and-lifecycle-correctness.md`

Use current-turn input for routing/research/recovery heuristics, separate cumulative limits from per-turn goal accounting, and establish one owner for `AgentFinished` publication.

### M005 — Agent-loop recovery and autonomous-execution state-machine simplification

Status: ready; M001, M002, and M004 are closed (M003 remains a soft dependency).

Plan: `plans/implementation/agent-runtime-correctness-autonomy-simplification/005-agent-loop-recovery-and-autonomy-state-machine.md`

Replace overlapping narration/bootstrap/continue/missing-tool repair paths with one bounded state machine, move model-specific bootstrap behavior out of the generic loop, and introduce typed tool outcome/status for recovery where it deletes string heuristics.

### M006 — Prompt compilation and control-policy consolidation

Status: blocked on M005

Plan: `plans/implementation/agent-runtime-correctness-autonomy-simplification/006-prompt-compilation-and-control-policy-consolidation.md`

Make prompt compilation the sole startup contract composition path, merge model-profile startup policies into compiler inputs, remove redundant/hard-coded capability text, and preserve only truly dynamic late control messages.

### M007 — Measured binary footprint and upstream dependency review

Status: ready

Plan: `plans/implementation/agent-runtime-correctness-autonomy-simplification/007-measured-binary-footprint-and-upstream-dependency-review.md`

Capture a fresh current release baseline, measure dominant contributors, verify Wasmtime/plugin-runtime patch safety, test a small set of no-feature-loss size candidates, and stop when measured benefit does not justify complexity.

### M008 — Routine CI and static-guard contraction

Status: ready

Plan: `plans/implementation/agent-runtime-correctness-autonomy-simplification/008-routine-ci-and-static-guard-contraction.md`

Remove duplicate builtin-agent verification, classify remaining custom guards by unique invariant, replace regex policy with focused Rust tests/types when stronger and cheaper, and retain the existing one-job/manual-release posture.

### M009 — Integration, documentation, and closure

Status: blocked on M001-M008

Plan: `plans/implementation/agent-runtime-correctness-autonomy-simplification/009-integration-documentation-and-closure.md`

Reconcile architecture documentation, run one minimal broad integration pass, capture final authority/autonomy/footprint/CI evidence, classify remaining findings, and create the workstream closure record.

## 8. Verification posture

Verification is intentionally minimal and change-specific.

For ordinary production milestones:

- run focused unit/integration tests for the exact changed invariant;
- run `scripts/verify.sh quick` once after the milestone is coherent;
- run additional feature-specific checks only when that milestone changes the feature.

Do not require a local full workspace test for every milestone. M009 owns one broad integration pass and one existing hosted `verify` run on the final merge candidate.

M007 may use `cargo bloat`, `cargo tree`, release builds, and targeted benchmarks manually. None becomes a CI gate.

M008 should delete checks rather than add replacement scripts when Rust construction/types or focused tests can make the invalid state impossible.

## 9. Security, compatibility, storage, protocol, migration, and observability

Security:

- M001 and M002 are security-sensitive because they determine when external/model-produced data becomes executable authority.
- M003 prevents cross-workspace authority leakage caused by process-global CWD.
- M005 must not recover from denial by silently broadening tool authority or restoring tools that the user/profile explicitly denied.

Compatibility:

- model-facing tool names and schemas remain stable unless a stale-schema defect requires a normal compatibility correction;
- models that require textual-tool repair retain that capability through explicit profile/adapter configuration;
- existing project/session/ACP interfaces remain stable;
- plugin/server/LSP/research/image and other supported features remain available.

Storage:

- no database schema migration is planned.
- permission persistence format should remain stable unless M001 proves that a typed receipt requires durable fields; prefer ephemeral per-call receipt plumbing over schema growth.

Protocol:

- no wire protocol change is expected.
- `AgentFinished` ownership cleanup is an internal event-lifecycle correction unless repository inspection proves external clients depend on duplicate events; preserve external projection compatibility while deleting duplicate internal publication.

Migration:

- mostly code deletion/construction changes; no user action expected.
- textual-tool adapter configuration should default from existing model-profile behavior so currently supported fragile models do not require manual migration.

Observability:

- recovery transitions should expose compact reason/action counters or tracing sufficient to diagnose repair decisions without logging raw secrets/tool outputs;
- permission decisions should expose real policy outcome/identity without fabricating metadata;
- no new telemetry service or persistence layer is required.

## 10. Exit conditions

The workstream is complete only when:

- unknown/remote MCP tools cannot bypass `Ask` through blanket name/path logic;
- external tool execution provenance is truthful and attributable;
- MCP schema/identity changes invalidate model-facing tool-definition caches;
- generic final prose cannot be converted into executable tool calls absent an explicit adapter contract;
- fragile-model textual-tool repair remains bounded and tested;
- production `AgentLoop`/snapshot construction is explicitly workspace-bound and does not consult process-global CWD for authority;
- current-turn heuristics use the current user turn;
- per-turn goal accounting no longer recharges cumulative tool-call history;
- one layer owns `AgentFinished`;
- overlapping recovery mechanisms are reduced to one bounded state machine with clear terminal/stall behavior;
- startup prompt/control policy has one authoritative compilation path;
- no supported feature is removed for footprint reduction, and final measurements record accepted/rejected candidates;
- Wasmtime/plugin-runtime lock state is checked against current applicable security fixes when plugins are supported;
- routine CI remains one bounded job and duplicate/static ceremonial checks are removed where safe;
- architecture/testing/tool documentation matches actual behavior;
- `scripts/verify.sh quick` passes on the final tree;
- one broad final workspace/feature check appropriate to the touched surfaces and one existing hosted `verify` run pass on the final merge candidate;
- `plans/closure/agent-runtime-correctness-autonomy-simplification/009-status.md` records requirement-to-evidence closure and any remaining low-severity/deferred work.

## 11. Deferred work

The following remain outside this roadmap unless implementation evidence elevates them:

- daemon/TUI executable split;
- broad provider HTTP-client unification;
- replacement of RustPython Parser;
- plugin-system redesign or capability marketplace changes;
- generalized effect systems beyond the metadata needed to fix MCP authorization;
- fully durable recovery-state persistence for ordinary interactive turns;
- autonomous multi-session goal scheduling beyond existing goal continuation;
- new provider-specific hosted-agent runtimes;
- automatic dependency audit/update services;
- new CI matrices, cross-platform release lanes, benchmark/coverage/size gates;
- release automation or fixed cadence;
- unrelated cleanup discovered while editing neighboring agent-loop modules.
