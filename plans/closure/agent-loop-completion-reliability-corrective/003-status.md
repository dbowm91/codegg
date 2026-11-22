# Agent Loop Completion Reliability C003 — Closure Status

Status: closed

Source implementation plan: `plans/implementation/agent-loop-completion-reliability-corrective/003-unparseable-tool-call-stop-is-not-success.md`

Source subsystem roadmap: `plans/subsystems/agent-loop-completion-reliability-corrective-addendum.md#c003--an-unparseable-tool-call-stop-is-not-success`

Repository baseline reviewed: `d585e75f895cf41cd2241512d8d7e7c37fcece99`

Implementation state: uncommitted working-tree changes in this worktree. This record
does not claim a commit or a verified commit head.

## 1. Executive finding

Both copies of the exhausted `stop_reason=tool_calls` arm now fail the turn with the
existing bounded 600-character preview. The diagnostic `AppEvent::Error` remains as a
side publication; it is no longer the only signal.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| The main loop's arm fails the turn | `test_missing_structured_tool_calls_emits_diagnostic_error_event` (harness, rewritten) | pass | `run()` returns `Err(AgentError::Invalid)` containing `stop_reason=tool_calls`; the diagnostic event still fires; exactly one provider request. |
| The follow-up copy matches | `an_unparseable_tool_call_stop_fails_the_turn` | pass | `drain_follow_up` returns `Err`; one provider call; the Finish is retained in `all_events`. |
| A normal answer still completes | `a_normal_finish_still_completes`; the wider agent suite | pass | 538 agent tests pass. |
| The work-plan judgment/blocked arms are unchanged | source: the arbiter arms are untouched | pass | The failure returns before the arbiter on this arm only. |
| No new parser or retry | source: the arm is the existing `matches!(processor.stop_reason(), Some("tool_calls"))` | pass | The 600-character preview is unchanged. |

## 3. Production implementation evidence

- `src/agent/loop.rs`: the arm builds one message, publishes `AppEvent::Error`, persists
  the turn transcript (so the failure does not also discard the evidence), resets the
  processor, and returns `Err(AppError::Agent(AgentError::Invalid(message)))`.
- `src/agent/follow_up.rs`: the same terminal, using `all_events.as_slice()` for the
  persist.
- The `TurnFailed` publication comes from the existing `run()` error wrapper and
  `turn_runtime`, which classify `AppError::Agent(_)` as `AgentInternal`.

## 4. Verification executed

```bash
cargo test --test agent_loop -- agent_loop_harness         # 76 passed, 1 ignored
cargo test -p codegg --lib agent::follow_up                # 5 passed
cargo test -p codegg --lib agent::                        # 538 passed, 0 failed
scripts/verify.sh quick                                   # passed
```

The harness family requires `cargo build -p codegg-sandbox-helper`; the earlier
missing-helper failure was environmental.

## 5. Invariant review

- The existing textual repair and the single bounded adapter repair are unchanged.
- `NeedsUserJudgment`, `BlockedReport`, `WaitForHandle`, `ContinueWithPrompt`, and
  `AllowCompletion` are untouched.
- A genuine finish with no tool calls still completes.
- No new parser and no additional retry after exhaustion.

## 6. Failure and recovery review

The failure is terminal for the turn; the next user message can retry with the persisted
transcript in hand. If cancel or steering is already set, the flags win at publication
(`interrupted`), so this arm is not reached with a user cancel pending in practice and
cannot overwrite it if it is.

## 7. Migration and compatibility review

No schema change. A session that previously showed a completed turn plus an error toast
now shows a failed turn. That is the correction.

## 8. Security review

No authority change. The message carries the existing bounded preview, so the failure
event cannot carry the whole raw stream.

## 9. Documentation and operations

The agent guide's limit section and the code comments state that an unparseable
tool-call stop fails the turn. No closed long-horizon closure record was edited.

## 10. Unresolved findings

- Persisting the transcript before returning the error is an addition beyond the
  plan's minimum. It was made because the failure path otherwise drops the turn's
  only durable transcript. It writes through the existing `persist_turn_transcript`
  owner; no new store or idempotency key was introduced.
- A model that repeatedly produces unparseable tool calls now fails each turn rather
  than appearing to finish. There is no automatic retry beyond the existing repair;
  that is the plan's boundary.

## 11. Roadmap disposition

C003 is closed.

## 12. Registry updates

The addendum and the registry row for C003 are set to closed.
