# ADR-0012: Native LSP Read Surface — Split Delivery, Warm-Only Reads, and File-Read Authorization

Status: proposed

Date: 2026-10-05

Decision owners: project maintainers

Related specification sections:

- `plans/000-long-term-specification.md#45-locality-by-default`
- `plans/000-long-term-specification.md#47-correctness-before-transparent-magic`
- `plans/000-long-term-specification.md#87-project-authorization`
- `plans/001-terminology-and-domain-model.md`

Affected subsystem roadmaps:

- `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md#m006-decomposition`

Related ADRs:

- ADR-0008 (accepted) — LSP preview apply authorization and runtime ownership.
  This ADR is bound by ADR-0008's conclusions and does not revisit them.
- ADR-0010 (accepted) — Desktop frontend and shared client boundary.
- ADR-0011 (accepted) — Editor document ownership and frontend replication.

## Context

M006-C will render LSP results in the TUI editor. It cannot start before M006-B
creates a native LSP **read** surface, for a reason that is structural rather
than incidental.

**The socket-mode TUI has no LSP at all.** `src/main.rs:3323` constructs
`app.lsp_tool` only under `if !is_socket_mode`, with the source comment stating
the reason directly: *"socket mode has no LspTool on the client side."* The
`LspService` itself is built under the same condition (`src/main.rs:3169`).
Today's `/lsp-servers`, `/lsp-capabilities`, and the status bar therefore work
only in local standalone mode, and they read client keys and health snapshots
rather than document content in any case.

**The daemon already owns an `LspService`** (`src/core/runtime_deps.rs:73`),
used for document synchronization (`src/core/daemon_documents.rs:12`) and turn
context (`src/core/daemon_turns.rs:421`), and it already dispatches the one
LSP protocol variant that exists — `CoreRequest::LspPreviewApply`
(`src/core/daemon_goals.rs:1004`). That variant is a **write**. There is no LSP
read variant anywhere in `codegg-protocol`.

**The capability gap is transport, not computation.** `egglsp` already
implements definition, declaration, implementation, references, hover,
document symbols, workspace symbols, document highlights, code lens, semantic
tokens, and signature help, all as `async` methods on `LspOperations`
(`crates/egglsp/src/operations/`). None are cached. Diagnostics are the
exception: they arrive by LSP `textDocument/publishDiagnostics` push into the
daemon-owned `DiagnosticsCollector` and are read from a local store
(`crates/egglsp/src/diagnostics.rs:205`, `:288`). So a diagnostics read is a
**local store hit**, and a point query is a **server round-trip** — two very
different cost profiles that must not be given one uniform mechanism.

Two properties of the existing transport constrain any push design. The
projection sink uses `try_send` and **drops the envelope when its queue is
full, logging only a warning** (`src/core/event_log.rs:180-186`); there is no
blocking backpressure and no coalescing at the sink edge. Recovery exists —
durable `projection_event` storage plus cursor-based `ProjectionResume` and
`mark_resync_required` (`src/core/transport/projection.rs:343`) — but it is
driven by the client noticing a gap, not by the drop itself. Separately,
`get_or_create_client` (`crates/egglsp/src/service.rs:1180`) is
**create-or-get**, so an ordinary read can spawn a language server as a side
effect, including multi-hundred-megabyte indexers.

M006-B is gated on an ADR because it introduces a new frontend protocol
surface, selects a delivery mechanism, and sets authorization semantics for
file-content-bearing responses.

## Decision drivers

- The socket-mode TUI must gain an LSP read path; this is the precondition for
  M006-C and cannot be deferred.
- The daemon-resolved `LspService` is canonical, per ADR-0008. The frontend
  must not stand up a second LSP service.
- Push must not be able to make the editor show **stale diagnostics as if they
  were current**. Silent staleness is worse than a visible gap.
- A read must not block an editor gesture on language-server process start and
  indexing.
- An empty result and a "not ready yet" result must be distinguishable, or a
  warming workspace reads as clean code.
- Diagnostics carry **source content**; the authorization gate must reflect
  that rather than treating them as mere project state.
- Output must stay bounded on every read, reusing existing budgets rather than
  inventing a second set.
- The pull surface the TUI already uses (~250 `CoreRequest::` call sites,
  zero projection consumption) should not be bypassed for point queries.

## Considered options

### Option A — Pull everything

One new `CoreRequest` read family; the TUI fetches diagnostics and point
queries alike.

*Benefits.* Smallest blast radius. No new projection kind, no subscription
lifecycle, no per-connection diagnostics cap, no resync contract. Bounded
trivially.

*Costs.* The TUI must poll to observe new diagnostics, so markers appear at
the poll interval rather than immediately. Polling cost is low — the daemon
already holds the set — but "low" is not "zero", and a large project
generates a diagnostic envelope per file per analysis pass.

### Option B — Push everything, including semantic tokens

Project-scoped push for all read results, including the highest-rate data in
the system.

*Benefits.* Lowest latency; no polling at all.

*Costs.* Semantic tokens change on every keystroke. Routing them through a
sink that drops on full would mean the highest-rate stream is the one most
likely to lose data. This inverts the risk profile and is rejected.

### Option C — Split by data shape *(selected)*

Push diagnostics, because they are a continuously-changing ambient property of
a project. Pull point queries, because they are answers to an explicit user
gesture at a known position.

The dividing line is not convenience. It is that diagnostics are a
**replace-set** while point queries are **request/response**:

- LSP `publishDiagnostics` replaces a document's entire diagnostic set. Each
  envelope therefore carries the **complete current set for one file at one
  revision**, never a delta. This is precisely what makes coalescing and
  dropping safe for this data and unsafe for general event data.
- Hover, definition, references, and symbols answer "what is at this
  position" or "what matches this query". There is no ambient state to keep
  current, so a stream would be carrying data with no natural supersession
  rule.

### Option D — Push diagnostics, pull point queries, with push as the only
diagnostics path

*Costs.* A dropped terminal envelope leaves the client permanently stale with
no way to recover, because nothing else can supply the current set. Rejected:
it makes a best-effort queue the authority for correctness.

## Decision

### 1. Diagnostics are pushed on a project-scoped projection stream; point
queries are pulled over `CoreRequest`

Diagnostics are published as a new `ProjectionEvent::LspDiagnosticsUpdated`
carrying the complete diagnostic set for one file at one revision, on
`ProjectionStreamKind::Project` (which already exists,
`crates/codegg-protocol/src/projection/replay.rs:34-37`).

Hover, definition, references, document symbols, workspace symbols, and
semantic tokens are served by new `CoreRequest`/`CoreResponse` variants on the
daemon, dispatched through `DaemonRequestFamily::of`
(`src/core/daemon_family.rs:52`).

### 2. The stream is a latency optimization; the pull is the correctness
authority

This is the clause that makes push safe despite `try_send` dropping.

- Every diagnostics envelope carries the **complete** set for its file, never
  a delta, plus a per-file monotonic sequence number.
- A client that observes a sequence gap, reconnects, or enters
  `ResyncRequired` issues `CoreRequest::LspDiagnosticsGet` to fetch the
  authoritative current set.
- Consequently a dropped envelope can cost **freshness, never correctness**.
  The client is never required to trust a stream it knows it missed.

If enforcement of this clause is ever considered optional, the correct response
is to fall back to Option A, not to weaken it.

### 3. Reads use already-warm servers only

A read against a server the daemon has not started returns a distinct
**`NotReady`** outcome — neither an error nor an empty result. The daemon may
start or warm a server in the background; the client is told it is warming and
the UI shows a warming affordance rather than a clean file.

This is a maintainer decision taken on 2026-10-05 against a recommendation to
allow a bounded blocking start. It was chosen because a bounded blocking start
still lets an editor gesture block for the whole startup-and-index window, and
because an empty result is the more dangerous failure: the user would read a
warming workspace as clean code.

`workspace_symbols` is unaffected: it uses `first_client_key()`
(`crates/egglsp/src/operations/navigation.rs:134`) and already fails with
`NotInitialized` rather than launching.

### 4. Authorization: `file.read`, scoped like its write-side sibling

- Point-query reads: `ScopeKind::ViaSession` + `Capability::FileRead`, exactly
  mirroring `lsp_preview_apply` = `ViaSession` + `FileModify`
  (`crates/codegg-core/src/authorization/policy.rs:750-754`). Per ADR-0008 the
  transport scope resolves through the owning session's project before any
  side effect.
- Diagnostics subscription: `ScopeKind::DirectProject` +
  `Capability::FileRead`.

**Not** `ProjectObserve`, which is what the existing generic
`ProjectionSubscribe` uses. That gate is correct for lifecycle projections
because they carry workflow state. LSP diagnostics carry source content —
messages, code, and snippets — so the weaker project-observability capability
would understate what is disclosed. Diagnostics are not a generic projection
and must not inherit the generic projection's gate.

Every new variant requires an explicit `operation_descriptor` arm
(`crates/codegg-core/src/authorization/policy.rs:101`);
`scripts/check_authorization_matrix.py` fails on a wildcard arm, and a row must
be added to `architecture/authorization.md`.

Per ADR-0008, human/TUI transport authorization and model-tool authorization
remain distinct caller boundaries. This ADR adds a transport read surface
only; it grants no model-facing capability and changes no tool contract.

### 5. Capability scope

Expose exactly what the roadmap names: diagnostics, hover, definition,
references, document symbols, workspace symbols, semantic tokens.

Explicitly out of scope:

- `type_definition` — **does not exist** in egglsp. The only occurrence is a
  doc comment (`crates/egglsp/src/operations/navigation.rs:15`). It is new
  egglsp work, not frontend wiring.
- `code_lens` — exists (`navigation.rs:464`) but has no wrapper anywhere in
  `src/`; deferred as unwired.
- Workspace-wide continuous semantic-token streaming, per Option B's rejection.

### 6. Bounded output

Reads inherit egglsp's existing budgets (`LspContextBudget` defaults,
`crates/egglsp/src/context.rs:58-69`) and the existing frontend per-operation
caps at `src/tool/lsp.rs:14-45` (`MAX_REFERENCES` 100, `MAX_SYMBOLS` 300,
`MAX_WORKSPACE_SYMBOLS` 200, `MAX_HOVER_CHARS` 2000, `MAX_SEMANTIC_TOKENS`
1000, `MAX_SYMBOL_QUERY_LEN` 200, `src/tool/lsp_read.rs:69`).

Diagnostics gain a **new** per-connection cap. The existing
`MAX_CONNECTION_DIAGNOSTICS = 32` (`src/core/transport/projection.rs:24`) is
**not** LSP infrastructure — it is a connection-local ring of lifecycle
diagnostic *strings* (`VecDeque<String>`, `projection.rs:386`). The name
coincides; it must not be reused or cited as precedent.

## Consequences

### Positive

- The socket-mode TUI gains an LSP read path, unblocking M006-C.
- Diagnostics appear without polling, and the warm-read rule keeps editor
  gestures off the process-start path.
- Because the stream is not the correctness authority, the existing
  drop-on-full sink behavior stops being a correctness risk for this data.
- Point queries reuse the TUI's established pull pattern and the existing
  `REQUEST_CAPACITY`/semaphore discipline in `codegg-client`.

### Negative

- Two mechanisms must be maintained: a stream for diagnostics and a pull
  family for point queries.
- The gap-detection clause is a real client obligation, not a no-op. A client
  that ignores sequence gaps degrades to silent staleness.
- Diagnostics must be published durably enough for replay correctness, which
  requires a deliberate decision in `should_persist`
  (`src/core/event_log.rs:37-57`) rather than inheriting the current default.

### Neutral or deferred

- `type_definition` and `code_lens` remain unavailable; neither is a
  regression.
- This ADR defines the read surface only. It does not authorize a model-facing
  read tool; that would be a separate decision under ADR-0008's caller-boundary
  rule.
- Graphical/Monaco IDE work remains long-term and is unaffected.

## Compatibility and migration

Additive only. No existing variant changes meaning. `LspPreviewApply` is
untouched, as is `src/lsp/mutation.rs`.

`DaemonRequestFamily::of` is exhaustively matched with no wildcard
(`src/core/daemon_family.rs:52`), so a new variant will not compile until it is
routed — a useful forcing function, not an obstacle. A new family module
(`src/core/daemon_lsp.rs`) is preferred over extending `daemon_goals.rs`, which
owns LSP preview apply today, so read and write authority stay separately
readable.

No storage migration. Diagnostics are ephemeral projection state; ADR-0008's
ruling that preview state stays bounded and non-durable is extended to
diagnostics.

## Security and reliability implications

**Authorization.** `authorize_request` runs as a preamble before any side
effect (`src/core/daemon.rs:617-644`, invoked at `:3509-3525`); denials emit
audit and return with zero effect. Principal identity comes from the
`ClientRegistry` at handshake (`src/core/daemon.rs:137-152`), never from the
payload. Diagnostics content is gated at `FileRead`, not `ProjectObserve`.

**Non-enumeration.** Diagnostics for an unauthorized or foreign project must
fail indistinguishably from an absent one, using the existing
`denial_as_not_found` convention.

**Redaction and disclosure.** Diagnostics carry source text and must be
classified in `safe_publication.rs`. They must not inherit the
`Internal`-classified treatment of generic projection payloads without an
explicit decision, and the disclosure guards
(`scripts/check_projection_disclosure.sh`) apply to the new event.

**Resource bounds.** The warm-read rule caps process growth caused by reads. A
read cannot spawn an indexer. Per-connection and per-read caps bound payload
size.

**Contention and cancellation.** Editor reads are frequent and cancellable;
they must run through the TUI's `spawn_tui_task` + `finish(request_id)` /
`fail(request_id, err)` discipline with stale-completion tests, never
`Handle::current().block_on(...)` on the render thread — which is what the
existing `/lsp-servers` and `/lsp-capabilities` commands do today and should
not be copied.

**Restart.** Diagnostics are ephemeral. After a daemon restart a client
reconnects, sees no sequence history, and re-pulls. This is why the pull is
the correctness authority.

## Verification

Evidence required before this ADR is considered implemented:

- A guard or test proving no LSP read handler is reachable without an explicit
  `operation_descriptor` arm — the authorization matrix guard is expected to
  cover this, and that expectation must be verified rather than assumed.
- A denial test per new variant, mirroring the `DocumentStatusGet` shape,
  asserting zero effect and non-enumerable error text.
- A test proving a read against a cold server returns `NotReady` and does not
  spawn a process.
- A test proving a client detecting a diagnostics sequence gap re-pulls rather
  than rendering a partial set.
- A test proving diagnostics content is refused at `ProjectObserve` and
  permitted at `FileRead`.
- Proof that `src/lsp/mutation.rs` and the `LspPreviewApply` path are unchanged
  by this work.
- Hosted CI green on the full workspace sweep.

## Supersession

None. This ADR extends ADR-0008 and must be read with it. If a later decision
makes the diagnostics stream the sole authority for the current diagnostic set,
this ADR is superseded rather than amended.
