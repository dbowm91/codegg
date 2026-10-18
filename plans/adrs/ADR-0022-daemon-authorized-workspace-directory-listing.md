# ADR-0022 — Daemon-Authorized Workspace Directory Listing for Remote TUI

Status: accepted (Option B)

Date: 2026-10-13

Acceptance: 2026-10-14 — project maintainer accepted the recommended
Option B (daemon-owned read-only workspace directory listing) with the
frozen contract below. No implementation authority existed before this
date. C005 (`007-daemon-scoped-directory-listing-corrective.md`) is now
unblocked and executes exactly this contract; any deviation requires a
superseding ADR, not implementation-time improvisation.

Frozen contract:

- Scope/capability: `DirectProject + FileRead` under operation
  `workspace_directory_list` — the same gate as `DocumentOpen`, so any
  principal that may open workspace files may list them, and
  `ProjectObserve` alone never implies listing access. No session
  locator: the tree is project/workspace-keyed, matching the existing
  TUI `FileTreeState` identity.
- DTO: additive `CoreRequest::WorkspaceDirectoryList { request:
  document::WorkspaceDirectoryListRequestDto }` carrying top-level
  `project_id`, `workspace_id`, and an optional workspace-relative
  `prefix` (default `""`); `CoreResponse::WorkspaceDirectoryList {
  result: document::WorkspaceDirectoryListResultDto }` carrying the
  echoed `workspace_id`/`prefix`, inert nested entries (`name`, `path`,
  `depth`, `kind`, `children`), `truncated` + `truncated_reason`,
  `examined`, and `status` (`ready` | `busy` | `unavailable`). No file
  bodies, no absolute daemon paths. Serialized wire cap
  `MAX_DIRECTORY_LISTING_RESPONSE_BYTES = 512 KiB`, enforced by
  trimming trailing top-level nodes with an explicit truncation reason.
- Family/handler: `Documents` family; handler beside
  `handle_document_request` in `src/core/daemon_documents.rs`, reusing
  `verify_project_workspace`, daemon-resolved `canonical_root`, and the
  `DocumentOpen` containment sequence (`validate_relative`-equivalent
  prefix check, `reject_symlink_components`, `starts_with`).
  No LSP service, no writer lease.
- Scanner: extract the C002/C004 walk into one daemon-neutral module
  (`src/directory_listing.rs`); `src/tui/file_tree.rs` delegates to it
  so there is exactly one walk authority. Bounds preserved: depth 12,
  nodes 4096, examined 100000, names 255B, symlinks never followed.
  Daemon admission is a per-daemon single-flight semaphore:
  contended scans answer `busy`, never queue unboundedly.
- Compatibility: fail closed always. Old daemons (unknown variant) and
  `unavailable` answer with an explicit tree-unavailable message; no
  client-local `read_dir` fallback in any mode, including standalone.
  The temporary co-location predicate in §6 is explicitly declined.
- Lifecycle: responses are keyed to the echoed daemon `workspace_id`
  plus the existing TUI generation/request fencing; revocation,
  reconnect, and project switch invalidate prior listings.
- Telemetry: counts, caps, cancellations, errors, denied requests, and
  durations only; no filenames or paths in diagnostics.

Owners: CodeGG maintainers

Related specs: `plans/000-long-term-specification.md` (project authorization, correctness, locality), `plans/001-terminology-and-domain-model.md`, `plans/003-planning-process.md`.
Affected roadmaps: `plans/subsystems/tui-editor-correctness-usability-post-closure-corrective-addendum.md`, `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md`.
Accepted decisions: ADR-0008 (LSP preview writes), ADR-0010 (shared native client), ADR-0011 (document ownership), ADR-0012 (native authorized LSP read), ADR-0018 (workspace-relative navigation), ADR-0021 (completion). This ADR adds directory metadata access only, not a second document-read route.

## Context and forces

The M006-D file-tree navigator was intentionally local and used `std::fs::read_dir` in the TUI. C002 bounded its traversal and added `local_walk_root`. That helper checks a workspace root is an absolute, non-symlink local directory. It does **not** prove that the local directory is the daemon's workspace: in the supported topology of separate processes/hosts, two machines can both have `/home/user/project` but different contents, mounts or owners. A socket URL/path, string equality and local `canonicalize` are insufficient identity proofs. SSH forwarding, containers or network mounts complicate locality further. That is a project-identity correctness risk, not merely a missing-path error.

Opening a selected file already routes through daemon-authorized `DocumentOpen` and its workspace-relative validation. However, displaying client-host directory names as if they were daemon workspace entries can mislead the user and reveal local files from the wrong host; failing only on non-existent root cannot catch this.

File tree needs a coherent durable ownership boundary across local daemon and remote TUI. Any additional public request/authorization contract meets CodeGG's ADR threshold.

## Decision drivers

1. The tree must describe the daemon-resolved authorized workspace, not a path guessed/coincidentally present on the TUI machine.
2. Directory listing is metadata revealing filenames and structure; it needs explicit authorization, denial and resource bounds.
3. No file-content read or mutation occurs in the tree service; selected files open via existing `DocumentOpen` and the document controller, with independent validation.
4. Directory traversal must obey symlink, path-component, node, depth, examined-entry and cancellation budgets, and never block the daemon's Tokio executor or TUI event loop.
5. Existing TUI file tree and `/files` must work for both local and socket/remote clients without two diverging authorities.
6. Old daemons and cancelled/disconnected clients must fail explicitly, not fallback to a local `read_dir`.
7. Avoid a broad generic remote filesystem/agent browsing API in a narrow correctness corrective.

## Considered options

### A — Keep TUI local walking with root existence/canonical path checks

Lowest change, but no proof of daemon identity on remote sessions or same-path different-host tests. Not acceptable as a general capability. Could remain a **deliberately authenticated same-process / proven-colocated** optimization only if reliable host/filesystem identity is explicitly established; mere path equality does not qualify.

### B — Daemon-owned read-only workspace directory listing (recommended)

Introduce one bounded, typed `WorkspaceDirectoryList` request/response in the shared native protocol/client. The daemon resolves the session's project/workspace and permitted root, enforces project-specific file metadata/read authorization and containment, then lists directory entries on the **daemon host** with existing `file_tree`-equivalent traversal limits. The TUI consumes only an inert, workspace-relative typed tree, never reads its own filesystem as fallback. A compatible daemon may use a shared read-only `WorkspaceDirectoryListingService`; the domain/transport adapter owns authorization. Tree UI remains presentation-only. This is one clear authority and supports remote without host attestation ambiguity.

Costs: additional native DTO, daemon family/authorization matrix changes, concurrency/admission and symlink/TOCTOU review, and backward-compatibility handling. Mitigated by bounded single purpose and reuse of C002 traversal structures/guards.

### C — Attest filesystem equivalence between hosts

Requires cross-platform, mount-aware strong host/filesystem identity and an authenticated proof bound to the daemon workspace. Complex and fragile for containers, volumes and SSH; may inadvertently bless distinct directories with same inode/dev on different machines. Rejected for this milestone.

### D — Disable file tree for every socket client

Safest interim mitigation but regresses local-socket users and does not deliver the intended multi-frontend IDE capability. Acceptable only as a **temporary fail-closed guard** until B is implemented, never as evidence of remote support.

## Decision (accepted; see Acceptance above)

1. **Authority:** all TUI file-tree directory enumeration is daemon-owned for both local and remote sessions, using a single explicit native read; no unconditional TUI-host direct `read_dir` in production TUI. The daemon's authenticated project/workspace locator determines canonical authority. Workspace-relative paths only.
2. **Authorization:** classify listing as a read of directory metadata under `ViaSession + FileRead` (or a narrower explicit directory-list capability if the auth domain already provides one with equivalent scope). Resolve authorization *before* any traversal. Denials are non-enumerable and distinct from NotReady/empty/unreadable/truncated. The browser UI never assumes `ProjectObserve` implies file-list access. Future read of file bytes still uses `DocumentOpen`; listing grants no new write/read body API.
3. **Protocol:** one typed bounded `CoreRequest::WorkspaceDirectoryList` and matching response carrying project/workspace/session locator, optionally bounded workspace-relative prefix and listing controls, and a typed tree with entries (`relative_path`, `name`, `kind`, depth) plus `truncated` reason/`examined` and status. Strict request validation and canonical root containment. No unbounded recursive streamed updates, arbitrary absolute paths or arbitrary file reads. Errors and truncated states explicit; old daemon reports Unsupported.
4. **Safety budgets:** preserve or tighten C002's 12 depth, 4096 retained nodes, 100000 examined entries (including skipped entries), 255-byte names, symlink never followed, and a total bounded serialized-response size (define verified wire cap). Skip special files. Resource budget enforcement and cooperative cancel on daemon-owned blocking worker; no unbounded request concurrency under repeated refreshes or disconnect. Traversal of mutable directory trees remains best-effort snapshot, not a TOCTOU-free authorization; **each selected file is independently authorized at open**.
5. **Lifecycle:** TUI validates request ID, scope/tab/connection epoch and tree generation before display. Permission loss, workspace rebind, reconnect and remote disconnect invalidate previous tree; never show cached metadata as current after revocation. A slow/stuck daemon host scan is bounded in admission, uses cancellation checkpoints and explicit busy response; a single blocked filesystem syscall cannot be hard-killed by aborting Tokio waiter.
6. **Compatibility:** no migration or durable state. Old daemons may not support the new request: show a descriptive tree-unavailable message, and **do not** attempt client-local directory fallback. Local TUI uses the same new path. If local read compatibility is retained temporarily, it is behind a proven co-location/authority predicate with explicit expiry; it must not silently remain the default.
7. **Telemetry:** record only read counts, caps, cancellations, errors, denied requests and durations; avoid filenames/full paths or file content in diagnostics.

## Consequences and deferrals

**Positive:** one authoritative tree across frontend topologies; correct project containment, stable authorization and no accidental same-path client-host listing; TUI can remain lightweight/presentation-only.

**Negative:** new backend route and roundtrip for local TUI, modest daemon resource overhead, feature-version negotiation, and new adversarial test surface.

**Deferred:** workspace-wide live filesystem watching, caching, search/index integration, file body streaming, persistent directory snapshots, agent-generated file edits, general-purpose remote browsing and multi-host federation. No claim of authoritative full-collection consistency while filesystem mutates midwalk.

## Security / recovery / failure semantics

A malicious client may supply path traversal, oversized depth, invalid UTF-8 names or repeated requests. Reject or cap before I/O; derive scope from authenticated session, use the same containment validators as document file access, and never follow symlinks, including at the root. Distinguish unauthorized/not-found from empty directory without revealing additional path details. Use bounded workers/permits and cancellation on project switch/disconnect; stale responses must fail TUI generation checks. The daemon-authorized path must remain correct even if a same-named directory exists on a remote TUI host.

## Verification required before ADR adoption

Maintainers must explicitly accept B (or another fully specified mechanism) and freeze DTO, auth descriptor, failure, compatibility and ownership semantics before implementation. Evidence must include:

- Two **different** host/workspace roots that stringify identically: tree must display only daemon source; local decoy not inspected.
- Authorization: team principal without `file.read` cannot list even if it can observe project lifecycle; a valid member can list only the authorized session/workspace; invalid path/symlink denied.
- Local daemon/TUI and remote/socket TUI both use same authorized request; old daemon unsupported and no local fallback.
- Worst-case examined-entry/node/depth/bytes limits; cancelled/slow worker admission; symlink/TOCTOU and special-entry denial; opaque unauthorized errors.
- Selected file still routes through `DocumentOpen` and is separately reauthorized, even after directory contents change.
- Native DTO roundtrip/compat, project-switch/reconnect fencing, static guards, focused integration, exact-head hosted CI and recorded real multi-host smoke or explicit not-run limitation.

## Supersession

None. Supplements ADR-0010/0011 and does not change their document or frontend ownership. ADR-0022 was proposed in its planning branch and accepted 2026-10-14 with the frozen contract recorded above.

## C006 provenance note (2026-10-13, does not alter the frozen contract)

The `Acceptance: 2026-10-14` claim above is chronologically inconsistent
with the repository record and must not be cited as verified approval
ordering:

- Implementation `e37774a9` is dated 2026-10-13 12:12:58 UTC.
- The acceptance + C005-closure commit `193a4441` is dated 2026-10-13
  12:16:08 UTC — four minutes after the implementation it claims to
  authorize, and one day before the stated 2026-10-14 acceptance date.
- Both commits share one author; no independent maintainer
  approval timestamp, PR review, or decision record has been located.

No approval is fabricated and no date is backdated here. The
architectural choice (accepted Option B, frozen contract above) is
preserved as recorded; only its chronological provenance is marked
**unverified**. C005's historical closure is retained unchanged. C006
(`plans/implementation/tui-editor-correctness-usability/008-daemon-listing-admission-wire-budget-corrective.md`)
implements strictly within that frozen contract (worker-owned permit,
full-response 512 KiB bound, trailing-top-level trim to zero) and
escalates the provenance question to maintainers for an explicit
decision or a superseding ADR correction.

## Amendment: the depth window is 3, not 12

The frozen scanner contract above records "Bounds preserved: depth 12, nodes
4096, examined 100000, names 255B". The depth figure is now **3**. Every other
bound, and the whole delivery/authorization design, is unchanged.

This is the *tighten* direction §4 permits, and it was forced by the wire cap
this same ADR introduced. C006's trailing-top-level trim keeps the longest
prefix that fits `MAX_DIRECTORY_LISTING_RESPONSE_BYTES`, so a single directory
with a deep subtree consumes the budget and every sibling root is trimmed
away. ADR-0022 already names the worst case — "trailing-top-level trim to
zero" — without a reachable user-visible consequence having been established.
One now is: on a 75,956-entry repository whose first alphabetical directory
was a large one, a 512 KiB budget retained **one** root node out of 37.
`/files` rendered a single row that could not be selected, because selection
clamps at `rows.len() - 1`, with the only affordance a truncation notice
naming a bound the user could do nothing about.

Measured on that repository, varying the embed depth alone:

| embed depth | retained roots (of 37) | serialized |
|---|---|---|
| 12 (as frozen) | 1 | 614 KB, over cap |
| 3 (this amendment) | 37 | 290 KB |

Raising `MAX_LISTING_NODES` does not help and is **not** the fix: at 65,536
nodes the same response is 5.9 MiB and the prefix trim still retains 4 roots.
The binding constraint was bytes, not node count, so the node budget is
unchanged at 4096 — against a single *directory scope*, which `l` re-walks
independently. Depth 3 is a window on one response, not a limit on how deep
the user can browse.

`LISTING_IGNORED_DIRS` additionally gains `.codegg`. It is this application's
own per-project runtime state and lives inside the tree it describes; in a
CodeGG checkout it was 21,482 entries and, being second alphabetically, it
consumed the entire retained-node budget before any source directory was
walked. `python_script::snapshot`, `python_script::executor`, and
`research::sources::local_repo` already skipped it — the listing list had
drifted from the other three walkers. The composer's `@` indexer skip list is
updated with it so the two keep describing the same workspace.

The ADR's provenance question above is unaffected and remains escalated.
