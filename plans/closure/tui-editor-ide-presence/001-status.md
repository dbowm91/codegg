# TUI Editor IDE Presence M001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tui-editor-ide-presence/001-interactive-open-returns-before-language-server-publication.md`

Source subsystem roadmap:

- `plans/subsystems/tui-editor-ide-presence-roadmap.md#milestone-1--interactive-open-returns-before-publication`

Decision implemented: `plans/adrs/ADR-0065-interactive-document-open-does-not-await-language-server-publication.md` (Option C).

Repository baseline: `d585e75f895cf41cd2241512d8d7e7c37fcece99`

## 1. Executive finding

M001 is complete. Workspace `DocumentOpen` now returns after document
authority — authorization, the document open, and the repository-locked,
path-validated `refresh_external_disk` — and does not await the managed
language-server push. The publication that gates `didOpen` runs on its own
task under `InitializedIsReady`, so the requested file's `didOpen` is no
longer sequenced behind a `publishDiagnostics` that only that `didOpen`
could cause. `CoreResponse::DocumentSnapshot` carries an additive
`lsp_sync` witness, and a payload that omits it still means the legacy
awaited success rather than `Pending`.

No profile constant was edited. `rust_analyzer_profile()` is still
`WaitForDiagnosticsOrTimeout { 30s }`. Change, save, and reload still await
their own sync and still carry only `lsp_degraded`.

## 2. Requirement-to-evidence matrix

| Requirement (plan §13) | Evidence | Result | Notes |
|---|---|---|---|
| A diagnostics-silent server does not hold `DocumentOpen` | `a_workspace_open_returns_before_language_server_publication` | pass | A daemon with no LSP service can only answer `Unavailable` on an awaited path; the response says `Pending`, so it did not await publication. |
| The TUI paints the returned snapshot while publication is `Pending` | `apply_editor_opened` (unchanged) plus the response contract | pass | The apply path already treats every non-error `DocumentSnapshot` as a successful open; `lsp_sync` is not an error channel. No second paint path, no TUI filesystem read. |
| `rust_analyzer_profile()` is still a 30-second diagnostics wait | `an_interactive_open_narrows_the_readiness_policy_and_none_keeps_it` | pass | Asserts the profile constant, then asserts `None` leaves it and the interactive policy replaces it with `InitializedIsReady`. |
| The requested file's interactive `didOpen` is not waiting on that policy | `apply_readiness_override` + `set_managed_document_interactive` | pass | The interactive publication acquires its client with `Some(InitializedIsReady)`; the awaited callers pass `None`. The override is narrowing-only and applies to that attempt. |
| A missing `lsp_sync` still means legacy awaited success | `a_payload_without_lsp_sync_decodes_as_the_legacy_awaited_meaning` | pass | `DocumentLspSync::Legacy` is the serde default; it is not `Pending` and is not `is_synced()`. |
| A new `Pending` payload is not a sync witness and not degraded | `pending_is_not_synced_and_not_degraded`, `a_new_snapshot_always_serializes_its_lsp_sync_state` | pass | New responses always serialize the field; `lsp_degraded` is `false` for `Pending`. |
| Change, save, and reload behavior is unchanged | `the_awaited_document_responses_carry_no_lsp_sync_state` | pass | The three variants carry no `lsp_sync`; their daemon arms still await `sync_managed_document`. |
| `scripts/check_tui_editor_text_authority.py` is green | guard run, below | pass | No frontend buffer, filesystem read, or server was added. |

## 3. Production implementation evidence

`crates/codegg-protocol/src/core.rs` adds `DocumentLspSync`
(`Legacy` / `Pending` / `Synced` / `Degraded` / `Unavailable`) and
`#[serde(default)] lsp_sync` on the `DocumentSnapshot` variant only.
`lsp_degraded` is derived from it: true only for `Degraded` and
`Unavailable`. The global-agent scope is deliberately outside workspace LSP
publication and answers `Unavailable`, which is a statement of fact rather
than a failed push.

`crates/egglsp/src/service.rs` adds
`get_or_create_client_with_readiness(path, Option<LspReadinessPolicy>)`
(`get_or_create_client` is now `None`), threads the override into the
single-flight initialization attempt, and applies it through
`apply_readiness_override`. `set_managed_document_interactive` is the only
caller that passes `Some(interactive_readiness_policy())`; the answer is
`InitializedIsReady`.

`src/core/daemon_documents.rs` extracts `ManagedPublisher` (owned
`Arc`s, so the task can outlive the request), `PublicationPolicy`
`{Awaited, Interactive}`, and `ManagedPublication`
`{Synced, Degraded, Unavailable}`. The workspace open arm schedules
publication with `schedule_managed_publication` and answers `Pending`;
the create and snapshot-get arms keep the awaited helper and answer its
three-way outcome; change/save/reload keep `sync_managed_document`. The
M003 sync witness is untouched: `record_lsp_synced` only after a successful
push, `clear_lsp_synced` on every other exit.

## 4. Verification executed

All results are LOCAL to this worktree. No hosted CI run was made for this
milestone.

```bash
cargo fmt
cargo test -p codegg-protocol --lib
# test result: ok. 340 passed; 0 failed

CARGO_BUILD_JOBS=2 cargo test -p codegg --test core_daemon -- document_open_deadline_m001
# 3 passed (the three tests above); 481 filtered out

CARGO_BUILD_JOBS=2 cargo test -p egglsp --lib -- an_interactive_open_narrows
# 1 passed; 1019 filtered out

CARGO_BUILD_JOBS=2 cargo check -p codegg --all-targets
CARGO_BUILD_JOBS=2 cargo check -p codegg-tui --all-targets
CARGO_BUILD_JOBS=2 cargo check -p codegg-client --all-targets
# clean: no errors

python3 scripts/check_tui_editor_text_authority.py
# TUI editor text-authority guard passed
```

`scripts/verify.sh scoped` is the roadmap-level closing sweep and is run
once for M001–M006; its result is recorded in the last milestone's closure
rather than claimed here. No live TUI receipt was taken and none is claimed.

## 5. Not claimed

- No 800-line open wall-clock was measured or claimed. The 30-second cap and
  the pre-`didOpen` wait are source facts, not measurements.
- No warm-open timing. A warm open that is still slow is a repository-lock
  wait in `refresh_external_disk`, which this milestone kept on the response
  path on purpose.
- No hosted CI result.
- Desktop was not executed here; its document-editor test fixture was updated
  for the new field only.

## 6. Handoff

M002, M004, and M006 were blocked on this closure; M005 was operationally
gated on it. The status row in the roadmap and the registry entry are updated
alongside this record.
