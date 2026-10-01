# Desktop Frontend and IDE Foundation Milestone 004 — Desktop Session Control-Plane Vertical Slice

Status: blocked

Repository baseline: `f388689094866fe4ab8b1bf1dafa680516805ad2`

Source roadmap:

- `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md#milestone-004--desktop-sessioncontrol-plane-vertical-slice`

Long-term requirements:

- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#13-multi-project-and-multi-session-tui`
- `plans/000-long-term-specification.md#15-read-only-session-observation`
- `plans/000-long-term-specification.md#24-protocol-and-storage-requirements`
- `plans/002-long-term-roadmap.md#phase-5--frontend-neutral-session-projections-and-durable-replay`

Applicable ADRs:

- `plans/adrs/ADR-0010-desktop-frontend-and-shared-client-boundary.md`
- `plans/adrs/ADR-0006-project-channel-chat-access-policy.md`
- `plans/adrs/ADR-0007-shared-session-control-ownership.md`

Primary class: capability

Hard dependency:

- M003 Tauri desktop shell and secure bridge must close first.

Operational dependency:

- M002 portable local transport must close before Windows-specific M004 qualification can be claimed.

## 1. Objective

Prove the desktop architecture with one bounded, useful end-to-end session/control-plane slice: select an authorized project/workspace, list/attach/create sessions, consume the canonical session projection with replay/resync, submit a prompt, render visible turn/tool/job/subagent activity, and answer the caller-authorized permission/question prompts through the existing daemon authority.

This milestone is the first closure boundary for the desktop foundation. It deliberately stops before editor buffers, repository browsing, graphical PTYs, or general IDE functionality.

## 2. Why this milestone is blocked

M004 depends on M003's independently tooled Tauri host, least-privilege capability configuration, typed bridge, and `codegg-client` connection ownership. Implementing the session slice before that shell closes would couple product behavior to an unqualified bridge.

The daemon/session/projection dependencies themselves are already closed and ready.

## 3. Current implementation evidence

The backend already contains most of the required domain capability:

- project catalog and explicit project/workspace identity;
- daemon-owned session list/create/selection and turn submission;
- frontend-neutral `SessionProjectionSnapshot`, `ProjectionEvent`, replay, subscription, cursor, acknowledgement, resync, artifact handles, and `ProjectionClientController`;
- permission/question requests carrying session/turn context and controller ownership policy;
- durable jobs/runs/subagent tree projections;
- project chat/presence as separate daemon-owned domains;
- current TUI async routing/stale-response patterns that provide useful failure cases but are not reusable presentation code.

The desktop should consume those contracts directly through the shared client. It must not translate the remote-TUI `TuiMessage` protocol or mirror TUI key/mouse/render state.

## 4. Invariants that must not regress

- Project/workspace/session identity is explicit and never inferred from renderer process cwd.
- The desktop does not become a durable session/message store.
- Session projection state is produced by the canonical Rust projection controller/reducer.
- The renderer never independently reorders/reduces CoreFrame projection events into a competing authoritative state machine.
- Prompt submission is at-most-once from one visible frontend intent despite async session creation/reconnect.
- Permission/question responses are authorized by the daemon and tied to the active controller/turn; the GUI cannot answer another caller's prompt merely because it knows an id.
- Observer/read-only state remains fail-closed.
- Large logs/diffs/artifacts stay behind bounded handles/on-demand reads.
- Stale async responses from an old project/session/connection generation cannot mutate the active desktop view.
- Desktop disconnect/reload does not cancel daemon-owned work.
- Concurrent TUI and desktop connections do not corrupt session projection or control ownership.

## 5. Scope

### In scope

- Bounded project selector sourced from daemon project catalog.
- Workspace selection/summary where a project has multiple workspaces.
- Bounded session list.
- Attach/open existing session.
- Create session.
- Select model/provider only through already-authorized existing session-selection/provider surfaces where required for session creation; do not build full provider administration.
- Projection capability negotiation.
- Session-scoped projection subscribe/snapshot/replay/resume/ack/unsubscribe.
- Visible messages/turn state/tool summaries/job/run/subagent summaries from the canonical projection.
- Prompt submit.
- Streaming visible text/progress update path.
- Pending permission UI and response.
- Pending question UI and response.
- Resync/reconnect/daemon-restart states.
- Renderer stale-generation protection.
- Minimal artifact/detail fetch sufficient to inspect a truncated projection reference, if required by the projection contract.
- End-to-end Tauri/WebDriver smoke.
- Concurrent TUI + desktop integration scenario.
- Foundation closure documentation.

### Explicitly out of scope

- Monaco or editable file buffers.
- File explorer/repository filesystem browsing.
- Direct file write/edit.
- Graphical integrated terminal.
- Git mutation UI.
- full project chat/team/presence administration UI unless a minimal read-only indicator is needed for shell consistency.
- provider credential entry/rotation.
- plugin UI parity.
- task/work-order authoring parity.
- audit UI.
- remote `/core` networking.
- web/mobile.
- editor/LSP document overlays.
- desktop release signing/updater.

## 6. Required production changes

### Core/domain

Prefer no daemon-domain changes.

If M004 discovers missing information in the canonical projection that every rich frontend needs, add it to the frontend-neutral protocol through the existing additive/versioned projection rules. Do not add a desktop-only state endpoint simply because the TUI currently computes something locally.

Any new mutation must map to an existing daemon-owned operation/capability family. If a genuinely new authority is needed, stop and re-plan.

### Client-side projection driver

M004 may add a small reusable `codegg-client` projection driver around `ProjectionClientController` if M001 intentionally left I/O orchestration thin.

That driver may own:

- capability request;
- subscribe/resume;
- cursor/ack cadence;
- reconnect epoch;
- unsubscribe;
- resync request/replace;
- immutable snapshot publication to frontend adapters.

It must not own UI tabs, React state, project picker state, modal focus, or daemon persistence.

The canonical reducer remains the only reducer.

### Desktop Rust host state

Introduce a bounded desktop session controller keyed by explicit:

- connection generation;
- project id;
- workspace id;
- session id;
- projection stream/subscription id;
- renderer subscription generation.

It owns the Rust-side projection controller/driver and emits immutable presentation snapshots/deltas across the desktop channel.

The renderer may keep view-local selection/scroll/draft state but must not be the sole owner of projection cursor/control identity.

### Bridge surface

Extend the M003 bridge with narrow operations, likely including equivalents of:

- project list/select;
- workspace list/select;
- session list/open/create;
- session projection start/stop;
- prompt submit;
- permission response;
- question response;
- artifact excerpt read if needed.

Exact command names may differ.

Each bridge request carries only required locators and user input. Principal/client identity remains Rust/transport-derived.

No generic CoreRequest passthrough.

### Presentation model

Create compact desktop presentation DTOs that are derived from canonical projections.

At minimum represent:

- connection/project/workspace/session identity;
- bounded visible message list;
- active turn coarse status;
- streamed assistant text;
- recent tool summaries and success/failure;
- job/run/subagent summary sufficient to show agent activity;
- pending permissions;
- pending questions;
- projection lag/resync/reconnect state;
- truncation/artifact handles.

Do not expose hidden provider reasoning as a product feature. Follow existing projection visibility rules.

### Prompt submission

Use explicit state for a pending submit:

```text
renderer intent id
project/workspace/session route token
connection generation
session-create generation if needed
text digest/identity
submission state
```

If no session exists, create it asynchronously then submit exactly once only if the captured route remains current.

Renderer double-submit/Enter repetition must be coalesced or produce distinct explicit user intents, never accidental duplicate turns.

### Permission/question handling

Render only pending items present in the authorized projection/response path.

Responses must include only the daemon-required id/choice/answers and explicit session/turn routing data already required by the native protocol. The host must not synthesize a LocalOwner principal in the payload.

On controller loss/revocation/stale turn, clear the stale UI and surface the daemon's bounded denial/resync state.

### Reconnect/resync

On transport reconnect:

1. increment connection generation;
2. renegotiate capabilities;
3. mark old subscriptions stale;
4. resume from valid cursor when the protocol permits;
5. otherwise install a fresh snapshot atomically;
6. re-fetch pending permission/question state from the canonical path;
7. drop old renderer completions.

Do not preserve an old subscription id as if it survived the new connection.

### Streaming and render cadence

Do not send one expensive React-wide state replacement per token if a bounded/coalesced text-delta path is available.

Use separate event classes where useful:

- projection snapshot/replacement;
- ordered projection update;
- coalesced text delta;
- critical permission/question change;
- connection/resync diagnostic.

All originate from one Rust-owned ordered source and carry enough generation/sequence metadata to reject stale delivery.

### Storage and migrations

No desktop durable database.

No SQLite migration is expected.

Renderer preferences may use a small non-authoritative settings mechanism, but session/project truth stays daemon-side.

### Protocol and DTOs

Use existing projection protocol version/capability negotiation.

If additive DTO fields are needed, follow unknown-field/unknown-optional semantics and update independent projection consumer fixtures.

No `TuiMessage` dependency in desktop code.

### Runtime and concurrency

- one desktop connection owner;
- bounded number of active projection subscriptions;
- cancellation token per renderer/session view generation;
- no detached task per token/event;
- stale completion checks before applying async project/session results;
- explicit unsubscribe on session view close when connection remains live;
- connection shutdown joins/cancels desktop-owned forwarders.

### Security and authorization

- project/session list results come from authorized daemon APIs;
- project existence is not inferred from local paths;
- permission/question response authority stays daemon-side;
- observer sessions cannot submit prompts or answer controls unless the established control-transfer policy permits it;
- content/artifact handles retain projection visibility/redaction policy;
- bridge logs redact prompt/body content according to existing logging policy and never log provider secrets.

### Documentation

Extend `architecture/desktop.md` with:

- session controller ownership;
- bridge operation table;
- projection/cursor lifecycle;
- stale-generation rules;
- permission/question control path;
- explicit non-goals before IDE work.

Update `architecture/client.md` if a shared projection driver is added.

## 7. Ordered work packages

### Work package A — Desktop project/workspace/session view model

Intent:

Establish explicit routing identity before turn interaction.

Required changes:

- authorized project list;
- workspace list/selection;
- bounded session list;
- attach/create;
- stable Rust host route token/generation.

Acceptance evidence:

- switching projects while requests are in flight cannot apply stale session results;
- no cwd/path inference;
- unauthorized/missing projects use canonical denial shape.

### Work package B — Projection-primary session consumption

Intent:

Make canonical projections the desktop truth.

Required changes:

- capability negotiate;
- subscribe/install snapshot;
- ordered event apply;
- ack cadence;
- unsubscribe;
- immutable desktop presentation snapshots.

Acceptance evidence:

- independent reducer fixture parity;
- sequence duplicate/gap handling;
- artifact/truncation handling;
- no TUI protocol dependency.

### Work package C — Prompt and visible activity

Intent:

Prove a real coding-agent turn can be initiated and observed.

Required changes:

- prompt draft/submit;
- create-then-submit continuation;
- visible text/progress;
- recent tool/job/run/subagent summaries.

Acceptance evidence:

- at-most-once prompt under double Enter/session-create latency;
- visible turn completes through canonical events;
- project/session switch drops stale deltas.

### Work package D — Permission and question control

Intent:

Prove human-in-the-loop authority works from the second rich frontend.

Required changes:

- pending control presentation;
- answer/deny operations;
- stale/revoked/controller mismatch handling;
- reconnect refresh.

Acceptance evidence:

- allowed controller can answer;
- observer/non-controller cannot;
- stale pending UI clears on daemon denial/resync;
- response does not trust renderer principal fields.

### Work package E — Reconnect/restart and two-client behavior

Intent:

Prove desktop lifecycle independence from daemon work.

Required changes:

- reconnect generation;
- resume or snapshot resync;
- daemon restart state;
- TUI + desktop simultaneous fixture.

Acceptance evidence:

- disconnect/reconnect during an active turn recovers visible state;
- desktop close does not cancel the turn;
- TUI remains live when desktop drops;
- desktop reconnect gets a new client identity and correct projection.

### Work package F — End-to-end desktop test and foundation closure

Intent:

Create evidence strong enough to begin editor/document planning.

Required changes:

- WebDriverIO/Tauri or equivalent built-app E2E;
- deterministic test daemon/project fixture;
- docs/guards;
- closure record.

Acceptance evidence:

One E2E trajectory covers:

1. launch desktop;
2. connect/reuse daemon;
3. choose project/session;
4. submit prompt;
5. observe streamed/projection activity;
6. answer a deterministic permission or question fixture;
7. complete turn;
8. force reconnect/resync;
9. verify state equivalence;
10. close desktop and verify daemon remains responsive.

## 8. Failure, cancellation, restart, and contention semantics

- Project/session fetch failures do not clear valid previous state until the replacement is accepted or the route changes.
- Session-create failure retains the editable prompt and does not duplicate the visible user message.
- Connection loss disables mutations until a new generation is ready.
- Projection gap/expired history enters typed resync; the UI never guesses missing state.
- Permission/question responses are one-shot visible intents; repeated renderer clicks are coalesced while the request is in flight.
- Session/project switch cancels frontend work but not daemon jobs/turns.
- Desktop exit detaches/unsubscribes as possible and then ends client-owned tasks; no implicit turn cancellation.
- Multiple frontends may observe the same session according to authorization; control responses still obey the shared-session controller lease.

## 9. Compatibility and migration

No desktop state migration is expected.

No change to TUI command/key behavior.

No change to ACP semantics.

No CoreFrame version bump expected unless a genuinely missing frontend-neutral projection field is added incompatibly; such a need is a stop condition requiring separate protocol planning.

The desktop can initially support the same local personal-owner mode as the TUI. Team/remote desktop access remains deferred until authenticated `/core` transport is planned.

## 10. Required tests

### Focused Rust tests

- route token/generation stale-drop;
- projection driver subscribe/resume/ack/unsubscribe;
- snapshot replacement atomicity;
- prompt create-then-submit at-most-once;
- permission/question response coalescing;
- connection generation reset.

### Projection compatibility tests

- existing golden snapshot/event fixtures consumed by desktop adapter;
- duplicate/out-of-order/gap;
- expired cursor/resync;
- artifact handle truncation;
- unknown optional event behavior.

### Security and negative tests

- observer prompt blocked;
- observer permission/question response blocked;
- project/session authorization denial;
- renderer payload cannot choose principal/client id;
- stale turn control rejected;
- cross-project locator mismatch fails closed.

### Integration tests

- concurrent TUI + desktop client;
- active turn survives desktop connection drop;
- desktop reconnect sees equivalent canonical state;
- daemon restart produces explicit resync/gone state as appropriate;
- project switch during slow session request;
- double-submit during async session create.

### Desktop E2E

Use the repository-selected Tauri WebDriver harness to execute the closure trajectory.

Prefer a deterministic fake/provider/test runtime so external model availability is not a closure dependency.

### Migration and compatibility tests

- root CLI/TUI quick suite;
- protocol golden fixtures;
- root Cargo/MSRV isolation from M003 remains intact.

## 11. Required verification commands

Adapt names to implementation and record actual commands.

```bash
cargo test -p codegg-client
cargo test -p codegg --lib
cargo test -p codegg --test projection_replay
cargo test -p codegg --test projection_transport_real --features server
scripts/verify.sh quick

cd apps/desktop
npm ci
npm run typecheck
npm test
npm run build
npm run test:e2e
```

If the E2E harness requires a built release/debug Tauri binary, record the exact build and driver mode.

## 12. Documentation updates

- `architecture/desktop.md`
- `architecture/client.md` if projection driving is extracted
- `architecture/protocol.md` if additive projection DTOs change
- `architecture/overview.md`
- desktop developer/E2E documentation
- roadmap/registry status and eventual closure record

## 13. Acceptance criteria

M004 is complete when:

1. the desktop selects daemon-authorized project/workspace/session state without cwd inference.
2. a session projection is negotiated, subscribed, reduced, acknowledged, resumed/resynced, and rendered through the canonical controller.
3. a prompt can create/attach a session as needed and submit exactly once.
4. visible assistant/tool/job/subagent activity arrives through bounded ordered desktop streaming.
5. an authorized permission/question can be answered from desktop and unauthorized/stale control fails closed.
6. project/session/connection generation guards prevent stale async mutation.
7. disconnect/reconnect and daemon restart produce deterministic recovery/resync behavior.
8. one daemon concurrently serves TUI and desktop without duplicate durable state.
9. desktop close/reload does not cancel daemon-owned work.
10. a built-app E2E trajectory demonstrates the above without external provider availability.
11. no editor/filesystem/PTY machine-authority shortcut entered the WebView.
12. no unresolved high/medium finding remains.

## 14. Stop conditions

Stop and re-plan if:

- M003 is not closed;
- desktop session behavior requires importing TUI App state;
- a missing projection feature is solved by a desktop-only daemon endpoint instead of a frontend-neutral contract;
- permission/question control would bypass ADR-0007 shared-session ownership;
- prompt at-most-once cannot be proven without changing session/turn idempotency semantics;
- editor/file access becomes necessary to satisfy the milestone;
- a generic filesystem/shell Tauri permission is proposed;
- remote/team access is required for closure;
- Windows is claimed without M002 evidence;
- a CoreFrame/projection compatibility break is required.

## 15. Closure evidence required

The closure record must include:

- implementation commit(s);
- desktop session-controller ownership diagram;
- bridge operation/event inventory;
- projection protocol/controller reuse evidence;
- at-most-once prompt fixture;
- permission/question authorization fixtures;
- reconnect/resync/restart evidence;
- TUI + desktop concurrency evidence;
- app-close/daemon-survival evidence;
- built-app E2E transcript/result;
- exact tests/commands;
- protocol/storage/MSRV/security impact statement;
- unresolved findings by severity;
- disposition on whether M005 document/buffer planning is now dependency-ready.

## 16. Handoff notes

Do not add an editor merely to make the window feel like an IDE. M004's value is proving the frontend architecture under real session interaction.

After M004 closes, perform a fresh audit of `egglsp`, file mutation/watch semantics, worktrees, and agent edit flows before writing M005. The document/buffer contract needs to reconcile unsaved user text with daemon/agent/LSP truth; it should not be guessed in advance.
