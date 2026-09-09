# Project Collaboration Milestone 003 — Separately Authorized Structured Chat Actions

Status: implemented (closed by `plans/closure/project-collaboration/003-status.md`)

Repository baseline: `9dfbc6e1f842a76ab6154d1bd99cbadfdff4b2e6`

Source roadmap: `plans/subsystems/project-collaboration-roadmap.md#M003--separately-authorized-structured-chat-actions`

Long-term requirements: `plans/000-long-term-specification.md#21-project-communication`, `#22-audit-architecture`, `#29-system-invariants`; roadmap Phase 12.

Applicable ADRs: none. Primary class: capability.

## 1. Objective

Add explicit typed message-associated actions for launching an agent task/review request or referencing/submitting a job through existing daemon authorization, durable run/job and audit owners, while proving free-text messages have no privileged execution semantics.

## 2. Why this milestone is ready

Unblocked by collaboration M001 closure (`plans/closure/project-collaboration/001-status.md`); identity/audit prerequisites are transitive through M001. Durable agent task/run/job/worktree systems are already closed and are consumers, not reimplemented here.

## 3. Current implementation evidence

Agent/task/job APIs already provide durable identity, idempotency/cancellation/scheduler authority. Collaboration M001 will provide message identity and authorship. No chat-triggered execution exists, which is the safe baseline.

## 4. Invariants that must not regress

A free-text body never becomes a command by parsing, mention, prefix or model inference. Structured action is a separate explicit protocol operation with capability check and idempotency key. Resulting task/run/job is canonical durable state; chat stores only reference/status projection. Audit links message -> auth decision -> action -> task/run/job. Observer read-only rules remain unless caller separately possesses the action capability.

## 5. Scope

In: structured action DTOs and action kinds initially agent task/review request/job reference-or-submit where canonical API exists; explicit UI affordance/confirmation consistent with existing UX; authorization/idempotency; result references; audit causation. Out: arbitrary shell commands, parsing natural language into actions, custom workflow engine, new agent/job semantics.

## 6. Required production changes

Protocol/core: action request names message/channel/project, typed action payload and idempotency key; daemon resolves actor from transport and checks ordinary semantic capability. Runtime: call existing Task/AgentRun/Job services; never execute in chat store. Storage: optional action-result link on message or separate relation; no duplicate execution on retry. Audit: causal links. Frontend: explicit command/button/picker, never implicit send behavior.

## 7. Ordered work packages

A — define minimal action taxonomy and map each to existing capability/canonical service.

B — implement daemon action dispatcher with message/project validation, authorization and idempotency.

C — persist/project action result references and audit causation; surface status in chat.

D — TUI explicit invocation ergonomics and observer capability behavior.

E — injection/retry/revocation/cancellation/end-to-end tests.

## 8. Failure, cancellation, restart, and contention semantics

Duplicate action idempotency returns existing result. Authorization is evaluated before creation; revocation race follows M003 authorization contract. Once canonical task/job exists, its own cancellation/recovery semantics apply. Chat action failure does not corrupt message history.

## 9. Compatibility and migration

Additive capability/version. Older clients render messages without action affordances. No text syntax is reserved as executable compatibility behavior.

## 10. Required tests

Free-text corpus proving zero execution; authorized/denied action matrix; message/project mismatch; duplicate retransmission; restart; membership removal race; observer without action capability; task/job failure/cancel; audit causal linkage; secret/ref privacy.

## 11. Required verification commands

```bash
cargo test --workspace collaboration --no-fail-fast
cargo test --workspace agent --no-fail-fast
cargo test --workspace jobs --no-fail-fast
cargo test --workspace audit --no-fail-fast
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

## 12. Documentation updates

Project communication action schema, authorization matrix, TUI help and security docs emphasizing non-executable free text.

## 13. Acceptance criteria

Explicit authorized chat action creates exactly one canonical task/run/job and links it to message/audit; retry is idempotent; denial creates nothing; ordinary text—including command-like text—never executes or changes authority.

## 14. Stop conditions

M001 not closed; desired action lacks a canonical existing service/capability; implementation proposes parsing free text for privileged intent; action would bypass scheduler/agent/job/audit ownership.

## 15. Closure evidence required

Action-capability-owner matrix, end-to-end message/action/audit/run linkage, free-text negative corpus, idempotency/restart/revocation tests, exact verification results.

## 16. Handoff notes

Start with the smallest actions that map cleanly to existing canonical services. Defer any action requiring a new workflow domain.
