# TUI Editor Post-Closure Corrective C003 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tui-editor-correctness-usability/005-completion-verdict-and-request-fence-corrective.md`
  (status `implemented` at closure)

Source subsystem roadmap:

- `plans/subsystems/tui-editor-correctness-usability-post-closure-corrective-addendum.md#C003`

Predecessors (history preserved untouched):

- M003 `plans/closure/tui-editor-correctness-usability/003-status.md`
  (`44671d690`); M004
  `plans/closure/tui-editor-correctness-usability/004-status.md`
  (`fd2faaaf1`)

Binding ADRs: ADR-0011, ADR-0012, ADR-0021 (accepted Option B).
No protocol, DTO, or storage change was made, per plan §4.

Repository baseline reviewed: `1e74aa19f245e3c8e27afe624e72c0ff7e079bec`
(branch `plans/tui-editor-correctness-usability-20261013`).

Implementation commits:

- `1e74aa19` — `tui-editor(c003): completion verdict, request fence,
  bounded trigger` (bounded UTF-16 column + refusal, typed
  status/revision policy, lifetime completion epoch end to end,
  defect-reproduction tests, trajectory tests, architecture + user docs)

## 1. Executive finding

C003 is implemented and closed as a pure frontend corrective. The three
demonstrated defects are repaired without touching daemon authority,
wire semantics, or the single-transaction edit path: exact daemon
`NotReady`/`Stale` constructors render their typed statuses on
nonzero-revision requests; a lifetime completion epoch makes request-id
reuse across clear/reconnect/switch/ping-pong unadmittable; and F8 on a
giant line refuses past a 32 KiB prefix budget instead of allocating the
line or sending a wrong UTF-16 cursor. No legitimate behavior regressed:
Ready stays exact-revision-bound with undo/redo intact, and all M003/M004
trajectory suites pass unchanged.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Exact `NotReady`/`Stale` render typed statuses on nonzero revision, never generic identity Error (AC1) | `editor_completion.rs::real_not_ready_and_stale_constructors_keep_typed_status` (real `not_ready("doc-1")` on rev-7 scope → `NotReady`; real `stale("doc-1", 8, Some(8))` → `Stale`; mismatched-rev Unsynced/Unsupported stay typed) + trajectory `stale_wire_constructor_renders_stale_not_an_identity_error` | pass | Pre-fix both presented "another document revision"; identity check now splits document binding (all statuses) from status-specific revision policy |
| Malformed/wrong-document Ready cannot populate candidates; non-ready never actionable, Enter safe (AC2) | `hostile_and_malformed_payloads_are_never_actionable` (non-ready + smuggled candidate → typed status, payload disarmed; Ready wrong-rev / inconsistent sync witness → Error, nothing selectable) + existing `identity_mismatch_is_an_error_not_a_suggestion`, `not_ready_and_error_states_refuse_with_words` trajectories | pass | Accept path unchanged and additionally starved: no candidates exist outside Ready |
| No old result populates a newer request across dismissal/clear/reconnect/reuse/close-open/switch/cancel, both orders (AC3) | `reused_request_id_after_clear_needs_a_new_epoch`, `scope_ping_pong_does_not_resurrect_an_old_answer`, `dismiss_then_retrigger_does_not_reuse_ids` (state level) + trajectory `reconnect_epoch_fence_drops_a_queued_pre_reset_answer` (real `on_projection_reconnect` with unchanged generation/scope, id reuse 1→1, old-first and old-last orders) + existing supersession/switch/detach trajectories | pass | Fence is `(epoch, scope, request_id, loading)`; epoch bumps on every request-id reset and travels on `EditorCompletionFinished.completion_epoch` |
| Giant-line F8 bounded, never wrong UTF-16; normal Unicode exact (AC4) | `codegg-document` unit tests (ASCII/tabs/combining/astral exactness, at-budget precision, past-budget refusal, bad-range rejection) + trajectory `giant_line_refuses_f8_without_sending_or_allocating_the_prefix` (2 MiB line, zero requests, closed popup, explicit toast) + `budget_boundary_sends_exactly_and_refuses_just_past_it` (32768 sends exact column; 32778 refuses) | pass | Rope-chunk walk borrows, never materializes; 32 KiB cap matches C002 line budget |
| Ready edit + undo/redo retained, no text-authority/authorization regression (AC5) | Existing `tui_completion_trajectory` Ready/accept/undo suites (22 pass, incl. `emoji_cursor_binds_and_applies_exactly`), `lsp_completion_trajectory` (12 pass), text/project guards green | pass | Controller remains sole text owner; no permission or scope change |
| Focused/quick/static/hosted checks captured; unrun checks identified (AC6) | §4 command table, all executed locally | pass | Hosted CI not run — low finding below, same standard as prior closures |

## 3. Production implementation evidence

- Trigger budget (`crates/codegg-document/src/buffer.rs`, `error.rs`):
  `DocumentSnapshot::MAX_COMPLETION_PREFIX_BYTES` (32 KiB),
  `utf16_column_bounded(line_start, offset)` over borrowed rope chunks
  (pre-validated boundaries, budget checked before touching the rope,
  `u32` overflow degrades to refusal), new
  `DocumentError::CompletionPrefixTooLong` variant (no exhaustive match
  exists on the enum; verified by inspection).
- Scope (`src/tui/commands/editor.rs::current_completion_scope`):
  bounded column replaces `read_bytes(row_start, cursor)`; returns
  `Result<_, CompletionScopeError>` distinguishing `LineTooLong`
  ("Line too long for completion at this position" toast) from
  `Unavailable` (existing open-a-file warning). Non-boundary cursor
  inconsistencies map to `Unavailable`, never to a wrong cursor.
- Status policy (`src/tui/editor_completion.rs::apply`): document-id
  verified for every status; `Ready` requires exact revision plus a
  consistent sync witness (`None` tolerated for skew, mismatch rejected);
  `NotReady`/`Stale`/`Unsynced`/`Unsupported` render typed with
  candidates unconditionally disarmed via `disarm_non_ready`; malformed
  Ready is an Error with nothing selectable.
- Fencing (`editor_completion.rs`, `commands/editor.rs`,
  `app/commands.rs`, `runtime/command_dispatch.rs`, `app/mod.rs`):
  `epoch` field bumped in `clear` and `reset_for_scope` (the two
  request-id resets); `begin` return unchanged plus `epoch()` getter;
  `apply` rejects epoch mismatch first; `EditorCompletionFinished`
  carries `completion_epoch` end to end through dispatch and the `App`
  facade. `dismiss` needs no bump (`loading = false` already fences,
  ids keep incrementing).
- Docs: `architecture/tui.md` (C003 status/fencing/budget subsections),
  `docs/tui.md` (reconnect fence + long-line refusal user contract).

## 4. Verification executed

### Commands run

```bash
cargo test -p codegg-document --locked                                 # 8 pass (5 existing + 3 new utf16_column_bounded)
cargo test -p codegg --lib tui::editor_completion --locked             # 15 pass (9 existing incl. updated clear test + 6 new F1/F2)
cargo test -p codegg --lib tui::commands::editor --locked              # 38 pass
cargo test -p codegg --lib tui::components::editor --locked            # 35 pass
cargo test --test tui_completion_trajectory --locked                   # 22 pass (18 existing + 4 new C003)
cargo test --test lsp_completion_trajectory --features lsp-test-support --locked  # 12 pass
cargo test --test document_client_trajectory --locked                  # 3 pass
python3 scripts/check_tui_editor_text_authority.py                      # pass
python3 scripts/check_tui_project_authority.py                         # pass
python3 scripts/check_authorization_matrix.py                          # pass (no matrix change: no new operation)
cargo fmt --all -- --check                                             # pass
cargo clippy --workspace --all-targets --locked -- -D warnings         # pass
scripts/verify.sh quick                                                # pass
```

### Results

All green locally. No step omitted or summarized without execution.
Real-`rust-analyzer` smoke was not run (no installed server in this
environment); plan §5E permits deterministic wire-level fake-server
tests as the minimum, which the trajectory suites provide — recorded as
a low finding, not a gap.

## 5. Invariant review

- `DocumentController` remains the only editable text replica; the
  trigger path performs no daemon I/O and no text mutation, and the
  accept path is byte-identical apart from the starved candidate set.
- Daemon completion authority unchanged: warm-only, `via_session +
  file.read`, synchronized witness intact; `LspPreviewApply` and
  model-facing completion untouched; DTO semantics unaltered (frontend
  adapts to the constructors, never the reverse).
- No counter reset revalidates an old completion: the only resets bump
  the epoch; `u64` rollover is unreachable and would degrade to a
  documented fence, never a panic.
- Render budget not weakened: refusal replaces unbounded work; exact
  columns on supported lines including surrogate pairs (astral = 2
  units, proven in tests).
- No payloads in logs: completions carry no new telemetry; error paths
  keep the existing bounded truncation.

## 6. Failure and recovery review

Reconnect leaves generation and scope unchanged and still cannot resurrect
a queued answer (proven at App level with the real reconnect path,
including background refresh traffic on the shared channel). Dismissal
stays side-effect-free; process restart gets a fresh epoch namespace
(`Default` is 0, first reset moves on); no candidate persistence or
replay. Non-ready responses are visible, non-actionable, with no
speculative retry or empty-Ready fallback. Bounded conversion refuses
rather than approximates. No timers or polling introduced.

## 7. Migration and compatibility review

No schema, migration, or wire change. Old-daemon unsupported response
stays explicit. Native `file.read` gate untouched. No new permissions.
Status display becomes more correct for old and new peers alike; denials
never become `NotReady`. No text or candidate bodies persisted. C001/C002
render budgets, prompt focus, `/files`, side panels, and PTY routing
unaffected (suites green).

## 8. Security review

Authorized scope only: fencing additions never widen what a completion
may display. Malicious candidate strings still sanitize at render;
smuggled non-ready payloads are dropped before state, not just hidden.
UTF-16 offsets stay exact on supported lines (no surrogate-split
queries); over-budget lines refuse instead of emitting a wrong cursor.
No result contents, paths, or source text in errors or telemetry beyond
the pre-existing bounded error truncation.

## 9. Documentation and operations

`architecture/tui.md` documents the split identity/status policy, the
epoch fence (including why reconnect is the sharp case), and the trigger
budget with its refusal contract. `docs/tui.md` states the user-visible
guarantees. No operator action required.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Hosted exact-SHA CI not run on `1e74aa19` | Same residual as all prior closures in this workstream; local `verify.sh quick` + clippy + focused suites green | Run hosted CI before release |
| low | Real `rust-analyzer` happy-path smoke not run | Plan minimum (deterministic wire-level fake-server coverage incl. warm/cold/Unicode/dirty-sync) satisfied; live-server behavior unproven | Optional live-server smoke on release qualification |

No critical, high, or medium findings.

## 11. Roadmap disposition

Milestone C003 closed. Hard deps (M003/M004 closures) were satisfied at
handoff and remain so. This was the last ready-but-open milestone in the
corrective addendum: C003 closed, C004 closed, C005 closed. Corrective
plan required: none. The `@`-mention indexer note from the C005 closure
remains deferred product work, unrelated to this milestone.

## 12. Registry updates

- `plans/implementation/tui-editor-correctness-usability/005-completion-verdict-and-request-fence-corrective.md`:
  `implemented`; implementation `1e74aa19`.
- `plans/subsystems/tui-editor-correctness-usability-post-closure-corrective-addendum.md`:
  C003 `closed` at `1e74aa19`; workstream C003/C004/C005 all closed.
- `plans/registry.md`: C003 `closed` with implementation + closure links;
  active roadmap row updated (all three corrective milestones closed).
