# Programmatic Tool Execution and Tool Programs Roadmap

Status: active — Milestone 006 closing

Long-term references:

- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#7-current-foundation-and-required-evolution`
- `plans/000-long-term-specification.md#16-durable-multilevel-agent-run-hierarchy`
- `plans/000-long-term-specification.md#17-job-scheduling-and-execution-backends`
- `plans/001-terminology-and-domain-model.md` — execution context, job, attempt, run, artifact, agent run
- `plans/002-long-term-roadmap.md#phase-9--durable-multilevel-agent-run-service`
- `plans/002-long-term-roadmap.md#phase-13--acp-adapter`

Related ADRs:

- `plans/adrs/ADR-0001-programmatic-tool-execution-authority.md`

External design input:

- OpenAI, “Programmatic tool calling,” `https://developers.openai.com/api/docs/guides/tools-programmatic-tool-calling`

## 1. Purpose and ownership boundary

This subsystem lets an agent submit a bounded program that invokes approved CodeGG tools through deterministic control flow without requiring a model turn for every tool call.

It owns:

- scheduler-owned Python script execution as the prerequisite execution boundary;
- structured tool metadata required for program composition;
- the canonical Tool Broker used by direct and programmatic calls;
- durable Tool Program identities, source records, capability manifests, checkpoints, and call ledgers;
- restricted-Python parsing, static validation, IR compilation, and interpretation;
- program budgets, heartbeat, stall detection, cancellation, retry, replay, and recovery;
- program-callable tool activation and structured output schemas;
- child-job composition for build, test, lint, format, and other safe scheduler-owned operations;
- foreground and background program submission, progress projection, and parent notification;
- optional OpenAI Responses hosted-program integration normalized to the same domain model;
- headless/ACP-oriented evaluation, Eggpool model validation, chaos testing, and performance closure.

It consumes:

- stable workspace identity and immutable execution contexts;
- the durable job scheduler and admission controller;
- ToolRegistry, permission policy, backend provenance, RunStore, artifacts, and context handles;
- Python sandbox and risk-analysis components for ordinary Python execution;
- managed process, test runner, Git-read, LSP, deterministic, repository-search, and context subsystems;
- session projections and provider capability negotiation.

It does not own:

- general-purpose workflow orchestration outside software-development tools;
- arbitrary Python execution inside the Tool Program runtime;
- approval UX or authorization policy itself;
- general file mutation, patch, shell, Git mutation, commit, push, or subagent delegation through programs in version 1;
- full ACP product implementation;
- provider-specific tool policy or a second provider-owned scheduler;
- transparent replacement of semantic agent reasoning with code.

## 2. Work classification

### Invariants

- The daemon scheduler is the sole admission authority for durable and heavy work.
- A Tool Program cannot acquire more authority than the submitting principal/session/agent and its frozen capability manifest.
- Every nested call uses the same authorization, path, schema, provenance, artifact, and audit boundary as a direct call.
- Every accepted program has finite static or runtime-enforced bounds.
- Every program attempt reaches one terminal or explicitly recoverable state.
- Completed calls are not repeated during retry or restart replay.
- Non-idempotent, destructive, approval-sensitive, and general mutation tools are direct-only in version 1.
- Cancellation propagates to all nested calls, child jobs, and process groups.
- Large intermediate outputs remain behind bounded artifact handles.
- Provider adapters cannot bypass native policy or persistence.

### Capabilities

- Agents can submit a restricted-Python tool program and await one compact structured result.
- Agents can express bounded loops, conditionals, parallel reads, filtering, aggregation, and validation.
- Programs can submit and await safe build/test/lint/format child jobs.
- Programs can run in the background while the parent agent continues and receives exactly one terminal notification.
- Operators can inspect program state, nested calls, budgets, artifacts, failures, and recovery history.
- OpenAI-hosted programmatic calling can be used when supported without changing CodeGG semantics.
- The feature can be exercised through a non-TUI harness and pinned Eggpool model configuration.

### Infrastructure

- Scheduler-owned Python executor and durable source reference.
- Tool contract metadata and Tool Broker.
- Tool Program store, call ledger, capability manifest, source/IR hashes, and checkpoint format.
- Restricted-Python parser, validator, compiler, and interpreter.
- Program result schema, progress events, cache keys, and context projection.
- Provider capability model and OpenAI Responses adapter.
- Deterministic fault-injection and evaluation harness.

### Polish

- Compact diagnostics and unsupported-syntax source spans.
- TUI program status and inspection views.
- Prompt guidance and tool-selection heuristics.
- Cache hit, token, latency, call-count, and artifact-volume metrics.
- Operator documentation and reusable harness skill.

## 3. Non-goals

- Executing arbitrary CPython bytecode as the orchestration runtime.
- Supporting unrestricted imports, reflection, native extensions, threads, signals, sockets, subprocesses, or filesystem access inside Tool Programs.
- Allowing unbounded `while` loops, recursion, dynamic evaluation, or self-modifying programs.
- Making every existing tool program-callable.
- Creating a durable scheduler job for every trivial read-only nested call.
- Treating reduced token use as sufficient correctness evidence.
- Depending on a live OpenAI service for deterministic unit or integration closure.
- Committing local Eggpool API keys or private-LAN endpoints to the repository.

## 4. Current state

At repository baseline `2f715941516a1d49be578fdef56714ad3ddfe8bf`:

- durable jobs and attempts already have typed identities, retry policy, idempotency, resource requests, cancellation requests, deadlines, heartbeat, and restart recovery;
- `JobKind` includes Python, test, build, lint, format, shell, managed process, subagent, Git, and research variants;
- `JobSubmissionService` is the canonical create-and-enqueue boundary with workspace validation and transport idempotency;
- the default scheduler executor registry includes test, managed argv, and optional subagent executors, but no production Python executor;
- `PythonScriptTool` executes through `execute_and_persist_python_script` directly, after which RunStore persistence is best effort;
- Python execution already has AST-first risk analysis, mode-specific capabilities, workspace containment, Landlock or portable fallback, snapshots, timeouts, changed-file detection, and projection;
- `Tool` exposes input parameters, category, optional structured output/provenance, deferred loading, and model exposure, but has no output schema, caller policy, effect class, implementation version, or programmatic retry contract;
- direct tool execution is not centralized behind a single broker carrying scheduler/job lineage, cancellation, authorization, schema, cache, and artifact context;
- tool outputs remain primarily strings, which are unsuitable as stable program inputs;
- the OpenAI-compatible provider targets Chat Completions and ordinary function tools; it does not model Responses program items or caller lineage;
- the agent loop already has turn/tool budgets, provider retry, timeout, idle detection, and doom-loop protections, but those do not provide durable program replay;
- context artifacts and progressive disclosure exist, but Python-run pseudo-labels and many tool strings are not uniformly expandable or content-addressed;
- RTK integration exists as an output-projection option, not a canonical typed-result boundary.

## 5. Target architecture

```text
AgentLoop / ACP / API
        |
        | direct tool call or tool_program submission
        v
Canonical Tool Broker
        |-- resolve frozen ToolContract
        |-- validate caller, authority, input schema, path policy
        |-- select inline execution or scheduler child job
        |-- validate output schema
        |-- persist provenance/artifacts/call ledger
        `-- return ToolValue

Tool Program submission
        |
        v
JobSubmissionService -> JobKind::ToolProgram
        |
        v
ToolProgramExecutor
        |-- load immutable source and capability manifest
        |-- parse/validate/compile restricted Python
        |-- execute metered IR
        |-- call Tool Broker
        |-- heartbeat/checkpoint after progress boundaries
        |-- replay completed calls after restart
        `-- emit structured terminal/incomplete result

Provider adapters
        |-- native restricted-Python function tool
        `-- OpenAI Responses hosted program adapter
                  |
                  `-- normalize nested calls into Tool Broker and ledger
```

### Primary domain objects

- `ToolProgramId` — durable logical program identity.
- `ToolProgramAttemptId` — one execution attempt; scheduler `AttemptId` remains authoritative and may be referenced directly.
- `ProgramCallId` — durable nested-call identity within one program.
- `ProgramSourceRef` — immutable content-addressed source reference.
- `ProgramCapabilityManifest` — frozen callable-tool contracts and authority digest.
- `ToolContract` — schemas, caller policy, effect/idempotency, version, retry, projection, and cache semantics.
- `ProgramCheckpoint` — deterministic interpreter position, loop counters, remaining budgets, and completed-call cursor.
- `ProgramCallRecord` — normalized input hash, result/artifacts, timing, retries, status, and replay disposition.
- `ProgramResult` — schema-validated terminal, incomplete, cancelled, timed-out, stalled, or failed outcome.

### Execution split

Inline broker execution is appropriate for bounded read-only deterministic operations such as file reads, grep/glob/list, deterministic helpers, safe LSP reads, repository maps, Git reads, and cached context retrieval.

Child scheduler jobs are required for build, test, lint, format, managed process, ordinary Python verify jobs, research, and any operation that consumes scarce resources or owns a process tree.

## 6. Dependency graph

```text
M001 Scheduler-owned Python execution
    |
    v
M002 Tool contracts and canonical broker
    |
    v
M003 Program domain, storage, and call ledger
    |
    v
M004 Restricted-Python frontend and static bounds
    |
    v
M005 Durable interpreter, watchdog, and restart recovery
    |
    v
M006 Read-only programmable tool palette and foreground tool
    |
    v
M007 Build/test child-job composition and output projection
    |
    v
M008 Background programs, projections, and parent notification
    |
    +------------------+
    |                  |
    v                  v
M009 OpenAI Responses  M010 ACP/headless, Eggpool, chaos,
adapter              performance, and closure
    |                  ^
    +------------------+
```

Dependency classification:

- Each milestone has a hard dependency on the preceding milestone through M008.
- M009 has a hard dependency on M008 and an interface dependency on provider capability negotiation.
- M010 has a hard dependency on M008, a soft dependency on M009 for hosted/native equivalence tests, and an operational dependency on a locally supplied Eggpool endpoint and credential.
- Full ACP transport testing is an interface dependency; the plan must use the production headless/native protocol path until the ACP adapter exists, then add the same fixtures to ACP without duplicating runtime logic.

## 7. Milestones

### Milestone 1 — Scheduler-owned Python execution

Class: invariant / infrastructure

Objective: make ordinary Python analyze, transform, and verify execution a durable scheduler-owned operation with cancellation, active RunStore ownership, immutable source references, and one production executor.

Dependencies: current scheduler, Python subsystem, workspace services, RunStore.

Exit conditions:

- `PythonScriptTool` and Bash Python routing submit through `JobSubmissionService`;
- `PythonJobExecutor` is registered and cancellation-aware;
- no production model-facing Python path executes directly outside scheduler authority;
- source integrity, timeout, sandbox, snapshots, artifacts, and terminal state survive restart/recovery rules.

Plan: `plans/implementation/tool-programs/001-scheduler-owned-python-execution.md`

### Milestone 2 — Tool contracts and canonical broker

Class: invariant / infrastructure

Objective: introduce structured tool contracts and route ordinary agent tool calls through one broker without changing user-visible semantics.

Dependencies: M001 closed.

Exit conditions:

- tools expose or inherit explicit caller, effect, schema, version, retry, projection, and cache contracts;
- direct execution passes through one broker carrying immutable execution context and lineage;
- permission, schema, provenance, artifact, and cancellation behavior has parity with existing paths;
- legacy tools remain compatible through conservative adapters.

Plan: `plans/implementation/tool-programs/002-tool-contracts-and-canonical-broker.md`

### Milestone 3 — Program domain, storage, and call ledger

Class: invariant / infrastructure

Objective: add durable program identities, manifests, source/IR references, checkpoints, call records, result records, storage migrations, and protocol DTOs before executing real programs.

Dependencies: M002 closed.

Exit conditions:

- program and call records are typed, bounded, versioned, and restart-readable;
- capability manifests are immutable and hash-verifiable;
- replay, retention, redaction, artifact, and migration rules are explicit;
- scheduler submission supports `JobKind::ToolProgram` but fails closed until an executor is available.

Plan: `plans/implementation/tool-programs/003-program-domain-storage-and-call-ledger.md`

### Milestone 4 — Restricted-Python frontend and static bounds

Class: infrastructure

Objective: parse a documented Python subset, reject unsafe/unbounded constructs, and compile accepted source to a deterministic versioned IR with source-span diagnostics.

Dependencies: M003 closed.

Exit conditions:

- accepted programs have bounded source, AST, loops, call sites, and parallel width;
- unsupported or dangerous syntax fails before scheduler execution;
- compiler output is deterministic and content-hashed;
- parser/compiler fuzz and adversarial suites have no accepted bypass.

Plan: `plans/implementation/tool-programs/004-restricted-python-frontend-and-static-bounds.md`

### Milestone 5 — Durable interpreter, watchdog, and restart recovery

Class: invariant / infrastructure

Objective: execute the IR under scheduler ownership with metered steps, heartbeats, checkpoints, nested broker calls, cancellation propagation, stall detection, and replay.

Dependencies: M004 closed.

Exit conditions:

- fixture programs execute and return schema-validated outcomes;
- completed calls are replayed rather than repeated after restart;
- divergence, budget exhaustion, stall, timeout, cancellation, and lost workers yield bounded terminal/recoverable states;
- fault injection cannot strand a running program indefinitely.

Plan: `plans/implementation/tool-programs/005-durable-interpreter-watchdog-and-recovery.md`

### Milestone 6 — Read-only programmable tool palette

Class: capability

Objective: expose the foreground `tool_program` model tool with a conservative read-only/safe-repeat palette, structured outputs, caching, context artifacts, and prompt routing guidance.

Dependencies: M005 closed.

Exit conditions:

- agents can run bounded multi-read/search/filter/aggregate programs through ordinary function calling;
- only explicitly migrated tools appear in program manifests;
- intermediate output stays out of the parent transcript by default;
- direct and programmatic answers are equivalent on the read-only evaluation corpus.

Plan: `plans/implementation/tool-programs/006-read-only-programmable-tool-palette.md`

### Milestone 7 — Build/test child-job composition

Class: capability / infrastructure

Objective: allow programs to submit and await safe scheduler-owned build, test, lint, and format jobs with inherited deadlines, permits, cancellation, structured output, and RTK-compatible projection.

Dependencies: M006 closed.

Exit conditions:

- programs do not spawn build/test processes directly;
- child-job status and artifacts are correlated to program calls;
- parent cancellation terminates all descendants;
- raw output is preserved while native typed projectors or RTK produce bounded summaries;
- resource contention cannot be bypassed through program parallelism.

Plan: `plans/implementation/tool-programs/007-build-test-child-job-composition.md`

### Milestone 8 — Background programs, projections, and parent notification

Class: capability

Objective: add background submission, progress/result projections, read-only inspection, and exactly-once parent-session terminal notification while the parent agent continues.

Dependencies: M007 closed.

Exit conditions:

- foreground and background submissions share one runtime;
- the parent is never required to poll manually;
- terminal notification is durable, idempotent, bounded, and injected exactly once;
- TUI/native protocol can inspect active program state and call history without exposing secrets or unbounded output.

Plan: `plans/implementation/tool-programs/008-background-projections-and-parent-notification.md`

### Milestone 9 — OpenAI Responses hosted-program adapter

Class: capability / interoperability

Objective: add an optional Responses API transport that normalizes hosted program items and nested client-owned calls into CodeGG program, broker, ledger, artifact, cancellation, and policy semantics.

Dependencies: M008 closed; stable provider capability interface.

Exit conditions:

- hosted programs never bypass Tool Broker or scheduler-owned child jobs;
- caller lineage, fingerprints, continuation, incomplete state, and program output are preserved;
- unsupported providers fall back to native restricted Python without silent semantic changes;
- deterministic fixtures cover streaming, retry, continuation, cancellation, and malformed provider items.

Plan: `plans/implementation/tool-programs/009-openai-responses-hosted-program-adapter.md`

### Milestone 10 — Harness, Eggpool, chaos, performance, and closure

Class: capability verification / polish

Objective: prove non-TUI usability, exact-model routing, failure containment, context reduction, performance, security, and documentation before declaring the subsystem closed.

Dependencies: M008 closed; M009 soft for hosted equivalence; local Eggpool access operationally supplied.

Exit conditions:

- scripted/headless and ACP-compatible fixtures execute the same runtime as the TUI;
- local Eggpool testing pins `mimo-v2.5` with no model fallback and no committed credentials;
- at least 10 percent injected provider/tool/worker/restart failures always converge to terminal or recoverable state;
- no program exceeds configured stall, cancellation, result, resource, or transcript bounds;
- closure evidence quantifies call reduction, token/context reduction, latency, cache behavior, correctness, and evidence retention;
- no unresolved high or medium finding remains.

Plan: `plans/implementation/tool-programs/010-harness-eggpool-chaos-performance-and-closure.md`

## 8. Cross-cutting requirements

### Storage and migration

- Use versioned records and additive SQLite migrations.
- Program source and compiled IR must be immutable, content-addressed, and integrity checked.
- Call-ledger arguments and results must be bounded and redactable; large bodies use artifacts.
- Retention of program source, call records, and artifacts must be independently configurable.
- Unknown future record variants must fail closed or remain inspectable without execution.

### Protocol and compatibility

- Add versioned program snapshots/events and bounded query operations.
- Preserve existing direct tool and Python schemas through compatibility adapters during migration.
- Provider capability negotiation must distinguish ordinary tools, native Tool Programs, and hosted program support.
- ACP and TUI consume the same frontend-neutral state; neither owns execution.

### Security and authorization

- Resolve authority per nested call, not only at program admission.
- Validate cache hits and replay against current authorization and immutable execution context.
- Never expose credentials, environment secrets, private provider reasoning, or unredacted sensitive tool arguments.
- Reject manifest/tool schema drift before invocation.
- Keep mutation-capable tools direct-only in version 1.

### Concurrency, cancellation, and recovery

- Program and child-job parallelism consume scheduler/resource budgets.
- Deadlines monotonically narrow from parent to program to child call/job.
- Cancellation is downward and idempotent.
- Heartbeat proves interpreter or child-job progress; elapsed time alone is not progress.
- Restart replay uses completed-call records and detects divergent control flow.
- Lost worker ownership must be reconciled by daemon generation and watchdog policy.

### Observability and audit

- Emit program submitted/admitted/started/progress/waiting/completed/failed/incomplete/cancelled/stalled events.
- Correlate program, scheduler job, attempt, session, turn, agent run, nested calls, child jobs, runs, and artifacts.
- Record budgets used, retries, cache hits, output truncation, source/manifest/IR hashes, and failure class.
- Do not persist raw hidden reasoning.

### Performance and resource use

- Avoid one durable scheduler job per trivial read; use a bounded call ledger instead.
- Bound in-flight calls, child jobs, queue capacity, source, AST, IR, checkpoint, result, and artifact sizes.
- Cache only read-only/safe-repeat calls with workspace revision and dirty-overlay identity.
- Preserve raw artifacts once; project compact model-facing summaries separately.

### Documentation and operations

- Maintain `architecture/tool_programs.md`, Python execution docs, tool contracts, provider capability docs, and operator troubleshooting.
- Provide a reusable repository skill and non-TUI harness examples.
- Document unsupported syntax, direct-only tools, failure states, cancellation, cache semantics, and credential handling.

## 9. Verification strategy

Subsystem closure requires:

- unit tests for schemas, caller policy, parser, static bounds, IR, interpreter, budgets, cache keys, and redaction;
- integration tests through the production Tool Broker, scheduler, RunStore, artifacts, Python executor, test runner, and projections;
- restart tests at every call boundary and while child jobs are active;
- cancellation races before admission, during inline calls, during child jobs, during checkpoint persistence, and after terminal completion;
- contention tests proving program parallelism cannot exceed scheduler permits;
- adversarial source tests for imports, reflection, dynamic execution, huge literals, deep ASTs, loop amplification, argument bombs, artifact expansion, and manifest drift;
- provider fixture tests for malformed and reordered hosted program items;
- scripted model and exact Eggpool model tests without relying on TUI interaction;
- repeated fault injection and resource-convergence loops;
- direct-versus-programmatic correctness and evidence equivalence evaluation;
- measured context/token reduction without loss of source or test evidence.

## 10. Risks and decision points

- Parser dependency weight and maintenance must be reviewed in M004; the parser must operate in-process and must not execute source.
- Output schema migration may reveal tools whose current string output is not sufficiently stable; those tools remain direct-only.
- Cache identity must account for dirty workspaces without hashing entire repositories on every call.
- Background parent notification must integrate with durable session follow-up without creating unsolicited model loops.
- Hosted provider semantics may differ from native programs; normalization must preserve explicit differences rather than fabricate equivalence.
- Full ACP transport evidence depends on the ACP adapter roadmap, but runtime closure must not depend on TUI-only behavior.
- Mutation-capable program calls require a future ADR if pursued.

## 11. Completion definition

This subsystem is closed only when:

1. M001–M010 each has an accepted closure record;
2. ordinary Python and Tool Program durable work is scheduler-owned;
3. direct and programmatic calls share one broker and policy boundary;
4. accepted programs cannot run indefinitely or strand child work;
5. restart and retry never repeat completed non-read effects;
6. only explicitly eligible tools can be called programmatically;
7. foreground, background, native-provider, hosted-provider, headless, and frontend projections converge on the same domain state;
8. exact-model and fault-injection evidence is recorded truthfully;
9. no unresolved high or medium correctness, security, recovery, or resource-leak finding remains;
10. roadmap, implementation plans, closure records, architecture docs, and registry agree.

## 12. Milestone status

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| 001 | closed | `plans/implementation/tool-programs/001-scheduler-owned-python-execution.md` | `plans/closure/tool-programs/001-status.md` | — |
| 002 | closed | `plans/implementation/tool-programs/002-tool-contracts-and-canonical-broker.md` | `plans/closure/tool-programs/002-status.md` | — |
| 003 | closed | `plans/implementation/tool-programs/003-program-domain-storage-and-call-ledger.md` | `plans/closure/tool-programs/003-status.md` | — |
| 004 | closed | `plans/implementation/tool-programs/004-restricted-python-frontend-and-static-bounds.md` | `plans/closure/tool-programs/004-status.md` | — |
| 005 | closed | `plans/implementation/tool-programs/005-durable-interpreter-watchdog-and-recovery.md` | `plans/closure/tool-programs/005-status.md` | — |
| 006 | closing | `plans/implementation/tool-programs/006-read-only-programmable-tool-palette.md` | `plans/closure/tool-programs/006-status.md` | — |
| 007 | blocked | `plans/implementation/tool-programs/007-build-test-child-job-composition.md` | — | M006 closure |
| 008 | blocked | `plans/implementation/tool-programs/008-background-projections-and-parent-notification.md` | — | M007 closure |
| 009 | blocked | `plans/implementation/tool-programs/009-openai-responses-hosted-program-adapter.md` | — | M008 closure and provider interface |
| 010 | blocked | `plans/implementation/tool-programs/010-harness-eggpool-chaos-performance-and-closure.md` | — | M008 closure; M009 soft; local Eggpool operational input |
