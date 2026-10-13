# TUI Editor Post-Closure Corrective C005 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tui-editor-correctness-usability/007-daemon-scoped-directory-listing-corrective.md`
  (status `implemented` at closure)

Source subsystem roadmap:

- `plans/subsystems/tui-editor-correctness-usability-post-closure-corrective-addendum.md#C005`

Accepted authority:

- `plans/adrs/ADR-0022-daemon-authorized-workspace-directory-listing.md`
  (accepted Option B 2026-10-14 with frozen contract; no implementation
  authority existed before that date)

Repository baseline reviewed: `e37774a9cf9b484d975787c0d92095d45ed1791d`
(branch `plans/tui-editor-correctness-usability-20261013`).

Implementation commits:

- `e37774a9` — `tui-editor(c005): daemon-owned workspace directory listing`
  (protocol DTO, auth/family/dispatch, shared walker extraction, daemon
  handler + admission, TUI rewire, decoy/auth/bounds/concurrency tests,
  static guard, architecture + user docs)
- Prior branch history preserved: `b9400c98` (C004, separately closed),
  `ba66a2da` (C004 closure + C005 blocked record, superseded by this
  record for C005).

## 1. Executive finding

C005 is implemented and closed. `/files` and the Ctrl+T tree display
exactly the daemon's authorized workspace directory metadata for local and
remote TUI alike: the TUI issues `CoreRequest::WorkspaceDirectoryList`
with the tab's canonical project/workspace binding and renders only
daemon-origin entries keyed to the echoed daemon workspace identity plus
the inherited generation/root/request fencing. No production TUI path
walks its own filesystem; old-daemon, denial, busy, and unavailable
outcomes are explicit and never fall back to a local `read_dir`. The
C004 single-flight semantics are preserved end to end (client-side one
round-trip + latest-only pending; daemon-side single-permit admission).
A latent node-budget blanking defect in the extracted walk was found by
the new exact-count test and fixed (see §5).

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| ADR-0022 accepted with frozen DTO/auth/ownership/failure/compat semantics (AC1, plan §4A) | ADR-0022 `Status: accepted (Option B)` + `Acceptance:` paragraph freezing `DirectProject + FileRead` op `workspace_directory_list`, DTO shapes + 512 KiB wire cap, Documents family, walk extraction, fail-closed compat, lifecycle keying, counts-only telemetry | pass | No implementation authority existed before 2026-10-14; implementation commit postdates acceptance |
| Same-path/different-filesystem decoy shows only daemon entries; no client-host read (AC2) | `tests/directory_listing_c005.rs::decoy_tree_with_same_shape_never_leaks_into_the_daemon_listing` (daemon serves only its resolved root) + `src/tui/commands/file_tree.rs::daemon_listing_never_reads_the_local_decoy_tree` (scripted fake transport; local decoy file present, never renders) + `rebuild_lists_through_the_daemon_for_a_remote_only_root` (nonexistent local root still lists) | pass | Plan §5E permits a fake-daemon transport decoy; no two physical hosts needed |
| `DirectProject + FileRead` scoping; traversal/observer denial before I/O (AC3) | `traversal_prefixes_are_denied_before_io` (`document_invalid_path` for `..`/absolute/empty-segment prefixes), `symlink_prefix_and_symlink_entries_never_escape`, `wrong_project_or_unknown_workspace_fails_closed` (`document_not_found` for cross-wired/unknown bindings), `outsider_is_masked_as_not_found_while_viewer_may_list` (outsider → `project_not_found`; Viewer with `file.read` → Ready) | pass | Denials masked via the `observe_private` rule, exactly like `DocumentOpen`; prefix validated in DTO before any I/O, containment re-derived daemon-side |
| Old-daemon/denial fails closed with no local fallback (AC4) | `src/tui/commands/file_tree.rs::daemon_errors_fail_closed_with_no_local_fallback` (transport failure, daemon `Error`, unexpected variant → explicit error disclaiming the fallback; local file present, never renders) + `rebuild_without_a_core_client_fails_explicitly_with_no_local_fallback` + `listing_for_another_workspace_is_discarded_without_rendering` | pass | Old daemons fail serde on the unknown variant → transport error → unavailable message; no co-location predicate retained (ADR declined it) |
| Bounded depth/node/examined/name/bytes + daemon-side worker admission measured (AC5) | `deep_tree_truncates_with_an_explicit_depth_reason`, `node_budget_truncates_a_wide_tree` (4200 files → 4096 retained + named reason), `trim_to_budget_*` unit tests, `held_admission_answers_busy_and_releases_to_ready` (held permit → all `busy` with `examined == 0`; released → Ready), `concurrent_bursts_all_settle_without_piling_workers` (100 concurrent → Ready/Busy only, permit restored) | pass | Examined-cap branch proven at walk level (`examined_budget_stops_a_walk_of_only_filtered_entries`); daemon walk reuses the same code |
| `/files` + Ctrl+T parity with authorized open path (AC6) | Unchanged shared-state mirror in `apply_listing` + `c004_switch_from_a_to_b_shows_only_b` parity tail + `listed_entry_still_opens_through_document_open` (listed `open-me.txt` opens with correct text via unchanged `DocumentOpen`) | pass | Listing grants nothing; open reauthorizes independently |
| Local/simulated-remote transports, reconnect, cancel, concurrent refresh + docs/guards + hosted CI (AC7) | `listing_shows_daemon_entries_with_explicit_bounds` (local path = same handler), TUI fake-transport suite (13 tests), C004 admission suite over round-trips (peak ≤ 1, 100 supersessions, shutdown, panic-conversion), `architecture/{tui,protocol,authorization}.md` + `docs/tui.md`, `check_daemon_listing_authority.py` + `check_authorization_matrix.py` green, `verify.sh quick` green | pass | Hosted CI not run — recorded as a low finding below, same standard as C001/C002/C004 |

## 3. Production implementation evidence

- Protocol (`crates/codegg-protocol/src/document.rs`, `core.rs`):
  `WorkspaceDirectoryListRequestDto` (`project_id`, `workspace_id`,
  `prefix` + `validate_prefix`), `WorkspaceDirectoryListResultDto`
  (echoed scope, inert nested entries, `truncated`/`truncated_reason`,
  `examined`, `Ready|Busy|Unavailable`), 512 KiB wire cap const,
  `CoreRequest::WorkspaceDirectoryList` / `CoreResponse::WorkspaceDirectoryList`.
  Client transport needs no new wrapper: the TUI issues the typed request
  through the existing `CoreClient::request` seam, exactly like the
  `AssetRefresh` command path.
- Auth (`crates/codegg-core/src/authorization/policy.rs`,
  `audit_instrumentation.rs`): `workspace_directory_list` →
  `DirectProject + FileRead` (exhaustive match forced the arm);
  uninstrumented-reads classification shared with `document_read`
  (denials audited upstream); team denials masked via the
  `observe_private` rule (`src/core/daemon.rs`); project resolved from
  the nested DTO locator (`resolve_authorization_project`).
- Family/dispatch (`src/core/daemon_family.rs`,
  `src/core/daemon_documents.rs`, `src/core/daemon_construct.rs`,
  `src/core/daemon.rs`): Documents family; handler beside
  `handle_document_request` reusing `verify_project_workspace`,
  daemon-resolved `canonical_root`, and the `DocumentOpen` containment
  sequence; per-daemon single-flight `Semaphore(1)` held from admission
  to blocking completion; `spawn_blocking` scan; wire-cap trim with
  reason; no LSP service, no writer lease, no file-byte reads.
- Shared walker (`src/directory_listing.rs`, `src/tui/file_tree.rs`):
  walk extracted verbatim into one daemon-neutral module; TUI
  presentation types delegate (`from_shared_listing`/`from_shared_node`);
  bounds and constants single-sourced; all 31 `file_tree` tests and the
  C002/C004 budget/cancel/error-flood tests pass unchanged against the
  shared implementation (plus the C005 flush regression test, §5).
- TUI (`src/tui/commands/file_tree.rs`, `app/commands.rs`,
  `runtime/command_dispatch.rs`, `file_tree_coordinator.rs`): daemon
  round-trip under unchanged single-flight admission (request now carries
  `project_id`/`workspace_id`); echoed `workspace_id` matched before
  render; `FileTreeListed`/`FileTreeFailed` completions with shared
  permit-release + pending-drive; `Busy`/`Unavailable` as visible
  truncated reasons; every failure disclaims the local fallback.
- Static guard (`scripts/check_daemon_listing_authority.py`, wired into
  `scripts/verify.sh quick`): rejects `read_dir`/`build_tree*`/
  `local_walk_root` on the listing path outside the two walker homes.
- Docs: `architecture/tui.md` (C005 authority/async/flush subsections),
  `architecture/protocol.md` (variant entry),
  `architecture/authorization.md` (matrix row), `docs/tui.md`
  (daemon-listing user contract).

## 4. Verification executed

### Commands run

```bash
cargo test -p codegg-protocol --locked                                   # 226 pass (incl. prefix validation + DTO roundtrip)
cargo test -p codegg-core --locked                                       # 835 pass
cargo test -p codegg --lib tui:: --locked                               # 1516 pass
cargo test -p codegg --lib core::daemon_documents --locked               # 3 pass (trim + symlink gate)
cargo test --test directory_listing_c005 --locked                        # 12 pass
cargo test --test file_tree_trajectory --locked                          # 4 pass
cargo test --test tui_panel_docking --locked                             # 33 pass
cargo test --test document_client_trajectory --locked                    # 3 pass
cargo test --test identity_m003_daemon_authorization --locked            # 9 pass
cargo test --test scheduler_authority_matrix --locked                    # 13 pass
python3 scripts/check_authorization_matrix.py                            # pass
python3 scripts/check_audit_coverage.py                                  # pass
python3 scripts/check_audit_invariants.py                                # pass
python3 scripts/check_daemon_listing_authority.py                        # pass
python3 scripts/check_tui_project_authority.py                           # pass
python3 scripts/check_tui_editor_text_authority.py                       # pass
cargo fmt --all -- --check                                               # pass
cargo clippy --workspace --all-targets --locked -- -D warnings           # pass
scripts/verify.sh quick                                                  # pass (incl. new guard + workspace check)
```

### Results

All green locally. No focused, integration, guard, fmt, or clippy step
is omitted or summarized without execution.

## 5. Invariant review

- No second text authority: the listing carries names/paths/depth/kind
  only; `check_tui_editor_text_authority.py` still passes and the walker
  reads entry metadata, never file bytes.
- No new protocol invented to finish a checklist: exactly the one ADR-
  frozen read was added; the client uses the existing `CoreClient`
  transport with no new wrapper or fallback branch.
- Single walk authority: `directory_listing.rs` is the only enumerator;
  the guard pins it.
- C005 flush fix (latent defect found, not a regression): hitting a walk
  bound `return`ed before the retained-entry flush loops, so a
  node-budget truncation always presented an empty tree with a correct
  reason. The pre-existing suite never asserted retained counts, which
  is why it survived since C002. Bound hits now `break` to the flush;
  pre-set cancellation still returns empty; mid-walk cancel keeps
  retained-so-far as a truncated listing. Covered by
  `retainable_entries_flush_when_a_later_bound_fires` and the
  `node_budget_truncates_a_wide_tree` exact count (4096).
- C004 admission preserved on both sides: TUI peak ≤ 1 concurrent
  round-trip across 100 supersessions (plus shutdown and panic-
  conversion paths); daemon semaphore restored to 1 after a 100-burst.

## 6. Failure and recovery review

Duplicate requests may legitimately see different metadata; results are
stamped to request/generation/daemon-scope and stale ones are discarded.
Cancel/reconnect/project-switch drops results, never a transaction
(`a_completion_from_before_a_project_switch_is_refused`,
`c004_switch_from_a_to_b_shows_only_b`,
`c004_shutdown_discards_late_completion_without_relaunch` over the new
transport). A stuck daemon syscall occupies the single permit while the
TUI reports `Waiting`; contended scans answer `busy`. A listed entry is
never a grant for `DocumentOpen` (`listed_entry_still_opens_through_
document_open` proves the reauthorization, including content). Daemon
restart invalidates in-flight scans; the client re-requests on explicit
refresh (`r`).

## 7. Migration and compatibility review

Additive protocol variants only; no DB migration; no durable state. Old
daemons fail the unknown variant at deserialization → transport error →
explicit unavailable message (proven by
`daemon_errors_fail_closed_with_no_local_fallback`). No local fallback
exists in any mode, so there is nothing version-skewed to silently
downgrade to. Local TUI uses the identical daemon path (in-process
client), so no privileged local exception exists.

## 8. Security review

- Authorization before I/O on every path: central dispatch gates
  `DirectProject + FileRead`; the handler re-verifies the
  project/workspace binding and prefix containment daemon-side.
- Non-enumerability: outsider denials mask as `project_not_found`;
  cross-wired/unknown bindings answer `document_not_found`;
  `Unavailable` messages carry no daemon paths; TUI diagnostics expose
  counts only (asserted in the C004 handoff test).
- Traversal/symlink/TOCTOU: prefix grammar rejects `..`/absolute/empty
  components pre-I/O; symlink components rejected on raw metadata both
  at the prefix and at every walk level; the walk never follows
  symlinks; special files skipped; long names skipped, not truncated.
- No `ProjectObserve`-implies-listing path: the matrix row requires
  `file.read`, and the observer-shaped principal (no grant) is denied in
  tests.

## 9. Documentation and operations

`architecture/tui.md` records the daemon-owned authority, the admission
design on both sides, the guard, and the flush fix;
`architecture/protocol.md` and `architecture/authorization.md` record the
variant and its gate; `docs/tui.md` states the user-visible contract
(daemon listing, explicit errors, no local fallback, `r` to retry a busy
daemon, no file watching). Telemetry is counts/caps/cancellations/errors/
durations only; no filenames or paths in diagnostics.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Hosted exact-SHA CI not run on `e37774a9` | Same residual as C001/C002/C004; local `verify.sh quick` + clippy + focused suites are green | Run hosted CI before release; no code impact expected |
| low | Real two-physical-hosts smoke not run | Plan §5E permits the fake-transport decoy as primary reproducible proof; physical-host behavior follows from the same authority argument | Optional physical-host smoke on release qualification |
| low | Background `@`-mention indexer (`index_files_sync` in `tui/app/mod.rs`) still walks the launch directory client-side | Separate client-side index with the same same-path confusion shape; explicitly deferred by the C005 plan's non-goals ("no separate client-side file index") | Future corrective if mention completion must be daemon-scoped; deliberately not covered by the listing guard |

No critical, high, or medium findings. No defect remains in landed code.

## 11. Roadmap disposition

Milestone C005 closed. The hard ADR-0022 gate is satisfied (accepted with
frozen contract, implemented exactly). The soft C004 integration is
satisfied (shared walk extracted from the closed C004 implementation;
admission preserved on both sides). C003 remains ready independent of
this work. This closes the corrective addendum's last open milestone:
C003 ready, C004 closed, C005 closed. Corrective plan required: none.
The `@`-mention indexer note above is deferred product work, not a
corrective pass on this milestone.

## 12. Registry updates

- `plans/implementation/tui-editor-correctness-usability/007-daemon-scoped-directory-listing-corrective.md`:
  `implemented`; implementation `e37774a9`.
- `plans/subsystems/tui-editor-correctness-usability-post-closure-corrective-addendum.md`
  C005 row: `closed` at `e37774a9`; workstream C003 ready / C004 closed /
  C005 closed.
- `plans/registry.md`: C005 `closed` with implementation + closure links;
  active roadmap row updated (C005 closed, nothing newly unblocked —
  C003 was already ready; unblock audit recorded here).
