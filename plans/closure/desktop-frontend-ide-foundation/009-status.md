# Desktop Frontend and IDE Foundation Milestone 009 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/desktop-frontend-ide-foundation/008-native-lsp-read-surface.md`
  (milestone M006-B; this closure uses `009` to follow the repository's
  sequential closure numbering — M006-A/D/E closed at `005`, `006`, and `007`)

Applicable ADR (controlling):

- `plans/adrs/ADR-0012-native-lsp-read-surface-delivery-and-authorization.md`
  — accepted, merged as `f7c8948a`

Source subsystem roadmap:

- `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md`

Audit of record:

- `plans/subsystems/desktop-frontend-ide-foundation-m006-tui-presentation-audit.md`

Repository baseline reviewed: `3bee7011` (merge head; fix developed on
`m006-b-read-surface` at `4ce0343b`)

Implementation commits:

- `49ed8b4d` — M006-B implementation plan.
- `9d40a2d0` — WP-A–E: protocol DTOs, authorization arms, daemon family and
  handler, digest-keyed diagnostics store, client read surface.
- `15fb05d8` — WP-D2/E2/F: event publication, resync trajectory evidence,
  architecture docs.
- `4ce0343b` — `LspDiagnosticsSubscribe` gated at `direct_project` + `file.read`
  and the `Safe` event classification.

PR: `#99`

## 1. Executive finding

M006-B delivers a native LSP **read** surface end to end, and the protocol now
has read operations at all: `LspReadGet` for point queries, `LspDiagnosticsGet`
as the resync authority, and a dedicated `LspDiagnosticsSubscribe` stream. The
M006 presentation audit recorded that no LSP read operation existed in the
native protocol and that the M005 document foundation had no consumer; this
milestone removes the first half of that finding and is the precondition for
M006-C, which renders the surface.

Two properties are structural rather than conventional, and they are the reason
this milestone was worth an ADR:

- **Warm-server-only reads.** `find_existing_client_for_root_hint` resolves an
  existing client or errors; it never falls through to a create path. A
  workspace whose language server is still warming returns `NotReady`, and
  `NotReady` carries no payload, so a caller cannot render a warming workspace
  as clean code. Denial is an error, not `NotReady` — a caller must be able to
  tell "nothing wrong here" from "we do not know yet".
- **Containment is shared with the write path, not reimplemented.** Targets
  resolve through `crate::tool::util::validate_target_path`, the identical
  primitive `src/lsp/mutation.rs` uses, and the workspace root comes from the
  daemon's session row rather than from the request. `src/lsp/mutation.rs` and
  `scripts/` are absent from the diff: the dirty-buffer rejection is untouched
  and was not weakened.

The authorization decision diverges from the generic projection gate
deliberately. Diagnostics carry source content, so both the pull and the stream
are gated at `file.read`, not at the generic `Opaque` + `project.observe`. The
stream is a *dedicated* request rather than a flag on `ProjectionSubscribe`
precisely because the generic gate cannot express `file.read`; the gate lives in
`operation_descriptor` so `authorize_request` enforces it before any handler
runs.

This milestone was blocked for part of its life by a pre-existing causal-advisor
timing flake. That was fixed separately, on its own branch and its own PR, so
this diff stays LSP-only; see §4 and §10.

## 2. Requirement-to-evidence matrix

| Requirement (plan §4) | Evidence | Result |
|---|---|---|
| Two request variants, not seven | `LspReadGet` with an `LspReadOperation` enum covering hover, definition, references, document symbols, workspace symbols, and semantic tokens; `LspDiagnosticsGet` separate | pass |
| Warm-server-only reads | `find_existing_client_for_root_hint` resolves or errors; no create path reachable from the handler; `NotReady` carries no payload | pass |
| Denial is an error, not `NotReady` | distinct paths; `LspReadStatus` separates the two and only `NotReady` is payload-free | pass |
| Workspace root from the session row | `lsp_workspace_root` reads the daemon's session row; the request cannot supply a root | pass |
| Containment identical to the write path | `resolve_target` calls `crate::tool::util::validate_target_path`, the same primitive as `src/lsp/mutation.rs` | pass |
| Point reads gated at `file.read` | `operation_descriptor` for `LspReadGet` requires `file.read` under `via_session`; asserted by `the_point_reads_are_gated_at_file_read_too` | pass |
| Diagnostics pull gated at `file.read` | `LspDiagnosticsGet` requires `file.read` under `direct_project` | pass |
| Stream is a dedicated gated request | `LspDiagnosticsSubscribe`, not a flag on `ProjectionSubscribe`, because the generic gate is `Opaque` + `project.observe` and cannot express `file.read`; asserted by `the_diagnostics_subscription_is_its_own_gated_request` | pass |
| Digest-keyed publication emits only on change | `an_unchanged_file_publishes_nothing`; `DiagnosticsTracker` computes its own digest rather than trusting a caller | pass |
| Drop-on-full costs freshness, not correctness | the stream is a latency optimization; `LspDiagnosticsGet` is the pull authority | pass |
| Digest excludes `age_ms` and normalizes emission order | `diagnostics_digest` implementation; verified by the trajectory tests | pass |
| File cap is reported, not silent | `MAX_TRACKED_FILES_PER_PROJECT` surfaced to the caller; asserted by `the_file_cap_is_reported_rather_than_silent` | pass |
| `SymbolKind` not owned by the protocol | read via its `serde(transparent)` i32 rather than a 26-arm protocol taxonomy | pass |
| `src/lsp/mutation.rs` untouched | `git diff --stat origin/main...HEAD` lists no `src/lsp/mutation.rs`; `LspPreviewApply` deliberately stays in `DaemonRequestFamily::Goals` | pass |
| `scripts/` untouched | absent from the diff | pass |
| Authorization matrix guard green | `python3 scripts/check_authorization_matrix.py` | pass |
| Projection seam guards green | `check_projection_publication_seam.sh`, `check_projection_transport_isolation.py`, `check_projection_disclosure.sh`, `check_websocket_bounds.py` | pass |
| Diagnostics are not persisted | `should_persist` excludes `LspDiagnosticsUpdated`, documented at `src/core/event_log.rs:37-57` | pass |
| Session-scoped consumers reject the new bundle | `ProjectionSnapshotBundle::LspDiagnostics` rejected/ignored exactly as `BoundedSessionList` is | pass |
| TUI rendering not attempted | no TUI file in the diff; that is M006-C | pass |
| Trajectory evidence | `tests/lsp_read_surface_trajectory.rs`, 14 tests, green locally and hosted | pass |
| No TUI text-authority or core-boundary regression | `verify.sh quick` exit 0, including the TUI authority guards and the `codegg-core` boundary guard | pass |
| Hosted `CI / verify` green | run `37313791286`, 24/24 steps, sweep 12287/12287 | pass |
| Hosted Desktop E2E green | run `37313791405`, success | pass |

## 3. Production implementation evidence

`LspReadGet` and `LspDiagnosticsGet` are `via_session`/`direct_project` requests
that go through the existing daemon request dispatch; a new
`DaemonRequestFamily::Lsp` carries them, while the write path
`LspPreviewApply` deliberately stays in `Goals` so the read surface cannot be
mistaken for a second, weaker mutation path.

`LspReadRequestDto::validate` is the protocol-side input gate. Reads resolve
targets through `validate_target_path` — the primitive `src/lsp/mutation.rs`
already uses — and take the workspace root from the daemon's session row, never
from the request, so a caller cannot widen the workspace by asking.

`LspDiagnosticsStore` is digest-keyed. `DiagnosticsTracker` computes the digest
itself rather than trusting a caller, the digest excludes `age_ms` (which
changes on every read and would make every read look like a change), and
emission order is normalized. The store is capped at
`MAX_TRACKED_FILES_PER_PROJECT = 512`, and hitting the cap is reported to the
caller rather than silently truncating.

`publish_changed_diagnostics` polls at `DIAGNOSTICS_POLL_INTERVAL = 750ms` and
publishes `CoreEvent::LspDiagnosticsUpdated` on the existing
`ProjectionStreamKind::Project` stream only when the digest changes. A dropped
envelope therefore costs freshness, never correctness: the client reconciles
against `LspDiagnosticsGet`, which is the resync authority. The
`DiagnosticsReconciler` in `crates/codegg-client/src/lsp.rs` makes that
resync contract explicit rather than leaving it as an emergent behaviour of the
event stream.

`LspDiagnosticsUpdated` is classified `Safe` in
`crates/codegg-core/src/projection_replay/safe_publication.rs`, coupled to the
gated subscribe — the classification is only sound because the stream itself is
gated at `file.read`, and the coupling is the reason that pairing is structural
rather than a comment.

Two traps found while planning are worth carrying forward, because both would
have been silent:

- `egglsp::diagnostics::get_diagnostics_for_file` (`crates/egglsp/src/diagnostics.rs:205`)
  **debounces and can return an empty set**. Using it as the authority would
  have wiped client diagnostics on a read. The correct accessor is
  `get_diagnostic_snapshot_for_file` (`:286`).
- `MAX_CONNECTION_DIAGNOSTICS = 32`
  (`src/core/transport/projection.rs:24`) is not LSP infrastructure. It is a
  `VecDeque<String>` of lifecycle diagnostic strings, and treating it as a
  diagnostics cap would have been a straightforward misread.

## 4. Verification executed

### Commands run

Local (darwin aarch64, toolchain 1.89):

```bash
cargo test --locked --test lsp_read_surface_trajectory      # 14 passed
cargo test --locked --test lsp_read_surface_trajectory      # re-run on the rebased head
cargo test -p codegg-protocol                               # 198 passed
cargo test -p codegg-core                                   # 818 passed
cargo clippy --workspace --all-targets --locked -- -D warnings
scripts/verify.sh quick
python3 scripts/check_authorization_matrix.py
bash    scripts/check_projection_publication_seam.sh
python3 scripts/check_projection_transport_isolation.py
python3 scripts/check_projection_disclosure.sh
python3 scripts/check_websocket_bounds.py
```

Hosted:

```bash
gh run view 37313791286     # CI / verify, PR #99
gh run view 37313791405     # Desktop E2E, PR #99
```

### Results

Local:

- `tests/lsp_read_surface_trajectory.rs`: **14 passed, 0 failed**, both on the
  original head and again on the rebased head `4ce0343b`, which is the head that
  hosted CI actually ran.
- `codegg-protocol`: 198 passed. `codegg-core`: 818 passed.
- lib tests: 5262 passed.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: clean.
- `scripts/verify.sh quick`: exit 0, all guards green.
- `check_authorization_matrix.py`, `check_projection_publication_seam.sh`,
  `check_projection_transport_isolation.py`, `check_projection_disclosure.sh`,
  `check_websocket_bounds.py`: all green.

Hosted (PR `#99`, head `4ce0343b`):

- CI run **`37313791286`**: verify job **success**, **24 of 24 steps green**, no
  failed step.
  - `Workspace tests`: **`12287` tests run, `12287` passed, `7` skipped**, in
    292.512s. The sweep completed rather than truncating under fail-fast.
  - All 14 `lsp_read_surface_trajectory` tests passed on hosted hardware.
  - `causal_active_m005::m005_holdout_structural_gates` passed (6.096s) and
    `m005_freeze_record_matches_live_contracts` passed — the gate that
    originally blocked this PR.
- Desktop E2E run **`37313791405`**: **success**, 18 steps including the
  production boundary guard, the built-app WebDriver trajectory, and the desktop
  renderer checks.

The 7 skipped are pre-existing and unrelated.

### The blocker, and how it was removed

This PR was red for most of its life. The first hosted run, `37262641992`, failed
`m005_holdout_structural_gates` with `p95 active evaluation 5.141 ms exceeds the
5 ms budget` and truncated its sweep at 5,716 of 12,287. That failure is
**pre-existing and unrelated to M006-B**, which touches no causal-advisor code.

Rather than absorb an unrelated fix into an LSP diff, it was handled as its own
additive corrective on its own branch and PR — `#100`, merged as `4bc27ee0` —
and this branch was then rebased onto it. The C002 work is therefore
independently reviewable and independently closable, and this diff stays
LSP-only: `git diff --name-only origin/main...HEAD` on the pre-merge head
contained no `causal` or `tool_advisor` path and nothing under `scripts/`.

The C002 record is at
`plans/closure/tool-selection-advisor-causal-frontier-timing-corrective/002-status.md`.
This closure does not restate it.

## 5. Invariant review

- `src/lsp/mutation.rs` is unmodified. The dirty-buffer rejection is correct as
  written and was not weakened, narrowed, or special-cased to accommodate the
  read surface. This was a hard constraint on the plan, not a preference.
- The `LspPreviewApply` write path remains in `DaemonRequestFamily::Goals`. The
  new `Lsp` family is read-only, so no read request can be routed to a mutation
  handler and no mutation can be routed to a read handler.
- Authorization diverges from the generic projection gate by decision, not by
  omission: `file.read` rather than `project.observe`, because diagnostics carry
  source content. Both the pull and the stream enforce it, and the enforcement
  point is `operation_descriptor`, so `authorize_request` rejects before any
  handler runs rather than trusting a handler to check.
- The workspace root is never taken from a request. A caller cannot address
  outside the session's workspace even if it can reach the handler.
- Target containment reuses `validate_target_path` rather than adding a second
  containment path that could drift.
- `LspDiagnosticsUpdated` is not persisted, extending ADR-0008's bounded and
  non-durable ruling. The cost is recorded rather than hidden: a restarted client
  has no sequence history and must re-pull, which is why the pull is the
  correctness authority and the stream is only a latency optimization.
- Session-scoped consumers reject `ProjectionSnapshotBundle::LspDiagnostics`
  exactly as they do `BoundedSessionList`, so no session-scoped surface can
  observe project-wide diagnostics by accident.
- The protocol does not own a `SymbolKind` taxonomy; it reads the transparent
  i32. No 26-arm enum was added to the wire contract.
- No TUI rendering was attempted. The diff contains no TUI file.
- No `scripts/` change and no guard relaxation.

## 6. Failure and recovery review

- **Dropped stream envelope.** The project stream uses `try_send` and drops on a
  full channel, logging only a warning
  (`src/core/event_log.rs:180-186`). This is a pre-existing transport property,
  and M006-B is designed around it rather than against it: a dropped envelope
  costs freshness, never correctness, because the client reconciles against
  `LspDiagnosticsGet`. The resync contract is a monotonic sequence plus digest,
  and the digest is computed by the tracker rather than trusted from a caller.
- **Server not warm.** `NotReady`, payload-free, so a caller cannot render an
  unmeasured workspace as clean. Denial is a distinct error, so "no problems"
  and "we do not know yet" cannot be confused.
- **Store cap reached.** Reported to the caller rather than silently truncating.
- **Tracker file cap.** Bounded at 512 files per project; the bound is a
  reported condition, not a silent drop.
- **Cancellation.** Point reads are synchronous within the daemon request
  handler; there is no long-lived handle to leak. The poller is a single
  interval-driven task owned by the daemon.
- **Restart.** Diagnostics are not persisted, so a restarted daemon re-publishes
  from a clean tracker and a reconnected client re-pulls. There is no replay of
  a stale sequence, because there is no durable sequence.

## 7. Migration and compatibility review

None. This milestone adds protocol variants and events; it changes no schema, no
migration, and no stored representation. `STORAGE_LAYOUT_VERSION` is untouched
and no migration is added, because nothing durable changed.

Compatibility is additive on the wire: new `CoreRequest` variants, new responses,
one new `CoreEvent`, one new `ProjectionSnapshotBundle` variant, and one new
`ProjectionStreamKind`-resident event. A peer that does not know the variants
cannot issue them, and the new event is classified so that consumers which do
not handle it are explicit about ignoring it rather than silently
misinterpreting it.

MSRV 1.89 is preserved. No new dependency was added.

## 8. Security review

This milestone is the security-relevant one in M006, because it is the first
place the native protocol exposes language-server output to a client.

- **Authorization.** Both read families are gated at `file.read`, not at the
  generic `Opaque` + `project.observe`, because diagnostics embed source
  content. The gate lives in `operation_descriptor`, so authorization runs
  before dispatch.
- **The stream is gated too, not just the pull.** A design that gated the
  authoritative pull but left the push stream on the generic gate would have
  been a disclosure hole: the stream carries the same content, continuously. It
  is a dedicated gated request for exactly that reason.
- **Containment.** Targets resolve through the same primitive the mutation path
  uses, so a read cannot escape the workspace through a path the write path
  would have rejected. `MAX_TRACKED_FILES_PER_PROJECT` bounds the store.
- **No capability is widened.** The read surface adds no new mutation, no new
  execution surface, and no new authority. `ProjectCatalog` root, sanitized path,
  and overlap checks are all downstream of the same validation.
- **What remains true of the caller.** Every diagnostic is LSP-generated or
  `-Wall`-level annotation; M006-B adds no trust beyond what a warm server
  reports, and the dirty-buffer exclusion still prevents echoing unsaved editor
  content. That exclusion is `is_managed_document_dirty` in
  `src/lsp/mutation.rs:240`, which rejects a preview whose target has unsaved
  editor changes ("save and regenerate the preview") *before* any disk write,
  and it is covered by
  `dirty_document_service_rejects_preview_before_disk_write`
  (`src/lsp/mutation.rs:706`). That file is unmodified by this milestone, so
  the rejection is unchanged rather than merely still-present.

No secret, credential, or environment surface is touched.

## 9. Documentation and operations

- `plans/adrs/ADR-0012-native-lsp-read-surface-delivery-and-authorization.md` —
  the controlling decision, accepted and merged as `f7c8948a`.
- `plans/implementation/desktop-frontend-ide-foundation/008-native-lsp-read-surface.md`
  — this milestone's plan.
- This closure record.
- `architecture/lsp.md` — the read surface, the resync contract, and the
  warm-server-only rule.
- `architecture/authorization.md` — the `file.read` gating for both read
  families and the coupling between the gated subscribe and the `Safe`
  classification.
- `src/core/event_log.rs:37-57` — the `should_persist` decision to exclude
  diagnostics, with its cost.
- `plans/registry.md` — roadmap status, implementation-plan row, closure row,
  and the desktop-frontend gate paragraph.
- No new operator surface and no configuration. There is nothing to enable and
  nothing to disable.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | The holdout p95 assertion is subsumed by the per-scenario assertion and cannot fire. Found and proven while fixing the blocker for this PR. | No budget is unenforced — all 284 scenarios are asserted under budget, which is strictly stronger than a p95 — but a reader could mistake the redundant gate for an independent one. | Discharged here as a pointer: fixed and filed at `plans/closure/tool-selection-advisor-causal-frontier-timing-corrective/002-status.md` §10, where a retirement/consolidation decision is recorded as deliberately not taken, because removing the line would falsify a second accepted C001 record. |
| medium | `codegg::project_activation::concurrent_same_owner_activation_coalesces_scope_and_bundle` and a M004 Desktop E2E transcript flake, filed by C001 §10. | Keeps branches red for reasons unrelated to the causal advisor and re-truncates the sweep. | Its own corrective. Not absorbed here, and not observed to fire in this milestone's hosted runs — both were green. |
| low | Diagnostics are not persisted, so a restarted client has no sequence history and must re-pull. | One extra round trip after a restart. | Accepted and documented; it is the price of ADR-0008's non-durable ruling, and the pull is the correctness authority by design. |
| low | The project stream drops envelopes on a full channel, logging only a warning (`src/core/event_log.rs:180-186`). | Latency, never correctness, for this surface. | Pre-existing transport behavior. Left unchanged; the resync contract absorbs it. |
| low | `scripts/check_projection_transport_lifecycle.py` fails on `main` with `daemon_socket.rs: raw forwarder is spawned without an owned handle`. Verified pre-existing at `cde7dbfe` on a clean worktree. | A guard outside the CI quick subset. | Unrelated to this milestone. Noted so it is not mistaken for fallout. |

None of these are regressions introduced by M006-B.

Not fixed here, and deliberately:

- **Dirty-buffer LSP apply.** M006-E was scoped to saved documents only by
  explicit user decision, and its apply-into-dirty-buffer half is deferred to a
  future ADR. `src/lsp/mutation.rs` is unchanged.
- **`type_definition` and unwired `code_lens`.** `type_definition` does not
  exist in egglsp — only a doc comment at
  `crates/egglsp/src/operations/navigation.rs:15` — and `code_lens` exists but
  is unwired in `src/`. Neither is exposed by `LspReadOperation`, so this
  milestone does not advertise operations the server layer cannot serve.
- **Socket-mode LSP.** `src/main.rs:3323` builds `app.lsp_tool` only under
  `if !is_socket_mode`, and `:3169` likewise for `lsp_service`. The socket-mode
  TUI has no LSP, so this read surface is not reachable from that mode. Out of
  scope and not claimed.

## 11. Roadmap disposition

M006-B is closed. The native protocol now has LSP read operations for the first
time, which was the first of the two findings in the M006 presentation audit
("no LSP read operation exists in the native protocol"). The second finding —
that the M005 `TuiDocumentSession` foundation has no consumer — is addressed by
M006-C, not here.

M006-C is now unblocked: the read surface, its authorization, and its resync
contract all exist, and M006-C's work is presentation. M006-D and M006-E remain
closed and unaffected.

ADR-0012 remains controlling and unmodified. Its push-for-diagnostics decision
was the user's call against a pull-only recommendation, and the resync contract
this milestone implements is what makes that choice safe rather than a
drop-on-full tuning question.

## 12. Registry updates

- `plans/registry.md` roadmap/status table: M006-B recorded as implemented and
  closed, with ADR-0012 and the hosted evidence.
- `plans/registry.md` implementation-plan table: M006-B status advanced from
  *ready for handoff* to *implemented; closed*, pointing at this closure.
- `plans/registry.md` closure table: add M006-B, closed, with both hosted run
  ids and the resync result.
- `plans/registry.md` desktop-frontend gate paragraph: record that M006-B is
  closed, that the C002 timing blocker is discharged, and that M006-C is
  unblocked and is the remaining consumer of the M005 foundation.
- `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md`: M006-B status.
