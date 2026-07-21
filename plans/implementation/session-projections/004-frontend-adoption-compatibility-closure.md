# Session Projections Milestone 004 — Frontend Adoption, Compatibility, and Closure

Status: blocked

Repository baseline: `f569386e4cb68d9752505c3b8d4205161a40c3c4` (`main`; planning-only commits after this baseline do not alter production behavior)

Activation criteria:

- Session Projections Milestone 003 must be strictly closed;
- Multi-Project TUI Milestone 003 must provide project/session-correct routing and active-view lifecycle seams;
- the corrective Session Projections Milestone 002 integration must remain strictly closed and regression-clean.

Source roadmap:

- `plans/subsystems/session-projections-roadmap.md#milestone-4--frontend-adoption-and-closure`

Primary class: capability / migration / closure

## 1. Objective

Migrate local and remote frontend session-state consumption to the canonical `SessionProjectionSnapshot` plus scoped durable replay, prove equivalent logical behavior in a second independent client, preserve bounded compatibility for older clients/daemons, and close the frontend-neutral session projections roadmap.

The milestone must eliminate bespoke frontend interpretation of raw core-event families as the primary state authority. Raw `CoreEvent` remains an additive compatibility and operational channel, but canonical session rendering, reconnect, resync, and inactive-tab summaries use the projection contract and deterministic reducer.

The milestone succeeds when:

- local TUI and remote TUI consume the same projection snapshot/event/replay contract;
- reconnect resumes from a cursor or performs explicit bounded resync;
- two independent clients produce equivalent logical state from production fixtures;
- multi-project tab switching remains project/session correct;
- incompatible or older peers receive explicit fallback/unsupported behavior;
- projection CPU, memory, replay, queue, snapshot, and render costs remain bounded;
- Phase 5 roadmap exit criteria have closure evidence.

## 2. Dependency assumptions

This plan assumes:

- M1 projection DTOs/reducer/adapters are closed;
- M2 durable replay, dispatch, live delivery, retention, restart, binding revision, and subscription ownership are strictly closed;
- M3 disclosure policy and artifact handles are closed;
- Multi-Project TUI M3 has one tab model, explicit route tokens, active/inactive reducers, and one controlled heavy active view;
- current TUI M2 picker/tab navigation remains stable;
- existing raw-core clients and protocol version negotiation remain supported during migration.

## 3. Current implementation evidence and gaps

At baseline `f569386`:

- projection M1 types and reducer exist in `codegg_protocol::projection`;
- projection M2 library code exists, but corrective daemon integration is separately planned;
- TUI M2 uses one global heavy `session_state` / `agent_state` compatibility surface;
- local TUI and remote TUI primarily consume `CoreResponse::SnapshotSession`, raw `CoreEvent`, and UI-specific state messages;
- `TuiMessage::StateSnapshot` and related remote-TUI paths are not yet canonical projection transport;
- the second independent consumer is a fixture/test consumer, not a complete transport/reconnect reference client;
- capability negotiation does not yet drive an end-to-end projection-primary/fallback state machine;
- frontend artifact expansion is not wired through projection handles;
- no final performance/compatibility/closure matrix exists.

## 4. Invariants

- `SessionProjectionSnapshot` plus ordered projection events is the canonical frontend session-state contract.
- `ProjectionReducer` remains the only canonical reducer; frontends do not fork semantics.
- The daemon remains execution/session authority; frontend projections are derived caches.
- Project/tab identity remains separate from stream/session identity.
- A frontend subscribes only to scopes it is authorized to observe.
- Redaction/visibility decisions are not reimplemented by frontends.
- Artifact content remains behind authorized bounded handles.
- Cursor/replay state is stream-scoped and cannot be reused against another project/session.
- Resync replaces projection state atomically; it does not merge incompatible snapshots.
- Local and remote TUI use the same logical adapter/reducer boundary.
- Older daemons/clients remain bounded through explicit fallback or unsupported diagnostics; no silent protocol reinterpretation.
- Raw core events may continue for operational/global compatibility but cannot mutate projection-primary session state through an independent reducer.
- Inactive tabs retain bounded projection summaries/checkpoints only, not full unbounded histories.
- Exactly one heavy active view remains unless a later roadmap explicitly changes that invariant.
- No raw terminal frames become canonical protocol state.
- No hidden reasoning, secrets, or unauthorized artifacts are exposed during compatibility fallback.

## 5. Scope

### In scope

- Shared frontend projection client/controller abstraction.
- Projection capability negotiation and mode selection.
- Subscribe, initial snapshot, live reduce, ack, resume, resync, unsubscribe, and reconnect lifecycle.
- Local TUI integration.
- Remote TUI/socket/server integration.
- Multi-project tab routing integration.
- Active-session heavy view derived from projection state.
- Inactive-tab bounded projection summaries/activity.
- Permission/question/run/job/tool/subagent/file-change presentation migration.
- Artifact-handle expansion through bounded APIs.
- Raw-core compatibility/fallback adapter with explicit limitations.
- Migration/deprecation of UI-specific snapshot/event paths where safe.
- A second independent reference client or headless observer/test client.
- Golden production fixtures and cross-client equivalence tests.
- Performance, resource, reconnect, compatibility, security, and closure testing.
- Protocol/TUI/client/server/projection/operations documentation.

### Explicitly out of scope

- Presence, chat, team observer UX, final roles, or organization policy.
- Cross-daemon merged event ordering.
- Replacing execution/session/message stores.
- Replacing final audit logs.
- Exposing provider-private reasoning.
- Multiple simultaneous heavy TUI views.
- Arbitrary artifact/file browsing.
- A production web UI beyond a bounded reference consumer if used for equivalence.
- ACP feature-complete implementation; only a reusable adapter/reference seam is required.

## 6. Frontend projection controller

Create a transport-neutral controller, for example:

```text
ProjectionClientController
|-- negotiated_capabilities
|-- mode: ProjectionPrimary | RawCompatibility | Unsupported
|-- subscriptions_by_scope
|-- reducer_state_by_stream
|-- cursor/ack state
|-- reconnect_epoch
|-- resync state
|-- bounded diagnostics/metrics
`-- artifact client
```

Responsibilities:

- negotiate projection version/capabilities;
- request session/project subscription;
- validate descriptor/scope/binding revision;
- atomically install initial snapshot;
- apply ordered events through `ProjectionReducer`;
- acknowledge applied cursors with bounded cadence;
- handle duplicates, gaps, lag, expiry, version mismatch, binding mismatch, capability change, and restart;
- unsubscribe/cleanup on scope removal;
- expose immutable/bounded view models to frontends;
- never perform daemon storage or execution work.

Keep transport adapters thin. Socket, in-process, stdio, and server clients should all feed the same controller inputs.

## 7. Capability and mode negotiation

Define an explicit state machine:

1. initialize base protocol;
2. inspect projection capability/version/limits;
3. inspect visibility/artifact capabilities;
4. select `ProjectionPrimary` only when required versions and operations intersect;
5. otherwise select documented `RawCompatibility` if safe and supported;
6. otherwise enter `Unsupported` with actionable diagnostics.

Do not partially enable projection events while retaining a second raw reducer for the same state.

Record negotiated projection version, replay limits, artifact limits, and fallback reason. Reconnect must renegotiate; a changed capability set invalidates subscriptions and may require full resync.

## 8. TUI integration

### 8.1 State ownership

Add projection state to the Milestone 003 tab/runtime model:

- each open tab may retain a bounded project/session projection summary and cursor metadata;
- the active tab owns one full bounded `SessionProjectionSnapshot` view;
- switching activates or resumes the target stream through the existing route-token/view-epoch transaction;
- inactive live events reduce into bounded summary state or retained bounded snapshot according to documented caps;
- stale stream events cannot mutate another tab.

Avoid maintaining full legacy `SessionState` and full projection state as independent mutable authorities. During migration, legacy render structures should be deterministic views/adapters over projection state or explicitly isolated fallback mode.

### 8.2 Rendering migration

Map canonical projection fields to existing UI components:

- session/title/status/model/agent/token usage;
- active/recent turns and bounded messages;
- tool lifecycle and output summaries/handles;
- permissions/questions;
- subagent tree placeholders;
- runs/jobs/tests;
- file changes/diff handles;
- errors/diagnostics;
- project/workspace summaries.

Unknown projection variants render bounded generic notices. Internal/redacted fields are never requested or reconstructed.

### 8.3 Commands and responses

User actions such as submit/cancel/steer, permission/question response, model/agent selection, run/test/job controls remain explicit daemon commands. Optimistic UI may mark pending intent but canonical state changes only when projection events/snapshots confirm them.

## 9. Remote TUI and server migration

- Replace UI-specific session-state replay with projection subscribe/resume where negotiated.
- Route live events by subscription ownership.
- Preserve raw-core fallback for older peers without exposing new shared-observer capabilities.
- Ensure server/WebSocket adapters do not serialize ratatui cells or raw render frames.
- Reconnect uses client-held cursor and daemon replay; in-memory UI buffers are only transient optimizations.
- Connection loss cancels transient receivers but preserves client cursor intent.
- Authentication/capability context is applied before subscriptions and artifact reads.

## 10. Raw compatibility path

Document a bounded compatibility adapter for peers without projection support:

- use existing `SnapshotSession` and raw events;
- support only existing single-user/local semantics;
- do not claim durable scoped replay equivalence;
- do not enable observer/team/shared projection features;
- enforce existing redaction and payload limits;
- expose a visible compatibility diagnostic;
- do not silently map unknown raw events into invented projection semantics;
- maintain regression tests until the declared support window ends.

Create explicit removal criteria rather than deleting compatibility immediately.

## 11. Reference second client

Build a headless reference consumer or small library-backed test client that:

- negotiates capabilities;
- subscribes to project/session streams;
- applies snapshots/events with `ProjectionReducer`;
- acknowledges and resumes;
- handles resync and artifact metadata/read;
- exports a bounded logical-state digest for comparison;
- contains no TUI-specific state or rendering code.

Use it in end-to-end tests against the production daemon. The client and TUI must produce semantically equivalent digests from the same stream, including reconnect/restart cases.

## 12. Artifact UX integration

- Render handle metadata/summary without fetching content automatically.
- Explicit user action requests a bounded range.
- Show truncation, total bytes if allowed, revision, expiry, and redaction status.
- Paginate/tail with caps; never concatenate unbounded content into session projection state.
- Cancel reads on tab/session switch.
- Reject stale/unauthorized handles with bounded diagnostics.
- Keep fetched excerpts in bounded ephemeral UI caches and clear on close/reconnect according to policy.

## 13. Migration sequence

1. land shared controller and fixture tests;
2. integrate one non-primary/headless consumer;
3. integrate local TUI behind capability/feature gate;
4. compare projection and legacy digests in test/shadow mode without double-render authority;
5. switch local TUI projection-capable path to primary;
6. integrate remote TUI/server path;
7. add artifact expansion;
8. remove/narrow duplicated reducers and UI-specific state messages;
9. run compatibility/performance/security matrix;
10. finalize docs and closure.

Any shadow comparison must avoid persisting or exposing duplicated sensitive content.

## 14. Performance and resource bounds

Define and verify caps for:

- projection snapshots per tab;
- messages/tools/runs/jobs/subagents/diagnostics in reducer state;
- subscriptions per client;
- pending replay batches;
- ack cadence and outstanding lag;
- artifact excerpt caches;
- reconnect attempts/backoff;
- reducer work per event and per resync;
- serialized snapshot/event bytes;
- TUI render/update frequency.

Avoid full-history rebuild on each event. Benchmark large bounded snapshots, replay batches, rapid tab switching, and long-running streams. Record memory and CPU tolerances.

## 15. Work packages

### A — Shared controller and negotiation

- Add controller, modes, subscription/cursor lifecycle, reconnect/resync.
- Add transport adapters and deterministic tests.

### B — Reference client and equivalence fixtures

- Build independent headless consumer.
- Add production daemon end-to-end digest comparison.

### C — Local TUI adoption

- Integrate with tab routing/active-view lifecycle.
- Migrate render/view models and command confirmation semantics.
- Add inactive summary handling.

### D — Remote/server adoption

- Use scoped replay over socket/server paths.
- Preserve explicit compatibility fallback.
- Verify subscription ownership and reconnect.

### E — Artifact UX and compatibility cleanup

- Add bounded handle expansion.
- Remove/narrow duplicate reducers/UI-specific state paths.
- Define compatibility removal criteria.

### F — Performance, security, docs, and closure

- Run stress/restart/reconnect/version/redaction matrix.
- Update architecture/operations/client docs.
- Complete roadmap closure evidence.

## 16. Required tests

- projection capability negotiation selects correct mode;
- version intersection and unsupported versions are explicit;
- initial snapshot installs atomically;
- ordered events reduce correctly;
- duplicate event is idempotent;
- gap/lag/expired/ahead/binding/version mismatch triggers resync;
- daemon restart resumes from retained cursor;
- capability change forces renegotiation/resync;
- Project A subscription/event cannot mutate Project B tab;
- rapid tab switch drops stale stream events and artifact reads;
- active/inactive projection summaries remain bounded;
- local TUI and headless client produce equivalent digests;
- remote TUI and local TUI produce equivalent logical state;
- permission/question ownership remains correct;
- user commands do not mutate canonical state before confirmation event;
- artifact content is fetched only on explicit action and within bounds;
- unauthorized/stale/expired handles fail safely;
- compatibility mode remains functional with older daemon/client fixtures;
- compatibility mode cannot access shared-observer projection features;
- unknown variants render safely;
- raw terminal frames are absent;
- secret/internal/redacted values remain absent in frontend fixtures/logs;
- long-running replay/tab/reconnect stress remains within resource caps;
- current TUI, protocol, replay, redaction, daemon, server, and static-guard suites remain green.

## 17. Acceptance criteria

- Local TUI and remote TUI use the canonical projection controller in projection-capable mode.
- The canonical reducer is the only projection state reducer.
- Scoped durable replay drives reconnect/resume; resync is explicit and atomic.
- Multi-project routing remains identity/epoch correct.
- A second independent production-like client produces equivalent logical state.
- Visibility/redaction/artifact policy is consumed, not duplicated.
- Artifact reads are explicit, bounded, cancellable, and ephemeral in frontends.
- Older peers receive documented safe compatibility or unsupported behavior.
- Duplicate raw/projection mutable authorities are removed or isolated by mode.
- Performance and long-running resource bounds are measured and met.
- All Phase 5/session-projection roadmap exit criteria are evidenced.
- Protocol compatibility matrix, migration/removal criteria, architecture, operations, troubleshooting, and strict closure records are complete.

## 18. Verification commands

At minimum:

```bash
cargo fmt -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
CARGO_BUILD_JOBS=1 cargo check --workspace --all-features
cargo test -p codegg-protocol
cargo test -p codegg-core
cargo test -p codegg --lib tui::
cargo test --test session_projection_consumer
cargo test --test projection_replay_subscription
cargo test --test projection_replay_resume
cargo test --test tui_project_tabs
cargo test --test tui_project_routing
cargo test --test tui --test tui_render
cargo test --test single_daemon_lifecycle
python3 scripts/check_daemon_cwd_usage.py
python3 scripts/check_git_forbidden_patterns.py
bash scripts/check-core-boundary.sh
```

Add focused projection controller, headless client, local/remote equivalence, compatibility-version, artifact UX, and performance/soak targets.

## 19. Roadmap closure

When this plan closes:

- mark the Frontend-Neutral Session Projections roadmap closed;
- record strict closure for Milestones 001–004 and the M2 corrective pass;
- update the central registry and long-term Phase 5 status;
- preserve future observer, ACP, web, presence, and team authorization work as consumers of the closed projection contract rather than extensions that reopen its core semantics.