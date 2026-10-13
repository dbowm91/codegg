# TUI Editor Post-Closure Corrective C005 — Daemon-Scoped Directory Listing

Status: implemented; closed — ADR-0022 accepted (Option B) 2026-10-14 with frozen contract; implementation `e37774a9` on branch `plans/tui-editor-correctness-usability-20261013`; closure `plans/closure/tui-editor-correctness-usability-post-closure-corrective/005-status.md`.

Baseline: `plans/tui-editor-correctness-usability-20261013` at `0266de94780e6f69c8f0fe6f1cfb44ea0aeb4b56`; review current tree at activation.
Source: `plans/subsystems/tui-editor-correctness-usability-post-closure-corrective-addendum.md` C005.
Historical sources: M006-D `plans/closure/desktop-frontend-ide-foundation/006-status.md`; C002 `plans/implementation/tui-editor-correctness-usability/002-bounded-editor-render-and-tree-walk.md` and `plans/closure/tui-editor-correctness-usability/002-status.md`.
Required decision: `plans/adrs/ADR-0022-daemon-authorized-workspace-directory-listing.md` (proposed Option B; no implementation authority).
Accepted ADRs: ADR-0010, ADR-0011, ADR-0012, ADR-0018.
Class: architecture/correctness / authorized capability.

## 1. Objective

Make `/files` and the Ctrl+T project tree display exactly the daemon's authorized workspace directory metadata for both local and remote TUI, eliminating direct filesystem enumeration of the client-host path and the same-path/different-host confusion. Preserve the resource, cancellation, generation and text-authority invariants of C002/C004. File opening must still go through the already-authorized `open_editor`/`DocumentOpen` seam.

## 2. Why blocked; evidence

Hard gate: ADR-0022 must be **accepted** with explicit native protocol/authorization/ownership choice. Its selected Option B introduces a new project/workspace-scoped, daemon-owned read; no plan should silently ship a new variant while the decision is proposed.

`src/tui/commands/file_tree.rs::rebuild` obtains `context.workspace_root` and checks it with `src/tui/file_tree.rs::local_walk_root`. That checks absolute path, local `symlink_metadata` and `is_dir`; it **cannot establish host identity**. If a remote daemon's `/home/user/project` differs from local TUI's same-spelled `/home/user/project`, its metadata can be displayed as if authoritative, while `open_editor` later opens daemon data. If a root is missing locally, the UI refuses, so the current behavior is both misleading in the collision case and incomplete in the remote-only case. No current authorized directory-listing service is used here.

C002's `build_tree_with_cancel` and bounds are reusable, but currently run in the client TUI. C004 separately addresses misleading one-blocking-worker claims. C005 must consume the C004 bounded admission/cancel semantics if available, or provide an equivalent daemon-side implementation with test evidence; C004 is a **soft interface integration dependency** and final strict closure requires its verified status.

Old tests assert same-process temporary roots and symlink skipping; they cannot catch identical path strings on two machines with divergent filesystems.

## 3. Invariants, security and non-goals

- Backend-only directory enumeration: the daemon resolves project/workspace/session identity and authorizes file/directory metadata under `file.read` before any filesystem access. Never trust a root supplied by TUI or compare only strings to establish authority.
- TUI owns only presentational, inert directory-entry DTOs. It never opens/read file bytes itself and never bypasses `DocumentController`/`DocumentOpen`.
- The tree reader does not become a generic remote filesystem RPC. It has a bounded workspace-relative target and a typed tree response with explicit empty/denied/truncated/unavailable/error states.
- No symlink following, traversal escape, arbitrary absolute path, TOCTOU assumption, special-entry opening, or extension of project-observe capability to filename access.
- Preserve depth 12, nodes 4096, examined <=100000, name 255B (or tighter) and a documented total response byte budget; no unbounded blocking admission.
- Only active authorized tab/session/workspace/epoch may display results. Revocation/reconnect/project switch invalidates prior listing and in-flight results.
- No new schema/migrations and no persistent tree cache. No full filesystem watcher, git status/tree normalization, search, workspace federation or separate client-side file index.
- Stop on unspecified native contract; do not weaken file.read to avoid an auth change.

## 4. Required production changes (after ADR acceptance)

**Protocol/shared client.** Add a small, typed, version-compatible `WorkspaceDirectoryList` request/result with an authenticated session/workspace locator, workspace-relative directory prefix and bounded options; validate before backend work. Result contains only inert relative entries and status/truncation/examined count, no file bodies or raw absolute daemon paths. Include explicit old-daemon Unsupported, denied/not-found, partial/truncated and error behavior with bounded message size. Model the existing `max_items` / pagination/version conventions if present rather than inventing invisible defaults.

**Daemon / authorization.** Implement one read handler under `src/core` with request family classification, `ViaSession + FileRead` operation_descriptor/authorization matrix entry, daemon-resolved canonical root and target containment checked consistently with `DocumentOpen`. Do not use the frontend's provided absolute root. No LSP service interaction or writer lease. Use bounded `spawn_blocking`/scanner coordinator with explicit permit held by actual blocking work; cancellation, time/resource limit policy and no panics on unreadable entries. Reuse or port existing `TreeListing` semantics without introducing a second text owner.

**TUI.** Replace `src/tui/commands/file_tree.rs` local `build_tree_with_cancel` call with client `CoreRequest::WorkspaceDirectoryList`. Keep `TuiCommand::FileTreeListed` or typed equivalent and current project/generation/root checks, but key the response to daemon-provided workspace identity and **connection epoch**; no local directory walk fallback in standalone or socket modes. `/files` dialog and Ctrl+T must consume one state. Deliberately choose whether old-daemon response shows “not supported” or a fully authenticated local-only path; if no attestation exists, fail closed. If a backend-request-specific alias or enum is introduced, apply static guards to prevent reintroducing a TUI direct `read_dir`.

**Docs/operational.** Update `architecture/tui.md`, `architecture/protocol.md`, `architecture/authorization.md`, `docs/tui.md` and appropriate client/read docs. Explicitly show filename metadata privacy, version skew and lack of file watching. Clarify that the directory list is advisory and a file may disappear before `DocumentOpen`, which independently reauthorizes.

## 5. Ordered work packages

A. Obtain explicit ADR-0022 acceptance. Inspect real DTO/auth conventions, original C002 scans and C004 admission results; freeze request status and caps. Identify root/identity contract and no-fallback behavior before code.

B. Add typed protocol/`codegg-client` accessor and negative validation/old-server tests. Keep protocol additive and bounded; no schema migration.

C. Implement authorized daemon directory scan with an admission permit retained until blocking completion; preserve symlink/TOCTOU defenses and examine-all-entries caps even on errors. Add adversarial tests: nonmember `file.read` denied before read, ../ or absolute path denied, symlink file/directory not followed, wrong project/workspace denied, huge ignored/error directories bounded.

D. Rewire the TUI so it displays only daemon-origin metadata, with existing tree/UI behaviors preserved. On project switch/reconnect, old results cannot paint. Root string alone cannot authenticate; validate daemon scope/epoch tokens. Treat unsupported old daemon as an explicit unavailable tree (never fall back to client-local read).

E. Run distinct-host test harness with a **local decoy directory at the same path string** and daemon tree rooted at different file content (use a fake daemon transport or container-mounted namespace with explicit mapped path, no need for actual two physical hosts). Assert only daemon entries show, local entries never scanned. Test valid local TUI (same code path), remote daemon, denial, reconnect, rapid refresh, `/files`, Ctrl+T, selected file open re-authorization, file deleted between listing/open.

F. Run focused/broad verification, owned tests and hosted exact-SHA CI; if actual remote multi-host smoke unavailable, retain tested different-mount transport as primary reproducible proof and state operational gap explicitly. Integrate with C004 cancellation/admission. Strict closure requires native endpoint, authorization and no fallback tests, not just a documentation claim.

## 6. Failure/recovery/contention semantics

Directory enumeration is a read with no side effects; duplicate same input may legitimately see different filesystem metadata, and a result is stamped to a request and project generation. Cancel/reconnect drops result, not a transaction. Do not present stale entry list as current on authorization revocation or project switch. Resource exhaustion produces typed/truncated/error without an unbounded worker or memory queue. A slow filesystem syscall may be uninterruptible; admission must account for in-flight worker and report busy. A listed entry is never a grant for `DocumentOpen`. Daemon restart invalidates in-memory scans; client re-requests on explicit refresh.

## 7. Compatibility/migration

New additive protocol variants and explicit old-server unsupported. No DB migration. Existing tree shortcuts and scroll/focus preserve behavior for local and remote clients; local host is not a privileged exception to daemon read authority. If protocol lacks `FileRead` for metadata, ADR must decide before implementing. No need to migrate old local cache: it was ephemeral.

## 8. Required verification

- Protocol: DTO roundtrip; invalid path/session/root, max limits, byte caps, malformed response, old daemon unsupported.
- Security: file.read on owning session/project; nonmember/project-observer only denial; symlink, traversal, stale grant, no raw daemon path/content leak.
- Concurrency: slow/blocked scan, 100 rapid requests, admitted blocking worker max bound, shutdown, transport cancellation, stale project epochs.
- Integration: same-text paths to different local/daemon directories (decoy test); local and simulated remote; missing root/old daemon; file switch and selected open always uses `DocumentOpen`; `/files` parity with Ctrl+T.
- Negative: malicious string/invalid UTF-8 names, file changed between list/open, unreadable directory errors and symlink races remain bounded.

~~~bash
cargo test --lib tui::file_tree --locked
cargo test --lib tui::commands::file_tree --locked
cargo test --test file_tree_trajectory --locked
cargo test --test tui_panel_docking --locked
cargo test --test document_client_trajectory --locked
python3 scripts/check_authorization_matrix.py
python3 scripts/check_tui_project_authority.py
python3 scripts/check_tui_editor_text_authority.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
scripts/verify.sh quick
~~~

Create a dedicated directory-listing protocol/daemon integration target and a decoy-root transport fixture, and run it. Add a static guard rejecting direct TUI root-directory enumeration after migration. Record actual commands and exact-head hosted CI with URL/state; never label tests successful before execution.

## 9. Acceptance criteria

1. ADR-0022 is accepted and the documented native directory-read authority exists.
2. In a same-path, different-filesystem fixture, the visible tree contains only the daemon's entries; no client-host directory bytes/names are read.
3. Request scoped to valid session/workspace and `file.read`; wrong principal/root/traversal denied before disk I/O.
4. Old daemon/unavailable/permission loss fails closed and clearly; no client-local fallback.
5. Depth/node/examined/name/serialized bytes and real blocking worker admission enforced, with measured tests.
6. `/files` and Ctrl+T remain equivalent and selected-file open continues the unchanged authorized document path.
7. Local and simulated remote transports, reconnect, cancellation and concurrent refresh work; docs, guards and exact-head hosted CI recorded.

## 10. Stop conditions

Do not implement without ADR-0022 acceptance. Stop for any unspecified auth mapping, file content exposure, root authority from frontend, unbounded blocking scan, speculative client-local fallback or new general remote FS service. Report the gap rather than silently widening the API.

## 11. Closure evidence

Create `plans/closure/tui-editor-correctness-usability-post-closure-corrective/005-status.md` with accepted ADR link, exact implementation SHAs, owner/authorization matrix, same-path decoy fixture, negative tests, worker/resource limits, version-skew behavior, hosted CI links, live/simulated remote qualification, residual limitations and classification. Update registry and addendum; preserve historical M006-D and C002 closure records.

## 12. Handoff

C005 is fully scoped but **blocked** until ADR-0022 is accepted. It may be worked independently of C003 once approved. C004 is parallel/soft integration but must be closed or reconciled to the daemon worker before strict C005 closure. The intention is a small authorized metadata read, not a second editor or a generic network filesystem framework.
