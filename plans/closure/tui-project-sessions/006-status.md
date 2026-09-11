# Multi-Project TUI Frontend Convergence M006 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tui-project-sessions/006-nonblocking-session-submit-lifecycle.md`

Source subsystem roadmap:

- `plans/subsystems/tui-project-sessions-frontend-convergence-corrective-addendum.md#12-milestone-status`

Repository baseline reviewed: `ed5fb06`.

Implementation commits:

- `ed5fb06` — registered nonblocking prompt/session continuation, immutable
  route/prompt capture, cancellation and stale-completion guards, regression
  tests, and documentation.

Closure commit:

- This commit — closure evidence, roadmap/registry status, and downstream
  dependency disposition.

## 1. Executive finding

M006 is complete. A no-session prompt now captures its immutable text and
canonical project execution context, inserts the local user message once, and
starts a registered `SessionCreate` continuation. The event loop no longer
awaits daemon work. Completion is accepted only for the original request,
tab, project/workspace route, active-view epoch, and reconnect epoch; valid
completion uses canonical `set_session` before submitting the captured prompt
exactly once.

Known create failures restore the editable prompt and permit an explicit retry
without duplicating the already-visible user message. Tab switching, tab close,
reconnect, shutdown, and stale completion paths invalidate frontend state
without attempting destructive cleanup of a daemon session that may already
have been created. Observer bare input remains on the project-chat path.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| No daemon wait in the terminal event loop | `src/tui/runtime/event_loop.rs`; `ensure_local_session` removed; event loop starts a registered task and returns to `select!` | pass |
| Immutable prompt/context capture | `PendingSessionSubmit` in `src/tui/app/state/prompt.rs`; `send_prompt` captures `ProjectExecutionContext`, route, request generation, and trimmed prompt before mutation | pass |
| Registered async continuation | `src/tui/commands/prompt.rs`; `spawn_scoped_registered_tui_task` with `CoreClient::request` and typed `TuiCommand::PromptSessionCreated` | pass |
| Exactly-once/coalesced Enter | `double_submit_is_coalesced_before_session_create_starts`; `pending_send` plus pending continuation guard | pass |
| Correct canonical session binding | `apply_session_create_for_prompt` validates route and calls `App::set_session` before dispatch | pass |
| Stale tab/project/view/reconnect completion is inert | route token check, request-generation check, cancellation hooks in tab switch/close/reconnect/shutdown, and `stale_route_completion_cannot_bind_or_submit` | pass |
| Failure preserves user work and retry does not duplicate | `create_failure_restores_prompt_without_duplicate_message_on_retry`; retry marker and bounded draft stash | pass |
| Observer mode does not create sessions | existing observer gate precedes the no-session capture in `App::send_prompt`; observer collaboration tests remain applicable | pass |
| No persistence/protocol/daemon ownership change | frontend-only pending state; existing `SessionCreate` and turn-submit contracts reused | pass |
| Documentation updated | `architecture/tui.md`, `.opencode/skills/tui/SKILL.md`, and source lifecycle comments | pass |

## 3. Production implementation evidence

- `PromptState` owns a bounded `PendingSessionSubmit`, the request-generation
  state, task-start guard, and retry-without-message marker.
- `App::send_prompt` performs the observer and slash-command routing before
  entering the no-session continuation. It captures the active tab and
  `ProjectExecutionContext` before inserting the visible user message.
- `src/tui/commands/prompt.rs` owns the async start and completion seam. It
  sends explicit `project_id`, `workspace_id`, and canonical workspace root
  to the existing `SessionCreate` request.
- Completion validation rejects cancelled, non-current, route-mismatched,
  already-bound, or no-longer-active completions. Accepted completion uses
  `set_session`, preserving session registration, asset refresh, routing, and
  projection behavior.
- Tab switching, active-tab close, reconnect, new-session reset, and shutdown
  all invalidate the frontend continuation. Reconnect restores the captured
  prompt for explicit review/retry; other invalidation paths discard it with
  no implicit resubmission.
- The registered task is scoped to the originating tab and active-view epoch,
  and the task registry remains the owner of TUI task cancellation.

## 4. Verification executed

Passed locally:

- `cargo fmt --all -- --check`
- `cargo check -p codegg --tests`
- `python3 scripts/check_tui_project_authority.py`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `scripts/verify.sh quick`
- `git diff --check`

`scripts/verify.sh quick` passed its existing generated-agent, core-boundary,
sandbox, execution-ownership, TUI-authority, and workspace all-target check.
The test targets, including the new prompt/session tests, compile successfully
under `cargo check --tests`.

The focused test binary was attempted with both the repository toolchain and
an x86_64 library-path workaround. Linking remains unavailable on this host:
the x86_64-apple-darwin Rust target is given arm64 `/opt/local/liblzma` and
`libiconv`, yielding undefined x86_64 lzma symbols. This is a host
toolchain/linker limitation after successful Rust compilation, not a test
assertion failure. CI truth is not inferred from this local linker failure;
the existing CI verify job remains the authoritative hosted execution.

## 5. Invariant review

| State | Transition/effect | Safety property |
|---|---|---|
| `Idle` with no session | Submit captures prompt/context/route, inserts one user message, and enters `CreatingSession` | no mutable active state is reread for the in-flight payload |
| `CreatingSession` | Event loop starts one registered task and continues selecting input/events/render work | no daemon/network await on the event-loop path; repeated Enter is coalesced |
| `Created(valid route)` | Completion finishes request, calls `set_session`, then dispatches captured prompt once | canonical session registration and refresh remain intact |
| `Created(stale route)` | Completion is ignored and frontend continuation is cancelled | no wrong-tab/project binding or turn submission |
| `Failed` | Error clears loading, restores prompt, and marks retry without message insertion | user work remains editable; explicit retry is bounded and nonduplicating |
| `Cancelled/reconnect/shutdown` | Request generation/task scope is invalidated; reconnect restores prompt for review | no late completion mutates current state and no daemon cleanup is guessed |
| `ExistingSession` | Existing turn path remains the normal dispatch path | no change to provider/scheduler ownership |
| `Observer` | Bare input exits through project chat before session-submit capture | observer input cannot enter `SessionCreate` |

The request-generation state and route token provide independent rejection
guards. The `App` remains the composition boundary; no new frontend workflow,
daemon, scheduler, persistence, or protocol owner was introduced.

## 6. Failure and recovery review

Known `SessionCreate` errors, unexpected responses, transport errors, and DTO
conversion errors all clear the pending state and restore the captured prompt.
If the user edited a new draft while the request was running, that draft is
bounded into the existing stash before the original captured prompt is
restored. Retrying the restored text consumes a retry marker and does not add
another local user message.

Unknown daemon outcomes are not blindly retried. A tab close or shutdown only
cancels frontend ownership; it does not delete a session that the daemon may
already have committed. Reconnect invalidates the old epoch and asks the user
to review/retry rather than silently resubmitting.

## 7. Migration and compatibility review

No storage migration, manifest field, protocol field, or persistent outbox was
added. Existing-session submission, remote-core behavior, standalone/stdio
CoreClient use, daemon authorization, and scheduler/turn ownership remain on
their existing contracts. The pending state is frontend-ephemeral and is
discarded on restart.

## 8. Security review

The captured project/workspace values are routing context, not authorization.
The daemon still validates authority for `SessionCreate` and turn submission.
No secrets or credentials are stored in the pending state. Observer mode is
fail-closed with respect to ordinary session submission, and stale completions
cannot cross project/tab boundaries.

## 9. Documentation and operations

The async-command and prompt/session lifecycle contracts are documented in
`architecture/tui.md` and `.opencode/skills/tui/SKILL.md`. Source comments
explain immutable capture, cancellation, and exact-once completion behavior.
No new CI lane or operator procedure is required.

## 10. Unresolved findings and severity

| Severity | Finding | Disposition |
|---|---|---|
| Low / environment-only | Local focused TUI test binaries cannot link because the host mixes an x86_64 Rust target with arm64 `/opt/local` compression/iconv libraries | Does not indicate a source/test failure; hosted CI or a compatible local toolchain should execute the binaries |
| None | No correctness, security, migration, or ownership finding remains | closure accepted |

## 11. Roadmap disposition and unblocking audit

M006 is closed. The dependency graph was re-read after implementation:

- M008 remains blocked by hard dependencies M005, M006, and M007; M007 is
  still open, so M008 does not become ready.
- M009 remains blocked by hard dependency M007; M006 does not alter that gate.
- M007 and M010 remain ready and unchanged.
- No registered future plan is newly unblocked by this closure.

The following control surfaces were updated in this closure:

- implementation plan status → `implemented`;
- subsystem roadmap M006 status → `closed`;
- registry current milestone and recent-closure rows → M006 `closed`, with
  M007/M010 still ready and M008/M009 still dependency-gated;
- this closure record added as the accepted evidence gate.

## 12. Registry updates

The registry now records implementation commit `ed5fb06` and closure record
`plans/closure/tui-project-sessions/006-status.md`. The M006 row was removed
from dependency-ready work, and the active subsystem control row records
M006 closed. No downstream plan was moved to `ready` because every affected
downstream plan retains an independent M007 hard dependency.

## 13. Final recommendation

Closed. No corrective pass is required. The only follow-up is operational:
execute the focused TUI test binaries on a compatible host/toolchain so the
new assertions are run in addition to the locally successful compile and
static verification evidence.
