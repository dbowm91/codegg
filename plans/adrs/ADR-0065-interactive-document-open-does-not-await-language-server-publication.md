# ADR-0065: Interactive document open does not await language-server publication

Status: accepted

Date: 2026-11-22

Decision owners: project maintainers

Related specification sections:

- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#25-tui-target-behavior`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/001-terminology-and-domain-model.md#6-session-and-interaction-terms`
- `plans/001-terminology-and-domain-model.md#13-protocol-terms`
- `plans/002-long-term-roadmap.md#phase-5--frontend-neutral-session-projections-and-durable-replay`

Affected subsystem roadmaps:

- `plans/subsystems/tui-editor-ide-presence-roadmap.md`

This decision does not amend ADR-0011, ADR-0012, ADR-0021, ADR-0034, or
ADR-0064.

## Context

Opening a file in the TUI editor navigates to the editor route immediately,
but the buffer stays empty until `DocumentOpen` returns. At the baseline
`d585e75f895cf41cd2241512d8d7e7c37fcece99`, the daemon's workspace
`DocumentOpen` arm awaits `refresh_external_disk` and then
`sync_managed_document` before it returns `CoreResponse::DocumentSnapshot`.

`sync_managed_document` calls `LspService::set_managed_document`, which
calls `get_or_create_client`. A cold server is spawned, initialized, and
published before `didOpen` of the file just requested. Publication runs
`compute_readiness_decision` against the server profile.
`rust_analyzer_profile()` is `WaitForDiagnosticsOrTimeout { timeout: 30s }`.
That wait polls for the first `textDocument/publishDiagnostics`. The
diagnostic it waits for is not one this open has caused, because `didOpen`
of the requested file has not been sent yet. The same shape, with a 15s
timeout, is the pyright profile. Other tier-1 profiles also wait for
diagnostics.

No wall-clock of an 800-line open was measured. The 30-second cap and the
pre-`didOpen` wait are source facts. "Several seconds" matches an early
diagnostic or the initialize handshake. The cost is workspace startup, not
the line count. A warm later open is `didOpen` plus the disk refresh,
unless the workspace repository lock is held.

`lsp_degraded` on `DocumentSnapshot`, `DocumentChanged`,
`DocumentSaved`, and `DocumentReloaded` is the boolean result of that
awaited sync. `false` today means "the sync this response waited for
completed without error," which callers can misread as "the server has the
file and is healthy." The TUI does not draw the flag. A not-ready server
therefore looks like a quiet editor.

Desktop and every other `DocumentOpen` caller share this daemon arm. A
TUI-only timeout would leave the contract lying for the next frontend.

## Decision drivers

- An interactive open must paint the daemon's document snapshot without
  waiting for a language-server qualification policy.
- The qualification policy is still the right policy for callers that are
  qualifying a server. Deleting it globally would make a Ready-and-empty
  diagnostics pull look like a clean file.
- ADR-0012 remains: reads are warm-server-only, and `NotReady` is not an
  empty success. This decision must not add a second push stream.
- ADR-0011 remains: the daemon owns document text and language-server
  processes. The frontend must not gain a buffer, a filesystem read, or its
  own server to make open feel fast.
- `lsp_degraded: false` must not keep meaning "synced" once the open
  response returns before sync finishes.
- Change, save, and reload are a different deadline. This decision must not
  silently make them asynchronous.

## Considered options

### Option A — Keep awaiting publication on DocumentOpen

The open response stays a single witness: when it returns, sync has been
attempted and `lsp_degraded` means what it means today.

Benefits: no protocol change; every caller that treats open as "the server
has this file" keeps working.

Costs: the interactive editor inherits a qualification wait of up to 30
seconds before `didOpen` of the file the user asked for. The blank buffer
is the product.

Failure mode: a server that publishes an unrelated diagnostic early returns
quickly and hides the defect; a server that publishes nothing holds every
frontend for the full timeout.

### Option B — Change the rust-analyzer profile to InitializedIsReady

`rust_analyzer_profile()` stops waiting for diagnostics. Every caller of
that profile, including qualification and restart, becomes ready at
`initialized`.

Benefits: one constant, and `didOpen` is no longer stuck behind the wait.

Costs: qualification no longer observes the failure mode the profile was
written to catch. A diagnostics pull that returns Ready with an empty list
is indistinguishable from a clean file. Pyright and the other
diagnostics-wait profiles are a second, easy-to-forget edit.

Failure mode: the editor looks healthy while rust-analyzer is still indexing,
which is the "LSP looks dead" symptom with the opposite lie.

### Option C — Document authority returns; publication does not gate it

`DocumentOpen` returns after authorization, the document open, and the disk
refresh. Language-server publication continues without holding that response.
The publication that gates `didOpen` for that interactive open does not use
`WaitForDiagnosticsOrTimeout`. The profile constants used by qualification
and restart stay as they are. The open response gains an explicit sync state
so `lsp_degraded: false` is no longer a synonym for synced.

Benefits: the buffer can paint while the server warms; qualification keeps
its policy; a warming server is not reported as clean; desktop shares the
daemon fix without a TUI-owned server.

Costs: callers that treated open-returned as server-has-the-file must read
the new state. An old client that denies unknown fields cannot decode a new
daemon response. Background publication can still fail after the snapshot is
on screen.

Failure mode: an implementation that only stops awaiting, but still runs the
diagnostics wait before `didOpen`, fixes the blank buffer and leaves the
requested file unsynchronized for up to 30 seconds. An implementation that
also flips the global profile reintroduces Option B.

## Decision

Option C.

1. The response deadline of workspace `DocumentOpen` is document authority:
   authorization, the document open, and `refresh_external_disk`. The
   response does not await `sync_managed_document` or language-server
   publication.
2. `rust_analyzer_profile()` and the other server profiles keep
   `WaitForDiagnosticsOrTimeout` for qualification, restart, and any
   non-interactive caller that already waits on that policy. This decision
   does not edit those profile constants.
3. The publication that gates `didOpen` for an interactive document open
   uses `InitializedIsReady`, or otherwise does not wait on
   `WaitForDiagnosticsOrTimeout`. The requested file's `didOpen` must not
   sit behind a diagnostic this open has not caused.
4. `CoreResponse::DocumentSnapshot` carries an additive `lsp_sync` state:
   `Pending`, `Synced`, `Degraded`, or `Unavailable`, with
   `#[serde(default)]`. `lsp_degraded` is true only for `Degraded` or
   `Unavailable`. On a payload that omits `lsp_sync`, the legacy meaning
   holds: the daemon that sent it had awaited sync, and `lsp_degraded:
   false` meant that wait completed without error.
5. A server that has initialized but has not published diagnostics for the
   opened file is not clean. Reads stay warm-server-only under ADR-0012.
   `NotReady` is not an empty success. This decision adds no push stream
   and no new projection event.
6. Ownership is unchanged. The daemon owns document text and language-server
   processes. No frontend gains a text buffer, a filesystem read, or its own
   language server. ADR-0034 Option B is unchanged: no frontend tree-sitter
   dependency. ADR-0021 is unchanged: completion acceptance stays an explicit,
   revision-checked local edit.
7. `DocumentChanged`, `DocumentSaved`, and `DocumentReloaded` are not
   made asynchronous by this decision. Their `lsp_degraded` fields keep
   today's meaning until a milestone names them.
8. The global-agent open path keeps its filename rule. If that path awaits
   publication, the same response deadline applies. It does not gain a
   workspace open's authority.

Starting the milestone that implements this Decision records acceptance for
that milestone. Selecting Option A or Option B inside the milestone is a
stop, not an implementation choice.

## Consequences

### Positive

- Interactive open is bounded by document authority, not by a 30-second
  diagnostics poll.
- Qualification and restart keep the policy that detects a server which
  never publishes diagnostics.
- `lsp_degraded: false` stops meaning "synced" on the open response.
- Every frontend that calls `DocumentOpen` gets the same deadline.

### Negative

- A client that treated open-returned as server-has-the-file is wrong after
  this change, until it reads `lsp_sync`.
- An old client that denies unknown fields cannot decode a new daemon's
  `DocumentSnapshot`. Frontends ship with the daemon that adds the field.
- The snapshot can be on screen while `didOpen` has not completed. The
  editor must not present that interval as a clean file. Naming that interval
  is a later milestone; this decision only forbids the lie.

### Neutral or deferred

- Debouncing `textDocument/didChange`, viewport semantic tokens, diagnostic
  underlines, and Insert-mode completion are not this decision.
- No live timing of an 800-line open is claimed. A warm open that is still
  slow after this change is measured at `refresh_external_disk` and the
  repository lock, not by skipping the disk refresh.
- Desktop presentation of the new state is out of scope. Desktop must still
  decode the response.

## Compatibility and migration

`lsp_sync` is additive and `#[serde(default)]`, so a new client decodes an
old daemon payload. A missing value means the legacy awaited-sync meaning,
not `Pending`.

A new daemon response carries the field. Older clients that deny unknown
fields fail closed on that response. That is accepted for a same-release
daemon and frontend. Do not dual-encode. Do not make the field optional by
omitting it from new responses: a new response that omits it would be read
as the legacy meaning.

`DocumentChanged`, `DocumentSaved`, and `DocumentReloaded` do not gain
the field in this decision.

No storage migration. No `STORAGE_LAYOUT_VERSION` bump. No configuration
key. The readiness-profile constants are not a migration.

## Security and reliability implications

Authorization still happens before the snapshot is returned. The disk refresh
still runs under the workspace repository lock, with the same path
validation. Returning earlier does not authorize a path the open would have
refused, and it does not skip the refresh.

The background publication uses the existing language-server service. It does
not start a server from the frontend, and it does not widen the process
launcher set. A failed publication clears the sync witness, as
`sync_managed_document` already does, and surfaces `Degraded` or
`Unavailable` rather than a synced revision.

Cancellation of the open request does not cancel document authority that
already committed. It must not leave a sync witness for a revision that never
reached a server. Two opens of the same document keep today's rejection and
switch rules.

`NotReady` during warmup remains a distinct outcome from an empty
diagnostic list. This decision does not grant a writer the existing
authorization check would refuse. Folding the TUI's follow-up writer acquire
into the open, if an implementation does that, must call the same authorization
the acquire uses.

## Verification

- A server that never publishes diagnostics does not hold workspace
  `DocumentOpen` for the profile timeout. The snapshot returns after
  document authority.
- `rust_analyzer_profile()` is still `WaitForDiagnosticsOrTimeout` with
  the 30-second timeout. The existing profile tests stay green.
- The interactive `didOpen` path does not call
  `wait_for_first_diagnostics`.
- A payload without `lsp_sync` decodes, and `lsp_degraded: false` on that
  payload still means the legacy awaited success. A new payload with
  `lsp_sync: Pending` has `lsp_degraded: false` and is not treated as
  synced by the new client.
- `scripts/check_tui_editor_text_authority.py` stays green. No frontend
  filesystem read and no frontend language-server spawn are added.
- Change, save, and reload tests that await sync still pass without this
  decision's deadline applied to them.

## Supersession

None. This decision does not supersede ADR-0011, ADR-0012, ADR-0021, or
ADR-0034.
