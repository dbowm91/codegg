# TUI Editor Correctness/Usability — Post-Closure Corrective Addendum

Status: active (C003 closed at `1e74aa19`; C004 closed at `b9400c98`; C005 closed at `e37774a9`)

Parent historical roadmap: `plans/subsystems/tui-editor-correctness-usability-roadmap.md` (C001, C002, M003, M004 recorded closed; keep their closures immutable).
Baseline: `plans/tui-editor-correctness-usability-20261013` at `0266de94780e6f69c8f0fe6f1cfb44ea0aeb4b56`, reviewed 2026-10-13. Implementation agents MUST recheck current head and unrelated changes.
Governance: `plans/003-planning-process.md` `7 corrective passes.
Long-term references: `plans/000-long-term-specification.md` (TUI-first, correctness, project authorization, locality); `plans/001-terminology-and-domain-model.md`; `plans/002-long-term-roadmap.md`.
Accepted ADRs: ADR-0010, ADR-0011, ADR-0012, ADR-0018, ADR-0021, ADR-0022 (daemon-authorized workspace directory listing, accepted Option B 2026-10-14 with frozen contract; gated C005 until acceptance).

## 1. Purpose and ownership boundary

Correct three concrete findings discovered during independent source review **after** the original four milestone closure records were written, without reopening their implementation scopes or weakening the strict authority model. Two ready passes address actual TUI completion-state and blocking-worker contract defects. A gated follow-up resolves remote TUI file-tree authority via a small, daemon-owned directory read if the maintainers accept ADR-0022.

The TUI owns viewport, keyboard, status presentation, in-memory latest-request fencing and panel/dialog routing. It does not own document text or LSP/service authority. `DocumentController` retains all frontend text edits; daemon-owned document and LSP services retain canonical content and authorized completion. The tree remains navigation-only and `DocumentOpen` independently authorizes file reads.

## 2. Findings and class

### Correctness / invariant — C003 completion verdict, fencing and trigger work

M003 `plans/closure/tui-editor-correctness-usability/003-status.md` and M004 `004-status.md` were recorded closed based on local focused and trajectory tests. Additional source review finds:
1. `EditorCompletionState::apply` compares `result.revision == scope.revision` before interpreting `result.status`. Real `LspCompletionResultDto::not_ready` returns revision 0; `stale` returns daemon-current revision; both can legitimately differ, so ordinary NotReady/Stale are reported as generic wrong-revision Error. The existing status mapping unit test fabricates matching-revision Ready then swaps the enum; it cannot catch this wire shape.
2. `EditorCompletionState::clear` resets `request_id` to zero. A reconnect on the same attachment clears state without necessarily changing editor generation/scope. Old queued response ID 1 can collide with new request ID 1. This is a failed cancellation/reconnect fence despite independent transaction revision checks.
3. `current_completion_scope` copies the full cursor-to-line-start text to count UTF-16 columns, bypassing the C002 bounded hot rendering work concept on multi-megabyte lines. This is a UI responsiveness risk at explicit completion trigger.

C003 repairs exactly these defects with positive real-DTO/race and measured-byte tests, no new protocol. The original M004 closure is not silently rewritten.

### Concurrency / resource bound — C004 blocking scan admission

C002 `plans/closure/tui-editor-correctness-usability/002-status.md` demonstrates bounded entries and async-waiter cancellation. `src/tui/commands/file_tree.rs::rebuild` nevertheless aborts the outer Tokio task with `cancel_kind(FileTree)`, immediately spawning a new waiter/`spawn_blocking` scan. The old blocking closure persists until it returns from filesystem I/O; the claimed one-worker bound is therefore not established by tests that count only registered async waiters. Worker admission must be held through the **actual blocking scan** and new requests coalesced; cancellation remains cooperative and cannot forcibly interrupt one filesystem syscall. C004 is local, independent of C003 and has no new authority/protocol.

### Architecture / workspace security — C005 remote file-tree authority

M006-D (`plans/closure/desktop-frontend-ide-foundation/006-status.md`) and C002 retain client-host filesystem scanning. `local_walk_root` checks absolute+existing+directory on the TUI host, not authoritative co-location with daemon workspace. Identical path strings can refer to unrelated files on remote machines. The *selected file* is still daemon-authorized via `DocumentOpen`, but displayed directory metadata can be wrong or leak a client-host tree. ADR-0022 proposes a single daemon-authorized directory metadata read, with `ViaSession + FileRead`, workspace-relative entries and no client-host fallback. C005 **blocked until ADR accepted**. This decision is distinct from C004's resource correction; C004 may implement now and be integrated into daemon-side worker admission as part of C005.

## 3. Invariants and non-goals

- Exactly one text authority via `DocumentController`; no second TUI buffer, direct editor file reads or language-server startup.
- Existing `via_session + file.read` before completion; result not authority to edit without explicit chosen local transaction, current revision, writer lease and undo correctness.
- Freshness tokens never reused across clear, dismiss, reconnect, tab switch, attachment lifecycle, even if path/session/revision unchanged.
- `Ready` requires current revision/identity; other statuses preserve their specified reply shape and are non-actionable.
- Bounded CPU/bytes/time-sensitive UI work; tree examined/node/depth/symlink limits; actual blocking-work admission rather than async-handle count.
- Correct project/workspace/host ownership; remote TUI must never treat a coincidentally present local path as authenticated daemon contents.
- No full snippet/command/extra-edits support, auto-trigger, CRDT, multi-buffer/split-pane IDE, new generic filesystem RPC, background FS watcher, GUI replication, or document storage migration.
- Do not rewrite historical accepted ADRs or closures; source current issue closure records explicitly.

## 4. Dependency graph

`C003 — completion correctness`: **closed** at `1e74aa19` (closure `plans/closure/tui-editor-correctness-usability-post-closure-corrective/003-status.md`). Hard deps M003/M004 closures (satisfied). Independent of C004/C005.

`C004 — tree blocking-worker admission`: **closed** at `b9400c98` (closure `plans/closure/tui-editor-correctness-usability-post-closure-corrective/004-status.md`). Independent of C003. Soft interface integrated into C005 via the shared walker.

`ADR-0022`: **accepted** (Option B) 2026-10-14 with frozen contract (`DirectProject + FileRead`, Documents family, fail-closed, 512 KiB wire cap, single-flight daemon admission).

`C005 — daemon-scoped tree metadata`: **closed** at `e37774a9` (closure `plans/closure/tui-editor-correctness-usability-post-closure-corrective/005-status.md`). C003 does not gate C005 and remains ready independently.

Do not block C003 or C004 on the ADR, a live rust-analyzer server, or unrelated Windows sandbox branch/CI status. Qualify each independently with focused and hosted evidence.

## 5. Milestone deliverables

### C003 — Completion verdict and request-fence correctness

Plan `plans/implementation/tui-editor-correctness-usability/005-completion-verdict-and-request-fence-corrective.md`. Status **implemented; closed** at `1e74aa19`.

Exit: real daemon NotReady/Stale DTO constructors map to intended popup states; every non-ready state non-actionable; late results cannot become a new request across reconnect/clear/same attachment; F8 on huge line has measured bounded input/UTF-16 work with explicit refusal beyond supported range; ordinary Ready insertion still applies one controller transaction/undo. Closure `plans/closure/tui-editor-correctness-usability-post-closure-corrective/003-status.md`.

### C004 — File-tree blocking scan admission

Plan `plans/implementation/tui-editor-correctness-usability/006-file-tree-worker-admission-corrective.md`. Status **implemented; closed** at `b9400c98`.

Exit: barrier test proves peak *actual* simultaneous blocking scans ≤ selected finite admission cap with 100 refreshes; newest-only pending intent; old work cannot release permit while still executing; UI stays responsive when a scan is stuck; cancellation/shutdown semantics stated accurately; original tree guard/test matrix passes. Closure `plans/closure/tui-editor-correctness-usability-post-closure-corrective/004-status.md`.

### C005 — Daemon-authoritative directory listing

Plan `plans/implementation/tui-editor-correctness-usability/007-daemon-scoped-directory-listing-corrective.md`. Status **implemented; closed** at `e37774a9` (ADR-0022 accepted 2026-10-14; frozen as `DirectProject + FileRead`, Documents family, fail-closed compat).

Exit: one bounded native daemon listing API with explicit `file.read` and canonical workspace authority; local and remote TUI show daemon-origin entries; same-path/different-fs decoy test proves client-local tree never scanned; old daemon/denial fails closed; `/files` and Ctrl+T share result and `DocumentOpen` still reauthorizes file. Closure `plans/closure/tui-editor-correctness-usability-post-closure-corrective/005-status.md`.

## 6. Cross-cutting design and verification

**Storage / migration:** C003/C004: none. C005: additive native DTO only, no database change; old daemon Unsupported, no silent fallback. No persisted candidate/tree data.

**Security:** explicit file.read and daemon-resolved project context, symlink/path containment and negative authorization tests; completion statuses cannot be mistaken for grants; viewer cannot read a different host's coincidentally named directory. Keep document bytes/snippets and sensitive filenames out of logs.

**Concurrency:** request epoch+token and generation checks; blocking-worker permit retained until closure exits; cancellation flags cooperative; no fake guarantees about interrupting slow OS filesystem syscall. Daemon/TUI disconnect and shutdown drop authority and pending results; no unbounded background work.

**Observability:** status type, rejected stale responses, measured input bytes/active workers and caps only; no raw source text or private directory path.

**Testing:** real DTO constructors rather than fabricated statuses; deterministic barrier races in both completion orders; distinct daemon/local directory fixtures; no wall-clock-only flaky assertions; exact-SHA hosted CI before strict closure. Live rust-analyzer smoke and live interactive TUI/PTY test are desirable for integration but absence must be recorded explicitly, not used to block independent safe implementation.

**Documentation:** update `architecture/tui.md` and `docs/tui.md` with the implemented behavior; C005, if accepted, also updates protocol and authorization documentation. Do not modify previously issued closure records; write separate corrective closure evidence.

## 7. Status / completion criteria

| Milestone | Status | Implementation plan | Required closure record | Blocker |
|---|---|---|---|---|
| C003 | ready | `plans/implementation/tui-editor-correctness-usability/005-completion-verdict-and-request-fence-corrective.md` | `plans/closure/tui-editor-correctness-usability-post-closure-corrective/003-status.md` | none |
| C004 | closed | `plans/implementation/tui-editor-correctness-usability/006-file-tree-worker-admission-corrective.md` | `plans/closure/tui-editor-correctness-usability-post-closure-corrective/004-status.md` | none (`b9400c98`) |
| C005 | blocked | `plans/implementation/tui-editor-correctness-usability/007-daemon-scoped-directory-listing-corrective.md` | `plans/closure/tui-editor-correctness-usability-post-closure-corrective/005-status.md` | ADR-0022 proposed; acceptance required |

This addendum closes only when all three are strictly qualified **or** the maintainers explicitly defer C005, with the local-only remote limitation prominently documented and enforced fail-closed. Completing local C003/C004 must not be delayed by the separate ADR gate. Each milestone's closure record must map acceptance criteria to exact tests, implementation SHA/CI, unresolved severity and a truthful recommendation.

## 8. Known risks and decision points

C003: if precise UTF-16 conversion beyond a fixed byte budget requires a new immutable text-core display index, use an explicit over-budget refusal for this corrective and reserve indexing as a separate performance milestone. C004: OS filesystem syscalls can block arbitrarily; admission bounds concurrency, not wall-clock syscall completion. C005: new network-visible directory metadata raises an authorization/versioning decision; ADR-0022 must be accepted intentionally. No blanket promise that the daemon's directory snapshot remains stable while filesystem is concurrently changing; open rechecks security independently.

## 9. Handoff order

Two independent implementation agents may implement C003 and C004 now on the active TUI branch, preserving unrelated edits. The ADR-0022 review may proceed in parallel but **must not be treated as acceptance**. After maintainer decision, activate C005 with a fresh baseline; if rejected, record an explicit local-only fail-closed alternative and retain C005 blocked/superseded per governance.
