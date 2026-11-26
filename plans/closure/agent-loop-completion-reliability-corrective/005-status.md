# Agent Loop Completion Reliability C005 — Closure Status

Status: closed

Source implementation plan: `plans/implementation/agent-loop-completion-reliability-corrective/005-each-question-call-keeps-its-own-answer.md`

Source subsystem roadmap: `plans/subsystems/agent-loop-completion-reliability-corrective-addendum.md#c005--each-question-call-keeps-its-own-answer`

Repository baseline reviewed: `d585e75f895cf41cd2241512d8d7e7c37fcece99`

Implementation state: uncommitted working-tree changes in this worktree. This record
does not claim a commit or a verified commit head.

## 1. Executive finding

A batch of `question` calls no longer shares one receiver or one answer. Each
registration keeps its own question id, tool-call id, and receiver; the batch waits on
them concurrently under the existing 300-second bound; and cleanup calls
`QuestionRegistry::unregister_scoped(session_id, question_id)` for each id it
registered.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Two questions get two distinct answers | `each_question_keeps_its_own_answer_and_cleanup_is_scoped` | pass | Each result contains only its own answer. |
| Cleanup is scoped by question id | same test | pass | Both ids are gone from the owning session and a third session's question survives. A session-id unregister (the baseline) would have left both registered, which the assertions catch. |
| A single question still works | the per-id path and the legacy path share the same formatting, cancel, and timeout vocabulary | pass (source) | 39 tool-batch tests pass. |
| The 300-second bound is not raised | source: `Duration::from_secs(300)` | pass | Concurrent waits keep the batch wall clock at one bound, not one bound per question. |
| The approval default is unchanged | no edit to `permission/approval.rs` or the config schema | pass | An omitted `approval_timeout_secs` still waits indefinitely. |

## 3. Production implementation evidence

- `AgentLoop` gained `pending_questions` holding the question id, the model's
  tool-call id, and that question's receiver. The single `question_rx` slot is no
  longer overwritten by a batch question.
- The permission branch retains the id and publishes `QuestionPending` with the
  same id it registered.
- `resolve_pending_question_answers` waits concurrently, maps outcomes by the
  model's tool-call id, and unregisters each id in the owning session.
- The exec-mode single channel remains as a legacy fallback. It owns no registered
  question, so it performs no registry cleanup.

## 4. Verification executed

```bash
cargo test -p codegg --lib agent::tool_batch               # 39 passed
cargo test -p codegg --lib agent::                         # 538 passed, 0 failed
scripts/verify.sh quick                                    # passed
```

## 5. Invariant review

- No new registry, no new question protocol, no event-field change.
- The 310-second opportunistic expiry is unchanged; scoped cleanup now removes an
  answered or timed-out question instead of waiting for it.
- Two answers arriving in either order stay paired by tool-call id; nothing assigns
  positionally by arrival.
- Channel capacity and approval waits are untouched.

## 6. Failure and recovery review

A closed receiver still produces the cancelled result for that question only; a
timeout produces the timeout text for that question only. Questions stranded by the
old cleanup are left to the existing expiry; no migration was added.

## 7. Migration and compatibility review

No schema change. `QuestionPending` is unchanged, so a frontend that answers by
question id keeps working; that is the id this milestone preserves.

## 8. Security review

No permission or sandbox change. Session scoping on cleanup is stricter than before.

## 9. Documentation and operations

The permission/agent note states that a batch of questions is one receiver per
question and that cleanup is scoped.

## 10. Unresolved findings

- The focused test drives the resolver and the live registry, not a full batch through
  the permission router. The router path is exercised by the wider `agent::` suite and
  by the existing dry-run test.
- The exec-mode single channel is kept for compatibility; nothing in the batch path
  writes it any more.

## 11. Roadmap disposition

C005 is closed.

## 12. Registry updates

The addendum and the registry row for C005 are set to closed.
