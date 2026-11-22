# Agent Loop Completion Reliability C005 — Each Question Call Keeps Its Own Answer

Status: closed
Repository baseline: `d585e75f895cf41cd2241512d8d7e7c37fcece99`

Source roadmap: `plans/subsystems/agent-loop-completion-reliability-corrective-addendum.md#c005--each-question-call-keeps-its-own-answer`
Long-term requirements: `plans/000-long-term-specification.md#26-reliability-and-recovery`; `plans/000-long-term-specification.md#28-observability`
Applicable ADRs: none new.
Primary class: invariant

## 1. Objective

Two `question` calls in one tool batch do not share one receiver or one answer, and cleanup removes each question by its own id in the owning session.

## 2. Why this milestone is ready

The registry already has a scoped unregister and a per-question oneshot. The defect is the batch storing one receiver and cleaning up with the wrong id. No open milestone owns this. The 300-second question wait is bounded; this is a wrong-answer and stale-entry defect, not a missing timeout.

## 3. Current implementation evidence

- For each non-dry-run `question` call, the batch registers a new id, publishes `QuestionPending`, and assigns `self.question_rx = Some(rx)` (`src/agent/tool_batch.rs`). The previous receiver is dropped.
- After the batch, one `question_rx` is taken. Its answer, cancellation, or 300-second timeout is copied into every result whose text is `__QUESTION_PENDING__` (`src/agent/tool_batch.rs`).
- Cleanup calls `QuestionRegistry::unregister(&self.session_id)`. `unregister` takes a question id and removes only entries owned by session `"default"` (`crates/codegg-core/src/bus/mod.rs`). A real session id therefore matches no question, and the registered questions stay until opportunistic cleanup.
- `AgentLoop::setup_question_channel_for_exec` (`src/agent/loop.rs`) is a separate single-channel exec-mode path. It is not this bug. Do not fold it into the batch map.

## 4. Invariants that must not regress

- The 300-second question wait stays. Do not raise it, and do not make it unbounded.
- Dry-run `question` calls still never touch `QuestionRegistry`.
- Registration still happens before publish.
- `QuestionRegistry::unregister` keeps its `"default"` compatibility behavior. New cleanup uses `unregister_scoped`.
- Human approval is not this milestone. Its unset timeout is the documented indefinite wait. Do not add a default approval deadline here.
- One question in a batch still receives that question's answer.

## 5. Scope

In:

- The batch retains one receiver per pending question, keyed by the question id it registered.
- Each answer, cancellation, or timeout is written only into the result for that question. A second pending question does not receive the first answer.
- Cleanup calls `unregister_scoped` with the owning session id and each question id. It does not pass the session id to `unregister`.
- A test with two question calls in one batch.

Out:

- Changing the approval-timeout default.
- A new question protocol, a new registry, or a frontend question widget.
- The exec-mode single channel, unless a test shows it is the same overwrite. Then stop and report; do not merge the two designs in this milestone.
- Raising or removing the 300-second wait.

## 6. Required production changes

- `src/agent/tool_batch.rs`: replace the single `question_rx` slot for the batch path with a per-id receiver. Keep the field if exec mode still needs one channel; do not overload it for the batch.
- The cleanup call site in the same file.
- Docs: one sentence in `architecture/permission.md` or the agent question note that a batch of questions is one receiver per question and cleanup is scoped. Do not rewrite the approval chapter.

## 7. Ordered work packages

### WP-A — One receiver per question

Intent: dropping a receiver cannot discard every question but the last.

Required changes: store the receiver with the question id and the tool-call id. Publish still happens after register. The post-batch wait may wait on the set; the 300-second bound applies per wait or once to the set, but it must not grow past 300 seconds per question. Pick one and test it. Do not wait 300 seconds times the batch size unless the existing code already does, which it does not.

Acceptance evidence: two registered questions leave two live receivers. Answering the first does not fill the second result.

### WP-B — Cleanup uses the question id

Intent: a finished batch does not leave the questions registered under a real session.

Required changes: `unregister_scoped(session_id, question_id)` for each id this batch registered. Do not call `unregister` with the session id.

Acceptance evidence: after the batch resolves, a respond to either question id in that session does not deliver. A question registered by another session is untouched.

## 8. Failure, cancellation, restart, contention semantics

- A closed receiver is the existing cancelled result for that question only.
- A timeout is the existing timeout text for that question only.
- Two answers arriving in either order stay paired. Do not sort by arrival and assign positionally.
- No new registry. No change to the 310-second opportunistic expiry. That expiry is not the cleanup this milestone requires.
- Restart: no migration. Questions already stranded by the old cleanup are left to the existing expiry.

## 9. Compatibility and migration

No schema change. The `QuestionPending` event is unchanged. Frontends that answer by question id keep working; that is the id this milestone must retain.

## 10. Required tests

- One focused test, in the existing tool-batch or agent-loop family. Two question calls, two different answers, each result contains only its own answer.
- One assertion that cleanup does not call `unregister` with the session id. A planted call with the session id must leave the entry, which is the baseline defect.
- Do not add a new integration executable.
- No live UI.

## 11. Required verification commands

The focused test, selected by its full member path so a renamed or zero-match filter cannot pass as success.

`scripts/verify.sh quick` is the merge bar for the implementation turn, not for this planning turn.

## 12. Documentation updates

One sentence, as in §6.

## 13. Acceptance criteria

- Two question calls in one batch return two distinct answers, each to its own call.
- Cleanup removes those question ids from the owning session and does not remove another session's question.
- A single question still works.
- The 300-second bound is not raised. The approval timeout default is not changed.

## 14. Stop conditions

- Pairing the answer requires a new question-event field. Stop. Do not add one.
- Exec mode and the batch path cannot both keep their current external behavior without a shared redesign. Stop and report. Do not merge them silently.
- The registry has no scoped unregister. It does. If that reading is stale, stop rather than adding a second registry.

## 15. Closure evidence required

- The two-question test, failing on the baseline copy-one-answer behavior and passing after the change.
- A citation of `unregister_scoped` at the cleanup site.
- An explicit statement that the approval timeout was not changed.

## 16. Handoff notes

Confirmed against the baseline after the tool-failure audit. The audit also reported an indefinite human-approval wait. That wait is the documented default of `approval_timeout_secs` (`crates/codegg-config/src/schema.rs`: omit to wait indefinitely). It is not a milestone of this addendum.
