# Session Projections Milestone 001 — Canonical Projection Contracts

Status: blocked

Repository baseline: `fbae374a2cd6172505204b1bc1bee1ef247afd5f` (production-code baseline; subsequent planning-only commits do not alter implementation state)

Source roadmap:

- `plans/subsystems/session-projections-roadmap.md#milestone-1--projection-contracts-and-canonical-reducer`

Long-term requirements:

- `plans/000-long-term-specification.md#8-read-only-session-observation`
- `plans/000-long-term-specification.md#14-acp-integration`
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md#phase-5--frontend-neutral-session-projections-and-durable-replay`

Applicable ADRs:

- None. Stop if the milestone requires redefining the authoritative event-log owner or selecting a new durable replay backend; those decisions belong to the next milestone and may require an ADR.

Primary class: infrastructure

## 1. Objective

Define versioned, bounded, frontend-neutral session projection DTOs and one deterministic canonical reducer that can reconstruct equivalent logical session state from existing snapshots/events. Map current turn, message, tool, permission, question, run, job, workspace, and placeholder agent-tree state into this contract without implementing durable replay storage or team authorization yet.

## 2. Why this milestone is blocked

Hard dependencies:

- Project Catalog protocol/server migration must provide project-scoped operation and subscription context.
- Multi-Project TUI state foundation must establish project/session routing without frontend-global assumptions.

The Domain Identity daemon/protocol dependency is closed by
`plans/closure/domain-identity/003-corrective-status.md`; this plan must consume
that stable identity contract and must not recreate a path-derived authority.

The agent must not build projections around legacy path IDs, one global project directory, or one privileged current-session TUI state.

## 3. Current implementation evidence

- `crates/codegg-protocol/src/core.rs` already defines versioned request/event envelopes with event sequence, timestamps, optional session/turn IDs, session and daemon snapshots, active-session/client summaries, and many core events.
- Existing events cover turn start/text/reasoning/completion/failure, tool start/completion, permissions, questions, file changes, subagent lifecycle, test/run/artifact/projection/job events.
- `src/server/ws.rs` and server architecture provide structured remote TUI messages, an in-memory bounded replay buffer, snapshot/resume/resync paths, and explicitly reject raw `RenderFrame` as canonical behavior.
- `architecture/tui.md` documents local remote-snapshot sequencing that frequently returns a fresh snapshot because no authoritative per-session durable replay exists.
- Existing `RunStore` and artifact handles provide a seam for keeping large output out of projection payloads.

Known gaps: no single `SessionProjectionSnapshot`, no shared reducer, inconsistent payload bounds/sensitivity, no projection capability version, and frontend-specific interpretation of events.

## 4. Invariants that must not regress

- Projection state is a derived frontend contract, not a second session execution authority.
- Two compliant reducers given the same snapshot/events produce equivalent state.
- Payloads are bounded; large bodies/logs/artifacts remain behind handles or summaries.
- Raw render frames are not introduced as canonical state.
- Unknown optional variants/fields degrade safely according to negotiated capabilities.
- Provider-private hidden reasoning is not exposed; only explicit protocol content is projected.
- Secret-bearing fields must have a safe classification/redaction seam even though full policy lands later.
- Existing core event transport and current clients remain compatible during introduction.

## 5. Scope

### In scope

- Versioned DTOs for:
  - `SessionProjectionSnapshot`;
  - session/project/workspace summaries;
  - `TurnProjection`;
  - bounded message/content projection;
  - tool call/result status projection;
  - permission/question pending/resolution summaries;
  - run/test/job status projection;
  - artifact handles/summaries;
  - selected model/agent/token summaries;
  - agent-tree placeholder/reference structure using stable IDs where available;
  - projection diagnostics/resync metadata.
- A canonical reducer/projector library independent of Ratatui/web/ACP.
- Mapping adapters from current `CoreResponse` snapshots and `CoreEvent` variants.
- Projection version/capability declaration and compatibility rules.
- Explicit payload/count/string limits and truncation/handle behavior.
- Golden fixtures and at least two independent reducer-consumer test paths.
- Documentation.

### Explicitly out of scope

- Durable replay database/index/checkpoints.
- Subscription registry, acknowledgements, cursor persistence, or retention.
- Final visibility/authorization policy.
- Presence, observer UX, chat, ACP adapter implementation, or web frontend.
- Exposing hidden chain-of-thought.
- Full durable agent-run tree semantics.
- Migrating all current TUI rendering to the new projection in this milestone.

## 6. Required production changes

### Core/domain

Keep projection types in `codegg-protocol` or a dependency-safe projection crate/module usable by clients and server. Domain execution records remain owned by core/session/run/job modules; projection types contain bounded summaries and references.

Define stable projection IDs/keys for nested items where current events have them. Avoid synthesizing unstable IDs from display text. For missing future agent-run IDs, use explicit optional/legacy references rather than fake durable identity.

### Storage and migrations

No production migration or durable replay table. Golden fixtures may serialize projection snapshots/events to test version stability. Artifact references reuse existing stores/interfaces where possible.

### Protocol and DTOs

Add projection version and capability declarations to initialization/capability negotiation. Introduce additive projection snapshot/event envelopes rather than replacing existing events immediately. Define:

- version number and compatible range behavior;
- stream/project/session scope fields;
- sequence placeholder semantics without claiming durable replay yet;
- payload and collection bounds;
- unknown variant behavior;
- safe error/resync representation.

### Runtime and concurrency

Implement a pure or deterministically stateful reducer:

- applies a bounded snapshot;
- applies ordered events idempotently where duplicates are detectable;
- rejects or flags impossible identity/sequence mismatches;
- updates only matching project/session state;
- never performs I/O, network, filesystem, or provider calls;
- exposes immutable or controlled state snapshots to clients.

Projection construction from daemon state may read existing stores/services through bounded queries, but the reducer itself remains frontend-neutral and deterministic.

### Frontend or operator surface

Provide a reference test adapter and a minimal TUI compatibility adapter or diagnostic path sufficient to prove the contract is consumable. Do not force full rendering migration.

### Security and authorization

- Define typed content classifications or visibility placeholders on projection fields/events.
- Ensure obvious credential fields are absent from DTOs.
- Bound strings, maps, arrays, tool arguments/results, and diagnostics.
- Mark raw/possibly sensitive tool payloads for later redaction and prefer safe summary/handle construction now.
- Do not project environment variables, secret-store data, provider auth config, or raw config.

### Documentation and static guards

Add projection architecture, versioning, reducer rules, bounds, compatibility matrix, and explicit non-goals. Add tests/guards preventing raw render-frame or unbounded artifact embedding in canonical projection DTOs.

## 7. Ordered work packages

### Work package A — Event/state inventory and projection schema

Intent: define one complete bounded logical state model before coding reducers.

Required changes:

- inventory current session/daemon snapshots and core events;
- classify each as projected, summarized, handle-only, deferred, or private;
- define DTOs and limits;
- define version/capability and unknown-field rules;
- define agent-tree placeholder semantics.

Acceptance evidence:

- event-to-projection mapping matrix;
- schema examples for idle, active turn, tool call, permission, run/job, and completed session;
- no raw secret/config/render-frame fields.

### Work package B — Canonical reducer

Intent: implement deterministic state transitions shared across frontends.

Required changes:

- reducer state and apply APIs;
- identity/scope checks;
- lifecycle transitions for turns/tools/questions/permissions/runs/jobs;
- duplicate/idempotence handling where event identity allows;
- bounded pruning/summary behavior;
- explicit diagnostics for impossible/out-of-order inputs.

Acceptance evidence:

- unit tests for every mapped event family;
- same inputs produce equivalent serialized state across repeated runs;
- unrelated session/project events are ignored or rejected by contract.

### Work package C — Snapshot and current-event adapters

Intent: bridge existing daemon protocol state into the new contract without replacing it.

Required changes:

- map current session/daemon snapshots to projection snapshot;
- map current `CoreEvent` variants to projection events or documented no-op/deferred outcomes;
- create safe artifact/tool/run summaries;
- preserve existing clients and capability negotiation;
- expose test/reference builder APIs.

Acceptance evidence:

- current protocol fixtures convert successfully;
- unsupported/deferred events are explicit;
- large outputs truncate or become handles according to limits.

### Work package D — Independent consumer fixtures and docs

Intent: prove frontend neutrality before durable replay work.

Required changes:

- build two independent consumers, such as reducer library tests plus a minimal fake TUI/web/CLI projection consumer;
- add golden snapshot/event fixtures;
- document versioning, limits, and migration path;
- add performance benchmarks or bounded stress tests for large sessions.

Acceptance evidence:

- both consumers produce equivalent logical state;
- fixture compatibility is reviewable;
- reducer memory/state remains bounded under configured stress.

## 8. Failure, cancellation, restart, and contention semantics

- Reducer application is atomic per event: invalid input leaves prior state unchanged and returns a typed diagnostic/error.
- Duplicate detectable events do not duplicate tool/run/message state.
- Events for another project/session do not mutate the target projection.
- Out-of-order or impossible lifecycle transitions are diagnosed; do not silently invent missing starts/completions unless the contract explicitly defines reconciliation.
- Projection building cancellation returns no partially published snapshot.
- Restart behavior in this milestone relies on rebuilding from current snapshots/messages; durable event replay is deferred.
- Concurrent readers may share immutable projection snapshots; one controlled writer/reducer applies ordered events per projection instance.

## 9. Compatibility and migration

- Existing `CoreEvent` and remote TUI paths remain available.
- New projection capability is additive and negotiated.
- Unknown optional fields/variants are ignored only when safe; required version mismatch produces explicit resync/unsupported behavior.
- No durable storage migration.
- Document how later milestones will migrate local/remote TUI and ACP without breaking older clients.
- Do not claim sequence resume durability until Milestone 2.

## 10. Required tests

### Focused unit tests

- DTO serde/version fixtures;
- reducer transitions for every event family;
- duplicate/idempotent application;
- identity/scope mismatch;
- payload truncation/handle conversion;
- unknown variant/version behavior.

### Integration tests

- current core snapshot/event adapters;
- two independent consumer equivalence;
- active session with interleaved tool/run/job/question events;
- several sessions/projects remain isolated.

### Restart and recovery tests

- rebuild projection from current durable session/messages/snapshots yields equivalent bounded state;
- incompatible fixture version produces explicit behavior.

### Contention and cancellation tests

- concurrent readers during event application;
- cancellation during snapshot build;
- bounded stress with many events/tools/runs.

### Security and negative tests

- credential/config/environment fields absent;
- oversized strings/arrays/maps handled safely;
- binary/large output becomes summary/handle;
- raw render frame cannot enter canonical DTO;
- hidden provider reasoning is not mapped.

### Migration and compatibility tests

- existing protocol tests remain green;
- existing remote TUI messages still serialize;
- projection capability absent/present negotiation;
- golden fixtures are stable or intentionally versioned.

## 11. Required verification commands

```bash
cargo fmt --all -- --check
cargo test -p codegg-protocol
cargo test protocol
cargo test server::ws
cargo test tui::
cargo test --test core_transport
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

Add focused projection reducer and equivalence test targets and run them explicitly before broad filters.

## 12. Documentation updates

- new session projection architecture document;
- `architecture/protocol.md` version/capability and mapping notes;
- `architecture/tui.md` compatibility-adapter seam;
- server/remote TUI documentation clarifying that durable replay is deferred;
- projection bounds and security classification documentation.

## 13. Acceptance criteria

- One versioned bounded projection schema represents current logical session state.
- One canonical deterministic reducer handles mapped snapshot/event inputs.
- At least two independent consumers produce equivalent logical state.
- Current protocol clients remain compatible.
- Large/sensitive data is omitted, summarized, classified, or represented by handles.
- No raw render-frame or hidden-reasoning dependency is introduced.
- The next milestone can add durable scoped replay without redesigning projection state.

## 14. Stop conditions

The agent must stop and report rather than improvise when:

- stable project/session identity or project-aware routing is unavailable;
- completing the contract requires selecting a durable replay backend;
- current event semantics are contradictory and need an ADR or core ownership change;
- a frontend-specific render model becomes necessary;
- security requires exposing raw secrets/tool outputs to preserve behavior;
- scope expands into authorization, presence, observer UI, ACP, or full agent hierarchy.

## 15. Closure evidence required

- implementation commit(s);
- event-to-projection mapping matrix;
- schema/version/bounds documentation;
- reducer transition and equivalence test results;
- security/negative evidence;
- compatibility evidence for current protocol/remote TUI;
- exact verification commands/results;
- deferred replay/redaction/authorization/frontend migration list;
- closure recommendation.

## 16. Handoff notes

- This plan remains blocked until the catalog and TUI state dependencies close.
- Keep the reducer I/O-free and frontend-neutral.
- Do not label the existing in-memory remote TUI buffer as durable replay.
- Avoid overfitting DTOs to Ratatui layout or current web/server route shapes.
- Inspect current `main` before implementation and record the actual production baseline in closure.
