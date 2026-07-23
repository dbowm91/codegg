# ADR-0001: Programmatic Tool Execution Authority

Status: accepted

Date: 2026-07-23

Decision owners: project maintainers

Related specification sections:

- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#7-current-foundation-and-required-evolution`
- `plans/000-long-term-specification.md#16-durable-multilevel-agent-run-hierarchy`
- `plans/000-long-term-specification.md#17-job-scheduling-and-execution-backends`
- `plans/001-terminology-and-domain-model.md` — execution context, job, attempt, run, artifact, agent run

Affected subsystem roadmaps:

- `plans/subsystems/tool-programs-roadmap.md`

External design input:

- OpenAI, “Programmatic tool calling,” `https://developers.openai.com/api/docs/guides/tools-programmatic-tool-calling`

## Context

CodeGG agents currently issue tools one model turn at a time. This is appropriate when the next action requires semantic judgment, user approval, or a mutation decision, but it is inefficient for predictable loops such as reading several files, filtering structured results, running a bounded build/test matrix, or polling a daemon-owned job to completion.

CodeGG already has most of the required foundations:

- a daemon-owned durable scheduler with jobs, attempts, deadlines, retries, cancellation, heartbeat, resource admission, and restart recovery;
- a model-facing tool registry and permission system;
- structured tool provenance and artifact storage seams;
- a Python scripting subsystem with AST-oriented risk analysis, capability profiles, sandboxing, snapshots, and timeouts;
- test, managed-process, Git, research, and subagent execution subsystems;
- frontend-neutral job and run projections.

The missing boundary is a reliable way for an agent to submit a bounded program that can invoke approved CodeGG tools without requiring another model turn for every invocation. Implementing this as unrestricted Python, as a provider-specific OpenAI feature, or as a second scheduler would violate CodeGG’s ownership and recovery invariants.

## Decision drivers

- The singleton daemon and global scheduler must remain the sole authority for durable and heavy work.
- Program execution must work with OpenAI-compatible providers that do not implement OpenAI Responses program items.
- Python should be a first-class authoring syntax without granting arbitrary interpreter, filesystem, network, or subprocess authority.
- Every nested call must pass through normal tool authorization, validation, provenance, and artifact boundaries.
- Programs must have finite budgets, deterministic cancellation, stall detection, and restart-safe recovery.
- Intermediate outputs should remain outside the parent model transcript unless explicitly promoted.
- Hosted provider implementations may optimize execution but must not create a separate policy or storage model.

## Considered options

### Option A — Unrestricted Python with injected tool functions

Execute user- or model-authored CPython and expose asynchronous tool functions through an injected module.

Benefits:

- minimal syntax translation;
- familiar Python control flow;
- easy access to existing Python libraries.

Costs and failure modes:

- opaque interpreter state is difficult to checkpoint and replay;
- imports, reflection, threads, signals, native extensions, and exception suppression expand the attack surface;
- static call and loop bounds are not reliable;
- cancellation and restart can strand partially completed calls;
- sandbox correctness becomes the sole protection against arbitrary code.

Rejected for the orchestration runtime. General Python scripting remains a separate capability.

### Option B — OpenAI Responses programmatic calling only

Expose the provider’s hosted JavaScript program item and execute nested client-owned calls as requested.

Benefits:

- direct alignment with OpenAI’s API;
- provider may reduce model round trips and host program state.

Costs and failure modes:

- unavailable to Eggpool/OpenAI-compatible chat-completions providers and other vendors;
- hosted continuation and fingerprint semantics become the internal architecture;
- difficult to provide identical behavior through ACP, local testing, or offline providers;
- provider-specific policy may diverge from native tool execution.

Rejected as the canonical architecture. Retained as an optional adapter.

### Option C — Provider-neutral Tool Program jobs with restricted Python compiled to CodeGG IR

The model submits a bounded Python-like program through a normal tool call. CodeGG parses and validates it, freezes a capability manifest, compiles it to a deterministic internal representation, and executes it as a scheduler-owned durable job. Nested calls use one canonical Tool Broker. Hosted provider program items normalize into the same domain model.

Benefits:

- provider portability;
- static rejection of unbounded or unsafe constructs;
- scheduler-owned budgets, cancellation, resource admission, heartbeat, and restart recovery;
- deterministic call ledger and replay;
- one policy boundary for direct, native-program, and hosted-program calls;
- compact structured final results and artifact-backed intermediate data.

Costs:

- requires a parser, IR, interpreter, call ledger, and new tool metadata;
- Python support is intentionally a restricted language subset;
- tool output schemas must be migrated incrementally.

Selected.

## Decision

CodeGG will implement a provider-neutral **Tool Program** subsystem with these properties:

1. A Tool Program is a durable scheduler job with a typed program identity, immutable source and capability manifest, bounded execution budgets, attempts, checkpoints, and terminal status.
2. Restricted Python is the initial authoring language. Source is parsed and compiled to a CodeGG-owned IR before admission. The runtime does not execute arbitrary CPython bytecode.
3. Version 1 rejects imports, filesystem/network/subprocess access, reflection, dynamic evaluation, recursion, user-defined functions, unbounded `while` loops, and any construct whose execution cannot be bounded or metered.
4. Every nested invocation goes through a canonical Tool Broker shared with ordinary agent tool execution.
5. Tool metadata includes input schema, output schema, caller policy, side-effect/idempotency class, implementation version, and retry policy. A tool is not programmatically callable until these contracts are explicit.
6. The program capability manifest is resolved and content-hashed before execution. A running program cannot dynamically acquire additional tools.
7. The program call ledger persists normalized arguments, result identity, artifacts, status, and replay information. Completed calls are not repeated after restart.
8. Automatic replay is limited to read-only and safe-repeat calls. Approval-sensitive, non-idempotent, destructive, and general mutation tools remain direct-only unless a later accepted ADR changes that boundary.
9. Build, test, lint, format, and similar heavy calls are represented as child scheduler jobs rather than inline process execution.
10. Intermediate output is artifact-backed and bounded. The parent model receives a structured terminal or incomplete result, selected evidence, and expansion handles.
11. OpenAI Responses programmatic tool calling may be implemented as an optional backend adapter. Hosted program items, caller lineage, nested calls, fingerprints, and program output normalize into the same Tool Program and Tool Broker contracts.
12. No provider adapter may bypass daemon authorization, tool policy, scheduler admission, cancellation, RunStore/artifact retention, or audit/projection boundaries.

## Consequences

### Positive

- Agents can express deterministic multi-call workflows with fewer model turns and less context growth.
- Program execution remains testable with Eggpool, ACP/headless harnesses, scripted providers, and offline fixtures.
- A stalled, cancelled, restarted, or partially failed program has one durable owner and a recoverable state.
- Tool output schemas become reusable across normal calls, programs, providers, projections, and evaluations.
- OpenAI-specific capabilities can be adopted without making them architectural dependencies.

### Negative

- Restricted Python is not general Python and requires clear diagnostics when source uses unsupported syntax.
- The Tool trait and registry require a staged compatibility migration.
- The call ledger and checkpoint data add storage and retention obligations.
- Hosted and native backends require equivalence testing rather than assuming identical semantics.

### Neutral or deferred

- General Python analysis, transform, and verify modes remain available through the existing sandboxed Python subsystem.
- Programmatic file mutation, patch application, Git mutation, shell, and subagent spawning are deferred.
- Local JavaScript execution is not required.
- A later ADR may broaden callable effects only after approval, fencing, and exactly-once semantics are proven.

## Compatibility and migration

- Existing `Tool::execute` implementations remain supported through adapters while structured metadata is added.
- Existing model-facing tool names and direct-call behavior remain stable unless a dedicated migration plan says otherwise.
- Existing Python scripting calls migrate to scheduler ownership before Tool Programs depend on them.
- Job, protocol, and storage additions must be versioned and backward compatible with older persisted records.
- Providers that do not support hosted programs use the native restricted-Python backend through an ordinary function tool.
- Providers that support hosted programs must negotiate capability explicitly and fall back safely when unsupported.

## Security and reliability implications

- A program receives only the intersection of principal, project, session, parent agent, tool, workspace, node, and program-manifest authority.
- Tool permission and path validation occurs for every nested call, including cache hits and replay.
- Source size, AST size, IR steps, loop iterations, tool-call count, per-tool call count, parallelism, in-flight child jobs, result bytes, intermediate bytes, per-call timeout, stall timeout, wall timeout, and transient retries are bounded.
- Scheduler cancellation propagates to the interpreter, active broker calls, child jobs, and process groups.
- Heartbeats advance on IR progress and child-job progress. Lack of progress within the stall budget produces a terminal structured failure.
- Completed call records are replayed, not re-executed. Replay divergence fails recoverably.
- Secrets and raw credentials never enter program source, capability manifests, projections, cache keys, or call-ledger arguments.

## Verification

Implementations conform only when tests prove:

- direct and brokered tool calls enforce the same authorization and schema rules;
- accepted programs are statically bounded or stopped by runtime budgets;
- cancellation and stall watchdogs terminate all owned work;
- restart replay never repeats a completed call;
- unsafe tools cannot be made program-callable through provider payloads or registry mistakes;
- native and hosted adapters produce equivalent normalized results for shared fixtures;
- at least 10 percent injected transient nested-call failures yield terminal or explicitly recoverable outcomes without indefinite blocking;
- intermediate output remains artifact-backed and absent from the parent transcript unless promoted.

## Supersession

None.
