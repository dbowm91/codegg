# TUI Editor Correctness and Usability M003 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tui-editor-correctness-usability/003-authorized-lsp-completion-contract.md`

Source subsystem roadmap:

- `plans/subsystems/tui-editor-correctness-usability-roadmap.md#M003`

Repository baseline reviewed: `6f6df8ba9`
(M003 activation: ADR-0021 Option B accepted, readiness mapping done).

Implementation commits or pull requests:

- `44671d690` — tui-editor(m003): authorized inert LSP completion read contract

## 1. Executive finding

M003 is complete as a capability infrastructure slice (a contract with no
consumer yet: that is M004's work, now unblocked). One typed, authorized,
bounded completion read returns inert candidates with explicit
Ready/NotReady/Stale/Unsynced/Unsupported states against the exact
synchronized snapshot. A cold request cannot launch a language server (the
warm-only path never reaches `get_or_create_client`, proven by a zero-process
cold test); a denied request never reaches one (denials precede every server
contact, proven by transcript assertions); controller text and disk bytes
never change under any read. Unsupported, snippet, multi-file, and
command-carrying data stays visible with a reason and is never silently
downgraded to label insertion. No popup, no edits, no mutation path: M004
owns all user interaction. No storage, migration, schema, or scheduler change.

## 2. Requirement-to-evidence matrix

| Requirement (plan §10 / ADR-0021) | Evidence | Result | Notes |
|---|---|---|---|
| ADR-0021 accepted with attachment/revision/LSP-version mapping | ADR status `accepted (Option B)` 2026-10-13; mapping table in `architecture/document.md` (controller `daemon_revision` → daemon buffer revision → `lsp_synced_revision` ledger) | pass | Acceptance recorded before any production code, per §11 |
| One typed authorized read, bounded inert candidates, Ready/NotReady/Stale/Unsupported states | `LspCompletionGet`/`LspCompletionResult` DTOs; `a_warmed_server_returns_typed_candidates_with_explicit_applicability` (9 items, all states of applicability, order + `is_incomplete` preserved) | pass | Plus `Unsynced`, the fifth state the plan requires |
| Cold request cannot launch LSP | `a_cold_read_is_not_ready_and_starts_no_process` (`NotReady`, empty, `client_keys` empty, Disabled service) | pass | Warm-only is structural: `completion_for_warm_client` takes a key |
| Denied request never reaches LSP | `scope_confusion_and_denied_callers_fail_closed` (outsider `authorization_denied`, wrong-project `authorization_denied`, transcript has zero completions) | pass | `via_session + file.read` descriptor; session→project resolution added for this variant (see §10) |
| Controller/disk never mutated by read | Disk bytes compared before/after in Ready test; revision echoes prove no bump; change test proves edits stay dirty buffers | pass | Handler takes no `&mut` document access; classification is pure over `&str` |
| Unsupported/snippet/multi-file/command never silently inserted | Snippet/AdditionalEdits/Command/OutOfRange/EditTooLarge/InsertAndReplace all `NonApplicable` with reasons, end to end | pass | `completion_bounded` untouched and not reused |
| Unicode/version/authority negatives | Surrogate split → `OutOfRange` at daemon; stale/superseded → `Stale`; `Unsynced` with warm server; cross-workspace → `lsp_read_not_found` | pass | UTF-16 validation shared with `position.rs` semantics |
| Old-daemon negative | Unknown envelope tags fail loudly (new tag roundtrips); client maps `Error`/unknown responses to `Rejected`, never inferred `Ready` | pass | No old daemon binary available; mechanism tested, not a live old binary |
| No hidden fallback | No sync walk, no label-insertion fallback, no `LspPreviewApply` reuse anywhere on the path | pass | Structural; `completion_response` maps only the new variant |
| New client API + docs + guards + hosted evidence | `lsp_completion` + `map_completion_response` + unit tests; 4 architecture docs; matrix + audit + text/project guards green; local `verify.sh quick` green | pass | Hosted exact-SHA CI not run (low finding, §10) |
| Caps: 100 candidates, 256 KiB, per-string + edit bounds | 101 items → 100 + `truncated`; 100×3 KiB edits → byte truncation below count cap; oversized edit → `EditTooLarge` uncut | pass | Display truncates, edits never truncate |
| Deterministic order / incomplete | Order asserted end to end; `is_incomplete: true` survives to the DTO | pass | `completion_for_warm_client` returns the flag; Vec fallback means complete |
| Scope confusion, reconnect/cancel, overlapping | Path/attachment mismatch → `invalid_lsp_read`; cross-workspace → not-found; reconnect re-query → `Ready`; late old-revision → `Stale` | pass | Reconnect sends no replay; cancellation is invalidation by revision |
| Cancel/reconnect transport semantics | Re-query on a fresh client answers `Ready`; no request is retried or auto-accepted by the client | pass | Frontend single-flight is M004's scope |

Before/after fixtures: pre-change `LspReadOperation` had no Completion
variant, `completion()` called `get_or_create_client`, no sync witness
existed (`set_managed_document` carried text+dirty only), and the TUI had
zero completion state. Post-change the dedicated family, warm-only query,
ledger, and classified inert candidates exist with the trajectory above;
`completion_bounded`, `LspReadGet`, and `LspPreviewApply` are byte-identical
in behavior (their suites green).

## 3. Production implementation evidence

- `crates/codegg-protocol/src/lsp.rs`: M003 DTO family — request
  (session/path/attachment/revision/UTF-16 cursor/trigger/cap with
  `validate()`), `LspCompletionStatus` (5 states), inert edit DTO,
  6-variant unsupported reason, applicability, candidate, result with
  identity/revision echo and `truncated`/`incomplete`; caps 100 / 256 KiB /
  200-char display / 8 KiB edit / 8-char trigger; `LspReadInvalid::InvalidAttachment`.
- `crates/codegg-protocol/src/core.rs`: `LspCompletionGet` request /
  `LspCompletionResult` response variants (wire tags `lsp_completion_get` /
  `lsp_completion_result`).
- `crates/codegg-core/src/authorization/policy.rs`: `lsp_completion_get`
  descriptor (`ViaSession + FileRead`) + representative request + dummy.
- `crates/codegg-core/src/audit_instrumentation.rs`: explicitly
  uninstrumented with the reads' rationale (capability-gated read, denial
  audited upstream).
- `crates/egglsp/src/operations/completion.rs`: `completion_for_warm_client`
  (warm key in, items + `is_incomplete` out; `completion()` refactored onto
  the shared core with identical behavior) and `classify_completion_items`
  (pure structural classification with snapshot UTF-16 validation).
- `crates/egglsp/src/operations/mod.rs`: re-export of the classification
  types (the `completion` module stays private).
- `src/document_service.rs`: `lsp_synced_revision` ledger (revisions only,
  never text) + `completion_binding()` + record/clear, including on both
  eviction paths.
- `src/core/daemon_documents.rs`: `sync_managed_document` records on push
  success, clears on every other exit (eviction clear matches the
  eviction-only `didClose`).
- `src/core/daemon_lsp_completion.rs` (new): `lsp_completion_get`
  (validate → daemon root → containment → attachment/workspace binding →
  locator coherence → Stale → warm/NotReady → Unsynced → capability →
  warm-only query → normalize + cap), pure `completion_sync_verdict`,
  `candidate_to_dto`, `cap_completion_candidates`, and unit tests.
- `src/core/daemon_lsp.rs`: dispatch arm + two new denial variants
  (`AttachmentNotFound` sharing the non-enumerable message/code,
  `ServerFailed`); three helpers widened to `pub(crate)` without behavior
  change.
- `src/core/daemon_family.rs`: route to `Lsp` (read authority only;
  `LspPreviewApply` stays in `Goals`).
- `src/core/daemon.rs`: `session_id_for_request` resolves the session for
  this variant, so team members with `file.read` pass and others are denied
  (see §10 observation on the sibling surface).
- `crates/codegg-client/src/lsp.rs`: `lsp_completion` (no synthesis, no
  replay, no inferred success) + pure `map_completion_response`.

Ownership preserved: no second document buffer (ledger holds `u64`s);
`DocumentController` untouched; agent preview/write path untouched;
`LspReadGet`/`LspPreviewApply` behavior unchanged.

## 4. Verification executed

### Commands run

```bash
cargo test --lib -p codegg-protocol lsp --locked
cargo test --lib -p codegg-client lsp --locked
cargo test --lib -p egglsp completion --locked
cargo test --locked --features lsp-test-support --test lsp_completion_trajectory
cargo test --locked --features lsp-test-support --test lsp_read_surface_trajectory
cargo test --locked --features lsp-test-support --test document_lsp_integration
cargo test --locked --test document_client_trajectory
cargo test --locked --test identity_m003_daemon_authorization
cargo test --lib --locked -- daemon_lsp_completion
cargo test --lib --locked -p codegg-core authorization
cargo test -p codegg-core --lib audit --locked
python3 scripts/check_authorization_matrix.py
python3 scripts/check_audit_coverage.py
python3 scripts/check_tui_editor_text_authority.py
python3 scripts/check_tui_project_authority.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
scripts/verify.sh quick
```

The focused trajectory target did not exist and was created with a
`lsp-test-support` gate in `Cargo.toml`, matching `document_lsp_integration`.

### Results

- Protocol `lsp`: 23 passed / 0 failed (11 new completion tests).
- Client `lsp`: 11 passed / 0 failed (2 new mapping tests).
- egglsp `completion`: 32 passed / 0 failed (12 new classification tests).
- `lsp_completion_trajectory`: 12 passed / 0 failed (all new; 9 daemon +
  fake-server scenarios, 3 wire-shape tests).
- `lsp_read_surface_trajectory`: 14 passed (unchanged behavior).
- `document_lsp_integration`: 1 passed (push path with ledger recording).
- `document_client_trajectory`: 3 passed. `identity_m003`: 9 passed.
- `daemon_lsp_completion` unit: 9 passed. `codegg-core authorization`:
  21 passed. `codegg-core audit`: 32 passed.
- Matrix, audit-coverage, text-authority, project-authority guards: pass.
- `cargo fmt --check`: pass.
- `cargo clippy --workspace --all-targets -- -D warnings`: pass.
- `scripts/verify.sh quick`: pass (all routine + change-triggered guards,
  workspace check).

Local truth only. Hosted root CI and any hosted runs were not executed from
this environment (see §10). All suites above ran locally on `44671d690`.

## 5. Invariant review

- Daemon is sole LSP owner: warm-only query takes an existing key; cold
  test proves zero starts. The pre-existing `completion()` spawn path is
  untouched and unreachable from the daemon.
- Caller path/revision are locators/preconditions: containment via the write
  path's primitive; attachment bound to the session workspace by canonical
  roots; locator disagreement rejected; revision mismatch is `Stale`.
- `via_session + file.read`, no weaker shortcut: descriptor row + preamble;
  outsider and wrong-project callers get `authorization_denied`.
- Candidates inert: no command execution (a command *marks* non-applicable);
  no snippet/additional-edit application path exists.
- Exact snapshot binding: triple comparison; optimistic revisions never
  assumed; `synced_revision` echo lets clients discard mismatches.
- Bounded, non-durable: 100 / 256 KiB / per-string / per-edit caps; no
  persistence, no session-history writes.
- M006-C / ADR-0012 behavior unchanged: read-surface suites green.
- Agent LSP preview/write path untouched: no shared code changed.

## 6. Failure and recovery review

- Cold/warming: `NotReady`, no payload, no process, retryable later.
- Stale caller: `Stale` with daemon binding echoed for rebase; late
  old-revision responses can never become applicable.
- Unsynced daemon: `Unsynced` with binding echoed; a re-push through any
  ordinary document path re-establishes the witness.
- Push failure: witness cleared (fail toward `Unsynced`, never toward a
  false `Ready`); `lsp_degraded` still reported on the document path.
- Daemon restart: in-memory ledger drops → `Unsynced` until re-push (safe
  direction; noted in §10).
- Server query failure after warm checks: `ServerFailed` error, distinct
  from denial and `NotReady`.
- No service in process: `ServiceUnavailable` denial.
- Unknown attachment / cross-workspace: non-enumerable not-found.
- Reconnect: fresh client + explicit re-query; no replayed acceptance.
- Capability missing: `Unsupported`, not empty `Ready`.

## 7. Migration and compatibility review

Additive native variants only; `PROTOCOL_VERSION` unchanged. Old daemons
fail the unknown envelope tag loudly (tested); the client maps that to
`Rejected`, never to success. Old clients never send the new tag. No
database migration, no persisted candidates, `document.v1` identity reused,
`LspReadGet`/`LspPreviewApply` shapes preserved. `session_id_for_request`
gains one arm (see §10); nothing else in the auth dispatch changes meaning.

## 8. Security review

`file.read` gate before any server contact (outsider/wrong-project
trajectories). Non-enumerable attachment denials (shared message/code with
the read surface). No path traversal past containment (write path's
primitive). No secrets in denials, logs, or telemetry (counters only in
future M004 scope; this milestone logs nothing per-request). Untrusted
server strings truncated as display-only; edits never truncated. No new
mutation authority: the only new descriptor is a read. `command` fields are
data, never dispatched. No document text in denial messages.

## 9. Documentation and operations

- `architecture/lsp.md`: completion read subsection (dedicated family
  rationale, warm-only + precedence, witness design, inert classification).
- `architecture/document.md`: sync-witness subsection with the
  controller→daemon→LSP mapping table (work package A deliverable).
- `architecture/authorization.md`: matrix row + prose gate note.
- `architecture/protocol.md`: `Lsp*` request count 4→5 + file-map line.
  (`architecture/overview.md` Verified counts not touched: its
  CoreRequest/CoreResponse/methods totals were already stale at baseline
  and use a different counting methodology; see §10.)
- Operational signals: five statuses, `truncated`/`incomplete` flags,
  identity/revision echo for client-side discard, per-reason
  non-applicability for M004 display.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Hosted root CI not run on `44671d690` | Strict closure prefers exact-SHA hosted green; local `verify.sh quick` + clippy + all focused suites substitute locally | Run hosted CI on the merge head; record runs here by factual correction if they fail |
| low | No live language-server roundtrip (fake-server scenarios only) | Real-server tests are excluded from default sweeps by policy; behavior against rust-analyzer quirks (e.g. `InsertAndReplace` frequency) is unqualified | M004 qualification (§E live-server pass) should prefer a real server for at least the happy path |
| low | Daemon restart drops the in-memory witness → `Unsynced` until re-push | Safe direction (never false `Ready`), but a restarted daemon reports `Unsynced` for documents the replayed server actually holds | If this proves noisy in practice, persist the witness or re-push on replay; new plan, not a fix here |
| low | `session_id_for_request` now resolves the session for `LspCompletionGet` but still not for `LspReadGet` | Deliberate per plan §3 (team `file.read` must work for completion); the sibling reads remain local-owner-effective for team principals. A future "harmonization" must not silently change either surface | Recorded here so the divergence is visible; any change needs its own plan |
| low | `architecture/overview.md` Verified counts stale (CoreRequest/CoreResponse/test-file totals predate this branch) | Unchanged by this milestone in methodology; my deltas are +1/+1/+1 by the guard's own regex | Recount under the review skill separately; not this closure's scope |

No critical/high/medium findings. No corrective pass required.

## 11. Roadmap disposition

Milestone M003 closed. M004's hard gates (M003 + C001 closed; C002 closed
for integration) are all satisfied, so M004 moves to `ready` in the same
commit. No other registered plan lists M003 as a dependency. No scope
expansion into presentation, snippets, or additional edits.

## 12. Registry updates

- `plans/implementation/tui-editor-correctness-usability/003-authorized-lsp-completion-contract.md`:
  `active` → `implemented; closed` (`44671d690`, this closure).
- `plans/subsystems/tui-editor-correctness-usability-roadmap.md`: M003
  section/row `active` → `closed` with closure link; M004 section/row
  `blocked on M003 and C001 closure` → `ready` (all gates satisfied).
- `plans/registry.md`: subsystem row to `active (C001–C002 closed; M003
  closed; M004 ready)`; M003 row `active` → `closed` with closure +
  implementation refs and full verification summary; M004 row `blocked` →
  `ready` with resolved-deps note; unblock audit recorded (M004 unblocked,
  nothing else affected).
