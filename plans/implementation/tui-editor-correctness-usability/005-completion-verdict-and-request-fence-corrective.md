# TUI Editor Post-Closure Corrective C003 — Completion Verdict and Request-Fence Correctness

Status: ready for handoff

Baseline: `plans/tui-editor-correctness-usability-20261013` at `0266de94780e6f69c8f0fe6f1cfb44ea0aeb4b56` (2026-10-13); recheck branch head before changes.

Source corrective addendum: `plans/subsystems/tui-editor-correctness-usability-post-closure-corrective-addendum.md` C003.
Predecessors: M003 (`plans/closure/tui-editor-correctness-usability/003-status.md`; `44671d690`) and M004 (`plans/closure/tui-editor-correctness-usability/004-status.md`; `fd2faaaf1`), historically closed. This is new corrective evidence, not a rewrite of either closure.
Binding ADRs: ADR-0011, ADR-0012, ADR-0021 (accepted Option B).
Class: correctness/invariant; no protocol or storage changes expected.

## 1. Objective

Preserve every legitimate completion status exactly as returned by the daemon, prevent an old in-flight result from being admitted after popup dismissal/reconnect/clear or reuse of a request identifier, and bound completion-trigger Unicode-position computation on pathological single-line documents. Preserve the existing single-transaction, revision-checked edit and undo path.

## 2. Current implementation evidence and defects

**F1 — status treated as invalid revision (medium; demonstrated by inspection).** `crates/codegg-protocol/src/lsp.rs::LspCompletionResultDto::not_ready` constructs `revision=0`, while `stale` returns the daemon's authoritative current revision, which intentionally differs from the caller's. `src/tui/editor_completion.rs::EditorCompletionState::apply` rejects any result where `result.revision != scope.revision` *before* matching the result status. Thus a valid cold `NotReady` for a nonzero-revision file and a true stale response both present the generic `Error` “another document revision” rather than “language server warming” / “document changed”. The M004 `every_daemon_status_maps_to_a_visible_state` test changes the status on a `ready_result("doc-1", 7)`, bypassing the actual wire constructors.

**F2 — request ID reuse after invalidation (medium; reproducible state transition, exploitability conditional).** `EditorCompletionState::clear` resets the whole struct including `request_id`, while `begin` increments it from zero. `src/tui/app/project_session.rs::on_projection_reconnect` clears completion without changing `editor_state.generation` or `EditorLspScope` if attachment is unchanged. A new `begin` can reuse ID 1 and identical scope after a reconnect, so an old `EditorCompletionFinished` arriving after a new request could satisfy `scope + request_id + loading` guards. The task registry abort is not sufficient because a transport completion may already be queued before disconnect. Consequence: the stale response could overwrite a new popup; actual edit is still subject to controller revision, but the result must never be admitted.

**F3 — unbounded trigger-time line prefix (medium performance).** `src/tui/commands/editor.rs::current_completion_scope` executes `snapshot.read_bytes(row_start, cursor)` to calculate a UTF-16 column. This reads the whole prefix of a logical line even where C002's renderer strictly limits hot-frame source reads to 32 KiB/line and 256 KiB/frame. F8 on a multi-megabyte single logical line can allocate and scan proportionally to the cursor column; C002 render tests do not exercise this command path.

Additional defensive concern: `EditorCompletionState::apply` stores returned candidates for any status, not just `Ready`. The trusted daemon currently constructs empty non-ready payloads, but a malformed/old peer must never expose actionable candidates in `NotReady/Stale/Unsynced/Unsupported` state.

## 3. Invariants

- `DocumentController` remains the only editable text replica. Completion does not write unless a user explicitly accepts an applicable, current, scoped edit through the existing local transaction path.
- Daemon completion authority (warm-only; `via_session + file.read`; synchronized attachment witness) unchanged. No changes to `LspPreviewApply` or model-facing completion.
- Different logical completion requests never share a frontend fence within a running TUI, regardless of `clear`/`dismiss`, tab switch, attachment reopen, reconnect or task cancellation. No counter reset that revalidates an old completion.
- Status semantics: `Ready` is bound to exact current requested revision; `Stale` may legitimately echo a newer authoritative revision; `NotReady` may return the existing documented placeholder (0); other non-ready statuses preserve typed semantics without admitting suggestions. Document identity and request envelope scope always validated. Non-ready results cannot become actionable.
- Render budget is not weakened. UTF-16 positions are exact on supported lines, including surrogate pairs; over-budget positions fail visibly and without emitting a wrong LSP query.
- Result contents, paths and source text are not logged in debug errors or telemetry.

## 4. Required production changes

**Core/protocol:** Prefer pure frontend correction, preserving the existing `LspCompletionStatus` and revision DTO. If a change to non-ready DTO semantics is proposed, stop and assess ADR-0021 compatibility and client version skew first; status semantics MUST NOT be altered merely to placate a frontend equality check.

**Frontend status mapping:** Split *request/result identity binding* from *status-specific revision policy*. Verify `document_id` exactly for all statuses. For `Ready` require `revision == scope.revision` and, where applicable, `synced_revision` consistent with the request; reject a malformed Ready payload. `Stale` may echo the daemon-current revision; display stale and disarm every candidate. `NotReady` with its constructor's placeholder revision is normal, not error. `Unsynced` and `Unsupported` display their typed state with no actionable candidate and no assumed revision equality. Check impossible combinations defensively (e.g. non-ready with candidates) and clear candidates on any non-ready response. Preserve `None/error` and unexpected daemon response behavior, with bounded messages.

**Freshness fencing:** Use a monotonic lifetime request token or a (reconnect_epoch, request_sequence) pair that is never reused over `EditorCompletionState::clear` or `reset_for_scope`; sequence arithmetic must not wrap back into a live request ID. If a new field is required in `TuiCommand::EditorCompletionFinished`, pass it and check it on receipt against current connection epoch and request identity. `dismiss` and `clear` invalidate the currently live token even when the same session/document/revision is reopened. Do not use `DocumentController` revision alone as a transport-epoch fence. Limit pending tasks/cancel superseded requests if practical; correctness must not depend on task abort being synchronous.

**Trigger position:** Replace full-prefix `read_bytes(row_start, cursor)` with a bounded/chunked, safely validated UTF-16 locator. Explicitly define a maximum work budget; for a cursor beyond the covered prefix, refuse `F8/:complete` with a clear “line too long for completion at this position” notice rather than sending a truncated/wrong UTF-16 character value. For ordinary lines preserve exact column and Unicode behavior, and do not perform filesystem or daemon I/O on render path. If controller snapshot exposes a suitable sublinear index, reuse it without caching second document text.

**Docs/guards:** Document status/fencing behavior and long-line limitation in `architecture/tui.md` and `docs/tui.md`. Extend existing editor authority and completion coverage guards if necessary; no broad app/keybinding refactor.

## 5. Ordered work packages

A. **Failing real DTO and race tests FIRST.** Reproduce exact `not_ready(document_id)` on revision 7 → expected `NotReady`; `stale(document_id, revision=8, synced=Some(8))` on request revision 7 → expected `Stale`. Show pre-fix Error and document the regression. Add malformed `Ready` and non-ready-with-candidates negative test. Reproduce old request ID after `clear`, `dismiss`, reconnect, repeated attachment close/reopen and late queued old `EditorCompletionFinished` (old result first and new result first). Capture old/new token equality before fix.

B. **Repair status/identity mapping.** Implement typed status policy without weakening document/attachment checks. Show `Enter` and `selected_candidate` cannot accept anything for non-ready outcomes. Ensure a stale result can show the server revision only as a message, not automatically rebase or retry.

C. **Make request identity lifetime unique.** Introduce durable-in-memory token source owned by editor/app for lifetime of process, or typed epoch+sequence, and fence dispatch, popup state, and receive. Invalidate on reconnect, switch, local edit, cancel/dismiss and lifecycle. Validate task-cancel and queued-completion race with deterministic barriers, not sleeps; telemetry counts only.

D. **Bound position calculation.** Test 1–4 MiB one-line text at cursor start/middle/end, as well as ASCII, tabs, combining marks and astral Unicode. Instrument `read_bytes` input byte count and allocations, prove a specified finite cap and explicit over-cap refusal, with no incorrect request, freeze or document mutation.

E. **Integrated qualification.** End-to-end `F8`/`:complete` → actual native client result → typed popup for each real constructor, and Ready → one applicable transaction → one undo/redo unit; cold/late/malformed results cannot edit. Prefer one real `rust-analyzer` happy-path smoke when available; at minimum use deterministic wire-level fake-server tests covering synchronized dirty document, warm/cold and Unicode without claiming the real server was exercised. Run hosted CI at exact candidate head and document result; standalone local CI cannot substitute for a completed hosted gate.

## 6. Failure, contention, restart and recovery

On reconnect, old tasks and queued completions are invalid even with identical tab/session/path/attachment/revision. Process restart gets a fresh process-local token namespace; no candidate persistence or replay. Dismissal is side-effect-free. Non-ready responses must be visible and non-actionable, with no speculative retry or empty-Ready fallback. Writer lease and controller failure leave text and undo history untouched. Bounded position conversion refuses rather than approximates. Do not introduce timers/polling to mask the issue.

## 7. Storage, compatibility, security

No schema, migration or native wire shape change expected. Keep old-daemon unsupported response explicit and native `file.read` gate before any LSP contact. No new permissions. Status display may become more correct for old clients; never turn denial into `NotReady`. No text or candidate bodies persisted to history.

## 8. Required verification

- Unit: exact DTO constructors; status/digest/identity combinations; scope mismatch; hostile non-ready candidates; clear/dismiss/epoch/token uniqueness; over-budget UTF-16 position.
- Integration: scripted TCP/socket equivalent late response reordering across reconnect, tab switch and same-revision reopen; one applied Ready edit/undo/redo; cold and unsynced; actual editor keyboard route; no source edits on error.
- Security: authorized scope only, malicious candidate strings, no command dispatch, no wrong UTF-16 surrogate offset, no leaking payloads in logs.
- Compatibility: C001/C002 behavior, editor prompt focus, `/files`, side panels and PTY routing unaffected.

~~~bash
cargo test --lib tui::editor_completion --locked
cargo test --lib tui::commands::editor --locked
cargo test --test tui_completion_trajectory --locked
cargo test --test lsp_completion_trajectory --locked
cargo test --test document_client_trajectory --locked
python3 scripts/check_tui_editor_text_authority.py
python3 scripts/check_tui_project_authority.py
python3 scripts/check_authorization_matrix.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
scripts/verify.sh quick
~~~

Validate suite names against HEAD and record substitutions; tests for real `NotReady`/`Stale` constructors are mandatory. A qualifying hosted `CI / verify` check on the exact corrective SHA (plus Desktop E2E status, if applicable) must be recorded as factual evidence, not asserted prospectively.

## 9. Acceptance criteria

1. Exact daemon `NotReady` and `Stale` constructors render their own statuses for nonzero request revision; neither is reported as a generic identity Error.
2. Malformed/wrong-document Ready cannot populate applicable candidates; non-ready statuses never carry actionable suggestions or accept Enter.
3. No old completion result can populate a newer request across dismissal, clear, reconnect, same-revision reuse, close/open, tab switch or task cancellation. Barrier-driven reordering tests pass in both orders.
4. `F8` on a giant line uses bounded input work and never sends a wrong UTF-16 cursor; normal Unicode still resolves precisely.
5. Ready completion retains one controlled edit with undo/redo, no text-authority or authorization regression.
6. Focused, quick, static, hosted checks and evidence matrix are captured; unrun live-server checks explicitly identified.

## 10. Stop conditions

Stop if solution requires bypassing `DocumentController`, weakening revision/identity checks, emitting speculative wrong UTF-16 cursor, silently changing ADR-0021 public DTO semantics, or hiding a status with an “unexpected response” fallback. Do not rearchitect LSP completion or introduce auto-trigger/snippet/edit-command support.

## 11. Closure evidence required

Create `plans/closure/tui-editor-correctness-usability-post-closure-corrective/003-status.md`. Include original milestone/closure links, before/after test that actually triggers each defect, token freshness proof, real/fixture wire status matrix, performance input budget, exact implementation commits and hosted run URLs, diagnostics/no mutation, residual findings by severity, docs, and an explicit strict/conditional/blocked disposition. Preserve prior M003/M004 closure history untouched.

## 12. Handoff

This is a single bounded frontend correctness pass, independent of C004/C005. Begin with the real DTO constructor tests and reconnect token race. Do not treat the existing status-mapping unit test as proof: it uses a fabricated matching-revision response.
