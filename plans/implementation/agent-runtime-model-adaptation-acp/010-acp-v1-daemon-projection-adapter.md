# Agent Runtime, Model Adaptation, and ACP Milestone 010 — ACP v1 Daemon and Projection Adapter

Status: implemented — closure record: `plans/closure/agent-runtime-model-adaptation-acp/010-status.md`

Repository baseline: `672479726f1c79bbc931d70f084cd1649e8b2ed4`

Source roadmap:

- `plans/subsystems/agent-runtime-model-adaptation-acp-roadmap.md#milestone-010--acp-v1-daemonprojection-adapter`

Long-term requirements:

- `plans/000-long-term-specification.md#1-product-definition`
- `plans/000-long-term-specification.md#4.4-frontends-render-projections`
- `plans/000-long-term-specification.md#23-acp-boundary`
- `plans/000-long-term-specification.md#24-protocol-and-storage-requirements`
- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/002-long-term-roadmap.md#phase-5--frontend-neutral-session-projections-and-durable-replay`

Primary specification:

- ACP v1: `https://agentclientprotocol.com/protocol/v1/`
- ACP Rust library: `https://agentclientprotocol.com/libraries/rust`

Primary class: capability

## 1. Objective

Implement a functional ACP v1 stdio agent entry point, preferably `codegg acp`, as a thin adapter over the existing CodeGG daemon protocol, session lifecycle, permissions, cancellation, event log, and canonical session projections.

The ACP process must not create a second independent `AgentLoop`, provider connection, scheduler, durable session store, or projection reducer. It must attach to the singleton daemon, map ACP requests to native operations, and translate native projection/events back to ACP updates with truthful capability negotiation and strict stdout protocol hygiene.

## 2. Dependencies

Hard dependencies:

- M003 functional nested-agent lineage/cancellation;
- M006 typed stalled/recovery outcomes and cancellation propagation;
- M009 actual context/request convergence so ACP uses the same runtime behavior as native frontends.

Closed external subsystem dependencies:

- frontend-neutral session projections and durable replay;
- native daemon protocol/transports;
- project/session/workspace identity and explicit execution context;
- permission/question lifecycle;
- runtime assets and provider connections.

Implementation must reverify the current stable ACP v1 Rust library and protocol before coding. Draft ACP v2 behavior is out of scope.

## 3. Current implementation evidence

Re-audit:

- the long-term specification requires ACP as an adapter over the native protocol;
- session projections provide bounded snapshots/events, replay, cursors, subscriptions, visibility/redaction, tool/subagent/job/permission/question state, and connection-owned transport semantics;
- current workspace dependencies contain no ACP SDK/runtime;
- no ACP initialize/session lifecycle handlers or stdio transport exist;
- root turn runtime and daemon already provide turn submit, streaming events, cancel, steer, session snapshots, and durable history;
- projection docs identify ACP as a future consumer, not an execution authority.

## 4. Invariants

- Native daemon/session/scheduler/provider/permission/workspace authority remains canonical.
- ACP advertises only implemented capabilities.
- ACP session IDs map deterministically to native sessions; no duplicate durable ACP-only store exists.
- `session/load` replays prior conversation/state as required by ACP; `session/resume` reattaches without replay when supported.
- Cancellation maps from ACP request/session operations to the native turn and descendant tree and returns a valid terminal ACP response/error.
- Projection redaction/visibility remains authoritative; private provider reasoning is never sent through ACP.
- Large output/files/artifacts use existing bounded handles/content APIs or ACP-supported bounded updates.
- Stdout contains newline-delimited ACP JSON-RPC frames only; logs and diagnostics use stderr.
- Unknown/unsupported versions and optional methods fail explicitly.
- Adapter disconnect/close releases subscriptions, request tasks, and transient mapping state without deleting durable native sessions unless explicitly requested.

## 5. Scope

### In scope

- Add the official ACP Rust dependency/runtime if compatible with repository MSRV and dependency policy; otherwise use its schema/runtime crate with the smallest justified wrapper.
- Add `codegg acp` or equivalent discoverable subcommand using stdio transport.
- Implement initialization/version/capability negotiation for ACP v1.
- Implement required session lifecycle:
  - `session/new`;
  - `session/prompt`;
  - `session/cancel` and protocol request cancellation mapping;
  - `session/update` streaming from canonical projections/events.
- Implement supported optional session lifecycle where native capability exists:
  - `session/load` with replay;
  - `session/resume` without replay;
  - `session/close`;
  - `session/list`/delete only if current ACP v1 and native ownership make them correct and capability-negotiated.
- Map absolute ACP cwd/project input to explicit native project/workspace/session binding without process-global cwd mutation.
- Map visible content/update families:
  - user and agent message chunks;
  - tool call start/progress/result/status;
  - plan/todo updates where ACP v1 supports them;
  - permission requests/responses;
  - elicitation/questions only when ACP capability exists;
  - file/diff/terminal/content updates through supported bounded ACP forms;
  - usage/cost/session metadata when supported;
  - terminal stop/failure/stalled/cancel reasons.
- Maintain per-connection transient correlation:
  - ACP request ID;
  - ACP session ID;
  - native project/workspace/session/turn IDs;
  - projection subscription/cursor;
  - active cancellation handle.
- Reuse canonical projection replay/reducer/controller rather than duplicating interpretation.
- Add stdout-purity and process-level interoperability fixtures.
- Document editor configuration for at least one ACP client without editor-specific runtime branches.

### Out of scope

- ACP v2/draft features.
- Network/WebSocket ACP transport.
- Editor-specific extensions or separate Zed/JetBrains implementations.
- Native project catalog, team administration, providers, scheduler, worktrees, chat, or audit over ACP beyond standard editor-agent scope.
- A second agent runtime or local ACP provider credentials.
- Exposing hidden reasoning.
- Automatic installation into editors.

## 6. Required production changes

### Crate/module boundary

Prefer a small adapter crate or dependency-contained module, for example:

```text
crates/codegg-acp/
    src/lib.rs
    src/agent.rs
    src/session_map.rs
    src/projection_map.rs
    src/stdio.rs
```

The adapter may depend on protocol/client crates and a narrow daemon client interface. Core/daemon crates must not depend on ACP presentation types unless an additive neutral mapping type is needed.

### Daemon client ownership

The ACP process should connect to the existing local daemon through the supported native IPC/client path. If in-process mode is used for tests, production semantics must remain equivalent. Do not instantiate a hidden standalone daemon unless existing CLI bootstrap explicitly starts/attaches the singleton daemon under normal rules.

### Session mapping

`session/new`:

- validate absolute cwd according to ACP;
- resolve/register/activate the native project/workspace through native requests;
- refresh/pin runtime assets through normal session creation;
- create native session;
- return ACP session identity plus truthful modes/capabilities.

`session/prompt`:

- convert ACP content blocks to native provider/user content using bounded supported types;
- submit one native turn;
- subscribe before/reliably with the request so early updates are not lost;
- stream mapped projection events until native terminal outcome;
- return ACP stop reason/result.

`session/load`:

- attach to the native durable session;
- request canonical snapshot/replay;
- emit ACP updates in required order;
- transition to live subscription without gap/duplicate using existing replay-to-live semantics.

`session/resume`:

- attach/reconstruct runtime without replaying prior content when ACP semantics require that distinction;
- advertise only if implemented correctly.

`session/close`:

- cancel active prompt/descendants;
- unsubscribe and join adapter tasks;
- release transient mappings;
- preserve durable native session unless ACP operation explicitly deletes it.

### Projection mapping

Create one deterministic mapping from canonical projection events/snapshots to ACP update types. Unknown native events are ignored or summarized only according to explicit mapping rules; they must not break the stream.

Use existing visibility/redaction and artifact handles. Do not remap internal reasoning deltas.

### Permission and question flow

ACP permission requests must correlate to native `PermissionPending` identity and return the client's decision through the native daemon operation. Timeouts, client disconnect, and cancel fail closed according to existing permission policy.

Questions/elicitation are advertised only if both ACP v1 and CodeGG mapping are implemented; otherwise native question use must be unavailable or translated into a supported visible prompt/blocker without claiming elicitation capability.

### Cancellation

Maintain request/session/turn correlation. ACP `$/cancel_request` or session cancellation should:

- signal native turn cancellation;
- cascade through M003 lineage;
- stop pending permission/question waits;
- wait for/observe one native terminal outcome;
- return ACP cancellation error/stop reason according to v1.

Do not terminate the ACP process as the primary cancellation mechanism.

### Stdio hygiene

- one serialized writer owns stdout;
- every frame is UTF-8 JSON-RPC followed by one newline;
- tracing/logging/panic diagnostics use stderr;
- accidental stdout writes are prevented by tests/initialization policy;
- malformed input returns protocol errors without corrupting subsequent framing when recovery is allowed.

## 7. Ordered work packages

### A — Protocol/library verification and mapping contract

- pin/verify ACP v1 library compatible with MSRV;
- inventory required/optional methods/capabilities;
- define ACP-native identity/session/event mapping matrix;
- identify unsupported content/update types and fail/omit policy;
- add protocol fixture scaffolding.

### B — Stdio runtime and initialization

- add CLI subcommand and isolated stdout writer;
- implement initialize/version/capabilities;
- connect/authenticate to native daemon;
- add stdout-purity and malformed-frame tests.

### C — Session new/prompt/update/cancel

- implement project/workspace/session creation and asset refresh through native operations;
- submit prompt and establish projection subscription without event loss;
- map message/tool/plan/usage/terminal updates;
- implement permission round trip;
- implement cancellation correlation and descendant propagation.

### D — Load/resume/close and replay-to-live

- implement load replay from canonical projections;
- implement resume only if semantically distinct/correct;
- implement close cleanup and active-turn cancellation;
- validate replay-to-live continuity and duplicate/gap behavior.

### E — Optional surfaces and documentation

- add list/delete/elicitation/terminal/file operations only when ACP v1 and native semantics are complete;
- configure one reference editor/client;
- document capabilities, limitations, diagnostics, and daemon ownership;
- add adapter architecture document and compatibility matrix.

## 8. Failure, cancellation, restart, and contention semantics

- Daemon unavailable: initialization/session operation returns typed ACP error and stderr diagnostic; no fallback standalone runtime.
- Daemon restart: adapter reconnects/resyncs through native session/projection mechanisms where feasible or returns explicit session error.
- ACP client disconnect: cancel active adapter-owned requests according to policy, unsubscribe, close writer, and join tasks; durable session remains.
- Slow client: outbound updates are bounded/backpressured; large content uses handles/truncation; no unbounded queue.
- Permission client disconnect/timeout fails closed.
- Duplicate prompt/request IDs use ACP/native idempotency or return explicit conflict; they do not create duplicate turns silently.
- Parallel ACP sessions share one daemon but maintain isolated mapping/subscription/cancellation state.
- One session failure does not terminate unrelated ACP sessions on the same adapter process unless transport failure is global.

## 9. Compatibility

- Native TUI/socket/protocol clients remain unchanged.
- Projection DTOs remain canonical; ACP mapping is additive.
- ACP v1 version/capabilities are explicit; future v2 requires a new plan.
- Existing session/project IDs remain native; ACP-facing IDs are mapped without redefining them.
- Editor clients that do not support optional features receive only baseline capabilities.
- MSRV and package/dependency footprint are reviewed; avoid pulling a broad async/server stack already present only for ACP if the official library offers narrower features.

## 10. Required tests

Focused/library:

- initialization/version/capability negotiation;
- identity/session mapping;
- content block conversion and bounds;
- projection event mapping for messages/tools/plans/permissions/usage/terminal states;
- private reasoning omission;
- unknown event compatibility;
- permission decision correlation;
- cancel correlation;
- load versus resume semantics;
- close cleanup;
- JSON-RPC frame/newline correctness.

Process/integration:

- spawn `codegg acp`, send initialize/new/prompt, receive streamed text and completion;
- tool call start/update/completion;
- permission request/response;
- cancel active prompt with nested child and observe terminal cancellation;
- load durable session with replay then live update without gap/duplicate;
- resume without replay when advertised;
- close while prompt active and verify native subscription/task cleanup;
- daemon unavailable/restart behavior;
- two concurrent sessions remain isolated;
- stdout contains only valid ACP frames while logs appear on stderr.

Negative/security:

- relative/escaped cwd rejected or resolved only through native project rules;
- unauthorized/native-denied operation cannot be granted by ACP;
- secret-bearing tool args/output remain redacted/handle-backed;
- hidden/private reasoning absent;
- malformed/oversized frames/content are bounded;
- client cannot answer another session's permission request;
- unsupported optional method returns correct error and is not advertised.

## 11. Verification commands

Adapt names to the final crate/targets:

```bash
cargo fmt --all -- --check
cargo test -p codegg-acp
cargo test --test acp_stdio
cargo test --test session_projection_consumer
cargo test --test projection_transport_real --features server
cargo check --workspace
```

Run one broad local library suite at handoff. Do not add live editor installation, network ACP transport, or a multi-editor CI matrix. A single process-level stdio fixture is mandatory.

## 12. Acceptance criteria

- `codegg acp` is a functional ACP v1 stdio agent.
- Initialize/new/prompt/update/cancel work with truthful capabilities.
- Load/replay and resume/close work only when advertised and preserve native semantics.
- ACP uses the singleton daemon and canonical session projections.
- Tool, permission, plan, usage, and terminal outcomes map correctly.
- Cancellation reaches nested descendants.
- Stdout is protocol-pure and queues/tasks are bounded/owned.
- Private reasoning and secrets are not exposed.
- Native clients remain unaffected.

## 13. Stop conditions

Stop if:

- the current official ACP Rust library is incompatible with MSRV and no bounded schema/runtime integration exists;
- correct ACP behavior requires a second session/agent authority;
- projection replay cannot supply load/replay semantics without reopening its closed ownership contract;
- permission/question identity cannot be correlated safely;
- cancellation cannot reach native descendant trees;
- stdout purity cannot be guaranteed with current CLI/log initialization;
- scope expands into ACP v2, network transport, or editor-specific extensions.

## 14. Closure evidence

Include:

- ACP version/library pin and capability matrix;
- ACP-to-native operation/event mapping table;
- process-level stdio transcript for initialize/new/prompt/tool/permission/completion;
- cancel-with-descendants evidence;
- load/replay-to-live and resume/close evidence;
- stdout/stderr purity evidence;
- resource/subscription cleanup evidence;
- negative disclosure/authorization results;
- focused and broad local verification results;
- known unsupported ACP v1 optional capabilities;
- closure recommendation.
