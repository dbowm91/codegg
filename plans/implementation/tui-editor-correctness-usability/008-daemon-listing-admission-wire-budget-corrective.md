# TUI Editor Post-Closure Corrective C006 — Daemon Directory Scan Permit and Wire-Size Bound

Status: implemented; closed — implementation `7995f8bf` on branch `plans/tui-editor-correctness-usability-20261013`; closure `plans/closure/tui-editor-correctness-usability-post-closure-corrective/006-status.md`.

Baseline: branch `plans/tui-editor-correctness-usability-20261013` at `f5cc17f24dc05a9e73938e2fd3328f23f5207f53` (2026-10-13); inspect current HEAD and preserve concurrent changes before starting.

Parent corrective roadmap: `plans/subsystems/tui-editor-correctness-usability-post-closure-corrective-addendum.md`, C006.

Original milestone: C005 (`plans/implementation/tui-editor-correctness-usability/007-daemon-scoped-directory-listing-corrective.md`), implemented `e37774a9` and closed at `plans/closure/tui-editor-correctness-usability-post-closure-corrective/005-status.md`. Retain its historical closure and evidence unchanged; this is a distinct post-closure corrective.

Authoritative specs: `plans/000-long-term-specification.md`, `plans/001-terminology-and-domain-model.md`, `plans/002-long-term-roadmap.md`, `plans/003-planning-process.md` `7.
Accepted ADRs: ADR-0010 (shared client), ADR-0011 (document ownership), ADR-0012 (daemon-owned read), ADR-0018 (workspace-relative identity), ADR-0022 (daemon-owned directory listing; accepted Option B **as recorded**, acceptance-date provenance must be reconciled as described below).
Classification: correctness / resource invariant, one bounded daemon-listing corrective.

## 1. Goal

Close two concrete C005 gaps without changing the selected daemon-owned, read-only directory-listing architecture:

1. Preserve daemon-wide **single-flight admission until the actual blocking filesystem operation exits**, even when its owning Tokio handler is aborted, cancelled, times out, disconnects or panics.
2. Guarantee every successful `CoreResponse::WorkspaceDirectoryList` **complete serialized response payload**, not merely its `nodes` vector, fits `MAX_DIRECTORY_LISTING_RESPONSE_BYTES = 512 * 1024` bytes. A single oversized top-level directory and its nested descendants must not escape the cap.

Also reconcile the ADR-0022 acceptance-date/provenance inconsistency before representing this workstream as strictly closed. Keep source authority, authorization, DTOs and UX intact.

## 2. Why the original C005 closure did not prove these

### F1 — permit owned by abortable handler (medium-high, source-confirmed)

`src/core/daemon_documents.rs::directory_listing` acquires `OwnedSemaphorePermit` into local `_permit` in the **async** handler, then awaits `tokio::task::spawn_blocking(move || blocking_directory_scan(&root, &prefix))`. If the handler is dropped during that await (request cancellation/transport teardown), `_permit` drops *immediately*, but `spawn_blocking` may continue doing filesystem I/O. Another call can acquire the free permit and start a second blocking scan. Thus `src/core/daemon.rs::directory_listing_permits` documentation (permit held through blocking completion) does not match all execution paths.

C005's `tests/directory_listing_c005.rs::held_admission_answers_busy_and_releases_to_ready` manually holds a permit and tests contention, while `concurrent_bursts_all_settle_without_piling_workers` completes normal requests. Neither cancels an active **handler after its blocking worker has begun**, so the genuine race is untested.

### F2 — wire cap checks only node vector, retains last oversized node (medium, source-confirmed)

`src/core/daemon_documents.rs::trim_to_budget` serializes `nodes` only. Its loop stops at `nodes.len() == 1` and returns that subtree even if still over budget; nested children can make one root exceed 512 KiB. The caller then appends `workspace_id`, `prefix`, `status`, `examined`, `truncated_reason`, response discriminator, and envelope fields (depending on the actual transport encoding) **after** budget checking. Serialized response can therefore exceed the declared limit even when the nodes-only vector nominally fits.

C005's `trim_to_budget_keeps_fitting_listings_untouched` and `trim_to_budget_trims_trailing_nodes_with_a_reason` cover short flat trees; they do not create a sole massive nested root or assert the byte size of the **actual response**.

### F3 — ADR approval chronology (documentation/provenance, not an assumed code defect)

`plans/adrs/ADR-0022-daemon-authorized-workspace-directory-listing.md` records “Acceptance: 2026-10-14” and claims `e37774a9` implemented *after* that acceptance. At this review (2026-10-13) the repository commits `e37774a9` and `193a4441` are dated 2026-10-13 UTC. Consequently the date/temporal ordering is internally inconsistent. No independent maintainer approval timestamp has been verified. Do **not** fabricate one or silently backdate acceptance. Locate an authoritative approval/decision record, correct the documentation with a citeable commit/PR/time, or mark the chronological claim unverified while preserving the actual architectural choice and escalating to maintainers. Any genuinely unapproved deviation from ADR-0022 needs an explicit decision, not post-hoc assertion.

## 3. Dependencies, invariants and non-goals

**Hard dependencies satisfied:** C002/C004 bounded traversal and C005 daemon-owned listing implementation. This corrective can begin immediately on its branch; no new ADR is needed to implement the existing single-permit and response-cap **intent**. If implementation proposes a different subtree-retention policy from ADR-0022's frozen “trim trailing top-level nodes” rule, stop and obtain an explicit ADR clarification first.

**Critical invariants:**

- All filesystem enumeration remains daemon-side under `CoreRequest::WorkspaceDirectoryList`, `DirectProject + FileRead`, daemon-resolved project/workspace root, validated prefix and no symlink following. The TUI never reads a local directory as fallback. `DocumentOpen` remains the only authorized selected-file open path.
- Exactly one daemon directory-listing blocking worker holds the one permit **from admission through actual worker completion**; aborting the async waiter cannot free it. No unbounded extra blocking tasks or implicit queuing. Concurrent requests return typed `Busy`, not a partially successful listing.
- Resource and containment caps continue: max depth 12, retained nodes 4096, examined entries 100000 including iterator errors/ignored items, entry name 255 bytes, and **full serialized listing response ≤ 524288 bytes**. Do not silently return an over-budget node.
- `Ready + nodes=[] + truncated=true` is distinct from genuinely empty `Ready`; if all top-level nodes must be dropped to obey the cap, provide an explicit bounded truncation reason visible in `/files` and Ctrl+T. `Busy` / `Unavailable` / authorization denials remain distinct and bounded.
- No text-buffer ownership changes, no raw file reads, no untrusted command execution, no sensitive directory entry names in logs, no permanent directory listing cache.
- Worker cancellation is cooperative at filesystem iteration checkpoints; it cannot forcibly interrupt a blocking kernel filesystem call. Correctness is a resource-admission guarantee, **not** a wall-clock guarantee.
- Preserve one shared walker `src/directory_listing.rs`; both client presentations still use daemon-origin typed metadata.

**Non-goals:** `@`-mention file indexer migration; new remote filesystem protocol; pagination/streaming; broad task-scheduler refactor; syntax highlighting/completion changes; worktree multiplexing; changing the accepted authorization policy, role grants, wire DTO shapes or file open/save behavior; full two-physical-host proof (already deferred by C005).

## 4. Required production changes

### A. Blocking worker owns its permit

In `src/core/daemon_documents.rs::directory_listing`, move the **owned** admission permit into the `spawn_blocking` closure (or a small worker-owned RAII guard that owns it), before the filesystem read can begin. It is released only when the blocking closure exits, including error and panic-unwind. Dropping/aborting its Tokio JoinHandle or outer CoreDaemon request must **not** release admission while the closure continues.

If the handler exits before spawning or `spawn_blocking` rejects the job, release the permit without leaking it. If optional request-scoped cooperative cancellation is introduced, send an `Arc<AtomicBool>` to the worker and flip it via a drop guard on handler cancellation; never free the permit until the worker actually finishes. Avoid cancellation guards that signal a false cancellation during successful execution. This optional optimization is not allowed to mask a missing admission proof.

Do not globally block Tokio threads to await a cancelled filesystem syscall. Continue returning existing `Busy` under contention, `Unavailable` on worker failure, and `Ready` on a successful authorized scan. Ensure one daemon-wide semaphore shared across all client instances; do not create a new semaphore per request.

### B. Bound the actual serialized response

Replace `trim_to_budget` with a **full-response** budget enforcer that considers the exact serializer used by the native transport and all response fields (`CoreResponse::WorkspaceDirectoryList { result }` and relevant envelope, if part of the protocol's stated response limit). Measure the object that is actually transmitted; do not count only `nodes`, character count, or an estimated per-node size. Use a deterministic, bounded-cost strategy. An incremental accumulator or monotone binary search for the longest retained **top-level prefix** is preferable to repeatedly serializing almost the entire 4096-entry tree O(n²) times.

The accepted ADR-0022 currently freezes **trimming trailing top-level nodes**. Implement that policy all the way to **zero** retained roots if even the last nested root cannot fit. Preserve the retained prefix's ordering/paths/kinds and mark `truncated=true` with a short bounded reason. Do **not** keep one oversized root “because one root always fits.” If preserving a large root by pruning nested descendants is considered necessary for usability, first seek an ADR amendment/explicit approval; no silent deviation to recursive trimming.

Count final `truncated_reason` (including combined walker and wire-limit notices), workspace/prefix, status, examined, discriminator, and any size-significant transport wrapper in the cap. If pathological metadata/overhead leaves even an empty Ready response too large, return an explicitly bounded error/Unavailable rather than an oversize payload or panic. Ensure serialized Ready, Busy and Unavailable responses all respect their documented budget.

### C. Canonical test seam and instrumentation

Add a small private worker-spawn/admission seam or test-only injected blocking scanner with deterministic barriers. Instrument **actual blocking-worker entry and exit**, not merely Tokio task/handler counts or `Semaphore::available_permits()`. Do not persist file paths in telemetry.

Add a helper/assertion that serializes the final `CoreResponse::WorkspaceDirectoryList` using the same framing/serialization as transport and checks ≤524288 bytes. Test actual maximum/worst-case nested payloads and escaped/Unicode JSON names. Include a negative test proving the pre-fix function would exceed the cap and the post-fix function does not.

Avoid flakiness: use barrier/notifiers rather than sleeps and don't assert arbitrary sub-5ms wall-clock limits on CI.

## 5. Ordered work packages

1. **Reproduce F1.** Start an authorized listing; block the injected blocking scanner *after* it has acquired the permit and entered. Cancel/abort the outer async handler and assert the first scanner remains active. Before releasing the scanner, invoke a second authorized listing; pre-fix it can enter (incorrect), post-fix it must answer `Busy` with no second scanner started. Release first worker, await its actual exit, assert permit count restored to one and third authorized request may be `Ready`. Include worker `Err`/panic and abort-before-start conditions, with a deterministic peak-live-scanners counter.
2. **Implement worker-owned permit / drop safety.** Move owned permit into worker closure; any `Drop`/join/cancel guard must be mechanically correct under async handler cancellation, panic unwind, and shutdown. Keep `Semaphore(1)` and explicit `Busy` semantics. Ensure aborted caller never yields a late authorized response to a new scope; TUI request/generation/epoch fences still apply.
3. **Reproduce F2.** Build legal-depth/entry-count DTO trees with one nested top-level root and enough children/paths to exceed 512 KiB, plus flat near-threshold roots, Unicode/JSON escape amplification, and maximal/combined truncation messages. Assert **full response** serialized byte size, not nodes-only size; verify the pre-fix helper retains an oversize single root.
4. **Enforce the complete budget.** Trim trailing **whole** top-level nodes to zero if needed, retaining prefix order; set `truncated=true` with bounded reason. Recompute/measure the **final** serialized envelope; don't silently keep a root. Keep budget-enforcer asymptotics reasonable under 4096 accepted nodes; tests should account for serialization attempts or measured work to prevent O(n²) regressions.
5. **Integration/regression.** Exercise actual daemon dispatch and authorization with `tests/directory_listing_c005.rs` or a new `tests/directory_listing_c006.rs`: `DirectProject + FileRead`, denied/unauthorized principal, prefix traversal, symlinks, file opened via unchanged `DocumentOpen`, rapid parallel clients, first-handler abort, reconnect/daemon shutdown and old-daemon fail-closed response. Verify `/files` and Ctrl+T render the wire-cap empty truncated case visibly; a single-root oversized tree is not mistaken for an empty project.
6. **ADR chronology and documentation reconciliation.** Check real commit dates and any review/approval record. Fix a demonstrably incorrect timestamp if supported by evidence; otherwise add an explicit provenance uncertainty note to the ADR/closure reference, without asserting a verified approval order. Do not alter the frozen Option B contract or rewrite C005's closure. Update `architecture/tui.md`, `architecture/protocol.md`, `docs/tui.md` for exact single-flight and response-cap semantics.
7. **Closure qualification.** Run focused tests, static guards, fmt, workspace clippy and quick verification. Record exact-head hosted CI plus Desktop E2E where relevant. Close only with an independent `plans/closure/tui-editor-correctness-usability-post-closure-corrective/006-status.md` documenting the before/after race, measured wire bytes, actual tested SHA and remaining limitations.

## 6. Failure, cancellation, contention and restart contract

- **Normal:** validated + authorized request acquires the one permit; blocking scan runs; worker exits and frees permit; daemon returns a validated bounded Ready result.
- **Contended:** another request while actual worker alive receives Busy; it never starts another scan.
- **Client disconnect/abort/timeout:** outer response may be abandoned but *actual* blocking scanner owns permit until it exits. Cancellation flag (if implemented) expedites termination at checkpoints; no new scanner may start early.
- **Worker panic / I/O error:** worker-owned guard releases the permit at actual exit; active client sees sanitized Unavailable, not Ready with fabricated empty listing. Never log private paths.
- **Shutdown:** do not claim a blocking OS syscall was forcibly terminated; cancellation and permit release follow actual completion. Daemon shutdown policy must not accumulate a second scan or misreport a completed worker.
- **Too-large listing:** deterministic retained top-level prefix, explicit truncation reason even if zero nodes; no partial bytes or oversize response; normal small listings unchanged. Auth denials and old-daemon unsupported remain fail-closed.
- **Cross-client scope:** all clients share the daemon semaphore, but each retains separate authenticated project context. No data from a cancelled or stale request can be adopted by another TUI tab.

## 7. Tests and verification checklist

New tests **must include**:

- `aborted_handler_does_not_release_permit_before_blocking_worker_exit`: held blocking scanner + abort handler + second Busy + first worker exit + third Ready; exact active/peak worker counter asserts 1.
- `panic_and_failure_release_permit_when_worker_exits`; `abort_before_worker_starts_does_not_leak_permit`; `hundred_concurrent_clients_never_enter_second_blocking_scan` (blocked worker held; Busy stable).
- `single_nested_root_over_512k_is_dropped_with_truncation`, `full_core_response_serialized_under_512k_with_metadata`, `near_limit_unicode_and_escape_payload`, `combined_truncation_reasons_are_counted`, `all_roots_removed_is_visible_truncated_not_empty_success` and `small_listing_keeps_original_order_content`.
- Valid/invalid project+workspace scope, `file.read` principal denial, symlink/traversal rejection, old daemon fail-closed, `DocumentOpen` separate reauthorization, TUI `/files`/tree display.
- Guard/compat: no TUI local directory enumeration, no change to authorization matrices or wire DTOs, no text-buffer ownership changes.

Suggested commands (verify current target names on HEAD):

~~~bash
cargo test -p codegg --lib core::daemon_documents --locked
cargo test --test directory_listing_c005 --locked
cargo test --test directory_listing_c006 --locked       # new if introduced
cargo test -p codegg-protocol --locked
cargo test -p codegg --lib tui::file_tree --locked
cargo test --test file_tree_trajectory --locked
cargo test --test tui_panel_docking --locked
python3 scripts/check_daemon_listing_authority.py
python3 scripts/check_authorization_matrix.py
python3 scripts/check_tui_project_authority.py
python3 scripts/check_tui_editor_text_authority.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
scripts/verify.sh quick
~~~

Required hosted evidence: check `CI / verify` and `Desktop E2E` on the **final corrective commit**; a pre-corrective success cannot qualify the patch. Live remote TUI/rust-analyzer scopes are not C006 hard gates. If hosted checks are unavailable, label strict closure **conditional**, not strictly passed.

## 8. Acceptance criteria

1. Blocking scan cancellation mid-operation **cannot** free daemon admission: actual peak blocking scans is ≤1 across canceled handlers, errors, panics and 100 contending clients. Permit returned only after actual worker exit; no leak after exit.
2. Every actual serialized `CoreResponse::WorkspaceDirectoryList` has ≤524288 bytes including metadata, truncation and discriminator; a sole oversized top-level subtree is *not* emitted. All-roots-dropped response is explicitly truncated.
3. Trimming is deterministic and bounded; normal tree identity, sorting, nested shapes and 4096/100000/depth/symlink limits remain intact. No covert new schema or ADR contradiction.
4. Busy, Unavailable, Ready/truncated, denied and unsupported remain distinct; file.read gate and standalone/remote no-local-fallback unaffected.
5. C005 historical closure preserved, ADR-0022 chronology resolved with verifiable provenance or visibly annotated as unresolved, not backdated.
6. Failing-before/passing-after regressions, static security checks, actual commands, hosted final-head runs, residual findings and documentation are recorded in C006 closure.

## 9. Stop / escalation conditions

Stop if the only fix releases permit on async JoinHandle drop, forcibly kills threads, performs daemon filesystem I/O on Tokio worker, silently changes ADR-0022's top-level-only trimming policy to recursive pruning, hides oversized listings as complete empty Ready, weakens `file.read`, or reintroduces TUI-local browsing. If exact wire framing requires a protocol change or valid metadata alone exceeds 512 KiB, stop and document the required ADR decision instead of claiming a cap. Do not broaden to the independent `@` mention-indexer issue.

## 10. Handoff and closure record

The implementation agent should touch only the daemon-owned listing admission/trim seam, narrow integration tests, relevant docs/guards and new closure record. Avoid broad task lifecycle, TUI input or editor completion changes.

Create `plans/closure/tui-editor-correctness-usability-post-closure-corrective/006-status.md` with exact implementation SHA, dated provenance, independently instrumented worker peak, actual serialized largest-byte result, denial/no-local-fallback tests, full verification outcomes and explicitly classified remaining issues. Update the corrective addendum and `plans/registry.md` status **only after** those criteria are verified.
