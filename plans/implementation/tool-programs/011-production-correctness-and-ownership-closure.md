# Tool Programs Milestone 011 — Production Correctness and Ownership Closure

Status: closed

Repository baseline: `4dbb04e9a402c85ee1dd97d94c55f3951d0debd4` (`main`)

Source roadmap and corrective addendum:

- `plans/subsystems/tool-programs-roadmap.md`
- `plans/subsystems/tool-programs-correctness-closure-addendum.md`

Historical closure record being superseded for strict closure:

- `plans/closure/tool-programs/010-status.md`

Applicable ADR:

- `plans/adrs/ADR-0001-programmatic-tool-execution-authority.md`

Primary class: invariant / correctness / ownership / recovery / interoperability / final closure

## 1. Objective

Correct the production-boundary defects found after Milestones 001–010 and establish one mechanism-faithful closure record for Tool Programs.

The pass must make logical invocation identity, scheduler ownership, authority propagation, broker enforcement, durable call replay, parent notification, child-job composition, timeout/heartbeat behavior, artifact ownership, and hosted-provider selection agree across the model-facing tool, daemon scheduler, interpreter, Tool Broker, provider layer, projections, and public inspection interfaces.

This pass is complete only when the production daemon path—not only library fixtures—proves that programs cannot become orphaned, duplicate completed calls, lose parent identity, bypass authorization, strand child work, misdeliver notifications, silently ignore deadlines, or claim hosted behavior that is not actually selectable.

This is a corrective closure pass. It must not broaden version-1 authority or add general mutation, patch, shell, Git mutation, commit, push, or subagent tools to the programmable palette.

## 2. Readiness and closure disposition

All implementation prerequisites are present:

- scheduler-owned Python execution;
- Tool Program source persistence and restricted-Python compilation;
- verified IR and metered interpretation;
- Tool Broker and structured contracts;
- durable job/attempt storage;
- read-only programmable tools;
- scheduler-owned build/test/lint/format jobs;
- background projections and notification service;
- Responses API hosted-program infrastructure;
- native `core-stdio` harness and deterministic fault tests.

No new architectural research phase is required. The implementation agent should begin with a production-path audit and make the smallest coherent domain/storage/API changes needed to satisfy the invariants below.

Until this milestone closes:

- the Tool Programs subsystem remains active;
- M010 is a historical conditional closure only;
- M002, M005, M007, M008, and M009 do not independently establish strict production closure where M011 identifies a conflict;
- documentation must distinguish component capability from production wiring.

## 3. Post-implementation findings to close

### F-01 — Source identity is being used as logical invocation identity

Current behavior derives `program_id` and transport idempotency primarily from source digest. Identical source can therefore collide across sessions, turns, manifests, timeouts, provider backends, or deliberate repeated invocations.

Required correction:

- source digest remains immutable content identity only;
- every accepted logical invocation receives a distinct durable `ToolProgramId`;
- submission deduplication uses an explicit caller-provided or daemon-derived invocation key, not source alone;
- retry of the same submission resolves to the same logical invocation;
- a new deliberate invocation of identical source creates a new program identity;
- source, IR, manifest, program, attempt, call, child job, run, artifact, provider continuation, and notification identities remain distinct.

Severity for closure: high.

### F-02 — Parent lineage and authority are dropped

Current model-facing submission does not reliably persist session, turn, agent, parent job/attempt, principal, permission mode, or authority snapshot. Programmatic Broker calls then use incomplete context and a boolean authorization bypass.

Required correction:

- define one immutable `ProgramExecutionContext` or equivalent persisted submission context;
- include workspace, session, turn, agent run, parent job/attempt where applicable, principal/authority reference, permission policy reference, path policy identity, backend policy, and submission correlation;
- carry this context into every nested Broker invocation and child job;
- perform per-call authorization and path validation using real policy services;
- authority digest is verified against a persisted immutable authority record or removed if it cannot be meaningfully verified;
- no `caller_authorized: true`-style bypass may substitute for an actual authority decision in the production program path.

Severity for closure: high.

### F-03 — Tool Broker is not the single enforced production boundary

Current direct AgentLoop execution still has a legacy primary path, while Broker enforcement does not fully apply input/output schemas, timeout, cancellation, path policy, authorization, artifact persistence, and terminal recording.

Required correction:

- route all production direct agent tool calls through `ToolBroker`;
- retain compatibility adapters inside the Broker boundary rather than beside it;
- validate JSON input against the contract input schema before execution;
- enforce caller policy, effect class, authority, workspace/path policy, deadline, cancellation, retry eligibility, and idempotency before dispatch;
- apply the effective timeout around execution;
- validate typed output against `output_schema`;
- bound display and structured values independently;
- persist large output once in the canonical artifact/context store and return a resolvable handle;
- persist invocation lineage, provenance, terminal status, elapsed time, truncation, artifacts, and failure class;
- convert tool execution failures to typed failures without accidentally returning an outer success result.

Severity for closure: high.

### F-04 — Production restart replay is not durable at call boundaries

The interpreter supports checkpoints and completed-call replay in memory, but the production executor starts a fresh interpreter and persists the public call ledger only after the full run returns.

Required correction:

- persist call reservation before dispatch;
- persist completed typed result/artifact references before advancing past a call instruction;
- persist checkpoint/replay cursor at deterministic progress boundaries;
- reload the latest verified checkpoint and completed-call ledger when an interrupted/recoverable attempt is resumed;
- replay from durable records without invoking the tool again;
- compare sequence, tool contract version, normalized arguments hash, workspace/context identity, and control-flow fingerprint;
- fail recoverably on divergence rather than executing an ambiguous call;
- make call reservation/completion/checkpoint ordering crash-testable;
- do not claim resume by serializing an interpreter process or trusting a stale in-memory map.

Severity for closure: high.

### F-05 — Program timeout and heartbeat are not scheduler-owned end to end

User-configured program timeout is stored on the job but is not consistently enforced as the executor wall deadline. Interpreter heartbeat currently does not update durable attempt progress.

Required correction:

- resolve one effective deadline at submission from requested timeout and parent deadline;
- persist it on the job/attempt;
- scheduler applies an outer timeout to every executor;
- executor and interpreter receive the same or narrower deadline;
- nested calls and child jobs receive monotonically narrower deadlines;
- heartbeat updates the durable attempt heartbeat and includes bounded program budget/progress metadata;
- active child-job progress counts as parent progress without allowing an indefinitely silent wait;
- watchdog reconciliation distinguishes active, stalled, timed out, cancelled, and lost-worker states;
- zero/unlimited values are prohibited for model-facing Tool Programs unless an administrator policy explicitly allows them with another finite bound.

Severity for closure: high.

### F-06 — Background notification is not durably parent-addressed

Current notification records are daemon-memory scoped, may be created with an empty session ID, and do not prove terminal update, persisted acknowledgement, or exactly-once recovery.

Required correction:

- persist notification records and delivery state in SQLite or the canonical durable event/inbox store;
- create notifications from terminal program state, not merely from background submission;
- use the real parent session, turn, and agent identities;
- define one notification ID from program identity plus notification kind/version;
- atomically or transactionally reconcile terminal result and pending notification publication;
- persist claim lease, injection/consumption acknowledgement, suppression, failure, and expiry state;
- recover pending and expired claims after restart;
- never recreate a delivered notification;
- do not inject progress updates as unsolicited model turns;
- bound payload size and place detailed result/evidence behind inspection/artifact handles;
- archived/deleted parent sessions follow an explicit suppression or retained-inbox policy.

Severity for closure: high.

### F-07 — Child-job identity, cancellation, deadline, resources, and artifacts are incomplete

Current child deduplication can collapse deliberate identical calls because it is based on operation/configuration rather than program call identity. Parent lineage, cancellation, deadlines, and result artifacts are incomplete.

Required correction:

- assign every `submit_job()` instruction a deterministic `ProgramCallId`/sequence identity;
- child submission idempotency includes program ID, attempt/replay identity, call sequence, operation, and normalized configuration hash;
- retry/replay of the same call resolves the same child job;
- two deliberate identical instructions create two distinct child jobs;
- persist parent program/job/attempt/call lineage on the child without abusing scheduler dependency semantics;
- child job source is programmatic/delegated rather than interactive;
- parent cancellation requests child cancellation and waits only for a bounded convergence period;
- child deadline cannot exceed the parent deadline;
- process-group termination is proven for managed test/build commands;
- child status, typed projector result, raw RunStore artifacts, RTK/native summary, and failure evidence are returned through durable handles;
- avoid nested admission deadlock: an orchestration program must not hold a permit dimension required exclusively by the child it awaits;
- capacity-one scheduler tests must prove progress and permit convergence.

Severity for closure: high.

### F-08 — Foreground result mapping is lossy and can misstate execution

Current foreground result parsing derives fields from summary strings and maps elapsed milliseconds into `steps_used`; intermediate artifact arrays can be placeholders.

Required correction:

- expose a typed durable `ProgramResultRecord` or result artifact from the executor;
- foreground and background consumers read the same typed result;
- do not parse semantic fields from human-readable summaries;
- return real steps, iterations, calls, retries, cache hits, artifacts, terminal class, failure class, and output handle/value;
- outer `StructuredToolResult.success` and terminal status reflect program outcome;
- raw source, arguments, and unbounded outputs remain excluded from parent transcript and public projections.

Severity for closure: medium.

### F-09 — Hosted Responses support is not selected through production runtime

The provider module and deterministic tests are substantial, but hosted execution is not yet proven through normal AgentLoop/provider/backend selection.

Required correction:

- integrate hosted capability negotiation and backend policy into the normal runtime factory/provider path;
- default remains native restricted Python unless configuration and provider capability explicitly select hosted execution;
- `HostedRequired` fails closed when unavailable;
- `HostedPreferred` fallback is explicit, observable, and semantically safe;
- hosted nested calls use the same ProgramExecutionContext, Broker, ledger, call identity, authorization, artifact, deadline, cancellation, and projection rules;
- continuation/fingerprint state is persisted where restart semantics require it;
- no provider-generated identity becomes a Codegg durable identity without normalization;
- exercise selection through a production-shaped provider fixture and, when available, a live OpenAI-compatible connection.

Severity for closure: medium.

### F-10 — Closure evidence is fixture-heavy and registry status is inconsistent with findings

Required correction:

- add process-level restart/kill tests through `core-stdio` or another public daemon protocol;
- test parent notification through a real session/AgentLoop boundary;
- test direct calls through the final Broker-only path;
- test child cancellation and capacity-one resource convergence;
- test hosted selection through normal runtime construction;
- update architecture, closure records, roadmap addendum, and registry truthfully;
- create `plans/closure/tool-programs/011-status.md` only after implementation and verification.

Severity for closure: medium.

## 4. Invariants that must not regress

- The daemon scheduler is the sole authority for durable/heavy execution.
- Source content does not define logical invocation identity.
- A program cannot acquire authority beyond its submitting principal and frozen manifest.
- Authorization and path policy are checked on every nested invocation, cache hit, and replay.
- Direct and programmatic calls share one Tool Broker implementation.
- Mutation-capable, destructive, approval-sensitive, shell, patch, Git mutation, commit, push, and subagent tools remain direct-only.
- Every accepted program has finite static and runtime bounds.
- Every attempt converges to one terminal or explicitly recoverable state.
- Completed calls are not re-executed during retry or daemon restart.
- Call reservations, completion records, checkpoints, artifacts, terminal results, and notifications are durable before dependent state advances.
- Cancellation is downward, idempotent, and bounded.
- Deadlines monotonically narrow.
- A parent program cannot deadlock the scheduler while waiting for a child.
- Large or sensitive data is stored once behind bounded handles.
- Public projection and parent transcript never expose raw secrets, hidden reasoning, unbounded call arguments, or unbounded outputs.
- Hosted providers cannot bypass native policy, persistence, or audit boundaries.
- Test-only fixtures cannot be registered as production executors or closure evidence.

## 5. Scope

### In scope

- Domain and storage changes needed for invocation identity, execution context, call ledger, checkpoints, results, child links, and notifications.
- Migration of direct AgentLoop calls into the canonical Tool Broker.
- Broker contract enforcement and artifact persistence.
- Production interpreter resume/replay and durable heartbeat.
- Scheduler outer timeout and child cancellation/deadline/resource propagation.
- Durable background terminal notification and parent-session injection.
- Typed foreground/background result convergence.
- Production hosted-provider capability selection and normalized lifecycle.
- Public protocol/projection changes needed for bounded inspection.
- Mechanism-faithful tests, static guards, architecture docs, closure record, roadmap/addendum reconciliation, and registry updates.

### Explicitly out of scope

- Adding new mutation-capable program tools.
- General workflow orchestration unrelated to software-development operations.
- Arbitrary CPython execution for Tool Programs.
- Replacing the scheduler, Tool Registry, permission subsystem, RunStore, or session projections wholesale.
- Implementing the full ACP product roadmap.
- Requiring a live external provider for deterministic correctness closure.
- Optimizing by weakening persistence, evidence, authorization, or resource controls.
- Rewriting historical closure records to erase the audit trail.

## 6. Required domain and storage model

Implement or refine the following canonical records. Exact Rust names may differ, but ownership and fields must remain explicit.

### ToolProgramInvocation

Required fields:

- generated `ToolProgramId`;
- immutable source ref/digest and IR ref/digest;
- frozen manifest ID/digest and contract versions;
- `ProgramExecutionContext` reference;
- invocation idempotency key;
- requested/effective timeout and deadline;
- foreground/background mode;
- native/hosted backend policy and selected backend;
- created/submitted timestamps;
- current logical state and terminal result reference.

### ProgramExecutionContext

Required fields:

- workspace ID and canonical root/path-policy identity;
- session ID;
- turn ID;
- agent/agent-run ID where available;
- parent job/attempt/call IDs where applicable;
- submitting principal/authority reference;
- permission mode and policy revision/reference;
- provider connection/model/backend policy where applicable;
- immutable correlation ID and schema version.

Do not persist secrets or raw credentials in this record.

### ProgramAttemptState

Required fields:

- scheduler attempt ID and daemon generation;
- checkpoint version and program counter/control-flow fingerprint;
- budget counters and remaining deadline;
- last durable heartbeat/progress time;
- completed-call cursor;
- child jobs currently awaited;
- terminal/recoverable state and failure class;
- restart/recovery disposition.

### ProgramCallRecord

Required states:

- reserved;
- dispatched;
- completed;
- failed terminal;
- retryable/recoverable;
- cancelled;
- replayed from durable result.

Required identity/evidence:

- program/attempt/call sequence;
- tool name, contract version, effect/idempotency class;
- normalized input hash, context/authority hash, workspace revision identity;
- reservation and completion timestamps;
- retries and selected backend;
- typed result/artifact handles, output schema version, terminal class;
- replay count/disposition and divergence diagnostics.

### ProgramNotificationRecord

Required fields:

- notification identity and schema version;
- program/job/result identity;
- parent session/turn/agent identity;
- bounded classification/summary and inspection handle;
- pending/claimed/delivered/suppressed/expired/failed state;
- claim owner/lease expiry;
- delivered/acknowledged timestamp;
- payload digest and retry count.

### Migration rules

- Use additive schema migration(s).
- Old jobs lacking a valid execution context or source reference fail closed and remain inspectable.
- Historical source-digest-derived program records must not be silently merged with new invocation IDs.
- Unknown record versions are inspectable but not executable.
- Migration is restart-safe and idempotent.
- Storage retention distinguishes source, IR, call records, result artifacts, and notification acknowledgements.

## 7. Ordered work packages

### Work package A — Reconcile identities and immutable submission context

1. Introduce generated logical program identity distinct from source digest.
2. Define persisted `ProgramExecutionContext`.
3. Thread real session, turn, agent, parent job/attempt, authority, workspace, timeout, and backend policy from AgentLoop/tool context into `ToolProgramTool` and `NewJob`.
4. Redesign submission idempotency:
   - same transport/tool-call retry returns the same program;
   - a new deliberate call with identical source creates a new program;
   - manifest/limits/backend/context mismatches reject reuse.
5. Add compatibility decoding for old payloads that fails closed when authority/context is incomplete.
6. Add domain tests for identity separation, deduplication, cross-session isolation, and repeated identical invocations.

Acceptance gate A:

- two sessions submitting identical source receive different program IDs and records;
- retrying the same tool call receives the original program/job;
- same source with different manifest or timeout cannot alias;
- all public inspection output correlates source and invocation without conflating them.

### Work package B — Complete canonical Tool Broker ownership

1. Inventory every production call site of `Tool::execute`, `execute_structured`, `execute_capture`, registry execution helpers, and backend-specific direct execution.
2. Move AgentLoop direct calls through `ToolBroker` while preserving permission UX, hooks, event emission, risk summaries, timeout behavior, and visible output.
3. Expand `BrokerInvocationContext` to carry cancellation, absolute deadline, execution context/authority references, path policy, job/attempt/call identities, and artifact services.
4. Replace boolean preauthorization with a typed verified authority decision/reference whose scope can be checked.
5. Enforce input JSON Schema, caller policy, effect/idempotency, path/workspace policy, deadline, cancellation, and retry eligibility.
6. Apply timeout around tool execution and return typed timeout/cancel outcomes.
7. Validate structured output schema and total byte size.
8. Persist large output through the canonical artifact/context store; handles must resolve and digest verification must pass.
9. Ensure an inner tool failure does not become an outer Broker success.
10. Strengthen `check_tool_broker_boundary.py` or equivalent semantic guard to reject new production bypasses.

Acceptance gate B:

- direct and programmatic execution of the same read-only tool use the same Broker function and contract snapshot;
- invalid inputs never invoke the tool;
- cancelled/timed-out calls terminate within their bound;
- output-schema mismatch is typed and persisted;
- generated artifact handles resolve to the original bounded content;
- no production direct execution bypass remains outside explicitly documented scheduler executor internals.

### Work package C — Durable call reservation, completion, checkpoint, and replay

1. Connect interpreter call sequence to durable `ProgramCallRecord` identities.
2. Persist reservation before Broker dispatch.
3. Persist typed completion/artifacts before interpreter advancement.
4. Persist a checkpoint after completed calls and other deterministic boundaries.
5. On scheduler restart recovery, load invocation, latest attempt/checkpoint, completed calls, remaining deadline, and child links.
6. Populate interpreter replay state before execution.
7. Verify tool, contract version, input hash, authority/context hash, workspace revision, and sequence before returning a replayed result.
8. Fail `Recoverable` with an inspectable divergence record on mismatch.
9. Define recovery for crash windows:
   - before reservation;
   - after reservation before dispatch;
   - during dispatch;
   - after side effect/result before completion record;
   - after completion record before checkpoint;
   - after checkpoint before job terminal publication.
10. For version-1 program-callable read-only tools, ambiguous dispatched-without-completion calls may be safely repeated only when the contract explicitly permits safe repeat and evidence records the ambiguity. Do not generalize this to non-read effects.

Acceptance gate C:

- killing the daemon after each durable boundary never repeats a durably completed call;
- completed child jobs are not resubmitted on replay;
- divergent source/IR/manifest/context/input fails recoverably;
- replayed result is byte/schema/artifact equivalent to the stored completion;
- no attempt remains `Running` after daemon generation reconciliation.

### Work package D — Scheduler timeout, heartbeat, watchdog, and terminal ownership

1. Resolve and persist the effective deadline during submission.
2. Add scheduler-level outer timeout around executor execution.
3. Ensure timeout and cancellation complete the attempt/job through one terminal persistence path.
4. Connect interpreter heartbeat to durable attempt heartbeat/progress.
5. Forward child-job progress as bounded parent progress while the parent waits.
6. Reconcile lost worker ownership by daemon generation and heartbeat age.
7. Define terminal precedence for simultaneous completion/cancel/timeout/restart.
8. Ensure exactly one logical terminal result and no later terminal overwrite.
9. Add metrics for requested timeout, effective deadline, last heartbeat, stall duration, cancellation latency, and terminal persistence latency.

Acceptance gate D:

- requested 500 ms/2 s fixture timeouts stop within documented tolerance;
- cancellation before admission, after admission, during call, during child wait, and after completion converges correctly;
- heartbeat advances durably during interpreter and child progress;
- a deliberately silent broker/child triggers stall or timeout;
- terminal state is stable under duplicate completion events.

### Work package E — Correct child-job ownership and nested resource behavior

1. Add program/attempt/call lineage to child submission.
2. Derive child submission key from the call identity plus normalized request.
3. Distinguish deliberate repeated identical instructions from replay of one instruction.
4. Propagate parent cancellation and narrowed deadline.
5. Persist child job identity before waiting so restart can reattach.
6. On restart, inspect existing child state rather than resubmit.
7. Return typed child result plus raw/summary artifact handles.
8. Preserve runner/build/test framework metadata and failure evidence.
9. Audit resource dimensions so an orchestration program does not hold a permit required by the awaited child.
10. Add capacity-one and multi-workspace contention tests.
11. Prove managed process-group cleanup on cancellation and timeout.

Acceptance gate E:

- two identical deliberate `submit_job()` instructions create two child jobs;
- replay of one instruction reuses its original child;
- parent cancellation cancels active descendants and process groups;
- child deadline never exceeds parent deadline;
- parent and child permits/leases converge to baseline;
- child artifacts resolve and are correlated to the program call.

### Work package F — Durable parent notification and session delivery

1. Move notification state to durable storage or the canonical durable inbox/event model.
2. Stop creating an actionable pending terminal notification at initial submission.
3. Publish pending notification from persisted terminal program result.
4. Populate real parent session/turn/agent identities from `ProgramExecutionContext`.
5. Persist claim lease and acknowledgement.
6. Update AgentLoop injection to:
   - claim a pending notification;
   - inject one bounded system/control message;
   - acknowledge only after the message is durably included in session/turn state;
   - release/expire claim on failure before durable inclusion.
7. Recover pending and expired claims on daemon restart.
8. Never recreate delivered/suppressed notifications.
9. Define ordering for multiple completed programs and per-session bounds.
10. Project inspection handles rather than full result bodies.

Acceptance gate F:

- a real background program completed for session A is never delivered to session B or an empty session;
- restart before claim, after claim, after injection, and after acknowledgement yields exactly one logical delivery;
- duplicate terminal events do not duplicate notification;
- delivered acknowledgement survives restart;
- archived parent policy is explicit and tested;
- the agent is not required or encouraged to poll.

### Work package G — Typed terminal result and artifact convergence

1. Persist one `ProgramResultRecord` for every terminal/recoverable outcome.
2. Foreground waiting, background notification, projections, public inspect, and hosted/native adapters consume the same record.
3. Remove summary-string parsing for steps, calls, status, and failure class.
4. Return real counters, result schema/version, output value/handle, call/artifact handles, retry/cache metrics, and terminal class.
5. Correct outer structured success/status mapping.
6. Store human-readable summary as presentation only.
7. Bound and redact public DTOs.

Acceptance gate G:

- foreground and background representations have the same program/result identity and counters;
- `steps_used` is instruction count, not elapsed milliseconds;
- failed programs produce a failed structured terminal result;
- intermediate raw outputs remain absent from parent transcript;
- artifact handles are real and resolvable.

### Work package H — Production hosted-provider selection

1. Add hosted-program capability and backend policy resolution to runtime/provider construction.
2. Define configuration defaults and validation.
3. Route hosted nested calls through the same Broker and durable call ledger.
4. Persist response ID/fingerprint/continuation state without provider secrets.
5. Apply native authority, deadline, cancellation, artifact, result, and projection rules.
6. Implement explicit fallback outcomes:
   - `NativeOnly` never calls hosted;
   - `HostedRequired` fails when unsupported;
   - `HostedPreferred` records fallback reason and uses native only where semantic equivalence is permitted;
   - no alias or provider substitution is silent.
7. Add a production-shaped scripted provider implementing Responses events.
8. Add optional live test profile without making it a deterministic gate.

Acceptance gate H:

- normal AgentLoop/runtime construction can select hosted execution from provider capabilities;
- unsupported hosted-required configuration fails before execution;
- nested call identity and results survive continuation/restart fixtures;
- hosted provider cannot request a direct-only tool;
- fallback is visible in events/result/provenance.

### Work package I — Public protocol, projections, and inspection reconciliation

1. Update protocol DTOs for invocation identity, execution context summaries, selected backend, attempt/recovery state, child links, durable notification state, and typed result handles.
2. Keep observer visibility/redaction explicit.
3. Ensure list/inspect/call-page endpoints query durable state rather than process memory.
4. Add pagination and byte/count bounds to all new collections.
5. Preserve compatibility for older clients through defaulted fields or versioned responses.
6. Update TUI display only as needed to represent corrected states; execution remains frontend-neutral.

Acceptance gate I:

- process restart does not erase inspectable state;
- public inspection cannot expose source bodies, raw call arguments, secrets, private provider reasoning, or unbounded outputs;
- projection replay converges to the same final program/notification state as a fresh snapshot.

### Work package J — Mechanism-faithful test harness and fault matrix

Extend the existing native harness rather than creating another runtime.

Required production-path scenarios:

1. identical source submitted in two sessions;
2. transport retry of one submission versus deliberate second invocation;
3. direct read and programmatic read through one Broker;
4. daemon kill before reservation;
5. daemon kill after reservation;
6. daemon kill after call completion before checkpoint;
7. daemon kill after checkpoint before job terminal publication;
8. restart while awaiting build/test child;
9. parent cancellation during inline call;
10. parent cancellation during child process;
11. capacity-one child admission without deadlock;
12. timeout while Broker is silent;
13. timeout while child is silent;
14. background terminal notification restart at pending/claimed/injected/acknowledged boundaries;
15. duplicate terminal event;
16. session isolation and archived-session policy;
17. artifact spill and retrieval;
18. output-schema mismatch;
19. manifest/contract/context divergence on replay;
20. hosted-required unavailable;
21. hosted-preferred explicit fallback;
22. hosted nested-call continuation/restart;
23. unrelated workspace/program continuity under target failure;
24. repeated mixed-fault runs at or above 10 percent injection;
25. resource baseline/convergence for tasks, processes, permits, leases, jobs, attempts, call reservations, notification claims, artifact writers, and provider streams.

Tests must use public daemon/protocol interfaces for closure-bearing assertions. Focused unit fixtures may inspect internals but cannot substitute for the scenarios above.

### Work package K — Static guards and repository-wide validation

Add or strengthen semantic guards for:

- Broker-only production tool execution;
- scheduler-only Python/build/test/managed execution;
- no source-derived logical program identity;
- no empty session identity on background submissions/notifications;
- no production `caller_authorized` bypass for program callers;
- no notification state that exists only in daemon memory;
- no end-of-run-only completed-call persistence;
- no summary-string semantic parsing;
- child submission key includes call identity;
- parent cancellation is observed while waiting for child;
- hosted adapter is reachable from production runtime or documentation marks it non-production;
- no program-callable mutating/direct-only tools;
- no unbounded queues, source, IR, checkpoint, result, artifact, or notification payloads;
- no committed provider credentials/endpoints.

Run the targeted test matrix with the repository's intended low-concurrency settings. Also run formatting, compile, clippy/guards, migration, and workspace tests. If a broad repository gate fails for unrelated pre-existing reasons, record exact failures and prove they do not mask Tool Program failures; strict M011 closure still requires all M011-owned warnings/errors to be clean.

### Work package L — Documentation, audit reconciliation, and closure

1. Update:
   - `architecture/tool_programs.md`;
   - `architecture/tool_broker.md`;
   - `architecture/jobs.md`;
   - `architecture/run_store.md`;
   - `architecture/python_scripting.md` where execution ownership intersects;
   - provider capability/Responses documentation;
   - harness skill and troubleshooting guidance.
2. Document identity distinctions, crash-boundary ordering, replay, notification delivery, child ownership, deadlines, cancellation, artifact semantics, and hosted selection.
3. Create `plans/closure/tool-programs/011-status.md` with an evidence matrix tied to exact commits and commands.
4. Reconcile historical M002/M005/M007/M008/M009/M010 claims without deleting them.
5. Update the roadmap addendum and `plans/registry.md` only after closure review.
6. Do not mark the subsystem closed while any high or medium finding remains.

## 8. Required test suites

The implementation may reorganize names, but closure evidence must include equivalent coverage.

### Domain/storage

- program identity and submission idempotency;
- execution context serialization/versioning;
- migration from historical payloads;
- call reservation/completion/checkpoint transactions;
- result record and notification state machine;
- retention and redaction.

### Broker

- direct/programmatic path equivalence;
- caller/authority/path denial;
- input and output schema validation;
- timeout/cancellation;
- artifact persistence and resolution;
- typed failure mapping;
- no bypass static guard.

### Runtime/recovery

- incremental checkpoint and call persistence;
- every crash window;
- replay equivalence and divergence;
- lost worker/daemon generation reconciliation;
- wall timeout, stall, heartbeat, and terminal precedence.

### Child jobs

- call-identity deduplication;
- deliberate repeated identical child operations;
- restart reattachment;
- cancellation/process-group cleanup;
- deadline narrowing;
- capacity-one and contention behavior;
- artifacts and typed projectors.

### Notifications

- terminal-created durable record;
- claim/inject/ack transaction boundaries;
- restart at each boundary;
- duplicate terminal events;
- session isolation;
- archived/suppressed policy;
- bounded payload and inspection handle.

### Hosted provider

- runtime capability selection;
- required/preferred/native policy;
- nested calls through Broker;
- continuation/fingerprint persistence;
- cancellation/timeout/restart;
- explicit fallback and provenance;
- direct-only rejection.

### Harness/closure

- native process-level scenarios;
- mixed fault injection at 10 percent or greater;
- repeated resource convergence;
- direct versus programmatic correctness/evidence/context comparison;
- optional exact Eggpool `mimo-v2.5` no-fallback run;
- optional live Responses provider run;
- full targeted commands and environment recorded.

## 9. Required implementation examples

These examples define semantics, not mandatory type signatures.

### Example: identity separation

```text
source_digest = sha256(source)
program_id = generated durable ID
invocation_key = hash(session_id, turn_id, model_tool_call_id, delegation_ordinal)
```

A retry with the same `invocation_key` returns the same `program_id`. A new model tool call with identical source receives a new `invocation_key` and `program_id` while reusing the immutable source blob.

### Example: child call identity

```text
program_id = tp-123
attempt_id = ja-2
call_sequence = 7
program_call_id = pc(tp-123, 7)
child_submission_key = hash(program_call_id, op, normalized_config)
```

Replay of call 7 reattaches to the existing child job. A later call 8 with identical operation/configuration creates a different child job.

### Example: crash-safe call ordering

```text
persist Reserved(call_id, input_hash, authority_hash)
dispatch through ToolBroker
persist Completed(call_id, typed_result_ref, artifact_refs)
persist Checkpoint(next_pc, completed_call_cursor)
advance interpreter
```

After restart, a completed record is replayed. A reservation without completion follows the tool contract's explicit ambiguity/retry policy and is never treated as definitely completed.

### Example: durable notification

```text
persist terminal ProgramResult
upsert PendingNotification(program_id, parent_session_id, result_ref)
claim notification with lease
append bounded notification message to durable session/turn input
acknowledge Delivered(notification_id)
```

Restart after durable message append but before acknowledgement must reconcile using message/notification identity and acknowledge without injecting a duplicate.

### Example: deadline narrowing

```text
program_deadline = min(parent_deadline, submitted_at + requested_timeout)
call_deadline = min(program_deadline, now + tool_contract_timeout)
child_deadline = min(program_deadline, now + child_requested_timeout)
```

No nested operation may extend the parent deadline.

## 10. Verification commands

Use repository-standard commands and test-thread constraints. At minimum record outcomes for:

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings

cargo test -p codegg --test tool_broker_integration -- --test-threads=1
cargo test -p codegg --test tool_program_runtime -- --test-threads=1
cargo test -p codegg --test tool_program_recovery -- --test-threads=1
cargo test -p codegg --test tool_program_fault_injection -- --test-threads=1
cargo test -p codegg --test tool_program_child_jobs -- --test-threads=1
cargo test -p codegg --test tool_program_child_recovery -- --test-threads=1
cargo test -p codegg --test tool_program_background -- --test-threads=1
cargo test -p codegg --test tool_program_notifications -- --test-threads=1
cargo test -p codegg --test tool_program_lifecycle -- --test-threads=1
cargo test -p codegg --test hosted_tool_program_adapter -- --test-threads=1
cargo test -p codegg --test hosted_tool_program_recovery -- --test-threads=1
cargo test -p codegg --test hosted_tool_program_security -- --test-threads=1
cargo test -p codegg --test hosted_tool_program_contention -- --test-threads=1
cargo test -p codegg --test hosted_tool_program_equivalence -- --test-threads=1

python3 scripts/e2e/tool_program_harness.py --mode native
python3 scripts/e2e/tool_program_harness.py --mode scripted --scenario all
```

Add and record the new M011 process-restart, Broker ownership, notification durability, identity, child cancellation, timeout/heartbeat, artifact, and hosted-selection suites.

Operational evidence when credentials are supplied:

```bash
python3 scripts/e2e/tool_program_harness.py \
  --mode eggpool \
  --model mimo-v2.5 \
  --no-model-fallback
```

Live provider runs must be reported separately from deterministic closure evidence and must never print credentials, private endpoints, or captured sensitive response bodies.

## 11. Acceptance criteria

Milestone 011 is accepted only when every criterion below is evidenced.

### Identity and lineage

- One deliberate invocation equals one durable generated program identity.
- Source identity is reusable without merging logical runs.
- Retry/deduplication and deliberate repetition are distinguishable.
- Session, turn, agent, workspace, parent job/attempt/call, authority, and backend lineage are retained where applicable.
- Cross-session and cross-workspace access is denied and tested.

### Broker and authority

- Every production direct and programmatic call enters the same Tool Broker boundary.
- Input schema, output schema, caller policy, effect/idempotency, authority, path policy, deadline, cancellation, artifact, provenance, and terminal status are enforced.
- No boolean authorization bypass grants program authority.
- Mutating/direct-only tools remain unavailable to programs.

### Recovery and bounded execution

- Calls are durably reserved and completed at crash-safe boundaries.
- Daemon restart does not repeat a durably completed call.
- Replay divergence is detected and inspectable.
- Program timeout is enforced by scheduler/executor/interpreter consistently.
- Durable heartbeat proves progress.
- Lost workers, stalls, cancellation, timeout, and persistence failures converge to one terminal/recoverable state.

### Child jobs

- Child idempotency is call-identity-based.
- Deliberate identical child calls remain distinct.
- Replay reattaches rather than resubmits.
- Cancellation and deadlines propagate.
- Capacity-one scheduling cannot deadlock.
- Process groups, permits, leases, jobs, and artifacts converge.

### Notifications

- Terminal notifications are durable and parent-session-addressed.
- Claim, injection, acknowledgement, suppression, expiry, and restart recovery are persisted.
- Exactly one logical notification is delivered.
- Delivered notifications are not recreated after restart.
- No manual polling is required.

### Results, artifacts, and projections

- Foreground/background/native/hosted/public inspection consume one typed result record.
- Semantic counters are not parsed from summaries.
- Artifact handles resolve and preserve evidence.
- Public DTOs and transcript content remain bounded and redacted.
- Projection replay and fresh snapshot converge.

### Hosted integration

- Hosted backend selection is reachable through normal production runtime construction.
- Capability and fallback policies are explicit and tested.
- Hosted nested calls use native Broker/authority/ledger/artifact/cancellation semantics.
- Hosted-required unsupported configurations fail closed.

### Verification and governance

- All M011-owned tests, static guards, formatting, compilation, and clippy checks pass.
- Process-level restart and notification scenarios use public production interfaces.
- Mixed fault injection at or above 10 percent converges without leaks or indefinite blocking.
- No unresolved high or medium finding remains.
- `plans/closure/tool-programs/011-status.md`, roadmap addendum, architecture docs, and registry agree.
- Live Eggpool/ACP/provider limitations are recorded truthfully and do not masquerade as deterministic evidence.

## 12. Handoff instructions

The implementing agent must:

1. read the canonical roadmap, corrective addendum, ADR, M005/M007/M008/M009/M010 implementation and closure records, and current architecture documents before editing;
2. inspect current production call sites and storage schema rather than relying on historical closure summaries;
3. preserve the existing restricted-Python safety model and read-only palette;
4. implement domain/storage ownership before adding compatibility shims;
5. land work in coherent commits with tests for each crash/ownership boundary;
6. keep migrations additive and restart-safe;
7. use public daemon interfaces for closure-bearing integration tests;
8. record exact commands, seeds, repetitions, environments, skips, and failures;
9. create a closure record only after the complete acceptance matrix is satisfied;
10. leave the plan `active` or `closing` rather than claiming closure when evidence is incomplete.

Suggested commit decomposition:

1. identity/context domain and migration;
2. canonical Broker migration and enforcement;
3. durable call/checkpoint/replay wiring;
4. scheduler timeout/heartbeat/watchdog;
5. child ownership/cancellation/resource correction;
6. durable notifications and AgentLoop delivery;
7. typed result/artifact/projection convergence;
8. hosted runtime selection;
9. mechanism-faithful tests and static guards;
10. documentation and closure reconciliation.

## 13. Closure artifact

After implementation, create:

- `plans/closure/tool-programs/011-status.md`

The closure record must contain:

- exact implementation and reviewed commit hashes;
- requirement-to-evidence matrix for every finding and acceptance criterion;
- migration and compatibility evidence;
- direct/programmatic/child/hosted ownership review;
- process-level restart and crash-boundary results;
- cancellation, timeout, heartbeat, and resource convergence results;
- notification exactly-once evidence across restart boundaries;
- security and redaction review;
- static guard output;
- full commands, seeds, repetitions, environment, pass/fail/skip results;
- remaining findings with severity and owning follow-up;
- explicit statement that historical closure records remain traceability artifacts;
- roadmap/addendum and registry disposition.

Strict closure is prohibited while any high or medium finding remains.
